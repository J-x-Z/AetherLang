//! GCC/Clang atomic builtins operate directly on the original typed object.
use super::*;
use crate::intrinsics::{MemoryKind as K, MemoryOrder};
impl CCodeGen {
    pub(super) fn generate_memory(
        &mut self,
        dest: Option<Register>,
        kind: K,
        ty: &IRType,
        args: &[Value],
        order: MemoryOrder,
        failure: MemoryOrder,
    ) {
        let args = args.iter().map(|v| self.value_to_c(v)).collect::<Vec<_>>();
        let ty = self.ir_type_to_c(ty);
        let order = order.c_name();
        if matches!(kind, K::VolatileRead | K::VolatileWrite) {
            // A packed field can have alignment one. Preserve access volatility
            // without creating a misaligned C lvalue of the bare element type.
            let place = format!(
                "((volatile struct __attribute__((packed)) {{ {ty} value; }}*)({}))->value",
                args[0]
            );
            if let Some(dest) = dest {
                let var = self.get_var(dest);
                self.writeln(&format!(
                    "if (sizeof({ty}) != 0) {{ {var} = {place}; }} else {{ {var} = ({ty}){{}}; }}"
                ));
            } else {
                self.writeln(&format!(
                    "if (sizeof({ty}) != 0) {{ {place} = {}; }}",
                    args[1]
                ));
            }
            return;
        }
        let expression = match kind {
            K::VolatileRead => format!("*((volatile {ty}*)({}))", args[0]),
            K::VolatileWrite => format!("*((volatile {ty}*)({})) = {}", args[0], args[1]),
            K::AtomicLoad => format!("__atomic_load_n(({}*)({}),{})", ty, args[0], order),
            K::AtomicStore => format!(
                "__atomic_store_n(({}*)({}),{},{})",
                ty, args[0], args[1], order
            ),
            K::FetchAdd | K::FetchSub => format!(
                "__atomic_fetch_{}(({}*)({}),{},{})",
                if kind == K::FetchAdd { "add" } else { "sub" },
                ty,
                args[0],
                args[1],
                order
            ),
            K::CompareExchange => {
                // Expected is an input value in Aether; do not mutate caller state.
                let var = self.get_var(dest.expect("CAS result"));
                self.writeln(&format!("{{ {ty} expected = {}; {var} = __atomic_compare_exchange_n(({ty}*)({}), &expected, {}, 0, {order}, {}); }}",args[1],args[0],args[2],failure.c_name()));
                return;
            }
            K::Fence => format!("__atomic_thread_fence({order})"),
        };
        if let Some(dest) = dest {
            let var = self.get_var(dest);
            self.writeln(&format!("{var} = {expression};"));
        } else {
            self.writeln(&format!("{expression};"));
        }
    }
}
