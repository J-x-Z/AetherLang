//! Script fixed arrays keep structured types while lowering to the same Core
//! const generic templates. No separate arithmetic evaluator lives here.
use super::*;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Dimension {
    Value(u64),
    Parameter(String),
}
impl fmt::Display for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Value(n) => write!(f, "{n}"),
            Self::Parameter(n) => write!(f, "{n}"),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ScriptType {
    Callable {params:Vec<ScriptType>,result:Box<ScriptType>,identity:String,core:Option<String>},
    Scalar(String),
    Array(Box<ScriptType>, Dimension),
}
impl From<&str> for ScriptType {
    fn from(name: &str) -> Self {
        Self::Scalar(name.into())
    }
}
impl From<String> for ScriptType {
    fn from(name: String) -> Self {
        Self::Scalar(name)
    }
}
impl PartialEq<&str> for ScriptType {
    fn eq(&self, name: &&str) -> bool {
        matches!(self,Self::Scalar(value) if value==*name)
    }
}
impl fmt::Display for ScriptType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Callable {core,..} => write!(f,"{}",core.as_deref().unwrap_or("_")),
            Self::Scalar(n) => write!(f, "{n}"),
            Self::Array(element, length) => write!(f, "[{element}; {length}]"),
        }
    }
}
impl ScriptType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Callable {..} => "Callable",
            Self::Scalar(name) => name,
            Self::Array(..) => "Array",
        }
    }
    fn infer_dimensions(
        &self,
        actual: &Self,
        bindings: &mut HashMap<String, Dimension>,
    ) -> Result<()> {
        if let (Self::Array(element, dimension), Self::Array(other, length)) = (self, actual) {
            if let Dimension::Parameter(name) = dimension {
                if let Some(previous) = bindings.get(name) {
                    if previous != length {
                        return Err(format!(
                            "conflicting array lengths for const parameter '{name}'"
                        ));
                    }
                } else {
                    bindings.insert(name.clone(), length.clone());
                }
            }
            element.infer_dimensions(other, bindings)?;
        }
        Ok(())
    }
    fn substitute(&self, bindings: &HashMap<String, Dimension>) -> Result<Self> {
        Ok(match self {
            Self::Scalar(_) | Self::Callable {..} => self.clone(),
            Self::Array(element, length) => Self::Array(
                Box::new(element.substitute(bindings)?),
                match length {
                    Dimension::Value(_) => length.clone(),
                    Dimension::Parameter(name) => bindings.get(name).cloned().ok_or_else(|| {
                        format!("cannot infer const parameter '{name}' from array arguments")
                    })?,
                },
            ),
        })
    }
    fn validate_dimensions(&self, names: &HashSet<String>) -> Result<()> {
        if let Self::Array(element, length) = self {
            if let Dimension::Parameter(name) = length {
                if !names.contains(name) {
                    return Err(format!(
                        "array dimension '{name}' is not a declared const parameter"
                    ));
                }
            }
            element.validate_dimensions(names)?;
        }
        Ok(())
    }
}
impl Transpiler {
    pub(super) fn map_type(hint: &TypeHint) -> Result<ScriptType> {
        if hint.name=="Callable" && !hint.generics.is_empty() {
            let mut types=hint.generics.iter().map(Self::map_type).collect::<Result<Vec<_>>>()?;
            let result=Box::new(types.pop().unwrap());
            return Ok(ScriptType::Callable{params:types,result,identity:format!("signature_{}",hint.span.start),core:None});
        }
        if hint.name == "Array" && hint.generics.len() == 2 && hint.const_value.is_none() {
            let element = Self::map_type(&hint.generics[0])?;
            let dimension = &hint.generics[1];
            if !dimension.generics.is_empty() {
                return Err("Array length must be an integer or const parameter".into());
            }
            let length = if let Some(value) = dimension.const_value {
                Dimension::Value(
                    u64::try_from(value).map_err(|_| "array length must be nonnegative")?,
                )
            } else {
                Dimension::Parameter(dimension.name.clone())
            };
            return Ok(ScriptType::Array(Box::new(element), length));
        }
        if !hint.generics.is_empty() || hint.const_value.is_some() {
            return Err("Script supports fixed Array[Element, Length] type hints".into());
        }
        Ok(match hint.name.as_str() {
            "int" => "i64",
            "float" => "f64",
            "str" => "*u8",
            "None" => "void",
            other => other,
        }
        .into())
    }
    pub(super) fn validate_signature(function: &FunctionDef) -> Result<()> {
        let names = function
            .const_params
            .iter()
            .map(|p| p.name.clone())
            .collect();
        for parameter in &function.params {
            if let Some(hint) = &parameter.type_hint {
                Self::map_type(hint)?.validate_dimensions(&names)?;
            }
        }
        if let Some(hint) = &function.return_type {
            Self::map_type(hint)?.validate_dimensions(&names)?;
        }
        Ok(())
    }
    pub(super) fn repeat_type(&self, args: &[Expr]) -> Result<ScriptType> {
        if args.len() != 2 {
            return Err("repeat(value, count) requires two arguments".into());
        }
        let dimension = match &args[1] {
            Expr::Integer { value, .. } if *value >= 0 => Dimension::Value(*value as u64),
            Expr::Identifier { name, .. } if self.const_parameters.contains_key(name) => {
                Dimension::Parameter(name.clone())
            }
            _ => {
                return Err(
                    "repeat count must be a nonnegative integer literal or const parameter".into(),
                )
            }
        };
        Ok(ScriptType::Array(
            Box::new(self.infer_type(&args[0])?),
            dimension,
        ))
    }
    pub(super) fn call_result_type(&self, name: &str, args: &[Expr]) -> Result<ScriptType> {
        let function = self
            .definitions
            .get(name)
            .ok_or_else(|| format!("unknown Script function '{name}'"))?;
        let result = self
            .functions
            .get(name)
            .expect("registered Script signature");
        if matches!(result, ScriptType::Callable {..}) {
            let candidates=function.params.iter().enumerate().filter_map(|(i,p)|p.type_hint.as_ref().and_then(|h|Self::map_type(h).ok()).filter(|t|t.same_callable_signature(result)).map(|_|i)).collect::<Vec<_>>();
            if candidates.len()!=1 {return Err("a Callable return requires one matching callback parameter".into());}
            return self.infer_type(args.get(candidates[0]).ok_or("missing returned callback argument")?);
        }
        if function.const_params.is_empty() {
            return Ok(result.clone());
        }
        let mut bindings = HashMap::new();
        for (parameter, arg) in function.params.iter().zip(args) {
            let hint = parameter
                .type_hint
                .as_ref()
                .ok_or("generic Script parameters need types")?;
            Self::map_type(hint)?.infer_dimensions(&self.infer_type(arg)?, &mut bindings)?;
        }
        result.substitute(&bindings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn script_array_generics_reach_the_core_specializer() {
        let source="def copy[const N: int](values: Array[int,N]) -> Array[int,N]:\n    return values\ndef main():\n    values = copy([3,5])\n    values[0] = 7\n    return values[0] - 7\n";
        let module = crate::script::parser::Parser::new(source).parse().unwrap();
        let core = Transpiler::new().transpile(&module).unwrap();
        let program =
            crate::frontend::parser::Parser::new(crate::frontend::lexer::Lexer::new(&core, 0))
                .parse_program()
                .unwrap();
        let program = crate::frontend::specialize::specialize(program).unwrap();
        crate::frontend::semantic::SemanticAnalyzer::new()
            .analyze(&program)
            .unwrap();
        assert!(core.contains("fn copy<const N: i64>"));
    }
}
