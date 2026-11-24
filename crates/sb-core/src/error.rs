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
    /// CONST or ENUM of a name that is a built-in constant or was defined
    /// before in the program.
    DuplicateConstant,
    IllegalFunctionCall,
    StackOverflow,
    ReturnWithoutGosub,
    OutOfData,
    /// A break, continue, return or label outside the place it belongs.
    IllegalStatement,
    // The errors of blocks that do not close or close nothing, which
    // SmileBASIC 4 finds as it compiles.
    ForWithoutNext,
    NextWithoutFor,
    RepeatWithoutUntil,
    UntilWithoutRepeat,
    WhileWithoutWend,
    WendWithoutWhile,
    ThenWithoutEndif,
    ElseWithoutEndif,
    EndifWithoutIf,
    DefWithoutEnd,
    LoopWithoutEndloop,
    EndloopWithoutLoop,
    WhenWithoutCase,
    OtherwiseWithoutCase,
    EndcaseWithoutCase,
    CaseWithoutEndcase,
    /// A DEF inside another.
    NestedDef,
    /// A call with more than 254 arguments or OUT variables, or a DEF
    /// with more than 254 of either.
    TooManyArguments,
    /// CONST or ENUM of a value that is not computed as the program
    /// compiles.
    IllegalConstantExpression,
    /// DEFOUT, DEFARG and the others of a DEF used outside one.
    CantUseOutsideDef,
    IllegalProjectName,
    UndefinedProjectName,
    /// A file named with its project where programs may not name one,
    /// as in EXEC.
    CantAccessOtherProject,
    /// SAVE, SAVEV or SAVEG into one of SmileBASIC 4's own `#` projects.
    CantSaveToSystemProject,
    /// LOAD or SAVE of a file type other than a program, such as `DAT:`.
    FileTypeMismatch,
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
    // The errors of SmileBASIC 4's MML parser, each numbered 81 plus the
    // parser's own number.
    MmlTooLong,
    MmlTrackAlreadyDefined,
    MmlRepeatEndWithoutRepeat,
    MmlRepeatWithoutRepeatEnd,
    MmlMacroAlreadyDefined,
    MmlUndefinedMacro,
    MmlIllegalMacroName,
    MmlMacroNameTooLong,
    MmlCantUseVariable,
    MmlTransposeOutOfRange,
    MmlOctaveOutOfRange,
    MmlLengthOutOfRange,
    MmlVolumeOutOfRange,
    MmlVelocityOutOfRange,
    MmlPanpotOutOfRange,
    MmlGateTimeOutOfRange,
    MmlTempoOutOfRange,
    MmlNoteOutOfRange,
    MmlDetuneOutOfRange,
    MmlEnvelopeOutOfRange,
    MmlModulationOutOfRange,
    MmlVariableNumberOutOfRange,
    MmlVariableValueOutOfRange,
    MmlRepeatCountOutOfRange,
    MmlInstrumentOutOfRange,
    WrongNumberOfArguments,
    IllegalFileName,
    IllegalFileFormat,
    /// PRGEDIT on the slot that is running.
    CantEditRunningProgram,
    /// PRGGET$ and the other PRG commands before PRGEDIT.
    UsePrgedit,
    /// EXEC of a file that cannot be loaded.
    LoadFailed,
    /// A command for programs only, such as EXEC, in DIRECT mode. SmileBASIC
    /// 4 shows it without a place.
    CantUseFromDirectMode,
    /// A command for DIRECT mode only, such as PROJECT, run in a program.
    CantUseInProgram,
    /// SYSPARAM of a name it does not know.
    IllegalSystemArgumentName,
    /// SYSPARAM asked for a number of values other than the number its
    /// item has, or setting one that can only be read.
    WrongNumberOfOutArguments,
    /// FORMAT$ with a conversion it does not know, or a file name that
    /// breaks SmileBASIC 4's rules.
    IllegalFormat,
    /// A file name, or the project named in it, of over 32 characters.
    FileNameTooLong,
    /// EXEC of a slot with code that may still run.
    CantUseSpecifiedSlot,
    /// CONT with nothing to continue.
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
    /// An array with fewer elements than a command needs, such as an empty
    /// parameter array of ARYOP.
    ArrayTooSmall,
    /// XSCREEN with a width or height that is not a multiple of 4.
    UseMultiplesOf4,
    /// XCTRLSTYLE asking for the IR Motion Camera with a style other than 1.
    IllegalControlStyleForIrCamera,
    /// XCTRLSTYLE with a pair of Toy-Con types that do not go together.
    IllegalToyConType,
    /// SPDEF from an array whose length is not a multiple of 7.
    IllegalArraySize,
    /// SPLINK of a sprite to one linked below it.
    CantLinkToDescendant,
    /// RESIZE to more than 2^30 elements, or to a negative size.
    ArrayTooLarge,
    /// RESIZE or RECSAVE of an array in the system's own memory, such as
    /// the one GARRAY gives.
    CantResizeThisArray,
    /// RESTORE "POP" when the latest GOSUB or DEF call came after the
    /// last place RESTORE "PUSH" kept.
    UseRestorePush,
    /// SPFUNC or TFUNC of a name that is neither a symbol of the program
    /// nor a label.
    IllegalLabelString,
    /// An animation of more than 32 keys.
    AnimationTooLong,
    /// An animation target that is neither a number nor a string.
    AnimationTypeMismatch,
    /// IRSTART with a count of arguments that its mode does not take.
    WrongArgumentsForIrMode,
    /// SPFUNC or TFUNC of a name that is not a DEF, such as the name of a
    /// built-in command.
    FunctionMustBeUserDefined,
}

