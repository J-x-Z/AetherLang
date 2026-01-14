//! Code Generation trait - Backend abstraction
//!
//! This trait allows swapping between LLVM and native backends.
#![allow(dead_code)]

use crate::middle::ir::IRModule;
use crate::utils::Result;

/// Code generation backend trait
pub trait CodeGen {
    /// Generate machine code from IR module
    fn generate(&mut self, module: &IRModule) -> Result<Vec<u8>>;
    
    /// Get the target triple (e.g., "x86_64-pc-windows-msvc")
    fn target_triple(&self) -> &str;
    
    /// Get the backend name
    fn name(&self) -> &str;
}
