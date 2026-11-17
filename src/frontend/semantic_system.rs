//! Target attributes are ABI contracts, not advisory optimizer annotations.
use super::*;

fn error(message: impl Into<String>, span: Span) -> Error {
    Error::Semantic {
        code: "E2402",
        message: message.into(),
        span,
    }
}
fn expressions(block: &Block) -> Vec<&Expr> {
    fn add_block<'a>(block: &'a Block, pending: &mut Vec<&'a Expr>) {
        for statement in &block.stmts {
            match statement {
                Stmt::Expr(expr) | Stmt::Semi(expr) => pending.push(expr),
                Stmt::Let {
                    value: Some(expr), ..
                }
                | Stmt::Return {
                    value: Some(expr), ..
                } => pending.push(expr),
                _ => {}
            }
        }
    }
    let mut pending = Vec::new();
    let mut result = Vec::new();
    add_block(block, &mut pending);
    while let Some(expr) = pending.pop() {
        result.push(expr);
        match expr {
            Expr::Binary { left, right, .. } => {
                pending.push(left);
                pending.push(right);
            }
            Expr::Unary { expr, .. }
            | Expr::Deref { expr, .. }
            | Expr::Ref { expr, .. }
            | Expr::Cast { expr, .. }
            | Expr::Field { expr, .. }
            | Expr::Try { expr, .. } => pending.push(expr),
            Expr::Call { func, args, .. } => {
                pending.push(func);
                pending.extend(args);
            }
            Expr::MethodCall { expr, args, .. } => {
                pending.push(expr);
                pending.extend(args);
            }
            Expr::Index { expr, index, .. } => {
                pending.push(expr);
                pending.push(index);
            }
            Expr::Block(block)
            | Expr::Unsafe { body: block, .. }
            | Expr::Loop { body: block, .. } => add_block(block, &mut pending),
            Expr::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                pending.push(cond);
                add_block(then_block, &mut pending);
                if let Some(block) = else_block {
                    add_block(block, &mut pending);
                }
            }
            Expr::While { cond, body, .. } => {
                pending.push(cond);
                add_block(body, &mut pending);
            }
            Expr::For { iter, body, .. } => {
                pending.push(iter);
                add_block(body, &mut pending);
            }
            Expr::Match { expr, arms, .. } => {
                pending.push(expr);
                for arm in arms {
                    pending.push(&arm.body);
                    if let Some(guard) = &arm.guard {
                        pending.push(guard);
                    }
                }
            }
            Expr::ArrayRepeat {value,count,..} => {pending.push(value);pending.push(count);},
            Expr::Array { elements, .. } | Expr::Tuple { elements, .. } => pending.extend(elements),
            Expr::StructLit { fields, .. } => pending.extend(fields.iter().map(|(_, value)| value)),
            Expr::Range { start, end, .. } => {
                pending.extend(start.iter().map(|x| x.as_ref()));
                pending.extend(end.iter().map(|x| x.as_ref()));
            }
            Expr::Asm { operands, .. } => {
                pending.extend(operands.iter().filter_map(|operand| operand.expr.as_ref()))
            }
            Expr::Closure { body, .. } => pending.push(body),
            _ => {}
        }
    }
    result
}
fn naked_body(block: &Block) -> Result<usize> {
    let mut count = 0;
    for statement in &block.stmts {
        match statement {
            Stmt::Empty {..} => {},
            Stmt::Expr(Expr::Unsafe {body,..}) | Stmt::Semi(Expr::Unsafe {body,..}) => count += naked_body(body)?,
            Stmt::Expr(Expr::Asm {operands,template,span}) | Stmt::Semi(Expr::Asm {operands,template,span}) => {
                if !operands.is_empty() || template.trim().is_empty() { return Err(error("naked assembly must be nonempty and operandless", *span)); }
                count += 1;
            }
            _ => return Err(error("naked functions contain only operandless asm! in unsafe blocks; assembly owns all control flow and the return", block.span)),
        }
    }
    Ok(count)
}
impl SemanticAnalyzer {
    pub(super) fn check_system_attributes(&self, program: &Program) -> Result<()> {
        let arch = if self.system_target == "native" {
            std::env::consts::ARCH
        } else {
            self.system_target.split('-').next().unwrap_or("")
        };
        let no_std = program
            .inner_attrs
            .iter()
            .any(|attribute| attribute.name.name == "no_std");
        let mut functions = Vec::new();
        for item in &program.items {
            match item {
                Item::Function(function) => functions.push((
                    function,
                    function.name.name.clone(),
                    function.name.name.clone(),
                )),
                Item::Impl(block) => {
                    for function in &block.methods {
                        let symbol = format!("{}::{}", block.target.name, function.name.name);
                        let key = self
                            .facts
                            .method_signatures
                            .get(&symbol)
                            .map(|signature| signature.emitted_name.clone())
                            .unwrap_or_else(|| symbol.clone());
                        functions.push((function, symbol, key));
                    }
                }
                _ => {}
            }
        }
        let handlers: HashSet<_> = functions
            .iter()
            .filter(|(function, _, _)| {
                function
                    .annotations
                    .iter()
                    .any(|annotation| annotation.name.name == "interrupt")
            })
            .map(|(_, symbol, _)| symbol.clone())
            .collect();
        for (function, symbol, key) in functions {
            let interrupt = function
                .annotations
                .iter()
                .any(|annotation| annotation.name.name == "interrupt");
            let naked = function
                .annotations
                .iter()
                .any(|annotation| annotation.name.name == "naked");
            if interrupt || naked {
                if interrupt && naked {
                    return Err(error(
                        "interrupt and naked cannot be combined",
                        function.span,
                    ));
                }
                let Some(Symbol {
                    kind: SymbolKind::Function { params, ret, .. },
                    ..
                }) = self.symbols.lookup(&symbol)
                else {
                    continue;
                };
                if *ret != ResolvedType::UNIT {
                    return Err(error(
                        "interrupt/naked functions must return void",
                        function.span,
                    ));
                }
                if !function.contracts.is_empty() {
                    return Err(error(
                        "interrupt/naked functions cannot have runtime contracts",
                        function.span,
                    ));
                }
                if naked {
                    if !matches!(arch, "x86_64" | "aarch64" | "arm64") {
                        return Err(error(
                            "naked functions currently support x86_64 and aarch64 targets",
                            function.span,
                        ));
                    }
                    if !params.is_empty() {
                        return Err(error("naked functions must have no parameters; assembly owns register inputs",function.span));
                    }
                    if naked_body(&function.body)? == 0 {
                        return Err(error(
                            "naked functions require an assembly body",
                            function.span,
                        ));
                    }
                } else {
                    if arch != "x86_64" || self.facts.pointer_width != PointerWidth::W64 {
                        return Err(error(
                            "interrupt handlers currently support 64-bit x86_64 targets",
                            function.span,
                        ));
                    }
                    let frame = params.first().and_then(|ty| {
                        if let ResolvedType::Pointer(inner) = ty {
                            Some(inner.as_ref())
                        } else {
                            None
                        }
                    });
                    let valid = frame.is_some_and(|ty|if let ResolvedType::Struct{name,..}=ty {
                        let fields=self.symbols.scopes[0].symbols.get(name).and_then(|symbol|if let SymbolKind::Struct{fields,..}=&symbol.kind{Some(fields)}else{None});
                        let packed=program.items.iter().any(|item|matches!(item,Item::Struct(definition) if definition.name.name==*name && definition.annotations.iter().any(|annotation|annotation.name.name=="repr" && matches!(annotation.args.first(),Some(Expr::Ident(name)) if name.name=="packed"))));
                        fields.is_some_and(|fields|fields.len()==5 && fields.iter().all(|(_,ty)|*ty==ResolvedType::U64)) && !packed && !self.facts.unions.contains(name)
                    }else{false});
                    if !valid
                        || !(params.len() == 1
                            || params.len() == 2 && params[1] == ResolvedType::U64)
                    {
                        return Err(error("x86_64 interrupt signature is fn(frame: *Frame [, error: u64]); Frame must contain five unpacked u64 fields in ip/cs/flags/sp/ss order",function.span));
                    }
                }
            }
            for expression in expressions(&function.body) {
                if matches!(expression, Expr::Asm { template, .. } if template.contains('\0')) {
                    return Err(error(
                        "inline assembly cannot contain a NUL byte",
                        expression.span(),
                    ));
                }
                if let Expr::Call { func, .. } = expression {
                    let name = match func.as_ref() {
                        Expr::Ident(name) => Some(name.name.clone()),
                        Expr::Path { segments, .. } => Some(
                            segments
                                .iter()
                                .map(|segment| segment.name.as_str())
                                .collect::<Vec<_>>()
                                .join("::"),
                        ),
                        _ => None,
                    };
                    if let Some(name) = &name {
                        if handlers.contains(name) {
                            return Err(error(
                                "interrupt handlers cannot be called as ordinary functions",
                                expression.span(),
                            ));
                        }
                        if no_std
                            && crate::intrinsics::memory(name).is_none()
                            && crate::intrinsics::vector(name).is_none()
                            && crate::intrinsics::platform(name).is_none()
                            && self
                                .symbols
                                .lookup(name)
                                .is_some_and(|symbol| symbol.span == Span::dummy())
                        {
                            return Err(error(
                                format!(
                                    "no_std requires an explicit extern declaration for '{name}'"
                                ),
                                expression.span(),
                            ));
                        }
                    }
                    if interrupt
                        && name
                            .as_deref()
                            .is_none_or(|name| crate::intrinsics::memory(name).is_none())
                    {
                        return Err(error("interrupt handlers currently allow integer/pointer operations and memory intrinsics, not ordinary calls",expression.span()));
                    }
                }
                if interrupt {
                    if matches!(expression, Expr::MethodCall { .. } | Expr::Asm { .. }) {
                        return Err(error("interrupt handlers do not support method calls or inline assembly; use a dedicated naked entry for manual register management",expression.span()));
                    }
                    if let Some(ty) = self.facts.expr_types.get(&(key.clone(), expression.span())) {
                        if matches!(
                            ty.unqualified(),
                            ResolvedType::Primitive(PrimitiveType::F32 | PrimitiveType::F64)
                                | ResolvedType::Array { .. }
                                | ResolvedType::Tuple(_)
                                | ResolvedType::Struct { .. }
                                | ResolvedType::Enum { .. }
                                | ResolvedType::Vector(..)
                        ) {
                            return Err(error("interrupt handlers currently support scalar integers and pointers, without floating/vector/aggregate values",expression.span()));
                        }
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
    fn check(source: &str, target: &str) -> Result<()> {
        let program = Parser::new(Lexer::new(source, 0)).parse_program()?;
        SemanticAnalyzer::for_target(target)
            .unwrap()
            .analyze(&program)
    }
    const FRAME: &str = "#![no_std]#![no_main]struct Frame{ip:u64,cs:u64,flags:u64,sp:u64,ss:u64}";
    #[test]
    fn machine_entry_signatures_and_bodies_are_checked_before_codegen() {
        for (source, target) in [
            (
                format!("{FRAME}@interrupt fn handler(frame:*Frame){{frame.ip=frame.ip+1;}}"),
                "x86_64-unknown-none",
            ),
            (
                format!("{FRAME}@interrupt fn exception(frame:*Frame,code:u64){{frame.ip=code;}}"),
                "x86_64-unknown-none",
            ),
            (
                "#![no_std]#![no_main]@naked fn raw(){unsafe{asm!(\"ret\");}}".into(),
                "aarch64-unknown-none",
            ),
        ] {
            check(&source, target).unwrap();
        }
        for (tail, target) in [
            ("@interrupt fn bad(frame:*Frame){}", "aarch64-unknown-none"),
            ("@interrupt fn bad(){}", "x86_64-unknown-none"),
            (
                "@interrupt fn bad(frame:*Frame,code:i64){}",
                "x86_64-unknown-none",
            ),
            (
                "@interrupt fn bad(frame:*Frame)->i32{0}",
                "x86_64-unknown-none",
            ),
            (
                "@interrupt fn bad(frame:*Frame){let x:f64=1.0;}",
                "x86_64-unknown-none",
            ),
            (
                "fn helper(){}@interrupt fn bad(frame:*Frame){helper();}",
                "x86_64-unknown-none",
            ),
            (
                "@interrupt fn handler(frame:*Frame){}fn bad(frame:*Frame){handler(frame);}",
                "x86_64-unknown-none",
            ),
            ("@naked fn bad(){let x:i32=1;}", "x86_64-unknown-none"),
            (
                "@naked fn bad(){unsafe{asm!(\"nop\",in(\"r\") 1);}}",
                "x86_64-unknown-none",
            ),
            (
                "@naked fn bad(){unsafe{asm!(\"ret\");}}",
                "riscv64-unknown-none-elf",
            ),
            (
                "fn bad(){puts(\"implicit hosted call\");}",
                "x86_64-unknown-none",
            ),
        ] {
            assert!(
                matches!(
                    check(&format!("{FRAME}{tail}"), target),
                    Err(Error::Semantic { code: "E2402", .. })
                ),
                "{tail}"
            );
        }
    }
}
