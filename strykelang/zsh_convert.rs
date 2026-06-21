//! Convert zsh source into stryke source.
//!
//! Pipeline: `zsh src → zsh::ported::parse → ZshProgram → [this module] →
//! strykelang::ast::Program → convert::convert_program → stryke source`.
//!
//! ## Classification (the core rule)
//!
//! * **zsh builtin** → native stryke (`echo`/`print` → `Say`/`Print`,
//!   `printf` → `Printf`, `cd` → `chdir`, `typeset`/`local`/`integer`/… + bare
//!   `x=v` → variable declarations / reassignments).
//! * **external command** → `system("…")`; external pipelines stay one shell
//!   string `system("a | b")`. **Only externals reach `system`.**
//! * **control structure** → native stryke: `;`/newline → statements, `&&`/`||`
//!   → `BinOp`, `if`/`elif`/`else` → `If`, `for x in …` → `Foreach`,
//!   `while`/`until` → `While`/`Until`, `case` → `if`/`elsif` chain, `foo() {}`
//!   → `SubDecl`. `[[ … ]]` / `test` / `[` conditions → boolean expressions
//!   (file tests, string/numeric comparisons, `&&`/`||`/`!`, `=~`).
//!
//! ## Declarations vs reassignment (style rule 10 — never bare `my`)
//!
//! Per scope (the top level and each function body are separate scopes), the
//! transpiler pre-counts how often each variable name is assigned:
//! * the **first** assignment to a name becomes a declaration — `val` when the
//!   name is written exactly once (or declared `readonly`), otherwise `var`;
//! * **later** assignments to the same name become reassignments (`$x = …`),
//!   never a second declaration.
//!
//! ## Parameters and arithmetic
//!
//! * Positional: `$1`..`$9` / `${10}` → `$ARGV[n-1]` (top level) or `$_[n-1]`
//!   (function); `$@`/`$*` → `@ARGV` / `@_`; `$0` stays `$0`; `$#`/`$#name` →
//!   `scalar(@…)`; `$?`/`$$` are spelled the same in stryke.
//! * Arithmetic: `$(( … ))`, the `(( … ))` command, `let`, and C-style
//!   `for ((init; cond; step))` are rewritten by sigil-prefixing bare
//!   identifiers (`i` → `$i`) and preserving operators/precedence verbatim,
//!   since shell and stryke share C-style operators.
//!
//! Command substitution `$( … )` and `` `backticks` `` capture output via
//! `qx` — a clean `qx "cmd"` when it is the whole value, or `#{qx "cmd"}`
//! interpolation when embedded in a larger string.
//!
//! ## Externals, redirections, pipelines
//!
//! External commands and pipelines become `system("…")`. The shell string is a
//! stryke double-quoted literal where a simple scalar `$var` is interpolated by
//! stryke (so the value flows in) and every other `$`/`@`/`#{` is escaped so the
//! shell expands it. Redirections (`>`, `>>`, `2>&1`, `<`, `<<<`, `&>`, …) are
//! reconstructed and appended; any command with a redirection — builtin or not —
//! routes through the shell so the redirect is preserved. Subshells `( … )` and
//! brace groups `{ … }` are inlined (subshell environment isolation is not
//! preserved, with a warning).
//!
//! Every user-defined function is emitted under a file-derived namespace —
//! `log() { … }` in `deploy.zsh` becomes `fn deploy::log { … }` and calls become
//! `deploy::log(...)`. This is required: a function whose name collides with a
//! stryke builtin (`log`, `sum`, …) cannot be redefined in `package main`, and
//! an unqualified call resolves to the builtin instead of the script's function.
//!
//! Control-flow and stack builtins map natively: `return [v]`, `break` → `last`,
//! `continue` → `next`, `exit [n]`, `shift` → `shift @ARGV`/`@_`, `pushd`/`popd`,
//! `pwd` → `cwd`; `:`/`true` are dropped as no-ops.
//!
//! Constructs not yet handled (`select` for, heredoc / process-substitution
//! redirections, `case` glob fall-through, the remaining stateful builtins like
//! `read`/`setopt`) are **skipped with a recorded warning** — never silently
//! mistranslated, never wrongly sent to `system`.

use crate::ast::{
    BinOp, Expr, ExprKind, Program, Sigil, Statement, StmtKind, StringPart, UnaryOp, VarDecl,
};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use zsh::zsh_ast::{
    CaseTerm, ForList, SublistOp, ZshAssign, ZshAssignValue, ZshCase, ZshCommand, ZshCond, ZshFor,
    ZshFuncDef, ZshIf, ZshList, ZshPipe, ZshProgram, ZshRedir, ZshSimple, ZshSublist, ZshWhile,
};

/// zsh's parser uses global mutable state and is not reentrant; serialize calls.
static PARSE_LOCK: Mutex<()> = Mutex::new(());

/// A lexical scope: the top level or one function body. `counts` is the total
/// number of assignments to each name within the scope (pre-scanned); `declared`
/// tracks names already emitted as a declaration so later writes reassign.
#[derive(Default)]
struct Scope {
    counts: HashMap<String, usize>,
    declared: HashSet<String>,
}

/// Conversion context: accumulated warnings, the active scope stack, and the
/// current function-nesting depth (positional params resolve to `@ARGV` at the
/// top level but `@_` inside a function body).
struct Ctx {
    warns: Vec<String>,
    scopes: Vec<Scope>,
    fn_depth: usize,
    /// Names of functions defined anywhere in the script — a call to one of
    /// these becomes a native stryke call, not `system(...)`.
    funcs: HashSet<String>,
    /// Namespace (package) every user-defined function is emitted under — both
    /// its definition (`fn FILE::name`) and its calls (`FILE::name(...)`).
    /// Derived from the source filename. A namespace is required because a
    /// function name colliding with a stryke builtin (`log`, `sum`, …) cannot be
    /// redefined in `package main`, and an unqualified call resolves to the
    /// builtin rather than the script's function.
    ns: String,
}

impl Ctx {
    fn new() -> Self {
        Ctx {
            warns: Vec::new(),
            scopes: Vec::new(),
            fn_depth: 0,
            funcs: HashSet::new(),
            ns: String::new(),
        }
    }

    /// Namespace-qualified name for a user-defined function.
    fn fn_name(&self, name: &str) -> String {
        format!("{}::{}", self.ns, name)
    }

    fn warn(&mut self, w: impl Into<String>) {
        self.warns.push(w.into());
    }

    /// True when conversion is inside a function body.
    fn in_fn(&self) -> bool {
        self.fn_depth > 0
    }

    fn scope_mut(&mut self) -> &mut Scope {
        self.scopes.last_mut().expect("scope stack is non-empty")
    }

    /// Push a scope pre-scanned from `lists`, run `f`, then pop it.
    fn with_scope<R>(&mut self, lists: &[ZshList], f: impl FnOnce(&mut Ctx) -> R) -> R {
        let mut counts = HashMap::new();
        count_lists(lists, &mut counts);
        self.scopes.push(Scope {
            counts,
            declared: HashSet::new(),
        });
        let r = f(self);
        self.scopes.pop();
        r
    }
}

/// Derive a valid stryke package name from the source path: the file stem with
/// non-identifier characters replaced by `_` (e.g. `my-deploy.zsh` → `my_deploy`,
/// stdin `-` → `zsh`).
pub fn namespace_from_path(path: &str) -> String {
    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| *s != "-" && !s.is_empty())
        .unwrap_or("zsh");
    let mut ns: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    if ns.chars().next().map_or(true, |c| !c.is_ascii_alphabetic() && c != '_') {
        ns.insert(0, 'z');
    }
    ns
}

/// Convert zsh `src` to stryke source. `namespace` is the package every
/// user-defined function is emitted under (see [`namespace_from_path`]).
/// Returns `(stryke_source, warnings)`.
pub fn convert_zsh(src: &str, namespace: &str) -> (String, Vec<String>) {
    let zprog: ZshProgram = {
        let _guard = PARSE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        zsh::ported::parse::parse_init(src);
        zsh::ported::parse::parse()
    };
    let mut ctx = Ctx::new();
    ctx.ns = namespace.to_string();
    // Pre-collect every function name so calls resolve to native stryke calls
    // (even forward references) instead of being shelled out via `system`.
    collect_funcs(&zprog.lists, &mut ctx.funcs);
    let statements = ctx.with_scope(&zprog.lists, |c| map_lists(&zprog.lists, c));
    let prog = Program { statements };
    let opts = crate::convert::ConvertOptions {
        val_var_decls: true,
        ..Default::default()
    };
    (
        crate::convert::convert_program_with_options(&prog, &opts),
        ctx.warns,
    )
}

