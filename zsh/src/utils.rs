//! Utility functions for zshrs
//!
//! Port from zsh/Src/utils.c
//!
//! Provides miscellaneous utilities: error handling, file operations,
//! string utilities, and character classification.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Script name for error messages
pub static mut SCRIPT_NAME: Option<String> = None;
/// Script filename
pub static mut SCRIPT_FILENAME: Option<String> = None;

/// Print an error message
pub fn zerr(msg: &str) {
    eprintln!("zsh: {}", msg);
}

/// Print an error message with command name
pub fn zerrnam(cmd: &str, msg: &str) {
    eprintln!("{}: {}", cmd, msg);
}

/// Print a warning message
pub fn zwarn(msg: &str) {
    eprintln!("zsh: warning: {}", msg);
}

/// Print a warning with command name  
pub fn zwarnnam(cmd: &str, msg: &str) {
    eprintln!("{}: warning: {}", cmd, msg);
}

/// Print formatted error with optional errno
pub fn zerrmsg(msg: &str, errno: Option<i32>) {
    if let Some(e) = errno {
        let errmsg = std::io::Error::from_raw_os_error(e);
        eprintln!("zsh: {}: {}", msg, errmsg);
    } else {
        eprintln!("zsh: {}", msg);
    }
}

/// Check if a path is a directory
pub fn is_directory(path: &str) -> bool {
    Path::new(path).is_dir()
}

/// Check if a file exists and is executable
pub fn is_executable(path: &str) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mode = meta.permissions().mode();
            return meta.is_file() && (mode & 0o111 != 0);
        }
        false
    }
    #[cfg(not(unix))]
    {
        Path::new(path).is_file()
    }
}

/// Find an executable in PATH
pub fn find_in_path(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let path = PathBuf::from(name);
        if is_executable(name) {
            return Some(path);
        }
        return None;
    }

    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(':') {
            let full_path = PathBuf::from(dir).join(name);
            if let Some(path_str) = full_path.to_str() {
                if is_executable(path_str) {
                    return Some(full_path);
                }
            }
        }
    }
    None
}

/// Expand tilde in a path
pub fn expand_tilde(path: &str) -> String {
    if !path.starts_with('~') {
        return path.to_string();
    }

    let (user, rest) = if let Some(pos) = path[1..].find('/') {
        (&path[1..pos + 1], &path[pos + 1..])
    } else {
        (&path[1..], "")
    };

    if user.is_empty() {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{}{}", home, rest);
        }
    } else {
        #[cfg(unix)]
        {
            if let Some(dir) = get_user_home(user) {
                return format!("{}{}", dir, rest);
            }
        }
    }

    path.to_string()
}

#[cfg(unix)]
fn get_user_home(user: &str) -> Option<String> {
    use std::ffi::CString;
    unsafe {
        let c_user = CString::new(user).ok()?;
        let pw = libc::getpwnam(c_user.as_ptr());
        if pw.is_null() {
            return None;
        }
        let dir = std::ffi::CStr::from_ptr((*pw).pw_dir);
        dir.to_str().ok().map(|s| s.to_string())
    }
}

/// Nicely format a string for display (escape unprintable chars)
pub fn nicechar(c: char) -> String {
    if c.is_ascii_control() {
        match c {
            '\n' => "\\n".to_string(),
            '\t' => "\\t".to_string(),
            '\r' => "\\r".to_string(),
            '\x1b' => "\\e".to_string(),
            _ => format!("^{}", ((c as u8) + 64) as char),
        }
    } else if c == '\x7f' {
        "^?".to_string()
    } else {
        c.to_string()
    }
}

/// Nicely format a string
pub fn nicezputs(s: &str) -> String {
    s.chars().map(nicechar).collect()
}

/// Check if character is a word character
pub fn is_word_char(c: char, wordchars: &str) -> bool {
    c.is_alphanumeric() || wordchars.contains(c)
}

