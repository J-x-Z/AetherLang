//! Explicit affine records. `own T` changes source ownership, not runtime layout.
//! Registered destructors run on normal scope exits. Raw payload access is explicit.
use super::*;

fn unsupported(message: impl Into<String>, span: Span) -> Error {
    Error::Semantic {
        code: "E2405",
        message: message.into(),
        span,
    }
}

impl SemanticAnalyzer {
    pub(super) fn resolve_owned_type(
        &self,
        inner: &Type,
        ownership: Ownership,
        span: Span,
    ) -> Result<ResolvedType> {
        if ownership != Ownership::Own {
            return Err(unsupported("only 'own T' has implemented ownership semantics; use typed references for borrowing", span));
        }
        let inner = self.resolve_type(inner)?;
        if !matches!(&inner, ResolvedType::Struct { name, .. } if !self.facts.unions.contains(name))
        {
            return Err(unsupported("'own T' requires a named struct payload", span));
        }
        if self.owner_payload_has_borrow(&inner, &mut HashSet::new()) {
            return Err(unsupported(
                "an owned payload cannot contain references or other owned values in this stage",
                span,
            ));
        }
        Ok(ResolvedType::Owned(Box::new(inner)))
    }

    fn owner_fields<'a>(
        &'a self,
        name: &str,
        fallback: &'a [(String, ResolvedType)],
    ) -> &'a [(String, ResolvedType)] {
        match self.symbols.scopes[0].symbols.get(name) {
            Some(Symbol {
                kind: SymbolKind::Struct { fields, .. },
                ..
            }) => fields,
            _ => fallback,
        }
    }

    fn owner_payload_has_borrow(&self, ty: &ResolvedType, seen: &mut HashSet<String>) -> bool {
        self.owner_payload_contains(ty, seen, |t| matches!(t, ResolvedType::Owned(_) | ResolvedType::Reference { .. }))
    }

    pub(super) fn owner_field_has_pointer(&self, ty: &ResolvedType) -> bool {
        self.owner_payload_contains(ty, &mut HashSet::new(), |t| matches!(t, ResolvedType::Pointer(_)))
    }

    fn owner_payload_contains(&self, ty: &ResolvedType, seen: &mut HashSet<String>, test: fn(&ResolvedType) -> bool) -> bool {
        if test(ty) { return true; }
        match ty {
            ResolvedType::Struct { name, fields } if seen.insert(name.clone()) =>
                self.owner_fields(name, fields).iter().any(|(_, t)| self.owner_payload_contains(t, seen, test)),
            ResolvedType::Enum { name } if seen.insert(name.clone()) =>
                self.facts.enum_variants.get(name).is_some_and(|variants|
                    variants.iter().any(|(_, fields)| fields.iter().any(|t| self.owner_payload_contains(t, seen, test)))),
            ResolvedType::Array { elem, .. } | ResolvedType::Volatile(elem) =>
                self.owner_payload_contains(elem, seen, test),
            ResolvedType::Tuple(elements) => elements.iter().any(|t| self.owner_payload_contains(t, seen, test)),
            _ => false,
        }
    }

    pub(super) fn contains_owned(&self, ty: &ResolvedType) -> bool {
        fn visit(
            analyzer: &SemanticAnalyzer,
            ty: &ResolvedType,
            seen: &mut HashSet<String>,
        ) -> bool {
            match ty {
                ResolvedType::Owned(_) => true,
                ResolvedType::Pointer(t)
                | ResolvedType::Reference { inner: t, .. }
                | ResolvedType::Volatile(t)
                | ResolvedType::Slice(t)
                | ResolvedType::Array { elem: t, .. } => visit(analyzer, t, seen),
                ResolvedType::Tuple(elements) => elements.iter().any(|t| visit(analyzer, t, seen)),
                ResolvedType::Struct { name, fields } if seen.insert(name.clone()) => analyzer
                    .owner_fields(name, fields)
                    .iter()
                    .any(|(_, t)| visit(analyzer, t, seen)),
                ResolvedType::Enum { name } if seen.insert(name.clone()) => analyzer
                    .facts
                    .enum_variants
                    .get(name)
                    .is_some_and(|variants| {
                        variants
                            .iter()
                            .any(|(_, fields)| fields.iter().any(|t| visit(analyzer, t, seen)))
                    }),
                _ => false,
            }
        }
        visit(self, ty, &mut HashSet::new())
    }

    pub(super) fn validate_owned_shape(
        &self,
        ty: &ResolvedType,
        allow_owner: bool,
        span: Span,
    ) -> Result<()> {
        fn visit(
            analyzer: &SemanticAnalyzer,
            ty: &ResolvedType,
            allow: bool,
            span: Span,
            seen: &mut HashSet<String>,
        ) -> Result<()> {
            match ty {
                ResolvedType::Owned(inner) => {
                    if !allow {
                        return Err(unsupported("owned values cannot yet be stored in aggregates, globals or raw-pointer storage", span));
                    }
                    if !matches!(inner.as_ref(), ResolvedType::Struct { name, .. } if !analyzer.facts.unions.contains(name))
                        || analyzer.owner_payload_has_borrow(inner, &mut HashSet::new())
                    {
                        return Err(unsupported("an owned payload must be a named struct without references or nested owners", span));
                    }
                    visit(analyzer, inner, false, span, seen)?;
                }
                ResolvedType::Reference { inner, .. } => visit(analyzer, inner, true, span, seen)?,
                ResolvedType::Pointer(inner) | ResolvedType::Slice(inner) => {
                    if analyzer.contains_owned(inner) {
                        return Err(unsupported("raw-pointer and slice storage of owned values is not implemented; borrow with '&own T'", span));
                    }
                }
                ResolvedType::Array { elem, .. } | ResolvedType::Volatile(elem) => {
                    visit(analyzer, elem, false, span, seen)?
                }
                ResolvedType::Tuple(elements) => {
                    for ty in elements {
                        visit(analyzer, ty, false, span, seen)?;
                    }
                }
                ResolvedType::Struct { name, fields } if seen.insert(name.clone()) => {
                    for (_, ty) in analyzer.owner_fields(name, fields) {
                        visit(analyzer, ty, false, span, seen)?;
                    }
                }
                ResolvedType::Enum { name } if seen.insert(name.clone()) => {
                    if let Some(variants) = analyzer.facts.enum_variants.get(name) {
                        for (_, fields) in variants {
                            for ty in fields {
                                visit(analyzer, ty, false, span, seen)?;
                            }
                        }
                    }
                }
                _ => {}
            }
            Ok(())
        }
        visit(self, ty, allow_owner, span, &mut HashSet::new())
    }

    pub(super) fn destructor_for(&self, ty: &ResolvedType) -> Option<&String> {
        let ResolvedType::Owned(inner) = ty else {
            return None;
        };
        self.facts.drop_functions.get(&Self::type_identity(inner))
    }

    pub(super) fn collect_destructors(&mut self, program: &Program) -> Result<()> {
        for item in &program.items {
            let invalid = match item {
                Item::Struct(def) => def.annotations.iter().find(|a| a.name.name == "drop"),
                Item::Static(def) => def.annotations.iter().find(|a| a.name.name == "drop"),
                Item::Extern(block) => block.items.iter().find_map(|item| match item {
                    ForeignItem::Fn { annotations, .. } => {
                        annotations.iter().find(|a| a.name.name == "drop")
                    }
                    _ => None,
                }),
                _ => None,
            };
            if let Some(annotation) = invalid {
                return Err(unsupported(
                    "@drop requires a concrete language function with a body",
                    annotation.span,
                ));
            }
            if let Item::Impl(block) = item {
                if block
                    .methods
                    .iter()
                    .any(|method| method.annotations.iter().any(|a| a.name.name == "drop"))
                {
                    return Err(unsupported(
                        "@drop requires a top-level ordinary function",
                        block.span,
                    ));
                }
            }
            let Item::Function(function) = item else {
                continue;
            };
            let markers: Vec<_> = function
                .annotations
                .iter()
                .filter(|a| a.name.name == "drop")
                .collect();
            if markers.is_empty() {
                continue;
            }
            if markers.len() != 1
                || !markers[0].args.is_empty()
                || !function.type_params.is_empty()
                || !function.contracts.is_empty()
            {
                return Err(unsupported("@drop is a single argument-free annotation on a concrete function without contracts", function.span));
            }
            let Some(Symbol {
                kind: SymbolKind::Function { params, ret, .. },
                ..
            }) = self.symbols.lookup(&function.name.name)
            else {
                unreachable!()
            };
            if params.len() != 1
                || *ret != ResolvedType::UNIT
                || !matches!(&params[0], ResolvedType::Owned(_))
            {
                return Err(unsupported(
                    "@drop requires exactly one own T parameter and a unit return",
                    function.span,
                ));
            }
            let ResolvedType::Owned(inner) = &params[0] else {
                unreachable!()
            };
            let key = Self::type_identity(inner);
            if let Some(previous) = self
                .facts
                .drop_functions
                .insert(key.clone(), function.name.name.clone())
            {
                return Err(unsupported(
                    format!(
                        "one destructor for {key}; both '{previous}' and '{}' were declared",
                        function.name.name
                    ),
                    function.span,
                ));
            }
        }
        Ok(())
    }

    pub(super) fn check_owned_match(&mut self, value: &Expr) -> Result<()> {
        if matches!(
            self.check_expr(value)?.unqualified(),
            ResolvedType::Owned(_)
        ) {
            return Err(unsupported("matching owned values is not implemented; inspect borrowed fields and transfer with if/else", value.span()));
        }
        Ok(())
    }

    pub(super) fn check_owned_cast(
        &self,
        source: &ResolvedType,
        target: &ResolvedType,
        span: Span,
    ) -> Result<()> {
        if Self::types_identical(source, target) {
            return Ok(());
        }
        let (ResolvedType::Owned(inner), raw) = (match (source, target) {
            (source @ ResolvedType::Owned(_), raw) => (source, raw),
            (raw, target @ ResolvedType::Owned(_)) => (target, raw),
            _ => unreachable!(),
        }) else {
            unreachable!()
        };
        if !Self::types_identical(inner, raw) {
            return Err(unsupported(
                "ownership conversion requires exactly the same nominal payload type",
                span,
            ));
        }
        if self.unsafe_depth == 0 {
            return Err(unsupported("adopting or releasing an owned value requires unsafe and a unique raw-resource transfer", span));
        }
        Ok(())
    }

    fn check_owned_function(
        &self,
        function: &Function,
        params: &[ResolvedType],
        ret: &ResolvedType,
    ) -> Result<()> {
        for (param, ty) in function.params.iter().zip(params) {
            self.validate_owned_shape(ty, true, param.span)?;
            if param.ownership == Ownership::Ref && self.contains_owned(ty) {
                return Err(unsupported(
                    "borrow an owned value using '&own T', not the legacy 'ref' parameter modifier",
                    param.span,
                ));
            }
        }
        self.validate_owned_shape(ret, true, function.span)?;
        if params
            .iter()
            .chain(std::iter::once(ret))
            .any(|t| self.contains_owned(t))
        {
            if function
                .annotations
                .iter()
                .any(|a| matches!(a.name.name.as_str(), "no_mangle" | "interrupt" | "naked"))
            {
                return Err(unsupported(
                    "owned values and their references cannot cross an external ABI boundary",
                    function.span,
                ));
            }
            if !function.contracts.is_empty() {
                return Err(unsupported("contracts on owned signatures require ownership-aware snapshots and are not implemented yet", function.span));
            }
        }
        Ok(())
    }

    pub(super) fn check_owned_declarations(&self, program: &Program) -> Result<()> {
        for item in &program.items {
            match item {
                Item::Function(function) => {
                    if let Some(Symbol {
                        kind: SymbolKind::Function { params, ret, .. },
                        ..
                    }) = self.symbols.lookup(&function.name.name)
                    {
                        self.check_owned_function(function, params, ret)?;
                    }
                }
                Item::Impl(block) => {
                    for method in &block.methods {
                        if let Some(signature) = self
                            .facts
                            .method_signatures
                            .get(&format!("{}::{}", block.target.name, method.name.name))
                        {
                            self.check_owned_function(method, &signature.params, &signature.ret)?;
                        }
                    }
                }
                Item::Struct(def) => {
                    for field in &def.fields {
                        self.validate_owned_shape(
                            &self.resolve_type(&field.ty)?,
                            false,
                            field.span,
                        )?;
                    }
                }
                Item::Union(def) => {
                    for field in &def.fields {
                        self.validate_owned_shape(
                            &self.resolve_type(&field.ty)?,
                            false,
                            field.span,
                        )?;
                    }
                }
                Item::Enum(def) => {
                    for variant in &def.variants {
                        for ty in &variant.fields {
                            self.validate_owned_shape(&self.resolve_type(ty)?, false, ty.span())?;
                        }
                    }
                }
                Item::Extern(block) => {
                    for item in &block.items {
                        let types: Vec<&Type> = match item {
                            ForeignItem::Fn {
                                params, ret_type, ..
                            } => params
                                .iter()
                                .map(|p| &p.ty)
                                .chain(ret_type.iter())
                                .collect(),
                            ForeignItem::Static { ty, .. } => vec![ty],
                        };
                        for ty in types {
                            if self.contains_owned(&self.resolve_type(ty)?) {
                                return Err(unsupported("owned values and their references cannot cross an external ABI boundary", ty.span()));
                            }
                        }
                    }
                }
                Item::Static(def) => {
                    if self.contains_owned(&self.resolve_type(&def.ty)?) {
                        return Err(unsupported(
                            "global owned storage is not implemented",
                            def.span,
                        ));
                    }
                }
                Item::Const(def) => {
                    if let Some(ty) = &def.ty {
                        if self.contains_owned(&self.resolve_type(ty)?) {
                            return Err(unsupported(
                                "constant owned values are not implemented",
                                def.span,
                            ));
                        }
                    }
                }
                Item::TypeAlias(def) => {
                    self.validate_owned_shape(&self.resolve_type(&def.ty)?, true, def.span)?
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::{lexer::Lexer, parser::Parser};

    fn check(body: &str) -> Result<()> {
        let source = format!("struct Handle {{ raw:i32 }} fn make(n:i32)->own Handle {{ unsafe {{ Handle{{raw:n}} as own Handle }} }} fn consume(value:own Handle){{}} {body}");
        let program = Parser::new(Lexer::new(&source, 0)).parse_program()?;
        SemanticAnalyzer::new().analyze(&program)
    }

    #[test]
    fn affine_transfer_preserves_identity_and_control_flow() {
        for source in [
            "fn relay(x:own Handle)->own Handle{x} fn main(){let first:own Handle=make(3);let second:own Handle=relay(first);consume(second);}",
            "fn inspect(x:&own Handle)->i32{x.raw} fn main(){let x:own Handle=make(3);let n:i32=inspect(&x);consume(x);}",
            "fn choose(flag:bool){let mut x:own Handle=make(1);if flag{consume(x);}else{consume(x);}x=make(2);consume(x);}",
            "fn choose(flag:bool)->own Handle{let x:own Handle=make(1);if flag{return x;}x}",
            "fn main(){let mut x:own Handle; x=make(1);let y:own Handle=x;x=make(2);consume(x);consume(y);}",
            "fn main(){let x:own Handle=make(1);{let x:own Handle=make(2);consume(x);}consume(x);}",
            "fn raw(x:own Handle)->Handle{unsafe{x as Handle}} fn main(){let x:own Handle=make(1);let value:Handle=raw(x);}",
        ] { check(source).unwrap_or_else(|error| panic!("{source}: {error:?}")); }
    }

    #[test]
    fn moved_or_possibly_moved_values_cannot_be_copied() {
        for source in [
            "fn main(){let x:own Handle=make(1);let y:own Handle=x;consume(x);}",
            "fn main(){let x:own Handle=make(1);consume(x);let n:i32=x.raw;}",
            "fn bad(flag:bool){let x:own Handle=make(1);if flag{consume(x);}consume(x);}",
            "fn bad(flag:bool){let x:own Handle=make(1);while flag{consume(x);}}",
            "fn main(){let x:own Handle=make(1);consume(x);let y:own Handle=x;}",
            "fn both(a:own Handle,b:own Handle){}fn main(){let x:own Handle=make(1);both(x,x);}",
        ] {
            assert!(
                matches!(check(source), Err(Error::Semantic { code: "E2301", .. })),
                "{source}: {:?}",
                check(source)
            );
        }
    }

    #[test]
    fn ownership_boundaries_are_explicit() {
        for source in [
            "fn main(){let x:own Handle=Handle{raw:1} as own Handle;}",
            "fn main(){let x:own Handle=make(1);let raw:Handle=x as Handle;}",
            "fn take(x:&own Handle)->own Handle{*x}",
            "fn take(x:&mut own Handle)->own Handle{*x}",
            "fn main(){let x:own Handle=make(1);let y:own Handle=*(&x);}",
            "fn main(){let mut x:own Handle=make(1);x=make(2);}",
            "fn bad(flag:bool){let mut x:own Handle=make(1);if flag{consume(x);}x=make(2);}",
            "struct Box{value:own Handle}",
            "enum Option{Some(own Handle),None}",
            "fn main(){let x:own Handle=make(1);(x,);}",
            "fn main(){let x:own Handle=make(1);[x];}",
            "fn bad(x:*own Handle){}",
            "extern \"C\"{fn take(x:own Handle);}",
            "extern \"C\"{fn take(x:&own Handle);}",
            "#[no_mangle] fn take(x:own Handle){}",
            "static VALUE:own Handle;",
            "fn bad(x:ref own Handle){}",
            "fn bad(x:shared Handle){}",
            "fn bad(x:own i32){}",
            "fn bad(x:own Handle){match x{other=>consume(other)}}",
        ] {
            assert!(
                matches!(check(source), Err(Error::Semantic { code: "E2405", .. })),
                "{source}: {:?}",
                check(source)
            );
        }
        assert!(check("fn main(){let x:own Handle=make(1);let loan:&own Handle=&x;consume(x);let n:i32=loan.raw;}").is_err());
        assert!(check("fn bad(x:own Handle)->&i32{&x.raw}").is_err());
        assert!(check("struct RefHandle{borrow:&i32} fn bad(x:own RefHandle){}").is_err());
    }
}
