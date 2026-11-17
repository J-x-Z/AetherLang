//! Typed const arguments and array dimensions for the shared specializer.
//! Arithmetic is delegated to semantic_constants; instances contain only
//! concrete types and normalized values, never source-expression cache keys.
use super::*;
use std::ops::{Deref, DerefMut};

#[derive(Clone, Default)]
pub(super) struct Bindings {
    types: HashMap<String, Type>,
    pub constants: HashMap<String, BoundConstant>,
    pub const_types: HashMap<String, Type>,
}
impl Bindings {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn parameters(generic: &[GenericParam], legacy: &[Ident]) -> Result<Self> {
        let mut result = Self::new();
        let mut names = HashSet::new();
        for parameter in parameters(generic, legacy) {
            let name = match &parameter {
                GenericParam::Type(name) | GenericParam::Const { name, .. } => name,
            };
            if !names.insert(name.name.clone()) {
                return Err(Error::DuplicateDefinition {
                    name: name.name.clone(),
                    span: name.span,
                });
            }
            if let GenericParam::Const { name, ty } = parameter {
                result.const_types.insert(name.name, *ty);
            }
        }
        Ok(result)
    }
}
impl Deref for Bindings {
    type Target = HashMap<String, Type>;
    fn deref(&self) -> &Self::Target {
        &self.types
    }
}
impl DerefMut for Bindings {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.types
    }
}
impl FromIterator<(String, Type)> for Bindings {
    fn from_iter<I: IntoIterator<Item = (String, Type)>>(iter: I) -> Self {
        Self {
            types: iter.into_iter().collect(),
            ..Self::default()
        }
    }
}
#[derive(Clone)]
pub(super) struct BoundConstant {
    pub ty: Type,
    pub expression: Expr,
    key: String,
    integer: Option<i128>,
}
#[derive(Clone)]
pub(super) enum InstanceArg {
    Type(Type),
    Const(BoundConstant),
}
impl InstanceArg {
    pub fn key(&self) -> String {
        match self {
            Self::Type(ty) => type_key(ty),
            Self::Const(value) => value.key.clone(),
        }
    }
}
pub(super) fn parameters(generic: &[GenericParam], legacy: &[Ident]) -> Vec<GenericParam> {
    if generic.is_empty() {
        legacy.iter().cloned().map(GenericParam::Type).collect()
    } else {
        generic.to_vec()
    }
}
pub(super) fn template_application(
    name: &str,
    generic: &[GenericParam],
    legacy: &[Ident],
    span: Span,
) -> Type {
    Type::GenericWithArgs {
        name: name.into(),
        args: parameters(generic, legacy)
            .into_iter()
            .map(|p| match p {
                GenericParam::Type(id) => GenericArg::Type(named(&id.name, id.span)),
                GenericParam::Const { name, .. } => GenericArg::Const(Expr::Ident(name)),
            })
            .collect(),
        span,
    }
}
pub(super) fn bind_instance_args(
    params: &[GenericParam],
    args: &[InstanceArg],
    base: &str,
    span: Span,
) -> Result<Bindings> {
    if params.len() != args.len() {
        return Err(problem(
            &format!(
                "{} generic arguments for '{base}', got {}",
                params.len(),
                args.len()
            ),
            span,
        ));
    }
    let mut result = Bindings::parameters(params, &[])?;
    for (parameter, argument) in params.iter().zip(args) {
        match (parameter, argument) {
            (GenericParam::Type(id), InstanceArg::Type(ty)) => {
                result.insert(id.name.clone(), ty.clone());
            }
            (GenericParam::Const { name, .. }, InstanceArg::Const(value)) => {
                result.constants.insert(name.name.clone(), value.clone());
            }
            _ => {
                return Err(problem(
                    "generic argument matching its type/const parameter kind",
                    span,
                ))
            }
        }
    }
    Ok(result)
}
pub(super) fn required_instance_args(
    params: &[GenericParam],
    bindings: &Bindings,
    base: &str,
    span: Span,
) -> Result<Vec<InstanceArg>> {
    params.iter().map(|p| {
        let (name,value,kind) = match p {
            GenericParam::Type(id) => (&id.name,bindings.get(&id.name).cloned().map(InstanceArg::Type),"type"),
            GenericParam::Const {name,..} => (&name.name,bindings.constants.get(&name.name).cloned().map(InstanceArg::Const),"const argument"),
        };
        value.ok_or_else(|| problem(&format!("a concrete {kind} for '{name}' in '{base}'; add an argument or result type annotation"),span))
    }).collect()
}
impl Specializer {
    fn constant(
        &self,
        expression: &Expr,
        expected: Option<&Type>,
        bindings: &Bindings,
    ) -> Result<BoundConstant> {
        let expression = substitute_constant(expression, bindings);
        let aliases = self
            .aliases
            .iter()
            .filter(|(_, a)| a.type_params.is_empty())
            .map(|(name, a)| (name.clone(), a.ty.clone()))
            .collect();
        let (ty, expression, key, integer) =
            super::super::semantic::SemanticAnalyzer::specialization_constant(
                self.pointer_width,
                &self.constant_defs,
                &aliases,
                &expression,
                expected,
            )?;
        Ok(BoundConstant {
            ty,
            expression,
            key,
            integer,
        })
    }
    pub(super) fn reject_const_shadow(&self, name: &Ident) -> Result<()> {
        if self.bindings.const_types.contains_key(&name.name) {
            Err(problem(
                &format!("local binding distinct from const generic '{}'", name.name),
                name.span,
            ))
        } else {
            Ok(())
        }
    }
    pub(super) fn array_size(&self, expression: &Expr, bindings: &Bindings) -> Result<usize> {
        // An annotated integer constant retains its own type while computing;
        // a final checked range conversion gives target-sized array lengths.
        let value = self.constant(expression, None, bindings)?;
        let length = value
            .integer
            .ok_or_else(|| problem("nonnegative integer array length", expression.span()))?;
        let maximum = if self.pointer_width == PointerWidth::W32 {
            u32::MAX as i128
        } else {
            u64::MAX as i128
        };
        if length < 0 || length > maximum {
            return Err(problem(
                "array length representable by target usize",
                expression.span(),
            ));
        }
        usize::try_from(length).map_err(|_| {
            problem(
                "array length representable by compiler host",
                expression.span(),
            )
        })
    }
    fn generic_parameters(&self, base: &str, span: Span) -> Result<Vec<GenericParam>> {
        if let Some(d) = self.structs.get(base) {
            Ok(parameters(&d.generic_params, &d.type_params))
        } else if let Some(d) = self.enums.get(base) {
            Ok(parameters(&d.generic_params, &d.type_params))
        } else if let Some(d) = self.aliases.get(base) {
            Ok(parameters(&[], &d.type_params))
        } else {
            Err(Error::UndefinedType {
                name: base.into(),
                span,
            })
        }
    }
    pub(super) fn resolve_arguments(
        &mut self,
        base: &str,
        args: &[GenericArg],
        outer: &Bindings,
        span: Span,
    ) -> Result<Vec<InstanceArg>> {
        let params = self.generic_parameters(base, span)?;
        if params.len() != args.len() {
            return Err(problem(
                &format!(
                    "{} generic arguments for '{base}', got {}",
                    params.len(),
                    args.len()
                ),
                span,
            ));
        }
        Bindings::parameters(&params, &[])?;
        params
            .iter()
            .zip(args)
            .map(|(parameter, arg)| match parameter {
                GenericParam::Type(_) => match arg {
                    GenericArg::Type(ty) => self.ty(ty, outer).map(InstanceArg::Type),
                    _ => Err(problem("type argument for a type parameter", span)),
                },
                GenericParam::Const { ty, .. } => {
                    let expression = argument_expression(arg, span)?;
                    let expected = self.ty(ty, outer)?;
                    self.constant(&expression, Some(&expected), outer)
                        .map(InstanceArg::Const)
                }
            })
            .collect()
    }
    pub(super) fn infer_constant(
        &self,
        template: &Expr,
        actual: &Expr,
        bindings: &mut Bindings,
        span: Span,
        expected: Option<&Type>,
    ) -> Result<()> {
        if let Expr::Ident(id) = template {
            if let Some(expected) = bindings.const_types.get(&id.name) {
                let value = self.constant(actual, Some(expected), &Bindings::new())?;
                if let Some(previous) = bindings.constants.get(&id.name) {
                    if previous.key != value.key {
                        return Err(problem(
                            &format!(
                                "consistent const argument '{}' ({} versus {})",
                                id.name, previous.key, value.key
                            ),
                            span,
                        ));
                    }
                } else {
                    bindings.constants.insert(id.name.clone(), value);
                }
                return Ok(());
            }
        }
        // Computed dimensions are checked once other positions infer their
        // operands. We do not guess inverses of arbitrary integer expressions.
        if !const_unbound(template, bindings) {
            // The nominal instance supplies the const parameter's declared
            // type. A literal in an alias must receive that context before it
            // defaults to i64 (Pair<T> = Block<T, 2>, where Block uses usize).
            let wanted = self.constant(template, expected, bindings)?;
            let actual = self.constant(actual, Some(&wanted.ty), &Bindings::new())?;
            if wanted.key != actual.key {
                return Err(problem("matching computed const generic argument", span));
            }
        }
        Ok(())
    }
    pub(super) fn unify_instance(
        &mut self,
        base: &str,
        args: &[GenericArg],
        name: &str,
        params: &[Ident],
        bindings: &mut Bindings,
        span: Span,
    ) -> Result<()> {
        if let Some((origin, actuals)) = self.origins.get(name).cloned() {
            if base != origin {
                return Ok(());
            }
            for (arg, actual) in args.iter().zip(actuals) {
                match actual {
                    InstanceArg::Type(actual) => {
                        if let GenericArg::Type(template) = arg {
                            self.unify(template, &actual, params, bindings, span)?;
                        }
                    }
                    InstanceArg::Const(value) => self.infer_constant(
                        &argument_expression(arg, span)?,
                        &value.expression,
                        bindings,
                        span,
                        Some(&value.ty),
                    )?,
                }
            }
        }
        Ok(())
    }
}
fn argument_expression(arg: &GenericArg, span: Span) -> Result<Expr> {
    match arg {
        GenericArg::Const(expression) => Ok(expression.clone()),
        GenericArg::Type(Type::Named(name, span)) => Ok(Expr::Ident(Ident {
            name: name.clone(),
            span: *span,
        })),
        _ => Err(problem("constant expression for a const parameter", span)),
    }
}
pub(super) fn substitute_constant(expression: &Expr, bindings: &Bindings) -> Expr {
    match expression {
        Expr::Ident(id) => bindings
            .constants
            .get(&id.name)
            .map(|value| value.expression.clone())
            .unwrap_or_else(|| expression.clone()),
        Expr::Unary { op, expr, span } => Expr::Unary {
            op: *op,
            expr: Box::new(substitute_constant(expr, bindings)),
            span: *span,
        },
        Expr::Binary {
            left,
            op,
            right,
            span,
        } => Expr::Binary {
            left: Box::new(substitute_constant(left, bindings)),
            op: *op,
            right: Box::new(substitute_constant(right, bindings)),
            span: *span,
        },
        Expr::Cast { expr, ty, span } => Expr::Cast {
            expr: Box::new(substitute_constant(expr, bindings)),
            ty: substitute(ty, bindings),
            span: *span,
        },
        _ => expression.clone(),
    }
}
pub(super) fn const_unbound(expression: &Expr, bindings: &Bindings) -> bool {
    match expression {
        Expr::Ident(id) => {
            bindings.const_types.contains_key(&id.name)
                && !bindings.constants.contains_key(&id.name)
        }
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } => const_unbound(expr, bindings),
        Expr::Binary { left, right, .. } => {
            const_unbound(left, bindings) || const_unbound(right, bindings)
        }
        _ => false,
    }
}
pub(super) fn expression_key(expression: &Expr) -> String {
    match expression {
        Expr::Ident(id) => id.name.clone(),
        Expr::Literal(Literal::Int(value, ..)) => value.to_string(),
        Expr::Unary { op, expr, .. } => format!("{op:?}{}", expression_key(expr)),
        Expr::Binary {
            left, op, right, ..
        } => format!("{}{op:?}{}", expression_key(left), expression_key(right)),
        Expr::Cast { expr, ty, .. } => format!("{}as{}", expression_key(expr), type_key(ty)),
        _ => "constant".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::{lexer::Lexer, parser::Parser};
    fn prepare(source: &str, width: PointerWidth) -> Result<Program> {
        specialize_for_width(Parser::new(Lexer::new(source, 0)).parse_program()?, width)
    }
    #[test]
    fn equal_values_reuse_nominal_instances_and_distinct_values_do_not() {
        let program=prepare("const N:usize=3;struct B<T,const K:usize>{x:[T;K]}fn main(){let a:B<i64,N>;let b:B<i64,{1+2}>;let c:B<i64,4>;}",PointerWidth::W64).unwrap();
        let definitions: Vec<_> = program
            .items
            .iter()
            .filter_map(|item| {
                if let Item::Struct(s) = item {
                    Some(s)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(definitions.len(), 2);
        let mut lengths = definitions
            .iter()
            .map(|s| match &s.fields[0].ty {
                Type::Array { size, .. } => *size,
                _ => panic!("symbolic dimension survived"),
            })
            .collect::<Vec<_>>();
        lengths.sort();
        assert_eq!(lengths, [3, 4]);
    }
    #[test]
    fn array_context_infers_const_parameter_and_removes_template() {
        let program = prepare(
            "fn copy<T,const N:usize>(x:[T;N])->[T;N]{x}fn main(){let a:[u8;2]=copy([1,2]);}",
            PointerWidth::W64,
        )
        .unwrap();
        let function = program
            .items
            .iter()
            .find_map(|item| {
                if let Item::Function(f) = item {
                    if f.name.name.starts_with("copy__spec_") {
                        Some(f)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .unwrap();
        assert!(function.generic_params.is_empty());
        assert!(
            matches!(&function.ret_type,Some(Type::Array{size:2,elem,..})if matches!(elem.as_ref(),Type::Named(name,_)if name=="u8"))
        );
    }
    #[test]
    fn target_width_changes_checked_const_evaluation() {
        let source = "struct B<const N:usize>{x:u8}fn main(){let a:B<{(4294967295 as usize)+1}>;}";
        assert!(prepare(source, PointerWidth::W64).is_ok());
        assert!(prepare(source, PointerWidth::W32)
            .unwrap_err()
            .to_string()
            .contains("integer overflow"));
    }
}