/// Check if character is an IFS character
pub fn is_ifs_char(c: char, ifs: &str) -> bool {
    ifs.contains(c)
}

/// Convert character to lowercase
pub fn tulower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Convert character to uppercase
pub fn tuupper(c: char) -> char {
    c.to_uppercase().next().unwrap_or(c)
}

/// Check if string is a valid identifier
pub fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Check if string looks like a number
pub fn is_number(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    let s = s.strip_prefix('-').or_else(|| s.strip_prefix('+')).unwrap_or(s);
    if s.is_empty() {
        return false;
    }
    s.chars().all(|c| c.is_ascii_digit())
}

/// Check if string is a valid floating point number
pub fn is_float(s: &str) -> bool {
    s.parse::<f64>().is_ok()
}

/// Get monotonic time in nanoseconds
pub fn monotonic_time_ns() -> u64 {
    use std::time::Instant;
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(Instant::now);
    start.elapsed().as_nanos() as u64
}

/// Sleep for a given number of seconds (fractional)
pub fn zsleep(seconds: f64) {
    let duration = std::time::Duration::from_secs_f64(seconds);
    std::thread::sleep(duration);
}

/// Write a string to a file descriptor
pub fn write_to_fd(fd: i32, data: &str) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::io::FromRawFd;
        let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
        write!(file, "{}", data)?;
        std::mem::forget(file); // Don't close the fd
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, data);
        Err(io::Error::new(io::ErrorKind::Unsupported, "Not supported"))
    }
}

/// Move a file descriptor to a high number (>10)
pub fn move_fd(fd: i32) -> i32 {
    #[cfg(unix)]
    {
        if fd < 10 {
            unsafe {
                let newfd = libc::fcntl(fd, libc::F_DUPFD, 10);
                if newfd >= 0 {
                    libc::close(fd);
                    return newfd;
                }
            }
        }
        fd
    }
    #[cfg(not(unix))]
    {
        fd
    }
}

/// Close a file descriptor
pub fn zclose(fd: i32) {
    #[cfg(unix)]
    unsafe {
        libc::close(fd);
    }
}

/// Check if a file descriptor is a tty
pub fn is_tty(fd: i32) -> bool {
    #[cfg(unix)]
    unsafe {
        libc::isatty(fd) != 0
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        false
    }
}

/// Get terminal width
pub fn get_term_width() -> usize {
    #[cfg(unix)]
    {
        unsafe {
            let mut ws: libc::winsize = std::mem::zeroed();
            if libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 {
                return ws.ws_col as usize;
            }
        }
    }
    std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(80)
}

/// Get terminal height
pub fn get_term_height() -> usize {
    #[cfg(unix)]
    {
        unsafe {
            let mut ws: libc::winsize = std::mem::zeroed();
            if libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_row > 0 {
                return ws.ws_row as usize;
            }
        }
    }
    std::env::var("LINES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(24)
}

/// Quote type constants for quotestring()
/// Port from zsh.h QT_* enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteType {
    None = 0,
    Backslash = 1,
    Single = 2,
    Double = 3,
    Dollars = 4,
    Backtick = 5,
    SingleOptional = 6,
    BackslashPattern = 7,
    BackslashShownull = 8,
}

impl QuoteType {
    /// Convert q flag count to QuoteType
    /// (q)=Backslash, (qq)=Single, (qqq)=Double, (qqqq)=Dollars
    pub fn from_q_count(count: u32) -> Self {
        match count {
            0 => QuoteType::None,
            1 => QuoteType::Backslash,
            2 => QuoteType::Single,
            3 => QuoteType::Double,
            _ => QuoteType::Dollars,
        }
    }
}

/// Check if character is special for shell
/// Port from ispecial() macro in zsh.h
fn is_special(c: char) -> bool {
    matches!(c,
        '|' | '&' | ';' | '<' | '>' | '(' | ')' | '$' | '`' | '"' | '\'' | '\\' |
        ' ' | '\t' | '\n' | '=' | '[' | ']' | '*' | '?' | '#' | '~' | '{' | '}' | '!' | '^'
    )
}

