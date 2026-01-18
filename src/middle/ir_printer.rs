//! IR Printer - Pretty print Aether IR
//!
//! Outputs human-readable IR for debugging.
#![allow(dead_code)]

use std::fmt::Write;
use crate::middle::ir::*;

/// Pretty printer for Aether IR
pub struct IRPrinter {
    output: String,
    indent: usize,
}

impl IRPrinter {
    pub fn new() -> Self {
        Self {
            output: String::new(),
            indent: 0,
        }
    }

    /// Print an IR module to string
    pub fn print_module(&mut self, module: &IRModule) -> String {
        self.output.clear();
        
        writeln!(self.output, "; Module: {}", module.name).unwrap();
        writeln!(self.output, "; Functions: {}", module.functions.len()).unwrap();
        writeln!(self.output).unwrap();

        for func in &module.functions {
            self.print_function(func);
            writeln!(self.output).unwrap();
        }

        self.output.clone()
    }

    /// Print a function
    fn print_function(&mut self, func: &IRFunction) {
        // Function signature
        write!(self.output, "fn {}(", func.name).unwrap();
        for (i, (name, ty)) in func.params.iter().enumerate() {
            if i > 0 {
                write!(self.output, ", ").unwrap();
            }
            write!(self.output, "{}: {}", name, self.type_str(ty)).unwrap();
        }
        writeln!(self.output, ") -> {} {{", self.type_str(&func.ret_type)).unwrap();

        // Basic blocks
        for block in &func.blocks {
            self.print_block(block);
        }

        writeln!(self.output, "}}").unwrap();
    }

    /// Print a basic block
    fn print_block(&mut self, block: &BasicBlock) {
        writeln!(self.output, "  {}:", block.label).unwrap();

        // Instructions
        for inst in &block.instructions {
            write!(self.output, "    ").unwrap();
            self.print_instruction(inst);
            writeln!(self.output).unwrap();
        }

        // Terminator
        if let Some(ref term) = block.terminator {
            write!(self.output, "    ").unwrap();
            self.print_terminator(term);
            writeln!(self.output).unwrap();
        }
    }

