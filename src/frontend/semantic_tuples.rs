//! Tuple values are structural language aggregates, not a promised C ABI.
use super::*;

impl SemanticAnalyzer {
    fn contains_value_tuple(&self, ty: &ResolvedType, seen: &mut HashSet<String>) -> bool {
        match ty {
            ResolvedType::Tuple(elements) => !elements.is_empty(),
            ResolvedType::Array { elem, .. } | ResolvedType::Volatile(elem) => self.contains_value_tuple(elem, seen),
            ResolvedType::Struct { name, .. } if seen.insert(name.clone()) => {
                self.symbols.scopes[0].symbols.get(name).is_some_and(|symbol| {
                    matches!(&symbol.kind, SymbolKind::Struct { fields, .. } if fields.iter().any(|(_, ty)| self.contains_value_tuple(ty, seen)))
                })
            }
            ResolvedType::Enum { name } if seen.insert(name.clone()) => self.facts.enum_variants.get(name)
                .is_some_and(|variants| variants.iter().any(|(_, fields)| fields.iter().any(|ty| self.contains_value_tuple(ty, seen)))),
            // Pointers may refer to opaque Aether-owned storage.
            _ => false,
        }
    }

    fn check_tuple_c_type(&self, ty: &Type) -> Result<()> {
        self.check_tuple_abi_type(&self.resolve_type(ty)?, ty.span())
    }

    pub(super) fn check_tuple_abi_type(&self, ty: &ResolvedType, span: Span) -> Result<()> {
        if self.contains_value_tuple(ty, &mut HashSet::new()) {
            return Err(Error::Semantic { code: "E2404", span,
                message: "tuple values have no stable C ABI; use a named repr(C) struct with C-compatible fields".into() });
        }
        Ok(())
    }

    pub(super) fn check_tuple_abi_boundaries(&self, program: &Program) -> Result<()> {
        for item in &program.items {
            match item {
                Item::Extern(block) => {
                    for item in &block.items {
                        match item {
                            ForeignItem::Fn {
                                params, ret_type, ..
                            } => {
                                for param in params {
                                    self.check_tuple_c_type(&param.ty)?;
                                }
                                if let Some(ty) = ret_type {
                                    self.check_tuple_c_type(ty)?;
                                }
                            }
                            ForeignItem::Static { ty, .. } => self.check_tuple_c_type(ty)?,
                        }
                    }
                }
                Item::Function(function)
                    if function
                        .annotations
                        .iter()
                        .any(|a| a.name.name == "no_mangle") =>
                {
                    for param in &function.params {
                        self.check_tuple_c_type(&param.ty)?;
                    }
                    if let Some(ty) = &function.ret_type {
                        self.check_tuple_c_type(ty)?;
                    }
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

    fn check(source: &str) -> Result<()> {
        let program = Parser::new(Lexer::new(source, 0)).parse_program()?;
        SemanticAnalyzer::new().analyze(&program)
    }

    #[test]
    fn tuples_have_structural_types_and_indexed_places() {
        check("fn take(value: &(i32, (bool, i8))) -> i8 { value.1.1 } fn main() { let mut pair: (i32, (bool, i8)) = (7, (true, 2)); pair.1.1 = 3; let x: i8 = take(&pair); }").unwrap();
        check("fn main() { let pair: (i32, (i8, bool)); pair.0 = 1; pair.1.0 = 2; pair.1.1 = true; let copy: (i32, (i8, bool)) = pair; let x: i32 = (1 as i32, true).0; }").unwrap();
        check("fn main() { let x: i32 = ((7 as i32,),).0.0; let y: i32 = ([1 as i32],).0[0]; }")
            .unwrap();
        check("fn main() { let pair: (i32,) = (7,); let first: &(i32,) = &pair; let outer: & &(i32,) = &first; let value: i32 = outer.0; }").unwrap();
        for source in [
            "fn main() { let t: (i32, bool) = (1, true); t.2; }",
            "fn main() { let t: (i32, bool) = (true, 1); }",
            "fn main() { let t: (i32,) = (1, 2); }",
            "fn main() { let t: (i32,) = (1,); t.0 = 2; }",
            "fn bad() -> (&i32,) { let x: i32 = 1; (&x,) }",
            "fn bad(value: &(i32,)) { value.0 = 2; }",
            "fn bad() -> &i32 { let value: (i32,) = (1,); &value.0 }",
            "fn bad() { let mut value: i32 = 1; let refs: (&i32,) = (&value,); value = 2; let x: i32 = *refs.0; }",
            "fn bad(value: &(i32,)) -> &mut i32 { &mut value.0 }",
            "fn bad() { let pair: (i32, i32); pair.0 = 1; let copy: (i32, i32) = pair; }",
            "fn bad() { let mut value: i32 = 1; let first: &i32 = &value; let outer: & &i32 = &first; value = 2; let used: i32 = **outer; }",
        ] {
            assert!(check(source).is_err(), "accepted: {source}");
        }
    }

    #[test]
    fn unsupported_function_values_are_rejected_during_check() {
        for source in [
            "fn main() { |x: i64| -> i64 { x + 1 }; }",
            "fn call(f: fn(i32) -> i32) -> i32 { f(1) }",
        ] {
            assert!(
                matches!(check(source), Err(Error::Semantic { code: "E2403", .. })),
                "{source}: {:?}",
                check(source)
            );
        }
        for source in [
            "extern \"C\" { fn take(x: (i32, i32)); }",
            "#[no_mangle] fn pair() -> (i32, i32) { (1, 2) }",
            "type Pair = (i32, i32); struct Wrap { pair: Pair } extern \"C\" { fn take(x: Wrap); }",
        ] {
            assert!(
                matches!(check(source), Err(Error::Semantic { code: "E2404", .. })),
                "{source}: {:?}",
                check(source)
            );
        }
    }
}
