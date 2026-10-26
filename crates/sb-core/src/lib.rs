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
pub mod host;
pub mod lexer;
pub mod ops;
pub mod parser;
pub mod value;
pub mod vm;

use error::{Error, ErrorKind};
use parser::ParseErrorKind;

/// Parses and compiles a program.
pub fn compile(source: &str, host: &dyn host::Host) -> error::Result<bytecode::Compiled> {
    let program = parser::parse(source).map_err(|e| {
        let kind = match e.kind {
            ParseErrorKind::Lex(lexer::LexErrorKind::Overflow) => ErrorKind::Overflow,
            _ => ErrorKind::SyntaxError,
        };
        Error {
            kind,
            line: e.span.line,
            col: e.span.col,
            detail: e.detail(),
        }
    })?;
    compiler::compile(&program, host)
}
