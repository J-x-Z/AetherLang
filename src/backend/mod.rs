//! Backend module - Code generation

pub mod codegen;

#[cfg(feature = "llvm")]
pub mod llvm;

pub use codegen::CodeGen;
