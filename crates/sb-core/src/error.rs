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
    UndefinedConstant,
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
    SpriteNotSet,
    StringTooLong,
    IllegalMml,
    WrongNumberOfArguments,
    IllegalFileName,
    IllegalFileFormat,
    /// PRGEDIT on the slot that is running.
    CantEditRunningProgram,
    /// PRGGET$ and the other PRG commands before PRGEDIT.
    UsePrgedit,
    /// EXEC of a file that cannot be loaded.
    LoadFailed,
    /// A command for programs only, such as EXEC, in direct mode. SmileBASIC
    /// 4 shows it without a place.
    CantUseFromDirectMode,
    /// SYSPARAM of a name it does not know.
    IllegalSystemArgumentName,
    /// SYSPARAM asked for another number of values than its item has, or
    /// setting one that can only be read.
    WrongNumberOfOutArguments,
    /// FORMAT$ with a conversion it does not know.
    IllegalFormat,
    /// EXEC of a slot with code that may still run.
    CantUseSpecifiedSlot,
    /// CONT with nothing to go on with.
    CantContinue,
    /// TRACE where it cannot run a statement.
    CantTrace,
    /// RESTORE "POP" with nothing kept.
    StackUnderflow,
    /// RETURN while places RESTORE "PUSH" kept in the subroutine remain.
    UseRestorePop,
    /// FONTINFO of an empty string.
    CantUseEmptyString,
    /// FONTINFO of a user-defined character.
    CantUseUserDefinedCharacter,
    /// IRSTART when XCTRLSTYLE did not ask for the IR Motion Camera.
    IrCameraNotAvailable,
    /// A Toy-Con command without XCTRLSTYLE setting up that Toy-Con.
    IllegalControlStyle,
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
            ErrorKind::UndefinedConstant => "Undefined constant",
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
            ErrorKind::SpriteNotSet => "Sprite not set",
            ErrorKind::StringTooLong => "String is too long",
            ErrorKind::IllegalMml => "Illegal MML",
            ErrorKind::WrongNumberOfArguments => "Wrong number of arguments",
            ErrorKind::IllegalFileName => "Illegal file name",
            ErrorKind::IllegalFileFormat => "Illegal file format",
            ErrorKind::CantEditRunningProgram => "Can't edit running program",
            ErrorKind::UsePrgedit => "Use PRGEDIT before any PRG function",
            ErrorKind::LoadFailed => "Load failed",
            ErrorKind::CantUseFromDirectMode => "Can't use from direct mode",
            ErrorKind::IllegalSystemArgumentName => "Illegal system argument name",
            ErrorKind::IllegalFormat => "Illegal format",
            ErrorKind::StackUnderflow => "Stack underflow",
            ErrorKind::CantUseSpecifiedSlot => "Can't use the specified slot",
            ErrorKind::CantContinue => "Can't continue",
            ErrorKind::CantTrace => "Can't trace",
            ErrorKind::UseRestorePop => "Use RESTORE POP before RETURN",
            ErrorKind::CantUseEmptyString => "Can't use empty string",
            // SmileBASIC 4's own spelling.
            ErrorKind::CantUseUserDefinedCharacter => "Can't use user-derined character",
            ErrorKind::IrCameraNotAvailable => "IR Camera is not available",
            ErrorKind::IllegalControlStyle => "Illegal control style",
            ErrorKind::WrongNumberOfOutArguments => "Wrong number of OUT arguments",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    /// Source line; 0 until the VM or compiler fills it in.
    pub line: u32,
    /// Column of the statement (or for syntax errors, the token) where it
    /// happened, in characters; 0 if unknown.
    pub col: u32,
    /// Extra context for developers, e.g. which name was undefined.
    pub detail: Option<String>,
    /// The program slot it happened in; `None` in direct mode.
    pub slot: Option<u32>,
}

impl Error {
    pub fn new(kind: ErrorKind) -> Self {
        Error {
            kind,
            line: 0,
            col: 0,
            detail: None,
            slot: None,
        }
    }

    pub fn with_detail(kind: ErrorKind, detail: impl Into<String>) -> Self {
        Error {
            kind,
            line: 0,
            col: 0,
            detail: Some(detail.into()),
            slot: None,
        }
    }

    pub fn at(mut self, line: u32) -> Self {
        if self.line == 0 {
            self.line = line;
        }
        self
    }

    /// Fills in the program slot, if not known yet.
    pub fn in_slot(mut self, slot: Option<u32>) -> Self {
        self.slot = self.slot.or(slot);
        self
    }

    /// Fills in the line and column, if not known yet.
    pub fn at_pos(mut self, line: u32, col: u32) -> Self {
        if self.line == 0 {
            (self.line, self.col) = (line, col);
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
        write!(f, "{}", self.kind.message())?;
        // These say no place on hardware.
        let placeless = matches!(
            self.kind,
            ErrorKind::CantUseFromDirectMode | ErrorKind::CantContinue | ErrorKind::CantTrace
        );
        if !placeless {
            write!(f, " in ")?;
            if let Some(slot) = self.slot {
                write!(f, "{slot}:")?;
            }
            write!(f, "{}", self.line)?;
        }
        if let Some(detail) = &self.detail {
            write!(f, " ({detail})")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
