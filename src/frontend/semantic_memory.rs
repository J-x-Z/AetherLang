//! Memory intrinsics use the same inventory and ordering rules as lowering.
use super::*;
use crate::intrinsics::{self, MemoryKind as K, MemoryOrder};
impl SemanticAnalyzer {
    pub(super) fn register_memory_intrinsics(&mut self) {
        for name in intrinsics::memory_names() {
            let spec = intrinsics::memory(&name).expect("intrinsic inventory must resolve");
            let effects = match spec.kind {
                K::VolatileRead | K::VolatileWrite => vec![Effect::IO],
                K::AtomicLoad => vec![Effect::Read],
                K::AtomicStore => vec![Effect::Write],
                K::FetchAdd | K::FetchSub | K::CompareExchange | K::Fence => {
                    vec![Effect::Read, Effect::Write]
                }
            };
            self.define_builtin_with_effects(
                &name,
                spec.params(),
                spec.result(),
                EffectSet {
                    is_pure: false,
                    effects,
                },
            );
        }
    }
    pub(super) fn validate_memory_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<()> {
        let Some(spec) = intrinsics::memory(name) else {
            return Ok(());
        };
        if !spec.explicit_order {
            self.facts
                .memory_orders
                .insert(span, (MemoryOrder::SeqCst, MemoryOrder::SeqCst));
            return Ok(());
        }
        let count = if spec.kind == K::CompareExchange {
            2
        } else {
            1
        };
        if args.len() < count {
            return Err(Error::semantic_expected(
                "E2201",
                "memory ordering arguments",
                span,
            ));
        }
        let mut orders = vec![];
        for arg in &args[args.len() - count..] {
            let value=self.evaluate_scalar(arg,Some(&ResolvedType::I32),&mut HashSet::new()).map_err(|_|Error::semantic_expected("E2201","compile-time integer memory order (0 relaxed, 1 acquire, 2 release, 3 acq_rel, 4 seq_cst)",arg.span()))?;
            let ScalarConstant::Int(value) = value.value else {
                return Err(Error::semantic_expected(
                    "E2201",
                    "integer memory order",
                    arg.span(),
                ));
            };
            orders.push(MemoryOrder::from_i64(value).ok_or_else(|| {
                Error::semantic_expected("E2201", "memory order between 0 and 4", arg.span())
            })?);
        }
        let success = orders[0];
        let failure = *orders.get(1).unwrap_or(&success);
        intrinsics::validate_orders(spec.kind, success, failure)
            .map_err(|message| Error::semantic_expected("E2201", message, span))?;
        self.facts.memory_orders.insert(span, (success, failure));
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::{lexer::Lexer, parser::Parser};
    fn check(source: &str) -> Result<()> {
        let program = Parser::new(Lexer::new(source, 0)).parse_program().unwrap();
        SemanticAnalyzer::new().analyze(&program)
    }
    #[test]
    fn shared_intrinsic_signatures_and_memory_orders_are_checked() {
        assert!(check(
            "const ACQUIRE:i32=1;fn f(p:*u32)->u32{__atomic_load_u32_explicit(p,ACQUIRE)}"
        )
        .is_ok());
        assert!(
            check("fn f(p:*u32)->bool{__atomic_compare_exchange_u32_explicit(p,1,2,3,1)}").is_ok()
        );
        for source in [
            "fn f(p:*u32)->u32{__atomic_load_u32_explicit(p,2)}",
            "fn f(p:*u32){__atomic_store_u32_explicit(p,1,1);}",
            "fn f(p:*u32)->bool{__atomic_compare_exchange_u32_explicit(p,1,2,0,1)}",
            "fn f(p:*u32,order:i32)->u32{__atomic_load_u32_explicit(p,order)}",
            "fn f(p:*u64)->u32{__atomic_load_u32(p)}",
            "fn f(){__atomic_thread_fence_explicit(0);}",
            "fn f(p:*u32)->u32 pure {__atomic_load_u32(p)}",
        ] {
            assert!(check(source).is_err(), "{source}");
        }
    }
}
