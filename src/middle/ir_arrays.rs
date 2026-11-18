//! Fixed array indexing and repetition.
use super::*;
impl IRGenerator {
    pub(super) fn checked_array_index(&mut self, index: Value, size: usize) -> Result<Value> {
        if matches!(index, Value::Constant(Constant::Int(n)) if (n as u64) < size as u64) {
            return Ok(index);
        }
        // Widen before comparing, including on 32-bit targets. A negative
        // signed index becomes a large u64, while u8(200) remains 200.
        let index = self.coerce_value(index, &IRType::U64);
        let limit = Value::Constant(Constant::Int(size as i64));
        let cond = self.binary_values(ast::BinOp::Lt, index.clone(), limit)?;
        let valid = self.add_block("index_valid");
        let invalid = self.add_block("index_invalid");
        self.set_terminator_current(Terminator::Branch {
            cond, then_target: valid, else_target: invalid,
        });
        self.current_block = invalid;
        self.set_terminator_current(Terminator::Trap);
        self.current_block = valid;
        Ok(index)
    }

    pub(super) fn generate_array_repeat(
        &mut self,
        expression: &Expr,
        value: &Expr,
        count: &Expr,
        expected: Option<IRType>,
    ) -> Result<Value> {
        let checked = self.checked_expr_type(expression).cloned();
        let (element, size) = match checked {
            Some(ResolvedType::Array { elem, size }) => {
                (Some(Self::resolved_type_to_ir(&elem)?), size)
            }
            _ => match count {
                Expr::Literal(ast::Literal::Int(size, ..)) => (
                    expected.clone(),
                    usize::try_from(*size).map_err(|_| {
                        crate::utils::Error::CodeGen("invalid array repeat count".into())
                    })?,
                ),
                _ => {
                    return Err(crate::utils::Error::CodeGen(
                        "array repeat count was not specialized".into(),
                    ))
                }
            },
        };
        let element = expected.or(element);
        // Even an empty repeat evaluates its operand. An early return/? leaves
        // no following fill instructions, and follows normal scope cleanup.
        let value = self.generate_expr_expected(value, element.as_ref())?;
        if self.get_current_terminator().is_some() {
            return Ok(Value::Unit);
        }
        let element = element
            .or_else(|| self.get_value_type(&value))
            .ok_or_else(|| crate::utils::Error::CodeGen("missing repeat element type".into()))?;
        let value = self.coerce_value(value, &element);
        let array = IRType::Array(Box::new(element.clone()), size);
        let storage = self.allocate_local(&array, None);
        if size > 0 && element != IRType::Unit {
            let integer = Self::pointer_integer(self.pointer_width(), false);
            let zero = self.coerce_value(Value::Constant(Constant::Int(0)), &integer);
            let index_storage = self.allocate_local(&integer, Some(zero));
            let condition = self.add_block("array_repeat_condition");
            let body = self.add_block("array_repeat_body");
            let done = self.add_block("array_repeat_done");
            self.set_terminator_current(Terminator::Jump { target: condition });
            self.current_block = condition;
            let index = self.load_address(index_storage.clone(), integer.clone());
            let limit = self.coerce_value(Value::Constant(Constant::Int(size as i64)), &integer);
            let test = self.alloc_register();
            self.emit_current_with_type(
                Instruction::BinOp {
                    dest: test,
                    op: IRBinOp::Lt,
                    left: index.clone(),
                    right: limit,
                },
                IRType::Bool,
            );
            self.set_terminator_current(Terminator::Branch {
                cond: Value::Register(test),
                then_target: body,
                else_target: done,
            });
            self.current_block = body;
            let address = self.alloc_register();
            self.emit_current_with_type(
                Instruction::ArrayElementPtr {
                    dest: address,
                    ptr: storage.clone(),
                    index: index.clone(),
                    array_ty: array.clone(),
                },
                IRType::Ptr(Box::new(element)),
            );
            self.emit_current(Instruction::Store {
                ptr: Value::Register(address),
                value,
            });
            let one = self.coerce_value(Value::Constant(Constant::Int(1)), &integer);
            let next = self.alloc_register();
            self.emit_current_with_type(
                Instruction::BinOp {
                    dest: next,
                    op: IRBinOp::Add,
                    left: index,
                    right: one,
                },
                integer,
            );
            self.emit_current(Instruction::Store {
                ptr: index_storage,
                value: Value::Register(next),
            });
            self.set_terminator_current(Terminator::Jump { target: condition });
            self.current_block = done;
        }
        Ok(self.load_address(storage, array))
    }
}