impl ErrorKind {
    /// Returns SmileBASIC's number for the error, as ENVSTAT gives it.
    /// The numbers are the same as in SmileBASIC 3.
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
            ErrorKind::ForWithoutNext => 20,
            ErrorKind::NextWithoutFor => 21,
            ErrorKind::RepeatWithoutUntil => 22,
            ErrorKind::UntilWithoutRepeat => 23,
            ErrorKind::WhileWithoutWend => 24,
            ErrorKind::WendWithoutWhile => 25,
            ErrorKind::ThenWithoutEndif => 26,
            ErrorKind::ElseWithoutEndif => 27,
            ErrorKind::EndifWithoutIf => 28,
            ErrorKind::DefWithoutEnd => 29,
            ErrorKind::ReturnWithoutGosub => 30,
            ErrorKind::NestedDef => 32,
            ErrorKind::TooManyArguments => 48,
            ErrorKind::IllegalConstantExpression => 51,
            ErrorKind::LoopWithoutEndloop => 54,
            ErrorKind::EndloopWithoutLoop => 55,
            ErrorKind::CantUseOutsideDef => 56,
            ErrorKind::WhenWithoutCase => 59,
            ErrorKind::OtherwiseWithoutCase => 60,
            ErrorKind::EndcaseWithoutCase => 61,
            ErrorKind::CaseWithoutEndcase => 62,
            ErrorKind::SubscriptOutOfRange => 31,
            ErrorKind::CantContinue => 33,
            ErrorKind::CantTrace => 34,
            ErrorKind::IllegalFileName => 37,
            ErrorKind::IllegalFileFormat => 38,
            ErrorKind::UsePrgedit => 39,
            ErrorKind::IllegalAnimationData => 41,
            ErrorKind::StringTooLong => 42,
            ErrorKind::CantUseFromDirectMode => 43,
            ErrorKind::CantUseInProgram => 44,
            ErrorKind::LoadFailed => 45,
            ErrorKind::DuplicateConstant => 49,
            ErrorKind::UndefinedConstant => 50,
            ErrorKind::WrongNumberOfArguments => 52,
            ErrorKind::WrongNumberOfOutArguments => 53,
            ErrorKind::SpriteNotSet => 57,
            ErrorKind::IllegalFontType => 58,
            ErrorKind::UseMultiplesOf4 => 69,
            ErrorKind::IllegalControlStyleForIrCamera => 109,
            ErrorKind::IllegalToyConType => 110,
            ErrorKind::IllegalArraySize => 117,
            ErrorKind::CantLinkToDescendant => 113,
            ErrorKind::IllegalLabelString => 35,
            ErrorKind::AnimationTooLong => 40,
            ErrorKind::AnimationTypeMismatch => 66,
            ErrorKind::WrongArgumentsForIrMode => 108,
            ErrorKind::FunctionMustBeUserDefined => 114,
            ErrorKind::IrCameraNotAvailable => 63,
            ErrorKind::IllegalIrCameraMode => 64,
            ErrorKind::CantUseEmptyString => 67,
            ErrorKind::CantUseUserDefinedCharacter => 68,
            ErrorKind::IllegalFormat => 71,
            ErrorKind::FileNameTooLong => 73,
            ErrorKind::CantAccessOtherProject => 74,
            ErrorKind::CantSaveToSystemProject => 75,
            ErrorKind::UndefinedProjectName => 76,
            ErrorKind::FileTypeMismatch => 77,
            ErrorKind::IllegalProjectName => 78,
            ErrorKind::IllegalMml => 81,
            ErrorKind::MmlTooLong => 82,
            ErrorKind::MmlTrackAlreadyDefined => 83,
            ErrorKind::MmlRepeatEndWithoutRepeat => 84,
            ErrorKind::MmlRepeatWithoutRepeatEnd => 85,
            ErrorKind::MmlMacroAlreadyDefined => 86,
            ErrorKind::MmlUndefinedMacro => 87,
            ErrorKind::MmlIllegalMacroName => 88,
            ErrorKind::MmlMacroNameTooLong => 89,
            ErrorKind::MmlCantUseVariable => 90,
            ErrorKind::MmlTransposeOutOfRange => 91,
            ErrorKind::MmlOctaveOutOfRange => 92,
            ErrorKind::MmlLengthOutOfRange => 93,
            ErrorKind::MmlVolumeOutOfRange => 94,
            ErrorKind::MmlVelocityOutOfRange => 95,
            ErrorKind::MmlPanpotOutOfRange => 96,
            ErrorKind::MmlGateTimeOutOfRange => 97,
            ErrorKind::MmlTempoOutOfRange => 98,
            ErrorKind::MmlNoteOutOfRange => 99,
            ErrorKind::MmlDetuneOutOfRange => 100,
            ErrorKind::MmlEnvelopeOutOfRange => 101,
            ErrorKind::MmlModulationOutOfRange => 102,
            ErrorKind::MmlVariableNumberOutOfRange => 103,
            ErrorKind::MmlVariableValueOutOfRange => 104,
            ErrorKind::MmlRepeatCountOutOfRange => 105,
            ErrorKind::MmlInstrumentOutOfRange => 106,
            ErrorKind::IllegalControlStyle => 107,
            ErrorKind::CantEditRunningProgram => 111,
            ErrorKind::IllegalSystemArgumentName => 112,
            ErrorKind::ArrayTooSmall => 116,
            ErrorKind::ArrayTooLarge => 47,
            ErrorKind::CantResizeThisArray => 65,
            ErrorKind::UseRestorePush => 119,
            ErrorKind::UseRestorePop => 118,
            ErrorKind::CantUseSpecifiedSlot => 120,
            // An error of the emulator only: a statement out of its
            // place, which SmileBASIC reports with a more specific error.
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
            ErrorKind::DuplicateConstant => "Duplicate constant",
            ErrorKind::IllegalFunctionCall => "Illegal function call",
            ErrorKind::StackOverflow => "Stack overflow",
            ErrorKind::ReturnWithoutGosub => "RETURN without GOSUB",
            ErrorKind::ForWithoutNext => "FOR without NEXT",
            ErrorKind::NextWithoutFor => "NEXT without FOR",
            ErrorKind::RepeatWithoutUntil => "REPEAT without UNTIL",
            ErrorKind::UntilWithoutRepeat => "UNTIL without REPEAT",
            ErrorKind::WhileWithoutWend => "WHILE without WEND",
            ErrorKind::WendWithoutWhile => "WEND without WHILE",
            ErrorKind::ThenWithoutEndif => "THEN without ENDIF",
            ErrorKind::ElseWithoutEndif => "ELSE without ENDIF",
            ErrorKind::EndifWithoutIf => "ENDIF without IF",
            ErrorKind::DefWithoutEnd => "DEF without END",
            ErrorKind::NestedDef => "Nested DEF",
            ErrorKind::TooManyArguments => "Too many arguments",
            ErrorKind::IllegalConstantExpression => "Illegal constant expression",
            ErrorKind::LoopWithoutEndloop => "LOOP without ENDLOOP",
            ErrorKind::EndloopWithoutLoop => "ENDLOOP without LOOP",
            ErrorKind::CantUseOutsideDef => "Can't use outside DEF",
            ErrorKind::WhenWithoutCase => "WHEN without CASE",
            ErrorKind::OtherwiseWithoutCase => "OTHERWISE without CASE",
            ErrorKind::EndcaseWithoutCase => "ENDCASE without CASE",
            ErrorKind::CaseWithoutEndcase => "CASE without ENDCASE",
            ErrorKind::OutOfData => "Out of DATA",
            ErrorKind::IllegalStatement => "Illegal statement",
            ErrorKind::IllegalProjectName => "Illegal project name",
            ErrorKind::UndefinedProjectName => "Undefined project name",
            ErrorKind::CantAccessOtherProject => "Can't access other project in this mode",
            ErrorKind::CantSaveToSystemProject => "Can't SAVE to system project",
            ErrorKind::FileTypeMismatch => "File type mismatch",
            ErrorKind::IllegalAnimationData => "Illegal animation data",
            ErrorKind::SpriteNotSet => "Sprite not set",
            ErrorKind::StringTooLong => "String is too long",
            ErrorKind::IllegalMml => "Illegal MML",
            ErrorKind::MmlTooLong => "MML:Too long",
            ErrorKind::MmlTrackAlreadyDefined => "MML:Track already defined",
            ErrorKind::MmlRepeatEndWithoutRepeat => "MML:RepeatEnd without repeat",
            ErrorKind::MmlRepeatWithoutRepeatEnd => "MML:Repeat without repeatEnd",
            ErrorKind::MmlMacroAlreadyDefined => "MML:Macro already defined",
            ErrorKind::MmlUndefinedMacro => "MML:Undefined macro",
            ErrorKind::MmlIllegalMacroName => "MML:Illegal macro name",
            ErrorKind::MmlMacroNameTooLong => "MML:Macro name is too long",
            ErrorKind::MmlCantUseVariable => "MML:Can't use variable",
            ErrorKind::MmlTransposeOutOfRange => "MML:Transpose out of range",
            ErrorKind::MmlOctaveOutOfRange => "MML:Octave out of range",
            ErrorKind::MmlLengthOutOfRange => "MML:Length out of range",
            ErrorKind::MmlVolumeOutOfRange => "MML:Volume out of range",
            ErrorKind::MmlVelocityOutOfRange => "MML:Velocity out of range",
            ErrorKind::MmlPanpotOutOfRange => "MML:Panpot out of range",
            ErrorKind::MmlGateTimeOutOfRange => "MML:GateTime out of range",
            ErrorKind::MmlTempoOutOfRange => "MML:Tempo out of range",
            ErrorKind::MmlNoteOutOfRange => "MML:Note out of range",
            ErrorKind::MmlDetuneOutOfRange => "MML:Detune out of range",
            ErrorKind::MmlEnvelopeOutOfRange => "MML:Envelope out of range",
            ErrorKind::MmlModulationOutOfRange => "MML:Modulation out of range",
            ErrorKind::MmlVariableNumberOutOfRange => "MML:Variable number out of range",
            ErrorKind::MmlVariableValueOutOfRange => "MML:Variable value out of range",
            ErrorKind::MmlRepeatCountOutOfRange => "MML:Repeat count out of range",
            ErrorKind::MmlInstrumentOutOfRange => "MML:Instrument out of range",
            ErrorKind::WrongNumberOfArguments => "Wrong number of arguments",
            ErrorKind::IllegalFileName => "Illegal file name",
            ErrorKind::IllegalFileFormat => "Illegal file format",
            ErrorKind::CantEditRunningProgram => "Can't edit running program",
            ErrorKind::UsePrgedit => "Use PRGEDIT before any PRG function",
            ErrorKind::LoadFailed => "Load failed",
            ErrorKind::CantUseFromDirectMode => "Can't use from direct mode",
            ErrorKind::CantUseInProgram => "Can't use in program",
            ErrorKind::IllegalSystemArgumentName => "Illegal system argument name",
            ErrorKind::IllegalFormat => "Illegal format",
            ErrorKind::FileNameTooLong => "File name is too long",
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
            ErrorKind::ArrayTooSmall => "Array is too small",
            ErrorKind::ArrayTooLarge => "Array is too large",
            ErrorKind::CantResizeThisArray => "Can't RESIZE this array",
            ErrorKind::UseRestorePush => "Use RESTORE PUSH before RESTORE POP",
            ErrorKind::UseMultiplesOf4 => "Use multiples of 4",
            ErrorKind::IllegalControlStyleForIrCamera => {
                "Illegal control style for using IR Camera"
            }
            ErrorKind::IllegalToyConType => "Illegal Toy-Con type",
            ErrorKind::IllegalArraySize => "Illegal array size",
            // As SmileBASIC 4 spells it.
            ErrorKind::CantLinkToDescendant => "Can't link to decendent sprite",
            ErrorKind::IllegalLabelString => "Illegal label string",
            ErrorKind::AnimationTooLong => "Animation is too long",
            ErrorKind::AnimationTypeMismatch => "Animation type mismatch",
            ErrorKind::WrongArgumentsForIrMode => {
                "Wrong number of arguments for this IR Camera mode"
            }
            ErrorKind::FunctionMustBeUserDefined => "Function must be user defined",
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
    /// Extra context for developers, e.g. which name was undefined. A
    /// detail in brackets is SmileBASIC 4's own, the command and argument
    /// as in `Overflow in 1:3(FILL:2)`, and is shown right after the place.
    pub detail: Option<String>,
    /// The program slot it happened in; `None` in DIRECT mode.
    pub slot: Option<u32>,
    /// The index of the command argument that was wrong, counted from 0,
    /// for ENVSTAT.
    pub arg: Option<u8>,
    /// The address of the value that was wrong, until the call that it
    /// was an argument of turns it into `arg`.
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

