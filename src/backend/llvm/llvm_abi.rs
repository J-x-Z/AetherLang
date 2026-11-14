// Target ABI lowering. Aether IR keeps aggregate values intact; this module
// alone translates them to the register/indirect convention at function edges.
impl LLVMCodeGen {
    unsafe fn homogeneous_float(&self, ty: &IRType) -> Option<(LLVMTypeRef, usize)> {
        match ty {
            IRType::F32 | IRType::F64 => Some((self.ir_type_to_llvm(ty), 1)),
            IRType::Array(element, length) => {
                let (kind, count) = self.homogeneous_float(element)?;
                let total = count * length;
                (total > 0 && total <= 4).then_some((kind, total))
            }
            IRType::Struct(name) => {
                let definition = self.type_definitions.get(name)?;
                let mut result = None;
                for (_, field) in &definition.fields {
                    let (kind, count) = self.homogeneous_float(field)?;
                    match result {
                        None => result = Some((kind, count)),
                        Some((previous, total)) if previous == kind => result = Some((kind, if definition.is_union { total.max(count) } else { total + count })),
                        _ => return None,
                    }
                }
                let (kind, count) = result?;
                (count <= 4 && LLVMABISizeOfType(self.target_data, self.ir_type_to_llvm(ty)) == count as u64 * LLVMABISizeOfType(self.target_data, kind)).then_some((kind, count))
            }
            _ => None,
        }
    }

    unsafe fn classify_abi(&self, ty: &IRType, result: bool, c_abi: bool) -> Result<AbiValue> {
        let native = self.ir_type_to_llvm(ty);
        if *ty == IRType::Unit { return Ok(AbiValue::Ignore); }
        if !c_abi { return Ok(AbiValue::Direct(native)); }
        if !matches!(ty, IRType::Array(_, _) | IRType::Struct(_)) { return Ok(AbiValue::Direct(native)); }
        if !(self.target_triple.contains("apple") && (self.target_triple.contains("aarch64") || self.target_triple.contains("arm64"))) {
            return Err(Error::CodeGen(format!("aggregate C ABI lowering is currently implemented for Apple arm64; target '{}' requires its own ABI classifier", self.target_triple)));
        }
        let size = LLVMABISizeOfType(self.target_data, native);
        if size == 0 { return Ok(AbiValue::Ignore); }
        if let Some((float, count)) = self.homogeneous_float(ty) {
            return Ok(if result { AbiValue::Direct(native) } else { AbiValue::Coerce(LLVMArrayType2(float, count as u64)) });
        }
        if size > 16 { return Ok(AbiValue::Indirect); }
        Ok(AbiValue::Coerce(if size <= 8 {
            LLVMIntTypeInContext(self.context, if result { (size * 8) as u32 } else { 64 })
        } else { LLVMArrayType2(LLVMInt64TypeInContext(self.context), 2) }))
    }

    unsafe fn abi_type(&self, ty: &AbiValue) -> Option<LLVMTypeRef> {
        match ty {
            AbiValue::Direct(ty) | AbiValue::Coerce(ty) => Some(*ty),
            AbiValue::Indirect => Some(LLVMPointerTypeInContext(self.context, 0)),
            AbiValue::Ignore => None,
        }
    }

    fn declare_abi(&mut self, name: &str, params: &[(String, IRType)], ret: &IRType, variadic: bool, c_abi: bool) -> Result<()> {
        unsafe {
            let abi = FunctionAbi {
                params: params.iter().map(|(_, ty)| Ok((ty.clone(), self.classify_abi(ty, false, c_abi)?))).collect::<Result<_>>()?,
                result: (ret.clone(), self.classify_abi(ret, true, c_abi)?),
                variadic,
            };
            let mut parameter_types = Vec::new();
            if matches!(abi.result.1, AbiValue::Indirect) { parameter_types.push(LLVMPointerTypeInContext(self.context, 0)); }
            for (_, parameter) in &abi.params {
                if let Some(ty) = self.abi_type(parameter) { parameter_types.push(ty); }
            }
            let result_type = match abi.result.1 {
                AbiValue::Indirect | AbiValue::Ignore => LLVMVoidTypeInContext(self.context),
                AbiValue::Direct(ty) | AbiValue::Coerce(ty) => ty,
            };
            let function_type = LLVMFunctionType(result_type, parameter_types.as_mut_ptr(), parameter_types.len() as u32, variadic as i32);
            let c_name = CString::new(name).unwrap();
            let mut function = LLVMGetNamedFunction(self.module, c_name.as_ptr());
            if function.is_null() { function = LLVMAddFunction(self.module, c_name.as_ptr(), function_type); }
            self.add_abi_attributes(function, &abi, false);
            self.abi.insert(name.into(), abi);
        }
        Ok(())
    }

