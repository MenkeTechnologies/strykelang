//! Native Rust implementation of compinit - parallelized with rayon
//!
//! compinit is the slowest part of zsh startup. It:
//! 1. Scans all directories in fpath
//! 2. Reads first line of every _* file
//! 3. Parses #compdef/#autoload directives
//! 4. Registers completion functions
//!
//! This native implementation parallelizes the scanning with rayon.

use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

/// Completion definition from #compdef line
#[derive(Clone, Debug)]
pub enum CompDef {
    /// Regular command completion: #compdef cmd1 cmd2 ...
    Commands(Vec<String>),
    /// Pattern completion: #compdef -p 'pattern'
    Pattern(String),
    /// Post-pattern completion: #compdef -P 'pattern'
    PostPattern(String),
    /// Key binding: #compdef -k style key1 key2 ...
    KeyBinding { style: String, keys: Vec<String> },
    /// Widget key binding: #compdef -K widget style key
    WidgetKey {
        widget: String,
        style: String,
        key: String,
    },
}

/// Parsed completion file
#[derive(Clone, Debug)]
pub struct CompFile {
    /// Full path to the file
    pub path: PathBuf,
    /// Function name (filename without path)
    pub name: String,
    /// What this file defines
    pub def: CompFileDef,
}

/// What a completion file defines
#[derive(Clone, Debug)]
pub enum CompFileDef {
    /// #compdef - completion function
    CompDef(CompDef),
    /// #autoload - helper function with options
    Autoload(Vec<String>),
    /// No recognized directive
    None,
}

/// Result of compinit scan
#[derive(Debug, Default)]
pub struct CompInitResult {
    /// Command -> function mapping (_comps)
    pub comps: HashMap<String, String>,
    /// Command -> service mapping (_services)
    pub services: HashMap<String, String>,
    /// Pattern -> function mapping (_patcomps)
    pub patcomps: HashMap<String, String>,
    /// Post-pattern -> function mapping (_postpatcomps)
    pub postpatcomps: HashMap<String, String>,
    /// Autoload functions with options (_compautos)
    pub compautos: HashMap<String, String>,
    /// All scanned files
    pub files: Vec<CompFile>,
    /// Scan duration
    pub scan_time_ms: u64,
    /// Number of directories scanned
    pub dirs_scanned: usize,
    /// Number of files scanned
    pub files_scanned: usize,
}

/// Parse the first line of a completion file
fn parse_first_line(line: &str) -> CompFileDef {
    let line = line.trim();

    if let Some(rest) = line.strip_prefix("#compdef") {
        let rest = rest.trim();
        if rest.is_empty() {
            return CompFileDef::None;
        }

        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.is_empty() {
            return CompFileDef::None;
        }

        // Check for options
        match parts[0] {
            "-p" if parts.len() >= 2 => {
                CompFileDef::CompDef(CompDef::Pattern(parts[1].to_string()))
            }
            "-P" if parts.len() >= 2 => {
                CompFileDef::CompDef(CompDef::PostPattern(parts[1].to_string()))
            }
            "-k" if parts.len() >= 3 => CompFileDef::CompDef(CompDef::KeyBinding {
                style: parts[1].to_string(),
                keys: parts[2..].iter().map(|s| s.to_string()).collect(),
            }),
            "-K" if parts.len() >= 4 => CompFileDef::CompDef(CompDef::WidgetKey {
                widget: parts[1].to_string(),
                style: parts[2].to_string(),
                key: parts[3].to_string(),
            }),
            _ => {
                // Regular command definitions
                let cmds: Vec<String> = parts
                    .iter()
                    .filter(|s| !s.starts_with('-'))
                    .map(|s| s.to_string())
                    .collect();
                if cmds.is_empty() {
                    CompFileDef::None
                } else {
                    CompFileDef::CompDef(CompDef::Commands(cmds))
                }
            }
        }
    } else if let Some(rest) = line.strip_prefix("#autoload") {
        let opts: Vec<String> = rest.split_whitespace().map(|s| s.to_string()).collect();
        CompFileDef::Autoload(opts)
    } else {
        CompFileDef::None
    }
}

/// Scan a single completion file
fn scan_file(path: &Path) -> Option<CompFile> {
    let name = path.file_name()?.to_string_lossy().to_string();

    // Must start with underscore
    if !name.starts_with('_') {
        return None;
    }

    // Skip certain patterns
    if name.contains(';')
        || name.contains('|')
        || name.contains('&')
        || name.ends_with('~')
        || name.ends_with(".zwc")
    {
        return None;
    }

    // Read first line
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let mut first_line = String::new();
    reader.read_line(&mut first_line).ok()?;

    let def = parse_first_line(&first_line);

    Some(CompFile {
        path: path.to_path_buf(),
        name,
        def,
    })
}

