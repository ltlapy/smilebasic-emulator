//! SmileBASIC language core: lexer, parser, compiler and bytecode VM.
//!
//! This crate is platform independent and has no knowledge of rendering,
//! audio or input devices; those are reached through [`host::Host`].

pub mod ast;
pub mod builtins;
pub mod bytecode;
pub mod compiler;
pub mod constants;
pub mod error;
pub mod fuse;
pub mod host;
pub mod inspect;
#[cfg(feature = "jit")]
pub mod jit;
pub mod lexer;
pub mod memory;
pub mod ops;
pub mod parser;
pub mod value;
mod vars;
pub mod vm;
pub mod watch;
pub mod wtf8;

use error::{Error, ErrorKind};
use parser::ParseErrorKind;

/// The build options that change how the VM runs, for version lines, so
/// that a report shows which build it came from: empty, or the names of
/// the enabled features, `unchecked-vars` and `jit`.
pub const BUILD_OPTIONS: &str = match (cfg!(feature = "unchecked-vars"), cfg!(feature = "jit")) {
    (false, false) => "",
    (true, false) => "unchecked-vars",
    (false, true) => "jit",
    (true, true) => "unchecked-vars jit",
};

/// Parses and compiles a program.
pub fn compile(source: &str, host: &dyn host::Host) -> error::Result<bytecode::Compiled> {
    compile_as(source, host, false, 0)
}

/// Parses and compiles the program in `slot`, which `#_SLOT` gives.
pub fn compile_in(
    source: &str,
    host: &dyn host::Host,
    slot: usize,
) -> error::Result<bytecode::Compiled> {
    compile_as(source, host, false, slot as i32)
}

/// Compiles a line typed in DIRECT mode, whose DIM and VAR declare as
/// they run (see [`bytecode::Op::Declare`]).
/// In the line, `#_SLOT` is `slot`: the slot that ran last.
pub fn compile_direct(
    source: &str,
    host: &dyn host::Host,
    slot: usize,
) -> error::Result<bytecode::Compiled> {
    compile_as(source, host, true, slot as i32)
}

fn compile_as(
    source: &str,
    host: &dyn host::Host,
    direct: bool,
    slot: i32,
) -> error::Result<bytecode::Compiled> {
    // The statements before one that does not parse compile first, and
    // give their errors before its own.
    let (program, unparsed) = parser::parse_partial(source, direct);
    let unparsed = unparsed.map(parse_error);
    match unparsed {
        Some(e) if program.stmts.is_empty() => Err(e),
        _ => compiler::compile(&program, host, direct, slot, unparsed),
    }
}

fn parse_error(e: parser::ParseError) -> Error {
    let kind = match e.kind {
        ParseErrorKind::DirectMode => ErrorKind::CantUseFromDirectMode,
        ParseErrorKind::Compile(kind) => kind,
        _ => ErrorKind::SyntaxError,
    };
    // The editor shows the statement, the same as SmileBASIC 4 does
    // (checked on hardware), unless the error is on a later line of it.
    let at = e.stmt.filter(|s| s.line == e.span.line).unwrap_or(e.span);
    let error = match e.detail() {
        Some(detail) => Error::with_detail(kind, detail),
        None => Error::new(kind),
    };
    error.at_pos(at.line, at.col)
}
