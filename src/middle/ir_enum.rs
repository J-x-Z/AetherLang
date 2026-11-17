//! Tagged enum lowering over the common by-value aggregate and union layout.
//! Only the active payload is constructed/read; zeroing inactive storage does
//! not create language-level references or execute an inactive constructor.
use super::*;
use crate::middle::ir::{IREnum, IRStruct, IRVariant, StructRepr};
use crate::utils::Error;

impl IRGenerator {
    fn enum_payload_name(name: &str) -> String {
        format!("__aeth_enum_{}_payload", name)
    }
    fn enum_variant_name(name: &str, index: usize) -> String {
        format!("__aeth_enum_{}_variant_{}", name, index)
    }
    fn enum_definition(&self, ty: &IRType) -> Option<IREnum> {
        let IRType::Struct(name) = ty else {
            return None;
        };
        self.module.enums.iter().find(|e| &e.name == name).cloned()
    }
    pub(super) fn register_enum(&mut self, definition: &ast::EnumDef) -> Result<()> {
        if !definition.generic_params.is_empty() || !definition.type_params.is_empty() {
            return Err(Error::CodeGen(
                "enum template reached lowering without specialization".into(),
            ));
        }
        let variants: Vec<_> = if let Some(facts) = self.semantic_facts.as_ref() {
            let variants = facts
                .enum_variants
                .get(&definition.name.name)
                .ok_or_else(|| {
                    Error::CodeGen(format!(
                        "missing checked enum layout: {}",
                        definition.name.name
                    ))
                })?;
            variants
                .iter()
                .map(|(name, fields)| {
                    Ok(IRVariant {
                        name: name.clone(),
                        fields: fields
                            .iter()
                            .map(Self::resolved_type_to_ir)
                            .collect::<Result<_>>()?,
                    })
                })
                .collect::<Result<_>>()?
        } else {
            // Direct IR unit tests may omit the checking session.
            definition
                .variants
                .iter()
                .map(|variant| Ok(IRVariant {
                    name: variant.name.name.clone(),
                    fields: variant
                        .fields
                        .iter()
                        .map(|ty| self.ast_type_to_ir(ty))
                        .collect::<Result<_>>()?,
                }))
                .collect::<Result<_>>()?
        };
        let name = &definition.name.name;
        let mut payload_fields = vec![];
        for (index, variant) in variants.iter().enumerate() {
            let variant_name = Self::enum_variant_name(name, index);
            let fields = if variant.fields.is_empty() {
                vec![("reserved".into(), IRType::U8)]
            } else {
                variant
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(i, t)| (format!("value_{}", i), t.clone()))
                    .collect()
            };
            self.struct_defs
                .insert(variant_name.clone(), fields.clone());
            self.module
                .add_struct(&variant_name, fields, StructRepr::Default);
            payload_fields.push((format!("variant_{}", index), IRType::Struct(variant_name)));
        }
        let payload = Self::enum_payload_name(name);
        self.struct_defs
            .insert(payload.clone(), payload_fields.clone());
        self.module.structs.push(IRStruct {
            name: payload.clone(),
            fields: payload_fields,
            repr: StructRepr::Default,
            is_union: true,
        });
        let fields = vec![
            ("tag".into(), IRType::U32),
            ("payload".into(), IRType::Struct(payload)),
        ];
        self.struct_defs.insert(name.clone(), fields.clone());
        self.module.add_struct(name, fields, StructRepr::Default);
        self.module.enums.push(IREnum {
            name: name.clone(),
            variants,
        });
        Ok(())
    }
    pub(super) fn enum_variant_path(&self, segments: &[ast::Ident]) -> Option<(IREnum, usize)> {
        if segments.len() != 2 {
            return None;
        }
        let definition = self
            .module
            .enums
            .iter()
            .find(|e| e.name == segments[0].name)?;
        let variant = definition
            .variants
            .iter()
            .position(|v| v.name == segments[1].name)?;
        Some((definition.clone(), variant))
    }
    fn aggregate_field(
        &mut self,
        base: Value,
        ty: &IRType,
        index: usize,
    ) -> Result<(Value, IRType)> {
        let IRType::Struct(name) = ty else {
            return Err(Error::CodeGen("field address needs aggregate type".into()));
        };
        let field_ty = self
            .struct_defs
            .get(name)
            .and_then(|fields| fields.get(index))
            .map(|(_, t)| t.clone())
            .ok_or_else(|| Error::CodeGen(format!("missing aggregate field {}:{}", name, index)))?;
        let dest = self.alloc_register();
        self.emit_current_with_type(
            Instruction::GetFieldPtr {
                dest,
                ptr: base,
                field: index,
                struct_ty: ty.clone(),
            },
            IRType::Ptr(Box::new(field_ty.clone())),
        );
        Ok((Value::Register(dest), field_ty))
    }
    fn enum_payload_place(
        &mut self,
        base: Value,
        definition: &IREnum,
        variant: usize,
    ) -> Result<(Value, IRType)> {
        let (payload, ty) =
            self.aggregate_field(base, &IRType::Struct(definition.name.clone()), 1)?;
        self.aggregate_field(payload, &ty, variant)
    }
    fn construct_enum_values(
        &mut self,
        definition: &IREnum,
        variant: usize,
        values: Vec<Value>,
    ) -> Result<Value> {
        let ty = IRType::Struct(definition.name.clone());
        let storage = self.allocate_local(&ty, None);
        self.emit_current(Instruction::ZeroInitialize {
            ptr: storage.clone(),
            ty: ty.clone(),
        });
        let (tag, tag_ty) = self.aggregate_field(storage.clone(), &ty, 0)?;
        let value = self.coerce_value(Value::Constant(Constant::Int(variant as i64)), &tag_ty);
        self.emit_current(Instruction::Store { ptr: tag, value });
        if !values.is_empty() {
            let (payload, payload_ty) =
                self.enum_payload_place(storage.clone(), definition, variant)?;
            for (index, value) in values.into_iter().enumerate() {
                let (field, field_ty) =
                    self.aggregate_field(payload.clone(), &payload_ty, index)?;
                let value = self.coerce_value(value, &field_ty);
                self.emit_current(Instruction::Store { ptr: field, value });
            }
        }
        Ok(self.load_address(storage, ty))
    }
    pub(super) fn construct_enum(
        &mut self,
        definition: &IREnum,
        variant: usize,
        args: &[Expr],
    ) -> Result<Value> {
        let fields = &definition.variants[variant].fields;
        if fields.len() != args.len() {
            return Err(Error::CodeGen(format!(
                "{}::{} expects {} payload values",
                definition.name,
                definition.variants[variant].name,
                fields.len()
            )));
        }
        let mut values = vec![];
        for (arg, ty) in args.iter().zip(fields) {
            values.push(self.generate_expr_expected(arg, Some(ty))?);
            if self.get_current_terminator().is_some() {
                return Ok(Value::Unit);
            }
        }
        self.construct_enum_values(definition, variant, values)
    }
    fn pattern_branch(
        &mut self,
        pattern: &ast::Pattern,
        value: Value,
        ty: &IRType,
        matched: BlockId,
        miss: BlockId,
    ) -> Result<()> {
        match pattern {
            ast::Pattern::Wildcard { .. } => {
                self.set_terminator_current(Terminator::Jump { target: matched })
            }
            ast::Pattern::Binding { name, .. } => {
                let address = self.allocate_local(ty, Some(value));
                self.locals
                    .insert(name.name.clone(), (address.clone(), ty.clone()));
                self.local_addresses.insert(name.name.clone(), address);
                self.set_terminator_current(Terminator::Jump { target: matched });
            }
            ast::Pattern::Literal(literal) => {
                let literal = self.generate_literal(literal);
                let literal = self.coerce_value(literal, ty);
                let dest = self.alloc_register();
                self.emit_current_with_type(
                    Instruction::BinOp {
                        dest,
                        op: IRBinOp::Eq,
                        left: value,
                        right: literal,
                    },
                    IRType::Bool,
                );
                self.set_terminator_current(Terminator::Branch {
                    cond: Value::Register(dest),
                    then_target: matched,
                    else_target: miss,
                });
            }
            ast::Pattern::Variant {
                variant, fields, ..
            } => {
                let definition = self
                    .enum_definition(ty)
                    .ok_or_else(|| Error::CodeGen("variant pattern needs enum value".into()))?;
                let index = definition
                    .variants
                    .iter()
                    .position(|v| v.name == variant.name)
                    .ok_or_else(|| Error::CodeGen("unknown enum pattern variant".into()))?;
                if definition.variants[index].fields.len() != fields.len() {
                    return Err(Error::CodeGen("enum pattern payload arity".into()));
                }
                let storage = self.allocate_local(ty, Some(value));
                let (tag, tag_ty) = self.aggregate_field(storage.clone(), ty, 0)?;
                let tag = self.load_address(tag, tag_ty.clone());
                let expected =
                    self.coerce_value(Value::Constant(Constant::Int(index as i64)), &tag_ty);
                let test = self.alloc_register();
                self.emit_current_with_type(
                    Instruction::BinOp {
                        dest: test,
                        op: IRBinOp::Eq,
                        left: tag,
                        right: expected,
                    },
                    IRType::Bool,
                );
                let payload_block = if fields.is_empty() {
                    matched
                } else {
                    self.add_block("enum_payload")
                };
                self.set_terminator_current(Terminator::Branch {
                    cond: Value::Register(test),
                    then_target: payload_block,
                    else_target: miss,
                });
                if !fields.is_empty() {
                    self.current_block = payload_block;
                    let (payload, payload_ty) =
                        self.enum_payload_place(storage, &definition, index)?;
                    for (i, pattern) in fields.iter().enumerate() {
                        let (field, field_ty) =
                            self.aggregate_field(payload.clone(), &payload_ty, i)?;
                        let value = self.load_address(field, field_ty.clone());
                        let next = if i + 1 == fields.len() {
                            matched
                        } else {
                            self.add_block("enum_pattern_field")
                        };
                        self.pattern_branch(pattern, value, &field_ty, next, miss)?;
                        if i + 1 < fields.len() {
                            self.current_block = next;
                        }
                    }
                }
            }
            ast::Pattern::Struct { fields, .. } => {
                let IRType::Struct(name) = ty else {
                    return Err(Error::CodeGen("struct pattern requires struct".into()));
                };
                let definition = self
                    .struct_defs
                    .get(name)
                    .cloned()
                    .ok_or_else(|| Error::CodeGen("missing struct pattern layout".into()))?;
                let storage = self.allocate_local(ty, Some(value));
                if fields.is_empty() {
                    self.set_terminator_current(Terminator::Jump { target: matched });
                }
                for (i, (name, pattern)) in fields.iter().enumerate() {
                    let index = definition
                        .iter()
                        .position(|(n, _)| n == &name.name)
                        .ok_or_else(|| Error::CodeGen("unknown struct pattern field".into()))?;
                    let (field, field_ty) = self.aggregate_field(storage.clone(), ty, index)?;
                    let value = self.load_address(field, field_ty.clone());
                    let next = if i + 1 == fields.len() {
                        matched
                    } else {
                        self.add_block("struct_pattern_field")
                    };
                    self.pattern_branch(pattern, value, &field_ty, next, miss)?;
                    if i + 1 < fields.len() {
                        self.current_block = next;
                    }
                }
            }
            _ => return Err(Error::CodeGen("unsupported pattern representation".into())),
        }
        Ok(())
    }
    pub(super) fn generate_match_value(
        &mut self,
        expr: &Expr,
        arms: &[ast::MatchArm],
    ) -> Result<Value> {
        let temporary_start=self.owned_checkpoint();
        let value = self.generate_expr(expr)?;
        self.cleanup_owned_temporaries(temporary_start,self.owned.depth,None)?;
        if self.get_current_terminator().is_some() {
            return Ok(Value::Unit);
        }
        let ty = self
            .get_value_type(&value)
            .ok_or_else(|| Error::CodeGen("match value has no type".into()))?;
        let exit = self.add_block("match_exit");
        let miss = self.add_block("match_unmatched");
        let mut incoming = vec![];
        let mut result_ty = None;
        let mut reaches_exit = false;
        for (i, arm) in arms.iter().enumerate() {
            let next = if i + 1 < arms.len() {
                self.add_block("match_next")
            } else {
                miss
            };
            let body = self.add_block("match_body");
            let guard = if arm.guard.is_some() {
                self.add_block("match_guard")
            } else {
                body
            };
            let locals = self.locals.clone();
            let addresses = self.local_addresses.clone();
            self.pattern_branch(&arm.pattern, value.clone(), &ty, guard, next)?;
            if let Some(condition) = &arm.guard {
                self.current_block = guard;
                let temporary_start=self.owned_checkpoint();
                let condition = self.generate_expr(condition)?;
                self.cleanup_owned_temporaries(temporary_start,self.owned.depth,None)?;
                self.set_terminator_current(Terminator::Branch {
                    cond: condition,
                    then_target: body,
                    else_target: next,
                });
            }
            self.current_block = body;
            let result = self.generate_expr(&arm.body)?;
            if self.get_current_terminator().is_none() {
                reaches_exit = true;
                self.transfer_owned_value(&result);
                let arm_type = self.require_value_type(&result)?;
                if !matches!(arm_type, IRType::Unit | IRType::Void) {
                    let ty = result_ty.get_or_insert(arm_type).clone();
                    let result = self.coerce_value(result, &ty);
                    incoming.push((result, self.current_block));
                }
                self.set_terminator_current(Terminator::Jump { target: exit });
            }
            self.locals = locals;
            self.local_addresses = addresses;
            self.current_block = next;
        }
        self.current_block = miss;
        self.set_terminator_current(Terminator::Unreachable);
        self.current_block = exit;
        if !reaches_exit {
            self.set_terminator_current(Terminator::Unreachable);
        }
        if let Some(ty) = result_ty {
            let dest = self.alloc_register();
            self.emit_current_with_type(Instruction::Phi { dest, incoming }, ty);
            Ok(Value::Register(dest))
        } else {
            Ok(Value::Unit)
        }
    }
    pub(super) fn generate_try_value(&mut self, expr: &Expr) -> Result<Value> {
        let value = self.generate_expr(expr)?;
        if self.get_current_terminator().is_some() {
            return Ok(Value::Unit);
        }
        let ty = self
            .get_value_type(&value)
            .ok_or_else(|| Error::CodeGen("? operand has no type".into()))?;
        let definition = self.enum_definition(&ty).ok_or_else(|| {
            Error::CodeGen("error propagation with '?' requires Result enum".into())
        })?;
        let returned_ty = self.current_fn.as_ref().unwrap().ret_type.clone();
        let returned = self.enum_definition(&returned_ty).ok_or_else(|| {
            Error::CodeGen("error propagation with '?' requires a Result return type".into())
        })?;
        let index = |d: &IREnum, name: &str| {
            d.variants
                .iter()
                .position(|v| v.name == name && v.fields.len() == 1)
                .ok_or_else(|| Error::CodeGen("Result requires Ok(T) and Err(E)".into()))
        };
        let ok = index(&definition, "Ok")?;
        let err = index(&definition, "Err")?;
        let returned_err = index(&returned, "Err")?;
        if definition.variants[err].fields != returned.variants[returned_err].fields {
            return Err(Error::CodeGen("? error payload types differ".into()));
        }
        let storage = self.allocate_local(&ty, Some(value));
        let (tag, tag_ty) = self.aggregate_field(storage.clone(), &ty, 0)?;
        let tag = self.load_address(tag, tag_ty.clone());
        let expected = self.coerce_value(Value::Constant(Constant::Int(ok as i64)), &tag_ty);
        let test = self.alloc_register();
        self.emit_current_with_type(
            Instruction::BinOp {
                dest: test,
                op: IRBinOp::Eq,
                left: tag,
                right: expected,
            },
            IRType::Bool,
        );
        let success = self.add_block("try_ok");
        let failure = self.add_block("try_err");
        self.set_terminator_current(Terminator::Branch {
            cond: Value::Register(test),
            then_target: success,
            else_target: failure,
        });
        self.current_block = failure;
        let (payload, payload_ty) = self.enum_payload_place(storage.clone(), &definition, err)?;
        let (field, field_ty) = self.aggregate_field(payload, &payload_ty, 0)?;
        let value = self.load_address(field, field_ty);
        let value = self.construct_enum_values(&returned, returned_err, vec![value])?;
        self.emit_checked_return(Some(value))?;
        self.current_block = success;
        let (payload, payload_ty) = self.enum_payload_place(storage, &definition, ok)?;
        let (field, field_ty) = self.aggregate_field(payload, &payload_ty, 0)?;
        Ok(self.load_address(field, field_ty))
    }
}
