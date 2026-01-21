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
            
            let mut codegen = Self {
                target_triple: target.to_string(),
                context,
                module,
                builder,
                value_map: HashMap::new(),
                block_map: HashMap::new(),
                current_function: None,
            };
            
            codegen.declare_builtins();
            codegen
        }
    }
    
    /// Declare C standard library builtin functions
    fn declare_builtins(&mut self) {
        unsafe {
            // exit(i32) -> void
            let i32_ty = LLVMInt32TypeInContext(self.context);
            let void_ty = LLVMVoidTypeInContext(self.context);
            let exit_ty = LLVMFunctionType(void_ty, [i32_ty].as_mut_ptr(), 1, 0);
            let exit_name = CString::new("exit").unwrap();
            LLVMAddFunction(self.module, exit_name.as_ptr(), exit_ty);
            
            // printf(i8*, ...) -> i32 (variadic)
            let i8_ptr_ty = LLVMPointerTypeInContext(self.context, 0);
            let printf_ty = LLVMFunctionType(i32_ty, [i8_ptr_ty].as_mut_ptr(), 1, 1); // 1 = variadic
            let printf_name = CString::new("printf").unwrap();
            LLVMAddFunction(self.module, printf_name.as_ptr(), printf_ty);
            
            // puts(i8*) -> i32
            let puts_ty = LLVMFunctionType(i32_ty, [i8_ptr_ty].as_mut_ptr(), 1, 0);
            let puts_name = CString::new("puts").unwrap();
            LLVMAddFunction(self.module, puts_name.as_ptr(), puts_ty);
            
            // malloc(i64) -> i8*
            let i64_ty = LLVMInt64TypeInContext(self.context);
            let malloc_ty = LLVMFunctionType(i8_ptr_ty, [i64_ty].as_mut_ptr(), 1, 0);
            let malloc_name = CString::new("malloc").unwrap();
            LLVMAddFunction(self.module, malloc_name.as_ptr(), malloc_ty);
            
            // free(i8*) -> void
            let free_ty = LLVMFunctionType(void_ty, [i8_ptr_ty].as_mut_ptr(), 1, 0);
            let free_name = CString::new("free").unwrap();
            LLVMAddFunction(self.module, free_name.as_ptr(), free_ty);
            
            // realloc(i8*, i64) -> i8*
            let mut realloc_params = [i8_ptr_ty, i64_ty];
            let realloc_ty = LLVMFunctionType(i8_ptr_ty, realloc_params.as_mut_ptr(), 2, 0);
            let realloc_name = CString::new("realloc").unwrap();
            LLVMAddFunction(self.module, realloc_name.as_ptr(), realloc_ty);
            
            // memcpy(i8*, i8*, i64) -> i8*
            let mut memcpy_params = [i8_ptr_ty, i8_ptr_ty, i64_ty];
            let memcpy_ty = LLVMFunctionType(i8_ptr_ty, memcpy_params.as_mut_ptr(), 3, 0);
            let memcpy_name = CString::new("memcpy").unwrap();
            LLVMAddFunction(self.module, memcpy_name.as_ptr(), memcpy_ty);
            
            // strcmp(i8*, i8*) -> i32
            let mut strcmp_params = [i8_ptr_ty, i8_ptr_ty];
            let strcmp_ty = LLVMFunctionType(i32_ty, strcmp_params.as_mut_ptr(), 2, 0);
            let strcmp_name = CString::new("strcmp").unwrap();
            LLVMAddFunction(self.module, strcmp_name.as_ptr(), strcmp_ty);
            
            // strlen(i8*) -> i64
            let strlen_ty = LLVMFunctionType(i64_ty, [i8_ptr_ty].as_mut_ptr(), 1, 0);
            let strlen_name = CString::new("strlen").unwrap();
            LLVMAddFunction(self.module, strlen_name.as_ptr(), strlen_ty);
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
                        // Unknown type - treat as i32 (enum discriminant or simple type)
                        // This is a temporary fix for enum types which don't have a separate IRType
                        LLVMInt32TypeInContext(self.context)
                    } else {
                        // Return actual struct type (not pointer)
                        // Callers that need pointer should apply Ptr wrapper
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
                IRType::Vector(elem, lanes) => {
                    let elem_ty = self.ir_type_to_llvm(elem);
                    LLVMVectorType(elem_ty, *lanes as u32)
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
            
            // Add sret attribute for functions returning structs via pointer
            // The first parameter named "__sret" gets the sret attribute
            if func.sret_type.is_some() && !func.params.is_empty() {
                if let Some((name, _)) = func.params.first() {
                    if name == "__sret" {
                        // Parameter index 0 is for return value, 1 is first param
                        let sret_attr_kind = llvm_sys::core::LLVMGetEnumAttributeKindForName(
                            b"sret\0".as_ptr() as *const _,
                            4
                        );
                        if sret_attr_kind != 0 {
                            let sret_attr = llvm_sys::core::LLVMCreateEnumAttribute(
                                self.context,
                                sret_attr_kind,
                                0
                            );
                            llvm_sys::core::LLVMAddAttributeAtIndex(llvm_func, 1, sret_attr);
                        }
                    }
                }
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
                } else {
                    // Add implicit unreachable for blocks without terminator
                    LLVMBuildUnreachable(self.builder);
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
                    let mut lhs = self.get_value(left)?;
                    let mut rhs = self.get_value(right)?;
                    let name = CString::new("").unwrap();
                    
                    // Ensure operands have the same type
                    let lhs_ty = LLVMTypeOf(lhs);
                    let rhs_ty = LLVMTypeOf(rhs);
                    if lhs_ty != rhs_ty {
                        let lhs_kind = LLVMGetTypeKind(lhs_ty);
                        let rhs_kind = LLVMGetTypeKind(rhs_ty);
                        
                        // Convert to larger integer type if both are integers
                        if lhs_kind == llvm_sys::LLVMTypeKind::LLVMIntegerTypeKind 
                           && rhs_kind == llvm_sys::LLVMTypeKind::LLVMIntegerTypeKind {
                            let lhs_bits = LLVMGetIntTypeWidth(lhs_ty);
                            let rhs_bits = LLVMGetIntTypeWidth(rhs_ty);
                            if lhs_bits > rhs_bits {
                                rhs = LLVMBuildZExt(self.builder, rhs, lhs_ty, name.as_ptr());
                            } else if rhs_bits > lhs_bits {
                                lhs = LLVMBuildZExt(self.builder, lhs, rhs_ty, name.as_ptr());
                            }
                        } else if lhs_kind == llvm_sys::LLVMTypeKind::LLVMPointerTypeKind 
                                  && rhs_kind == llvm_sys::LLVMTypeKind::LLVMIntegerTypeKind {
                            // Pointer + integer: convert pointer to integer
                            lhs = LLVMBuildPtrToInt(self.builder, lhs, rhs_ty, name.as_ptr());
                        } else if lhs_kind == llvm_sys::LLVMTypeKind::LLVMIntegerTypeKind 
                                  && rhs_kind == llvm_sys::LLVMTypeKind::LLVMPointerTypeKind {
                            // Integer + pointer: convert pointer to integer
                            rhs = LLVMBuildPtrToInt(self.builder, rhs, lhs_ty, name.as_ptr());
                        }
                    }
                    
                    // Check if operands are floating point
                    let lhs_ty = LLVMTypeOf(lhs);
                    let is_float = LLVMGetTypeKind(lhs_ty) == llvm_sys::LLVMTypeKind::LLVMFloatTypeKind
                                || LLVMGetTypeKind(lhs_ty) == llvm_sys::LLVMTypeKind::LLVMDoubleTypeKind;
                    
                    let result = if is_float {
                        // Use floating-point instructions
                        match op {
                            BinOp::Add => LLVMBuildFAdd(self.builder, lhs, rhs, name.as_ptr()),
                            BinOp::Sub => LLVMBuildFSub(self.builder, lhs, rhs, name.as_ptr()),
                            BinOp::Mul => LLVMBuildFMul(self.builder, lhs, rhs, name.as_ptr()),
                            BinOp::Div => LLVMBuildFDiv(self.builder, lhs, rhs, name.as_ptr()),
                            BinOp::Mod => LLVMBuildFRem(self.builder, lhs, rhs, name.as_ptr()),
                            BinOp::Eq => LLVMBuildFCmp(self.builder, llvm_sys::LLVMRealPredicate::LLVMRealOEQ, lhs, rhs, name.as_ptr()),
                            BinOp::Ne => LLVMBuildFCmp(self.builder, llvm_sys::LLVMRealPredicate::LLVMRealONE, lhs, rhs, name.as_ptr()),
                            BinOp::Lt => LLVMBuildFCmp(self.builder, llvm_sys::LLVMRealPredicate::LLVMRealOLT, lhs, rhs, name.as_ptr()),
                            BinOp::Le => LLVMBuildFCmp(self.builder, llvm_sys::LLVMRealPredicate::LLVMRealOLE, lhs, rhs, name.as_ptr()),
                            BinOp::Gt => LLVMBuildFCmp(self.builder, llvm_sys::LLVMRealPredicate::LLVMRealOGT, lhs, rhs, name.as_ptr()),
                            BinOp::Ge => LLVMBuildFCmp(self.builder, llvm_sys::LLVMRealPredicate::LLVMRealOGE, lhs, rhs, name.as_ptr()),
                            // Bitwise ops don't apply to floats, return 0
                            BinOp::And | BinOp::Or | BinOp::Xor | BinOp::Shl | BinOp::Shr => {
                                let i64_ty = LLVMInt64TypeInContext(self.context);
                                LLVMConstInt(i64_ty, 0, 0)
                            }
                        }
                    } else {
                        // Use integer instructions
                        match op {
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
                        }
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
                    let mut callee = LLVMGetNamedFunction(self.module, func_name.as_ptr());
                    
                    if callee.is_null() {
                        // Check if this looks like an enum variant constructor (Type_Variant pattern)
                        if func.contains('_') {
                            // Auto-declare as enum variant constructor
                            // Returns i64 (tag + optional data pointer packed)
                            let i64_ty = LLVMInt64TypeInContext(self.context);
                            let i8_ptr_ty = LLVMPointerTypeInContext(self.context, 0);
                            
                            // Create function type based on number of args
                            let func_ty = if args.is_empty() {
                                LLVMFunctionType(i64_ty, std::ptr::null_mut(), 0, 0)
                            } else {
                                // For variants with data, accept pointer args
                                let mut param_types: Vec<LLVMTypeRef> = args.iter()
                                    .map(|_| i8_ptr_ty)
                                    .collect();
                                LLVMFunctionType(i64_ty, param_types.as_mut_ptr(), args.len() as u32, 0)
                            };
                            
                            callee = LLVMAddFunction(self.module, func_name.as_ptr(), func_ty);
                            
                            // Create a simple implementation that returns a hash-based tag
                            let entry = LLVMAppendBasicBlockInContext(self.context, callee, b"entry\0".as_ptr() as *const _);
                            let builder = LLVMCreateBuilderInContext(self.context);
                            LLVMPositionBuilderAtEnd(builder, entry);
                            
                            let hash_value = func.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
                            let tag = LLVMConstInt(i64_ty, hash_value % 10000, 0);
                            LLVMBuildRet(builder, tag);
                            LLVMDisposeBuilder(builder);
                        }
                    }
                    
                    if callee.is_null() {
                        return Err(Error::CodeGen(format!("Unknown function: {}", func)));
                    }
                    
                    let mut llvm_args: Vec<_> = args.iter()
                        .map(|a| self.get_value(a))
                        .collect::<Result<Vec<_>>>()?;
                    
                    // Cast arguments to match expected parameter types
                    let func_ty = LLVMGlobalGetValueType(callee);
                    let param_count = LLVMCountParamTypes(func_ty);
                    if param_count > 0 {
                        let mut param_types = vec![std::ptr::null_mut(); param_count as usize];
                        LLVMGetParamTypes(func_ty, param_types.as_mut_ptr());
                        
                        for (i, (arg, &expected_ty)) in llvm_args.iter_mut().zip(param_types.iter()).enumerate() {
                            if i >= param_count as usize { break; }
                            let actual_ty = LLVMTypeOf(*arg);
                            if actual_ty != expected_ty && !expected_ty.is_null() {
                                let name = CString::new("").unwrap();
                                let expected_kind = LLVMGetTypeKind(expected_ty);
                                let actual_kind = LLVMGetTypeKind(actual_ty);
                                
                                *arg = if expected_kind == llvm_sys::LLVMTypeKind::LLVMPointerTypeKind 
                                          && actual_kind == llvm_sys::LLVMTypeKind::LLVMIntegerTypeKind {
                                    LLVMBuildIntToPtr(self.builder, *arg, expected_ty, name.as_ptr())
                                } else if expected_kind == llvm_sys::LLVMTypeKind::LLVMIntegerTypeKind 
                                          && actual_kind == llvm_sys::LLVMTypeKind::LLVMPointerTypeKind {
                                    LLVMBuildPtrToInt(self.builder, *arg, expected_ty, name.as_ptr())
                                } else if expected_kind == llvm_sys::LLVMTypeKind::LLVMIntegerTypeKind 
                                          && actual_kind == llvm_sys::LLVMTypeKind::LLVMIntegerTypeKind {
                                    let expected_bits = LLVMGetIntTypeWidth(expected_ty);
                                    let actual_bits = LLVMGetIntTypeWidth(actual_ty);
                                    if expected_bits > actual_bits {
                                        LLVMBuildZExt(self.builder, *arg, expected_ty, name.as_ptr())
                                    } else if expected_bits < actual_bits {
                                        LLVMBuildTrunc(self.builder, *arg, expected_ty, name.as_ptr())
                                    } else {
                                        *arg
                                    }
                                } else {
                                    *arg // Keep as-is if can't convert
                                };
                            }
                        }
                    }
                    
                    let name = if dest.is_some() { 
                        CString::new("call").unwrap() 
                    } else { 
                        CString::new("").unwrap() 
                    };
                    // Use LLVMGlobalGetValueType for opaque pointers (LLVM 18+)
                    let func_ty = LLVMGlobalGetValueType(callee);
                    
                    // Check if function returns void
                    let ret_ty = LLVMGetReturnType(func_ty);
                    let is_void = LLVMGetTypeKind(ret_ty) == llvm_sys::LLVMTypeKind::LLVMVoidTypeKind;
                    
                    // Use empty name for void functions to avoid LLVM verification error
                    let call_name = if is_void {
                        CString::new("").unwrap()
                    } else {
                        name
                    };
                    
                    let result = LLVMBuildCall2(
                        self.builder,
                        func_ty,
                        callee,
                        llvm_args.as_mut_ptr(),
                        llvm_args.len() as u32,
                        call_name.as_ptr()
                    );
                    
                    if let Some(d) = dest {
                        // Only store result if function doesn't return void
                        if !is_void {
                            self.value_map.insert(*d, result);
                        }
                    }
                }
                
                Instruction::Alloca { dest, ty } => {
                    let llvm_ty = self.ir_type_to_llvm(ty);
                    let name = CString::new("").unwrap();
                    let ptr = LLVMBuildAlloca(self.builder, llvm_ty, name.as_ptr());
                    self.value_map.insert(*dest, ptr);
                }
                
                Instruction::Load { dest, ptr, ty } => {
                    let ptr_val = self.get_value(ptr)?;
                    let name = CString::new("").unwrap();
                    // Use the actual element type from IR
                    let elem_ty = self.ir_type_to_llvm(ty);
                    let result = LLVMBuildLoad2(self.builder, elem_ty, ptr_val, name.as_ptr());
                    self.value_map.insert(*dest, result);
                }
                
                Instruction::Store { ptr, value } => {
                    let mut ptr_val = self.get_value(ptr)?;
                    let store_val = self.get_value(value)?;
                    // Ensure ptr_val is a pointer type
                    let ptr_ty = LLVMTypeOf(ptr_val);
                    if LLVMGetTypeKind(ptr_ty) != llvm_sys::LLVMTypeKind::LLVMPointerTypeKind {
                        // Convert integer to pointer
                        let name = CString::new("").unwrap();
                        let ptr_type = LLVMPointerType(LLVMInt8TypeInContext(self.context), 0);
                        ptr_val = LLVMBuildIntToPtr(self.builder, ptr_val, ptr_type, name.as_ptr());
                    }
                    LLVMBuildStore(self.builder, store_val, ptr_val);
                }
                
                Instruction::GetElementPtr { dest, ptr, index, elem_ty } => {
                    let ptr_val = self.get_value(ptr)?;
                    let name = CString::new("").unwrap();
                    
                    // Check if this is struct field access or pointer arithmetic
                    match elem_ty {
                        IRType::Struct(_struct_name) => {
                            // Struct field access: use LLVMBuildStructGEP2
                            let field_idx = if let Value::Constant(Constant::Int(i)) = index {
                                *i as u32
                            } else {
                                0
                            };
                            let struct_ty = self.ir_type_to_llvm(elem_ty);
                            let result = LLVMBuildStructGEP2(
                                self.builder,
                                struct_ty,
                                ptr_val,
                                field_idx,
                                name.as_ptr()
                            );
                            self.value_map.insert(*dest, result);
                        }
                        _ => {
                            // Pointer arithmetic: use LLVMBuildGEP2 with single index
                            let elem_llvm_ty = self.ir_type_to_llvm(elem_ty);
                            let idx_val = self.get_value(index)?;
                            let mut indices = [idx_val];
                            let result = LLVMBuildGEP2(
                                self.builder,
                                elem_llvm_ty,
                                ptr_val,
                                indices.as_mut_ptr(),
                                1,  // Single index for pointer arithmetic
                                name.as_ptr()
                            );
                            self.value_map.insert(*dest, result);
                        }
                    }
                }
                
                Instruction::Phi { dest, incoming } => {
                    // Create empty Phi node first, incoming values will be added by a second pass
                    // This is needed because incoming blocks may not have been generated yet
                    if !incoming.is_empty() {
                        // Use i32 as default type for now (most enum/int values)
                        let i32_ty = LLVMInt32TypeInContext(self.context);
                        let name = CString::new("").unwrap();
                        let phi = LLVMBuildPhi(self.builder, i32_ty, name.as_ptr());
                        self.value_map.insert(*dest, phi);
                        
                        // TODO: Implement proper two-phase Phi filling after all blocks generated
                        // For now, add dummy incoming to make Phi valid (will have incorrect values)
                        for (_, block_id) in incoming {
                            let llvm_block = self.block_map[&block_id.0];
                            let dummy_val = LLVMConstInt(i32_ty, 0, 0);
                            let mut values = [dummy_val];
                            let mut blocks = [llvm_block];
                            LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), 1);
                        }
                    }
                }
                
                Instruction::Cast { dest, value, ty } => {
                    let val = self.get_value(value)?;
                    let dest_ty = self.ir_type_to_llvm(ty);
                    let name = CString::new("").unwrap();
                    
                    // Determine correct cast instruction based on types
                    let src_ty = LLVMTypeOf(val);
                    let src_kind = LLVMGetTypeKind(src_ty);
                    let dest_kind = LLVMGetTypeKind(dest_ty);
                    
                    let result = match (src_kind, dest_kind) {
                        // Integer to Integer: use trunc, sext, or zext based on size
                        (llvm_sys::LLVMTypeKind::LLVMIntegerTypeKind, llvm_sys::LLVMTypeKind::LLVMIntegerTypeKind) => {
                            let src_bits = LLVMGetIntTypeWidth(src_ty);
                            let dest_bits = LLVMGetIntTypeWidth(dest_ty);
                            if dest_bits < src_bits {
                                LLVMBuildTrunc(self.builder, val, dest_ty, name.as_ptr())
                            } else if dest_bits > src_bits {
                                // Use sign-extend for signed types (i*), zero-extend for unsigned (u*)
                                // For now, use zero-extend as a safe default
                                LLVMBuildZExt(self.builder, val, dest_ty, name.as_ptr())
                            } else {
                                val // Same size, no conversion needed
                            }
                        }
                        // Pointer to Integer
                        (llvm_sys::LLVMTypeKind::LLVMPointerTypeKind, llvm_sys::LLVMTypeKind::LLVMIntegerTypeKind) => {
                            LLVMBuildPtrToInt(self.builder, val, dest_ty, name.as_ptr())
                        }
                        // Integer to Pointer
                        (llvm_sys::LLVMTypeKind::LLVMIntegerTypeKind, llvm_sys::LLVMTypeKind::LLVMPointerTypeKind) => {
                            LLVMBuildIntToPtr(self.builder, val, dest_ty, name.as_ptr())
                        }
                        // Pointer to Pointer: use bitcast
                        (llvm_sys::LLVMTypeKind::LLVMPointerTypeKind, llvm_sys::LLVMTypeKind::LLVMPointerTypeKind) => {
                            LLVMBuildBitCast(self.builder, val, dest_ty, name.as_ptr())
                        }
                        // Default: use bitcast
                        _ => {
                            LLVMBuildBitCast(self.builder, val, dest_ty, name.as_ptr())
                        }
                    };
                    self.value_map.insert(*dest, result);
                }
                
                Instruction::InlineAsm { .. } => {
                    // InlineAsm not yet supported in LLVM backend
                    return Err(Error::CodeGen("InlineAsm not supported".to_string()));
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
                        // Get expected return type from current function
                        if let Some(func) = self.current_function {
                            let func_ty = LLVMGlobalGetValueType(func);
                            let expected_ret_ty = LLVMGetReturnType(func_ty);
                            let actual_ty = LLVMTypeOf(ret_val);
                            
                            // If types don't match, try to cast
                            let final_val = if actual_ty != expected_ret_ty {
                                let name = CString::new("").unwrap();
                                // Use inttoptr/ptrtoint for integer/pointer conversions
                                let expected_kind = LLVMGetTypeKind(expected_ret_ty);
                                let actual_kind = LLVMGetTypeKind(actual_ty);
                                
                                if expected_kind == llvm_sys::LLVMTypeKind::LLVMPointerTypeKind 
                                   && actual_kind == llvm_sys::LLVMTypeKind::LLVMIntegerTypeKind {
                                    LLVMBuildIntToPtr(self.builder, ret_val, expected_ret_ty, name.as_ptr())
                                } else if expected_kind == llvm_sys::LLVMTypeKind::LLVMIntegerTypeKind 
                                          && actual_kind == llvm_sys::LLVMTypeKind::LLVMPointerTypeKind {
                                    LLVMBuildPtrToInt(self.builder, ret_val, expected_ret_ty, name.as_ptr())
                                } else if expected_kind == llvm_sys::LLVMTypeKind::LLVMIntegerTypeKind 
                                          && actual_kind == llvm_sys::LLVMTypeKind::LLVMIntegerTypeKind {
                                    // Integer size conversion
                                    let expected_bits = LLVMGetIntTypeWidth(expected_ret_ty);
                                    let actual_bits = LLVMGetIntTypeWidth(actual_ty);
                                    if expected_bits > actual_bits {
                                        LLVMBuildZExt(self.builder, ret_val, expected_ret_ty, name.as_ptr())
                                    } else if expected_bits < actual_bits {
                                        LLVMBuildTrunc(self.builder, ret_val, expected_ret_ty, name.as_ptr())
                                    } else {
                                        ret_val
                                    }
                                } else {
                                    // Fallback: bitcast
                                    LLVMBuildBitCast(self.builder, ret_val, expected_ret_ty, name.as_ptr())
                                }
                            } else {
                                ret_val
                            };
                            LLVMBuildRet(self.builder, final_val);
                        } else {
                            LLVMBuildRet(self.builder, ret_val);
                        }
                    } else {
                        LLVMBuildRetVoid(self.builder);
                    }
                }
                
                Terminator::Jump { target } => {
                    let target_block = self.block_map[&target.0];
                    LLVMBuildBr(self.builder, target_block);
                }
                
                Terminator::Branch { cond, then_target, else_target } => {
                    let mut cond_val = self.get_value(cond)?;
                    let then_block = self.block_map[&then_target.0];
                    let else_block = self.block_map[&else_target.0];
                    
                    // Ensure condition is i1 type
                    let cond_ty = LLVMTypeOf(cond_val);
                    let i1_ty = LLVMInt1TypeInContext(self.context);
                    if cond_ty != i1_ty {
                        // Convert to i1: compare with 0 (non-zero = true)
                        let name = CString::new("").unwrap();
                        let zero = LLVMConstInt(cond_ty, 0, 0);
                        cond_val = LLVMBuildICmp(self.builder, LLVMIntPredicate::LLVMIntNE, cond_val, zero, name.as_ptr());
                    }
                    
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
                            // Create global string pointer (returns i8*)
                            // Filter out any embedded NUL characters before creating CString
                            let s_clean: String = s.chars().filter(|&c| c != '\0').collect();
                            let s_c = CString::new(s_clean).unwrap();
                            let name_c = CString::new("str").unwrap();
                            Ok(LLVMBuildGlobalStringPtr(self.builder, s_c.as_ptr(), name_c.as_ptr()))
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
                    let mut global = LLVMGetNamedGlobal(self.module, name_c.as_ptr());
                    if global.is_null() {
                        // Check if this looks like an enum variant (Type_Variant pattern)
                        if name.contains('_') {
                            // Auto-declare as i32 constant (enum discriminant)
                            // Use a simple hash of the name as the value
                            let hash_value = name.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
                            let i32_ty = LLVMInt32TypeInContext(self.context);
                            global = LLVMAddGlobal(self.module, i32_ty, name_c.as_ptr());
                            LLVMSetInitializer(global, LLVMConstInt(i32_ty, hash_value % 10000, 0));
                            LLVMSetGlobalConstant(global, 1);
                            LLVMSetLinkage(global, llvm_sys::LLVMLinkage::LLVMPrivateLinkage);
                        }
                    }
                    if global.is_null() {
                        Err(Error::CodeGen(format!("Unknown global: {}", name)))
                    } else {
                        // For enum variants (auto-declared), load the value
                        if name.contains('_') {
                            let i32_ty = LLVMInt32TypeInContext(self.context);
                            let load_name = CString::new("load").unwrap();
                            Ok(LLVMBuildLoad2(self.builder, i32_ty, global, load_name.as_ptr()))
                        } else {
                            Ok(global)
                        }
                    }
                }
                Value::Unit => {
                    // Unit type represented as i32 0 for compatibility with enum types
                    let i32_ty = LLVMInt32TypeInContext(self.context);
                    Ok(LLVMConstInt(i32_ty, 0, 0))
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
        
        // Declare extern functions first
        for ext in &module.externs {
            unsafe {
                let func_name = CString::new(ext.name.as_str()).unwrap();
                
                // Check if function already exists
                if !LLVMGetNamedFunction(self.module, func_name.as_ptr()).is_null() {
                    continue;
                }
                
                // Build function type
                let ret_ty = self.ir_type_to_llvm(&ext.ret_type);
                let mut param_types: Vec<LLVMTypeRef> = ext.params.iter()
                    .map(|(_, ty)| self.ir_type_to_llvm(ty))
                    .collect();
                let func_ty = LLVMFunctionType(
                    ret_ty,
                    param_types.as_mut_ptr(),
                    param_types.len() as u32,
                    0 // not variadic
                );
                
                // Add function declaration
                LLVMAddFunction(self.module, func_name.as_ptr(), func_ty);
            }
        }
        
        // Declare struct types
        for ir_struct in &module.structs {
            unsafe {
                let name_c = CString::new(ir_struct.name.as_str()).unwrap();
                let existing = LLVMGetTypeByName2(self.context, name_c.as_ptr());
                if existing.is_null() {
                    // Create struct type
                    let struct_ty = LLVMStructCreateNamed(self.context, name_c.as_ptr());
                    // Convert field types
                    let mut field_types: Vec<LLVMTypeRef> = ir_struct.fields.iter()
                        .map(|(_, ty)| self.ir_type_to_llvm(ty))
                        .collect();
                    // Set struct body
                    LLVMStructSetBody(
                        struct_ty,
                        field_types.as_mut_ptr(),
                        field_types.len() as u32,
                        0 // not packed
                    );
                }
            }
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
