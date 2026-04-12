use std::fs::File;
use std::io::{self, BufReader, IsTerminal, Read as IoRead, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Mutex;

use clap::Parser;
use rand::Rng;
use rayon::prelude::*;

use perlrs::ast::Program;
use perlrs::error::{ErrorKind, PerlError};
use perlrs::interpreter::Interpreter;
use perlrs::perl_fs::{
    decode_utf8_or_latin1, read_file_text_perl_compat, read_line_perl_compat,
    read_logical_line_perl_compat,
};

mod repl;

/// perlrs — A highly parallel Perl 5 interpreter written in Rust
#[derive(Parser, Debug, Default)]
#[command(name = "perlrs", version, about, long_about = None)]
#[command(disable_version_flag = true, disable_help_flag = true)]
#[command(override_usage = "perlrs [switches] [--] [programfile] [arguments]")]
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

    /// Number of threads for parallel operations (perlrs extension)
    #[arg(short = 'j', long = "threads", value_name = "N")]
    threads: Option<usize>,

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
        "-help" => return Some(vec!["-h".to_string()]),
        "-version" => return Some(vec!["-v".to_string()]),
        _ => {}
    }
    if arg == "-" || !arg.starts_with('-') || arg.starts_with("--") {
        return None;
    }
    let s = arg.strip_prefix('-')?;
    if s.is_empty() || s.len() == 1 {
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
            // `-F` takes the rest of the bundled token as the split pattern (Perl `perl -F: -a`…).
            b'F' => {
                out.push("-F".to_string());
                i += 1;
                if i < b.len() {
                    out.push(s[i..].to_string());
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

    println!("{C} ██████╗ ███████╗██████╗ ██╗     ██████╗ ███████╗{N}");
    println!("{C} ██╔══██╗██╔════╝██╔══██╗██║     ██╔══██╗██╔════╝{N}");
    println!("{M} ██████╔╝█████╗  ██████╔╝██║     ██████╔╝███████╗{N}");
    println!("{M} ██╔═══╝ ██╔══╝  ██╔══██╗██║     ██╔══██╗╚════██║{N}");
    println!("{R} ██║     ███████╗██║  ██║███████╗██║  ██║███████║{N}");
    println!("{R} ╚═╝     ╚══════╝╚═╝  ╚═╝╚══════╝╚═╝  ╚═╝╚══════╝{N}");
    println!("{C} ┌──────────────────────────────────────────────────────┐{N}");
    println!("{C} │ STATUS: ONLINE  // CORES: {threads:<2} // SIGNAL: ████████░░ │{N}");
    println!("{C} └──────────────────────────────────────────────────────┘{N}");
    println!("{M}  >> PARALLEL PERL5 INTERPRETER // RUST-POWERED v{version} <<{N}");
    println!();
    println!();
    println!("A highly parallel Perl 5 interpreter written in Rust");
    println!();
    println!("{Y}  USAGE:{N} {bin} [switches] [--] [programfile] [arguments]");
    println!();
    println!("{C}  ── EXECUTION ──────────────────────────────────────────{N}");
    println!("  -e CODE                {G}//{N} One line of program (several -e's allowed)");
    println!("  -E CODE                {G}//{N} Like -e, but enables all optional features");
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
    println!("  --no-jit               {G}//{N} Disable Cranelift JIT (bytecode interpreter only)");
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
    println!(
        "  --remote-worker        {G}//{N} Persistent cluster worker (stdio); only arg after {bin}"
    );
    println!(
        "  --remote-worker-v1     {G}//{N} Legacy one-shot worker (stdio); only arg after {bin}"
    );
    if matches!(bin, "pe" | "perlrs") {
        println!(
            "  (no switches, TTY stdin) {G}//{N} Interactive REPL (readline; exit with quit or EOF)"
        );
    }
    println!("{C}  ── PARALLEL EXTENSIONS (perlrs) ─────────────────────{N}");
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
    println!("{C}  ── TYPING (perlrs) ───────────────────────────────────{N}");
    println!(
        "  typed my \\$x : Int|Str|Float  {G}//{N} Optional scalar types; runtime checks on assign"
    );
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
    dir.join(format!("{name}.pe-tmp-{rnd}"))
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
            None | Some("") => interp.irs = "\0".to_string(),
            Some("777") => interp.irs = String::new(),
            Some(oct_str) => {
                if let Ok(val) = u32::from_str_radix(oct_str, 8) {
                    if let Some(ch) = char::from_u32(val) {
                        interp.irs = ch.to_string();
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
        eprintln!("perlrs: taint mode acknowledged but not enforced");
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
                        .set_scalar(name, perlrs::value::PerlValue::string(val.to_string()));
                } else {
                    let _ = interp
                        .scope
                        .set_scalar(switch, perlrs::value::PerlValue::integer(1));
                }
            }
        }
        argv = remaining;
    }

    interp.argv = argv.clone();
    interp.scope.declare_array(
        "ARGV",
        argv.into_iter()
            .map(perlrs::value::PerlValue::string)
            .collect(),
    );

    // Order: `-I`, in-tree `vendor/perl` (pure-Perl List::Util, …), system `perl`’s @INC, script
    // dir, `PERLRS_INC`, then `.` (deduped).
    let mut inc_paths: Vec<String> = cli.include.clone();
    let vendor = perlrs::vendor_perl_inc_path();
    if vendor.is_dir() {
        perlrs::perl_inc::push_unique_string_paths(
            &mut inc_paths,
            vec![vendor.to_string_lossy().into_owned()],
        );
    }
    perlrs::perl_inc::push_unique_string_paths(
        &mut inc_paths,
        perlrs::perl_inc::paths_from_system_perl(),
    );
    if filename != "-e" && filename != "-" && filename != "repl" {
        if let Some(parent) = std::path::Path::new(filename).parent() {
            if !parent.as_os_str().is_empty() {
                perlrs::perl_inc::push_unique_string_paths(
                    &mut inc_paths,
                    vec![parent.to_string_lossy().into_owned()],
                );
            }
        }
    }
    if let Ok(extra) = std::env::var("PERLRS_INC") {
        let extra: Vec<String> = std::env::split_paths(&extra)
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        perlrs::perl_inc::push_unique_string_paths(&mut inc_paths, extra);
    }
    perlrs::perl_inc::push_unique_string_paths(&mut inc_paths, vec![".".to_string()]);
    let inc_dirs: Vec<perlrs::value::PerlValue> = inc_paths
        .into_iter()
        .map(perlrs::value::PerlValue::string)
        .collect();
    interp.scope.declare_array("INC", inc_dirs);

    if cli.debugger.is_some() {
        eprintln!("perlrs: debugger not yet implemented, running normally");
    }
}

fn main() {
    // AOT: if the running binary carries an embedded script trailer, execute it and
    // exit. Bypasses clap, flags, REPL — the embedded binary behaves like a plain native
    // program: all command-line args become `@ARGV` for the embedded script. The probe
    // costs one file open + one 32-byte read (~50 µs) on the no-trailer path.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(embedded) = perlrs::aot::try_load_embedded(&exe) {
            let argv: Vec<String> = std::env::args().skip(1).collect();
            process::exit(run_embedded_script(embedded, argv));
        }
    }

    let args = expand_perl_bundled_argv(std::env::args().collect());

    if args.len() == 2 && args[1] == "--remote-worker" {
        // Persistent v3 session loop: HELLO → SESSION_INIT → many JOBs → SHUTDOWN.
        // The basic v1 one-shot loop is still reachable via `--remote-worker-v1` for the
        // round-trip integration test.
        process::exit(perlrs::remote_wire::run_remote_worker_session());
    }
    if args.len() == 2 && args[1] == "--remote-worker-v1" {
        process::exit(perlrs::remote_wire::run_remote_worker_stdio());
    }

    if args.len() == 2 && args[1] == "--lsp" {
        process::exit(perlrs::run_lsp_stdio());
    }

    // `pe build SCRIPT -o OUT` subcommand: intercept before clap so `build` does not have
    // to be added to the main `Cli` struct (keeping the perl-compatible flag surface clean).
    if args.len() >= 2 && args[1] == "build" {
        process::exit(run_build_subcommand(&args[2..]));
    }

    // Fast path: `perlrs SCRIPT [ARGS...]` with no dashes anywhere — the common case, and
    // clap parsing is the dominant term on `print "hello\n"` (it knocks ~1ms off the
    // startup bench). We can't bypass clap when any flag is present, so fall through to the
    // full parser in that case.
    let mut cli = if args.len() >= 2
        && !args[1].starts_with('-')
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

    if cli.help {
        print_cyberpunk_help();
        return;
    }

    if cli.show_version {
        println!(
            "This is perlrs v{} — A highly parallel Perl 5 interpreter (Rust)\n",
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
        match perlrs::error::explain_error(code) {
            Some(text) => println!("{}", text),
            None => {
                eprintln!("perlrs: unknown explain code {:?}", code);
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

    let is_repl = matches!(env!("CARGO_BIN_NAME"), "pe" | "perlrs")
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

    let (program_text, data_opt) = perlrs::data_section::split_data_section(&raw_script);
    let code = strip_shebang_and_extract(&program_text, cli.extract.is_some());

    let mut full_code = module_prelude(&cli);
    full_code.push_str(&code);

    // `.pec` bytecode cache fast path — skip parse AND compile on warm starts.
    //
    // Keyed on (crate version, filename, full source including `-M` prelude). Enabled by
    // `PERLRS_BC_CACHE=1` (opt-in for v1 — see [`perlrs::pec::cache_enabled`]). On a hit,
    // the [`perlrs::pec::PecBundle`] carries both the AST `Program` and the compiled
    // `Chunk`; we hand the chunk to the interpreter via a sideband field that
    // [`perlrs::try_vm_execute`] consumes. On a miss, we parse normally and stash the
    // fingerprint so the try-VM path persists the freshly-compiled chunk after run.
    //
    // **Disabled for `-e` / `-E` one-liners.** Measured: warm `.pec` is ~2-3× *slower* than
    // cold for tiny scripts because the deserialize cost (~1-2 ms for fs read + zstd decode
    // + bincode) dominates the parse+compile work it replaces (~500 µs). One-liners would
    // also pollute the cache directory with one entry per unique `-e` invocation, with no
    // GC in v1. The break-even is around 1000+ lines, so file-based scripts only.
    let is_one_liner = !cli.execute.is_empty() || !cli.execute_features.is_empty();
    let pec_on = perlrs::pec::cache_enabled()
        && !cli.line_mode
        && !cli.print_mode
        && !cli.lint
        && !cli.check_only
        && !cli.dump_ast
        && !cli.format_source
        && !cli.profile
        && !is_one_liner
        && !filename.is_empty();
    let pec_fp_opt: Option<[u8; 32]> = if pec_on {
        // `strict_vars` enters the fingerprint as `false` here; an eventual [`PecBundle::strict_vars`]
        // mismatch at load time is treated as a miss (see [`perlrs::pec::try_load`]), so two strict
        // modes may collide in one slot without producing wrong answers.
        Some(perlrs::pec::source_fingerprint(
            false, &filename, &full_code,
        ))
    } else {
        None
    };
    let cached_bundle = pec_fp_opt
        .as_ref()
        .and_then(|fp| perlrs::pec::try_load(fp, false).ok().flatten());

    let (program, pec_precompiled) = if let Some(bundle) = cached_bundle {
        (bundle.program, Some(bundle.chunk))
    } else {
        let parsed = match perlrs::parse_with_file(&full_code, &filename) {
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
                eprintln!("perlrs: failed to serialize AST to JSON: {}", e);
                process::exit(1);
            }
        }
        return;
    }

    if cli.format_source {
        println!("{}", perlrs::format_program(&program));
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
        match perlrs::lint_program(&program, &mut interp) {
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
    if cli.profile {
        interp.profiler = Some(perlrs::profiler::Profiler::new(filename.clone()));
    }
    // Hand the `.pec` sideband to the interpreter so `try_vm_execute` either runs the
    // pre-compiled chunk (cache hit) or saves the freshly-compiled one (cache miss).
    interp.pec_precompiled_chunk = pec_precompiled;
    interp.pec_cache_fingerprint = if pec_on { pec_fp_opt } else { None };
    configure_interpreter(&cli, &mut interp, &filename);
    if let Some(data) = data_opt {
        interp.install_data_handle(data);
    }

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
                p.print_report();
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
                p.print_report();
            }
            if let ErrorKind::Exit(code) = e.kind {
                process::exit(code);
            }
            eprintln!("{}", e);
            process::exit(255);
        }
        if let Err(e) = interp.run_end_blocks() {
            if let Some(mut p) = interp.profiler.take() {
                p.print_report();
            }
            if let ErrorKind::Exit(code) = e.kind {
                process::exit(code);
            }
            eprintln!("{}", e);
            process::exit(255);
        }
        let _ = interp.run_global_teardown();
        if let Some(mut p) = interp.profiler.take() {
            p.print_report();
        }
    } else {
        // Normal execution
        match interp.execute(&program) {
            Ok(_) => {
                let _ = interp.run_global_teardown();
                let _ = io::stdout().flush();
                if let Some(mut p) = interp.profiler.take() {
                    p.print_report();
                }
            }
            Err(e) => match e.kind {
                ErrorKind::Exit(code) => {
                    if let Some(mut p) = interp.profiler.take() {
                        p.print_report();
                    }
                    process::exit(code);
                }
                ErrorKind::Die => {
                    if let Some(mut p) = interp.profiler.take() {
                        p.print_report();
                    }
                    eprint!("{}", e);
                    process::exit(255);
                }
                _ => {
                    if let Some(mut p) = interp.profiler.take() {
                        p.print_report();
                    }
                    eprintln!("{}", e);
                    process::exit(255);
                }
            },
        }
    }
}

/// Run an [`perlrs::aot::EmbeddedScript`] as if it were the primary program. Minimal
/// `@INC` setup: current directory only — the AOT binary is meant to be self-contained, so
/// the target machine's `perl` (which may not exist) is not consulted. `-I` at build time
/// is not yet supported (v1); drop everything into the `rust { ... }` block instead.
fn run_embedded_script(embedded: perlrs::aot::EmbeddedScript, argv: Vec<String>) -> i32 {
    // AOT binaries pick up the `.pec` bytecode cache for free when `PERLRS_BC_CACHE=1` —
    // the first run of a shipped binary parses and compiles the embedded source, then
    // every subsequent run reuses the cached chunk. Cache key includes the script name
    // embedded in the trailer, so two binaries with different embedded scripts will not
    // collide.
    let pec_on = perlrs::pec::cache_enabled();
    let pec_fp = if pec_on {
        Some(perlrs::pec::source_fingerprint(
            false,
            &embedded.name,
            &embedded.source,
        ))
    } else {
        None
    };
    let cached = pec_fp
        .as_ref()
        .and_then(|fp| perlrs::pec::try_load(fp, false).ok().flatten());
    let (program, pec_precompiled) = if let Some(bundle) = cached {
        (bundle.program, Some(bundle.chunk))
    } else {
        let parsed = match perlrs::parse_with_file(&embedded.source, &embedded.name) {
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
            .map(perlrs::value::PerlValue::string)
            .collect(),
    );
    interp.scope.declare_array(
        "INC",
        vec![perlrs::value::PerlValue::string(".".to_string())],
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

/// `pe build SCRIPT [-o OUT]` — compile a Perl script into a standalone binary by
/// copying the currently-running `pe` and appending a zstd-compressed source trailer.
/// The resulting file behaves as a native program: all CLI args go to the embedded script.
fn run_build_subcommand(args: &[String]) -> i32 {
    let mut script: Option<String> = None;
    let mut out: Option<String> = None;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("pe build: -o requires an argument");
                    return 2;
                }
                out = Some(args[i].clone());
            }
            "-h" | "--help" => {
                println!("usage: pe build SCRIPT [-o OUTPUT]");
                println!();
                println!(
                    "Compile a Perl script into a standalone executable binary. The output is"
                );
                println!(
                    "a copy of this pe binary with the script source embedded as a compressed"
                );
                println!(
                    "trailer. `scp` the result to any compatible machine and run it directly —"
                );
                println!("no perl, no perlrs, no @INC setup required.");
                println!();
                println!("Examples:");
                println!("  pe build app.pl                     # → ./app");
                println!("  pe build app.pl -o /usr/local/bin/app");
                return 0;
            }
            s if script.is_none() && !s.starts_with('-') => script = Some(s.to_string()),
            other => {
                eprintln!("pe build: unknown argument: {}", other);
                eprintln!("usage: pe build SCRIPT [-o OUTPUT]");
                return 2;
            }
        }
        i += 1;
    }
    let Some(script) = script else {
        eprintln!("pe build: missing SCRIPT");
        eprintln!("usage: pe build SCRIPT [-o OUTPUT]");
        return 2;
    };
    let script_path = PathBuf::from(&script);
    let out_path = PathBuf::from(out.unwrap_or_else(|| {
        script_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "a.out".to_string())
    }));
    match perlrs::aot::build(&script_path, &out_path) {
        Ok(p) => {
            eprintln!("pe build: wrote {}", p.display());
            0
        }
        Err(e) => {
            eprintln!("{}", e);
            1
        }
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
            "perlpath" => "perlrs".to_string(),
            _ => {
                eprintln!("Unknown config variable: {}", var);
                return;
            }
        };
        println!("{}='{}'", var, val);
    } else {
        println!("Summary of perlrs v{} configuration:\n", version);
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
        println!("    perlpath=perlrs");
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
        let a = args(&["perlrs", "s.pl", "--regex", "--foo"]);
        let cli = parse_cli_prelude(&a).expect("expected prelude parse");
        assert_eq!(cli.script.as_deref(), Some("s.pl"));
        assert_eq!(cli.args, vec!["--regex".to_string(), "--foo".to_string()]);
    }

    #[test]
    fn prelude_with_dash_w_before_script() {
        let a = args(&["perlrs", "-w", "s.pl", "--regex"]);
        let cli = parse_cli_prelude(&a).expect("expected prelude parse");
        assert!(cli.warnings);
        assert_eq!(cli.script.as_deref(), Some("s.pl"));
        assert_eq!(cli.args, vec!["--regex".to_string()]);
    }

    #[test]
    fn prelude_dash_e_then_argv_with_long_flag() {
        let a = args(&["perlrs", "-e", "1", "foo", "--regex"]);
        let mut cli = parse_cli_prelude(&a).expect("expected prelude parse");
        normalize_argv_after_dash_e(&mut cli);
        assert_eq!(cli.execute, vec!["1"]);
        assert!(cli.script.is_none());
        assert_eq!(cli.args, vec!["foo".to_string(), "--regex".to_string()]);
    }

    #[test]
    fn explicit_user_double_dash_skips_prelude() {
        let a = args(&["perlrs", "--", "s.pl", "x"]);
        assert!(parse_cli_prelude(&a).is_none());
    }

    #[test]
    fn bundled_lane_le_lne_maps_to_split_switches() {
        for (flag, code, expect_a, expect_n) in [
            ("-lane", "print 1", true, true),
            ("-le", "print 2", false, false),
            ("-lne", "print 3", false, true),
            ("-lnE", "say 4", false, true),
        ] {
            let a = expand_perl_bundled_argv(args(&["perlrs", flag, code]));
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
        let a = expand_perl_bundled_argv(args(&["perlrs", "-lpe", "print 1"]));
        let cli = Cli::try_parse_from(&a).expect("parse");
        assert!(cli.print_mode);
        assert_eq!(cli.execute, vec!["print 1"]);
    }

    #[test]
    fn bundled_0777_not_split() {
        let a = expand_perl_bundled_argv(args(&["perlrs", "-0777", "-e", "1"]));
        assert!(
            a.contains(&"-0777".to_string()),
            "expected -0777 kept intact: {a:?}"
        );
    }

    #[test]
    fn bundled_0ne_splits_like_perl() {
        let a = expand_perl_bundled_argv(args(&["perlrs", "-0ne", "print 1"]));
        let cli = Cli::try_parse_from(&a).expect("parse");
        assert_eq!(cli.execute, vec!["print 1"]);
        assert!(cli.line_mode);
    }

    #[test]
    fn bundled_f_colon_takes_rest_of_token() {
        let a = expand_perl_bundled_argv(args(&["perlrs", "-F:", "-anE", "say $F[0]"]));
        let cli = Cli::try_parse_from(&a).expect("parse");
        assert_eq!(cli.field_separator.as_deref(), Some(":"));
        assert!(cli.auto_split);
        assert!(cli.line_mode);
        assert_eq!(cli.execute_features, vec!["say $F[0]"]);
    }

    #[test]
    fn bundled_f_comma_takes_rest_of_token() {
        let a = expand_perl_bundled_argv(args(&["perlrs", "-F,", "-anE", "print 1"]));
        let cli = Cli::try_parse_from(&a).expect("parse");
        assert_eq!(cli.field_separator.as_deref(), Some(","));
    }

    #[test]
    fn help_alias_not_bundled_as_h_e_l_p() {
        let a = expand_perl_bundled_argv(args(&["perlrs", "-help"]));
        let cli = Cli::try_parse_from(&a).expect("parse");
        assert!(cli.help);
    }
}
