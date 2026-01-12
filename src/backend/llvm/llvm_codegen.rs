//! LLVM Code Generator
//!
//! Translates Aether IR to LLVM IR and generates machine code.

use llvm_sys::core::*;
use llvm_sys::prelude::*;
use llvm_sys::target::*;
use llvm_sys::target_machine::*;
use llvm_sys::analysis::*;
use llvm_sys::LLVMIntPredicate;

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::ptr;

use crate::backend::codegen::CodeGen;
use crate::middle::ir::*;
use crate::utils::{Error, Result};

/// LLVM-based code generator
pub struct LLVMCodeGen {
    target_triple: String,
    context: LLVMContextRef,
    module: LLVMModuleRef,
    builder: LLVMBuilderRef,
    
    // Mapping from IR values to LLVM values
    value_map: HashMap<Register, LLVMValueRef>,
    // Mapping from block IDs to LLVM basic blocks
    block_map: HashMap<usize, LLVMBasicBlockRef>,
    // Current function being generated
    current_function: Option<LLVMValueRef>,
}

impl LLVMCodeGen {
    pub fn new(target: &str) -> Self {
        unsafe {
            let context = LLVMContextCreate();
            let module_name = CString::new("aether_module").unwrap();
            let module = LLVMModuleCreateWithNameInContext(module_name.as_ptr(), context);
            let builder = LLVMCreateBuilderInContext(context);
            
            Self {
                target_triple: target.to_string(),
                context,
                module,
                builder,
                value_map: HashMap::new(),
                block_map: HashMap::new(),
                current_function: None,
            }
        }
    }

    /// Initialize LLVM targets
    fn init_targets() {
        unsafe {
            LLVM_InitializeAllTargetInfos();
            LLVM_InitializeAllTargets();
            LLVM_InitializeAllTargetMCs();
            LLVM_InitializeAllAsmParsers();
            LLVM_InitializeAllAsmPrinters();
        }
    }

    /// Convert IR type to LLVM type
    fn ir_type_to_llvm(&self, ty: &IRType) -> LLVMTypeRef {
        unsafe {
            match ty {
                IRType::Void => LLVMVoidTypeInContext(self.context),
                IRType::Bool => LLVMInt1TypeInContext(self.context),
                IRType::I8 | IRType::U8 => LLVMInt8TypeInContext(self.context),
                IRType::I16 | IRType::U16 => LLVMInt16TypeInContext(self.context),
                IRType::I32 | IRType::U32 => LLVMInt32TypeInContext(self.context),
                IRType::I64 | IRType::U64 => LLVMInt64TypeInContext(self.context),
                IRType::F32 => LLVMFloatTypeInContext(self.context),
                IRType::F64 => LLVMDoubleTypeInContext(self.context),
                IRType::Ptr(inner) => {
                    let inner_ty = self.ir_type_to_llvm(inner);
                    LLVMPointerType(inner_ty, 0)
                }
                IRType::Array(elem, size) => {
                    let elem_ty = self.ir_type_to_llvm(elem);
                    LLVMArrayType(elem_ty, *size as u32)
                }
                IRType::Struct(name) => {
                    let name_c = CString::new(name.as_str()).unwrap();
                    let ty = LLVMGetTypeByName2(self.context, name_c.as_ptr());
                    if ty.is_null() {
                        // Create opaque struct if not found
                        LLVMStructCreateNamed(self.context, name_c.as_ptr())
                    } else {
                        ty
                    }
                }
                IRType::Function { params, ret } => {
                    let ret_ty = self.ir_type_to_llvm(ret);
                    let mut param_types: Vec<_> = params.iter()
                        .map(|p| self.ir_type_to_llvm(p))
                        .collect();
                    LLVMFunctionType(ret_ty, param_types.as_mut_ptr(), param_types.len() as u32, 0)
                }
            }
        }
    }

