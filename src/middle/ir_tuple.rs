//! Structural tuples use the same value, copy and ABI machinery as aggregates.
//! Names are private implementation details; equality is checked on full fields.
use super::*;
use std::hash::{Hash, Hasher};

impl IRGenerator {
    pub(super) fn tuple_ir_type(elements: &[IRType]) -> IRType {
        if elements.is_empty() {
            return IRType::Unit;
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        format!("{elements:?}").hash(&mut hasher);
        IRType::Struct(format!("__aether_tuple_{:016x}", hasher.finish()))
    }

    pub(super) fn record_tuple_layout(&self, elements: Vec<IRType>) -> IRType {
        let ty = Self::tuple_ir_type(&elements);
        if let IRType::Struct(name) = &ty {
            // Keep both layouts if a hash collision occurs: final validation
            // must diagnose it rather than silently equate distinct types.
            let mut layouts = self.tuple_layouts.borrow_mut();
            if layouts
                .get(name)
                .is_some_and(|existing| *existing != elements)
            {
                layouts.insert(format!("{name}_collision"), elements);
            } else {
                layouts.insert(name.clone(), elements);
            }
        }
        ty
    }

    pub(super) fn tuple_fields(&self, name: &str) -> Option<Vec<(String, IRType)>> {
        self.tuple_layouts.borrow().get(name).map(|elements| {
            elements
                .iter()
                .enumerate()
                .map(|(i, ty)| (format!("_{i}"), ty.clone()))
                .collect()
        })
    }

    fn register_resolved_tuple_type(&self, ty: &ResolvedType) -> Result<()> {
        match ty {
            ResolvedType::Tuple(elements) => {
                for ty in elements {
                    self.register_resolved_tuple_type(ty)?;
                }
                self.record_tuple_layout(
                    elements
                        .iter()
                        .map(Self::resolved_type_to_ir)
                        .collect::<Result<_>>()?,
                );
            }
            ResolvedType::Pointer(inner)
            | ResolvedType::Reference { inner, .. }
            | ResolvedType::Volatile(inner)
            | ResolvedType::Array { elem: inner, .. }
            | ResolvedType::Vector(inner, _) => self.register_resolved_tuple_type(inner)?,
            ResolvedType::Function { params, ret } => {
                for ty in params {
                    self.register_resolved_tuple_type(ty)?;
                }
                self.register_resolved_tuple_type(ret)?;
            }
            // Nominal layouts are registered from their declarations, not
            // recursively from potentially incomplete recursive snapshots.
            _ => {}
        }
        Ok(())
    }

    pub(super) fn register_resolved_tuple_layouts(&self) -> Result<()> {
        if let Some(facts) = &self.semantic_facts {
            for ty in facts
                .aliases
                .values()
                .chain(facts.global_types.values())
                .chain(facts.expr_types.values())
            {
                self.register_resolved_tuple_type(ty)?;
            }
            for (params, ret) in facts.signatures.values() {
                for ty in params { self.register_resolved_tuple_type(ty)?; }
                self.register_resolved_tuple_type(ret)?;
            }
            for signature in facts.method_signatures.values() {
                for ty in &signature.params {
                    self.register_resolved_tuple_type(ty)?;
                }
                self.register_resolved_tuple_type(&signature.ret)?;
            }
            for variants in facts.enum_variants.values() {
                for (_, fields) in variants {
                    for ty in fields {
                        self.register_resolved_tuple_type(ty)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn finish_tuple_layouts(&mut self) -> Result<()> {
        for (name, elements) in self.tuple_layouts.borrow().iter() {
            if name.ends_with("_collision")
                || self
                    .module
                    .structs
                    .iter()
                    .any(|layout| layout.name == *name)
            {
                return Err(crate::utils::Error::CodeGen(format!(
                    "private tuple layout name collision: {name}"
                )));
            }
            self.module.add_struct(
                name,
                elements
                    .iter()
                    .enumerate()
                    .map(|(i, ty)| (format!("_{i}"), ty.clone()))
                    .collect(),
                crate::middle::ir::StructRepr::Default,
            );
        }
        Ok(())
    }

    pub(super) fn generate_tuple(
        &mut self,
        elements: &[Expr],
        expected: Option<&IRType>,
    ) -> Result<Value> {
        if elements.is_empty() {
            return Ok(Value::Unit);
        }
        let fields = match expected {
            Some(IRType::Struct(name)) => self.tuple_fields(name),
            _ => None,
        };
        if fields
            .as_ref()
            .is_some_and(|fields| fields.len() != elements.len())
        {
            return Err(crate::utils::Error::CodeGen("tuple length mismatch".into()));
        }
        // Evaluate and snapshot each value in source order, including nested
        // aggregates. Later mutations cannot change an earlier element.
        let mut values = Vec::new();
        let mut types = Vec::new();
        for (i, expression) in elements.iter().enumerate() {
            let expected = fields.as_ref().map(|fields| &fields[i].1);
            let value = self.generate_expr_expected(expression, expected)?;
            if self.get_current_terminator().is_some() {
                return Ok(Value::Unit);
            }
            let ty = expected
                .cloned()
                .or_else(|| self.get_value_type(&value))
                .ok_or_else(|| crate::utils::Error::CodeGen("unknown tuple element type".into()))?;
            values.push(value);
            types.push(ty);
        }
        let tuple_ty = self.record_tuple_layout(types.clone());
        let storage = self.allocate_local(&tuple_ty, None);
        self.emit_current(Instruction::ZeroInitialize {
            ptr: storage.clone(),
            ty: tuple_ty.clone(),
        });
        for (index, (value, ty)) in values.into_iter().zip(types).enumerate() {
            let dest = self.alloc_register();
            self.emit_current_with_type(
                Instruction::GetFieldPtr {
                    dest,
                    ptr: storage.clone(),
                    field: index,
                    struct_ty: tuple_ty.clone(),
                },
                IRType::Ptr(Box::new(ty)),
            );
            self.emit_current(Instruction::Store {
                ptr: Value::Register(dest),
                value,
            });
        }
        Ok(self.load_address(storage, tuple_ty))
    }
}
