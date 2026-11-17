//! Backend guard for the supported machine entry ABIs, including direct IR users.
use crate::{
    middle::ir::*,
    utils::{Error, Result},
};

pub fn validate(module: &IRModule, target: &str) -> Result<()> {
    let arch = if target == "native" {
        std::env::consts::ARCH
    } else {
        target.split('-').next().unwrap_or("")
    };
    for function in &module.functions {
        let bad = |message: &str| Error::CodeGen(format!("{}: {message}", function.name));
        if function.naked {
            if function.interrupt
                || !matches!(arch, "x86_64" | "aarch64" | "arm64")
                || !function.params.is_empty()
                || function.ret_type != IRType::Void
            {
                return Err(bad(
                    "naked entry requires x86_64/aarch64, no parameters, and void result",
                ));
            }
            if function.blocks.len()!=1 || function.blocks[0].instructions.is_empty()
                || function.blocks[0].instructions.iter().any(|instruction|!matches!(instruction,Instruction::InlineAsm{operands,..} if operands.is_empty()))
                || !matches!(function.blocks[0].terminator,Some(Terminator::Unreachable)) {
                return Err(bad("naked body must be operandless assembly with no generated prologue or return"));
            }
        }
        if function.interrupt {
            let frame = function.params.first().and_then(|(_, ty)| {
                if let IRType::Ptr(inner) = ty {
                    if let IRType::Struct(name) = inner.as_ref() {
                        module
                            .structs
                            .iter()
                            .find(|definition| definition.name == *name)
                    } else {
                        None
                    }
                } else {
                    None
                }
            });
            if arch != "x86_64"
                || function.ret_type != IRType::Void
                || !(function.params.len() == 1
                    || function.params.len() == 2 && function.params[1].1 == IRType::U64)
                || !frame.is_some_and(|definition| {
                    !definition.is_union
                        && definition.repr != StructRepr::Packed
                        && definition.fields.len() == 5
                        && definition.fields.iter().all(|(_, ty)| *ty == IRType::U64)
                })
            {
                return Err(bad("interrupt entry requires the x86_64 Frame pointer and optional u64 exception-code ABI"));
            }
            if function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| {
                    matches!(
                        instruction,
                        Instruction::Call { .. } | Instruction::InlineAsm { .. }
                    )
                })
            {
                return Err(bad(
                    "interrupt entry cannot make ordinary calls or use unchecked assembly",
                ));
            }
        }
    }
    Ok(())
}
