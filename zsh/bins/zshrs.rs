//! zshrs - The most powerful shell ever created
//!
//! A drop-in zsh replacement that combines:
//! - Full bash/zsh script compatibility  
//! - Fish-quality completions with SQLite indexing
//! - Fish-style syntax highlighting and autosuggestions
//!
//! Copyright (C) 2026 MenkeTechnologies
//! License: GPL-2.0 (incorporates code from fish-shell)

use std::env;
use std::io::{self, BufRead};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use nu_ansi_term::{Color as AnsiColor, Style as AnsiStyle};
use reedline::Color;
use reedline::{
    default_emacs_keybindings, ColumnarMenu, Completer, DefaultHinter, Emacs, FileBackedHistory,
    Highlighter, KeyCode, KeyModifiers, MenuBuilder, Prompt, PromptHistorySearch,
    PromptHistorySearchStatus, Reedline, ReedlineEvent, ReedlineMenu, Signal, Span, StyledText,
    Suggestion, ValidationResult, Validator,
};

use zsh::exec::ShellExecutor;
use zsh::history::HistoryEngine;
use zsh::zwc;

use compsys::{
    build_cache_from_fpath, cache::CompsysCache, compinit_lazy, do_completion, get_system_fpath,
    Completion as CompsysCompletion, CompletionState,
};

use zsh::{
    highlight_shell, validate_command, with_abbrs_mut, AbbrPosition, Abbreviation, HighlightRole,
    ValidationStatus,
};

/// Print help message identical to zsh --help
fn print_help() {
    println!(
        r#"Usage: zsh [<options>] [<argument> ...]

Special options:
  --help       show this message, then exit
  --version    show zsh version number, then exit
  --zsh-compat enable zsh compatibility mode (use .zcompdump, fpath scanning)
  -b           end option processing, like --
  -c           take first argument as a command to execute
  -o OPTION    set an option by name (see below)

Normal options are named.  An option may be turned on by
`-o OPTION', `--OPTION', `+o no_OPTION' or `+-no-OPTION'.  An
option may be turned off by `-o no_OPTION', `--no-OPTION',
`+o OPTION' or `+-OPTION'.  Options are listed below only in
`--OPTION' or `--no-OPTION' form.

Named options:
  --aliases
  --aliasfuncdef
  --allexport
  --alwayslastprompt
  --alwaystoend
  --appendcreate
  --appendhistory
  --autocd
  --autocontinue
  --autolist
  --automenu
  --autonamedirs
  --autoparamkeys
  --autoparamslash
  --autopushd
  --autoremoveslash
  --autoresume
  --badpattern
  --banghist
  --bareglobqual
  --bashautolist
  --bashrematch
  --beep
  --bgnice
  --braceccl
  --bsdecho
  --caseglob
  --casematch
  --casepaths
  --cbases
  --cdablevars
  --cdsilent
  --chasedots
  --chaselinks
  --checkjobs
  --checkrunningjobs
  --clobber
  --clobberempty
  --combiningchars
  --completealiases
  --completeinword
  --continueonerror
  --correct
  --correctall
  --cprecedences
  --cshjunkiehistory
  --cshjunkieloops
  --cshjunkiequotes
  --cshnullcmd
  --cshnullglob
  --debugbeforecmd
  --dvorak
  --emacs
  --equals
  --errexit
  --errreturn
  --evallineno
  --exec
  --extendedglob
  --extendedhistory
  --flowcontrol
  --forcefloat
  --functionargzero
  --glob
  --globalexport
  --globalrcs
  --globassign
  --globcomplete
  --globdots
  --globstarshort
  --globsubst
  --hashcmds
  --hashdirs
  --hashexecutablesonly
  --hashlistall
  --histallowclobber
  --histbeep
  --histexpiredupsfirst
  --histfcntllock
  --histfindnodups
  --histignorealldups
  --histignoredups
  --histignorespace
  --histlexwords
  --histnofunctions
  --histnostore
  --histreduceblanks
  --histsavebycopy
  --histsavenodups
  --histsubstpattern
  --histverify
  --hup
  --ignorebraces
  --ignoreclosebraces
  --ignoreeof
  --incappendhistory
  --incappendhistorytime
  --interactive
  --interactivecomments
  --ksharrays
  --kshautoload
  --kshglob
  --kshoptionprint
  --kshtypeset
  --kshzerosubscript
  --listambiguous
  --listbeep
  --listpacked
  --listrowsfirst
  --listtypes
  --localloops
  --localoptions
  --localpatterns
  --localtraps
  --login
  --longlistjobs
  --magicequalsubst
  --mailwarning
  --markdirs
  --menucomplete
  --monitor
  --multibyte
  --multifuncdef
  --multios
  --nomatch
  --notify
  --nullglob
  --numericglobsort
  --octalzeroes
  --overstrike
  --pathdirs
  --pathscript
  --pipefail
  --posixaliases
  --posixargzero
  --posixbuiltins
  --posixcd
  --posixidentifiers
  --posixjobs
  --posixstrings
  --posixtraps
  --printeightbit
  --printexitvalue
  --privileged
  --promptbang
  --promptcr
  --promptpercent
  --promptsp
  --promptsubst
  --pushdignoredups
  --pushdminus
  --pushdsilent
  --pushdtohome
  --rcexpandparam
  --rcquotes
  --rcs
  --recexact
  --rematchpcre
  --restricted
  --rmstarsilent
  --rmstarwait
  --sharehistory
  --shfileexpansion
  --shglob
  --shinstdin
  --shnullcmd
  --shoptionletters
  --shortloops
  --shortrepeat
  --shwordsplit
  --singlecommand
  --singlelinezle
  --sourcetrace
  --sunkeyboardhack
  --transientrprompt
  --trapsasync
  --typesetsilent
  --typesettounset
  --unset
  --verbose
  --vi
  --warncreateglobal
  --warnnestedvar
  --xtrace
  --zle

Option aliases:
  --braceexpand            equivalent to --no-ignorebraces
  --dotglob                equivalent to --globdots
  --hashall                equivalent to --hashcmds
  --histappend             equivalent to --appendcreate
  --histexpand             equivalent to --badpattern
  --log                    equivalent to --no-histnofunctions
  --mailwarn               equivalent to --mailwarning
  --onecmd                 equivalent to --singlecommand
  --physical               equivalent to --cdsilent
  --promptvars             equivalent to --promptsubst
  --stdin                  equivalent to --shinstdin
  --trackall               equivalent to --hashcmds

Option letters:
  -0    equivalent to --completeinword
  -1    equivalent to --printexitvalue
  -2    equivalent to --no-autoresume
  -3    equivalent to --no-nomatch
  -4    equivalent to --globdots
  -5    equivalent to --notify
  -6    equivalent to --beep
  -7    equivalent to --ignoreeof
  -8    equivalent to --markdirs
  -9    equivalent to --autocontinue
  -B    equivalent to --no-bashrematch
  -C    equivalent to --no-checkjobs
  -D    equivalent to --pushdtohome
  -E    equivalent to --pushdsilent
  -F    equivalent to --no-glob
  -G    equivalent to --nullglob
  -H    equivalent to --rmstarsilent
  -I    equivalent to --ignorebraces
  -J    equivalent to --appendhistory
  -K    equivalent to --no-badpattern
  -L    equivalent to --sunkeyboardhack
  -M    equivalent to --singlelinezle
  -N    equivalent to --autoparamslash
  -O    equivalent to --continueonerror
  -P    equivalent to --rcexpandparam
  -Q    equivalent to --pathdirs
  -R    equivalent to --longlistjobs
  -S    equivalent to --recexact
  -T    equivalent to --cbases
  -U    equivalent to --mailwarning
  -V    equivalent to --no-promptcr
  -W    equivalent to --autoremoveslash
  -X    equivalent to --listtypes
  -Y    equivalent to --menucomplete
  -Z    equivalent to --zle
  -a    equivalent to --allexport
  -d    equivalent to --no-globalrcs
  -e    equivalent to --errexit
  -f    equivalent to --no-rcs
  -g    equivalent to --histignorespace
  -h    equivalent to --histignoredups
  -i    equivalent to --interactive
  -k    equivalent to --interactivecomments
  -l    equivalent to --login
  -m    equivalent to --monitor
  -n    equivalent to --no-exec
  -p    equivalent to --privileged
  -r    equivalent to --restricted
  -s    equivalent to --shinstdin
  -t    equivalent to --singlecommand
  -u    equivalent to --no-unset
  -v    equivalent to --verbose
  -w    equivalent to --cdsilent
  -x    equivalent to --xtrace
  -y    equivalent to --shwordsplit
"#
    );
}

