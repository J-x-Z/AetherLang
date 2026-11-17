// Global constants retain the target layout and relocations. A union initializer
// may select a different member from its alignment carrier; flattening leaves
// into explicit packed storage avoids illegal aggregate bitcasts and startup code.
impl LLVMCodeGen {
    unsafe fn layout_value(&self, query: IRLayoutQuery, ty: &IRType, field: Option<usize>) -> Result<u64> {
        if *ty == IRType::Void { return Ok(match query { IRLayoutQuery::Size => 0, IRLayoutQuery::Align => 1, IRLayoutQuery::Offset => return Err(Error::CodeGen("unit has no field offsets".into())) }); }
        let native = self.ir_type_to_llvm(ty);
        Ok(match query {
            IRLayoutQuery::Size => LLVMABISizeOfType(self.target_data, native),
            IRLayoutQuery::Align => LLVMABIAlignmentOfType(self.target_data, native) as u64,
            IRLayoutQuery::Offset => {
                let field = field.ok_or_else(|| Error::CodeGen("offset_of requires a field".into()))?;
                let definition = match ty { IRType::Struct(name) => self.type_definitions.get(name), _ => None }
                    .ok_or_else(|| Error::CodeGen("offset_of requires an aggregate".into()))?;
                if field >= definition.fields.len() { return Err(Error::CodeGen("offset_of field index is out of bounds".into())); }
                if definition.is_union { 0 } else { LLVMOffsetOfElement(self.target_data, native, field as u32) }
            }
        })
    }

    unsafe fn initializer_leaves(&self, value: &IRInitializer, ty: &IRType, base: u64,
        leaves: &mut Vec<(u64, IRType, IRInitializer)>) -> Result<()> {
        match value {
            IRInitializer::Zero => {}
            IRInitializer::Repeat(value,count)=>{
                let IRType::Array(element,size)=ty else{return Err(Error::CodeGen("repeat initializer requires array type".into()));};
                if count!=size{return Err(Error::CodeGen("repeat initializer length mismatch".into()));}
                let stride=LLVMABISizeOfType(self.target_data,self.ir_type_to_llvm(element));
                for index in 0..*count{self.initializer_leaves(value,element,base+index as u64*stride,leaves)?;}
            }
            IRInitializer::Aggregate(values) => match ty {
                IRType::Array(element, count) => {
                    let stride = LLVMABISizeOfType(self.target_data, self.ir_type_to_llvm(element));
                    for (index, value) in values {
                        if index >= count { return Err(Error::CodeGen("global array initializer index out of bounds".into())); }
                        self.initializer_leaves(value, element, base + *index as u64 * stride, leaves)?;
                    }
                }
                IRType::Struct(name) => {
                    let definition = self.type_definitions.get(name).ok_or_else(|| Error::CodeGen("missing initializer layout".into()))?;
                    for (index, value) in values {
                        let field = &definition.fields[*index].1;
                        let offset = self.layout_value(IRLayoutQuery::Offset, ty, Some(*index))?;
                        self.initializer_leaves(value, field, base + offset, leaves)?;
                    }
                }
                _ => return Err(Error::CodeGen("aggregate initializer has scalar type".into())),
            },
            _ => leaves.push((base, ty.clone(), value.clone())),
        }
        Ok(())
    }

    unsafe fn initializer_chunks(&self, global: &IRGlobal) -> Result<Vec<(IRType, IRInitializer)>> {
        let mut leaves = Vec::new();
        self.initializer_leaves(global.initializer.as_ref().unwrap(), &global.ty, 0, &mut leaves)?;
        leaves.sort_by_key(|(offset, _, _)| *offset);
        let mut chunks = Vec::new();
        let mut end = 0;
        for (offset, ty, value) in leaves {
            if offset < end { return Err(Error::CodeGen("overlapping global initializer fields".into())); }
            if offset > end { chunks.push((IRType::Array(Box::new(IRType::U8), (offset - end) as usize), IRInitializer::Zero)); }
            end = offset + LLVMABISizeOfType(self.target_data, self.ir_type_to_llvm(&ty));
            chunks.push((ty, value));
        }
        let size = LLVMABISizeOfType(self.target_data, self.ir_type_to_llvm(&global.ty));
        if size > end { chunks.push((IRType::Array(Box::new(IRType::U8), (size - end) as usize), IRInitializer::Zero)); }
        Ok(chunks)
    }

