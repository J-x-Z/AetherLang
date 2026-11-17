//! Checked abstract interface contracts for concrete, statically called impls.
use super::*;

impl SemanticAnalyzer {
    fn interface_signature(&mut self, signature: &FunctionSig, target: &ResolvedType) -> Result<(Vec<ResolvedType>, ResolvedType)> {
        self.symbols.enter_scope();
        let result = (|| {
            self.symbols.define(Symbol { name: "Self".into(), kind: SymbolKind::TypeAlias { target: target.clone() },
                ty: target.clone(), span: signature.span, mutable: false })?;
            let mut names = HashSet::new();
            for parameter in &signature.params {
                if !names.insert(&parameter.name.name) { return Err(Error::DuplicateDefinition { name: parameter.name.name.clone(), span: parameter.span }); }
            }
            if !signature.contracts.is_empty() {
                return Err(Error::semantic_expected("E2104", "interface contracts require implemented contract lowering", signature.span));
            }
            let params = signature.params.iter().map(|p| self.resolve_type(&p.ty)).collect::<Result<Vec<_>>>()?;
            let ret = signature.ret_type.as_ref().map(|ty| self.resolve_type(ty)).transpose()?.unwrap_or(ResolvedType::UNIT);
            Ok((params, ret))
        })();
        self.symbols.exit_scope();
        result
    }

    pub(super) fn collect_interfaces(&mut self, program: &Program) -> Result<()> {
        for item in &program.items {
            let (Item::Interface(definition) | Item::Trait(definition)) = item else { continue; };
            if self.symbols.scopes[0].symbols.contains_key(&definition.name.name)
                || self.interfaces.contains_key(&definition.name.name) {
                return Err(Error::DuplicateDefinition { name: definition.name.name.clone(), span: definition.span });
            }
            if !definition.type_params.is_empty() || !definition.associated_types.is_empty()
                || !definition.supertraits.is_empty() || !definition.default_methods.is_empty() {
                return Err(Error::semantic_expected("E2104", "abstract non-generic interface; default methods, associated types, supertraits and generic interfaces are not implemented", definition.span));
            }
            let mut names = HashSet::new();
            for signature in &definition.methods {
                if !names.insert(&signature.name.name) {
                    return Err(Error::DuplicateDefinition { name: signature.name.name.clone(), span: signature.span });
                }
                // Unused declarations must be valid too. Self is an abstract
                // nominal placeholder until each concrete implementation below.
                self.interface_signature(signature, &ResolvedType::GenericParam("Self".into()))?;
            }
            self.interfaces.insert(definition.name.name.clone(), definition.clone());
        }
        Ok(())
    }

    pub(super) fn check_interface_implementations(&mut self, program: &Program) -> Result<()> {
        let mut implementations = HashSet::new();
        for item in &program.items {
            let Item::Impl(block) = item else { continue; };
            let target = self.resolve_type(&Type::Named(block.target.name.clone(), block.target.span))?;
            if !matches!(target, ResolvedType::Struct { .. } | ResolvedType::Enum { .. }) {
                return Err(Error::semantic_expected("E2104", "nominal struct or enum impl target", block.target.span));
            }
            let Some(interface) = &block.interface else { continue; };
            let definition = self.interfaces.get(&interface.name).cloned().ok_or_else(|| Error::Semantic {
                code: "E2002", span: interface.span, message: format!("undefined interface '{}'", interface.name),
            })?;
            let identity = (interface.name.clone(), Self::type_identity(&target));
            if !implementations.insert(identity) {
                return Err(Error::DuplicateDefinition { name: format!("impl {} for {}", interface.name, block.target.name), span: block.span });
            }
            for method in &block.methods {
                if !definition.methods.iter().any(|required| required.name.name == method.name.name) {
                    return Err(Error::semantic_expected("E2104", format!("method declared by interface '{}'; '{}' belongs in an inherent impl", interface.name, method.name.name), method.span));
                }
            }
            for required in &definition.methods {
                let implementation = block.methods.iter().find(|method| method.name.name == required.name.name)
                    .ok_or_else(|| Error::semantic_expected("E2104", format!("implementation of required method '{}::{}'", interface.name, required.name.name), block.span))?;
                let (params, ret) = self.interface_signature(required, &target)?;
                let symbol = format!("{}::{}", block.target.name, required.name.name);
                let actual = &self.facts.method_signatures[&symbol];
                let expected_type = ResolvedType::Function { params, ret: Box::new(ret) };
                let actual_type = ResolvedType::Function { params: actual.params.clone(), ret: Box::new(actual.ret.clone()) };
                if !Self::types_identical(&expected_type, &actual_type) {
                    return Err(Error::TypeMismatch { expected: Self::type_identity(&expected_type), got: Self::type_identity(&actual_type), span: implementation.span });
                }
                if required.effects.is_pure || !required.effects.effects.is_empty() {
                    self.interface_effect_bounds.push((symbol, required.effects.clone(), implementation.span));
                }
            }
        }
        Ok(())
    }

