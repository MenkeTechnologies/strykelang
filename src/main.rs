use std::fs::File;
use std::io::{self, BufReader, IsTerminal, Read as IoRead, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Mutex;

use clap::Parser;
use rand::Rng;
use rayon::prelude::*;

use stryke::ast::Program;
use stryke::error::{ErrorKind, PerlError};
use stryke::interpreter::Interpreter;
use stryke::perl_fs::{
    decode_utf8_or_latin1, read_file_text_perl_compat, read_line_perl_compat,
    read_logical_line_perl_compat,
};

mod repl;

/// stryke — A highly parallel Perl 5 interpreter written in Rust
#[derive(Parser, Debug, Default)]
#[command(name = "stryke", version, about, long_about = None)]
#[command(disable_version_flag = true, disable_help_flag = true)]
#[command(override_usage = "stryke [switches] [--] [programfile] [arguments]")]
pub(crate) struct Cli {
    /// Specify record separator (\0 if no argument); -0777 for slurp mode
    #[arg(short = '0', value_name = "OCTAL")]
    input_separator: Option<Option<String>>,

    /// Autosplit mode with -n or -p (splits $_ into @F)
    #[arg(short = 'a')]
    auto_split: bool,

    /// Enables the listed Unicode features
    #[arg(short = 'C', value_name = "NUMBER/LIST")]
    unicode_features: Option<Option<String>>,

    /// Check syntax only (parse; does not compile or run)
    #[arg(short = 'c')]
    check_only: bool,

    /// Parse and compile without executing (bytecode compile check; alias `--check`)
    #[arg(long = "lint", alias = "check")]
    lint: bool,

    /// Print bytecode disassembly to stderr before VM execution (alias `--disassemble`)
    #[arg(long = "disasm", alias = "disassemble")]
    disasm: bool,

    /// Dump the parsed abstract syntax tree as JSON to stdout and exit (no execution)
    #[arg(long = "ast")]
    dump_ast: bool,

    /// Pretty-print parsed Perl to stdout and exit (no execution)
    #[arg(long = "fmt")]
    format_source: bool,

    /// Wall-clock profile: per-line + per-sub timings on stderr (VM: opcode-level lines; JIT off)
    #[arg(long = "profile")]
    profile: bool,

    /// Flamegraph: colored terminal bars (TTY) or SVG to stdout (piped: stryke --flame x.stk > flame.svg)
    #[arg(long = "flame")]
    flame: bool,

    /// Disable Cranelift JIT for bytecode VM (opcode interpreter only)
    #[arg(long = "no-jit")]
    no_jit: bool,

    /// Print expanded hint for an error code (e.g. E0001) and exit
    #[arg(long = "explain", value_name = "CODE")]
    explain: Option<String>,

    /// Run program under debugger or module Devel::MOD
    #[arg(short = 'd', value_name = "MOD")]
    debugger: Option<Option<String>>,

    /// Set debugging flags (argument is a bit mask or alphabets)
    #[arg(short = 'D', value_name = "FLAGS")]
    debug_flags: Option<Option<String>>,

    /// One line of program (several -e's allowed, omit programfile)
    #[arg(short = 'e')]
    execute: Vec<String>,

    /// Like -e, but enables all optional features
    #[arg(short = 'E')]
    execute_features: Vec<String>,

    /// Don't do $sitelib/sitecustomize.pl at startup
    #[arg(short = 'f')]
    no_sitecustomize: bool,

    /// Split() pattern for -a switch (//'s are optional)
    #[arg(short = 'F', value_name = "PATTERN")]
    field_separator: Option<String>,

    /// Read all input in one go (slurp), alias for -0777
    #[arg(short = 'g')]
    slurp: bool,

    /// Edit <> files in place (makes backup if extension supplied)
    #[arg(short = 'i', value_name = "EXTENSION")]
    inplace: Option<Option<String>>,

    /// Specify @INC/#include directory (several -I's allowed)
    #[arg(short = 'I', value_name = "DIRECTORY")]
    include: Vec<String>,

    /// Enable line ending processing, specifies line terminator
    #[arg(short = 'l', value_name = "OCTNUM")]
    line_ending: Option<Option<String>>,

    /// Execute "use module..." before executing program
    #[arg(short = 'M', value_name = "MODULE")]
    use_module: Vec<String>,

    /// Execute "use module ()" before executing program (no import)
    #[arg(short = 'm', value_name = "MODULE")]
    use_module_no_import: Vec<String>,

    /// Assume "while (<>) { ... }" loop around program
    #[arg(short = 'n')]
    line_mode: bool,

    /// Assume loop like -n but print line also, like sed
    #[arg(short = 'p')]
    print_mode: bool,

    /// Enable rudimentary parsing for switches after programfile
    #[arg(short = 's')]
    switch_parsing: bool,

    /// Look for programfile using PATH environment variable
    #[arg(short = 'S')]
    path_lookup: bool,

    /// Enable tainting warnings
    #[arg(short = 't')]
    taint_warn: bool,

    /// Enable tainting checks
    #[arg(short = 'T')]
    taint_check: bool,

    /// Dump core after parsing program
    #[arg(short = 'u')]
    dump_core: bool,

    /// Allow unsafe operations
    #[arg(short = 'U')]
    unsafe_ops: bool,

    /// Print version, patchlevel and license
    #[arg(short = 'v')]
    show_version: bool,

    /// Print configuration summary (or a single Config.pm variable)
    #[arg(short = 'V', value_name = "CONFIGVAR")]
    show_config: Option<Option<String>>,

    /// Enable many useful warnings
    #[arg(short = 'w')]
    warnings: bool,

    /// Enable all warnings
    #[arg(short = 'W')]
    all_warnings: bool,

    /// Ignore text before #!perl line (optionally cd to directory)
    #[arg(short = 'x', value_name = "DIRECTORY")]
    extract: Option<Option<String>>,

    /// Disable all warnings
    #[arg(short = 'X')]
    no_warnings: bool,

    /// Print help
    #[arg(short = 'h', long = "help")]
    help: bool,

    /// Number of threads for parallel operations (stryke extension)
    #[arg(short = 'j', long = "threads", value_name = "N")]
    threads: Option<usize>,

    /// Perl 5 strict-compatibility mode: disable all stryke extensions
    #[arg(long = "compat")]
    compat: bool,

    /// Force argument to be treated as a script file (skip code detection)
    #[arg(long = "script")]
    force_script: bool,

    /// Script file to execute
    #[arg(value_name = "SCRIPT")]
    script: Option<String>,

    /// Arguments passed to the script (@ARGV)
    #[arg(value_name = "ARGS", trailing_var_arg = true)]
    args: Vec<String>,
}

/// Expand Perl-style bundled short switches (`-lane` → `-l -a -n -e`, `-0777` unchanged) before
/// clap parses. Stock clap treats `-lane` as `-l` with value `ane`.
fn expand_perl_bundled_argv(args: Vec<String>) -> Vec<String> {
    if args.is_empty() {
        return args;
    }
    let mut out = vec![args[0].clone()];
    let mut seen_dd = false;
    for arg in args.into_iter().skip(1) {
        if seen_dd {
            out.push(arg);
            continue;
        }
        if arg == "--" {
            seen_dd = true;
            out.push(arg);
            continue;
        }
        match expand_perl_bundled_token(&arg) {
            Some(parts) => out.extend(parts),
            None => out.push(arg),
        }
    }
    out
}

/// Perl documents `-help` / `-version` as aliases; bundling would mis-parse them as `-h`+`-e`+….
fn expand_perl_bundled_token(arg: &str) -> Option<Vec<String>> {
    match arg {
        "-help" | "--help" => return Some(vec!["-h".to_string()]),
        "-version" | "--version" => return Some(vec!["-v".to_string()]),
        _ => {}
    }
    if arg == "-" || !arg.starts_with('-') || arg.starts_with("--") {
        return None;
    }
    let s = arg.strip_prefix('-')?;
    if s.is_empty() || s.len() == 1 {
        return None;
    }
    // Operators like `->>`, `->`, `-~>` start with non-letter after `-`; not bundled flags.
    if s.starts_with('>') || s.starts_with('~') {
        return None;
    }
    // `-0` / `-0777` — record separator; do not split into `-0` `-7` …
    if let Some(rest) = s.strip_prefix('0') {
        let rest_ok = rest.chars().all(|c| matches!(c, '0'..='7'));
        if rest_ok {
            return None;
        }
    }
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        match b[i] {
            b'0' if i == 0 => {
                let mut j = i + 1;
                while j < b.len() && matches!(b[j], b'0'..=b'7') {
                    j += 1;
                }
                out.push("-0".to_string());
                if j > i + 1 {
                    out.push(s[i + 1..j].to_string());
                }
                i = j;
            }
            b'e' | b'E' => {
                let flag = if b[i] == b'e' { "-e" } else { "-E" };
                out.push(flag.to_string());
                if i + 1 < b.len() {
                    out.push(s[i + 1..].to_string());
                }
                return Some(out);
            }
            b'l' => {
                out.push("-l".to_string());
                i += 1;
                let start = i;
                while i < b.len() && matches!(b[i], b'0'..=b'7') {
                    i += 1;
                }
                if i > start {
                    out.push(s[start..i].to_string());
                }
            }
            // Flags that consume the rest of the token as their value:
            //   -F pattern  — split pattern for -a
            //   -M module   — use module
            //   -m module   — use module ()
            //   -I dir      — @INC directory
            //   -V:var      — config variable (Perl: `perl -V:version`)
            //   -d:mod      — debugger module
            //   -D flags    — debug flags
            //   -x dir      — ignore text before #!perl
            //   -C flags    — unicode features
            b'F' | b'M' | b'm' | b'I' | b'd' | b'D' | b'x' | b'C' => {
                let ch = b[i] as char;
                out.push(format!("-{ch}"));
                i += 1;
                if i < b.len() {
                    out.push(s[i..].to_string());
                }
                return Some(out);
            }
            b'V' => {
                // `-V:var` → `-V` `:var`; `-V` alone → `-V`
                out.push("-V".to_string());
                i += 1;
                if i < b.len() {
                    // Perl's `-V:version` passes `:version` but the handler expects just `version`.
                    let rest = &s[i..];
                    let rest = rest.strip_prefix(':').unwrap_or(rest);
                    out.push(rest.to_string());
                }
                return Some(out);
            }
            b'i' => {
                out.push("-i".to_string());
                i += 1;
                if i < b.len() && matches!(b[i], b'e' | b'E') {
                    continue;
                }
                if i < b.len() && b[i] == b'.' {
                    let start = i;
                    while i < b.len() && !matches!(b[i], b'e' | b'E') {
                        i += 1;
                    }
                    out.push(s[start..i].to_string());
                }
            }
            _ => {
                out.push(format!("-{}", b[i] as char));
                i += 1;
            }
        }
    }
    Some(out)
}

fn print_cyberpunk_help() {
    let version = env!("CARGO_PKG_VERSION");
    let bin = env!("CARGO_BIN_NAME");
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    // ANSI color codes
    const C: &str = "\x1b[36m"; // cyan
    const M: &str = "\x1b[35m"; // magenta
    const R: &str = "\x1b[31m"; // red
    const Y: &str = "\x1b[33m"; // yellow
    const G: &str = "\x1b[32m"; // green
    const N: &str = "\x1b[0m"; // reset

    println!("{C} ███████╗████████╗██████╗ ██╗   ██╗██╗  ██╗███████╗{N}");
    println!("{C} ██╔════╝╚══██╔══╝██╔══██╗╚██╗ ██╔╝██║ ██╔╝██╔════╝{N}");
    println!("{M} ███████╗   ██║   ██████╔╝ ╚████╔╝ █████╔╝ █████╗  {N}");
    println!("{M} ╚════██║   ██║   ██╔══██╗  ╚██╔╝  ██╔═██╗ ██╔══╝  {N}");
    println!("{R} ███████║   ██║   ██║  ██║   ██║   ██║  ██╗███████╗{N}");
    println!("{R} ╚══════╝   ╚═╝   ╚═╝  ╚═╝   ╚═╝   ╚═╝  ╚═╝╚══════╝{N}");
    println!("{C} ┌──────────────────────────────────────────────────┐{N}");
    println!("{C} │ STATUS: ONLINE // CORES: {threads:<2} // SIGNAL: ██░      │{N}");
    println!("{C} └──────────────────────────────────────────────────┘{N}");
    println!("{M}  >> PARALLEL PERL5 INTERPRETER // RUST-POWERED v{version} <<{N}");
    println!();
    println!();
    println!("A highly parallel Perl 5 interpreter written in Rust");
    println!();
    println!("{Y}  USAGE:{N} {bin} 'CODE'                     {G}//{N} -e is optional");
    println!("{Y}        {N} {bin} [switches] [--] [programfile] [arguments]");
    println!();
    println!("{C}  ── EXECUTION ──────────────────────────────────────────{N}");
    println!("  'CODE'                 {G}//{N} Inline code — no -e needed if arg looks like code");
    println!("  -e CODE                {G}//{N} Explicit inline (required with -n/-p/-l/-a)");
    println!("  -E CODE                {G}//{N} Like -e, but enables all optional features");
    println!("  --script               {G}//{N} Force arg to be a file (skip code detection)");
    println!("  -c                     {G}//{N} Check syntax only (parse; no compile/run)");
    println!("  --lint / --check       {G}//{N} Parse + compile bytecode without running");
    println!(
        "  --disasm / --disassemble {G}//{N} Print bytecode disassembly to stderr before VM run"
    );
    println!("  --ast                  {G}//{N} Dump parsed AST as JSON and exit (no execution)");
    println!("  --fmt                  {G}//{N} Pretty-print parsed Perl to stdout and exit");
    println!(
        "  --explain CODE         {G}//{N} Print expanded hint for an error code (e.g. E0001) and exit"
    );
    println!(
        "  --profile              {G}//{N} Wall-clock profile stderr (VM op lines; flamegraph-ready)"
    );
    println!(
        "  --flame                {G}//{N} Flamegraph: terminal bars (TTY) or SVG (piped to file)"
    );
    println!("  --no-jit               {G}//{N} Disable Cranelift JIT (bytecode interpreter only)");
    println!(
        "  --compat               {G}//{N} Perl 5 strict-compat: disable all stryke extensions"
    );
    println!("  -d[t][:MOD]            {G}//{N} Run program under debugger or module Devel::MOD");
    println!("  -D[number/letters]     {G}//{N} Set debugging flags");
    println!("  -u                     {G}//{N} Dump core after parsing program");
    println!("{C}  ── INPUT PROCESSING ─────────────────────────────────{N}");
    println!("  -n                     {G}//{N} Assume \"while (<>) {{...}}\" loop around program");
    println!("  -p                     {G}//{N} Like -n but print line also, like sed");
    println!("  -a                     {G}//{N} Autosplit mode (splits $_ into @F)");
    println!("  -F/pattern/            {G}//{N} split() pattern for -a switch");
    println!("  -l[octnum]             {G}//{N} Enable line ending processing");
    println!("  -0[octal]              {G}//{N} Specify record separator (\\0 if no arg)");
    println!("  -g                     {G}//{N} Slurp all input at once (alias for -0777)");
    println!("  -i[extension]          {G}//{N} Edit <> files in place (backup if ext supplied; multiple files in parallel)");
    println!("{C}  ── MODULES & PATHS ──────────────────────────────────{N}");
    println!("  -M MODULE              {G}//{N} Execute \"use module...\" before program");
    println!(
        "  -m MODULE              {G}//{N} Execute \"use module ()\" before program (no import)"
    );
    println!("  -I DIRECTORY           {G}//{N} Specify @INC directory (several allowed)");
    println!("  -f                     {G}//{N} Don't do $sitelib/sitecustomize.pl at startup");
    println!("  -S                     {G}//{N} Look for programfile using PATH");
    println!("  -x[directory]          {G}//{N} Ignore text before #!perl line");
    println!("{C}  ── UNICODE & SAFETY ─────────────────────────────────{N}");
    println!("  -C[number/list]        {G}//{N} Enable listed Unicode features");
    println!("  -t                     {G}//{N} Enable tainting warnings");
    println!("  -T                     {G}//{N} Enable tainting checks");
    println!("  -U                     {G}//{N} Allow unsafe operations");
    println!("  -s                     {G}//{N} Enable switch parsing for programfile args");
    println!("{C}  ── WARNINGS ─────────────────────────────────────────{N}");
    println!("  -w                     {G}//{N} Enable many useful warnings");
    println!("  -W                     {G}//{N} Enable all warnings");
    println!("  -X                     {G}//{N} Disable all warnings");
    println!("{C}  ── INFO ─────────────────────────────────────────────{N}");
    println!("  -v                     {G}//{N} Print version, patchlevel and license");
    println!("  -V[:configvar]         {G}//{N} Print configuration summary");
    println!("  -h, --help             {G}//{N} Print help");
    println!("{C}  ── TOOLCHAIN ─────────────────────────────────────────{N}");
    println!(
        "  --lsp                  {G}//{N} Language Server (JSON-RPC on stdio); must be the only arg after {bin}"
    );
    println!(
        "  build SCRIPT [-o OUT]  {G}//{N} AOT: copy this binary with SCRIPT embedded (standalone exe)"
    );
    println!("  docs [TOPIC]           {G}//{N} Built-in docs (stryke docs pmap, stryke docs |>, stryke docs)");
    println!(
        "  serve [PORT] [SCRIPT]  {G}//{N} HTTP server (stryke serve, stryke serve 8080 app.stk)"
    );
    println!("  fmt [-i] FILE...       {G}//{N} Format source files (stryke fmt -i .)");
    println!(
        "  bench [FILE|DIR]       {G}//{N} Run benchmarks from bench/ or benches/ (stryke bench)"
    );
    println!("  init [NAME]            {G}//{N} Scaffold a new project (stryke init myapp)");
    println!(
        "  repl [--load FILE]     {G}//{N} Interactive REPL with optional pre-load (stryke repl)"
    );
    println!(
        "  --remote-worker        {G}//{N} Persistent cluster worker (stdio); only arg after {bin}"
    );
    println!(
        "  --remote-worker-v1     {G}//{N} Legacy one-shot worker (stdio); only arg after {bin}"
    );
    println!("{C}  ── PARALLEL EXTENSIONS (stryke) ─────────────────────{N}");
    println!("  -j N                   {G}//{N} Set number of parallel threads (rayon)");
    println!(
        "  pmap  {{BLOCK}} @list [, progress => EXPR] {G}//{N} Parallel map; optional stderr progress bar"
    );
    println!(
        "  pmap_chunked N {{BLOCK}} @list [, progress => EXPR] {G}//{N} Parallel map in batches of N items per thread"
    );
    println!(
        "  pcache {{BLOCK}} @list [, progress => EXPR] {G}//{N} Parallel memoize (key = stringified topic)"
    );
    println!(
        "  par_lines PATH, CODE [, progress => EXPR] {G}//{N} mmap + parallel line scan (tree-walker)"
    );
    println!(
        "  par_walk PATH, CODE [, progress => EXPR] {G}//{N} parallel recursive dir walk; topic is each path"
    );
    println!(
        "  par_sed PATTERN, REPLACEMENT, FILES... [, progress => EXPR] {G}//{N} parallel in-place regex replace per file (g)"
    );
    println!(
        "  pipeline @list ->filter/map/take/collect {G}//{N} Lazy iterator (runs on collect); chain ->pmap/pgrep/pfor/pmap_chunked/psort/pcache/preduce/… like top-level p*"
    );
    println!(
        "  par_pipeline @list same chain; filter/map parallel on collect (order kept); par_pipeline(source=>…,stages=>…,workers=>…) channel stages"
    );
    println!(
        "  async {{BLOCK}}           {G}//{N} Run block on a worker thread; returns a task handle"
    );
    println!("  spawn {{BLOCK}}           {G}//{N} Same as async (Rust-style); join with await");
    println!("  await EXPR                {G}//{N} Join async task or pass through non-task value");
    println!(
        "  pgrep {{BLOCK}} @list [, progress => EXPR] {G}//{N} Parallel grep across all cores"
    );
    println!(
        "  pfor  {{BLOCK}} @list [, progress => EXPR] {G}//{N} Parallel foreach across all cores"
    );
    println!(
        "  psort {{BLOCK}} @list [, progress => EXPR] {G}//{N} Parallel sort across all cores"
    );
    println!(
        "  @list |> reduce {{BLOCK}}   {G}//{N} Sequential left fold ($a accum, $b next element); also reduce {{BLOCK}} @list"
    );
    println!(
        "  @list |> preduce {{BLOCK}} [, progress => EXPR] {G}//{N} Parallel tree fold (rayon; associative ops only); also preduce {{BLOCK}} @list"
    );
    println!(
        "  @list |> preduce_init EXPR, {{BLOCK}} [, progress => EXPR] {G}//{N} Parallel fold with identity; also preduce_init EXPR, {{BLOCK}} @list"
    );
    println!(
        "  @list |> pmap_reduce {{MAP}} {{REDUCE}} [, progress => EXPR] {G}//{N} Fused parallel map + tree reduce; also pmap_reduce {{MAP}} {{REDUCE}} @list"
    );
    println!(
        "  fan [N] {{BLOCK}} [, progress => EXPR]  {G}//{N} Execute BLOCK N times (default N = rayon pool; $_ = index); progress may follow }} without a comma"
    );
    println!(
        "  fan_cap [N] {{BLOCK}} [, progress => EXPR]  {G}//{N} Like fan; returns list of block return values (index order)"
    );
    println!("{C}  ── TYPING (stryke) ───────────────────────────────────{N}");
    println!(
        "  typed my \\$x : Int|Str|Float  {G}//{N} Optional scalar types; runtime checks on assign"
    );
    println!(
        "  fn (\\$a: Int, \\$b: Str) {{}}   {G}//{N} Typed sub params; runtime checks on call"
    );
    println!("{C}  ── SERIALIZATION (stryke) ───────────────────────────────{N}");
    println!(
        "  str \\$val / stringify \\$val  {G}//{N} Convert any value to parseable stryke literal"
    );
    println!("  eval str \\$fn              {G}//{N} Round-trip: serialize + deserialize coderefs");
    println!("{C}  ── POSITIONAL ─────────────────────────────────────────{N}");
    println!("  [programfile]          {G}//{N} Perl script to execute");
    println!("  [arguments]            {G}//{N} Arguments passed to script (@ARGV)");
    println!();
    println!();
    println!("{C}  ── SYSTEM ─────────────────────────────────────────{N}");
    println!("{M}  v{version} {N}// {Y}(c) MenkeTechnologies{N}");
    println!("{M}  There is more than one way to do it — in parallel.{N}");
    println!("{Y}  >>> PARSE. EXECUTE. PARALLELIZE. OWN YOUR CORES. <<<{N}");
    println!("{C} ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░{N}");
}

