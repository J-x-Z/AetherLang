// Target ABI lowering. Aether IR keeps aggregate values intact; this module
// alone translates them to the register/indirect convention at function edges.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EightbyteClass { None, Integer, Sse }
#[derive(Clone, Copy)]
struct Eightbyte { class: EightbyteClass, end: u64, floats: u8, double: bool }
impl Default for Eightbyte {
    fn default() -> Self { Self { class: EightbyteClass::None, end: 0, floats: 0, double: false } }
}

impl LLVMCodeGen {
    unsafe fn homogeneous_float(&self, ty: &IRType) -> Option<(LLVMTypeRef, usize)> {
        match ty {
            IRType::F32 | IRType::F64 => Some((self.ir_type_to_llvm(ty), 1)),
            IRType::Array(element, length) => {
                let (kind, count) = self.homogeneous_float(element)?;
                let total = count.checked_mul(*length)?;
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

    fn contains_vector(&self, ty: &IRType) -> bool {
        match ty { IRType::Vector(..)=>true, IRType::Array(element,_)|IRType::Volatile(element)=>self.contains_vector(element),
            IRType::Struct(name)=>self.type_definitions.get(name).is_some_and(|definition|definition.fields.iter().any(|(_,field)|self.contains_vector(field))), _=>false }
    }
    /// AMD64 SysV 3.2.3: recursively merge scalar fields into eightbytes.
    /// Vectors/x87 are deliberately outside the currently supported subset.
    unsafe fn sysv_fields(&self, ty: &IRType, offset: u64, classes: &mut [Eightbyte; 2]) -> Result<bool> {
        let ty = ty.unqualified();
        let native = self.ir_type_to_llvm(ty);
        let size = LLVMABISizeOfType(self.target_data, native);
        if size == 0 { return Ok(true); }
        if offset % LLVMABIAlignmentOfType(self.target_data, native) as u64 != 0 { return Ok(false); }
        match ty {
            IRType::Array(element, count) => {
                let stride = LLVMABISizeOfType(self.target_data, self.ir_type_to_llvm(element));
                for index in 0..*count {
                    if !self.sysv_fields(element, offset + index as u64 * stride, classes)? { return Ok(false); }
                }
            }
            IRType::Struct(name) => {
                let definition = self.type_definitions.get(name).ok_or_else(|| Error::CodeGen(format!("missing ABI layout for {}", name)))?;
                for (index, (_, field)) in definition.fields.iter().enumerate() {
                    let position = if definition.is_union { 0 } else { LLVMOffsetOfElement(self.target_data, native, index as u32) };
                    if !self.sysv_fields(field, offset + position, classes)? { return Ok(false); }
                }
            }
            _ => {
                let class = if matches!(ty, IRType::F32 | IRType::F64) { EightbyteClass::Sse } else { EightbyteClass::Integer };
                for byte in offset..offset + size {
                    let Some(chunk) = classes.get_mut((byte / 8) as usize) else { return Ok(false); };
                    chunk.end = chunk.end.max(byte % 8 + 1);
                    chunk.class = match (chunk.class, class) {
                        (EightbyteClass::Integer, _) | (_, EightbyteClass::Integer) => EightbyteClass::Integer,
                        _ => EightbyteClass::Sse,
                    };
                    if matches!(ty, IRType::F64) { chunk.double = true; }
                    if matches!(ty, IRType::F32) { chunk.floats |= 1 << ((offset % 8) / 4); }
                }
            }
        }
        Ok(true)
    }

    unsafe fn classify_sysv(&self, ty: &IRType, result: bool) -> Result<AbiValue> {
        let native = self.ir_type_to_llvm(ty);
        let size = LLVMABISizeOfType(self.target_data, native);
        if size == 0 { return Ok(AbiValue::Ignore); }
        let memory = || if result { AbiValue::Indirect } else { AbiValue::ByVal { alignment: LLVMABIAlignmentOfType(self.target_data, native).max(8) } };
        if size > 16 { return Ok(memory()); }
        let mut classes = [Eightbyte::default(); 2];
        if !self.sysv_fields(ty, 0, &mut classes)? { return Ok(memory()); }
        let mut parts = Vec::new();
        for (index, chunk) in classes.iter().enumerate() {
            let lowered = match chunk.class {
                EightbyteClass::None => continue,
                EightbyteClass::Integer => LLVMIntTypeInContext(self.context, (chunk.end * 8) as u32),
                EightbyteClass::Sse if chunk.double => LLVMDoubleTypeInContext(self.context),
                EightbyteClass::Sse if chunk.floats == 1 => LLVMFloatTypeInContext(self.context),
                EightbyteClass::Sse => LLVMVectorType(LLVMFloatTypeInContext(self.context), 2),
            };
            parts.push(AbiPart { ty: lowered, offset: index as u64 * 8 });
        }
        Ok(if parts.is_empty() { AbiValue::Ignore } else { AbiValue::Expand(parts) })
    }

    unsafe fn indirect_aggregate(&self, native: LLVMTypeRef) -> bool {
        matches!(LLVMGetTypeKind(native), llvm_sys::LLVMTypeKind::LLVMArrayTypeKind | llvm_sys::LLVMTypeKind::LLVMStructTypeKind)
            && LLVMTypeIsSized(native) != 0
            && LLVMABISizeOfType(self.target_data, native) > 2 * u64::from(LLVMPointerSize(self.target_data))
    }

    unsafe fn classify_abi(&self, ty: &IRType, result: bool, c_abi: bool) -> Result<AbiValue> {
        let native = self.ir_type_to_llvm(ty);
        if *ty == IRType::Unit { return Ok(AbiValue::Ignore); }
        if !c_abi {
            // Large Aether values travel in caller-owned storage. Expanding
            // complete aggregate parameters or returns into registers can
            // overwhelm instruction selection even when local copies are lowered.
            if self.indirect_aggregate(native) {
                return Ok(AbiValue::Indirect);
            }
            return Ok(AbiValue::Direct(native));
        }
        let target = ForeignAbiTarget::for_triple(&self.target_triple);
        if self.contains_vector(ty) {
            if matches!(ty, IRType::Vector(..)) && LLVMABISizeOfType(self.target_data,native)==16 && target.supports_vector128() {
                return Ok(AbiValue::Direct(native));
            }
            return Err(Error::CodeGen(VECTOR_ABI_ERROR.into()));
        }
        if !matches!(ty, IRType::Array(_, _) | IRType::Struct(_)) { return Ok(AbiValue::Direct(native)); }
        if target == ForeignAbiTarget::SysvX86_64 { return self.classify_sysv(ty, result); }
        if target != ForeignAbiTarget::DarwinAarch64 { return Err(Error::CodeGen(format!("{}: '{}'", AGGREGATE_ABI_ERROR, self.target_triple))); }
        let size = LLVMABISizeOfType(self.target_data, native);
        if size == 0 { return Ok(AbiValue::Ignore); }
        if let Some((float, count)) = self.homogeneous_float(ty) {
            return Ok(AbiValue::Coerce(LLVMArrayType2(float, count as u64)));
        }
        if size > 16 { return Ok(AbiValue::Indirect); }
        Ok(AbiValue::Coerce(if size <= 8 {
            LLVMIntTypeInContext(self.context, if result { (size * 8) as u32 } else { 64 })
        } else { LLVMArrayType2(LLVMInt64TypeInContext(self.context), 2) }))
    }

    unsafe fn abi_type(&self, ty: &AbiValue) -> Option<LLVMTypeRef> {
        match ty {
            AbiValue::Direct(ty) | AbiValue::Coerce(ty) => Some(*ty),
            AbiValue::Indirect | AbiValue::ByVal { .. } => Some(LLVMPointerTypeInContext(self.context, 0)),
            AbiValue::Expand(parts) if parts.len() == 1 => Some(parts[0].ty),
            AbiValue::Expand(parts) => {
                let mut types: Vec<_> = parts.iter().map(|part| part.ty).collect();
                Some(LLVMStructTypeInContext(self.context, types.as_mut_ptr(), types.len() as u32, 0))
            }
            AbiValue::Ignore => None,
        }
    }

    fn declare_abi(&mut self, name: &str, params: &[(String, IRType)], ret: &IRType, variadic: bool, c_abi: bool) -> Result<LLVMValueRef> {
        unsafe {
            let arch = self.target_triple.split('-').next().unwrap_or("");
            let extend_narrow_integers = c_abi && match arch {
                "aarch64" | "arm64" => self.target_triple.contains("-apple-"),
                "x86_64" => self.target_triple.contains("-apple-") || self.target_triple.contains("-linux-"),
                _ => false,
            };
            let result = (ret.clone(), self.classify_abi(ret, true, c_abi)?);
            let sysv = c_abi && ForeignAbiTarget::for_triple(&self.target_triple) == ForeignAbiTarget::SysvX86_64;
            let mut gp = if matches!(result.1, AbiValue::Indirect) { 5usize } else { 6usize };
            let mut sse = 8usize;
            let mut lowered_params = Vec::new();
            for (_, ty) in params {
                let mut lowering = self.classify_abi(ty, false, c_abi)?;
                if sysv {
                    match &lowering {
                        AbiValue::Expand(parts) => {
                            let integer = parts.iter().filter(|part| LLVMGetTypeKind(part.ty) == llvm_sys::LLVMTypeKind::LLVMIntegerTypeKind).count();
                            let float = parts.len() - integer;
                            if integer > gp || float > sse {
                                lowering = AbiValue::ByVal { alignment: LLVMABIAlignmentOfType(self.target_data, self.ir_type_to_llvm(ty)).max(8) };
                            } else { gp -= integer; sse -= float; }
                        }
                        AbiValue::Direct(_) if matches!(ty.unqualified(), IRType::F32 | IRType::F64) => sse = sse.saturating_sub(1),
                        AbiValue::Direct(_) => gp = gp.saturating_sub(1),
                        _ => {},
                    }
                }
                lowered_params.push((ty.clone(), lowering));
            }
            let abi = FunctionAbi { extend_narrow_integers, params: lowered_params, result, variadic };
            let mut parameter_types = Vec::new();
            if matches!(abi.result.1, AbiValue::Indirect) { parameter_types.push(LLVMPointerTypeInContext(self.context, 0)); }
            for (_, parameter) in &abi.params {
                if let AbiValue::Expand(parts) = parameter { parameter_types.extend(parts.iter().map(|part| part.ty)); }
                else if let Some(ty) = self.abi_type(parameter) { parameter_types.push(ty); }
            }
            let result_type = match abi.result.1 {
                AbiValue::Indirect | AbiValue::Ignore => LLVMVoidTypeInContext(self.context),
                _ => self.abi_type(&abi.result.1).expect("nonempty return ABI"),
            };
            let function_type = LLVMFunctionType(result_type, parameter_types.as_mut_ptr(), parameter_types.len() as u32, variadic as i32);
            let c_name = CString::new(name).unwrap();
            let mut function = LLVMGetNamedFunction(self.module, c_name.as_ptr());
            if function.is_null() { function = LLVMAddFunction(self.module, c_name.as_ptr(), function_type); }
            self.add_abi_attributes(function, &abi, false);
            self.abi.insert(name.into(), abi);
            Ok(function)
        }
    }

    unsafe fn add_abi_attributes(&self, function: LLVMValueRef, abi: &FunctionAbi, call: bool) {
        let attach = |index, attr| {
            if call { LLVMAddCallSiteAttribute(function, index, attr); }
            else { LLVMAddAttributeAtIndex(function, index, attr); }
        };
        let alignment = |index, amount| {
            let kind = LLVMGetEnumAttributeKindForName(c"align".as_ptr(), 5);
            attach(index, LLVMCreateEnumAttribute(self.context, kind, amount as u64));
        };
        let extend = |index, ty: &IRType| {
            if !abi.extend_narrow_integers { return; }
            let attribute = match ty.unqualified() {
                IRType::I8 | IRType::I16 => c"signext",
                IRType::U8 | IRType::U16 | IRType::Bool => c"zeroext",
                _ => return,
            };
            let kind = LLVMGetEnumAttributeKindForName(attribute.as_ptr(), attribute.to_bytes().len());
            attach(index, LLVMCreateEnumAttribute(self.context, kind, 0));
        };
        let mut index = 1;
        if matches!(abi.result.1, AbiValue::Indirect) {
            let ty = self.ir_type_to_llvm(&abi.result.0);
            let kind = LLVMGetEnumAttributeKindForName(c"sret".as_ptr(), 4);
            attach(index, LLVMCreateTypeAttribute(self.context, kind, ty));
            alignment(index, LLVMABIAlignmentOfType(self.target_data, ty));
            index += 1;
        } else { extend(0, &abi.result.0); }
        for (semantic, lowering) in &abi.params {
            match lowering {
                AbiValue::Ignore => {},
                AbiValue::Expand(parts) => index += parts.len() as u32,
                AbiValue::ByVal { alignment: align } => {
                    let kind = LLVMGetEnumAttributeKindForName(c"byval".as_ptr(), 5);
                    attach(index, LLVMCreateTypeAttribute(self.context, kind, self.ir_type_to_llvm(semantic)));
                    alignment(index, *align);
                    index += 1;
                }
                _ => { extend(index, semantic); index += 1; }
            }
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

    unsafe fn parts_slot(&self, semantic: &IRType, parts: &[AbiPart]) -> (LLVMTypeRef, LLVMValueRef) {
        let size = parts.iter().map(|part| part.offset + LLVMABISizeOfType(self.target_data, part.ty))
            .fold(LLVMABISizeOfType(self.target_data, self.ir_type_to_llvm(semantic)), u64::max);
        let bytes = LLVMArrayType2(LLVMInt8TypeInContext(self.context), size);
        let slot = self.entry_alloca(bytes);
        let clear = LLVMBuildStore(self.builder, LLVMConstNull(bytes), slot);
        LLVMSetAlignment(clear, 1);
        (bytes, slot)
    }
    unsafe fn byte_offset(&self, pointer: LLVMValueRef, offset: u64) -> LLVMValueRef {
        let mut index = LLVMConstInt(LLVMInt64TypeInContext(self.context), offset, 0);
        LLVMBuildGEP2(self.builder, LLVMInt8TypeInContext(self.context), pointer, &mut index, 1, c"abi.part".as_ptr())
    }
    unsafe fn encode_abi_parts(&self, value: LLVMValueRef, semantic: &IRType, parts: &[AbiPart]) -> Vec<LLVMValueRef> {
        let (_, slot) = self.parts_slot(semantic, parts);
        let store = LLVMBuildStore(self.builder, value, slot); LLVMSetAlignment(store, 1);
        parts.iter().map(|part| self.unaligned_load(part.ty, self.byte_offset(slot, part.offset))).collect()
    }
    unsafe fn decode_abi_parts(&self, values: &[LLVMValueRef], semantic: &IRType, parts: &[AbiPart]) -> LLVMValueRef {
        let (_, slot) = self.parts_slot(semantic, parts);
        for (value, part) in values.iter().zip(parts) {
            let store = LLVMBuildStore(self.builder, *value, self.byte_offset(slot, part.offset)); LLVMSetAlignment(store, 1);
        }
        self.unaligned_load(self.ir_type_to_llvm(semantic), slot)
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
            AbiValue::Expand(parts) => {
                let values = self.encode_abi_parts(value, semantic, parts);
                if values.len() == 1 { values[0] } else {
                    let mut result = LLVMGetUndef(self.abi_type(lowering).unwrap());
                    for (index, part) in values.iter().enumerate() { result = LLVMBuildInsertValue(self.builder, result, *part, index as u32, c"".as_ptr()); }
                    result
                }
            }
            AbiValue::Indirect | AbiValue::ByVal { .. } => {
                let slot = self.entry_alloca(self.ir_type_to_llvm(semantic));
                if let AbiValue::ByVal { alignment } = lowering { LLVMSetAlignment(slot, *alignment); }
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
            AbiValue::Expand(parts) => {
                let values: Vec<_> = if parts.len() == 1 { vec![value] } else {
                    (0..parts.len()).map(|index| LLVMBuildExtractValue(self.builder, value, index as u32, c"".as_ptr())).collect()
                };
                self.decode_abi_parts(&values, semantic, parts)
            }
            AbiValue::Indirect | AbiValue::ByVal { .. } => self.unaligned_load(self.ir_type_to_llvm(semantic), value),
        }
    }
}