    /// Names `command` in the message the same way SmileBASIC 4 does:
    /// `(FILL:2)` with the argument from 1 when it is known, `(PUSH)` when
    /// not. A detail of the emulator stays after it, as in
    /// `(LOADV) (TXT into an array)`. A detail already in SmileBASIC 4's
    /// brackets is kept unchanged.
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

impl Error {
    /// The message as SmileBASIC 4 prints it for a line typed in DIRECT
    /// mode: with no place, and a space before the command it names, as in
    /// `Out of range (LOCATE:2)`.
    pub fn without_place(&self) -> String {
        Placed(self, false).to_string()
    }
}

/// An error's message, with its place or without.
struct Placed<'a>(&'a Error, bool);

impl fmt::Display for Placed<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Placed(e, place) = *self;
        write!(f, "{}", e.kind.message())?;
        // These show no place on hardware.
        let placeless = matches!(
            e.kind,
            ErrorKind::CantUseFromDirectMode | ErrorKind::CantContinue | ErrorKind::CantTrace
        );
        let place = place && !placeless;
        if place {
            write!(f, " in ")?;
            if let Some(slot) = e.slot {
                write!(f, "{slot}:")?;
            }
            write!(f, "{}", e.line)?;
        }
        match &e.detail {
            // SmileBASIC 4's own brackets follow a place with no space.
            Some(detail) if place && detail.starts_with('(') => write!(f, "{detail}")?,
            Some(detail) if detail.starts_with('(') => write!(f, " {detail}")?,
            Some(detail) => write!(f, " ({detail})")?,
            None => {}
        }
        Ok(())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Placed(self, true).fmt(f)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mml_errors_follow_illegal_mml() {
        assert_eq!(ErrorKind::MmlTooLong.code(), 82);
        assert_eq!(ErrorKind::MmlVolumeOutOfRange.code(), 94);
        assert_eq!(ErrorKind::MmlInstrumentOutOfRange.code(), 106);
        assert_eq!(
            ErrorKind::MmlLengthOutOfRange.message(),
            "MML:Length out of range"
        );
    }

    /// A line typed in DIRECT mode names no place (TEST70).
    #[test]
    fn without_place() {
        let e = Error::new(ErrorKind::OutOfRange)
            .of_arg(1)
            .of_command("LOCATE")
            .at(1);
        assert_eq!(e.to_string(), "Out of range in 1(LOCATE:2)");
        assert_eq!(e.without_place(), "Out of range (LOCATE:2)");
        let e = Error::with_detail(ErrorKind::SyntaxError, "expected expression").at(1);
        assert_eq!(e.without_place(), "Syntax error (expected expression)");
        let e = Error::new(ErrorKind::SyntaxError);
        assert_eq!(e.without_place(), "Syntax error");
    }
}
