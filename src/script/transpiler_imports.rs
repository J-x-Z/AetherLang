//! A typed boundary to the ordinary Core module resolver. Imported declarations
//! are inspected, not reimplemented; final checking still consumes the same AST.
use super::*;
use crate::frontend::ast as core;

#[derive(Clone, Default)]
pub(super) struct ImportedApi {
    pub functions: HashMap<String, (Vec<ScriptType>, ScriptType)>,
    pub async_functions: HashSet<String>,
    pub values: HashMap<String, ScriptType>,
    pub fields: HashMap<(String, String), ScriptType>,
    pub methods: HashMap<(String, String), (Vec<ScriptType>, ScriptType)>,
}
struct Reader<'a> {
    names: HashMap<String, String>,
    aliases: HashMap<String, &'a core::Type>,
    constants: HashMap<String, core::ConstDef>,
    width: crate::types::PointerWidth,
}
impl Reader<'_> {
    fn ty(&self, ty: &core::Type, depth: usize) -> Result<ScriptType> {
        if depth > 64 {
            return Err("finite imported type alias expansion".into());
        }
        Ok(match ty {
            core::Type::Named(name,_)=>{
                if let Some(alias)=self.aliases.get(name) {return self.ty(alias,depth+1);}
                match name.as_str() {
                    "void"|"bool"|"char"|"i8"|"u8"|"i16"|"u16"|"i32"|"u32"|"i64"|"u64"|"isize"|"usize"|"f32"|"f64"=>name.clone().into(),
                    _=>self.names.get(name).ok_or_else(||format!("imported API exposes private or unsupported type '{name}'"))?.clone().into(),
                }
            }
            core::Type::Ref{mutable,inner,..}=>match inner.as_ref() {
                core::Type::Dyn(name,_)=>ScriptType::BorrowedInterface {name:self.names.get(name).ok_or("dynamic interface must be publicly reachable")?.clone(),mutable:*mutable},
                _=>ScriptType::Reference{mutable:*mutable,inner:Box::new(self.ty(inner,depth+1)?)},
            },
            core::Type::Array{elem,size,..}=>ScriptType::Array(Box::new(self.ty(elem,depth+1)?),Dimension::Value(*size as u64)),
            core::Type::ArrayExpr{elem,size,..}=>{
                let aliases=self.aliases.iter().map(|(n,t)|(n.clone(),(**t).clone())).collect();
                let (_,_,_,value)=crate::frontend::semantic::SemanticAnalyzer::specialization_constant(self.width,&self.constants,&aliases,size,None).map_err(|e|e.to_string())?;
                let maximum=if self.width==crate::types::PointerWidth::W32 {u32::MAX as i128}else{u64::MAX as i128};
                let value=value.filter(|n|*n>=0&&*n<=maximum).ok_or("imported array length must be a nonnegative target usize constant")?;
                ScriptType::Array(Box::new(self.ty(elem,depth+1)?),Dimension::Value(value as u64))
            },
            core::Type::Pointer(inner,_)=>{
                let inner=self.ty(inner,depth+1)?;
                if !matches!(inner,ScriptType::Scalar(_)) {return Err("imported raw pointers currently require a named scalar or record pointee".into());}
                format!("*{inner}").into()
            }
            core::Type::Unit(_)=>"void".into(),
            core::Type::Never(_)=>"!".into(),
            _=>return Err("typed Script imports currently support non-generic scalar/record values, fixed arrays, references and borrowed dynamic interfaces".into()),
        })
    }
}
impl ImportedApi {
    fn load(
        program: &core::Program,
        bindings: &HashMap<String, String>,
        requested: &HashSet<String>,
        width: crate::types::PointerWidth,
    ) -> Result<Self> {
        let mut reader = Reader {
            names: HashMap::new(),
            aliases: HashMap::new(),
            constants: program
                .items
                .iter()
                .filter_map(|i| {
                    if let core::Item::Const(c) = i {
                        Some((c.name.name.clone(), c.clone()))
                    } else {
                        None
                    }
                })
                .collect(),
            width,
        };
        let mut bindings = bindings.iter().collect::<Vec<_>>();
        bindings.sort_by_key(|(name, _)| (name.len(), (*name).clone()));
        for (name, target) in &bindings {
            reader
                .names
                .entry((*target).clone())
                .or_insert_with(|| (*name).clone());
        }
        for item in &program.items {
            if let core::Item::TypeAlias(alias) = item {
                if alias.type_params.is_empty() {
                    reader.aliases.insert(alias.name.name.clone(), &alias.ty);
                }
            }
        }
        let mut result = Self::default();
        for (name, target) in bindings {
            let mut found = false;
            let inspected = (|| -> Result<()> {
                for item in &program.items {
                    match item {
                        core::Item::Function(f) if f.name.name == *target => {
                            found = true;
                            if !f.type_params.is_empty() || !f.generic_params.is_empty() {
                                return Err("generic Core function imports are not yet exposed in Script; use a concrete public adapter".into());
                            }
                            let params = f
                                .params
                                .iter()
                                .map(|p| reader.ty(&p.ty, 0))
                                .collect::<Result<Vec<_>>>()?;
                            let ret = f
                                .ret_type
                                .as_ref()
                                .map(|t| reader.ty(t, 0))
                                .transpose()?
                                .unwrap_or_else(|| "void".into());
                            result.functions.insert(name.clone(), (params, ret));
                            if requested.contains(name)
                                && f.annotations.iter().any(|a| a.name.name == "async")
                            {
                                result.async_functions.insert(name.clone());
                            }
                        }
                        core::Item::Struct(s) if s.name.name == *target => {
                            found = true;
                            if !s.type_params.is_empty() || !s.generic_params.is_empty() {
                                return Err("import a concrete Core record or alias instead of a generic record template".into());
                            }
                            for field in &s.fields {
                                result.fields.insert(
                                    (name.clone(), field.name.name.clone()),
                                    reader.ty(&field.ty, 0)?,
                                );
                            }
                        }
                        core::Item::Trait(t) | core::Item::Interface(t)
                            if t.name.name == *target =>
                        {
                            found = true;
                            for method in &t.methods {
                                let params = method
                                    .params
                                    .iter()
                                    .skip(1)
                                    .map(|p| reader.ty(&p.ty, 0))
                                    .collect::<Result<Vec<_>>>()?;
                                let ret = method
                                    .ret_type
                                    .as_ref()
                                    .map(|t| reader.ty(t, 0))
                                    .transpose()?
                                    .unwrap_or_else(|| "void".into());
                                result.methods.insert(
                                    (name.clone(), method.name.name.clone()),
                                    (params, ret),
                                );
                            }
                        }
                        core::Item::Const(c) if c.name.name == *target => {
                            found = true;
                            result.values.insert(
                                name.clone(),
                                reader.ty(
                                    c.ty.as_ref()
                                        .ok_or("imported constants require an explicit type")?,
                                    0,
                                )?,
                            );
                        }
                        core::Item::Static(s) if s.name.name == *target => {
                            found = true;
                            result.values.insert(name.clone(), reader.ty(&s.ty, 0)?);
                        }
                        core::Item::TypeAlias(a) if a.name.name == *target => {
                            found = true;
                            reader.ty(&a.ty, 0)?;
                        }
                        core::Item::Extern(block) => {
                            for foreign in &block.items {
                                match foreign {
                                    core::ForeignItem::Fn {
                                        name: foreign,
                                        params,
                                        ret_type,
                                        variadic,
                                        ..
                                    } if foreign.name == *target => {
                                        found = true;
                                        if *variadic {
                                            return Err(
                                                "variadic Core imports require a concrete wrapper"
                                                    .into(),
                                            );
                                        }
                                        result.functions.insert(
                                            name.clone(),
                                            (
                                                params
                                                    .iter()
                                                    .map(|p| reader.ty(&p.ty, 0))
                                                    .collect::<Result<Vec<_>>>()?,
                                                ret_type
                                                    .as_ref()
                                                    .map(|t| reader.ty(t, 0))
                                                    .transpose()?
                                                    .unwrap_or_else(|| "void".into()),
                                            ),
                                        );
                                    }
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
                if !found {
                    return Err("this imported declaration kind is not exposed in Script".into());
                }
                Ok(())
            })();
            if requested.contains(name) {
                inspected.map_err(|e| format!("Core import '{name}': {e}"))?;
            }
        }
        Ok(result)
    }
}
impl Transpiler {
    pub fn import_prelude(module: &ScriptModule) -> (String, HashSet<String>) {
        let mut lines = Vec::new();
        let mut names = HashSet::new();
        for statement in &module.stmts {
            if let Stmt::Import(import) = statement {
                for (name, alias) in &import.names {
                    names.insert(alias.clone());
                    lines.push(format!(
                        "use {}::{} as {};",
                        import.path.join("::"),
                        name,
                        alias
                    ));
                }
            }
        }
        (lines.join("\n"), names)
    }
    pub fn with_core_imports(
        mut self,
        program: &core::Program,
        bindings: &HashMap<String, String>,
        requested: &HashSet<String>,
        width: crate::types::PointerWidth,
    ) -> Result<Self> {
        self.imported = ImportedApi::load(program, bindings, requested, width)?;
        Ok(self)
    }
    pub(super) fn imported_method(
        &self,
        target: &Expr,
        method: &str,
    ) -> Result<Option<(Vec<ScriptType>, ScriptType)>> {
        let mut ty = self.infer_type(target)?;
        while let ScriptType::Reference { inner, .. } = ty {
            ty = *inner;
        }
        let ScriptType::BorrowedInterface { name, .. } = ty else {
            return Ok(None);
        };
        Ok(self.imported.methods.get(&(name, method.into())).cloned())
    }
    pub(super) fn imported_field(&self, target: &Expr, field: &str) -> Result<ScriptType> {
        let mut ty = self.infer_type(target)?;
        while let ScriptType::Reference { inner, .. } = ty {
            ty = *inner;
        }
        let ScriptType::Scalar(name) = ty else {
            return Err("field access requires an imported Core record".into());
        };
        self.imported
            .fields
            .get(&(name, field.into()))
            .cloned()
            .ok_or_else(|| format!("unknown imported record field '{field}'"))
    }
    pub(super) fn reference_builtin(
        &self,
        func: &Expr,
        args: &[Expr],
    ) -> Result<Option<(ScriptType, String)>> {
        let Expr::Identifier { name, .. } = func else {
            return Ok(None);
        };
        if self.functions.contains_key(name)
            || self.locals.contains_key(name)
            || !matches!(name.as_str(), "ref" | "ref_mut" | "deref")
        {
            return Ok(None);
        }
        let [value] = args else {
            return Err(format!("{name} requires one value"));
        };
        let ty = self.infer_type(value)?;
        let value = self.expression(value)?;
        if name == "deref" {
            let ScriptType::Reference { inner, .. } = ty else {
                return Err("deref requires a safe reference".into());
            };
            Ok(Some((*inner, format!("(*({value}))"))))
        } else {
            let mutable = name == "ref_mut";
            Ok(Some((
                ScriptType::Reference {
                    inner: Box::new(ty),
                    mutable,
                },
                format!("(&{}({value}))", if mutable { "mut " } else { "" }),
            )))
        }
    }
}
