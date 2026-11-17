//! Lower already-resolved method calls; no name-based intrinsic guessing.
use super::*;
use crate::frontend::semantic::ResolvedMethodSignature;
use crate::utils::{Error, Span};

impl IRGenerator {
    pub(super) fn resolved_method_signature(&self, target: &str, method: &str) -> Result<ResolvedMethodSignature> {
        let symbol = format!("{}::{}", target, method);
        self.semantic_facts.as_ref().and_then(|facts| facts.method_signatures.get(&symbol)).cloned()
            .ok_or_else(|| Error::CodeGen(format!("method '{}' has no resolved semantic signature", symbol)))
    }

    pub(super) fn generate_method_call(&mut self, receiver: &Expr, args: &[Expr], span: Span) -> Result<Value> {
        let call = self.semantic_facts.as_ref().and_then(|facts| {
            self.current_fn.as_ref().and_then(|function| facts.method_calls.get(&(function.name.clone(), span)))
        }).cloned().ok_or_else(|| Error::CodeGen("method call has no resolved semantic callee".into()))?;
        match call {
            ResolvedMethodCall::DivergingReceiver => {
                self.generate_expr(receiver)?;
                if self.get_current_terminator().is_none() { return Err(Error::CodeGen("diverging method receiver did not terminate".into())); }
                Ok(Value::Unit)
            }
            ResolvedMethodCall::PointerAdd { element } => {
                let ptr = self.generate_expr(receiver)?;
                if self.get_current_terminator().is_some() { return Ok(Value::Unit); }
                if args.len() != 1 { return Err(Error::CodeGen("resolved pointer.add arity changed".into())); }
                let index = self.generate_expr(&args[0])?;
                if self.get_current_terminator().is_some() { return Ok(Value::Unit); }
                let elem_ty = Self::resolved_type_to_ir(&element)?;
                let result_ty = IRType::Ptr(Box::new(elem_ty.clone()));
                if self.get_value_type(&ptr) != Some(result_ty.clone()) {
                    return Err(Error::CodeGen("resolved pointer.add receiver type changed".into()));
                }
                let dest = self.alloc_register();
                self.emit_current_with_type(Instruction::GetElementPtr { dest, ptr, index, elem_ty }, result_ty);
                Ok(Value::Register(dest))
            }
            ResolvedMethodCall::Function { symbol, receiver: adjustment } => {
                let signature = self.semantic_facts.as_ref().and_then(|facts| facts.method_signatures.get(&symbol)).cloned()
                    .ok_or_else(|| Error::CodeGen(format!("resolved method '{}' has no signature", symbol)))?;
                let params = self.function_parameters.get(&signature.emitted_name).cloned()
                    .ok_or_else(|| Error::CodeGen(format!("resolved method '{}' was not declared", symbol)))?;
                let ret_type = self.function_signatures.get(&signature.emitted_name).cloned()
                    .ok_or_else(|| Error::CodeGen(format!("resolved method '{}' has no return type", symbol)))?;
                if params.len() != args.len() + 1 { return Err(Error::CodeGen(format!("resolved method '{}' arity changed", symbol))); }
                // Capture the receiver before evaluating arguments, including when
                // an argument mutates the variable containing a pointer receiver.
                let value = match adjustment {
                    ReceiverAdjustment::Value => self.generate_expr(receiver)?,
                    ReceiverAdjustment::Borrow { .. } => self.address_or_temporary(receiver)?.0,
                };
                if self.get_current_terminator().is_some() { return Ok(Value::Unit); }
                if self.get_value_type(&value) != params.first().cloned() {
                    return Err(Error::CodeGen(format!("resolved method '{}' receiver representation changed", symbol)));
                }
                let mut values = vec![value];
                for (arg, expected) in args.iter().zip(&params[1..]) {
                    let value = self.generate_expr_expected(arg, Some(expected))?;
                    if self.get_current_terminator().is_some() { return Ok(Value::Unit); }
                    values.push(self.coerce_value(value, expected));
                }
                let dest = if ret_type == IRType::Void { None } else { Some(self.alloc_register()) };
                self.emit_current_with_type(Instruction::Call { dest, func: signature.emitted_name, args: values }, ret_type);
                Ok(dest.map(Value::Register).unwrap_or(Value::Unit))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::{lexer::Lexer, parser::Parser, semantic::SemanticAnalyzer};

    #[test]
    fn canonical_method_signatures_and_callees_reach_ir() {
        let source = "struct S{x:i32} impl S{fn add(self:&Self,n:i32)->i32{self.x+n} fn view(self:&Self)->&Self{self}} fn f(s:&S,p:*i32)->i32{let q:*i32=p.add(1);s.add(2)}";
        let program = Parser::new(Lexer::new(source, 0)).parse_program().unwrap();
        let mut analyzer = SemanticAnalyzer::new();
        analyzer.analyze(&program).unwrap();
        let module = IRGenerator::new("methods").with_semantic_facts(analyzer.facts().clone()).generate(&program).unwrap();
        let expected = IRType::Ptr(Box::new(IRType::Struct("S".into())));
        let method = module.functions.iter().find(|function| function.name == "S_view").unwrap();
        assert_eq!(method.params[0].1, expected);
        assert_eq!(method.ret_type, expected);
        let caller = module.functions.iter().find(|function| function.name == "f").unwrap();
        assert!(caller.blocks.iter().flat_map(|block| &block.instructions).any(|instruction| matches!(instruction, Instruction::GetElementPtr {elem_ty:IRType::I32,..})));
        assert!(caller.blocks.iter().flat_map(|block| &block.instructions).any(|instruction| matches!(instruction, Instruction::Call {func,..} if func=="S_add")));
        let error = IRGenerator::new("unchecked").generate(&program).unwrap_err();
        assert!(format!("{error}").contains("resolved semantic signature"));
    }
}
