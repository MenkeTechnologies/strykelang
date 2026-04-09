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
    pub fn new(kind: ErrorKind, message: impl Into<String>, line: usize, file: impl Into<String>) -> Self {
        Self { kind, message: message.into(), line, file: file.into() }
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
