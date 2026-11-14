//! Native vector operations with IEEE floating semantics and explicit lane bounds.
use super::*;
use crate::intrinsics::VectorKind as K;
use llvm_sys::LLVMRealPredicate as F;
impl LLVMCodeGen {
    pub(super) unsafe fn generate_vector(
        &mut self,
        dest: Option<Register>,
        kind: K,
        ty: &IRType,
        args: &[Value],
    ) -> Result<()> {
        let IRType::Vector(element, lanes) = ty else {
            unreachable!()
        };
        let values = args
            .iter()
            .map(|v| self.get_value(v))
            .collect::<Result<Vec<_>>>()?;
        let native = self.ir_type_to_llvm(ty);
        let scalar = self.ir_type_to_llvm(element);
        let floating = matches!(element.as_ref(), IRType::F32 | IRType::F64);
        let n = c"".as_ptr();
        let b = self.builder;
        let index = |lane| LLVMConstInt(LLVMInt32TypeInContext(self.context), lane as u64, 0);
        let value = match kind {
            K::New | K::Splat => {
                let mut result = LLVMConstNull(native);
                for lane in 0..*lanes {
                    result = LLVMBuildInsertElement(
                        b,
                        result,
                        values[if kind == K::Splat { 0 } else { lane }],
                        index(lane),
                        n,
                    );
                }
                result
            }
            K::Load => {
                let value = LLVMBuildLoad2(b, native, values[0], n);
                LLVMSetAlignment(value, 1);
                value
            }
            K::Store => {
                let store = LLVMBuildStore(b, values[1], values[0]);
                LLVMSetAlignment(store, 1);
                return Ok(());
            }
            K::Extract => {
                let invalid = LLVMBuildICmp(
                    b,
                    LLVMIntPredicate::LLVMIntUGE,
                    values[1],
                    LLVMConstInt(LLVMTypeOf(values[1]), *lanes as u64, 0),
                    n,
                );
                let function = self.current_function.unwrap();
                let trap_block =
                    LLVMAppendBasicBlockInContext(self.context, function, c"lane.trap".as_ptr());
                let valid =
                    LLVMAppendBasicBlockInContext(self.context, function, c"lane.valid".as_ptr());
                LLVMBuildCondBr(b, invalid, trap_block, valid);
                LLVMPositionBuilderAtEnd(b, trap_block);
                let trap_type =
                    LLVMFunctionType(LLVMVoidTypeInContext(self.context), ptr::null_mut(), 0, 0);
                let mut trap = LLVMGetNamedFunction(self.module, c"llvm.trap".as_ptr());
                if trap.is_null() {
                    trap = LLVMAddFunction(self.module, c"llvm.trap".as_ptr(), trap_type);
                }
                LLVMBuildCall2(b, trap_type, trap, ptr::null_mut(), 0, n);
                LLVMBuildUnreachable(b);
                LLVMPositionBuilderAtEnd(b, valid);
                LLVMBuildExtractElement(b, values[0], values[1], n)
            }
            K::Sum => {
                let mut sum = LLVMBuildExtractElement(b, values[0], index(0), n);
                for lane in 1..*lanes {
                    let next = LLVMBuildExtractElement(b, values[0], index(lane), n);
                    sum = if floating {
                        LLVMBuildFAdd(b, sum, next, n)
                    } else {
                        LLVMBuildAdd(b, sum, next, n)
                    };
                }
                sum
            }
            K::All | K::Any => {
                let mut result = LLVMConstInt(
                    LLVMInt1TypeInContext(self.context),
                    (kind == K::All) as u64,
                    0,
                );
                for lane in 0..*lanes {
                    let item = LLVMBuildExtractElement(b, values[0], index(lane), n);
                    let truth = LLVMBuildICmp(
                        b,
                        LLVMIntPredicate::LLVMIntNE,
                        item,
                        LLVMConstNull(scalar),
                        n,
                    );
                    result = if kind == K::All {
                        LLVMBuildAnd(b, result, truth, n)
                    } else {
                        LLVMBuildOr(b, result, truth, n)
                    };
                }
                result
            }
            K::Add => {
                if floating {
                    LLVMBuildFAdd(b, values[0], values[1], n)
                } else {
                    LLVMBuildAdd(b, values[0], values[1], n)
                }
            }
            K::Sub => {
                if floating {
                    LLVMBuildFSub(b, values[0], values[1], n)
                } else {
                    LLVMBuildSub(b, values[0], values[1], n)
                }
            }
            K::Mul => {
                if floating {
                    LLVMBuildFMul(b, values[0], values[1], n)
                } else {
                    LLVMBuildMul(b, values[0], values[1], n)
                }
            }
            K::Div => LLVMBuildFDiv(b, values[0], values[1], n),
            _ => {
                let comparison = if floating {
                    let op = match kind {
                        K::Eq => F::LLVMRealOEQ,
                        K::Ne => F::LLVMRealUNE,
                        K::Lt => F::LLVMRealOLT,
                        K::Le => F::LLVMRealOLE,
                        K::Gt => F::LLVMRealOGT,
                        K::Ge => F::LLVMRealOGE,
                        _ => unreachable!(),
                    };
                    LLVMBuildFCmp(b, op, values[0], values[1], n)
                } else {
                    let op = match kind {
                        K::Eq => LLVMIntPredicate::LLVMIntEQ,
                        K::Ne => LLVMIntPredicate::LLVMIntNE,
                        K::Lt => LLVMIntPredicate::LLVMIntSLT,
                        K::Le => LLVMIntPredicate::LLVMIntSLE,
                        K::Gt => LLVMIntPredicate::LLVMIntSGT,
                        K::Ge => LLVMIntPredicate::LLVMIntSGE,
                        _ => unreachable!(),
                    };
                    LLVMBuildICmp(b, op, values[0], values[1], n)
                };
                let mask = LLVMVectorType(
                    if matches!(element.as_ref(), IRType::F64 | IRType::I64) {
                        LLVMInt64TypeInContext(self.context)
                    } else {
                        LLVMInt32TypeInContext(self.context)
                    },
                    *lanes as u32,
                );
                LLVMBuildSExt(b, comparison, mask, n)
            }
        };
        if let Some(dest) = dest {
            self.value_map.insert(dest, value);
        }
        Ok(())
    }
}
