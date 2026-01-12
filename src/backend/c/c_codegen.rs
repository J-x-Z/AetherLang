//! C Code Generator
//!
//! Translates Aether IR to C code for compilation with clang/gcc.

use std::fmt::Write;
use std::collections::HashMap;
use std::process::Command;
use std::path::Path;
use std::fs;

use crate::backend::codegen::CodeGen;
use crate::middle::ir::*;
use crate::utils::{Error, Result};

/// C code generator
pub struct CCodeGen {
    target_triple: String,
    output: String,
    indent: usize,
    /// Map from register to C variable name
    var_names: HashMap<Register, String>,
    /// Counter for generating unique variable names
    var_counter: usize,
    /// Map from block ID to label name
    block_labels: HashMap<usize, String>,
}

impl CCodeGen {
    pub fn new(target: &str) -> Self {
        Self {
            target_triple: target.to_string(),
            output: String::new(),
            indent: 0,
            var_names: HashMap::new(),
            var_counter: 0,
            block_labels: HashMap::new(),
        }
    }

    /// Generate a unique variable name
    fn fresh_var(&mut self) -> String {
        let name = format!("_t{}", self.var_counter);
        self.var_counter += 1;
        name
    }

    /// Get variable name for a register
    fn get_var(&mut self, reg: Register) -> String {
        if let Some(name) = self.var_names.get(&reg) {
            name.clone()
        } else {
            let name = self.fresh_var();
            self.var_names.insert(reg, name.clone());
            name
        }
    }

