use std::io::{self, BufRead, Write};
use std::process;

use clap::Parser;

use perlrs::error::ErrorKind;
use perlrs::interpreter::Interpreter;

/// perlrs — A highly parallel Perl 5 interpreter written in Rust
#[derive(Parser, Debug)]
#[command(name = "perlrs", version, about, long_about = None)]
#[command(
    after_help = "Parallel extensions:\n  \
        pmap { BLOCK } @list    Parallel map (rayon)\n  \
        pgrep { BLOCK } @list   Parallel grep (rayon)\n  \
        pfor { BLOCK } @list    Parallel foreach (rayon)\n  \
        psort { BLOCK } @list   Parallel sort (rayon)\n\n\
        Examples:\n  \
        perlrs -e 'print \"Hello, world!\\n\"'\n  \
        perlrs -e 'my @r = pmap { $_ * 2 } 1..1000000; print scalar @r, \"\\n\"'\n  \
        perlrs script.pl arg1 arg2\n  \
        echo 'data' | perlrs -ne 'print uc $_'"
)]
struct Cli {
    /// Execute a single line of Perl code
    #[arg(short = 'e')]
    execute: Vec<String>,

    /// Like -e but enables all optional features
    #[arg(short = 'E')]
    execute_features: Vec<String>,

    /// Process input line by line (wraps code in while(<>){ ... })
    #[arg(short = 'n')]
    line_mode: bool,

    /// Like -n but also prints $_ after each iteration
    #[arg(short = 'p')]
    print_mode: bool,

    /// Edit files in place (with optional backup extension)
    #[arg(short = 'i', value_name = "EXT")]
    inplace: Option<Option<String>>,

    /// Enable warnings
    #[arg(short = 'w')]
    warnings: bool,

    /// Enable all warnings
    #[arg(short = 'W')]
    all_warnings: bool,

    /// Check syntax only (don't execute)
    #[arg(short = 'c')]
    check_only: bool,

    /// Automatic line-end processing (chomp + add newline on print)
    #[arg(short = 'l')]
    line_ending: bool,

    /// Auto-split mode (populate @F)
    #[arg(short = 'a')]
    auto_split: bool,

    /// Field separator pattern for -a
    #[arg(short = 'F', value_name = "PATTERN")]
    field_separator: Option<String>,

    /// Input record separator (as octal or hex)
    #[arg(short = '0', value_name = "DIGITS")]
    input_separator: Option<String>,

    /// Print version
    #[arg(short = 'v')]
    show_version: bool,

    /// Add directory to @INC
    #[arg(short = 'I', value_name = "DIR")]
    include: Vec<String>,

    /// Load module before executing (use module)
    #[arg(short = 'M', value_name = "MODULE")]
    use_module: Vec<String>,

    /// Load module before executing (without import)
    #[arg(short = 'm', value_name = "MODULE")]
    use_module_no_import: Vec<String>,

    /// Number of threads for parallel operations
    #[arg(short = 'j', long = "threads", value_name = "N")]
    threads: Option<usize>,

    /// Script file to execute
    #[arg(value_name = "SCRIPT")]
    script: Option<String>,

    /// Arguments passed to the script (@ARGV)
    #[arg(value_name = "ARGS", trailing_var_arg = true)]
    args: Vec<String>,
}

fn main() {
    let cli = Cli::parse();

    if cli.show_version {
        println!(
            "perlrs v{} — A highly parallel Perl 5 interpreter (Rust)",
            env!("CARGO_PKG_VERSION")
        );
        println!("Built with rayon for parallel map/grep/for/sort");
        println!("Threads: {}", rayon::current_num_threads());
        return;
    }

    // Configure rayon thread pool
    if let Some(n) = cli.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global()
            .ok();
    }

    // Build the source code
    let (code, filename) = if !cli.execute.is_empty() {
        (cli.execute.join("; "), "-e".to_string())
    } else if !cli.execute_features.is_empty() {
        (cli.execute_features.join("; "), "-E".to_string())
    } else if let Some(ref script) = cli.script {
        match std::fs::read_to_string(script) {
            Ok(content) => {
                // Strip shebang
                let code = if content.starts_with("#!") {
                    if let Some(pos) = content.find('\n') {
                        content[pos + 1..].to_string()
                    } else {
                        String::new()
                    }
                } else {
                    content
                };
                (code, script.clone())
            }
            Err(e) => {
                eprintln!("Can't open perl script \"{}\": {}", script, e);
                process::exit(2);
            }
        }
    } else if cli.line_mode || cli.print_mode {
        // No code but -n/-p: read from stdin and execute empty program
        (String::new(), "-".to_string())
    } else {
        // Read from stdin
        let mut code = String::new();
        io::stdin().read_line(&mut code).ok();
        (code, "-".to_string())
    };

    // Prepend module loads
    let mut full_code = String::new();
    for module in &cli.use_module {
        full_code.push_str(&format!("use {};\n", module));
    }
    for module in &cli.use_module_no_import {
        full_code.push_str(&format!("use {} ();\n", module));
    }
    full_code.push_str(&code);

    // Parse
    let program = match perlrs::parse(&full_code) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(255);
        }
    };

    if cli.check_only {
        eprintln!("{} syntax OK", filename);
        return;
    }

    // Create interpreter
    let mut interp = Interpreter::new();
    interp.set_file(&filename);
    interp.warnings = cli.warnings || cli.all_warnings;
    interp.auto_split = cli.auto_split;
    interp.field_separator = cli.field_separator.clone();
    interp.program_name = filename.clone();

    // Set @ARGV
    let argv: Vec<String> = if cli.script.is_some() {
        cli.args.clone()
    } else {
        Vec::new()
    };
    interp.argv = argv.clone();
    interp
        .scope
        .declare_array("ARGV", argv.into_iter().map(perlrs::value::PerlValue::String).collect());

    // Set %ENV
    for (k, v) in &interp.env.clone() {
        interp.scope.set_hash_element("ENV", k, v.clone());
    }

    // Line processing mode (-n / -p)
    if cli.line_mode || cli.print_mode {
        if cli.line_ending {
            interp.ors = "\n".to_string();
        }

        // First execute the program to register subs/BEGIN blocks
        if let Err(e) = interp.execute(&program) {
            if !matches!(e.kind, ErrorKind::Exit(_)) {
                eprintln!("{}", e);
                process::exit(255);
            }
        }

        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(l) => {
                    let input = format!("{}\n", l);
                    match interp.process_line(&input, &program) {
                        Ok(Some(output)) => {
                            if cli.print_mode {
                                print!("{}", output);
                                let _ = io::stdout().flush();
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            if let ErrorKind::Exit(code) = e.kind {
                                process::exit(code);
                            }
                            eprintln!("{}", e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error reading input: {}", e);
                    break;
                }
            }
        }
    } else {
        // Normal execution
        match interp.execute(&program) {
            Ok(_) => {}
            Err(e) => {
                match e.kind {
                    ErrorKind::Exit(code) => process::exit(code),
                    ErrorKind::Die => {
                        eprint!("{}", e);
                        process::exit(255);
                    }
                    _ => {
                        eprintln!("{}", e);
                        process::exit(255);
                    }
                }
            }
        }
    }
}
