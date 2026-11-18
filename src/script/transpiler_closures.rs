//! Typed Script lambdas lower to the same concrete Core closure environments.
use super::*;
impl ScriptType {
    pub(super) fn same_callable_signature(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Callable {
                    params: a,
                    result: b,
                    ..
                },
                Self::Callable {
                    params: c,
                    result: d,
                    ..
                },
            ) => {
                a.len() == c.len()
                    && a.iter().zip(c).all(|(x, y)| x.same_callable_signature(y))
                    && b.same_callable_signature(d)
            }
            (Self::Array(a, n), Self::Array(b, m)) => n == m && a.same_callable_signature(b),
            _ => self == other,
        }
    }
    fn contains_callable(&self) -> bool {
        matches!(self, Self::Callable { .. })
            || matches!(self,Self::Array(t,_) if t.contains_callable())
    }
    pub(super) fn anonymous_callable(&self) -> bool {
        match self {
            Self::Callable { core, .. } => core.is_none(),
            Self::Array(element, _) => element.anonymous_callable(),
            _ => false,
        }
    }
}
impl Transpiler {
    pub(super) fn check_callable_argument(
        &self,
        expected: &ScriptType,
        argument: &Expr,
    ) -> Result<()> {
        if expected.contains_callable()
            && !expected.same_callable_signature(&self.infer_type(argument)?)
        {
            return Err(
                "Callable argument/return must match its declared parameter and result types"
                    .into(),
            );
        }
        Ok(())
    }
    pub(super) fn check_named_callbacks(&self, name: &str, args: &[Expr]) -> Result<()> {
        if let Some((params,_))=self.imported.functions.get(name) {
            for (parameter,argument) in params.iter().zip(args) {self.check_callable_argument(parameter,argument)?;}
        }
        if let Some(function) = self.definitions.get(name) {
            for (parameter, argument) in function.params.iter().zip(args) {
                if let Some(hint) = &parameter.type_hint {
                    self.check_callable_argument(&Self::map_type(hint)?, argument)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn callable_result(&self, func: &Expr, args: &[Expr]) -> Result<Option<ScriptType>> {
        if let Expr::Identifier { name, .. } = func {
            if !self.locals.contains_key(name) {
                return Ok(None);
            }
        }
        let ty = self.infer_type(func)?;
        let ScriptType::Callable { params, result, .. } = ty else {
            return Err("Script call target is not a function or closure".into());
        };
        if params.len() != args.len() {
            return Err(format!(
                "closure requires {} arguments, got {}",
                params.len(),
                args.len()
            ));
        }
        for (expected, argument) in params.iter().zip(args) {
            self.check_callable_argument(expected, argument)?;
        }
        // Core checks scalar conversions against the actual instantiated signature.
        Ok(Some(*result))
    }
    pub(super) fn lambda_parts(
        &self,
        params: &[Param],
        ret: Option<&TypeHint>,
        body: &Expr,
        span: super::super::token::Span,
    ) -> Result<(ScriptType, String)> {
        let mut context = Transpiler::new();
        context.imported = self.imported.clone();
        context.locals = self.locals.clone();
        context.assigned = self.assigned.clone();
        context.functions = self.functions.clone();
        context.definitions = self.definitions.clone();
        context.const_parameters = self.const_parameters.clone();
        let mut parameter_types = Vec::new();
        let mut parameter_code = Vec::new();
        let mut seen = HashSet::new();
        for p in params {
            if !seen.insert(p.name.clone()) {
                return Err(format!("duplicate lambda parameter '{}'", p.name));
            }
            let ty = Self::map_type(p.type_hint.as_ref().ok_or("lambda parameters need types")?)?;
            if ty.anonymous_callable() {
                return Err(
                    "nested Callable parameters on Script lambdas need a named generic function"
                        .into(),
                );
            }
            parameter_code.push(format!("{}: {}", p.name, ty));
            parameter_types.push(ty.clone());
            context.locals.insert(p.name.clone(), ty);
            context.assigned.insert(p.name.clone());
        }
        let result = if let Some(ret) = ret {
            Self::map_type(ret)?
        } else {
            context.infer_type(body)?
        };
        let annotation = if result.anonymous_callable() {
            String::new()
        } else {
            format!(" -> {}", result)
        };
        let code = format!(
            "(|{}|{} {{ {} }})",
            parameter_code.join(", "),
            annotation,
            context.expression(body)?
        );
        Ok((
            ScriptType::Callable {
                params: parameter_types,
                result: Box::new(result),
                identity: format!("lambda_{}", span.start),
                core: None,
            },
            code,
        ))
    }
}
