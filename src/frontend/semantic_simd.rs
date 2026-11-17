//! Canonical SIMD types, effects and explicit raw-memory access.
use super::*;
use crate::intrinsics::{self, VectorKind as K};
impl SemanticAnalyzer {
    pub(super) fn register_vector_intrinsics(&mut self) {
        for name in intrinsics::vector_names() {
            let spec = intrinsics::vector(&name).unwrap();
            let effects = match spec.kind {
                K::Load => vec![Effect::Read],
                K::Store => vec![Effect::Write],
                _ => vec![],
            };
            self.define_builtin_with_effects(
                &name,
                spec.params(self.facts.pointer_width),
                spec.result(),
                EffectSet {
                    is_pure: effects.is_empty(),
                    effects,
                },
            );
        }
    }
    pub(super) fn validate_vector_call(&self, name: &str, span: Span) -> Result<()> {
        if let Some(spec) = intrinsics::vector(name) {
            if matches!(spec.kind, K::Load | K::Store) && self.unsafe_depth == 0 {
                return Err(Error::semantic_expected(
                    "E2401",
                    "unsafe block for SIMD pointer load/store",
                    span,
                ));
            }
        }
        Ok(())
    }
}
