//! Materialize an ordinary destructor for each owned instance of its nominal family.
use super::*;

fn invalid(text: &str, span: Span) -> Error {
    Error::semantic_expected("E2405", text, span)
}

impl Specializer {
    pub(super) fn collect_generic_destructors(&mut self, program: &Program, aliases: &HashMap<String, Type>) -> Result<()> {
        for item in &program.items {
            let Item::Function(f) = item else { continue };
            let params = parameters(&f.generic_params, &f.type_params);
            if params.is_empty() { continue; }
            let mut markers = f.annotations.iter().filter(|a| a.name.name == "drop");
            let Some(marker) = markers.next() else { continue };
            if markers.next().is_some() || !marker.args.is_empty() || !f.contracts.is_empty() {
                return Err(invalid("one argument-free @drop on an ordinary function without contracts", f.span));
            }
            let [Param { ty: Type::Owned { inner, ownership: Ownership::Own, .. }, .. }] = &f.params[..] else {
                return Err(invalid("one own Record<T> parameter for a generic destructor", f.span));
            };
            if let Some(ret) = &f.ret_type {
                if !(matches!(ret, Type::Unit(_)) || matches!(ret, Type::Tuple(fields, _) if fields.is_empty()) ||
                    matches!(ret, Type::Named(name, _) if name == "void")) {
                    return Err(invalid("a unit return for @drop", ret.span()));
                }
            }
            let (family, args) = match inner.as_ref() {
                Type::Generic(name, args, _) => (name, args.iter().cloned().map(GenericArg::Type).collect::<Vec<_>>()),
                Type::GenericWithArgs { name, args, .. } => (name, args.clone()),
                _ => return Err(invalid("a generic nominal struct with its parameters in declaration order", inner.span())),
            };
            let record = self.structs.get(family).ok_or_else(|| invalid("a declared struct for generic @drop", inner.span()))?;
            let record_params = parameters(&record.generic_params, &record.type_params);
            let matches = params.len() == args.len() && args.len() == record_params.len() &&
                params.iter().zip(&args).zip(&record_params).all(|((p, a), r)| match (p, a, r) {
                    (GenericParam::Type(p), GenericArg::Type(Type::Named(a, _)), GenericParam::Type(_)) => p.name == *a,
                    (GenericParam::Const { name, .. }, GenericArg::Type(Type::Named(a, _)), GenericParam::Const { .. }) => name.name == *a,
                    (GenericParam::Const { name, .. }, GenericArg::Const(Expr::Ident(a)), GenericParam::Const { .. }) => name.name == a.name,
                    _ => false,
                });
            if !matches {
                return Err(invalid("all generic @drop parameters in the same order and kind as the owned struct", inner.span()));
            }
            for (parameter, field) in params.iter().zip(&record_params) {
                if let (GenericParam::Const {ty: left, ..}, GenericParam::Const {ty: right, ..}) = (parameter, field) {
                    let resolve = |ty| super::super::semantic::SemanticAnalyzer::specialization_parameter(
                        self.pointer_width, aliases, ty);
                    if resolve(left)? != resolve(right)? {
                        return Err(invalid("matching const parameter types on a generic destructor and its struct", left.span()));
                    }
                }
            }
            if self.generic_destructors.insert(family.clone(), f.name.name.clone()).is_some() {
                return Err(invalid("one generic destructor per nominal struct", f.span));
            }
        }
        Ok(())
    }

    pub(super) fn instantiate_destructor(&mut self, ty: &Type, span: Span) -> Result<()> {
        let Type::Named(instance, _) = ty else { return Ok(()) };
        let Some((family, args)) = self.origins.get(instance) else { return Ok(()) };
        let Some(symbol) = self.generic_destructors.get(family) else { return Ok(()) };
        let f = self.functions[symbol].clone();
        let params = parameters(&f.generic_params, &f.type_params);
        let bindings = bind_instance_args(&params, args, symbol, span)?;
        // instantiate_function interns the symbol before visiting its signature.
        // Revisiting own Record<T> there finds the existing concrete destructor.
        self.instantiate_function(&f, &bindings, span)?;
        Ok(())
    }
}
