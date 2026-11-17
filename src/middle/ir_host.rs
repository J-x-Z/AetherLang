//! Hosted builtins lower to ordinary typed calls or CFG before backend selection.
use super::*;
use crate::intrinsics::HostBuiltin;

impl IRGenerator {
    pub(super) fn validate_hosted_abi(&self) -> Result<()> {
        if self.module.no_std {
            return Ok(());
        }
        fn same(a: &IRType, b: &IRType) -> bool {
            a == b
                || matches!(
                    (a, b),
                    (IRType::Ptr(_), IRType::Ptr(_))
                        | (IRType::Void, IRType::Unit)
                        | (IRType::Unit, IRType::Void)
                )
        }
        for (name, params, ret, variadic) in self
            .module
            .externs
            .iter()
            .map(|f| (&f.name, &f.params, &f.ret_type, f.variadic))
            .chain(
                self.module
                    .functions
                    .iter()
                    .map(|f| (&f.name, &f.params, &f.ret_type, false)),
            )
        {
            let Some((expected, result, variable)) =
                crate::intrinsics::host_c_signature(name, self.pointer_width())
            else {
                continue;
            };
            let expected = expected
                .iter()
                .map(Self::resolved_type_to_ir)
                .collect::<Result<Vec<_>>>()?;
            let result = Self::resolved_type_to_ir(&result)?;
            if variadic != variable
                || params.len() != expected.len()
                || !same(ret, &result)
                || !params
                    .iter()
                    .zip(&expected)
                    .all(|((_, actual), expected)| same(actual, expected))
            {
                return Err(crate::utils::Error::CodeGen(format!(
                    "inconsistent C ABI declaration and definition for '{name}': hosted symbol must match its target C ABI"
                )));
            }
        }
        for global in &self.module.globals {
            if crate::intrinsics::host_c_signature(&global.name, self.pointer_width()).is_some() {
                return Err(crate::utils::Error::CodeGen(format!(
                    "hosted symbol '{}' is a C function, not global storage",
                    global.name
                )));
            }
        }
        Ok(())
    }
    pub(super) fn checked_host_call(&self, span: crate::utils::Span) -> Option<HostBuiltin> {
        let function = self.current_fn.as_ref()?;
        self.semantic_facts
            .as_ref()?
            .host_calls
            .get(&(function.name.clone(), span))
            .copied()
    }

    fn require_host_extern(
        &mut self,
        name: &str,
        params: Vec<IRType>,
        ret_type: IRType,
        variadic: bool,
    ) -> Result<()> {
        fn same_abi(a: &IRType, b: &IRType) -> bool {
            a == b || matches!((a, b), (IRType::Ptr(_), IRType::Ptr(_)))
        }
        if let Some(existing) = self.module.externs.iter().find(|item| item.name == name) {
            if existing.variadic != variadic
                || !same_abi(&existing.ret_type, &ret_type)
                || existing.params.len() != params.len()
                || !existing
                    .params
                    .iter()
                    .zip(&params)
                    .all(|((_, a), b)| same_abi(a, b))
            {
                return Err(crate::utils::Error::CodeGen(format!("host builtin requires the target C ABI for '{name}'; its explicit declaration conflicts")));
            }
            return Ok(());
        }
        self.module.externs.push(IRExtern {
            name: name.into(),
            params: params
                .into_iter()
                .enumerate()
                .map(|(index, ty)| (format!("arg{index}"), ty))
                .collect(),
            ret_type,
            variadic,
        });
        Ok(())
    }

    fn host_call(
        &mut self,
        name: &str,
        params: Vec<IRType>,
        ret: IRType,
        variadic: bool,
        args: Vec<Value>,
        keep: bool,
    ) -> Result<Value> {
        self.require_host_extern(name, params, ret.clone(), variadic)?;
        let dest = (keep && ret != IRType::Void).then(|| self.alloc_register());
        self.emit_current_with_type(
            Instruction::Call {
                dest,
                func: name.into(),
                args,
            },
            ret,
        );
        Ok(dest.map(Value::Register).unwrap_or(Value::Unit))
    }

    pub(super) fn generate_host_call(
        &mut self,
        builtin: HostBuiltin,
        args: &[Expr],
    ) -> Result<Value> {
        if self.module.no_std {
            return Err(crate::utils::Error::CodeGen(
                "implicit hosted builtin in no_std module".into(),
            ));
        }
        let params = builtin
            .params(self.pointer_width())
            .iter()
            .map(Self::resolved_type_to_ir)
            .collect::<Result<Vec<_>>>()?;
        if params.len() != args.len() {
            return Err(crate::utils::Error::CodeGen(
                "host builtin argument count mismatch".into(),
            ));
        }
        let mut values = Vec::new();
        for (arg, ty) in args.iter().zip(&params) {
            values.push(self.generate_expr_expected(arg, Some(ty))?);
            if self.get_current_terminator().is_some() {
                return Ok(Value::Unit);
            }
        }
        let bytes = IRType::Ptr(Box::new(IRType::U8));
        use HostBuiltin::*;
        match builtin {
            Print | PrintI64 | PrintlnI64 => {
                let format = match builtin {
                    Print => "%s",
                    PrintI64 => "%lld",
                    _ => "%lld\n",
                };
                values.insert(0, Value::Constant(Constant::String(format.into())));
                self.host_call("printf", vec![bytes], IRType::I32, true, values, false)
            }
            Println | Puts => self.host_call(
                "puts",
                vec![bytes],
                IRType::I32,
                false,
                values,
                builtin == Puts,
            ),
            Alloc | Malloc => self.host_call("malloc", params, bytes, false, values, true),
            Free => self.host_call("free", params, IRType::Void, false, values, false),
            Atof => self.host_call("atof", params, IRType::F64, false, values, true),
            Strcmp => self.host_call("strcmp", params, IRType::I32, false, values, true),
            Exit => {
                self.host_call("exit", params, IRType::Void, false, values, false)?;
                self.set_terminator_current(Terminator::Unreachable);
                Ok(Value::Unit)
            }
            Assert => {
                let passed = self.add_block("assert_passed");
                let failed = self.add_block("assert_failed");
                self.set_terminator_current(Terminator::Branch {
                    cond: values.remove(0),
                    then_target: passed,
                    else_target: failed,
                });
                self.current_block = failed;
                self.set_terminator_current(Terminator::Trap);
                self.current_block = passed;
                Ok(Value::Unit)
            }
        }
    }
}
