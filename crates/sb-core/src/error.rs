//! Run-time and compile-time errors, named after SmileBASIC's messages.

use std::fmt;

use crate::value::Value;

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
    /// More live strings and arrays than SmileBASIC 4's memory holds.
    OutOfMemory,
    /// A program too long for its slot's code memory, as it compiles.
    OutOfCodeMemory,
    /// Never seen by a program: the running estimate of what is live says
    /// a request may not fit, so the VM counts exactly and runs the
    /// instruction again. See [`crate::memory`].
    MemoryCheck,
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
    IllegalIrCameraMode,
    /// A Toy-Con command without XCTRLSTYLE setting up that Toy-Con.
    IllegalControlStyle,
    /// GPUTCHR with a font type other than 8 or 16.
    IllegalFontType,
}

impl ErrorKind {
    /// SmileBASIC's number for the error, as ENVSTAT gives it; the same
    /// as SmileBASIC 3's.
    pub fn code(self) -> i32 {
        match self {
            ErrorKind::SyntaxError => 3,
            ErrorKind::IllegalFunctionCall => 4,
            ErrorKind::StackOverflow => 5,
            ErrorKind::StackUnderflow => 6,
            ErrorKind::DivideByZero => 7,
            ErrorKind::TypeMismatch => 8,
            ErrorKind::Overflow => 9,
            ErrorKind::OutOfRange => 10,
            ErrorKind::OutOfMemory | ErrorKind::MemoryCheck => 11,
            ErrorKind::OutOfCodeMemory => 12,
            ErrorKind::OutOfData => 13,
            ErrorKind::UndefinedLabel => 14,
            ErrorKind::UndefinedVariable => 15,
            ErrorKind::UndefinedFunction => 16,
            ErrorKind::DuplicateLabel => 17,
            ErrorKind::DuplicateVariable => 18,
            ErrorKind::DuplicateFunction => 19,
            ErrorKind::ReturnWithoutGosub => 30,
            ErrorKind::SubscriptOutOfRange => 31,
            ErrorKind::CantContinue => 33,
            ErrorKind::CantTrace => 34,
            ErrorKind::IllegalFileName => 37,
            ErrorKind::IllegalFileFormat => 38,
            ErrorKind::UsePrgedit => 39,
            ErrorKind::IllegalAnimationData => 41,
            ErrorKind::StringTooLong => 42,
            ErrorKind::CantUseFromDirectMode => 43,
            ErrorKind::LoadFailed => 45,
            ErrorKind::UndefinedConstant => 50,
            ErrorKind::WrongNumberOfArguments => 52,
            ErrorKind::WrongNumberOfOutArguments => 53,
            ErrorKind::SpriteNotSet => 57,
            ErrorKind::IllegalFontType => 58,
            ErrorKind::IrCameraNotAvailable => 63,
            ErrorKind::IllegalIrCameraMode => 64,
            ErrorKind::CantUseEmptyString => 67,
            ErrorKind::CantUseUserDefinedCharacter => 68,
            ErrorKind::IllegalFormat => 71,
            ErrorKind::UndefinedProjectName => 76,
            ErrorKind::IllegalProjectName => 78,
            ErrorKind::IllegalMml => 81,
            ErrorKind::IllegalControlStyle => 107,
            ErrorKind::CantEditRunningProgram => 111,
            ErrorKind::IllegalSystemArgumentName => 112,
            ErrorKind::UseRestorePop => 118,
            ErrorKind::CantUseSpecifiedSlot => 120,
            // The emulator's own: a statement out of its place, which
            // SmileBASIC reports with a more particular error.
            ErrorKind::IllegalStatement => 3,
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            ErrorKind::SyntaxError => "Syntax error",
            ErrorKind::TypeMismatch => "Type mismatch",
            ErrorKind::DivideByZero => "Divide by zero",
            ErrorKind::Overflow => "Overflow",
            ErrorKind::SubscriptOutOfRange => "Subscript out of range",
            ErrorKind::OutOfRange => "Out of range",
            ErrorKind::OutOfMemory | ErrorKind::MemoryCheck => "Out of memory",
            ErrorKind::OutOfCodeMemory => "Out of code memory",
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
            ErrorKind::CantUseUserDefinedCharacter => "Can't use user-defined character",
            ErrorKind::IrCameraNotAvailable => "IR Camera is not available",
            ErrorKind::IllegalIrCameraMode => "Illegal IR Camera mode",
            ErrorKind::IllegalControlStyle => "Illegal control style",
            ErrorKind::IllegalFontType => "Illegal font type",
            ErrorKind::WrongNumberOfOutArguments => "Wrong number of OUT arguments",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Error {
    pub kind: ErrorKind,
    /// Source line; 0 until the VM or compiler fills it in.
    pub line: u32,
    /// Column of the statement (or for syntax errors, the token) where it
    /// happened, in characters; 0 if unknown.
    pub col: u32,
    /// Extra context for developers, e.g. which name was undefined. One in
    /// brackets is SmileBASIC 4's own, the command and argument as in
    /// `Overflow in 1:3(FILL:2)`, and shows right after the place.
    pub detail: Option<String>,
    /// The program slot it happened in; `None` in direct mode.
    pub slot: Option<u32>,
    /// Which of a command's arguments was wrong, from 0, for ENVSTAT.
    pub arg: Option<u8>,
    /// Where the value that was wrong is, until the call it was an
    /// argument of turns it into `arg`.
    value_at: usize,
}

impl PartialEq for Error {
    fn eq(&self, other: &Self) -> bool {
        (
            self.kind,
            self.line,
            self.col,
            &self.detail,
            self.slot,
            self.arg,
        ) == (
            other.kind,
            other.line,
            other.col,
            &other.detail,
            other.slot,
            other.arg,
        )
    }
}

impl Eq for Error {}

impl Error {
    pub fn new(kind: ErrorKind) -> Self {
        Error {
            kind,
            line: 0,
            col: 0,
            detail: None,
            slot: None,
            arg: None,
            value_at: 0,
        }
    }

    pub fn with_detail(kind: ErrorKind, detail: impl Into<String>) -> Self {
        Error {
            detail: Some(detail.into()),
            ..Error::new(kind)
        }
    }

    /// Marks `value` as what was wrong: if it is one of a command's
    /// arguments, the error tells which.
    pub fn of_value(mut self, value: &Value) -> Self {
        if self.value_at == 0 {
            self.value_at = value as *const Value as usize;
        }
        self
    }

    /// Marks the argument that was wrong by its number, from 0.
    pub fn of_arg(mut self, arg: usize) -> Self {
        self.arg = self.arg.or(u8::try_from(arg).ok());
        self
    }

    /// Turns the value marked by [`Error::of_value`] into the number of
    /// the argument it is, if it is one of `args`.
    pub fn in_args(mut self, args: &[Value]) -> Self {
        let size = std::mem::size_of::<Value>();
        let start = args.as_ptr() as usize;
        if self.arg.is_none()
            && self.value_at >= start
            && self.value_at < start + std::mem::size_of_val(args)
        {
            let arg = (self.value_at - start) / size;
            self = self.of_arg(arg);
        }
        self.value_at = 0;
        self
    }

    /// Names `command` in the message as SmileBASIC 4 does, `(FILL:2)`
    /// with the argument from 1 when it is known, `(PUSH)` when not. A
    /// detail of our own stays after it, as `(LOADV) (TXT into an array)`;
    /// one in SmileBASIC 4's brackets already is kept as it is.
    pub fn of_command(mut self, command: &str) -> Self {
        let named = match self.arg {
            Some(arg) => format!("({command}:{})", u32::from(arg) + 1),
            None => format!("({command})"),
        };
        self.detail = match self.detail.take() {
            None => Some(named),
            Some(own) if own.starts_with('(') => Some(own),
            Some(own) => Some(format!("{named} ({own})")),
        };
        self
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
        match &self.detail {
            Some(detail) if detail.starts_with('(') => write!(f, "{detail}")?,
            Some(detail) => write!(f, " ({detail})")?,
            None => {}
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