/// Check if character is a pattern character
/// Port from ipattern() macro in zsh.h
fn is_pattern(c: char) -> bool {
    matches!(c, '*' | '?' | '[' | ']' | '<' | '>' | '(' | ')' | '|' | '#' | '^' | '~')
}

/// Quote a string according to the specified type
/// Port from zsh/Src/utils.c quotestring() (lines 6141-6452)
pub fn quotestring(s: &str, quote_type: QuoteType) -> String {
    if s.is_empty() {
        return match quote_type {
            QuoteType::None => String::new(),
            QuoteType::BackslashShownull | QuoteType::Backslash => "''".to_string(),
            QuoteType::Single | QuoteType::SingleOptional => "''".to_string(),
            QuoteType::Double => "\"\"".to_string(),
            QuoteType::Dollars => "$''".to_string(),
            QuoteType::BackslashPattern => String::new(),
            QuoteType::Backtick => String::new(),
        };
    }

    match quote_type {
        QuoteType::None => s.to_string(),

        QuoteType::BackslashPattern => {
            // Only quote pattern characters (lines 6242-6247)
            let mut result = String::with_capacity(s.len() * 2);
            for c in s.chars() {
                if is_pattern(c) {
                    result.push('\\');
                }
                result.push(c);
            }
            result
        }

        QuoteType::Backslash | QuoteType::BackslashShownull => {
            // Backslash quoting (lines 6260-6416)
            let mut result = String::with_capacity(s.len() * 2);
            for c in s.chars() {
                if is_special(c) {
                    result.push('\\');
                }
                result.push(c);
            }
            result
        }

        QuoteType::Single => {
            // Single quote: 'string' (lines 6359-6382)
            let mut result = String::with_capacity(s.len() + 4);
            result.push('\'');
            for c in s.chars() {
                if c == '\'' {
                    // End quote, add escaped quote, start new quote
                    result.push_str("'\\''");
                } else if c == '\n' {
                    // Newlines need $'...' quoting
                    result.push_str("'$'\\n''");
                } else {
                    result.push(c);
                }
            }
            result.push('\'');
            result
        }

        QuoteType::SingleOptional => {
            // Only add quotes where necessary (lines 6314-6363)
            let needs_quoting = s.chars().any(|c| is_special(c));
            if !needs_quoting {
                return s.to_string();
            }

            let mut result = String::with_capacity(s.len() + 4);
            let mut in_quotes = false;

            for c in s.chars() {
                if c == '\'' {
                    if in_quotes {
                        result.push('\'');
                        in_quotes = false;
                    }
                    result.push_str("\\'");
                } else if is_special(c) {
                    if !in_quotes {
                        result.push('\'');
                        in_quotes = true;
                    }
                    result.push(c);
                } else {
                    if in_quotes {
                        result.push('\'');
                        in_quotes = false;
                    }
                    result.push(c);
                }
            }
            if in_quotes {
                result.push('\'');
            }
            result
        }

        QuoteType::Double => {
            // Double quote: "string" (lines 6272-6280, 6311-6312)
            let mut result = String::with_capacity(s.len() + 4);
            result.push('"');
            for c in s.chars() {
                if matches!(c, '$' | '`' | '"' | '\\') {
                    result.push('\\');
                }
                result.push(c);
            }
            result.push('"');
            result
        }

        QuoteType::Dollars => {
            // $'...' quoting with escape sequences (lines 6203-6241)
            let mut result = String::with_capacity(s.len() + 4);
            result.push_str("$'");
            for c in s.chars() {
                match c {
                    '\\' | '\'' => {
                        result.push('\\');
                        result.push(c);
                    }
                    '\n' => result.push_str("\\n"),
                    '\r' => result.push_str("\\r"),
                    '\t' => result.push_str("\\t"),
                    '\x1b' => result.push_str("\\e"),
                    '\x07' => result.push_str("\\a"),
                    '\x08' => result.push_str("\\b"),
                    '\x0c' => result.push_str("\\f"),
                    '\x0b' => result.push_str("\\v"),
                    c if c.is_ascii_control() => {
                        // Octal escape for control characters
                        result.push_str(&format!("\\{:03o}", c as u8));
                    }
                    c => result.push(c),
                }
            }
            result.push('\'');
            result
        }

        QuoteType::Backtick => {
            // Backtick quoting (minimal - just escape backticks)
            s.replace('`', "\\`")
        }
    }
}

