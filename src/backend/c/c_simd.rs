//! Portable fixed-width vectors. Arithmetic and lane order do not depend on ISA names.
use super::*;
use crate::intrinsics::VectorKind as K;
impl CCodeGen {
    pub(super) fn generate_vector(
        &mut self,
        dest: Option<Register>,
        kind: K,
        ty: &IRType,
        args: &[Value],
    ) {
        let IRType::Vector(element, lanes) = ty else {
            unreachable!()
        };
        let values = args.iter().map(|a| self.value_to_c(a)).collect::<Vec<_>>();
        let native = self.ir_type_to_c(ty);
        let var = dest.map(|r| self.get_var(r));
        if kind == K::Load {
            self.writeln(&format!(
                "memcpy(&{}, (const void*)({}), sizeof({native}));",
                var.unwrap(),
                values[0]
            ));
            return;
        }
        if kind == K::Store {
            self.writeln(&format!(
                "memcpy((void*)({}), &({}), sizeof({native}));",
                values[0], values[1]
            ));
            return;
        }
        let result = match kind {
            K::New => format!("({native}){{{}}}", values.join(", ")),
            K::Splat => format!(
                "({native}){{{}}}",
                vec![values[0].clone(); *lanes].join(", ")
            ),
            K::Extract => {
                self.writeln(&format!(
                    "if ((uint64_t)({}) >= {}ULL) __builtin_trap();",
                    values[1], lanes
                ));
                format!("({})[{}]", values[0], values[1])
            }
            K::Sum => {
                let scalar = self.ir_type_to_c(element);
                let integer = matches!(element.as_ref(), IRType::I32 | IRType::I64);
                let intermediate = if **element == IRType::I32 {
                    "uint32_t"
                } else {
                    "uint64_t"
                };
                let mut sum = format!("({})[0]", values[0]);
                for lane in 1..*lanes {
                    sum = if integer {
                        format!(
                            "({scalar})(({intermediate})({sum}) + ({intermediate})(({})[{lane}]))",
                            values[0]
                        )
                    } else {
                        format!("({sum} + ({})[{lane}])", values[0])
                    };
                }
                sum
            }
            K::All | K::Any => (0..*lanes)
                .map(|lane| format!("(({})[{lane}] != 0)", values[0]))
                .collect::<Vec<_>>()
                .join(if kind == K::All { " && " } else { " || " }),
            _ => {
                let op = match kind {
                    K::Add => "+",
                    K::Sub => "-",
                    K::Mul => "*",
                    K::Div => "/",
                    K::Eq => "==",
                    K::Ne => "!=",
                    K::Lt => "<",
                    K::Le => "<=",
                    K::Gt => ">",
                    K::Ge => ">=",
                    _ => unreachable!(),
                };
                if matches!(kind, K::Add | K::Sub | K::Mul)
                    && matches!(element.as_ref(), IRType::I32 | IRType::I64)
                {
                    let unsigned = self.ir_type_to_c(&IRType::Vector(
                        Box::new(if **element == IRType::I32 {
                            IRType::U32
                        } else {
                            IRType::U64
                        }),
                        *lanes,
                    ));
                    format!(
                        "({native})(({unsigned})({}) {op} ({unsigned})({}))",
                        values[0], values[1]
                    )
                } else {
                    format!("({}) {op} ({})", values[0], values[1])
                }
            }
        };
        self.writeln(&format!("{} = {};", var.unwrap(), result));
    }
}
