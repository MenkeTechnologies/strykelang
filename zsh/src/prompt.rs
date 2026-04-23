//! Prompt expansion for zshrs
//!
//! Direct port from zsh/Src/prompt.c
//!
//! Supports zsh prompt escape sequences:
//! - %d, %/, %~ - current directory
//! - %c, %., %C - trailing path components
//! - %n - username
//! - %m, %M - hostname
//! - %l - tty name
//! - %? - exit status
//! - %# - privilege indicator
//! - %h, %! - history number
//! - %j - number of jobs
//! - %L - shell level
//! - %D, %T, %t, %*, %w, %W - date/time
//! - %B, %b - bold on/off
//! - %U, %u - underline on/off
//! - %S, %s - standout on/off
//! - %F{color}, %f - foreground color
//! - %K{color}, %k - background color
//! - %{ %}  - literal escape sequences
//! - %(x.true.false) - conditional

use std::env;

/// Parser states for %_ in prompts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmdState {
    For,
    While,
    Repeat,
    Select,
    Until,
    If,
    Then,
    Else,
    Elif,
    Math,
    Cond,
    CmdOr,
    CmdAnd,
    Pipe,
    ErrPipe,
    Foreach,
    Case,
    Function,
    Subsh,
    Cursh,
    Array,
    Quote,
    DQuote,
    BQuote,
    CmdSubst,
    MathSubst,
    ElifThen,
    Heredoc,
    HeredocD,
    Brace,
    BraceParam,
    Always,
}

impl CmdState {
    pub fn name(&self) -> &'static str {
        match self {
            CmdState::For => "for",
            CmdState::While => "while",
            CmdState::Repeat => "repeat",
            CmdState::Select => "select",
            CmdState::Until => "until",
            CmdState::If => "if",
            CmdState::Then => "then",
            CmdState::Else => "else",
            CmdState::Elif => "elif",
            CmdState::Math => "math",
            CmdState::Cond => "cond",
            CmdState::CmdOr => "cmdor",
            CmdState::CmdAnd => "cmdand",
            CmdState::Pipe => "pipe",
            CmdState::ErrPipe => "errpipe",
            CmdState::Foreach => "foreach",
            CmdState::Case => "case",
            CmdState::Function => "function",
            CmdState::Subsh => "subsh",
            CmdState::Cursh => "cursh",
            CmdState::Array => "array",
            CmdState::Quote => "quote",
            CmdState::DQuote => "dquote",
            CmdState::BQuote => "bquote",
            CmdState::CmdSubst => "cmdsubst",
            CmdState::MathSubst => "mathsubst",
            CmdState::ElifThen => "elif-then",
            CmdState::Heredoc => "heredoc",
            CmdState::HeredocD => "heredocd",
            CmdState::Brace => "brace",
            CmdState::BraceParam => "braceparam",
            CmdState::Always => "always",
        }
    }
}

/// Text attributes for prompt formatting
#[derive(Debug, Clone, Copy, Default)]
pub struct TextAttrs {
    pub bold: bool,
    pub underline: bool,
    pub standout: bool,
    pub fg_color: Option<Color>,
    pub bg_color: Option<Color>,
}

/// Color specification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    Default,
    Numbered(u8),
    Rgb(u8, u8, u8),
}

impl Color {
    pub fn from_name(name: &str) -> Option<Color> {
        match name.to_lowercase().as_str() {
            "black" => Some(Color::Black),
            "red" => Some(Color::Red),
            "green" => Some(Color::Green),
            "yellow" => Some(Color::Yellow),
            "blue" => Some(Color::Blue),
            "magenta" => Some(Color::Magenta),
            "cyan" => Some(Color::Cyan),
            "white" => Some(Color::White),
            "default" => Some(Color::Default),
            _ => {
                if let Ok(n) = name.parse::<u8>() {
                    Some(Color::Numbered(n))
                } else {
                    None
                }
            }
        }
    }

