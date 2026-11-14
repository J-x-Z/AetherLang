//! Canonical enum payload types, pattern binding, and Result propagation.
use super::*;
impl SemanticAnalyzer {
    pub(super) fn collect_enum_facts(&mut self, program: &Program) -> Result<()> {
        for item in &program.items {
            let Item::Enum(def) = item else {
                continue;
            };
            if !def.generic_params.is_empty() || !def.type_params.is_empty() {
                continue;
            }
            let mut names = HashSet::new();
            let mut variants = vec![];
            for variant in &def.variants {
                if !names.insert(variant.name.name.clone()) {
                    return Err(Error::DuplicateDefinition {
                        name: variant.name.name.clone(),
                        span: variant.span,
                    });
                }
                let fields = variant
                    .fields
                    .iter()
                    .map(|ty| self.resolve_type(ty))
                    .collect::<Result<Vec<_>>>()?;
                let ret = ResolvedType::Enum {
                    name: def.name.name.clone(),
                };
                let (kind, ty) = if fields.is_empty() {
                    (SymbolKind::Variable, ret)
                } else {
                    (
                        SymbolKind::Function {
                            params: fields.clone(),
                            ret: ret.clone(),
                            type_params: vec![],
                            const_params: vec![],
                            effects: EffectSet {
                                is_pure: true,
                                effects: vec![],
                            },
                        },
                        ResolvedType::Function {
                            params: fields.clone(),
                            ret: Box::new(ret),
                        },
                    )
                };
                self.symbols.define(Symbol {
                    name: format!("{}::{}", def.name.name, variant.name.name),
                    kind,
                    ty,
                    span: variant.span,
                    mutable: false,
                })?;
                variants.push((variant.name.name.clone(), fields));
            }
            self.facts
                .enum_variants
                .insert(def.name.name.clone(), variants);
        }
        Ok(())
    }
    pub(super) fn result_payloads(
        &self,
        ty: &ResolvedType,
        span: Span,
    ) -> Result<(usize, ResolvedType, usize, ResolvedType)> {
        let ResolvedType::Enum { name } = ty else {
            return Err(Error::semantic_expected(
                "E2104",
                "Result enum with one-payload Ok and Err variants",
                span,
            ));
        };
        let variants = self
            .facts
            .enum_variants
            .get(name)
            .ok_or_else(|| Error::semantic_expected("E2104", "concrete Result enum", span))?;
        if variants.len() != 2 {
            return Err(Error::semantic_expected(
                "E2104",
                "Result enum with exactly Ok and Err variants",
                span,
            ));
        }
        let ok = variants
            .iter()
            .enumerate()
            .find(|(_, (name, fields))| name == "Ok" && fields.len() == 1);
        let err = variants
            .iter()
            .enumerate()
            .find(|(_, (name, fields))| name == "Err" && fields.len() == 1);
        match (ok, err) {
            (Some((oi, (_, of))), Some((ei, (_, ef)))) => {
                Ok((oi, of[0].clone(), ei, ef[0].clone()))
            }
            _ => Err(Error::semantic_expected(
                "E2104",
                "Result enum with one-payload Ok and Err variants",
                span,
            )),
        }
    }
    pub(super) fn bind_pattern(
        &mut self,
        pattern: &Pattern,
        expected: &ResolvedType,
    ) -> Result<()> {
        match pattern {
            Pattern::Wildcard { .. } => Ok(()),
            Pattern::Binding {
                name,
                mutable,
                span,
            } => self.symbols.define(Symbol {
                name: name.name.clone(),
                kind: SymbolKind::Variable,
                ty: expected.clone(),
                span: *span,
                mutable: *mutable,
            }),
            Pattern::Literal(literal) => {
                self.check_expected_expr(&Expr::Literal(literal.clone()), expected)?;
                Ok(())
            }
            Pattern::Variant {
                enum_name,
                variant,
                fields,
                span,
            } => {
                let ResolvedType::Enum { name } = expected else {
                    return Err(Error::semantic_expected(
                        "E2104",
                        "enum scrutinee for variant pattern",
                        *span,
                    ));
                };
                if enum_name
                    .as_ref()
                    .is_some_and(|specified| &specified.name != name)
                {
                    return Err(Error::semantic_expected(
                        "E2104",
                        format!("variant of enum '{name}'"),
                        *span,
                    ));
                }
                let payload = self
                    .facts
                    .enum_variants
                    .get(name)
                    .and_then(|variants| variants.iter().find(|(name, _)| name == &variant.name))
                    .map(|(_, fields)| fields.clone())
                    .ok_or_else(|| Error::UndefinedVariable {
                        name: format!("{}::{}", name, variant.name),
                        span: *span,
                    })?;
                if payload.len() != fields.len() {
                    return Err(Error::ArgCountMismatch {
                        func_name: format!("{}::{}", name, variant.name),
                        expected: payload.len(),
                        got: fields.len(),
                        span: *span,
                    });
                }
                for (pattern, ty) in fields.iter().zip(&payload) {
                    self.bind_pattern(pattern, ty)?;
                }
                Ok(())
            }
            _ => Err(Error::semantic_expected(
                "E2104",
                "literal, binding, wildcard, or enum variant pattern",
                pattern_span(pattern),
            )),
        }
    }
    /// Pattern-matrix specialization keeps correlations between payload
    /// columns. Checking each nested variant independently would incorrectly
    /// accept Pair(true, _) | Pair(_, true), which misses Pair(false, false).
    pub(super) fn match_exhaustive(&self, ty: &ResolvedType, arms: &[MatchArm]) -> bool {
        let rows = arms
            .iter()
            .filter(|arm| arm.guard.is_none())
            .map(|arm| vec![Some(&arm.pattern)])
            .collect::<Vec<_>>();
        self.exhaustive_matrix(&rows, std::slice::from_ref(ty))
    }
    fn exhaustive_matrix(&self, rows: &[Vec<Option<&Pattern>>], types: &[ResolvedType]) -> bool {
        if types.is_empty() {
            return !rows.is_empty();
        }
        if rows
            .iter()
            .any(|row| row.iter().all(|pattern| pattern.is_none_or(irrefutable)))
        {
            return true;
        }
        let wildcard = |pattern: Option<&Pattern>| pattern.is_none_or(irrefutable);
        match &types[0] {
            ResolvedType::Enum { name } => {
                let Some(variants) = self.facts.enum_variants.get(name) else { return false; };
                variants.iter().all(|(name, payload)| {
                    let specialized = rows.iter().filter_map(|row| {
                        let mut fields = if wildcard(row[0]) { vec![None; payload.len()] }
                        else if let Some(Pattern::Variant { variant, fields, .. }) = row[0] {
                            if variant.name != *name { return None; }
                            fields.iter().map(Some).collect()
                        } else { return None; };
                        fields.extend_from_slice(&row[1..]);
                        Some(fields)
                    }).collect::<Vec<_>>();
                    let mut remaining = payload.clone(); remaining.extend_from_slice(&types[1..]);
                    self.exhaustive_matrix(&specialized, &remaining)
                })
            }
            ty if *ty == ResolvedType::BOOL => [false, true].iter().all(|value| {
                let specialized = rows.iter().filter(|row| {
                    wildcard(row[0]) || matches!(row[0], Some(Pattern::Literal(Literal::Bool(found, _))) if found == value)
                }).map(|row| row[1..].to_vec()).collect::<Vec<_>>();
                self.exhaustive_matrix(&specialized, &types[1..])
            }),
            _ => {
                // Scalar literal lists do not establish coverage of an open
                // domain; an unguarded wildcard/binding must cover the rest.
                let defaults = rows.iter().filter(|row| wildcard(row[0]))
                    .map(|row| row[1..].to_vec()).collect::<Vec<_>>();
                self.exhaustive_matrix(&defaults, &types[1..])
            }
        }
    }
    pub(super) fn check_try(&mut self, expr: &Expr, span: Span) -> Result<ResolvedType> {
        let operand = self.check_expr(expr)?;
        if operand == ResolvedType::NEVER {
            return Ok(operand);
        }
        let (_, ok, _, error) = self.result_payloads(&operand, span)?;
        let returned = self.current_return.clone().ok_or_else(|| {
            Error::semantic_expected("E2104", "Result-returning function for '?'", span)
        })?;
        let (_, _, _, return_error) = self.result_payloads(&returned, span)?;
        self.require_type(&return_error, &error, span)?;
        Ok(ok)
    }
}
pub(super) fn irrefutable(pattern: &Pattern) -> bool {
    matches!(pattern, Pattern::Wildcard { .. } | Pattern::Binding { .. })
}
fn pattern_span(pattern: &Pattern) -> Span {
    match pattern {
        Pattern::Wildcard { span }
        | Pattern::Binding { span, .. }
        | Pattern::Struct { span, .. }
        | Pattern::Tuple { span, .. }
        | Pattern::Variant { span, .. } => *span,
        Pattern::Literal(literal) => literal.span(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::{lexer::Lexer, parser::Parser};
    fn check(source: &str) -> Result<()> {
        let program = Parser::new(Lexer::new(source, 0)).parse_program()?;
        SemanticAnalyzer::new().analyze(&program)
    }
    #[test]
    fn enum_construction_and_exhaustive_payload_binding() {
        let valid="enum E {A(i32),B} fn f(x:E)->i32 {match x {E::A(value)=>value,E::B=>0}} fn g()->i32{f(E::A(7))}";
        assert!(check(valid).is_ok(), "{:?}", check(valid));
        for invalid in [
            "enum E {A(i32)} fn f(){E::A(true);}",
            "enum E {A(i32),B} fn f(x:E)->i32{match x {E::A(v)=>v}}",
            "enum E {A(i32)} fn f(x:E)->i32{match x {E::A()=>0}}",
            "enum E {A(i32)} fn f(){E::Missing;}",
            "enum E {A(E)}",
        ] {
            assert!(check(invalid).is_err(), "{invalid}");
        }
    }
    #[test]
    fn nested_pattern_matrix_preserves_payload_correlations() {
        let prefix = "enum I { A(i32), B } enum E { Wrap(I), Pair(bool,bool), Empty } fn f(e:E)->i32{match e{";
        let full = format!("{prefix}E::Wrap(I::A(x))=>x,E::Wrap(I::B)=>0,E::Pair(true,_)=>1,E::Pair(false,true)=>2,E::Pair(false,false)=>3,E::Empty=>4}}}}");
        assert!(check(&full).is_ok(), "{:?}", check(&full));
        for arms in [
            "E::Wrap(I::A(x))=>x,E::Pair(_,_)=>0,E::Empty=>0",
            "E::Wrap(_)=>0,E::Pair(true,_)=>1,E::Pair(_,true)=>2,E::Empty=>0",
            "E::Wrap(_)=>0,E::Pair(true,_)=>1,E::Pair(false,_) if true=>2,E::Empty=>0",
        ] {
            assert!(check(&format!("{prefix}{arms}}}}}")).is_err(), "{arms}");
        }
    }
    #[test]
    fn array_references_index_elements_and_preserve_mutability() {
        for source in [
            "fn sum(a:&[i32;2])->i32{a[0]+a[1]}",
            "fn sum(a:& &[i32;2])->i32{a[0]+a[1]}",
            "fn set(a:&mut [i32;2]){a[0]=7;}",
        ] {
            assert!(check(source).is_ok(), "{source}: {:?}", check(source));
        }
        assert!(check("fn set(a:&[i32;2]){a[0]=7;}").is_err());
        assert!(check("fn bad()->&i32{let a:[i32;2]=[1,2];let r:&[i32;2]=&a;&r[0]}").is_err());
    }
    #[test]
    fn result_propagation_checks_error_type_and_preserves_ok_type() {
        let valid="enum R {Ok(i32),Err(i64)} enum S {Ok(bool),Err(i64)} fn f(x:R)->S {let value:i32=x?;S::Ok(value>0)}";
        assert!(check(valid).is_ok(), "{:?}", check(valid));
        for invalid in [
            "enum R {Ok(i32),Err(i64)} fn f(x:R)->i32{x?}",
            "enum R {Ok(i32),Err(i64)} enum S {Ok(i32),Err(bool)} fn f(x:R)->S{S::Ok(x?)}",
            "enum R {Ok(i32),Err(i64),Other} fn f(x:R)->R{R::Ok(x?)}",
        ] {
            assert!(check(invalid).is_err(), "{invalid}");
        }
    }
    #[test]
    fn enum_references_obey_payload_lifetimes() {
        let valid="enum E {Some(&i32),None} fn f(x:&i32)->E{E::Some(x)} fn g(x:E)->i32{match x {E::Some(r)=>*r,E::None=>0}}";
        assert!(check(valid).is_ok(), "{:?}", check(valid));
        assert!(check("enum E {Some(&i32),None} fn f()->E{let x:i32=1;E::Some(&x)}").is_err());
        assert!(check("enum E {Some(&i32),None} fn f()->&i32{let x:i32=1;let e:E=E::Some(&x);match e {E::Some(r)=>r,E::None=>loop{}}}").is_err());
    }
    #[test]
    fn existing_methods_have_checked_signatures_and_provenance() {
        let valid =
            "struct S {x:i32} impl S {fn get(self:&Self)->&i32{&self.x}} fn f(x:&S)->&i32{x.get()}";
        assert!(check(valid).is_ok(), "{:?}", check(valid));
        assert!(check("struct S{x:i32} impl S {fn get(self:&Self)->&i32{&self.x}} fn f()->&i32{let s:S=S{x:1};s.get()}").is_err());
        assert!(check("struct S{x:i32} fn f(s:S){s.missing();}").is_err());
    }
    #[test]
    fn nominal_type_names_are_not_runtime_values() {
        for source in [
            "struct S{x:i32} fn f()->S{S}",
            "enum E{A} fn f()->E{E}",
            "type T=i32;fn f()->T{T}",
        ] {
            let program = Parser::new(Lexer::new(source, 0)).parse_program().unwrap();
            assert!(
                SemanticAnalyzer::new().analyze(&program).is_err(),
                "{source}"
            );
        }
    }
}
