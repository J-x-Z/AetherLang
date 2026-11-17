//! Hosted libc names have a target C ABI even when supplied by user source.
use super::*;

impl SemanticAnalyzer {
    pub(super) fn c_integer_type(ty: &ResolvedType) -> &ResolvedType {
        match ty {
            ResolvedType::Primitive(PrimitiveType::Usize(PointerWidth::W32)) => &ResolvedType::U32,
            ResolvedType::Primitive(PrimitiveType::Usize(PointerWidth::W64)) => &ResolvedType::U64,
            ResolvedType::Primitive(PrimitiveType::Isize(PointerWidth::W32)) => &ResolvedType::I32,
            ResolvedType::Primitive(PrimitiveType::Isize(PointerWidth::W64)) => &ResolvedType::I64,
            _ => ty,
        }
    }

    pub(super) fn check_hosted_abi(&self, program: &Program) -> Result<()> {
        if program
            .inner_attrs
            .iter()
            .any(|attr| attr.name.name == "no_std")
        {
            return Ok(());
        }
        fn same(a: &ResolvedType, b: &ResolvedType) -> bool {
            matches!((a, b), (ResolvedType::Pointer(_), ResolvedType::Pointer(_)))
                || (a == &ResolvedType::NEVER && b == &ResolvedType::UNIT)
                || SemanticAnalyzer::c_integer_type(a) == SemanticAnalyzer::c_integer_type(b)
        }
        let check = |name: &Ident,
                     params: &[Param],
                     ret: Option<&Type>,
                     variadic: bool|
         -> Result<()> {
            let Some((expected, result, variable)) =
                crate::intrinsics::host_c_signature(&name.name, self.facts.pointer_width, self.facts.c_long_bits)
            else {
                return Ok(());
            };
            let params = params
                .iter()
                .map(|p| self.resolve_type(&p.ty))
                .collect::<Result<Vec<_>>>()?;
            let ret = ret
                .map(|ty| self.resolve_type(ty))
                .transpose()?
                .unwrap_or(ResolvedType::UNIT);
            if variadic != variable
                || params.len() != expected.len()
                || !same(&ret, &result)
                || !params.iter().zip(&expected).all(|(a, b)| same(a, b))
            {
                return Err(Error::Semantic {
                    code: "E2202",
                    span: name.span,
                    message: format!("hosted symbol '{}' must match its target C ABI", name.name),
                });
            }
            Ok(())
        };
        let storage = |name: &Ident| -> Result<()> {
            if crate::intrinsics::host_c_signature(&name.name, self.facts.pointer_width, self.facts.c_long_bits).is_some() {
                return Err(Error::Semantic {
                    code: "E2202",
                    span: name.span,
                    message: format!(
                        "hosted symbol '{}' is a C function, not global storage",
                        name.name
                    ),
                });
            }
            Ok(())
        };
        for item in &program.items {
            match item {
                Item::Function(function) => check(
                    &function.name,
                    &function.params,
                    function.ret_type.as_ref(),
                    false,
                )?,
                Item::Extern(block) => {
                    for item in &block.items {
                        match item {
                            ForeignItem::Fn {
                                name,
                                params,
                                ret_type,
                                variadic,
                                ..
                            } => check(name, params, ret_type.as_ref(), *variadic)?,
                            ForeignItem::Static { name, .. } => storage(name)?,
                        }
                    }
                }
                Item::Static(definition) => storage(&definition.name)?,
                _ => {}
            }
        }
        Ok(())
    }
}