/// Initialize default fish-style abbreviations
fn init_default_abbreviations() {
    with_abbrs_mut(|set| {
        // Git abbreviations (command position only)
        set.add(Abbreviation::new("g", "g", "git", AbbrPosition::Command));
        set.add(Abbreviation::new(
            "ga",
            "ga",
            "git add",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "gaa",
            "gaa",
            "git add --all",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "gc",
            "gc",
            "git commit",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "gcm",
            "gcm",
            "git commit -m",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "gco",
            "gco",
            "git checkout",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "gd",
            "gd",
            "git diff",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "gds",
            "gds",
            "git diff --staged",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "gl",
            "gl",
            "git log --oneline",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "gp",
            "gp",
            "git push",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "gpl",
            "gpl",
            "git pull",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "gs",
            "gs",
            "git status",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "gsw",
            "gsw",
            "git switch",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "gb",
            "gb",
            "git branch",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "gst",
            "gst",
            "git stash",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "grb",
            "grb",
            "git rebase",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "gm",
            "gm",
            "git merge",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "gf",
            "gf",
            "git fetch",
            AbbrPosition::Command,
        ));

        // Directory navigation
        set.add(Abbreviation::new(
            "...",
            "...",
            "../..",
            AbbrPosition::Anywhere,
        ));
        set.add(Abbreviation::new(
            "....",
            "....",
            "../../..",
            AbbrPosition::Anywhere,
        ));
        set.add(Abbreviation::new(
            ".....",
            ".....",
            "../../../..",
            AbbrPosition::Anywhere,
        ));

        // Common commands
        set.add(Abbreviation::new("l", "l", "ls -la", AbbrPosition::Command));
        set.add(Abbreviation::new(
            "ll",
            "ll",
            "ls -l",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "la",
            "la",
            "ls -la",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "md",
            "md",
            "mkdir -p",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "rd",
            "rd",
            "rmdir",
            AbbrPosition::Command,
        ));

        // Cargo/Rust
        set.add(Abbreviation::new(
            "cb",
            "cb",
            "cargo build",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "cr",
            "cr",
            "cargo run",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "ct",
            "ct",
            "cargo test",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "cc",
            "cc",
            "cargo check",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "cf",
            "cf",
            "cargo fmt",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "ccl",
            "ccl",
            "cargo clippy",
            AbbrPosition::Command,
        ));

        // Docker
        set.add(Abbreviation::new(
            "dc",
            "dc",
            "docker compose",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "dcu",
            "dcu",
            "docker compose up",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "dcd",
            "dcd",
            "docker compose down",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "dps",
            "dps",
            "docker ps",
            AbbrPosition::Command,
        ));

        // Kubernetes
        set.add(Abbreviation::new(
            "k",
            "k",
            "kubectl",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "kgp",
            "kgp",
            "kubectl get pods",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "kgs",
            "kgs",
            "kubectl get services",
            AbbrPosition::Command,
        ));
        set.add(Abbreviation::new(
            "kgd",
            "kgd",
            "kubectl get deployments",
            AbbrPosition::Command,
        ));
    });
}

/// Global compatibility mode flag
static mut ZSH_COMPAT_MODE: bool = false;