fn ex(kind: ExprKind) -> Expr {
    Expr { kind, line: 0 }
}
fn st(kind: StmtKind) -> Statement {
    Statement::new(kind, 0)
}
fn untok(w: &str) -> String {
    zsh::ported::lex::untokenize(w)
}

fn map_lists(lists: &[ZshList], ctx: &mut Ctx) -> Vec<Statement> {
    let mut out = Vec::new();
    for list in lists {
        if list.flags.async_ {
            ctx.warn("`&` background command not converted; ran synchronously");
        }
        out.extend(map_sublist_to_stmts(&list.sublist, ctx));
    }
    out
}

fn map_block(prog: &ZshProgram, ctx: &mut Ctx) -> Vec<Statement> {
    map_lists(&prog.lists, ctx)
}

/// A sublist becomes one or more statements. A single compound command or a
/// standalone variable declaration yields a statement directly; everything else
/// becomes an expression statement (a `BinOp` chain of command expressions).
fn map_sublist_to_stmts(sub: &ZshSublist, ctx: &mut Ctx) -> Vec<Statement> {
    if sub.next.is_none() && !sub.flags.not && sub.pipe.next.is_none() {
        match &sub.pipe.cmd {
            ZshCommand::Simple(simple) => {
                if let Some(stmts) = map_declaration(simple, ctx) {
                    return stmts;
                }
                if let Some(stmts) = map_let(simple, ctx) {
                    return stmts;
                }
            }
            ZshCommand::Arith(s) => {
                return vec![st(StmtKind::Expression(ex(ExprKind::Bareword(
                    arith_to_stryke(&strip_arith_delims(s)),
                ))))]
            }
            ZshCommand::If(z) => return vec![map_if(z, ctx)],
            ZshCommand::For(z) => return map_for(z, ctx).into_iter().collect(),
            ZshCommand::While(z) => return vec![map_while(z, false, ctx)],
            ZshCommand::Until(z) => return vec![map_while(z, true, ctx)],
            ZshCommand::Case(z) => return map_case(z, ctx).into_iter().collect(),
            ZshCommand::FuncDef(z) => return map_funcdef(z, ctx),
            ZshCommand::Subsh(p) => {
                ctx.warn("subshell `( … )` inlined; environment isolation not preserved");
                return map_block(p, ctx);
            }
            ZshCommand::Cursh(p) => return map_block(p, ctx),
            _ => {}
        }
    }
    match map_sublist_expr(sub, ctx) {
        Some(e) => vec![st(StmtKind::Expression(e))],
        None => Vec::new(),
    }
}

// ── compound forms ───────────────────────────────────────────────────────────

fn map_if(z: &ZshIf, ctx: &mut Ctx) -> Statement {
    st(StmtKind::If {
        condition: map_cond(&z.cond, ctx),
        body: map_block(&z.then, ctx),
        elsifs: z
            .elif
            .iter()
            .map(|(c, b)| (map_cond(c, ctx), map_block(b, ctx)))
            .collect(),
        else_block: z.else_.as_ref().map(|b| map_block(b, ctx)),
    })
}

fn map_while(z: &ZshWhile, until: bool, ctx: &mut Ctx) -> Statement {
    let condition = map_cond(&z.cond, ctx);
    let body = map_block(&z.body, ctx);
    if until || z.until {
        st(StmtKind::Until {
            condition,
            body,
            label: None,
            continue_block: None,
        })
    } else {
        st(StmtKind::While {
            condition,
            body,
            label: None,
            continue_block: None,
        })
    }
}

fn map_for(z: &ZshFor, ctx: &mut Ctx) -> Option<Statement> {
    if z.is_select {
        ctx.warn("`select` loop not yet converted");
        return None;
    }
    match &z.list {
        ForList::Words(words) => {
            let items: Vec<String> = words.iter().map(|w| untok(w)).collect();
            let list = ex(ExprKind::QW(items));
            Some(st(StmtKind::Foreach {
                var: z.var.clone(),
                list,
                body: map_block(&z.body, ctx),
                label: None,
                continue_block: None,
            }))
        }
        ForList::CStyle { init, cond, step } => {
            let mk_expr = |s: &str| {
                let t = s.trim();
                (!t.is_empty()).then(|| ex(ExprKind::Bareword(arith_to_stryke(t))))
            };
            Some(st(StmtKind::For {
                init: cstyle_init(init),
                condition: mk_expr(cond),
                step: mk_expr(step),
                body: map_block(&z.body, ctx),
                label: None,
                continue_block: None,
            }))
        }
        ForList::Positional => {
            ctx.warn("`for x` over positional params not yet converted");
            None
        }
    }
}

/// `case` → an `if`/`elsif` chain on the topic. Literal patterns become string
/// equality; `*` becomes the `else`. Glob patterns are approximated as equality
/// (a warning is emitted), and `;&`/`;|` fall-through terminators warn.
fn map_case(z: &ZshCase, ctx: &mut Ctx) -> Option<Statement> {
    let topic = word_to_expr(&untok(&z.word), ctx.in_fn());
    let mut elsifs: Vec<(Expr, Vec<Statement>)> = Vec::new();
    let mut first: Option<(Expr, Vec<Statement>)> = None;
    let mut else_block: Option<Vec<Statement>> = None;

    for arm in &z.arms {
        if arm.terminator != CaseTerm::Break {
            ctx.warn("case `;&`/`;|` fall-through not converted (treated as break)");
        }
        let body = map_block(&arm.body, ctx);
        let is_default = arm.patterns.iter().any(|p| untok(p) == "*");
        if is_default {
            else_block = Some(body);
            continue;
        }
        // OR the patterns: topic eq p1 || topic eq p2 …
        let mut cond: Option<Expr> = None;
        for p in &arm.patterns {
            let pat = untok(p);
            if pat.contains(['*', '?', '[']) {
                ctx.warn(format!("case glob pattern `{pat}` approximated as equality"));
            }
            let eq = ex(ExprKind::BinOp {
                left: Box::new(topic.clone()),
                op: BinOp::StrEq,
                right: Box::new(ex(ExprKind::String(pat))),
            });
            cond = Some(match cond {
                None => eq,
                Some(prev) => ex(ExprKind::BinOp {
                    left: Box::new(prev),
                    op: BinOp::LogOr,
                    right: Box::new(eq),
                }),
            });
        }
        let cond = cond.unwrap_or_else(|| ex(ExprKind::Integer(0)));
        if first.is_none() {
            first = Some((cond, body));
        } else {
            elsifs.push((cond, body));
        }
    }

    let (condition, body) = first?;
    Some(st(StmtKind::If {
        condition,
        body,
        elsifs,
        else_block,
    }))
}

fn map_funcdef(z: &ZshFuncDef, ctx: &mut Ctx) -> Vec<Statement> {
    // A function body is its own scope: variables it assigns are local for the
    // purposes of declaration/reassignment classification, and positional
    // params resolve against `@_` rather than `@ARGV`.
    ctx.fn_depth += 1;
    let body = ctx.with_scope(&z.body.lists, |c| map_block(&z.body, c));
    ctx.fn_depth -= 1;
    z.names
        .iter()
        .map(|name| {
            st(StmtKind::SubDecl {
                name: ctx.fn_name(name),
                params: Vec::new(),
                body: body.clone(),
                prototype: None,
            })
        })
        .collect()
}

// ── conditions (if / while / until / [[ ]] / test) ───────────────────────────

/// A condition program → a boolean Expr. The status of the last command in the
/// list is the condition; `&&`/`||` chain into logical ops.
fn map_cond(prog: &ZshProgram, ctx: &mut Ctx) -> Expr {
    match prog.lists.last() {
        Some(list) => map_cond_sublist(&list.sublist, ctx),
        None => ex(ExprKind::Integer(1)),
    }
}

fn map_cond_sublist(sub: &ZshSublist, ctx: &mut Ctx) -> Expr {
    let mut left = map_cond_pipe(&sub.pipe, ctx);
    if let Some((op, next)) = &sub.next {
        let right = map_cond_sublist(next, ctx);
        let op = match op {
            SublistOp::And => BinOp::LogAnd,
            SublistOp::Or => BinOp::LogOr,
        };
        left = ex(ExprKind::BinOp {
            left: Box::new(left),
            op,
            right: Box::new(right),
        });
    }
    if sub.flags.not {
        left = ex(ExprKind::UnaryOp {
            op: UnaryOp::LogNot,
            expr: Box::new(left),
        });
    }
    left
}

