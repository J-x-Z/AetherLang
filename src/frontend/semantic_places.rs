//! Memory-place facts captured while bindings and projection types are in scope.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceOrigin { Local, Global, Indirect }

#[derive(Debug, Clone)]
pub struct ResolvedPlace {
    pub ty: ResolvedType,
    pub origin: PlaceOrigin,
    /// Additional implicit pointer loads needed to compute this address.
    pub address_reads: bool,
}

impl SemanticAnalyzer {
    pub(super) fn remember_place(&mut self, expr: &Expr, ty: &ResolvedType) {
        let Some(function) = &self.current_function_name else { return; };
        let (origin, address_reads) = match expr {
            Expr::Ident(name) => {
                let global = self.facts.global_types.contains_key(&name.name)
                    && self.symbols.lookup(&name.name).zip(self.symbols.scopes[0].symbols.get(&name.name))
                        .is_some_and(|(resolved, global)| std::ptr::eq(resolved, global));
                (if global { PlaceOrigin::Global } else { PlaceOrigin::Local }, false)
            }
            Expr::Deref { .. } => (PlaceOrigin::Indirect, false),
            Expr::Field { expr: base, .. } | Expr::Index { expr: base, .. } => {
                let Some(base_place) = self.facts.places.get(&(function.clone(), base.span())) else { return; };
                let mut base_ty = base_place.ty.unqualified();
                let mut dereferences = 0;
                if matches!(expr, Expr::Field { .. }) {
                    while let ResolvedType::Pointer(inner) | ResolvedType::Reference { inner, .. } = base_ty {
                        dereferences += 1;
                        base_ty = inner.unqualified();
                    }
                } else {
                    while let ResolvedType::Reference { inner, .. } = base_ty {
                        dereferences += 1;
                        base_ty = inner.unqualified();
                    }
                    if matches!(base_ty, ResolvedType::Pointer(_) | ResolvedType::Slice(_)) { dereferences += 1; }
                }
                (if dereferences > 0 { PlaceOrigin::Indirect } else { base_place.origin }, dereferences > 1)
            }
            // An rvalue can be a projection base, e.g. get_ref().field.
            _ => (PlaceOrigin::Local, false),
        };
        self.facts.places.insert((function.clone(), expr.span()), ResolvedPlace { ty: ty.clone(), origin, address_reads });
    }
}