/// Scan a directory for completion files (parallel)
fn scan_directory(dir: &Path) -> Vec<CompFile> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();

    // Parallel scan of files within directory
    paths.par_iter().filter_map(|p| scan_file(p)).collect()
}

/// Initialize the completion system by scanning fpath
///
/// This is the main entry point - replaces the zsh compinit function.
/// Uses rayon for parallel directory and file scanning.
pub fn compinit(fpath: &[PathBuf]) -> CompInitResult {
    let start = Instant::now();

    // Track seen function names (first one wins)
    let seen: Mutex<HashSet<String>> = Mutex::new(HashSet::new());

    // Parallel scan of all directories
    let all_files: Vec<CompFile> = fpath
        .par_iter()
        .filter(|dir| dir.as_os_str() != "." && dir.exists())
        .flat_map(|dir| scan_directory(dir))
        .filter(|f| {
            let mut seen = seen.lock().unwrap();
            if seen.contains(&f.name) {
                false
            } else {
                seen.insert(f.name.clone());
                true
            }
        })
        .collect();

    let files_scanned = all_files.len();
    let dirs_scanned = fpath.len();

    // Build the result maps
    let mut result = CompInitResult {
        scan_time_ms: start.elapsed().as_millis() as u64,
        dirs_scanned,
        files_scanned,
        ..Default::default()
    };

    for file in &all_files {
        match &file.def {
            CompFileDef::CompDef(compdef) => {
                match compdef {
                    CompDef::Commands(cmds) => {
                        for cmd in cmds {
                            // Handle service syntax: cmd=service
                            if let Some(eq_pos) = cmd.find('=') {
                                let cmd_name = &cmd[..eq_pos];
                                let service = &cmd[eq_pos + 1..];
                                result.comps.insert(cmd_name.to_string(), file.name.clone());
                                result
                                    .services
                                    .insert(cmd_name.to_string(), service.to_string());
                            } else {
                                result.comps.insert(cmd.clone(), file.name.clone());
                            }
                        }
                    }
                    CompDef::Pattern(pat) => {
                        result.patcomps.insert(pat.clone(), file.name.clone());
                    }
                    CompDef::PostPattern(pat) => {
                        result.postpatcomps.insert(pat.clone(), file.name.clone());
                    }
                    CompDef::KeyBinding { .. } | CompDef::WidgetKey { .. } => {
                        // Key bindings need to be handled by the shell
                    }
                }
            }
            CompFileDef::Autoload(opts) => {
                let opts_str = opts.join(" ");
                result.compautos.insert(file.name.clone(), opts_str);
            }
            CompFileDef::None => {}
        }
    }

    result.files = all_files;
    result
}

/// Dump the compinit state to a cache file
pub fn compdump(
    result: &CompInitResult,
    dump_path: &Path,
    zsh_version: &str,
) -> std::io::Result<()> {
    use std::io::Write;

    let mut file = File::create(dump_path)?;

    // Header line: #compdump <num_files> . <zsh_version>
    writeln!(file, "#compdump {} . {}", result.files_scanned, zsh_version)?;

    // Dump _comps
    writeln!(
        file,
        "typeset -gHA _comps _services _patcomps _postpatcomps _compautos"
    )?;
    writeln!(file, "_comps=(")?;
    for (cmd, func) in &result.comps {
        writeln!(
            file,
            "  '{}' '{}'",
            escape_zsh_string(cmd),
            escape_zsh_string(func)
        )?;
    }
    writeln!(file, ")")?;

    // Dump _services
    writeln!(file, "_services=(")?;
    for (cmd, svc) in &result.services {
        writeln!(
            file,
            "  '{}' '{}'",
            escape_zsh_string(cmd),
            escape_zsh_string(svc)
        )?;
    }
    writeln!(file, ")")?;

    // Dump _patcomps
    writeln!(file, "_patcomps=(")?;
    for (pat, func) in &result.patcomps {
        writeln!(
            file,
            "  '{}' '{}'",
            escape_zsh_string(pat),
            escape_zsh_string(func)
        )?;
    }
    writeln!(file, ")")?;

    // Dump _postpatcomps
    writeln!(file, "_postpatcomps=(")?;
    for (pat, func) in &result.postpatcomps {
        writeln!(
            file,
            "  '{}' '{}'",
            escape_zsh_string(pat),
            escape_zsh_string(func)
        )?;
    }
    writeln!(file, ")")?;

    // Dump _compautos
    writeln!(file, "_compautos=(")?;
    for (name, opts) in &result.compautos {
        writeln!(
            file,
            "  '{}' '{}'",
            escape_zsh_string(name),
            escape_zsh_string(opts)
        )?;
    }
    writeln!(file, ")")?;

    // Autoload all completion functions
    writeln!(file, "autoload -Uz \\")?;
    for file_info in &result.files {
        if matches!(file_info.def, CompFileDef::CompDef(_)) {
            writeln!(file, "  {} \\", file_info.name)?;
        }
    }
    writeln!(file)?;

    Ok(())
}

