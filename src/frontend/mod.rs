//! Frontend module - Lexer, Parser, Semantic Analysis

pub mod token;
pub(crate) mod escape;
pub mod lexer;
pub mod ast;
pub mod parser;
pub mod semantic;

pub mod resolve;

pub mod session;

pub mod symbols;

pub mod specialize;