/// `-M` / `-m` prelude prepended to each program line (shared with REPL).
pub(crate) fn module_prelude(cli: &Cli) -> String {
    let mut full_code = String::new();
    for module in &cli.use_module {
        if let Some((mod_name, args)) = module.split_once('=') {
            full_code.push_str(&format!(
                "use {} qw({});\n",
                mod_name,
                args.replace(',', " ")
            ));
        } else {
            full_code.push_str(&format!("use {};\n", module));
        }
    }
    for module in &cli.use_module_no_import {
        if let Some(rest) = module.strip_prefix('-') {
            full_code.push_str(&format!("no {};\n", rest));
        } else {
            full_code.push_str(&format!("use {} ();\n", module));
        }
    }
    full_code
}

/// Like `perl`, arguments after the script (or after `-e` / `-E` code) are passed to the program
/// unchanged, including tokens that look like long options (`--regex`, …). Clap rejects unknown
/// `--flags` unless they appear after `--`; we find the Perl-consistent split and insert `--`
/// before the first script argument when needed.
fn parse_cli_prelude(args: &[String]) -> Option<Cli> {
    if args.len() <= 1 {
        return None;
    }
    // User already used `--` as the end-of-options delimiter; let clap handle it.
    if args[1..].iter().any(|s| s == "--") {
        return None;
    }
    for k in (1..=args.len()).rev() {
        let trial: Vec<String> = if k == args.len() {
            args.to_vec()
        } else {
            let mut t = args[..k].to_vec();
            t.push("--".to_string());
            t.extend(args[k..].iter().cloned());
            t
        };
        let Some(cli) = Cli::try_parse_from(&trial).ok() else {
            continue;
        };
        if cli.args.as_slice() == args[k..].as_ref() {
            return Some(cli);
        }
    }
    None
}

/// When `-e` / `-E` supplies the program, the optional positional `SCRIPT` is actually the first
/// `@ARGV` element (Perl semantics), not a second script path. Fold it into `args`.
fn normalize_argv_after_dash_e(cli: &mut Cli) {
    if (!cli.execute.is_empty() || !cli.execute_features.is_empty()) && cli.script.is_some() {
        let mut v = vec![cli.script.take().unwrap()];
        v.append(&mut cli.args);
        cli.args = v;
    }
}

/// Unique temp path next to `target` for atomic in-place replace (`rename` into place).
fn adjacent_temp_path(target: &Path) -> PathBuf {
    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let rnd: u32 = rand::thread_rng().gen();
    dir.join(format!("{name}.stryke-tmp-{rnd}"))
}

