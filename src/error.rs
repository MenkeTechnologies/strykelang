use std::fmt;

use crate::value::PerlValue;

#[derive(Debug, Clone)]
pub struct PerlError {
    pub kind: ErrorKind,
    pub message: String,
    pub line: usize,
    pub file: String,
    /// When `die` is called with a ref argument, the original value is preserved here
    /// so that `$@` can hold the ref (not just its stringification).
    pub die_value: Option<PerlValue>,
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
            die_value: None,
        }
    }

    pub fn syntax(message: impl Into<String>, line: usize) -> Self {
        Self::new(ErrorKind::Syntax, message, line, "-e")
    }

    pub fn runtime(message: impl Into<String>, line: usize) -> Self {
        Self::new(ErrorKind::Runtime, message, line, "-e")
    }

    pub fn type_error(message: impl Into<String>, line: usize) -> Self {
        Self::new(ErrorKind::Type, message, line, "-e")
    }

    /// Replace line number (e.g. map VM op line onto an error).
    pub fn at_line(mut self, line: usize) -> Self {
        self.line = line;
        self
    }

    pub fn die(message: impl Into<String>, line: usize) -> Self {
        Self::new(ErrorKind::Die, message, line, "-e")
    }

    pub fn die_with_value(value: PerlValue, message: String, line: usize) -> Self {
        let mut e = Self::new(ErrorKind::Die, message, line, "-e");
        e.die_value = Some(value);
        e
    }
}

impl fmt::Display for PerlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            ErrorKind::Die => write!(f, "{}", self.message),
            ErrorKind::Exit(_) => write!(f, ""),
            // Perl 5 ends runtime errors with `.` after the line number
            // (`Illegal division by zero at -e line 1.`). Matches stock
            // perl for `fo --compat` parity — see `tests/suite/error_parity.rs`.
            _ => write!(f, "{} at {} line {}.", self.message, self.file, self.line),
        }
    }
}

impl std::error::Error for PerlError {}

pub type PerlResult<T> = Result<T, PerlError>;

/// Long-form hints for `fo --explain CODE` (rustc-style).
pub fn explain_error(code: &str) -> Option<&'static str> {
    match code {
        "E0001" => Some(
            "Undefined subroutine: no `sub name` or builtin exists for this bare call. \
Declare the sub, use the correct package (`Foo::bar`), or import via `use Module qw(name)`.",
        ),
        "E0002" => Some(
            "Runtime error from `die`, a failed builtin, or an I/O/regex/sqlite failure. \
Check the message above; use `try { } catch ($e) { }` to recover.",
        ),
        "E0003" => Some(
            "pmap_reduce / preduce require an associative reduce op: order of pairwise combines is not fixed. \
Do not use for non-associative operations.",
        ),
        _ => None,
    }
}

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

    #[test]
    fn division_by_zero_kind_matches_message_display() {
        let e = PerlError::new(ErrorKind::DivisionByZero, "divide by zero", 2, "t.pl");
        assert_eq!(e.kind, ErrorKind::DivisionByZero);
        let s = e.to_string();
        assert!(s.contains("divide by zero"));
        assert!(s.contains("t.pl"));
        assert!(s.contains("line 2"));
    }

    #[test]
    fn type_error_display_matches_runtime_shape() {
        let e = PerlError::type_error("expected array", 9);
        assert_eq!(e.kind, ErrorKind::Type);
        let s = e.to_string();
        assert!(s.contains("expected array"));
        assert!(s.contains("line 9"));
    }

    #[test]
    fn at_line_overrides_line_number() {
        let e = PerlError::runtime("x", 1).at_line(99);
        assert_eq!(e.line, 99);
        assert!(e.to_string().contains("line 99"));
    }

    #[test]
    fn explain_error_known_codes() {
        assert!(explain_error("E0001").is_some());
        assert!(explain_error("E0002").is_some());
        assert!(explain_error("E0003").is_some());
    }

    #[test]
    fn explain_error_unknown_returns_none() {
        assert!(explain_error("E9999").is_none());
        assert!(explain_error("").is_none());
    }

    #[test]
    fn perl_error_implements_std_error() {
        let e: Box<dyn std::error::Error> = Box::new(PerlError::syntax("x", 1));
        assert!(!e.to_string().is_empty());
    }
}
