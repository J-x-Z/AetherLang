//! Lower vector builtins through checked signatures, never target function names.
use super::*;
impl IRGenerator {
    pub(super) fn generate_vector_call(&mut self, name: &str, args: &[Expr]) -> Result<Value> {
        let spec = crate::intrinsics::vector(name).unwrap();
        let mut values = vec![];
        for (arg, param) in args.iter().zip(spec.params(self.pointer_width())) {
            let ty = Self::resolved_type_to_ir(&param)?;
            let value = self.generate_expr_expected(arg, Some(&ty))?;
            if self.get_current_terminator().is_some() {
                return Ok(Value::Unit);
            }
            values.push(self.coerce_value(value, &ty));
        }
        let ty = Self::resolved_type_to_ir(&spec.vector())?;
        let result = Self::resolved_type_to_ir(&spec.result())?;
        let dest = if result == IRType::Unit {
            None
        } else {
            Some(self.alloc_register())
        };
        self.emit_current_with_type(
            Instruction::Vector {
                dest,
                kind: spec.kind,
                ty,
                args: values,
            },
            result,
        );
        Ok(dest.map(Value::Register).unwrap_or(Value::Unit))
    }
}