/// Write `new_content` to `path` in place; optional backup `path` + `inplace_edit` (Perl `$^I`).
fn commit_in_place_edit(path: &Path, inplace_edit: &str, new_content: &str) -> std::io::Result<()> {
    let tmp = adjacent_temp_path(path);
    std::fs::write(&tmp, new_content)?;
    if !inplace_edit.is_empty() {
        let backup = PathBuf::from(format!("{}{}", path.display(), inplace_edit));
        let _ = std::fs::remove_file(&backup);
        std::fs::rename(path, &backup)?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// `BufRead::lines()` strips the terminator; Perl’s `<>` leaves it in `$_` unless **`-l`** is set,
/// in which case Perl **chomps** each record. Match that so `print` + `$\` does not double newlines.
fn line_mode_input_record(cli: &Cli, l: String) -> String {
    if cli.line_ending.is_some() {
        l
    } else {
        format!("{}\n", l)
    }
}

/// Content of one `read_line` result, without the trailing `\n` / `\r\n` / `\r` (same string `lines()`
/// would have yielded for that physical line).
fn line_content_from_stdin_read_line(buf: &str) -> String {
    buf.strip_suffix("\r\n")
        .or_else(|| buf.strip_suffix('\n'))
        .or_else(|| buf.strip_suffix('\r'))
        .unwrap_or(buf)
        .to_string()
}

/// `-n` / `-p` input loop: `@ARGV` files when non-empty, else stdin; `-i` rewrites named files.
fn run_line_mode_loop(
    cli: &Cli,
    interp: &mut Interpreter,
    program: &Program,
    slurp: bool,
) -> Result<(), PerlError> {
    let inplace = cli.inplace.is_some();
    let use_argv_files = !interp.argv.is_empty();
    let suppressed_stdout_for_inplace = inplace && use_argv_files;
    let print_to_stdout = cli.print_mode && !suppressed_stdout_for_inplace;
    // With `-i` and named files, per-line print is suppressed; files are independent, so rayon can
    // process them in parallel (stock `perl` processes `@ARGV` files sequentially).
    let parallel_argv_inplace = inplace && use_argv_files;

    if slurp {
        if use_argv_files {
            if parallel_argv_inplace {
                let template = Mutex::new(interp.line_mode_worker_clone());
                let paths = interp.argv.clone();
                paths.into_par_iter().try_for_each(|path| {
                    let mut local = template
                        .lock()
                        .expect("line-mode template mutex poisoned")
                        .line_mode_worker_clone();
                    local.line_number = 0;
                    local.argv_current_file = path.clone();
                    let content = read_file_text_perl_compat(&path).map_err(|e| {
                        PerlError::new(
                            ErrorKind::IO,
                            format!("Can't open {}: {}", path, e),
                            0,
                            "-e",
                        )
                    })?;
                    if let Some(output) = local.process_line(&content, program, true)? {
                        commit_in_place_edit(Path::new(&path), &local.inplace_edit, &output)
                            .map_err(|e| PerlError::new(ErrorKind::IO, e.to_string(), 0, "-e"))?;
                    }
                    Ok(())
                })?;
            } else {
                for path in interp.argv.clone() {
                    interp.line_number = 0;
                    interp.argv_current_file = path.clone();
                    let content = read_file_text_perl_compat(&path).map_err(|e| {
                        PerlError::new(
                            ErrorKind::IO,
                            format!("Can't open {}: {}", path, e),
                            0,
                            "-e",
                        )
                    })?;
                    if let Some(output) = interp.process_line(&content, program, true)? {
                        if inplace {
                            commit_in_place_edit(Path::new(&path), &interp.inplace_edit, &output)
                                .map_err(|e| {
                                    PerlError::new(ErrorKind::IO, e.to_string(), 0, "-e")
                                })?;
                        } else if cli.print_mode {
                            print!("{}", output);
                            let _ = io::stdout().flush();
                        }
                    }
                }
            }
        } else {
            let mut input = String::new();
            let mut raw = Vec::new();
            let _ = IoRead::read_to_end(&mut io::stdin(), &mut raw);
            input.push_str(&decode_utf8_or_latin1(&raw));
            if let Some(output) = interp.process_line(&input, program, true)? {
                if print_to_stdout {
                    print!("{}", output);
                    let _ = io::stdout().flush();
                }
            }
        }
        return Ok(());
    }

    if use_argv_files {
        if parallel_argv_inplace {
            let template = Mutex::new(interp.line_mode_worker_clone());
            let paths = interp.argv.clone();
            paths.into_par_iter().try_for_each(|path| {
                let mut local = template
                    .lock()
                    .expect("line-mode template mutex poisoned")
                    .line_mode_worker_clone();
                local.line_number = 0;
                local.argv_current_file = path.clone();
                let file = File::open(&path).map_err(|e| {
                    PerlError::new(
                        ErrorKind::IO,
                        format!("Can't open {}: {}", path, e),
                        0,
                        "-e",
                    )
                })?;
                let mut reader = BufReader::new(file);
                let mut accumulated = String::new();
                let mut pending: Option<String> = None;
                loop {
                    let l = if let Some(s) = pending.take() {
                        s
                    } else {
                        match read_logical_line_perl_compat(&mut reader).map_err(|e| {
                            PerlError::new(
                                ErrorKind::IO,
                                format!("Error reading {}: {}", path, e),
                                0,
                                "-e",
                            )
                        })? {
                            None => break,
                            Some(s) => s,
                        }
                    };
                    let is_last = match read_logical_line_perl_compat(&mut reader).map_err(|e| {
                        PerlError::new(
                            ErrorKind::IO,
                            format!("Error reading {}: {}", path, e),
                            0,
                            "-e",
                        )
                    })? {
                        None => true,
                        Some(next) => {
                            pending = Some(next);
                            false
                        }
                    };
                    let input = line_mode_input_record(cli, l);
                    if let Some(output) = local.process_line(&input, program, is_last)? {
                        accumulated.push_str(&output);
                    }
                }
                commit_in_place_edit(Path::new(&path), &local.inplace_edit, &accumulated)
                    .map_err(|e| PerlError::new(ErrorKind::IO, e.to_string(), 0, "-e"))?;
                Ok(())
            })?;
        } else {
            for path in interp.argv.clone() {
                interp.line_number = 0;
                interp.argv_current_file = path.clone();
                let file = File::open(&path).map_err(|e| {
                    PerlError::new(
                        ErrorKind::IO,
                        format!("Can't open {}: {}", path, e),
                        0,
                        "-e",
                    )
                })?;
                let mut reader = BufReader::new(file);
                let mut accumulated = String::new();
                let mut pending: Option<String> = None;
                loop {
                    let l = if let Some(s) = pending.take() {
                        s
                    } else {
                        match read_logical_line_perl_compat(&mut reader).map_err(|e| {
                            PerlError::new(
                                ErrorKind::IO,
                                format!("Error reading {}: {}", path, e),
                                0,
                                "-e",
                            )
                        })? {
                            None => break,
                            Some(s) => s,
                        }
                    };
                    let is_last = match read_logical_line_perl_compat(&mut reader).map_err(|e| {
                        PerlError::new(
                            ErrorKind::IO,
                            format!("Error reading {}: {}", path, e),
                            0,
                            "-e",
                        )
                    })? {
                        None => true,
                        Some(next) => {
                            pending = Some(next);
                            false
                        }
                    };
                    let input = line_mode_input_record(cli, l);
                    if let Some(output) = interp.process_line(&input, program, is_last)? {
                        if print_to_stdout {
                            print!("{}", output);
                            let _ = io::stdout().flush();
                        }
                        if inplace {
                            accumulated.push_str(&output);
                        }
                    }
                }
                if inplace {
                    commit_in_place_edit(Path::new(&path), &interp.inplace_edit, &accumulated)
                        .map_err(|e| PerlError::new(ErrorKind::IO, e.to_string(), 0, "-e"))?;
                }
            }
        }
    } else {
        // Read stdin with `read_line` and **do not** hold `StdinLock` across `process_line` (the body
        // may call `<>` / `readline`, which also locks stdin — exclusive lock would deadlock).
        //
        // Peek-reading the next line to set `is_last` for `eof` would consume that line from the
        // kernel buffer; push it onto [`Interpreter::line_mode_stdin_pending`] so body `<>` reads it
        // first (Perl shares one fd between the implicit `while (<>)` and inner `readline`).
        interp.line_mode_stdin_pending.clear();
        loop {
            let mut current = String::new();
            let n = if let Some(queued) = interp.line_mode_stdin_pending.pop_front() {
                current = queued;
                current.len()
            } else {
                let mut lock = io::stdin().lock();
                read_line_perl_compat(&mut lock, &mut current).map_err(|e| {
                    PerlError::new(ErrorKind::IO, format!("Error reading stdin: {e}"), 0, "-e")
                })?
            };
            if n == 0 {
                break;
            }
            let (is_last, peek_line) = {
                let mut lock = io::stdin().lock();
                let mut peek = String::new();
                let n = read_line_perl_compat(&mut lock, &mut peek).map_err(|e| {
                    PerlError::new(ErrorKind::IO, format!("Error reading stdin: {e}"), 0, "-e")
                })?;
                if n == 0 {
                    (true, None)
                } else {
                    (false, Some(peek))
                }
            };
            if let Some(pl) = peek_line {
                interp.line_mode_stdin_pending.push_back(pl);
            }
            let l = line_content_from_stdin_read_line(&current);
            let input = line_mode_input_record(cli, l);
            match interp.process_line(&input, program, is_last) {
                Ok(Some(output)) => {
                    if print_to_stdout {
                        print!("{}", output);
                        let _ = io::stdout().flush();
                    }
                }
                Ok(None) => {}
                Err(e) => return Err(e),
            }
        }
    }
    Ok(())
}

pub(crate) fn configure_interpreter(cli: &Cli, interp: &mut Interpreter, filename: &str) {
    interp.set_file(filename);
    interp.warnings = (cli.warnings || cli.all_warnings) && !cli.no_warnings;
    interp.auto_split = cli.auto_split;
    interp.field_separator = cli.field_separator.clone();
    interp.program_name = filename.to_string();

    if let Some(ref sep) = cli.input_separator {
        match sep.as_deref() {
            None | Some("") => interp.irs = Some("\0".to_string()),
            Some("777") => interp.irs = None, // perl `-0777` enables slurp mode
            Some(oct_str) => {
                if let Ok(val) = u32::from_str_radix(oct_str, 8) {
                    if let Some(ch) = char::from_u32(val) {
                        interp.irs = Some(ch.to_string());
                    }
                }
            }
        }
    }

    if let Some(ref octnum) = cli.line_ending {
        match octnum.as_deref() {
            None | Some("") => {
                interp.ors = "\n".to_string();
            }
            Some(oct_str) => {
                if let Ok(val) = u32::from_str_radix(oct_str, 8) {
                    if let Some(ch) = char::from_u32(val) {
                        interp.ors = ch.to_string();
                    }
                }
            }
        }
    }

    if (cli.taint_check || cli.taint_warn) && cli.warnings {
        eprintln!("stryke: taint mode acknowledged but not enforced");
    }

    if let Some(ref ext_opt) = cli.inplace {
        interp.inplace_edit = ext_opt.clone().unwrap_or_default();
    }

    // Trailing arguments become `@ARGV` for `perl script.pl …` and for `perl -e '…' …` (Perl
    // compatibility).
    let mut argv: Vec<String> =
        if cli.script.is_some() || !cli.execute.is_empty() || !cli.execute_features.is_empty() {
            cli.args.clone()
        } else {
            Vec::new()
        };

    if cli.switch_parsing {
        let mut switches_done = false;
        let mut remaining = Vec::new();
        for arg in &argv {
            if switches_done || !arg.starts_with('-') || arg == "--" {
                if arg == "--" {
                    switches_done = true;
                } else {
                    remaining.push(arg.clone());
                }
            } else {
                let switch = &arg[1..];
                if let Some((name, val)) = switch.split_once('=') {
                    let _ = interp
                        .scope
                        .set_scalar(name, stryke::value::PerlValue::string(val.to_string()));
                } else {
                    let _ = interp
                        .scope
                        .set_scalar(switch, stryke::value::PerlValue::integer(1));
                }
            }
        }
        argv = remaining;
    }

    interp.argv = argv.clone();
    interp.scope.declare_array(
        "ARGV",
        argv.into_iter()
            .map(stryke::value::PerlValue::string)
            .collect(),
    );

    // Order: `-I`, in-tree `vendor/perl` (pure-Perl List::Util, …), system `perl`’s @INC, script
    // dir, `STRYKE_INC`, then `.` (deduped).
    let mut inc_paths: Vec<String> = cli.include.clone();
    let vendor = stryke::vendor_perl_inc_path();
    if vendor.is_dir() {
        stryke::perl_inc::push_unique_string_paths(
            &mut inc_paths,
            vec![vendor.to_string_lossy().into_owned()],
        );
    }
    stryke::perl_inc::push_unique_string_paths(
        &mut inc_paths,
        stryke::perl_inc::paths_from_system_perl(),
    );
    if filename != "-e" && filename != "-" && filename != "repl" {
        if let Some(parent) = std::path::Path::new(filename).parent() {
            if !parent.as_os_str().is_empty() {
                stryke::perl_inc::push_unique_string_paths(
                    &mut inc_paths,
                    vec![parent.to_string_lossy().into_owned()],
                );
            }
        }
    }
    if let Ok(extra) = std::env::var("STRYKE_INC") {
        let extra: Vec<String> = std::env::split_paths(&extra)
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        stryke::perl_inc::push_unique_string_paths(&mut inc_paths, extra);
    }
    stryke::perl_inc::push_unique_string_paths(&mut inc_paths, vec![".".to_string()]);
    let inc_dirs: Vec<stryke::value::PerlValue> = inc_paths
        .into_iter()
        .map(stryke::value::PerlValue::string)
        .collect();
    interp.scope.declare_array("INC", inc_dirs);

    if cli.debugger.is_some() {
        eprintln!("stryke: debugger not yet implemented, running normally");
    }
}

/// Emit profiler output.
///
/// `--flame` + piped stdout → SVG flamegraph to saved fd.
/// `--flame` + TTY stdout  → colored terminal bars to stderr.
/// `--profile` (no flame)  → plain text report to stderr.
fn emit_profiler_report(
    p: &mut stryke::profiler::Profiler,
    flame_out: &Option<File>,
    flame_tty: bool,
) {
    if let Some(f) = flame_out {
        // stdout was piped — write SVG to the saved fd
        let mut w = io::BufWriter::new(f);
        if let Err(e) = p.render_flame_svg(&mut w) {
            eprintln!("stryke --flame: {}", e);
        }
    } else if flame_tty {
        // stdout is a TTY — render colored bars to stderr
        p.render_flame_tty();
    } else {
        // plain --profile
        p.print_report();
    }
}

fn main() {
    // AOT: if the running binary carries an embedded script trailer, execute it and
    // exit. Bypasses clap, flags, REPL — the embedded binary behaves like a plain native
    // program: all command-line args become `@ARGV` for the embedded script. The probe
    // costs one file open + one 32-byte read (~50 µs) on the no-trailer path.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(payload) = stryke::aot::try_load_embedded(&exe) {
            let argv: Vec<String> = std::env::args().skip(1).collect();
            match payload {
                stryke::aot::EmbeddedPayload::Script(embedded) => {
                    process::exit(run_embedded_script(embedded, argv));
                }
                stryke::aot::EmbeddedPayload::Bundle(bundle) => {
                    process::exit(run_embedded_bundle(bundle, argv));
                }
            }
        }
    }

    let args = expand_perl_bundled_argv(std::env::args().collect());

    if args.len() == 2 && args[1] == "--remote-worker" {
        // Persistent v3 session loop: HELLO → SESSION_INIT → many JOBs → SHUTDOWN.
        // The basic v1 one-shot loop is still reachable via `--remote-worker-v1` for the
        // round-trip integration test.
        process::exit(stryke::remote_wire::run_remote_worker_session());
    }
    if args.len() == 2 && args[1] == "--remote-worker-v1" {
        process::exit(stryke::remote_wire::run_remote_worker_stdio());
    }

    if args.len() == 2 && args[1] == "--lsp" {
        process::exit(stryke::run_lsp_stdio());
    }

    // `stryke build SCRIPT -o OUT` subcommand: intercept before clap so `build` does not have
    // to be added to the main `Cli` struct (keeping the perl-compatible flag surface clean).
    if args.len() >= 2 && args[1] == "build" {
        process::exit(run_build_subcommand(&args[2..]));
    }

    // `stryke convert FILE...` subcommand: convert Perl source to stryke syntax with |> pipes.
    if args.len() >= 2 && args[1] == "convert" {
        process::exit(run_convert_subcommand(&args[2..]));
    }

    // `stryke deconvert FILE...` subcommand: convert stryke .stk files back to standard Perl .pl syntax.
    if args.len() >= 2 && args[1] == "deconvert" {
        process::exit(run_deconvert_subcommand(&args[2..]));
    }

    // `stryke docs [TOPIC]` subcommand: built-in documentation browser.
    if args.len() >= 2 && args[1] == "docs" {
        process::exit(run_doc_subcommand(&args[2..]));
    }

    // `stryke fmt [-i] FILE...` — format stryke source files.
    if args.len() >= 2 && args[1] == "fmt" {
        process::exit(run_fmt_subcommand(&args[2..]));
    }

    // `stryke bench [FILE|DIR]` — discover and run benchmark files.
    if args.len() >= 2 && args[1] == "bench" {
        process::exit(run_bench_subcommand(&args[0], &args[2..]));
    }

    // `stryke init [NAME]` — scaffold a new stryke project.
    if args.len() >= 2 && args[1] == "init" {
        process::exit(run_init_subcommand(&args[2..]));
    }

    // `stryke repl [--load FILE]` — explicit REPL entry.
    if args.len() >= 2 && args[1] == "repl" {
        process::exit(run_repl_subcommand(&args[2..]));
    }

    // `stryke run` — run main.stk in current directory (or specified file).
    if args.len() >= 2 && args[1] == "run" {
        let script = if args.len() >= 3 {
            args[2].clone()
        } else {
            // Search for main.stk, then src/main.stk
            if std::path::Path::new("main.stk").exists() {
                "main.stk".to_string()
            } else if std::path::Path::new("src/main.stk").exists() {
                "src/main.stk".to_string()
            } else {
                eprintln!("stryke run: no main.stk found (checked ./main.stk and ./src/main.stk)");
                process::exit(1);
            }
        };
        // Re-exec self with the script path — isolated interpreter per run.
        let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from(&args[0]));
        let mut cmd = process::Command::new(exe);
        cmd.arg(&script);
        if args.len() > 3 {
            cmd.args(&args[3..]);
        }
        let status = cmd.status().unwrap_or_else(|e| {
            eprintln!("stryke run: {}", e);
            process::exit(1);
        });
        process::exit(status.code().unwrap_or(1));
    }

    // `stryke prun FILE...` — run multiple files in parallel.
    if args.len() >= 2 && args[1] == "prun" {
        process::exit(run_prun_subcommand(&args[0], &args[2..]));
    }

    // `stryke test [FILE|DIR]` — run test files.
    if args.len() >= 2 && (args[1] == "test" || args[1] == "t") {
        let target = if args.len() >= 3 {
            args[2].clone()
        } else {
            // Search for t/ directory
            if std::path::Path::new("t").is_dir() {
                "t".to_string()
            } else if std::path::Path::new("tests").is_dir() {
                "tests".to_string()
            } else {
                eprintln!("stryke test: no t/ or tests/ directory found");
                process::exit(1);
            }
        };
        let target_path = std::path::Path::new(&target);
        let test_files: Vec<String> = if target_path.is_dir() {
            let mut files: Vec<String> = std::fs::read_dir(target_path)
                .unwrap_or_else(|e| {
                    eprintln!("stryke test: {}: {}", target, e);
                    process::exit(1);
                })
                .filter_map(|e| e.ok())
                .map(|e| e.path().to_string_lossy().to_string())
                .filter(|p| {
                    let name = std::path::Path::new(p)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    (name.starts_with("test_") || name.starts_with("t_"))
                        && (name.ends_with(".stk")
                            || name.ends_with(".st")
                            || name.ends_with(".pl"))
                })
                .collect();
            files.sort();
            files
        } else {
            vec![target]
        };
        if test_files.is_empty() {
            eprintln!("stryke test: no test files found");
            process::exit(1);
        }
        let total = test_files.len();
        let mut failed = 0;
        let mut total_pass = 0usize;
        let mut total_fail = 0usize;
        eprintln!(
            "\x1b[36mRunning {} test file{}\x1b[0m\n",
            total,
            if total == 1 { "" } else { "s" }
        );
        for f in &test_files {
            let name = std::path::Path::new(f)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| f.clone());
            eprintln!("\x1b[1m── {} ──\x1b[0m", name);
            // Resolve exe to absolute path. Try current_exe first, then
            // canonicalize args[0], then fall back to bare args[0] (PATH lookup).
            let exe = std::env::current_exe()
                .ok()
                .filter(|p| p.exists())
                .or_else(|| std::fs::canonicalize(&args[0]).ok())
                .unwrap_or_else(|| std::path::PathBuf::from(&args[0]));
            let script_abs =
                std::fs::canonicalize(f).unwrap_or_else(|_| std::path::PathBuf::from(f));
            // Project root = parent of t/ directory, so `require "./lib/..."` works.
            let project_root = script_abs
                .parent() // t/
                .and_then(|p| p.parent()) // project/
                .unwrap_or(std::path::Path::new("."));
            // Capture stderr to count assertions
            let output = process::Command::new(&exe)
                .arg(&script_abs)
                .current_dir(project_root)
                .stderr(process::Stdio::piped())
                .output();
            match output {
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    // Print the stderr (test output)
                    eprint!("{}", stderr);
                    // Count ✓ and ✗ in output (only lines starting with "  ✓" or "  ✗")
                    for line in stderr.lines() {
                        let trimmed = line.trim_start();
                        if trimmed.starts_with("\x1b[32m✓\x1b[0m") || trimmed.starts_with("✓") {
                            // Check it's not the summary "✓ All X tests" line
                            if !trimmed.contains("All ") && !trimmed.contains(" passed") {
                                total_pass += 1;
                            }
                        } else if trimmed.starts_with("\x1b[31m✗\x1b[0m")
                            || trimmed.starts_with("✗")
                        {
                            // Check it's not the summary "✗ X of Y tests failed" line
                            if !trimmed.contains(" of ") || !trimmed.contains(" failed") {
                                total_fail += 1;
                            }
                        }
                    }
                    if !out.status.success() {
                        failed += 1;
                    }
                }
                Err(e) => {
                    eprintln!("  failed to run: {}", e);
                    failed += 1;
                }
            }
            eprintln!();
        }
        let grand_total = total_pass + total_fail;
        eprintln!("═══════════════════════════════");
        if failed == 0 {
            eprintln!(
                "\x1b[32m✓ All {} test file{} passed ({} assertions)\x1b[0m",
                total,
                if total == 1 { "" } else { "s" },
                grand_total
            );
            process::exit(0);
        } else {
            eprintln!(
                "\x1b[31m✗ {} of {} test file{} failed ({} passed, {} failed)\x1b[0m",
                failed,
                total,
                if total == 1 { "" } else { "s" },
                total_pass,
                total_fail
            );
            process::exit(1);
        }
    }

    // `stryke serve PORT SCRIPT` or `stryke serve PORT -e CODE` subcommand.
    if args.len() >= 2 && args[1] == "serve" {
        process::exit(run_serve_subcommand(&args[2..]));
    }

    // `stryke check FILE...` — parse + compile without executing.
    if args.len() >= 2 && args[1] == "check" {
        process::exit(run_check_subcommand(&args[2..]));
    }

    // `stryke disasm FILE` — disassemble bytecode.
    if args.len() >= 2 && args[1] == "disasm" {
        process::exit(run_disasm_subcommand(&args[2..]));
    }

    // `stryke profile FILE` — run with profiling and output structured data.
    if args.len() >= 2 && args[1] == "profile" {
        process::exit(run_profile_subcommand(&args[0], &args[2..]));
    }

    // `stryke lsp` — start Language Server Protocol over stdio.
    if args.len() >= 2 && args[1] == "lsp" {
        process::exit(stryke::run_lsp_stdio());
    }

    // `stryke completions [SHELL]` — emit shell completions.
    if args.len() >= 2 && args[1] == "completions" {
        process::exit(run_completions_subcommand(&args[2..]));
    }

    // `stryke ast FILE` — dump AST as JSON.
    if args.len() >= 2 && args[1] == "ast" {
        process::exit(run_ast_subcommand(&args[2..]));
    }

    // Fast path: `stryke SCRIPT [ARGS...]` with no dashes anywhere — the common case, and
    // clap parsing is the dominant term on `print "hello\n"` (it knocks ~1ms off the
    // startup bench). We can't bypass clap when any flag is present, so fall through to the
    // full parser in that case.
    // Exception: `->>`, `->`, `~>` look like flags but are actually threading operators for
    // inline code — detect via `looks_like_code` and treat as script.
    let arg1_is_code_not_flag =
        args.len() >= 2 && args[1].starts_with('-') && looks_like_code(&args[1]);
    let mut cli = if args.len() >= 2
        && (!args[1].starts_with('-') || arg1_is_code_not_flag)
        && !args[1].is_empty()
        && args[2..].iter().all(|a| !a.starts_with('-'))
    {
        Cli {
            script: Some(args[1].clone()),
            args: if args.len() > 2 {
                args[2..].to_vec()
            } else {
                Vec::new()
            },
            ..Default::default()
        }
    } else {
        parse_cli_prelude(&args).unwrap_or_else(|| Cli::parse_from(&args))
    };
    normalize_argv_after_dash_e(&mut cli);

    // Set global compat-mode flag before any parsing happens.
    if cli.compat {
        stryke::set_compat_mode(true);
    }

    if cli.help {
        print_cyberpunk_help();
        return;
    }

    if cli.show_version {
        println!(
            "This is stryke v{} — A highly parallel Perl 5 interpreter (Rust)\n",
            env!("CARGO_PKG_VERSION")
        );
        println!("Built with rayon for parallel map/grep/for/sort");
        println!(
            "Threads available: {}\n",
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        );
        println!(
            "Copyright 2026 MenkeTechnologies. Licensed under MIT.\n\n\
             This is free software; you can redistribute it and/or modify it\n\
             under the terms of the MIT License."
        );
        return;
    }

    if let Some(ref configvar) = cli.show_config {
        print_config(configvar.as_deref());
        return;
    }

    if let Some(code) = &cli.explain {
        match stryke::error::explain_error(code) {
            Some(text) => println!("{}", text),
            None => {
                eprintln!("stryke: unknown explain code {:?}", code);
                process::exit(1);
            }
        }
        return;
    }

    // Configure rayon thread pool
    if let Some(n) = cli.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global()
            .ok();
    }

    // Multi-file execution: `st *.stk` runs all files.
    // Use `-j N` for parallel execution: `st -j4 *.stk`
    // Only triggers when ALL args are existing script files on disk.
    // `st file.stk ARG1 ARG2` still works because ARG1/ARG2 won't exist as files.
    if let Some(script) = cli.script.as_ref() {
        if cli.execute.is_empty()
            && cli.execute_features.is_empty()
            && !cli.line_mode
            && !cli.print_mode
            && !cli.args.is_empty()
        {
            let is_stk_ext = |p: &str| {
                p.ends_with(".stk") || p.ends_with(".pl") || p.ends_with(".pm") || p.ends_with(".t")
            };
            let is_existing_script = |p: &str| is_stk_ext(p) && Path::new(p).is_file();

            if is_existing_script(script) && cli.args.iter().all(|a| is_existing_script(a)) {
                let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from(&args[0]));
                let mut all_files = vec![script.clone()];
                all_files.extend(cli.args.iter().cloned());

                // Parallel execution when -j is specified
                let failed = if cli.threads.is_some() {
                    use std::sync::atomic::{AtomicUsize, Ordering};
                    let failed = AtomicUsize::new(0);
                    all_files.par_iter().for_each(|f| {
                        let status = process::Command::new(&exe).arg(f).status();
                        match status {
                            Ok(s) if !s.success() => {
                                failed.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(e) => {
                                eprintln!("{}: {}", f, e);
                                failed.fetch_add(1, Ordering::Relaxed);
                            }
                            _ => {}
                        }
                    });
                    failed.load(Ordering::Relaxed)
                } else {
                    // Sequential execution (default)
                    let mut failed = 0usize;
                    for f in &all_files {
                        let status = process::Command::new(&exe).arg(f).status();
                        match status {
                            Ok(s) if !s.success() => failed += 1,
                            Err(e) => {
                                eprintln!("{}: {}", f, e);
                                failed += 1;
                            }
                            _ => {}
                        }
                    }
                    failed
                };
                process::exit(if failed > 0 { 1 } else { 0 });
            }
        }
    }

    // Check both compile-time binary name and runtime invocation name for REPL trigger.
    // This handles symlinks like `s` -> `stryke` and `st` -> `stryke`.
    let runtime_bin_name = args
        .first()
        .and_then(|p| Path::new(p).file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let is_stryke_bin = matches!(env!("CARGO_BIN_NAME"), "stryke" | "st" | "s")
        || matches!(runtime_bin_name.as_str(), "stryke" | "st" | "s");
    let is_repl = is_stryke_bin
        && cli.script.is_none()
        && cli.execute.is_empty()
        && cli.execute_features.is_empty()
        && !cli.line_mode
        && !cli.print_mode
        && !cli.check_only
        && !cli.lint
        && !cli.disasm
        && !cli.dump_ast
        && !cli.format_source
        && !cli.profile
        && !cli.flame
        && !cli.dump_core
        && cli.explain.is_none()
        && io::stdin().is_terminal();

    if is_repl {
        repl::run(&cli);
        return;
    }

    // Determine slurp mode
    let slurp = cli.slurp
        || cli
            .input_separator
            .as_ref()
            .is_some_and(|v| v.as_deref() == Some("777"));

    // Build the source code (`__DATA__` is split out before shebang / `-x` handling)
    let (raw_script, filename): (String, String) = if !cli.execute.is_empty() {
        (cli.execute.join("; "), "-e".to_string())
    } else if !cli.execute_features.is_empty() {
        (cli.execute_features.join("; "), "-E".to_string())
    } else if let Some(ref script) = cli.script {
        if script == "-" {
            // Like `perl -`: program text from stdin (not a file named `-` in cwd).
            let mut code = Vec::new();
            let _ = IoRead::read_to_end(&mut io::stdin(), &mut code);
            let code = decode_utf8_or_latin1(&code);
            (code, "-".to_string())
        } else {
            let script_path = if cli.path_lookup {
                find_in_path(script).unwrap_or_else(|| script.clone())
            } else {
                script.clone()
            };
            match read_file_text_perl_compat(&script_path) {
                Ok(content) => (content, script_path),
                Err(_) if !cli.force_script && looks_like_code(&script_path) => {
                    // One-liner-first: `stryke 'p 1+2'` works without `-e`
                    (script_path, "-e".to_string())
                }
                Err(e) => {
                    eprintln!("Can't open perl script \"{}\": {}", script_path, e);
                    process::exit(2);
                }
            }
        }
    } else if cli.line_mode || cli.print_mode {
        (String::new(), "-".to_string())
    } else {
        let mut code = Vec::new();
        // Match `perl`: program from stdin is the full script (pipe, heredoc, or terminal until EOF).
        let _ = IoRead::read_to_end(&mut io::stdin(), &mut code);
        let code = decode_utf8_or_latin1(&code);
        (code, "-".to_string())
    };

    let (program_text, data_opt) = stryke::data_section::split_data_section(&raw_script);
    let code = strip_shebang_and_extract(&program_text, cli.extract.is_some());

    let mut full_code = module_prelude(&cli);
    full_code.push_str(&code);

    // `.pec` bytecode cache fast path — skip parse AND compile on warm starts.
    //
    // Keyed on (crate version, filename, full source including `-M` prelude). Enabled by
    // `STRYKE_BC_CACHE=1` (opt-in for v1 — see [`stryke::pec::cache_enabled`]). On a hit,
    // the [`stryke::pec::PecBundle`] carries both the AST `Program` and the compiled
    // `Chunk`; we hand the chunk to the interpreter via a sideband field that
    // [`stryke::try_vm_execute`] consumes. On a miss, we parse normally and stash the
    // fingerprint so the try-VM path persists the freshly-compiled chunk after run.
    //
    // **Disabled for `-e` / `-E` one-liners.** Measured: warm `.pec` is ~2-3× *slower* than
    // cold for tiny scripts because the deserialize cost (~1-2 ms for fs read + zstd decode
    // + bincode) dominates the parse+compile work it replaces (~500 µs). One-liners would
    // also pollute the cache directory with one entry per unique `-e` invocation, with no
    // GC in v1. The break-even is around 1000+ lines, so file-based scripts only.
    let is_one_liner = !cli.execute.is_empty() || !cli.execute_features.is_empty();
    let pec_on = stryke::pec::cache_enabled()
        && !cli.line_mode
        && !cli.print_mode
        && !cli.lint
        && !cli.check_only
        && !cli.dump_ast
        && !cli.format_source
        && !cli.profile
        && !cli.flame
        && !is_one_liner
        && !filename.is_empty();
    let pec_fp_opt: Option<[u8; 32]> = if pec_on {
        // `strict_vars` enters the fingerprint as `false` here; an eventual [`PecBundle::strict_vars`]
        // mismatch at load time is treated as a miss (see [`stryke::pec::try_load`]), so two strict
        // modes may collide in one slot without producing wrong answers.
        Some(stryke::pec::source_fingerprint(
            false, &filename, &full_code,
        ))
    } else {
        None
    };
    let cached_bundle = pec_fp_opt
        .as_ref()
        .and_then(|fp| stryke::pec::try_load(fp, false).ok().flatten());

    let (program, pec_precompiled) = if let Some(bundle) = cached_bundle {
        (bundle.program, Some(bundle.chunk))
    } else {
        let parsed = match stryke::parse_with_file(&full_code, &filename) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{}", e);
                process::exit(255);
            }
        };
        (parsed, None)
    };

    if cli.dump_ast {
        match serde_json::to_string_pretty(&program) {
            Ok(json) => println!("{}", json),
            Err(e) => {
                eprintln!("stryke: failed to serialize AST to JSON: {}", e);
                process::exit(1);
            }
        }
        return;
    }

    if cli.format_source {
        // Use convert_program for clean stryke (.stk) syntax with pipes
        println!("{}", stryke::convert::convert_program(&program));
        return;
    }

    if cli.lint {
        let mut interp = Interpreter::new();
        if cli.no_jit {
            interp.vm_jit_enabled = false;
        }
        configure_interpreter(&cli, &mut interp, &filename);
        if let Some(data) = data_opt {
            interp.install_data_handle(data);
        }
        match stryke::lint_program(&program, &mut interp) {
            Ok(()) => {
                eprintln!("{} compile OK", filename);
                return;
            }
            Err(e) => {
                eprintln!("{}", e);
                process::exit(255);
            }
        }
    }

    if cli.check_only {
        eprintln!("{} syntax OK", filename);
        return;
    }

    if cli.dump_core {
        eprintln!("{} syntax OK (dump not supported)", filename);
        return;
    }

    let mut interp = Interpreter::new();
    if cli.no_jit {
        interp.vm_jit_enabled = false;
    }
    if cli.disasm {
        interp.disasm_bytecode = true;
    }
    if cli.profile || cli.flame {
        interp.profiler = Some(stryke::profiler::Profiler::new(filename.clone()));
    }
    // Hand the `.pec` sideband to the interpreter so `try_vm_execute` either runs the
    // pre-compiled chunk (cache hit) or saves the freshly-compiled one (cache miss).
    interp.pec_precompiled_chunk = pec_precompiled;
    interp.pec_cache_fingerprint = if pec_on { pec_fp_opt } else { None };
    configure_interpreter(&cli, &mut interp, &filename);
    if let Some(data) = data_opt {
        interp.install_data_handle(data);
    }

    // --flame: when stdout is piped to a file, save real stdout for the SVG and redirect
    // script output to stderr so `stryke --flame x.stk > flame.svg` captures a clean SVG.
    // When stdout is a TTY, skip the redirect — we'll render colored bars to stderr instead.
    let flame_is_tty = cli.flame && io::stdout().is_terminal();
    #[cfg(unix)]
    let flame_stdout: Option<File> = if cli.flame && !flame_is_tty {
        use std::os::unix::io::FromRawFd;
        let saved = unsafe { libc::dup(1) };
        if saved >= 0 {
            unsafe { libc::dup2(2, 1) };
            Some(unsafe { File::from_raw_fd(saved) })
        } else {
            None
        }
    } else {
        None
    };
    #[cfg(not(unix))]
    let flame_stdout: Option<File> = None;

    // Line processing mode (-n / -p)
    if cli.line_mode || cli.print_mode {
        if cli.line_ending.is_some() {
            interp.ors = "\n".to_string();
        }

        // Prelude only: subs / `use` / BEGIN … INIT — main runs per line in `process_line`, not here
        // (stock `perl` wraps `-e` in `while (<>) { … }`, so a bare `print` must not run before input).
        interp.line_mode_skip_main = true;
        if let Err(e) = interp.execute(&program) {
            interp.line_mode_skip_main = false;
            if let Some(mut p) = interp.profiler.take() {
                emit_profiler_report(&mut p, &flame_stdout, flame_is_tty);
            }
            if let ErrorKind::Exit(code) = e.kind {
                process::exit(code);
            }
            eprintln!("{}", e);
            process::exit(255);
        }
        interp.line_mode_skip_main = false;

        if let Err(e) = run_line_mode_loop(&cli, &mut interp, &program, slurp) {
            if let Some(mut p) = interp.profiler.take() {
                emit_profiler_report(&mut p, &flame_stdout, flame_is_tty);
            }
            if let ErrorKind::Exit(code) = e.kind {
                process::exit(code);
            }
            eprintln!("{}", e);
            process::exit(255);
        }
        if let Err(e) = interp.run_end_blocks() {
            if let Some(mut p) = interp.profiler.take() {
                emit_profiler_report(&mut p, &flame_stdout, flame_is_tty);
            }
            if let ErrorKind::Exit(code) = e.kind {
                process::exit(code);
            }
            eprintln!("{}", e);
            process::exit(255);
        }
        let _ = interp.run_global_teardown();
        if let Some(mut p) = interp.profiler.take() {
            emit_profiler_report(&mut p, &flame_stdout, flame_is_tty);
        }
    } else {
        // Normal execution
        match interp.execute(&program) {
            Ok(_) => {
                let _ = interp.run_global_teardown();
                let _ = io::stdout().flush();
                if let Some(mut p) = interp.profiler.take() {
                    emit_profiler_report(&mut p, &flame_stdout, flame_is_tty);
                }
            }
            Err(e) => match e.kind {
                ErrorKind::Exit(code) => {
                    if let Some(mut p) = interp.profiler.take() {
                        emit_profiler_report(&mut p, &flame_stdout, flame_is_tty);
                    }
                    process::exit(code);
                }
                ErrorKind::Die => {
                    if let Some(mut p) = interp.profiler.take() {
                        emit_profiler_report(&mut p, &flame_stdout, flame_is_tty);
                    }
                    eprint!("{}", e);
                    process::exit(255);
                }
                _ => {
                    if let Some(mut p) = interp.profiler.take() {
                        emit_profiler_report(&mut p, &flame_stdout, flame_is_tty);
                    }
                    eprintln!("{}", e);
                    process::exit(255);
                }
            },
        }
    }
}

