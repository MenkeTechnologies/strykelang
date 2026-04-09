use std::fmt;

#[derive(Debug, Clone)]
pub struct PerlError {
    pub kind: ErrorKind,
    pub message: String,
    pub line: usize,
    pub file: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorKind {
    Syntax,
    Runtime,
    Type,
    UndefinedVariable,
    UndefinedSubroutine,
    FileNotFound,
    IO,
    Regex,
    DivisionByZero,
    Die,
    Exit(i32),
}

impl PerlError {
    pub fn new(
        kind: ErrorKind,
        message: impl Into<String>,
        line: usize,
        file: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            message: message.into(),
            line,
            file: file.into(),
        }
    }

    pub fn syntax(message: impl Into<String>, line: usize) -> Self {
        Self::new(ErrorKind::Syntax, message, line, "-e")
    }

    pub fn runtime(message: impl Into<String>, line: usize) -> Self {
        Self::new(ErrorKind::Runtime, message, line, "-e")
    }

    pub fn die(message: impl Into<String>, line: usize) -> Self {
        Self::new(ErrorKind::Die, message, line, "-e")
    }
}

impl fmt::Display for PerlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            ErrorKind::Die => write!(f, "{}", self.message),
            ErrorKind::Exit(_) => write!(f, ""),
            _ => write!(f, "{} at {} line {}", self.message, self.file, self.line),
        }
    }
}

impl std::error::Error for PerlError {}

pub type PerlResult<T> = Result<T, PerlError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax_error_display_includes_message_and_line() {
        let e = PerlError::syntax("bad token", 7);
        let s = e.to_string();
        assert!(s.contains("bad token"));
        assert!(s.contains("line 7"));
    }

    #[test]
    fn die_error_display_is_message_only() {
        let e = PerlError::die("halt", 1);
        assert_eq!(e.to_string(), "halt");
    }

    #[test]
    fn exit_error_display_is_empty() {
        let e = PerlError::new(ErrorKind::Exit(0), "ignored", 1, "-e");
        assert_eq!(e.to_string(), "");
    }

    #[test]
    fn runtime_error_display_includes_file_and_line() {
        let e = PerlError::runtime("boom", 3);
        let s = e.to_string();
        assert!(s.contains("boom"));
        assert!(s.contains("-e"));
        assert!(s.contains("line 3"));
    }
}
