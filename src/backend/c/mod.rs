//! C Backend - Generate C code from Aether IR
//!
//! This backend generates C code that can be compiled with any C compiler.
//! It's a fallback for when LLVM is not available.

mod c_codegen;

pub use c_codegen::CCodeGen;
