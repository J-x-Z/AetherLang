//! Resolved method identities and signatures shared by effects, flow and IR.
use super::*;

#[derive(Debug, Clone)]
pub struct ResolvedMethodSignature {
    pub emitted_name: String,
    pub target: ResolvedType,
    pub params: Vec<ResolvedType>,
    pub ret: ResolvedType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverAdjustment {
    Value,
    Borrow { mutable: bool },
}

#[derive(Debug, Clone)]
pub enum ResolvedMethodCall {
    PointerAdd { element: ResolvedType },
    Function { symbol: String, receiver: ReceiverAdjustment },
    DivergingReceiver,
}

impl SemanticAnalyzer {
    pub(super) fn collect_methods(&mut self, program: &Program) -> Result<()> {
        for item in &program.items {
            let Item::Impl(block) = item else {
                continue;
            };
            let target =
                self.resolve_type(&Type::Named(block.target.name.clone(), block.target.span))?;
            for method in &block.methods {
                self.symbols.enter_scope();
                self.symbols.define(Symbol {
                    name: "Self".into(),
                    kind: SymbolKind::TypeAlias {
                        target: target.clone(),
                    },
                    ty: target.clone(),
                    span: block.span,
                    mutable: false,
                })?;
                self.define_type_parameters(&method.type_params)?;
                let params = method
                    .params
                    .iter()
                    .map(|p| self.resolve_type(&p.ty))
                    .collect::<Result<Vec<_>>>()?;
                let ret = method
                    .ret_type
                    .as_ref()
                    .map(|ty| self.resolve_type(ty))
                    .transpose()?
                    .unwrap_or(ResolvedType::UNIT);
                self.symbols.exit_scope();
                let symbol = format!("{}::{}", block.target.name, method.name.name);
                let emitted_name = format!("{}_{}", block.target.name, method.name.name);
                if self.symbols.lookup(&emitted_name).is_some() || self.facts.method_signatures.values().any(|signature| signature.emitted_name == emitted_name) {
                    return Err(Error::DuplicateDefinition { name: emitted_name, span: method.span });
                }
                self.facts.method_signatures.insert(symbol.clone(), ResolvedMethodSignature {
                    emitted_name, target: target.clone(), params: params.clone(), ret: ret.clone(),
                });
                self.symbols.define(Symbol {
                    name: symbol,
                    kind: SymbolKind::Function {
                        params: params.clone(),
                        ret: ret.clone(),
                        type_params: method.type_params.iter().map(|p| p.name.clone()).collect(),
                        const_params: vec![],
                        effects: method.effects.clone(),
                    },
                    ty: ResolvedType::Function {
                        params,
                        ret: Box::new(ret),
                    },
                    span: method.span,
                    mutable: false,
                })?;
            }
        }
        Ok(())
    }

    fn remember_method_call(&mut self, span: Span, call: ResolvedMethodCall) {
        if let Some(function) = &self.current_function_name {
            self.facts.method_calls.insert((function.clone(), span), call);
        }
    }