/// Run an [`stryke::aot::EmbeddedScript`] as if it were the primary program. Minimal
/// `@INC` setup: current directory only — the AOT binary is meant to be self-contained, so
/// the target machine's `perl` (which may not exist) is not consulted. `-I` at build time
/// is not yet supported (v1); drop everything into the `rust { ... }` block instead.
fn run_embedded_script(embedded: stryke::aot::EmbeddedScript, argv: Vec<String>) -> i32 {
    // AOT binaries pick up the `.pec` bytecode cache for free when `STRYKE_BC_CACHE=1` —
    // the first run of a shipped binary parses and compiles the embedded source, then
    // every subsequent run reuses the cached chunk. Cache key includes the script name
    // embedded in the trailer, so two binaries with different embedded scripts will not
    // collide.
    let pec_on = stryke::pec::cache_enabled();
    let pec_fp = if pec_on {
        Some(stryke::pec::source_fingerprint(
            false,
            &embedded.name,
            &embedded.source,
        ))
    } else {
        None
    };
    let cached = pec_fp
        .as_ref()
        .and_then(|fp| stryke::pec::try_load(fp, false).ok().flatten());
    let (program, pec_precompiled) = if let Some(bundle) = cached {
        (bundle.program, Some(bundle.chunk))
    } else {
        let parsed = match stryke::parse_with_file(&embedded.source, &embedded.name) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{}", e);
                return 255;
            }
        };
        (parsed, None)
    };
    let mut interp = Interpreter::new();
    interp.set_file(&embedded.name);
    interp.program_name = embedded.name.clone();
    interp.argv = argv.clone();
    interp.scope.declare_array(
        "ARGV",
        argv.into_iter()
            .map(stryke::value::PerlValue::string)
            .collect(),
    );
    interp.scope.declare_array(
        "INC",
        vec![stryke::value::PerlValue::string(".".to_string())],
    );
    interp.pec_precompiled_chunk = pec_precompiled;
    interp.pec_cache_fingerprint = pec_fp;
    match interp.execute(&program) {
        Ok(_) => {
            let _ = interp.run_global_teardown();
            let _ = io::stdout().flush();
            0
        }
        Err(e) => match e.kind {
            ErrorKind::Exit(code) => code,
            ErrorKind::Die => {
                eprint!("{}", e);
                255
            }
            _ => {
                eprintln!("{}", e);
                255
            }
        },
    }
}

/// Run an embedded bundle (v2 AOT) — registers all bundled files as virtual modules,
/// then executes the entry point.
fn run_embedded_bundle(bundle: stryke::aot::EmbeddedBundle, argv: Vec<String>) -> i32 {
    let entry_source = match bundle.files.get(&bundle.entry) {
        Some(s) => s.clone(),
        None => {
            eprintln!("stryke: bundle missing entry point: {}", bundle.entry);
            return 255;
        }
    };

    let program = match stryke::parse_with_file(&entry_source, &bundle.entry) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e);
            return 255;
        }
    };

    let mut interp = Interpreter::new();
    interp.set_file(&bundle.entry);
    interp.program_name = bundle.entry.clone();
    interp.argv = argv.clone();
    interp.scope.declare_array(
        "ARGV",
        argv.into_iter()
            .map(stryke::value::PerlValue::string)
            .collect(),
    );
    interp.scope.declare_array(
        "INC",
        vec![stryke::value::PerlValue::string(".".to_string())],
    );

    for (path, source) in &bundle.files {
        interp.register_virtual_module(path.clone(), source.clone());
    }

    match interp.execute(&program) {
        Ok(_) => {
            let _ = interp.run_global_teardown();
            let _ = io::stdout().flush();
            0
        }
        Err(e) => match e.kind {
            ErrorKind::Exit(code) => code,
            ErrorKind::Die => {
                eprint!("{}", e);
                255
            }
            _ => {
                eprintln!("{}", e);
                255
            }
        },
    }
}

/// `stryke check FILE...` — parse + compile without executing.
/// Reports errors with file:line:col format suitable for CI and editor integration.
fn run_check_subcommand(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("usage: stryke check FILE...");
        eprintln!();
        eprintln!("Parse and compile stryke/perl files without executing.");
        eprintln!("Reports warnings and errors with file:line:col format.");
        eprintln!();
        eprintln!("Options:");
        eprintln!("  -q, --quiet    Only output errors, no success messages");
        eprintln!("  --json         Output diagnostics as JSON (one object per line)");
        return 0;
    }

    let mut files: Vec<String> = Vec::new();
    let mut quiet = false;
    let mut json_output = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-q" | "--quiet" => quiet = true,
            "--json" => json_output = true,
            "-h" | "--help" => {
                eprintln!("usage: stryke check FILE...");
                return 0;
            }
            s if !s.starts_with('-') => files.push(s.to_string()),
            other => {
                eprintln!("stryke check: unknown option: {}", other);
                return 2;
            }
        }
        i += 1;
    }

    if files.is_empty() {
        eprintln!("stryke check: no files specified");
        return 2;
    }

    let mut errors = 0;
    for file in &files {
        let source = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                if json_output {
                    println!(
                        r#"{{"file":"{}","line":0,"col":0,"severity":"error","message":"{}"}}"#,
                        file,
                        e.to_string().replace('"', "\\\"")
                    );
                } else {
                    eprintln!("{}:0:0: error: {}", file, e);
                }
                errors += 1;
                continue;
            }
        };

        let program = match stryke::parse_with_file(&source, file) {
            Ok(p) => p,
            Err(e) => {
                if json_output {
                    println!(
                        r#"{{"file":"{}","line":{},"col":0,"severity":"error","message":"{}"}}"#,
                        file,
                        e.line,
                        e.to_string().replace('"', "\\\"").replace('\n', "\\n")
                    );
                } else {
                    eprintln!("{}:{}:0: error: {}", file, e.line, e);
                }
                errors += 1;
                continue;
            }
        };

        let mut interp = Interpreter::new();
        interp.set_file(file);
        match stryke::lint_program(&program, &mut interp) {
            Ok(()) => {
                if !quiet && !json_output {
                    eprintln!("{}: OK", file);
                }
            }
            Err(e) => {
                if json_output {
                    println!(
                        r#"{{"file":"{}","line":{},"col":0,"severity":"error","message":"{}"}}"#,
                        file,
                        e.line,
                        e.to_string().replace('"', "\\\"").replace('\n', "\\n")
                    );
                } else {
                    eprintln!("{}:{}:0: error: {}", file, e.line, e);
                }
                errors += 1;
            }
        }
    }

    if errors > 0 {
        if !quiet && !json_output {
            eprintln!();
            eprintln!(
                "{} error{} in {} file{}",
                errors,
                if errors == 1 { "" } else { "s" },
                files.len(),
                if files.len() == 1 { "" } else { "s" }
            );
        }
        1
    } else {
        if !quiet && !json_output && files.len() > 1 {
            eprintln!();
            eprintln!("All {} files OK", files.len());
        }
        0
    }
}

/// `stryke disasm FILE` — disassemble bytecode.
fn run_disasm_subcommand(args: &[String]) -> i32 {
    let mut file: Option<String> = None;
    let mut show_jit = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--jit" => show_jit = true,
            "-h" | "--help" => {
                println!("usage: stryke disasm [--jit] FILE");
                println!();
                println!("Disassemble stryke bytecode for a file.");
                println!();
                println!("Options:");
                println!("  --jit    Also show Cranelift IR (when JIT is enabled)");
                return 0;
            }
            s if !s.starts_with('-') && file.is_none() => file = Some(s.to_string()),
            other => {
                eprintln!("stryke disasm: unknown option: {}", other);
                return 2;
            }
        }
        i += 1;
    }

    let Some(file) = file else {
        eprintln!("usage: stryke disasm [--jit] FILE");
        return 2;
    };

    let source = match std::fs::read_to_string(&file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("stryke disasm: {}: {}", file, e);
            return 1;
        }
    };

    let program = match stryke::parse_with_file(&source, &file) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };

    let mut interp = Interpreter::new();
    interp.set_file(&file);
    if let Err(e) = stryke::lint_program(&program, &mut interp) {
        eprintln!("{}", e);
        return 1;
    }

    let comp = stryke::compiler::Compiler::new().with_source_file(file.clone());
    let chunk = match comp.compile_program(&program) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("compile error: {:?}", e);
            return 1;
        }
    };

    println!("=== Bytecode for {} ===", file);
    println!("{}", chunk.disassemble());

    if show_jit {
        println!();
        println!("=== Cranelift IR ===");
        println!("(JIT IR dump not yet implemented)");
    }

    0
}