fn map_cond_pipe(pipe: &ZshPipe, ctx: &mut Ctx) -> Expr {
    if pipe.next.is_some() {
        ctx.warn("pipeline as a condition not converted precisely");
    }
    match &pipe.cmd {
        ZshCommand::Cond(c) => map_zcond(c, ctx),
        ZshCommand::Arith(s) => ex(ExprKind::Bareword(arith_to_stryke(&strip_arith_delims(s)))),
        ZshCommand::Simple(s) => {
            let head = s.words.first().map(|w| untok(w)).unwrap_or_default();
            if head == "[" || head == "test" {
                return map_test_words(&s.words, ctx);
            }
            // a plain command condition is true when it exits 0
            if is_builtin(&head) {
                ctx.warn(format!("builtin `{head}` as a condition not converted precisely"));
            }
            ex(ExprKind::BinOp {
                left: Box::new(ex(ExprKind::System(vec![ex(ExprKind::String(shell_stage(s)))]))),
                op: BinOp::NumEq,
                right: Box::new(ex(ExprKind::Integer(0))),
            })
        }
        other => {
            ctx.warn(format!(
                "condition form not converted: {:?}",
                std::mem::discriminant(other)
            ));
            ex(ExprKind::Integer(1))
        }
    }
}

/// Map a `[[ … ]]` conditional to a boolean expression.
fn map_zcond(c: &ZshCond, ctx: &mut Ctx) -> Expr {
    match c {
        ZshCond::Not(inner) => ex(ExprKind::UnaryOp {
            op: UnaryOp::LogNot,
            expr: Box::new(map_zcond(inner, ctx)),
        }),
        ZshCond::And(a, b) => ex(ExprKind::BinOp {
            left: Box::new(map_zcond(a, ctx)),
            op: BinOp::LogAnd,
            right: Box::new(map_zcond(b, ctx)),
        }),
        ZshCond::Or(a, b) => ex(ExprKind::BinOp {
            left: Box::new(map_zcond(a, ctx)),
            op: BinOp::LogOr,
            right: Box::new(map_zcond(b, ctx)),
        }),
        ZshCond::Unary(op, arg) => map_unary_test(op, arg, ctx),
        // zsh_ast stores Binary as (left, op, right).
        ZshCond::Binary(l, op, r) => map_binary_test(op, l, r, ctx),
        ZshCond::Regex(s, re) => ex(ExprKind::Match {
            expr: Box::new(word_to_expr(&untok(s), ctx.in_fn())),
            pattern: untok(re),
            flags: String::new(),
            scalar_g: false,
            delim: '/',
        }),
    }
}

/// `test`/`[` builtin args → reuse the `[[ ]]` mapping. Handles `! x`, unary
/// `-f file`, and binary `a OP b` forms; strips a trailing `]`.
fn map_test_words(words: &[String], ctx: &mut Ctx) -> Expr {
    let mut a: Vec<String> = words[1..].iter().map(|w| untok(w)).collect();
    if a.last().map(|s| s == "]").unwrap_or(false) {
        a.pop();
    }
    if let Some(rest) = a.strip_first_if("!") {
        return ex(ExprKind::UnaryOp {
            op: UnaryOp::LogNot,
            expr: Box::new(map_test_slice(&rest, ctx)),
        });
    }
    map_test_slice(&a, ctx)
}

fn map_test_slice(a: &[String], ctx: &mut Ctx) -> Expr {
    match a.len() {
        1 => word_to_expr(&a[0], ctx.in_fn()),
        2 => map_unary_test(&a[0], &a[1], ctx),
        3 => map_binary_test(&a[1], &a[0], &a[2], ctx),
        _ => {
            ctx.warn("complex test expression not converted precisely");
            ex(ExprKind::Integer(1))
        }
    }
}

fn map_unary_test(op: &str, arg: &str, ctx: &mut Ctx) -> Expr {
    let arg_e = word_to_expr(&untok(arg), ctx.in_fn());
    // The op also carries zsh tokenization sentinels — untokenize it before
    // matching. `[[ ]]` stores it without a dash (`z`); `test`/`[` keep it (`-z`).
    let op = untok(op);
    let f = op.trim_start_matches('-');
    let empty = || ex(ExprKind::String(String::new()));
    match f {
        "z" => ex(ExprKind::BinOp {
            left: Box::new(arg_e),
            op: BinOp::StrEq,
            right: Box::new(empty()),
        }),
        "n" => ex(ExprKind::BinOp {
            left: Box::new(arg_e),
            op: BinOp::StrNe,
            right: Box::new(empty()),
        }),
        // file tests: f d e r w x s L b c p S g u k O G t h …
        _ if f.len() == 1 && f.chars().next().unwrap().is_ascii_alphabetic() => {
            ex(ExprKind::FileTest {
                op: f.chars().next().unwrap(),
                expr: Box::new(arg_e),
            })
        }
        _ => {
            ctx.warn(format!("unary test `{op}` not converted precisely"));
            arg_e
        }
    }
}

fn map_binary_test(op: &str, l: &str, r: &str, ctx: &mut Ctx) -> Expr {
    let in_fn = ctx.in_fn();
    let le = Box::new(word_to_expr(&untok(l), in_fn));
    let re = Box::new(word_to_expr(&untok(r), in_fn));
    // The op carries zsh tokenization sentinels — untokenize before matching.
    // numeric ops may arrive with or without a dash (`-eq` from `test`, `eq` from `[[ ]]`).
    let op = untok(op);
    let bop = match op.as_str() {
        "=" | "==" => BinOp::StrEq,
        "!=" => BinOp::StrNe,
        "<" => BinOp::StrLt,
        ">" => BinOp::StrGt,
        "-eq" | "eq" => BinOp::NumEq,
        "-ne" | "ne" => BinOp::NumNe,
        "-lt" | "lt" => BinOp::NumLt,
        "-gt" | "gt" => BinOp::NumGt,
        "-le" | "le" => BinOp::NumLe,
        "-ge" | "ge" => BinOp::NumGe,
        _ => {
            ctx.warn(format!("binary test `{op}` not converted precisely"));
            BinOp::StrEq
        }
    };
    if (op == "==" || op == "=") && r.contains(['*', '?', '[']) {
        ctx.warn(format!("glob match `{}` approximated as equality", untok(r)));
    }
    ex(ExprKind::BinOp {
        left: le,
        op: bop,
        right: re,
    })
}

// ── simple commands (builtin vs external) ────────────────────────────────────

fn map_sublist_expr(sub: &ZshSublist, ctx: &mut Ctx) -> Option<Expr> {
    if sub.flags.not {
        ctx.warn("`!`-negated pipeline not converted");
        return None;
    }
    let left = map_pipe_expr(&sub.pipe, ctx)?;
    match &sub.next {
        Some((op, next)) => {
            let right = map_sublist_expr(next, ctx)?;
            let op = match op {
                SublistOp::And => BinOp::LogAnd,
                SublistOp::Or => BinOp::LogOr,
            };
            Some(ex(ExprKind::BinOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            }))
        }
        None => Some(left),
    }
}

fn map_pipe_expr(pipe: &ZshPipe, ctx: &mut Ctx) -> Option<Expr> {
    let mut simples = Vec::new();
    let mut cur = Some(pipe);
    while let Some(p) = cur {
        match &p.cmd {
            ZshCommand::Simple(s) => simples.push(s),
            other => {
                ctx.warn(format!(
                    "compound command in pipeline not converted: {:?}",
                    std::mem::discriminant(other)
                ));
                return None;
            }
        }
        cur = p.next.as_deref();
    }
    if simples.len() == 1 {
        return map_simple_command(simples[0], ctx);
    }
    // Multi-stage pipeline → a single shell string; the shell supplies the pipe
    // and any builtin stages (`echo … | grep …`). Bails if a stage carries an
    // unsupported redirection.
    let mut stages = Vec::with_capacity(simples.len());
    for s in &simples {
        stages.push(simple_shell_cmd(s, ctx)?);
    }
    Some(ex(ExprKind::System(vec![system_arg(&stages.join(" | "))])))
}

