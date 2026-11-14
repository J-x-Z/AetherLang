//! Lower memory operations from the same inventory used by semantic checking.
use super::*;
use crate::intrinsics::{self, MemoryKind, MemoryOrder};
use crate::utils::{Error, Span};

impl IRGenerator {
    pub(super) fn generate_memory_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<Value> {
        let spec = intrinsics::memory(name)
            .ok_or_else(|| Error::CodeGen(format!("unknown memory operation {name}")))?;
        let params = spec.params();
        if args.len() != params.len() {
            return Err(Error::CodeGen(format!(
                "incorrect memory operation arity for {name}"
            )));
        }
        let (order, failure_order) = if spec.explicit_order {
            self.semantic_facts
                .as_ref()
                .and_then(|f| f.memory_orders.get(&span))
                .copied()
                .ok_or_else(|| {
                    Error::CodeGen("memory order requires validated constant facts".into())
                })?
        } else {
            (MemoryOrder::SeqCst, MemoryOrder::SeqCst)
        };
        intrinsics::validate_orders(spec.kind, order, failure_order).map_err(Error::CodeGen)?;
        let value_count = args.len()
            - if spec.explicit_order {
                if spec.kind == MemoryKind::CompareExchange {
                    2
                } else {
                    1
                }
            } else {
                0
            };
        let mut values = Vec::new();
        for (arg, param) in args[..value_count].iter().zip(&params) {
            let ty = Self::resolved_type_to_ir(param)?;
            let value = self.generate_expr_expected(arg, Some(&ty))?;
            if self.get_current_terminator().is_some() {
                return Ok(Value::Unit);
            }
            values.push(self.coerce_value(value, &ty));
        }
        let ty = Self::resolved_type_to_ir(&ResolvedType::Primitive(spec.element.clone()))?;
        let result = Self::resolved_type_to_ir(&spec.result())?;
        let dest = if matches!(result, IRType::Unit | IRType::Void) {
            None
        } else {
            Some(self.alloc_register())
        };
        self.emit_current_with_type(
            Instruction::Memory {
                dest,
                kind: spec.kind,
                ty,
                args: values,
                order,
                failure_order,
            },
            result,
        );
        Ok(dest.map(Value::Register).unwrap_or(Value::Unit))
    }
}