    /// Generate LLVM IR for a function
    fn generate_function(&mut self, func: &IRFunction) -> Result<()> {
        unsafe {
            // Create function type
            let ret_type = self.ir_type_to_llvm(&func.ret_type);
            let mut param_types: Vec<_> = func.params.iter()
                .map(|(_, ty)| self.ir_type_to_llvm(ty))
                .collect();
            
            let func_type = LLVMFunctionType(
                ret_type,
                param_types.as_mut_ptr(),
                param_types.len() as u32,
                0 // not variadic
            );
            
            // Create function
            let name = CString::new(func.name.as_str()).unwrap();
            let llvm_func = LLVMAddFunction(self.module, name.as_ptr(), func_type);
            self.current_function = Some(llvm_func);
            
            // Clear mappings for new function
            self.value_map.clear();
            self.block_map.clear();
            
            // Create basic blocks
            for (i, block) in func.blocks.iter().enumerate() {
                let block_name = CString::new(block.label.as_str()).unwrap();
                let llvm_block = LLVMAppendBasicBlockInContext(
                    self.context,
                    llvm_func,
                    block_name.as_ptr()
                );
                self.block_map.insert(i, llvm_block);
            }
            
            // Map parameters
            for (i, (name, _)) in func.params.iter().enumerate() {
                let param = LLVMGetParam(llvm_func, i as u32);
                let param_name = CString::new(name.as_str()).unwrap();
                LLVMSetValueName2(param, param_name.as_ptr(), name.len());
                // Store parameter in a special register
                self.value_map.insert(Register(1000 + i), param);
            }
            
            // Generate code for each block
            for (i, block) in func.blocks.iter().enumerate() {
                let llvm_block = self.block_map[&i];
                LLVMPositionBuilderAtEnd(self.builder, llvm_block);
                
                // Generate instructions
                for inst in &block.instructions {
                    self.generate_instruction(inst)?;
                }
                
                // Generate terminator
                if let Some(ref term) = block.terminator {
                    self.generate_terminator(term)?;
                }
            }
            
            self.current_function = None;
            Ok(())
        }
    }