    pub fn to_ansi_fg(&self) -> String {
        match self {
            Color::Black => "\x1b[30m".to_string(),
            Color::Red => "\x1b[31m".to_string(),
            Color::Green => "\x1b[32m".to_string(),
            Color::Yellow => "\x1b[33m".to_string(),
            Color::Blue => "\x1b[34m".to_string(),
            Color::Magenta => "\x1b[35m".to_string(),
            Color::Cyan => "\x1b[36m".to_string(),
            Color::White => "\x1b[37m".to_string(),
            Color::Default => "\x1b[39m".to_string(),
            Color::Numbered(n) => format!("\x1b[38;5;{}m", n),
            Color::Rgb(r, g, b) => format!("\x1b[38;2;{};{};{}m", r, g, b),
        }
    }

    pub fn to_ansi_bg(&self) -> String {
        match self {
            Color::Black => "\x1b[40m".to_string(),
            Color::Red => "\x1b[41m".to_string(),
            Color::Green => "\x1b[42m".to_string(),
            Color::Yellow => "\x1b[43m".to_string(),
            Color::Blue => "\x1b[44m".to_string(),
            Color::Magenta => "\x1b[45m".to_string(),
            Color::Cyan => "\x1b[46m".to_string(),
            Color::White => "\x1b[47m".to_string(),
            Color::Default => "\x1b[49m".to_string(),
            Color::Numbered(n) => format!("\x1b[48;5;{}m", n),
            Color::Rgb(r, g, b) => format!("\x1b[48;2;{};{};{}m", r, g, b),
        }
    }
}

/// Context for prompt expansion
pub struct PromptContext {
    pub pwd: String,
    pub home: String,
    pub user: String,
    pub host: String,
    pub host_short: String,
    pub tty: String,
    pub lastval: i32,
    pub histnum: i64,
    pub shlvl: i32,
    pub num_jobs: i32,
    pub is_root: bool,
    pub cmd_stack: Vec<CmdState>,
    pub psvar: Vec<String>,
    pub term_width: usize,
    pub lineno: i64,
}

impl Default for PromptContext {
    fn default() -> Self {
        let home = env::var("HOME").unwrap_or_default();
        let pwd = env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "/".to_string());

        let user = env::var("USER")
            .or_else(|_| env::var("LOGNAME"))
            .unwrap_or_else(|_| "user".to_string());

        let host = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "localhost".to_string());

        let host_short = host.split('.').next().unwrap_or(&host).to_string();

        let tty = std::fs::read_link("/proc/self/fd/0")
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| String::new());

        let shlvl = env::var("SHLVL")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);

        PromptContext {
            pwd,
            home,
            user,
            host,
            host_short,
            tty,
            lastval: 0,
            histnum: 1,
            shlvl,
            num_jobs: 0,
            is_root: unsafe { libc::geteuid() } == 0,
            cmd_stack: Vec::new(),
            psvar: Vec::new(),
            term_width: 80,
            lineno: 1,
        }
    }
}

/// Prompt expander
pub struct PromptExpander<'a> {
    ctx: &'a PromptContext,
    input: &'a str,
    pos: usize,
    output: String,
    attrs: TextAttrs,
    in_escape: bool,
    prompt_percent: bool,
    prompt_bang: bool,
}

impl<'a> PromptExpander<'a> {
    pub fn new(input: &'a str, ctx: &'a PromptContext) -> Self {
        PromptExpander {
            ctx,
            input,
            pos: 0,
            output: String::with_capacity(input.len() * 2),
            attrs: TextAttrs::default(),
            in_escape: false,
            prompt_percent: true,
            prompt_bang: true,
        }
    }

    pub fn with_prompt_percent(mut self, enable: bool) -> Self {
        self.prompt_percent = enable;
        self
    }

    pub fn with_prompt_bang(mut self, enable: bool) -> Self {
        self.prompt_bang = enable;
        self
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn parse_number(&mut self) -> Option<i32> {
        let start = self.pos;
        let mut negative = false;

        if self.peek() == Some('-') {
            negative = true;
            self.advance();
        }

        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.advance();
            } else {
                break;
            }
        }

        if self.pos == start || (negative && self.pos == start + 1) {
            if negative {
                self.pos = start;
            }
            return None;
        }

