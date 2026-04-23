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

/// Quote a string for safe shell use
pub fn quote_string(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    
    let needs_quotes = s.chars().any(|c| {
        matches!(
            c,
            ' ' | '\t' | '\n' | '\'' | '"' | '\\' | '$' | '`' | '!' | '*' | '?' | '[' | ']'
            | '{' | '}' | '(' | ')' | '<' | '>' | '|' | '&' | ';' | '#' | '~'
        )
    });

    if !needs_quotes {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
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

#[cfg(test)]
mod tests {
    use super::*;

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
