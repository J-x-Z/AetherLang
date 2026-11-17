//! Type system module

mod pointer_width;
pub use pointer_width::PointerWidth;
pub mod type_system;

pub use type_system::*;

pub mod ffi_target;