    /// Write indented line
    fn writeln(&mut self, line: &str) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
        self.output.push_str(line);
        self.output.push('\n');
    }

    /// Write raw line (no indent)
    fn write_raw(&mut self, line: &str) {
        self.output.push_str(line);
    }

    /// Convert IR type to C type
    fn ir_type_to_c(&self, ty: &IRType) -> String {
        match ty {
            IRType::Void => "void".to_string(),
            IRType::Bool => "bool".to_string(),
            IRType::I8 => "int8_t".to_string(),
            IRType::I16 => "int16_t".to_string(),
            IRType::I32 => "int32_t".to_string(),
            IRType::I64 => "int64_t".to_string(),
            IRType::U8 => "uint8_t".to_string(),
            IRType::U16 => "uint16_t".to_string(),
            IRType::U32 => "uint32_t".to_string(),
            IRType::U64 => "uint64_t".to_string(),
            IRType::F32 => "float".to_string(),
            IRType::F64 => "double".to_string(),
            IRType::Ptr(inner) => format!("{}*", self.ir_type_to_c(inner)),
            IRType::Array(elem, size) => format!("{}[{}]", self.ir_type_to_c(elem), size),
            IRType::Struct(name) => format!("struct {}", name),
            IRType::Function { params, ret } => {
                let params_str: Vec<_> = params.iter().map(|p| self.ir_type_to_c(p)).collect();
                format!("{}(*)({})", self.ir_type_to_c(ret), params_str.join(", "))
            }
        }
    }

    /// Convert binary operator to C operator
    fn binop_to_c(&self, op: BinOp) -> &'static str {
        match op {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::And => "&&",
            BinOp::Or => "||",
            BinOp::Xor => "^",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
        }
    }

    /// Convert value to C expression
    fn value_to_c(&mut self, val: &Value) -> String {
        match val {
            Value::Register(reg) => self.get_var(*reg),
            Value::Constant(c) => match c {
                Constant::Int(n) => format!("{}LL", n),
                Constant::Float(f) => format!("{}", f),
                Constant::Bool(b) => if *b { "1" } else { "0" }.to_string(),
                Constant::String(s) => format!("\"{}\"", s.escape_default()),
                Constant::Null => "NULL".to_string(),
            },
            Value::Parameter(i) => format!("_arg{}", i),
            Value::Global(name) => name.clone(),
            Value::Unit => "((void)0)".to_string(),
        }
    }

    /// Generate C code for a function
    fn generate_function(&mut self, func: &IRFunction) -> Result<()> {
        // Reset state for new function
        self.var_names.clear();
        self.var_counter = 0;
        self.block_labels.clear();

        // Generate block labels
        for (i, block) in func.blocks.iter().enumerate() {
            self.block_labels.insert(i, block.label.clone());
        }

        // Function signature
        let ret_type = self.ir_type_to_c(&func.ret_type);
        let params: Vec<String> = func.params.iter().enumerate()
            .map(|(i, (_, ty))| format!("{} _arg{}", self.ir_type_to_c(ty), i))
            .collect();
        
        let params_str = if params.is_empty() { "void".to_string() } else { params.join(", ") };
        self.writeln(&format!("{} {}({}) {{", ret_type, func.name, params_str));
        self.indent += 1;

        // Declare all variables upfront (C89 style for compatibility)
        let mut declarations = Vec::new();
        for block in &func.blocks {
            for inst in &block.instructions {
                match inst {
                    Instruction::Assign { dest, .. } |
                    Instruction::BinOp { dest, .. } |
                    Instruction::UnaryOp { dest, .. } |
                    Instruction::Alloca { dest, .. } |
                    Instruction::Load { dest, .. } |
                    Instruction::GetElementPtr { dest, .. } |
                    Instruction::Phi { dest, .. } => {
                        let var = self.get_var(*dest);
                        // Use int64_t as default type (we'd need type info for proper types)
                        declarations.push(format!("int64_t {};", var));
                    }
                    Instruction::Call { dest: Some(dest), .. } => {
                        let var = self.get_var(*dest);
                        declarations.push(format!("int64_t {};", var));
                    }
                    _ => {}
                }
            }
        }

        // Remove duplicates and emit declarations
        declarations.sort();
        declarations.dedup();
        for decl in declarations {
            self.writeln(&decl);
        }
        if !func.blocks.is_empty() {
            self.writeln("");
        }

        // Generate code for each block
        for (i, block) in func.blocks.iter().enumerate() {
            // Label (except for entry block)
            if i > 0 {
                self.indent -= 1;
                self.writeln(&format!("{}:", block.label));
                self.indent += 1;
            }

            // Instructions
            for inst in &block.instructions {
                self.generate_instruction(inst)?;
            }

            // Terminator
            if let Some(ref term) = block.terminator {
                self.generate_terminator(term)?;
            }
        }

        self.indent -= 1;
        self.writeln("}");
        self.writeln("");

        Ok(())
    }

    /// Generate C code for an instruction
    fn generate_instruction(&mut self, inst: &Instruction) -> Result<()> {
        match inst {
            Instruction::Assign { dest, value } => {
                let var = self.get_var(*dest);
                let val = self.value_to_c(value);
                self.writeln(&format!("{} = {};", var, val));
            }
            
            Instruction::BinOp { dest, op, left, right } => {
                let var = self.get_var(*dest);
                let l = self.value_to_c(left);
                let r = self.value_to_c(right);
                let op_str = self.binop_to_c(*op);
                self.writeln(&format!("{} = {} {} {};", var, l, op_str, r));
            }
            
            Instruction::UnaryOp { dest, op, value } => {
                let var = self.get_var(*dest);
                let val = self.value_to_c(value);
                let op_str = match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "!",
                    UnaryOp::BitNot => "~",
                };
                self.writeln(&format!("{} = {}{};", var, op_str, val));
            }
            
            Instruction::Call { dest, func, args } => {
                let args_str: Vec<_> = args.iter().map(|a| self.value_to_c(a)).collect();
                
                // Map built-in function names to C runtime functions
                let c_func = match func.as_str() {
                    "print" => "aether_print",
                    "println" => "aether_println",
                    "print_i64" => "aether_print_i64",
                    "println_i64" => "aether_println_i64",
                    "assert" => "aether_assert",
                    "alloc" => "malloc",
                    "free" => "free",
                    "exit" => "exit",
                    _ => func.as_str(),
                };
                
                let call = format!("{}({})", c_func, args_str.join(", "));
                
                if let Some(d) = dest {
                    let var = self.get_var(*d);
                    self.writeln(&format!("{} = {};", var, call));
                } else {
                    self.writeln(&format!("{};", call));
                }
            }

            
            Instruction::Alloca { dest, ty } => {
                let var = self.get_var(*dest);
                let c_type = self.ir_type_to_c(ty);
                // Alloca in C is just a local variable
                self.writeln(&format!("{} _alloca_{};", c_type, var));
                self.writeln(&format!("{} = &_alloca_{};", var, var));
            }
            
            Instruction::Load { dest, ptr } => {
                let var = self.get_var(*dest);
                let p = self.value_to_c(ptr);
                self.writeln(&format!("{} = *{};", var, p));
            }
            
            Instruction::Store { ptr, value } => {
                let p = self.value_to_c(ptr);
                let val = self.value_to_c(value);
                self.writeln(&format!("*{} = {};", p, val));
            }
            
            Instruction::GetElementPtr { dest, ptr, index } => {
                let var = self.get_var(*dest);
                let p = self.value_to_c(ptr);
                let idx = self.value_to_c(index);
                self.writeln(&format!("{} = &{}[{}];", var, p, idx));
            }
            
            Instruction::Phi { dest, incoming } => {
                // Phi nodes are handled by predecessor blocks in structured C
                // For now, just use the first incoming value as a placeholder
                if let Some((val, _)) = incoming.first() {
                    let var = self.get_var(*dest);
                    let v = self.value_to_c(val);
                    self.writeln(&format!("{} = {};", var, v));
                }
            }
        }
        Ok(())
    }

    /// Generate C code for a terminator
    fn generate_terminator(&mut self, term: &Terminator) -> Result<()> {
        match term {
            Terminator::Return { value } => {
                if let Some(val) = value {
                    let v = self.value_to_c(val);
                    self.writeln(&format!("return {};", v));
                } else {
                    self.writeln("return;");
                }
            }
            
            Terminator::Jump { target } => {
                let label = &self.block_labels[&target.0];
                self.writeln(&format!("goto {};", label));
            }
            
            Terminator::Branch { cond, then_target, else_target } => {
                let c = self.value_to_c(cond);
                let then_label = self.block_labels[&then_target.0].clone();
                let else_label = self.block_labels[&else_target.0].clone();
                self.writeln(&format!("if ({}) goto {}; else goto {};", c, then_label, else_label));
            }
            
            Terminator::Unreachable => {
                self.writeln("__builtin_unreachable();");
            }
        }
        Ok(())
    }

    /// Generate the complete C source file
    pub fn generate_source(&mut self, module: &IRModule) -> Result<String> {
        self.output.clear();
        
        // Header
        self.writeln("/* Generated by AetherLang C Backend */");
        self.writeln("#include <stdint.h>");
        self.writeln("#include <stdbool.h>");
        self.writeln("#include <stdio.h>");
        self.writeln("#include <stdlib.h>");
        self.writeln("");
        
        // Runtime support functions
        self.writeln("/* AetherLang Runtime */");
        self.writeln("static void aether_print(const char* s) { printf(\"%s\", s); }");
        self.writeln("static void aether_println(const char* s) { printf(\"%s\\n\", s); }");
        self.writeln("static void aether_print_i64(int64_t n) { printf(\"%lld\", (long long)n); }");
        self.writeln("static void aether_println_i64(int64_t n) { printf(\"%lld\\n\", (long long)n); }");
        self.writeln("static void aether_assert(bool c) { if(!c) { fprintf(stderr, \"Assertion failed\\n\"); exit(1); } }");
        self.writeln("");
        
        // Forward declarations
        for func in &module.functions {
            let ret_type = self.ir_type_to_c(&func.ret_type);
            let params: Vec<String> = func.params.iter()
                .map(|(_, ty)| self.ir_type_to_c(ty))
                .collect();
            let params_str = if params.is_empty() { "void".to_string() } else { params.join(", ") };
            self.writeln(&format!("{} {}({});", ret_type, func.name, params_str));
        }
        self.writeln("");
        
        // Function definitions
        for func in &module.functions {
            self.generate_function(func)?;
        }
        
        Ok(self.output.clone())
    }

    /// Compile C source to object file using clang/gcc
    fn compile_c_to_object(&self, c_source: &str) -> Result<Vec<u8>> {
        // Write C source to temp file
        let temp_dir = std::env::temp_dir();
        let c_file = temp_dir.join("aether_temp.c");
        let obj_file = temp_dir.join("aether_temp.o");
        
        fs::write(&c_file, c_source).map_err(|e| Error::Io(e.to_string()))?;
        
        // Try clang first, then gcc
        let compilers = ["clang", "gcc", "cc"];
        let mut last_error = String::new();
        
        for compiler in &compilers {
            let result = Command::new(compiler)
                .args(&["-c", "-o"])
                .arg(&obj_file)
                .arg(&c_file)
                .output();
            
            match result {
                Ok(output) if output.status.success() => {
                    // Read object file
                    let obj_bytes = fs::read(&obj_file)
                        .map_err(|e| Error::Io(e.to_string()))?;
                    
                    // Cleanup
                    let _ = fs::remove_file(&c_file);
                    let _ = fs::remove_file(&obj_file);
                    
                    return Ok(obj_bytes);
                }
                Ok(output) => {
                    last_error = String::from_utf8_lossy(&output.stderr).to_string();
                }
                Err(e) => {
                    last_error = e.to_string();
                }
            }
        }
        
        // Cleanup on failure
        let _ = fs::remove_file(&c_file);
        
        Err(Error::CodeGen(format!("Failed to compile C code: {}", last_error)))
    }

    /// Get the generated C source (for debugging)
    pub fn get_c_source(&self) -> &str {
        &self.output
    }
}

