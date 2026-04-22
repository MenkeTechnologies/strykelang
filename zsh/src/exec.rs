//! Shell command executor for zshrs
//!
//! Executes the parsed shell AST.

use crate::history::HistoryEngine;

/// Convert float to hex representation (%a/%A format)
fn float_to_hex(val: f64, uppercase: bool) -> String {
    if val.is_nan() {
        return if uppercase { "NAN" } else { "nan" }.to_string();
    }
    if val.is_infinite() {
        return if val > 0.0 {
            if uppercase {
                "INF"
            } else {
                "inf"
            }
        } else {
            if uppercase {
                "-INF"
            } else {
                "-inf"
            }
        }
        .to_string();
    }
    if val == 0.0 {
        let sign = if val.is_sign_negative() { "-" } else { "" };
        return if uppercase {
            format!("{}0X0P+0", sign)
        } else {
            format!("{}0x0p+0", sign)
        };
    }

    let sign = if val < 0.0 { "-" } else { "" };
    let abs_val = val.abs();
    let bits = abs_val.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32 - 1023;
    let mantissa = bits & 0xfffffffffffff;

    let hex_mantissa = format!("{:013x}", mantissa);
    let hex_mantissa = hex_mantissa.trim_end_matches('0');
    let hex_mantissa = if hex_mantissa.is_empty() {
        "0"
    } else {
        hex_mantissa
    };

    if uppercase {
        format!("{}0X1.{}P{:+}", sign, hex_mantissa.to_uppercase(), exponent)
    } else {
        format!("{}0x1.{}p{:+}", sign, hex_mantissa, exponent)
    }
}

/// Quote a string for shell output (like zsh's set output)
fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    // Check if quoting is needed
    let needs_quotes = s.chars().any(|c| {
        matches!(
            c,
            ' ' | '\t'
                | '\n'
                | '\''
                | '"'
                | '\\'
                | '$'
                | '`'
                | '!'
                | '*'
                | '?'
                | '['
                | ']'
                | '{'
                | '}'
                | '('
                | ')'
                | '<'
                | '>'
                | '|'
                | '&'
                | ';'
                | '#'
                | '~'
        )
    });
    if !needs_quotes {
        return s.to_string();
    }
    // Use single quotes, escaping single quotes as '\''
    format!("'{}'", s.replace('\'', "'\\''"))
}
use crate::jobs::{continue_job, wait_for_child, wait_for_job, JobState, JobTable};
use crate::shell_ast::*;
use crate::zwc::ZwcFile;
use std::collections::HashMap;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// A completion specification for the `complete` builtin
#[derive(Debug, Clone, Default)]
pub struct CompSpec {
    pub actions: Vec<String>,     // -a, -b, -c, etc.
    pub wordlist: Option<String>, // -W wordlist
    pub function: Option<String>, // -F function
    pub command: Option<String>,  // -C command
    pub globpat: Option<String>,  // -G glob
    pub prefix: Option<String>,   // -P prefix
    pub suffix: Option<String>,   // -S suffix
}

/// A single completion match for zsh-style completion
#[derive(Debug, Clone)]
pub struct CompMatch {
    pub word: String,                   // The actual completion word
    pub display: Option<String>,        // Display string (-d)
    pub prefix: Option<String>,         // -P prefix (inserted but not part of match)
    pub suffix: Option<String>,         // -S suffix (inserted but not part of match)
    pub hidden_prefix: Option<String>,  // -p hidden prefix
    pub hidden_suffix: Option<String>,  // -s hidden suffix
    pub ignored_prefix: Option<String>, // -i ignored prefix
    pub ignored_suffix: Option<String>, // -I ignored suffix
    pub group: Option<String>,          // -J/-V group name
    pub description: Option<String>,    // -X explanation
    pub remove_suffix: Option<String>,  // -r remove chars
    pub file_match: bool,               // -f flag
    pub quote_match: bool,              // -q flag
}

impl Default for CompMatch {
    fn default() -> Self {
        Self {
            word: String::new(),
            display: None,
            prefix: None,
            suffix: None,
            hidden_prefix: None,
            hidden_suffix: None,
            ignored_prefix: None,
            ignored_suffix: None,
            group: None,
            description: None,
            remove_suffix: None,
            file_match: false,
            quote_match: false,
        }
    }
}

/// Completion group for organizing matches
#[derive(Debug, Clone, Default)]
pub struct CompGroup {
    pub name: String,
    pub matches: Vec<CompMatch>,
    pub explanation: Option<String>,
    pub sorted: bool,
}

/// zsh completion state (compstate associative array)
#[derive(Debug, Clone, Default)]
pub struct CompState {
    pub context: String,               // completion context
    pub exact: String,                 // exact match handling
    pub exact_string: String,          // the exact string if matched
    pub ignored: i32,                  // number of ignored matches
    pub insert: String,                // what to insert
    pub insert_positions: String,      // cursor positions after insert
    pub last_prompt: String,           // whether to return to last prompt
    pub list: String,                  // listing style
    pub list_lines: i32,               // number of lines for listing
    pub list_max: i32,                 // max matches to list
    pub nmatches: i32,                 // number of matches
    pub old_insert: String,            // previous insert value
    pub old_list: String,              // previous list value
    pub parameter: String,             // parameter being completed
    pub pattern_insert: String,        // pattern insert mode
    pub pattern_match: String,         // pattern matching mode
    pub quote: String,                 // quoting type
    pub quoting: String,               // current quoting
    pub redirect: String,              // redirection type
    pub restore: String,               // restore mode
    pub to_end: String,                // move to end mode
    pub unambiguous: String,           // unambiguous prefix
    pub unambiguous_cursor: i32,       // cursor pos in unambiguous
    pub unambiguous_positions: String, // positions in unambiguous
    pub vared: String,                 // vared context
}

/// zstyle entry for completion configuration
#[derive(Debug, Clone)]
pub struct ZStyle {
    pub pattern: String,
    pub style: String,
    pub values: Vec<String>,
}

bitflags::bitflags! {
    /// Flags for autoloaded functions
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct AutoloadFlags: u32 {
        const NO_ALIAS = 0b00000001;      // -U: don't expand aliases
        const ZSH_STYLE = 0b00000010;     // -z: zsh-style autoload
        const KSH_STYLE = 0b00000100;     // -k: ksh-style autoload
        const TRACE = 0b00001000;         // -t: trace execution
        const USE_CALLER_DIR = 0b00010000; // -d: use calling function's dir
        const LOADED = 0b00100000;        // function has been loaded
    }
}

pub struct ShellExecutor {
    pub functions: HashMap<String, ShellCommand>,
    pub aliases: HashMap<String, String>,
    pub last_status: i32,
    pub variables: HashMap<String, String>,
    pub arrays: HashMap<String, Vec<String>>,
    pub assoc_arrays: HashMap<String, HashMap<String, String>>, // zsh associative arrays
    pub jobs: JobTable,
    pub fpath: Vec<PathBuf>,
    pub zwc_cache: HashMap<PathBuf, ZwcFile>,
    pub positional_params: Vec<String>,
    pub history: Option<HistoryEngine>,
    process_sub_counter: u32,
    pub traps: HashMap<String, String>,
    pub options: HashMap<String, bool>,
    pub completions: HashMap<String, CompSpec>, // command -> completion spec
    pub dir_stack: Vec<PathBuf>,
    // zsh completion system state
    pub comp_matches: Vec<CompMatch>, // Current completion matches
    pub comp_groups: Vec<CompGroup>,  // Completion groups
    pub comp_state: CompState,        // compstate associative array
    pub zstyles: Vec<ZStyle>,         // zstyle configurations
    pub comp_words: Vec<String>,      // words on command line
    pub comp_current: i32,            // current word index (1-based)
    pub comp_prefix: String,          // PREFIX parameter
    pub comp_suffix: String,          // SUFFIX parameter
    pub comp_iprefix: String,         // IPREFIX parameter
    pub comp_isuffix: String,         // ISUFFIX parameter
    pub readonly_vars: std::collections::HashSet<String>, // Read-only variables
    pub autoload_pending: HashMap<String, AutoloadFlags>, // Functions marked for autoload
}

impl ShellExecutor {
    pub fn new() -> Self {
        // Initialize fpath from FPATH env var or use defaults
        let fpath = env::var("FPATH")
            .unwrap_or_default()
            .split(':')
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect();

        let history = HistoryEngine::new().ok();

        Self {
            functions: HashMap::new(),
            aliases: HashMap::new(),
            last_status: 0,
            variables: HashMap::new(),
            arrays: HashMap::new(),
            assoc_arrays: HashMap::new(),
            jobs: JobTable::new(),
            fpath,
            zwc_cache: HashMap::new(),
            positional_params: Vec::new(),
            history,
            completions: HashMap::new(),
            dir_stack: Vec::new(),
            process_sub_counter: 0,
            traps: HashMap::new(),
            options: Self::default_options(),
            // zsh completion system
            comp_matches: Vec::new(),
            comp_groups: Vec::new(),
            comp_state: CompState::default(),
            zstyles: Vec::new(),
            comp_words: Vec::new(),
            comp_current: 0,
            comp_prefix: String::new(),
            comp_suffix: String::new(),
            comp_iprefix: String::new(),
            comp_isuffix: String::new(),
            readonly_vars: std::collections::HashSet::new(),
            autoload_pending: HashMap::new(),
        }
    }