fn map_simple_command(s: &ZshSimple, ctx: &mut Ctx) -> Option<Expr> {
    if s.words.is_empty() {
        return None;
    }
    // A redirection forces the whole command through the shell so the redirect
    // is preserved — even for a builtin like `echo hi > file`.
    if !s.redirs.is_empty() {
        return simple_shell_cmd(s, ctx).map(|cmd| ex(ExprKind::System(vec![system_arg(&cmd)])));
    }
    let in_fn = ctx.in_fn();
    let head = untok(&s.words[0]);
    let rest: Vec<String> = s.words[1..].iter().map(|w| untok(w)).collect();
    match head.as_str() {
        "echo" | "print" => {
            let (no_newline, args) = match rest.split_first() {
                Some((flag, tail)) if flag == "-n" => (true, tail.to_vec()),
                _ => (false, rest),
            };
            let arg = join_as_one_string(&args, in_fn);
            let kind = if no_newline {
                ExprKind::Print {
                    handle: None,
                    args: vec![arg],
                }
            } else {
                ExprKind::Say {
                    handle: None,
                    args: vec![arg],
                }
            };
            Some(ex(kind))
        }
        "printf" => Some(ex(ExprKind::Printf {
            handle: None,
            args: rest.iter().map(|w| word_to_expr(w, in_fn)).collect(),
        })),
        "cd" | "chdir" => Some(ex(ExprKind::FuncCall {
            name: "chdir".into(),
            args: vec![word_to_expr(&rest.first().cloned().unwrap_or_default(), in_fn)],
        })),
        // Control flow — works standalone and inside `&&` / `||` chains
        // (`cmd || return 1`, `cmd && break`). stryke accepts these in
        // expression position, so they are emitted as expressions.
        "return" => {
            let body = match rest.first() {
                Some(w) => {
                    format!("return {}", crate::fmt::format_expr(&scalar_value_expr(w, in_fn)))
                }
                None => "return".to_string(),
            };
            Some(ex(ExprKind::Bareword(body)))
        }
        "break" => Some(ex(ExprKind::Bareword("last".into()))),
        "continue" => Some(ex(ExprKind::Bareword("next".into()))),
        "exit" => Some(ex(ExprKind::Exit(
            rest.first().map(|w| Box::new(scalar_value_expr(w, in_fn))),
        ))),
        "shift" => {
            let arr = rest
                .first()
                .cloned()
                .unwrap_or_else(|| if in_fn { "_".into() } else { "ARGV".into() });
            Some(ex(ExprKind::FuncCall {
                name: "shift".into(),
                args: vec![ex(ExprKind::ArrayVar(arr))],
            }))
        }
        "pushd" => Some(ex(ExprKind::FuncCall {
            name: "pushd".into(),
            args: rest.first().map(|w| word_to_expr(w, in_fn)).into_iter().collect(),
        })),
        "popd" => Some(ex(ExprKind::FuncCall {
            name: "popd".into(),
            args: Vec::new(),
        })),
        "pwd" => Some(ex(ExprKind::FuncCall {
            name: "cwd".into(),
            args: Vec::new(),
        })),
        // `:` and `true` are deliberate no-ops — drop them silently.
        ":" | "true" => None,
        // A call to a function defined in the script → native, namespaced call.
        _ if ctx.funcs.contains(&head) => Some(ex(ExprKind::FuncCall {
            name: ctx.fn_name(&head),
            args: rest.iter().map(|w| word_to_expr(w, in_fn)).collect(),
        })),
        _ if is_builtin(&head) => {
            ctx.warn(format!("builtin `{head}` has no native mapping yet; skipped"));
            None
        }
        _ => Some(ex(ExprKind::System(vec![system_arg(&shell_stage(s))]))),
    }
}

// ── declarations / reassignment ──────────────────────────────────────────────

fn map_declaration(s: &ZshSimple, ctx: &mut Ctx) -> Option<Vec<Statement>> {
    let in_fn = ctx.in_fn();
    // bare `name=value` (possibly an array) with no command word
    if !s.assigns.is_empty() && s.words.is_empty() {
        let stmts = s
            .assigns
            .iter()
            .filter_map(|a| {
                if a.append {
                    ctx.warn(format!("`{}+=` append not converted precisely", a.name));
                }
                let (sigil, init) = assign_sigil_init(a, in_fn);
                emit_assign_or_decl(&a.name, sigil, init, false, ctx)
            })
            .collect();
        return Some(stmts);
    }
    // `typeset`/`local`/`integer`/`export`/`readonly` …
    if let Some(head) = s.words.first().map(|w| untok(w)) {
        if is_decl_keyword(&head) {
            let readonly = head == "readonly";
            let mut stmts = Vec::new();
            for w in &s.words[1..] {
                let word = untok(w);
                if word.starts_with('-') {
                    continue;
                }
                match word.split_once('=') {
                    Some((name, value)) if is_var_name(name) => {
                        if let Some(stm) = emit_assign_or_decl(
                            name,
                            Sigil::Scalar,
                            Some(scalar_value_expr(value, in_fn)),
                            readonly,
                            ctx,
                        ) {
                            stmts.push(stm);
                        }
                    }
                    None if is_var_name(&word) => {
                        if let Some(stm) =
                            emit_assign_or_decl(&word, Sigil::Scalar, None, readonly, ctx)
                        {
                            stmts.push(stm);
                        }
                    }
                    _ => {}
                }
            }
            return Some(stmts);
        }
    }
    None
}

/// Emit a declaration the first time a name is assigned in the scope, a
/// reassignment thereafter. `val` is chosen for a write-once name (or one marked
/// `readonly`); `var` for a name written more than once or declared without an
/// initializer. A redundant re-declaration with no initializer yields nothing.
fn emit_assign_or_decl(
    name: &str,
    sigil: Sigil,
    init: Option<Expr>,
    readonly: bool,
    ctx: &mut Ctx,
) -> Option<Statement> {
    if ctx.scope_mut().declared.contains(name) {
        let value = init?;
        let target = ex(match sigil {
            Sigil::Array => ExprKind::ArrayVar(name.to_string()),
            Sigil::Hash => ExprKind::HashVar(name.to_string()),
            _ => ExprKind::ScalarVar(name.to_string()),
        });
        return Some(st(StmtKind::Expression(ex(ExprKind::Assign {
            target: Box::new(target),
            value: Box::new(value),
        }))));
    }
    ctx.scope_mut().declared.insert(name.to_string());
    let count = ctx.scope_mut().counts.get(name).copied().unwrap_or(1);
    // `val` requires an initializer; an uninitialized declaration must be `var`.
    let frozen = init.is_some() && (readonly || count <= 1);
    Some(st(StmtKind::My(vec![VarDecl {
        sigil,
        name: name.to_string(),
        initializer: init,
        frozen,
        type_annotation: None,
        list_context: false,
    }])))
}

fn assign_sigil_init(a: &ZshAssign, in_fn: bool) -> (Sigil, Option<Expr>) {
    match &a.value {
        ZshAssignValue::Scalar(v) => (Sigil::Scalar, Some(scalar_value_expr(&untok(v), in_fn))),
        ZshAssignValue::Array(items) => (
            Sigil::Array,
            Some(ex(ExprKind::QW(items.iter().map(|i| untok(i)).collect()))),
        ),
    }
}

fn scalar_value_expr(v: &str, in_fn: bool) -> Expr {
    if let Ok(n) = v.parse::<i64>() {
        return ex(ExprKind::Integer(n));
    }
    // A value that is exactly one arithmetic substitution becomes a real
    // expression (`x=$(( 2 + 3 ))` → `2 + 3`), not a string.
    if let Some(inner) = whole_arith(v) {
        return ex(ExprKind::Bareword(arith_to_stryke(&inner)));
    }
    // A value that is exactly one command substitution captures output via `qx`
    // (`x=$(date)` / `` x=`date` `` → `qx "date"`).
    if let Some(cmd) = whole_cmdsub(v) {
        return ex(ExprKind::Qx(Box::new(ex(ExprKind::String(cmd)))));
    }
    // Parameter expansions (`${y:-d}`, `${f#*.}`, `${(U)x}`, …) flow through
    // `word_to_expr` as raw `${ … }` text — stryke's double-quoted string parser
    // expands them natively via zshrs.
    word_to_expr(v, in_fn)
}

// ── arithmetic ───────────────────────────────────────────────────────────────

/// The `let` builtin: each argument is an arithmetic expression. `let "i = i+1"`
/// → `$i = $i + 1`. Returns `None` for any other command.
fn map_let(s: &ZshSimple, _ctx: &mut Ctx) -> Option<Vec<Statement>> {
    if s.words.first().map(|w| untok(w)).as_deref() != Some("let") {
        return None;
    }
    Some(
        s.words[1..]
            .iter()
            .map(|w| {
                st(StmtKind::Expression(ex(ExprKind::Bareword(arith_to_stryke(
                    &untok(w),
                )))))
            })
            .collect(),
    )
}