/// Check if zsh compatibility mode is enabled
pub fn is_zsh_compat() -> bool {
    unsafe { ZSH_COMPAT_MODE }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    // Handle --zsh-compat global flag (must be checked early)
    if args.iter().any(|a| a == "--zsh-compat") {
        unsafe {
            ZSH_COMPAT_MODE = true;
        }
    }

    // Handle --help (must be identical to zsh --help)
    if args.iter().any(|a| a == "--help") {
        print_help();
        return;
    }

    // Handle --version
    if args.iter().any(|a| a == "--version") {
        println!("zshrs {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // Handle --dump-zwc for debugging .zwc files
    if args.len() >= 3 && args[1] == "--dump-zwc" {
        if args.len() >= 4 {
            // Dump specific function
            if let Err(e) = zwc::dump_zwc_function(&args[2], &args[3]) {
                eprintln!("zshrs: {}: {}", args[2], e);
                std::process::exit(1);
            }
        } else {
            // List all functions
            if let Err(e) = zwc::dump_zwc_info(&args[2]) {
                eprintln!("zshrs: {}: {}", args[2], e);
                std::process::exit(1);
            }
        }
        return;
    }

    // Filter out flags that don't affect -c / script dispatch
    let args: Vec<String> = args
        .into_iter()
        .filter(|a| a != "--zsh-compat" && a != "-f" && a != "--no-rcs")
        .collect();

    // Handle -c 'command' syntax
    if args.len() >= 3 && args[1] == "-c" {
        let code = &args[2];

        let mut executor = ShellExecutor::new();
        executor.zsh_compat = is_zsh_compat();
        let start = Instant::now();
        let result = executor.execute_script(code);
        let duration = start.elapsed().as_millis() as i64;

        // Track in history
        if let Some(ref engine) = executor.history {
            let cwd = std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().to_string());
            if let Ok(id) = engine.add(code, cwd.as_deref()) {
                let _ = engine.update_last(id, duration, executor.last_status);
            }
        }

        if let Err(e) = result {
            eprintln!("zshrs: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // Handle script file argument
    if args.len() >= 2 && !args[1].starts_with('-') {
        let mut executor = ShellExecutor::new();
        executor.zsh_compat = is_zsh_compat();
        match std::fs::read_to_string(&args[1]) {
            Ok(script) => {
                if let Err(e) = executor.execute_script(&script) {
                    eprintln!("zshrs: {}: {}", args[1], e);
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("zshrs: {}: {}", args[1], e);
                std::process::exit(1);
            }
        }
        return;
    }

    // Check if stdin is a TTY
    if atty::is(atty::Stream::Stdin) {
        run_interactive();
    } else {
        run_non_interactive();
    }
}

fn run_non_interactive() {
    let mut executor = ShellExecutor::new();
    executor.zsh_compat = is_zsh_compat();
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        match line {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() || line == "exit" || line == "logout" {
                    continue;
                }
                process_line(line, &mut executor);
            }
            Err(_) => break,
        }
    }
}

fn get_zdotdir() -> PathBuf {
    std::env::var("ZDOTDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")))
}

/// Source zsh startup files in correct order per zshall(1) STARTUP/SHUTDOWN FILES
///
/// Behavior is controlled by RCS and GLOBAL_RCS options:
/// - RCS (default: on) - if unset, no startup files are read
/// - GLOBAL_RCS (default: on) - if unset, /etc/* files are skipped
///
/// Order for login shell:
///   1. /etc/zshenv (always, cannot be overridden - even with -f)
///   2. $ZDOTDIR/.zshenv
///   3. /etc/zprofile (login only)
///   4. $ZDOTDIR/.zprofile (login only)
///   5. /etc/zshrc (interactive only)
///   6. $ZDOTDIR/.zshrc (interactive only)
///   7. /etc/zlogin (login only)
///   8. $ZDOTDIR/.zlogin (login only)
///
/// If file.zwc exists and is newer than file, the compiled version is used.
fn source_startup_files(
    executor: &mut ShellExecutor,
    is_login: bool,
    is_interactive: bool,
    no_rcs: bool,
) {
    let zdotdir = get_zdotdir();

    // /etc/zshenv is ALWAYS read first (cannot be overridden, even with -f)
    source_file_with_zwc(executor, &PathBuf::from("/etc/zshenv"));

    // If -f (no_rcs) was passed, stop here
    if no_rcs {
        return;
    }

    // Check RCS option - if unset, skip remaining startup files
    if !executor.options.get("rcs").copied().unwrap_or(true) {
        return;
    }

    // $ZDOTDIR/.zshenv
    source_file_with_zwc(executor, &zdotdir.join(".zshenv"));

    // Re-check RCS after .zshenv (it could have unset it)
    if !executor.options.get("rcs").copied().unwrap_or(true) {
        return;
    }

    // Login shell: /etc/zprofile, $ZDOTDIR/.zprofile
    if is_login {
        if executor.options.get("globalrcs").copied().unwrap_or(true) {
            source_file_with_zwc(executor, &PathBuf::from("/etc/zprofile"));
        }
        if executor.options.get("rcs").copied().unwrap_or(true) {
            source_file_with_zwc(executor, &zdotdir.join(".zprofile"));
        }
    }

    // Re-check RCS
    if !executor.options.get("rcs").copied().unwrap_or(true) {
        return;
    }

    // Interactive shell: /etc/zshrc, $ZDOTDIR/.zshrc
    if is_interactive {
        if executor.options.get("globalrcs").copied().unwrap_or(true) {
            source_file_with_zwc(executor, &PathBuf::from("/etc/zshrc"));
        }
        if executor.options.get("rcs").copied().unwrap_or(true) {
            source_file_with_zwc(executor, &zdotdir.join(".zshrc"));
        }
    }

    // Re-check RCS
    if !executor.options.get("rcs").copied().unwrap_or(true) {
        return;
    }

    // Login shell: /etc/zlogin, $ZDOTDIR/.zlogin (after zshrc)
    if is_login {
        if executor.options.get("globalrcs").copied().unwrap_or(true) {
            source_file_with_zwc(executor, &PathBuf::from("/etc/zlogin"));
        }
        if executor.options.get("rcs").copied().unwrap_or(true) {
            source_file_with_zwc(executor, &zdotdir.join(".zlogin"));
        }
    }
}

/// Source a file
fn source_file_with_zwc(executor: &mut ShellExecutor, path: &PathBuf) {
    source_file(executor, path);
}

/// Source a single file, handling multi-line constructs
fn source_file(executor: &mut ShellExecutor, path: &PathBuf) {
    if !path.exists() {
        return;
    }

    if let Ok(contents) = std::fs::read_to_string(path) {
        let mut buffer = String::new();
        let mut in_multiline = false;

        for line in contents.lines() {
            let trimmed = line.trim();

            // Skip empty lines and comments (unless in multiline)
            if !in_multiline && (trimmed.is_empty() || trimmed.starts_with('#')) {
                continue;
            }

            // Check for line continuation
            if line.ends_with('\\') {
                buffer.push_str(&line[..line.len() - 1]);
                buffer.push(' ');
                in_multiline = true;
                continue;
            }

            // Check for unclosed constructs (heredoc, quotes, braces)
            if in_multiline {
                buffer.push_str(line);
                // Simple heuristic: if we have balanced braces/quotes, execute
                let open_braces = buffer.matches('{').count();
                let close_braces = buffer.matches('}').count();
                let open_parens = buffer.matches('(').count();
                let close_parens = buffer.matches(')').count();

                if open_braces == close_braces && open_parens == close_parens {
                    process_line(&buffer, executor);
                    buffer.clear();
                    in_multiline = false;
                } else {
                    buffer.push('\n');
                }
            } else {
                process_line(line, executor);
            }
        }

        // Process any remaining buffered content
        if !buffer.is_empty() {
            process_line(&buffer, executor);
        }
    }
}

/// Source logout files when shell exits (per zshall(1))
/// Only for login shells, respects RCS and GLOBAL_RCS options
#[allow(dead_code)]
fn source_logout_files(executor: &mut ShellExecutor, is_login: bool) {
    if !is_login {
        return;
    }

    // Check RCS option
    if !executor.options.get("rcs").copied().unwrap_or(true) {
        return;
    }

    let zdotdir = get_zdotdir();

    // $ZDOTDIR/.zlogout first
    source_file_with_zwc(executor, &zdotdir.join(".zlogout"));

    // /etc/zlogout (only if GLOBAL_RCS is set)
    if executor.options.get("globalrcs").copied().unwrap_or(true) {
        source_file_with_zwc(executor, &PathBuf::from("/etc/zlogout"));
    }
}

fn run_interactive() {
    // Set up signal handling
    let interrupted = Arc::new(AtomicBool::new(false));
    let i = interrupted.clone();
    ctrlc::set_handler(move || {
        i.store(true, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

    // Initialize fish-style abbreviations
    init_default_abbreviations();

    // Initialize compsys cache (single SQLite db for all completions)
    let cache_path = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("zshrs/compsys.db");
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let compsys_cache = match CompsysCache::open(&cache_path) {
        Ok(mut cache) => {
            // Index PATH executables on first run
            if !cache.has_executables().unwrap_or(false) {
                eprint!("Indexing completions... ");
                let path_var = std::env::var("PATH").unwrap_or_default();
                let mut executables = Vec::new();
                for dir in path_var.split(':') {
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        for entry in entries.flatten() {
                            if let Ok(ft) = entry.file_type() {
                                if ft.is_file() || ft.is_symlink() {
                                    if let Some(name) = entry.file_name().to_str() {
                                        executables.push((name.to_string(), dir.to_string()));
                                    }
                                }
                            }
                        }
                    }
                }
                let cmd_count = executables.len();
                let _ = cache.set_executables_bulk(&executables);
                eprintln!("{} commands indexed", cmd_count);
            }
            Some(cache)
        }
        Err(e) => {
            eprintln!("Warning: compsys cache failed to initialize: {e}");
            None
        }
    };

    // Initialize SQLite history engine for frequency tracking
    let history_engine = match HistoryEngine::new() {
        Ok(engine) => {
            let count = engine.count().unwrap_or(0);
            if count > 0 {
                eprintln!("Loaded {} history entries", count);
            }
            Some(engine)
        }
        Err(e) => {
            eprintln!("Warning: history engine failed to initialize: {e}");
            None
        }
    };

    let line_editor = setup_editor(compsys_cache);
    if line_editor.is_none() {
        eprintln!("Failed to initialize line editor");
        return;
    }
    let mut line_editor = line_editor.unwrap();

    let mut executor = ShellExecutor::new();
    executor.zsh_compat = is_zsh_compat();

    // Determine shell type from invocation per zshall(1)
    let args: Vec<String> = std::env::args().collect();

    // -f: don't source startup files (except /etc/zshenv which is ALWAYS read)
    let no_rcs = args.iter().any(|a| a == "-f" || a == "--no-rcs");

    // Login shell detection:
    // - explicit -l or --login flag
    // - invoked as -zshrs (name starts with -)
    // - $SHELL ends with zshrs (login shell)
    let is_login = args.iter().any(|a| a == "-l" || a == "--login")
        || args.first().map(|a| a.starts_with('-')).unwrap_or(false)
        || std::env::var("SHELL")
            .map(|s| s.ends_with("zshrs"))
            .unwrap_or(false);

    let is_interactive = true; // We're in run_interactive()

    // Set default options (RCS and GLOBAL_RCS are on by default)
    executor.options.insert("rcs".to_string(), true);
    executor.options.insert("globalrcs".to_string(), true);

    // Source startup files in correct zsh order per zshall(1)
    source_startup_files(&mut executor, is_login, is_interactive, no_rcs);

    println!("zshrs {}", env!("CARGO_PKG_VERSION"));
    println!("Type exit to quit\n");

    loop {
        let prompt = ZshrsPrompt::new(&executor);
        match line_editor.read_line(&prompt) {
            Ok(Signal::Success(line)) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                if line == "exit" || line == "logout" {
                    break;
                }

                let start = Instant::now();
                process_line(line, &mut executor);
                let duration = start.elapsed().as_millis() as i64;

                // Track in SQLite history with frequency
                if let Some(ref engine) = history_engine {
                    let cwd = std::env::current_dir()
                        .ok()
                        .map(|p| p.to_string_lossy().to_string());
                    if let Ok(id) = engine.add(line, cwd.as_deref()) {
                        let _ = engine.update_last(id, duration, executor.last_status);
                    }
                }
            }
            Ok(Signal::CtrlD) => {
                // EOF - exit shell
                executor.run_trap("EXIT");
                println!();
                break;
            }
            Ok(Signal::CtrlC) => {
                // Interrupt - run INT trap if set, otherwise just print newline
                interrupted.store(false, Ordering::SeqCst);
                executor.run_trap("INT");
                println!();
                continue;
            }
            Ok(_) => {
                // Handle any other signals
                continue;
            }
            Err(err) => {
                eprintln!("Error: {err}");
                break;
            }
        }
    }
}

fn process_line(line: &str, executor: &mut ShellExecutor) {
    if let Err(e) = executor.execute_script(line) {
        eprintln!("zshrs: {}", e);
    }
}

fn setup_editor(compsys_cache: Option<CompsysCache>) -> Option<Reedline> {
    let history_path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".zshrs_history");

    let history = Box::new(FileBackedHistory::with_file(10000, history_path).ok()?);

    let mut keybindings = default_emacs_keybindings();

    // Add Tab keybinding to trigger completion menu
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );

    let edit_mode = Box::new(Emacs::new(keybindings));

    let mut editor = Reedline::create()
        .with_history(history)
        .with_edit_mode(edit_mode)
        .with_hinter(Box::new(
            DefaultHinter::default().with_style(AnsiStyle::new().fg(AnsiColor::DarkGray)),
        ))
        .with_highlighter(Box::new(ZshrsHighlighter))
        .with_validator(Box::new(ZshrsValidator));

    if let Some(cache) = compsys_cache {
        let completer = Box::new(ZshrsCompleter::new(cache));

        // Create a completion menu (Tab triggers this)
        let completion_menu = Box::new(
            ColumnarMenu::default()
                .with_name("completion_menu")
                .with_columns(4)
                .with_column_padding(2),
        );

        editor = editor
            .with_completer(completer)
            .with_menu(ReedlineMenu::EngineCompleter(completion_menu));
    }

    Some(editor)
}

