//! C Code Generator
//!
//! Translates Aether IR to C code for compilation with clang/gcc.
#![allow(dead_code)]

use std::collections::HashMap;
use std::process::Command;
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
    
    // Structure layouts (struct name -> fields)
    struct_layouts: HashMap<String, Vec<(String, IRType)>>,
    
    // Type tracking
    reg_types: HashMap<Register, IRType>,
    param_types: HashMap<usize, IRType>,
    func_ret_types: HashMap<String, IRType>,
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
            struct_layouts: HashMap::new(),
            reg_types: HashMap::new(),
            param_types: HashMap::new(),
            func_ret_types: HashMap::new(),
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
            // SIMD vector types - use platform-specific intrinsics
            IRType::Vector(elem, lanes) => {
                match (elem.as_ref(), lanes) {
                    // ARM NEON types
                    (IRType::F32, 4) if self.target_triple.contains("aarch64") || self.target_triple.contains("arm") => 
                        "float32x4_t".to_string(),
                    (IRType::F64, 2) if self.target_triple.contains("aarch64") => 
                        "float64x2_t".to_string(),
                    (IRType::I32, 4) if self.target_triple.contains("aarch64") || self.target_triple.contains("arm") => 
                        "int32x4_t".to_string(),
                    (IRType::I64, 2) if self.target_triple.contains("aarch64") => 
                        "int64x2_t".to_string(),
                    // x86 SSE/AVX types  
                    (IRType::F32, 4) => "__m128".to_string(),
                    (IRType::F32, 8) => "__m256".to_string(),
                    (IRType::F64, 2) => "__m128d".to_string(),
                    (IRType::F64, 4) => "__m256d".to_string(),
                    (IRType::I32, 4) => "__m128i".to_string(),
                    (IRType::I32, 8) => "__m256i".to_string(),
                    (IRType::I64, 2) => "__m128i".to_string(),
                    (IRType::I64, 4) => "__m256i".to_string(),
                    // Fallback: use GCC vector extension
                    _ => format!("{} __attribute__((vector_size({})))", 
                        self.ir_type_to_c(elem), 
                        elem.size_bytes() * lanes)
                }
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

    /// Analyze instruction for type inference
    fn analyze_instruction(&mut self, inst: &Instruction) {
        match inst {
            Instruction::Assign { dest, value } => {
                if let Some(ty) = self.get_value_type(value) {
                    self.reg_types.insert(*dest, ty);
                }
            }
            Instruction::BinOp { dest, .. } => {
                self.reg_types.insert(*dest, IRType::I64); 
            }
            Instruction::UnaryOp { dest, value, .. } => {
                 if let Some(ty) = self.get_value_type(value) {
                    self.reg_types.insert(*dest, ty);
                }
            }
            Instruction::Call { dest: Some(dest), func, .. } => {
                let ret_ty = self.func_ret_types.get(func).cloned();
                if let Some(ty) = ret_ty {
                    self.reg_types.insert(*dest, ty);
                }
            }
            Instruction::Alloca { dest, ty } => {
                self.reg_types.insert(*dest, IRType::Ptr(Box::new(ty.clone())));
            }
            Instruction::Load { dest, ptr, ty: _ } => {
                 if let Some(IRType::Ptr(inner)) = self.get_value_type(ptr) {
                    self.reg_types.insert(*dest, *inner);
                }
            }
            Instruction::GetElementPtr { dest, ptr, index, elem_ty: _ } => {
                let ptr_ty = self.get_value_type(ptr);
                if let Some(IRType::Ptr(inner)) = &ptr_ty {
                    if let IRType::Struct(struct_name) = &**inner {
                         if let Value::Constant(Constant::Int(idx)) = index {
                             let field_info = if let Some(fields) = self.struct_layouts.get(struct_name) {
                                 fields.get(*idx as usize).cloned()
                             } else {
                                 None
                             };
                             
                             if let Some((_, field_type)) = field_info {
                                 self.reg_types.insert(*dest, IRType::Ptr(Box::new(field_type)));
                                 return; // Handled
                             }
                         }
                    }
                }
                // Fallback
                if let Some(IRType::Ptr(inner)) = ptr_ty {
                      self.reg_types.insert(*dest, IRType::Ptr(inner));
                }
            }
            Instruction::Phi { dest, incoming } => {
                if let Some((val, _)) = incoming.first() {
                    if let Some(ty) = self.get_value_type(val) {
                        self.reg_types.insert(*dest, ty);
                    }
                }
            }
            Instruction::Cast { dest, ty, .. } => {
                self.reg_types.insert(*dest, ty.clone());
            }
            _ => {}
        }
    }

    /// Generate C code for a function
    fn generate_function(&mut self, func: &IRFunction) -> Result<()> {
        // Reset state for new function
        self.var_names.clear();
        self.var_counter = 0;
        self.block_labels.clear();
        self.reg_types.clear();
        self.param_types.clear();

        // Populate param types
        for (i, (_, ty)) in func.params.iter().enumerate() {
            self.param_types.insert(i, ty.clone());
        }

        // Generate block labels with L_ prefix to avoid C reserved words
        for (i, block) in func.blocks.iter().enumerate() {
            self.block_labels.insert(i, format!("L_{}", block.label));
        }


        // Function signature
        let ret_type = self.ir_type_to_c(&func.ret_type);
        let params: Vec<String> = func.params.iter().enumerate()
            .map(|(i, (_, ty))| format!("{} _arg{}", self.ir_type_to_c(ty), i))
            .collect();
        
        // Generate effect annotations as comments (for documentation/static analysis)
        if !func.contracts.effects.is_empty() {
            let effects_str = func.contracts.effects.join(", ");
            self.writeln(&format!("/* @effects: {} */", effects_str));
        }
        
        // Generate SIMD vectorization hints for @simd annotated functions
        if func.simd {
            self.writeln("/* @simd: auto-vectorization enabled */");
            self.writeln("#if defined(__GNUC__) || defined(__clang__)");
            self.writeln("__attribute__((optimize(\"tree-vectorize\")))");
            self.writeln("#endif");
        }
        
        let params_str = if params.is_empty() { "void".to_string() } else { params.join(", ") };
        self.writeln(&format!("{} {}({}) {{", ret_type, func.name, params_str));
        self.indent += 1;

        // Generate precondition assertions (requires clauses)
        if !func.contracts.requires.is_empty() {
            self.writeln("/* Precondition assertions */");
            for (i, require) in func.contracts.requires.iter().enumerate() {
                self.writeln(&format!("assert({});  /* requires #{} */", require, i + 1));
            }
            self.writeln("");
        }

        // Pass 1: Analyze types
        for block in &func.blocks {
            for inst in &block.instructions {
                self.analyze_instruction(inst);
            }
        }

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
                    Instruction::Phi { dest, .. } |
                    Instruction::Cast { dest, .. } => {
                        let var = self.get_var(*dest);
                        let c_type = self.reg_types.get(dest)
                            .map(|t| self.ir_type_to_c(t))
                            .unwrap_or("int64_t".to_string());
                        declarations.push(format!("{} {};", c_type, var));
                    }
                    Instruction::Call { dest: Some(dest), .. } => {
                        let var = self.get_var(*dest);
                        let c_type = self.reg_types.get(dest)
                            .map(|t| self.ir_type_to_c(t))
                            .unwrap_or("int64_t".to_string());
                        declarations.push(format!("{} {};", c_type, var));
                    }
                    Instruction::InlineAsm { operands, .. } => {
                        for op in operands {
                            if let Some(reg) = op.output {
                                let var = self.get_var(reg);
                                // Default to int64_t for asm outputs for not
                                declarations.push(format!("int64_t {};", var));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Remove duplicates and emit declarations (skip void types)
        declarations.sort();
        declarations.dedup();
        for decl in declarations {
            // Skip void declarations
            if !decl.starts_with("void ") {
                self.writeln(&decl);
            }
        }
        if !func.blocks.is_empty() {
            self.writeln("");
        }

        // Generate code for each block
        for (i, block) in func.blocks.iter().enumerate() {
            // Label (except for entry block)
            if i > 0 {
                let label = &self.block_labels[&i];
                self.indent -= 1;
                self.writeln(&format!("{}:", label));
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

    /// Get type of a value if known
    fn get_value_type(&self, val: &Value) -> Option<IRType> {
        match val {
            Value::Register(reg) => self.reg_types.get(reg).cloned(),
            Value::Parameter(idx) => self.param_types.get(idx).cloned(),
            _ => None,
        }
    }

    /// Generate C code for an instruction

    fn generate_instruction(&mut self, inst: &Instruction) -> Result<()> {
        match inst {
            Instruction::Assign { dest, value } => {
                let var = self.get_var(*dest);
                let val = self.value_to_c(value);
                self.writeln(&format!("{} = {};", var, val));
                
                // Track type
                if let Some(ty) = self.get_value_type(value) {
                    self.reg_types.insert(*dest, ty);
                }
            }
            
            Instruction::BinOp { dest, op, left, right } => {
                let var = self.get_var(*dest);
                let l = self.value_to_c(left);
                let r = self.value_to_c(right);
                let op_str = self.binop_to_c(*op);
                self.writeln(&format!("{} = {} {} {};", var, l, op_str, r));
                self.reg_types.insert(*dest, IRType::I64); // Default to I64 for now
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
                if let Some(ty) = self.get_value_type(value) {
                    self.reg_types.insert(*dest, ty);
                }
            }
            
            Instruction::Call { dest, func, args } => {
                let args_str: Vec<_> = args.iter().map(|a| self.value_to_c(a)).collect();
                
                // Map built-in function names to C runtime functions
                let (c_func, is_builtin_void) = match func.as_str() {
                    "print" => ("aether_print", true),
                    "println" => ("aether_println", true),
                    "print_i64" => ("aether_print_i64", true),
                    "println_i64" => ("aether_println_i64", true),
                    "assert" => ("aether_assert", true),
                    "alloc" => ("malloc", false),
                    "free" => ("free", true),
                    "exit" => ("exit", true),
                    // SIMD intrinsics - map to platform-specific calls
                    "f32x4_splat" => ("_mm_set1_ps", false),
                    "f32x4_add" => ("_mm_add_ps", false),
                    "f32x4_sub" => ("_mm_sub_ps", false),
                    "f32x4_mul" => ("_mm_mul_ps", false),
                    "f32x4_div" => ("_mm_div_ps", false),
                    "f64x2_splat" => ("_mm_set1_pd", false),
                    "f64x2_add" => ("_mm_add_pd", false),
                    "f64x2_mul" => ("_mm_mul_pd", false),
                    "i32x4_splat" => ("_mm_set1_epi32", false),
                    "i32x4_add" => ("_mm_add_epi32", false),
                    "i32x4_mul" => ("_mm_mullo_epi32", false),
                    _ => (func.as_str(), false),
                };
                
                // Check if user-defined function returns void
                let ret_ty = self.func_ret_types.get(func).cloned();
                let is_void = is_builtin_void || matches!(ret_ty, Some(IRType::Void)) || matches!(ret_ty, None);
                
                let call = format!("{}({})", c_func, args_str.join(", "));
                
                // For void functions, don't assign to a variable
                if is_void || dest.is_none() {
                    self.writeln(&format!("{};", call));
                } else if let Some(d) = dest {
                    let var = self.get_var(*d);
                    self.writeln(&format!("{} = {};", var, call));
                    
                    if let Some(ty) = ret_ty {
                        self.reg_types.insert(*d, ty);
                    }
                }
            }
            
            Instruction::Alloca { dest, ty } => {
                let var = self.get_var(*dest);
                let c_type = self.ir_type_to_c(ty);
                // Alloca in C is just a local variable
                self.writeln(&format!("{} _alloca_{};", c_type, var));
                self.writeln(&format!("{} = &_alloca_{};", var, var));
                
                self.reg_types.insert(*dest, IRType::Ptr(Box::new(ty.clone())));
            }
            
            Instruction::Load { dest, ptr, ty: _ } => {
                let var = self.get_var(*dest);
                let p = self.value_to_c(ptr);
                self.writeln(&format!("{} = *{};", var, p));
                
                if let Some(IRType::Ptr(inner)) = self.get_value_type(ptr) {
                    self.reg_types.insert(*dest, *inner);
                }
            }
            
            Instruction::Store { ptr, value } => {
                let p = self.value_to_c(ptr);
                let val = self.value_to_c(value);
                
                // Check if we're storing a struct pointer to a struct field
                // In that case, we need to dereference the value
                let ptr_ty = self.get_value_type(ptr);
                let val_ty = self.get_value_type(value);
                
                if let (Some(IRType::Ptr(ptr_inner)), Some(IRType::Ptr(val_inner))) = (&ptr_ty, &val_ty) {
                    // Both are pointers - check if storing struct* to struct field
                    if let IRType::Struct(_) = &**ptr_inner {
                        if let IRType::Struct(_) = &**val_inner {
                            // Dereference the value: *ptr = *val
                            self.writeln(&format!("*{} = *{};", p, val));
                            return Ok(());
                        }
                    }
                }
                
                self.writeln(&format!("*{} = {};", p, val));
            }
            
            Instruction::GetElementPtr { dest, ptr, index, elem_ty: _ } => {
                let ptr_ty = self.get_value_type(ptr);
                let mut handled = false;
                
                // Check if this is a struct field access
                if let Some(IRType::Ptr(inner)) = &ptr_ty {
                    if let IRType::Struct(struct_name) = &**inner {
                        if let Value::Constant(Constant::Int(idx)) = index {
                             let field_info = if let Some(fields) = self.struct_layouts.get(struct_name) {
                                 fields.get(*idx as usize).cloned()
                             } else {
                                 None
                             };

                             if let Some((field_name, field_type)) = field_info {
                                 let var = self.get_var(*dest);
                                 let p = self.value_to_c(ptr);
                                 self.writeln(&format!("{} = &{}->{};", var, p, field_name));
                                 
                                 self.reg_types.insert(*dest, IRType::Ptr(Box::new(field_type)));
                                 handled = true;
                             }
                        }
                    }
                }
                
                if !handled {
                    let var = self.get_var(*dest);
                    let p = self.value_to_c(ptr);
                    let idx = self.value_to_c(index);
                    self.writeln(&format!("{} = &{}[{}];", var, p, idx));
                    
                    if let Some(IRType::Ptr(inner)) = ptr_ty {
                         self.reg_types.insert(*dest, IRType::Ptr(inner));
                     }
                }
            }

            Instruction::Cast { dest, value, ty } => {
                let var = self.get_var(*dest);
                let val = self.value_to_c(value);
                let type_name = self.ir_type_to_c(ty);
                self.writeln(&format!("{} = ({}){};", var, type_name, val));
                self.reg_types.insert(*dest, ty.clone());
            }

            Instruction::InlineAsm { template, operands } => {
                let mut inputs = Vec::new();
                let mut outputs = Vec::new();
                let mut clobbers = Vec::new();
                
                for op in operands {
                    match op.kind {
                        IRAsmOperandKind::Input => {
                            if let Some(ref val) = op.input {
                                let val_str = self.value_to_c(val);
                                inputs.push(format!("\"{}\" ({})", op.constraint, val_str));
                            }
                        }
                        IRAsmOperandKind::Output => {
                            if let Some(reg) = op.output {
                                let var = self.get_var(reg);
                                let constraint = if !op.constraint.starts_with('=') && !op.constraint.starts_with('+') {
                                    format!("={}", op.constraint)
                                } else {
                                    op.constraint.clone()
                                };
                                outputs.push(format!("\"{}\" ({})", constraint, var));
                                self.reg_types.insert(reg, IRType::I64); // Default to I64
                            }
                        }
                        IRAsmOperandKind::InOut => {
                             if let Some(reg) = op.output {
                                 let var = self.get_var(reg);
                                 if let Some(ref val) = op.input {
                                     let val_str = self.value_to_c(val);
                                     self.writeln(&format!("{} = {};", var, val_str));
                                 }
                                 let constraint = if !op.constraint.starts_with('+') {
                                    format!("+{}", op.constraint)
                                } else {
                                    op.constraint.clone()
                                };
                                outputs.push(format!("\"{}\" ({})", constraint, var));
                                self.reg_types.insert(reg, IRType::I64); 
                             }
                        }
                        IRAsmOperandKind::Clobber => {
                             clobbers.push(format!("\"{}\"", op.constraint));
                        }
                    }
                }
                
                let outputs_str = outputs.join(", ");
                let inputs_str = inputs.join(", ");
                let clobbers_str = clobbers.join(", ");
                
                self.writeln(&format!("__asm__ volatile (\"{}\" : {} : {} : {});", 
                    template, outputs_str, inputs_str, clobbers_str));
            }
            
            Instruction::Phi { dest, incoming } => {
                // Phi nodes are handled by predecessor blocks in structured C
                // For now, just use the first incoming value as a placeholder
                if let Some((val, _)) = incoming.first() {
                    let var = self.get_var(*dest);
                    let v = self.value_to_c(val);
                    self.writeln(&format!("{} = {};", var, v));
                    
                    if let Some(ty) = self.get_value_type(val) {
                        self.reg_types.insert(*dest, ty);
                    }
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
        // SIMD headers (platform-specific)
        if self.target_triple.contains("aarch64") || self.target_triple.contains("arm") {
            self.writeln("#include <arm_neon.h>");
        } else {
            self.writeln("#include <immintrin.h>  /* SSE/AVX */");
        }
        self.writeln("");
        
        // Runtime support functions
        self.writeln("/* AetherLang Runtime */");
        self.writeln("static void aether_print(const char* s) { printf(\"%s\", s); }");
        self.writeln("static void aether_println(const char* s) { printf(\"%s\\n\", s); }");
        self.writeln("static void aether_print_i64(int64_t n) { printf(\"%lld\", (long long)n); }");
        self.writeln("static void aether_println_i64(int64_t n) { printf(\"%lld\\n\", (long long)n); }");
        self.writeln("static void aether_assert(bool c) { if(!c) { fprintf(stderr, \"Assertion failed\\n\"); exit(1); } }");
        self.writeln("");
        
        // Struct definitions
        self.writeln("/* Struct Definitions */");
        for struct_def in &module.structs {
            // Add GCC attributes based on repr
            let attr = match struct_def.repr {
                StructRepr::C => "",  // C layout is default for GCC structs
                StructRepr::Packed => " __attribute__((packed))",
                StructRepr::Transparent => "",  // Transparent is a Rust concept, no direct C equivalent
                StructRepr::Default => "",  // No special attribute
            };
            self.writeln(&format!("struct {}{} {{", struct_def.name, attr));
            for (field_name, field_type) in &struct_def.fields {
                let c_type = self.ir_type_to_c(field_type);
                self.writeln(&format!("    {} {};", c_type, field_name));
            }
            self.writeln("};");
            self.writeln("");
        }
            
        // Populate layout map
        for struct_def in &module.structs {
            self.struct_layouts.insert(struct_def.name.clone(), struct_def.fields.clone());
        }
        
        // Populate function return types
        for func in &module.functions {
            self.func_ret_types.insert(func.name.clone(), func.ret_type.clone());
        }
        
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