/// `stryke prun FILE...` — run multiple files in parallel.
fn run_prun_subcommand(exe_arg: &str, args: &[String]) -> i32 {
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        println!("usage: stryke prun FILE...");
        println!();
        println!("Run multiple stryke files in parallel using all available cores.");
        println!();
        println!("Examples:");
        println!("  stryke prun *.stk              # run all .stk files in parallel");
        println!("  stryke prun a.stk b.stk c.stk  # run specific files in parallel");
        println!();
        println!("For sequential execution, use: stryke *.stk");
        println!("For parallel with thread limit: stryke -j4 *.stk");
        return if args.is_empty() { 2 } else { 0 };
    }

    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from(exe_arg));
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();

    if files.is_empty() {
        eprintln!("stryke prun: no files specified");
        return 2;
    }

    use std::sync::atomic::{AtomicUsize, Ordering};
    let failed = AtomicUsize::new(0);
    let total = files.len();

    eprintln!(
        "\x1b[36mRunning {} file{} in parallel\x1b[0m",
        total,
        if total == 1 { "" } else { "s" }
    );

    files.par_iter().for_each(|f| {
        let status = process::Command::new(&exe)
            .arg(f)
            .stdout(process::Stdio::inherit())
            .stderr(process::Stdio::inherit())
            .status();
        match status {
            Ok(s) if !s.success() => {
                failed.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                eprintln!("{}: {}", f, e);
                failed.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    });

    let failed_count = failed.load(Ordering::Relaxed);
    if failed_count > 0 {
        eprintln!("\x1b[31m✗ {} of {} failed\x1b[0m", failed_count, total);
        1
    } else {
        eprintln!("\x1b[32m✓ All {} completed\x1b[0m", total);
        0
    }
}

/// `stryke profile FILE` — run with profiling and output structured data.
fn run_profile_subcommand(exe_arg: &str, args: &[String]) -> i32 {
    let mut file: Option<String> = None;
    let mut output: Option<String> = None;
    let mut flame = false;
    let mut json = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("stryke profile: -o requires an argument");
                    return 2;
                }
                output = Some(args[i].clone());
            }
            "--flame" => flame = true,
            "--json" => json = true,
            "-h" | "--help" => {
                println!("usage: stryke profile [OPTIONS] FILE");
                println!();
                println!("Run a file with profiling enabled and output structured data.");
                println!();
                println!("Options:");
                println!("  -o, --output FILE   Write output to FILE instead of stdout/stderr");
                println!("  --flame             Generate flamegraph SVG");
                println!("  --json              Output profile data as JSON");
                return 0;
            }
            s if !s.starts_with('-') && file.is_none() => file = Some(s.to_string()),
            other => {
                eprintln!("stryke profile: unknown option: {}", other);
                return 2;
            }
        }
        i += 1;
    }

    let Some(file) = file else {
        eprintln!("usage: stryke profile [OPTIONS] FILE");
        return 2;
    };

    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from(exe_arg));
    let mut cmd = process::Command::new(exe);

    if flame {
        cmd.arg("--flame");
    } else {
        cmd.arg("--profile");
    }
    cmd.arg(&file);

    if let Some(ref out) = output {
        if flame {
            let out_file = match std::fs::File::create(out) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("stryke profile: cannot create {}: {}", out, e);
                    return 1;
                }
            };
            cmd.stdout(out_file);
        }
    }

    let status = match cmd.status() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("stryke profile: {}", e);
            return 1;
        }
    };

    if json && !flame {
        eprintln!("(--json profile output not yet implemented; use --flame -o file.svg for structured output)");
    }

    status.code().unwrap_or(1)
}

/// `stryke completions [SHELL]` — emit shell completions.
fn run_completions_subcommand(args: &[String]) -> i32 {
    let shell = args.first().map(|s| s.as_str()).unwrap_or("");
    match shell {
        "zsh" | "" => {
            let completions = include_str!("../completions/_stryke");
            println!("{}", completions);
            0
        }
        "-h" | "--help" => {
            println!("usage: stryke completions [SHELL]");
            println!();
            println!("Emit shell completions to stdout.");
            println!();
            println!("Supported shells:");
            println!("  zsh   (default)");
            println!();
            println!("Examples:");
            println!("  stryke completions zsh > /usr/local/share/zsh/site-functions/_stryke");
            println!("  stryke completions >> ~/.zshrc");
            0
        }
        other => {
            eprintln!("stryke completions: unsupported shell: {}", other);
            eprintln!("Supported: zsh");
            2
        }
    }
}

/// `stryke ast FILE` — dump AST as JSON.
fn run_ast_subcommand(args: &[String]) -> i32 {
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        println!("usage: stryke ast FILE");
        println!();
        println!("Parse a file and dump the AST as JSON to stdout.");
        return if args.is_empty() { 2 } else { 0 };
    }

    let file = &args[0];
    let source = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}: {}", file, e);
            return 1;
        }
    };

    let program = match stryke::parse_with_file(&source, file) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };

    match serde_json::to_string_pretty(&program) {
        Ok(json) => {
            println!("{}", json);
            0
        }
        Err(e) => {
            eprintln!("stryke ast: failed to serialize: {}", e);
            1
        }
    }
}

/// `stryke build SCRIPT [-o OUT]` or `stryke build --project DIR [-o OUT]`
/// Compile a Perl script (or project with lib/) into a standalone binary.
fn run_build_subcommand(args: &[String]) -> i32 {
    let mut script: Option<String> = None;
    let mut project_dir: Option<String> = None;
    let mut out: Option<String> = None;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("stryke build: -o requires an argument");
                    return 2;
                }
                out = Some(args[i].clone());
            }
            "--project" | "-p" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("stryke build: --project requires a directory argument");
                    return 2;
                }
                project_dir = Some(args[i].clone());
            }
            "-h" | "--help" => {
                println!("usage: stryke build SCRIPT [-o OUTPUT]");
                println!("       stryke build --project DIR [-o OUTPUT]");
                println!();
                println!(
                    "Compile a Perl script into a standalone executable binary. The output is"
                );
                println!(
                    "a copy of this stryke binary with the script source embedded as a compressed"
                );
                println!(
                    "trailer. `scp` the result to any compatible machine and run it directly —"
                );
                println!("no perl, no stryke, no @INC setup required.");
                println!();
                println!("Options:");
                println!("  --project DIR   Bundle main.stk + lib/*.stk (excludes t/ tests)");
                println!();
                println!("Examples:");
                println!("  stryke build app.pl                     # → ./app");
                println!("  stryke build app.pl -o /usr/local/bin/app");
                println!("  stryke build --project ./myapp -o myapp # bundle project");
                return 0;
            }
            s if script.is_none() && project_dir.is_none() && !s.starts_with('-') => {
                script = Some(s.to_string())
            }
            other => {
                eprintln!("stryke build: unknown argument: {}", other);
                eprintln!("usage: stryke build SCRIPT [-o OUTPUT]");
                eprintln!("       stryke build --project DIR [-o OUTPUT]");
                return 2;
            }
        }
        i += 1;
    }

    if let Some(dir) = project_dir {
        let project_path = PathBuf::from(&dir);
        let out_path = PathBuf::from(out.unwrap_or_else(|| {
            project_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "a.out".to_string())
        }));
        match stryke::aot::build_project(&project_path, &out_path) {
            Ok(p) => {
                eprintln!("stryke build: wrote {}", p.display());
                0
            }
            Err(e) => {
                eprintln!("{}", e);
                1
            }
        }
    } else {
        let Some(script) = script else {
            eprintln!("stryke build: missing SCRIPT or --project DIR");
            eprintln!("usage: stryke build SCRIPT [-o OUTPUT]");
            eprintln!("       stryke build --project DIR [-o OUTPUT]");
            return 2;
        };
        let script_path = PathBuf::from(&script);
        let out_path = PathBuf::from(out.unwrap_or_else(|| {
            script_path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "a.out".to_string())
        }));
        match stryke::aot::build(&script_path, &out_path) {
            Ok(p) => {
                eprintln!("stryke build: wrote {}", p.display());
                0
            }
            Err(e) => {
                eprintln!("{}", e);
                1
            }
        }
    }
}

/// `stryke convert FILE...` — convert Perl source to idiomatic stryke syntax.
fn run_convert_subcommand(args: &[String]) -> i32 {
    let mut files: Vec<String> = Vec::new();
    let mut in_place = false;
    let mut output_delim: Option<char> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-i" | "--in-place" => in_place = true,
            "-d" | "--output-delim" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("stryke convert: --output-delim requires an argument");
                    return 2;
                }
                let delim_str = &args[i];
                if delim_str.chars().count() != 1 {
                    eprintln!(
                        "stryke convert: --output-delim must be a single character, got {:?}",
                        delim_str
                    );
                    return 2;
                }
                output_delim = delim_str.chars().next();
            }
            "-h" | "--help" => {
                println!("usage: stryke convert [-i] [-d DELIM] FILE...");
                println!();
                println!("Convert standard Perl source to idiomatic stryke syntax:");
                println!("  - Nested calls → |> pipe-forward chains");
                println!("  - map/grep/sort/join LIST → LIST |> map/grep/sort/join");
                println!("  - No trailing semicolons");
                println!("  - 4-space indentation");
                println!("  - #!/usr/bin/env stryke shebang");
                println!();
                println!("Options:");
                println!("  -i, --in-place       Write .stk files alongside originals");
                println!("  -d, --output-delim   Delimiter for s///, tr///, m// (default: preserve original)");
                println!();
                println!("Examples:");
                println!("  stryke convert app.pl              # print to stdout");
                println!("  stryke convert -i lib/*.pm         # write lib/*.stk");
                println!("  stryke convert -d '|' app.pl       # use | as delimiter: s|old|new|g");
                return 0;
            }
            s if s.starts_with('-') => {
                eprintln!("stryke convert: unknown option: {}", s);
                eprintln!("usage: stryke convert [-i] [-d DELIM] FILE...");
                return 2;
            }
            s => files.push(s.to_string()),
        }
        i += 1;
    }
    if files.is_empty() {
        eprintln!("stryke convert: no input files");
        eprintln!("usage: stryke convert [-i] [-d DELIM] FILE...");
        return 2;
    }
    let opts = stryke::convert::ConvertOptions { output_delim };
    let mut errors = 0;
    for f in &files {
        let code = match std::fs::read_to_string(f) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("stryke convert: {}: {}", f, e);
                errors += 1;
                continue;
            }
        };
        let program = match stryke::parse_with_file(&code, f) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("stryke convert: {}: {}", f, e);
                errors += 1;
                continue;
            }
        };
        let converted = stryke::convert_to_stryke_with_options(&program, &opts);
        if in_place {
            let out_path = std::path::Path::new(f).with_extension("pr");
            if let Err(e) = std::fs::write(&out_path, &converted) {
                eprintln!("stryke convert: {}: {}", out_path.display(), e);
                errors += 1;
            }
        } else {
            println!("{}", converted);
        }
    }
    if errors > 0 {
        1
    } else {
        0
    }
}

/// `stryke serve [PORT] [SCRIPT]` or `stryke serve [PORT] -e CODE` — start an HTTP server (default port 8000).
///
/// Wraps the user's handler in `serve(PORT, fn ($req) { ... })`.
fn run_serve_subcommand(args: &[String]) -> i32 {
    if !args.is_empty() && (args[0] == "-h" || args[0] == "--help") {
        eprintln!("usage: stryke serve [PORT] [SCRIPT | -e CODE]");
        eprintln!();
        eprintln!("  stryke serve                   serve $PWD on port 8000");
        eprintln!("  stryke serve PORT              serve $PWD as static files");
        eprintln!("  stryke serve PORT SCRIPT       run script (must call serve())");
        eprintln!("  stryke serve PORT -e CODE      one-liner handler");
        eprintln!();
        eprintln!("  Handler receives $req (hashref: method, path, query, headers, body, peer)");
        eprintln!("  and returns: string (200 OK), key-value pairs, hashref, or undef (404).");
        eprintln!();
        eprintln!("examples:");
        eprintln!(
            "  stryke serve                                              # static file server on 8000"
        );
        eprintln!(
            "  stryke serve 8080                                         # static file server"
        );
        eprintln!("  stryke serve 8080 app.stk                                 # script handler");
        eprintln!("  stryke serve 3000 -e '\"hello \" . $req->{{path}}'           # one-liner");
        eprintln!("  stryke serve 8080 -e 'status => 200, body => json_encode(+{{ok => 1}})'");
        return 0;
    }

    // If first arg is a valid port number, consume it; otherwise default to 8000.
    let (port, rest) = if !args.is_empty() && args[0].parse::<u16>().is_ok() {
        (args[0].clone(), &args[1..])
    } else {
        ("8000".to_string(), args)
    };

    // Detect mode: no arg or directory = static file server, -e = one-liner, else = script
    let static_dir = if rest.is_empty() {
        Some(
            std::env::current_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
        )
    } else if rest[0] != "-e" && Path::new(&rest[0]).is_dir() {
        Some(
            std::fs::canonicalize(&rest[0])
                .unwrap_or_else(|_| PathBuf::from(&rest[0]))
                .to_string_lossy()
                .to_string(),
        )
    } else {
        None
    };

    let code = if let Some(dir) = static_dir {
        let dir_escaped = dir.replace('\\', "\\\\").replace('"', "\\\"");
        eprintln!("stryke: serving {} on http://0.0.0.0:{}", dir, port);
        format!(
            r#"
chdir "{dir_escaped}"

my %mime = (
    html => "text/html; charset=utf-8",
    htm => "text/html; charset=utf-8",
    css => "text/css; charset=utf-8",
    js => "application/javascript; charset=utf-8",
    mjs => "application/javascript; charset=utf-8",
    json => "application/json; charset=utf-8",
    xml => "text/xml; charset=utf-8",
    md => "text/markdown; charset=utf-8",
    txt => "text/plain; charset=utf-8",
    toml => "application/toml; charset=utf-8",
    pl => "text/x-perl; charset=utf-8",
    pr => "text/x-perl; charset=utf-8",
    pm => "text/x-perl; charset=utf-8",
    png => "image/png",
    jpg => "image/jpeg",
    jpeg => "image/jpeg",
    gif => "image/gif",
    svg => "image/svg+xml",
    webp => "image/webp",
    avif => "image/avif",
    ico => "image/x-icon",
    woff2 => "font/woff2",
    woff => "font/woff",
    ttf => "font/ttf",
    mp3 => "audio/mpeg",
    ogg => "audio/ogg",
    mp4 => "video/mp4",
    webm => "video/webm",
    zip => "application/zip",
    gz => "application/gzip",
    wasm => "application/wasm",
    pdf => "application/pdf"
)

fn mime_for($path) {{
    my $ext = $path =~ /\.([^.]+)$/ ? lc($1) : ""
    $mime{{$ext}} // "text/plain"
}}

fn dir_listing($url_path, $fs_path) {{
    $url_path .= "/" unless $url_path =~ m|/$|
    my $prefix = $fs_path eq "." ? "" : "$fs_path/"
    my @entries
    push @entries, ".." unless $url_path eq "/"
    push @entries, dirs($fs_path)
    push @entries, filesf($fs_path)
    my $html = ""
    for my $e (@entries) {{
        my $full = $e eq ".." ? ".." : "$prefix$e"
        my $name = $e
        my $href = $url_path . $name
        if (-d $full) {{
            $html .= "<li class=\"dir\"><a href=\"$href/\">$name/</a></li>"
        }} else {{
            my $sz = (stat($full))[7] // 0
            $html .= "<li><a href=\"$href\">$name</a> <span style=\"color:#888\">($sz bytes)</span></li>"
        }}
    }}
    "<!DOCTYPE html><html><head><meta charset=\"utf-8\">"
    . "<title>Directory listing for $url_path</title>"
    . "<style>body{{font-family:monospace;margin:2em}}a{{text-decoration:none}}a:hover{{text-decoration:underline}}li{{padding:2px 0}}.dir{{font-weight:bold}}</style>"
    . "</head><body><h1>Directory listing for $url_path</h1><hr><ul>"
    . $html
    . "</ul><hr><p style=\"color:#888\">stryke/{port}</p></body></html>"
}}

serve {port}, fn ($req) {{
    my $url_path = $req->{{path}}
    $url_path =~ s|\.\./||g
    my $fs_path = $url_path =~ s|^/||r
    $fs_path = "." if $fs_path eq ""

    if (-d $fs_path) {{
        my $idx = $fs_path eq "." ? "index.html" : "$fs_path/index.html"
        if (-f $idx) {{
            +{{ status => 200, body => cat($idx), headers => +{{ "content-type" => "text/html; charset=utf-8" }} }}
        }} else {{
            +{{ status => 200, body => dir_listing($url_path, $fs_path), headers => +{{ "content-type" => "text/html; charset=utf-8" }} }}
        }}
    }} elsif (-f $fs_path) {{
        +{{ status => 200, body => cat($fs_path), headers => +{{ "content-type" => mime_for($fs_path) }} }}
    }} else {{
        +{{ status => 404, body => "404 Not Found: $url_path\n" }}
    }}
}}
"#
        )
    } else if rest[0] == "-e" {
        if rest.len() < 2 {
            eprintln!("stryke serve: -e requires an argument");
            return 1;
        }
        let handler_body = rest[1..].join(" ");
        format!("serve {}, fn ($req) {{ {} }}", port, handler_body)
    } else {
        // Script file — the script must call serve() itself.
        // PORT is injected as $ENV{STRYKE_PORT} for convenience.
        let script_path = &rest[0];
        match std::fs::read_to_string(script_path) {
            Ok(src) => {
                format!("$ENV{{STRYKE_PORT}} = {}\n{}", port, src)
            }
            Err(e) => {
                eprintln!("stryke serve: {}: {}", script_path, e);
                return 1;
            }
        }
    };

    let mut interp = stryke::interpreter::Interpreter::new();
    match stryke::parse_and_run_string(&code, &mut interp) {
        Ok(_) => 0,
        Err(e) => {
            if let stryke::error::ErrorKind::Exit(code) = e.kind {
                return code;
            }
            eprintln!("{}", e);
            255
        }
    }
}

