//! Executable, strictly pure function predicates. These are checks, not assumptions.
use super::*;

fn contract_expression(expr: &Expr) -> Result<()> {
    let mut pending = vec![expr];
    while let Some(expr) = pending.pop() {
        match expr {
            Expr::Binary { left, op, right, .. } => {
                if matches!(op, BinOp::Assign | BinOp::AddAssign | BinOp::SubAssign | BinOp::MulAssign | BinOp::DivAssign) {
                    return Err(Error::EffectViolation { message: "contract predicates cannot assign values".into(), span: expr.span() });
                }
                pending.extend([left.as_ref(), right.as_ref()]);
            }
            Expr::Unary { expr, .. } | Expr::Deref { expr, .. } | Expr::Ref { expr, .. }
            | Expr::Cast { expr, .. } | Expr::Field { expr, .. } => pending.push(expr),
            Expr::Call { func, args, .. } => { pending.push(func); pending.extend(args); }
            Expr::MethodCall { expr, args, .. } => { pending.push(expr); pending.extend(args); }
            Expr::Index { expr, index, .. } => pending.extend([expr.as_ref(), index.as_ref()]),
            Expr::Block(block) => contract_block(block, &mut pending)?,
            Expr::If { cond, then_block, else_block, .. } => {
                pending.push(cond); contract_block(then_block, &mut pending)?;
                if let Some(block) = else_block { contract_block(block, &mut pending)?; }
            }
            Expr::Match { expr, arms, .. } => {
                pending.push(expr);
                for arm in arms { pending.push(&arm.body); if let Some(guard) = &arm.guard { pending.push(guard); } }
            }
            Expr::ArrayRepeat {value,count,..} => {pending.push(value);pending.push(count);},
            Expr::Array { elements, .. } | Expr::Tuple { elements, .. } => pending.extend(elements),
            Expr::StructLit { fields, .. } => pending.extend(fields.iter().map(|(_, value)| value)),
            Expr::Range { start, end, .. } => {
                pending.extend(start.iter().map(|e| e.as_ref())); pending.extend(end.iter().map(|e| e.as_ref()));
            }
            Expr::Loop { .. } | Expr::While { .. } | Expr::For { .. } | Expr::Try { .. }
            | Expr::Asm { .. } | Expr::Unsafe { .. } | Expr::Closure { .. } => {
                return Err(Error::semantic_expected("E2104", "contract predicate without loops, early exits, unsafe blocks, assembly or closures", expr.span()));
            }
            Expr::Literal(_) | Expr::Ident(_) | Expr::Path { .. } | Expr::Layout { .. } => {}
        }
    }
    Ok(())
}
fn contract_block<'a>(block: &'a Block, pending: &mut Vec<&'a Expr>) -> Result<()> {
    for statement in &block.stmts {
        match statement {
            Stmt::Expr(expr) | Stmt::Semi(expr) => pending.push(expr),
            Stmt::Empty { .. } => {},
            _ => return Err(Error::semantic_expected("E2104", "contract expression block without local declarations or control transfer", block.span)),
        }
    }
    Ok(())
}

impl SemanticAnalyzer {
    pub(super) fn check_function_contracts(&mut self, function: &Function, return_type: &ResolvedType) -> Result<()> {
        if !function.contracts.is_empty() && function.annotations.iter().any(|a| a.name.name == "volatile") {
            return Err(Error::EffectViolation { message: "volatile functions cannot execute pure contract predicates".into(), span: function.span });
        }
        for contract in &function.contracts {
            if contract.kind == ContractKind::Invariant {
                return Err(Error::semantic_expected("E2104", "requires or ensures on a function; type invariants are not implemented", contract.span));
            }
            contract_expression(&contract.condition)?;
            let ensures = contract.kind == ContractKind::Ensures;
            if ensures {
                self.symbols.enter_scope();
                self.symbols.define(Symbol { name: "result".into(), kind: SymbolKind::Variable,
                    ty: return_type.clone(), span: contract.span, mutable: false })?;
            }
            let checked = self.check_expr(&contract.condition);
            if ensures { self.symbols.exit_scope(); }
            let actual = checked?;
            if actual != ResolvedType::BOOL {
                return Err(Error::TypeMismatch { expected: "bool contract predicate".into(), got: Self::type_identity(&actual), span: contract.span });
            }
        }
        Ok(())
    }

    pub(super) fn check_contract_effects(&self, program: &Program) -> Result<()> {
        for item in &program.items {
            let functions = match item {
                Item::Function(function) => vec![(function.name.name.clone(), function)],
                Item::Impl(block) => block.methods.iter().map(|method| {
                    let symbol = format!("{}::{}", block.target.name, method.name.name);
                    (self.facts.method_signatures[&symbol].emitted_name.clone(), method)
                }).collect(),
                _ => continue,
            };
            for (name, function) in functions {
                for contract in &function.contracts {
                    let effects = self.contract_expression_effects(&name, &contract.condition);
                    if !effects.is_empty() {
                        return Err(Error::EffectViolation { message: format!("contract predicate must be pure; requires {:?}", effects), span: contract.span });
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::{lexer::Lexer, parser::Parser};
    fn analyze(source: &str) -> Result<()> {
        let program = Parser::new(Lexer::new(source, 0)).parse_program()?;
        SemanticAnalyzer::new().analyze(&program)
    }
    #[test]
    fn predicates_have_lexical_result_and_strict_pure_types() {
        for source in [
            "fn valid(n:i32)->bool pure{n>0} fn f(n:i32)->i32 [requires valid(n),ensures result>n]{n+1}",
            "struct S{x:i32}fn f(s:S)->i32 [requires s.x>0,ensures result==s.x]{s.x}",
            "fn unit(x:())->bool pure{true}fn f()[ensures unit(result)]{}",
            "fn f(result:i32)->i32[requires result>0,ensures result==7]{7}",
        ] { analyze(source).unwrap_or_else(|e| panic!("{source}: {e:?}")); }
        for source in [
            "fn f()->i32[requires 1]{1}",
            "fn f()->i32[requires result==1]{1}",
            "fn f()->i32[ensures result==1]{result}",
            "fn f(p:*i32)->i32[requires *p>0]{1}",
            "fn f(p:*volatile i32)->i32[requires *p>0]{1}",
            "fn bad()->bool{puts(\"bad\");true}fn f()[requires bad()]{}",
            "fn f(n:mut i32)[requires {n=2;true}]{}",
            "fn f()[requires {return;true}]{}",
            "fn f()[requires {let n:i32=1;true}]{}",
            "fn f()[invariant true]{}",
        ] { assert!(analyze(source).is_err(), "accepted {source}"); }
    }
}