/// Quote a string for safe shell use (convenience wrapper)
pub fn quote_string(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    
    let needs_quotes = s.chars().any(is_special);

    if !needs_quotes {
        s.to_string()
    } else {
        quotestring(s, QuoteType::Single)
    }
}

/// Split a string respecting quotes
pub fn split_quoted(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escape_next = false;
    
    for c in s.chars() {
        if escape_next {
            current.push(c);
            escape_next = false;
            continue;
        }
        
        match c {
            '\\' if !in_single_quote => escape_next = true,
            '\'' if !in_double_quote => in_single_quote = !in_single_quote,
            '"' if !in_single_quote => in_double_quote = !in_double_quote,
            ' ' | '\t' if !in_single_quote && !in_double_quote => {
                if !current.is_empty() {
                    result.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    
    if !current.is_empty() {
        result.push(current);
    }
    
    result
}

/// Split string by separator - port from zsh/Src/utils.c sepsplit() lines 3961-3992
///
/// If sep is None, performs IFS-style word splitting (spacesplit).
/// Otherwise splits on the given separator string.
/// allownull: if true, allows empty strings in result
pub fn sepsplit(s: &str, sep: Option<&str>, allownull: bool) -> Vec<String> {
    // Handle Nularg at start (zsh internal marker) - line 3968
    let s = if s.starts_with('\x00') && s.len() > 1 {
        &s[1..]
    } else {
        s
    };

    match sep {
        None => spacesplit(s, allownull),
        Some(sep) if sep.is_empty() => {
            // Empty separator: split into characters
            if allownull {
                s.chars().map(|c| c.to_string()).collect()
            } else {
                s.chars()
                    .map(|c| c.to_string())
                    .filter(|c| !c.is_empty())
                    .collect()
            }
        }
        Some(sep) => {
            let parts: Vec<String> = s.split(sep).map(|p| p.to_string()).collect();
            if allownull {
                parts
            } else {
                parts.into_iter().filter(|p| !p.is_empty()).collect()
            }
        }
    }
}

/// IFS-style word splitting - port from zsh/Src/utils.c spacesplit()
///
/// Splits on whitespace (space, tab, newline), treating consecutive
/// whitespace as a single separator.
pub fn spacesplit(s: &str, allownull: bool) -> Vec<String> {
    if allownull {
        s.split(|c: char| c == ' ' || c == '\t' || c == '\n')
            .map(|p| p.to_string())
            .collect()
    } else {
        s.split_whitespace().map(|p| p.to_string()).collect()
    }
}

/// Join array with separator - port from zsh/Src/utils.c sepjoin() lines 3926-3958
///
/// If sep is None, uses first char of IFS (defaults to space).
pub fn sepjoin(arr: &[String], sep: Option<&str>) -> String {
    if arr.is_empty() {
        return String::new();
    }
    let sep = sep.unwrap_or(" ");
    arr.join(sep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sepsplit() {
        assert_eq!(sepsplit("a:b:c", Some(":"), false), vec!["a", "b", "c"]);
        assert_eq!(sepsplit("a::b", Some(":"), false), vec!["a", "b"]);
        assert_eq!(sepsplit("a::b", Some(":"), true), vec!["a", "", "b"]);
    }

    #[test]
    fn test_spacesplit() {
        assert_eq!(spacesplit("a b c", false), vec!["a", "b", "c"]);
        assert_eq!(spacesplit("a  b", false), vec!["a", "b"]);
    }

    #[test]
    fn test_sepjoin() {
        assert_eq!(sepjoin(&["a".into(), "b".into(), "c".into()], Some(":")), "a:b:c");
        assert_eq!(sepjoin(&["a".into(), "b".into()], None), "a b");
    }

    #[test]
    fn test_is_identifier() {
        assert!(is_identifier("foo"));
        assert!(is_identifier("_bar"));
        assert!(is_identifier("baz123"));
        assert!(!is_identifier("123abc"));
        assert!(!is_identifier("foo-bar"));
    }

    #[test]
    fn test_is_number() {
        assert!(is_number("123"));
        assert!(is_number("-456"));
        assert!(is_number("+789"));
        assert!(!is_number("12.34"));
        assert!(!is_number("abc"));
    }

    #[test]
    fn test_nicechar() {
        assert_eq!(nicechar('\n'), "\\n");
        assert_eq!(nicechar('\t'), "\\t");
        assert_eq!(nicechar('a'), "a");
    }

    #[test]
    fn test_quote_string() {
        assert_eq!(quote_string("simple"), "simple");
        assert_eq!(quote_string("has space"), "'has space'");
        assert_eq!(quote_string("it's"), "'it'\\''s'");
    }

    #[test]
    fn test_quotestring_backslash() {
        assert_eq!(quotestring("hello", QuoteType::Backslash), "hello");
        assert_eq!(quotestring("has space", QuoteType::Backslash), "has\\ space");
        assert_eq!(quotestring("$var", QuoteType::Backslash), "\\$var");
    }

    #[test]
    fn test_quotestring_single() {
        assert_eq!(quotestring("hello", QuoteType::Single), "'hello'");
        assert_eq!(quotestring("it's", QuoteType::Single), "'it'\\''s'");
    }

    #[test]
    fn test_quotestring_double() {
        assert_eq!(quotestring("hello", QuoteType::Double), "\"hello\"");
        assert_eq!(quotestring("say \"hi\"", QuoteType::Double), "\"say \\\"hi\\\"\"");
    }

    #[test]
    fn test_quotestring_dollars() {
        assert_eq!(quotestring("hello", QuoteType::Dollars), "$'hello'");
        assert_eq!(quotestring("line\nbreak", QuoteType::Dollars), "$'line\\nbreak'");
        assert_eq!(quotestring("tab\there", QuoteType::Dollars), "$'tab\\there'");
    }

    #[test]
    fn test_quotestring_pattern() {
        assert_eq!(quotestring("*.txt", QuoteType::BackslashPattern), "\\*.txt");
        assert_eq!(quotestring("file[1]", QuoteType::BackslashPattern), "file\\[1\\]");
    }

    #[test]
    fn test_quotetype_from_q_count() {
        assert_eq!(QuoteType::from_q_count(1), QuoteType::Backslash);
        assert_eq!(QuoteType::from_q_count(2), QuoteType::Single);
        assert_eq!(QuoteType::from_q_count(3), QuoteType::Double);
        assert_eq!(QuoteType::from_q_count(4), QuoteType::Dollars);
    }

    #[test]
    fn test_split_quoted() {
        let result = split_quoted("foo bar baz");
        assert_eq!(result, vec!["foo", "bar", "baz"]);
        
        let result = split_quoted("'hello world' test");
        assert_eq!(result, vec!["hello world", "test"]);
        
        let result = split_quoted("\"double quoted\" value");
        assert_eq!(result, vec!["double quoted", "value"]);
    }

    #[test]
    fn test_expand_tilde() {
        // Just test that it doesn't crash - actual expansion depends on env
        let result = expand_tilde("~/test");
        assert!(!result.starts_with('~') || result == "~/test");
    }

    #[test]
    fn test_tulower_tuupper() {
        assert_eq!(tulower('A'), 'a');
        assert_eq!(tuupper('a'), 'A');
        assert_eq!(tulower('1'), '1');
    }
}