/// The init clause of a C-style `for`. `i=0` declares the loop variable
/// (`var $i = 0`); any other form is emitted as an arithmetic expression.
fn cstyle_init(init: &str) -> Option<Box<Statement>> {
    let t = init.trim();
    if t.is_empty() {
        return None;
    }
    if let Some((name, value)) = t.split_once('=') {
        let name = name.trim();
        if is_var_name(name) && !value.starts_with('=') {
            return Some(Box::new(st(StmtKind::My(vec![VarDecl {
                sigil: Sigil::Scalar,
                name: name.to_string(),
                initializer: Some(ex(ExprKind::Bareword(arith_to_stryke(value)))),
                frozen: false,
                type_annotation: None,
                list_context: false,
            }]))));
        }
    }
    Some(Box::new(st(StmtKind::Expression(ex(ExprKind::Bareword(
        arith_to_stryke(t),
    ))))))
}

/// Rewrite a shell arithmetic expression into stryke source: bare identifiers
/// gain a `$` sigil (`i` → `$i`), already-sigiled vars and numeric literals are
/// left untouched, and every operator/paren is preserved verbatim — shell and
/// stryke share C-style operators and precedence, so the text maps across
/// unchanged.
fn arith_to_stryke(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'$' {
            // existing variable reference: `$name` or `${ … }`
            out.push('$');
            i += 1;
            if i < b.len() && b[i] == b'{' {
                while i < b.len() {
                    let done = b[i] == b'}';
                    out.push(b[i] as char);
                    i += 1;
                    if done {
                        break;
                    }
                }
            } else {
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    out.push(b[i] as char);
                    i += 1;
                }
            }
        } else if c.is_ascii_digit() {
            // numeric literal (incl. `0x..`, `0b..`, decimals) — keep verbatim
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'.') {
                out.push(b[i] as char);
                i += 1;
            }
        } else if c.is_ascii_alphabetic() || c == b'_' {
            // bare identifier → stryke scalar variable
            out.push('$');
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                out.push(b[i] as char);
                i += 1;
            }
        } else {
            out.push(c as char);
            i += 1;
        }
    }
    out.trim().to_string()
}

/// If `v` is exactly one arithmetic substitution `$(( … ))`, return its inner
/// text; otherwise `None`.
fn whole_arith(v: &str) -> Option<String> {
    let t = v.trim();
    Some(t.strip_prefix("$((")?.strip_suffix("))")?.to_string())
}

/// If `v` is exactly one command substitution — `$( … )` (not `$(( … ))`) or a
/// `` `backtick` `` — return the inner command; otherwise `None`.
fn whole_cmdsub(v: &str) -> Option<String> {
    let t = v.trim();
    if let Some(rest) = t.strip_prefix("$(") {
        if !rest.starts_with('(') {
            if let Some(inner) = rest.strip_suffix(')') {
                return Some(inner.trim().to_string());
            }
        }
    }
    if t.len() >= 2 && t.starts_with('`') && t.ends_with('`') {
        return Some(t[1..t.len() - 1].trim().to_string());
    }
    None
}

/// Strip a surrounding `(( … ))` from an arithmetic command body.
fn strip_arith_delims(s: &str) -> String {
    let t = s.trim();
    t.strip_prefix("((")
        .and_then(|x| x.strip_suffix("))"))
        .unwrap_or(t)
        .trim()
        .to_string()
}

// ── assignment pre-scan (mutation analysis) ──────────────────────────────────

/// Count assignments to each name within one scope: descends into block bodies
/// (`if`/`for`/`while`/`until`/`case`) but NOT nested function definitions,
/// which open their own scope.
/// Collect every function name defined anywhere in the program (including
/// nested definitions), so calls to them convert to native stryke calls.
fn collect_funcs(lists: &[ZshList], out: &mut HashSet<String>) {
    for l in lists {
        collect_funcs_sub(&l.sublist, out);
    }
}

fn collect_funcs_sub(sub: &ZshSublist, out: &mut HashSet<String>) {
    let mut cur = Some(&sub.pipe);
    while let Some(p) = cur {
        collect_funcs_cmd(&p.cmd, out);
        cur = p.next.as_deref();
    }
    if let Some((_, n)) = &sub.next {
        collect_funcs_sub(n, out);
    }
}

fn collect_funcs_cmd(cmd: &ZshCommand, out: &mut HashSet<String>) {
    match cmd {
        ZshCommand::FuncDef(f) => {
            for n in &f.names {
                out.insert(n.clone());
            }
            collect_funcs(&f.body.lists, out);
        }
        ZshCommand::If(z) => {
            collect_funcs(&z.then.lists, out);
            for (_, b) in &z.elif {
                collect_funcs(&b.lists, out);
            }
            if let Some(e) = &z.else_ {
                collect_funcs(&e.lists, out);
            }
        }
        ZshCommand::For(z) => collect_funcs(&z.body.lists, out),
        ZshCommand::While(z) | ZshCommand::Until(z) => collect_funcs(&z.body.lists, out),
        ZshCommand::Case(z) => {
            for a in &z.arms {
                collect_funcs(&a.body.lists, out);
            }
        }
        ZshCommand::Subsh(p) | ZshCommand::Cursh(p) => collect_funcs(&p.lists, out),
        _ => {}
    }
}

fn count_lists(lists: &[ZshList], counts: &mut HashMap<String, usize>) {
    for l in lists {
        count_sublist(&l.sublist, counts);
    }
}

fn count_sublist(sub: &ZshSublist, counts: &mut HashMap<String, usize>) {
    count_pipe(&sub.pipe, counts);
    if let Some((_, next)) = &sub.next {
        count_sublist(next, counts);
    }
}

fn count_pipe(pipe: &ZshPipe, counts: &mut HashMap<String, usize>) {
    count_cmd(&pipe.cmd, counts);
    if let Some(n) = &pipe.next {
        count_pipe(n, counts);
    }
}

fn count_cmd(cmd: &ZshCommand, counts: &mut HashMap<String, usize>) {
    match cmd {
        ZshCommand::Simple(s) => count_simple(s, counts),
        ZshCommand::If(z) => {
            count_lists(&z.then.lists, counts);
            for (_, b) in &z.elif {
                count_lists(&b.lists, counts);
            }
            if let Some(e) = &z.else_ {
                count_lists(&e.lists, counts);
            }
        }
        ZshCommand::For(z) => count_lists(&z.body.lists, counts),
        ZshCommand::While(z) | ZshCommand::Until(z) => count_lists(&z.body.lists, counts),
        ZshCommand::Case(z) => {
            for arm in &z.arms {
                count_lists(&arm.body.lists, counts);
            }
        }
        _ => {}
    }
}

fn count_simple(s: &ZshSimple, counts: &mut HashMap<String, usize>) {
    if !s.assigns.is_empty() && s.words.is_empty() {
        for a in &s.assigns {
            *counts.entry(a.name.clone()).or_default() += 1;
        }
        return;
    }
    if let Some(head) = s.words.first().map(|w| untok(w)) {
        if is_decl_keyword(&head) {
            for w in &s.words[1..] {
                let word = untok(w);
                if word.starts_with('-') {
                    continue;
                }
                let name = word.split_once('=').map(|(n, _)| n).unwrap_or(&word);
                if is_var_name(name) {
                    *counts.entry(name.to_string()).or_default() += 1;
                }
            }
        }
    }
}

// ── words, interpolation, quoting ────────────────────────────────────────────

fn join_as_one_string(words: &[String], in_fn: bool) -> Expr {
    if words.is_empty() {
        return ex(ExprKind::String(String::new()));
    }
    let mut parts: Vec<StringPart> = Vec::new();
    for (i, w) in words.iter().enumerate() {
        if i > 0 {
            push_literal(&mut parts, " ");
        }
        for p in interp_parts(w, in_fn) {
            merge_part(&mut parts, p);
        }
    }
    parts_to_expr(parts)
}

fn word_to_expr(w: &str, in_fn: bool) -> Expr {
    parts_to_expr(interp_parts(w, in_fn))
}

fn parts_to_expr(parts: Vec<StringPart>) -> Expr {
    match parts.as_slice() {
        [] => ex(ExprKind::String(String::new())),
        [StringPart::Literal(s)] => ex(ExprKind::String(s.clone())),
        _ => ex(ExprKind::InterpolatedString(parts)),
    }
}

fn push_literal(parts: &mut Vec<StringPart>, s: &str) {
    if let Some(StringPart::Literal(last)) = parts.last_mut() {
        last.push_str(s);
    } else {
        parts.push(StringPart::Literal(s.to_string()));
    }
}

