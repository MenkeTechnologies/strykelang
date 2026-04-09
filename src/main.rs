use std::fs::File;
use std::io::{self, BufRead, BufReader, IsTerminal, Read as IoRead, Write};
use std::path::{Path, PathBuf};
use std::process;

use clap::Parser;
use rand::Rng;

use perlrs::ast::Program;
use perlrs::error::{ErrorKind, PerlError};
use perlrs::interpreter::Interpreter;

mod repl;

/// perlrs — A highly parallel Perl 5 interpreter written in Rust
#[derive(Parser, Debug)]
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

    /// Check syntax only (runs BEGIN and CHECK blocks)
    #[arg(short = 'c')]
    check_only: bool,

    /// Dump the parsed abstract syntax tree as JSON to stdout and exit (no execution)
    #[arg(long = "ast")]
    dump_ast: bool,

    /// Pretty-print parsed Perl to stdout and exit (no execution)
    #[arg(long = "fmt")]
    format_source: bool,

    /// Wall-clock profile: per-line and per-sub timings on stderr (tree-walker only)
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

fn print_cyberpunk_help() {
    let version = env!("CARGO_PKG_VERSION");
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
    println!("{Y}  USAGE:{N} pe [switches] [--] [programfile] [arguments]");
    println!();
    println!("{C}  ── EXECUTION ──────────────────────────────────────────{N}");
    println!("  -e CODE                {G}//{N} One line of program (several -e's allowed)");
    println!("  -E CODE                {G}//{N} Like -e, but enables all optional features");
    println!("  -c                     {G}//{N} Check syntax only (runs BEGIN and CHECK blocks)");
    println!("  --ast                  {G}//{N} Dump parsed AST as JSON and exit (no execution)");
    println!("  --fmt                  {G}//{N} Pretty-print parsed Perl to stdout and exit");
    println!("  --profile              {G}//{N} Wall-clock profile (stderr); tree-walker only");
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
    println!("  -i[extension]          {G}//{N} Edit <> files in place (backup if ext supplied)");
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
        "  pipeline @list ->filter/map/take/collect {G}//{N} Lazy iterator (runs on collect); chain ->pmap/pgrep/pfor/pmap_chunked/psort/pcache/preduce/… like top-level p*"
    );
    println!(
        "  par_pipeline @list same chain; filter/map parallel on collect (order kept); par_pipeline(source=>…,stages=>…,workers=>…) channel stages"
    );
    println!(
        "  async {{BLOCK}}           {G}//{N} Run block on a worker thread; returns a task handle"
    );
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
        "  reduce {{BLOCK}} @list   {G}//{N} Sequential left fold ($a accum, $b next element)"
    );
    println!(
        "  preduce {{BLOCK}} @list  {G}//{N} Parallel tree fold (rayon; associative ops only)"
    );
    println!(
        "  preduce_init EXPR, {{BLOCK}} @list  {G}//{N} Parallel fold with identity; hash accumulators merge by key"
    );
    println!(
        "  fan [N] {{BLOCK}} [, progress => EXPR]  {G}//{N} Execute BLOCK N times (default N = rayon pool; $_ = index)"
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

    if slurp {
        if use_argv_files {
            for path in interp.argv.clone() {
                interp.line_number = 0;
                interp.argv_current_file = path.clone();
                let content = std::fs::read_to_string(&path).map_err(|e| {
                    PerlError::new(
                        ErrorKind::IO,
                        format!("Can't open {}: {}", path, e),
                        0,
                        "-e",
                    )
                })?;
                if let Some(output) = interp.process_line(&content, program)? {
                    if inplace {
                        commit_in_place_edit(Path::new(&path), &interp.inplace_edit, &output)
                            .map_err(|e| PerlError::new(ErrorKind::IO, e.to_string(), 0, "-e"))?;
                    } else if cli.print_mode {
                        print!("{}", output);
                        let _ = io::stdout().flush();
                    }
                }
            }
        } else {
            let mut input = String::new();
            io::stdin().read_to_string(&mut input).ok();
            if let Some(output) = interp.process_line(&input, program)? {
                if print_to_stdout {
                    print!("{}", output);
                    let _ = io::stdout().flush();
                }
            }
        }
        return Ok(());
    }

    if use_argv_files {
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
            let reader = BufReader::new(file);
            let mut accumulated = String::new();
            for line in reader.lines() {
                let l = line.map_err(|e| {
                    PerlError::new(
                        ErrorKind::IO,
                        format!("Error reading {}: {}", path, e),
                        0,
                        "-e",
                    )
                })?;
                let input = format!("{}\n", l);
                if let Some(output) = interp.process_line(&input, program)? {
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
    } else {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(l) => {
                    let input = format!("{}\n", l);
                    match interp.process_line(&input, program) {
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
                Err(e) => {
                    eprintln!("Error reading input: {}", e);
                    break;
                }
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
    let mut cli = Cli::parse();
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

    let is_repl = env!("CARGO_BIN_NAME") == "pe"
        && cli.script.is_none()
        && cli.execute.is_empty()
        && cli.execute_features.is_empty()
        && !cli.line_mode
        && !cli.print_mode
        && !cli.check_only
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
        let script_path = if cli.path_lookup {
            find_in_path(script).unwrap_or_else(|| script.clone())
        } else {
            script.clone()
        };
        match std::fs::read_to_string(&script_path) {
            Ok(content) => (content, script_path),
            Err(e) => {
                eprintln!("Can't open perl script \"{}\": {}", script_path, e);
                process::exit(2);
            }
        }
    } else if cli.line_mode || cli.print_mode {
        (String::new(), "-".to_string())
    } else {
        let mut code = String::new();
        io::stdin().read_line(&mut code).ok();
        (code, "-".to_string())
    };

    let (program_text, data_opt) = perlrs::data_section::split_data_section(&raw_script);
    let code = strip_shebang_and_extract(&program_text, cli.extract.is_some());

    let mut full_code = module_prelude(&cli);
    full_code.push_str(&code);

    // Parse
    let program = match perlrs::parse(&full_code) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(255);
        }
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
    if cli.profile {
        interp.profiler = Some(perlrs::profiler::Profiler::new(filename.clone()));
    }
    configure_interpreter(&cli, &mut interp, &filename);
    if let Some(data) = data_opt {
        interp.install_data_handle(data);
    }

    // Line processing mode (-n / -p)
    if cli.line_mode || cli.print_mode {
        if cli.line_ending.is_some() {
            interp.ors = "\n".to_string();
        }

        // First execute the program to register subs/BEGIN blocks
        if let Err(e) = interp.execute(&program) {
            if let Some(p) = interp.profiler.take() {
                p.print_report();
            }
            if let ErrorKind::Exit(code) = e.kind {
                process::exit(code);
            }
            eprintln!("{}", e);
            process::exit(255);
        }

        if let Err(e) = run_line_mode_loop(&cli, &mut interp, &program, slurp) {
            if let Some(p) = interp.profiler.take() {
                p.print_report();
            }
            if let ErrorKind::Exit(code) = e.kind {
                process::exit(code);
            }
            eprintln!("{}", e);
            process::exit(255);
        }
        if let Some(p) = interp.profiler.take() {
            p.print_report();
        }
    } else {
        // Normal execution
        match interp.execute(&program) {
            Ok(_) => {
                if let Some(p) = interp.profiler.take() {
                    p.print_report();
                }
            }
            Err(e) => match e.kind {
                ErrorKind::Exit(code) => {
                    if let Some(p) = interp.profiler.take() {
                        p.print_report();
                    }
                    process::exit(code);
                }
                ErrorKind::Die => {
                    if let Some(p) = interp.profiler.take() {
                        p.print_report();
                    }
                    eprint!("{}", e);
                    process::exit(255);
                }
                _ => {
                    if let Some(p) = interp.profiler.take() {
                        p.print_report();
                    }
                    eprintln!("{}", e);
                    process::exit(255);
                }
            },
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
