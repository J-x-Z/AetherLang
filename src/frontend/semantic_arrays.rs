//! Repetition is a typed value operation, not parser-time AST duplication.
use super::*;
impl SemanticAnalyzer {
    pub(super) fn array_repeat_count(&mut self, count: &Expr) -> Result<usize> {
        let value = self.evaluate_scalar(
            count,
            Some(&ResolvedType::Primitive(PrimitiveType::Usize(
                self.facts.pointer_width,
            ))),
            &mut HashSet::new(),
        )?;
        let ScalarConstant::Int(bits) = value.value else {
            return Err(Error::semantic_expected(
                "E2501",
                "constant nonnegative array repeat count",
                count.span(),
            ));
        };
        usize::try_from(bits as u64).map_err(|_| {
            Error::semantic_expected(
                "E2501",
                "array repeat count representable on compiler host",
                count.span(),
            )
        })
    }
    pub(super) fn check_array_repeat(
        &mut self,
        value: &Expr,
        count: &Expr,
        span: Span,
        expected: Option<&ResolvedType>,
    ) -> Result<ResolvedType> {
        let size = self.array_repeat_count(count)?;
        let ty = if let Some(expected) = expected {
            self.check_expected_expr(value, expected)?
        } else {
            self.check_expr(value)?
        };
        if ty == ResolvedType::NEVER {
            return Ok(ty);
        }
        if self.contains_owned(&ty) {
            return Err(Error::semantic_expected(
                "E2405",
                "array elements without affine owners",
                span,
            ));
        }
        if size > 1 && self.repeat_exclusive(&ty, &mut HashSet::new()) {
            return Err(Error::semantic_expected(
                "E2302",
                "repeat element without exclusive references when count exceeds one",
                span,
            ));
        }
        Ok(ResolvedType::Array {
            elem: Box::new(ty),
            size,
        })
    }
    fn repeat_exclusive(&self, ty: &ResolvedType, seen: &mut HashSet<String>) -> bool {
        match self.refresh_nominal_type(ty.clone()) {
            ResolvedType::Reference { mutable: true, .. } | ResolvedType::Owned(_) => true,
            ResolvedType::Struct { name, fields } if seen.insert(name.clone()) => {
                fields.iter().any(|(_, t)| self.repeat_exclusive(t, seen))
            }
            ResolvedType::Enum { name } if seen.insert(name.clone()) => {
                self.facts.enum_variants.get(&name).is_some_and(|variants| {
                    variants
                        .iter()
                        .any(|(_, fields)| fields.iter().any(|t| self.repeat_exclusive(t, seen)))
                })
            }
            ResolvedType::Array { elem, .. } | ResolvedType::Volatile(elem) => {
                self.repeat_exclusive(&elem, seen)
            }
            ResolvedType::Tuple(elements) => {
                elements.iter().any(|t| self.repeat_exclusive(t, seen))
            }
            _ => false,
        }
    }
}
