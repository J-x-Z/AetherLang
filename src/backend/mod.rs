//! Backend module - Code generation
#![allow(unused_imports)]

pub mod codegen;
pub(crate) mod system_attributes;
pub mod linker;

// C Backend (always available)
pub mod c;

// Both supported LLVM versions use this backend.
#[cfg(any(feature = "llvm", feature = "llvm23"))]
pub mod llvm;

pub use codegen::CodeGen;
pub use c::CCodeGen;
