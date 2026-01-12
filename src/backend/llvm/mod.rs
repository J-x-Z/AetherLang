//! LLVM Backend

#[cfg(feature = "llvm")]
mod llvm_codegen;

#[cfg(feature = "llvm")]
pub use llvm_codegen::LLVMCodeGen;