// ============================================================================
// SYNTAX HIGHLIGHTER - Fish-style real-time highlighting
// ============================================================================

struct ZshrsHighlighter;

impl Highlighter for ZshrsHighlighter {
    fn highlight(&self, line: &str, _cursor: usize) -> StyledText {
        let specs = highlight_shell(line);
        let mut styled = StyledText::new();

        if line.is_empty() {
            return styled;
        }

        let mut current_style = AnsiStyle::new();
        let mut current_text = String::new();
        let mut last_role = HighlightRole::Normal;

        for (i, c) in line.chars().enumerate() {
            let byte_pos = line.char_indices().nth(i).map(|(p, _)| p).unwrap_or(i);
            let role = specs
                .get(byte_pos)
                .map(|s| s.foreground)
                .unwrap_or(HighlightRole::Normal);

            if role != last_role && !current_text.is_empty() {
                styled.push((current_style, current_text.clone()));
                current_text.clear();
            }

            if role != last_role {
                current_style = role_to_style(role);
                last_role = role;
            }

            current_text.push(c);
        }

        if !current_text.is_empty() {
            styled.push((current_style, current_text));
        }

        styled
    }
}

fn role_to_style(role: HighlightRole) -> AnsiStyle {
    match role {
        HighlightRole::Normal => AnsiStyle::new(),
        HighlightRole::Command => AnsiStyle::new().fg(AnsiColor::Green).bold(),
        HighlightRole::Keyword => AnsiStyle::new().fg(AnsiColor::Blue).bold(),
        HighlightRole::Statement => AnsiStyle::new().fg(AnsiColor::Magenta).bold(),
        HighlightRole::Param => AnsiStyle::new(),
        HighlightRole::Option => AnsiStyle::new().fg(AnsiColor::Cyan),
        HighlightRole::Comment => AnsiStyle::new().fg(AnsiColor::DarkGray),
        HighlightRole::Error => AnsiStyle::new().fg(AnsiColor::Red).bold(),
        HighlightRole::String => AnsiStyle::new().fg(AnsiColor::Yellow),
        HighlightRole::Escape => AnsiStyle::new().fg(AnsiColor::Yellow).bold(),
        HighlightRole::Operator => AnsiStyle::new().fg(AnsiColor::White).bold(),
        HighlightRole::Redirection => AnsiStyle::new().fg(AnsiColor::Magenta),
        HighlightRole::Path => AnsiStyle::new().underline(),
        HighlightRole::PathValid => AnsiStyle::new().fg(AnsiColor::Green).underline(),
        HighlightRole::Autosuggestion => AnsiStyle::new().fg(AnsiColor::DarkGray),
        HighlightRole::Selection => AnsiStyle::new().reverse(),
        HighlightRole::Search => AnsiStyle::new().fg(AnsiColor::Black).on(AnsiColor::Yellow),
        HighlightRole::Variable => AnsiStyle::new().fg(AnsiColor::Cyan).bold(),
        HighlightRole::Quote => AnsiStyle::new().fg(AnsiColor::Yellow),
    }
}