fn merge_part(parts: &mut Vec<StringPart>, p: StringPart) {
    match p {
        StringPart::Literal(s) => push_literal(parts, &s),
        other => parts.push(other),
    }
}

fn interp_parts(w: &str, in_fn: bool) -> Vec<StringPart> {
    let bytes = w.as_bytes();
    let mut parts: Vec<StringPart> = Vec::new();
    let mut i = 0;
    let mut lit = String::new();
    let flush = |parts: &mut Vec<StringPart>, lit: &mut String| {
        if !lit.is_empty() {
            parts.push(StringPart::Literal(std::mem::take(lit)));
        }
    };
    while i < bytes.len() {
        if bytes[i] == b'`' {
            // backtick command substitution `` ` … ` `` → `#{qx " … "}`
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j] != b'`' {
                j += 1;
            }
            flush(&mut parts, &mut lit);
            parts.push(cmdsub_interp(&w[start..j.min(w.len())]));
            i = if j < bytes.len() { j + 1 } else { j };
            continue;
        }
        if bytes[i] == b'$' && i + 2 < bytes.len() && bytes[i + 1] == b'(' && bytes[i + 2] == b'(' {
            // arithmetic substitution `$(( … ))` → `#{ … }` interpolation
            let start = i + 3;
            let mut depth = 2;
            let mut j = start;
            while j < bytes.len() && depth > 0 {
                match bytes[j] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            let inner_end = if depth == 0 { j - 2 } else { j };
            let inner = &w[start..inner_end.min(w.len())];
            flush(&mut parts, &mut lit);
            parts.push(StringPart::Expr(ex(ExprKind::Bareword(format!(
                "#{{{}}}",
                arith_to_stryke(inner)
            )))));
            i = j;
            continue;
        }
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            if next == b'{' {
                if let Some(close) = w[i + 2..].find('}') {
                    let name = &w[i + 2..i + 2 + close];
                    if is_var_name(name) {
                        flush(&mut parts, &mut lit);
                        parts.push(StringPart::ScalarVar(name.to_string()));
                        i += 2 + close + 1;
                        continue;
                    }
                    // `${10}` — braced positional parameter.
                    if let Ok(n) = name.parse::<usize>() {
                        flush(&mut parts, &mut lit);
                        parts.push(StringPart::Expr(positional_expr(n, in_fn)));
                        i += 2 + close + 1;
                        continue;
                    }
                    // Any other `${ … }` parameter expansion (`:-`, `#`, `%`,
                    // `/`, `(flags)`, subscripts, `${#x}`, …) → pass the raw zsh
                    // form straight through. stryke's double-quoted string parser
                    // expands it natively via zshrs's `paramsubst` (see
                    // `parser.rs::zsh_param_form`), so no special lowering here.
                    lit.push_str(&format!("${{{}}}", name));
                    i += 2 + close + 1;
                    continue;
                }
            } else if next.is_ascii_alphabetic() || next == b'_' {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len()
                    && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_')
                {
                    end += 1;
                }
                flush(&mut parts, &mut lit);
                parts.push(StringPart::ScalarVar(w[start..end].to_string()));
                i = end;
                continue;
            } else if next.is_ascii_digit() {
                // `$1`..`$9` (and longer runs) — positional parameter.
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && bytes[end].is_ascii_digit() {
                    end += 1;
                }
                let n: usize = w[start..end].parse().unwrap_or(0);
                flush(&mut parts, &mut lit);
                parts.push(StringPart::Expr(positional_expr(n, in_fn)));
                i = end;
                continue;
            } else if next == b'@' || next == b'*' {
                // `$@` / `$*` — all positional parameters.
                flush(&mut parts, &mut lit);
                let name = if in_fn { "_" } else { "ARGV" };
                parts.push(StringPart::ArrayVar(name.to_string()));
                i += 2;
                continue;
            } else if next == b'#' {
                // `$#` arg count, `$#name` array element count.
                let start = i + 2;
                let mut end = start;
                while end < bytes.len()
                    && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_')
                {
                    end += 1;
                }
                let arr = if end > start {
                    &w[start..end]
                } else if in_fn {
                    "_"
                } else {
                    "ARGV"
                };
                flush(&mut parts, &mut lit);
                parts.push(StringPart::Expr(ex(ExprKind::Bareword(format!(
                    "#{{scalar(@{arr})}}"
                )))));
                i = end;
                continue;
            } else if next == b'?' || next == b'$' {
                // `$?` last exit status, `$$` pid — stryke spells both the same.
                flush(&mut parts, &mut lit);
                parts.push(StringPart::Expr(ex(ExprKind::Bareword(
                    format!("${}", next as char),
                ))));
                i += 2;
                continue;
            } else if next == b'(' {
                // command substitution `$( … )` → `#{qx " … "}`
                let start = i + 2;
                let mut depth = 1;
                let mut j = start;
                while j < bytes.len() && depth > 0 {
                    match bytes[j] {
                        b'(' => depth += 1,
                        b')' => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                let inner_end = if depth == 0 { j - 1 } else { j };
                flush(&mut parts, &mut lit);
                parts.push(cmdsub_interp(&w[start..inner_end.min(w.len())]));
                i = j;
                continue;
            }
        }
        lit.push(bytes[i] as char);
        i += 1;
    }
    flush(&mut parts, &mut lit);
    parts
}

/// A shell positional parameter `$n` as a stryke expression. `$0` is the program
/// name (`$0` in stryke too); `$1`.. index into `@ARGV` at the top level or `@_`
/// inside a function (shell positional params become the function's args).
/// A command substitution embedded in a larger string → a `#{qx " … "}`
/// interpolation that captures the command's output at that point.
fn cmdsub_interp(inner: &str) -> StringPart {
    StringPart::Expr(ex(ExprKind::Bareword(format!(
        "#{{qx \"{}\"}}",
        inner.trim().replace('"', "\\\"")
    ))))
}

fn positional_expr(n: usize, in_fn: bool) -> Expr {
    if n == 0 {
        return ex(ExprKind::ScalarVar("0".to_string()));
    }
    let array = if in_fn { "_" } else { "ARGV" };
    ex(ExprKind::ArrayElement {
        array: array.to_string(),
        index: Box::new(ex(ExprKind::Integer((n - 1) as i64))),
    })
}

fn is_var_name(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(ch) if ch.is_ascii_alphabetic() || ch == '_')
        && c.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn shell_stage(s: &ZshSimple) -> String {
    s.words
        .iter()
        .map(|w| shell_quote(&untok(w)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(w: &str) -> String {
    if w.is_empty() {
        return "''".to_string();
    }
    // Words containing shell expansion (`$var`, `$(...)`, backticks) pass
    // through unquoted: `$var` is handled later by `system_arg` (stryke
    // interpolation), and `$(...)` / backticks stay for the shell to expand.
    if w.contains('$') || w.contains('`') {
        return w.to_string();
    }
    // Glob metacharacters also pass through so the shell can expand them.
    let safe = w
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._-/=:@%+,*?[]{}~^".contains(c));
    if safe {
        w.to_string()
    } else {
        format!("'{}'", w.replace('\'', "'\\''"))
    }
}

/// One simple command rendered as a shell string: its words plus any
/// redirections. Returns `None` (after a warning) when a redirection has no
/// faithful single-line shell form (heredoc / process substitution).
fn simple_shell_cmd(s: &ZshSimple, ctx: &mut Ctx) -> Option<String> {
    let mut cmd = shell_stage(s);
    for r in &s.redirs {
        match redir_to_shell(r) {
            Some(rd) => {
                cmd.push(' ');
                cmd.push_str(&rd);
            }
            None => {
                ctx.warn("heredoc / process-substitution redirection not converted; command skipped");
                return None;
            }
        }
    }
    Some(cmd)
}

/// Reconstruct a redirection's shell spelling. `rtype` values are zsh's
/// `REDIR_*` constants (`zshrs::ported::zsh_h`): WRITE=0, WRITENOW=1, APP=2,
/// APPNOW=3, ERRWRITE=4…ERRAPPNOW=7, READWRITE=8, READ=9, HERESTR=12,
/// MERGEIN=13, MERGEOUT=14, CLOSE=15. Heredocs (10/11) and process
/// substitution (16/17) have no single-line form → `None`.
fn redir_to_shell(r: &ZshRedir) -> Option<String> {
    // (operator, default fd, whether an explicit fd prefix may be shown)
    let (op, default_fd, show_fd): (&str, i32, bool) = match r.rtype & 0x1f {
        0 => (">", 1, true),
        1 => (">|", 1, true),
        2 => (">>", 1, true),
        3 => (">>|", 1, true),
        4 => ("&>", 1, false),
        5 => ("&>|", 1, false),
        6 => ("&>>", 1, false),
        7 => ("&>>|", 1, false),
        8 => ("<>", 0, true),
        9 => ("<", 0, true),
        12 => ("<<<", 0, true),
        13 => ("<&", 0, true),
        14 => (">&", 1, true),
        15 => (">&-", 1, true), // close
        _ => return None,
    };
    let name = untok(&r.name);
    let fd = if show_fd && r.fd >= 0 && r.fd != default_fd {
        r.fd.to_string()
    } else {
        String::new()
    };
    Some(format!("{fd}{op}{name}"))
}

/// Build the `system(...)` argument for a shell command string as a stryke
/// double-quoted literal (emitted verbatim via `Bareword`). A simple scalar
/// `$name` / `${name}` is left as a stryke interpolation so the variable's value
/// flows into the command; every other `$`, the array sigil `@`, the
/// interpolation opener `#{`, `"`, and `\` are backslash-escaped so the shell —
/// not stryke — handles them.
fn system_arg(cmd: &str) -> Expr {
    let b = cmd.as_bytes();
    let mut out = String::from("\"");
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'$' && i + 1 < b.len() && (b[i + 1].is_ascii_alphabetic() || b[i + 1] == b'_') {
            // `$name` scalar — keep as stryke interpolation
            out.push('$');
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                out.push(b[i] as char);
                i += 1;
            }
            continue;
        }
        if c == b'$' && i + 1 < b.len() && b[i + 1] == b'{' {
            if let Some(close) = cmd[i + 2..].find('}') {
                let name = &cmd[i + 2..i + 2 + close];
                if is_var_name(name) {
                    out.push_str("${");
                    out.push_str(name);
                    out.push('}');
                    i += 2 + close + 1;
                    continue;
                }
            }
        }
        match c {
            b'$' => out.push_str("\\$"),
            b'@' => out.push_str("\\@"),
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'#' if i + 1 < b.len() && b[i + 1] == b'{' => out.push_str("\\#"),
            _ => out.push(c as char),
        }
        i += 1;
    }
    out.push('"');
    ex(ExprKind::Bareword(out))
}

fn is_decl_keyword(head: &str) -> bool {
    matches!(
        head,
        "typeset" | "local" | "declare" | "integer" | "float" | "readonly" | "export"
    )
}

fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "echo" | "print" | "printf" | "typeset" | "declare" | "local" | "export" | "readonly"
            | "integer" | "float" | "read" | "cd" | "chdir" | "pwd" | "pushd" | "popd" | "dirs"
            | "setopt" | "unsetopt" | "set" | "unset" | "shift" | "return" | "break" | "continue"
            | "let" | "eval" | "source" | "." | "alias" | "unalias" | "true" | "false" | ":"
            | "test" | "[" | "[[" | "exit" | "trap" | "bindkey" | "zstyle" | "autoload"
            | "functions" | "whence" | "which" | "type" | "builtin" | "command" | "emulate"
            | "zmodload" | "zle" | "vared" | "getopts" | "hash" | "jobs" | "kill" | "wait"
            | "fc" | "history" | "bg" | "fg" | "disown"
    )
}