        let num_str = &self.input[if negative { start + 1 } else { start }..self.pos];
        let num: i32 = num_str.parse().ok()?;
        Some(if negative { -num } else { num })
    }

    fn parse_braced_arg(&mut self) -> Option<String> {
        if self.peek() != Some('{') {
            return None;
        }
        self.advance(); // skip {

        let start = self.pos;
        let mut depth = 1;

        while let Some(c) = self.advance() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(self.input[start..self.pos - 1].to_string());
                    }
                }
                '\\' => {
                    self.advance(); // skip escaped char
                }
                _ => {}
            }
        }

        None
    }

    /// Get path with tilde substitution
    fn path_with_tilde(&self, path: &str) -> String {
        if !self.ctx.home.is_empty() && path.starts_with(&self.ctx.home) {
            format!("~{}", &path[self.ctx.home.len()..])
        } else {
            path.to_string()
        }
    }

    /// Get trailing path components
    fn trailing_path(&self, path: &str, n: usize, with_tilde: bool) -> String {
        let path = if with_tilde {
            self.path_with_tilde(path)
        } else {
            path.to_string()
        };

        if n == 0 {
            return path;
        }

        let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if components.len() <= n {
            return path;
        }

        components[components.len() - n..].join("/")
    }

    /// Get leading path components
    fn leading_path(&self, path: &str, n: usize) -> String {
        if n == 0 {
            return path.to_string();
        }

        let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if components.len() <= n {
            return path.to_string();
        }

        let result = components[..n].join("/");
        if path.starts_with('/') {
            format!("/{}", result)
        } else {
            result
        }
    }

    /// Start escape sequence (non-printing characters)
    fn start_escape(&mut self) {
        if !self.in_escape {
            self.output.push('\x01'); // RL_PROMPT_START_IGNORE
            self.in_escape = true;
        }
    }

    /// End escape sequence
    fn end_escape(&mut self) {
        if self.in_escape {
            self.output.push('\x02'); // RL_PROMPT_END_IGNORE
            self.in_escape = false;
        }
    }

    /// Apply text attributes
    fn apply_attrs(&mut self) {
        self.start_escape();

        // Reset first
        self.output.push_str("\x1b[0m");

        if self.attrs.bold {
            self.output.push_str("\x1b[1m");
        }
        if self.attrs.underline {
            self.output.push_str("\x1b[4m");
        }
        if self.attrs.standout {
            self.output.push_str("\x1b[7m");
        }
        if let Some(ref color) = self.attrs.fg_color {
            self.output.push_str(&color.to_ansi_fg());
        }
        if let Some(ref color) = self.attrs.bg_color {
            self.output.push_str(&color.to_ansi_bg());
        }

        self.end_escape();
    }

    /// Parse conditional %(x.true.false)
    fn parse_conditional(&mut self, arg: i32) -> bool {
        if self.peek() != Some('(') {
            return false;
        }
        self.advance(); // skip (

        // Parse condition character
        let cond_char = match self.advance() {
            Some(c) => c,
            None => return false,
        };

        // Evaluate condition
        let test = match cond_char {
            '/' | 'c' | '.' | '~' | 'C' => {
                // Directory depth test
                let path = self.path_with_tilde(&self.ctx.pwd);
                let depth = path.matches('/').count() as i32;
                if arg == 0 { depth > 0 } else { depth >= arg }
            }
            '?' => self.ctx.lastval == arg,
            '#' => {
                let euid = unsafe { libc::geteuid() };
                euid == arg as u32
            }
            'L' => self.ctx.shlvl >= arg,
            'j' => self.ctx.num_jobs >= arg,
            'v' => (arg as usize) <= self.ctx.psvar.len(),
            'V' => {
                if arg <= 0 || (arg as usize) > self.ctx.psvar.len() {
                    false
                } else {
                    !self.ctx.psvar[arg as usize - 1].is_empty()
                }
            }
            '_' => self.ctx.cmd_stack.len() >= arg as usize,
            't' | 'T' | 'd' | 'D' | 'w' => {
                let now = chrono::Local::now();
                match cond_char {
                    't' => now.format("%M").to_string().parse::<i32>().unwrap_or(0) == arg,
                    'T' => now.format("%H").to_string().parse::<i32>().unwrap_or(0) == arg,
                    'd' => now.format("%d").to_string().parse::<i32>().unwrap_or(0) == arg,
                    'D' => now.format("%m").to_string().parse::<i32>().unwrap_or(0) == arg - 1,
                    'w' => now.format("%w").to_string().parse::<i32>().unwrap_or(0) == arg,
                    _ => false,
                }
            }
            '!' => self.ctx.is_root,
            _ => false,
        };

        // Get separator
        let sep = match self.advance() {
            Some(c) => c,
            None => return false,
        };

        // Parse true branch
        let true_start = self.pos;
        let mut depth = 1;
        while let Some(c) = self.peek() {
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            } else if c == sep && depth == 1 {
                break;
            }
            self.advance();
        }
        let true_branch = &self.input[true_start..self.pos].to_string();

        if self.peek() != Some(sep) {
            return false;
        }
        self.advance(); // skip separator

        // Parse false branch
        let false_start = self.pos;
        depth = 1;
        while let Some(c) = self.peek() {
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            self.advance();
        }
        let false_branch = &self.input[false_start..self.pos].to_string();

        if self.peek() != Some(')') {
            return false;
        }
        self.advance(); // skip )

        // Expand the appropriate branch
        let branch = if test { true_branch } else { false_branch };
        let expanded = expand_prompt(branch, self.ctx);
        self.output.push_str(&expanded);

        true
    }

    /// Parse and process a % escape sequence
    fn process_percent(&mut self) {
        let arg = self.parse_number().unwrap_or(0);

        // Check for conditional
        if self.peek() == Some('(') {
            self.parse_conditional(arg);
            return;
        }

        let c = match self.advance() {
            Some(c) => c,
            None => return,
        };

        match c {
            // Directory
            '~' => {
                let path = if arg == 0 {
                    self.path_with_tilde(&self.ctx.pwd)
                } else if arg > 0 {
                    self.trailing_path(&self.ctx.pwd, arg as usize, true)
                } else {
                    self.leading_path(&self.path_with_tilde(&self.ctx.pwd), (-arg) as usize)
                };
                self.output.push_str(&path);
            }
            'd' | '/' => {
                let path = if arg == 0 {
                    self.ctx.pwd.clone()
                } else if arg > 0 {
                    self.trailing_path(&self.ctx.pwd, arg as usize, false)
                } else {
                    self.leading_path(&self.ctx.pwd, (-arg) as usize)
                };
                self.output.push_str(&path);
            }
            'c' | '.' => {
                let n = if arg == 0 { 1 } else { arg.unsigned_abs() as usize };
                let path = self.trailing_path(&self.ctx.pwd, n, true);
                self.output.push_str(&path);
            }
            'C' => {
                let n = if arg == 0 { 1 } else { arg.unsigned_abs() as usize };
                let path = self.trailing_path(&self.ctx.pwd, n, false);
                self.output.push_str(&path);
            }

            // User/host
            'n' => self.output.push_str(&self.ctx.user),
            'M' => self.output.push_str(&self.ctx.host),
            'm' => {
                let n = if arg == 0 { 1 } else { arg };
                if n > 0 {
                    let parts: Vec<&str> = self.ctx.host.split('.').collect();
                    let take = (n as usize).min(parts.len());
                    self.output.push_str(&parts[..take].join("."));
                } else {
                    let parts: Vec<&str> = self.ctx.host.split('.').collect();
                    let skip = ((-n) as usize).min(parts.len());
                    self.output.push_str(&parts[skip..].join("."));
                }
            }

            // TTY
            'l' => {
                let tty = if self.ctx.tty.starts_with("/dev/tty") {
                    &self.ctx.tty[8..]
                } else if self.ctx.tty.starts_with("/dev/") {
                    &self.ctx.tty[5..]
                } else {
                    "()"
                };
                self.output.push_str(tty);
            }
            'y' => {
                let tty = if self.ctx.tty.starts_with("/dev/") {
                    &self.ctx.tty[5..]
                } else {
                    &self.ctx.tty
                };
                self.output.push_str(tty);
            }

            // Status
            '?' => self.output.push_str(&self.ctx.lastval.to_string()),
            '#' => self.output.push(if self.ctx.is_root { '#' } else { '%' }),

            // History
            'h' | '!' => self.output.push_str(&self.ctx.histnum.to_string()),

            // Jobs
            'j' => self.output.push_str(&self.ctx.num_jobs.to_string()),

            // Shell level
            'L' => self.output.push_str(&self.ctx.shlvl.to_string()),

            // Line number
            'i' => self.output.push_str(&self.ctx.lineno.to_string()),

            // Date/time
            'D' => {
                let now = chrono::Local::now();
                if let Some(fmt) = self.parse_braced_arg() {
                    let zsh_fmt = convert_zsh_time_format(&fmt);
                    self.output.push_str(&now.format(&zsh_fmt).to_string());
                } else {
                    self.output.push_str(&now.format("%y-%m-%d").to_string());
                }
            }
            'T' => {
                let now = chrono::Local::now();
                self.output.push_str(&now.format("%H:%M").to_string());
            }
            '*' => {
                let now = chrono::Local::now();
                self.output.push_str(&now.format("%H:%M:%S").to_string());
            }
            't' | '@' => {
                let now = chrono::Local::now();
                self.output.push_str(&now.format("%l:%M%p").to_string());
            }
            'w' => {
                let now = chrono::Local::now();
                self.output.push_str(&now.format("%a %e").to_string());
            }
            'W' => {
                let now = chrono::Local::now();
                self.output.push_str(&now.format("%m/%d/%y").to_string());
            }

            // Text attributes
            'B' => {
                self.attrs.bold = true;
                self.apply_attrs();
            }
            'b' => {
                self.attrs.bold = false;
                self.apply_attrs();
            }
            'U' => {
                self.attrs.underline = true;
                self.apply_attrs();
            }
            'u' => {
                self.attrs.underline = false;
                self.apply_attrs();
            }
            'S' => {
                self.attrs.standout = true;
                self.apply_attrs();
            }
            's' => {
                self.attrs.standout = false;
                self.apply_attrs();
            }

            // Colors
            'F' => {
                let color = if let Some(name) = self.parse_braced_arg() {
                    Color::from_name(&name)
                } else if arg > 0 {
                    Some(Color::Numbered(arg as u8))
                } else {
                    None
                };
                if let Some(c) = color {
                    self.attrs.fg_color = Some(c);
                    self.apply_attrs();
                }
            }
            'f' => {
                self.attrs.fg_color = None;
                self.apply_attrs();
            }
            'K' => {
                let color = if let Some(name) = self.parse_braced_arg() {
                    Color::from_name(&name)
                } else if arg > 0 {
                    Some(Color::Numbered(arg as u8))
                } else {
                    None
                };
                if let Some(c) = color {
                    self.attrs.bg_color = Some(c);
                    self.apply_attrs();
                }
            }
            'k' => {
                self.attrs.bg_color = None;
                self.apply_attrs();
            }

            // Literal escape sequences
            '{' => self.start_escape(),
            '}' => self.end_escape(),

            // Glitch space
            'G' => {
                let n = if arg > 0 { arg as usize } else { 1 };
                for _ in 0..n {
                    self.output.push(' ');
                }
            }

            // psvar
            'v' => {
                let idx = if arg == 0 { 1 } else { arg };
                if idx > 0 && (idx as usize) <= self.ctx.psvar.len() {
                    self.output.push_str(&self.ctx.psvar[idx as usize - 1]);
                }
            }

            // Command stack
            '_' => {
                if !self.ctx.cmd_stack.is_empty() {
                    let n = if arg == 0 {
                        self.ctx.cmd_stack.len()
                    } else if arg > 0 {
                        (arg as usize).min(self.ctx.cmd_stack.len())
                    } else {
                        ((-arg) as usize).min(self.ctx.cmd_stack.len())
                    };

                    let names: Vec<&str> = if arg >= 0 {
                        self.ctx.cmd_stack.iter().rev().take(n).map(|s| s.name()).collect()
                    } else {
                        self.ctx.cmd_stack.iter().take(n).map(|s| s.name()).collect()
                    };
                    self.output.push_str(&names.join(" "));
                }
            }

            // Clear to end of line
            'E' => {
                self.start_escape();
                self.output.push_str("\x1b[K");
                self.end_escape();
            }

            // Literal characters
            '%' => self.output.push('%'),
            ')' => self.output.push(')'),
            '\0' => {}

            // Unknown - output literally
            _ => {
                self.output.push('%');
                self.output.push(c);
            }
        }
    }

    /// Expand the prompt
    pub fn expand(mut self) -> String {
        while let Some(c) = self.advance() {
            if c == '%' && self.prompt_percent {
                self.process_percent();
            } else if c == '!' && self.prompt_bang {
                if self.peek() == Some('!') {
                    self.advance();
                    self.output.push('!');
                } else {
                    self.output.push_str(&self.ctx.histnum.to_string());
                }
            } else {
                self.output.push(c);
            }
        }

        // Reset attributes at end
        if self.attrs.bold || self.attrs.underline || self.attrs.standout
            || self.attrs.fg_color.is_some() || self.attrs.bg_color.is_some()
        {
            self.start_escape();
            self.output.push_str("\x1b[0m");
            self.end_escape();
        }

        self.output
    }
}