#[allow(non_snake_case)]
/// `stryke docs [TOPIC]` — interactive built-in documentation book.
///
/// - `stryke docs`          → full-screen interactive book (vim-style navigation)
/// - `stryke docs TOPIC`    → single-topic lookup
/// - `stryke docs -t`       → table of contents
/// - `stryke docs -s PAT`   → search topics
/// - `stryke docs -h`       → help
fn run_doc_subcommand(args: &[String]) -> i32 {
    let theme = DocTheme {
        C: "\x1b[36m",
        G: "\x1b[32m",
        Y: "\x1b[1;33m",
        M: "\x1b[35m",
        B: "\x1b[1m",
        D: "\x1b[2m",
        N: "\x1b[0m",
    };
    let DocTheme {
        C,
        G,
        Y,
        M,
        B,
        D,
        N,
    } = theme;

    // Build topic entries from categorized list, then pick up any uncategorized leftovers.
    // Deduplicate aliases that map to the same doc text (e.g. thread/t, hmac/hmac_sha256).
    let mut entries: Vec<(&str, &str, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut seen_text_ptrs = std::collections::HashSet::new();
    for &(category, topics) in stryke::lsp::DOC_CATEGORIES {
        for &topic in topics {
            if let Some(text) = stryke::lsp::doc_text_for(topic) {
                let ptr = text.as_ptr() as usize;
                if !seen_text_ptrs.insert(ptr) {
                    seen.insert(topic);
                    continue; // alias — same doc already rendered under canonical name
                }
                let rendered = render_page_content(topic, text, C, G, D, N);
                entries.push((category, topic, rendered));
                seen.insert(topic);
            }
        }
    }
    // Pick up every documented topic not yet in a category
    for topic in stryke::lsp::doc_topics() {
        if seen.contains(topic) {
            continue;
        }
        if let Some(text) = stryke::lsp::doc_text_for(topic) {
            let ptr = text.as_ptr() as usize;
            if !seen_text_ptrs.insert(ptr) {
                continue; // alias already rendered
            }
            let rendered = render_page_content(topic, text, C, G, D, N);
            entries.push(("Other", topic, rendered));
        }
    }
    if entries.is_empty() {
        eprintln!("stryke docs: no documentation pages found");
        return 1;
    }

    // Pack topics until adding the next would overflow the content area.
    // Header=11 rows, footer=3 rows → content area = term_h - 14.
    let content_area = term_height().saturating_sub(14).max(4);
    let mut pages = build_fixed_pages(&entries, content_area);

    // Insert intro page at position 0
    let entry_count = entries.len();
    let chapter_count = stryke::lsp::DOC_CATEGORIES.len();
    let mut intro = format!(
        "\
  {D}>> THE STRYKE ENCYCLOPEDIA // INTERACTIVE REFERENCE SYSTEM <<{N}\n\
\n\
  {B}A comprehensive reference for every stryke builtin, keyword,{N}\n\
  {B}and extension. {G}{entry_count}{N} {B}topics across {G}{chapter_count}{N} {B}chapters.{N}\n\
\n\
  {D}── GETTING STARTED ─────────────────────────────────────────────{N}\n\
\n\
  {C}j{N} / {C}n{N} / {C}space{N}        next page\n\
  {C}k{N} / {C}p{N}                previous page\n\
  {C}]{N} / {C}[{N}                next / previous chapter\n\
  {C}d{N} / {C}u{N}                forward / back 5 pages\n\
  {C}g{N} / {C}G{N}                first / last page\n\
  {C}t{N}                    table of contents\n\
  {C}/{N}                    search all pages\n\
  {C}:{N}                    jump to page number\n\
  {C}r{N}                    random page\n\
  {C}?{N}                    full keybinding help\n\
  {C}q{N}                    quit\n\
\n\
  {D}── CHAPTERS ───────────────────────────────────────────────────{N}\n\
"
    );
    for (i, &(cat, topics)) in stryke::lsp::DOC_CATEGORIES.iter().enumerate() {
        intro.push_str(&format!(
            "  {C}{:>2}.{N} {B}{:<32}{N} {D}{} topics{N}\n",
            i + 1,
            cat,
            topics.len(),
        ));
    }
    intro.push_str(&format!(
        "\n  {D}press {C}j{D} or {C}space{D} to begin >>>{N}\n"
    ));
    // Pad intro to content area height
    let intro_page = pad_to_height(&intro, content_area);
    pages.insert(0, ("Introduction".to_string(), intro_page, Vec::new()));
    let total = pages.len();

    if args.first().map(|s| s.as_str()) == Some("-h")
        || args.first().map(|s| s.as_str()) == Some("--help")
    {
        println!();
        doc_print_banner(theme);
        doc_print_hline('┌', '┐', theme);
        doc_print_boxline(
            &format!(" {G}STATUS: ONLINE{N}  {D}//{N} {C}SIGNAL: {G}████████{D}░░{N}  {D}//{N} {M}STRYKE DOCS{N}"),
            theme,
        );
        doc_print_hline('└', '┘', theme);
        println!("  {D}>> THE STRYKE ENCYCLOPEDIA // INTERACTIVE REFERENCE SYSTEM <<{N}");
        println!();
        println!("  {B}USAGE:{N} stryke docs {D}[OPTIONS] [PAGE|TOPIC]{N}");
        println!();
        doc_print_separator("OPTIONS", theme);
        println!("  {C}-h, --help{N}                          {D}// Show this help{N}");
        println!("  {C}-t, --toc{N}                           {D}// Table of contents{N}");
        println!("  {C}-s, --search <pattern>{N}              {D}// Search pages{N}");
        println!("  {C}-l, --list{N}                          {D}// List all pages{N}");
        println!(
            "  {C}TOPIC{N}                               {D}// Jump to topic (stryke docs pmap){N}"
        );
        println!("  {C}PAGE{N}                                {D}// Jump to page number{N}");
        println!();
        doc_print_separator("NAVIGATION (vim-style)", theme);
        println!("  {C}j / n / l / enter / space{N}           {D}// Next page{N}");
        println!("  {C}k / p / h{N}                           {D}// Previous page{N}");
        println!("  {C}d{N}                                   {D}// Forward 5 pages{N}");
        println!("  {C}u{N}                                   {D}// Back 5 pages{N}");
        println!("  {C}g / 0{N}                               {D}// First page{N}");
        println!("  {C}G / ${N}                               {D}// Last page{N}");
        println!("  {C}] / }}{N}                              {D}// Next chapter{N}");
        println!("  {C}[ / {{{N}                              {D}// Previous chapter{N}");
        println!("  {C}t{N}                                   {D}// Table of contents{N}");
        println!("  {C}/ <pattern>{N}                         {D}// Search pages{N}");
        println!("  {C}:<number>{N}                           {D}// Jump to page{N}");
        println!("  {C}r{N}                                   {D}// Random page{N}");
        println!("  {C}?{N}                                   {D}// Keybinding help{N}");
        println!("  {C}q{N}                                   {D}// Quit{N}");
        println!();
        doc_print_separator("EXAMPLES", theme);
        println!("  {C}stryke docs{N}                             {D}// start from page 1{N}");
        println!("  {C}stryke docs --toc{N}                       {D}// table of contents{N}");
        println!("  {C}stryke docs 42{N}                          {D}// jump to page 42{N}");
        println!("  {C}stryke docs pmap{N}                        {D}// jump to pmap{N}");
        println!("  {C}stryke docs --search parallel{N}           {D}// find parallel pages{N}");
        println!();
        return 0;
    }

    // --toc: print table of contents and exit
    if args.first().map(|s| s.as_str()) == Some("-t")
        || args.first().map(|s| s.as_str()) == Some("--toc")
    {
        doc_print_toc_entries(&entries, &pages, theme);
        return 0;
    }

    // --list: compact list
    if args.first().map(|s| s.as_str()) == Some("-l")
        || args.first().map(|s| s.as_str()) == Some("--list")
    {
        for (i, (_, topic, _)) in entries.iter().enumerate() {
            println!("{:>3}. {}", i + 1, topic);
        }
        return 0;
    }

    // --search: search and exit
    if (args.first().map(|s| s.as_str()) == Some("-s")
        || args.first().map(|s| s.as_str()) == Some("--search"))
        && args.len() >= 2
    {
        let pat = args[1].to_lowercase();
        let mut found = 0;
        for (i, (cat, topic, text)) in entries.iter().enumerate() {
            if topic.to_lowercase().contains(&pat)
                || cat.to_lowercase().contains(&pat)
                || text.to_lowercase().contains(&pat)
            {
                println!("  {C}{:>3}.{N} {B}{}{N}  {D}({}){N}", i + 1, topic, cat);
                found += 1;
            }
        }
        if found == 0 {
            println!("  {Y}no results for '{}'{N}", pat);
        }
        return 0;
    }

    // Single topic or page number — find which page contains it
    let mut start_page: usize = 0;
    if !args.is_empty() {
        let arg = &args[0];
        // Try page number
        if let Ok(n) = arg.parse::<usize>() {
            if n >= 1 && n <= total {
                start_page = n - 1;
            }
        } else {
            // Try topic name → find which page contains that entry
            let lower = arg.to_lowercase();
            let entry_idx = entries
                .iter()
                .position(|(_, t, _)| t.to_lowercase() == lower)
                .or_else(|| {
                    entries
                        .iter()
                        .position(|(_, t, _)| t.to_lowercase().contains(&lower))
                });
            match entry_idx {
                Some(eidx) => {
                    // Find the page that contains this entry index
                    start_page = pages
                        .iter()
                        .position(|(_, _, indices)| indices.contains(&eidx))
                        .unwrap_or(0);
                }
                None => {
                    eprintln!("stryke docs: no documentation for '{}'", arg);
                    eprintln!("run 'stryke docs -h' for help");
                    return 1;
                }
            }
        }
    }

    // ── Interactive TUI book mode ────────────────────────────
    if !io::stdout().is_terminal() {
        // Not a TTY — just dump the page
        print!("{}", pages[start_page].1);
        return 0;
    }

    doc_interactive_loop(&pages, &entries, &intro, start_page, total, theme)
}

/// Truncate/pad text to exactly `height` lines, joined with `\r\n`.
fn pad_to_height(text: &str, height: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut buf: Vec<&str> = Vec::with_capacity(height);
    for line in lines.iter().take(height) {
        buf.push(line);
    }
    while buf.len() < height {
        buf.push("");
    }
    buf.join("\r\n")
}

/// Pack topics into pages that fit within `max_lines` of content.
/// Pack 2–3 entries per page. Uses 3 when they fit in `max_lines`,
/// otherwise 2. New chapter always starts a new page.
fn build_fixed_pages(
    entries: &[(&str, &str, String)],
    max_lines: usize,
) -> Vec<(String, String, Vec<usize>)> {
    let mut pages: Vec<(String, String, Vec<usize>)> = Vec::new();
    let mut i = 0;
    while i < entries.len() {
        let cat = entries[i].0.to_string();
        // Always take at least 2 (or 1 if last entry)
        let mut end = (i + 2).min(entries.len());
        // Try to fit a 3rd if same chapter and lines fit
        if end < entries.len() && entries[end].0 == cat {
            let lines: usize = (i..=end).map(|j| entries[j].2.lines().count() + 1).sum();
            if lines <= max_lines {
                end += 1;
            }
        }
        // Stop at chapter boundary
        if let Some(pos) = entries[i + 1..end].iter().position(|e| e.0 != cat) {
            end = i + 1 + pos;
        }
        let mut buf = String::new();
        let mut indices = Vec::new();
        for (j, entry) in entries.iter().enumerate().take(end).skip(i) {
            if j > i {
                buf.push('\n');
            }
            buf.push_str(&entry.2);
            indices.push(j);
        }
        pages.push((cat, buf, indices));
        i = end;
    }
    pages
}

/// Find the page whose `indices` contains `entry_idx`.
fn find_page_for_entry(pages: &[(String, String, Vec<usize>)], entry_idx: usize) -> usize {
    for (pi, (_cat, _content, indices)) in pages.iter().enumerate() {
        if indices.contains(&entry_idx) {
            return pi;
        }
    }
    0
}

/// SIGWINCH flag — set by the signal handler, cleared after re-render.
static SIGWINCH_RECEIVED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Bare signal handler — just sets the atomic flag.
#[cfg(unix)]
extern "C" fn sigwinch_handler(_sig: libc::c_int) {
    SIGWINCH_RECEIVED.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[allow(non_snake_case)]
#[derive(Clone, Copy)]
struct DocTheme<'a> {
    C: &'a str,
    G: &'a str,
    Y: &'a str,
    M: &'a str,
    B: &'a str,
    D: &'a str,
    N: &'a str,
}

/// The interactive full-screen pager loop.
#[cfg(unix)]
fn doc_interactive_loop(
    pages: &[(String, String, Vec<usize>)],
    entries: &[(&str, &str, String)],
    intro_raw: &str,
    start: usize,
    total: usize,
    theme: DocTheme,
) -> i32 {
    let DocTheme {
        C, G, M, B, D, N, ..
    } = theme;
    use std::os::unix::io::AsRawFd;

    let stdin_fd = io::stdin().as_raw_fd();
    // Save terminal state and enter raw mode
    let mut old_termios: libc::termios = unsafe { std::mem::zeroed() };
    unsafe { libc::tcgetattr(stdin_fd, &mut old_termios) };
    let mut raw = old_termios;
    unsafe { libc::cfmakeraw(&mut raw) };
    unsafe { libc::tcsetattr(stdin_fd, libc::TCSANOW, &raw) };

    // Install SIGWINCH handler
    let old_sigwinch = unsafe {
        libc::signal(
            libc::SIGWINCH,
            sigwinch_handler as *const () as libc::sighandler_t,
        )
    };

    // Mutable — rebuilt on terminal resize
    let mut pages = pages.to_vec();
    let mut total = total;
    let mut current: usize = start;

    // In raw mode, \n doesn't do \r\n — use this macro for every output line.
    macro_rules! rprint {
        () => { print!("\r\n"); };
        ($($arg:tt)*) => { print!("{}\r\n", format!($($arg)*)); };
    }

    let render = |cur: usize, pages: &[(String, String, Vec<usize>)], total: usize| {
        let (ref cat, ref content, ref indices) = pages[cur];
        // Build topic list for status line
        let topic_list: String = indices
            .iter()
            .take(3)
            .map(|&i| entries[i].1)
            .collect::<Vec<_>>()
            .join(", ");
        let topic_display = if indices.len() > 3 {
            format!("{} +{}", topic_list, indices.len() - 3)
        } else {
            topic_list
        };
        let term_h = term_height();

        // Clear entire screen
        print!("\x1b[H\x1b[2J");

        // ── Header (rows 1-11, absolute positioned) ──
        print!("\x1b[1;1H"); // row 1
        rprint!();
        rprint!(" {C}███████╗████████╗██████╗ ██╗   ██╗██╗  ██╗███████╗{N}");
        rprint!(" {C}██╔════╝╚══██╔══╝██╔══██╗╚██╗ ██╔╝██║ ██╔╝██╔════╝{N}");
        rprint!(" {M}███████╗   ██║   ██████╔╝ ╚████╔╝ █████╔╝ █████╗  {N}");
        rprint!(" {M}╚════██║   ██║   ██╔══██╗  ╚██╔╝  ██╔═██╗ ██╔══╝  {N}");
        rprint!(" {C}███████║   ██║   ██║  ██║   ██║   ██║  ██╗███████╗{N}");
        rprint!(" {C}╚══════╝   ╚═╝   ╚═╝  ╚═╝   ╚═╝   ╚═╝  ╚═╝╚══════╝{N}");
        // Status box
        print!(" {D}┌");
        for _ in 0..74 {
            print!("─");
        }
        print!("┐{N}\r\n");
        let status = format!(
            " {G}{:>3}/{}{N}  {D}//{N} {C}{}{N}  {D}//{N} {M}{}{N}",
            cur + 1,
            total,
            topic_display,
            cat,
        );
        let vis_len = strip_ansi_len(&status);
        let pad = 74_usize.saturating_sub(vis_len);
        print!(" {D}│{N}{status}{:>pad$}{D}│{N}\r\n", "", pad = pad);
        print!(" {D}└");
        for _ in 0..74 {
            print!("─");
        }
        print!("┘{N}\r\n");

        // ── Content (row 12 onward, truncated to fit above footer) ──
        let content_start = 12;
        let footer_rows = 3; // separator + keybindings + prompt
        let max_content = if term_h > content_start + footer_rows {
            term_h - content_start - footer_rows
        } else {
            1
        };
        print!("\x1b[{};1H", content_start);
        for (li, line) in content.lines().enumerate() {
            if li >= max_content {
                break; // truncate — don't scroll past footer
            }
            print!("{line}\r\n");
        }

        // ── Footer (pinned to last 3 rows, absolute positioned) ──
        print!("\x1b[{};1H", term_h - 2);
        print!("  {D}");
        for _ in 0..76 {
            print!("─");
        }
        print!("{N}\r\n");
        print!("  {C}j{N}/{C}n{N} next  {C}k{N}/{C}p{N} prev  {C}d{N}/{C}u{N} ±5  {C}]{N}/{C}[{N} chapter  {C}t{N} toc  {C}/{N} search  {C}:{N}num  {C}r{N} rand  {C}?{N} help  {C}q{N} quit\r\n");
        print!("  {D}>>>{N} ");
        let _ = io::stdout().flush();
    };

    render(current, &pages, total);

    loop {
        let mut buf = [0u8; 1];
        let nread = unsafe { libc::read(stdin_fd, buf.as_mut_ptr() as *mut libc::c_void, 1) };
        if nread != 1 {
            // SIGWINCH — rebuild pages for new terminal height, then re-render
            if SIGWINCH_RECEIVED.swap(false, std::sync::atomic::Ordering::Relaxed) {
                let entry_idx = pages[current].2.first().copied().unwrap_or(0);
                let th = term_height();
                let content_area = th.saturating_sub(14).max(4);
                let mut rebuilt = build_fixed_pages(entries, content_area);
                let intro_page = pad_to_height(intro_raw, content_area);
                rebuilt.insert(0, ("Introduction".to_string(), intro_page, Vec::new()));
                pages = rebuilt;
                total = pages.len();
                current = if entry_idx == 0 && current == 0 {
                    0
                } else {
                    find_page_for_entry(&pages, entry_idx).min(total - 1)
                };
                render(current, &pages, total);
                continue;
            }
            break;
        }
        let key = buf[0];
        match key {
            // Next: j n l space enter
            b'j' | b'n' | b'l' | b' ' | b'\n' | b'\r' if current < total - 1 => {
                current += 1;
            }
            // Prev: k p h
            b'k' | b'p' | b'h' => {
                current = current.saturating_sub(1);
            }
            // First: g 0
            b'g' | b'0' => current = 0,
            // Last: G $
            b'G' | b'$' => current = total - 1,
            // Forward 5: d
            b'd' => {
                current = (current + 5).min(total - 1);
            }
            // Back 5: u
            b'u' => {
                current = current.saturating_sub(5);
            }
            // Next chapter: ] }
            b']' | b'}' => {
                let cur_cat = &pages[current].0;
                while current < total - 1 {
                    current += 1;
                    if pages[current].0 != *cur_cat {
                        break;
                    }
                }
            }
            // Prev chapter: [ {
            b'[' | b'{' => {
                let cur_cat = pages[current].0.clone();
                while current > 0 {
                    current -= 1;
                    if pages[current].0 != cur_cat {
                        break;
                    }
                }
            }
            // Random: r
            b'r' => {
                current = rand::thread_rng().gen_range(0..total);
            }
            // TOC: t
            b't' => {
                // Restore cooked mode for line input
                unsafe { libc::tcsetattr(stdin_fd, libc::TCSANOW, &old_termios) };
                print!("\x1b[H\x1b[2J");
                doc_print_toc_entries(entries, &pages, theme);
                print!("  {D}enter page number or press enter to return >>>{N} ");
                let _ = io::stdout().flush();
                let mut line = String::new();
                let _ = io::stdin().read_line(&mut line);
                if let Ok(n) = line.trim().parse::<usize>() {
                    if n >= 1 && n <= total {
                        current = n - 1;
                    }
                }
                unsafe { libc::tcsetattr(stdin_fd, libc::TCSANOW, &raw) };
            }
            // Search: /
            b'/' => {
                unsafe { libc::tcsetattr(stdin_fd, libc::TCSANOW, &old_termios) };
                print!("\r  {C}/{N}");
                let _ = io::stdout().flush();
                let mut line = String::new();
                let _ = io::stdin().read_line(&mut line);
                let pat = line.trim().to_lowercase();
                if !pat.is_empty() {
                    // Search forward from current page
                    let start_from = (current + 1) % total;
                    let mut found = false;
                    for i in 0..total {
                        let idx = (start_from + i) % total;
                        let (ref cat, ref content, _) = pages[idx];
                        if cat.to_lowercase().contains(&pat)
                            || content.to_lowercase().contains(&pat)
                        {
                            current = idx;
                            found = true;
                            break;
                        }
                    }
                    let _ = found; // overwritten by render
                }
                unsafe { libc::tcsetattr(stdin_fd, libc::TCSANOW, &raw) };
            }
            // Goto: :
            b':' => {
                unsafe { libc::tcsetattr(stdin_fd, libc::TCSANOW, &old_termios) };
                print!("\r  {C}:{N}");
                let _ = io::stdout().flush();
                let mut line = String::new();
                let _ = io::stdin().read_line(&mut line);
                if let Ok(n) = line.trim().parse::<usize>() {
                    if n >= 1 && n <= total {
                        current = n - 1;
                    }
                }
                unsafe { libc::tcsetattr(stdin_fd, libc::TCSANOW, &raw) };
            }
            // Help: ?
            b'?' => {
                print!("\x1b[H\x1b[2J");
                rprint!();
                rprint!("  {D}── KEYBINDINGS ────────────────────────────────────────────────────{N}");
                rprint!();
                rprint!("  {B}Navigation{N}");
                rprint!("  {C}j n l space enter{N}    {D}next page{N}");
                rprint!("  {C}k p h{N}                {D}previous page{N}");
                rprint!("  {C}d{N}                    {D}forward 5 pages{N}");
                rprint!("  {C}u{N}                    {D}back 5 pages{N}");
                rprint!("  {C}g 0{N}                  {D}first page{N}");
                rprint!("  {C}G ${N}                  {D}last page{N}");
                rprint!("  {C}] }}{N}                  {D}next chapter{N}");
                rprint!("  {C}[ {{{N}                  {D}previous chapter{N}");
                rprint!();
                rprint!("  {B}Search & Jump{N}");
                rprint!("  {C}/{N}                    {D}search pages{N}");
                rprint!("  {C}:{N}                    {D}go to page number{N}");
                rprint!("  {C}t{N}                    {D}table of contents{N}");
                rprint!("  {C}r{N}                    {D}random page{N}");
                rprint!();
                rprint!("  {B}Other{N}");
                rprint!("  {C}?{N}                    {D}this help{N}");
                rprint!("  {C}q Q{N}                  {D}quit{N}");
                rprint!();
                rprint!("  {D}press any key to return{N}");
                let _ = io::stdout().flush();
                let mut b2 = [0u8; 1];
                let _ = unsafe { libc::read(stdin_fd, b2.as_mut_ptr() as *mut _, 1) };
            }
            // Quit: q Q
            b'q' | b'Q' | 0x03 /* ctrl-c */ => {
                break;
            }
            _ => {}
        }
        render(current, &pages, total);
    }

    // Restore terminal and SIGWINCH handler
    unsafe { libc::signal(libc::SIGWINCH, old_sigwinch) };
    unsafe { libc::tcsetattr(stdin_fd, libc::TCSANOW, &old_termios) };
    print!("\x1b[H\x1b[2J");
    let _ = io::stdout().flush();
    0
}

#[cfg(not(unix))]
fn doc_interactive_loop(
    pages: &[(String, String, Vec<usize>)],
    _entries: &[(&str, &str, String)],
    _intro_raw: &str,
    start: usize,
    _total: usize,
    _theme: DocTheme,
) -> i32 {
    // Fallback: just print the starting page
    print!("{}", pages[start].1);
    0
}

fn term_height() -> usize {
    #[cfg(unix)]
    {
        let mut ws = libc::winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        if unsafe { libc::ioctl(2, libc::TIOCGWINSZ, &mut ws) } == 0 && ws.ws_row > 0 {
            return ws.ws_row as usize;
        }
    }
    24
}

fn term_width() -> usize {
    #[cfg(unix)]
    {
        let mut ws = libc::winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        if unsafe { libc::ioctl(2, libc::TIOCGWINSZ, &mut ws) } == 0 && ws.ws_col > 0 {
            return ws.ws_col as usize;
        }
    }
    80
}

fn strip_ansi_len(s: &str) -> usize {
    let mut len = 0;
    let mut in_esc = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_esc = true;
        } else if in_esc {
            if c == 'm' {
                in_esc = false;
            }
        } else {
            len += 1;
        }
    }
    len
}

