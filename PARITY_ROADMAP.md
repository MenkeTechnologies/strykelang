# Perl 5 parity roadmap (perlrs)

This is an **ordered engineering program**, not a promise of bit-identical `perl` for every program. Each phase has a **definition of done** so work can land in sequence without scope creep.

---

## Phase 0 — Measure (baseline)

**Goal:** Stop guessing; every later change is justified by a failing parity case or a new `parity/cases/*.pl` file.

**Done when:**

- `parity/run_parity.sh` runs `perl` and `pe` on the same scripts and compares **combined stdout+stderr** (exact bytes, `LC_ALL=C`).
- `parity/cases/` holds at least a small **seed** set that passes today; new cases are added **whenever** a bug is fixed or a Perl edge case is claimed.

**Run:** `bash parity/run_parity.sh` (from repo root; builds `target/release/pe` if missing).

---

## Completed slices (reference)

- **`^=` / `<<=` / `>>=`** — lexer already emitted [`Token::XorAssign`](src/token.rs) / [`ShiftLeftAssign`](src/token.rs) / [`ShiftRightAssign`](src/token.rs); [`parse_assign_expr`](src/parser.rs) now parses them as [`ExprKind::CompoundAssign`](src/ast.rs) with [`BinOp::BitXor`](src/ast.rs) / [`ShiftLeft`](src/ast.rs) / [`ShiftRight`](src/ast.rs). Scalar bytecode uses existing [`Op::ScalarCompoundAssign`](src/bytecode.rs) (same op table as `+=` / `|=`). Covered by [`crate_api_tests::run_compound_assign_xor_shift`](src/crate_api_tests.rs) and [`parser_shape_tests::shape_compound_assign_xor_shift`](src/parser_shape_tests.rs).

- **Typeglob assignment** — `*foo = \&bar` and `*foo = *bar` copy subroutine entries (`Interpreter::subs`) plus scalar/array/hash slots and `glob_handle_alias` (`Interpreter::copy_typeglob_slots`). Tree interpreter only until bytecode supports assigning to `ExprKind::Typeglob`. Lexer: `y` after `::` is not forced into `tr`/`y` when followed by `;`, `=`, etc., so names like `Foo::y` tokenize as identifiers.
- **`__FILE__` / `__LINE__`** — compile-time literals; bytecode uses `Compiler::source_file` (wired from `Interpreter::file` in `try_vm_execute`). Covered by `parity/cases/006_magic_line.pl` and integration tests.
- **`$!` (errno dualvar)** — numeric errno + string message (`PerlValue::errno_dual` / `ErrnoDual`); I/O paths set `errno` / `errno_code`; assignment to `$!` updates both (see [`SPECIAL_VARIABLES.md`](SPECIAL_VARIABLES.md)). Parity cases for errno-heavy paths still welcome.
- **`$@` (eval dualvar)** — `eval_error` + `eval_error_code` with the same heap representation as **`$!`**; `eval` / `evalblock` use `set_eval_error` / `clear_eval_error` (see [`SPECIAL_VARIABLES.md`](SPECIAL_VARIABLES.md)).
- **`%SIG` (Unix)** — `SIGINT` / `SIGTERM` / `SIGALRM` / `SIGCHLD` invoke `%SIG{…}` code refs **between statements** via [`src/perl_signal.rs`](src/perl_signal.rs). Subprocess / controlled parity cases still welcome.
- **`${^GLOBAL_PHASE}`** — tree-walker [`execute_tree`](src/interpreter.rs) and bytecode [`compile_program`](src/compiler.rs) both drive **`Interpreter::global_phase`** to match Perl 5 (including **`START`** during **`UNITCHECK`** blocks; VM emits **`Op::SetGlobalPhase`**). Parity: [`parity/cases/007_global_phase.pl`](parity/cases/007_global_phase.pl), [`parity/cases/009_global_phase_all.pl`](parity/cases/009_global_phase_all.pl). **`DESTRUCT`** is not modeled.
- **Lexer `${^NAME}`** — `${…}` after **`$`** is matched before the single-character special branch so **`${^GLOBAL_PHASE}`** tokenizes as one scalar (not **`$` `{`**).
- **String `\x{hex}`** — double-quoted / `qq` escapes: braced Unicode scalars and unbraced one- or two-digit hex (Perl-like). Parity: [`parity/cases/010_string_hex_escape.pl`](parity/cases/010_string_hex_escape.pl).
- **`$@` (eval/die)** — `die` / `warn` append **` at FILE line N.`** (trailing period before newline) when the message does not already end with newline, matching Perl 5’s **`$@`** for `eval { die }`. Parity: [`parity/cases/008_eval_at.pl`](parity/cases/008_eval_at.pl).
- **Lexer `x` / parser call shapes** — `x` tokenizes as the repetition operator only after a complete term (`3 x 4`); when a term is expected (`sub x {`, leading bare `x`, …) it stays an identifier. **Parser:** comma after paren-less `->method` ends the method “arg” list so `foo($obj->meth, $y)` parses; **`$coderef(...)`** and **`&$coderef(...)`** are [`ExprKind::IndirectCall`](src/ast.rs) (tree interpreter evaluates coderefs / symbolic names); **`&$cr`** with no following **`(...)`** passes the caller’s **`@_`** ([`IndirectCall::pass_caller_arglist`](src/ast.rs)); while parsing **`sort $coderef (LIST)`**’s comparator, postfix `(` is not folded into an indirect call so **`sort $k (1)`** still treats `(1)` as the sort list. **`goto EXPR`** takes postfix **`if`/`unless`** like **`last`/`next`** (e.g. **`goto &$boots if defined &$boots;`** in **`XSLoader.pm`**). Statement labels are **`Ident:`** at statement start when the next token is a single colon (not **`::`**), including mixed-case **`boot:`** for **`XSLoader.pm`**. **`pe -c`** accepts the system core **`B.pm`** from a typical Homebrew **`perl`** install (path varies by OS/version).