    unsafe fn initializer_leaf(&self, ty: &IRType, value: &IRInitializer) -> Result<LLVMValueRef> {
        let native = self.ir_type_to_llvm(ty);
        Ok(match value {
            IRInitializer::Zero => LLVMConstNull(native),
            IRInitializer::Scalar(Constant::Int(number)) => {
                if matches!(ty, IRType::Ptr(_)) {
                    LLVMConstIntToPtr(LLVMConstInt(LLVMInt64TypeInContext(self.context), *number as u64, 0), native)
                } else { LLVMConstInt(native, *number as u64, ty.is_signed_integer() as i32) }
            }
            IRInitializer::Scalar(Constant::Float(number)) => LLVMConstReal(native, *number),
            IRInitializer::Scalar(Constant::Bool(value)) => LLVMConstInt(native, *value as u64, 0),
            IRInitializer::Scalar(value @ Constant::String(_)) => self.get_value(&Value::Constant(value.clone()))?,
            IRInitializer::Scalar(Constant::Null) => LLVMConstNull(native),
            IRInitializer::Layout { query, ty, field } => LLVMConstInt(native, self.layout_value(*query, ty, *field)?, 0),
            IRInitializer::Address { global, path } => {
                let name = CString::new(global.as_str()).unwrap();
                let mut pointer = LLVMGetNamedGlobal(self.module, name.as_ptr());
                if pointer.is_null() { return Err(Error::CodeGen(format!("unknown static address {}", global))); }
                let mut current = self.global_types.get(global).cloned().ok_or_else(|| Error::CodeGen("unknown static type".into()))?;
                for index in path {
                    let next = match &current {
                        IRType::Array(element, _) => (**element).clone(),
                        IRType::Struct(name) => self.type_definitions[name].fields[*index].1.clone(),
                        _ => return Err(Error::CodeGen("invalid static address path".into())),
                    };
                    let union = matches!(&current, IRType::Struct(name) if self.type_definitions[name].is_union);
                    if !union {
                        // Array indices are pointer-sized signed GEP offsets. A fixed
                        // i32 index would turn 2^31 into a negative displacement.
                        // Struct field selectors, in contrast, must remain i32.
                        let index_ty = if matches!(&current, IRType::Array(_, _)) {
                            LLVMIntPtrTypeInContext(self.context, self.target_data)
                        } else { LLVMInt32TypeInContext(self.context) };
                        let mut indices = [LLVMConstInt(index_ty, 0, 0), LLVMConstInt(index_ty, *index as u64, 0)];
                        pointer = LLVMConstGEP2(self.ir_type_to_llvm(&current), pointer, indices.as_mut_ptr(), 2);
                    }
                    current = next;
                }
                pointer
            }
            _ => return Err(Error::CodeGen("invalid scalar static initializer".into())),
        })
    }

    fn generate_globals(&mut self, globals: &[IRGlobal]) -> Result<()> {
        unsafe {
            // Reserve every symbol before resolving any address relocations.
            for global in globals {
                let native = if global.initializer.is_some() {
                    let chunks = self.initializer_chunks(global)?;
                    let mut fields = chunks.iter().map(|(ty, _)| self.ir_type_to_llvm(ty)).collect::<Vec<_>>();
                    LLVMStructTypeInContext(self.context, fields.as_mut_ptr(), fields.len() as u32, 1)
                } else { self.ir_type_to_llvm(&global.ty) };
                let name = CString::new(global.name.as_str()).unwrap();
                let value = LLVMAddGlobal(self.module, native, name.as_ptr());
                if global.initializer.is_some() && !global.exported {
                    LLVMSetLinkage(value, llvm_sys::LLVMLinkage::LLVMInternalLinkage);
                }
                LLVMSetAlignment(value, LLVMABIAlignmentOfType(self.target_data, self.ir_type_to_llvm(&global.ty)));
                LLVMSetGlobalConstant(value, (!global.mutable) as i32);
            }
            for global in globals {
                if global.initializer.is_none() { continue; }
                let chunks = self.initializer_chunks(global)?;
                let mut values = chunks.iter().map(|(ty, value)| self.initializer_leaf(ty, value)).collect::<Result<Vec<_>>>()?;
                let initializer = LLVMConstStructInContext(self.context, values.as_mut_ptr(), values.len() as u32, 1);
                let name = CString::new(global.name.as_str()).unwrap();
                LLVMSetInitializer(LLVMGetNamedGlobal(self.module, name.as_ptr()), initializer);
            }
        }
        Ok(())
    }
}
