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
    fn collect_effect_node(&self, function_key: &str, expr: &Expr, access: Access,
        effects: &mut Vec<Effect>, callees: &mut Vec<String>) {
                if let Some(place) = self.facts.places.get(&(function_key.to_owned(), expr.span())) {
                    if place.origin != PlaceOrigin::Local {
                        if matches!(access, Access::Read | Access::ReadWrite) { extend(effects, &[Effect::Read]); }
                        if matches!(access, Access::Write | Access::ReadWrite) { extend(effects, &[Effect::Write]); }
                    }
                    if place.address_reads { extend(effects, &[Effect::Read]); }
                }
                match expr {
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
                        extend(effects, &ALL);
                    }
                }
                Expr::MethodCall { span, .. } => {
                    match self.facts.method_calls.get(&(function_key.to_owned(), *span)) {
                        Some(ResolvedMethodCall::Function { symbol, .. }) => callees.push(symbol.clone()),
                        Some(ResolvedMethodCall::PointerAdd { .. } | ResolvedMethodCall::DivergingReceiver) => {},
                        None => extend(effects, &ALL),
                    }
                }
                Expr::Asm { .. } => extend(effects, &ALL),
                _ => {}
                }
    }

    /// Uses the same place/callee interpretation as body effect inference.
    pub(super) fn contract_expression_effects(&self, function: &str, expr: &Expr) -> Vec<Effect> {
        let mut effects = vec![];
        let mut callees = vec![];
        let walker = EffectWalker { facts: &self.facts, function };
        walker.expr(expr, Access::Read, &mut |node, access| {
            self.collect_effect_node(function, node, access, &mut effects, &mut callees);
            if matches!(access, Access::Read | Access::ReadWrite)
                && self.facts.places.get(&(function.to_owned(), node.span()))
                    .is_some_and(|place| matches!(place.ty, ResolvedType::Volatile(_))) {
                extend(&mut effects, &[Effect::Read]);
            }
        });
        for callee in callees {
            if let Some(Symbol { kind: SymbolKind::Function { effects: required, .. }, .. }) = self.symbols.lookup(&callee) {
                extend(&mut effects, &required.effects);
            } else { extend(&mut effects, &ALL); }
        }
        effects
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
            // @volatile turns the function's parameter/local storage accesses
            // into observable memory operations, including compiler allocas.
            let mut effects = if function.annotations.iter().any(|a| a.name.name == "volatile") {
                vec![Effect::Read, Effect::Write]
            } else { vec![] };
            let mut callees = vec![];
            let function_key = self.facts.method_signatures.get(name)
                .map(|signature| &signature.emitted_name).unwrap_or(name);
            let walker = EffectWalker { facts: &self.facts, function: function_key };
            walker.block(&function.body, &mut |expr, access| {
                self.collect_effect_node(function_key, expr, access, &mut effects, &mut callees);
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
/// Computing an address is distinct from reading or writing its contents.
/// Index operands and pointer-valued bases still execute/read normally.
#[derive(Clone, Copy)]
enum Access { Read, Write, ReadWrite, Address }

struct EffectWalker<'a> { facts: &'a SemanticFacts, function: &'a str }
impl EffectWalker<'_> {
    fn block(&self, block: &Block, visitor: &mut impl FnMut(&Expr, Access)) {
        for statement in &block.stmts {
            match statement {
                Stmt::Let { value: Some(expr), .. } | Stmt::Return { value: Some(expr), .. }
                | Stmt::Expr(expr) | Stmt::Semi(expr) => self.expr(expr, Access::Read, visitor),
                _ => {}
            }
        }
    }
    fn expr(&self, expr: &Expr, access: Access, visitor: &mut impl FnMut(&Expr, Access)) {
        visitor(expr, access);
        match expr {
            Expr::Binary { left, op, right, .. } => {
                let left_access = match op {
                    BinOp::Assign => Access::Write,
                    BinOp::AddAssign | BinOp::SubAssign | BinOp::MulAssign | BinOp::DivAssign => Access::ReadWrite,
                    _ => Access::Read,
                };
                self.expr(left, left_access, visitor);
                self.expr(right, Access::Read, visitor);
            }
            Expr::Ref { expr, .. } => self.expr(expr, Access::Address, visitor),
            Expr::Unary { expr, .. } | Expr::Deref { expr, .. } | Expr::Cast { expr, .. }
            | Expr::Try { expr, .. } => self.expr(expr, Access::Read, visitor),
            Expr::Field { expr: base, .. } | Expr::Index { expr: base, .. } => {
                let indirect = self.facts.places.get(&(self.function.to_owned(), base.span()))
                    .is_some_and(|place| matches!(place.ty.unqualified(), ResolvedType::Reference { .. } | ResolvedType::Pointer(_) | ResolvedType::Slice(_)));
                self.expr(base, if indirect { Access::Read } else { Access::Address }, visitor);
                if let Expr::Index { index, .. } = expr { self.expr(index, Access::Read, visitor); }
            }
            Expr::Call { func, args, .. } => {
                self.expr(func, Access::Read, visitor);
                for arg in args { self.expr(arg, Access::Read, visitor); }
            }
            Expr::MethodCall { expr, args, span, .. } => {
                let borrowed = matches!(self.facts.method_calls.get(&(self.function.to_owned(), *span)),
                    Some(ResolvedMethodCall::Function { receiver: ReceiverAdjustment::Borrow { .. }, .. }));
                self.expr(expr, if borrowed { Access::Address } else { Access::Read }, visitor);
                for arg in args { self.expr(arg, Access::Read, visitor); }
            }
            Expr::Block(body) | Expr::Unsafe { body, .. } | Expr::Loop { body, .. } => self.block(body, visitor),
            Expr::If { cond, then_block, else_block, .. } => {
                self.expr(cond, Access::Read, visitor); self.block(then_block, visitor);
                if let Some(body) = else_block { self.block(body, visitor); }
            }
            Expr::While { cond, body, .. } => { self.expr(cond, Access::Read, visitor); self.block(body, visitor); }
            Expr::For { iter, body, .. } => { self.expr(iter, Access::Read, visitor); self.block(body, visitor); }
            Expr::Match { expr, arms, .. } => {
                self.expr(expr, Access::Read, visitor);
                for arm in arms {
                    if let Some(guard) = &arm.guard { self.expr(guard, Access::Read, visitor); }
                    self.expr(&arm.body, Access::Read, visitor);
                }
            }
            Expr::StructLit { fields, .. } => { for (_, expr) in fields { self.expr(expr, Access::Read, visitor); } }
            Expr::Array { elements, .. } | Expr::Tuple { elements, .. } => { for expr in elements { self.expr(expr, Access::Read, visitor); } }
            Expr::Range { start, end, .. } => { for expr in [start, end].into_iter().flatten() { self.expr(expr, Access::Read, visitor); } }
            Expr::Asm { operands, .. } => {
                for operand in operands { if let Some(expr) = &operand.expr { self.expr(expr, Access::Read, visitor); } }
            }
            Expr::Closure { body, .. } => self.expr(body, Access::Read, visitor),
            _ => {}
        }
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
    #[test]
    fn projection_effects_distinguish_contents_from_address_computation() {
        let structure = "struct S{x:i32} struct Outer{inner:S} struct Holder{p:*S} ";
        for body in [
            "fn change(s:&mut S) pure{s.x=7;}",
            "fn change(a:&mut [i32;1]) pure{a[0]=9;}",
            "fn change(a:*i32) pure{a[0]=9;}",
            "fn change(s:&mut Outer) pure{s.inner.x=7;}",
            "fn change(h:Holder) pure{h.p.x=7;}",
            "static mut GLOBAL:S=S{x:0};fn change() pure{GLOBAL.x=7;}",
            "fn change(s:&mut S){s.x=7;}fn wrapper(s:&mut S) pure{change(s);}",
            "impl S{fn change(self:&mut Self) pure{self.x=7;}}",
            "fn change(s:&mut S) effect[write]{s.x+=1;}",
            "fn read(s:&S)->i32 pure{s.x}",
            "fn index()->i64{puts(\"io\");0}fn change(a:&mut [i32;1]) effect[write]{a[index()]=1;}",
            "fn change(s:**S) effect[write]{s.x=7;}",
        ] {
            let source = format!("{structure}{body}");
            let program = Parser::new(Lexer::new(&source, 0)).parse_program().unwrap();
            let error = SemanticAnalyzer::new().analyze(&program).expect_err(&source);
            assert!(matches!(error, Error::EffectViolation {..}), "{source}: {error:?}");
        }
        for body in [
            "fn change(s:&mut S,a:&mut [i32;1]) effect[write]{s.x=7;a[0]=9;}",
            "fn change(s:&mut S) effect[read,write]{s.x+=1;}",
            "fn read(s:&S)->i32 effect[read]{s.x}",
            "fn address(s:&mut S)->&mut i32 pure{&mut s.x}",
            "fn local()->i32 pure{let mut s:S=S{x:1};let mut a:[i32;1]=[2];s.x=7;a[0]=9;s.x+a[0]}",
            "static mut GLOBAL:S=S{x:0};fn shadow()->i32 pure{let mut GLOBAL:S=S{x:1};GLOBAL.x=7;GLOBAL.x}",
            "static mut GLOBAL:S=S{x:0};fn change() effect[write]{GLOBAL.x=7;}",
            "fn change(s:**S) effect[read,write]{s.x=7;}",
        ] {
            let source = format!("{structure}{body}");
            let program = Parser::new(Lexer::new(&source, 0)).parse_program().unwrap();
            SemanticAnalyzer::new().analyze(&program).unwrap_or_else(|error| panic!("{source}: {error:?}"));
        }
    }

}