    unsafe fn add_abi_attributes(&self, function: LLVMValueRef, abi: &FunctionAbi, call: bool) {
        if matches!(abi.result.1, AbiValue::Indirect) {
            let kind = LLVMGetEnumAttributeKindForName(c"sret".as_ptr(), 4);
            let attr = LLVMCreateTypeAttribute(self.context, kind, self.ir_type_to_llvm(&abi.result.0));
            if call { LLVMAddCallSiteAttribute(function, 1, attr); }
            else { LLVMAddAttributeAtIndex(function, 1, attr); }
        }
    }

    /// ABI temporary storage is reserved once in the entry block, including
    /// calls inside loops. Coercion never allocates a fresh stack object per turn.
    unsafe fn entry_alloca(&self, ty: LLVMTypeRef) -> LLVMValueRef {
        let entry = LLVMGetEntryBasicBlock(self.current_function.unwrap());
        let builder = LLVMCreateBuilderInContext(self.context);
        let first = LLVMGetFirstInstruction(entry);
        if first.is_null() { LLVMPositionBuilderAtEnd(builder, entry); }
        else { LLVMPositionBuilderBefore(builder, first); }
        let slot = LLVMBuildAlloca(builder, ty, c"abi.slot".as_ptr());
        LLVMDisposeBuilder(builder);
        slot
    }

    unsafe fn unaligned_load(&self, ty: LLVMTypeRef, pointer: LLVMValueRef) -> LLVMValueRef {
        let value = LLVMBuildLoad2(self.builder, ty, pointer, c"".as_ptr());
        LLVMSetAlignment(value, 1);
        value
    }

    /// Reinterpret only allocated bytes: odd-sized and packed aggregates can
    /// have a larger ABI register representation than their physical layout.
    unsafe fn coerce_abi(&self, value: LLVMValueRef, destination: LLVMTypeRef) -> LLVMValueRef {
        let source = LLVMTypeOf(value);
        if source == destination { return value; }
        let size = LLVMABISizeOfType(self.target_data, source).max(LLVMABISizeOfType(self.target_data, destination));
        let bytes = LLVMArrayType2(LLVMInt8TypeInContext(self.context), size);
        let slot = self.entry_alloca(bytes);
        let zero = LLVMBuildStore(self.builder, LLVMConstNull(bytes), slot);
        LLVMSetAlignment(zero, 1);
        let store = LLVMBuildStore(self.builder, value, slot);
        LLVMSetAlignment(store, 1);
        self.unaligned_load(destination, slot)
    }

    unsafe fn encode_abi(&self, value: LLVMValueRef, semantic: &IRType, lowering: &AbiValue) -> Option<LLVMValueRef> {
        Some(match lowering {
            AbiValue::Ignore => return None,
            AbiValue::Direct(ty) => {
                // Scalar literals may still have their default integer width.
                if LLVMTypeOf(value) != *ty && LLVMGetTypeKind(*ty) == llvm_sys::LLVMTypeKind::LLVMIntegerTypeKind {
                    LLVMBuildIntCast2(self.builder, value, *ty, semantic.is_signed_integer() as i32, c"".as_ptr())
                } else { value }
            }
            AbiValue::Coerce(ty) => self.coerce_abi(value, *ty),
            AbiValue::Indirect => {
                let slot = self.entry_alloca(self.ir_type_to_llvm(semantic));
                let store = LLVMBuildStore(self.builder, value, slot);
                LLVMSetAlignment(store, 1);
                slot
            }
        })
    }

    unsafe fn decode_abi(&self, value: LLVMValueRef, semantic: &IRType, lowering: &AbiValue) -> LLVMValueRef {
        match lowering {
            AbiValue::Ignore => LLVMConstNull(self.ir_type_to_llvm(semantic)),
            AbiValue::Direct(_) => value,
            AbiValue::Coerce(_) => self.coerce_abi(value, self.ir_type_to_llvm(semantic)),
            AbiValue::Indirect => self.unaligned_load(self.ir_type_to_llvm(semantic), value),
        }
    }
}
