//! Backend module - Code generation

pub mod codegen;

// C Backend (always available)
pub mod c;

// LLVM Backend (optional, requires --features llvm)
#[cfg(feature = "llvm")]
pub mod llvm;

pub use codegen::CodeGen;
pub use c::CCodeGen;