    pub(super) fn check_interface_effect_bounds(&self) -> Result<()> {
        for (name, bound, span) in &self.interface_effect_bounds {
            let Some(Symbol { kind: SymbolKind::Function { effects, .. }, .. }) = self.symbols.lookup(name) else { continue; };
            if let Some(effect) = effects.effects.iter().find(|effect| bound.is_pure || !bound.effects.contains(effect)) {
                return Err(Error::EffectViolation { message: format!("implementation '{}' requires {:?}, outside its interface effect upper bound", name, effect), span: *span });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::{lexer::Lexer, parser::Parser};
    fn analyze(source: &str) -> Result<()> {
        let program = Parser::new(Lexer::new(source, 0)).parse_program()?;
        SemanticAnalyzer::new().analyze(&program)
    }
    #[test]
    fn abstract_signatures_validate_unused_names_and_duplicate_methods() {
        for source in [
            "interface I{fn get()->MissingType;}",
            "trait I{fn get(value:MissingType);}",
            "interface I{fn get(x:i32,x:i32);}",
            "interface I{fn get();fn get();}",
            "interface I{}interface I{}",
        ] { assert!(analyze(source).is_err(), "{source}"); }
        analyze("pub interface I{fn get(self:&Self)->i32 effect[read];}pub trait T{fn make(value:i32)->Self;}").unwrap();
        let error=analyze("trait I<T>{fn get(value:T)->T;}").unwrap_err();
        assert!(error.to_string().contains("generic interfaces are not implemented"), "{error}");
    }
    #[test]
    fn implementations_match_canonical_signatures_and_all_required_methods() {
        analyze("type Count=i32;interface I{fn make(value:Count)->Self;fn get(self:&Self)->Count;}struct S{x:i32}impl I for S{fn make(other:i32)->S{S{x:other}}fn get(self:&S)->i32{self.x}}").unwrap();
        for source in [
            "struct S{x:i32}impl Missing for S{}",
            "interface I{}impl I for Missing{}",
            "interface I{fn get()->i32;}struct S{x:i32}impl I for S{}",
            "interface I{}struct S{x:i32}impl I for S{fn extra(){}}",
            "interface I{fn get(self:&Self)->i32;}struct S{x:i32}impl I for S{fn get()->i32{1}}",
            "interface I{fn get(self:&mut Self)->i32;}struct S{x:i32}impl I for S{fn get(self:&Self)->i32{1}}",
            "interface I{fn get()->i32;}struct S{x:i32}impl I for S{fn get()->i64{1}}",
            "interface I{}struct S{x:i32}impl I for S{}impl I for S{}",
        ] { assert!(analyze(source).is_err(), "{source}"); }
    }
    #[test]
    fn interface_effect_bounds_apply_to_inferred_implementation_effects() {
        analyze("interface I{fn set(self:&mut Self) effect[write];}struct S{x:i32}impl I for S{fn set(self:&mut Self){self.x=7;}}").unwrap();
        for source in [
            "interface I{fn run(self:&Self) pure;}struct S{x:i32}impl I for S{fn run(self:&Self){puts(\"effect\");}}",
            "interface I{fn set(self:&mut Self) pure;}struct S{x:i32}impl I for S{fn set(self:&mut Self){self.x=7;}}",
            "interface I{fn read(self:&Self)->i32 effect[write];}struct S{x:i32}impl I for S{fn read(self:&Self)->i32{self.x}}",
        ] { let error=analyze(source).unwrap_err();assert!(matches!(error,Error::EffectViolation{..}),"{source}: {error:?}"); }
    }
}