// ============================================================================
// VALIDATOR - Multi-line support for incomplete commands
// ============================================================================

struct ZshrsValidator;

impl Validator for ZshrsValidator {
    fn validate(&self, line: &str) -> ValidationResult {
        match validate_command(line) {
            ValidationStatus::Valid => ValidationResult::Complete,
            ValidationStatus::Incomplete => ValidationResult::Incomplete,
            ValidationStatus::Invalid(_) => ValidationResult::Complete, // Let execution show the error
        }
    }
}

struct ZshrsCompleter {
    cache: CompsysCache,
    #[allow(dead_code)]
    comp_state: CompletionState,
}

impl ZshrsCompleter {
    fn new(mut cache: CompsysCache) -> Self {
        // Check if completion mappings need to be built
        let (valid, count) = compinit_lazy(&cache);
        if !valid || count == 0 {
            // Build cache from fpath
            let fpath = get_system_fpath();
            let _ = build_cache_from_fpath(&fpath, &mut cache);
        }

        Self {
            cache,
            comp_state: CompletionState::new(),
        }
    }

    /// Get completions for command options using compsys
    #[allow(dead_code)]
    fn complete_options(&mut self, cmd: &str, prefix: &str) -> Vec<CompsysCompletion> {
        // Use do_completion with a closure that adds known options
        let line = format!("{} {}", cmd, prefix);
        let cursor = line.len();

        self.comp_state = CompletionState::from_line(&line, cursor);

        let _nmatches = do_completion(&line, cursor, &mut self.comp_state, |state| {
            state.begin_group("options", true);

            match cmd {
                "ls" => {
                    for (opt, desc) in &[
                        ("-l", "long listing format"),
                        ("-a", "show hidden files"),
                        ("-h", "human readable sizes"),
                        ("-R", "recursive"),
                        ("-t", "sort by time"),
                        ("-S", "sort by size"),
                        ("-r", "reverse order"),
                        ("-1", "one entry per line"),
                        ("-d", "list directories themselves"),
                        ("-F", "append indicator"),
                        ("--color", "colorize output"),
                        ("--help", "display help"),
                    ] {
                        if opt.starts_with(prefix) || prefix.is_empty() {
                            let mut comp = CompsysCompletion::new(*opt);
                            comp.desc = Some(desc.to_string());
                            state.add_match(comp, Some("options"));
                        }
                    }
                }
                "git" => {
                    for (opt, desc) in &[
                        ("--version", "show version"),
                        ("--help", "show help"),
                        ("-C", "run as if started in path"),
                        ("-c", "pass configuration parameter"),
                        ("--exec-path", "path to git executables"),
                        ("--work-tree", "set working tree"),
                        ("--git-dir", "set git directory"),
                    ] {
                        if opt.starts_with(prefix) || prefix.is_empty() {
                            let mut comp = CompsysCompletion::new(*opt);
                            comp.desc = Some(desc.to_string());
                            state.add_match(comp, Some("options"));
                        }
                    }
                }
                "grep" | "rg" => {
                    for (opt, desc) in &[
                        ("-i", "ignore case"),
                        ("-v", "invert match"),
                        ("-r", "recursive"),
                        ("-n", "line numbers"),
                        ("-l", "files with matches"),
                        ("-c", "count matches"),
                        ("-E", "extended regex"),
                        ("-w", "whole words"),
                        ("-A", "lines after"),
                        ("-B", "lines before"),
                        ("-C", "lines context"),
                        ("--color", "colorize output"),
                    ] {
                        if opt.starts_with(prefix) || prefix.is_empty() {
                            let mut comp = CompsysCompletion::new(*opt);
                            comp.desc = Some(desc.to_string());
                            state.add_match(comp, Some("options"));
                        }
                    }
                }
                "cargo" => {
                    for (opt, desc) in &[
                        ("--help", "show help"),
                        ("--version", "show version"),
                        ("-V", "show version"),
                        ("-v", "verbose"),
                        ("-q", "quiet"),
                        ("--color", "colorize output"),
                        ("--frozen", "require lockfile up to date"),
                        ("--locked", "require lockfile matches"),
                        ("--offline", "run without network"),
                    ] {
                        if opt.starts_with(prefix) || prefix.is_empty() {
                            let mut comp = CompsysCompletion::new(*opt);
                            comp.desc = Some(desc.to_string());
                            state.add_match(comp, Some("options"));
                        }
                    }
                }
                _ => {
                    // Generic options for unknown commands
                    for opt in &["--help", "--version", "-h", "-v"] {
                        if opt.starts_with(prefix) || prefix.is_empty() {
                            state.add_match(CompsysCompletion::new(*opt), Some("options"));
                        }
                    }
                }
            }

            state.end_group();
        });

        // Collect all completions from groups
        let mut result = Vec::new();
        for group in &self.comp_state.groups {
            for comp in &group.matches {
                result.push(comp.clone());
            }
        }
        result
    }
}