/// Check if dump file is valid and can be used
pub fn check_dump(dump_path: &Path, fpath: &[PathBuf], zsh_version: &str) -> bool {
    let file = match File::open(dump_path) {
        Ok(f) => f,
        Err(_) => return false,
    };

    let mut reader = BufReader::new(file);
    let mut first_line = String::new();
    if reader.read_line(&mut first_line).is_err() {
        return false;
    }

    // Parse header: #compdump <num_files> . <version>
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 4 || parts[0] != "#compdump" {
        return false;
    }

    let stored_count: usize = match parts[1].parse() {
        Ok(n) => n,
        Err(_) => return false,
    };

    let stored_version = parts[3];

    // Quick count of files in fpath
    let current_count: usize = fpath
        .par_iter()
        .filter(|dir| dir.as_os_str() != "." && dir.exists())
        .map(|dir| {
            fs::read_dir(dir)
                .map(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .filter(|e| e.file_name().to_string_lossy().starts_with('_'))
                        .count()
                })
                .unwrap_or(0)
        })
        .sum();

    stored_count == current_count && stored_version == zsh_version
}

/// Escape a string for zsh single quotes
fn escape_zsh_string(s: &str) -> String {
    s.replace('\'', "'\\''")
}

/// Options for compinit
#[derive(Clone, Debug, Default)]
pub struct CompInitOpts {
    /// Dump file path (-d)
    pub dump_file: Option<PathBuf>,
    /// Skip dump (-D)
    pub no_dump: bool,
    /// Skip security check (-C)
    pub no_check: bool,
    /// Ignore insecure dirs (-i)
    pub ignore_insecure: bool,
    /// Use insecure dirs (-u)
    pub use_insecure: bool,
}

impl CompInitOpts {
    /// Parse compinit arguments
    pub fn parse(args: &[String]) -> Self {
        let mut opts = Self::default();
        let mut i = 0;

        while i < args.len() {
            match args[i].as_str() {
                "-d" => {
                    if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                        opts.dump_file = Some(PathBuf::from(&args[i + 1]));
                        i += 1;
                    }
                }
                "-D" => opts.no_dump = true,
                "-C" => opts.no_check = true,
                "-i" => opts.ignore_insecure = true,
                "-u" => opts.use_insecure = true,
                _ => {}
            }
            i += 1;
        }

        opts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_compdef_commands() {
        let def = parse_first_line("#compdef git svn hg");
        match def {
            CompFileDef::CompDef(CompDef::Commands(cmds)) => {
                assert_eq!(cmds, vec!["git", "svn", "hg"]);
            }
            _ => panic!("Expected Commands"),
        }
    }

    #[test]
    fn test_parse_compdef_pattern() {
        let def = parse_first_line("#compdef -p 'c*'");
        match def {
            CompFileDef::CompDef(CompDef::Pattern(pat)) => {
                assert_eq!(pat, "'c*'");
            }
            _ => panic!("Expected Pattern"),
        }
    }

    #[test]
    fn test_parse_autoload() {
        let def = parse_first_line("#autoload -U -z");
        match def {
            CompFileDef::Autoload(opts) => {
                assert_eq!(opts, vec!["-U", "-z"]);
            }
            _ => panic!("Expected Autoload"),
        }
    }

    #[test]
    fn test_parse_compdef_key() {
        let def = parse_first_line("#compdef -k complete-word ^X^C");
        match def {
            CompFileDef::CompDef(CompDef::KeyBinding { style, keys }) => {
                assert_eq!(style, "complete-word");
                assert_eq!(keys, vec!["^X^C"]);
            }
            _ => panic!("Expected KeyBinding"),
        }
    }

    #[test]
    fn test_escape_zsh_string() {
        assert_eq!(escape_zsh_string("hello"), "hello");
        assert_eq!(escape_zsh_string("it's"), "it'\\''s");
    }
}