fn doc_print_banner(theme: DocTheme) {
    let DocTheme { C, M, N, .. } = theme;
    println!(" {C}███████╗████████╗██████╗ ██╗   ██╗██╗  ██╗███████╗{N}");
    println!(" {C}██╔════╝╚══██╔══╝██╔══██╗╚██╗ ██╔╝██║ ██╔╝██╔════╝{N}");
    println!(" {M}███████╗   ██║   ██████╔╝ ╚████╔╝ █████╔╝ █████╗  {N}");
    println!(" {M}╚════██║   ██║   ██╔══██╗  ╚██╔╝  ██╔═██╗ ██╔══╝  {N}");
    println!(" {C}███████║   ██║   ██║  ██║   ██║   ██║  ██╗███████╗{N}");
    println!(" {C}╚══════╝   ╚═╝   ╚═╝  ╚═╝   ╚═╝   ╚═╝  ╚═╝╚══════╝{N}");
}

fn doc_print_hline(left: char, right: char, theme: DocTheme) {
    let DocTheme { D, N, .. } = theme;
    print!(" {D}{left}");
    for _ in 0..74 {
        print!("─");
    }
    println!("{right}{N}");
}

fn doc_print_boxline(content: &str, theme: DocTheme) {
    let DocTheme { D, N, .. } = theme;
    // Strip ANSI to measure visible width
    let stripped = content
        .bytes()
        .fold((Vec::new(), false), |(mut acc, in_esc), b| {
            if b == 0x1b {
                (acc, true)
            } else if in_esc {
                (acc, b != b'm')
            } else {
                acc.push(b);
                (acc, false)
            }
        })
        .0;
    let visible = String::from_utf8_lossy(&stripped).chars().count();
    let inner: usize = 74;
    let pad = inner.saturating_sub(visible);
    println!(" {D}│{N}{content}{:>pad$}{D}│{N}", "", pad = pad);
}

fn doc_print_separator(label: &str, theme: DocTheme) {
    let DocTheme { D, N, .. } = theme;
    let trail = 72usize.saturating_sub(label.len());
    print!("  {D}── {label} ");
    for _ in 0..trail {
        print!("─");
    }
    println!("{N}");
}

fn doc_print_toc_entries(
    entries: &[(&str, &str, String)],
    pages: &[(String, String, Vec<usize>)],
    theme: DocTheme,
) {
    let DocTheme {
        C, G, M, B, D, N, ..
    } = theme;
    let topic_count = entries.len();
    let page_count = pages.len();
    println!();
    doc_print_banner(theme);
    doc_print_hline('┌', '┐', theme);
    doc_print_boxline(
        &format!(
            " {G}TABLE OF CONTENTS{N}  {D}//{N} {C}{topic_count} topics, {page_count} pages{N}  {D}//{N} {M}The stryke Encyclopedia{N}"
        ),
        theme,
    );
    doc_print_hline('└', '┘', theme);
    println!();
    let mut last_cat = "";
    for (entry_idx, (cat, topic, _)) in entries.iter().enumerate() {
        if *cat != last_cat {
            println!();
            println!("  {B}{cat}{N}");
            last_cat = cat;
        }
        // Find which page this entry is on (skip intro page at index 0)
        let page_num = pages
            .iter()
            .position(|(_, _, indices)| indices.contains(&entry_idx))
            .map(|p| p + 1)
            .unwrap_or(0);
        println!(
            "    {C}{:>3}.{N} {:<30} {D}p.{}{N}",
            entry_idx + 1,
            topic,
            page_num
        );
    }
    println!();
}

/// Word-wrap a plain-text line at `max_vis` visible characters.
/// Returns wrapped lines (without leading indent — caller adds it).
/// ANSI escapes are not counted toward visible width.
fn word_wrap(text: &str, max_vis: usize) -> Vec<String> {
    if max_vis == 0 {
        return vec![text.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut vis = 0usize;

    for word in text.split(' ') {
        let wvis = strip_ansi_len(word);
        if vis > 0 && vis + 1 + wvis > max_vis {
            // wrap
            lines.push(cur);
            cur = word.to_string();
            vis = wvis;
        } else {
            if vis > 0 {
                cur.push(' ');
                vis += 1;
            }
            cur.push_str(word);
            vis += wvis;
        }
    }
    if !cur.is_empty() || lines.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Render a single page's content (without banner/chrome).
/// Prose lines are word-wrapped at 76 visible columns (80 - 2*indent).
/// Code lines are kept as-is (indented 4 spaces).
#[allow(non_snake_case)]
fn render_page_content(topic: &str, text: &str, C: &str, G: &str, D: &str, N: &str) -> String {
    let max_vis = term_width().saturating_sub(4).max(40); // width - 2 indent - 2 margin
    let mut out = String::with_capacity(text.len() + 512);
    out.push_str(&format!("  {C}{topic}{N}\n"));
    out.push_str(&format!(
        "  {D}{}{N}\n",
        "─".repeat(topic.len().max(20).min(max_vis))
    ));
    let mut in_code = false;
    for line in text.split('\n') {
        if line.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            out.push_str(&format!("  {G}  {line}{N}\n"));
        } else if line.trim().is_empty() {
            out.push('\n');
        } else {
            let rendered = render_inline_code(line, C, N);
            for wrapped in word_wrap(&rendered, max_vis) {
                out.push_str(&format!("  {wrapped}\n"));
            }
        }
    }
    out
}

/// Replace `backtick` spans with colored versions for terminal output.
fn render_inline_code(line: &str, color: &str, reset: &str) -> String {
    let mut out = String::with_capacity(line.len() + 64);
    let mut in_tick = false;
    for ch in line.chars() {
        if ch == '`' {
            if in_tick {
                out.push_str(reset);
            } else {
                out.push_str(color);
            }
            in_tick = !in_tick;
        } else {
            out.push(ch);
        }
    }
    out
}

/// `stryke deconvert FILE...` — convert stryke .stk files back to standard Perl .pl syntax.
fn run_deconvert_subcommand(args: &[String]) -> i32 {
    let mut files: Vec<String> = Vec::new();
    let mut in_place = false;
    let mut output_delim: Option<char> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-i" | "--in-place" => in_place = true,
            "-d" | "--output-delim" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("stryke deconvert: --output-delim requires an argument");
                    return 2;
                }
                let delim_str = &args[i];
                if delim_str.chars().count() != 1 {
                    eprintln!(
                        "stryke deconvert: --output-delim must be a single character, got {:?}",
                        delim_str
                    );
                    return 2;
                }
                output_delim = delim_str.chars().next();
            }
            "-h" | "--help" => {
                println!("usage: stryke deconvert [-i] [-d DELIM] FILE...");
                println!();
                println!("Convert stryke .stk files back to standard Perl .pl syntax:");
                println!("  - Pipe chains and thread macros → nested function calls");
                println!("  - fn → sub");
                println!("  - p → say");
                println!("  - Adds trailing semicolons");
                println!("  - #!/usr/bin/env perl shebang prepended");
                println!();
                println!("Options:");
                println!("  -i, --in-place       Write .pl files alongside originals");
                println!("  -d, --output-delim   Delimiter for s///, tr///, m// (default: preserve original)");
                println!();
                println!("Examples:");
                println!("  stryke deconvert app.stk             # print to stdout");
                println!("  stryke deconvert -i lib/*.stk        # write lib/*.pl");
                println!(
                    "  stryke deconvert -d '|' app.stk      # use | as delimiter: s|old|new|g"
                );
                return 0;
            }
            s if s.starts_with('-') => {
                eprintln!("stryke deconvert: unknown option: {}", s);
                eprintln!("usage: stryke deconvert [-i] [-d DELIM] FILE...");
                return 2;
            }
            s => files.push(s.to_string()),
        }
        i += 1;
    }
    if files.is_empty() {
        eprintln!("stryke deconvert: no input files");
        eprintln!("usage: stryke deconvert [-i] [-d DELIM] FILE...");
        return 2;
    }
    let opts = stryke::deconvert::DeconvertOptions { output_delim };
    let mut errors = 0;
    for f in &files {
        let code = match std::fs::read_to_string(f) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("stryke deconvert: {}: {}", f, e);
                errors += 1;
                continue;
            }
        };
        let program = match stryke::parse_with_file(&code, f) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("stryke deconvert: {}: {}", f, e);
                errors += 1;
                continue;
            }
        };
        let deconverted = stryke::deconvert_to_perl_with_options(&program, &opts);
        if in_place {
            let out_path = std::path::Path::new(f).with_extension("pl");
            if let Err(e) = std::fs::write(&out_path, &deconverted) {
                eprintln!("stryke deconvert: {}: {}", out_path.display(), e);
                errors += 1;
            }
        } else {
            println!("{}", deconverted);
        }
    }
    if errors > 0 {
        1
    } else {
        0
    }
}

/// Strip shebang line; if extract mode (-x), skip everything until #!...perl line.
fn strip_shebang_and_extract(content: &str, extract: bool) -> String {
    if extract {
        // -x: skip lines until we find one starting with #! and containing "perl"
        let mut found = false;
        let mut lines = Vec::new();
        for line in content.lines() {
            if !found {
                if line.starts_with("#!") && line.contains("perl") {
                    found = true;
                    // Don't include the shebang line itself
                }
                continue;
            }
            // Stop at __END__ or __DATA__
            if line == "__END__" || line == "__DATA__" {
                break;
            }
            lines.push(line);
        }
        lines.join("\n")
    } else if content.starts_with("#!") {
        if let Some(pos) = content.find('\n') {
            content[pos + 1..].to_string()
        } else {
            String::new()
        }
    } else {
        content.to_string()
    }
}

/// `stryke fmt [-i] [-w WIDTH] FILE...` — format stryke source files.
fn run_fmt_subcommand(args: &[String]) -> i32 {
    let mut files: Vec<String> = Vec::new();
    let mut in_place = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-i" | "--in-place" => in_place = true,
            "-h" | "--help" => {
                println!("usage: stryke fmt [-i] FILE...");
                println!();
                println!("Format stryke source files (parse → pretty-print).");
                println!();
                println!("Options:");
                println!("  -i, --in-place   Rewrite files in place (default: print to stdout)");
                println!();
                println!("Examples:");
                println!("  stryke fmt app.stk              # print formatted source to stdout");
                println!("  stryke fmt -i lib/*.stk          # rewrite files in place");
                println!("  stryke fmt -i .                  # format all .stk files recursively");
                return 0;
            }
            s if s.starts_with('-') => {
                eprintln!("stryke fmt: unknown option: {}", s);
                eprintln!("usage: stryke fmt [-i] FILE...");
                return 2;
            }
            s => files.push(s.to_string()),
        }
        i += 1;
    }
    if files.is_empty() {
        eprintln!("stryke fmt: no input files");
        eprintln!("usage: stryke fmt [-i] FILE...");
        return 2;
    }
    // Expand directory arguments: recursively collect .stk/.pl/.pm files.
    let mut expanded: Vec<String> = Vec::new();
    for f in &files {
        let p = std::path::Path::new(f);
        if p.is_dir() {
            collect_stryke_files(p, &mut expanded);
        } else {
            expanded.push(f.clone());
        }
    }
    if expanded.is_empty() {
        eprintln!("stryke fmt: no .stk/.pl/.pm files found");
        return 1;
    }
    let mut errors = 0;
    for f in &expanded {
        let code = match std::fs::read_to_string(f) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("stryke fmt: {}: {}", f, e);
                errors += 1;
                continue;
            }
        };
        let program = match stryke::parse_with_file(&code, f) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("stryke fmt: {}: {}", f, e);
                errors += 1;
                continue;
            }
        };
        let formatted = stryke::convert_to_stryke(&program);
        if in_place {
            if formatted == code {
                continue; // already formatted
            }
            if let Err(e) = std::fs::write(f, &formatted) {
                eprintln!("stryke fmt: {}: {}", f, e);
                errors += 1;
            } else {
                eprintln!("  formatted {}", f);
            }
        } else {
            print!("{}", formatted);
        }
    }
    if errors > 0 {
        1
    } else {
        0
    }
}

