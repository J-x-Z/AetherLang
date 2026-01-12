//! Middle-end module - IR and optimization

pub mod ir;
pub mod ir_gen;
pub mod ir_printer;
pub mod optimize;

pub use ir::*;
pub use ir_gen::IRGenerator;
pub use ir_printer::print_ir;
pub use optimize::Optimizer;