/// Small slice helper: if the first element equals `tok`, return the rest.
trait StripFirst {
    fn strip_first_if(&self, tok: &str) -> Option<Vec<String>>;
}
impl StripFirst for Vec<String> {
    fn strip_first_if(&self, tok: &str) -> Option<Vec<String>> {
        match self.split_first() {
            Some((h, rest)) if h == tok => Some(rest.to_vec()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(src: &str) -> String {
        convert_zsh(src, "t").0
    }

    #[test]
    fn echo_native_external_system() {
        assert!(!conv("echo hi").contains("system"));
        assert!(conv("ls -la").contains("system"));
    }

    #[test]
    fn interpolation_builds_scalar_parts() {
        match word_to_expr("count is $count", false).kind {
            ExprKind::InterpolatedString(parts) => assert!(parts
                .iter()
                .any(|p| matches!(p, StringPart::ScalarVar(n) if n == "count"))),
            other => panic!("expected InterpolatedString, got {other:?}"),
        }
        assert!(matches!(word_to_expr("plain", false).kind, ExprKind::String(_)));
    }

    #[test]
    fn positional_params_map_to_argv_or_args() {
        // Top level: `$1` → `$ARGV[0]`, never a literal `$1` (which stryke
        // would read as a regex capture group).
        let top = conv("echo $1\n");
        assert!(top.contains("$ARGV[0]"), "top-level $1 → $ARGV[0]:\n{top}");
        assert!(!top.contains("$1"), "must not leave a literal $1:\n{top}");
        // Inside a function: `$1` → `$_[0]`.
        let infn = conv("greet() {\necho $1\n}\n");
        assert!(infn.contains("$_[0]"), "function $1 → $_[0]:\n{infn}");
        // `$@` becomes the arg array.
        let all = conv("echo $@\n");
        assert!(all.contains("@ARGV"), "$@ → @ARGV:\n{all}");
    }

    #[test]
    fn typeset_becomes_var_decl() {
        assert!(!conv("typeset -i count=5").contains("system"));
        assert!(conv("name=value").contains("name"));
    }

    #[test]
    fn never_emits_bare_my() {
        // Style rule 10: declarations use `val`/`var`, never bare `my`.
        let out = conv("x=1\ny=2\nx=3\n");
        assert!(!out.contains("my "), "must not emit bare `my`:\n{out}");
    }

    #[test]
    fn write_once_is_val_rewritten_is_var() {
        // `x` is assigned twice → its declaration is `var`, and the second
        // assignment is a reassignment (no second declaration keyword). `y` is
        // assigned once → `val`.
        let out = conv("x=1\ny=2\nx=3\n");
        assert!(out.contains("var $x = 1"), "x should declare as var:\n{out}");
        assert!(out.contains("val $y = 2"), "y should declare as val:\n{out}");
        // The reassignment line is bare `$x = 3`, not another declaration.
        assert!(out.contains("$x = 3"), "x must be reassigned:\n{out}");
        assert_eq!(
            out.matches("$x =").count(),
            2,
            "exactly one decl + one reassign for x:\n{out}"
        );
        // Only one keyword introduces x.
        assert_eq!(out.matches("var $x").count(), 1, "{out}");
    }

    #[test]
    fn readonly_is_always_val() {
        let out = conv("readonly NAME=fixed\n");
        assert!(out.contains("val $NAME = "), "readonly → val:\n{out}");
    }

    #[test]
    fn reassignment_inside_block_shares_scope() {
        // `count` is declared at top level, then reassigned inside the loop
        // body — the loop body shares the enclosing scope, so the inner write
        // is a reassignment, not a redeclaration.
        let out = conv("count=0\nfor f in a b c\ndo\ncount=1\ndone\n");
        assert!(out.contains("var $count = 0"), "decl as var:\n{out}");
        assert_eq!(
            out.matches("var $count").count(),
            1,
            "no redeclaration inside the loop:\n{out}"
        );
    }

    #[test]
    fn function_body_is_its_own_scope() {
        // A name written once at top level and once in a function is `val` in
        // each — the function body does not share the top-level scope.
        let out = conv("x=1\nfoo() {\nx=2\n}\n");
        assert!(out.contains("val $x = 1"), "top-level x is val:\n{out}");
        assert!(out.contains("val $x = 2"), "function x is val:\n{out}");
    }

    #[test]
    fn arith_sigil_rewrite_preserves_operators() {
        // Bare identifiers gain `$`; numbers, `$vars`, operators and parens stay.
        assert_eq!(arith_to_stryke("i + 1"), "$i + 1");
        assert_eq!(arith_to_stryke("(a + b) * c"), "($a + $b) * $c");
        assert_eq!(arith_to_stryke("count = count * 2"), "$count = $count * 2");
        assert_eq!(arith_to_stryke("$x + 0x1f"), "$x + 0x1f");
        assert_eq!(arith_to_stryke("i++"), "$i++");
    }

    #[test]
    fn arith_value_is_real_expression_not_string() {
        // `x=$(( 2 + 3 ))` must produce `2 + 3`, not a quoted string.
        let out = conv("x=$(( 2 + 3 ))\n");
        assert!(out.contains("= 2 + 3"), "arith value not unwrapped:\n{out}");
        assert!(!out.contains("\"2 + 3\""), "arith must not be stringified:\n{out}");
    }

    #[test]
    fn arith_command_becomes_expression() {
        // `(( count++ ))` → `$count++` statement, not a system() call.
        let out = conv("count=0\n(( count++ ))\n");
        assert!(out.contains("$count++"), "arith command not converted:\n{out}");
        assert!(!out.contains("system"), "arith must not reach system:\n{out}");
    }

    #[test]
    fn cstyle_for_becomes_c_for() {
        let out = conv("for ((i=0; i<3; i++))\ndo\necho $i\ndone\n");
        // init declares the loop var, condition + step are real expressions
        // (spacing follows the source — `i<3` has no internal spaces).
        assert!(out.contains("var $i = 0"), "for-init decl missing:\n{out}");
        assert!(out.contains("$i<3"), "for-cond missing:\n{out}");
        assert!(out.contains("$i++"), "for-step missing:\n{out}");
        assert!(!out.contains("not yet converted"), "{out}");
    }

    #[test]
    fn control_flow_builtins_map_natively() {
        // `return`/`break`/`continue` previously fell through to a warning and
        // were dropped — they are core control flow and must convert.
        assert!(conv("helper() {\nreturn 5\n}\n").contains("return 5"));
        let loops = conv("for f in a b\ndo\nbreak\ncontinue\ndone\n");
        assert!(loops.contains("last"), "break → last:\n{loops}");
        assert!(loops.contains("next"), "continue → next:\n{loops}");
        assert!(conv("exit 1\n").contains("exit(1)"), "exit n");
        assert!(conv("exit\n").contains("exit"), "bare exit");
    }

    #[test]
    fn stack_and_arg_builtins_map() {
        assert!(conv("shift\n").contains("shift @ARGV"), "top-level shift → @ARGV");
        assert!(conv("f() {\nshift\n}\n").contains("shift @_"), "in-fn shift → @_");
        assert!(conv("pushd /tmp\n").contains("pushd \"/tmp\""), "pushd");
        assert!(conv("popd\n").contains("popd("), "popd");
        assert!(conv("pwd\n").contains("cwd("), "pwd → cwd");
        // `:` / `true` are no-ops: emitted as nothing, no warning.
        let noop = convert_zsh(":\ntrue\n", "t");
        assert!(noop.1.is_empty(), "no warnings for no-ops: {:?}", noop.1);
    }

    #[test]
    fn control_flow_in_logical_chain() {
        // `cmd || return 1` / `cmd && break` must survive — these were dropped
        // when control-flow ops only worked as standalone statements.
        let r = conv("f() {\nls /x || return 1\n}\n");
        assert!(r.contains("|| return 1"), "chain return:\n{r}");
        let b = conv("for x in 1 2\ndo\nls /x && break\ndone\n");
        assert!(b.contains("&& last"), "chain break:\n{b}");
    }

    #[test]
    fn user_function_call_is_native() {
        // A call to a script-defined function is a native, namespaced stryke
        // call, not system(). The namespace (`t::`) is derived from the file.
        let out = conv("greet() {\necho hi\n}\ngreet world\n");
        assert!(out.contains("fn t::greet"), "namespaced definition:\n{out}");
        assert!(
            out.contains("t::greet(\"world\")") || out.contains("t::greet \"world\""),
            "namespaced call:\n{out}"
        );
        assert!(!out.contains("system(\"greet"), "must not shell out:\n{out}");
    }

    #[test]
    fn colliding_function_name_is_namespaced() {
        // `log` is a stryke builtin — defining it in `package main` is an error,
        // and an unqualified `log` call resolves to the builtin. The namespace
        // makes both the definition and the call resolve to the script's `log`.
        let out = conv("log() {\necho hi\n}\nlog start\n");
        assert!(out.contains("fn t::log"), "namespaced def:\n{out}");
        assert!(
            out.contains("t::log(\"start\")") || out.contains("t::log \"start\""),
            "namespaced call:\n{out}"
        );
    }

    #[test]
    fn namespace_from_path_sanitizes() {
        assert_eq!(namespace_from_path("/x/deploy.zsh"), "deploy");
        assert_eq!(namespace_from_path("my-deploy.sh"), "my_deploy");
        assert_eq!(namespace_from_path("-"), "zsh");
        assert_eq!(namespace_from_path("/x/9lives.zsh"), "z9lives");
    }

    #[test]
    fn external_arg_interpolates_scalar_var() {
        // `$dir` flows into the command as stryke interpolation, not a literal
        // single-quoted `'$dir'`.
        let out = conv("ls -la $dir\n");
        assert!(out.contains("system(\"ls -la $dir\")"), "want clean interp:\n{out}");
    }

    #[test]
    fn redirections_reconstructed() {
        assert!(conv("cat $f > out.txt\n").contains("system(\"cat $f >out.txt\")"));
        assert!(conv("grep foo bar.txt 2>&1\n").contains("2>&1"));
        assert!(conv("sort < in.txt\n").contains("system(\"sort <in.txt\")"));
        assert!(conv("echo hi >> log\n").contains("system(\"echo hi >>log\")"));
    }

    #[test]
    fn builtin_only_pipeline_goes_to_shell() {
        // `echo $x | grep foo` — both stages run in the shell; `$x` interpolates.
        let out = conv("x=1\necho $x | grep foo\n");
        assert!(out.contains("system(\"echo $x | grep foo\")"), "{out}");
        assert!(!out.contains("not converted"), "{out}");
    }

    #[test]
    fn command_subst_in_external_arg_stays_for_shell() {
        // `$(date)` must reach the shell verbatim (escaped so stryke leaves it
        // alone), not be mis-read as the `$(` special var.
        let out = conv("touch file-$(date +%s)\n");
        assert!(out.contains("\\$(date +%s)"), "command subst must be escaped:\n{out}");
    }

    #[test]
    fn subshell_and_group_inlined() {
        let sub = conv("(echo a; echo b)\n");
        assert!(sub.contains("p \"a\""), "subshell body inlined:\n{sub}");
        assert!(sub.contains("p \"b\""), "subshell body inlined:\n{sub}");
    }

    #[test]
    fn command_substitution_becomes_qx() {
        // Whole value → clean `qx "cmd"`.
        let dollar = conv("now=$(date)\n");
        assert!(dollar.contains("qx \"date\""), "$(...) → qx:\n{dollar}");
        let tick = conv("now=`date`\n");
        assert!(tick.contains("qx \"date\""), "backticks → qx:\n{tick}");
        // Embedded in a string → `#{qx "cmd"}` interpolation.
        let embed = conv("echo \"today is $(date)\"\n");
        assert!(embed.contains("#{qx \"date\"}"), "embedded $(...) → #{{qx}}:\n{embed}");
    }

    #[test]
    fn let_builtin_converts_arithmetic() {
        let out = conv("x=1\nlet \"x = x + 2\"\n");
        assert!(out.contains("$x = $x + 2"), "let not converted:\n{out}");
        assert!(!out.contains("system"), "let must not reach system:\n{out}");
    }

    #[test]
    fn zcond_file_test_maps_to_filetest() {
        use zsh::zsh_ast::ZshCond;
        let mut ctx = Ctx::new();
        let e = map_zcond(&ZshCond::Unary("-f".into(), "x".into()), &mut ctx);
        assert!(matches!(e.kind, ExprKind::FileTest { op: 'f', .. }), "{:?}", e.kind);
    }

    #[test]
    fn zcond_numeric_and_string_comparisons() {
        use zsh::zsh_ast::ZshCond;
        let mut ctx = Ctx::new();
        let n = map_zcond(&ZshCond::Binary("a".into(), "-eq".into(), "b".into()), &mut ctx);
        assert!(matches!(n.kind, ExprKind::BinOp { op: BinOp::NumEq, .. }));
        let s = map_zcond(&ZshCond::Binary("a".into(), "==".into(), "b".into()), &mut ctx);
        assert!(matches!(s.kind, ExprKind::BinOp { op: BinOp::StrEq, .. }));
        let z = map_zcond(&ZshCond::Unary("-z".into(), "v".into()), &mut ctx);
        assert!(matches!(z.kind, ExprKind::BinOp { op: BinOp::StrEq, .. }));
    }

    #[test]
    fn zcond_not_and_or() {
        use zsh::zsh_ast::ZshCond;
        let mut ctx = Ctx::new();
        let c = ZshCond::And(
            Box::new(ZshCond::Unary("-f".into(), "a".into())),
            Box::new(ZshCond::Not(Box::new(ZshCond::Unary("-d".into(), "b".into())))),
        );
        assert!(matches!(map_zcond(&c, &mut ctx).kind, ExprKind::BinOp { op: BinOp::LogAnd, .. }));
    }
}
