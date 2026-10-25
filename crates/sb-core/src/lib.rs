//! SmileBASIC language core: lexer, parser, compiler and bytecode VM.
//!
//! This crate is platform independent and has no knowledge of rendering,
//! audio or input devices.

pub mod ast;
pub mod error;
pub mod lexer;
pub mod ops;
pub mod parser;
pub mod value;
