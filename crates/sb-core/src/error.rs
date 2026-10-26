//! Run-time and compile-time errors, named after SmileBASIC's messages.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    SyntaxError,
    TypeMismatch,
    DivideByZero,
    Overflow,
    SubscriptOutOfRange,
    OutOfRange,
    UndefinedVariable,
    UndefinedLabel,
    UndefinedFunction,
    DuplicateVariable,
    DuplicateLabel,
    DuplicateFunction,
    IllegalFunctionCall,
    StackOverflow,
    ReturnWithoutGosub,
    OutOfData,
    /// A break, continue, return or label outside the place it belongs.
    IllegalStatement,
    IllegalProjectName,
    UndefinedProjectName,
    IllegalAnimationData,
}

impl ErrorKind {
    pub fn message(self) -> &'static str {
        match self {
            ErrorKind::SyntaxError => "Syntax error",
            ErrorKind::TypeMismatch => "Type mismatch",
            ErrorKind::DivideByZero => "Divide by zero",
            ErrorKind::Overflow => "Overflow",
            ErrorKind::SubscriptOutOfRange => "Subscript out of range",
            ErrorKind::OutOfRange => "Out of range",
            ErrorKind::UndefinedVariable => "Undefined variable",
            ErrorKind::UndefinedLabel => "Undefined label",
            ErrorKind::UndefinedFunction => "Undefined function",
            ErrorKind::DuplicateVariable => "Duplicate variable",
            ErrorKind::DuplicateLabel => "Duplicate label",
            ErrorKind::DuplicateFunction => "Duplicate function",
            ErrorKind::IllegalFunctionCall => "Illegal function call",
            ErrorKind::StackOverflow => "Stack overflow",
            ErrorKind::ReturnWithoutGosub => "RETURN without GOSUB",
            ErrorKind::OutOfData => "Out of DATA",
            ErrorKind::IllegalStatement => "Illegal statement",
            ErrorKind::IllegalProjectName => "Illegal project name",
            ErrorKind::UndefinedProjectName => "Undefined project name",
            ErrorKind::IllegalAnimationData => "Illegal animation data",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    /// Source line; 0 until the VM or compiler fills it in.
    pub line: u32,
    /// Extra context for developers, e.g. which name was undefined.
    pub detail: Option<String>,
}

impl Error {
    pub fn new(kind: ErrorKind) -> Self {
        Error {
            kind,
            line: 0,
            detail: None,
        }
    }

    pub fn with_detail(kind: ErrorKind, detail: impl Into<String>) -> Self {
        Error {
            kind,
            line: 0,
            detail: Some(detail.into()),
        }
    }

    pub fn at(mut self, line: u32) -> Self {
        if self.line == 0 {
            self.line = line;
        }
        self
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Self {
        Error::new(kind)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} in {}", self.kind.message(), self.line)?;
        if let Some(detail) = &self.detail {
            write!(f, " ({detail})")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
