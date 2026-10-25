//! SmileBASIC language core: lexer, parser, compiler and bytecode VM.
//!
//! This crate is platform independent and has no knowledge of rendering,
//! audio or input devices.

pub mod ast;
pub mod lexer;
pub mod parser;