impl Completer for ZshrsCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        let line_to_pos = &line[..pos];
        let word_start = line_to_pos
            .rfind(|c: char| c.is_whitespace())
            .map(|i| i + 1)
            .unwrap_or(0);
        let current_word = &line_to_pos[word_start..];
        let current_word = current_word.trim_start_matches('@');

        let words: Vec<&str> = line_to_pos.split_whitespace().collect();
        let is_first_word = words.len() <= 1 && !line_to_pos.ends_with(' ');

        let mut suggestions = Vec::new();

        if is_first_word {
            // Command position - complete executables, builtins, shell functions
            if !current_word.is_empty() {
                // Executables from cache
                if let Ok(executables) = self.cache.get_executables_prefix_fts(current_word) {
                    for (name, path) in executables.into_iter().take(100) {
                        suggestions.push(Suggestion {
                            value: name,
                            description: Some(path),
                            style: None,
                            extra: Some(vec!["command".to_string()]),
                            span: Span::new(word_start, pos),
                            append_whitespace: true,
                            display_override: None,
                            match_indices: None,
                        });
                    }
                }

                // Builtins
                let builtins = [
                    "alias",
                    "autoload",
                    "bg",
                    "bindkey",
                    "break",
                    "builtin",
                    "cd",
                    "command",
                    "compctl",
                    "continue",
                    "declare",
                    "dirs",
                    "disown",
                    "echo",
                    "emulate",
                    "enable",
                    "eval",
                    "exec",
                    "exit",
                    "export",
                    "false",
                    "fc",
                    "fg",
                    "float",
                    "functions",
                    "getopts",
                    "hash",
                    "history",
                    "integer",
                    "jobs",
                    "kill",
                    "let",
                    "limit",
                    "local",
                    "log",
                    "logout",
                    "noglob",
                    "popd",
                    "print",
                    "printf",
                    "pushd",
                    "pwd",
                    "read",
                    "readonly",
                    "rehash",
                    "return",
                    "set",
                    "setopt",
                    "shift",
                    "source",
                    "suspend",
                    "test",
                    "times",
                    "trap",
                    "true",
                    "type",
                    "typeset",
                    "ulimit",
                    "umask",
                    "unalias",
                    "unfunction",
                    "unhash",
                    "unlimit",
                    "unset",
                    "unsetopt",
                    "wait",
                    "whence",
                    "where",
                    "which",
                    "zcompile",
                    "zformat",
                    "zle",
                    "zmodload",
                    "zparseopts",
                    "zprof",
                    "zpty",
                    "zregexparse",
                    "zsocket",
                    "zstat",
                    "zstyle",
                ];
                let prefix_lower = current_word.to_lowercase();
                for builtin in builtins {
                    if builtin.starts_with(&prefix_lower) {
                        suggestions.push(Suggestion {
                            value: builtin.to_string(),
                            description: Some("builtin".to_string()),
                            style: None,
                            extra: Some(vec!["builtin".to_string()]),
                            span: Span::new(word_start, pos),
                            append_whitespace: true,
                            display_override: None,
                            match_indices: None,
                        });
                    }
                }

                // Shell functions from cache
                if let Ok(funcs) = self.cache.get_shell_functions_prefix(current_word) {
                    for (name, source) in funcs.into_iter().take(50) {
                        suggestions.push(Suggestion {
                            value: name,
                            description: Some(source),
                            style: None,
                            extra: Some(vec!["function".to_string()]),
                            span: Span::new(word_start, pos),
                            append_whitespace: true,
                            display_override: None,
                            match_indices: None,
                        });
                    }
                }
            }
        } else if current_word.starts_with('-') {
            // Option completion - use compsys cache to find options
            if let Some(cmd) = words.first() {
                // Try to get options from completion function in cache
                if let Ok(Some(func)) = self.cache.get_comp(*cmd) {
                    if let Ok(Some(stub)) = self.cache.get_autoload(&func) {
                        if let Ok(content) = std::fs::read_to_string(&stub.source) {
                            let prefix_lower = current_word.to_lowercase();
                            for line in content.lines() {
                                let line = line.trim();
                                if !line.contains('[') || line.starts_with('#') {
                                    continue;
                                }
                                for segment in line.split('\'') {
                                    if let Some((opt, desc)) = parse_option_spec(segment) {
                                        if opt.to_lowercase().starts_with(&prefix_lower) {
                                            suggestions.push(Suggestion {
                                                value: opt,
                                                description: if desc.is_empty() {
                                                    None
                                                } else {
                                                    Some(desc)
                                                },
                                                style: None,
                                                extra: Some(vec!["option".to_string()]),
                                                span: Span::new(word_start, pos),
                                                append_whitespace: true,
                                                display_override: None,
                                                match_indices: None,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Fallback: hardcoded options for common commands
                if suggestions.is_empty() {
                    let options: Vec<(&str, &str)> = match *cmd {
                        "ls" => vec![
                            ("-l", "long listing"),
                            ("-a", "show hidden"),
                            ("-h", "human sizes"),
                            ("-R", "recursive"),
                            ("-t", "sort by time"),
                            ("-S", "sort by size"),
                            ("-r", "reverse"),
                            ("-1", "one per line"),
                            ("-d", "directories"),
                            ("-F", "indicators"),
                            ("--color", "colorize"),
                            ("--help", "help"),
                        ],
                        "git" => vec![
                            ("--version", "version"),
                            ("--help", "help"),
                            ("-C", "path"),
                            ("-c", "config"),
                        ],
                        "grep" | "rg" => vec![
                            ("-i", "ignore case"),
                            ("-v", "invert"),
                            ("-r", "recursive"),
                            ("-n", "line numbers"),
                            ("-l", "files only"),
                            ("-c", "count"),
                        ],
                        "cargo" => vec![
                            ("--help", "help"),
                            ("--version", "version"),
                            ("-v", "verbose"),
                            ("-q", "quiet"),
                        ],
                        "cd" => vec![
                            ("-", "previous"),
                            ("-L", "follow symlinks"),
                            ("-P", "physical"),
                        ],
                        _ => vec![
                            ("--help", "help"),
                            ("--version", "version"),
                            ("-h", "help"),
                            ("-v", "verbose"),
                        ],
                    };
                    for (opt, desc) in options {
                        if opt.starts_with(current_word) {
                            suggestions.push(Suggestion {
                                value: opt.to_string(),
                                description: Some(desc.to_string()),
                                style: None,
                                extra: Some(vec!["option".to_string()]),
                                span: Span::new(word_start, pos),
                                append_whitespace: true,
                                display_override: None,
                                match_indices: None,
                            });
                        }
                    }
                }
            }
        } else {
            // Argument position - complete files
            let (dir, file_prefix) = if current_word.contains('/') {
                let idx = current_word.rfind('/').unwrap();
                let dir = if idx == 0 { "/" } else { &current_word[..idx] };
                (dir.to_string(), &current_word[idx + 1..])
            } else {
                (".".to_string(), current_word)
            };

            let dir_path = if dir.starts_with('~') {
                dirs::home_dir()
                    .map(|h| dir.replacen('~', &h.to_string_lossy(), 1))
                    .unwrap_or(dir.clone())
            } else {
                dir.clone()
            };

            if let Ok(entries) = std::fs::read_dir(&dir_path) {
                let prefix_lower = file_prefix.to_lowercase();
                for entry in entries.take(100).flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        if name.to_lowercase().starts_with(&prefix_lower) || file_prefix.is_empty()
                        {
                            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                            let display = if dir == "." {
                                name.to_string()
                            } else if dir.ends_with('/') {
                                format!("{}{}", dir, name)
                            } else {
                                format!("{}/{}", dir, name)
                            };
                            let value = if is_dir {
                                format!("{}/", display)
                            } else {
                                display
                            };
                            suggestions.push(Suggestion {
                                value,
                                description: if is_dir {
                                    Some("directory".to_string())
                                } else {
                                    None
                                },
                                style: None,
                                extra: Some(vec!["file".to_string()]),
                                span: Span::new(word_start, pos),
                                append_whitespace: !is_dir,
                                display_override: None,
                                match_indices: None,
                            });
                        }
                    }
                }
            }
        }

        // Deduplicate by value
        suggestions.sort_by(|a, b| a.value.cmp(&b.value));
        suggestions.dedup_by(|a, b| a.value == b.value);
        suggestions
    }
}

fn parse_option_spec(spec: &str) -> Option<(String, String)> {
    let spec = spec.trim();
    if !spec.contains('-') {
        return None;
    }
    let opt_start = if spec.starts_with('(') {
        spec.find(')')?.checked_add(1)?
    } else {
        0
    };
    let rest = &spec[opt_start..];
    if !rest.starts_with('-') {
        return None;
    }
    let opt_end = rest
        .find(|c| c == '[' || c == '=' || c == ':' || c == ' ')
        .unwrap_or(rest.len());
    let opt_name = rest[..opt_end].trim_end_matches(|c| c == '+' || c == '=');
    if opt_name.is_empty() || opt_name == "-" || opt_name == "--" {
        return None;
    }
    let desc = if let Some(bracket_start) = rest.find('[') {
        if let Some(bracket_end) = rest[bracket_start..].find(']') {
            rest[bracket_start + 1..bracket_start + bracket_end].to_string()
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    Some((opt_name.to_string(), desc))
}

/// Custom prompt that supports PS1/PROMPT with zsh escape sequences
struct ZshrsPrompt {
    left_prompt: String,
    right_prompt: String,
}

impl ZshrsPrompt {
    fn new(executor: &ShellExecutor) -> Self {
        // Check for PS1 or PROMPT (zsh uses PROMPT, bash uses PS1)
        let prompt_str = executor
            .variables
            .get("PROMPT")
            .or_else(|| executor.variables.get("PS1"))
            .cloned()
            .or_else(|| env::var("PROMPT").ok())
            .or_else(|| env::var("PS1").ok())
            .unwrap_or_else(|| "%n@%m %1~ %# ".to_string());

        // Check for RPROMPT (right prompt, zsh feature)
        let rprompt_str = executor
            .variables
            .get("RPROMPT")
            .cloned()
            .or_else(|| env::var("RPROMPT").ok())
            .unwrap_or_default();

        let left_prompt = expand_prompt_escapes(&prompt_str, executor);
        let right_prompt = expand_prompt_escapes(&rprompt_str, executor);

        Self {
            left_prompt,
            right_prompt,
        }
    }
}

impl Prompt for ZshrsPrompt {
    fn render_prompt_left(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(&self.left_prompt)
    }

    fn render_prompt_right(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(&self.right_prompt)
    }

    fn render_prompt_indicator(
        &self,
        _edit_mode: reedline::PromptEditMode,
    ) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed("")
    }

    fn render_prompt_multiline_indicator(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed("> ")
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: PromptHistorySearch,
    ) -> std::borrow::Cow<'_, str> {
        let prefix = match history_search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "failing ",
        };
        std::borrow::Cow::Owned(format!(
            "({}reverse-i-search)`{}': ",
            prefix, history_search.term
        ))
    }

    fn get_prompt_color(&self) -> Color {
        Color::Green
    }

    fn get_indicator_color(&self) -> Color {
        Color::Cyan
    }

    fn get_prompt_right_color(&self) -> Color {
        Color::AnsiValue(5)
    }

    fn right_prompt_on_last_line(&self) -> bool {
        true
    }
}

fn expand_prompt_escapes(prompt: &str, executor: &ShellExecutor) -> String {
    let mut result = String::new();
    let mut chars = prompt.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('n') => {
                    // Username
                    result.push_str(&env::var("USER").unwrap_or_else(|_| "user".to_string()));
                }
                Some('m') => {
                    // Hostname (short)
                    result.push_str(
                        &hostname::get()
                            .map(|h| {
                                h.to_string_lossy()
                                    .split('.')
                                    .next()
                                    .unwrap_or("localhost")
                                    .to_string()
                            })
                            .unwrap_or_else(|_| "localhost".to_string()),
                    );
                }
                Some('M') => {
                    // Hostname (full)
                    result.push_str(
                        &hostname::get()
                            .map(|h| h.to_string_lossy().to_string())
                            .unwrap_or_else(|_| "localhost".to_string()),
                    );
                }
                Some('~') | Some('d') => {
                    // Current directory (~ for home)
                    let cwd = env::current_dir()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| "?".to_string());
                    if let Some(home) = dirs::home_dir() {
                        let home_str = home.to_string_lossy();
                        if cwd.starts_with(home_str.as_ref()) {
                            result.push('~');
                            result.push_str(&cwd[home_str.len()..]);
                        } else {
                            result.push_str(&cwd);
                        }
                    } else {
                        result.push_str(&cwd);
                    }
                }
                Some('/') => {
                    // Current directory (full path)
                    result.push_str(
                        &env::current_dir()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|_| "?".to_string()),
                    );
                }
                Some('1') | Some('c') | Some('C') => {
                    // Trailing component of current directory
                    let cwd = env::current_dir()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| "?".to_string());
                    if let Some(name) = PathBuf::from(&cwd).file_name() {
                        result.push_str(&name.to_string_lossy());
                    } else {
                        result.push('/');
                    }
                }
                Some('#') | Some('%') => {
                    // # if root, % otherwise
                    let is_root = env::var("EUID")
                        .or_else(|_| env::var("UID"))
                        .map(|uid| uid == "0")
                        .unwrap_or(false);
                    if is_root {
                        result.push('#');
                    } else {
                        result.push('%');
                    }
                }
                Some('?') => {
                    // Exit status of last command
                    result.push_str(&executor.last_status.to_string());
                }
                Some('j') => {
                    // Number of jobs
                    result.push_str(&executor.jobs.count().to_string());
                }
                Some('T') => {
                    // Current time in 12-hour format
                    let now = chrono::Local::now();
                    result.push_str(&now.format("%I:%M").to_string());
                }
                Some('t') | Some('@') => {
                    // Current time in 12-hour format with am/pm
                    let now = chrono::Local::now();
                    result.push_str(&now.format("%I:%M %p").to_string());
                }
                Some('*') => {
                    // Current time in 24-hour format
                    let now = chrono::Local::now();
                    result.push_str(&now.format("%H:%M").to_string());
                }
                Some('D') => {
                    // Date
                    let now = chrono::Local::now();
                    result.push_str(&now.format("%Y-%m-%d").to_string());
                }
                Some('F') => {
                    // Bold (start)
                    result.push_str("\x1b[1m");
                }
                Some('f') => {
                    // Bold (end) / reset
                    result.push_str("\x1b[0m");
                }
                Some('B') => {
                    // Bold (start, alternative)
                    result.push_str("\x1b[1m");
                }
                Some('b') => {
                    // Bold (end, alternative)
                    result.push_str("\x1b[22m");
                }
                Some('{') => {
                    // Start of literal escape sequence (ignored)
                }
                Some('}') => {
                    // End of literal escape sequence (ignored)
                }
                Some(other) => {
                    result.push('%');
                    result.push(other);
                }
                None => {
                    result.push('%');
                }
            }
        } else if c == '\\' {
            // Bash-style escapes
            match chars.next() {
                Some('u') => {
                    result.push_str(&env::var("USER").unwrap_or_else(|_| "user".to_string()));
                }
                Some('h') => {
                    result.push_str(
                        &hostname::get()
                            .map(|h| {
                                h.to_string_lossy()
                                    .split('.')
                                    .next()
                                    .unwrap_or("localhost")
                                    .to_string()
                            })
                            .unwrap_or_else(|_| "localhost".to_string()),
                    );
                }
                Some('H') => {
                    result.push_str(
                        &hostname::get()
                            .map(|h| h.to_string_lossy().to_string())
                            .unwrap_or_else(|_| "localhost".to_string()),
                    );
                }
                Some('w') => {
                    let cwd = env::current_dir()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| "?".to_string());
                    if let Some(home) = dirs::home_dir() {
                        let home_str = home.to_string_lossy();
                        if cwd.starts_with(home_str.as_ref()) {
                            result.push('~');
                            result.push_str(&cwd[home_str.len()..]);
                        } else {
                            result.push_str(&cwd);
                        }
                    } else {
                        result.push_str(&cwd);
                    }
                }
                Some('W') => {
                    let cwd = env::current_dir()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| "?".to_string());
                    if let Some(name) = PathBuf::from(&cwd).file_name() {
                        result.push_str(&name.to_string_lossy());
                    } else {
                        result.push('/');
                    }
                }
                Some('$') => {
                    let is_root = env::var("EUID")
                        .or_else(|_| env::var("UID"))
                        .map(|uid| uid == "0")
                        .unwrap_or(false);
                    if is_root {
                        result.push('#');
                    } else {
                        result.push('$');
                    }
                }
                Some('n') => {
                    result.push('\n');
                }
                Some('r') => {
                    result.push('\r');
                }
                Some('\\') => {
                    result.push('\\');
                }
                Some('[') | Some(']') => {
                    // Non-printing character markers (ignored in output)
                }
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => {
                    result.push('\\');
                }
            }
        } else {
            result.push(c);
        }
    }

    result
}
