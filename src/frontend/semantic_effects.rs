//! Whole-call-graph effect inference. Explicit annotations are upper bounds.
use super::*;
const ALL: [Effect; 5] = [
    Effect::Read,
    Effect::Write,
    Effect::IO,
    Effect::Alloc,
    Effect::Panic,
];
fn extend(target: &mut Vec<Effect>, source: &[Effect]) {
    for effect in source {
        if !target.contains(effect) {
            target.push(*effect);
        }
    }
}

impl SemanticAnalyzer {
    pub(super) fn foreign_effects(&self, name: &str, annotations: &[Annotation]) -> EffectSet {
        let known = self.symbols.lookup(name).and_then(|symbol| {
            if let SymbolKind::Function { effects, .. } = &symbol.kind {
                Some(effects.clone())
            } else {
                None
            }
        });
        let mut explicit = false;
        let mut effects = vec![];
        for annotation in annotations {
            match annotation.name.name.as_str() {
                "pure" => explicit = true,
                "read" | "reads" => {
                    explicit = true;
                    extend(&mut effects, &[Effect::Read]);
                }
                "write" | "writes" => {
                    explicit = true;
                    extend(&mut effects, &[Effect::Write]);
                }
                "io" => {
                    explicit = true;
                    extend(&mut effects, &[Effect::IO]);
                }
                "alloc" | "allocs" => {
                    explicit = true;
                    extend(&mut effects, &[Effect::Alloc]);
                }
                "panic" | "panics" => {
                    explicit = true;
                    extend(&mut effects, &[Effect::Panic]);
                }
                _ => {}
            }
        }
        if let Some(known) = known {
            extend(&mut effects, &known.effects);
        } else if !explicit {
            effects = ALL.to_vec();
        }
        EffectSet {
            is_pure: effects.is_empty(),
            effects,
        }
    }
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
                self.symbols.define(Symbol {
                    name: format!("{}::{}", block.target.name, method.name.name),
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
    pub(super) fn infer_effects(&mut self, program: &Program) -> Result<()> {
        let mut functions = vec![];
        for item in &program.items {
            match item {
                Item::Function(function) => functions.push((function.name.name.clone(), function)),
                Item::Impl(block) => {
                    for method in &block.methods {
                        functions.push((
                            format!("{}::{}", block.target.name, method.name.name),
                            method,
                        ));
                    }
                }
                _ => {}
            }
        }
        let mut direct = HashMap::new();
        let mut calls = HashMap::new();
        for (name, function) in &functions {
            let mut effects = vec![];
            let mut callees = vec![];
            visit_block(&function.body, &mut |expr| match expr {
                Expr::Call { func, .. } => {
                    let callee = match &**func {
                        Expr::Ident(name) => Some(name.name.clone()),
                        Expr::Path { segments, .. } => Some(
                            segments
                                .iter()
                                .map(|s| s.name.as_str())
                                .collect::<Vec<_>>()
                                .join("::"),
                        ),
                        _ => None,
                    };
                    if let Some(callee) = callee {
                        callees.push(callee);
                    } else {
                        extend(&mut effects, &ALL);
                    }
                }
                Expr::MethodCall { method, .. } if method.name != "add" => {
                    let suffix = format!("::{}", method.name);
                    let targets: Vec<_> = functions
                        .iter()
                        .filter(|(name, _)| name.ends_with(&suffix))
                        .map(|(name, _)| name.clone())
                        .collect();
                    if targets.is_empty() {
                        extend(&mut effects, &ALL);
                    } else {
                        callees.extend(targets);
                    }
                }
                Expr::Ident(name) if self.facts.global_types.contains_key(&name.name) => {
                    extend(&mut effects, &[Effect::Read])
                }
                Expr::Deref { .. } => extend(&mut effects, &[Effect::Read]),
                Expr::Binary { left, op, .. }
                    if matches!(
                        op,
                        BinOp::Assign
                            | BinOp::AddAssign
                            | BinOp::SubAssign
                            | BinOp::MulAssign
                            | BinOp::DivAssign
                    ) && (matches!(&**left, Expr::Deref { .. })
                        || matches!(&**left,Expr::Ident(name) if self.facts.global_types.contains_key(&name.name))) =>
                {
                    extend(&mut effects, &[Effect::Write])
                }
                Expr::Asm { .. } => extend(&mut effects, &ALL),
                _ => {}
            });
            direct.insert(name.clone(), effects);
            calls.insert(name.clone(), callees);
        }
        let mut inferred = direct.clone();
        loop {
            let mut changed = false;
            for (name, _) in &functions {
                let mut effects = direct[name].clone();
                for callee in &calls[name] {
                    if let Some(required) = inferred.get(callee) {
                        extend(&mut effects, required);
                    } else if let Some(Symbol {
                        kind:
                            SymbolKind::Function {
                                effects: required, ..
                            },
                        ..
                    }) = self.symbols.lookup(callee)
                    {
                        extend(&mut effects, &required.effects);
                    } else {
                        extend(&mut effects, &ALL);
                    }
                }
                if inferred[name] != effects {
                    inferred.insert(name.clone(), effects);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        for (name, function) in functions {
            let required = &inferred[&name];
            let declared = &function.effects;
            if declared.is_pure || !declared.effects.is_empty() {
                if let Some(effect) = required
                    .iter()
                    .find(|effect| declared.is_pure || !declared.effects.contains(effect))
                {
                    return Err(Error::EffectViolation {
                        message: format!(
                            "function '{}' requires {:?}, outside its declared {}",
                            name,
                            effect,
                            if declared.is_pure {
                                "pure upper bound"
                            } else {
                                "effect upper bound"
                            }
                        ),
                        span: function.span,
                    });
                }
            }
            if let Some(Symbol {
                kind: SymbolKind::Function { effects, .. },
                ..
            }) = self.symbols.scopes[0].symbols.get_mut(&name)
            {
                *effects = EffectSet {
                    is_pure: required.is_empty(),
                    effects: required.clone(),
                };
            }
        }
        Ok(())
    }
}
fn visit_block(block: &Block, visitor: &mut impl FnMut(&Expr)) {
    for statement in &block.stmts {
        match statement {
            Stmt::Let {
                value: Some(expr), ..
            }
            | Stmt::Return {
                value: Some(expr), ..
            }
            | Stmt::Expr(expr)
            | Stmt::Semi(expr) => visit_expr(expr, visitor),
            _ => {}
        }
    }
}
fn visit_expr(expr: &Expr, visitor: &mut impl FnMut(&Expr)) {
    visitor(expr);
    match expr {
        Expr::Binary { left, right, .. } => {
            visit_expr(left, visitor);
            visit_expr(right, visitor);
        }
        Expr::Unary { expr, .. }
        | Expr::Deref { expr, .. }
        | Expr::Ref { expr, .. }
        | Expr::Cast { expr, .. }
        | Expr::Field { expr, .. }
        | Expr::Try { expr, .. } => visit_expr(expr, visitor),
        Expr::Call { func, args, .. } => {
            visit_expr(func, visitor);
            for arg in args {
                visit_expr(arg, visitor);
            }
        }
        Expr::MethodCall { expr, args, .. } => {
            visit_expr(expr, visitor);
            for arg in args {
                visit_expr(arg, visitor);
            }
        }
        Expr::Index { expr, index, .. } => {
            visit_expr(expr, visitor);
            visit_expr(index, visitor);
        }
        Expr::Block(body) | Expr::Unsafe { body, .. } | Expr::Loop { body, .. } => {
            visit_block(body, visitor)
        }
        Expr::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            visit_expr(cond, visitor);
            visit_block(then_block, visitor);
            if let Some(body) = else_block {
                visit_block(body, visitor);
            }
        }
        Expr::While { cond, body, .. } => {
            visit_expr(cond, visitor);
            visit_block(body, visitor);
        }
        Expr::For { iter, body, .. } => {
            visit_expr(iter, visitor);
            visit_block(body, visitor);
        }
        Expr::Match { expr, arms, .. } => {
            visit_expr(expr, visitor);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    visit_expr(guard, visitor);
                }
                visit_expr(&arm.body, visitor);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, expr) in fields {
                visit_expr(expr, visitor);
            }
        }
        Expr::Array { elements, .. } | Expr::Tuple { elements, .. } => {
            for expr in elements {
                visit_expr(expr, visitor);
            }
        }
        Expr::Range { start, end, .. } => {
            for expr in [start, end].into_iter().flatten() {
                visit_expr(expr, visitor);
            }
        }
        Expr::Asm { operands, .. } => {
            for operand in operands {
                if let Some(expr) = &operand.expr {
                    visit_expr(expr, visitor);
                }
            }
        }
        Expr::Closure { body, .. } => visit_expr(body, visitor),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::{lexer::Lexer, parser::Parser};
    #[test]
    fn trusted_foreign_contracts_merge_without_erasing_known_effects() {
        for (source,ok) in [
            ("extern \"C\"{@pure fn external(x:i32)->i32;}fn f()->i32 pure{external(1)}",true),
            ("extern \"C\"{@pure fn puts(p:*u8)->i32;}fn f() pure{puts(\"x\");}",false),
            ("extern \"C\"{@pure fn external();}extern \"C\"{@writes fn external();}fn f() pure{external();}",false),
        ] {
            let program=Parser::new(Lexer::new(source,0)).parse_program().unwrap();
            assert_eq!(SemanticAnalyzer::new().analyze(&program).is_ok(),ok,"{source}");
        }
    }
}