    fn all_zsh_options() -> &'static [&'static str] {
        &[
            "aliases",
            "aliasfuncdef",
            "allexport",
            "alwayslastprompt",
            "alwaystoend",
            "appendcreate",
            "appendhistory",
            "autocd",
            "autocontinue",
            "autolist",
            "automenu",
            "autonamedirs",
            "autoparamkeys",
            "autoparamslash",
            "autopushd",
            "autoremoveslash",
            "autoresume",
            "badpattern",
            "banghist",
            "bareglobqual",
            "bashautolist",
            "bashrematch",
            "beep",
            "bgnice",
            "braceccl",
            "braceexpand",
            "bsdecho",
            "caseglob",
            "casematch",
            "casepaths",
            "cbases",
            "cdablevars",
            "cdsilent",
            "chasedots",
            "chaselinks",
            "checkjobs",
            "checkrunningjobs",
            "clobber",
            "clobberempty",
            "combiningchars",
            "completealiases",
            "completeinword",
            "continueonerror",
            "correct",
            "correctall",
            "cprecedences",
            "cshjunkiehistory",
            "cshjunkieloops",
            "cshjunkiequotes",
            "cshnullcmd",
            "cshnullglob",
            "debugbeforecmd",
            "dotglob",
            "dvorak",
            "emacs",
            "equals",
            "errexit",
            "errreturn",
            "evallineno",
            "exec",
            "extendedglob",
            "extendedhistory",
            "flowcontrol",
            "forcefloat",
            "functionargzero",
            "glob",
            "globassign",
            "globcomplete",
            "globdots",
            "globstarshort",
            "globsubst",
            "globalexport",
            "globalrcs",
            "hashall",
            "hashcmds",
            "hashdirs",
            "hashexecutablesonly",
            "hashlistall",
            "histallowclobber",
            "histappend",
            "histbeep",
            "histexpand",
            "histexpiredupsfirst",
            "histfcntllock",
            "histfindnodups",
            "histignorealldups",
            "histignoredups",
            "histignorespace",
            "histlexwords",
            "histnofunctions",
            "histnostore",
            "histreduceblanks",
            "histsavebycopy",
            "histsavenodups",
            "histsubstpattern",
            "histverify",
            "hup",
            "ignorebraces",
            "ignoreclosebraces",
            "ignoreeof",
            "incappendhistory",
            "incappendhistorytime",
            "interactive",
            "interactivecomments",
            "ksharrays",
            "kshautoload",
            "kshglob",
            "kshoptionprint",
            "kshtypeset",
            "kshzerosubscript",
            "listambiguous",
            "listbeep",
            "listpacked",
            "listrowsfirst",
            "listtypes",
            "localloops",
            "localoptions",
            "localpatterns",
            "localtraps",
            "log",
            "login",
            "longlistjobs",
            "magicequalsubst",
            "mailwarn",
            "mailwarning",
            "markdirs",
            "menucomplete",
            "monitor",
            "multibyte",
            "multifuncdef",
            "multios",
            "nomatch",
            "notify",
            "nullglob",
            "numericglobsort",
            "octalzeroes",
            "onecmd",
            "overstrike",
            "pathdirs",
            "pathscript",
            "physical",
            "pipefail",
            "posixaliases",
            "posixargzero",
            "posixbuiltins",
            "posixcd",
            "posixidentifiers",
            "posixjobs",
            "posixstrings",
            "posixtraps",
            "printeightbit",
            "printexitvalue",
            "privileged",
            "promptbang",
            "promptcr",
            "promptpercent",
            "promptsp",
            "promptsubst",
            "promptvars",
            "pushdignoredups",
            "pushdminus",
            "pushdsilent",
            "pushdtohome",
            "rcexpandparam",
            "rcquotes",
            "rcs",
            "recexact",
            "rematchpcre",
            "restricted",
            "rmstarsilent",
            "rmstarwait",
            "sharehistory",
            "shfileexpansion",
            "shglob",
            "shinstdin",
            "shnullcmd",
            "shoptionletters",
            "shortloops",
            "shortrepeat",
            "shwordsplit",
            "singlecommand",
            "singlelinezle",
            "sourcetrace",
            "stdin",
            "sunkeyboardhack",
            "trackall",
            "transientrprompt",
            "trapsasync",
            "typesetsilent",
            "typesettounset",
            "unset",
            "verbose",
            "vi",
            "warncreateglobal",
            "warnnestedvar",
            "xtrace",
            "zle",
        ]
    }

    fn default_options() -> HashMap<String, bool> {
        let mut opts = HashMap::new();
        // Initialize all options to false first
        for opt in Self::all_zsh_options() {
            opts.insert(opt.to_string(), false);
        }
        // Set zsh defaults (options marked with <D> or <Z> in zshoptions man page)
        let defaults_on = [
            "aliases",
            "alwayslastprompt",
            "appendhistory",
            "autolist",
            "automenu",
            "autoparamkeys",
            "autoparamslash",
            "autoremoveslash",
            "badpattern",
            "banghist",
            "bareglobqual",
            "beep",
            "bgnice",
            "caseglob",
            "casematch",
            "checkjobs",
            "checkrunningjobs",
            "clobber",
            "debugbeforecmd",
            "equals",
            "evallineno",
            "exec",
            "flowcontrol",
            "functionargzero",
            "glob",
            "globalexport",
            "globalrcs",
            "hashcmds",
            "hashdirs",
            "hashlistall",
            "histbeep",
            "histsavebycopy",
            "hup",
            "interactive",
            "listambiguous",
            "listbeep",
            "listtypes",
            "monitor",
            "multibyte",
            "multifuncdef",
            "multios",
            "nomatch",
            "notify",
            "promptcr",
            "promptpercent",
            "promptsp",
            "rcs",
            "shinstdin",
            "shortloops",
            "unset",
            "zle",
        ];
        for opt in defaults_on {
            opts.insert(opt.to_string(), true);
        }
        opts
    }

    /// Normalize option name: lowercase, remove underscores/hyphens, handle "no" prefix
    fn normalize_option_name(name: &str) -> (String, bool) {
        let normalized = name.to_lowercase().replace(['-', '_'], "");
        if let Some(stripped) = normalized.strip_prefix("no") {
            // Check if the stripped version is a valid option
            if Self::all_zsh_options().contains(&stripped) {
                return (stripped.to_string(), false);
            }
        }
        (normalized, true)
    }

    /// Check if option name matches a pattern (for -m flag)
    fn option_matches_pattern(opt: &str, pattern: &str) -> bool {
        let pat = pattern.to_lowercase().replace(['-', '_'], "");
        let opt_lower = opt.to_lowercase();

        if pat.contains('*') || pat.contains('?') || pat.contains('[') {
            // Simple glob matching
            let regex_pat = pat.replace('.', "\\.").replace('*', ".*").replace('?', ".");
            regex::Regex::new(&format!("^{}$", regex_pat))
                .map(|re| re.is_match(&opt_lower))
                .unwrap_or(false)
        } else {
            opt_lower == pat
        }
    }

    /// Try to load a function from ZWC files in fpath
    pub fn autoload_function(&mut self, name: &str) -> Option<ShellCommand> {
        // First check if already loaded
        if let Some(func) = self.functions.get(name) {
            return Some(func.clone());
        }

        // Search fpath for the function
        for dir in &self.fpath.clone() {
            // Try directory.zwc first
            let zwc_path = dir.with_extension("zwc");
            if zwc_path.exists() {
                if let Some(func) = self.load_function_from_zwc(&zwc_path, name) {
                    return Some(func);
                }
            }

            // Try individual function.zwc
            let func_zwc = dir.join(format!("{}.zwc", name));
            if func_zwc.exists() {
                if let Some(func) = self.load_function_from_zwc(&func_zwc, name) {
                    return Some(func);
                }
            }

            // Look for directory/*.zwc files containing this function
            if dir.is_dir() {
                if let Ok(entries) = fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().map_or(false, |e| e == "zwc") {
                            if let Some(func) = self.load_function_from_zwc(&path, name) {
                                return Some(func);
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// Load a specific function from a ZWC file
    fn load_function_from_zwc(&mut self, path: &Path, name: &str) -> Option<ShellCommand> {
        // Check cache
        let zwc = if let Some(cached) = self.zwc_cache.get(path) {
            cached
        } else {
            // Load and cache the ZWC file
            let zwc = ZwcFile::load(path).ok()?;
            self.zwc_cache.insert(path.to_path_buf(), zwc);
            self.zwc_cache.get(path)?
        };

        // Find the function
        let func = zwc.get_function(name)?;
        let decoded = zwc.decode_function(func)?;

        // Convert to shell command and cache
        let shell_func = decoded.to_shell_function()?;

        // Register the function
        if let ShellCommand::FunctionDef(fname, body) = &shell_func {
            self.functions.insert(fname.clone(), (**body).clone());
        }

        Some(shell_func)
    }

    /// Add a directory to fpath
    pub fn add_fpath(&mut self, path: PathBuf) {
        if !self.fpath.contains(&path) {
            self.fpath.insert(0, path);
        }
    }

    pub fn execute_script(&mut self, script: &str) -> Result<i32, String> {
        // Expand history references before parsing
        let expanded = self.expand_history(script);

        let mut parser = ShellParser::new(&expanded);
        let commands = parser.parse_script()?;

        for cmd in commands {
            self.execute_command(&cmd)?;
        }

        Ok(self.last_status)
    }

    /// Expand history references: !!, !n, !-n, !string, !?string?
    fn expand_history(&self, input: &str) -> String {
        let Some(ref engine) = self.history else {
            return input.to_string();
        };

        if !input.contains('!') {
            return input.to_string();
        }

        let history_count = engine.count().unwrap_or(0) as usize;
        if history_count == 0 {
            return input.to_string();
        }

        let mut result = String::new();
        let chars: Vec<char> = input.chars().collect();
        let mut i = 0;
        let mut in_brace = 0; // Track ${...} nesting

        while i < chars.len() {
            // Track brace depth to avoid expanding ! inside ${...}
            if i + 1 < chars.len() && chars[i] == '$' && chars[i + 1] == '{' {
                in_brace += 1;
                result.push(chars[i]);
                i += 1;
                result.push(chars[i]);
                i += 1;
                continue;
            }
            if chars[i] == '}' && in_brace > 0 {
                in_brace -= 1;
                result.push(chars[i]);
                i += 1;
                continue;
            }

            if chars[i] == '!' && in_brace == 0 {
                if i + 1 < chars.len() {
                    match chars[i + 1] {
                        '!' => {
                            // !! - previous command
                            if let Ok(Some(entry)) = engine.get_by_offset(0) {
                                result.push_str(&entry.command);
                            }
                            i += 2;
                            continue;
                        }
                        '-' => {
                            // !-n - nth previous command
                            i += 2;
                            let start = i;
                            while i < chars.len() && chars[i].is_ascii_digit() {
                                i += 1;
                            }
                            if i > start {
                                let n: usize = chars[start..i]
                                    .iter()
                                    .collect::<String>()
                                    .parse()
                                    .unwrap_or(0);
                                if n > 0 {
                                    if let Ok(Some(entry)) = engine.get_by_offset(n - 1) {
                                        result.push_str(&entry.command);
                                    }
                                }
                            }
                            continue;
                        }
                        '?' => {
                            // !?string? - contains search
                            i += 2;
                            let start = i;
                            while i < chars.len() && chars[i] != '?' {
                                i += 1;
                            }
                            let search: String = chars[start..i].iter().collect();
                            if i < chars.len() && chars[i] == '?' {
                                i += 1;
                            }
                            if let Ok(entries) = engine.search(&search, 1) {
                                if let Some(entry) = entries.first() {
                                    result.push_str(&entry.command);
                                }
                            }
                            continue;
                        }
                        c if c.is_ascii_digit() => {
                            // !n - command at position n
                            i += 1;
                            let start = i;
                            while i < chars.len() && chars[i].is_ascii_digit() {
                                i += 1;
                            }
                            let n: i64 = chars[start..i]
                                .iter()
                                .collect::<String>()
                                .parse()
                                .unwrap_or(0);
                            if n > 0 {
                                if let Ok(Some(entry)) = engine.get_by_number(n) {
                                    result.push_str(&entry.command);
                                }
                            }
                            continue;
                        }
                        c if c.is_alphabetic() || c == '_' || c == '/' || c == '.' => {
                            // !string - prefix search
                            i += 1;
                            let start = i;
                            while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '!' {
                                i += 1;
                            }
                            let prefix: String = chars[start..i].iter().collect();
                            if let Ok(entries) = engine.search_prefix(&prefix, 1) {
                                if let Some(entry) = entries.first() {
                                    result.push_str(&entry.command);
                                }
                            }
                            continue;
                        }
                        _ => {}
                    }
                }
            }
            result.push(chars[i]);
            i += 1;
        }

        result
    }

    pub fn execute_command(&mut self, cmd: &ShellCommand) -> Result<i32, String> {
        match cmd {
            ShellCommand::Simple(simple) => self.execute_simple(simple),
            ShellCommand::Pipeline(cmds, negated) => {
                let status = self.execute_pipeline(cmds)?;
                if *negated {
                    self.last_status = if status == 0 { 1 } else { 0 };
                } else {
                    self.last_status = status;
                }
                Ok(self.last_status)
            }
            ShellCommand::List(items) => self.execute_list(items),
            ShellCommand::Compound(compound) => self.execute_compound(compound),
            ShellCommand::FunctionDef(name, body) => {
                self.functions.insert(name.clone(), (**body).clone());
                self.last_status = 0;
                Ok(0)
            }
        }
    }

    fn execute_simple(&mut self, cmd: &SimpleCommand) -> Result<i32, String> {
        // Handle assignments
        for (var, val) in &cmd.assignments {
            match val {
                ShellWord::ArrayLiteral(elements) => {
                    // Array assignment: arr=(a b c)
                    let array: Vec<String> = elements.iter().map(|e| self.expand_word(e)).collect();
                    self.arrays.insert(var.clone(), array);
                }
                _ => {
                    let expanded = self.expand_word(val);
                    if cmd.words.is_empty() {
                        // Just assignment, set in environment
                        env::set_var(var, &expanded);
                    }
                    self.variables.insert(var.clone(), expanded);
                }
            }
        }

        if cmd.words.is_empty() {
            self.last_status = 0;
            return Ok(0);
        }

        let words: Vec<String> = cmd
            .words
            .iter()
            .flat_map(|w| self.expand_word_glob(w))
            .collect();
        if words.is_empty() {
            self.last_status = 0;
            return Ok(0);
        }
        let cmd_name = &words[0];
        let args = &words[1..];

        // Check for shell builtins
        let status = match cmd_name.as_str() {
            "cd" => self.builtin_cd(args),
            "pwd" => self.builtin_pwd(&cmd.redirects),
            "echo" => self.builtin_echo(args, &cmd.redirects),
            "export" => self.builtin_export(args),
            "unset" => self.builtin_unset(args),
            "source" | "." => self.builtin_source(args),
            "exit" | "bye" | "logout" => self.builtin_exit(args),
            "return" => self.builtin_return(args),
            "true" => 0,
            "false" => 1,
            ":" => 0,
            "chdir" => self.builtin_cd(args),
            "test" | "[" => self.builtin_test(args),
            "local" => self.builtin_local(args),
            "declare" | "typeset" => self.builtin_declare(args),
            "read" => self.builtin_read(args),
            "shift" => self.builtin_shift(args),
            "eval" => self.builtin_eval(args),
            "jobs" => self.builtin_jobs(args),
            "fg" => self.builtin_fg(args),
            "bg" => self.builtin_bg(args),
            "kill" => self.builtin_kill(args),
            "disown" => self.builtin_disown(args),
            "wait" => self.builtin_wait(args),
            "autoload" => self.builtin_autoload(args),
            "history" => self.builtin_history(args),
            "fc" => self.builtin_fc(args),
            "trap" => self.builtin_trap(args),
            "suspend" => self.builtin_suspend(args),
            "alias" => self.builtin_alias(args),
            "unalias" => self.builtin_unalias(args),
            "set" => self.builtin_set(args),
            "shopt" => self.builtin_shopt(args),
            "setopt" => self.builtin_setopt(args),
            "unsetopt" => self.builtin_unsetopt(args),
            "getopts" => self.builtin_getopts(args),
            "type" => self.builtin_type(args),
            "hash" => self.builtin_hash(args),
            "command" => self.builtin_command(args, &cmd.redirects),
            "builtin" => self.builtin_builtin(args, &cmd.redirects),
            "let" => self.builtin_let(args),
            "compgen" => self.builtin_compgen(args),
            "complete" => self.builtin_complete(args),
            "compopt" => self.builtin_compopt(args),
            "compadd" => self.builtin_compadd(args),
            "compset" => self.builtin_compset(args),
            "zstyle" => self.builtin_zstyle(args),
            "pushd" => self.builtin_pushd(args),
            "popd" => self.builtin_popd(args),
            "dirs" => self.builtin_dirs(args),
            "printf" => self.builtin_printf(args),
            // Control flow
            "break" => self.builtin_break(args),
            "continue" => self.builtin_continue(args),
            // Aliases for existing builtins
            "bye" | "logout" => self.builtin_exit(args),
            "chdir" => self.builtin_cd(args),
            // Enable/disable builtins
            "disable" => self.builtin_disable(args),
            "enable" => self.builtin_enable(args),
            // Emulation
            "emulate" => self.builtin_emulate(args),
            // Exec
            "exec" => self.builtin_exec(args),
            // Typed variables
            "float" => self.builtin_float(args),
            "integer" => self.builtin_integer(args),
            // Functions
            "functions" => self.builtin_functions(args),
            // Print (zsh style)
            "print" => self.builtin_print(args),
            // Command lookup
            "whence" => self.builtin_whence(args),
            "where" => self.builtin_where(args),
            "which" => self.builtin_which(args),
            // Resource limits
            "ulimit" => self.builtin_ulimit(args),
            "limit" => self.builtin_limit(args),
            "unlimit" => self.builtin_unlimit(args),
            // File mask
            "umask" => self.builtin_umask(args),
            // Hash table
            "rehash" => self.builtin_rehash(args),
            "unhash" => self.builtin_unhash(args),
            // Times
            "times" => self.builtin_times(args),
            // Module loading (stub)
            "zmodload" => self.builtin_zmodload(args),
            // Redo
            "r" => self.builtin_r(args),
            // TTY control
            "ttyctl" => self.builtin_ttyctl(args),
            // Noglob
            "noglob" => self.builtin_noglob(args, &cmd.redirects),
            // zsh/stat module
            "zstat" | "stat" => self.builtin_zstat(args),
            // zsh/datetime module
            "strftime" => self.builtin_strftime(args),
            // sleep with fractional seconds
            "zsleep" => self.builtin_zsleep(args),
            // zsh/files module
            "zln" | "zmv" | "zcp" => self.builtin_zfiles(cmd_name, args),
            // coproc management
            "coproc" => self.builtin_coproc(args),
            // zparseopts - option parsing
            "zparseopts" => self.builtin_zparseopts(args),
            // readonly/unfunction
            "readonly" => self.builtin_readonly(args),
            "unfunction" => self.builtin_unfunction(args),
            // getln/pushln
            "getln" => self.builtin_getln(args),
            "pushln" => self.builtin_pushln(args),
            // bindkey stub
            "bindkey" => self.builtin_bindkey(args),
            // zle stub
            "zle" => self.builtin_zle(args),
            // sched
            "sched" => self.builtin_sched(args),
            // zformat
            "zformat" => self.builtin_zformat(args),
            // zcompile
            "zcompile" => self.builtin_zcompile(args),
            // vared - visual edit
            "vared" => self.builtin_vared(args),
            // terminal capabilities
            "echotc" => self.builtin_echotc(args),
            "echoti" => self.builtin_echoti(args),
            // PTY and socket operations
            "zpty" => self.builtin_zpty(args),
            "zprof" => self.builtin_zprof(args),
            "zsocket" => self.builtin_zsocket(args),
            "ztcp" => self.builtin_ztcp(args),
            "zregexparse" => self.builtin_zregexparse(args),
            "clone" => self.builtin_clone(args),
            "log" => self.builtin_log(args),
            // Completion system builtins
            "comparguments" => self.builtin_comparguments(args),
            "compcall" => self.builtin_compcall(args),
            "compctl" => self.builtin_compctl(args),
            "compdescribe" => self.builtin_compdescribe(args),
            "compfiles" => self.builtin_compfiles(args),
            "compgroups" => self.builtin_compgroups(args),
            "compquote" => self.builtin_compquote(args),
            "comptags" => self.builtin_comptags(args),
            "comptry" => self.builtin_comptry(args),
            "compvalues" => self.builtin_compvalues(args),
            // Capabilities (Linux-specific, stubs on macOS)
            "cap" | "getcap" | "setcap" => self.builtin_cap(args),
            // FTP client
            "zftp" => self.builtin_zftp(args),
            // zsh/curses module
            "zcurses" => self.builtin_zcurses(args),
            // zsh/system module
            "sysread" => self.builtin_sysread(args),
            "syswrite" => self.builtin_syswrite(args),
            "syserror" => self.builtin_syserror(args),
            "sysopen" => self.builtin_sysopen(args),
            "sysseek" => self.builtin_sysseek(args),
            // zsh/mapfile module
            "mapfile" => 0, // mapfile is a special parameter, not a command
            // zsh/param/private
            "private" => self.builtin_private(args),
            // zsh/attr (extended attributes)
            "zgetattr" | "zsetattr" | "zdelattr" | "zlistattr" => {
                self.builtin_zattr(cmd_name, args)
            }
            // Completion helper functions (now implemented in Rust compsys crate)
            // These are stubs that return success during non-completion execution
            "_arguments" | "_describe" | "_description" | "_message" | "_tags" | "_requested"
            | "_all_labels" | "_next_label" | "_files" | "_path_files" | "_directories" | "_cd"
            | "_default" | "_dispatch" | "_complete" | "_main_complete" | "_normal"
            | "_approximate" | "_correct" | "_expand" | "_history" | "_match" | "_menu"
            | "_oldlist" | "_list" | "_prefix" | "_generic" | "_wanted" | "_alternative"
            | "_values" | "_sequence" | "_sep_parts" | "_multi_parts" | "_combination"
            | "_parameters" | "_command" | "_command_names" | "_commands" | "_functions"
            | "_aliases" | "_builtins" | "_jobs" | "_pids" | "_process_names" | "_signals"
            | "_users" | "_groups" | "_hosts" | "_domains" | "_urls" | "_email_addresses"
            | "_options" | "_contexts" | "_set_options" | "_unset_options" | "_vars"
            | "_env_variables" | "_shell_variables" | "_arrays" | "_globflags" | "_globquals"
            | "_globqual_delims" | "_subscript" | "_history_modifiers" | "_brace_parameter"
            | "_tilde" | "_style" | "_cache_invalid" | "_store_cache" | "_retrieve_cache"
            | "_call_function" | "_call_program" | "_pick_variant" | "_setup"
            | "_comp_priv_prefix" | "_regex_arguments" | "_regex_words" | "_guard"
            | "_gnu_generic" | "_long_options" | "_x_arguments" | "_sub_commands"
            | "_cmdstring" | "_cmdambivalent" | "_first" | "_precommand" | "_user_at_host"
            | "_user_expand" | "_path_commands" | "_globbed_files" | "_have_glob_qual" => {
                // Return success - these functions are for completion context only
                // The actual completion logic is in the compsys Rust crate
                0
            }
            _ => {
                // Check for function
                if let Some(func) = self.functions.get(cmd_name).cloned() {
                    return self.call_function(&func, args);
                }

                // Try autoloading from pending autoload list
                if self.maybe_autoload(cmd_name) {
                    if let Some(func) = self.functions.get(cmd_name).cloned() {
                        return self.call_function(&func, args);
                    }
                }

                // Try autoloading from ZWC
                if self.autoload_function(cmd_name).is_some() {
                    if let Some(func) = self.functions.get(cmd_name).cloned() {
                        return self.call_function(&func, args);
                    }
                }

                // External command
                self.execute_external(cmd_name, args, &cmd.redirects)?
            }
        };

        self.last_status = status;
        Ok(status)
    }

    /// Call a function with positional parameters
    fn call_function(&mut self, func: &ShellCommand, args: &[String]) -> Result<i32, String> {
        // Save current positional params
        let saved_params = std::mem::take(&mut self.positional_params);

        // Set new positional params
        self.positional_params = args.to_vec();

        // Execute the function
        let result = self.execute_command(func);

        // Restore positional params
        self.positional_params = saved_params;

        result
    }

    fn execute_external(
        &mut self,
        cmd: &str,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<i32, String> {
        self.execute_external_bg(cmd, args, redirects, false)
    }

    fn execute_external_bg(
        &mut self,
        cmd: &str,
        args: &[String],
        redirects: &[Redirect],
        background: bool,
    ) -> Result<i32, String> {
        let mut command = Command::new(cmd);
        command.args(args);

        // Apply redirections
        for redir in redirects {
            let target = self.expand_word(&redir.target);
            match redir.op {
                RedirectOp::Read => match File::open(&target) {
                    Ok(f) => {
                        command.stdin(Stdio::from(f));
                    }
                    Err(e) => return Err(format!("Cannot open {}: {}", target, e)),
                },
                RedirectOp::Write => match File::create(&target) {
                    Ok(f) => {
                        command.stdout(Stdio::from(f));
                    }
                    Err(e) => return Err(format!("Cannot create {}: {}", target, e)),
                },
                RedirectOp::Append => {
                    match OpenOptions::new().create(true).append(true).open(&target) {
                        Ok(f) => {
                            command.stdout(Stdio::from(f));
                        }
                        Err(e) => return Err(format!("Cannot open {}: {}", target, e)),
                    }
                }
                RedirectOp::WriteBoth => match File::create(&target) {
                    Ok(f) => {
                        let f2 = f
                            .try_clone()
                            .map_err(|e| format!("Cannot clone fd: {}", e))?;
                        command.stdout(Stdio::from(f));
                        command.stderr(Stdio::from(f2));
                    }
                    Err(e) => return Err(format!("Cannot create {}: {}", target, e)),
                },
                RedirectOp::AppendBoth => {
                    match OpenOptions::new().create(true).append(true).open(&target) {
                        Ok(f) => {
                            let f2 = f
                                .try_clone()
                                .map_err(|e| format!("Cannot clone fd: {}", e))?;
                            command.stdout(Stdio::from(f));
                            command.stderr(Stdio::from(f2));
                        }
                        Err(e) => return Err(format!("Cannot open {}: {}", target, e)),
                    }
                }
                RedirectOp::HereDoc => {
                    // Here-document - provide content as stdin
                    if let Some(ref content) = redir.heredoc_content {
                        // Expand variables in content (unless delimiter was quoted)
                        let expanded = self.expand_string(content);
                        command.stdin(Stdio::piped());
                        // Store the content to write after spawn
                        // For now, create a temp file
                        use std::io::Write;
                        let mut temp_file = tempfile::NamedTempFile::new()
                            .map_err(|e| format!("Cannot create temp file: {}", e))?;
                        temp_file
                            .write_all(expanded.as_bytes())
                            .map_err(|e| format!("Cannot write to temp file: {}", e))?;
                        let temp_path = temp_file.into_temp_path();
                        let f = File::open(&temp_path)
                            .map_err(|e| format!("Cannot open temp file: {}", e))?;
                        command.stdin(Stdio::from(f));
                    }
                }
                RedirectOp::HereString => {
                    // Here-string - provide target as stdin
                    use std::io::Write;
                    let content = format!("{}\n", target);
                    let mut temp_file = tempfile::NamedTempFile::new()
                        .map_err(|e| format!("Cannot create temp file: {}", e))?;
                    temp_file
                        .write_all(content.as_bytes())
                        .map_err(|e| format!("Cannot write to temp file: {}", e))?;
                    let temp_path = temp_file.into_temp_path();
                    let f = File::open(&temp_path)
                        .map_err(|e| format!("Cannot open temp file: {}", e))?;
                    command.stdin(Stdio::from(f));
                }
                _ => {
                    // Other redirections handled simply
                }
            }

            // Handle {varname}>file syntax - store FD in variable
            if let Some(ref var_name) = redir.fd_var {
                // For {varname}>file, we open the file and store the fd number
                // This is typically used with exec, but we'll handle it for commands too
                #[cfg(unix)]
                {
                    use std::os::unix::io::AsRawFd;
                    let fd = match redir.op {
                        RedirectOp::Write | RedirectOp::Append => {
                            let f = if redir.op == RedirectOp::Write {
                                File::create(&target)
                            } else {
                                OpenOptions::new().create(true).append(true).open(&target)
                            };
                            match f {
                                Ok(file) => {
                                    let raw_fd = file.as_raw_fd();
                                    std::mem::forget(file); // Don't close the file
                                    raw_fd
                                }
                                Err(e) => return Err(format!("Cannot open {}: {}", target, e)),
                            }
                        }
                        RedirectOp::Read => match File::open(&target) {
                            Ok(file) => {
                                let raw_fd = file.as_raw_fd();
                                std::mem::forget(file);
                                raw_fd
                            }
                            Err(e) => return Err(format!("Cannot open {}: {}", target, e)),
                        },
                        _ => continue,
                    };
                    self.variables.insert(var_name.clone(), fd.to_string());
                }
            }
        }

        if background {
            match command.spawn() {
                Ok(child) => {
                    let pid = child.id();
                    let cmd_str = format!("{} {}", cmd, args.join(" "));
                    let job_id = self.jobs.add_job(child, cmd_str, JobState::Running);
                    println!("[{}] {}", job_id, pid);
                    Ok(0)
                }
                Err(e) => {
                    if e.kind() == io::ErrorKind::NotFound {
                        eprintln!("zshrs: command not found: {}", cmd);
                        Ok(127)
                    } else {
                        Err(format!("zshrs: {}: {}", cmd, e))
                    }
                }
            }
        } else {
            match command.status() {
                Ok(status) => Ok(status.code().unwrap_or(1)),
                Err(e) => {
                    if e.kind() == io::ErrorKind::NotFound {
                        eprintln!("zshrs: command not found: {}", cmd);
                        Ok(127)
                    } else {
                        Err(format!("zshrs: {}: {}", cmd, e))
                    }
                }
            }
        }
    }

    fn execute_pipeline(&mut self, cmds: &[ShellCommand]) -> Result<i32, String> {
        if cmds.len() == 1 {
            return self.execute_command(&cmds[0]);
        }

        let mut children: Vec<Child> = Vec::new();
        let mut prev_stdout: Option<std::process::ChildStdout> = None;

        for (i, cmd) in cmds.iter().enumerate() {
            if let ShellCommand::Simple(simple) = cmd {
                let words: Vec<String> = simple.words.iter().map(|w| self.expand_word(w)).collect();
                if words.is_empty() {
                    continue;
                }

                let mut command = Command::new(&words[0]);
                command.args(&words[1..]);

                if let Some(stdout) = prev_stdout.take() {
                    command.stdin(Stdio::from(stdout));
                }

                if i < cmds.len() - 1 {
                    command.stdout(Stdio::piped());
                }

                match command.spawn() {
                    Ok(mut child) => {
                        prev_stdout = child.stdout.take();
                        children.push(child);
                    }
                    Err(e) => {
                        eprintln!("zshrs: {}: {}", words[0], e);
                        return Ok(127);
                    }
                }
            }
        }

        // Wait for all children
        let mut last_status = 0;
        for mut child in children {
            if let Ok(status) = child.wait() {
                last_status = status.code().unwrap_or(1);
            }
        }

        Ok(last_status)
    }

    fn execute_list(&mut self, items: &[(ShellCommand, ListOp)]) -> Result<i32, String> {
        for (cmd, op) in items {
            // Check if this command should run in background
            let background = matches!(op, ListOp::Amp);

            let status = if background {
                self.execute_command_bg(cmd)?
            } else {
                self.execute_command(cmd)?
            };

            match op {
                ListOp::And => {
                    if status != 0 {
                        return Ok(status);
                    }
                }
                ListOp::Or => {
                    if status == 0 {
                        return Ok(0);
                    }
                }
                ListOp::Amp => {
                    // Already backgrounded above, continue
                }
                ListOp::Semi | ListOp::Newline => {
                    // Sequential, continue
                }
            }
        }

        Ok(self.last_status)
    }

    fn execute_command_bg(&mut self, cmd: &ShellCommand) -> Result<i32, String> {
        // For simple commands, run in background
        if let ShellCommand::Simple(simple) = cmd {
            if simple.words.is_empty() {
                return Ok(0);
            }
            let words: Vec<String> = simple.words.iter().map(|w| self.expand_word(w)).collect();
            let cmd_name = &words[0];
            let args: Vec<String> = words[1..].to_vec();
            return self.execute_external_bg(cmd_name, &args, &simple.redirects, true);
        }
        // For complex commands, just execute normally (could fork in future)
        self.execute_command(cmd)
    }

    fn execute_compound(&mut self, compound: &CompoundCommand) -> Result<i32, String> {
        match compound {
            CompoundCommand::BraceGroup(cmds) | CompoundCommand::Subshell(cmds) => {
                for cmd in cmds {
                    self.execute_command(cmd)?;
                }
                Ok(self.last_status)
            }

            CompoundCommand::If {
                conditions,
                else_part,
            } => {
                for (cond, body) in conditions {
                    // Execute condition
                    for cmd in cond {
                        self.execute_command(cmd)?;
                    }

                    if self.last_status == 0 {
                        // Condition true, execute body
                        for cmd in body {
                            self.execute_command(cmd)?;
                        }
                        return Ok(self.last_status);
                    }
                }

                // All conditions false, execute else
                if let Some(else_cmds) = else_part {
                    for cmd in else_cmds {
                        self.execute_command(cmd)?;
                    }
                }

                Ok(self.last_status)
            }

            CompoundCommand::For { var, words, body } => {
                let items: Vec<String> = if let Some(words) = words {
                    words
                        .iter()
                        .flat_map(|w| self.expand_word_split(w))
                        .collect()
                } else {
                    // Iterate over positional parameters
                    self.positional_params.clone()
                };

                for item in items {
                    env::set_var(var, &item);
                    self.variables.insert(var.clone(), item);

                    for cmd in body {
                        self.execute_command(cmd)?;
                    }
                }

                Ok(self.last_status)
            }

            CompoundCommand::ForArith {
                init,
                cond,
                step,
                body,
            } => {
                // C-style for loop: for ((init; cond; step))
                // Execute init expression (use evaluate_arithmetic_expr for assignment support)
                if !init.is_empty() {
                    self.evaluate_arithmetic_expr(init);
                }

                // Loop while condition is true
                loop {
                    // Evaluate condition (use eval_arith_expr for comparison result)
                    if !cond.is_empty() {
                        let cond_result = self.eval_arith_expr(cond);
                        if cond_result == 0 {
                            break;
                        }
                    }

                    // Execute body
                    for cmd in body {
                        self.execute_command(cmd)?;
                    }

                    // Execute step (use evaluate_arithmetic_expr for assignment support like i++)
                    if !step.is_empty() {
                        self.evaluate_arithmetic_expr(step);
                    }
                }
                Ok(self.last_status)
            }

            CompoundCommand::While { condition, body } => {
                loop {
                    for cmd in condition {
                        self.execute_command(cmd)?;
                    }

                    if self.last_status != 0 {
                        break;
                    }

                    for cmd in body {
                        self.execute_command(cmd)?;
                    }
                }
                Ok(self.last_status)
            }

            CompoundCommand::Until { condition, body } => {
                loop {
                    for cmd in condition {
                        self.execute_command(cmd)?;
                    }

                    if self.last_status == 0 {
                        break;
                    }

                    for cmd in body {
                        self.execute_command(cmd)?;
                    }
                }
                Ok(self.last_status)
            }

            CompoundCommand::Case { word, cases } => {
                let value = self.expand_word(word);

                for (patterns, body, term) in cases {
                    for pattern in patterns {
                        let pat = self.expand_word(pattern);
                        if self.matches_pattern(&value, &pat) {
                            for cmd in body {
                                self.execute_command(cmd)?;
                            }

                            match term {
                                CaseTerminator::Break => return Ok(self.last_status),
                                CaseTerminator::Fallthrough => {
                                    // Continue to next case body
                                }
                                CaseTerminator::Continue => {
                                    // Continue pattern matching
                                    break;
                                }
                            }
                        }
                    }
                }

                Ok(self.last_status)
            }

            CompoundCommand::Select { var, words, body } => {
                // Simplified: just use first word
                if let Some(words) = words {
                    if let Some(first) = words.first() {
                        let val = self.expand_word(first);
                        env::set_var(var, &val);
                        for cmd in body {
                            self.execute_command(cmd)?;
                        }
                    }
                }
                Ok(self.last_status)
            }

            CompoundCommand::Cond(expr) => {
                let result = self.eval_cond_expr(expr);
                self.last_status = if result { 0 } else { 1 };
                Ok(self.last_status)
            }

            CompoundCommand::Arith(expr) => {
                // Evaluate arithmetic expression and set variables
                let result = self.evaluate_arithmetic_expr(expr);
                // (( )) returns 0 if result is non-zero, 1 if result is zero
                self.last_status = if result != 0 { 0 } else { 1 };
                Ok(self.last_status)
            }

            CompoundCommand::Coproc { name, body } => {
                // Create pipes for stdin and stdout
                let (stdin_read, stdin_write) =
                    os_pipe::pipe().map_err(|e| format!("Cannot create pipe: {}", e))?;
                let (stdout_read, stdout_write) =
                    os_pipe::pipe().map_err(|e| format!("Cannot create pipe: {}", e))?;

                // Get the command to run
                let cmd_str = match body.as_ref() {
                    ShellCommand::Simple(simple) => simple
                        .words
                        .iter()
                        .map(|w| self.expand_word(w))
                        .collect::<Vec<_>>()
                        .join(" "),
                    ShellCommand::Compound(CompoundCommand::BraceGroup(cmds)) => {
                        // Just run as a subshell with the commands
                        // For simplicity, we'll use bash -c
                        "bash -c 'true'".to_string()
                    }
                    _ => "true".to_string(),
                };

                // Fork and run the command in background with redirected stdin/stdout
                let parts: Vec<&str> = cmd_str.split_whitespace().collect();
                if parts.is_empty() {
                    return Ok(0);
                }

                let mut command = Command::new(parts[0]);
                if parts.len() > 1 {
                    command.args(&parts[1..]);
                }

                use std::os::unix::io::{FromRawFd, IntoRawFd};

                command.stdin(unsafe { Stdio::from_raw_fd(stdin_read.into_raw_fd()) });
                command.stdout(unsafe { Stdio::from_raw_fd(stdout_write.into_raw_fd()) });

                match command.spawn() {
                    Ok(child) => {
                        let pid = child.id();
                        let coproc_name = name.clone().unwrap_or_else(|| "COPROC".to_string());

                        // Store file descriptors in environment-like variables
                        // COPROC[0] = read from coproc (stdout_read)
                        // COPROC[1] = write to coproc (stdin_write)
                        let read_fd = stdout_read.into_raw_fd();
                        let write_fd = stdin_write.into_raw_fd();

                        self.arrays.insert(
                            coproc_name.clone(),
                            vec![read_fd.to_string(), write_fd.to_string()],
                        );

                        // Also store PID
                        self.variables
                            .insert(format!("{}_PID", coproc_name), pid.to_string());

                        let cmd_str_clone = cmd_str.clone();
                        self.jobs.add_job(child, cmd_str_clone, JobState::Running);

                        Ok(0)
                    }
                    Err(e) => {
                        if e.kind() == io::ErrorKind::NotFound {
                            eprintln!("zshrs: command not found: {}", parts[0]);
                            Ok(127)
                        } else {
                            Err(format!("zshrs: coproc: {}: {}", parts[0], e))
                        }
                    }
                }
            }
        }
    }

    /// Expand a word with brace and glob expansion (for command arguments)
    fn expand_word_glob(&mut self, word: &ShellWord) -> Vec<String> {
        match word {
            ShellWord::SingleQuoted(s) => vec![s.clone()],
            ShellWord::DoubleQuoted(parts) => {
                // Double quotes prevent glob and brace expansion
                vec![parts.iter().map(|p| self.expand_word(p)).collect()]
            }
            _ => {
                let expanded = self.expand_word(word);

                // First do brace expansion
                let brace_expanded = self.expand_braces(&expanded);

                // Then glob expansion on each result
                brace_expanded
                    .into_iter()
                    .flat_map(|s| {
                        if s.contains('*')
                            || s.contains('?')
                            || s.contains('[')
                            || self.has_extglob_pattern(&s)
                        {
                            self.expand_glob(&s)
                        } else {
                            vec![s]
                        }
                    })
                    .collect()
            }
        }
    }

    /// Expand brace patterns like {a,b,c} and {1..10}
    fn expand_braces(&self, s: &str) -> Vec<String> {
        // Find a brace pattern
        let mut depth = 0;
        let mut brace_start = None;

        for (i, c) in s.char_indices() {
            match c {
                '{' => {
                    if depth == 0 {
                        brace_start = Some(i);
                    }
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(start) = brace_start {
                            let prefix = &s[..start];
                            let content = &s[start + 1..i];
                            let suffix = &s[i + 1..];

                            // Check if this is a sequence {a..b} or a list {a,b,c}
                            let expansions = if content.contains("..") {
                                self.expand_brace_sequence(content)
                            } else if content.contains(',') {
                                self.expand_brace_list(content)
                            } else {
                                // Not a valid brace expansion, return as-is
                                return vec![s.to_string()];
                            };

                            // Combine prefix, expansions, and suffix
                            let mut results = Vec::new();
                            for exp in expansions {
                                let combined = format!("{}{}{}", prefix, exp, suffix);
                                // Recursively expand any remaining braces
                                results.extend(self.expand_braces(&combined));
                            }
                            return results;
                        }
                    }
                }
                _ => {}
            }
        }

        // No brace expansion found
        vec![s.to_string()]
    }

    /// Expand comma-separated brace list like {a,b,c}
    fn expand_brace_list(&self, content: &str) -> Vec<String> {
        // Split by comma, but respect nested braces
        let mut parts = Vec::new();
        let mut current = String::new();
        let mut depth = 0;

        for c in content.chars() {
            match c {
                '{' => {
                    depth += 1;
                    current.push(c);
                }
                '}' => {
                    depth -= 1;
                    current.push(c);
                }
                ',' if depth == 0 => {
                    parts.push(current.clone());
                    current.clear();
                }
                _ => current.push(c),
            }
        }
        parts.push(current);

        parts
    }

    /// Expand sequence brace pattern like {1..10} or {a..z}
    fn expand_brace_sequence(&self, content: &str) -> Vec<String> {
        let parts: Vec<&str> = content.splitn(3, "..").collect();
        if parts.len() < 2 {
            return vec![content.to_string()];
        }

        let start = parts[0];
        let end = parts[1];
        let step: i64 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);

        // Try numeric sequence
        if let (Ok(start_num), Ok(end_num)) = (start.parse::<i64>(), end.parse::<i64>()) {
            let mut results = Vec::new();
            if start_num <= end_num {
                let mut i = start_num;
                while i <= end_num {
                    results.push(i.to_string());
                    i += step;
                }
            } else {
                let mut i = start_num;
                while i >= end_num {
                    results.push(i.to_string());
                    i -= step;
                }
            }
            return results;
        }

        // Try character sequence
        if start.len() == 1 && end.len() == 1 {
            let start_char = start.chars().next().unwrap();
            let end_char = end.chars().next().unwrap();
            let mut results = Vec::new();

            if start_char <= end_char {
                let mut c = start_char;
                while c <= end_char {
                    results.push(c.to_string());
                    c = (c as u8 + step as u8) as char;
                    if c as u8 > end_char as u8 {
                        break;
                    }
                }
            } else {
                let mut c = start_char;
                while c >= end_char {
                    results.push(c.to_string());
                    if (c as u8) < step as u8 {
                        break;
                    }
                    c = (c as u8 - step as u8) as char;
                }
            }
            return results;
        }

        vec![content.to_string()]
    }

    /// Expand glob pattern to matching files
    fn expand_glob(&self, pattern: &str) -> Vec<String> {
        // Check for zsh glob qualifiers at end: *(.) *(/) *(@) etc.
        let (glob_pattern, qualifiers) = self.parse_glob_qualifiers(pattern);

        // Check for extended glob patterns: ?(pat), *(pat), +(pat), @(pat), !(pat)
        if self.has_extglob_pattern(&glob_pattern) {
            let expanded = self.expand_extglob(&glob_pattern);
            return self.filter_by_qualifiers(expanded, &qualifiers);
        }

        // Check for zsh-style options
        let nullglob = self.options.get("nullglob").copied().unwrap_or(false);
        let dotglob = self.options.get("dotglob").copied().unwrap_or(false);
        let nocaseglob = self.options.get("nocaseglob").copied().unwrap_or(false);

        let options = glob::MatchOptions {
            case_sensitive: !nocaseglob,
            require_literal_separator: false,
            require_literal_leading_dot: !dotglob,
        };

        match glob::glob_with(&glob_pattern, options) {
            Ok(paths) => {
                let mut expanded: Vec<String> = paths
                    .filter_map(|p| p.ok())
                    .map(|p| p.to_string_lossy().to_string())
                    .collect();

                // Apply glob qualifiers
                expanded = self.filter_by_qualifiers(expanded, &qualifiers);

                // Sort for consistent output
                expanded.sort();

                if expanded.is_empty() {
                    if nullglob {
                        // nullglob: return empty vec when no matches
                        vec![]
                    } else {
                        // Default: return pattern as-is
                        vec![pattern.to_string()]
                    }
                } else {
                    expanded
                }
            }
            Err(_) => {
                if nullglob {
                    vec![]
                } else {
                    vec![pattern.to_string()]
                }
            }
        }
    }

    /// Parse zsh glob qualifiers from the end of a pattern
    /// Returns (pattern_without_qualifiers, qualifiers_string)
    fn parse_glob_qualifiers(&self, pattern: &str) -> (String, String) {
        // Check if pattern ends with (...) that looks like qualifiers
        // Qualifiers are single chars like . / @ * % or combinations
        if !pattern.ends_with(')') {
            return (pattern.to_string(), String::new());
        }

        // Find matching opening paren
        let chars: Vec<char> = pattern.chars().collect();
        let mut depth = 0;
        let mut qual_start = None;

        for i in (0..chars.len()).rev() {
            match chars[i] {
                ')' => depth += 1,
                '(' => {
                    depth -= 1;
                    if depth == 0 {
                        qual_start = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }

        if let Some(start) = qual_start {
            let qual_content: String = chars[start + 1..chars.len() - 1].iter().collect();

            // Check if this looks like glob qualifiers (not extglob)
            // Qualifiers are things like: . / @ * % r w x ^ - etc.
            // Extglob would have | inside
            if !qual_content.contains('|') && self.looks_like_glob_qualifiers(&qual_content) {
                let base_pattern: String = chars[..start].iter().collect();
                return (base_pattern, qual_content);
            }
        }

        (pattern.to_string(), String::new())
    }

    /// Check if string looks like glob qualifiers
    fn looks_like_glob_qualifiers(&self, s: &str) -> bool {
        if s.is_empty() {
            return false;
        }
        // Valid qualifier chars: . / @ = p * % r w x A I E R W X s S t ^ - + :
        // Also numbers for depth limits, and things like [1,5] for ranges
        let valid_chars = "./@=p*%brwxAIERWXsStfedDLNnMmcaou^-+:0123456789,[]FT";
        s.chars()
            .all(|c| valid_chars.contains(c) || c.is_whitespace())
    }

    /// Filter file list by glob qualifiers
    fn filter_by_qualifiers(&self, files: Vec<String>, qualifiers: &str) -> Vec<String> {
        if qualifiers.is_empty() {
            return files;
        }

        let mut result = files;
        let mut negate = false;
        let mut chars = qualifiers.chars().peekable();

        while let Some(c) = chars.next() {
            match c {
                // Negation
                '^' => negate = !negate,

                // File types
                '.' => {
                    result = result
                        .into_iter()
                        .filter(|f| {
                            let is_file = std::path::Path::new(f).is_file();
                            if negate {
                                !is_file
                            } else {
                                is_file
                            }
                        })
                        .collect();
                    negate = false;
                }
                '/' => {
                    result = result
                        .into_iter()
                        .filter(|f| {
                            let is_dir = std::path::Path::new(f).is_dir();
                            if negate {
                                !is_dir
                            } else {
                                is_dir
                            }
                        })
                        .collect();
                    negate = false;
                }
                '@' => {
                    result = result
                        .into_iter()
                        .filter(|f| {
                            let is_link = std::path::Path::new(f).is_symlink();
                            if negate {
                                !is_link
                            } else {
                                is_link
                            }
                        })
                        .collect();
                    negate = false;
                }
                '=' => {
                    // Sockets
                    result = result
                        .into_iter()
                        .filter(|f| {
                            use std::os::unix::fs::FileTypeExt;
                            let is_socket = std::fs::symlink_metadata(f)
                                .map(|m| m.file_type().is_socket())
                                .unwrap_or(false);
                            if negate {
                                !is_socket
                            } else {
                                is_socket
                            }
                        })
                        .collect();
                    negate = false;
                }
                'p' => {
                    // Named pipes (FIFOs)
                    result = result
                        .into_iter()
                        .filter(|f| {
                            use std::os::unix::fs::FileTypeExt;
                            let is_fifo = std::fs::symlink_metadata(f)
                                .map(|m| m.file_type().is_fifo())
                                .unwrap_or(false);
                            if negate {
                                !is_fifo
                            } else {
                                is_fifo
                            }
                        })
                        .collect();
                    negate = false;
                }
                '*' => {
                    // Executable files
                    result = result
                        .into_iter()
                        .filter(|f| {
                            use std::os::unix::fs::PermissionsExt;
                            let is_exec = std::fs::metadata(f)
                                .map(|m| m.is_file() && (m.permissions().mode() & 0o111) != 0)
                                .unwrap_or(false);
                            if negate {
                                !is_exec
                            } else {
                                is_exec
                            }
                        })
                        .collect();
                    negate = false;
                }
                '%' => {
                    // Device files
                    let next = chars.peek().copied();
                    result = result
                        .into_iter()
                        .filter(|f| {
                            use std::os::unix::fs::FileTypeExt;
                            let meta = std::fs::symlink_metadata(f);
                            let is_device = match next {
                                Some('b') => meta
                                    .map(|m| m.file_type().is_block_device())
                                    .unwrap_or(false),
                                Some('c') => meta
                                    .map(|m| m.file_type().is_char_device())
                                    .unwrap_or(false),
                                _ => meta
                                    .map(|m| {
                                        m.file_type().is_block_device()
                                            || m.file_type().is_char_device()
                                    })
                                    .unwrap_or(false),
                            };
                            if negate {
                                !is_device
                            } else {
                                is_device
                            }
                        })
                        .collect();
                    if next == Some('b') || next == Some('c') {
                        chars.next();
                    }
                    negate = false;
                }

                // Permission qualifiers
                'r' => {
                    // Owner-readable (0400)
                    result = self.filter_by_permission(result, 0o400, negate);
                    negate = false;
                }
                'w' => {
                    // Owner-writable (0200)
                    result = self.filter_by_permission(result, 0o200, negate);
                    negate = false;
                }
                'x' => {
                    // Owner-executable (0100)
                    result = self.filter_by_permission(result, 0o100, negate);
                    negate = false;
                }
                'A' => {
                    // Group-readable (0040)
                    result = self.filter_by_permission(result, 0o040, negate);
                    negate = false;
                }
                'I' => {
                    // Group-writable (0020)
                    result = self.filter_by_permission(result, 0o020, negate);
                    negate = false;
                }
                'E' => {
                    // Group-executable (0010)
                    result = self.filter_by_permission(result, 0o010, negate);
                    negate = false;
                }
                'R' => {
                    // World-readable (0004)
                    result = self.filter_by_permission(result, 0o004, negate);
                    negate = false;
                }
                'W' => {
                    // World-writable (0002)
                    result = self.filter_by_permission(result, 0o002, negate);
                    negate = false;
                }
                'X' => {
                    // World-executable (0001)
                    result = self.filter_by_permission(result, 0o001, negate);
                    negate = false;
                }
                's' => {
                    // Setuid (04000)
                    result = self.filter_by_permission(result, 0o4000, negate);
                    negate = false;
                }
                'S' => {
                    // Setgid (02000)
                    result = self.filter_by_permission(result, 0o2000, negate);
                    negate = false;
                }
                't' => {
                    // Sticky bit (01000)
                    result = self.filter_by_permission(result, 0o1000, negate);
                    negate = false;
                }

                // Full/empty directories
                'F' => {
                    // Non-empty directories
                    result = result
                        .into_iter()
                        .filter(|f| {
                            let path = std::path::Path::new(f);
                            let is_nonempty = path.is_dir()
                                && std::fs::read_dir(path)
                                    .map(|mut d| d.next().is_some())
                                    .unwrap_or(false);
                            if negate {
                                !is_nonempty
                            } else {
                                is_nonempty
                            }
                        })
                        .collect();
                    negate = false;
                }

                // Ownership
                'U' => {
                    // Owned by effective UID
                    result = result
                        .into_iter()
                        .filter(|f| {
                            use std::os::unix::fs::MetadataExt;
                            let is_owned = std::fs::metadata(f)
                                .map(|m| m.uid() == unsafe { libc::geteuid() })
                                .unwrap_or(false);
                            if negate {
                                !is_owned
                            } else {
                                is_owned
                            }
                        })
                        .collect();
                    negate = false;
                }
                'G' => {
                    // Owned by effective GID
                    result = result
                        .into_iter()
                        .filter(|f| {
                            use std::os::unix::fs::MetadataExt;
                            let is_owned = std::fs::metadata(f)
                                .map(|m| m.gid() == unsafe { libc::getegid() })
                                .unwrap_or(false);
                            if negate {
                                !is_owned
                            } else {
                                is_owned
                            }
                        })
                        .collect();
                    negate = false;
                }

                // Sorting modifiers
                'o' => {
                    // Sort by name (ascending) - already default
                    if chars.peek() == Some(&'n') {
                        chars.next();
                        // Sort by name
                        result.sort();
                    } else if chars.peek() == Some(&'L') {
                        chars.next();
                        // Sort by size
                        result.sort_by_key(|f| std::fs::metadata(f).map(|m| m.len()).unwrap_or(0));
                    } else if chars.peek() == Some(&'m') {
                        chars.next();
                        // Sort by modification time
                        result.sort_by_key(|f| {
                            std::fs::metadata(f)
                                .and_then(|m| m.modified())
                                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                        });
                    } else if chars.peek() == Some(&'a') {
                        chars.next();
                        // Sort by access time
                        result.sort_by_key(|f| {
                            std::fs::metadata(f)
                                .and_then(|m| m.accessed())
                                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                        });
                    }
                }
                'O' => {
                    // Reverse sort
                    if chars.peek() == Some(&'n') {
                        chars.next();
                        result.sort();
                        result.reverse();
                    } else if chars.peek() == Some(&'L') {
                        chars.next();
                        result.sort_by_key(|f| std::fs::metadata(f).map(|m| m.len()).unwrap_or(0));
                        result.reverse();
                    } else if chars.peek() == Some(&'m') {
                        chars.next();
                        result.sort_by_key(|f| {
                            std::fs::metadata(f)
                                .and_then(|m| m.modified())
                                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                        });
                        result.reverse();
                    } else {
                        // Just reverse current order
                        result.reverse();
                    }
                }

                // Subscript range [n] or [n,m]
                '[' => {
                    let mut range_str = String::new();
                    while let Some(&ch) = chars.peek() {
                        if ch == ']' {
                            chars.next();
                            break;
                        }
                        range_str.push(chars.next().unwrap());
                    }

                    if let Some((start, end)) = self.parse_subscript_range(&range_str, result.len())
                    {
                        result = result.into_iter().skip(start).take(end - start).collect();
                    }
                }

                // Depth limit (for **/)
                'D' => {
                    // Include dotfiles (handled by dotglob)
                }
                'N' => {
                    // Nullglob for this pattern
                }

                // Unknown qualifier - ignore
                _ => {}
            }
        }

        result
    }

    /// Filter files by permission bits
    fn filter_by_permission(&self, files: Vec<String>, mode: u32, negate: bool) -> Vec<String> {
        use std::os::unix::fs::PermissionsExt;
        files
            .into_iter()
            .filter(|f| {
                let has_perm = std::fs::metadata(f)
                    .map(|m| (m.permissions().mode() & mode) != 0)
                    .unwrap_or(false);
                if negate {
                    !has_perm
                } else {
                    has_perm
                }
            })
            .collect()
    }

    /// Parse subscript range like "1" or "1,5" or "-1" or "1,-1"
    fn parse_subscript_range(&self, s: &str, len: usize) -> Option<(usize, usize)> {
        if s.is_empty() || len == 0 {
            return None;
        }

        let parts: Vec<&str> = s.split(',').collect();

        let parse_idx = |idx_str: &str| -> Option<usize> {
            let idx: i64 = idx_str.trim().parse().ok()?;
            if idx < 0 {
                // Negative index from end
                let abs = (-idx) as usize;
                if abs > len {
                    None
                } else {
                    Some(len - abs)
                }
            } else if idx == 0 {
                Some(0)
            } else {
                // 1-indexed
                Some((idx as usize).saturating_sub(1).min(len))
            }
        };

        match parts.len() {
            1 => {
                // Single element [n]
                let idx = parse_idx(parts[0])?;
                Some((idx, idx + 1))
            }
            2 => {
                // Range [n,m]
                let start = parse_idx(parts[0])?;
                let end = parse_idx(parts[1])?.saturating_add(1);
                Some((start.min(end), start.max(end)))
            }
            _ => None,
        }
    }

    /// Check if pattern contains extended glob syntax
    fn has_extglob_pattern(&self, pattern: &str) -> bool {
        let chars: Vec<char> = pattern.chars().collect();
        for i in 0..chars.len().saturating_sub(1) {
            if (chars[i] == '?'
                || chars[i] == '*'
                || chars[i] == '+'
                || chars[i] == '@'
                || chars[i] == '!')
                && chars[i + 1] == '('
            {
                return true;
            }
        }
        false
    }

    /// Convert extended glob pattern to regex
    fn extglob_to_regex(&self, pattern: &str) -> String {
        let mut regex = String::from("^");
        let chars: Vec<char> = pattern.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let c = chars[i];

            // Check for extglob patterns
            if i + 1 < chars.len() && chars[i + 1] == '(' {
                match c {
                    '?' => {
                        // ?(pattern) - zero or one occurrence
                        let (inner, end) = self.extract_extglob_inner(&chars, i + 2);
                        let inner_regex = self.extglob_inner_to_regex(&inner);
                        regex.push_str(&format!("({})?", inner_regex));
                        i = end + 1;
                        continue;
                    }
                    '*' => {
                        // *(pattern) - zero or more occurrences
                        let (inner, end) = self.extract_extglob_inner(&chars, i + 2);
                        let inner_regex = self.extglob_inner_to_regex(&inner);
                        regex.push_str(&format!("({})*", inner_regex));
                        i = end + 1;
                        continue;
                    }
                    '+' => {
                        // +(pattern) - one or more occurrences
                        let (inner, end) = self.extract_extglob_inner(&chars, i + 2);
                        let inner_regex = self.extglob_inner_to_regex(&inner);
                        regex.push_str(&format!("({})+", inner_regex));
                        i = end + 1;
                        continue;
                    }
                    '@' => {
                        // @(pattern) - exactly one occurrence
                        let (inner, end) = self.extract_extglob_inner(&chars, i + 2);
                        let inner_regex = self.extglob_inner_to_regex(&inner);
                        regex.push_str(&format!("({})", inner_regex));
                        i = end + 1;
                        continue;
                    }
                    '!' => {
                        // !(pattern) - handled specially in expand_extglob
                        // Just skip this extglob for regex, will do manual filtering
                        let (_, end) = self.extract_extglob_inner(&chars, i + 2);
                        regex.push_str(".*"); // Match anything, we filter later
                        i = end + 1;
                        continue;
                    }
                    _ => {}
                }
            }

            // Handle regular glob characters
            match c {
                '*' => regex.push_str(".*"),
                '?' => regex.push('.'),
                '.' => regex.push_str("\\."),
                '[' => {
                    regex.push('[');
                    i += 1;
                    while i < chars.len() && chars[i] != ']' {
                        if chars[i] == '!' && regex.ends_with('[') {
                            regex.push('^');
                        } else {
                            regex.push(chars[i]);
                        }
                        i += 1;
                    }
                    regex.push(']');
                }
                '^' | '$' | '(' | ')' | '{' | '}' | '|' | '\\' => {
                    regex.push('\\');
                    regex.push(c);
                }
                _ => regex.push(c),
            }
            i += 1;
        }

        regex.push('$');
        regex
    }

    /// Extract the inner part of an extglob pattern (until closing paren)
    fn extract_extglob_inner(&self, chars: &[char], start: usize) -> (String, usize) {
        let mut inner = String::new();
        let mut depth = 1;
        let mut i = start;

        while i < chars.len() && depth > 0 {
            if chars[i] == '(' {
                depth += 1;
            } else if chars[i] == ')' {
                depth -= 1;
                if depth == 0 {
                    return (inner, i);
                }
            }
            inner.push(chars[i]);
            i += 1;
        }

        (inner, i)
    }

    /// Convert the inner part of extglob (handles | for alternation)
    fn extglob_inner_to_regex(&self, inner: &str) -> String {
        // Split by | and convert each alternative
        let alternatives: Vec<String> = inner
            .split('|')
            .map(|alt| {
                let mut result = String::new();
                for c in alt.chars() {
                    match c {
                        '*' => result.push_str(".*"),
                        '?' => result.push('.'),
                        '.' => result.push_str("\\."),
                        '^' | '$' | '(' | ')' | '{' | '}' | '\\' => {
                            result.push('\\');
                            result.push(c);
                        }
                        _ => result.push(c),
                    }
                }
                result
            })
            .collect();

        alternatives.join("|")
    }

    /// Expand extended glob pattern
    fn expand_extglob(&self, pattern: &str) -> Vec<String> {
        // Determine directory to search
        let (search_dir, file_pattern) = if let Some(last_slash) = pattern.rfind('/') {
            (&pattern[..last_slash], &pattern[last_slash + 1..])
        } else {
            (".", pattern)
        };

        // Check for !(pattern) - negative matching
        if let Some((neg_pat, suffix)) = self.extract_neg_extglob(file_pattern) {
            return self.expand_neg_extglob(search_dir, &neg_pat, &suffix, pattern);
        }

        // Convert file pattern to regex for positive extglob
        let regex_str = self.extglob_to_regex(file_pattern);

        let re = match regex::Regex::new(&regex_str) {
            Ok(r) => r,
            Err(_) => return vec![pattern.to_string()],
        };

        let mut results = Vec::new();

        if let Ok(entries) = std::fs::read_dir(search_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                // Skip hidden files unless pattern starts with .
                if name.starts_with('.') && !file_pattern.starts_with('.') {
                    continue;
                }

                if re.is_match(&name) {
                    let full_path = if search_dir == "." {
                        name
                    } else {
                        format!("{}/{}", search_dir, name)
                    };
                    results.push(full_path);
                }
            }
        }

        if results.is_empty() {
            vec![pattern.to_string()]
        } else {
            results.sort();
            results
        }
    }

    /// Handle !(pattern) negative extglob expansion
    fn expand_neg_extglob(
        &self,
        search_dir: &str,
        neg_pat: &str,
        suffix: &str,
        original_pattern: &str,
    ) -> Vec<String> {
        let mut results = Vec::new();

        if let Ok(entries) = std::fs::read_dir(search_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                // Skip hidden files
                if name.starts_with('.') {
                    continue;
                }

                // File must end with suffix
                if !name.ends_with(suffix) {
                    continue;
                }

                let basename = &name[..name.len() - suffix.len()];
                // Check if basename matches any negated alternative
                let alts: Vec<&str> = neg_pat.split('|').collect();
                let matches_neg = alts.iter().any(|alt| {
                    if alt.contains('*') || alt.contains('?') {
                        let alt_re = self.extglob_inner_to_regex(alt);
                        if let Ok(r) = regex::Regex::new(&format!("^{}$", alt_re)) {
                            r.is_match(basename)
                        } else {
                            *alt == basename
                        }
                    } else {
                        *alt == basename
                    }
                });

                if !matches_neg {
                    let full_path = if search_dir == "." {
                        name
                    } else {
                        format!("{}/{}", search_dir, name)
                    };
                    results.push(full_path);
                }
            }
        }

        if results.is_empty() {
            vec![original_pattern.to_string()]
        } else {
            results.sort();
            results
        }
    }

    /// Extract !(pattern) info from file pattern, returns (inner_pattern, suffix)
    fn extract_neg_extglob(&self, pattern: &str) -> Option<(String, String)> {
        let chars: Vec<char> = pattern.chars().collect();
        if chars.len() >= 3 && chars[0] == '!' && chars[1] == '(' {
            let mut depth = 1;
            let mut i = 2;
            while i < chars.len() && depth > 0 {
                if chars[i] == '(' {
                    depth += 1;
                } else if chars[i] == ')' {
                    depth -= 1;
                }
                i += 1;
            }
            if depth == 0 {
                let inner: String = chars[2..i - 1].iter().collect();
                let suffix: String = chars[i..].iter().collect();
                return Some((inner, suffix));
            }
        }
        None
    }

    /// Expand a word with word splitting (for contexts like `for x in $words`)
    fn expand_word_split(&mut self, word: &ShellWord) -> Vec<String> {
        match word {
            ShellWord::Literal(s) => {
                // Check if this is an array expansion ${arr[@]}
                self.expand_string_split(s)
            }
            ShellWord::SingleQuoted(s) => vec![s.clone()],
            ShellWord::DoubleQuoted(parts) => {
                // Double quotes prevent word splitting
                vec![parts.iter().map(|p| self.expand_word(p)).collect()]
            }
            ShellWord::Variable(name) => {
                let val = env::var(name).unwrap_or_default();
                self.split_words(&val)
            }
            ShellWord::VariableBraced(name, modifier) => {
                let val = env::var(name).ok();
                let expanded = self.apply_var_modifier(name, val, modifier.as_deref());
                self.split_words(&expanded)
            }
            ShellWord::ArrayVar(name, index) => {
                let idx_str = self.expand_word(index);
                if idx_str == "@" || idx_str == "*" {
                    // ${arr[@]} returns each element as separate word
                    self.arrays.get(name).cloned().unwrap_or_default()
                } else {
                    vec![self.expand_array_access(name, index)]
                }
            }
            ShellWord::Glob(pattern) => match glob::glob(pattern) {
                Ok(paths) => {
                    let expanded: Vec<String> = paths
                        .filter_map(|p| p.ok())
                        .map(|p| p.to_string_lossy().to_string())
                        .collect();
                    if expanded.is_empty() {
                        vec![pattern.clone()]
                    } else {
                        expanded
                    }
                }
                Err(_) => vec![pattern.clone()],
            },
            _ => vec![self.expand_word(word)],
        }
    }

    /// Expand string with word splitting - returns Vec for array expansions
    fn expand_string_split(&mut self, s: &str) -> Vec<String> {
        let mut results: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut chars = s.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '$' {
                if chars.peek() == Some(&'{') {
                    chars.next(); // consume '{'
                    let mut brace_content = String::new();
                    let mut depth = 1;
                    while let Some(ch) = chars.next() {
                        if ch == '{' {
                            depth += 1;
                            brace_content.push(ch);
                        } else if ch == '}' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                            brace_content.push(ch);
                        } else {
                            brace_content.push(ch);
                        }
                    }

                    // Check if this is an array expansion ${arr[@]} or ${arr[*]}
                    if let Some(bracket_start) = brace_content.find('[') {
                        let var_name = &brace_content[..bracket_start];
                        let bracket_content = &brace_content[bracket_start + 1..];
                        if let Some(bracket_end) = bracket_content.find(']') {
                            let index = &bracket_content[..bracket_end];
                            if (index == "@" || index == "*")
                                && bracket_end + 1 == bracket_content.len()
                            {
                                // This is ${arr[@]} - expand to separate elements
                                if !current.is_empty() {
                                    results.push(current.clone());
                                    current.clear();
                                }
                                if let Some(arr) = self.arrays.get(var_name) {
                                    results.extend(arr.clone());
                                }
                                continue;
                            }
                        }
                    }

                    // Not an array expansion, use normal expansion
                    current.push_str(&self.expand_braced_variable(&brace_content));
                } else {
                    // Simple variable like $var
                    let mut var_name = String::new();
                    while let Some(&ch) = chars.peek() {
                        if ch.is_alphanumeric() || ch == '_' {
                            var_name.push(chars.next().unwrap());
                        } else {
                            break;
                        }
                    }
                    let val = self.get_variable(&var_name);
                    // Split this variable's value
                    if !current.is_empty() {
                        results.push(current.clone());
                        current.clear();
                    }
                    results.extend(self.split_words(&val));
                }
            } else {
                current.push(c);
            }
        }

        if !current.is_empty() {
            results.push(current);
        }

        if results.is_empty() {
            results.push(String::new());
        }

        results
    }

    /// Split a string into words based on IFS
    fn split_words(&self, s: &str) -> Vec<String> {
        let ifs = self
            .variables
            .get("IFS")
            .cloned()
            .or_else(|| env::var("IFS").ok())
            .unwrap_or_else(|| " \t\n".to_string());

        if ifs.is_empty() {
            return vec![s.to_string()];
        }

        s.split(|c: char| ifs.contains(c))
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect()
    }

    fn expand_word(&mut self, word: &ShellWord) -> String {
        match word {
            ShellWord::Literal(s) => {
                let expanded = self.expand_string(s);
                // Don't glob-expand here, that's done in expand_word_glob
                expanded
            }
            ShellWord::SingleQuoted(s) => s.clone(),
            ShellWord::DoubleQuoted(parts) => parts.iter().map(|p| self.expand_word(p)).collect(),
            ShellWord::Variable(name) => self.get_variable(name),
            ShellWord::VariableBraced(name, modifier) => {
                let val = env::var(name).ok();
                self.apply_var_modifier(name, val, modifier.as_deref())
            }
            ShellWord::Tilde(user) => {
                if let Some(u) = user {
                    // ~user expansion (simplified)
                    format!("/home/{}", u)
                } else {
                    dirs::home_dir()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| "~".to_string())
                }
            }
            ShellWord::Glob(pattern) => {
                // Expand glob
                match glob::glob(pattern) {
                    Ok(paths) => {
                        let expanded: Vec<String> = paths
                            .filter_map(|p| p.ok())
                            .map(|p| p.to_string_lossy().to_string())
                            .collect();
                        if expanded.is_empty() {
                            pattern.clone()
                        } else {
                            expanded.join(" ")
                        }
                    }
                    Err(_) => pattern.clone(),
                }
            }
            ShellWord::Concat(parts) => parts.iter().map(|p| self.expand_word(p)).collect(),
            ShellWord::CommandSub(cmd) => self.execute_command_substitution(cmd),
            ShellWord::ProcessSubIn(cmd) => self.execute_process_sub_in(cmd),
            ShellWord::ProcessSubOut(cmd) => self.execute_process_sub_out(cmd),
            ShellWord::ArithSub(expr) => self.evaluate_arithmetic(expr),
            ShellWord::ArrayVar(name, index) => self.expand_array_access(name, index),
            ShellWord::ArrayLiteral(elements) => elements
                .iter()
                .map(|e| self.expand_word(e))
                .collect::<Vec<_>>()
                .join(" "),
        }
    }

    fn expand_braced_variable(&mut self, content: &str) -> String {
        // Handle zsh-style parameter expansion flags ${(flags)var}
        if content.starts_with('(') {
            if let Some(close_paren) = content.find(')') {
                let flags_str = &content[1..close_paren];
                let rest = &content[close_paren + 1..];
                let flags = self.parse_zsh_flags(flags_str);

                // Get the base variable value
                let var_name = rest
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("");
                let mut val = self.get_variable(var_name);

                // Apply flags in order
                for flag in &flags {
                    val = self.apply_zsh_param_flag(&val, var_name, flag);
                }
                return val;
            }
        }

        // Handle ${#arr[@]} - array length
        if content.starts_with('#') {
            let rest = &content[1..];
            if let Some(bracket_start) = rest.find('[') {
                let var_name = &rest[..bracket_start];
                let bracket_content = &rest[bracket_start + 1..];
                if let Some(bracket_end) = bracket_content.find(']') {
                    let index = &bracket_content[..bracket_end];
                    if index == "@" || index == "*" {
                        // ${#arr[@]} - return array length
                        return self
                            .arrays
                            .get(var_name)
                            .map(|arr| arr.len().to_string())
                            .unwrap_or_else(|| "0".to_string());
                    }
                }
            }
            // ${#var} - string length
            let val = self.get_variable(rest);
            return val.len().to_string();
        }

        // Handle ${arr[idx]}
        if let Some(bracket_start) = content.find('[') {
            let var_name = &content[..bracket_start];
            let bracket_content = &content[bracket_start + 1..];
            if let Some(bracket_end) = bracket_content.find(']') {
                let index = &bracket_content[..bracket_end];

                if index == "@" || index == "*" {
                    // ${arr[@]} - return all elements
                    return self
                        .arrays
                        .get(var_name)
                        .map(|arr| arr.join(" "))
                        .unwrap_or_default();
                } else if let Ok(idx) = index.parse::<i64>() {
                    // ${arr[1]} - return element at index (1-indexed in zsh)
                    return self
                        .arrays
                        .get(var_name)
                        .and_then(|arr| {
                            let actual_idx = if idx > 0 { (idx - 1) as usize } else { 0 };
                            arr.get(actual_idx).cloned()
                        })
                        .unwrap_or_default();
                } else {
                    // Index is a variable, expand it first
                    let expanded_idx = self.get_variable(index);
                    if let Ok(idx) = expanded_idx.parse::<i64>() {
                        return self
                            .arrays
                            .get(var_name)
                            .and_then(|arr| {
                                let actual_idx = if idx > 0 { (idx - 1) as usize } else { 0 };
                                arr.get(actual_idx).cloned()
                            })
                            .unwrap_or_default();
                    }
                }
            }
        }

        // Handle ${var:-default}, ${var:=default}, ${var:?error}, ${var:+alternate}
        if let Some(colon_pos) = content.find(':') {
            let var_name = &content[..colon_pos];
            let rest = &content[colon_pos + 1..];
            let val = self.get_variable(var_name);
            let val_opt = if val.is_empty() {
                None
            } else {
                Some(val.clone())
            };

            if rest.starts_with('-') {
                // ${var:-default}
                return match val_opt {
                    Some(v) if !v.is_empty() => v,
                    _ => self.expand_string(&rest[1..]),
                };
            } else if rest.starts_with('=') {
                // ${var:=default}
                return match val_opt {
                    Some(v) if !v.is_empty() => v,
                    _ => {
                        let default = self.expand_string(&rest[1..]);
                        self.variables.insert(var_name.to_string(), default.clone());
                        default
                    }
                };
            } else if rest.starts_with('?') {
                // ${var:?error}
                return match val_opt {
                    Some(v) if !v.is_empty() => v,
                    _ => {
                        let msg = self.expand_string(&rest[1..]);
                        eprintln!("zshrs: {}: {}", var_name, msg);
                        String::new()
                    }
                };
            } else if rest.starts_with('+') {
                // ${var:+alternate}
                return match val_opt {
                    Some(v) if !v.is_empty() => self.expand_string(&rest[1..]),
                    _ => String::new(),
                };
            } else if rest == "l" {
                // ${var:l} - lowercase (zsh history modifier style)
                return val.to_lowercase();
            } else if rest == "u" {
                // ${var:u} - uppercase (zsh history modifier style)
                return val.to_uppercase();
            } else if rest
                .chars()
                .next()
                .map(|c| c.is_ascii_digit() || c == '-')
                .unwrap_or(false)
            {
                // ${var:offset} or ${var:offset:length}
                let parts: Vec<&str> = rest.splitn(2, ':').collect();
                let offset: i64 = parts[0].parse().unwrap_or(0);
                let length: Option<usize> = parts.get(1).and_then(|s| s.parse().ok());

                let start = if offset < 0 {
                    (val.len() as i64 + offset).max(0) as usize
                } else {
                    (offset as usize).min(val.len())
                };

                return if let Some(len) = length {
                    val.chars().skip(start).take(len).collect()
                } else {
                    val.chars().skip(start).collect()
                };
            }
        }

        // Handle ${var/pattern/replacement} and ${var//pattern/replacement}
        // Only if the part before / is a valid variable name
        if let Some(slash_pos) = content.find('/') {
            let var_name = &content[..slash_pos];
            // Variable names must start with letter/underscore and contain only alnum/_
            if !var_name.is_empty()
                && var_name
                    .chars()
                    .next()
                    .map(|c| c.is_alphabetic() || c == '_')
                    .unwrap_or(false)
                && var_name.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                let rest = &content[slash_pos + 1..];
                let val = self.get_variable(var_name);

                let replace_all = rest.starts_with('/');
                let rest = if replace_all { &rest[1..] } else { rest };

                let parts: Vec<&str> = rest.splitn(2, '/').collect();
                let pattern = parts.get(0).unwrap_or(&"");
                let replacement = parts.get(1).unwrap_or(&"");

                return if replace_all {
                    val.replace(pattern, replacement)
                } else {
                    val.replacen(pattern, replacement, 1)
                };
            }
        }

        // Handle ${var#pattern} and ${var##pattern} - remove prefix
        // But only if the # is not at the start (which would be length)
        if let Some(hash_pos) = content.find('#') {
            if hash_pos > 0 {
                let var_name = &content[..hash_pos];
                // Make sure var_name looks like a valid variable name
                if var_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    let rest = &content[hash_pos + 1..];
                    let val = self.get_variable(var_name);

                    let long = rest.starts_with('#');
                    let pattern = if long { &rest[1..] } else { rest };

                    // Convert shell glob pattern to regex-style for matching prefixes
                    // Escape special regex chars, then convert glob wildcards
                    let pattern_regex = regex::escape(pattern)
                        .replace(r"\*", ".*")
                        .replace(r"\?", ".");

                    if let Ok(re) = regex::Regex::new(&format!("^{}", pattern_regex)) {
                        if long {
                            // Remove longest prefix match - find all matches and use the longest
                            let mut longest_end = 0;
                            for m in re.find_iter(&val) {
                                if m.end() > longest_end {
                                    longest_end = m.end();
                                }
                            }
                            if longest_end > 0 {
                                return val[longest_end..].to_string();
                            }
                        } else {
                            // Remove shortest prefix match
                            if let Some(m) = re.find(&val) {
                                return val[m.end()..].to_string();
                            }
                        }
                    }
                    return val;
                }
            }
        }

        // Handle ${var%pattern} and ${var%%pattern} - remove suffix
        if let Some(pct_pos) = content.find('%') {
            if pct_pos > 0 {
                let var_name = &content[..pct_pos];
                if var_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    let rest = &content[pct_pos + 1..];
                    let val = self.get_variable(var_name);

                    let long = rest.starts_with('%');
                    let pattern = if long { &rest[1..] } else { rest };

                    // Use glob pattern matching for suffix removal
                    if let Ok(glob) = glob::Pattern::new(pattern) {
                        if long {
                            // Remove longest suffix match - find earliest matching position
                            for i in 0..=val.len() {
                                if glob.matches(&val[i..]) {
                                    return val[..i].to_string();
                                }
                            }
                        } else {
                            // Remove shortest suffix match - find latest matching position
                            for i in (0..=val.len()).rev() {
                                if glob.matches(&val[i..]) {
                                    return val[..i].to_string();
                                }
                            }
                        }
                    }
                    return val;
                }
            }
        }

        // Handle ${var^} and ${var^^} - uppercase
        if let Some(caret_pos) = content.find('^') {
            let var_name = &content[..caret_pos];
            let val = self.get_variable(var_name);
            let all = content[caret_pos + 1..].starts_with('^');

            return if all {
                val.to_uppercase()
            } else {
                let mut chars = val.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().to_string() + chars.as_str(),
                    None => String::new(),
                }
            };
        }

        // Handle ${var,} and ${var,,} - lowercase
        if let Some(comma_pos) = content.find(',') {
            let var_name = &content[..comma_pos];
            let val = self.get_variable(var_name);
            let all = content[comma_pos + 1..].starts_with(',');

            return if all {
                val.to_lowercase()
            } else {
                let mut chars = val.chars();
                match chars.next() {
                    Some(first) => first.to_lowercase().to_string() + chars.as_str(),
                    None => String::new(),
                }
            };
        }

        // Handle ${!prefix*} and ${!prefix@} - expand to variable names with prefix
        if content.starts_with('!') {
            let rest = &content[1..];
            if rest.ends_with('*') || rest.ends_with('@') {
                let prefix = &rest[..rest.len() - 1];
                let mut matches: Vec<String> = self
                    .variables
                    .keys()
                    .filter(|k| k.starts_with(prefix))
                    .cloned()
                    .collect();
                // Also check arrays
                for k in self.arrays.keys() {
                    if k.starts_with(prefix) && !matches.contains(k) {
                        matches.push(k.clone());
                    }
                }
                matches.sort();
                return matches.join(" ");
            }

            // ${!var} - indirect expansion
            let var_name = self.get_variable(rest);
            return self.get_variable(&var_name);
        }

        // Default: just get the variable
        self.get_variable(content)
    }

    fn expand_array_access(&mut self, name: &str, index: &ShellWord) -> String {
        let idx_str = self.expand_word(index);

        if idx_str == "@" || idx_str == "*" {
            // Return all elements
            if let Some(arr) = self.arrays.get(name) {
                arr.join(" ")
            } else {
                String::new()
            }
        } else if let Ok(idx) = idx_str.parse::<i64>() {
            // Return element at index (zsh is 1-indexed by default)
            if let Some(arr) = self.arrays.get(name) {
                let actual_idx = if idx > 0 { (idx - 1) as usize } else { 0 };
                arr.get(actual_idx).cloned().unwrap_or_default()
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    }

    fn expand_string(&mut self, s: &str) -> String {
        let mut result = String::new();
        let mut chars = s.chars().peekable();

        while let Some(c) = chars.next() {
            // \x00 prefix marks chars from single quotes - keep them literal
            if c == '\x00' {
                if let Some(literal_char) = chars.next() {
                    result.push(literal_char);
                }
                continue;
            }
            if c == '$' {
                if chars.peek() == Some(&'(') {
                    chars.next(); // consume '('

                    // Check for $(( )) arithmetic
                    if chars.peek() == Some(&'(') {
                        chars.next(); // consume second '('
                        let expr = Self::collect_until_double_paren(&mut chars);
                        result.push_str(&self.evaluate_arithmetic(&expr));
                    } else {
                        // Command substitution $(...)
                        let cmd_str = Self::collect_until_paren(&mut chars);
                        result.push_str(&self.run_command_substitution(&cmd_str));
                    }
                } else if chars.peek() == Some(&'{') {
                    chars.next();
                    // Collect the full braced expression including brackets
                    let mut brace_content = String::new();
                    let mut depth = 1;
                    while let Some(c) = chars.next() {
                        if c == '{' {
                            depth += 1;
                            brace_content.push(c);
                        } else if c == '}' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                            brace_content.push(c);
                        } else {
                            brace_content.push(c);
                        }
                    }
                    result.push_str(&self.expand_braced_variable(&brace_content));
                } else {
                    let mut var_name = String::new();
                    while let Some(&c) = chars.peek() {
                        if c.is_alphanumeric()
                            || c == '_'
                            || c == '@'
                            || c == '*'
                            || c == '#'
                            || c == '?'
                        {
                            var_name.push(chars.next().unwrap());
                            // Handle single-char special vars
                            if matches!(
                                var_name.as_str(),
                                "@" | "*"
                                    | "#"
                                    | "?"
                                    | "0"
                                    | "1"
                                    | "2"
                                    | "3"
                                    | "4"
                                    | "5"
                                    | "6"
                                    | "7"
                                    | "8"
                                    | "9"
                            ) {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    result.push_str(&self.get_variable(&var_name));
                }
            } else if c == '`' {
                // Backtick command substitution
                let cmd_str: String = chars.by_ref().take_while(|&c| c != '`').collect();
                result.push_str(&self.run_command_substitution(&cmd_str));
            } else if c == '<' && chars.peek() == Some(&'(') {
                // Process substitution <(cmd)
                chars.next(); // consume '('
                let cmd_str = Self::collect_until_paren(&mut chars);
                result.push_str(&self.run_process_sub_in(&cmd_str));
            } else if c == '>' && chars.peek() == Some(&'(') {
                // Process substitution >(cmd)
                chars.next(); // consume '('
                let cmd_str = Self::collect_until_paren(&mut chars);
                result.push_str(&self.run_process_sub_out(&cmd_str));
            } else if c == '~' && result.is_empty() {
                if let Some(home) = dirs::home_dir() {
                    result.push_str(&home.to_string_lossy());
                } else {
                    result.push(c);
                }
            } else {
                result.push(c);
            }
        }

        result
    }

    fn collect_until_paren(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
        let mut result = String::new();
        let mut depth = 1;

        while let Some(c) = chars.next() {
            if c == '(' {
                depth += 1;
                result.push(c);
            } else if c == ')' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                result.push(c);
            } else {
                result.push(c);
            }
        }

        result
    }

    fn collect_until_double_paren(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
        let mut result = String::new();
        let mut arith_depth = 1; // Tracks $(( ... )) nesting
        let mut paren_depth = 0; // Tracks ( ... ) nesting within expression

        while let Some(c) = chars.next() {
            if c == '(' {
                if paren_depth == 0 && chars.peek() == Some(&'(') {
                    // Nested $(( - but we need to see if it's really another arithmetic
                    // For simplicity, track inner parens
                    paren_depth += 1;
                    result.push(c);
                } else {
                    paren_depth += 1;
                    result.push(c);
                }
            } else if c == ')' {
                if paren_depth > 0 {
                    // Inside nested parens, just close one level
                    paren_depth -= 1;
                    result.push(c);
                } else if chars.peek() == Some(&')') {
                    // At top level and seeing )) - this closes our arithmetic
                    chars.next();
                    arith_depth -= 1;
                    if arith_depth == 0 {
                        break;
                    }
                    result.push_str("))");
                } else {
                    // Single ) at top level - shouldn't happen in valid expression
                    result.push(c);
                }
            } else {
                result.push(c);
            }
        }

        result
    }

    fn run_process_sub_in(&mut self, cmd_str: &str) -> String {
        use std::fs;
        use std::process::Stdio;

        // Parse the command
        let mut parser = crate::shell_parse::ShellParser::new(cmd_str);
        let commands = match parser.parse_script() {
            Ok(cmds) => cmds,
            Err(_) => return String::new(),
        };

        // Create a unique FIFO in temp directory
        let fifo_path = format!("/tmp/zshrs_psub_{}", std::process::id());
        let fifo_counter = self.process_sub_counter;
        self.process_sub_counter += 1;
        let fifo_path = format!("{}_{}", fifo_path, fifo_counter);

        // Remove if exists, then create FIFO
        let _ = fs::remove_file(&fifo_path);
        if let Err(_) = nix::unistd::mkfifo(fifo_path.as_str(), nix::sys::stat::Mode::S_IRWXU) {
            return String::new();
        }

        // Spawn command that writes to the FIFO
        let fifo_clone = fifo_path.clone();
        if let Some(cmd) = commands.first() {
            if let ShellCommand::Simple(simple) = cmd {
                let words: Vec<String> = simple.words.iter().map(|w| self.expand_word(w)).collect();
                if !words.is_empty() {
                    let cmd_name = words[0].clone();
                    let args: Vec<String> = words[1..].to_vec();

                    std::thread::spawn(move || {
                        // Open FIFO for writing (will block until reader connects)
                        if let Ok(fifo) = fs::OpenOptions::new().write(true).open(&fifo_clone) {
                            let _ = Command::new(&cmd_name)
                                .args(&args)
                                .stdout(fifo)
                                .stderr(Stdio::inherit())
                                .status();
                        }
                        // Clean up FIFO after command completes
                        let _ = fs::remove_file(&fifo_clone);
                    });
                }
            }
        }

        fifo_path
    }

    fn run_process_sub_out(&mut self, cmd_str: &str) -> String {
        use std::fs;
        use std::process::Stdio;

        // Parse the command
        let mut parser = crate::shell_parse::ShellParser::new(cmd_str);
        let commands = match parser.parse_script() {
            Ok(cmds) => cmds,
            Err(_) => return String::new(),
        };

        // Create a unique FIFO in temp directory
        let fifo_path = format!("/tmp/zshrs_psub_{}", std::process::id());
        let fifo_counter = self.process_sub_counter;
        self.process_sub_counter += 1;
        let fifo_path = format!("{}_{}", fifo_path, fifo_counter);

        // Remove if exists, then create FIFO
        let _ = fs::remove_file(&fifo_path);
        if let Err(_) = nix::unistd::mkfifo(fifo_path.as_str(), nix::sys::stat::Mode::S_IRWXU) {
            return String::new();
        }

        // Spawn command that reads from the FIFO
        let fifo_clone = fifo_path.clone();
        if let Some(cmd) = commands.first() {
            if let ShellCommand::Simple(simple) = cmd {
                let words: Vec<String> = simple.words.iter().map(|w| self.expand_word(w)).collect();
                if !words.is_empty() {
                    let cmd_name = words[0].clone();
                    let args: Vec<String> = words[1..].to_vec();

                    std::thread::spawn(move || {
                        // Open FIFO for reading (will block until writer connects)
                        if let Ok(fifo) = fs::File::open(&fifo_clone) {
                            let _ = Command::new(&cmd_name)
                                .args(&args)
                                .stdin(fifo)
                                .stdout(Stdio::inherit())
                                .stderr(Stdio::inherit())
                                .status();
                        }
                        // Clean up FIFO after command completes
                        let _ = fs::remove_file(&fifo_clone);
                    });
                }
            }
        }

        fifo_path
    }

    fn run_command_substitution(&mut self, cmd_str: &str) -> String {
        use std::process::Stdio;

        // Parse and execute the command
        let mut parser = crate::shell_parse::ShellParser::new(cmd_str);
        let commands = match parser.parse_script() {
            Ok(cmds) => cmds,
            Err(_) => return String::new(),
        };

        if commands.is_empty() {
            return String::new();
        }

        // For simple external commands, capture output directly
        if let ShellCommand::Simple(simple) = &commands[0] {
            let words: Vec<String> = simple.words.iter().map(|w| self.expand_word(w)).collect();
            if words.is_empty() {
                return String::new();
            }

            let cmd_name = &words[0];
            let args = &words[1..];

            // Handle echo specially
            if cmd_name == "echo" {
                return args.join(" ");
            }

            if cmd_name == "pwd" {
                return env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
            }

            // External command
            let output = Command::new(cmd_name)
                .args(args)
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .output();

            match output {
                Ok(output) => String::from_utf8_lossy(&output.stdout)
                    .trim_end_matches('\n')
                    .to_string(),
                Err(_) => String::new(),
            }
        } else {
            String::new()
        }
    }

    /// Process substitution <(cmd) - returns FIFO path
    fn execute_process_sub_in(&mut self, cmd: &ShellCommand) -> String {
        if let ShellCommand::Simple(simple) = cmd {
            let words: Vec<String> = simple.words.iter().map(|w| self.expand_word(w)).collect();
            let cmd_str = words.join(" ");
            self.run_process_sub_in(&cmd_str)
        } else {
            String::new()
        }
    }

    /// Process substitution >(cmd) - returns FIFO path
    fn execute_process_sub_out(&mut self, cmd: &ShellCommand) -> String {
        if let ShellCommand::Simple(simple) = cmd {
            let words: Vec<String> = simple.words.iter().map(|w| self.expand_word(w)).collect();
            let cmd_str = words.join(" ");
            self.run_process_sub_out(&cmd_str)
        } else {
            String::new()
        }
    }

    fn get_variable(&self, name: &str) -> String {
        // Handle special parameters
        match name {
            "@" | "*" => self.positional_params.join(" "),
            "#" => self.positional_params.len().to_string(),
            "?" => self.last_status.to_string(),
            "0" => env::args().next().unwrap_or_default(),
            n if n.chars().all(|c| c.is_ascii_digit()) => {
                let idx: usize = n.parse().unwrap_or(0);
                if idx == 0 {
                    env::args().next().unwrap_or_default()
                } else {
                    self.positional_params
                        .get(idx - 1)
                        .cloned()
                        .unwrap_or_default()
                }
            }
            _ => {
                // Check local variables first, then env
                self.variables
                    .get(name)
                    .cloned()
                    .or_else(|| env::var(name).ok())
                    .unwrap_or_default()
            }
        }
    }

    fn apply_var_modifier(
        &mut self,
        name: &str,
        val: Option<String>,
        modifier: Option<&VarModifier>,
    ) -> String {
        match modifier {
            None => val.unwrap_or_default(),

            // ${var:-word} - use default value
            Some(VarModifier::Default(word)) => match &val {
                Some(v) if !v.is_empty() => v.clone(),
                _ => self.expand_word(word),
            },

            // ${var:=word} - assign default value
            Some(VarModifier::DefaultAssign(word)) => match &val {
                Some(v) if !v.is_empty() => v.clone(),
                _ => self.expand_word(word),
            },

            // ${var:?word} - error if null or unset
            Some(VarModifier::Error(word)) => match &val {
                Some(v) if !v.is_empty() => v.clone(),
                _ => {
                    let msg = self.expand_word(word);
                    eprintln!("zshrs: {}", msg);
                    String::new()
                }
            },

            // ${var:+word} - use alternate value
            Some(VarModifier::Alternate(word)) => match &val {
                Some(v) if !v.is_empty() => self.expand_word(word),
                _ => String::new(),
            },

            // ${#var} - string length
            Some(VarModifier::Length) => val
                .map(|v| v.len().to_string())
                .unwrap_or_else(|| "0".to_string()),

            // ${var:offset} or ${var:offset:length} - substring
            Some(VarModifier::Substring(offset, length)) => {
                let v = val.unwrap_or_default();
                let start = if *offset < 0 {
                    (v.len() as i64 + offset).max(0) as usize
                } else {
                    (*offset as usize).min(v.len())
                };

                if let Some(len) = length {
                    let len = (*len as usize).min(v.len().saturating_sub(start));
                    v.chars().skip(start).take(len).collect()
                } else {
                    v.chars().skip(start).collect()
                }
            }

            // ${var#pattern} - remove shortest prefix
            Some(VarModifier::RemovePrefix(pattern)) => {
                let v = val.unwrap_or_default();
                let pat = self.expand_word(pattern);
                if v.starts_with(&pat) {
                    v[pat.len()..].to_string()
                } else {
                    v
                }
            }

            // ${var##pattern} - remove longest prefix
            Some(VarModifier::RemovePrefixLong(pattern)) => {
                let v = val.unwrap_or_default();
                let pat = self.expand_word(pattern);
                // For glob patterns, find longest match from start
                if let Ok(glob) = glob::Pattern::new(&pat) {
                    for i in (0..=v.len()).rev() {
                        if glob.matches(&v[..i]) {
                            return v[i..].to_string();
                        }
                    }
                }
                v
            }

            // ${var%pattern} - remove shortest suffix
            Some(VarModifier::RemoveSuffix(pattern)) => {
                let v = val.unwrap_or_default();
                let pat = self.expand_word(pattern);
                // For glob patterns, find shortest match from end
                if let Ok(glob) = glob::Pattern::new(&pat) {
                    for i in (0..=v.len()).rev() {
                        if glob.matches(&v[i..]) {
                            return v[..i].to_string();
                        }
                    }
                } else if v.ends_with(&pat) {
                    return v[..v.len() - pat.len()].to_string();
                }
                v
            }

            // ${var%%pattern} - remove longest suffix
            Some(VarModifier::RemoveSuffixLong(pattern)) => {
                let v = val.unwrap_or_default();
                let pat = self.expand_word(pattern);
                // For glob patterns, find longest match from end
                if let Ok(glob) = glob::Pattern::new(&pat) {
                    for i in 0..=v.len() {
                        if glob.matches(&v[i..]) {
                            return v[..i].to_string();
                        }
                    }
                }
                v
            }

            // ${var/pattern/replacement} - replace first match
            Some(VarModifier::Replace(pattern, replacement)) => {
                let v = val.unwrap_or_default();
                let pat = self.expand_word(pattern);
                let repl = self.expand_word(replacement);
                v.replacen(&pat, &repl, 1)
            }

            // ${var//pattern/replacement} - replace all matches
            Some(VarModifier::ReplaceAll(pattern, replacement)) => {
                let v = val.unwrap_or_default();
                let pat = self.expand_word(pattern);
                let repl = self.expand_word(replacement);
                v.replace(&pat, &repl)
            }

            // ${var^} or ${var^^} - uppercase
            Some(VarModifier::Upper) => val.map(|v| v.to_uppercase()).unwrap_or_default(),

            // ${var,} or ${var,,} - lowercase
            Some(VarModifier::Lower) => val.map(|v| v.to_lowercase()).unwrap_or_default(),

            // ${(flags)var} - zsh parameter expansion flags
            Some(VarModifier::ZshFlags(flags)) => {
                let mut result = val.unwrap_or_default();
                for flag in flags {
                    result = self.apply_zsh_param_flag(&result, name, flag);
                }
                result
            }

            // Array-related modifiers are handled elsewhere
            Some(VarModifier::ArrayLength)
            | Some(VarModifier::ArrayIndex(_))
            | Some(VarModifier::ArrayAll) => val.unwrap_or_default(),
        }
    }

    /// Parse zsh parameter expansion flags from a string like "L", "U", "j:,:"
    fn parse_zsh_flags(&self, s: &str) -> Vec<ZshParamFlag> {
        use crate::shell_ast::ZshParamFlag;
        let mut flags = Vec::new();
        let mut chars = s.chars().peekable();

        while let Some(c) = chars.next() {
            match c {
                'L' => flags.push(ZshParamFlag::Lower),
                'U' => flags.push(ZshParamFlag::Upper),
                'C' => flags.push(ZshParamFlag::Capitalize),
                'j' => {
                    // j:sep: - join with separator
                    if chars.peek() == Some(&':') {
                        chars.next();
                        let mut sep = String::new();
                        while let Some(&ch) = chars.peek() {
                            if ch == ':' {
                                chars.next();
                                break;
                            }
                            sep.push(chars.next().unwrap());
                        }
                        flags.push(ZshParamFlag::Join(sep));
                    }
                }
                'F' => flags.push(ZshParamFlag::JoinNewline),
                's' => {
                    // s:sep: - split on separator
                    if chars.peek() == Some(&':') {
                        chars.next();
                        let mut sep = String::new();
                        while let Some(&ch) = chars.peek() {
                            if ch == ':' {
                                chars.next();
                                break;
                            }
                            sep.push(chars.next().unwrap());
                        }
                        flags.push(ZshParamFlag::Split(sep));
                    }
                }
                'f' => flags.push(ZshParamFlag::SplitLines),
                'z' => flags.push(ZshParamFlag::SplitWords),
                't' => flags.push(ZshParamFlag::Type),
                'w' => flags.push(ZshParamFlag::Words),
                'b' => flags.push(ZshParamFlag::QuoteBackslash),
                'q' => {
                    if chars.peek() == Some(&'q') {
                        chars.next();
                        flags.push(ZshParamFlag::DoubleQuote);
                    } else {
                        flags.push(ZshParamFlag::Quote);
                    }
                }
                'u' => flags.push(ZshParamFlag::Unique),
                'O' => flags.push(ZshParamFlag::Reverse),
                'o' => flags.push(ZshParamFlag::Sort),
                'n' => flags.push(ZshParamFlag::NumericSort),
                'a' => flags.push(ZshParamFlag::IndexSort),
                'k' => flags.push(ZshParamFlag::Keys),
                'v' => flags.push(ZshParamFlag::Values),
                '#' => flags.push(ZshParamFlag::Length),
                'c' => flags.push(ZshParamFlag::CountChars),
                'e' => flags.push(ZshParamFlag::Expand),
                '%' => {
                    if chars.peek() == Some(&'%') {
                        chars.next();
                        flags.push(ZshParamFlag::PromptExpandFull);
                    } else {
                        flags.push(ZshParamFlag::PromptExpand);
                    }
                }
                'V' => flags.push(ZshParamFlag::Visible),
                'D' => flags.push(ZshParamFlag::Directory),
                'l' => {
                    // l:len:fill: - pad left
                    if chars.peek() == Some(&':') {
                        chars.next();
                        let mut len_str = String::new();
                        while let Some(&ch) = chars.peek() {
                            if ch == ':' {
                                chars.next();
                                break;
                            }
                            len_str.push(chars.next().unwrap());
                        }
                        let mut fill = ' ';
                        if let Some(&ch) = chars.peek() {
                            if ch != ':' {
                                fill = chars.next().unwrap();
                                if chars.peek() == Some(&':') {
                                    chars.next();
                                }
                            }
                        }
                        if let Ok(len) = len_str.parse() {
                            flags.push(ZshParamFlag::PadLeft(len, fill));
                        }
                    }
                }
                'r' => {
                    // r:len:fill: - pad right
                    if chars.peek() == Some(&':') {
                        chars.next();
                        let mut len_str = String::new();
                        while let Some(&ch) = chars.peek() {
                            if ch == ':' {
                                chars.next();
                                break;
                            }
                            len_str.push(chars.next().unwrap());
                        }
                        let mut fill = ' ';
                        if let Some(&ch) = chars.peek() {
                            if ch != ':' {
                                fill = chars.next().unwrap();
                                if chars.peek() == Some(&':') {
                                    chars.next();
                                }
                            }
                        }
                        if let Ok(len) = len_str.parse() {
                            flags.push(ZshParamFlag::PadRight(len, fill));
                        }
                    }
                }
                'm' => {
                    // Width for padding - parse number if present
                    let mut width_str = String::new();
                    while let Some(&ch) = chars.peek() {
                        if ch.is_ascii_digit() {
                            width_str.push(chars.next().unwrap());
                        } else {
                            break;
                        }
                    }
                    if let Ok(w) = width_str.parse() {
                        flags.push(ZshParamFlag::Width(w));
                    }
                }
                _ => {}
            }
        }
        flags
    }

    /// Apply a single zsh parameter expansion flag
    fn apply_zsh_param_flag(&self, val: &str, name: &str, flag: &ZshParamFlag) -> String {
        use crate::shell_ast::ZshParamFlag;
        match flag {
            ZshParamFlag::Lower => val.to_lowercase(),
            ZshParamFlag::Upper => val.to_uppercase(),
            ZshParamFlag::Capitalize => val
                .split_whitespace()
                .map(|word| {
                    let mut c = word.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" "),
            ZshParamFlag::Join(sep) => {
                if let Some(arr) = self.arrays.get(name) {
                    arr.join(sep)
                } else {
                    val.to_string()
                }
            }
            ZshParamFlag::Split(sep) => val.split(sep).collect::<Vec<_>>().join(" "),
            ZshParamFlag::SplitLines => val.lines().collect::<Vec<_>>().join(" "),
            ZshParamFlag::Type => {
                if self.arrays.contains_key(name) {
                    "array".to_string()
                } else if self.assoc_arrays.contains_key(name) {
                    "association".to_string()
                } else if self.functions.contains_key(name) {
                    "function".to_string()
                } else if std::env::var(name).is_ok() || self.variables.contains_key(name) {
                    "scalar".to_string()
                } else {
                    "".to_string()
                }
            }
            ZshParamFlag::Words => val.split_whitespace().collect::<Vec<_>>().join(" "),
            ZshParamFlag::Quote => format!("'{}'", val.replace('\'', "'\\''")),
            ZshParamFlag::DoubleQuote => format!("\"{}\"", val.replace('"', "\\\"")),
            ZshParamFlag::Unique => {
                let mut seen = std::collections::HashSet::new();
                val.split_whitespace()
                    .filter(|s| seen.insert(*s))
                    .collect::<Vec<_>>()
                    .join(" ")
            }
            ZshParamFlag::Reverse => val.chars().rev().collect(),
            ZshParamFlag::Sort => {
                let mut words: Vec<&str> = val.split_whitespace().collect();
                words.sort();
                words.join(" ")
            }
            ZshParamFlag::NumericSort => {
                let mut words: Vec<&str> = val.split_whitespace().collect();
                words.sort_by(|a, b| {
                    let na: i64 = a.parse().unwrap_or(0);
                    let nb: i64 = b.parse().unwrap_or(0);
                    na.cmp(&nb)
                });
                words.join(" ")
            }
            ZshParamFlag::Keys => {
                if let Some(assoc) = self.assoc_arrays.get(name) {
                    assoc.keys().cloned().collect::<Vec<_>>().join(" ")
                } else {
                    String::new()
                }
            }
            ZshParamFlag::Values => {
                if let Some(assoc) = self.assoc_arrays.get(name) {
                    assoc.values().cloned().collect::<Vec<_>>().join(" ")
                } else {
                    val.to_string()
                }
            }
            ZshParamFlag::Length => val.len().to_string(),
            ZshParamFlag::Head(n) => val
                .split_whitespace()
                .take(*n)
                .collect::<Vec<_>>()
                .join(" "),
            ZshParamFlag::Tail(n) => {
                let words: Vec<&str> = val.split_whitespace().collect();
                if words.len() > *n {
                    words[words.len() - n..].join(" ")
                } else {
                    val.to_string()
                }
            }
            ZshParamFlag::JoinNewline => {
                if let Some(arr) = self.arrays.get(name) {
                    arr.join("\n")
                } else {
                    val.to_string()
                }
            }
            ZshParamFlag::SplitWords => {
                // Shell-style word splitting
                val.split_whitespace().collect::<Vec<_>>().join(" ")
            }
            ZshParamFlag::QuoteBackslash => {
                // Quote special pattern chars with backslashes
                let mut result = String::new();
                for c in val.chars() {
                    if "\\*?[]{}()".contains(c) {
                        result.push('\\');
                    }
                    result.push(c);
                }
                result
            }
            ZshParamFlag::IndexSort => {
                // Array index order - just return as-is (default)
                val.to_string()
            }
            ZshParamFlag::CountChars => {
                // Count total characters
                val.chars().count().to_string()
            }
            ZshParamFlag::Expand => {
                // Would need mutable self to do expansions
                val.to_string()
            }
            ZshParamFlag::PromptExpand => {
                // Expand prompt escapes
                self.expand_prompt_string(val)
            }
            ZshParamFlag::PromptExpandFull => {
                // Full prompt expansion
                self.expand_prompt_string(val)
            }
            ZshParamFlag::Visible => {
                // Make non-printable characters visible
                val.chars()
                    .map(|c| {
                        if c.is_control() {
                            format!("^{}", (c as u8 + 64) as char)
                        } else {
                            c.to_string()
                        }
                    })
                    .collect()
            }
            ZshParamFlag::Directory => {
                // Substitute leading directory with ~ if it's home
                if let Some(home) = dirs::home_dir() {
                    let home_str = home.to_string_lossy();
                    if val.starts_with(home_str.as_ref()) {
                        format!("~{}", &val[home_str.len()..])
                    } else {
                        val.to_string()
                    }
                } else {
                    val.to_string()
                }
            }
            ZshParamFlag::PadLeft(len, fill) => {
                if val.len() >= *len {
                    val.to_string()
                } else {
                    let padding: String = std::iter::repeat(*fill).take(len - val.len()).collect();
                    format!("{}{}", padding, val)
                }
            }
            ZshParamFlag::PadRight(len, fill) => {
                if val.len() >= *len {
                    val.to_string()
                } else {
                    let padding: String = std::iter::repeat(*fill).take(len - val.len()).collect();
                    format!("{}{}", val, padding)
                }
            }
            ZshParamFlag::Width(_) => {
                // Width modifier - used with padding, just return value
                val.to_string()
            }
        }
    }

    /// Expand prompt escape sequences
    fn expand_prompt_string(&self, s: &str) -> String {
        let mut result = String::new();
        let mut chars = s.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '%' {
                match chars.next() {
                    Some('%') => result.push('%'),
                    Some('~') => {
                        // Current directory with ~ substitution
                        let cwd = std::env::current_dir().unwrap_or_default();
                        let cwd_str = cwd.to_string_lossy();
                        if let Some(home) = dirs::home_dir() {
                            let home_str = home.to_string_lossy();
                            if cwd_str.starts_with(home_str.as_ref()) {
                                result.push('~');
                                result.push_str(&cwd_str[home_str.len()..]);
                            } else {
                                result.push_str(&cwd_str);
                            }
                        } else {
                            result.push_str(&cwd_str);
                        }
                    }
                    Some('/') | Some('d') => {
                        // Current directory
                        let cwd = std::env::current_dir().unwrap_or_default();
                        result.push_str(&cwd.to_string_lossy());
                    }
                    Some('c') | Some('.') => {
                        // Trailing component of cwd
                        let cwd = std::env::current_dir().unwrap_or_default();
                        if let Some(name) = cwd.file_name() {
                            result.push_str(&name.to_string_lossy());
                        }
                    }
                    Some('n') => {
                        // Username
                        result.push_str(
                            &std::env::var("USER").unwrap_or_else(|_| "user".to_string()),
                        );
                    }
                    Some('m') => {
                        // Hostname up to first dot
                        let host = hostname::get()
                            .map(|h| h.to_string_lossy().to_string())
                            .unwrap_or_else(|_| "localhost".to_string());
                        if let Some(dot) = host.find('.') {
                            result.push_str(&host[..dot]);
                        } else {
                            result.push_str(&host);
                        }
                    }
                    Some('M') => {
                        // Full hostname
                        let host = hostname::get()
                            .map(|h| h.to_string_lossy().to_string())
                            .unwrap_or_else(|_| "localhost".to_string());
                        result.push_str(&host);
                    }
                    Some('#') => {
                        // # if root, % otherwise
                        if unsafe { libc::geteuid() } == 0 {
                            result.push('#');
                        } else {
                            result.push('%');
                        }
                    }
                    Some('?') => {
                        // Exit status of last command
                        result.push_str(&self.last_status.to_string());
                    }
                    Some('!') => {
                        // History event number
                        result.push_str("1"); // Simplified
                    }
                    Some('i') | Some('l') => {
                        // Line number
                        result.push_str("1");
                    }
                    Some('j') => {
                        // Number of jobs
                        result.push_str(&self.jobs.list().len().to_string());
                    }
                    Some('D') => {
                        // Date
                        let now = chrono::Local::now();
                        result.push_str(&now.format("%y-%m-%d").to_string());
                    }
                    Some('T') => {
                        // Time (24h)
                        let now = chrono::Local::now();
                        result.push_str(&now.format("%H:%M").to_string());
                    }
                    Some('t') | Some('@') => {
                        // Time (12h with am/pm)
                        let now = chrono::Local::now();
                        result.push_str(&now.format("%I:%M%p").to_string());
                    }
                    Some('*') => {
                        // Time (24h with seconds)
                        let now = chrono::Local::now();
                        result.push_str(&now.format("%H:%M:%S").to_string());
                    }
                    Some('w') => {
                        // Day of week
                        let now = chrono::Local::now();
                        result.push_str(&now.format("%a").to_string());
                    }
                    Some('W') => {
                        // Date in mm/dd/yy
                        let now = chrono::Local::now();
                        result.push_str(&now.format("%m/%d/%y").to_string());
                    }
                    Some('B') | Some('b') => {} // Bold on/off - ignore
                    Some('U') | Some('u') => {} // Underline on/off - ignore
                    Some('S') | Some('s') => {} // Standout on/off - ignore
                    Some('F') | Some('f') | Some('K') | Some('k') => {
                        // Foreground/background color - skip the color spec
                        if chars.peek() == Some(&'{') {
                            chars.next();
                            while let Some(ch) = chars.next() {
                                if ch == '}' {
                                    break;
                                }
                            }
                        }
                    }
                    Some('{') => {
                        // Literal escape sequence - skip to }
                        while let Some(ch) = chars.next() {
                            if ch == '%' && chars.peek() == Some(&'}') {
                                chars.next();
                                break;
                            }
                        }
                    }
                    Some(d) if d.is_ascii_digit() => {
                        // Truncation - skip for now
                        while chars.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                            chars.next();
                        }
                    }
                    Some(other) => {
                        result.push('%');
                        result.push(other);
                    }
                    None => result.push('%'),
                }
            } else {
                result.push(c);
            }
        }

        result
    }

    /// Execute a command and capture its output (command substitution)
    fn execute_command_substitution(&mut self, cmd: &ShellCommand) -> String {
        match self.execute_command_capture(cmd) {
            Ok(output) => output.trim_end_matches('\n').to_string(),
            Err(_) => String::new(),
        }
    }

    /// Execute a command and capture its stdout
    fn execute_command_capture(&mut self, cmd: &ShellCommand) -> Result<String, String> {
        // For simple commands, we can use Command directly
        if let ShellCommand::Simple(simple) = cmd {
            let words: Vec<String> = simple.words.iter().map(|w| self.expand_word(w)).collect();
            if words.is_empty() {
                return Ok(String::new());
            }

            let cmd_name = &words[0];
            let args = &words[1..];

            // Handle some builtins that can return values
            match cmd_name.as_str() {
                "echo" => {
                    let output = args.join(" ");
                    return Ok(format!("{}\n", output));
                }
                "printf" => {
                    if !args.is_empty() {
                        // Simple printf - just format string with args
                        let format = &args[0];
                        let result = if args.len() > 1 {
                            // Very basic: just handle %s
                            let mut out = format.clone();
                            for (i, arg) in args[1..].iter().enumerate() {
                                out = out.replacen("%s", arg, 1);
                                out = out.replacen(&format!("${}", i + 1), arg, 1);
                            }
                            out
                        } else {
                            format.clone()
                        };
                        return Ok(result);
                    }
                    return Ok(String::new());
                }
                "pwd" => {
                    return Ok(env::current_dir()
                        .map(|p| format!("{}\n", p.display()))
                        .unwrap_or_default());
                }
                _ => {}
            }

            // External command - capture its output
            let output = Command::new(cmd_name)
                .args(args)
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .output();

            match output {
                Ok(output) => {
                    self.last_status = output.status.code().unwrap_or(1);
                    Ok(String::from_utf8_lossy(&output.stdout).to_string())
                }
                Err(e) => {
                    self.last_status = 127;
                    Err(format!("{}: {}", cmd_name, e))
                }
            }
        } else if let ShellCommand::Pipeline(cmds, _negated) = cmd {
            // For pipelines, execute and capture output of the last command
            // This is simplified - proper implementation would pipe between all
            if let Some(last) = cmds.last() {
                return self.execute_command_capture(last);
            }
            Ok(String::new())
        } else {
            // For compound commands, execute them and return empty
            // (complex case - could be expanded later)
            let _ = self.execute_command(cmd);
            Ok(String::new())
        }
    }

    /// Evaluate arithmetic expression
    fn evaluate_arithmetic(&mut self, expr: &str) -> String {
        // Simple arithmetic evaluator
        let expr = self.expand_string(expr);

        // Use float evaluation to preserve precision
        let result = self.eval_arith_expr_float(&expr);

        // Format: if it's a whole number, show as int; otherwise show float
        if result.fract() == 0.0 && result.abs() < i64::MAX as f64 {
            format!("{}", result as i64)
        } else {
            format!("{}", result)
        }
    }

    fn eval_arith_expr(&mut self, expr: &str) -> i64 {
        self.eval_arith_expr_float(expr) as i64
    }

    fn eval_arith_expr_float(&mut self, expr: &str) -> f64 {
        let expr = expr.trim();

        // Handle assignment: var=expr (must check before other operators)
        // But not ==, !=, <=, >=
        if let Some(eq_pos) = expr.find('=') {
            let chars: Vec<char> = expr.chars().collect();
            let is_comparison = (eq_pos > 0
                && (chars[eq_pos - 1] == '!'
                    || chars[eq_pos - 1] == '<'
                    || chars[eq_pos - 1] == '>'
                    || chars[eq_pos - 1] == '='))
                || chars.get(eq_pos + 1) == Some(&'=');

            if !is_comparison {
                let var_part = &expr[..eq_pos];
                if !var_part.contains(|c: char| "+-*/%<>!&|()".contains(c)) {
                    let var_name = var_part.trim();
                    if !var_name.is_empty()
                        && var_name.chars().all(|c| c.is_alphanumeric() || c == '_')
                    {
                        let value_expr = &expr[eq_pos + 1..];
                        let value = self.eval_arith_expr_float(value_expr);
                        self.variables
                            .insert(var_name.to_string(), value.to_string());
                        env::set_var(var_name, value.to_string());
                        return value;
                    }
                }
            }
        }

        // Handle post-increment/decrement: var++ var--
        if expr.ends_with("++") {
            let var_name = expr[..expr.len() - 2].trim();
            let current = self.get_variable(var_name).parse::<f64>().unwrap_or(0.0);
            let new_val = current + 1.0;
            self.variables
                .insert(var_name.to_string(), new_val.to_string());
            env::set_var(var_name, new_val.to_string());
            return current;
        }
        if expr.ends_with("--") {
            let var_name = expr[..expr.len() - 2].trim();
            let current = self.get_variable(var_name).parse::<f64>().unwrap_or(0.0);
            let new_val = current - 1.0;
            self.variables
                .insert(var_name.to_string(), new_val.to_string());
            env::set_var(var_name, new_val.to_string());
            return current;
        }

        // Handle pre-increment/decrement: ++var --var
        if expr.starts_with("++") {
            let var_name = expr[2..].trim();
            let current = self.get_variable(var_name).parse::<f64>().unwrap_or(0.0);
            let new_val = current + 1.0;
            self.variables
                .insert(var_name.to_string(), new_val.to_string());
            env::set_var(var_name, new_val.to_string());
            return new_val;
        }
        if expr.starts_with("--") {
            let var_name = expr[2..].trim();
            let current = self.get_variable(var_name).parse::<f64>().unwrap_or(0.0);
            let new_val = current - 1.0;
            self.variables
                .insert(var_name.to_string(), new_val.to_string());
            env::set_var(var_name, new_val.to_string());
            return new_val;
        }

        // Handle math functions BEFORE parentheses: sin(x), cos(x), etc.
        if let Some(paren_pos) = expr.find('(') {
            if expr.ends_with(')') {
                let func_name = expr[..paren_pos].trim();
                if !func_name.is_empty()
                    && func_name.chars().all(|c| c.is_alphanumeric() || c == '_')
                {
                    let args_str = &expr[paren_pos + 1..expr.len() - 1];

                    let args: Vec<f64> = if args_str.is_empty() {
                        Vec::new()
                    } else {
                        args_str
                            .split(',')
                            .map(|a| self.eval_arith_expr_float(a.trim()))
                            .collect()
                    };

                    return self.eval_math_function(func_name, &args);
                }
            }
        }

        // Handle parentheses (grouping)
        if expr.starts_with('(') && expr.ends_with(')') {
            let mut depth = 0;
            let mut is_simple_group = true;
            for (i, c) in expr.chars().enumerate() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 && i < expr.len() - 1 {
                            is_simple_group = false;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if is_simple_group {
                return self.eval_arith_expr_float(&expr[1..expr.len() - 1]);
            }
        }

        // Handle binary operators (lowest to highest precedence)
        let mut depth = 0;
        let chars: Vec<char> = expr.chars().collect();

        // Comparison operators
        for i in 0..chars.len() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '<' if depth == 0 => {
                    if chars.get(i + 1) == Some(&'=') {
                        let l = self.eval_arith_expr_float(&expr[..i]);
                        let r = self.eval_arith_expr_float(&expr[i + 2..]);
                        return if l <= r { 1.0 } else { 0.0 };
                    } else if chars.get(i + 1) != Some(&'<') {
                        let l = self.eval_arith_expr_float(&expr[..i]);
                        let r = self.eval_arith_expr_float(&expr[i + 1..]);
                        return if l < r { 1.0 } else { 0.0 };
                    }
                }
                '>' if depth == 0 => {
                    if chars.get(i + 1) == Some(&'=') {
                        let l = self.eval_arith_expr_float(&expr[..i]);
                        let r = self.eval_arith_expr_float(&expr[i + 2..]);
                        return if l >= r { 1.0 } else { 0.0 };
                    } else if chars.get(i + 1) != Some(&'>') {
                        let l = self.eval_arith_expr_float(&expr[..i]);
                        let r = self.eval_arith_expr_float(&expr[i + 1..]);
                        return if l > r { 1.0 } else { 0.0 };
                    }
                }
                '=' if depth == 0 && chars.get(i + 1) == Some(&'=') => {
                    let l = self.eval_arith_expr_float(&expr[..i]);
                    let r = self.eval_arith_expr_float(&expr[i + 2..]);
                    return if (l - r).abs() < f64::EPSILON {
                        1.0
                    } else {
                        0.0
                    };
                }
                '!' if depth == 0 && chars.get(i + 1) == Some(&'=') => {
                    let l = self.eval_arith_expr_float(&expr[..i]);
                    let r = self.eval_arith_expr_float(&expr[i + 2..]);
                    return if (l - r).abs() >= f64::EPSILON {
                        1.0
                    } else {
                        0.0
                    };
                }
                _ => {}
            }
        }

        depth = 0;
        // Addition and subtraction
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '+' | '-' if depth == 0 && i > 0 => {
                    let l = self.eval_arith_expr_float(&expr[..i]);
                    let r = self.eval_arith_expr_float(&expr[i + 1..]);
                    return if chars[i] == '+' { l + r } else { l - r };
                }
                _ => {}
            }
        }

        // Exponentiation **
        depth = 0;
        let mut last_exp_pos = None;
        for i in 0..chars.len().saturating_sub(1) {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '*' if depth == 0 && chars.get(i + 1) == Some(&'*') => {
                    last_exp_pos = Some(i);
                }
                _ => {}
            }
        }
        if let Some(pos) = last_exp_pos {
            let l = self.eval_arith_expr_float(&expr[..pos]);
            let r = self.eval_arith_expr_float(&expr[pos + 2..]);
            return l.powf(r);
        }

        // Multiplication, division, modulo
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '*' if depth == 0
                    && chars.get(i + 1) != Some(&'*')
                    && (i == 0 || chars[i - 1] != '*') =>
                {
                    let l = self.eval_arith_expr_float(&expr[..i]);
                    let r = self.eval_arith_expr_float(&expr[i + 1..]);
                    return l * r;
                }
                '/' if depth == 0 => {
                    let l = self.eval_arith_expr_float(&expr[..i]);
                    let r = self.eval_arith_expr_float(&expr[i + 1..]);
                    return if r != 0.0 { l / r } else { 0.0 };
                }
                '%' if depth == 0 => {
                    let l = self.eval_arith_expr_float(&expr[..i]);
                    let r = self.eval_arith_expr_float(&expr[i + 1..]);
                    return if r != 0.0 { l % r } else { 0.0 };
                }
                _ => {}
            }
        }

        // Try to parse as number
        if let Ok(f) = expr.parse::<f64>() {
            return f;
        }

        // Try as variable
        self.get_variable(expr).parse().unwrap_or(0.0)
    }

    /// Evaluate math functions (zsh/mathfunc module)
    fn eval_math_function(&self, name: &str, args: &[f64]) -> f64 {
        match name {
            // Single argument trig functions
            "sin" => args.first().map(|x| x.sin()).unwrap_or(0.0),
            "cos" => args.first().map(|x| x.cos()).unwrap_or(0.0),
            "tan" => args.first().map(|x| x.tan()).unwrap_or(0.0),
            "asin" => args.first().map(|x| x.asin()).unwrap_or(0.0),
            "acos" => args.first().map(|x| x.acos()).unwrap_or(0.0),
            "atan" => {
                if args.len() >= 2 {
                    args[0].atan2(args[1])
                } else {
                    args.first().map(|x| x.atan()).unwrap_or(0.0)
                }
            }
            "sinh" => args.first().map(|x| x.sinh()).unwrap_or(0.0),
            "cosh" => args.first().map(|x| x.cosh()).unwrap_or(0.0),
            "tanh" => args.first().map(|x| x.tanh()).unwrap_or(0.0),
            "asinh" => args.first().map(|x| x.asinh()).unwrap_or(0.0),
            "acosh" => args.first().map(|x| x.acosh()).unwrap_or(0.0),
            "atanh" => args.first().map(|x| x.atanh()).unwrap_or(0.0),

            // Exponential and logarithmic
            "exp" => args.first().map(|x| x.exp()).unwrap_or(0.0),
            "expm1" => args.first().map(|x| x.exp_m1()).unwrap_or(0.0),
            "log" | "ln" => args.first().map(|x| x.ln()).unwrap_or(0.0),
            "log10" => args.first().map(|x| x.log10()).unwrap_or(0.0),
            "log2" => args.first().map(|x| x.log2()).unwrap_or(0.0),
            "log1p" => args.first().map(|x| x.ln_1p()).unwrap_or(0.0),

            // Power and roots
            "sqrt" => args.first().map(|x| x.sqrt()).unwrap_or(0.0),
            "cbrt" => args.first().map(|x| x.cbrt()).unwrap_or(0.0),
            "pow" => {
                if args.len() >= 2 {
                    args[0].powf(args[1])
                } else {
                    0.0
                }
            }
            "hypot" => {
                if args.len() >= 2 {
                    args[0].hypot(args[1])
                } else {
                    0.0
                }
            }

            // Rounding
            "ceil" => args.first().map(|x| x.ceil()).unwrap_or(0.0),
            "floor" => args.first().map(|x| x.floor()).unwrap_or(0.0),
            "round" => args.first().map(|x| x.round()).unwrap_or(0.0),
            "trunc" | "int" => args.first().map(|x| x.trunc()).unwrap_or(0.0),

            // Absolute value and sign
            "abs" | "fabs" => args.first().map(|x| x.abs()).unwrap_or(0.0),
            "copysign" => {
                if args.len() >= 2 {
                    args[0].copysign(args[1])
                } else {
                    0.0
                }
            }

            // Misc
            "fmod" => {
                if args.len() >= 2 && args[1] != 0.0 {
                    args[0] % args[1]
                } else {
                    0.0
                }
            }
            "ldexp" => {
                if args.len() >= 2 {
                    args[0] * 2f64.powi(args[1] as i32)
                } else {
                    0.0
                }
            }
            "frexp" => args
                .first()
                .map(|x| {
                    let (mantissa, _exp) = libm::frexp(*x);
                    mantissa
                })
                .unwrap_or(0.0),

            // Min/max
            "min" => args.iter().copied().reduce(f64::min).unwrap_or(0.0),
            "max" => args.iter().copied().reduce(f64::max).unwrap_or(0.0),
            "sum" => args.iter().sum(),

            // Float conversion
            "float" => args.first().copied().unwrap_or(0.0),

            // Random
            "rand" => rand::random::<f64>(),
            "rand48" => rand::random::<f64>(),

            // Special functions (using libm crate if available, else approximations)
            "erf" => args.first().map(|x| libm::erf(*x)).unwrap_or(0.0),
            "erfc" => args.first().map(|x| libm::erfc(*x)).unwrap_or(0.0),
            "gamma" | "tgamma" => args.first().map(|x| libm::tgamma(*x)).unwrap_or(0.0),
            "lgamma" => args.first().map(|x| libm::lgamma_r(*x).0).unwrap_or(0.0),
            "j0" => args.first().map(|x| libm::j0(*x)).unwrap_or(0.0),
            "j1" => args.first().map(|x| libm::j1(*x)).unwrap_or(0.0),
            "jn" => {
                if args.len() >= 2 {
                    libm::jn(args[0] as i32, args[1])
                } else {
                    0.0
                }
            }
            "y0" => args.first().map(|x| libm::y0(*x)).unwrap_or(0.0),
            "y1" => args.first().map(|x| libm::y1(*x)).unwrap_or(0.0),
            "yn" => {
                if args.len() >= 2 {
                    libm::yn(args[0] as i32, args[1])
                } else {
                    0.0
                }
            }

            _ => 0.0,
        }
    }

    fn matches_pattern(&self, value: &str, pattern: &str) -> bool {
        // Simple glob matching
        if pattern == "*" {
            return true;
        }
        if pattern.contains('*') || pattern.contains('?') {
            // Use glob matching
            glob::Pattern::new(pattern)
                .map(|p| p.matches(value))
                .unwrap_or(false)
        } else {
            value == pattern
        }
    }

    fn eval_cond_expr(&mut self, expr: &CondExpr) -> bool {
        match expr {
            CondExpr::FileExists(w) => std::path::Path::new(&self.expand_word(w)).exists(),
            CondExpr::FileRegular(w) => std::path::Path::new(&self.expand_word(w)).is_file(),
            CondExpr::FileDirectory(w) => std::path::Path::new(&self.expand_word(w)).is_dir(),
            CondExpr::FileSymlink(w) => std::path::Path::new(&self.expand_word(w)).is_symlink(),
            CondExpr::FileReadable(w) => std::path::Path::new(&self.expand_word(w)).exists(),
            CondExpr::FileWritable(w) => std::path::Path::new(&self.expand_word(w)).exists(),
            CondExpr::FileExecutable(w) => std::path::Path::new(&self.expand_word(w)).exists(),
            CondExpr::FileNonEmpty(w) => std::fs::metadata(&self.expand_word(w))
                .map(|m| m.len() > 0)
                .unwrap_or(false),
            CondExpr::StringEmpty(w) => self.expand_word(w).is_empty(),
            CondExpr::StringNonEmpty(w) => !self.expand_word(w).is_empty(),
            CondExpr::StringEqual(a, b) => self.expand_word(a) == self.expand_word(b),
            CondExpr::StringNotEqual(a, b) => self.expand_word(a) != self.expand_word(b),
            CondExpr::StringMatch(a, b) => {
                let val = self.expand_word(a);
                let pattern = self.expand_word(b);
                regex::Regex::new(&pattern)
                    .map(|re| re.is_match(&val))
                    .unwrap_or(false)
            }
            CondExpr::StringLess(a, b) => self.expand_word(a) < self.expand_word(b),
            CondExpr::StringGreater(a, b) => self.expand_word(a) > self.expand_word(b),
            CondExpr::NumEqual(a, b) => {
                let a_val = self.expand_word(a).parse::<i64>().unwrap_or(0);
                let b_val = self.expand_word(b).parse::<i64>().unwrap_or(0);
                a_val == b_val
            }
            CondExpr::NumNotEqual(a, b) => {
                let a_val = self.expand_word(a).parse::<i64>().unwrap_or(0);
                let b_val = self.expand_word(b).parse::<i64>().unwrap_or(0);
                a_val != b_val
            }
            CondExpr::NumLess(a, b) => {
                let a_val = self.expand_word(a).parse::<i64>().unwrap_or(0);
                let b_val = self.expand_word(b).parse::<i64>().unwrap_or(0);
                a_val < b_val
            }
            CondExpr::NumLessEqual(a, b) => {
                let a_val = self.expand_word(a).parse::<i64>().unwrap_or(0);
                let b_val = self.expand_word(b).parse::<i64>().unwrap_or(0);
                a_val <= b_val
            }
            CondExpr::NumGreater(a, b) => {
                let a_val = self.expand_word(a).parse::<i64>().unwrap_or(0);
                let b_val = self.expand_word(b).parse::<i64>().unwrap_or(0);
                a_val > b_val
            }
            CondExpr::NumGreaterEqual(a, b) => {
                let a_val = self.expand_word(a).parse::<i64>().unwrap_or(0);
                let b_val = self.expand_word(b).parse::<i64>().unwrap_or(0);
                a_val >= b_val
            }
            CondExpr::Not(inner) => !self.eval_cond_expr(inner),
            CondExpr::And(a, b) => self.eval_cond_expr(a) && self.eval_cond_expr(b),
            CondExpr::Or(a, b) => self.eval_cond_expr(a) || self.eval_cond_expr(b),
        }
    }

    // Builtins

    fn builtin_cd(&mut self, args: &[String]) -> i32 {
        let path = args.first().map(|s| s.as_str()).unwrap_or("~");
        let path = if path == "~" || path == "" {
            dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
        } else if path.starts_with("~/") {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(&path[2..])
        } else if path == "-" {
            if let Ok(oldpwd) = env::var("OLDPWD") {
                PathBuf::from(oldpwd)
            } else {
                eprintln!("cd: OLDPWD not set");
                return 1;
            }
        } else {
            PathBuf::from(path)
        };

        if let Ok(cwd) = env::current_dir() {
            env::set_var("OLDPWD", cwd);
        }

        match env::set_current_dir(&path) {
            Ok(_) => {
                if let Ok(cwd) = env::current_dir() {
                    env::set_var("PWD", cwd);
                }
                0
            }
            Err(e) => {
                eprintln!("cd: {}: {}", path.display(), e);
                1
            }
        }
    }

    fn builtin_pwd(&mut self, _redirects: &[Redirect]) -> i32 {
        match env::current_dir() {
            Ok(path) => {
                println!("{}", path.display());
                0
            }
            Err(e) => {
                eprintln!("pwd: {}", e);
                1
            }
        }
    }

    fn builtin_echo(&mut self, args: &[String], _redirects: &[Redirect]) -> i32 {
        let mut newline = true;
        let mut interpret_escapes = false;
        let mut start = 0;

        for (i, arg) in args.iter().enumerate() {
            match arg.as_str() {
                "-n" => {
                    newline = false;
                    start = i + 1;
                }
                "-e" => {
                    interpret_escapes = true;
                    start = i + 1;
                }
                "-E" => {
                    interpret_escapes = false;
                    start = i + 1;
                }
                _ => break,
            }
        }

        let output = args[start..].join(" ");
        if interpret_escapes {
            print!("{}", output.replace("\\n", "\n").replace("\\t", "\t"));
        } else {
            print!("{}", output);
        }

        if newline {
            println!();
        }
        0
    }

    fn builtin_export(&mut self, args: &[String]) -> i32 {
        for arg in args {
            if let Some((key, value)) = arg.split_once('=') {
                env::set_var(key, value);
            }
        }
        0
    }

    fn builtin_unset(&mut self, args: &[String]) -> i32 {
        for arg in args {
            env::remove_var(arg);
            self.variables.remove(arg);
        }
        0
    }

    fn builtin_source(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            eprintln!("source: filename argument required");
            return 1;
        }

        let path = &args[0];
        match std::fs::read_to_string(path) {
            Ok(content) => match self.execute_script(&content) {
                Ok(status) => status,
                Err(e) => {
                    eprintln!("source: {}: {}", path, e);
                    1
                }
            },
            Err(e) => {
                eprintln!("source: {}: {}", path, e);
                1
            }
        }
    }

    fn builtin_exit(&mut self, args: &[String]) -> i32 {
        let code = args
            .first()
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(self.last_status);
        std::process::exit(code);
    }

    fn builtin_return(&mut self, args: &[String]) -> i32 {
        args.first()
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(self.last_status)
    }

    fn builtin_test(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            return 1;
        }

        let args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

        match args.as_slice() {
            // String tests
            ["-z", s] => {
                if s.is_empty() {
                    0
                } else {
                    1
                }
            }
            ["-n", s] => {
                if !s.is_empty() {
                    0
                } else {
                    1
                }
            }

            // File existence/type tests
            ["-a", path] | ["-e", path] => {
                if std::path::Path::new(path).exists() {
                    0
                } else {
                    1
                }
            }
            ["-f", path] => {
                if std::path::Path::new(path).is_file() {
                    0
                } else {
                    1
                }
            }
            ["-d", path] => {
                if std::path::Path::new(path).is_dir() {
                    0
                } else {
                    1
                }
            }
            ["-b", path] => {
                use std::os::unix::fs::FileTypeExt;
                if std::fs::symlink_metadata(path)
                    .map(|m| m.file_type().is_block_device())
                    .unwrap_or(false)
                {
                    0
                } else {
                    1
                }
            }
            ["-c", path] => {
                use std::os::unix::fs::FileTypeExt;
                if std::fs::symlink_metadata(path)
                    .map(|m| m.file_type().is_char_device())
                    .unwrap_or(false)
                {
                    0
                } else {
                    1
                }
            }
            ["-p", path] => {
                use std::os::unix::fs::FileTypeExt;
                if std::fs::symlink_metadata(path)
                    .map(|m| m.file_type().is_fifo())
                    .unwrap_or(false)
                {
                    0
                } else {
                    1
                }
            }
            ["-S", path] => {
                use std::os::unix::fs::FileTypeExt;
                if std::fs::symlink_metadata(path)
                    .map(|m| m.file_type().is_socket())
                    .unwrap_or(false)
                {
                    0
                } else {
                    1
                }
            }
            ["-h", path] | ["-L", path] => {
                if std::path::Path::new(path).is_symlink() {
                    0
                } else {
                    1
                }
            }

            // File permission tests
            ["-r", path] => {
                use std::os::unix::fs::MetadataExt;
                if let Ok(meta) = std::fs::metadata(path) {
                    let mode = meta.mode();
                    let uid = unsafe { libc::geteuid() };
                    let gid = unsafe { libc::getegid() };
                    let readable = if meta.uid() == uid {
                        mode & 0o400 != 0
                    } else if meta.gid() == gid {
                        mode & 0o040 != 0
                    } else {
                        mode & 0o004 != 0
                    };
                    if readable {
                        0
                    } else {
                        1
                    }
                } else {
                    1
                }
            }
            ["-w", path] => {
                use std::os::unix::fs::MetadataExt;
                if let Ok(meta) = std::fs::metadata(path) {
                    let mode = meta.mode();
                    let uid = unsafe { libc::geteuid() };
                    let gid = unsafe { libc::getegid() };
                    let writable = if meta.uid() == uid {
                        mode & 0o200 != 0
                    } else if meta.gid() == gid {
                        mode & 0o020 != 0
                    } else {
                        mode & 0o002 != 0
                    };
                    if writable {
                        0
                    } else {
                        1
                    }
                } else {
                    1
                }
            }
            ["-x", path] => {
                use std::os::unix::fs::MetadataExt;
                if let Ok(meta) = std::fs::metadata(path) {
                    let mode = meta.mode();
                    let uid = unsafe { libc::geteuid() };
                    let gid = unsafe { libc::getegid() };
                    let executable = if meta.uid() == uid {
                        mode & 0o100 != 0
                    } else if meta.gid() == gid {
                        mode & 0o010 != 0
                    } else {
                        mode & 0o001 != 0
                    };
                    if executable {
                        0
                    } else {
                        1
                    }
                } else {
                    1
                }
            }

            // Special permission bits
            ["-g", path] => {
                use std::os::unix::fs::MetadataExt;
                if std::fs::metadata(path)
                    .map(|m| m.mode() & 0o2000 != 0)
                    .unwrap_or(false)
                {
                    0
                } else {
                    1
                }
            }
            ["-k", path] => {
                use std::os::unix::fs::MetadataExt;
                if std::fs::metadata(path)
                    .map(|m| m.mode() & 0o1000 != 0)
                    .unwrap_or(false)
                {
                    0
                } else {
                    1
                }
            }
            ["-u", path] => {
                use std::os::unix::fs::MetadataExt;
                if std::fs::metadata(path)
                    .map(|m| m.mode() & 0o4000 != 0)
                    .unwrap_or(false)
                {
                    0
                } else {
                    1
                }
            }

            // File size
            ["-s", path] => {
                if std::fs::metadata(path)
                    .map(|m| m.len() > 0)
                    .unwrap_or(false)
                {
                    0
                } else {
                    1
                }
            }

            // Ownership
            ["-O", path] => {
                use std::os::unix::fs::MetadataExt;
                if std::fs::metadata(path)
                    .map(|m| m.uid() == unsafe { libc::geteuid() })
                    .unwrap_or(false)
                {
                    0
                } else {
                    1
                }
            }
            ["-G", path] => {
                use std::os::unix::fs::MetadataExt;
                if std::fs::metadata(path)
                    .map(|m| m.gid() == unsafe { libc::getegid() })
                    .unwrap_or(false)
                {
                    0
                } else {
                    1
                }
            }

            // File times
            ["-N", path] => {
                use std::os::unix::fs::MetadataExt;
                if let Ok(meta) = std::fs::metadata(path) {
                    if meta.mtime() > meta.atime() {
                        0
                    } else {
                        1
                    }
                } else {
                    1
                }
            }

            // Terminal test
            ["-t", fd] => {
                if let Ok(fd_num) = fd.parse::<i32>() {
                    if unsafe { libc::isatty(fd_num) } == 1 {
                        0
                    } else {
                        1
                    }
                } else {
                    1
                }
            }

            // Variable test
            ["-v", varname] => {
                if self.variables.contains_key(*varname) || std::env::var(varname).is_ok() {
                    0
                } else {
                    1
                }
            }

            // Option test
            ["-o", opt] => {
                let (name, _) = Self::normalize_option_name(opt);
                if self.options.get(&name).copied().unwrap_or(false) {
                    0
                } else {
                    1
                }
            }

            // String comparisons
            [a, "=", b] | [a, "==", b] => {
                if a == b {
                    0
                } else {
                    1
                }
            }
            [a, "!=", b] => {
                if a != b {
                    0
                } else {
                    1
                }
            }
            [a, "<", b] => {
                if *a < *b {
                    0
                } else {
                    1
                }
            }
            [a, ">", b] => {
                if *a > *b {
                    0
                } else {
                    1
                }
            }

            // Numeric comparisons
            [a, "-eq", b] => {
                let a: i64 = a.parse().unwrap_or(0);
                let b: i64 = b.parse().unwrap_or(0);
                if a == b {
                    0
                } else {
                    1
                }
            }
            [a, "-ne", b] => {
                let a: i64 = a.parse().unwrap_or(0);
                let b: i64 = b.parse().unwrap_or(0);
                if a != b {
                    0
                } else {
                    1
                }
            }
            [a, "-lt", b] => {
                let a: i64 = a.parse().unwrap_or(0);
                let b: i64 = b.parse().unwrap_or(0);
                if a < b {
                    0
                } else {
                    1
                }
            }
            [a, "-le", b] => {
                let a: i64 = a.parse().unwrap_or(0);
                let b: i64 = b.parse().unwrap_or(0);
                if a <= b {
                    0
                } else {
                    1
                }
            }
            [a, "-gt", b] => {
                let a: i64 = a.parse().unwrap_or(0);
                let b: i64 = b.parse().unwrap_or(0);
                if a > b {
                    0
                } else {
                    1
                }
            }
            [a, "-ge", b] => {
                let a: i64 = a.parse().unwrap_or(0);
                let b: i64 = b.parse().unwrap_or(0);
                if a >= b {
                    0
                } else {
                    1
                }
            }

            // File comparisons
            [f1, "-nt", f2] => {
                let m1 = std::fs::metadata(f1).and_then(|m| m.modified()).ok();
                let m2 = std::fs::metadata(f2).and_then(|m| m.modified()).ok();
                match (m1, m2) {
                    (Some(t1), Some(t2)) => {
                        if t1 > t2 {
                            0
                        } else {
                            1
                        }
                    }
                    (Some(_), None) => 0,
                    _ => 1,
                }
            }
            [f1, "-ot", f2] => {
                let m1 = std::fs::metadata(f1).and_then(|m| m.modified()).ok();
                let m2 = std::fs::metadata(f2).and_then(|m| m.modified()).ok();
                match (m1, m2) {
                    (Some(t1), Some(t2)) => {
                        if t1 < t2 {
                            0
                        } else {
                            1
                        }
                    }
                    (None, Some(_)) => 0,
                    _ => 1,
                }
            }
            [f1, "-ef", f2] => {
                use std::os::unix::fs::MetadataExt;
                let m1 = std::fs::metadata(f1).ok();
                let m2 = std::fs::metadata(f2).ok();
                match (m1, m2) {
                    (Some(a), Some(b)) => {
                        if a.dev() == b.dev() && a.ino() == b.ino() {
                            0
                        } else {
                            1
                        }
                    }
                    _ => 1,
                }
            }

            // Single string test
            [s] => {
                if !s.is_empty() {
                    0
                } else {
                    1
                }
            }

            _ => 1,
        }
    }

    fn builtin_local(&mut self, args: &[String]) -> i32 {
        let mut is_array = false;
        let mut is_assoc = false;
        let mut i = 0;

        // Parse options
        while i < args.len() {
            let arg = &args[i];
            if arg.starts_with('-') {
                for c in arg[1..].chars() {
                    match c {
                        'a' => is_array = true,
                        'A' => is_assoc = true,
                        'i' | 'r' | 'x' | 'l' | 'u' => {} // ignore other flags
                        _ => {}
                    }
                }
                i += 1;
            } else {
                break;
            }
        }

        // Process remaining args - might be "name=(elem1 elem2 ...)" split across multiple args
        while i < args.len() {
            let arg = &args[i];

            // Check if this starts an array assignment: "name=(" or "name=(value"
            if let Some(eq_pos) = arg.find('=') {
                let name = &arg[..eq_pos];
                let rest = &arg[eq_pos + 1..];

                if rest.starts_with('(') {
                    // Array assignment - collect all elements until we find ')'
                    let mut elements = Vec::new();
                    let mut current = rest[1..].to_string(); // skip '('

                    // Check if closing ) is in this arg
                    if let Some(close_pos) = current.find(')') {
                        let content = &current[..close_pos];
                        if !content.is_empty() {
                            elements.extend(content.split_whitespace().map(|s| s.to_string()));
                        }
                    } else {
                        // Add elements from current
                        if !current.is_empty() {
                            elements.push(current);
                        }
                        // Collect remaining args until )
                        i += 1;
                        while i < args.len() {
                            let next = &args[i];
                            if next.ends_with(')') {
                                let content = &next[..next.len() - 1];
                                if !content.is_empty() {
                                    elements.push(content.to_string());
                                }
                                break;
                            } else if let Some(close_pos) = next.find(')') {
                                let content = &next[..close_pos];
                                if !content.is_empty() {
                                    elements.push(content.to_string());
                                }
                                break;
                            } else {
                                elements.push(next.clone());
                            }
                            i += 1;
                        }
                    }

                    // Set array variable
                    self.arrays.insert(name.to_string(), elements);
                    self.variables.insert(name.to_string(), String::new());
                } else {
                    // Regular assignment
                    self.variables.insert(name.to_string(), rest.to_string());
                }
            } else if is_array || is_assoc {
                // Just declaring the variable
                if is_assoc {
                    self.assoc_arrays
                        .insert(arg.clone(), std::collections::HashMap::new());
                } else {
                    self.arrays.insert(arg.clone(), Vec::new());
                }
                self.variables.insert(arg.clone(), String::new());
            } else {
                self.variables.insert(arg.clone(), String::new());
            }
            i += 1;
        }
        0
    }

    fn builtin_declare(&mut self, args: &[String]) -> i32 {
        self.builtin_local(args)
    }

    fn builtin_read(&mut self, args: &[String]) -> i32 {
        let var = args.first().map(|s| s.as_str()).unwrap_or("REPLY");
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(_) => {
                let input = input.trim_end();
                env::set_var(var, input);
                self.variables.insert(var.to_string(), input.to_string());
                0
            }
            Err(_) => 1,
        }
    }

    fn builtin_shift(&mut self, _args: &[String]) -> i32 {
        // Simplified: no-op for now
        0
    }

    fn builtin_eval(&mut self, args: &[String]) -> i32 {
        let code = args.join(" ");
        match self.execute_script(&code) {
            Ok(status) => status,
            Err(e) => {
                eprintln!("eval: {}", e);
                1
            }
        }
    }

    fn builtin_autoload(&mut self, args: &[String]) -> i32 {
        // Parse options like zsh: -U (no alias), -z (zsh style), -k (ksh style),
        // -X (execute now), -x (export), -r (resolve), -R (resolve recurse),
        // -t (trace), -T (trace local), -W (warn nested), -d (use calling dir)
        let mut functions = Vec::new();
        let mut no_alias = false; // -U
        let mut zsh_style = false; // -z
        let mut ksh_style = false; // -k
        let mut execute_now = false; // -X
        let mut resolve = false; // -r
        let mut trace = false; // -t
        let mut use_caller_dir = false; // -d
        let mut list_mode = false;
        let mut plus_mode = false; // +U, +z, etc. to turn off flags

        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];

            if arg == "--" {
                i += 1;
                break;
            }

            if arg.starts_with('+') {
                plus_mode = true;
                let flags = &arg[1..];
                for c in flags.chars() {
                    match c {
                        'U' => no_alias = false,
                        'z' => zsh_style = false,
                        'k' => ksh_style = false,
                        't' => trace = false,
                        'd' => use_caller_dir = false,
                        _ => {}
                    }
                }
            } else if arg.starts_with('-') {
                let flags = &arg[1..];
                if flags.is_empty() {
                    // Just "-" means end of options
                    i += 1;
                    break;
                }
                for c in flags.chars() {
                    match c {
                        'U' => no_alias = true,
                        'z' => zsh_style = true,
                        'k' => ksh_style = true,
                        'X' => execute_now = true,
                        'r' | 'R' => resolve = true,
                        't' => trace = true,
                        'T' => {} // trace local
                        'W' => {} // warn nested
                        'd' => use_caller_dir = true,
                        'w' => {} // wordcode
                        'm' => {} // pattern match
                        _ => {}
                    }
                }
            } else {
                functions.push(arg.clone());
            }
            i += 1;
        }

        // Collect remaining args as function names
        while i < args.len() {
            functions.push(args[i].clone());
            i += 1;
        }

        // If no functions specified, list autoloaded functions
        if functions.is_empty() && !execute_now {
            for (name, _) in &self.autoload_pending {
                if no_alias && zsh_style {
                    println!("autoload -Uz {}", name);
                } else if no_alias {
                    println!("autoload -U {}", name);
                } else {
                    println!("autoload {}", name);
                }
            }
            return 0;
        }

        // Handle -X: load and execute function immediately (called from stub)
        // When a stub function calls `builtin autoload -Xz`, we load the real function
        // and then need to execute it with the original arguments
        if execute_now {
            for func_name in &functions {
                // Load the function from fpath
                if let Some(loaded) = self.load_autoload_function(func_name) {
                    // Extract body from FunctionDef
                    let body = match loaded {
                        ShellCommand::FunctionDef(_, body) => (*body).clone(),
                        other => other,
                    };
                    // Replace the stub with the real function
                    self.functions.insert(func_name.clone(), body);
                    // Remove from pending
                    self.autoload_pending.remove(func_name);
                } else {
                    eprintln!(
                        "autoload: {}: function definition file not found",
                        func_name
                    );
                    return 1;
                }
            }
            return 0;
        }

        // Register functions for autoload - create stub functions
        for func_name in &functions {
            // Store autoload metadata
            let mut flags = AutoloadFlags::empty();
            if no_alias {
                flags |= AutoloadFlags::NO_ALIAS;
            }
            if zsh_style {
                flags |= AutoloadFlags::ZSH_STYLE;
            }
            if ksh_style {
                flags |= AutoloadFlags::KSH_STYLE;
            }
            if trace {
                flags |= AutoloadFlags::TRACE;
            }
            if use_caller_dir {
                flags |= AutoloadFlags::USE_CALLER_DIR;
            }

            self.autoload_pending.insert(func_name.clone(), flags);

            // Create a stub function: `builtin autoload -Xz funcname && funcname "$@"`
            // When called, this loads the real function and re-calls it
            let autoload_opts = if zsh_style && no_alias {
                "-XUz"
            } else if zsh_style {
                "-Xz"
            } else if no_alias {
                "-XU"
            } else {
                "-X"
            };

            // The stub: builtin autoload -Xz funcname && funcname "$@"
            let stub = ShellCommand::List(vec![
                (
                    ShellCommand::Simple(SimpleCommand {
                        assignments: vec![],
                        words: vec![
                            ShellWord::Literal("builtin".to_string()),
                            ShellWord::Literal("autoload".to_string()),
                            ShellWord::Literal(autoload_opts.to_string()),
                            ShellWord::Literal(func_name.clone()),
                        ],
                        redirects: vec![],
                    }),
                    ListOp::And,
                ),
                (
                    ShellCommand::Simple(SimpleCommand {
                        assignments: vec![],
                        words: vec![
                            ShellWord::Literal(func_name.clone()),
                            ShellWord::DoubleQuoted(vec![ShellWord::Variable("@".to_string())]),
                        ],
                        redirects: vec![],
                    }),
                    ListOp::Semi,
                ),
            ]);

            self.functions.insert(func_name.clone(), stub);

            // If -r or -R, resolve the path now to verify it exists
            if resolve {
                if self.find_function_file(func_name).is_none() {
                    eprintln!(
                        "autoload: {}: function definition file not found",
                        func_name
                    );
                }
            }
        }

        0
    }

    /// Find a function file in fpath
    fn find_function_file(&self, name: &str) -> Option<PathBuf> {
        for dir in &self.fpath {
            let path = dir.join(name);
            if path.exists() && path.is_file() {
                return Some(path);
            }
        }
        None
    }

    /// Load an autoloaded function from fpath - reads file and parses it
    fn load_autoload_function(&mut self, name: &str) -> Option<ShellCommand> {
        // First try ZWC cache (but skip if we're reloading an existing function)
        if !self.functions.contains_key(name) {
            // Try to load from ZWC files
            for dir in &self.fpath.clone() {
                // Try dir.zwc (e.g., /path/to/src.zwc for /path/to/src)
                let zwc_path = dir.with_extension("zwc");
                if zwc_path.exists() {
                    // Function name in directory ZWC includes path prefix
                    let prefixed_name = format!(
                        "{}/{}",
                        dir.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                        name
                    );
                    if let Some(func) = self.load_function_from_zwc(&zwc_path, &prefixed_name) {
                        return Some(func);
                    }
                    // Also try without prefix
                    if let Some(func) = self.load_function_from_zwc(&zwc_path, name) {
                        return Some(func);
                    }
                }
                // Try individual function.zwc
                let func_zwc = dir.join(format!("{}.zwc", name));
                if func_zwc.exists() {
                    if let Some(func) = self.load_function_from_zwc(&func_zwc, name) {
                        return Some(func);
                    }
                }
            }
        }

        // Find the function file in fpath
        let path = self.find_function_file(name)?;

        // Read the file
        let content = std::fs::read_to_string(&path).ok()?;

        // Parse the content
        let mut parser = crate::shell_parse::ShellParser::new(&content);

        if let Ok(commands) = parser.parse_script() {
            if commands.is_empty() {
                return None;
            }

            // Check if it's a single function definition for this name (ksh style)
            if commands.len() == 1 {
                if let ShellCommand::FunctionDef(ref fn_name, _) = commands[0] {
                    if fn_name == name {
                        return Some(commands[0].clone());
                    }
                }
            }

            // Otherwise, the file contents become the function body (zsh style)
            // Wrap all commands in a List
            let body = if commands.len() == 1 {
                commands.into_iter().next().unwrap()
            } else {
                // Convert to List with Semi separators
                let list_cmds: Vec<(ShellCommand, ListOp)> =
                    commands.into_iter().map(|c| (c, ListOp::Semi)).collect();
                ShellCommand::List(list_cmds)
            };

            return Some(ShellCommand::FunctionDef(name.to_string(), Box::new(body)));
        }

        None
    }

    /// Check if a function is autoload pending and load it if so
    pub fn maybe_autoload(&mut self, name: &str) -> bool {
        if self.autoload_pending.contains_key(name) {
            if let Some(func) = self.load_autoload_function(name) {
                // For FunctionDef, extract the body and store it
                let to_store = match func {
                    ShellCommand::FunctionDef(_, body) => (*body).clone(),
                    other => other,
                };
                self.functions.insert(name.to_string(), to_store);
                self.autoload_pending.remove(name);
                return true;
            }
        }
        false
    }

    fn builtin_jobs(&mut self, _args: &[String]) -> i32 {
        // Reap finished jobs first
        for job in self.jobs.reap_finished() {
            println!("[{}]  Done                    {}", job.id, job.command);
        }

        // List remaining jobs
        for job in self.jobs.list() {
            let marker = if job.is_current { "+" } else { "-" };
            let state = match job.state {
                JobState::Running => "Running",
                JobState::Stopped => "Stopped",
                JobState::Done => "Done",
            };
            println!(
                "[{}]{} {}                    {}",
                job.id, marker, state, job.command
            );
        }
        0
    }

    fn builtin_fg(&mut self, args: &[String]) -> i32 {
        let job_id = if let Some(arg) = args.first() {
            // Parse %N or just N
            let s = arg.trim_start_matches('%');
            match s.parse::<usize>() {
                Ok(id) => Some(id),
                Err(_) => {
                    eprintln!("fg: {}: no such job", arg);
                    return 1;
                }
            }
        } else {
            self.jobs.current().map(|j| j.id)
        };

        let Some(id) = job_id else {
            eprintln!("fg: no current job");
            return 1;
        };

        let Some(job) = self.jobs.get(id) else {
            eprintln!("fg: %{}: no such job", id);
            return 1;
        };

        let pid = job.pid;
        let cmd = job.command.clone();
        println!("{}", cmd);

        // Continue the job
        if let Err(e) = continue_job(pid) {
            eprintln!("fg: {}", e);
            return 1;
        }

        // Wait for it
        match wait_for_job(pid) {
            Ok(status) => {
                self.jobs.remove(id);
                status
            }
            Err(e) => {
                eprintln!("fg: {}", e);
                1
            }
        }
    }

    fn builtin_bg(&mut self, args: &[String]) -> i32 {
        let job_id = if let Some(arg) = args.first() {
            let s = arg.trim_start_matches('%');
            match s.parse::<usize>() {
                Ok(id) => Some(id),
                Err(_) => {
                    eprintln!("bg: {}: no such job", arg);
                    return 1;
                }
            }
        } else {
            self.jobs.current().map(|j| j.id)
        };

        let Some(id) = job_id else {
            eprintln!("bg: no current job");
            return 1;
        };

        let Some(job) = self.jobs.get_mut(id) else {
            eprintln!("bg: %{}: no such job", id);
            return 1;
        };

        let pid = job.pid;
        let cmd = job.command.clone();

        if let Err(e) = continue_job(pid) {
            eprintln!("bg: {}", e);
            return 1;
        }

        job.state = JobState::Running;
        println!("[{}] {} &", id, cmd);
        0
    }

    fn builtin_kill(&mut self, args: &[String]) -> i32 {
        use crate::jobs::send_signal;
        use nix::sys::signal::Signal;

        if args.is_empty() {
            eprintln!("kill: usage: kill [-signal] pid ...");
            return 1;
        }

        let mut sig = Signal::SIGTERM;
        let mut start = 0;

        // Check for signal specification
        if let Some(first) = args.first() {
            if first.starts_with('-') {
                let sig_str = first.trim_start_matches('-');
                sig = match sig_str.to_uppercase().as_str() {
                    "1" | "HUP" | "SIGHUP" => Signal::SIGHUP,
                    "2" | "INT" | "SIGINT" => Signal::SIGINT,
                    "3" | "QUIT" | "SIGQUIT" => Signal::SIGQUIT,
                    "9" | "KILL" | "SIGKILL" => Signal::SIGKILL,
                    "15" | "TERM" | "SIGTERM" => Signal::SIGTERM,
                    "18" | "CONT" | "SIGCONT" => Signal::SIGCONT,
                    "19" | "STOP" | "SIGSTOP" => Signal::SIGSTOP,
                    _ => {
                        eprintln!("kill: {}: invalid signal", first);
                        return 1;
                    }
                };
                start = 1;
            }
        }

        let mut status = 0;
        for arg in &args[start..] {
            // Handle %job syntax
            if arg.starts_with('%') {
                let id: usize = match arg[1..].parse() {
                    Ok(id) => id,
                    Err(_) => {
                        eprintln!("kill: {}: no such job", arg);
                        status = 1;
                        continue;
                    }
                };
                if let Some(job) = self.jobs.get(id) {
                    if let Err(e) = send_signal(job.pid, sig) {
                        eprintln!("kill: {}", e);
                        status = 1;
                    }
                } else {
                    eprintln!("kill: {}: no such job", arg);
                    status = 1;
                }
            } else {
                // Direct PID
                let pid: u32 = match arg.parse() {
                    Ok(p) => p,
                    Err(_) => {
                        eprintln!("kill: {}: invalid pid", arg);
                        status = 1;
                        continue;
                    }
                };
                if let Err(e) = send_signal(pid, sig) {
                    eprintln!("kill: {}", e);
                    status = 1;
                }
            }
        }
        status
    }

    fn builtin_disown(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            // Disown current job
            if let Some(job) = self.jobs.current() {
                let id = job.id;
                self.jobs.remove(id);
            }
            return 0;
        }

        for arg in args {
            let s = arg.trim_start_matches('%');
            if let Ok(id) = s.parse::<usize>() {
                self.jobs.remove(id);
            } else {
                eprintln!("disown: {}: no such job", arg);
            }
        }
        0
    }

    fn builtin_wait(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            // Wait for all jobs
            let ids: Vec<usize> = self.jobs.list().iter().map(|j| j.id).collect();
            for id in ids {
                if let Some(mut job) = self.jobs.remove(id) {
                    if let Some(ref mut child) = job.child {
                        let _ = wait_for_child(child);
                    }
                }
            }
            return 0;
        }

        let mut status = 0;
        for arg in args {
            if arg.starts_with('%') {
                let id: usize = match arg[1..].parse() {
                    Ok(id) => id,
                    Err(_) => {
                        eprintln!("wait: {}: no such job", arg);
                        status = 127;
                        continue;
                    }
                };
                if let Some(mut job) = self.jobs.remove(id) {
                    if let Some(ref mut child) = job.child {
                        match wait_for_child(child) {
                            Ok(s) => status = s,
                            Err(e) => {
                                eprintln!("wait: {}", e);
                                status = 127;
                            }
                        }
                    }
                } else {
                    eprintln!("wait: {}: no such job", arg);
                    status = 127;
                }
            } else {
                let pid: u32 = match arg.parse() {
                    Ok(p) => p,
                    Err(_) => {
                        eprintln!("wait: {}: invalid pid", arg);
                        status = 127;
                        continue;
                    }
                };
                match wait_for_job(pid) {
                    Ok(s) => status = s,
                    Err(e) => {
                        eprintln!("wait: {}", e);
                        status = 127;
                    }
                }
            }
        }
        status
    }

    fn builtin_suspend(&self, args: &[String]) -> i32 {
        let mut force = false;
        for arg in args {
            if arg == "-f" {
                force = true;
            }
        }

        #[cfg(unix)]
        {
            use nix::sys::signal::{kill, Signal};
            use nix::unistd::getppid;

            // Check if we're a login shell (parent is init/PID 1)
            let ppid = getppid();
            if !force && ppid == nix::unistd::Pid::from_raw(1) {
                eprintln!("suspend: cannot suspend a login shell");
                return 1;
            }

            // Send SIGTSTP to ourselves
            let pid = nix::unistd::getpid();
            if let Err(e) = kill(pid, Signal::SIGTSTP) {
                eprintln!("suspend: {}", e);
                return 1;
            }
            0
        }

        #[cfg(not(unix))]
        {
            eprintln!("suspend: not supported on this platform");
            1
        }
    }
}