    /// Generate LLVM IR for an instruction
    fn generate_instruction(&mut self, inst: &Instruction) -> Result<()> {
        unsafe {
            match inst {
                Instruction::Assign { dest, value } => {
                    let val = self.get_value(value)?;
                    self.value_map.insert(*dest, val);
                }
                
                Instruction::BinOp { dest, op, left, right } => {
                    let lhs = self.get_value(left)?;
                    let rhs = self.get_value(right)?;
                    let name = CString::new("").unwrap();
                    
                    let result = match op {
                        BinOp::Add => LLVMBuildAdd(self.builder, lhs, rhs, name.as_ptr()),
                        BinOp::Sub => LLVMBuildSub(self.builder, lhs, rhs, name.as_ptr()),
                        BinOp::Mul => LLVMBuildMul(self.builder, lhs, rhs, name.as_ptr()),
                        BinOp::Div => LLVMBuildSDiv(self.builder, lhs, rhs, name.as_ptr()),
                        BinOp::Mod => LLVMBuildSRem(self.builder, lhs, rhs, name.as_ptr()),
                        BinOp::And => LLVMBuildAnd(self.builder, lhs, rhs, name.as_ptr()),
                        BinOp::Or => LLVMBuildOr(self.builder, lhs, rhs, name.as_ptr()),
                        BinOp::Xor => LLVMBuildXor(self.builder, lhs, rhs, name.as_ptr()),
                        BinOp::Shl => LLVMBuildShl(self.builder, lhs, rhs, name.as_ptr()),
                        BinOp::Shr => LLVMBuildAShr(self.builder, lhs, rhs, name.as_ptr()),
                        BinOp::Eq => LLVMBuildICmp(self.builder, LLVMIntPredicate::LLVMIntEQ, lhs, rhs, name.as_ptr()),
                        BinOp::Ne => LLVMBuildICmp(self.builder, LLVMIntPredicate::LLVMIntNE, lhs, rhs, name.as_ptr()),
                        BinOp::Lt => LLVMBuildICmp(self.builder, LLVMIntPredicate::LLVMIntSLT, lhs, rhs, name.as_ptr()),
                        BinOp::Le => LLVMBuildICmp(self.builder, LLVMIntPredicate::LLVMIntSLE, lhs, rhs, name.as_ptr()),
                        BinOp::Gt => LLVMBuildICmp(self.builder, LLVMIntPredicate::LLVMIntSGT, lhs, rhs, name.as_ptr()),
                        BinOp::Ge => LLVMBuildICmp(self.builder, LLVMIntPredicate::LLVMIntSGE, lhs, rhs, name.as_ptr()),
                    };
                    self.value_map.insert(*dest, result);
                }
                
                Instruction::UnaryOp { dest, op, value } => {
                    let val = self.get_value(value)?;
                    let name = CString::new("").unwrap();
                    
                    let result = match op {
                        UnaryOp::Neg => LLVMBuildNeg(self.builder, val, name.as_ptr()),
                        UnaryOp::Not | UnaryOp::BitNot => LLVMBuildNot(self.builder, val, name.as_ptr()),
                    };
                    self.value_map.insert(*dest, result);
                }
                
                Instruction::Call { dest, func, args } => {
                    let func_name = CString::new(func.as_str()).unwrap();
                    let callee = LLVMGetNamedFunction(self.module, func_name.as_ptr());
                    
                    if callee.is_null() {
                        return Err(Error::CodeGen(format!("Unknown function: {}", func)));
                    }
                    
                    let mut llvm_args: Vec<_> = args.iter()
                        .map(|a| self.get_value(a))
                        .collect::<Result<Vec<_>>>()?;
                    
                    let name = CString::new("").unwrap();
                    let func_ty = LLVMGlobalGetValueType(callee);
                    let result = LLVMBuildCall2(
                        self.builder,
                        func_ty,
                        callee,
                        llvm_args.as_mut_ptr(),
                        llvm_args.len() as u32,
                        name.as_ptr()
                    );
                    
                    if let Some(d) = dest {
                        self.value_map.insert(*d, result);
                    }
                }
                
                Instruction::Alloca { dest, ty } => {
                    let llvm_ty = self.ir_type_to_llvm(ty);
                    let name = CString::new("").unwrap();
                    let ptr = LLVMBuildAlloca(self.builder, llvm_ty, name.as_ptr());
                    self.value_map.insert(*dest, ptr);
                }
                
                Instruction::Load { dest, ptr } => {
                    let ptr_val = self.get_value(ptr)?;
                    let name = CString::new("").unwrap();
                    // Get the element type from the pointer type
                    let ptr_ty = LLVMTypeOf(ptr_val);
                    let elem_ty = LLVMGetElementType(ptr_ty);
                    let result = LLVMBuildLoad2(self.builder, elem_ty, ptr_val, name.as_ptr());
                    self.value_map.insert(*dest, result);
                }
                
                Instruction::Store { ptr, value } => {
                    let ptr_val = self.get_value(ptr)?;
                    let store_val = self.get_value(value)?;
                    LLVMBuildStore(self.builder, store_val, ptr_val);
                }
                
                Instruction::GetElementPtr { dest, ptr, index } => {
                    let ptr_val = self.get_value(ptr)?;
                    let idx_val = self.get_value(index)?;
                    let name = CString::new("").unwrap();
                    let ptr_ty = LLVMTypeOf(ptr_val);
                    let elem_ty = LLVMGetElementType(ptr_ty);
                    let mut indices = [idx_val];
                    let result = LLVMBuildGEP2(
                        self.builder,
                        elem_ty,
                        ptr_val,
                        indices.as_mut_ptr(),
                        1,
                        name.as_ptr()
                    );
                    self.value_map.insert(*dest, result);
                }
                
                Instruction::Phi { dest, incoming } => {
                    // Get the type from the first incoming value
                    if let Some((first_val, _)) = incoming.first() {
                        let val = self.get_value(first_val)?;
                        let ty = LLVMTypeOf(val);
                        let name = CString::new("").unwrap();
                        let phi = LLVMBuildPhi(self.builder, ty, name.as_ptr());
                        
                        for (val, block_id) in incoming {
                            let llvm_val = self.get_value(val)?;
                            let llvm_block = self.block_map[&block_id.0];
                            let mut values = [llvm_val];
                            let mut blocks = [llvm_block];
                            LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), 1);
                        }
                        
                        self.value_map.insert(*dest, phi);
                    }
                }
            }
        }
        Ok(())
    }

    /// Generate LLVM IR for a terminator
    fn generate_terminator(&mut self, term: &Terminator) -> Result<()> {
        unsafe {
            match term {
                Terminator::Return { value } => {
                    if let Some(val) = value {
                        let ret_val = self.get_value(val)?;
                        LLVMBuildRet(self.builder, ret_val);
                    } else {
                        LLVMBuildRetVoid(self.builder);
                    }
                }
                
                Terminator::Jump { target } => {
                    let target_block = self.block_map[&target.0];
                    LLVMBuildBr(self.builder, target_block);
                }
                
                Terminator::Branch { cond, then_target, else_target } => {
                    let cond_val = self.get_value(cond)?;
                    let then_block = self.block_map[&then_target.0];
                    let else_block = self.block_map[&else_target.0];
                    LLVMBuildCondBr(self.builder, cond_val, then_block, else_block);
                }
                
                Terminator::Unreachable => {
                    LLVMBuildUnreachable(self.builder);
                }
            }
        }
        Ok(())
    }

    /// Get LLVM value from IR value
    fn get_value(&self, val: &Value) -> Result<LLVMValueRef> {
        unsafe {
            match val {
                Value::Register(reg) => {
                    self.value_map.get(reg)
                        .copied()
                        .ok_or_else(|| Error::CodeGen(format!("Unknown register: {:?}", reg)))
                }
                Value::Constant(c) => {
                    match c {
                        Constant::Int(n) => {
                            let i64_ty = LLVMInt64TypeInContext(self.context);
                            Ok(LLVMConstInt(i64_ty, *n as u64, 1))
                        }
                        Constant::Float(f) => {
                            let f64_ty = LLVMDoubleTypeInContext(self.context);
                            Ok(LLVMConstReal(f64_ty, *f))
                        }
                        Constant::Bool(b) => {
                            let i1_ty = LLVMInt1TypeInContext(self.context);
                            Ok(LLVMConstInt(i1_ty, *b as u64, 0))
                        }
                        Constant::String(s) => {
                            let bytes = s.as_bytes();
                            let len = bytes.len() as u32;
                            Ok(LLVMConstStringInContext(
                                self.context,
                                bytes.as_ptr() as *const i8,
                                len,
                                0 // don't null-terminate
                            ))
                        }
                        Constant::Null => {
                            let ptr_ty = LLVMPointerType(LLVMInt8TypeInContext(self.context), 0);
                            Ok(LLVMConstNull(ptr_ty))
                        }
                    }
                }
                Value::Parameter(i) => {
                    // Parameters are stored with offset 1000
                    self.value_map.get(&Register(1000 + *i))
                        .copied()
                        .ok_or_else(|| Error::CodeGen(format!("Unknown parameter: {}", i)))
                }
                Value::Global(name) => {
                    let name_c = CString::new(name.as_str()).unwrap();
                    let global = LLVMGetNamedGlobal(self.module, name_c.as_ptr());
                    if global.is_null() {
                        Err(Error::CodeGen(format!("Unknown global: {}", name)))
                    } else {
                        Ok(global)
                    }
                }
                Value::Unit => {
                    // Unit type represented as void, but we need a dummy value
                    let i8_ty = LLVMInt8TypeInContext(self.context);
                    Ok(LLVMConstInt(i8_ty, 0, 0))
                }
            }
        }
    }

    /// Verify the generated module
    fn verify_module(&self) -> Result<()> {
        unsafe {
            let mut error_msg: *mut i8 = ptr::null_mut();
            let result = LLVMVerifyModule(
                self.module,
                LLVMVerifierFailureAction::LLVMReturnStatusAction,
                &mut error_msg
            );
            
            if result != 0 {
                let msg = if error_msg.is_null() {
                    "Unknown verification error".to_string()
                } else {
                    let c_str = CStr::from_ptr(error_msg);
                    let msg = c_str.to_string_lossy().to_string();
                    LLVMDisposeMessage(error_msg);
                    msg
                };
                return Err(Error::CodeGen(msg));
            }
        }
        Ok(())
    }

    /// Emit object file
    fn emit_object(&self) -> Result<Vec<u8>> {
        unsafe {
            let triple = CString::new(self.target_triple.as_str()).unwrap();
            LLVMSetTarget(self.module, triple.as_ptr());
            
            let mut target: LLVMTargetRef = ptr::null_mut();
            let mut error_msg: *mut i8 = ptr::null_mut();
            
            if LLVMGetTargetFromTriple(triple.as_ptr(), &mut target, &mut error_msg) != 0 {
                let msg = if error_msg.is_null() {
                    "Failed to get target".to_string()
                } else {
                    let c_str = CStr::from_ptr(error_msg);
                    let msg = c_str.to_string_lossy().to_string();
                    LLVMDisposeMessage(error_msg);
                    msg
                };
                return Err(Error::CodeGen(msg));
            }
            
            let cpu = CString::new("generic").unwrap();
            let features = CString::new("").unwrap();
            
            let target_machine = LLVMCreateTargetMachine(
                target,
                triple.as_ptr(),
                cpu.as_ptr(),
                features.as_ptr(),
                LLVMCodeGenOptLevel::LLVMCodeGenLevelDefault,
                LLVMRelocMode::LLVMRelocDefault,
                LLVMCodeModel::LLVMCodeModelDefault
            );
            
            if target_machine.is_null() {
                return Err(Error::CodeGen("Failed to create target machine".to_string()));
            }
            
            // Get data layout
            let data_layout = LLVMCreateTargetDataLayout(target_machine);
            LLVMSetModuleDataLayout(self.module, data_layout);
            
            // Emit to memory buffer
            let mut mem_buf: LLVMMemoryBufferRef = ptr::null_mut();
            let mut error_msg: *mut i8 = ptr::null_mut();
            
            let result = LLVMTargetMachineEmitToMemoryBuffer(
                target_machine,
                self.module,
                LLVMCodeGenFileType::LLVMObjectFile,
                &mut error_msg,
                &mut mem_buf
            );
            
            LLVMDisposeTargetMachine(target_machine);
            
            if result != 0 {
                let msg = if error_msg.is_null() {
                    "Failed to emit object file".to_string()
                } else {
                    let c_str = CStr::from_ptr(error_msg);
                    let msg = c_str.to_string_lossy().to_string();
                    LLVMDisposeMessage(error_msg);
                    msg
                };
                return Err(Error::CodeGen(msg));
            }
            
            // Copy buffer to Vec
            let size = LLVMGetBufferSize(mem_buf);
            let start = LLVMGetBufferStart(mem_buf) as *const u8;
            let bytes = std::slice::from_raw_parts(start, size).to_vec();
            LLVMDisposeMemoryBuffer(mem_buf);
            
            Ok(bytes)
        }
    }

    /// Print LLVM IR to string (for debugging)
    pub fn print_ir(&self) -> String {
        unsafe {
            let c_str = LLVMPrintModuleToString(self.module);
            let result = CStr::from_ptr(c_str).to_string_lossy().to_string();
            LLVMDisposeMessage(c_str);
            result
        }
    }
}

