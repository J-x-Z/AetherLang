//! Native LLVM memory operations. Ordering was validated before IR generation.
use super::*;
use crate::intrinsics::{MemoryKind as K, MemoryOrder as O};
use llvm_sys::{LLVMAtomicOrdering as A, LLVMAtomicRMWBinOp};

fn ordering(order: O) -> A {
    match order {
        O::Relaxed => A::LLVMAtomicOrderingMonotonic,
        O::Acquire => A::LLVMAtomicOrderingAcquire,
        O::Release => A::LLVMAtomicOrderingRelease,
        O::AcqRel => A::LLVMAtomicOrderingAcquireRelease,
        O::SeqCst => A::LLVMAtomicOrderingSequentiallyConsistent,
    }
}
impl LLVMCodeGen {
    pub(super) unsafe fn generate_memory(
        &mut self,
        dest: Option<Register>,
        kind: K,
        ty: &IRType,
        args: &[Value],
        order: O,
        failure: O,
    ) -> Result<()> {
        let values = args
            .iter()
            .map(|v| self.get_value(v))
            .collect::<Result<Vec<_>>>()?;
        let element = self.ir_type_to_llvm(ty);
        if matches!(kind, K::VolatileRead | K::VolatileWrite)
            && LLVMABISizeOfType(self.target_data, element) == 0
        {
            if let Some(dest) = dest {
                self.value_map.insert(dest, LLVMConstNull(element));
            }
            return Ok(());
        }
        let alignment = LLVMABIAlignmentOfType(self.target_data, element);
        let value = match kind {
            K::VolatileRead | K::AtomicLoad => {
                let load = LLVMBuildLoad2(self.builder, element, values[0], c"".as_ptr());
                if kind == K::VolatileRead {
                    LLVMSetVolatile(load, 1);
                    LLVMSetAlignment(load, 1);
                } else {
                    LLVMSetOrdering(load, ordering(order));
                    LLVMSetAlignment(load, alignment);
                }
                Some(load)
            }
            K::VolatileWrite | K::AtomicStore => {
                let store = LLVMBuildStore(self.builder, values[1], values[0]);
                if kind == K::VolatileWrite {
                    LLVMSetVolatile(store, 1);
                    LLVMSetAlignment(store, 1);
                } else {
                    LLVMSetOrdering(store, ordering(order));
                    LLVMSetAlignment(store, alignment);
                }
                None
            }
            K::FetchAdd | K::FetchSub => {
                let op = if kind == K::FetchAdd {
                    LLVMAtomicRMWBinOp::LLVMAtomicRMWBinOpAdd
                } else {
                    LLVMAtomicRMWBinOp::LLVMAtomicRMWBinOpSub
                };
                let instruction =
                    LLVMBuildAtomicRMW(self.builder, op, values[0], values[1], ordering(order), 0);
                LLVMSetAlignment(instruction, alignment);
                Some(instruction)
            }
            K::CompareExchange => {
                let instruction = LLVMBuildAtomicCmpXchg(
                    self.builder,
                    values[0],
                    values[1],
                    values[2],
                    ordering(order),
                    ordering(failure),
                    0,
                );
                LLVMSetWeak(instruction, 0);
                LLVMSetAlignment(instruction, alignment);
                Some(LLVMBuildExtractValue(
                    self.builder,
                    instruction,
                    1,
                    c"".as_ptr(),
                ))
            }
            K::Fence => {
                LLVMBuildFence(self.builder, ordering(order), 0, c"".as_ptr());
                None
            }
        };
        if let (Some(dest), Some(value)) = (dest, value) {
            self.value_map.insert(dest, value);
        }
        Ok(())
    }
}