impl Default for ShellExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_echo() {
        let mut exec = ShellExecutor::new();
        let status = exec.execute_script("true").unwrap();
        assert_eq!(status, 0);
    }

    #[test]
    fn test_if_true() {
        let mut exec = ShellExecutor::new();
        let status = exec.execute_script("if true; then true; fi").unwrap();
        assert_eq!(status, 0);
    }

    #[test]
    fn test_if_false() {
        let mut exec = ShellExecutor::new();
        let status = exec
            .execute_script("if false; then true; else false; fi")
            .unwrap();
        assert_eq!(status, 1);
    }

    #[test]
    fn test_for_loop() {
        let mut exec = ShellExecutor::new();
        exec.execute_script("for i in a b c; do true; done")
            .unwrap();
        assert_eq!(exec.last_status, 0);
    }

    #[test]
    fn test_and_list() {
        let mut exec = ShellExecutor::new();
        let status = exec.execute_script("true && true").unwrap();
        assert_eq!(status, 0);

        let status = exec.execute_script("true && false").unwrap();
        assert_eq!(status, 1);
    }

    #[test]
    fn test_or_list() {
        let mut exec = ShellExecutor::new();
        let status = exec.execute_script("false || true").unwrap();
        assert_eq!(status, 0);
    }
}

