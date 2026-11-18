//! C Code Generator
//!
//! Translates Aether IR to C code for compilation with clang/gcc.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
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
    union_names: HashSet<String>,
    global_types: HashMap<String, IRType>,
    
    // Type tracking
    reg_types: HashMap<Register, IRType>,
    param_types: HashMap<usize, IRType>,
    func_ret_types: HashMap<String, IRType>,
    
    // Track globals used (for enum variants)
    globals_used: HashSet<String>,
    
    // Track calls to undefined functions (potential enum variant constructors)
    undefined_calls: HashSet<(String, usize)>,  // (func_name, arg_count)
    
    // Current function being generated (for main's argc/argv handling)
    current_func_name: String,
    current_func_param_count: usize,
    extern_conversions: HashMap<String, String>,
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
            union_names: HashSet::new(),
            global_types: HashMap::new(),
            reg_types: HashMap::new(),
            param_types: HashMap::new(),
            func_ret_types: HashMap::new(),
            globals_used: HashSet::new(),
            undefined_calls: HashSet::new(),
            current_func_name: String::new(),
            current_func_param_count: 0,
            extern_conversions: HashMap::new(),
        }


    }

    /// The C library writes a char* through its second parameter. Aether
    /// represents byte addresses as uint8_t*, and the slot may be unaligned.
    /// A native temporary and a byte copy preserve both types and alignment.
    fn emit_host_conversions(&mut self, module: &IRModule) {
        let mut occupied: HashSet<_> = module.externs.iter().map(|f| f.name.clone())
            .chain(module.functions.iter().map(|f| f.name.clone()))
            .chain(module.globals.iter().map(|g| g.name.clone())).collect();
        self.extern_conversions.clear();
        for ext in &module.externs {
            let Some(conversion) = crate::intrinsics::NumberConversion::lookup(&ext.name) else { continue };
            let mut wrapper = format!("__aeth_parse_{}", ext.name);
            while !occupied.insert(wrapper.clone()) { wrapper.push('_'); }
            let base_param = if conversion.has_base() { ", int base" } else { "" };
            let base_arg = if conversion.has_base() { ", base" } else { "" };
            let ty = conversion.c_result();
            self.writeln(&format!("static inline {ty} {wrapper}(const void *input, void *slot{base_param}) {{"));
            self.writeln("    char *end;");
            self.writeln(&format!("    {ty} value = {}((const char*)input, slot ? &end : NULL{base_arg});", ext.name));
            self.writeln("    if (slot) __builtin_memcpy(slot, &end, sizeof(end));");
            self.writeln("    return value;");
            self.writeln("}");
            self.extern_conversions.insert(ext.name.clone(), wrapper);
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

    /// Escape string for C (converts to valid C escape sequences)
    fn escape_for_c(s: &str) -> String {
        let mut result = String::new();
        for c in s.chars() {
            match c {
                // C octal escapes consume up to three digits: a short \0
                // would absorb a following source byte in the range 0..7.
                '\0' => result.push_str("\\000"),
                '\n' => result.push_str("\\n"),
                '\r' => result.push_str("\\r"),
                '\t' => result.push_str("\\t"),
                '\\' => result.push_str("\\\\"),
                '"' => result.push_str("\\\""),
                c if c.is_ascii() && !c.is_control() => result.push(c),
                c => {
                    // Fixed-width octal cannot absorb any following digit.
                    for b in c.to_string().bytes() {
                        result.push_str(&format!("\\{:03o}", b));
                    }
                }
            }
        }
        result
    }

    fn array_name(ty: &IRType) -> String {
        fn key(ty: &IRType) -> String {
            match ty {
                IRType::Array(elem, len) => format!("a{}_{}", len, key(elem)),
                IRType::Vector(elem, lanes) => format!("vec{}_{}", lanes, key(elem)),
                IRType::Ptr(elem) => format!("p_{}", key(elem)),
                IRType::Volatile(elem) => format!("v_{}", key(elem)),
                IRType::Struct(name) => format!("s{}_{}", name.len(), name),
                other => format!("{:?}", other).to_lowercase(),
            }
        }
        format!("__aeth_array_{}", key(ty))
    }

    fn emit_layout(&mut self, ty: &IRType, module: &IRModule, emitted: &mut HashSet<String>) -> Result<()> {
        match ty {
            IRType::Volatile(inner) => self.emit_layout(inner, module, emitted)?,
            IRType::Ptr(inner) => { // Pointer targets need a tag, but no complete layout.
                if matches!(inner.as_ref(), IRType::Array(_, _)) {
                    self.writeln(&format!("struct {};", Self::array_name(inner)));
                }
            }
            IRType::Array(elem, size) => {
                let name = Self::array_name(ty);
                if emitted.contains(&name) { return Ok(()); }
                self.emit_layout(elem, module, emitted)?;
                self.writeln(&format!("struct {} {{ {} data[{}]; }};", name, self.ir_type_to_c(elem), size));
                emitted.insert(name);
            }
            IRType::Struct(name) => {
                if emitted.contains(name) { return Ok(()); }
                let definition = module.structs.iter().find(|d| d.name == *name)
                    .ok_or_else(|| Error::CodeGen(format!("missing struct layout for {}", name)))?;
                for (_, field) in &definition.fields { self.emit_layout(field, module, emitted)?; }
                let attr = if definition.repr == StructRepr::Packed { " __attribute__((packed))" } else { "" };
                self.writeln(&format!("{}{} {} {{", if definition.is_union { "union" } else { "struct" }, attr, name));
                for (name, field) in &definition.fields { self.writeln(&format!("    {} {};", self.ir_type_to_c(field), name)); }
                self.writeln("};");
                emitted.insert(name.clone());
            }
            _ => {}
        }
        Ok(())
    }

    fn layout_expression(&self, query: IRLayoutQuery, ty: &IRType, field: Option<usize>) -> Result<String> {
        if *ty == IRType::Void { return Ok(match query { IRLayoutQuery::Size => "0", IRLayoutQuery::Align => "1", IRLayoutQuery::Offset => return Err(Error::CodeGen("unit has no field offsets".into())) }.into()); }
        Ok(match query {
            IRLayoutQuery::Size => format!("sizeof({})", self.ir_type_to_c(ty)),
            IRLayoutQuery::Align => format!("_Alignof({})", self.ir_type_to_c(ty)),
            IRLayoutQuery::Offset => {
                let name = match ty { IRType::Struct(name) => name, _ => return Err(Error::CodeGen("offset_of requires an aggregate".into())) };
                let field = &self.struct_layouts.get(name).and_then(|fields| fields.get(field.unwrap_or(usize::MAX)))
                    .ok_or_else(|| Error::CodeGen("offset_of unknown field".into()))?.0;
                format!("offsetof({}, {})", self.ir_type_to_c(ty), field)
            }
        })
    }

    fn initializer_to_c(&mut self, value: &IRInitializer, ty: &IRType) -> Result<String> {
        Ok(match value {
            IRInitializer::Zero => if matches!(ty, IRType::Unit) { "{}".into() } else if matches!(ty, IRType::Struct(_) | IRType::Array(_, _)) { "{0}".into() } else { "0".into() },
            IRInitializer::Scalar(value) => format!("({})({})", self.ir_type_to_c(ty), self.value_to_c(&Value::Constant(value.clone()))),
            IRInitializer::Layout { query, ty, field } => self.layout_expression(*query, ty, *field)?,
            IRInitializer::Address { global, path } => {
                let mut current = self.global_types.get(global).cloned().ok_or_else(|| Error::CodeGen(format!("unknown static {}", global)))?;
                let mut offsets = Vec::new();
                for index in path {
                    match current {
                        IRType::Array(element, _) => {
                            offsets.push(format!("{} * sizeof({})", index, self.ir_type_to_c(&element)));
                            current = *element;
                        }
                        IRType::Struct(ref name) => {
                            let (field, field_ty) = self.struct_layouts[name][*index].clone();
                            offsets.push(format!("offsetof({}, {})", self.ir_type_to_c(&current), field));
                            current = field_ty;
                        }
                        _ => return Err(Error::CodeGen("invalid static address path".into())),
                    }
                }
                format!("({})((unsigned char*)&{} + ({}))", self.ir_type_to_c(ty), global, if offsets.is_empty() { "0".into() } else { offsets.join(" + ") })
            }
            IRInitializer::Repeat(value,count)=>{
                let IRType::Array(element,size)=ty else{return Err(Error::CodeGen("repeat initializer requires array type".into()));};
                if count!=size{return Err(Error::CodeGen("repeat initializer length mismatch".into()));}
                let value=self.initializer_to_c(value,element)?;
                format!("{{ .data = {{ {} }} }}",vec![value;*count].join(", "))
            }
            IRInitializer::Aggregate(values) => {
                let mut fields = Vec::new();
                match ty {
                    IRType::Array(element, _) => {
                        for (index, value) in values { fields.push(format!("[{}] = {}", index, self.initializer_to_c(value, element)?)); }
                        format!("{{ .data = {{ {} }} }}", fields.join(", "))
                    }
                    IRType::Struct(name) => {
                        for (index, value) in values {
                            let (name, ty) = self.struct_layouts[name][*index].clone();
                            fields.push(format!(".{} = {}", name, self.initializer_to_c(value, &ty)?));
                        }
                        format!("{{ {} }}", fields.join(", "))
                    }
                    _ => return Err(Error::CodeGen("non-aggregate static initializer".into())),
                }
            }
        })
    }

    /// Convert IR type to C type
    fn ir_type_to_c(&self, ty: &IRType) -> String {
        match ty {
            IRType::Void => "void".to_string(),
            IRType::Unit => "struct __aeth_unit".to_string(),
            IRType::Volatile(inner) => self.ir_type_to_c(inner),
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
            // `void` is the language unit alias, so both *void and *() carry
            // Ptr(Unit) in IR. Their C storage/ABI type must be void*, which
            // interoperates with opaque C objects such as FILE*. Keep the
            // pointee's Unit IR type: loads, stores and GEP still use its
            // zero-byte layout rather than C's sizeof(void) extension.
            IRType::Ptr(inner) if matches!(inner.unqualified(), IRType::Unit) => "void*".to_string(),
            IRType::Ptr(inner) => format!("{}*", self.ir_type_to_c(inner)),
            IRType::Array(_, _) => format!("struct {}", Self::array_name(ty)),
            IRType::Struct(name) => format!("{} {}", if self.union_names.contains(name) { "union" } else { "struct" }, name),
            IRType::Function { params, ret } => {
                let params_str: Vec<_> = params.iter().map(|p| self.ir_type_to_c(p)).collect();
                format!("{}(*)({})", self.ir_type_to_c(ret), params_str.join(", "))
            }
            IRType::Vector(elem, lanes) => format!("__aeth_v_{}_{}", self.ir_type_to_c(elem),lanes),
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
            BinOp::And => "&",
            BinOp::Or => "|",
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
                Constant::Int(n) if *n == i64::MIN => "INT64_MIN".to_string(),
                Constant::Int(n) => format!("{}LL", n),
                Constant::Float(f) if f.is_nan() => "__builtin_nan(\"\")".into(),
                Constant::Float(f) if f.is_infinite() => if f.is_sign_negative() { "(-__builtin_inf())".into() } else { "__builtin_inf()".into() },
                Constant::Float(f) => format!("{:?}", f),
                Constant::Bool(b) => if *b { "1" } else { "0" }.to_string(),
                Constant::String(s) => format!("\"{}\"", Self::escape_for_c(s)),
                Constant::Null => "NULL".to_string(),
            },
            Value::Parameter(i) => {
                // Special handling for main's argc/argv
                if self.current_func_name == "main" && self.current_func_param_count == 2 {
                    match i {
                        0 => "argc".to_string(),
                        1 => "((uint8_t**)argv)".to_string(),
                        _ => format!("_arg{}", i),
                    }
                } else {
                    format!("_arg{}", i)
                }
            },
            Value::Global(name) => {
                // Track this global for later definition generation
                self.globals_used.insert(name.clone());
                format!("&{}", name)
            },
            Value::Unit => "(struct __aeth_unit){}".to_string(),
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
            Instruction::BinOp { dest, op, left, .. } => {
                let ty = if matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge) {
                    IRType::Bool
                } else { self.get_value_type(left).unwrap_or(IRType::I64) };
                self.reg_types.insert(*dest, ty);
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
            Instruction::ArrayElementPtr { dest, array_ty, .. } => {
                if let IRType::Array(elem, _) = array_ty { self.reg_types.insert(*dest, IRType::Ptr(elem.clone())); }
            }
            Instruction::GetElementPtr { dest, elem_ty, .. } => {
                self.reg_types.insert(*dest, IRType::Ptr(Box::new(elem_ty.clone())));
            }
            Instruction::GetFieldPtr { dest, field, struct_ty, .. } => {
                if let IRType::Struct(name) = struct_ty {
                    if let Some((_, ty)) = self.struct_layouts.get(name).and_then(|fields| fields.get(*field)) {
                        self.reg_types.insert(*dest, IRType::Ptr(Box::new(ty.clone())));
                    }
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
        // Track current function for argc/argv handling in main
        self.current_func_name = func.name.clone();
        self.current_func_param_count = func.params.len();


        // Populate param types
        for (i, (_, ty)) in func.params.iter().enumerate() {
            self.param_types.insert(i, ty.clone());
        }

        // Generate block labels with L_ prefix and index to avoid conflicts and C reserved words
        for (i, block) in func.blocks.iter().enumerate() {
            self.block_labels.insert(i, format!("L_{}_{}", block.label, i));
        }


        // Function signature
        let ret_type = self.ir_type_to_c(&func.ret_type);
        
        // Special handling for main function: use C standard signature
        let params: Vec<String> = if func.name == "main" && func.params.len() == 2 {
            // Use standard C main signature: int argc, char** argv
            vec!["int argc".to_string(), "char** argv".to_string()]
        } else {
            func.params.iter().enumerate()
                .map(|(i, (_, ty))| format!("{} _arg{}", self.ir_type_to_c(ty), i))
                .collect()
        };
        
        // @simd does not authorize ISA changes or relaxed floating semantics.
        // Generate naked function attribute (no prologue/epilogue)
        if func.naked {
            self.writeln("/* @naked: no prologue/epilogue */");
            self.writeln("#if defined(__GNUC__) || defined(__clang__)");
            self.writeln("__attribute__((naked))");
            self.writeln("#endif");
        }
        
        // Generate interrupt handler attribute
        if func.interrupt {
            self.writeln("/* @interrupt: interrupt handler */");
            self.writeln("#if defined(__GNUC__) || defined(__clang__)");
            self.writeln("__attribute__((interrupt, target(\"general-regs-only\")))");
            self.writeln("#endif");
        }
        
        let params_str = if params.is_empty() { "void".to_string() } else { params.join(", ") };
        let linkage = if func.c_abi { "" } else { "static " };
        self.writeln(&format!("{linkage}{} {}({}) {{", ret_type, func.name, params_str));

        self.indent += 1;
        if func.naked {
            for instruction in &func.blocks[0].instructions {
                if let Instruction::InlineAsm { template, .. } = instruction {
                    self.writeln(&format!("__asm__ volatile(\"{}\");", Self::escape_for_c(template)));
                }
            }
            self.indent -= 1;
            self.writeln("}");
            return Ok(());
        }

        // Pass 1: Analyze types
        for block in &func.blocks {
            for inst in &block.instructions {
                self.analyze_instruction(inst);
            }
        }

        self.reg_types.extend(func.register_types.clone());

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
                    Instruction::ArrayElementPtr { dest, .. } |
                    Instruction::GetFieldPtr { dest, .. } |
                    Instruction::Phi { dest, .. } |
                    Instruction::Layout { dest, .. } |
                    Instruction::Platform { dest, .. } |
                    Instruction::Cast { dest, .. } => {
                        let var = self.get_var(*dest);
                        let c_type = self.reg_types.get(dest)
                            .map(|t| self.ir_type_to_c(t))
                            .unwrap_or("int64_t".to_string());
                        declarations.push(format!("{} {};", c_type, var));
                    }
                    Instruction::Call { dest: Some(dest), .. } | Instruction::Memory { dest: Some(dest), .. } | Instruction::Vector { dest: Some(dest), .. } => {
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
                                let ty = self.reg_types.get(&reg).cloned().unwrap_or(IRType::I64);
                                declarations.push(format!("{} {};", self.ir_type_to_c(&ty), var));
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
                self.writeln(&format!("{}: ;", label));
                self.indent += 1;
            }


            // Instructions
            for inst in &block.instructions {
                self.generate_instruction(inst)?;
            }

            // Terminator (with phi assignments for correct SSA semantics)
            if let Some(ref term) = block.terminator {
                if let Some(barrier) = crate::backend::system_attributes::interrupt_epilogue(func, term) {
                    self.generate_instruction(&barrier)?;
                }
                self.generate_terminator_with_phis(term, i, func)?;
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
            Value::Global(name) => self.global_types.get(name).map(|ty| IRType::Ptr(Box::new(ty.clone()))),
            Value::Constant(Constant::Int(_)) => Some(IRType::I64),
            Value::Constant(Constant::Float(_)) => Some(IRType::F64),
            Value::Constant(Constant::Bool(_)) => Some(IRType::Bool),
            Value::Constant(Constant::String(_)) => Some(IRType::Ptr(Box::new(IRType::U8))),
            Value::Constant(Constant::Null) => Some(IRType::Ptr(Box::new(IRType::Void))),
            Value::Unit => Some(IRType::Unit),
            _ => None,
        }
    }

    /// Emit one typed instruction.
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
                let ty = self.get_value_type(left).unwrap_or(IRType::I64);
                if let Some(bits) = ty.integer_bits() {
                    let result_type = self.ir_type_to_c(&ty);
                    let expression = match op {
                        BinOp::Add | BinOp::Sub | BinOp::Mul => format!("({})((uint64_t)({}) {} (uint64_t)({}))", result_type, l, op_str, r),
                        BinOp::Shl => format!("({})((uint64_t)({}) << ((uint64_t)({}) & {}))", result_type, l, r, bits - 1),
                        BinOp::Shr if ty.is_signed_integer() => format!("({})(({}) < 0 ? ~(~(uint64_t)({}) >> ((uint64_t)({}) & {})) : ((uint64_t)({}) >> ((uint64_t)({}) & {})))", result_type, l, l, r, bits - 1, l, r, bits - 1),
                        BinOp::Shr => format!("({})((uint64_t)({}) >> ((uint64_t)({}) & {}))", result_type, l, r, bits - 1),
                        BinOp::Div | BinOp::Mod => {
                            let overflow = if ty.is_signed_integer() { format!(" || (({}) == INT{}_MIN && ({}) == -1)", l, bits, r) } else { String::new() };
                            self.writeln(&format!("if (({}) == 0{}) __builtin_trap();", r, overflow));
                            format!("{} {} {}", l, op_str, r)
                        }
                        _ => format!("{} {} {}", l, op_str, r),
                    };
                    self.writeln(&format!("{} = {};", var, expression));
                } else {
                    self.writeln(&format!("{} = {} {} {};", var, l, op_str, r));
                }

            }
            
            Instruction::UnaryOp { dest, op, value } => {
                let var = self.get_var(*dest);
                let val = self.value_to_c(value);
                let op_str = match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "!",
                    UnaryOp::BitNot => "~",
                };
                if matches!(op, UnaryOp::Neg) && self.get_value_type(value).and_then(|t| t.integer_bits()).is_some() {
                    let ty = self.ir_type_to_c(&self.get_value_type(value).unwrap());
                    self.writeln(&format!("{} = ({})((uint64_t)0 - (uint64_t)({}));", var, ty, val));
                } else { self.writeln(&format!("{} = {}{};", var, op_str, val)); }
                if let Some(ty) = self.get_value_type(value) {
                    self.reg_types.insert(*dest, ty);
                }
            }
            
            Instruction::Platform {dest, op} => {
                let var = self.get_var(*dest);
                match op { crate::intrinsics::PlatformOp::ErrnoGet => self.writeln(&format!("{var} = errno;")) }
            }
            Instruction::Vector {dest,kind,ty,args} => {self.generate_vector(*dest,*kind,ty,args);}
            Instruction::Memory {dest,kind,ty,args,order,failure_order} => {
                self.generate_memory(*dest,*kind,ty,args,*order,*failure_order);
            }

            Instruction::Call { dest, func, args } => {
                let args_str: Vec<_> = args.iter().map(|a| self.value_to_c(a)).collect();

                // Check if user-defined function returns void
                let ret_ty = self.func_ret_types.get(func).cloned();
                let is_undefined_call = ret_ty.is_none();
                
                // Track undefined calls - might be enum variant constructors
                if is_undefined_call && !func.starts_with("_") {
                    self.undefined_calls.insert((func.clone(), args.len()));
                }
                
                let is_void = matches!(ret_ty, Some(IRType::Void)) || matches!(ret_ty, None);
                
                let callee = self.extern_conversions.get(func).unwrap_or(func);
                let call = format!("{}({})", callee, args_str.join(", "));
                
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
                self.writeln(&format!("{} _alloca_{};", c_type, var));
                self.writeln(&format!("{} = &_alloca_{};", var, var));
                self.reg_types.insert(*dest, IRType::Ptr(Box::new(ty.clone())));
            }
            
            Instruction::Load { dest, ptr, ty } => {
                let var = self.get_var(*dest);
                let pointer = self.value_to_c(ptr);
                // memcpy preserves unaligned packed fields and strict aliasing.
                self.writeln(&format!("if (sizeof({0}) != 0) __builtin_memcpy((void*)&{0}, (const void*){1}, sizeof({2})); else __builtin_memset(&{0}, 0, sizeof({0}));", var, pointer, var));
                self.reg_types.insert(*dest, ty.clone());
            }
            Instruction::Store { ptr, value } => {
                let pointer = self.value_to_c(ptr);
                let value_str = self.value_to_c(value);
                let ty = match self.get_value_type(ptr) {
                    Some(IRType::Ptr(inner)) => *inner,
                    _ => return Err(Error::CodeGen("store requires pointer type".into())),
                };
                self.writeln(&format!("{{ {} _store = {}; if (sizeof(_store) != 0) __builtin_memcpy((void*){}, (const void*)&_store, sizeof(_store)); }}", self.ir_type_to_c(&ty), value_str, pointer));
            }
            Instruction::ArrayElementPtr { dest, ptr, index, array_ty } => {
                let var = self.get_var(*dest);
                let p = self.value_to_c(ptr);
                let idx = self.value_to_c(index);
                let element = match array_ty { IRType::Array(element, _) => element.as_ref(), _ => return Err(Error::CodeGen("array address requires fixed array".into())) };
                self.writeln(&format!("{0} = sizeof({3}) == 0 ? (void*){1} : (void*)((unsigned char*){1} + (ptrdiff_t)({2}) * (ptrdiff_t)sizeof({3}));", var, p, idx, self.ir_type_to_c(element)));
                if let IRType::Array(elem, _) = array_ty { self.reg_types.insert(*dest, IRType::Ptr(elem.clone())); }
            }
            Instruction::GetElementPtr { dest, ptr, index, elem_ty } => {
                let var = self.get_var(*dest);
                let p = self.value_to_c(ptr);
                let idx = self.value_to_c(index);
                let ty = self.ir_type_to_c(elem_ty);
                self.writeln(&format!("{0} = sizeof({3}) == 0 ? (void*){1} : (void*)((unsigned char*){1} + (ptrdiff_t)({2}) * (ptrdiff_t)sizeof({3}));", var, p, idx, ty));
                self.reg_types.insert(*dest, IRType::Ptr(Box::new(elem_ty.clone())));
            }
            Instruction::GetFieldPtr { dest, ptr, field, struct_ty } => {
                let (name, ty) = match struct_ty {
                    IRType::Struct(name) => self.struct_layouts.get(name).and_then(|fields| fields.get(*field)).cloned(),
                    _ => None,
                }.ok_or_else(|| Error::CodeGen("invalid struct field address".into()))?;
                let var = self.get_var(*dest);
                let p = self.value_to_c(ptr);
                self.writeln(&format!("{0} = offsetof({2}, {3}) == 0 ? (void*){1} : (void*)((unsigned char*){1} + offsetof({2}, {3}));", var, p, self.ir_type_to_c(struct_ty), name));
                self.reg_types.insert(*dest, IRType::Ptr(Box::new(ty)));
            }

            Instruction::Layout { dest, query, ty, field } => {
                let var = self.get_var(*dest);
                let expression = self.layout_expression(*query, ty, *field)?;
                self.writeln(&format!("{} = {};", var, expression));
            }
            Instruction::ZeroInitialize { ptr, ty } => {
                let pointer = self.value_to_c(ptr);
                self.writeln(&format!("if (sizeof({1}) != 0) __builtin_memset((void*){0}, 0, sizeof({1}));", pointer, self.ir_type_to_c(ty)));
            }
            Instruction::Cast { dest, value, ty } => {
                let var = self.get_var(*dest);
                let val = self.value_to_c(value);
                let type_name = self.ir_type_to_c(ty);
                if matches!(self.get_value_type(value), Some(IRType::F32 | IRType::F64)) && ty.integer_bits().is_some() && *ty != IRType::Bool {
                    let bits = ty.integer_bits().unwrap();
                    let signed = ty.is_signed_integer();
                    let boundary = 2_f64.powi(if signed { bits as i32 - 1 } else { bits as i32 });
                    let max = if signed { format!("INT{}_MAX", bits) } else { format!("UINT{}_MAX", bits) };
                    let min = if signed { format!("INT{}_MIN", bits) } else { "0".into() };
                    let lower = if signed { -boundary } else { 0.0 };
                    self.writeln(&format!("{} = __builtin_isnan({}) ? 0 : (({}) >= {:.1} ? {} : (({}) <= {:.1} ? {} : ({})({})));", var, val, val, boundary, max, val, lower, min, type_name, val));
                } else {
                    self.writeln(&format!("{} = ({}){};", var, type_name, val));
                }
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
                    Self::escape_for_c(&render_asm_template(template, operands, true).map_err(Error::CodeGen)?), outputs_str, inputs_str, clobbers_str));
            }
            
            Instruction::Phi { dest, incoming } => {
                // Phi nodes in C: we declare the variable and let predecessor blocks assign to it.
                // The actual assignments are inserted by generate_phi_assignments() before terminators.
                // Here we just ensure the variable exists and track its type.
                let var = self.get_var(*dest);

                // Track type from the first incoming value
                if let Some((val, _)) = incoming.first() {
                    if let Some(ty) = self.get_value_type(val) {
                        self.reg_types.insert(*dest, ty);
                    }
                }

                // Add a comment for debugging
                let incoming_str: Vec<String> = incoming.iter()
                    .map(|(v, b)| format!("({}, block{})", self.value_to_c(v), b.0))
                    .collect();
                self.writeln(&format!("/* phi {} = [{}] */", var, incoming_str.join(", ")));
            }
        }
        Ok(())
    }


    /// Generate phi assignments for a jump to target block.
    /// This inserts assignments like `phi_var = value;` before the jump.
    fn generate_phi_assignments(&mut self, current_block_id: usize, target_block_id: usize, func: &IRFunction) {
        // Find all phi instructions in the target block
        if let Some(target_block) = func.blocks.get(target_block_id) {
            for inst in &target_block.instructions {
                if let Instruction::Phi { dest, incoming } = inst {
                    // Find the value for the current block
                    for (val, from_block) in incoming {
                        if from_block.0 == current_block_id {
                            let var = self.get_var(*dest);
                            let v = self.value_to_c(val);
                            self.writeln(&format!("{} = {};  /* phi from block {} */", var, v, current_block_id));
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Generate C code for a terminator (with phi assignments)
    fn generate_terminator_with_phis(&mut self, term: &Terminator, current_block_id: usize, func: &IRFunction) -> Result<()> {
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
                // Generate phi assignments before jump
                self.generate_phi_assignments(current_block_id, target.0, func);
                let label = &self.block_labels[&target.0];
                self.writeln(&format!("goto {};", label));
            }

            Terminator::Branch { cond, then_target, else_target } => {
                let c = self.value_to_c(cond);
                let then_label = self.block_labels[&then_target.0].clone();
                let else_label = self.block_labels[&else_target.0].clone();

                // For branches, we need to generate phi assignments in each branch
                self.writeln(&format!("if ({}) {{", c));
                self.indent += 1;
                self.generate_phi_assignments(current_block_id, then_target.0, func);
                self.writeln(&format!("goto {};", then_label));
                self.indent -= 1;
                self.writeln("} else {");
                self.indent += 1;
                self.generate_phi_assignments(current_block_id, else_target.0, func);
                self.writeln(&format!("goto {};", else_label));
                self.indent -= 1;
                self.writeln("}");
            }

            Terminator::Trap => {
                self.writeln("__builtin_trap();");
            }
            Terminator::Unreachable => {
                self.writeln("__builtin_unreachable();");
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
            
            Terminator::Trap => {
                self.writeln("__builtin_trap();");
            }
            Terminator::Unreachable => {
                self.writeln("__builtin_unreachable();");
            }
        }
        Ok(())
    }

    /// Generate the complete C source file
    pub fn generate_source(&mut self, module: &IRModule) -> Result<String> {
        crate::backend::system_attributes::validate(module, &self.target_triple)?;
        self.output.clear();

        self.union_names = module.structs.iter().filter(|definition| definition.is_union).map(|definition| definition.name.clone()).collect();
        self.struct_layouts = module.structs.iter().map(|definition| (definition.name.clone(), definition.fields.clone())).collect();
        self.global_types = module.globals.iter().map(|global| (global.name.clone(), global.ty.clone())).collect();
        
        // Header
        self.writeln("/* Generated by AetherLang C Backend */");
        if crate::backend::system_attributes::bare_x86_64(&self.target_triple) {
            self.writeln("/* Compile this bare-metal x86-64 translation unit with -mno-red-zone. */");
        }
        // glibc hides POSIX/XSI declarations when consumers compile with
        // -std=c11. Request their real prototypes before any system header.
        if !module.no_std {
            self.writeln("#if defined(__linux__)");
            self.writeln("#ifndef _XOPEN_SOURCE");
            self.writeln("#define _XOPEN_SOURCE 700");
            self.writeln("#endif");
            self.writeln("#ifndef _DEFAULT_SOURCE");
            self.writeln("#define _DEFAULT_SOURCE 1");
            self.writeln("#endif");
            self.writeln("#endif");
        }
        self.writeln("#include <stdint.h>");
        if module.functions.iter().flat_map(|function| &function.blocks).flat_map(|block| &block.instructions)
            .any(|instruction| matches!(instruction, Instruction::Platform { op: crate::intrinsics::PlatformOp::ErrnoGet, .. })) {
            self.writeln("#include <errno.h>");
        }
        self.writeln("#include <stddef.h>");
        self.writeln("#include <stdbool.h>");
        if !module.no_std {
            self.writeln("#include <stdio.h>");
            self.writeln("#include <stdlib.h>");
            self.writeln("#include <string.h>");
            self.writeln("#include <math.h>");
            self.emit_host_conversions(module);
        }
        self.writeln(&format!("_Static_assert(sizeof(void*) == {}, \"Aether target pointer width mismatch\");", module.pointer_width.bytes()));
        self.writeln("struct __aeth_unit { }; /* zero-sized language unit */");
        // Fixed-width portable vector representation: the host compiler selects ISA.
        for element in [IRType::F32,IRType::F64,IRType::I32,IRType::I64,IRType::U32,IRType::U64] {
            for lanes in [2,4,8] {
                let name=self.ir_type_to_c(&IRType::Vector(Box::new(element.clone()),lanes));
                self.writeln(&format!("typedef {} {} __attribute__((vector_size({})));",self.ir_type_to_c(&element),name,element.size_bytes()*lanes));
            }
        }
        self.writeln("");

        
        // Struct definitions
        self.writeln("/* Struct Definitions */");
        for definition in &module.structs { self.writeln(&format!("{} {};", if definition.is_union { "union" } else { "struct" }, definition.name)); }
        let mut emitted = HashSet::new();
        for definition in &module.structs { self.emit_layout(&IRType::Struct(definition.name.clone()), module, &mut emitted)?; }
        for function in &module.functions {
            for (_, ty) in &function.params { self.emit_layout(ty, module, &mut emitted)?; }
            self.emit_layout(&function.ret_type, module, &mut emitted)?;
            for block in &function.blocks {
                for instruction in &block.instructions {
                    if let Instruction::Layout { ty, .. } = instruction { self.emit_layout(ty, module, &mut emitted)?; }
                }
            }
            for ty in function.register_types.values() {
                let ty = match ty { IRType::Ptr(inner) => inner.as_ref(), other => other };
                self.emit_layout(ty, module, &mut emitted)?;
            }
        }
        fn initializer_layouts(codegen: &mut CCodeGen, value: &IRInitializer, module: &IRModule, emitted: &mut HashSet<String>) -> Result<()> {
            match value {
                IRInitializer::Layout { ty, .. } => codegen.emit_layout(ty, module, emitted)?,
                IRInitializer::Repeat(value,_)=>initializer_layouts(codegen,value,module,emitted)?,
                IRInitializer::Aggregate(fields) => for (_, value) in fields { initializer_layouts(codegen, value, module, emitted)?; },
                _ => {}
            }
            Ok(())
        }
        for global in &module.globals {
            self.emit_layout(&global.ty, module, &mut emitted)?;
            if let Some(value) = &global.initializer { initializer_layouts(self, value, module, &mut emitted)?; }
        }
        for function in &module.externs {
            for (_, ty) in &function.params { self.emit_layout(ty, module, &mut emitted)?; }
            self.emit_layout(&function.ret_type, module, &mut emitted)?;
        }
        // Declarations precede initializers, allowing forward symbol addresses.
        for global in &module.globals {
            let storage = if global.initializer.is_some() && !global.exported { "static" } else { "extern" };
            self.writeln(&format!("{} {}{} {};", storage, self.ir_type_to_c(&global.ty), if global.mutable { "" } else { " const" }, global.name));
        }
        for global in &module.globals {
            if let Some(initializer) = &global.initializer {
                let value = self.initializer_to_c(initializer, &global.ty)?;
                self.writeln(&format!("{}{}{} {} = {};", if global.exported { "" } else { "static " }, self.ir_type_to_c(&global.ty), if global.mutable { "" } else { " const" }, global.name, value));
            }
        }
        // Enum definitions (tagged unions)
        self.writeln("/* Enum Definitions (Tagged Unions) */");
        for enum_def in &module.enums {
            if module.structs.iter().any(|definition| definition.name == enum_def.name) { continue; }
            let enum_name = &enum_def.name;
            
            // Generate tag enum
            self.writeln(&format!("enum {}_Tag {{", enum_name));
            for (idx, variant) in enum_def.variants.iter().enumerate() {
                self.writeln(&format!("    {}_{}_TAG = {},", enum_name, variant.name, idx));
            }
            self.writeln("};");
            self.writeln("");
            
            // Generate the union of all variant data
            let has_data_variants: Vec<_> = enum_def.variants.iter()
                .filter(|v| !v.fields.is_empty())
                .collect();
            
            if !has_data_variants.is_empty() {
                self.writeln(&format!("union {}_Data {{", enum_name));
                for variant in &has_data_variants {
                    if variant.fields.len() == 1 {
                        // Single field variant: union member is the field type directly
                        let c_type = self.ir_type_to_c(&variant.fields[0]);
                        self.writeln(&format!("    {} {};", c_type, variant.name.to_lowercase()));
                    } else {
                        // Multiple fields: wrap in a struct
                        self.writeln(&format!("    struct {{"));
                        for (i, field_ty) in variant.fields.iter().enumerate() {
                            let c_type = self.ir_type_to_c(field_ty);
                            self.writeln(&format!("        {} _{};", c_type, i));
                        }
                        self.writeln(&format!("    }} {};", variant.name.to_lowercase()));
                    }
                }
                self.writeln("};");
                self.writeln("");
            }
            
            // Generate the main enum struct
            self.writeln(&format!("struct {} {{", enum_name));
            self.writeln(&format!("    enum {}_Tag tag;", enum_name));
            if !has_data_variants.is_empty() {
                self.writeln(&format!("    union {}_Data data;", enum_name));
            }
            self.writeln("};");
            self.writeln("");
            
            // Generate constructor functions for each variant
            // Constructors return pointers (heap-allocated) to match IR calling convention
            for variant in &enum_def.variants {
                let func_name = format!("{}_{}", enum_name, variant.name);
                
                // Register this as a known function with its return type (pointer to struct)
                self.func_ret_types.insert(func_name.clone(), IRType::Ptr(Box::new(IRType::Struct(enum_name.clone()))));
                
                if variant.fields.is_empty() {
                    // Unit variant constructor
                    self.writeln(&format!("static struct {}* {}(void) {{", enum_name, func_name));
                    self.writeln(&format!("    struct {}* result = (struct {}*)malloc(sizeof(struct {}));", enum_name, enum_name, enum_name));
                    self.writeln(&format!("    result->tag = {}_{}_TAG;", enum_name, variant.name));
                    self.writeln("    return result;");
                    self.writeln("}");
                } else if variant.fields.len() == 1 {
                    // Single-field variant constructor
                    let c_type = self.ir_type_to_c(&variant.fields[0]);
                    self.writeln(&format!("static struct {}* {}({} value) {{", enum_name, func_name, c_type));
                    self.writeln(&format!("    struct {}* result = (struct {}*)malloc(sizeof(struct {}));", enum_name, enum_name, enum_name));
                    self.writeln(&format!("    result->tag = {}_{}_TAG;", enum_name, variant.name));
                    self.writeln(&format!("    result->data.{} = value;", variant.name.to_lowercase()));
                    self.writeln("    return result;");
                    self.writeln("}");
                } else {
                    // Multi-field variant constructor
                    let params: Vec<String> = variant.fields.iter().enumerate()
                        .map(|(i, ty)| format!("{} _{}", self.ir_type_to_c(ty), i))
                        .collect();
                    self.writeln(&format!("static struct {}* {}({}) {{", enum_name, func_name, params.join(", ")));
                    self.writeln(&format!("    struct {}* result = (struct {}*)malloc(sizeof(struct {}));", enum_name, enum_name, enum_name));
                    self.writeln(&format!("    result->tag = {}_{}_TAG;", enum_name, variant.name));
                    for i in 0..variant.fields.len() {
                        self.writeln(&format!("    result->data.{}._{} = _{};", variant.name.to_lowercase(), i, i));
                    }
                    self.writeln("    return result;");
                    self.writeln("}");
                }
                self.writeln("");
            }

        }
            
        // Populate layout map
        for struct_def in &module.structs {
            self.struct_layouts.insert(struct_def.name.clone(), struct_def.fields.clone());
        }

        
        // Populate function return types for internal functions
        for func in &module.functions {
            self.func_ret_types.insert(func.name.clone(), func.ret_type.clone());
        }
        
        // Also register extern function return types
        for ext in &module.externs {
            self.func_ret_types.insert(ext.name.clone(), ext.ret_type.clone());
        }
        
        // C library headers own their ABI declarations; every other explicit
        // foreign function receives a prototype, including its variadic marker.
        const HEADER_FUNCTIONS: &[&str] = &[
            "abort", "exit", "malloc", "calloc", "realloc", "free", "atoi", "atol", "atof", "strtol", "strtoul",
            "system", "getenv", "setenv", "unsetenv", "realpath", "qsort", "bsearch", "abs", "labs", "llabs", "rand", "srand",
            "puts", "putchar", "getchar", "printf", "fprintf", "sprintf", "snprintf", "scanf", "sscanf", "fscanf",
            "fopen", "fclose", "fread", "fwrite", "fseek", "ftell", "rewind", "fgets", "fputs", "fflush", "feof", "ferror", "clearerr",
            "tmpfile", "remove", "rename", "perror", "setbuf", "setvbuf", "fileno", "fgetc", "fputc", "getc", "putc", "ungetc", "freopen",
            "strlen", "strcmp", "strncmp", "strcpy", "strncpy", "strcat", "strncat", "strchr", "strrchr", "strstr",
            "memcpy", "memmove", "memset", "memcmp", "memchr", "strerror", "strdup", "assert",
            // Remaining ISO C stdio APIs and common POSIX stdio extensions.
            "fdopen", "popen", "pclose", "fseeko", "ftello", "fgetpos", "fsetpos", "tmpnam", "tmpnam_r",
            "vprintf", "vfprintf", "vsprintf", "vsnprintf", "vscanf", "vfscanf", "vsscanf", "dprintf", "vdprintf",
            "getline", "getdelim", "flockfile", "ftrylockfile", "funlockfile", "getc_unlocked", "putc_unlocked",
            "getchar_unlocked", "putchar_unlocked", "getw", "putw", "ctermid",
            // Scalar conversion/allocation/environment APIs declared by stdlib.
            "strtod", "strtof", "strtold", "strtoll", "strtoull", "atoll", "div", "ldiv", "lldiv",
            "atexit", "at_quick_exit", "quick_exit", "_Exit", "aligned_alloc", "mblen", "mbtowc", "wctomb", "mbstowcs", "wcstombs",
            // Remaining byte-string APIs from string.h.
            "strcoll", "strxfrm", "strspn", "strcspn", "strpbrk", "strtok", "strtok_r", "strndup", "strnlen",

        ];
        let mut declared = HashSet::new();
        for ext in &module.externs {
            // Strict ISO modes may hide POSIX declarations; when imported,
            // use the exact system prototype in either feature-test mode.
            if !module.no_std && matches!(ext.name.as_str(), "mkdtemp" | "mkstemp") {
                if declared.insert(ext.name.clone()) {
                    let result = if ext.name == "mkdtemp" { "char *" } else { "int " };
                    self.writeln(&format!("extern {result}{}(char *);", ext.name));
                }
                continue;
            }
            if (!module.no_std && HEADER_FUNCTIONS.contains(&ext.name.as_str())) || !declared.insert(ext.name.clone()) { continue; }
            let mut params: Vec<_> = ext.params.iter().map(|(_, ty)| self.ir_type_to_c(ty)).collect();
            if ext.variadic { params.push("...".into()); }
            if params.is_empty() { params.push("void".into()); }
            self.writeln(&format!("extern {} {}({});", self.ir_type_to_c(&ext.ret_type), ext.name, params.join(", ")));
        }

        // Forward declarations
        for func in &module.functions {
            let ret_type = self.ir_type_to_c(&func.ret_type);
            // Special handling for main function forward declaration
            let params: Vec<String> = if func.name == "main" && func.params.len() == 2 {
                vec!["int".to_string(), "char**".to_string()]
            } else {
                func.params.iter()
                    .map(|(_, ty)| self.ir_type_to_c(ty))
                    .collect()
            };
            let params_str = if params.is_empty() { "void".to_string() } else { params.join(", ") };
            let attribute = if func.interrupt { "__attribute__((interrupt, target(\"general-regs-only\"))) " } else if func.naked { "__attribute__((naked)) " } else { "" };
            let linkage = if func.c_abi { "" } else { "static " };
            self.writeln(&format!("{linkage}{}{} {}({});", attribute, ret_type, func.name, params_str));
        }

        self.writeln("");
        
        // Function definitions
        for func in &module.functions {
            self.generate_function(func)?;
        }
        
        Ok(self.output.clone())

    }

    /// Compile C source to object file using clang/gcc
    fn compile_c_to_object(&self, c_source: &str, module: &IRModule) -> Result<Vec<u8>> {
        // Write C source to temp file
        let temp_dir = std::env::temp_dir();
        let c_file = temp_dir.join("aether_temp.c");
        let obj_file = temp_dir.join("aether_temp.o");
        
        fs::write(&c_file, c_source).map_err(|e| Error::Io(e.to_string()))?;
        
        // Try clang first, then gcc
        let compilers = ["clang", "gcc", "cc"];
        let mut last_error = String::new();
        
        for compiler in &compilers {
            let mut command = Command::new(compiler);
            crate::backend::system_attributes::c_machine_flags(&mut command, &self.target_triple, module);
            if crate::backend::system_attributes::bare_x86_64(&self.target_triple) {
                command.arg(format!("--target={}", self.target_triple));
            }
            let result = command.args(&["-c", "-o"])
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
        self.compile_c_to_object(&c_source, module)
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
        let gen = IRGenerator::new("test");
        gen.generate(&program).unwrap()
    }

    fn generate_c(source: &str) -> String {
        let ir_module = compile_to_ir(source);
        let mut codegen = CCodeGen::new("x86_64-pc-windows-msvc");
        codegen.generate_source(&ir_module).unwrap()
    }

    #[test]
    fn string_nul_does_not_absorb_adjacent_octal_digits() {
        for digit in '0'..='7' {
            assert_eq!(CCodeGen::escape_for_c(&format!("\0{digit}")), format!("\\000{digit}"));
        }
        assert_eq!(CCodeGen::escape_for_c("\0\0"), "\\000\\000");
    }

    #[test]
    fn string_unicode_preserves_utf8_bytes_and_adjacent_ascii() {
        assert_eq!(
            CCodeGen::escape_for_c("éA中7🦀f\0"),
            "\\303\\251A\\344\\270\\2557\\360\\237\\246\\200f\\000"
        );
        assert_eq!(CCodeGen::escape_for_c("\u{1}7\u{7f}F"), "\\0017\\177F");
    }

    #[test]
    fn unit_pointer_uses_opaque_c_pointer_without_erasing_unit_values() {
        let codegen = CCodeGen::new("x86_64-unknown-linux-gnu");
        let unit_ptr = IRType::Ptr(Box::new(IRType::Unit));
        assert_eq!(codegen.ir_type_to_c(&IRType::Unit), "struct __aeth_unit");
        assert_eq!(codegen.ir_type_to_c(&unit_ptr), "void*");
        assert_eq!(codegen.ir_type_to_c(&IRType::Ptr(Box::new(unit_ptr))), "void**");
        assert_eq!(codegen.ir_type_to_c(&IRType::Ptr(Box::new(IRType::Volatile(Box::new(IRType::Unit))))), "void*");
        assert_eq!(codegen.ir_type_to_c(&IRType::Ptr(Box::new(IRType::I32))), "int32_t*");
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
        // Signed arithmetic must use unsigned intermediates so overflow has
        // the same wrapping semantics as LLVM (runtime cases are in the matrix).
        assert!(c.contains("(int64_t)((uint64_t)(1LL) + (uint64_t)(2LL))"));
    }

    #[test]
    fn test_if_expression() {
        let c = generate_c("fn test() { if true { return } else { return } }");
        println!("{}", c);
        assert!(c.contains("if"));
        assert!(c.contains("goto"));
    }
}

#[path = "c_memory.rs"]
mod memory;

#[path = "c_simd.rs"]
mod simd;