/// Convert zsh time format to chrono format
fn convert_zsh_time_format(fmt: &str) -> String {
    let mut result = String::new();
    let mut chars = fmt.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('a') => result.push_str("%a"), // weekday abbrev
                Some('A') => result.push_str("%A"), // weekday full
                Some('b') | Some('h') => result.push_str("%b"), // month abbrev
                Some('B') => result.push_str("%B"), // month full
                Some('c') => result.push_str("%c"), // locale datetime
                Some('C') => result.push_str("%y"), // century (use year for simplicity)
                Some('d') => result.push_str("%d"), // day of month
                Some('D') => result.push_str("%m/%d/%y"), // date
                Some('e') => result.push_str("%e"), // day of month, space padded
                Some('f') => result.push_str("%e"), // zsh: day of month, no padding
                Some('F') => result.push_str("%Y-%m-%d"), // ISO date
                Some('H') => result.push_str("%H"), // hour 24
                Some('I') => result.push_str("%I"), // hour 12
                Some('j') => result.push_str("%j"), // day of year
                Some('k') => result.push_str("%k"), // hour 24, space padded
                Some('K') => result.push_str("%H"), // zsh: hour 24
                Some('l') => result.push_str("%l"), // hour 12, space padded
                Some('L') => result.push_str("%3f"),// zsh: milliseconds (approx)
                Some('m') => result.push_str("%m"), // month
                Some('M') => result.push_str("%M"), // minute
                Some('n') => result.push('\n'),
                Some('N') => result.push_str("%9f"),// zsh: nanoseconds (approx)
                Some('p') => result.push_str("%p"), // AM/PM
                Some('P') => result.push_str("%P"), // am/pm
                Some('r') => result.push_str("%r"), // 12-hour time
                Some('R') => result.push_str("%R"), // 24-hour time
                Some('s') => result.push_str("%s"), // epoch seconds
                Some('S') => result.push_str("%S"), // seconds
                Some('t') => result.push('\t'),
                Some('T') => result.push_str("%T"), // time
                Some('u') => result.push_str("%u"), // weekday 1-7
                Some('U') => result.push_str("%U"), // week of year (Sunday)
                Some('V') => result.push_str("%V"), // ISO week
                Some('w') => result.push_str("%w"), // weekday 0-6
                Some('W') => result.push_str("%W"), // week of year (Monday)
                Some('x') => result.push_str("%x"), // locale date
                Some('X') => result.push_str("%X"), // locale time
                Some('y') => result.push_str("%y"), // year 2-digit
                Some('Y') => result.push_str("%Y"), // year 4-digit
                Some('z') => result.push_str("%z"), // timezone offset
                Some('Z') => result.push_str("%Z"), // timezone name
                Some('%') => result.push('%'),
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

/// Expand a prompt string
pub fn expand_prompt(s: &str, ctx: &PromptContext) -> String {
    PromptExpander::new(s, ctx).expand()
}

/// Expand a prompt string with default context
pub fn expand_prompt_default(s: &str) -> String {
    let ctx = PromptContext::default();
    expand_prompt(s, &ctx)
}

/// Count the visible width of an expanded prompt (ignoring escape sequences)
pub fn prompt_width(s: &str) -> usize {
    let mut width = 0;
    let mut in_escape = false;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\x01' => in_escape = true,  // RL_PROMPT_START_IGNORE
            '\x02' => in_escape = false, // RL_PROMPT_END_IGNORE
            '\x1b' => {
                // ANSI escape - skip until 'm' or end
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next == 'm' {
                        break;
                    }
                }
            }
            _ if !in_escape => {
                width += unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
            }
            _ => {}
        }
    }

    width
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> PromptContext {
        PromptContext {
            pwd: "/home/user/projects/test".to_string(),
            home: "/home/user".to_string(),
            user: "testuser".to_string(),
            host: "myhost.example.com".to_string(),
            host_short: "myhost".to_string(),
            tty: "/dev/pts/0".to_string(),
            lastval: 0,
            histnum: 42,
            shlvl: 2,
            num_jobs: 1,
            is_root: false,
            cmd_stack: vec![],
            psvar: vec!["one".to_string(), "two".to_string()],
            term_width: 80,
            lineno: 10,
        }
    }

    #[test]
    fn test_directory() {
        let ctx = test_ctx();
        assert_eq!(expand_prompt("%~", &ctx), "~/projects/test");
        assert_eq!(expand_prompt("%/", &ctx), "/home/user/projects/test");
        assert_eq!(expand_prompt("%d", &ctx), "/home/user/projects/test");
        assert_eq!(expand_prompt("%1~", &ctx), "test");
        assert_eq!(expand_prompt("%2~", &ctx), "projects/test");
        assert_eq!(expand_prompt("%c", &ctx), "test");
        assert_eq!(expand_prompt("%2c", &ctx), "projects/test");
    }

    #[test]
    fn test_user_host() {
        let ctx = test_ctx();
        assert_eq!(expand_prompt("%n", &ctx), "testuser");
        assert_eq!(expand_prompt("%M", &ctx), "myhost.example.com");
        assert_eq!(expand_prompt("%m", &ctx), "myhost");
        assert_eq!(expand_prompt("%2m", &ctx), "myhost.example");
    }

    #[test]
    fn test_status() {
        let mut ctx = test_ctx();
        ctx.lastval = 127;
        assert_eq!(expand_prompt("%?", &ctx), "127");
        assert_eq!(expand_prompt("%#", &ctx), "%");
    }

    #[test]
    fn test_history() {
        let ctx = test_ctx();
        assert_eq!(expand_prompt("%h", &ctx), "42");
        assert_eq!(expand_prompt("%!", &ctx), "42");
    }

    #[test]
    fn test_misc() {
        let ctx = test_ctx();
        assert_eq!(expand_prompt("%L", &ctx), "2");
        assert_eq!(expand_prompt("%j", &ctx), "1");
        assert_eq!(expand_prompt("%i", &ctx), "10");
        assert_eq!(expand_prompt("%%", &ctx), "%");
    }

    #[test]
    fn test_psvar() {
        let ctx = test_ctx();
        assert_eq!(expand_prompt("%v", &ctx), "one");
        assert_eq!(expand_prompt("%1v", &ctx), "one");
        assert_eq!(expand_prompt("%2v", &ctx), "two");
        assert_eq!(expand_prompt("%3v", &ctx), ""); // out of bounds
    }

    #[test]
    fn test_conditional() {
        let mut ctx = test_ctx();
        ctx.lastval = 0;
        assert_eq!(expand_prompt("%(?.ok.fail)", &ctx), "ok");
        ctx.lastval = 1;
        assert_eq!(expand_prompt("%(?.ok.fail)", &ctx), "fail");
    }

    #[test]
    fn test_time_format() {
        let fmt = convert_zsh_time_format("%Y-%m-%d %H:%M:%S");
        assert_eq!(fmt, "%Y-%m-%d %H:%M:%S");
    }

    #[test]
    fn test_bang_expansion() {
        let ctx = test_ctx();
        let exp = PromptExpander::new("cmd !!", &ctx).with_prompt_bang(true);
        assert_eq!(exp.expand(), "cmd !");

        let exp2 = PromptExpander::new("cmd !", &ctx).with_prompt_bang(true);
        assert_eq!(exp2.expand(), "cmd 42");
    }
}