impl ShellExecutor {
    fn builtin_history(&self, args: &[String]) -> i32 {
        let Some(ref engine) = self.history else {
            eprintln!("history: history engine not available");
            return 1;
        };

        // Parse options
        let mut count = 20usize;
        let mut show_all = false;
        let mut search_query = None;

        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "-c" | "--clear" => {
                    // Clear history - need mutable access
                    eprintln!("history: clear not supported in this mode");
                    return 1;
                }
                "-a" | "--all" => show_all = true,
                "-n" => {
                    if i + 1 < args.len() {
                        i += 1;
                        count = args[i].parse().unwrap_or(20);
                    }
                }
                s if s.starts_with('-') && s[1..].chars().all(|c| c.is_ascii_digit()) => {
                    count = s[1..].parse().unwrap_or(20);
                }
                s if s.chars().all(|c| c.is_ascii_digit()) => {
                    count = s.parse().unwrap_or(20);
                }
                s => {
                    search_query = Some(s.to_string());
                }
            }
            i += 1;
        }

        if show_all {
            count = 10000;
        }

        let entries = if let Some(ref q) = search_query {
            engine.search(q, count)
        } else {
            engine.recent(count)
        };

        match entries {
            Ok(entries) => {
                // Print in chronological order (reverse the results since recent() is newest-first)
                for entry in entries.into_iter().rev() {
                    println!("{:>6}  {}", entry.id, entry.command);
                }
                0
            }
            Err(e) => {
                eprintln!("history: {}", e);
                1
            }
        }
    }

    fn builtin_fc(&mut self, args: &[String]) -> i32 {
        let Some(ref engine) = self.history else {
            eprintln!("fc: history engine not available");
            return 1;
        };

        // fc -l: list
        // fc -e -: re-execute last command
        // fc -s old=new: substitute and execute

        if args.is_empty() || args[0] == "-l" {
            // List mode
            let count = if args.len() > 1 {
                args[1].parse().unwrap_or(16)
            } else {
                16
            };

            match engine.recent(count) {
                Ok(entries) => {
                    for entry in entries.into_iter().rev() {
                        println!("{:>6}  {}", entry.id, entry.command);
                    }
                    0
                }
                Err(e) => {
                    eprintln!("fc: {}", e);
                    1
                }
            }
        } else if args[0] == "-e" && args.get(1).map(|s| s.as_str()) == Some("-") {
            // Re-execute last command
            match engine.get_by_offset(0) {
                Ok(Some(entry)) => {
                    println!("{}", entry.command);
                    self.execute_script(&entry.command).unwrap_or(1)
                }
                Ok(None) => {
                    eprintln!("fc: no command to re-execute");
                    1
                }
                Err(e) => {
                    eprintln!("fc: {}", e);
                    1
                }
            }
        } else if args[0] == "-s" {
            // Substitution mode: fc -s old=new or fc -s pattern
            let substitution = args.get(1);

            match engine.get_by_offset(0) {
                Ok(Some(entry)) => {
                    let mut cmd = entry.command.clone();

                    if let Some(sub) = substitution {
                        if let Some((old, new)) = sub.split_once('=') {
                            cmd = cmd.replace(old, new);
                        }
                    }

                    println!("{}", cmd);
                    self.execute_script(&cmd).unwrap_or(1)
                }
                Ok(None) => {
                    eprintln!("fc: no command to re-execute");
                    1
                }
                Err(e) => {
                    eprintln!("fc: {}", e);
                    1
                }
            }
        } else if args[0].starts_with('-') && args[0].chars().skip(1).all(|c| c.is_ascii_digit()) {
            // fc -N: re-execute Nth previous command
            let n: usize = args[0][1..].parse().unwrap_or(1);
            match engine.get_by_offset(n - 1) {
                Ok(Some(entry)) => {
                    println!("{}", entry.command);
                    self.execute_script(&entry.command).unwrap_or(1)
                }
                Ok(None) => {
                    eprintln!("fc: event not found");
                    1
                }
                Err(e) => {
                    eprintln!("fc: {}", e);
                    1
                }
            }
        } else {
            // Try to find command by prefix
            let prefix = &args[0];
            match engine.search_prefix(prefix, 1) {
                Ok(entries) if !entries.is_empty() => {
                    println!("{}", entries[0].command);
                    self.execute_script(&entries[0].command).unwrap_or(1)
                }
                Ok(_) => {
                    eprintln!("fc: event not found: {}", prefix);
                    1
                }
                Err(e) => {
                    eprintln!("fc: {}", e);
                    1
                }
            }
        }
    }

    fn builtin_trap(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            // List all traps
            for (sig, action) in &self.traps {
                println!("trap -- '{}' {}", action, sig);
            }
            return 0;
        }

        // trap -l: list signal names
        if args.len() == 1 && args[0] == "-l" {
            let signals = [
                "HUP", "INT", "QUIT", "ILL", "TRAP", "ABRT", "BUS", "FPE", "KILL", "USR1", "SEGV",
                "USR2", "PIPE", "ALRM", "TERM", "STKFLT", "CHLD", "CONT", "STOP", "TSTP", "TTIN",
                "TTOU", "URG", "XCPU", "XFSZ", "VTALRM", "PROF", "WINCH", "IO", "PWR", "SYS",
            ];
            for (i, sig) in signals.iter().enumerate() {
                print!("{:2}) SIG{:<8}", i + 1, sig);
                if (i + 1) % 5 == 0 {
                    println!();
                }
            }
            println!();
            return 0;
        }

        // trap -p [sigspec...]: print trap commands
        if args.len() >= 1 && args[0] == "-p" {
            let signals = if args.len() > 1 {
                &args[1..]
            } else {
                &[] as &[String]
            };
            if signals.is_empty() {
                for (sig, action) in &self.traps {
                    println!("trap -- '{}' {}", action, sig);
                }
            } else {
                for sig in signals {
                    if let Some(action) = self.traps.get(sig) {
                        println!("trap -- '{}' {}", action, sig);
                    }
                }
            }
            return 0;
        }

        // trap '' signal: reset to default
        // trap action signal...: set trap
        // trap signal: print current action for signal
        if args.len() == 1 {
            // Print trap for this signal
            let sig = &args[0];
            if let Some(action) = self.traps.get(sig) {
                println!("trap -- '{}' {}", action, sig);
            }
            return 0;
        }

        let action = &args[0];
        let signals = &args[1..];

        for sig in signals {
            let sig_upper = sig.to_uppercase();
            let sig_name = if sig_upper.starts_with("SIG") {
                sig_upper[3..].to_string()
            } else {
                sig_upper.clone()
            };

            if action.is_empty() || action == "-" {
                // Reset to default
                self.traps.remove(&sig_name);
            } else {
                self.traps.insert(sig_name, action.clone());
            }
        }

        0
    }

    /// Execute trap handlers for a signal
    pub fn run_trap(&mut self, signal: &str) {
        if let Some(action) = self.traps.get(signal).cloned() {
            let _ = self.execute_script(&action);
        }
    }

    fn builtin_alias(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            // List all aliases
            for (name, value) in &self.aliases {
                println!("alias {}='{}'", name, value);
            }
            return 0;
        }

        for arg in args {
            if let Some(eq_pos) = arg.find('=') {
                // Define alias: name=value
                let name = &arg[..eq_pos];
                let value = &arg[eq_pos + 1..];
                self.aliases.insert(name.to_string(), value.to_string());
            } else {
                // Print alias
                if let Some(value) = self.aliases.get(arg) {
                    println!("alias {}='{}'", arg, value);
                } else {
                    eprintln!("zshrs: alias: {}: not found", arg);
                    return 1;
                }
            }
        }
        0
    }

    fn builtin_unalias(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            eprintln!("zshrs: unalias: usage: unalias [-a] name [name ...]");
            return 1;
        }

        if args.len() == 1 && args[0] == "-a" {
            // Remove all aliases
            self.aliases.clear();
            return 0;
        }

        for name in args {
            if name == "-a" {
                continue;
            }
            if self.aliases.remove(name).is_none() {
                eprintln!("zshrs: unalias: {}: not found", name);
                return 1;
            }
        }
        0
    }

    fn builtin_set(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            // List all variables and their values (zsh behavior)
            let mut vars: Vec<_> = self.variables.iter().collect();
            vars.sort_by_key(|(k, _)| *k);
            for (k, v) in vars {
                println!("{}={}", k, shell_quote(v));
            }
            // Also print arrays
            let mut arrs: Vec<_> = self.arrays.iter().collect();
            arrs.sort_by_key(|(k, _)| *k);
            for (k, v) in arrs {
                let quoted: Vec<String> = v.iter().map(|s| shell_quote(s)).collect();
                println!("{}=( {} )", k, quoted.join(" "));
            }
            return 0;
        }

        // Check for "+" alone - print just variable names
        if args.len() == 1 && args[0] == "+" {
            let mut names: Vec<_> = self.variables.keys().collect();
            names.extend(self.arrays.keys());
            names.sort();
            names.dedup();
            for name in names {
                println!("{}", name);
            }
            return 0;
        }

        let mut iter = args.iter().peekable();
        let mut set_array: Option<bool> = None; // Some(true) = -A, Some(false) = +A
        let mut array_name: Option<String> = None;
        let mut sort_asc = false;
        let mut sort_desc = false;

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "-o" => {
                    // -o with no arg: print all options in "option on/off" format
                    if iter.peek().is_none()
                        || iter
                            .peek()
                            .map(|s| s.starts_with('-') || s.starts_with('+'))
                            .unwrap_or(false)
                    {
                        self.print_options_table();
                        continue;
                    }
                    if let Some(opt) = iter.next() {
                        let (name, enable) = Self::normalize_option_name(opt);
                        self.options.insert(name, enable);
                    }
                }
                "+o" => {
                    // +o with no arg: print options in re-entrant format
                    if iter.peek().is_none()
                        || iter
                            .peek()
                            .map(|s| s.starts_with('-') || s.starts_with('+'))
                            .unwrap_or(false)
                    {
                        self.print_options_reentrant();
                        continue;
                    }
                    if let Some(opt) = iter.next() {
                        let (name, enable) = Self::normalize_option_name(opt);
                        self.options.insert(name, !enable);
                    }
                }
                "-A" => {
                    set_array = Some(true);
                    if let Some(name) = iter.next() {
                        if !name.starts_with('-') && !name.starts_with('+') {
                            array_name = Some(name.clone());
                        }
                    }
                    if array_name.is_none() {
                        // Print all arrays with values
                        let mut arrs: Vec<_> = self.arrays.iter().collect();
                        arrs.sort_by_key(|(k, _)| *k);
                        for (k, v) in arrs {
                            let quoted: Vec<String> = v.iter().map(|s| shell_quote(s)).collect();
                            println!("{}=( {} )", k, quoted.join(" "));
                        }
                        return 0;
                    }
                }
                "+A" => {
                    set_array = Some(false);
                    if let Some(name) = iter.next() {
                        if !name.starts_with('-') && !name.starts_with('+') {
                            array_name = Some(name.clone());
                        }
                    }
                    if array_name.is_none() {
                        // Print array names only
                        let mut names: Vec<_> = self.arrays.keys().collect();
                        names.sort();
                        for name in names {
                            println!("{}", name);
                        }
                        return 0;
                    }
                }
                "-s" => sort_asc = true,
                "+s" => sort_desc = true,
                "-e" => {
                    self.options.insert("errexit".to_string(), true);
                }
                "+e" => {
                    self.options.insert("errexit".to_string(), false);
                }
                "-x" => {
                    self.options.insert("xtrace".to_string(), true);
                }
                "+x" => {
                    self.options.insert("xtrace".to_string(), false);
                }
                "-u" => {
                    self.options.insert("nounset".to_string(), true);
                }
                "+u" => {
                    self.options.insert("nounset".to_string(), false);
                }
                "-v" => {
                    self.options.insert("verbose".to_string(), true);
                }
                "+v" => {
                    self.options.insert("verbose".to_string(), false);
                }
                "-n" => {
                    self.options.insert("exec".to_string(), false);
                }
                "+n" => {
                    self.options.insert("exec".to_string(), true);
                }
                "-f" => {
                    self.options.insert("glob".to_string(), false);
                }
                "+f" => {
                    self.options.insert("glob".to_string(), true);
                }
                "-m" => {
                    self.options.insert("monitor".to_string(), true);
                }
                "+m" => {
                    self.options.insert("monitor".to_string(), false);
                }
                "-C" => {
                    self.options.insert("clobber".to_string(), false);
                }
                "+C" => {
                    self.options.insert("clobber".to_string(), true);
                }
                "-b" => {
                    self.options.insert("notify".to_string(), true);
                }
                "+b" => {
                    self.options.insert("notify".to_string(), false);
                }
                "--" => {
                    let remaining: Vec<String> = iter.cloned().collect();
                    if let Some(ref name) = array_name {
                        let mut values = remaining;
                        if sort_asc {
                            values.sort();
                        } else if sort_desc {
                            values.sort();
                            values.reverse();
                        }
                        if set_array == Some(true) {
                            self.arrays.insert(name.clone(), values);
                        } else {
                            // +A: replace initial elements
                            let arr = self.arrays.entry(name.clone()).or_default();
                            for (i, v) in values.into_iter().enumerate() {
                                if i < arr.len() {
                                    arr[i] = v;
                                } else {
                                    arr.push(v);
                                }
                            }
                        }
                    } else if remaining.is_empty() {
                        // "set --" with nothing after unsets positional params
                        self.positional_params.clear();
                    } else {
                        let mut values = remaining;
                        if sort_asc {
                            values.sort();
                        } else if sort_desc {
                            values.sort();
                            values.reverse();
                        }
                        self.positional_params = values;
                    }
                    return 0;
                }
                _ => {
                    // Handle single-letter options like -ex (multiple options)
                    if arg.starts_with('-') && arg.len() > 1 {
                        for c in arg[1..].chars() {
                            match c {
                                'e' => {
                                    self.options.insert("errexit".to_string(), true);
                                }
                                'x' => {
                                    self.options.insert("xtrace".to_string(), true);
                                }
                                'u' => {
                                    self.options.insert("nounset".to_string(), true);
                                }
                                'v' => {
                                    self.options.insert("verbose".to_string(), true);
                                }
                                'n' => {
                                    self.options.insert("exec".to_string(), false);
                                }
                                'f' => {
                                    self.options.insert("glob".to_string(), false);
                                }
                                'm' => {
                                    self.options.insert("monitor".to_string(), true);
                                }
                                'C' => {
                                    self.options.insert("clobber".to_string(), false);
                                }
                                'b' => {
                                    self.options.insert("notify".to_string(), true);
                                }
                                _ => {
                                    eprintln!("zshrs: set: -{}: invalid option", c);
                                    return 1;
                                }
                            }
                        }
                        continue;
                    }
                    if arg.starts_with('+') && arg.len() > 1 {
                        for c in arg[1..].chars() {
                            match c {
                                'e' => {
                                    self.options.insert("errexit".to_string(), false);
                                }
                                'x' => {
                                    self.options.insert("xtrace".to_string(), false);
                                }
                                'u' => {
                                    self.options.insert("nounset".to_string(), false);
                                }
                                'v' => {
                                    self.options.insert("verbose".to_string(), false);
                                }
                                'n' => {
                                    self.options.insert("exec".to_string(), true);
                                }
                                'f' => {
                                    self.options.insert("glob".to_string(), true);
                                }
                                'm' => {
                                    self.options.insert("monitor".to_string(), false);
                                }
                                'C' => {
                                    self.options.insert("clobber".to_string(), true);
                                }
                                'b' => {
                                    self.options.insert("notify".to_string(), false);
                                }
                                _ => {
                                    eprintln!("zshrs: set: +{}: invalid option", c);
                                    return 1;
                                }
                            }
                        }
                        continue;
                    }
                    // Treat as positional params
                    let mut values: Vec<String> =
                        std::iter::once(arg.clone()).chain(iter.cloned()).collect();
                    if sort_asc {
                        values.sort();
                    } else if sort_desc {
                        values.sort();
                        values.reverse();
                    }
                    if let Some(ref name) = array_name {
                        if set_array == Some(true) {
                            self.arrays.insert(name.clone(), values);
                        } else {
                            let arr = self.arrays.entry(name.clone()).or_default();
                            for (i, v) in values.into_iter().enumerate() {
                                if i < arr.len() {
                                    arr[i] = v;
                                } else {
                                    arr.push(v);
                                }
                            }
                        }
                    } else {
                        self.positional_params = values;
                    }
                    return 0;
                }
            }
        }
        0
    }

    fn default_on_options() -> &'static [&'static str] {
        &[
            "aliases",
            "alwayslastprompt",
            "appendhistory",
            "autolist",
            "automenu",
            "autoparamkeys",
            "autoparamslash",
            "autoremoveslash",
            "badpattern",
            "banghist",
            "bareglobqual",
            "beep",
            "bgnice",
            "caseglob",
            "casematch",
            "checkjobs",
            "checkrunningjobs",
            "clobber",
            "debugbeforecmd",
            "equals",
            "evallineno",
            "exec",
            "flowcontrol",
            "functionargzero",
            "glob",
            "globalexport",
            "globalrcs",
            "hashcmds",
            "hashdirs",
            "hashlistall",
            "histbeep",
            "histsavebycopy",
            "hup",
            "interactive",
            "listambiguous",
            "listbeep",
            "listtypes",
            "monitor",
            "multibyte",
            "multifuncdef",
            "multios",
            "nomatch",
            "notify",
            "promptcr",
            "promptpercent",
            "promptsp",
            "rcs",
            "shinstdin",
            "shortloops",
            "unset",
            "zle",
        ]
    }

    fn print_options_table(&self) {
        let mut opts: Vec<_> = Self::all_zsh_options().to_vec();
        opts.sort();
        let defaults_on = Self::default_on_options();
        for &opt in &opts {
            let enabled = self.options.get(opt).copied().unwrap_or(false);
            let is_default_on = defaults_on.contains(&opt);
            // zsh format: for default-ON options, show "noOPTION off" when on, "noOPTION on" when off
            // for default-OFF options, show "OPTION off" when off, "OPTION on" when on
            let (display_name, display_state) = if is_default_on {
                (format!("no{}", opt), if enabled { "off" } else { "on" })
            } else {
                (opt.to_string(), if enabled { "on" } else { "off" })
            };
            println!("{:<22}{}", display_name, display_state);
        }
    }

    fn print_options_reentrant(&self) {
        let mut opts: Vec<_> = Self::all_zsh_options().to_vec();
        opts.sort();
        let defaults_on = Self::default_on_options();
        for &opt in &opts {
            let enabled = self.options.get(opt).copied().unwrap_or(false);
            let is_default_on = defaults_on.contains(&opt);
            // zsh format: use noOPTION for default-on options
            let (display_name, use_minus) = if is_default_on {
                (format!("no{}", opt), !enabled)
            } else {
                (opt.to_string(), enabled)
            };
            if use_minus {
                println!("set -o {}", display_name);
            } else {
                println!("set +o {}", display_name);
            }
        }
    }

    fn builtin_shopt(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            // List all shell options
            for (opt, val) in &self.options {
                println!("shopt {} {}", if *val { "-s" } else { "-u" }, opt);
            }
            return 0;
        }

        let mut set = None;
        let mut opts = Vec::new();

        for arg in args {
            match arg.as_str() {
                "-s" => set = Some(true),
                "-u" => set = Some(false),
                "-p" => {
                    // Print option status
                    for opt in &opts {
                        let val = self.options.get(opt).copied().unwrap_or(false);
                        println!("shopt {} {}", if val { "-s" } else { "-u" }, opt);
                    }
                    return 0;
                }
                _ => opts.push(arg.clone()),
            }
        }

        if let Some(enable) = set {
            for opt in &opts {
                self.options.insert(opt.clone(), enable);
            }
        } else {
            // Query options
            for opt in &opts {
                let val = self.options.get(opt).copied().unwrap_or(false);
                println!("shopt {} {}", if val { "-s" } else { "-u" }, opt);
            }
        }
        0
    }

    /// zsh-compatible setopt builtin
    fn builtin_setopt(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            // List options that differ from compiled-in defaults (zsh behavior)
            // For default-ON options: show "noOPTION" if currently OFF
            // For default-OFF options: show "OPTION" if currently ON
            let defaults_on = Self::default_on_options();
            let mut diff_opts: Vec<String> = Vec::new();

            for &opt in Self::all_zsh_options() {
                let enabled = self.options.get(opt).copied().unwrap_or(false);
                let is_default_on = defaults_on.contains(&opt);

                if is_default_on && !enabled {
                    // Default ON but currently OFF -> show noOPTION
                    diff_opts.push(format!("no{}", opt));
                } else if !is_default_on && enabled {
                    // Default OFF but currently ON -> show OPTION
                    diff_opts.push(opt.to_string());
                }
            }
            diff_opts.sort();
            for opt in diff_opts {
                println!("{}", opt);
            }
            return 0;
        }

        let mut use_pattern = false;
        let mut iter = args.iter().peekable();

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "-m" => use_pattern = true,
                "-o" => {
                    // -o option_name: set option
                    if let Some(opt) = iter.next() {
                        let (name, enable) = Self::normalize_option_name(opt);
                        self.options.insert(name, enable);
                    }
                }
                "+o" => {
                    // +o option_name: unset option
                    if let Some(opt) = iter.next() {
                        let (name, enable) = Self::normalize_option_name(opt);
                        self.options.insert(name, !enable);
                    }
                }
                _ => {
                    if use_pattern {
                        // Match pattern against all options
                        for opt in Self::all_zsh_options() {
                            if Self::option_matches_pattern(opt, arg) {
                                self.options.insert(opt.to_string(), true);
                            }
                        }
                    } else {
                        let (name, enable) = Self::normalize_option_name(arg);
                        // Verify it's a valid option (zsh doesn't error on bad names in setopt)
                        self.options.insert(name, enable);
                    }
                }
            }
        }
        0
    }

    /// zsh-compatible unsetopt builtin
    fn builtin_unsetopt(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            // List all options in the format you'd pass to unsetopt to disable them
            // For default-ON options: show "noOPTION" (to turn it off)
            // For default-OFF options: show "OPTION" (already off, but this is what you'd type)
            let defaults_on = Self::default_on_options();
            let mut all_opts: Vec<String> = Vec::new();

            for &opt in Self::all_zsh_options() {
                let is_default_on = defaults_on.contains(&opt);
                if is_default_on {
                    all_opts.push(format!("no{}", opt));
                } else {
                    all_opts.push(opt.to_string());
                }
            }
            all_opts.sort();
            for opt in all_opts {
                println!("{}", opt);
            }
            return 0;
        }

        let mut use_pattern = false;
        let mut iter = args.iter().peekable();

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "-m" => use_pattern = true,
                "-o" => {
                    // -o option_name: unset option
                    if let Some(opt) = iter.next() {
                        let (name, enable) = Self::normalize_option_name(opt);
                        self.options.insert(name, !enable);
                    }
                }
                "+o" => {
                    // +o option_name: set option (opposite in unsetopt)
                    if let Some(opt) = iter.next() {
                        let (name, enable) = Self::normalize_option_name(opt);
                        self.options.insert(name, enable);
                    }
                }
                _ => {
                    if use_pattern {
                        for opt in Self::all_zsh_options() {
                            if Self::option_matches_pattern(opt, arg) {
                                self.options.insert(opt.to_string(), false);
                            }
                        }
                    } else {
                        let (name, enable) = Self::normalize_option_name(arg);
                        // unsetopt turns OFF the option (or ON if "no" prefix)
                        self.options.insert(name, !enable);
                    }
                }
            }
        }
        0
    }

    fn builtin_getopts(&mut self, args: &[String]) -> i32 {
        if args.len() < 2 {
            eprintln!("zshrs: getopts: usage: getopts optstring name [arg ...]");
            return 1;
        }

        let optstring = &args[0];
        let varname = &args[1];
        let opt_args: Vec<&str> = if args.len() > 2 {
            args[2..].iter().map(|s| s.as_str()).collect()
        } else {
            self.positional_params.iter().map(|s| s.as_str()).collect()
        };

        // Get current OPTIND
        let optind: usize = self
            .variables
            .get("OPTIND")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);

        if optind > opt_args.len() {
            self.variables.insert(varname.to_string(), "?".to_string());
            return 1;
        }

        let current_arg = opt_args[optind - 1];

        if !current_arg.starts_with('-') || current_arg == "-" {
            self.variables.insert(varname.to_string(), "?".to_string());
            return 1;
        }

        if current_arg == "--" {
            self.variables
                .insert("OPTIND".to_string(), (optind + 1).to_string());
            self.variables.insert(varname.to_string(), "?".to_string());
            return 1;
        }

        // Get current option position within the argument
        let optpos: usize = self
            .variables
            .get("_OPTPOS")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);

        let opt_char = current_arg.chars().nth(optpos);

        if let Some(c) = opt_char {
            // Look up option in optstring
            let opt_idx = optstring.find(c);

            match opt_idx {
                Some(idx) => {
                    // Check if option takes an argument
                    let takes_arg = optstring.chars().nth(idx + 1) == Some(':');

                    if takes_arg {
                        // Get argument
                        let arg = if optpos + 1 < current_arg.len() {
                            // Argument is rest of current arg
                            current_arg[optpos + 1..].to_string()
                        } else if optind < opt_args.len() {
                            // Argument is next arg
                            self.variables
                                .insert("OPTIND".to_string(), (optind + 2).to_string());
                            self.variables.remove("_OPTPOS");
                            opt_args[optind].to_string()
                        } else {
                            // Missing argument
                            self.variables.insert(varname.to_string(), "?".to_string());
                            if !optstring.starts_with(':') {
                                eprintln!("zshrs: getopts: option requires an argument -- {}", c);
                            }
                            self.variables.insert("OPTARG".to_string(), c.to_string());
                            return 1;
                        };

                        self.variables.insert("OPTARG".to_string(), arg);
                        self.variables
                            .insert("OPTIND".to_string(), (optind + 1).to_string());
                        self.variables.remove("_OPTPOS");
                    } else {
                        // No argument needed
                        if optpos + 1 < current_arg.len() {
                            // More options in this arg
                            self.variables
                                .insert("_OPTPOS".to_string(), (optpos + 1).to_string());
                        } else {
                            // Move to next arg
                            self.variables
                                .insert("OPTIND".to_string(), (optind + 1).to_string());
                            self.variables.remove("_OPTPOS");
                        }
                    }

                    self.variables.insert(varname.to_string(), c.to_string());
                    0
                }
                None => {
                    // Unknown option
                    if !optstring.starts_with(':') {
                        eprintln!("zshrs: getopts: illegal option -- {}", c);
                    }
                    self.variables.insert(varname.to_string(), "?".to_string());
                    self.variables.insert("OPTARG".to_string(), c.to_string());

                    // Advance to next option/arg
                    if optpos + 1 < current_arg.len() {
                        self.variables
                            .insert("_OPTPOS".to_string(), (optpos + 1).to_string());
                    } else {
                        self.variables
                            .insert("OPTIND".to_string(), (optind + 1).to_string());
                        self.variables.remove("_OPTPOS");
                    }
                    0
                }
            }
        } else {
            // No more options in current arg
            self.variables
                .insert("OPTIND".to_string(), (optind + 1).to_string());
            self.variables.remove("_OPTPOS");
            self.variables.insert(varname.to_string(), "?".to_string());
            1
        }
    }

    fn builtin_type(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            return 0;
        }

        let mut show_all = false;
        let mut path_only = false;
        let mut silent = false;
        let mut show_type = false;
        let mut names = Vec::new();

        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            if arg.starts_with('-') && arg.len() > 1 {
                for c in arg[1..].chars() {
                    match c {
                        'a' => show_all = true,
                        'p' => path_only = true,
                        'P' => path_only = true,
                        's' => silent = true,
                        't' => show_type = true,
                        'f' => {} // ignore functions (we still show them)
                        'w' => {} // like -t but different format
                        _ => {}
                    }
                }
            } else {
                names.push(arg.clone());
            }
        }

        if names.is_empty() {
            return 0;
        }

        let mut status = 0;
        for name in &names {
            let mut found_any = false;

            // Check for alias (skip if -p)
            if !path_only && self.aliases.contains_key(name) {
                found_any = true;
                if !silent {
                    if show_type {
                        println!("alias");
                    } else {
                        println!(
                            "{} is aliased to `{}'",
                            name,
                            self.aliases.get(name).unwrap()
                        );
                    }
                }
                if !show_all {
                    continue;
                }
            }

            // Check for function (skip if -p)
            if !path_only && self.functions.contains_key(name) {
                found_any = true;
                if !silent {
                    if show_type {
                        println!("function");
                    } else {
                        println!("{} is a shell function", name);
                    }
                }
                if !show_all {
                    continue;
                }
            }

            // Check for builtin (skip if -p)
            if !path_only && (self.is_builtin(name) || name == ":" || name == "[") {
                found_any = true;
                if !silent {
                    if show_type {
                        println!("builtin");
                    } else {
                        println!("{} is a shell builtin", name);
                    }
                }
                if !show_all {
                    continue;
                }
            }

            // Check for external command in PATH
            if let Ok(path_env) = std::env::var("PATH") {
                for dir in path_env.split(':') {
                    let full_path = format!("{}/{}", dir, name);
                    if std::path::Path::new(&full_path).exists() {
                        found_any = true;
                        if !silent {
                            if show_type {
                                println!("file");
                            } else {
                                println!("{} is {}", name, full_path);
                            }
                        }
                        if !show_all {
                            break;
                        }
                    }
                }
            }

            if !found_any {
                if !silent {
                    eprintln!("zshrs: type: {}: not found", name);
                }
                status = 1;
            }
        }
        status
    }

    fn builtin_hash(&mut self, args: &[String]) -> i32 {
        // Simplified hash - just report if command exists
        if args.is_empty() {
            return 0;
        }

        for name in args {
            if let Ok(output) = std::process::Command::new("which").arg(name).output() {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout);
                    println!("{}={}", name, path.trim());
                } else {
                    eprintln!("zshrs: hash: {}: not found", name);
                    return 1;
                }
            }
        }
        0
    }

    fn builtin_command(&mut self, args: &[String], redirects: &[Redirect]) -> i32 {
        // Run command, bypassing functions and aliases
        if args.is_empty() {
            return 0;
        }

        let cmd = &args[0];
        let cmd_args = &args[1..];

        // Execute as external command
        self.execute_external(cmd, cmd_args, redirects)
            .unwrap_or(127)
    }

    fn builtin_builtin(&mut self, args: &[String], redirects: &[Redirect]) -> i32 {
        // Run builtin, bypassing functions and aliases
        if args.is_empty() {
            return 0;
        }

        let cmd = &args[0];
        let cmd_args = &args[1..];

        match cmd.as_str() {
            "cd" => self.builtin_cd(cmd_args),
            "pwd" => self.builtin_pwd(redirects),
            "echo" => self.builtin_echo(cmd_args, redirects),
            "export" => self.builtin_export(cmd_args),
            "unset" => self.builtin_unset(cmd_args),
            "exit" => self.builtin_exit(cmd_args),
            "return" => self.builtin_return(cmd_args),
            "true" => 0,
            "false" => 1,
            ":" => 0,
            "test" | "[" => self.builtin_test(cmd_args),
            "local" => self.builtin_local(cmd_args),
            "declare" | "typeset" => self.builtin_declare(cmd_args),
            "read" => self.builtin_read(cmd_args),
            "shift" => self.builtin_shift(cmd_args),
            "eval" => self.builtin_eval(cmd_args),
            "alias" => self.builtin_alias(cmd_args),
            "unalias" => self.builtin_unalias(cmd_args),
            "set" => self.builtin_set(cmd_args),
            "shopt" => self.builtin_shopt(cmd_args),
            "getopts" => self.builtin_getopts(cmd_args),
            "type" => self.builtin_type(cmd_args),
            "hash" => self.builtin_hash(cmd_args),
            "autoload" => self.builtin_autoload(cmd_args),
            "source" | "." => self.builtin_source(cmd_args),
            "functions" => self.builtin_functions(cmd_args),
            "zle" => self.builtin_zle(cmd_args),
            "bindkey" => self.builtin_bindkey(cmd_args),
            "setopt" => self.builtin_setopt(cmd_args),
            "unsetopt" => self.builtin_unsetopt(cmd_args),
            "emulate" => self.builtin_emulate(cmd_args),
            "zstyle" => self.builtin_zstyle(cmd_args),
            "compadd" => self.builtin_compadd(cmd_args),
            "compset" => self.builtin_compset(cmd_args),
            _ => {
                eprintln!("zshrs: builtin: {}: not a shell builtin", cmd);
                1
            }
        }
    }

    fn builtin_let(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            return 1;
        }

        let mut result = 0i64;
        for expr in args {
            result = self.evaluate_arithmetic_expr(expr);
        }

        // let returns 1 if last expression evaluates to 0, 0 otherwise
        if result == 0 {
            1
        } else {
            0
        }
    }

    /// Generate completion candidates
    fn builtin_compgen(&self, args: &[String]) -> i32 {
        let mut i = 0;
        let mut prefix = String::new();
        let mut actions = Vec::new();
        let mut wordlist = None;
        let mut globpat = None;

        while i < args.len() {
            match args[i].as_str() {
                "-W" => {
                    i += 1;
                    if i < args.len() {
                        wordlist = Some(args[i].clone());
                    }
                }
                "-G" => {
                    i += 1;
                    if i < args.len() {
                        globpat = Some(args[i].clone());
                    }
                }
                "-a" => actions.push("alias"),
                "-b" => actions.push("builtin"),
                "-c" => actions.push("command"),
                "-d" => actions.push("directory"),
                "-e" => actions.push("export"),
                "-f" => actions.push("file"),
                "-j" => actions.push("job"),
                "-k" => actions.push("keyword"),
                "-u" => actions.push("user"),
                "-v" => actions.push("variable"),
                s if !s.starts_with('-') => prefix = s.to_string(),
                _ => {}
            }
            i += 1;
        }

        let mut results = Vec::new();

        // Generate based on actions
        for action in actions {
            match action {
                "alias" => {
                    for name in self.aliases.keys() {
                        if name.starts_with(&prefix) {
                            results.push(name.clone());
                        }
                    }
                }
                "builtin" => {
                    for name in [
                        "cd", "pwd", "echo", "export", "unset", "source", "exit", "return", "true",
                        "false", ":", "test", "[", "local", "declare", "jobs", "fg", "bg", "kill",
                        "disown", "wait", "alias", "unalias", "set", "shopt",
                    ] {
                        if name.starts_with(&prefix) {
                            results.push(name.to_string());
                        }
                    }
                }
                "directory" => {
                    if let Ok(entries) = std::fs::read_dir(".") {
                        for entry in entries.flatten() {
                            if let Ok(ft) = entry.file_type() {
                                if ft.is_dir() {
                                    let name = entry.file_name().to_string_lossy().to_string();
                                    if name.starts_with(&prefix) {
                                        results.push(name);
                                    }
                                }
                            }
                        }
                    }
                }
                "file" => {
                    if let Ok(entries) = std::fs::read_dir(".") {
                        for entry in entries.flatten() {
                            let name = entry.file_name().to_string_lossy().to_string();
                            if name.starts_with(&prefix) {
                                results.push(name);
                            }
                        }
                    }
                }
                "variable" => {
                    for name in self.variables.keys() {
                        if name.starts_with(&prefix) {
                            results.push(name.clone());
                        }
                    }
                    for name in std::env::vars().map(|(k, _)| k) {
                        if name.starts_with(&prefix) && !results.contains(&name) {
                            results.push(name);
                        }
                    }
                }
                _ => {}
            }
        }

        // Handle wordlist
        if let Some(words) = wordlist {
            for word in words.split_whitespace() {
                if word.starts_with(&prefix) {
                    results.push(word.to_string());
                }
            }
        }

        // Handle glob pattern
        if let Some(pattern) = globpat {
            let full_pattern = format!("{}*", prefix);
            if let Ok(paths) = glob::glob(&full_pattern) {
                for path in paths.flatten() {
                    results.push(path.to_string_lossy().to_string());
                }
            }
        }

        results.sort();
        results.dedup();
        for r in results {
            println!("{}", r);
        }
        0
    }

    /// Define completion spec for a command
    fn builtin_complete(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            // List all completion specs
            for (cmd, spec) in &self.completions {
                let mut parts = vec!["complete".to_string()];
                for action in &spec.actions {
                    parts.push(format!("-{}", action));
                }
                if let Some(ref w) = spec.wordlist {
                    parts.push("-W".to_string());
                    parts.push(format!("'{}'", w));
                }
                if let Some(ref f) = spec.function {
                    parts.push("-F".to_string());
                    parts.push(f.clone());
                }
                if let Some(ref c) = spec.command {
                    parts.push("-C".to_string());
                    parts.push(c.clone());
                }
                parts.push(cmd.clone());
                println!("{}", parts.join(" "));
            }
            return 0;
        }

        let mut spec = CompSpec::default();
        let mut commands = Vec::new();
        let mut i = 0;

        while i < args.len() {
            match args[i].as_str() {
                "-W" => {
                    i += 1;
                    if i < args.len() {
                        spec.wordlist = Some(args[i].clone());
                    }
                }
                "-F" => {
                    i += 1;
                    if i < args.len() {
                        spec.function = Some(args[i].clone());
                    }
                }
                "-C" => {
                    i += 1;
                    if i < args.len() {
                        spec.command = Some(args[i].clone());
                    }
                }
                "-G" => {
                    i += 1;
                    if i < args.len() {
                        spec.globpat = Some(args[i].clone());
                    }
                }
                "-P" => {
                    i += 1;
                    if i < args.len() {
                        spec.prefix = Some(args[i].clone());
                    }
                }
                "-S" => {
                    i += 1;
                    if i < args.len() {
                        spec.suffix = Some(args[i].clone());
                    }
                }
                "-a" => spec.actions.push("a".to_string()),
                "-b" => spec.actions.push("b".to_string()),
                "-c" => spec.actions.push("c".to_string()),
                "-d" => spec.actions.push("d".to_string()),
                "-e" => spec.actions.push("e".to_string()),
                "-f" => spec.actions.push("f".to_string()),
                "-j" => spec.actions.push("j".to_string()),
                "-r" => {
                    // Remove completion spec
                    i += 1;
                    while i < args.len() {
                        self.completions.remove(&args[i]);
                        i += 1;
                    }
                    return 0;
                }
                s if !s.starts_with('-') => commands.push(s.to_string()),
                _ => {}
            }
            i += 1;
        }

        for cmd in commands {
            self.completions.insert(cmd, spec.clone());
        }
        0
    }

    /// Modify completion options
    fn builtin_compopt(&mut self, args: &[String]) -> i32 {
        // Basic stub - just accept the options
        let _ = args;
        0
    }

    /// zsh compadd - add completion matches
    fn builtin_compadd(&mut self, args: &[String]) -> i32 {
        // Basic stub for zsh completion system
        // In a full implementation, this would add completion candidates
        let _ = args;
        0
    }

    /// zsh compset - modify completion prefix/suffix
    fn builtin_compset(&mut self, args: &[String]) -> i32 {
        // Basic stub for zsh completion system
        let _ = args;
        0
    }

    /// zsh zstyle - configure styles for completion
    fn builtin_zstyle(&mut self, args: &[String]) -> i32 {
        // Parse zstyle commands: zstyle [pattern] [style] [values...]
        if args.is_empty() {
            // List all styles
            for style in &self.zstyles {
                println!(
                    "zstyle '{}' {} {}",
                    style.pattern,
                    style.style,
                    style.values.join(" ")
                );
            }
            return 0;
        }

        if args.len() >= 2 {
            let pattern = args[0].clone();
            let style = args[1].clone();
            let values: Vec<String> = args[2..].to_vec();

            // Check for existing style and update or add
            let existing = self
                .zstyles
                .iter_mut()
                .find(|s| s.pattern == pattern && s.style == style);
            if let Some(s) = existing {
                s.values = values;
            } else {
                self.zstyles.push(ZStyle {
                    pattern,
                    style,
                    values,
                });
            }
        }
        0
    }

    /// Push directory onto stack and cd to it
    fn builtin_pushd(&mut self, args: &[String]) -> i32 {
        let current = match std::env::current_dir() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("pushd: {}", e);
                return 1;
            }
        };

        if args.is_empty() {
            // Swap top two directories
            if self.dir_stack.is_empty() {
                eprintln!("pushd: no other directory");
                return 1;
            }
            let target = self.dir_stack.pop().unwrap();
            self.dir_stack.push(current.clone());
            if let Err(e) = std::env::set_current_dir(&target) {
                eprintln!("pushd: {}: {}", target.display(), e);
                self.dir_stack.pop();
                self.dir_stack.push(target);
                return 1;
            }
            self.print_dir_stack();
            return 0;
        }

        let arg = &args[0];

        // Handle +N and -N for rotating the stack
        if arg.starts_with('+') || arg.starts_with('-') {
            if let Ok(n) = arg[1..].parse::<usize>() {
                let total = self.dir_stack.len() + 1;
                if n >= total {
                    eprintln!("pushd: {}: directory stack index out of range", arg);
                    return 1;
                }
                // Rotate stack
                let rotate_pos = if arg.starts_with('+') { n } else { total - n };
                let mut full_stack = vec![current.clone()];
                full_stack.extend(self.dir_stack.iter().cloned());
                full_stack.rotate_left(rotate_pos);

                let target = full_stack.remove(0);
                self.dir_stack = full_stack;

                if let Err(e) = std::env::set_current_dir(&target) {
                    eprintln!("pushd: {}: {}", target.display(), e);
                    return 1;
                }
                self.print_dir_stack();
                return 0;
            }
        }

        // Regular directory push
        let target = PathBuf::from(arg);
        self.dir_stack.push(current);
        if let Err(e) = std::env::set_current_dir(&target) {
            eprintln!("pushd: {}: {}", arg, e);
            self.dir_stack.pop();
            return 1;
        }
        self.print_dir_stack();
        0
    }

    /// Pop directory from stack and cd to it
    fn builtin_popd(&mut self, args: &[String]) -> i32 {
        if self.dir_stack.is_empty() {
            eprintln!("popd: directory stack empty");
            return 1;
        }

        // Handle +N and -N
        if !args.is_empty() {
            let arg = &args[0];
            if arg.starts_with('+') || arg.starts_with('-') {
                if let Ok(n) = arg[1..].parse::<usize>() {
                    let total = self.dir_stack.len() + 1;
                    if n >= total {
                        eprintln!("popd: {}: directory stack index out of range", arg);
                        return 1;
                    }
                    let remove_pos = if arg.starts_with('+') {
                        n
                    } else {
                        total - 1 - n
                    };
                    if remove_pos == 0 {
                        // Remove current and cd to next
                        let target = self.dir_stack.remove(0);
                        if let Err(e) = std::env::set_current_dir(&target) {
                            eprintln!("popd: {}: {}", target.display(), e);
                            return 1;
                        }
                    } else {
                        self.dir_stack.remove(remove_pos - 1);
                    }
                    self.print_dir_stack();
                    return 0;
                }
            }
        }

        let target = self.dir_stack.pop().unwrap();
        if let Err(e) = std::env::set_current_dir(&target) {
            eprintln!("popd: {}: {}", target.display(), e);
            self.dir_stack.push(target);
            return 1;
        }
        self.print_dir_stack();
        0
    }

    /// Display directory stack
    fn builtin_dirs(&self, args: &[String]) -> i32 {
        let mut clear = false;
        let mut verbose = false;
        let mut per_line = false;

        for arg in args {
            match arg.as_str() {
                "-c" => clear = true,
                "-v" => verbose = true,
                "-p" => per_line = true,
                "-l" => {} // Don't expand ~ (we don't expand it anyway)
                _ => {}
            }
        }

        if clear {
            // Can't clear from &self, would need &mut self
            // For now just print empty
            return 0;
        }

        let current = std::env::current_dir().unwrap_or_default();

        if verbose || per_line {
            println!(" 0  {}", current.display());
            for (i, dir) in self.dir_stack.iter().rev().enumerate() {
                println!("{:2}  {}", i + 1, dir.display());
            }
        } else {
            let mut parts = vec![current.to_string_lossy().to_string()];
            for dir in self.dir_stack.iter().rev() {
                parts.push(dir.to_string_lossy().to_string());
            }
            println!("{}", parts.join(" "));
        }
        0
    }

    fn print_dir_stack(&self) {
        let current = std::env::current_dir().unwrap_or_default();
        let mut parts = vec![current.to_string_lossy().to_string()];
        for dir in self.dir_stack.iter().rev() {
            parts.push(dir.to_string_lossy().to_string());
        }
        println!("{}", parts.join(" "));
    }

    /// printf builtin - format and print data (zsh/bash compatible)
    fn builtin_printf(&self, args: &[String]) -> i32 {
        if args.is_empty() {
            eprintln!("printf: usage: printf format [arguments]");
            return 1;
        }

        let format = &args[0];
        let format_args = &args[1..];
        let mut arg_idx = 0;
        let mut output = String::new();
        let mut chars = format.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => output.push('\n'),
                    Some('t') => output.push('\t'),
                    Some('r') => output.push('\r'),
                    Some('\\') => output.push('\\'),
                    Some('a') => output.push('\x07'),
                    Some('b') => output.push('\x08'),
                    Some('e') | Some('E') => output.push('\x1b'),
                    Some('f') => output.push('\x0c'),
                    Some('v') => output.push('\x0b'),
                    Some('"') => output.push('"'),
                    Some('\'') => output.push('\''),
                    Some('0') => {
                        let mut octal = String::new();
                        while octal.len() < 3 {
                            if let Some(&d) = chars.peek() {
                                if d >= '0' && d <= '7' {
                                    octal.push(d);
                                    chars.next();
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                        if octal.is_empty() {
                            output.push('\0');
                        } else if let Ok(val) = u8::from_str_radix(&octal, 8) {
                            output.push(val as char);
                        }
                    }
                    Some('x') => {
                        let mut hex = String::new();
                        while hex.len() < 2 {
                            if let Some(&d) = chars.peek() {
                                if d.is_ascii_hexdigit() {
                                    hex.push(d);
                                    chars.next();
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                        if !hex.is_empty() {
                            if let Ok(val) = u8::from_str_radix(&hex, 16) {
                                output.push(val as char);
                            }
                        }
                    }
                    Some('u') => {
                        let mut hex = String::new();
                        while hex.len() < 4 {
                            if let Some(&d) = chars.peek() {
                                if d.is_ascii_hexdigit() {
                                    hex.push(d);
                                    chars.next();
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                        if !hex.is_empty() {
                            if let Ok(val) = u32::from_str_radix(&hex, 16) {
                                if let Some(c) = char::from_u32(val) {
                                    output.push(c);
                                }
                            }
                        }
                    }
                    Some('U') => {
                        let mut hex = String::new();
                        while hex.len() < 8 {
                            if let Some(&d) = chars.peek() {
                                if d.is_ascii_hexdigit() {
                                    hex.push(d);
                                    chars.next();
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                        if !hex.is_empty() {
                            if let Ok(val) = u32::from_str_radix(&hex, 16) {
                                if let Some(c) = char::from_u32(val) {
                                    output.push(c);
                                }
                            }
                        }
                    }
                    Some('c') => {
                        print!("{}", output);
                        return 0;
                    }
                    Some(other) => {
                        output.push('\\');
                        output.push(other);
                    }
                    None => output.push('\\'),
                }
            } else if c == '%' {
                if chars.peek() == Some(&'%') {
                    chars.next();
                    output.push('%');
                    continue;
                }

                let mut flags = String::new();
                while let Some(&f) = chars.peek() {
                    if f == '-' || f == '+' || f == ' ' || f == '#' || f == '0' {
                        flags.push(f);
                        chars.next();
                    } else {
                        break;
                    }
                }

                let mut width = String::new();
                if chars.peek() == Some(&'*') {
                    chars.next();
                    if arg_idx < format_args.len() {
                        width = format_args[arg_idx].clone();
                        arg_idx += 1;
                    }
                } else {
                    while let Some(&d) = chars.peek() {
                        if d.is_ascii_digit() {
                            width.push(d);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }

                let mut precision = String::new();
                if chars.peek() == Some(&'.') {
                    chars.next();
                    if chars.peek() == Some(&'*') {
                        chars.next();
                        if arg_idx < format_args.len() {
                            precision = format_args[arg_idx].clone();
                            arg_idx += 1;
                        }
                    } else {
                        while let Some(&d) = chars.peek() {
                            if d.is_ascii_digit() {
                                precision.push(d);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                    }
                }

                let specifier = chars.next().unwrap_or('s');
                let arg = if arg_idx < format_args.len() {
                    let a = &format_args[arg_idx];
                    arg_idx += 1;
                    a.clone()
                } else {
                    String::new()
                };

                let width_val: usize = width.parse().unwrap_or(0);
                let prec_val: Option<usize> = if precision.is_empty() {
                    None
                } else {
                    precision.parse().ok()
                };
                let left_align = flags.contains('-');
                let zero_pad = flags.contains('0') && !left_align;
                let plus_sign = flags.contains('+');
                let space_sign = flags.contains(' ') && !plus_sign;
                let alt_form = flags.contains('#');

                match specifier {
                    's' => {
                        let mut s = arg;
                        if let Some(p) = prec_val {
                            s = s.chars().take(p).collect();
                        }
                        if width_val > s.len() {
                            if left_align {
                                output.push_str(&s);
                                output.push_str(&" ".repeat(width_val - s.len()));
                            } else {
                                output.push_str(&" ".repeat(width_val - s.len()));
                                output.push_str(&s);
                            }
                        } else {
                            output.push_str(&s);
                        }
                    }
                    'b' => {
                        let expanded = self.expand_printf_escapes(&arg);
                        if let Some(p) = prec_val {
                            let s: String = expanded.chars().take(p).collect();
                            output.push_str(&s);
                        } else {
                            output.push_str(&expanded);
                        }
                    }
                    'c' => {
                        if let Some(ch) = arg.chars().next() {
                            output.push(ch);
                        }
                    }
                    'q' => {
                        output.push('\'');
                        for ch in arg.chars() {
                            if ch == '\'' {
                                output.push_str("'\\''");
                            } else {
                                output.push(ch);
                            }
                        }
                        output.push('\'');
                    }
                    'd' | 'i' => {
                        let val: i64 = if arg.starts_with("0x") || arg.starts_with("0X") {
                            i64::from_str_radix(&arg[2..], 16).unwrap_or(0)
                        } else if arg.starts_with("0") && arg.len() > 1 && !arg.contains('.') {
                            i64::from_str_radix(&arg[1..], 8).unwrap_or(0)
                        } else if arg.starts_with('\'') || arg.starts_with('"') {
                            arg.chars().nth(1).map(|c| c as i64).unwrap_or(0)
                        } else {
                            arg.parse().unwrap_or(0)
                        };

                        let sign = if val < 0 {
                            "-"
                        } else if plus_sign {
                            "+"
                        } else if space_sign {
                            " "
                        } else {
                            ""
                        };
                        let abs_val = val.abs();
                        let num_str = abs_val.to_string();
                        let total_len = sign.len() + num_str.len();

                        if width_val > total_len {
                            if left_align {
                                output.push_str(sign);
                                output.push_str(&num_str);
                                output.push_str(&" ".repeat(width_val - total_len));
                            } else if zero_pad {
                                output.push_str(sign);
                                output.push_str(&"0".repeat(width_val - total_len));
                                output.push_str(&num_str);
                            } else {
                                output.push_str(&" ".repeat(width_val - total_len));
                                output.push_str(sign);
                                output.push_str(&num_str);
                            }
                        } else {
                            output.push_str(sign);
                            output.push_str(&num_str);
                        }
                    }
                    'u' => {
                        let val: u64 = if arg.starts_with("0x") || arg.starts_with("0X") {
                            u64::from_str_radix(&arg[2..], 16).unwrap_or(0)
                        } else if arg.starts_with("0") && arg.len() > 1 {
                            u64::from_str_radix(&arg[1..], 8).unwrap_or(0)
                        } else {
                            arg.parse().unwrap_or(0)
                        };
                        let num_str = val.to_string();
                        if width_val > num_str.len() {
                            if left_align {
                                output.push_str(&num_str);
                                output.push_str(&" ".repeat(width_val - num_str.len()));
                            } else if zero_pad {
                                output.push_str(&"0".repeat(width_val - num_str.len()));
                                output.push_str(&num_str);
                            } else {
                                output.push_str(&" ".repeat(width_val - num_str.len()));
                                output.push_str(&num_str);
                            }
                        } else {
                            output.push_str(&num_str);
                        }
                    }
                    'o' => {
                        let val: u64 = arg.parse().unwrap_or(0);
                        let num_str = format!("{:o}", val);
                        let prefix = if alt_form && val != 0 { "0" } else { "" };
                        let total_len = prefix.len() + num_str.len();
                        if width_val > total_len {
                            if left_align {
                                output.push_str(prefix);
                                output.push_str(&num_str);
                                output.push_str(&" ".repeat(width_val - total_len));
                            } else {
                                output.push_str(&" ".repeat(width_val - total_len));
                                output.push_str(prefix);
                                output.push_str(&num_str);
                            }
                        } else {
                            output.push_str(prefix);
                            output.push_str(&num_str);
                        }
                    }
                    'x' => {
                        let val: u64 = arg.parse().unwrap_or(0);
                        let num_str = format!("{:x}", val);
                        let prefix = if alt_form && val != 0 { "0x" } else { "" };
                        let total_len = prefix.len() + num_str.len();
                        if width_val > total_len {
                            if left_align {
                                output.push_str(prefix);
                                output.push_str(&num_str);
                                output.push_str(&" ".repeat(width_val - total_len));
                            } else {
                                output.push_str(&" ".repeat(width_val - total_len));
                                output.push_str(prefix);
                                output.push_str(&num_str);
                            }
                        } else {
                            output.push_str(prefix);
                            output.push_str(&num_str);
                        }
                    }
                    'X' => {
                        let val: u64 = arg.parse().unwrap_or(0);
                        let num_str = format!("{:X}", val);
                        let prefix = if alt_form && val != 0 { "0X" } else { "" };
                        let total_len = prefix.len() + num_str.len();
                        if width_val > total_len {
                            if left_align {
                                output.push_str(prefix);
                                output.push_str(&num_str);
                                output.push_str(&" ".repeat(width_val - total_len));
                            } else {
                                output.push_str(&" ".repeat(width_val - total_len));
                                output.push_str(prefix);
                                output.push_str(&num_str);
                            }
                        } else {
                            output.push_str(prefix);
                            output.push_str(&num_str);
                        }
                    }
                    'e' | 'E' => {
                        let val: f64 = arg.parse().unwrap_or(0.0);
                        let prec = prec_val.unwrap_or(6);
                        let formatted = if specifier == 'e' {
                            format!("{:.prec$e}", val, prec = prec)
                        } else {
                            format!("{:.prec$E}", val, prec = prec)
                        };
                        if width_val > formatted.len() {
                            if left_align {
                                output.push_str(&formatted);
                                output.push_str(&" ".repeat(width_val - formatted.len()));
                            } else {
                                output.push_str(&" ".repeat(width_val - formatted.len()));
                                output.push_str(&formatted);
                            }
                        } else {
                            output.push_str(&formatted);
                        }
                    }
                    'f' | 'F' => {
                        let val: f64 = arg.parse().unwrap_or(0.0);
                        let prec = prec_val.unwrap_or(6);
                        let sign = if val < 0.0 {
                            "-"
                        } else if plus_sign {
                            "+"
                        } else if space_sign {
                            " "
                        } else {
                            ""
                        };
                        let formatted = format!("{:.prec$}", val.abs(), prec = prec);
                        let total = sign.len() + formatted.len();
                        if width_val > total {
                            if left_align {
                                output.push_str(sign);
                                output.push_str(&formatted);
                                output.push_str(&" ".repeat(width_val - total));
                            } else if zero_pad {
                                output.push_str(sign);
                                output.push_str(&"0".repeat(width_val - total));
                                output.push_str(&formatted);
                            } else {
                                output.push_str(&" ".repeat(width_val - total));
                                output.push_str(sign);
                                output.push_str(&formatted);
                            }
                        } else {
                            output.push_str(sign);
                            output.push_str(&formatted);
                        }
                    }
                    'g' | 'G' => {
                        let val: f64 = arg.parse().unwrap_or(0.0);
                        let prec = prec_val.unwrap_or(6).max(1);
                        let formatted = if specifier == 'g' {
                            format!("{:.prec$}", val, prec = prec)
                        } else {
                            format!("{:.prec$}", val, prec = prec).to_uppercase()
                        };
                        output.push_str(&formatted);
                    }
                    'a' | 'A' => {
                        let val: f64 = arg.parse().unwrap_or(0.0);
                        let formatted = float_to_hex(val, specifier == 'A');
                        output.push_str(&formatted);
                    }
                    _ => {
                        output.push('%');
                        output.push(specifier);
                    }
                }
            } else {
                output.push(c);
            }
        }

        print!("{}", output);
        0
    }

    fn expand_printf_escapes(&self, s: &str) -> String {
        let mut result = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => result.push('\n'),
                    Some('t') => result.push('\t'),
                    Some('r') => result.push('\r'),
                    Some('\\') => result.push('\\'),
                    Some('a') => result.push('\x07'),
                    Some('b') => result.push('\x08'),
                    Some('e') | Some('E') => result.push('\x1b'),
                    Some('f') => result.push('\x0c'),
                    Some('v') => result.push('\x0b'),
                    Some('0') => {
                        let mut octal = String::new();
                        while octal.len() < 3 {
                            if let Some(&d) = chars.peek() {
                                if d >= '0' && d <= '7' {
                                    octal.push(d);
                                    chars.next();
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                        if octal.is_empty() {
                            result.push('\0');
                        } else if let Ok(val) = u8::from_str_radix(&octal, 8) {
                            result.push(val as char);
                        }
                    }
                    Some('c') => break,
                    Some(other) => {
                        result.push('\\');
                        result.push(other);
                    }
                    None => result.push('\\'),
                }
            } else {
                result.push(c);
            }
        }
        result
    }

    fn evaluate_arithmetic_expr(&mut self, expr: &str) -> i64 {
        // Handle simple assignment like x=5
        // But not if it's a comparison like == or !=
        if let Some(eq_pos) = expr.find('=') {
            // Check it's not part of ==, !=, <=, >=
            let chars: Vec<char> = expr.chars().collect();
            let is_comparison = (eq_pos > 0
                && (chars[eq_pos - 1] == '!'
                    || chars[eq_pos - 1] == '<'
                    || chars[eq_pos - 1] == '>'
                    || chars[eq_pos - 1] == '='))
                || chars.get(eq_pos + 1) == Some(&'=');

            if !is_comparison && !expr[..eq_pos].contains(|c| "+-*/%<>!&|".contains(c)) {
                let var_name = expr[..eq_pos].trim();
                if !var_name.is_empty() {
                    let value_expr = &expr[eq_pos + 1..];
                    let value = self.evaluate_arithmetic_expr(value_expr);
                    self.variables
                        .insert(var_name.to_string(), value.to_string());
                    env::set_var(var_name, value.to_string());
                    return value;
                }
            }
        }

        // Use the existing arithmetic evaluator
        let result_str = self.evaluate_arithmetic(expr);
        result_str.parse().unwrap_or(0)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Additional zsh builtins
    // ═══════════════════════════════════════════════════════════════════════════

    /// break - exit from for/while/until loop
    fn builtin_break(&self, args: &[String]) -> i32 {
        let _levels: i32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(1);
        // Note: actual break handling is done in the loop execution code
        // This just returns the level count for the executor to handle
        0
    }

    /// continue - skip to next iteration of loop
    fn builtin_continue(&self, args: &[String]) -> i32 {
        let _levels: i32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(1);
        0
    }

    /// disable - disable shell builtins, aliases, functions
    fn builtin_disable(&mut self, args: &[String]) -> i32 {
        let mut disable_aliases = false;
        let mut disable_builtins = false;
        let mut disable_functions = false;
        let mut names = Vec::new();

        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "-a" => disable_aliases = true,
                "-f" => disable_functions = true,
                "-r" => disable_builtins = true,
                _ if arg.starts_with('-') => {}
                _ => names.push(arg.clone()),
            }
        }

        // Default to builtins if no flags
        if !disable_aliases && !disable_functions {
            disable_builtins = true;
        }

        for name in names {
            if disable_aliases {
                self.aliases.remove(&name);
            }
            if disable_functions {
                self.functions.remove(&name);
            }
            if disable_builtins {
                // Store disabled builtins
                self.options.insert(format!("_disabled_{}", name), true);
            }
        }
        0
    }

    /// enable - enable disabled shell builtins
    fn builtin_enable(&mut self, args: &[String]) -> i32 {
        for arg in args {
            if !arg.starts_with('-') {
                self.options.remove(&format!("_disabled_{}", arg));
            }
        }
        0
    }

    /// emulate - set up zsh emulation mode
    fn builtin_emulate(&mut self, args: &[String]) -> i32 {
        let mode = args.first().map(|s| s.as_str()).unwrap_or("zsh");
        match mode {
            "zsh" => {
                self.options.insert("emulate".to_string(), true);
                self.variables
                    .insert("EMULATE".to_string(), "zsh".to_string());
            }
            "sh" | "ksh" | "csh" => {
                self.options.insert("emulate".to_string(), true);
                self.variables
                    .insert("EMULATE".to_string(), mode.to_string());
            }
            _ => {
                eprintln!("emulate: unknown mode: {}", mode);
                return 1;
            }
        }
        0
    }

    /// exec - replace the shell with a command
    fn builtin_exec(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            return 0;
        }

        let cmd = &args[0];
        let cmd_args: Vec<&str> = args[1..].iter().map(|s| s.as_str()).collect();

        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(cmd).args(&cmd_args).exec();
        eprintln!("exec: {}: {}", cmd, err);
        1
    }

    /// float - declare floating point variables
    fn builtin_float(&mut self, args: &[String]) -> i32 {
        for arg in args {
            if arg.starts_with('-') {
                continue;
            }
            if let Some(eq_pos) = arg.find('=') {
                let name = &arg[..eq_pos];
                let value = &arg[eq_pos + 1..];
                let float_val: f64 = value.parse().unwrap_or(0.0);
                self.variables
                    .insert(name.to_string(), float_val.to_string());
                self.options.insert(format!("_float_{}", name), true);
            } else {
                self.variables.insert(arg.clone(), "0.0".to_string());
                self.options.insert(format!("_float_{}", arg), true);
            }
        }
        0
    }

    /// integer - declare integer variables
    fn builtin_integer(&mut self, args: &[String]) -> i32 {
        for arg in args {
            if arg.starts_with('-') {
                continue;
            }
            if let Some(eq_pos) = arg.find('=') {
                let name = &arg[..eq_pos];
                let value = &arg[eq_pos + 1..];
                let int_val: i64 = value.parse().unwrap_or(0);
                self.variables.insert(name.to_string(), int_val.to_string());
                self.options.insert(format!("_integer_{}", name), true);
            } else {
                self.variables.insert(arg.clone(), "0".to_string());
                self.options.insert(format!("_integer_{}", arg), true);
            }
        }
        0
    }

    /// functions - list or manipulate function definitions
    fn builtin_functions(&self, args: &[String]) -> i32 {
        let mut list_only = false;
        let mut show_trace = false;
        let mut names: Vec<&str> = Vec::new();

        for arg in args {
            match arg.as_str() {
                "-l" => list_only = true,
                "-t" => show_trace = true,
                _ if arg.starts_with('-') => {}
                _ => names.push(arg),
            }
        }

        if names.is_empty() {
            // List all functions
            let mut func_names: Vec<_> = self.functions.keys().collect();
            func_names.sort();
            for name in func_names {
                if list_only {
                    println!("{}", name);
                } else {
                    println!("{} () {{ ... }}", name);
                }
            }
        } else {
            // Show specific functions
            for name in names {
                if let Some(_func) = self.functions.get(name) {
                    if show_trace {
                        println!("functions -t {}", name);
                    } else {
                        println!("{} () {{ ... }}", name);
                    }
                } else {
                    eprintln!("functions: no such function: {}", name);
                    return 1;
                }
            }
        }
        0
    }

    /// print - zsh print builtin with many options
    fn builtin_print(&self, args: &[String]) -> i32 {
        let mut no_newline = false;
        let mut interpret_escapes = false;
        let mut to_stderr = false;
        let mut columns = 0usize;
        let mut null_terminate = false;
        let mut push_to_stack = false;
        let mut output_args: Vec<&str> = Vec::new();

        let mut iter = args.iter().peekable();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "-n" => no_newline = true,
                "-r" => interpret_escapes = false, // raw
                "-e" => interpret_escapes = true,
                "-E" => interpret_escapes = false,
                "-u" => to_stderr = true,
                "-N" => null_terminate = true,
                "-z" => push_to_stack = true,
                "-C" => {
                    if let Some(n) = iter.next() {
                        columns = n.parse().unwrap_or(0);
                    }
                }
                "-c" => columns = 1,            // single column
                "-l" => {}                      // one arg per line (default with -c)
                "-a" | "-o" | "-O" | "-i" => {} // array/sort options (need array context)
                "--" => {
                    output_args.extend(iter.map(|s| s.as_str()));
                    break;
                }
                _ if arg.starts_with('-') => {}
                _ => output_args.push(arg),
            }
        }

        let _ = push_to_stack; // TODO: implement push to buffer stack

        let output = if interpret_escapes {
            output_args
                .iter()
                .map(|s| self.expand_printf_escapes(s))
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            output_args.join(" ")
        };

        let terminator = if null_terminate {
            "\0"
        } else if no_newline {
            ""
        } else {
            "\n"
        };

        if columns > 0 {
            // Column output
            let words: Vec<&str> = output.split_whitespace().collect();
            for chunk in words.chunks(columns) {
                if to_stderr {
                    eprintln!("{}", chunk.join(" "));
                } else {
                    println!("{}", chunk.join(" "));
                }
            }
        } else if to_stderr {
            eprint!("{}{}", output, terminator);
        } else {
            print!("{}{}", output, terminator);
        }
        0
    }

    /// whence - show how a command would be interpreted
    fn builtin_whence(&self, args: &[String]) -> i32 {
        let mut show_path = false;
        let mut show_all = false;
        let mut verbose = false;
        let mut names: Vec<&str> = Vec::new();

        for arg in args {
            match arg.as_str() {
                "-p" => show_path = true,
                "-a" => show_all = true,
                "-v" => verbose = true,
                "-c" | "-f" | "-m" | "-s" | "-w" => {}
                _ if arg.starts_with('-') => {}
                _ => names.push(arg),
            }
        }

        let mut status = 0;
        for name in names {
            let mut found = false;

            if !show_path {
                // Check aliases
                if let Some(alias_val) = self.aliases.get(name) {
                    found = true;
                    if verbose {
                        println!("{} is an alias for {}", name, alias_val);
                    } else {
                        println!("{}", alias_val);
                    }
                    if !show_all {
                        continue;
                    }
                }

                // Check functions
                if self.functions.contains_key(name) {
                    found = true;
                    if verbose {
                        println!("{} is a shell function", name);
                    } else {
                        println!("{}", name);
                    }
                    if !show_all {
                        continue;
                    }
                }

                // Check builtins
                if self.is_builtin(name) {
                    found = true;
                    if verbose {
                        println!("{} is a shell builtin", name);
                    } else {
                        println!("{}", name);
                    }
                    if !show_all {
                        continue;
                    }
                }
            }

            // Check PATH
            if let Some(path) = self.find_in_path(name) {
                found = true;
                if verbose {
                    println!("{} is {}", name, path);
                } else {
                    println!("{}", path);
                }
            }

            if !found {
                if verbose {
                    println!("{} not found", name);
                }
                status = 1;
            }
        }
        status
    }

    /// where - show all locations of a command
    fn builtin_where(&self, args: &[String]) -> i32 {
        // where is like whence -ca
        let mut new_args = vec!["-a".to_string()];
        new_args.extend(args.iter().cloned());
        self.builtin_whence(&new_args)
    }

    /// which - show path of command
    fn builtin_which(&self, args: &[String]) -> i32 {
        // which is like whence -c
        self.builtin_whence(args)
    }

    /// Helper to check if name is a builtin
    fn is_builtin(&self, name: &str) -> bool {
        matches!(
            name,
            "cd" | "chdir"
                | "pwd"
                | "echo"
                | "export"
                | "unset"
                | "source"
                | "exit"
                | "return"
                | "bye"
                | "logout"
                | "log"
                | "true"
                | "false"
                | "test"
                | "local"
                | "declare"
                | "typeset"
                | "read"
                | "shift"
                | "eval"
                | "jobs"
                | "fg"
                | "bg"
                | "kill"
                | "disown"
                | "wait"
                | "autoload"
                | "history"
                | "fc"
                | "trap"
                | "suspend"
                | "alias"
                | "unalias"
                | "set"
                | "shopt"
                | "setopt"
                | "unsetopt"
                | "getopts"
                | "type"
                | "hash"
                | "command"
                | "builtin"
                | "let"
                | "pushd"
                | "popd"
                | "dirs"
                | "printf"
                | "break"
                | "continue"
                | "disable"
                | "enable"
                | "emulate"
                | "exec"
                | "float"
                | "integer"
                | "functions"
                | "print"
                | "whence"
                | "where"
                | "which"
                | "ulimit"
                | "limit"
                | "unlimit"
                | "umask"
                | "rehash"
                | "unhash"
                | "times"
                | "zmodload"
                | "r"
                | "ttyctl"
                | "noglob"
                | "zstat"
                | "stat"
                | "strftime"
                | "zsleep"
                | "zln"
                | "zmv"
                | "zcp"
                | "coproc"
                | "zparseopts"
                | "readonly"
                | "unfunction"
                | "getln"
                | "pushln"
                | "bindkey"
                | "zle"
                | "sched"
                | "zformat"
                | "zcompile"
                | "vared"
                | "echotc"
                | "echoti"
                | "zpty"
                | "zprof"
                | "zsocket"
                | "ztcp"
                | "zregexparse"
                | "clone"
                | "comparguments"
                | "compcall"
                | "compctl"
                | "compdescribe"
                | "compfiles"
                | "compgroups"
                | "compquote"
                | "comptags"
                | "comptry"
                | "compvalues"
                | "cap"
                | "getcap"
                | "setcap"
                | "zftp"
                | "zcurses"
                | "sysread"
                | "syswrite"
                | "syserror"
                | "sysopen"
                | "sysseek"
                | "private"
                | "zgetattr"
                | "zsetattr"
                | "zdelattr"
                | "zlistattr"
        ) || name.starts_with('_')
    }

    /// Helper to find command in PATH
    fn find_in_path(&self, name: &str) -> Option<String> {
        let path_var = env::var("PATH").unwrap_or_default();
        for dir in path_var.split(':') {
            let full_path = format!("{}/{}", dir, name);
            if std::path::Path::new(&full_path).exists() {
                return Some(full_path);
            }
        }
        None
    }

    /// ulimit - get/set resource limits
    fn builtin_ulimit(&self, args: &[String]) -> i32 {
        use libc::{getrlimit, rlimit, setrlimit};
        use libc::{RLIMIT_AS, RLIMIT_CORE, RLIMIT_CPU, RLIMIT_DATA, RLIMIT_FSIZE};
        use libc::{RLIMIT_NOFILE, RLIMIT_NPROC, RLIMIT_RSS, RLIMIT_STACK};

        let mut resource = RLIMIT_FSIZE; // default: file size
        let mut hard = false;
        let mut soft = true;
        let mut value: Option<u64> = None;

        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "-H" => {
                    hard = true;
                    soft = false;
                }
                "-S" => {
                    soft = true;
                    hard = false;
                }
                "-a" => {
                    // Print all limits
                    self.print_all_limits(soft);
                    return 0;
                }
                "-c" => resource = RLIMIT_CORE,
                "-d" => resource = RLIMIT_DATA,
                "-f" => resource = RLIMIT_FSIZE,
                "-n" => resource = RLIMIT_NOFILE,
                "-s" => resource = RLIMIT_STACK,
                "-t" => resource = RLIMIT_CPU,
                "-u" => resource = RLIMIT_NPROC,
                "-v" => resource = RLIMIT_AS,
                "-m" => resource = RLIMIT_RSS,
                "unlimited" => value = Some(libc::RLIM_INFINITY as u64),
                _ if !arg.starts_with('-') => {
                    value = arg.parse().ok();
                }
                _ => {}
            }
        }

        let mut rlim = rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        unsafe {
            if getrlimit(resource, &mut rlim) != 0 {
                eprintln!("ulimit: cannot get limit");
                return 1;
            }
        }

        if let Some(v) = value {
            // Set limit
            if soft {
                rlim.rlim_cur = v as libc::rlim_t;
            }
            if hard {
                rlim.rlim_max = v as libc::rlim_t;
            }
            unsafe {
                if setrlimit(resource, &rlim) != 0 {
                    eprintln!("ulimit: cannot set limit");
                    return 1;
                }
            }
        } else {
            // Print limit
            let limit = if hard { rlim.rlim_max } else { rlim.rlim_cur };
            if limit == libc::RLIM_INFINITY as libc::rlim_t {
                println!("unlimited");
            } else {
                println!("{}", limit);
            }
        }
        0
    }

    fn print_all_limits(&self, soft: bool) {
        use libc::{getrlimit, rlimit};
        use libc::{RLIMIT_AS, RLIMIT_CORE, RLIMIT_CPU, RLIMIT_DATA, RLIMIT_FSIZE};
        use libc::{RLIMIT_NOFILE, RLIMIT_NPROC, RLIMIT_RSS, RLIMIT_STACK};

        let limits = [
            (RLIMIT_CORE, "core file size", "blocks", 512),
            (RLIMIT_DATA, "data seg size", "kbytes", 1024),
            (RLIMIT_FSIZE, "file size", "blocks", 512),
            (RLIMIT_NOFILE, "open files", "", 1),
            (RLIMIT_STACK, "stack size", "kbytes", 1024),
            (RLIMIT_CPU, "cpu time", "seconds", 1),
            (RLIMIT_NPROC, "max user processes", "", 1),
            (RLIMIT_AS, "virtual memory", "kbytes", 1024),
            (RLIMIT_RSS, "max memory size", "kbytes", 1024),
        ];

        for (resource, name, unit, divisor) in limits {
            let mut rlim = rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            unsafe {
                if getrlimit(resource, &mut rlim) == 0 {
                    let limit = if soft { rlim.rlim_cur } else { rlim.rlim_max };
                    let unit_str = if unit.is_empty() {
                        ""
                    } else {
                        &format!("({})", unit)
                    };
                    if limit == libc::RLIM_INFINITY as libc::rlim_t {
                        println!("{:25} {} unlimited", name, unit_str);
                    } else {
                        println!("{:25} {} {}", name, unit_str, limit / divisor);
                    }
                }
            }
        }
    }

    /// limit - csh-style resource limits
    fn builtin_limit(&self, args: &[String]) -> i32 {
        // Delegate to ulimit with csh-style names
        self.builtin_ulimit(args)
    }

    /// unlimit - remove resource limits
    fn builtin_unlimit(&self, args: &[String]) -> i32 {
        let mut new_args = args.to_vec();
        new_args.push("unlimited".to_string());
        self.builtin_ulimit(&new_args)
    }

    /// umask - get/set file creation mask
    fn builtin_umask(&self, args: &[String]) -> i32 {
        use libc::umask;

        let mut symbolic = false;
        let mut value: Option<&str> = None;

        for arg in args {
            match arg.as_str() {
                "-S" => symbolic = true,
                _ if !arg.starts_with('-') => value = Some(arg),
                _ => {}
            }
        }

        if let Some(v) = value {
            // Set umask
            if let Ok(mask) = u32::from_str_radix(v, 8) {
                unsafe {
                    umask(mask as libc::mode_t);
                }
            } else {
                eprintln!("umask: invalid mask: {}", v);
                return 1;
            }
        } else {
            // Get umask
            let mask = unsafe {
                let m = umask(0);
                umask(m);
                m
            };
            if symbolic {
                let u = 7 - ((mask >> 6) & 7);
                let g = 7 - ((mask >> 3) & 7);
                let o = 7 - (mask & 7);
                println!(
                    "u={}{}{}g={}{}{}o={}{}{}",
                    if u & 4 != 0 { "r" } else { "" },
                    if u & 2 != 0 { "w" } else { "" },
                    if u & 1 != 0 { "x" } else { "" },
                    if g & 4 != 0 { "r" } else { "" },
                    if g & 2 != 0 { "w" } else { "" },
                    if g & 1 != 0 { "x" } else { "" },
                    if o & 4 != 0 { "r" } else { "" },
                    if o & 2 != 0 { "w" } else { "" },
                    if o & 1 != 0 { "x" } else { "" },
                );
            } else {
                println!("{:04o}", mask);
            }
        }
        0
    }

    /// rehash - rebuild command hash table
    fn builtin_rehash(&mut self, _args: &[String]) -> i32 {
        // Clear any cached command paths
        // In a full implementation, this would rebuild the hash table
        0
    }

    /// unhash - remove entries from hash table
    fn builtin_unhash(&mut self, args: &[String]) -> i32 {
        let mut remove_aliases = false;
        let mut remove_functions = false;
        let mut remove_dirs = false;
        let mut names: Vec<&str> = Vec::new();

        for arg in args {
            match arg.as_str() {
                "-a" => remove_aliases = true,
                "-f" => remove_functions = true,
                "-d" => remove_dirs = true,
                "-m" => {} // pattern matching (TODO)
                _ if arg.starts_with('-') => {}
                _ => names.push(arg),
            }
        }

        for name in names {
            if remove_aliases {
                self.aliases.remove(name);
            }
            if remove_functions {
                self.functions.remove(name);
            }
            if remove_dirs {
                // Remove from named directories (TODO)
            }
        }
        0
    }

    /// times - print accumulated user and system times
    fn builtin_times(&self, _args: &[String]) -> i32 {
        use libc::{getrusage, rusage, RUSAGE_CHILDREN, RUSAGE_SELF};

        let mut self_usage: rusage = unsafe { std::mem::zeroed() };
        let mut child_usage: rusage = unsafe { std::mem::zeroed() };

        unsafe {
            getrusage(RUSAGE_SELF, &mut self_usage);
            getrusage(RUSAGE_CHILDREN, &mut child_usage);
        }

        let self_user =
            self_usage.ru_utime.tv_sec as f64 + self_usage.ru_utime.tv_usec as f64 / 1_000_000.0;
        let self_sys =
            self_usage.ru_stime.tv_sec as f64 + self_usage.ru_stime.tv_usec as f64 / 1_000_000.0;
        let child_user =
            child_usage.ru_utime.tv_sec as f64 + child_usage.ru_utime.tv_usec as f64 / 1_000_000.0;
        let child_sys =
            child_usage.ru_stime.tv_sec as f64 + child_usage.ru_stime.tv_usec as f64 / 1_000_000.0;

        println!("{:.3}s {:.3}s", self_user, self_sys);
        println!("{:.3}s {:.3}s", child_user, child_sys);
        0
    }

    /// zmodload - load/unload zsh modules (stub)
    fn builtin_zmodload(&mut self, args: &[String]) -> i32 {
        let mut list_loaded = false;
        let mut unload = false;
        let mut modules: Vec<&str> = Vec::new();

        for arg in args {
            match arg.as_str() {
                "-l" | "-L" => list_loaded = true,
                "-u" => unload = true,
                "-a" | "-b" | "-c" | "-d" | "-e" | "-f" | "-i" | "-p" | "-s" => {}
                _ if arg.starts_with('-') => {}
                _ => modules.push(arg),
            }
        }

        if list_loaded || modules.is_empty() {
            // List loaded modules (stub - we don't really have modules)
            println!("zsh/complete");
            println!("zsh/complist");
            println!("zsh/parameter");
            println!("zsh/zutil");
            return 0;
        }

        for module in modules {
            if unload {
                // Unload module (stub)
                self.options.remove(&format!("_module_{}", module));
            } else {
                // Load module (stub)
                self.options.insert(format!("_module_{}", module), true);
            }
        }
        0
    }

    /// r - redo last command (alias for fc -e -)
    fn builtin_r(&mut self, args: &[String]) -> i32 {
        let mut fc_args = vec!["-e".to_string(), "-".to_string()];
        fc_args.extend(args.iter().cloned());
        self.builtin_fc(&fc_args)
    }

    /// ttyctl - control terminal settings
    fn builtin_ttyctl(&self, args: &[String]) -> i32 {
        for arg in args {
            match arg.as_str() {
                "-f" => {
                    // Freeze terminal settings
                    // In a full implementation, this would save terminal state
                }
                "-u" => {
                    // Unfreeze terminal settings
                }
                _ => {}
            }
        }
        0
    }

    /// noglob - run command without globbing
    fn builtin_noglob(&mut self, args: &[String], redirects: &[Redirect]) -> i32 {
        if args.is_empty() {
            return 0;
        }

        // Temporarily disable globbing
        let saved = self.options.get("noglob").cloned();
        self.options.insert("noglob".to_string(), true);

        // Execute the command
        let status = self.builtin_command(args, redirects);

        // Restore globbing state
        if let Some(v) = saved {
            self.options.insert("noglob".to_string(), v);
        } else {
            self.options.remove("noglob");
        }

        status
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // zsh module builtins
    // ═══════════════════════════════════════════════════════════════════════════

    /// zstat - file status (zsh/stat module)
    fn builtin_zstat(&self, args: &[String]) -> i32 {
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::fs::PermissionsExt;

        let mut show_all = true;
        let mut symbolic_mode = false;
        let mut show_link = false;
        let mut as_array = false;
        let mut array_name = String::new();
        let mut format_time = String::new();
        let mut elements: Vec<String> = Vec::new();
        let mut files: Vec<&str> = Vec::new();

        let mut iter = args.iter().peekable();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "-s" => symbolic_mode = true,
                "-L" => show_link = true,
                "-N" => {} // Don't resolve symlinks
                "-n" => {} // Numeric user/group
                "-o" => show_all = false,
                "-A" => {
                    as_array = true;
                    if let Some(name) = iter.next() {
                        array_name = name.clone();
                    }
                }
                "-F" => {
                    if let Some(fmt) = iter.next() {
                        format_time = fmt.clone();
                    }
                }
                s if s.starts_with('+') => {
                    elements.push(s[1..].to_string());
                    show_all = false;
                }
                s if !s.starts_with('-') => files.push(s),
                _ => {}
            }
        }

        if files.is_empty() {
            eprintln!("zstat: no files specified");
            return 1;
        }

        for file in files {
            let meta = if show_link {
                std::fs::symlink_metadata(file)
            } else {
                std::fs::metadata(file)
            };

            let meta = match meta {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("zstat: {}: {}", file, e);
                    return 1;
                }
            };

            let output_element = |name: &str, value: &str| {
                if as_array {
                    // Would need mutable self to store in array
                    println!("{}={}", name, value);
                } else if show_all || elements.contains(&name.to_string()) {
                    println!("{}: {}", name, value);
                }
            };

            output_element("device", &meta.dev().to_string());
            output_element("inode", &meta.ino().to_string());

            if symbolic_mode {
                let mode = meta.permissions().mode();
                let mode_str = format!(
                    "{}{}{}{}{}{}{}{}{}{}",
                    match mode & 0o170000 {
                        0o040000 => 'd',
                        0o120000 => 'l',
                        0o100000 => '-',
                        0o060000 => 'b',
                        0o020000 => 'c',
                        0o010000 => 'p',
                        0o140000 => 's',
                        _ => '?',
                    },
                    if mode & 0o400 != 0 { 'r' } else { '-' },
                    if mode & 0o200 != 0 { 'w' } else { '-' },
                    if mode & 0o4000 != 0 {
                        's'
                    } else if mode & 0o100 != 0 {
                        'x'
                    } else {
                        '-'
                    },
                    if mode & 0o040 != 0 { 'r' } else { '-' },
                    if mode & 0o020 != 0 { 'w' } else { '-' },
                    if mode & 0o2000 != 0 {
                        's'
                    } else if mode & 0o010 != 0 {
                        'x'
                    } else {
                        '-'
                    },
                    if mode & 0o004 != 0 { 'r' } else { '-' },
                    if mode & 0o002 != 0 { 'w' } else { '-' },
                    if mode & 0o1000 != 0 {
                        't'
                    } else if mode & 0o001 != 0 {
                        'x'
                    } else {
                        '-'
                    },
                );
                output_element("mode", &mode_str);
            } else {
                output_element("mode", &format!("{:o}", meta.permissions().mode()));
            }

            output_element("nlink", &meta.nlink().to_string());
            output_element("uid", &meta.uid().to_string());
            output_element("gid", &meta.gid().to_string());
            output_element("rdev", &meta.rdev().to_string());
            output_element("size", &meta.len().to_string());

            let format_timestamp = |secs: i64| -> String {
                if format_time.is_empty() {
                    secs.to_string()
                } else {
                    chrono::DateTime::from_timestamp(secs, 0)
                        .map(|dt| dt.format(&format_time).to_string())
                        .unwrap_or_else(|| secs.to_string())
                }
            };

            output_element("atime", &format_timestamp(meta.atime()));
            output_element("mtime", &format_timestamp(meta.mtime()));
            output_element("ctime", &format_timestamp(meta.ctime()));
            output_element("blksize", &meta.blksize().to_string());
            output_element("blocks", &meta.blocks().to_string());

            if show_link && meta.file_type().is_symlink() {
                if let Ok(target) = std::fs::read_link(file) {
                    output_element("link", &target.to_string_lossy());
                }
            }
        }

        0
    }

    /// strftime - format date/time (zsh/datetime module)
    fn builtin_strftime(&self, args: &[String]) -> i32 {
        let mut format = "%c".to_string();
        let mut timestamp: Option<i64> = None;
        let mut to_var = false;
        let mut var_name = String::new();

        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "-s" => {
                    to_var = true;
                    if let Some(name) = iter.next() {
                        var_name = name.clone();
                    }
                }
                "-r" => {
                    // Reference time from a variable
                    if let Some(ts_str) = iter.next() {
                        timestamp = ts_str.parse().ok();
                    }
                }
                s if !s.starts_with('-') => {
                    if format == "%c" {
                        format = s.to_string();
                    } else if timestamp.is_none() {
                        timestamp = s.parse().ok();
                    }
                }
                _ => {}
            }
        }

        let ts = timestamp.unwrap_or_else(|| chrono::Local::now().timestamp());

        let result = chrono::DateTime::from_timestamp(ts, 0)
            .map(|dt: chrono::DateTime<chrono::Utc>| {
                dt.with_timezone(&chrono::Local).format(&format).to_string()
            })
            .unwrap_or_else(|| "invalid timestamp".to_string());

        if to_var && !var_name.is_empty() {
            // Would need mutable self
            println!("{}={}", var_name, result);
        } else {
            println!("{}", result);
        }

        0
    }

    /// zsleep - sleep with fractional seconds
    fn builtin_zsleep(&self, args: &[String]) -> i32 {
        if args.is_empty() {
            eprintln!("zsleep: missing argument");
            return 1;
        }

        let secs: f64 = match args[0].parse() {
            Ok(s) => s,
            Err(_) => {
                eprintln!("zsleep: invalid number: {}", args[0]);
                return 1;
            }
        };

        std::thread::sleep(std::time::Duration::from_secs_f64(secs));
        0
    }

    /// zln/zmv/zcp - file operations (zsh/files module)
    fn builtin_zfiles(&self, cmd: &str, args: &[String]) -> i32 {
        let mut force = false;
        let mut verbose = false;
        let mut files: Vec<&str> = Vec::new();

        for arg in args {
            match arg.as_str() {
                "-f" => force = true,
                "-v" => verbose = true,
                "-i" => {} // interactive - ignored
                s if !s.starts_with('-') => files.push(s),
                _ => {}
            }
        }

        if files.len() < 2 {
            eprintln!("{}: missing operand", cmd);
            return 1;
        }

        let target = files.pop().unwrap();
        let target_is_dir = std::path::Path::new(target).is_dir();

        for src in files {
            let dest = if target_is_dir {
                format!(
                    "{}/{}",
                    target,
                    std::path::Path::new(src)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| src.to_string())
                )
            } else {
                target.to_string()
            };

            if !force && std::path::Path::new(&dest).exists() {
                eprintln!("{}: '{}' already exists", cmd, dest);
                continue;
            }

            let result = match cmd {
                "zln" => {
                    #[cfg(unix)]
                    {
                        std::os::unix::fs::symlink(src, &dest)
                    }
                    #[cfg(not(unix))]
                    {
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Unsupported,
                            "symlinks not supported",
                        ))
                    }
                }
                "zcp" => std::fs::copy(src, &dest).map(|_| ()),
                "zmv" => std::fs::rename(src, &dest),
                _ => Ok(()),
            };

            match result {
                Ok(()) => {
                    if verbose {
                        println!("{} -> {}", src, dest);
                    }
                }
                Err(e) => {
                    eprintln!("{}: {}: {}", cmd, src, e);
                    return 1;
                }
            }
        }

        0
    }

    /// coproc - manage coprocesses
    fn builtin_coproc(&mut self, args: &[String]) -> i32 {
        // Basic coproc implementation
        if args.is_empty() {
            // List coprocesses
            println!("(no coprocesses)");
            return 0;
        }

        // Start a coprocess
        let cmd = args.join(" ");
        match std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
        {
            Ok(child) => {
                println!("[coproc] {}", child.id());
                0
            }
            Err(e) => {
                eprintln!("coproc: {}", e);
                1
            }
        }
    }

    /// zparseopts - parse options from positional parameters
    fn builtin_zparseopts(&mut self, args: &[String]) -> i32 {
        let mut remove_parsed = false; // -D
        let mut keep_going = false; // -E
        let mut fail_on_error = false; // -F
        let mut keep_values = false; // -K
        let mut map_names = false; // -M
        let mut array_name: Option<String> = None; // -a
        let mut assoc_name: Option<String> = None; // -A
        let mut specs: Vec<String> = Vec::new();

        let mut iter = args.iter().peekable();

        // Parse zparseopts options
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "-D" => remove_parsed = true,
                "-E" => keep_going = true,
                "-F" => fail_on_error = true,
                "-K" => keep_values = true,
                "-M" => map_names = true,
                "-a" => {
                    if let Some(name) = iter.next() {
                        array_name = Some(name.clone());
                    }
                }
                "-A" => {
                    if let Some(name) = iter.next() {
                        assoc_name = Some(name.clone());
                    }
                }
                "-" | "--" => break,
                s if !s.starts_with('-') || s.contains('=') || s.contains(':') => {
                    specs.push(s.to_string());
                }
                _ => specs.push(arg.clone()),
            }
        }

        // Collect remaining specs
        for arg in iter {
            specs.push(arg.clone());
        }

        // Parse the specs to understand what options we're looking for
        #[derive(Clone)]
        struct OptSpec {
            name: String,
            takes_arg: bool,
            optional_arg: bool,
            append: bool,
            target_array: Option<String>,
        }

        let mut opt_specs: Vec<OptSpec> = Vec::new();
        for spec in &specs {
            let mut s = spec.as_str();
            let mut target = None;

            // Check for =array at end
            if let Some(eq_pos) = s.rfind('=') {
                if !s[eq_pos + 1..].contains(':') {
                    target = Some(s[eq_pos + 1..].to_string());
                    s = &s[..eq_pos];
                }
            }

            let append = s.ends_with('+') || s.contains("+:");
            let s = s.trim_end_matches('+');

            let (name, takes_arg, optional_arg) = if s.ends_with("::") {
                (s.trim_end_matches(':').trim_end_matches(':'), true, true)
            } else if s.ends_with(':') {
                (s.trim_end_matches(':'), true, false)
            } else {
                (s, false, false)
            };

            opt_specs.push(OptSpec {
                name: name.to_string(),
                takes_arg,
                optional_arg,
                append,
                target_array: target,
            });
        }

        // Get positional parameters to parse
        let positionals: Vec<String> = (1..=99)
            .map(|i| self.get_variable(&i.to_string()))
            .take_while(|v| !v.is_empty())
            .collect();

        // Results
        let mut results: Vec<(String, Option<String>)> = Vec::new();
        let mut i = 0;
        let mut parsed_count = 0;

        while i < positionals.len() {
            let arg = &positionals[i];

            if arg == "-" || arg == "--" {
                parsed_count = i + 1;
                break;
            }

            if !arg.starts_with('-') {
                if !keep_going {
                    break;
                }
                i += 1;
                continue;
            }

            // Try to match against specs
            let opt_name = arg.trim_start_matches('-');
            let mut matched = false;

            for spec in &opt_specs {
                if opt_name == spec.name || opt_name.starts_with(&format!("{}=", spec.name)) {
                    matched = true;

                    if spec.takes_arg {
                        let arg_value = if opt_name.contains('=') {
                            Some(opt_name.splitn(2, '=').nth(1).unwrap_or("").to_string())
                        } else if i + 1 < positionals.len()
                            && (!positionals[i + 1].starts_with('-') || spec.optional_arg)
                        {
                            i += 1;
                            Some(positionals[i].clone())
                        } else if spec.optional_arg {
                            None
                        } else if fail_on_error {
                            eprintln!("zparseopts: missing argument for option: {}", spec.name);
                            return 1;
                        } else {
                            None
                        };
                        results.push((format!("-{}", spec.name), arg_value));
                    } else {
                        results.push((format!("-{}", spec.name), None));
                    }
                    break;
                }
            }

            if !matched && !keep_going {
                break;
            }

            i += 1;
            parsed_count = i;
        }

        // Store results in array
        if let Some(arr_name) = &array_name {
            let mut arr_values: Vec<String> = Vec::new();
            for (opt, val) in &results {
                arr_values.push(opt.clone());
                if let Some(v) = val {
                    arr_values.push(v.clone());
                }
            }
            self.arrays.insert(arr_name.clone(), arr_values);
        }

        // Store in associative array
        if let Some(assoc) = &assoc_name {
            let mut map: HashMap<String, String> = HashMap::new();
            for (opt, val) in &results {
                map.insert(opt.clone(), val.clone().unwrap_or_default());
            }
            self.assoc_arrays.insert(assoc.clone(), map);
        }

        // Store in per-option arrays
        for spec in &opt_specs {
            if let Some(target) = &spec.target_array {
                let values: Vec<String> = results
                    .iter()
                    .filter(|(opt, _)| opt.trim_start_matches('-') == spec.name)
                    .flat_map(|(opt, val)| {
                        let mut v = vec![opt.clone()];
                        if let Some(arg) = val {
                            v.push(arg.clone());
                        }
                        v
                    })
                    .collect();
                if !values.is_empty() || !keep_values {
                    self.arrays.insert(target.clone(), values);
                }
            }
        }

        // Remove parsed arguments if -D
        if remove_parsed && parsed_count > 0 {
            for i in 1..=parsed_count {
                self.variables.remove(&i.to_string());
                std::env::remove_var(i.to_string());
            }
            // Shift remaining
            let remaining: Vec<String> = ((parsed_count + 1)..=99)
                .map(|i| self.get_variable(&i.to_string()))
                .take_while(|v| !v.is_empty())
                .collect();
            for (i, val) in remaining.iter().enumerate() {
                self.variables.insert((i + 1).to_string(), val.clone());
            }
        }

        0
    }

    /// readonly - mark variables as read-only
    fn builtin_readonly(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            // List readonly variables
            for name in &self.readonly_vars {
                if let Some(val) = self.variables.get(name) {
                    println!("readonly {}={}", name, val);
                }
            }
            return 0;
        }

        for arg in args {
            if arg == "-p" {
                for name in &self.readonly_vars {
                    if let Some(val) = self.variables.get(name) {
                        println!("declare -r {}=\"{}\"", name, val);
                    }
                }
            } else if let Some(eq_pos) = arg.find('=') {
                let name = &arg[..eq_pos];
                let value = &arg[eq_pos + 1..];
                self.variables.insert(name.to_string(), value.to_string());
                self.readonly_vars.insert(name.to_string());
            } else {
                self.readonly_vars.insert(arg.clone());
            }
        }
        0
    }

    /// unfunction - remove function definitions
    fn builtin_unfunction(&mut self, args: &[String]) -> i32 {
        for name in args {
            if self.functions.remove(name).is_none() {
                eprintln!("unfunction: no such function: {}", name);
            }
        }
        0
    }

    /// getln - read line from buffer
    fn builtin_getln(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            eprintln!("getln: missing variable name");
            return 1;
        }
        // Read from line buffer (simplified - just reads from stdin)
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).is_ok() {
            let line = line.trim_end_matches('\n');
            self.variables.insert(args[0].clone(), line.to_string());
            0
        } else {
            1
        }
    }

    /// pushln - push line to buffer
    fn builtin_pushln(&mut self, args: &[String]) -> i32 {
        for arg in args {
            println!("{}", arg);
        }
        0
    }

    /// bindkey - key binding management
    fn builtin_bindkey(&mut self, args: &[String]) -> i32 {
        use crate::zle::{zle, KeymapName};

        if args.is_empty() {
            // List all bindings in main keymap
            let zle = zle();
            for (keys, widget) in zle
                .keymaps
                .get(&KeymapName::Main)
                .map(|km| km.list_bindings().collect::<Vec<_>>())
                .unwrap_or_default()
            {
                println!("\"{}\" {}", keys, widget);
            }
            return 0;
        }

        let mut iter = args.iter().peekable();
        let mut keymap = KeymapName::Main;
        let mut list_mode = false;
        let mut list_all = false;
        let mut remove = false;

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "-l" => {
                    list_mode = true;
                }
                "-L" => {
                    list_mode = true;
                    list_all = true;
                }
                "-la" | "-lL" => {
                    list_mode = true;
                    list_all = true;
                }
                "-M" => {
                    if let Some(name) = iter.next() {
                        if let Some(km) = KeymapName::from_str(name) {
                            keymap = km;
                        }
                    }
                }
                "-r" => {
                    remove = true;
                }
                "-A" => {
                    // Link keymaps - stub
                    return 0;
                }
                "-N" => {
                    // Create new keymap - stub
                    return 0;
                }
                "-e" => {
                    keymap = KeymapName::Emacs;
                }
                "-v" => {
                    keymap = KeymapName::ViInsert;
                }
                "-a" => {
                    keymap = KeymapName::ViCommand;
                }
                key if !key.starts_with('-') => {
                    // Key sequence - next arg is widget
                    if let Some(widget) = iter.next() {
                        let mut zle = zle();
                        if remove {
                            zle.unbind_key(keymap, key);
                        } else {
                            zle.bind_key(keymap, key, widget);
                        }
                    }
                    return 0;
                }
                _ => {}
            }
        }

        if list_mode {
            let zle = zle();
            if list_all {
                for km_name in &[
                    KeymapName::Emacs,
                    KeymapName::ViInsert,
                    KeymapName::ViCommand,
                ] {
                    println!("{}", km_name.as_str());
                }
            } else {
                if let Some(km) = zle.keymaps.get(&keymap) {
                    for (keys, widget) in km.list_bindings() {
                        println!("bindkey \"{}\" {}", keys, widget);
                    }
                }
            }
        }

        0
    }

    /// zle - line editor control
    fn builtin_zle(&mut self, args: &[String]) -> i32 {
        use crate::zle::zle;

        if args.is_empty() {
            return 0;
        }

        let mut iter = args.iter().peekable();

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "-l" => {
                    // List widgets
                    let zle = zle();
                    let mut widgets: Vec<&str> = zle.list_widgets();
                    widgets.sort();
                    for w in widgets {
                        println!("{}", w);
                    }
                    return 0;
                }
                "-la" | "-lL" => {
                    // List all widgets with details
                    let zle = zle();
                    let mut widgets: Vec<&str> = zle.list_widgets();
                    widgets.sort();
                    for w in widgets {
                        println!("{}", w);
                    }
                    return 0;
                }
                "-N" => {
                    // Define new widget: zle -N widget-name [function]
                    if let Some(widget_name) = iter.next() {
                        let func_name = iter
                            .next()
                            .map(|s| s.as_str())
                            .unwrap_or(widget_name.as_str());
                        let mut zle = zle();
                        zle.define_widget(widget_name, func_name);
                    }
                    return 0;
                }
                "-D" => {
                    // Delete widget - stub
                    return 0;
                }
                "-A" => {
                    // Define widget alias - stub
                    return 0;
                }
                "-R" => {
                    // Redisplay
                    return 0;
                }
                "-U" => {
                    // Unget characters - stub
                    return 0;
                }
                "-K" => {
                    // Select keymap - stub
                    return 0;
                }
                "-F" => {
                    // Install file descriptor handler - stub
                    return 0;
                }
                "-M" => {
                    // Display message - stub
                    return 0;
                }
                "-I" => {
                    // Invalidate completion - stub
                    return 0;
                }
                "-f" => {
                    // Check widget exists
                    if let Some(name) = iter.next() {
                        let zle = zle();
                        return if zle.get_widget(name).is_some() { 0 } else { 1 };
                    }
                    return 1;
                }
                widget_name if !widget_name.starts_with('-') => {
                    // Call widget
                    let mut zle = zle();
                    match zle.execute_widget(widget_name, None) {
                        crate::shell_zle::WidgetResult::Ok => return 0,
                        crate::shell_zle::WidgetResult::Error(e) => {
                            eprintln!("zle: {}", e);
                            return 1;
                        }
                        crate::shell_zle::WidgetResult::CallFunction(func) => {
                            // Would need to call shell function
                            drop(zle);
                            if let Some(f) = self.functions.get(&func).cloned() {
                                return self.call_function(&f, &[]).unwrap_or(1);
                            }
                            return 1;
                        }
                        _ => return 0,
                    }
                }
                _ => {}
            }
        }

        0
    }

    /// sched - scheduled command execution (stub)
    fn builtin_sched(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            // List scheduled commands
            println!("(no scheduled commands)");
            return 0;
        }
        // Would need background scheduler - stub for now
        eprintln!("sched: scheduling not implemented");
        0
    }

    /// zcompile - compile shell scripts to ZWC format
    fn builtin_zcompile(&mut self, args: &[String]) -> i32 {
        use crate::shell_zwc::{ZwcBuilder, ZwcFile};

        let mut list_mode = false; // -t: list functions in zwc
        let mut compile_current = false; // -c: compile current functions
        let mut compile_auto = false; // -a: compile autoload functions
        let mut files: Vec<String> = Vec::new();

        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            if arg.starts_with('-') && arg.len() > 1 {
                for c in arg[1..].chars() {
                    match c {
                        't' => list_mode = true,
                        'c' => compile_current = true,
                        'a' => compile_auto = true,
                        'U' | 'M' | 'R' | 'm' | 'z' | 'k' => {} // ignored for now
                        _ => {
                            eprintln!("zcompile: unknown option: -{}", c);
                            return 1;
                        }
                    }
                }
            } else {
                files.push(arg.clone());
            }
            i += 1;
        }

        if files.is_empty() {
            eprintln!("zcompile: not enough arguments");
            return 1;
        }

        // -t mode: list functions in ZWC file
        if list_mode {
            let zwc_path = if files[0].ends_with(".zwc") {
                files[0].clone()
            } else {
                format!("{}.zwc", files[0])
            };

            match ZwcFile::load(&zwc_path) {
                Ok(zwc) => {
                    println!("zwc file for zshrs-{}", env!("CARGO_PKG_VERSION"));
                    if files.len() > 1 {
                        // Check specific functions
                        for name in &files[1..] {
                            if zwc.get_function(name).is_some() {
                                println!("{}", name);
                            } else {
                                eprintln!("zcompile: function not found: {}", name);
                                return 1;
                            }
                        }
                    } else {
                        // List all functions
                        for name in zwc.list_functions() {
                            println!("{}", name);
                        }
                    }
                    return 0;
                }
                Err(e) => {
                    eprintln!("zcompile: can't read zwc file: {}: {}", zwc_path, e);
                    return 1;
                }
            }
        }

        // -c or -a mode: compile current/autoload functions
        if compile_current || compile_auto {
            let zwc_path = if files[0].ends_with(".zwc") {
                files[0].clone()
            } else {
                format!("{}.zwc", files[0])
            };

            let mut builder = ZwcBuilder::new();

            if files.len() > 1 {
                // Compile specific functions
                for name in &files[1..] {
                    if let Some(func) = self.functions.get(name) {
                        // Serialize the function (simplified - just store as comment for now)
                        let source = format!("# Compiled function: {}\n# Body: {:?}", name, func);
                        builder.add_source(name, &source);
                    } else if compile_auto && self.autoload_pending.contains_key(name) {
                        // Try to load autoload function source
                        if let Some(path) = self.find_function_file(name) {
                            if let Err(e) = builder.add_file(&path) {
                                eprintln!("zcompile: can't read {}: {}", name, e);
                                return 1;
                            }
                        }
                    } else {
                        eprintln!("zcompile: no such function: {}", name);
                        return 1;
                    }
                }
            } else {
                // Compile all functions
                for (name, func) in &self.functions {
                    let source = format!("# Compiled function: {}\n# Body: {:?}", name, func);
                    builder.add_source(name, &source);
                }
            }

            if let Err(e) = builder.write(&zwc_path) {
                eprintln!("zcompile: can't write {}: {}", zwc_path, e);
                return 1;
            }
            return 0;
        }

        // Default: compile files to ZWC
        let zwc_path = if files[0].ends_with(".zwc") {
            files[0].clone()
        } else {
            format!("{}.zwc", files[0])
        };

        let mut builder = ZwcBuilder::new();

        // If only one file given, it's both the source and output base
        let source_files = if files.len() == 1 {
            // Check if it's a directory
            let path = std::path::Path::new(&files[0]);
            if path.is_dir() {
                // Compile all files in directory
                match std::fs::read_dir(path) {
                    Ok(entries) => {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            if p.is_file() && !p.extension().map_or(false, |e| e == "zwc") {
                                if let Err(e) = builder.add_file(&p) {
                                    eprintln!("zcompile: can't read {:?}: {}", p, e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("zcompile: can't read directory: {}", e);
                        return 1;
                    }
                }
                vec![]
            } else {
                vec![files[0].clone()]
            }
        } else {
            files[1..].to_vec()
        };

        for file in &source_files {
            let path = std::path::Path::new(file);
            if let Err(e) = builder.add_file(path) {
                eprintln!("zcompile: can't read {}: {}", file, e);
                return 1;
            }
        }

        if let Err(e) = builder.write(&zwc_path) {
            eprintln!("zcompile: can't write {}: {}", zwc_path, e);
            return 1;
        }

        0
    }

    /// zformat - format strings
    fn builtin_zformat(&self, args: &[String]) -> i32 {
        if args.len() < 2 {
            eprintln!("zformat: not enough arguments");
            return 1;
        }

        match args[0].as_str() {
            "-f" => {
                // Format string: zformat -f var format specs...
                if args.len() < 3 {
                    return 1;
                }
                let _var_name = &args[1];
                let format = &args[2];
                let specs: HashMap<char, &str> = args[3..]
                    .iter()
                    .filter_map(|s| {
                        let mut chars = s.chars();
                        let key = chars.next()?;
                        if chars.next() == Some(':') {
                            Some((key, &s[2..]))
                        } else {
                            None
                        }
                    })
                    .collect();

                let mut result = String::new();
                let mut chars = format.chars().peekable();
                while let Some(c) = chars.next() {
                    if c == '%' {
                        if let Some(&spec_char) = chars.peek() {
                            if let Some(replacement) = specs.get(&spec_char) {
                                result.push_str(replacement);
                                chars.next();
                                continue;
                            }
                        }
                    }
                    result.push(c);
                }
                println!("{}", result);
            }
            "-a" => {
                // Format into array elements
                println!("zformat -a: not implemented");
            }
            _ => {
                eprintln!("zformat: unknown option: {}", args[0]);
                return 1;
            }
        }
        0
    }

    /// vared - visually edit a variable
    fn builtin_vared(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            eprintln!("vared: not enough arguments");
            return 1;
        }

        let mut var_name = String::new();
        let mut prompt = String::new();
        let mut rprompt = String::new();
        let mut history = false;
        let mut i = 0;

        while i < args.len() {
            match args[i].as_str() {
                "-p" if i + 1 < args.len() => {
                    i += 1;
                    prompt = args[i].clone();
                }
                "-r" if i + 1 < args.len() => {
                    i += 1;
                    rprompt = args[i].clone();
                }
                "-h" => history = true,
                "-c" => {} // Use completion - ignored
                "-e" => {} // Use emacs mode - ignored
                "-M" | "-m" => {
                    i += 1;
                } // Main/alt keymap - skip arg
                "-a" | "-A" => {
                    i += 1;
                } // Array assignment - skip arg
                s if !s.starts_with('-') => {
                    var_name = s.to_string();
                }
                _ => {}
            }
            i += 1;
        }

        if var_name.is_empty() {
            eprintln!("vared: not enough arguments");
            return 1;
        }

        // Get current value
        let current = self.get_variable(&var_name);

        // Simple line editing using stdin
        if !prompt.is_empty() {
            eprint!("{}", prompt);
        }
        print!("{}", current);
        if !rprompt.is_empty() {
            eprint!("{}", rprompt);
        }

        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_ok() {
            let value = input.trim_end_matches('\n').to_string();
            self.variables.insert(var_name, value);
            return 0;
        }
        1
    }

    /// echotc - output termcap value
    fn builtin_echotc(&self, args: &[String]) -> i32 {
        if args.is_empty() {
            eprintln!("echotc: not enough arguments");
            return 1;
        }

        // Common termcap capabilities
        match args[0].as_str() {
            "cl" => print!("\x1b[H\x1b[2J"), // clear screen
            "cd" => print!("\x1b[J"),        // clear to end of display
            "ce" => print!("\x1b[K"),        // clear to end of line
            "cm" => {
                // cursor motion - needs row, col args
                if args.len() >= 3 {
                    if let (Ok(row), Ok(col)) = (args[1].parse::<u32>(), args[2].parse::<u32>()) {
                        print!("\x1b[{};{}H", row + 1, col + 1);
                    }
                }
            }
            "up" => print!("\x1b[A"),    // cursor up
            "do" => print!("\x1b[B"),    // cursor down
            "le" => print!("\x1b[D"),    // cursor left
            "nd" => print!("\x1b[C"),    // cursor right
            "ho" => print!("\x1b[H"),    // home cursor
            "vi" => print!("\x1b[?25l"), // invisible cursor
            "ve" => print!("\x1b[?25h"), // visible cursor
            "so" => print!("\x1b[7m"),   // standout mode
            "se" => print!("\x1b[27m"),  // end standout
            "us" => print!("\x1b[4m"),   // underline
            "ue" => print!("\x1b[24m"),  // end underline
            "md" => print!("\x1b[1m"),   // bold
            "me" => print!("\x1b[0m"),   // end all attributes
            "mr" => print!("\x1b[7m"),   // reverse video
            "AF" | "setaf" => {
                // Set foreground color
                if args.len() >= 2 {
                    if let Ok(color) = args[1].parse::<u32>() {
                        print!("\x1b[38;5;{}m", color);
                    }
                }
            }
            "AB" | "setab" => {
                // Set background color
                if args.len() >= 2 {
                    if let Ok(color) = args[1].parse::<u32>() {
                        print!("\x1b[48;5;{}m", color);
                    }
                }
            }
            "Co" | "colors" => {
                // Number of colors - assume 256
                println!("256");
            }
            "co" | "cols" => {
                // Number of columns
                println!(
                    "{}",
                    std::env::var("COLUMNS")
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(80u16)
                );
            }
            "li" | "lines" => {
                // Number of lines
                println!(
                    "{}",
                    std::env::var("LINES")
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(24u16)
                );
            }
            cap => {
                eprintln!("echotc: unknown capability: {}", cap);
                return 1;
            }
        }
        use std::io::Write;
        let _ = std::io::stdout().flush();
        0
    }

    /// echoti - output terminfo value
    fn builtin_echoti(&self, args: &[String]) -> i32 {
        // echoti is similar to echotc but uses terminfo names
        // For simplicity, we'll use the same implementation
        self.builtin_echotc(args)
    }

    /// zpty - manage pseudo-terminals
    fn builtin_zpty(&mut self, args: &[String]) -> i32 {
        // Stub - PTY management is complex
        if args.is_empty() {
            // List ptys
            println!("zpty: no ptys active");
            return 0;
        }

        match args[0].as_str() {
            "-d" => {
                // Delete pty
                if args.len() < 2 {
                    eprintln!("zpty: -d requires pty name");
                    return 1;
                }
                println!("zpty: {} deleted", args[1]);
            }
            "-w" => {
                // Write to pty
                eprintln!("zpty: -w not implemented");
                return 1;
            }
            "-r" => {
                // Read from pty
                eprintln!("zpty: -r not implemented");
                return 1;
            }
            "-t" => {
                // Test if data available
                return 1; // No data
            }
            "-L" => {
                // List in script-friendly format
                println!("zpty: no ptys active");
            }
            _ => {
                // Create new pty
                eprintln!("zpty: pty creation not implemented");
                return 1;
            }
        }
        0
    }

    /// zprof - profiling support
    fn builtin_zprof(&self, _args: &[String]) -> i32 {
        println!("num  calls                time                       self            name");
        println!(
            "-----------------------------------------------------------------------------------"
        );
        println!("(profiling not implemented)");
        0
    }

    /// zsocket - create/manage sockets
    fn builtin_zsocket(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            eprintln!("zsocket: not enough arguments");
            return 1;
        }

        match args[0].as_str() {
            "-l" => {
                // Listen on socket
                eprintln!("zsocket: listening not implemented");
                return 1;
            }
            "-a" => {
                // Accept connection
                eprintln!("zsocket: accept not implemented");
                return 1;
            }
            "-d" => {
                // Close socket
                if args.len() >= 2 {
                    println!("zsocket: closed fd {}", args[1]);
                }
            }
            "-v" => {
                // Verbose - store fd in variable
                eprintln!("zsocket: -v not implemented");
                return 1;
            }
            _ => {
                // Connect to host:port
                eprintln!("zsocket: connect not implemented");
                return 1;
            }
        }
        0
    }

    /// ztcp - TCP socket operations
    fn builtin_ztcp(&mut self, args: &[String]) -> i32 {
        // Similar to zsocket but TCP specific
        self.builtin_zsocket(args)
    }

    /// zregexparse - parse with regex
    fn builtin_zregexparse(&mut self, args: &[String]) -> i32 {
        if args.len() < 2 {
            eprintln!("zregexparse: not enough arguments");
            return 1;
        }

        // zregexparse var regex [action ...]
        // This is a complex builtin for parsing strings with regexes
        // Stub implementation
        eprintln!("zregexparse: not fully implemented");
        0
    }

    /// clone - create a subshell
    fn builtin_clone(&mut self, _args: &[String]) -> i32 {
        // Clone creates a subshell with the same state
        // In a real implementation, this would fork
        eprintln!("clone: not implemented (would fork)");
        0
    }

    /// log - same as logout for login shells
    fn builtin_log(&mut self, args: &[String]) -> i32 {
        self.builtin_exit(args)
    }

    // Completion system builtins (stubs for compsys)

    /// comparguments - parse completion arguments
    fn builtin_comparguments(&mut self, _args: &[String]) -> i32 {
        // Used internally by _arguments
        0
    }

    /// compcall - call completion function
    fn builtin_compcall(&mut self, _args: &[String]) -> i32 {
        // Calls the completion function
        0
    }

    /// compctl - old-style completion (deprecated)
    fn builtin_compctl(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            println!("compctl: old-style completion system");
            println!("Use the new completion system (compsys) instead");
            return 0;
        }
        // Parse compctl options for backwards compatibility
        0
    }

    /// compdescribe - describe completions
    fn builtin_compdescribe(&mut self, _args: &[String]) -> i32 {
        0
    }

    /// compfiles - complete files
    fn builtin_compfiles(&mut self, _args: &[String]) -> i32 {
        0
    }

    /// compgroups - group completions
    fn builtin_compgroups(&mut self, _args: &[String]) -> i32 {
        0
    }

    /// compquote - quote completion strings
    fn builtin_compquote(&mut self, _args: &[String]) -> i32 {
        0
    }

    /// comptags - manage completion tags
    fn builtin_comptags(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            return 1;
        }
        match args[0].as_str() {
            "-i" => {
                // Initialize tags
                0
            }
            "-S" => {
                // Set tags
                0
            }
            _ => 1,
        }
    }

    /// comptry - try completion
    fn builtin_comptry(&mut self, _args: &[String]) -> i32 {
        1 // No match
    }

    /// compvalues - complete values
    fn builtin_compvalues(&mut self, _args: &[String]) -> i32 {
        0
    }

    /// cap/getcap/setcap - Linux capabilities (stub on macOS)
    fn builtin_cap(&self, args: &[String]) -> i32 {
        // Linux capabilities are not available on macOS
        // On Linux, these would interact with libcap
        if args.is_empty() {
            println!("cap: display/set capabilities");
            println!("  getcap file...  - display capabilities");
            println!("  setcap caps file - set capabilities");
            return 0;
        }

        #[cfg(target_os = "linux")]
        {
            // On Linux, we could use libcap bindings
            // For now, just run the external commands
            let status = std::process::Command::new(&args[0])
                .args(&args[1..])
                .status();
            return status.map(|s| s.code().unwrap_or(1)).unwrap_or(1);
        }

        #[cfg(not(target_os = "linux"))]
        {
            eprintln!("cap: capabilities not supported on this platform");
            1
        }
    }

    /// zcurses - curses interface (stub)
    fn builtin_zcurses(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            eprintln!("zcurses: requires subcommand");
            return 1;
        }

        match args[0].as_str() {
            "init" => {
                // Initialize curses
                println!("zcurses: would initialize curses");
                0
            }
            "end" => {
                // End curses mode
                println!("zcurses: would end curses");
                0
            }
            "addwin" => {
                // Add a window
                0
            }
            "delwin" => {
                // Delete a window
                0
            }
            "refresh" => {
                // Refresh display
                0
            }
            "move" => {
                // Move cursor
                0
            }
            "clear" => {
                // Clear window
                0
            }
            "char" | "string" => {
                // Output character/string
                0
            }
            "border" => {
                // Draw border
                0
            }
            "attr" => {
                // Set attributes
                0
            }
            "color" => {
                // Set colors
                0
            }
            "scroll" => {
                // Scroll window
                0
            }
            "input" => {
                // Get input
                0
            }
            "mouse" => {
                // Mouse support
                0
            }
            "querychar" => {
                // Query character at position
                0
            }
            "resize" => {
                // Resize window
                0
            }
            cmd => {
                eprintln!("zcurses: unknown subcommand: {}", cmd);
                1
            }
        }
    }

    /// sysread - low-level read (zsh/system module)
    fn builtin_sysread(&mut self, args: &[String]) -> i32 {
        use std::io::Read;

        let mut fd = 0i32; // stdin
        let mut count: Option<usize> = None;
        let mut var_name = "REPLY".to_string();
        let mut i = 0;

        while i < args.len() {
            match args[i].as_str() {
                "-c" if i + 1 < args.len() => {
                    i += 1;
                    count = args[i].parse().ok();
                }
                "-i" if i + 1 < args.len() => {
                    i += 1;
                    fd = args[i].parse().unwrap_or(0);
                }
                "-o" if i + 1 < args.len() => {
                    i += 1;
                    var_name = args[i].clone();
                }
                "-t" if i + 1 < args.len() => {
                    i += 1;
                    // Timeout - ignored for now
                }
                _ => {
                    var_name = args[i].clone();
                }
            }
            i += 1;
        }

        let mut buffer = vec![0u8; count.unwrap_or(8192)];

        // Only support stdin for now
        if fd == 0 {
            match std::io::stdin().read(&mut buffer) {
                Ok(n) => {
                    buffer.truncate(n);
                    let s = String::from_utf8_lossy(&buffer).to_string();
                    self.variables.insert(var_name, s);
                    0
                }
                Err(_) => 1,
            }
        } else {
            eprintln!("sysread: only fd 0 (stdin) supported");
            1
        }
    }

    /// syswrite - low-level write (zsh/system module)
    fn builtin_syswrite(&mut self, args: &[String]) -> i32 {
        use std::io::Write;

        let mut fd = 1i32; // stdout
        let mut data = String::new();
        let mut i = 0;

        while i < args.len() {
            match args[i].as_str() {
                "-o" if i + 1 < args.len() => {
                    i += 1;
                    fd = args[i].parse().unwrap_or(1);
                }
                "-c" if i + 1 < args.len() => {
                    i += 1;
                    // Count - ignored
                }
                _ => {
                    data = args[i].clone();
                }
            }
            i += 1;
        }

        match fd {
            1 => {
                let _ = std::io::stdout().write_all(data.as_bytes());
                let _ = std::io::stdout().flush();
                0
            }
            2 => {
                let _ = std::io::stderr().write_all(data.as_bytes());
                let _ = std::io::stderr().flush();
                0
            }
            _ => {
                eprintln!("syswrite: only fd 1 (stdout) and 2 (stderr) supported");
                1
            }
        }
    }

    /// syserror - get error message (zsh/system module)
    fn builtin_syserror(&self, args: &[String]) -> i32 {
        let errno = if args.is_empty() {
            // Use last errno
            std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
        } else {
            args[0].parse().unwrap_or(0)
        };

        let err = std::io::Error::from_raw_os_error(errno);
        println!("{}", err);
        0
    }

    /// sysopen - open file descriptor (zsh/system module)
    fn builtin_sysopen(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            eprintln!("sysopen: need filename");
            return 1;
        }
        // Stub - would need fd management
        eprintln!("sysopen: not fully implemented");
        0
    }

    /// sysseek - seek on file descriptor (zsh/system module)
    fn builtin_sysseek(&mut self, args: &[String]) -> i32 {
        if args.len() < 2 {
            eprintln!("sysseek: need fd and offset");
            return 1;
        }
        // Stub
        eprintln!("sysseek: not fully implemented");
        0
    }

    /// private - declare private variables (zsh/param/private module)
    fn builtin_private(&mut self, args: &[String]) -> i32 {
        // Similar to local but with stricter scoping
        self.builtin_local(args)
    }

    /// zgetattr/zsetattr/zdelattr/zlistattr - extended attributes (zsh/attr module)
    fn builtin_zattr(&self, cmd: &str, args: &[String]) -> i32 {
        match cmd {
            "zgetattr" => {
                if args.len() < 2 {
                    eprintln!("zgetattr: need file and attribute name");
                    return 1;
                }
                #[cfg(target_os = "macos")]
                {
                    // macOS uses xattr
                    let output = std::process::Command::new("xattr")
                        .arg("-p")
                        .arg(&args[1])
                        .arg(&args[0])
                        .output();
                    if let Ok(out) = output {
                        print!("{}", String::from_utf8_lossy(&out.stdout));
                        return if out.status.success() { 0 } else { 1 };
                    }
                }
                #[cfg(target_os = "linux")]
                {
                    let output = std::process::Command::new("getfattr")
                        .arg("-n")
                        .arg(&args[1])
                        .arg(&args[0])
                        .output();
                    if let Ok(out) = output {
                        print!("{}", String::from_utf8_lossy(&out.stdout));
                        return if out.status.success() { 0 } else { 1 };
                    }
                }
                1
            }
            "zsetattr" => {
                if args.len() < 3 {
                    eprintln!("zsetattr: need file, attribute name, and value");
                    return 1;
                }
                #[cfg(target_os = "macos")]
                {
                    let status = std::process::Command::new("xattr")
                        .arg("-w")
                        .arg(&args[1])
                        .arg(&args[2])
                        .arg(&args[0])
                        .status();
                    return status.map(|s| if s.success() { 0 } else { 1 }).unwrap_or(1);
                }
                #[cfg(target_os = "linux")]
                {
                    let status = std::process::Command::new("setfattr")
                        .arg("-n")
                        .arg(&args[1])
                        .arg("-v")
                        .arg(&args[2])
                        .arg(&args[0])
                        .status();
                    return status.map(|s| if s.success() { 0 } else { 1 }).unwrap_or(1);
                }
                #[allow(unreachable_code)]
                1
            }
            "zdelattr" => {
                if args.len() < 2 {
                    eprintln!("zdelattr: need file and attribute name");
                    return 1;
                }
                #[cfg(target_os = "macos")]
                {
                    let status = std::process::Command::new("xattr")
                        .arg("-d")
                        .arg(&args[1])
                        .arg(&args[0])
                        .status();
                    return status.map(|s| if s.success() { 0 } else { 1 }).unwrap_or(1);
                }
                #[cfg(target_os = "linux")]
                {
                    let status = std::process::Command::new("setfattr")
                        .arg("-x")
                        .arg(&args[1])
                        .arg(&args[0])
                        .status();
                    return status.map(|s| if s.success() { 0 } else { 1 }).unwrap_or(1);
                }
                #[allow(unreachable_code)]
                1
            }
            "zlistattr" => {
                if args.is_empty() {
                    eprintln!("zlistattr: need file");
                    return 1;
                }
                #[cfg(target_os = "macos")]
                {
                    let output = std::process::Command::new("xattr").arg(&args[0]).output();
                    if let Ok(out) = output {
                        print!("{}", String::from_utf8_lossy(&out.stdout));
                        return if out.status.success() { 0 } else { 1 };
                    }
                }
                #[cfg(target_os = "linux")]
                {
                    let output = std::process::Command::new("getfattr")
                        .arg("-d")
                        .arg(&args[0])
                        .output();
                    if let Ok(out) = output {
                        print!("{}", String::from_utf8_lossy(&out.stdout));
                        return if out.status.success() { 0 } else { 1 };
                    }
                }
                1
            }
            _ => 1,
        }
    }

    /// zftp - FTP client builtin
    fn builtin_zftp(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            println!("zftp: FTP client");
            println!("  zftp open host [port]");
            println!("  zftp login [user [password]]");
            println!("  zftp cd dir");
            println!("  zftp get file [localfile]");
            println!("  zftp put file [remotefile]");
            println!("  zftp ls [dir]");
            println!("  zftp close");
            return 0;
        }

        match args[0].as_str() {
            "open" => {
                if args.len() < 2 {
                    eprintln!("zftp open: need hostname");
                    return 1;
                }
                // Would connect to FTP server
                println!("zftp: would connect to {}", args[1]);
                0
            }
            "login" => {
                // Would authenticate
                println!("zftp: would login");
                0
            }
            "cd" => {
                if args.len() < 2 {
                    eprintln!("zftp cd: need directory");
                    return 1;
                }
                println!("zftp: would cd to {}", args[1]);
                0
            }
            "get" => {
                if args.len() < 2 {
                    eprintln!("zftp get: need filename");
                    return 1;
                }
                println!("zftp: would download {}", args[1]);
                0
            }
            "put" => {
                if args.len() < 2 {
                    eprintln!("zftp put: need filename");
                    return 1;
                }
                println!("zftp: would upload {}", args[1]);
                0
            }
            "ls" => {
                println!("zftp: would list directory");
                0
            }
            "close" | "quit" => {
                println!("zftp: would close connection");
                0
            }
            "params" => {
                // Display/set FTP parameters
                println!("ZFTP_HOST=");
                println!("ZFTP_PORT=21");
                println!("ZFTP_USER=");
                println!("ZFTP_PWD=");
                println!("ZFTP_TYPE=A");
                0
            }
            cmd => {
                eprintln!("zftp: unknown command: {}", cmd);
                1
            }
        }
    }
}