    pub(super) fn check_method_call(&mut self, receiver: &Expr, method: &Ident, args: &[Expr], span: Span) -> Result<ResolvedType> {
        let receiver_ty = self.check_expr(receiver)?;
        if receiver_ty == ResolvedType::NEVER {
            for arg in args { self.check_expr(arg)?; }
            self.remember_method_call(span, ResolvedMethodCall::DivergingReceiver);
            return Ok(ResolvedType::NEVER);
        }
        if let ResolvedType::Pointer(inner) = &receiver_ty {
            if method.name == "add" {
                if args.len() != 1 {
                    return Err(Error::ArgCountMismatch { func_name: "ptr.add".into(), expected: 1, got: args.len(), span });
                }
                let offset = self.check_expr(&args[0])?;
                if offset != ResolvedType::NEVER && !Self::is_integer(&offset) {
                    return Err(Error::TypeMismatch { expected: "integer".into(), got: format!("{:?}", offset), span: args[0].span() });
                }
                self.remember_method_call(span, ResolvedMethodCall::PointerAdd { element: (**inner).clone() });
                return Ok(if offset == ResolvedType::NEVER { offset } else { receiver_ty });
            }
        }
        let base = match &receiver_ty {
            ResolvedType::Reference { inner, .. } | ResolvedType::Pointer(inner) => inner.as_ref(),
            other => other,
        };
        let symbol = match base {
            ResolvedType::Struct { name, .. } | ResolvedType::Enum { name } => format!("{}::{}", name, method.name),
            _ => return Err(Error::UndefinedVariable { name: method.name.clone(), span }),
        };
        let signature = self.facts.method_signatures.get(&symbol).cloned()
            .ok_or_else(|| Error::UndefinedVariable { name: symbol.clone(), span })?;
        if signature.params.is_empty() || signature.params.len() != args.len() + 1 {
            return Err(Error::ArgCountMismatch { func_name: symbol, expected: signature.params.len().saturating_sub(1), got: args.len(), span });
        }
        let adjustment = if let ResolvedType::Reference { mutable, inner } = &signature.params[0] {
            if !matches!(receiver_ty, ResolvedType::Reference { .. }) {
                self.require_type(inner, &receiver_ty, span)?;
                if *mutable { self.check_assignable(receiver)?; }
                ReceiverAdjustment::Borrow { mutable: *mutable }
            } else {
                self.require_type(&signature.params[0], &receiver_ty, span)?;
                ReceiverAdjustment::Value
            }
        } else {
            self.require_type(&signature.params[0], &receiver_ty, span)?;
            ReceiverAdjustment::Value
        };
        let mut diverges = false;
        for (arg, ty) in args.iter().zip(&signature.params[1..]) {
            diverges |= self.check_expected_expr(arg, ty)? == ResolvedType::NEVER;
        }
        self.remember_method_call(span, ResolvedMethodCall::Function { symbol, receiver: adjustment });
        Ok(if diverges { ResolvedType::NEVER } else { signature.ret })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::{lexer::Lexer, parser::Parser};

    fn analyze(source: &str) -> Result<SemanticFacts> {
        let program = Parser::new(Lexer::new(source, 0)).parse_program()?;
        let mut analyzer = SemanticAnalyzer::new();
        analyzer.analyze(&program)?;
        Ok(analyzer.facts)
    }

    #[test]
    fn self_signatures_and_add_calls_are_resolved_once() {
        let facts = analyze("struct S{x:i32} impl S {fn add(self:&Self,n:i32)->i32{self.x+n} fn view(self:&Self)->&Self{self}} fn f(s:&S,p:*i32)->i32 {let q:*i32=p.add(1);s.add(2)}").unwrap();
        let signature = &facts.method_signatures["S::view"];
        assert_eq!(signature.emitted_name, "S_view");
        assert_eq!(SemanticAnalyzer::type_identity(&signature.params[0]), "&falsestruct S");
        assert_eq!(signature.params[0], signature.ret);
        assert!(facts.method_calls.values().any(|call| matches!(call, ResolvedMethodCall::PointerAdd { element } if *element == ResolvedType::I32)));
        assert!(facts.method_calls.values().any(|call| matches!(call, ResolvedMethodCall::Function { symbol, receiver:ReceiverAdjustment::Value } if symbol == "S::add")));
        let error = analyze("struct S{x:i32} impl S{fn bump(self:&mut Self){self.x+=1;}} fn f(s:&S){s.bump();}").unwrap_err();
        assert!(matches!(error, Error::TypeMismatch {..}), "{error:?}");
    }

    #[test]
    fn effects_follow_the_resolved_receiver_including_user_add() {
        let source = "struct A{x:i32} struct B{x:i32} impl A{fn run(self:&Self)->i32{1}} impl B{fn run(self:&Self)->i32{puts(\"effect\");2}} fn f(a:&A)->i32 pure{a.run()}";
        analyze(source).unwrap();
        let error = analyze("struct A{x:i32} impl A{fn add(self:&Self,n:i32)->i32{puts(\"effect\");n}} fn f(a:&A)->i32 pure{a.add(2)}").unwrap_err();
        assert!(matches!(error, Error::EffectViolation {..}), "{error:?}");
        analyze("fn f(p:*i32)->*i32 pure{p.add(1)}").unwrap();
    }

    #[test]
    fn receiver_adjustment_and_method_symbol_collisions_are_checked() {
        let facts = analyze("struct S{x:i32} impl S{fn bump(self:&mut Self){self.x+=1;}} fn f(){let mut s:S=S{x:1};s.bump();}").unwrap();
        assert!(facts.method_calls.values().any(|call| matches!(call, ResolvedMethodCall::Function { receiver:ReceiverAdjustment::Borrow {mutable:true}, .. })));
        assert!(analyze("struct S{x:i32} impl S{fn x(self:&Self)->i32{1}} fn S_x()->i32{2}").is_err());
        assert!(analyze("struct S{x:i32} impl S{fn add(self:&Self,n:i32)->i32{n}} fn f(s:&S){s.add(true);}").is_err());
    }
}