## Phase 1 — Documented runtime gaps (specials, I/O, signals)

**Goal:** Close items in [`SPECIAL_VARIABLES.md`](SPECIAL_VARIABLES.md) and related README bullets in a **test-driven** way.

**Rough order (dependencies matter):**

1. **`$@`** — extend **parity** for **`eval`** / **`die`** edge cases still diverging from Perl 5 (e.g. string-`eval` errors, `eval` in VM-only paths).
2. **`%SIG`** — extend coverage (more signals, Windows behavior if desired); add parity cases for `SIGINT`/`SIGTERM`/`SIGALRM`/`SIGCHLD` in a controlled subprocess.
3. **`$.` / per-handle line counters** — align with Perl where feasible; add file-reading cases.
4. **`${^GLOBAL_PHASE}`** — extend parity cases for **`UNITCHECK` / `CHECK` / `INIT`** if needed; **`DESTRUCT`** when modeled.

**Done when:** Each item has parity cases (or explicit `SKIP` in `perl` with a comment in the case file explaining why Perl differs).

---

## Phase 2 — VM / bytecode coverage (tree-only features)

**Goal:** Reduce `execute_tree` fallback for hot paths.

**Targets:** `typed my`, `each`, complex lvalues, and any construct currently marked `Unsupported` in [`src/compiler.rs`](src/compiler.rs) where semantics are stable. **Done (VM + tree):** `try`/`catch`/`finally`; `given`/`when`/`default` via [`Op::Given`](src/bytecode.rs) (body still interpreted); [`Op::EvalTimeout`](src/bytecode.rs); algebraic `match` via [`Op::AlgebraicMatch`](src/bytecode.rs).