impl CodeGen for CCodeGen {
    fn generate(&mut self, module: &IRModule) -> Result<Vec<u8>> {
        let c_source = self.generate_source(module)?;
        self.compile_c_to_object(&c_source)
    }
    
    fn target_triple(&self) -> &str {
        &self.target_triple
    }
    
    fn name(&self) -> &str {
        "C"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::lexer::Lexer;
    use crate::frontend::parser::Parser;
    use crate::middle::ir_gen::IRGenerator;

    fn compile_to_ir(source: &str) -> IRModule {
        let lexer = Lexer::new(source, 0);
        let mut parser = Parser::new(lexer);
        let program = parser.parse_program().unwrap();
        let mut gen = IRGenerator::new("test");
        gen.generate(&program).unwrap()
    }

    fn generate_c(source: &str) -> String {
        let ir_module = compile_to_ir(source);
        let mut codegen = CCodeGen::new("x86_64-pc-windows-msvc");
        codegen.generate_source(&ir_module).unwrap()
    }

    #[test]
    fn test_empty_function() {
        let c = generate_c("fn main() {}");
        println!("{}", c);
        assert!(c.contains("void main(void)"));
        assert!(c.contains("return;"));
    }

    #[test]
    fn test_return_constant() {
        let c = generate_c("fn answer() -> i64 { return 42 }");
        println!("{}", c);
        assert!(c.contains("return 42"));
    }

    #[test]
    fn test_binary_expression() {
        let c = generate_c("fn add() -> i64 { return 1 + 2 }");
        println!("{}", c);
        assert!(c.contains("1LL + 2LL"));
    }

    #[test]
    fn test_if_expression() {
        let c = generate_c("fn test() { if true { return } else { return } }");
        println!("{}", c);
        assert!(c.contains("if"));
        assert!(c.contains("goto"));
    }
}
