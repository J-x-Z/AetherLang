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

/// Source values and machine ABI values deliberately have separate types.
#[derive(Clone)]
enum AbiValue { Direct(LLVMTypeRef), Coerce(LLVMTypeRef), Indirect, Ignore }
#[derive(Clone)]
struct FunctionAbi {
    params: Vec<(IRType, AbiValue)>,
    result: (IRType, AbiValue),
    variadic: bool,
}

/// LLVM-based code generator
pub struct LLVMCodeGen {
    target_triple: String,
    opt_level: u8,
    context: LLVMContextRef,
    module: LLVMModuleRef,
    builder: LLVMBuilderRef,
    target_machine: LLVMTargetMachineRef,
    target_data: LLVMTargetDataRef,
    abi: HashMap<String, FunctionAbi>,
    current_abi: Option<FunctionAbi>,
    return_slot: Option<LLVMValueRef>,
    type_definitions: HashMap<String, IRStruct>,
    global_types: HashMap<String, IRType>,
    
    // Mapping from IR values to LLVM values
    value_map: HashMap<Register, LLVMValueRef>,
    // Mapping from block IDs to LLVM basic blocks
    block_map: HashMap<usize, LLVMBasicBlockRef>,
    // Current function being generated
    current_function: Option<LLVMValueRef>,
    register_types: HashMap<Register, IRType>,
    parameter_types: Vec<IRType>,
    parameter_values: Vec<LLVMValueRef>,
    // Allocas for multiply-assigned registers
    alloca_map: HashMap<Register, LLVMValueRef>,
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
                opt_level: 0,
                context,
                module,
                builder,
                target_machine: ptr::null_mut(),
                target_data: ptr::null_mut(),
                abi: HashMap::new(),
                current_abi: None,
                return_slot: None,
                type_definitions: HashMap::new(),
                global_types: HashMap::new(),
                value_map: HashMap::new(),
                block_map: HashMap::new(),
                current_function: None,
                alloca_map: HashMap::new(),
                register_types: HashMap::new(),
                parameter_types: Vec::new(),
                parameter_values: Vec::new(),
            };
            
            codegen.declare_builtins();
            codegen
        }
    }
    
    pub fn with_opt_level(mut self, opt_level: u8) -> Self {
        self.opt_level = opt_level.min(3);
        self
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
        // LLVM target registration mutates process-global lists. Concurrent
        // registration can corrupt them even when every module has its own
        // context. Publish the complete registry exactly once per process.
        static INITIALIZE: std::sync::Once = std::sync::Once::new();
        INITIALIZE.call_once(|| unsafe {
            LLVM_InitializeAllTargetInfos();
            LLVM_InitializeAllTargets();
            LLVM_InitializeAllTargetMCs();
            LLVM_InitializeAllAsmParsers();
            LLVM_InitializeAllAsmPrinters();
        });
    }

    /// Convert IR type to LLVM type
    fn ir_type_to_llvm(&self, ty: &IRType) -> LLVMTypeRef {
        unsafe {
            match ty {
                IRType::Void => LLVMVoidTypeInContext(self.context),
                IRType::Unit => LLVMStructTypeInContext(self.context, ptr::null_mut(), 0, 0),
                IRType::Volatile(inner) => self.ir_type_to_llvm(inner),
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
                    LLVMArrayType2(elem_ty, *size as u64)
                }
                IRType::Struct(name) => {
                    let name_c = CString::new(name.as_str()).unwrap();
                    let ty = LLVMGetTypeByName2(self.context, name_c.as_ptr());
                    if ty.is_null() {
                        // Preserve the nominal type until the layout declaration pass.
                        LLVMStructCreateNamed(self.context, name_c.as_ptr())
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
            let abi = self.abi.get(&func.name).cloned().ok_or_else(|| Error::CodeGen("function ABI missing".into()))?;
            let name = CString::new(func.name.as_str()).unwrap();
            let llvm_func = LLVMGetNamedFunction(self.module, name.as_ptr());
            self.current_function = Some(llvm_func);
            self.current_abi = Some(abi.clone());
            self.return_slot = None;
            // Clear mappings for new function
            self.value_map.clear();
            self.block_map.clear();
            self.register_types = func.register_types.clone();
            self.parameter_types = func.params.iter().map(|(_, ty)| ty.clone()).collect();
            self.parameter_values.clear();
            
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
            
            LLVMPositionBuilderAtEnd(self.builder, self.block_map[&func.entry_block.0]);
            let mut index = 0;
            if matches!(abi.result.1, AbiValue::Indirect) {
                self.return_slot = Some(LLVMGetParam(llvm_func, 0));
                index += 1;
            }
            for (semantic, lowering) in &abi.params {
                let value = match lowering {
                    AbiValue::Ignore => LLVMConstNull(self.ir_type_to_llvm(semantic)),
                    _ => {
                        let machine = LLVMGetParam(llvm_func, index);
                        index += 1;
                        self.decode_abi(machine, semantic, lowering)
                    }
                };
                self.parameter_values.push(value);
            }
            // Add naked attribute if function is marked naked
            if func.naked {
                let naked_attr_kind = llvm_sys::core::LLVMGetEnumAttributeKindForName(
                    b"naked\0".as_ptr() as *const _,
                    5
                );
                if naked_attr_kind != 0 {
                    let naked_attr = llvm_sys::core::LLVMCreateEnumAttribute(
                        self.context,
                        naked_attr_kind,
                        0
                    );
                    // Function index is -1 (LLVMAttributeFunctionIndex)
                    llvm_sys::core::LLVMAddAttributeAtIndex(llvm_func, u32::MAX, naked_attr);
                }
            }

            // Add interrupt attribute if function is marked interrupt
            if func.interrupt {
                let interrupt_attr_kind = llvm_sys::core::LLVMGetEnumAttributeKindForName(
                    b"interrupt\0".as_ptr() as *const _,
                    9
                );
                if interrupt_attr_kind != 0 {
                    let interrupt_attr = llvm_sys::core::LLVMCreateEnumAttribute(
                        self.context,
                        interrupt_attr_kind,
                        0
                    );
                    llvm_sys::core::LLVMAddAttributeAtIndex(llvm_func, u32::MAX, interrupt_attr);
                }
            }

            // Add SIMD optimization hints if function is marked simd
            if func.simd {
                // Add target-features for SIMD (AVX2 on x86_64, NEON on ARM)
                let target_features = CString::new("target-features").unwrap();
                let simd_features = CString::new("+avx2,+sse4.2").unwrap();
                let attr = llvm_sys::core::LLVMCreateStringAttribute(
                    self.context,
                    target_features.as_ptr(),
                    target_features.as_bytes().len() as u32,
                    simd_features.as_ptr(),
                    simd_features.as_bytes().len() as u32,
                );
                llvm_sys::core::LLVMAddAttributeAtIndex(llvm_func, u32::MAX, attr);

                // Add "no-nans-fp-math" for faster floating point
                let no_nans = CString::new("no-nans-fp-math").unwrap();
                let true_val = CString::new("true").unwrap();
                let no_nans_attr = llvm_sys::core::LLVMCreateStringAttribute(
                    self.context,
                    no_nans.as_ptr(),
                    no_nans.as_bytes().len() as u32,
                    true_val.as_ptr(),
                    true_val.as_bytes().len() as u32,
                );
                llvm_sys::core::LLVMAddAttributeAtIndex(llvm_func, u32::MAX, no_nans_attr);
            }

            // Pre-scan: find registers that are assigned multiple times (need alloca)
            let mut assign_counts: std::collections::HashMap<Register, u32> = std::collections::HashMap::new();
            for block in &func.blocks {
                for inst in &block.instructions {
                    if let Instruction::Assign { dest, .. } = inst {
                        *assign_counts.entry(*dest).or_insert(0) += 1;
                    }
                }
            }
            
            // Create allocas for multiply-assigned registers at function entry
            let mut alloca_map: std::collections::HashMap<Register, LLVMValueRef> = std::collections::HashMap::new();
            if !func.blocks.is_empty() {
                let entry_block = self.block_map[&0];
                // Position at start of entry block
                let first_inst = LLVMGetFirstInstruction(entry_block);
                if !first_inst.is_null() {
                    LLVMPositionBuilderBefore(self.builder, first_inst);
                } else {
                    LLVMPositionBuilderAtEnd(self.builder, entry_block);
                }
                
                for (&reg, &count) in &assign_counts {
                    if count > 1 {
                        // Create alloca for this register
                        let name = CString::new(format!("var_{}", reg.0)).unwrap();
                        let ty = self.register_types.get(&reg).cloned().unwrap_or(IRType::I64);
                        let alloca = LLVMBuildAlloca(self.builder, self.ir_type_to_llvm(&ty), name.as_ptr());
                        alloca_map.insert(reg, alloca);
                    }
                }
            }
            
            // Store alloca_map for use in generate_instruction
            self.alloca_map = alloca_map;
            
            if let Some(entry) = self.block_map.get(&func.entry_block.0).copied() {
                LLVMPositionBuilderAtEnd(self.builder, entry);
                for block in &func.blocks {
                    for inst in &block.instructions {
                        if let Instruction::Alloca { dest, ty } = inst {
                            let pointer = LLVMBuildAlloca(self.builder, self.ir_type_to_llvm(ty), c"".as_ptr());
                            self.value_map.insert(*dest, pointer);
                        }
                    }
                }
            }

            // Create all phi destinations before emitting ordinary instructions;
            // nested CFGs can refer to a later-numbered predecessor.
            for (i, block) in func.blocks.iter().enumerate() {
                LLVMPositionBuilderAtEnd(self.builder, self.block_map[&i]);
                for inst in &block.instructions {
                    if let Instruction::Phi { dest, incoming } = inst {
                        let ty = self.register_types.get(dest).cloned()
                            .or_else(|| incoming.first().and_then(|(v, _)| self.value_type(v)))
                            .ok_or_else(|| Error::CodeGen("phi has no type".into()))?;
                        let phi = LLVMBuildPhi(self.builder, self.ir_type_to_llvm(&ty), c"".as_ptr());
                        self.value_map.insert(*dest, phi);
                    }
                }
            }

            let mut block_exits = HashMap::new();
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
                block_exits.insert(i, LLVMGetInsertBlock(self.builder));
            }

            // Resolve real incoming values after every definition exists. Loads
            // of mutable values belong on the predecessor edge, before its jump.
            for block in &func.blocks {
                for inst in &block.instructions {
                    if let Instruction::Phi { dest, incoming } = inst {
                        let phi = self.value_map[dest];
                        for (value, predecessor) in incoming {
                            let incoming_block = block_exits[&predecessor.0];
                            let term = LLVMGetBasicBlockTerminator(incoming_block);
                            if term.is_null() { LLVMPositionBuilderAtEnd(self.builder, incoming_block); }
                            else { LLVMPositionBuilderBefore(self.builder, term); }
                            let mut val = self.get_value(value)?;
                            let ty = self.register_types.get(dest).cloned().unwrap_or(IRType::I64);
                            val = self.cast_scalar(val, value, &ty)?;
                            let mut values = [val];
                            let mut blocks = [incoming_block];
                            LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), 1);
                        }
                    }
                }
            }
            self.current_function = None;
            Ok(())
        }
    }

    fn asm_constraint(&self, constraint: &str) -> String {
        if !self.target_triple.contains("x86") { return constraint.into(); }
        let base = constraint.trim_start_matches(['=', '+', '&', '%']);
        let prefix = &constraint[..constraint.len() - base.len()];
        let register = match base { "a" => "ax", "b" => "bx", "c" => "cx", "d" => "dx", "S" => "si", "D" => "di", _ => return constraint.into() };
        format!("{}{{{}}}", prefix, register)
    }

    /// Generate LLVM IR for an instruction
    fn generate_instruction(&mut self, inst: &Instruction) -> Result<()> {
        unsafe {
            match inst {
                Instruction::Assign { dest, value } => {
                    let val = self.get_value(value)?;
                    
                    // Check if this register uses alloca (multiply-assigned)
                    if let Some(&alloca) = self.alloca_map.get(dest) {
                        let ty = self.register_types.get(dest).cloned().unwrap_or(IRType::I64);
                        let store_val = self.cast_scalar(val, value, &ty)?;
                        LLVMBuildStore(self.builder, store_val, alloca);
                    } else {
                        self.value_map.insert(*dest, val);
                    }
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
                    
                    let unsigned = self.value_type(left).map(|ty| Self::is_unsigned(&ty)).unwrap_or(false);
                    if !is_float {
                        if matches!(op, BinOp::Shl | BinOp::Shr) {
                            let mask = LLVMConstInt(LLVMTypeOf(rhs), (LLVMGetIntTypeWidth(lhs_ty) - 1) as u64, 0);
                            rhs = LLVMBuildAnd(self.builder, rhs, mask, c"shift.mask".as_ptr());
                        }
                        if matches!(op, BinOp::Div | BinOp::Mod) {
                            self.guard_integer_division(lhs, rhs, !unsigned)?;
                        }
                    }
                    let result = if is_float {
                        // Use floating-point instructions
                        match op {
                            BinOp::Add => LLVMBuildFAdd(self.builder, lhs, rhs, name.as_ptr()),
                            BinOp::Sub => LLVMBuildFSub(self.builder, lhs, rhs, name.as_ptr()),
                            BinOp::Mul => LLVMBuildFMul(self.builder, lhs, rhs, name.as_ptr()),
                            BinOp::Div => LLVMBuildFDiv(self.builder, lhs, rhs, name.as_ptr()),
                            BinOp::Mod => LLVMBuildFRem(self.builder, lhs, rhs, name.as_ptr()),
                            BinOp::Eq => LLVMBuildFCmp(self.builder, llvm_sys::LLVMRealPredicate::LLVMRealOEQ, lhs, rhs, name.as_ptr()),
                            BinOp::Ne => LLVMBuildFCmp(self.builder, llvm_sys::LLVMRealPredicate::LLVMRealUNE, lhs, rhs, name.as_ptr()),
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
                            BinOp::Div => if unsigned { LLVMBuildUDiv(self.builder, lhs, rhs, name.as_ptr()) } else { LLVMBuildSDiv(self.builder, lhs, rhs, name.as_ptr()) },
                            BinOp::Mod => if unsigned { LLVMBuildURem(self.builder, lhs, rhs, name.as_ptr()) } else { LLVMBuildSRem(self.builder, lhs, rhs, name.as_ptr()) },
                            BinOp::And => LLVMBuildAnd(self.builder, lhs, rhs, name.as_ptr()),
                            BinOp::Or => LLVMBuildOr(self.builder, lhs, rhs, name.as_ptr()),
                            BinOp::Xor => LLVMBuildXor(self.builder, lhs, rhs, name.as_ptr()),
                            BinOp::Shl => LLVMBuildShl(self.builder, lhs, rhs, name.as_ptr()),
                            BinOp::Shr => if unsigned { LLVMBuildLShr(self.builder, lhs, rhs, name.as_ptr()) } else { LLVMBuildAShr(self.builder, lhs, rhs, name.as_ptr()) },
                            BinOp::Eq => LLVMBuildICmp(self.builder, LLVMIntPredicate::LLVMIntEQ, lhs, rhs, name.as_ptr()),
                            BinOp::Ne => LLVMBuildICmp(self.builder, LLVMIntPredicate::LLVMIntNE, lhs, rhs, name.as_ptr()),
                            BinOp::Lt => LLVMBuildICmp(self.builder, if unsigned { LLVMIntPredicate::LLVMIntULT } else { LLVMIntPredicate::LLVMIntSLT }, lhs, rhs, name.as_ptr()),
                            BinOp::Le => LLVMBuildICmp(self.builder, if unsigned { LLVMIntPredicate::LLVMIntULE } else { LLVMIntPredicate::LLVMIntSLE }, lhs, rhs, name.as_ptr()),
                            BinOp::Gt => LLVMBuildICmp(self.builder, if unsigned { LLVMIntPredicate::LLVMIntUGT } else { LLVMIntPredicate::LLVMIntSGT }, lhs, rhs, name.as_ptr()),
                            BinOp::Ge => LLVMBuildICmp(self.builder, if unsigned { LLVMIntPredicate::LLVMIntUGE } else { LLVMIntPredicate::LLVMIntSGE }, lhs, rhs, name.as_ptr()),
                        }
                    };
                    self.value_map.insert(*dest, result);
                }
                
                Instruction::UnaryOp { dest, op, value } => {
                    let val = self.get_value(value)?;
                    let name = CString::new("").unwrap();
                    
                    let result = match op {
                        UnaryOp::Neg => if matches!(LLVMGetTypeKind(LLVMTypeOf(val)), llvm_sys::LLVMTypeKind::LLVMFloatTypeKind | llvm_sys::LLVMTypeKind::LLVMDoubleTypeKind) {
                            LLVMBuildFNeg(self.builder, val, name.as_ptr())
                        } else { LLVMBuildNeg(self.builder, val, name.as_ptr()) },
                        UnaryOp::Not | UnaryOp::BitNot => LLVMBuildNot(self.builder, val, name.as_ptr()),
                    };
                    self.value_map.insert(*dest, result);
                }
                
                Instruction::Memory {dest,kind,ty,args,order,failure_order} => {
                    self.generate_memory(*dest,*kind,ty,args,*order,*failure_order)?;
                }

                Instruction::Call { dest, func, args } => {
                    let name = CString::new(func.as_str()).unwrap();
                    let callee = LLVMGetNamedFunction(self.module, name.as_ptr());
                    if callee.is_null() { return Err(Error::CodeGen(format!("Unknown function: {}", func))); }
                    let abi = self.abi.get(func).cloned().ok_or_else(|| Error::CodeGen(format!("missing ABI for {}", func)))?;
                    let mut llvm_args = Vec::new();
                    let result_slot = if matches!(abi.result.1, AbiValue::Indirect) {
                        let slot = self.entry_alloca(self.ir_type_to_llvm(&abi.result.0));
                        llvm_args.push(slot);
                        Some(slot)
                    } else { None };
                    for (index, argument) in args.iter().enumerate() {
                        let value = self.get_value(argument)?;
                        if let Some((semantic, lowering)) = abi.params.get(index) {
                            if let Some(value) = self.encode_abi(value, semantic, lowering) { llvm_args.push(value); }
                        } else {
                            if matches!(self.value_type(argument), Some(IRType::Array(_, _) | IRType::Struct(_))) {
                                return Err(Error::CodeGen("aggregate arguments in C variadic positions are not implemented".into()));
                            }
                            llvm_args.push(value);
                        }
                    }
                    let function_type = LLVMGlobalGetValueType(callee);
                    let call = LLVMBuildCall2(self.builder, function_type, callee, llvm_args.as_mut_ptr(), llvm_args.len() as u32, c"".as_ptr());
                    self.add_abi_attributes(call, &abi, true);
                    if let Some(dest) = dest {
                        let value = if let Some(slot) = result_slot { self.unaligned_load(self.ir_type_to_llvm(&abi.result.0), slot) }
                            else { self.decode_abi(call, &abi.result.0, &abi.result.1) };
                        self.value_map.insert(*dest, value);
                    }
                }

                Instruction::Alloca { .. } => { /* reserved once in the entry block */ }

                Instruction::Load { dest, ptr, ty } => {
                    let ptr_val = self.get_value(ptr)?;
                    let name = CString::new("").unwrap();
                    // Use the actual element type from IR
                    let elem_ty = self.ir_type_to_llvm(ty);
                    let result = if LLVMABISizeOfType(self.target_data, elem_ty) == 0 { LLVMConstNull(elem_ty) }
                        else { let load = LLVMBuildLoad2(self.builder, elem_ty, ptr_val, name.as_ptr()); LLVMSetAlignment(load, 1); load };
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
                    if LLVMABISizeOfType(self.target_data, LLVMTypeOf(store_val)) != 0 {
                        let store_inst = LLVMBuildStore(self.builder, store_val, ptr_val);
                        LLVMSetAlignment(store_inst, 1);
                    }
                }
                
                Instruction::ArrayElementPtr { dest, ptr, index, array_ty } => {
                    let ptr = self.get_value(ptr)?;
                    let mut indices = [LLVMConstInt(LLVMInt32TypeInContext(self.context), 0, 0), self.get_value(index)?];
                    let result = LLVMBuildGEP2(self.builder, self.ir_type_to_llvm(array_ty), ptr, indices.as_mut_ptr(), 2, c"".as_ptr());
                    self.value_map.insert(*dest, result);
                }
                Instruction::GetElementPtr { dest, ptr, index, elem_ty } => {
                    let ptr_val = self.get_value(ptr)?;
                    let mut indices = [self.get_value(index)?];
                    let result = LLVMBuildGEP2(self.builder, self.ir_type_to_llvm(elem_ty), ptr_val,
                        indices.as_mut_ptr(), 1, c"".as_ptr());
                    self.value_map.insert(*dest, result);
                }
                Instruction::GetFieldPtr { dest, ptr, field, struct_ty } => {
                    let ptr_val = self.get_value(ptr)?;
                    let is_union = matches!(struct_ty, IRType::Struct(name) if self.type_definitions.get(name).is_some_and(|definition| definition.is_union));
                    let zero_offset = is_union || LLVMOffsetOfElement(self.target_data, self.ir_type_to_llvm(struct_ty), *field as u32) == 0;
                    let result = if zero_offset { ptr_val } else { LLVMBuildStructGEP2(self.builder, self.ir_type_to_llvm(struct_ty), ptr_val,
                        *field as u32, c"".as_ptr()) };
                    self.value_map.insert(*dest, result);
                }

                Instruction::Phi { .. } => { /* emitted in the two-phase CFG pass */ }

                Instruction::Layout { dest, query, ty, field } => {
                    let value = self.layout_value(*query, ty, *field)?;
                    self.value_map.insert(*dest, LLVMConstInt(LLVMInt64TypeInContext(self.context), value, 0));
                }
                Instruction::ZeroInitialize { ptr, ty } => {
                    let ptr = self.get_value(ptr)?;
                    let size = LLVMABISizeOfType(self.target_data, self.ir_type_to_llvm(ty));
                    if size > 0 { LLVMBuildMemSet(self.builder, ptr, LLVMConstInt(LLVMInt8TypeInContext(self.context), 0, 0), LLVMConstInt(LLVMInt64TypeInContext(self.context), size, 0), 1); }
                }
                Instruction::Cast { dest, value, ty } => {
                    let val = self.get_value(value)?;
                    let result = self.cast_scalar(val, value, ty)?;
                    self.value_map.insert(*dest, result);
                }

                Instruction::InlineAsm { template, operands } => {
                    // Build LLVM inline assembly
                    // Format: constraints and template for LLVM
                    let mut inputs: Vec<LLVMValueRef> = Vec::new();
                    let mut input_constraints = Vec::new();
                    let mut output_constraints = Vec::new();
                    let mut clobbers = Vec::new();
                    let mut output_regs: Vec<Register> = Vec::new();

                    for op in operands {
                        match op.kind {
                            IRAsmOperandKind::Input => {
                                if let Some(ref val) = op.input {
                                    let llvm_val = self.get_value(val)?;
                                    inputs.push(llvm_val);
                                    input_constraints.push(self.asm_constraint(&op.constraint));
                                }
                            }
                            IRAsmOperandKind::Output => {
                                if let Some(reg) = op.output {
                                    let constraint = if !op.constraint.starts_with('=') {
                                        format!("={}", op.constraint)
                                    } else {
                                        op.constraint.clone()
                                    };
                                    output_constraints.push(self.asm_constraint(&constraint));
                                    output_regs.push(reg);
                                }
                            }
                            IRAsmOperandKind::InOut => {
                                if let Some(reg) = op.output {
                                    let constraint = format!("={}", op.constraint.trim_start_matches(['+', '=']));
                                    output_constraints.push(self.asm_constraint(&constraint));
                                    let tied_index = output_regs.len();
                                    output_regs.push(reg);
                                    if let Some(ref val) = op.input {
                                        inputs.push(self.get_value(val)?);
                                        input_constraints.push(tied_index.to_string());
                                    }
                                }
                            }
                            IRAsmOperandKind::Clobber => {
                                clobbers.push(format!("~{{{}}}", op.constraint));
                            }
                        }
                    }

                    // Build constraint string: "outputs,inputs,clobbers"
                    let mut all_constraints = Vec::new();
                    all_constraints.extend(output_constraints.clone());
                    all_constraints.extend(input_constraints);
                    all_constraints.extend(clobbers);
                    let constraints_str = all_constraints.join(",");
                    let constraints_c = CString::new(constraints_str.clone()).unwrap();
                    let template = render_asm_template(template, operands, false).map_err(Error::CodeGen)?;
                    let template_c = CString::new(template.as_str()).unwrap();

                    let mut output_types: Vec<_> = output_regs.iter().map(|reg| {
                        self.ir_type_to_llvm(self.register_types.get(reg).unwrap_or(&IRType::I64))
                    }).collect();
                    let ret_ty = match output_types.len() {
                        0 => LLVMVoidTypeInContext(self.context),
                        1 => output_types[0],
                        _ => LLVMStructTypeInContext(self.context, output_types.as_mut_ptr(), output_types.len() as u32, 0),
                    };
                    let mut param_types: Vec<_> = inputs.iter().map(|value| LLVMTypeOf(*value)).collect();
                    let asm_func_ty = LLVMFunctionType(ret_ty, param_types.as_mut_ptr(), param_types.len() as u32, 0);

                    // Create inline asm value
                    let asm_val = LLVMGetInlineAsm(
                        asm_func_ty,
                        template_c.as_ptr() as *mut i8,
                        template.len(),
                        constraints_c.as_ptr() as *mut i8,
                        constraints_str.len(),
                        1,  // has_side_effects
                        0,  // is_align_stack
                        llvm_sys::LLVMInlineAsmDialect::LLVMInlineAsmDialectATT,
                        0,  // can_throw
                    );

                    // Call the inline asm
                    let name = CString::new(if output_regs.is_empty() { "" } else { "asm_result" }).unwrap();
                    let result = LLVMBuildCall2(
                        self.builder,
                        asm_func_ty,
                        asm_val,
                        inputs.as_mut_ptr(),
                        inputs.len() as u32,
                        name.as_ptr(),
                    );

                    // Store outputs
                    if output_regs.len() == 1 {
                        self.value_map.insert(output_regs[0], result);
                    } else if output_regs.len() > 1 {
                        for (i, reg) in output_regs.iter().enumerate() {
                            let extract_name = CString::new(format!("asm_out_{}", i)).unwrap();
                            let extracted = LLVMBuildExtractValue(self.builder, result, i as u32, extract_name.as_ptr());
                            self.value_map.insert(*reg, extracted);
                        }
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
                    let abi = self.current_abi.clone().unwrap();
                    if let Some(value) = value {
                        let value = self.get_value(value)?;
                        if let Some(slot) = self.return_slot {
                            let store = LLVMBuildStore(self.builder, value, slot);
                            LLVMSetAlignment(store, 1);
                            LLVMBuildRetVoid(self.builder);
                        } else if let Some(value) = self.encode_abi(value, &abi.result.0, &abi.result.1) {
                            LLVMBuildRet(self.builder, value);
                        } else { LLVMBuildRetVoid(self.builder); }
                    } else { LLVMBuildRetVoid(self.builder); }
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

    unsafe fn guard_integer_division(&self, lhs: LLVMValueRef, rhs: LLVMValueRef, signed: bool) -> Result<()> {
        let ty = LLVMTypeOf(lhs);
        let zero = LLVMConstNull(ty);
        let mut invalid = LLVMBuildICmp(self.builder, LLVMIntPredicate::LLVMIntEQ, rhs, zero, c"div.zero".as_ptr());
        if signed {
            let bits = LLVMGetIntTypeWidth(ty);
            let minimum = LLVMConstInt(ty, 1u64 << (bits - 1), 0);
            let negative_one = LLVMConstAllOnes(ty);
            let min_lhs = LLVMBuildICmp(self.builder, LLVMIntPredicate::LLVMIntEQ, lhs, minimum, c"div.min".as_ptr());
            let minus_one_rhs = LLVMBuildICmp(self.builder, LLVMIntPredicate::LLVMIntEQ, rhs, negative_one, c"div.minusone".as_ptr());
            let overflow = LLVMBuildAnd(self.builder, min_lhs, minus_one_rhs, c"div.overflow".as_ptr());
            invalid = LLVMBuildOr(self.builder, invalid, overflow, c"div.invalid".as_ptr());
        }
        let function = self.current_function.ok_or_else(|| Error::CodeGen("division outside function".into()))?;
        let trap_block = LLVMAppendBasicBlockInContext(self.context, function, c"arithmetic.trap".as_ptr());
        let valid_block = LLVMAppendBasicBlockInContext(self.context, function, c"arithmetic.valid".as_ptr());
        LLVMBuildCondBr(self.builder, invalid, trap_block, valid_block);
        LLVMPositionBuilderAtEnd(self.builder, trap_block);
        let trap_type = LLVMFunctionType(LLVMVoidTypeInContext(self.context), ptr::null_mut(), 0, 0);
        let mut trap = LLVMGetNamedFunction(self.module, c"llvm.trap".as_ptr());
        if trap.is_null() { trap = LLVMAddFunction(self.module, c"llvm.trap".as_ptr(), trap_type); }
        LLVMBuildCall2(self.builder, trap_type, trap, ptr::null_mut(), 0, c"".as_ptr());
        LLVMBuildUnreachable(self.builder);
        LLVMPositionBuilderAtEnd(self.builder, valid_block);
        Ok(())
    }

    fn is_unsigned(ty: &IRType) -> bool {
        matches!(ty, IRType::U8 | IRType::U16 | IRType::U32 | IRType::U64 | IRType::Bool)
    }

    fn value_type(&self, value: &Value) -> Option<IRType> {
        match value {
            Value::Global(name) => self.global_types.get(name).map(|ty| IRType::Ptr(Box::new(ty.clone()))),
            Value::Register(reg) => self.register_types.get(reg).cloned(),
            Value::Parameter(i) => self.parameter_types.get(*i).cloned(),
            Value::Unit => Some(IRType::Unit),
            Value::Constant(Constant::Int(_)) => Some(IRType::I64),
            Value::Constant(Constant::Float(_)) => Some(IRType::F64),
            Value::Constant(Constant::Bool(_)) => Some(IRType::Bool),
            Value::Constant(Constant::String(_)) => Some(IRType::Ptr(Box::new(IRType::U8))),
            _ => None,
        }
    }

    unsafe fn cast_scalar(&self, val: LLVMValueRef, source: &Value, ty: &IRType) -> Result<LLVMValueRef> {
        use llvm_sys::LLVMTypeKind::*;
        let src_ty = LLVMTypeOf(val);
        let dest_ty = self.ir_type_to_llvm(ty);
        if src_ty == dest_ty { return Ok(val); }
        let src_kind = LLVMGetTypeKind(src_ty);
        let dst_kind = LLVMGetTypeKind(dest_ty);
        let unsigned = self.value_type(source).map(|t| Self::is_unsigned(&t)).unwrap_or(false);
        let name = c"".as_ptr();
        Ok(match (src_kind, dst_kind) {
            (LLVMIntegerTypeKind, LLVMIntegerTypeKind) => {
                if LLVMGetIntTypeWidth(dest_ty) < LLVMGetIntTypeWidth(src_ty) {
                    LLVMBuildTrunc(self.builder, val, dest_ty, name)
                } else if unsigned { LLVMBuildZExt(self.builder, val, dest_ty, name) }
                else { LLVMBuildSExt(self.builder, val, dest_ty, name) }
            }
            (LLVMPointerTypeKind, LLVMIntegerTypeKind) => LLVMBuildPtrToInt(self.builder, val, dest_ty, name),
            (LLVMIntegerTypeKind, LLVMPointerTypeKind) => LLVMBuildIntToPtr(self.builder, val, dest_ty, name),
            (LLVMPointerTypeKind, LLVMPointerTypeKind) => val,
            (LLVMFloatTypeKind, LLVMDoubleTypeKind) => LLVMBuildFPExt(self.builder, val, dest_ty, name),
            (LLVMDoubleTypeKind, LLVMFloatTypeKind) => LLVMBuildFPTrunc(self.builder, val, dest_ty, name),
            (LLVMIntegerTypeKind, LLVMFloatTypeKind | LLVMDoubleTypeKind) => {
                if unsigned { LLVMBuildUIToFP(self.builder, val, dest_ty, name) }
                else { LLVMBuildSIToFP(self.builder, val, dest_ty, name) }
            }
            (LLVMFloatTypeKind | LLVMDoubleTypeKind, LLVMIntegerTypeKind) => {
                let signed = if Self::is_unsigned(ty) { "u" } else { "s" };
                let source_bits = if src_kind == LLVMFloatTypeKind { 32 } else { 64 };
                let intrinsic = CString::new(format!("llvm.fpto{}i.sat.i{}.f{}", signed, LLVMGetIntTypeWidth(dest_ty), source_bits)).unwrap();
                let mut function = LLVMGetNamedFunction(self.module, intrinsic.as_ptr());
                let function_type = LLVMFunctionType(dest_ty, [src_ty].as_mut_ptr(), 1, 0);
                if function.is_null() { function = LLVMAddFunction(self.module, intrinsic.as_ptr(), function_type); }
                LLVMBuildCall2(self.builder, function_type, function, [val].as_mut_ptr(), 1, name)
            }
            _ => return Err(Error::CodeGen(format!("unsupported scalar cast to {:?}", ty))),
        })
    }

    /// Get LLVM value from IR value
    fn get_value(&self, val: &Value) -> Result<LLVMValueRef> {
        unsafe {
            match val {
                Value::Register(reg) => {
                    // Check if this register uses alloca (multiply-assigned)
                    if let Some(&alloca) = self.alloca_map.get(reg) {
                        // Load from alloca
                        let name = CString::new("").unwrap();
                        let ty = self.register_types.get(reg).cloned().unwrap_or(IRType::I64);
                        let loaded = LLVMBuildLoad2(self.builder, self.ir_type_to_llvm(&ty), alloca, name.as_ptr());
                        return Ok(loaded);
                    }
                    
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
                            // Preserve the byte sequence, including interior NULs.
                            let bytes = s.as_bytes();
                            let initializer = LLVMConstStringInContext2(self.context,
                                bytes.as_ptr() as *const _, bytes.len(), 0);
                            let global = LLVMAddGlobal(self.module, LLVMTypeOf(initializer), c"str".as_ptr());
                            LLVMSetInitializer(global, initializer);
                            LLVMSetGlobalConstant(global, 1);
                            LLVMSetLinkage(global, llvm_sys::LLVMLinkage::LLVMPrivateLinkage);
                            LLVMSetUnnamedAddress(global, llvm_sys::LLVMUnnamedAddr::LLVMGlobalUnnamedAddr);
                            Ok(global)
                        }
                        Constant::Null => {
                            let ptr_ty = LLVMPointerType(LLVMInt8TypeInContext(self.context), 0);
                            Ok(LLVMConstNull(ptr_ty))
                        }
                    }
                }
                Value::Parameter(i) => {
                    // Parameters are stored with offset 1000
                    self.parameter_values.get(*i)
                        .copied()
                        .ok_or_else(|| Error::CodeGen(format!("Unknown parameter: {}", i)))
                }
                Value::Global(name) => {
                    let name_c = CString::new(name.as_str()).unwrap();
                    let global = LLVMGetNamedGlobal(self.module, name_c.as_ptr());
                    if global.is_null() { Err(Error::CodeGen(format!("Unknown global: {}", name))) }
                    else { Ok(global) }
                }
                Value::Unit => Ok(LLVMConstNull(self.ir_type_to_llvm(&IRType::Unit)))
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

    fn initialize_target(&mut self) -> Result<()> {
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
                match self.opt_level {
                    0 => LLVMCodeGenOptLevel::LLVMCodeGenLevelNone,
                    1 => LLVMCodeGenOptLevel::LLVMCodeGenLevelLess,
                    2 => LLVMCodeGenOptLevel::LLVMCodeGenLevelDefault,
                    _ => LLVMCodeGenOptLevel::LLVMCodeGenLevelAggressive,
                },
                LLVMRelocMode::LLVMRelocDefault,
                LLVMCodeModel::LLVMCodeModelDefault
            );
            
            if target_machine.is_null() {
                return Err(Error::CodeGen("Failed to create target machine".to_string()));
            }
            
            // Get data layout
            let data_layout = LLVMCreateTargetDataLayout(target_machine);
            LLVMSetModuleDataLayout(self.module, data_layout);
            self.target_data = data_layout;
            self.target_machine = target_machine;
            LLVMSetTarget(self.module, triple.as_ptr());

        }
        Ok(())
    }

    fn optimize_module(&self) -> Result<()> {
        unsafe {
            let target_machine = self.target_machine;
            if self.opt_level > 0 {
                use llvm_sys::transforms::pass_builder::*;
                let pipeline = CString::new(format!("default<O{}>", self.opt_level)).unwrap();
                let options = LLVMCreatePassBuilderOptions();
                let error = LLVMRunPasses(self.module, pipeline.as_ptr(), target_machine, options);
                LLVMDisposePassBuilderOptions(options);
                if !error.is_null() {
                    let message = llvm_sys::error::LLVMGetErrorMessage(error);
                    let detail = CStr::from_ptr(message).to_string_lossy().into_owned();
                    llvm_sys::error::LLVMDisposeErrorMessage(message);
                    return Err(Error::CodeGen(format!("LLVM optimization failed: {}", detail)));
                }
                if let Err(error) = self.verify_module() {
                    return Err(error);
                }
            }

        }
        self.verify_module()
    }

    /// Return verified target-layout IR before machine emission can rewrite it.
    pub fn generate_llvm_ir(&mut self, module: &IRModule) -> Result<String> {
        self.prepare_module(module)?;
        self.optimize_module()?;
        Ok(self.print_ir())
    }

    /// Emit object file
    fn emit_object(&self) -> Result<Vec<u8>> {
        unsafe {
            let target_machine = self.target_machine;
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

impl LLVMCodeGen {
    fn prepare_module(&mut self, module: &IRModule) -> Result<()> {
        Self::init_targets();
        self.initialize_target()?;
        self.type_definitions = module.structs.iter().map(|definition| (definition.name.clone(), definition.clone())).collect();
        self.global_types = module.globals.iter().map(|global| (global.name.clone(), global.ty.clone())).collect();
        
        // Set module name
        unsafe {
            let name = CString::new(module.name.as_str()).unwrap();
            LLVMSetModuleIdentifier(self.module, name.as_ptr(), module.name.len());
        }
        
        // Establish every nominal name before any field or ABI references it.
        for definition in &module.structs {
            unsafe {
                let name = CString::new(definition.name.as_str()).unwrap();
                if LLVMGetTypeByName2(self.context, name.as_ptr()).is_null() {
                    LLVMStructCreateNamed(self.context, name.as_ptr());
                }
            }
        }
        // Declare struct types
        for ir_struct in &module.structs {
            unsafe {
                let name_c = CString::new(ir_struct.name.as_str()).unwrap();
                let existing = LLVMGetTypeByName2(self.context, name_c.as_ptr());
                let struct_ty = if existing.is_null() { LLVMStructCreateNamed(self.context, name_c.as_ptr()) } else { existing };
                if LLVMIsOpaqueStruct(struct_ty) != 0 {
                    // Convert field types
                    let mut field_types: Vec<LLVMTypeRef> = ir_struct.fields.iter()
                        .map(|(_, ty)| self.ir_type_to_llvm(ty))
                        .collect();
                    if ir_struct.is_union {
                        let size = field_types.iter().map(|ty| LLVMABISizeOfType(self.target_data, *ty)).max().unwrap_or(0);
                        if ir_struct.repr == StructRepr::Packed {
                            field_types = vec![LLVMArrayType2(LLVMInt8TypeInContext(self.context), size)];
                        } else {
                            let carrier = field_types.iter().copied().max_by_key(|ty| (LLVMABIAlignmentOfType(self.target_data, *ty), LLVMABISizeOfType(self.target_data, *ty)))
                                .unwrap_or(LLVMArrayType2(LLVMInt8TypeInContext(self.context), 0));
                            let align = LLVMABIAlignmentOfType(self.target_data, carrier) as u64;
                            let size = size.div_ceil(align) * align;
                            let padding = size - LLVMABISizeOfType(self.target_data, carrier);
                            field_types = vec![carrier];
                            if padding > 0 { field_types.push(LLVMArrayType2(LLVMInt8TypeInContext(self.context), padding)); }
                        }
                    }
                    // Set struct body with appropriate packing based on repr
                    let packed = match ir_struct.repr {
                        StructRepr::Packed => 1,  // packed (no padding)
                        _ => 0,  // C, Default, Transparent all use natural alignment
                    };
                    LLVMStructSetBody(
                        struct_ty,
                        field_types.as_mut_ptr(),
                        field_types.len() as u32,
                        packed
                    );
                }
            }
        }
        
        self.generate_globals(&module.globals)?;

        // Layouts are complete before target ABI classification.
        for function in &module.externs {
            self.declare_abi(&function.name, &function.params, &function.ret_type, function.variadic, true)?;
        }
        for function in &module.functions {
            self.declare_abi(&function.name, &function.params, &function.ret_type, false, function.c_abi)?;
        }
        // Generate code for each function
        for func in &module.functions {
            self.generate_function(func)?;
        }
        
        // Verify
        self.verify_module()?;
        
        Ok(())
    }
    

}

impl CodeGen for LLVMCodeGen {
    fn generate(&mut self, module: &IRModule) -> Result<Vec<u8>> {
        self.prepare_module(module)?;
        self.optimize_module()?;
        self.emit_object()
    }
    fn target_triple(&self) -> &str {
        &self.target_triple
    }
    
    fn name(&self) -> &str {
        "LLVM"
    }
}

include!("llvm_abi.rs");
include!("llvm_globals.rs");

impl Drop for LLVMCodeGen {
    fn drop(&mut self) {
        unsafe {
            if !self.target_data.is_null() { LLVMDisposeTargetData(self.target_data); }
            if !self.target_machine.is_null() { LLVMDisposeTargetMachine(self.target_machine); }
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
    fn target_registration_is_safe_concurrently() {
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let threads: Vec<_> = (0..8).map(|_| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                LLVMCodeGen::init_targets();
            })
        }).collect();
        for thread in threads { thread.join().unwrap(); }
    }

    #[test]
    fn test_empty_function() {
        let ir_module = compile_to_ir("fn main() {}");
        let mut codegen = LLVMCodeGen::new("x86_64-unknown-linux-gnu");
        
        // Should not panic
        let result = codegen.generate(&ir_module);
        println!("LLVM IR:\n{}", codegen.print_ir());
        assert!(result.is_ok());
    }

    #[test]
    fn test_return_constant() {
        let ir_module = compile_to_ir("fn answer() -> i64 { return 42 }");
        let mut codegen = LLVMCodeGen::new("x86_64-unknown-linux-gnu");
        
        let result = codegen.generate(&ir_module);
        println!("LLVM IR:\n{}", codegen.print_ir());
        assert!(result.is_ok());
    }

    #[test]
    fn test_binary_expression() {
        let ir_module = compile_to_ir("fn add() -> i64 { return 1 + 2 }");
        let mut codegen = LLVMCodeGen::new("x86_64-unknown-linux-gnu");
        
        let result = codegen.generate(&ir_module);
        println!("LLVM IR:\n{}", codegen.print_ir());
        assert!(result.is_ok());
    }
}

#[path = "llvm_memory.rs"]
mod memory;