**Progress (non-exhaustive):** **Hash slice through a scalar ref** `@$href{k1,k2}` ([`ExprKind::HashSliceDeref`](src/ast.rs)) compiles to [`Op::HashSliceDeref`](src/bytecode.rs) for reads (delegates to [`Interpreter::hash_slice_deref_values`](src/interpreter.rs)) and [`Op::SetHashSliceDeref`](src/bytecode.rs) for assignment (delegates to [`Interpreter::assign_hash_slice_deref`](src/interpreter.rs)); plain `%h` / `$h{k}` slices were already covered. **Array slice through a scalar ref** `@$aref[i1,i2,…]` parses as [`ExprKind::ArrowDeref`](src/ast.rs) with [`DerefKind::Array`](src/ast.rs) and a [`ExprKind::List`](src/ast.rs) index; the compiler uses [`Compiler::compile_arrow_array_base_expr`](src/compiler.rs) so the base is the **array reference** (scalar), not `SymbolicDeref` expansion; multi-index reads emit [`Op::ArrowArraySlice`](src/bytecode.rs) (VM uses [`Interpreter::read_arrow_array_element`](src/interpreter.rs) per index). Literal **`@$r[i,j] = (u,v)`** assignment compiles to a **`SetArrowArray`** per pair when the RHS is a matching [`ExprKind::List`](src/ast.rs); other slice-assign RHS shapes stay on the tree interpreter. Single-index **`@$r[i]`** uses the same ref peel + [`Op::ArrowArray`](src/bytecode.rs) / [`Op::SetArrowArray`](src/bytecode.rs) as **`$r->[i]`**. **Single-key** compound assignment **`@$href{"k"} OP= EXPR`** (e.g. `+=`, `//=`, `||=`, `&&=`) lowers like **`$href->{"k"} OP= EXPR`** — [`Compiler::compile_expr`](src/compiler.rs) `CompoundAssign` + [`Op::Dup2`](src/bytecode.rs) + [`Op::ArrowHash`](src/bytecode.rs) + [`Op::SetArrowHash`](src/bytecode.rs) / [`Op::SetArrowHashKeep`](src/bytecode.rs). **Multi-key** `@$href{k1,k2} OP= EXPR` uses a dedicated [`Op::HashSliceDerefCompound`](src/bytecode.rs) that delegates to [`Interpreter::compound_assign_hash_slice_deref`](src/interpreter.rs) — reads the slice list, folds via `eval_binop` (scalar context — Perl's `@slice` in numeric context), then rewrites through [`Interpreter::assign_hash_slice_deref`](src/interpreter.rs); matches the tree-walker generic `CompoundAssign` fallback exactly, which still diverges from Perl 5's per-last-element semantics (tracked as a parity bug separate from VM coverage). Logical short-circuit compound ops (`//=`, `||=`, `&&=`) on multi-key slices stay `Unsupported` (the tree fallback doesn't handle them either). **`++@$href{"k"}`** / **`--@$href{"k"}`** / **`@$href{"k"}++`** use the same **`ArrowHash`** / **`SetArrowHashKeep`** / **`ArrowHashPostfix`** sequences as **`$href->{"k"}`** (multi-key stays `Unsupported`). `do { } while (COND)` is parsed as [`StmtKind::DoWhile`](src/ast.rs) and compiled to the bytecode VM; `splice` / `unshift` on plain `@array` compile to `CallBuiltin` with real mutating implementations (see [`Interpreter::splice_builtin_execute`](src/interpreter.rs)). [`ExprKind::IndirectCall`](src/ast.rs) (`$coderef(...)`, `&$coderef(...)`, `&$cr` with caller `@_`) compiles to [`Op::IndirectCall`](src/bytecode.rs) and uses [`Interpreter::dispatch_indirect_call`](src/interpreter.rs) in the VM. Dynamic coderef `\&{ EXPR }` ([`ExprKind::DynamicSubCodeRef`](src/ast.rs)) compiles to [`Op::LoadDynamicSubRef`](src/bytecode.rs) (name from stack at run time). Dynamic typeglob `*{ EXPR }` ([`ExprKind::TypeglobExpr`](src/ast.rs)) compiles to [`Op::LoadDynamicTypeglob`](src/bytecode.rs) (resolved handle string via [`Interpreter::resolve_io_handle_name`](src/interpreter.rs)). `sort $coderef LIST` ([`SortComparator::Code`](src/ast.rs)) compiles to [`Op::SortWithCodeComparator`](src/bytecode.rs) (stack: list, then coderef; comparator uses `$a`/`$b` like the tree interpreter). **`grep EXPR, LIST`** ([`ExprKind::GrepExprComma`](src/ast.rs)) compiles to [`Op::GrepWithExpr`](src/bytecode.rs) with the filter expression in [`Chunk::grep_expr_entries`](src/bytecode.rs) (VM sets `$_` per element and evaluates the expression like the tree interpreter). Assignment to **`$href->{key}`** / blessed hash arrow deref ([`ExprKind::ArrowDeref`](src/ast.rs) with [`DerefKind::Hash`](src/ast.rs)) compiles to [`Op::SetArrowHash`](src/bytecode.rs) (delegates to [`Interpreter::assign_arrow_hash_deref`](src/interpreter.rs)). Assignment to **`$aref->[$i]`** ([`DerefKind::Array`](src/ast.rs)) compiles to [`Op::SetArrowArray`](src/bytecode.rs) (delegates to [`Interpreter::assign_arrow_array_deref`](src/interpreter.rs)); compound **`$aref->[$i] += EXPR`** uses [`Op::Dup2`](src/bytecode.rs) + [`Op::ArrowArray`](src/bytecode.rs) + the same rotate pattern as arrow-hash compound assigns; **`$href->{k} //=`** / **`||=`** and **`$aref->[$i] //=`** / **`||=`** use the same short-circuit jumps + [`Op::SetArrowHashKeep`](src/bytecode.rs) / [`Op::SetArrowArrayKeep`](src/bytecode.rs) (shortcut path drops ref/key or ref/index with `Swap`/`Pop` ×2). **Pre-**increment / **pre-**decrement on **`$aref->[$i]`** and **`$href->{k}`** (`++$x`, `--$x`) compile to **`Dup2`**, [`Op::ArrowArray`](src/bytecode.rs) / [`Op::ArrowHash`](src/bytecode.rs), arithmetic, **`Dup`**, [`Op::Pop`](src/bytecode.rs), **`Swap`**, **`Rot`**, **`Swap`**, then [`Op::SetArrowArrayKeep`](src/bytecode.rs) / [`Op::SetArrowHashKeep`](src/bytecode.rs) so the expression value (the new numeric slot) remains on the stack for **`my $v = ++…`**. Postfix **`$aref->[$i]++`** / **`$href->{k}++`** (and **`--`**) use [`Op::ArrowArrayPostfix`](src/bytecode.rs) / [`Op::ArrowHashPostfix`](src/bytecode.rs) (stack **`[ref, index|key]`**, key/index on top → pushes **old** value; [`Interpreter::arrow_array_postfix`](src/interpreter.rs) / [`arrow_hash_postfix`](src/interpreter.rs)). Arrow **method call** assign lhs (`$obj->meth(...) = …`) remains tree-only. **Symbolic ref deref** ([`ExprKind::Deref`](src/ast.rs) — `$$r`, `@{...}`, `%{...}`, `*{...}`) compiles to [`Op::SymbolicDeref`](src/bytecode.rs) (delegates to [`Interpreter::symbolic_deref`](src/interpreter.rs)); `strict refs` still forces the tree path via [`try_vm_execute`](src/lib.rs). Assignment **`$$r = EXPR`** uses [`Op::SetSymbolicScalarRef`](src/bytecode.rs) / [`Op::SetSymbolicScalarRefKeep`](src/bytecode.rs) (delegates to [`Interpreter::assign_scalar_ref_deref`](src/interpreter.rs)); **`$$r += EXPR`** uses [`Op::SymbolicDeref`](src/bytecode.rs) for the read and **`SetSymbolicScalarRef`** for the write; **`$$r //=`** / **`$$r ||=`** short-circuit with [`Op::JumpIfDefinedKeep`](src/bytecode.rs) / [`Op::JumpIfTrueKeep`](src/bytecode.rs) and [`Op::SetSymbolicScalarRefKeep`](src/bytecode.rs) (mirrors binary `//` / `||` in [`compile_expr`](src/compiler.rs)). Plain lexical **`$x //=`** / **`$x ||=`** use [`Compiler::emit_get_scalar`](src/compiler.rs) + the same keep-jumps + [`Compiler::emit_set_scalar_keep`](src/compiler.rs) (no extra `Pop` after the keep-jump). **`&&=`** ([`Token::AndAssign`](src/token.rs) in [`parse_assign_expr`](src/parser.rs)) uses [`Op::JumpIfFalseKeep`](src/bytecode.rs) with the same merge pattern as **`||=`**; [`compound_scalar_binop`](src/interpreter.rs) handles [`BinOp::LogAnd`](src/ast.rs) for [`scalar_compound_assign_scalar_target`](src/interpreter.rs). Plain **`$a[$i] //=`** / **`||=`** and **`$h{key} //=`** / **`||=`** use **`Dup`** + [`Op::GetArrayElem`](src/bytecode.rs) / [`Op::GetHashElem`](src/bytecode.rs), the keep-jumps, **`Swap`** + [`Op::SetArrayElemKeep`](src/bytecode.rs) / [`Op::SetHashElemKeep`](src/bytecode.rs), and a two-**`Pop`** shortcut path (index or key under the old value). **`++$$r`** / **`--$$r`** use **`Dup`** + [`Op::SymbolicDeref`](src/bytecode.rs) + arithmetic + **`Swap`** + **`SetSymbolicScalarRefKeep`**; **`$$r++`** / **`$$r--`** use [`Op::SymbolicScalarRefPostfix`](src/bytecode.rs) ([`Interpreter::symbolic_scalar_ref_postfix`](src/interpreter.rs)). **Aggregate** symbolic **`@{…}` / `%{…}`** (including **`@$…`** / **`%$…`**) pre/post **`++`/`--`** are **invalid in Perl 5** (`Can't modify array/hash dereference in …`); the tree interpreter returns the same class of error (no silent numeric-on-length bug). The bytecode compiler still hits **`Unsupported`** (`PreInc`/`PreDec`/`PostfixOp` on non-scalar) for those forms—**`try_vm_execute`** falls back, then errors. Assignment to **`@{ EXPR }`** / **`%{ EXPR }`** (array or hash ref, or package name string when not `strict refs`) uses [`Op::SetSymbolicArrayRef`](src/bytecode.rs) / [`Op::SetSymbolicHashRef`](src/bytecode.rs) ([`Interpreter::assign_symbolic_array_ref_deref`](src/interpreter.rs) / [`assign_symbolic_hash_ref_deref`](src/interpreter.rs)). Assigning through **`*{ EXPR }`** (symbolic glob) lhs remains tree-only. **`\$scalar`** (`scalar_binding_ref`) compiles to [`Op::MakeScalarBindingRef`](src/bytecode.rs) (not `MakeScalarRef` + `GetScalar`), so the ref aliases the live lexical/package slot like the tree interpreter.

**Done when:** Compiler emits bytecode for a subset; **existing** integration tests still pass; new parity cases cover **before/after** behavior.

---

## Phase 3 — Regular expressions

**Goal:** Either **document** divergence from Perl 5’s engine or **narrow** it with a deliberate strategy.

**Progress:** After expanding Perl `\Q…\E` / flags, compilation tries [`regex`](https://docs.rs/regex) first, then [`fancy-regex`](https://docs.rs/fancy-regex) on failure (e.g. backreferences), then **PCRE2** ([`pcre2`](https://docs.rs/pcre2)) when both Rust engines reject the pattern. See [`src/perl_regex.rs`](src/perl_regex.rs). This is still **not** full PCRE/Perl parity; it removes a large class of “invalid regex” hard failures.

**Done when:** `parity/cases/` includes regex patterns that matter to real scripts; failures drive a written **compatibility matrix** (not vibes).

---

## Phase 4 — `require` / `use` / pure-Perl core

**Goal:** Run more **pure-Perl** modules from `@INC` without silent wrong behavior.

**Progress:** `require` / `use` / `%INC` / Exporter-style import are implemented; [`src/perl_inc.rs`](src/perl_inc.rs) can merge system `@INC` from a real `perl`. The **Top-N CPAN smoke** harness lives under [`parity/cpan_topn/`](parity/cpan_topn/README.md): list in [`parity/cpan_topn/MODULES.txt`](parity/cpan_topn/MODULES.txt), install via `install_deps.sh`, run via `run_cpan_topn.sh` under **`pe`** with **`pe -I …/local/lib/perl5`**. `use VERSION` (e.g. `use 5.008;`) is accepted as a no-op so common CPAN headers parse. **`use constant NAME => EXPR`** is handled natively in [`Interpreter::apply_use_constant`](src/interpreter.rs) (installs a nullary sub per pair; does **not** load core `constant.pm`, which uses syntax we do not parse yet). Unquoted expression terms are [`ExprKind::Bareword`](src/ast.rs) (resolved to a nullary sub if one exists, else stringified like a Perl bareword); quoted `'…'` / `"…"` literals stay [`ExprKind::String`](src/ast.rs). Fat-arrow hash keys (`foo => …`) still parse as `String`. **`join`’s** list operands evaluate with list `wantarray` so `join(",", List::Util::uniq(...))` passes list context into `uniq`. Lexer accepts **`$::{$key}`** (main stash / `%::`) for core `Carp.pm` / introspection. Parser accepts **`*$_{…}`** / **`*$scalar`** (typeglob from a sigil expression) as [`TypeglobExpr`](src/ast.rs). Anonymous subs accept an optional prototype before the block: **`sub () { … }`** (shared `parse_sub_prototype_opt` with named subs) so assignments like **`*UTF8_REGEXP_PROBLEM = sub () { 1 }`** in **`Carp.pm`** parse. At statement boundaries, **`sub { … }`**, **`sub () { … }`**, and **`sub :ATTR { … }`** (no name) are accepted like Perl 5. For **JSON::PP**, put **`vendor/perl` before system paths in `-I`** so the minimal [`vendor/perl/Carp.pm`](vendor/perl/Carp.pm) shadows core Carp. **`q` / `qq` / `qw`** with pairing delimiters **`()[]{}<>`** balance nested delimiters to the outer closer (same idea as Perl): e.g. **`eval(q(sub ($) { 1 }))`** in **`Carp.pm`**, **`qw( (SV*)pWARN_ALL )`** in core **`B.pm`**. **`eval qq/…/`** with **`\$`** in the `qq` body still builds the same source Perl does (private-use sentinel in the lexer + `parse_interpolated_string`). **`require` of the full system `JSON/PP.pm`** can still hit other gaps (e.g. **`sub B::GV::SAFENAME`**-style package-qualified subs, core **`B.pm`** / **`Carp.pm`** beyond what the stubs cover — tracked as further work).
**Recent parser/runtime for `require` chains:** chained hash subscripts on deref results (`$x->{a}{b}` / `$h{k}{k2}`); `my`/`our` declaration with postfix `if`/`unless`; `sub name : attr { }` attribute skipping; statement labels as **`Ident:`** at statement start (not **`::`**; distinct from ternary `?:`); `our $x ||= 0` / `//=`; `\&{ EXPR }` (dynamic subroutine coderef) and `*{ EXPR }` (dynamic typeglob slot) for **Exporter.pm**-style code; `%$href` (hash deref of scalar ref); symbolic `@{ EXPR }`; **`local $h{k} = …`** (hash slot localization, e.g. **`local $SIG{__WARN__}`** in system **Exporter.pm**) — tree interpreter; bytecode falls back.

**Syntax error paths:** [`parse_with_file`](src/lib.rs) / [`Lexer::new_with_file`](src/lexer.rs) / [`Parser::new_with_file`](src/parser.rs) thread a **source path** into lexer and parser diagnostics (not hardcoded `-e`). The **`pe`** driver and **`require`** / **`do` file** loading use the real script or module path; [`parse_and_run_string_in_file`](src/lib.rs) sets [`Interpreter::file`](src/interpreter.rs) for the loaded unit so **`__FILE__`** matches during that execution.

**Done when:** `parity/cpan_topn` smoke is green in CI; **XS** modules either work via a bridge (Phase 6) or fail with a **clear** error.

---

## Phase 5 — `format` / `write` reports

**Goal:** Implement enough of Perl’s report formatting to run scripts that use `format`/`write`.

**Done when:** Parser + runtime + parity cases for picture lines and `write` (see [`src/format.rs`](src/format.rs)).

---

## Phase 6 — XS / embedding (only if required)

**Goal:** Run extensions that are not expressible in pure Perl.

**Options:** FFI to `libperl`, subprocess delegation to `perl`, or explicit “no XS” policy for this interpreter.

**Done when:** Policy is chosen and documented; one reference XS module is either supported or explicitly out of scope.

---

## Maintenance rule

- Any PR that claims “Perl compatibility” for a feature must add or extend **`parity/cases/`** (or `cargo test` coverage that compares to `perl` where applicable).
- Update this file when a **phase completes** or **scope changes**.