impl CodeGen for LLVMCodeGen {
    fn generate(&mut self, module: &IRModule) -> Result<Vec<u8>> {
        Self::init_targets();
        
        // Set module name
        unsafe {
            let name = CString::new(module.name.as_str()).unwrap();
            LLVMSetModuleIdentifier(self.module, name.as_ptr(), module.name.len());
        }
        
        // Generate code for each function
        for func in &module.functions {
            self.generate_function(func)?;
        }
        
        // Verify
        self.verify_module()?;
        
        // Emit object code
        self.emit_object()
    }
    
    fn target_triple(&self) -> &str {
        &self.target_triple
    }
    
    fn name(&self) -> &str {
        "LLVM"
    }
}

impl Drop for LLVMCodeGen {
    fn drop(&mut self) {
        unsafe {
            LLVMDisposeBuilder(self.builder);
            LLVMDisposeModule(self.module);
            LLVMContextDispose(self.context);
        }
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

    #[test]
    fn test_empty_function() {
        let ir_module = compile_to_ir("fn main() {}");
        let mut codegen = LLVMCodeGen::new("x86_64-pc-windows-gnu");
        
        // Should not panic
        let result = codegen.generate(&ir_module);
        println!("LLVM IR:\n{}", codegen.print_ir());
        assert!(result.is_ok());
    }

    #[test]
    fn test_return_constant() {
        let ir_module = compile_to_ir("fn answer() -> i64 { return 42 }");
        let mut codegen = LLVMCodeGen::new("x86_64-pc-windows-gnu");
        
        let result = codegen.generate(&ir_module);
        println!("LLVM IR:\n{}", codegen.print_ir());
        assert!(result.is_ok());
    }

    #[test]
    fn test_binary_expression() {
        let ir_module = compile_to_ir("fn add() -> i64 { return 1 + 2 }");
        let mut codegen = LLVMCodeGen::new("x86_64-pc-windows-gnu");
        
        let result = codegen.generate(&ir_module);
        println!("LLVM IR:\n{}", codegen.print_ir());
        assert!(result.is_ok());
    }
}