    /// Print an instruction
    fn print_instruction(&mut self, inst: &Instruction) {
        match inst {
            Instruction::Assign { dest, value } => {
                write!(self.output, "{} = {}", dest, self.value_str(value)).unwrap();
            }
            Instruction::BinOp { dest, op, left, right } => {
                write!(
                    self.output, 
                    "{} = {} {} {}", 
                    dest, 
                    op, 
                    self.value_str(left), 
                    self.value_str(right)
                ).unwrap();
            }
            Instruction::Cast { dest, value, ty } => {
                write!(self.output, "{} = cast {} to {:?}", dest, self.value_str(value), ty).unwrap();
            }
            Instruction::UnaryOp { dest, op, value } => {
                let op_str = match op {
                    UnaryOp::Neg => "neg",
                    UnaryOp::Not => "not",
                    UnaryOp::BitNot => "bitnot",
                };
                write!(self.output, "{} = {} {}", dest, op_str, self.value_str(value)).unwrap();
            }
            Instruction::Call { dest, func, args } => {
                if let Some(d) = dest {
                    write!(self.output, "{} = ", d).unwrap();
                }
                write!(self.output, "call {}(", func).unwrap();
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(self.output, ", ").unwrap();
                    }
                    write!(self.output, "{}", self.value_str(arg)).unwrap();
                }
                write!(self.output, ")").unwrap();
            }
            Instruction::Alloca { dest, ty } => {
                write!(self.output, "{} = alloca {}", dest, self.type_str(ty)).unwrap();
            }
            Instruction::Load { dest, ptr, ty } => {
                write!(self.output, "{} = load {}", dest, self.value_str(ptr)).unwrap();
            }
            Instruction::Store { ptr, value } => {
                write!(self.output, "store {}, {}", self.value_str(value), self.value_str(ptr)).unwrap();
            }
            Instruction::GetElementPtr { dest, ptr, index, elem_ty: _ } => {
                write!(
                    self.output, 
                    "{} = gep {}, {}", 
                    dest, 
                    self.value_str(ptr), 
                    self.value_str(index)
                ).unwrap();
            }
            Instruction::Phi { dest, incoming } => {
                write!(self.output, "{} = phi ", dest).unwrap();
                for (i, (val, block)) in incoming.iter().enumerate() {
                    if i > 0 {
                        write!(self.output, ", ").unwrap();
                    }
                    write!(self.output, "[{}, bb{}]", self.value_str(val), block.0).unwrap();
                }
            }
            Instruction::InlineAsm { template, operands } => {
                write!(self.output, "asm!(\"{}\"", template).unwrap();
                for op in operands {
                    write!(self.output, ", ").unwrap();
                    match op.kind {
                        IRAsmOperandKind::Input => {
                            if let Some(ref val) = op.input {
                                write!(self.output, "in(\"{}\") {}", op.constraint, self.value_str(val)).unwrap();
                            }
                        }
                        IRAsmOperandKind::Output => {
                            if let Some(reg) = op.output {
                                write!(self.output, "out(\"{}\") %{}", op.constraint, reg.0).unwrap();
                            }
                        }
                        IRAsmOperandKind::InOut => {
                            if let Some(reg) = op.output {
                                 write!(self.output, "inout(\"{}\") ", op.constraint).unwrap();
                                 if let Some(ref val) = op.input {
                                     write!(self.output, "{} -> %{}", self.value_str(val), reg.0).unwrap();
                                 }
                            }
                        }
                        IRAsmOperandKind::Clobber => {
                             write!(self.output, "clobber(\"{}\")", op.constraint).unwrap();
                        }
                    }
                }
                write!(self.output, ")").unwrap();
            }
        }
    }

    /// Print a terminator
    fn print_terminator(&mut self, term: &Terminator) {
        match term {
            Terminator::Return { value } => {
                if let Some(v) = value {
                    write!(self.output, "ret {}", self.value_str(v)).unwrap();
                } else {
                    write!(self.output, "ret void").unwrap();
                }
            }
            Terminator::Jump { target } => {
                write!(self.output, "br bb{}", target.0).unwrap();
            }
            Terminator::Branch { cond, then_target, else_target } => {
                write!(
                    self.output, 
                    "br {}, bb{}, bb{}", 
                    self.value_str(cond), 
                    then_target.0, 
                    else_target.0
                ).unwrap();
            }
            Terminator::Unreachable => {
                write!(self.output, "unreachable").unwrap();
            }
        }
    }

    /// Convert value to string
    fn value_str(&self, value: &Value) -> String {
        match value {
            Value::Register(r) => format!("{}", r),
            Value::Constant(c) => format!("{}", c),
            Value::Parameter(i) => format!("arg{}", i),
            Value::Global(name) => format!("@{}", name),
            Value::Unit => "()".to_string(),
        }
    }

    /// Convert type to string
    fn type_str(&self, ty: &IRType) -> String {
        match ty {
            IRType::Void => "void".to_string(),
            IRType::Bool => "bool".to_string(),
            IRType::I8 => "i8".to_string(),
            IRType::I16 => "i16".to_string(),
            IRType::I32 => "i32".to_string(),
            IRType::I64 => "i64".to_string(),
            IRType::U8 => "u8".to_string(),
            IRType::U16 => "u16".to_string(),
            IRType::U32 => "u32".to_string(),
            IRType::U64 => "u64".to_string(),
            IRType::F32 => "f32".to_string(),
            IRType::F64 => "f64".to_string(),
            IRType::Ptr(inner) => format!("*{}", self.type_str(inner)),
            IRType::Array(elem, size) => format!("[{}; {}]", self.type_str(elem), size),
            IRType::Struct(name) => name.clone(),
            IRType::Function { params, ret } => {
                let params_str: Vec<_> = params.iter().map(|t| self.type_str(t)).collect();
                format!("fn({}) -> {}", params_str.join(", "), self.type_str(ret))
            }
        }
    }
}

impl Default for IRPrinter {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function to print a module
pub fn print_ir(module: &IRModule) -> String {
    let mut printer = IRPrinter::new();
    printer.print_module(module)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::lexer::Lexer;
    use crate::frontend::parser::Parser;
    use crate::middle::ir_gen::IRGenerator;

    fn compile_and_print(source: &str) -> String {
        let lexer = Lexer::new(source, 0);
        let mut parser = Parser::new(lexer);
        let program = parser.parse_program().unwrap();
        let mut gen = IRGenerator::new("test");
        let module = gen.generate(&program).unwrap();
        print_ir(&module)
    }

    #[test]
    fn test_print_empty_function() {
        let ir = compile_and_print("fn main() {}");
        assert!(ir.contains("fn main()"));
        assert!(ir.contains("ret void"));
        println!("{}", ir);
    }

    #[test]
    fn test_print_return_constant() {
        let ir = compile_and_print("fn foo() -> i32 { return 42 }");
        assert!(ir.contains("fn foo()"));
        assert!(ir.contains("42"));
        println!("{}", ir);
    }

    #[test]
    fn test_print_binary_expression() {
        let ir = compile_and_print("fn add() -> i32 { return 1 + 2 }");
        assert!(ir.contains("add"));
        println!("{}", ir);
    }

    #[test]
    fn test_print_if_expression() {
        let ir = compile_and_print("fn test() { if true { return 1 } else { return 0 } }");
        assert!(ir.contains("br "));
        assert!(ir.contains("then"));
        assert!(ir.contains("else"));
        println!("{}", ir);
    }
}