/// Recursively collect `.stk`, `.pl`, `.pm` files from a directory.
fn collect_stryke_files(dir: &std::path::Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<std::path::PathBuf> =
        entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            // Skip hidden dirs and common noise.
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            collect_stryke_files(&p, out);
        } else if let Some(ext) = p.extension() {
            let ext = ext.to_string_lossy();
            if ext == "stk" || ext == "pl" || ext == "pm" {
                out.push(p.to_string_lossy().to_string());
            }
        }
    }
}

/// `stryke bench [FILE|DIR]` — discover and run benchmark files with timing.
fn run_bench_subcommand(argv0: &str, args: &[String]) -> i32 {
    if !args.is_empty() && (args[0] == "-h" || args[0] == "--help") {
        println!("usage: stryke bench [FILE|DIR]");
        println!();
        println!("Discover and run benchmark files. Looks for bench_*.stk / b_*.stk");
        println!("in bench/ or benches/ directories (or a specified path).");
        println!();
        println!("Each file is run and timed. Use the `bench {{ }}` builtin inside");
        println!("files for micro-benchmarks with iteration counts and ops/sec.");
        println!();
        println!("Examples:");
        println!("  stryke bench                    # auto-discover bench/ or benches/");
        println!("  stryke bench bench/bench_sort.stk  # run a single benchmark");
        println!("  stryke bench benches/            # run all in a directory");
        return 0;
    }
    let target = if !args.is_empty() {
        args[0].clone()
    } else if std::path::Path::new("bench").is_dir() {
        "bench".to_string()
    } else if std::path::Path::new("benches").is_dir() {
        "benches".to_string()
    } else {
        eprintln!("stryke bench: no bench/ or benches/ directory found");
        return 1;
    };
    let target_path = std::path::Path::new(&target);
    let bench_files: Vec<String> = if target_path.is_dir() {
        let mut files: Vec<String> = std::fs::read_dir(target_path)
            .unwrap_or_else(|e| {
                eprintln!("stryke bench: {}: {}", target, e);
                process::exit(1);
            })
            .filter_map(|e| e.ok())
            .map(|e| e.path().to_string_lossy().to_string())
            .filter(|p| {
                let name = std::path::Path::new(p)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                (name.starts_with("bench_") || name.starts_with("b_"))
                    && (name.ends_with(".stk") || name.ends_with(".st") || name.ends_with(".pl"))
            })
            .collect();
        files.sort();
        files
    } else {
        vec![target]
    };
    if bench_files.is_empty() {
        eprintln!("stryke bench: no benchmark files found (bench_*.stk or b_*.stk)");
        return 1;
    }
    let total = bench_files.len();
    let mut failed = 0;
    eprintln!(
        "\x1b[36mRunning {} benchmark{}\x1b[0m\n",
        total,
        if total == 1 { "" } else { "s" }
    );
    let exe = std::env::current_exe()
        .ok()
        .filter(|p| p.exists())
        .or_else(|| std::fs::canonicalize(argv0).ok())
        .unwrap_or_else(|| PathBuf::from(argv0));
    for f in &bench_files {
        let name = std::path::Path::new(f)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| f.clone());
        eprint!("\x1b[1m── {} ──\x1b[0m ", name);
        let script_abs = std::fs::canonicalize(f).unwrap_or_else(|_| PathBuf::from(f));
        let project_root = script_abs
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or(std::path::Path::new("."));
        let start = std::time::Instant::now();
        let output = process::Command::new(&exe)
            .arg(&script_abs)
            .args(&args[1..])
            .current_dir(project_root)
            .stderr(process::Stdio::piped())
            .stdout(process::Stdio::piped())
            .output();
        let elapsed = start.elapsed();
        match output {
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let stdout = String::from_utf8_lossy(&out.stdout);
                if out.status.success() {
                    eprintln!("\x1b[32m{:.3}s\x1b[0m", elapsed.as_secs_f64());
                } else {
                    eprintln!("\x1b[31mFAILED ({:.3}s)\x1b[0m", elapsed.as_secs_f64());
                    failed += 1;
                }
                // Print benchmark output (stderr first, then stdout).
                if !stderr.is_empty() {
                    eprint!("{}", stderr);
                }
                if !stdout.is_empty() {
                    print!("{}", stdout);
                }
            }
            Err(e) => {
                eprintln!("\x1b[31mfailed to run: {}\x1b[0m", e);
                failed += 1;
            }
        }
        eprintln!();
    }
    eprintln!("═══════════════════════════════");
    if failed == 0 {
        eprintln!(
            "\x1b[32m✓ All {} benchmark{} completed\x1b[0m",
            total,
            if total == 1 { "" } else { "s" }
        );
        0
    } else {
        eprintln!(
            "\x1b[31m✗ {} of {} benchmark{} failed\x1b[0m",
            failed,
            total,
            if total == 1 { "" } else { "s" }
        );
        1
    }
}

/// `stryke init [NAME]` — scaffold a new stryke project.
fn run_init_subcommand(args: &[String]) -> i32 {
    if !args.is_empty() && (args[0] == "-h" || args[0] == "--help") {
        println!("usage: stryke init [NAME]");
        println!();
        println!("Create a new stryke project directory with:");
        println!("  NAME/main.stk      — entry point");
        println!("  NAME/lib/          — library modules");
        println!("  NAME/t/            — test directory");
        println!("  NAME/bench/        — benchmark files");
        println!("  NAME/.gitignore    — ignore build artifacts");
        println!();
        println!("If NAME is omitted, initializes the current directory.");
        println!();
        println!("Examples:");
        println!("  stryke init myapp        # create myapp/ project");
        println!("  stryke init              # init in current directory");
        return 0;
    }
    let project_dir = if !args.is_empty() {
        let dir = PathBuf::from(&args[0]);
        if dir.exists() && !dir.is_dir() {
            eprintln!(
                "stryke init: {} already exists and is not a directory",
                args[0]
            );
            return 1;
        }
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("stryke init: {}: {}", args[0], e);
            return 1;
        }
        dir
    } else {
        PathBuf::from(".")
    };
    let name = if !args.is_empty() {
        args[0].clone()
    } else {
        std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "stryke_project".to_string())
    };
    // Create main.stk
    let main_path = project_dir.join("main.stk");
    if !main_path.exists() {
        let main_content = format!("#!/usr/bin/env stryke\n\np \"hello from {}!\"\n", name);
        if let Err(e) = std::fs::write(&main_path, main_content) {
            eprintln!("stryke init: {}: {}", main_path.display(), e);
            return 1;
        }
        eprintln!("  created {}", main_path.display());
    }
    // Create lib/ directory
    let lib_dir = project_dir.join("lib");
    if !lib_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&lib_dir) {
            eprintln!("stryke init: {}: {}", lib_dir.display(), e);
            return 1;
        }
        eprintln!("  created {}/", lib_dir.display());
    }
    // Create t/ directory
    let test_dir = project_dir.join("t");
    if !test_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&test_dir) {
            eprintln!("stryke init: {}: {}", test_dir.display(), e);
            return 1;
        }
        eprintln!("  created {}/", test_dir.display());
    }
    // Create bench/ directory
    let bench_dir = project_dir.join("bench");
    if !bench_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&bench_dir) {
            eprintln!("stryke init: {}: {}", bench_dir.display(), e);
            return 1;
        }
        eprintln!("  created {}/", bench_dir.display());
    }
    // Create a sample test
    let test_path = test_dir.join("test_main.stk");
    if !test_path.exists() {
        let test_content =
            "#!/usr/bin/env stryke\n\nuse Test\n\nok 1, \"it works\"\n\ndone_testing()\n";
        if let Err(e) = std::fs::write(&test_path, test_content) {
            eprintln!("stryke init: {}: {}", test_path.display(), e);
            return 1;
        }
        eprintln!("  created {}", test_path.display());
    }
    // Create .gitignore
    let gi_path = project_dir.join(".gitignore");
    if !gi_path.exists() {
        let gi_content = "# stryke build artifacts\n/target/\n*.pec\n";
        if let Err(e) = std::fs::write(&gi_path, gi_content) {
            eprintln!("stryke init: {}: {}", gi_path.display(), e);
            return 1;
        }
        eprintln!("  created {}", gi_path.display());
    }
    eprintln!(
        "\x1b[32m✓ Initialized stryke project{}\x1b[0m",
        if !args.is_empty() {
            format!(" in {}/", name)
        } else {
            String::new()
        }
    );
    eprintln!();
    eprintln!("  stryke run           # run main.stk");
    eprintln!("  stryke test          # run tests in t/");
    eprintln!("  stryke build main.stk  # compile to standalone binary");
    0
}

/// `stryke repl [--load FILE]` — explicit REPL entry with optional pre-load.
fn run_repl_subcommand(args: &[String]) -> i32 {
    let mut load_file: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--load" | "-l" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("stryke repl: --load requires a file argument");
                    return 2;
                }
                load_file = Some(args[i].clone());
            }
            "-h" | "--help" => {
                println!("usage: stryke repl [--load FILE]");
                println!();
                println!("Start the interactive REPL (readline, history, tab completion).");
                println!();
                println!("Options:");
                println!("  -l, --load FILE  Evaluate FILE before entering the REPL");
                println!();
                println!("Examples:");
                println!("  stryke repl                   # start REPL");
                println!("  stryke repl --load lib.stk    # pre-load a library, then REPL");
                return 0;
            }
            other => {
                eprintln!("stryke repl: unknown option: {}", other);
                eprintln!("usage: stryke repl [--load FILE]");
                return 2;
            }
        }
        i += 1;
    }
    // Build a Cli struct for the REPL, optionally with a pre-load script.
    let mut cli = Cli::default();
    if let Some(ref path) = load_file {
        if !std::path::Path::new(path).exists() {
            eprintln!("stryke repl: file not found: {}", path);
            return 1;
        }
        // Use -e to pre-execute: `require "FILE";`
        cli.execute.push(format!("require {:?}", path));
    }
    repl::run(&cli);
    0
}

/// Heuristic: does this string look like inline code rather than a filename?
/// Used for `stryke 'p 1+2'` (no `-e` needed).
fn looks_like_code(s: &str) -> bool {
    // Contains whitespace, Perl operators, or known statement starters
    s.contains(' ')
        || s.contains(';')
        || s.contains('|')
        || s.contains('{')
        || s.contains('(')
        || s.contains('$')
        || s.contains('@')
        || s.contains('>')
}

/// Look for a script file in PATH (-S flag).
fn find_in_path(script: &str) -> Option<String> {
    if std::path::Path::new(script).is_absolute() || script.contains('/') {
        return Some(script.to_string());
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(':') {
            let full = format!("{}/{}", dir, script);
            if std::path::Path::new(&full).exists() {
                return Some(full);
            }
        }
    }
    None
}

/// Print configuration summary (-V flag).
fn print_config(configvar: Option<&str>) {
    let version = env!("CARGO_PKG_VERSION");
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    if let Some(var) = configvar {
        // Print a single config variable
        let val = match var {
            "version" | "api_version" => version.to_string(),
            "archname" => format!("{}-{}", arch, os),
            "osname" => os.to_string(),
            "threads" => threads.to_string(),
            "useithreads" | "usethreads" => "define".to_string(),
            "use64bitint" | "use64bitall" => "define".to_string(),
            "cc" => "rustc".to_string(),
            "optimize" => "-O3 -lto".to_string(),
            "prefix" | "installprefix" => "/usr/local".to_string(),
            "perlpath" => "stryke".to_string(),
            _ => {
                eprintln!("Unknown config variable: {}", var);
                return;
            }
        };
        println!("{}='{}'", var, val);
    } else {
        println!("Summary of stryke v{} configuration:\n", version);
        println!("  Platform:");
        println!("    osname={}, archname={}-{}", os, arch, os);
        println!("  Compiler:");
        println!("    cc=rustc, optimize=-O3 -lto");
        println!("  Threading:");
        println!("    useithreads=define, threads={}", threads);
        println!("  Integer/Float:");
        println!("    use64bitint=define, use64bitall=define");
        println!("  Parallel extensions:");
        println!("    rayon=define, pmap=define, pmap_chunked=define, pipeline=define, par_pipeline=define, async=define, await=define, pgrep=define, pfor=define, psort=define, reduce=define, preduce=define, preduce_init=define, jit=define");
        println!("  Install:");
        println!("    perlpath=stryke");
    }
}

#[cfg(test)]
mod cli_argv_tests {
    use super::{expand_perl_bundled_argv, normalize_argv_after_dash_e, parse_cli_prelude, Cli};
    use clap::Parser;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn prelude_inserts_double_dash_before_script_argv_long_flags() {
        let a = args(&["stryke", "s.pl", "--regex", "--foo"]);
        let cli = parse_cli_prelude(&a).expect("expected prelude parse");
        assert_eq!(cli.script.as_deref(), Some("s.pl"));
        assert_eq!(cli.args, vec!["--regex".to_string(), "--foo".to_string()]);
    }

    #[test]
    fn prelude_with_dash_w_before_script() {
        let a = args(&["stryke", "-w", "s.pl", "--regex"]);
        let cli = parse_cli_prelude(&a).expect("expected prelude parse");
        assert!(cli.warnings);
        assert_eq!(cli.script.as_deref(), Some("s.pl"));
        assert_eq!(cli.args, vec!["--regex".to_string()]);
    }

    #[test]
    fn prelude_dash_e_then_argv_with_long_flag() {
        let a = args(&["stryke", "-e", "1", "foo", "--regex"]);
        let mut cli = parse_cli_prelude(&a).expect("expected prelude parse");
        normalize_argv_after_dash_e(&mut cli);
        assert_eq!(cli.execute, vec!["1"]);
        assert!(cli.script.is_none());
        assert_eq!(cli.args, vec!["foo".to_string(), "--regex".to_string()]);
    }

    #[test]
    fn explicit_user_double_dash_skips_prelude() {
        let a = args(&["stryke", "--", "s.pl", "x"]);
        assert!(parse_cli_prelude(&a).is_none());
    }

    #[test]
    fn bundled_lane_le_lne_maps_to_split_switches() {
        for (flag, code, expect_a, expect_n) in [
            ("-lane", "print 1", true, true),
            ("-le", "print 2", false, false),
            ("-lne", "print 3", false, true),
            ("-lnE", "p 4", false, true),
        ] {
            let a = expand_perl_bundled_argv(args(&["stryke", flag, code]));
            let cli = Cli::try_parse_from(&a).expect("parse bundled flags");
            assert!(
                cli.line_ending.is_some(),
                "{flag}: expected -l (line ending)"
            );
            assert_eq!(cli.auto_split, expect_a, "{flag}: autosplit (-a)");
            assert_eq!(cli.line_mode, expect_n, "{flag}: line loop (-n)");
            if flag.contains('E') {
                assert_eq!(cli.execute_features, vec![code]);
                assert!(cli.execute.is_empty());
            } else {
                assert_eq!(cli.execute, vec![code]);
                assert!(cli.execute_features.is_empty());
            }
        }
    }

    #[test]
    fn bundled_lpe_preserves_print_mode() {
        let a = expand_perl_bundled_argv(args(&["stryke", "-lpe", "print 1"]));
        let cli = Cli::try_parse_from(&a).expect("parse");
        assert!(cli.print_mode);
        assert_eq!(cli.execute, vec!["print 1"]);
    }

    #[test]
    fn bundled_0777_not_split() {
        let a = expand_perl_bundled_argv(args(&["stryke", "-0777", "-e", "1"]));
        assert!(
            a.contains(&"-0777".to_string()),
            "expected -0777 kept intact: {a:?}"
        );
    }

    #[test]
    fn bundled_0ne_splits_like_perl() {
        let a = expand_perl_bundled_argv(args(&["stryke", "-0ne", "print 1"]));
        let cli = Cli::try_parse_from(&a).expect("parse");
        assert_eq!(cli.execute, vec!["print 1"]);
        assert!(cli.line_mode);
    }

    #[test]
    fn bundled_f_colon_takes_rest_of_token() {
        let a = expand_perl_bundled_argv(args(&["stryke", "-F:", "-anE", "say $F[0]"]));
        let cli = Cli::try_parse_from(&a).expect("parse");
        assert_eq!(cli.field_separator.as_deref(), Some(":"));
        assert!(cli.auto_split);
        assert!(cli.line_mode);
        assert_eq!(cli.execute_features, vec!["say $F[0]"]);
    }

    #[test]
    fn bundled_f_comma_takes_rest_of_token() {
        let a = expand_perl_bundled_argv(args(&["stryke", "-F,", "-anE", "print 1"]));
        let cli = Cli::try_parse_from(&a).expect("parse");
        assert_eq!(cli.field_separator.as_deref(), Some(","));
    }

    #[test]
    fn help_alias_not_bundled_as_h_e_l_p() {
        let a = expand_perl_bundled_argv(args(&["stryke", "-help"]));
        let cli = Cli::try_parse_from(&a).expect("parse");
        assert!(cli.help);
    }

    #[test]
    fn thread_operator_not_bundled() {
        // `->>` and `~>` are threading operators, not bundled flags
        let a = expand_perl_bundled_argv(args(&["stryke", "->> 1 p"]));
        assert_eq!(a, args(&["stryke", "->> 1 p"]));

        let b = expand_perl_bundled_argv(args(&["stryke", "~> 1 p"]));
        assert_eq!(b, args(&["stryke", "~> 1 p"]));
    }
}
