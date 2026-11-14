//! Type-directed monomorphization, after module resolution and before semantics.
//!
//! Templates never reach IR. Each requested type/function has an ordinary AST
//! definition with its complete concrete signature. Names encode types rather
//! than hashes; caching before expansion permits ordinary recursive functions.
use crate::frontend::ast::*;
use crate::utils::{Error, Result, Span};
use std::collections::{HashMap, HashSet};

type Bindings = HashMap<String, Type>;

pub fn specialize(program: Program) -> Result<Program> {
    Specializer::new(&program)?.run(program)
}

#[derive(Clone)]
struct Signature {
    params: Vec<Type>,
    ret: Type,
}

struct Specializer {
    structs: HashMap<String, StructDef>,
    enums: HashMap<String, EnumDef>,
    functions: HashMap<String, Function>,
    aliases: HashMap<String, TypeAliasDef>,
    signatures: HashMap<String, Signature>,
    origins: HashMap<String, (String, Vec<Type>)>,
    instances: HashMap<String, String>,
    function_bindings: HashMap<String, Bindings>,
    names: HashSet<String>,
    output: Vec<Item>,
    globals: Bindings,
    locals: Bindings,
    bindings: Bindings,
    return_type: Option<Type>,
    expansion_depth: usize,
}

impl Specializer {
    fn new(program: &Program) -> Result<Self> {
        let mut this = Self {
            structs: HashMap::new(),
            enums: HashMap::new(),
            functions: HashMap::new(),
            aliases: HashMap::new(),
            signatures: HashMap::new(),
            origins: HashMap::new(),
            instances: HashMap::new(),
            function_bindings: HashMap::new(),
            names: HashSet::new(),
            output: Vec::new(),
            globals: Bindings::new(),
            locals: Bindings::new(),
            bindings: Bindings::new(),
            return_type: None,
            expansion_depth: 0,
        };
        let mut definitions = HashSet::new();
        for item in &program.items {
            let name = match item {
                Item::Struct(d) => {
                    this.structs.insert(d.name.name.clone(), d.clone());
                    Some(&d.name)
                }
                Item::Enum(d) => {
                    this.enums.insert(d.name.name.clone(), d.clone());
                    Some(&d.name)
                }
                Item::Function(d) => {
                    this.functions.insert(d.name.name.clone(), d.clone());
                    Some(&d.name)
                }
                Item::TypeAlias(d) => {
                    this.aliases.insert(d.name.name.clone(), d.clone());
                    Some(&d.name)
                }
                Item::Extern(block) => {
                    // ABI symbols are also in the linker namespace. Compatible
                    // repeated declarations are checked later by semantics.
                    for item in &block.items {
                        let (ForeignItem::Fn { name, .. } | ForeignItem::Static { name, .. }) =
                            item;
                        this.names.insert(name.name.clone());
                    }
                    None
                }
                Item::Union(d) => Some(&d.name),
                Item::Const(d) => Some(&d.name),
                Item::Static(d) => Some(&d.name),
                _ => None,
            };
            if let Some(name) = name {
                this.names.insert(name.name.clone());
                if !definitions.insert(name.name.clone()) {
                    return Err(Error::DuplicateDefinition {
                        name: name.name.clone(),
                        span: name.span,
                    });
                }
            }
        }
        // Low-level operations use the same canonical signature inventory as
        // checking/lowering, so generic inference can consume their results.
        for name in crate::intrinsics::memory_names() {
            if let Some(spec) = crate::intrinsics::memory(&name) {
                let params = spec
                    .params()
                    .iter()
                    .map(intrinsic_type)
                    .collect::<Option<Vec<_>>>();
                if let (Some(params), Some(ret)) = (params, intrinsic_type(&spec.result())) {
                    this.signatures.insert(name, Signature { params, ret });
                }
            }
        }
        for name in crate::intrinsics::vector_names() {
            let spec = crate::intrinsics::vector(&name).expect("vector inventory");
            let params = spec.params().iter().map(intrinsic_type).collect::<Option<Vec<_>>>();
            if let (Some(params),Some(ret)) = (params,intrinsic_type(&spec.result())) {
                this.signatures.insert(name,Signature {params,ret});
            }
        }
        for name in crate::intrinsics::platform_names() {
            let spec = crate::intrinsics::platform(name).expect("platform inventory");
            this.signatures.insert(name.into(), Signature {
                params: vec![], ret: intrinsic_type(&spec.result()).expect("platform result type"),
            });
        }
        Ok(this)
    }

    fn run(mut self, program: Program) -> Result<Program> {
        // Resolve every ordinary signature before visiting any body, including
        // forward references. Requested generic definitions are appended here.
        for mut item in program.items {
            match &mut item {
                Item::Function(f) => {
                    if !f.type_params.is_empty() {
                        continue;
                    }
                    self.signature(f, &Bindings::new())?;
                }
                Item::Struct(d) => {
                    if !d.generic_params.is_empty() || !d.type_params.is_empty() {
                        continue;
                    }
                    for field in &mut d.fields {
                        field.ty = self.ty(&field.ty, &Bindings::new())?;
                    }
                    self.structs.insert(d.name.name.clone(), d.clone());
                }
                Item::Enum(d) => {
                    if !d.generic_params.is_empty() || !d.type_params.is_empty() {
                        continue;
                    }
                    for variant in &mut d.variants {
                        for field in &mut variant.fields {
                            *field = self.ty(field, &Bindings::new())?;
                        }
                    }
                    self.enums.insert(d.name.name.clone(), d.clone());
                }
                Item::Union(d) => {
                    for field in &mut d.fields {
                        field.ty = self.ty(&field.ty, &Bindings::new())?;
                    }
                }
                Item::TypeAlias(d) => {
                    if !d.type_params.is_empty() {
                        continue;
                    }
                    d.ty = self.ty(&d.ty, &Bindings::new())?;
                }
                Item::Extern(block) => {
                    for foreign in &mut block.items {
                        match foreign {
                            ForeignItem::Fn {
                                name,
                                params,
                                ret_type,
                                span,
                                ..
                            } => {
                                for p in params.iter_mut() {
                                    p.ty = self.ty(&p.ty, &Bindings::new())?;
                                }
                                if let Some(t) = ret_type {
                                    *t = self.ty(t, &Bindings::new())?;
                                }
                                self.signatures.insert(
                                    name.name.clone(),
                                    Signature {
                                        params: params.iter().map(|p| p.ty.clone()).collect(),
                                        ret: ret_type.clone().unwrap_or(Type::Unit(*span)),
                                    },
                                );
                            }
                            ForeignItem::Static { name, ty, .. } => {
                                *ty = self.ty(ty, &Bindings::new())?;
                                self.globals.insert(name.name.clone(), ty.clone());
                            }
                        }
                    }
                }
                Item::Const(d) => {
                    if let Some(t) = &mut d.ty {
                        *t = self.ty(t, &Bindings::new())?;
                        self.globals.insert(d.name.name.clone(), t.clone());
                    }
                }
                Item::Static(d) => {
                    d.ty = self.ty(&d.ty, &Bindings::new())?;
                    self.globals.insert(d.name.name.clone(), d.ty.clone());
                }
                Item::Impl(d) => {
                    for f in &mut d.methods {
                        if !f.type_params.is_empty() {
                            return Err(problem(
                                "generic methods require a generic impl representation",
                                f.span,
                            ));
                        }
                        self.signature(f, &Bindings::new())?;
                    }
                }
                _ => {}
            }
            self.output.push(item);
        }
        let mut index = 0;
        while index < self.output.len() {
            let mut item = self.output[index].clone();
            match &mut item {
                Item::Function(f) => self.function_body(f)?,
                Item::Impl(d) => {
                    for f in &mut d.methods {
                        self.function_body(f)?;
                    }
                }
                Item::Const(d) => {
                    self.locals = self.globals.clone();
                    self.expr(&mut d.value, d.ty.as_ref())?;
                }
                Item::Static(d) => {
                    if let Some(v) = &mut d.value {
                        self.locals = self.globals.clone();
                        self.expr(v, Some(&d.ty))?;
                    }
                }
                _ => {}
            }
            self.output[index] = item;
            index += 1;
        }
        Ok(Program {
            items: self.output,
            inner_attrs: program.inner_attrs,
        })
    }

    fn signature(&mut self, f: &mut Function, bindings: &Bindings) -> Result<()> {
        for p in &mut f.params {
            p.ty = self.ty(&p.ty, bindings)?;
        }
        if let Some(t) = &mut f.ret_type {
            *t = self.ty(t, bindings)?;
        }
        self.signatures.insert(
            f.name.name.clone(),
            Signature {
                params: f.params.iter().map(|p| p.ty.clone()).collect(),
                ret: f.ret_type.clone().unwrap_or(Type::Unit(f.span)),
            },
        );
        Ok(())
    }

    fn function_body(&mut self, f: &mut Function) -> Result<()> {
        self.bindings = self
            .function_bindings
            .get(&f.name.name)
            .cloned()
            .unwrap_or_default();
        self.locals = self.globals.clone();
        for p in &f.params {
            self.locals.insert(p.name.name.clone(), p.ty.clone());
        }
        self.return_type = Some(f.ret_type.clone().unwrap_or(Type::Unit(f.span)));
        let expected = self.return_type.clone();
        for c in &mut f.contracts {
            self.expr(&mut c.condition, Some(&named("bool", c.span)))?;
        }
        self.block(&mut f.body, expected.as_ref())?;
        self.bindings.clear();
        self.return_type = None;
        Ok(())
    }

    fn ty(&mut self, ty: &Type, bindings: &Bindings) -> Result<Type> {
        self.expansion_depth += 1;
        if self.expansion_depth > 128 {
            self.expansion_depth -= 1;
            return Err(problem(
                "finite type expansion (recursive alias or growing generic type)",
                ty.span(),
            ));
        }
        let result = self.ty_inner(ty, bindings);
        self.expansion_depth -= 1;
        result
    }

    fn ty_inner(&mut self, ty: &Type, bindings: &Bindings) -> Result<Type> {
        Ok(match ty {
            Type::Named(n, s) => {
                if let Some(t) = bindings.get(n) {
                    t.clone()
                } else if let Some(alias) = self.aliases.get(n).cloned() {
                    if !alias.type_params.is_empty() {
                        return Err(problem(
                            &format!("type arguments for generic alias '{n}'"),
                            *s,
                        ));
                    }
                    self.ty(&alias.ty, bindings)?
                } else {
                    if self
                        .structs
                        .get(n)
                        .is_some_and(|d| !d.generic_params.is_empty() || !d.type_params.is_empty())
                        || self.enums.get(n).is_some_and(|d| {
                            !d.generic_params.is_empty() || !d.type_params.is_empty()
                        })
                    {
                        return Err(problem(
                            &format!("type arguments for generic type '{n}'"),
                            *s,
                        ));
                    }
                    ty.clone()
                }
            }
            Type::Generic(n, args, s) => {
                let args = args
                    .iter()
                    .map(|t| self.ty(t, bindings))
                    .collect::<Result<Vec<_>>>()?;
                self.instantiate_type(n, args, *s)?
            }
            Type::GenericWithArgs { name, args, span } => {
                let args = args
                    .iter()
                    .map(|a| match a {
                        GenericArg::Type(t) => self.ty(t, bindings),
                        GenericArg::Const(_) => Err(problem(
                            "type-only generics; const generic arguments are not implemented",
                            *span,
                        )),
                    })
                    .collect::<Result<Vec<_>>>()?;
                self.instantiate_type(name, args, *span)?
            }
            Type::Pointer(t, s) => Type::Pointer(Box::new(self.ty(t, bindings)?), *s),
            Type::Ref {
                mutable,
                lifetime,
                inner,
                span,
            } => Type::Ref {
                mutable: *mutable,
                lifetime: lifetime.clone(),
                inner: Box::new(self.ty(inner, bindings)?),
                span: *span,
            },
            Type::Array { elem, size, span } => Type::Array {
                elem: Box::new(self.ty(elem, bindings)?),
                size: *size,
                span: *span,
            },
            Type::Slice(t, s) => Type::Slice(Box::new(self.ty(t, bindings)?), *s),
            Type::Tuple(ts, s) if ts.is_empty() => Type::Unit(*s),
            Type::Tuple(ts, s) => Type::Tuple(
                ts.iter()
                    .map(|t| self.ty(t, bindings))
                    .collect::<Result<_>>()?,
                *s,
            ),
            Type::Function { params, ret, span } => Type::Function {
                params: params
                    .iter()
                    .map(|t| self.ty(t, bindings))
                    .collect::<Result<_>>()?,
                ret: Box::new(self.ty(ret, bindings)?),
                span: *span,
            },
            Type::Owned {
                inner,
                ownership,
                span,
            } => Type::Owned {
                inner: Box::new(self.ty(inner, bindings)?),
                ownership: *ownership,
                span: *span,
            },
            Type::Volatile(t, s) => Type::Volatile(Box::new(self.ty(t, bindings)?), *s),
            _ => ty.clone(),
        })
    }

    fn instantiate_type(&mut self, base: &str, args: Vec<Type>, span: Span) -> Result<Type> {
        if let Some(alias) = self.aliases.get(base).cloned() {
            let bindings = bind_args(&alias.type_params, &args, base, span)?;
            return self.ty(&alias.ty, &bindings);
        }
        let key = instance_key(base, &args);
        if let Some(name) = self.instances.get(&key) {
            return Ok(named(name, span));
        }
        let mut definition = if let Some(d) = self.structs.get(base) {
            Item::Struct(d.clone())
        } else if let Some(d) = self.enums.get(base) {
            Item::Enum(d.clone())
        } else {
            return Err(Error::UndefinedType {
                name: base.to_string(),
                span,
            });
        };
        let params = match &definition {
            Item::Struct(d) => type_params(&d.generic_params, &d.type_params, span)?,
            Item::Enum(d) => type_params(&d.generic_params, &d.type_params, span)?,
            _ => unreachable!(),
        };
        if params.is_empty() {
            return Err(problem(
                &format!("non-generic type '{base}' without type arguments"),
                span,
            ));
        }
        let bindings = bind_args(&params, &args, base, span)?;
        let name = self.instance_name(base, &args, span)?;
        self.instances.insert(key, name.clone());
        self.origins.insert(name.clone(), (base.to_string(), args));
        match &mut definition {
            Item::Struct(d) => {
                d.name.name = name.clone();
                d.generic_params.clear();
                d.type_params.clear();
                for field in &mut d.fields {
                    field.ty = self.ty(&field.ty, &bindings)?;
                }
                self.structs.insert(name.clone(), d.clone());
            }
            Item::Enum(d) => {
                d.name.name = name.clone();
                d.generic_params.clear();
                d.type_params.clear();
                for variant in &mut d.variants {
                    for field in &mut variant.fields {
                        *field = self.ty(field, &bindings)?;
                    }
                }
                self.enums.insert(name.clone(), d.clone());
            }
            _ => unreachable!(),
        }
        self.output.push(definition);
        Ok(named(&name, span))
    }

    fn instance_name(&mut self, base: &str, args: &[Type], span: Span) -> Result<String> {
        if self.instances.len() >= 1024 {
            return Err(problem(
                "at most 1024 generic instances (possible polymorphic recursion)",
                span,
            ));
        }
        let candidate = format!(
            "{base}__spec_{}",
            args.iter().map(type_key).collect::<Vec<_>>().join("__")
        );
        if candidate.len() > 16384 {
            return Err(problem(
                "bounded generic type nesting (possible polymorphic recursion)",
                span,
            ));
        }
        let mut name = candidate.clone();
        let mut suffix = 0;
        while !self.names.insert(name.clone()) {
            suffix += 1;
            name = format!("{candidate}_d{suffix}");
        }
        Ok(name)
    }

    fn instantiate_function(
        &mut self,
        template: &Function,
        bindings: &Bindings,
        span: Span,
    ) -> Result<String> {
        let args = template.type_params.iter().map(|p| bindings.get(&p.name).cloned().ok_or_else(|| problem(&format!("a concrete type for '{}' in call to '{}'; add an argument or result type annotation", p.name, template.name.name), span))).collect::<Result<Vec<_>>>()?;
        let key = instance_key(&template.name.name, &args);
        if let Some(name) = self.instances.get(&key) {
            return Ok(name.clone());
        }
        let name = self.instance_name(&template.name.name, &args, span)?;
        self.instances.insert(key, name.clone());
        let mut function = template.clone();
        function.name.name = name.clone();
        function.type_params.clear();
        self.signature(&mut function, bindings)?;
        self.function_bindings
            .insert(name.clone(), bindings.clone());
        self.output.push(Item::Function(function));
        Ok(name)
    }

    fn block(&mut self, block: &mut Block, expected: Option<&Type>) -> Result<Option<Type>> {
        let saved = self.locals.clone();
        let mut result = Some(Type::Unit(block.span));
        let count = block.stmts.len();
        for (index, stmt) in block.stmts.iter_mut().enumerate() {
            match stmt {
                Stmt::Let {
                    name, ty, value, ..
                } => {
                    if let Some(t) = ty {
                        *t = self.ty(t, &self.bindings.clone())?;
                    }
                    let inferred = if let Some(v) = value {
                        self.expr(v, ty.as_ref())?
                    } else {
                        None
                    };
                    if let Some(t) = ty.clone().or(inferred) {
                        self.locals.insert(name.name.clone(), t);
                    }
                    result = Some(Type::Unit(name.span));
                }
                Stmt::Expr(e) => {
                    result = self.expr(e, if index + 1 == count { expected } else { None })?;
                }
                Stmt::Semi(e) => {
                    self.expr(e, None)?;
                    result = Some(Type::Unit(e.span()));
                }
                Stmt::Return { value, span } => {
                    let ret = self.return_type.clone();
                    if let Some(v) = value {
                        self.expr(v, ret.as_ref())?;
                    }
                    result = Some(Type::Never(*span));
                }
                _ => {}
            }
        }
        self.locals = saved;
        Ok(result)
    }

    fn expr(&mut self, expr: &mut Expr, expected: Option<&Type>) -> Result<Option<Type>> {
        let span = expr.span();
        match expr {
            Expr::Layout { ty, .. } => {
                *ty = self.ty(ty, &self.bindings.clone())?;
                Ok(Some(named("usize", span)))
            }
            Expr::Literal(l) => Ok(Some(match l {
                Literal::Int(_, _) => expected
                    .filter(|t| is_numeric(t))
                    .cloned()
                    .unwrap_or_else(|| named("i64", span)),
                Literal::Float(_, _) => expected
                    .filter(|t| is_float(t))
                    .cloned()
                    .unwrap_or_else(|| named("f64", span)),
                Literal::Bool(_, _) => named("bool", span),
                Literal::Char(_, _) => named("char", span),
                Literal::String(_, _) => Type::Pointer(Box::new(named("u8", span)), span),
            })),
            Expr::Ident(id) => {
                if let Some(template) = self
                    .functions
                    .get(&id.name)
                    .filter(|f| !f.type_params.is_empty() && !self.locals.contains_key(&id.name))
                    .cloned()
                {
                    let mut bindings = Bindings::new();
                    if let Some(Type::Function { params, ret, .. }) = expected {
                        for (p, a) in template.params.iter().zip(params) {
                            self.unify(&p.ty, a, &template.type_params, &mut bindings, span)?;
                        }
                        if let Some(t) = &template.ret_type {
                            self.unify(t, ret, &template.type_params, &mut bindings, span)?;
                        }
                    }
                    id.name = self.instantiate_function(&template, &bindings, span)?;
                }
                Ok(self.locals.get(&id.name).cloned().or_else(|| {
                    self.signatures.get(&id.name).map(|s| Type::Function {
                        params: s.params.clone(),
                        ret: Box::new(s.ret.clone()),
                        span,
                    })
                }))
            }
            Expr::Path { segments, .. } => {
                if segments.len() == 2 && self.enums.contains_key(&segments[0].name) {
                    let (ty, _) = self.enum_constructor(segments, &mut [], expected, span)?;
                    Ok(Some(ty))
                } else {
                    Ok(None)
                }
            }
            Expr::Call { func, args, .. } => {
                if let Expr::Path { segments, .. } = func.as_mut() {
                    if segments.len() == 2 && self.enums.contains_key(&segments[0].name) {
                        return self
                            .enum_constructor(segments, args, expected, span)
                            .map(|(t, _)| Some(t));
                    }
                }
                if let Expr::Ident(id) = func.as_mut() {
                    if let Some(template) = self
                        .functions
                        .get(&id.name)
                        .filter(|f| {
                            !f.type_params.is_empty() && !self.locals.contains_key(&id.name)
                        })
                        .cloned()
                    {
                        let mut bindings = Bindings::new();
                        if let (Some(ret), Some(want)) = (&template.ret_type, expected) {
                            self.unify(ret, want, &template.type_params, &mut bindings, span)?;
                        }
                        self.infer_arguments(
                            &template
                                .params
                                .iter()
                                .map(|p| p.ty.clone())
                                .collect::<Vec<_>>(),
                            args,
                            &template.type_params,
                            &mut bindings,
                            span,
                        )?;
                        id.name = self.instantiate_function(&template, &bindings, span)?;
                        return Ok(self.signatures.get(&id.name).map(|s| s.ret.clone()));
                    }
                }
                let callable = self.expr(func, None)?;
                let signature = match callable {
                    Some(Type::Function { params, ret, .. }) => {
                        Some(Signature { params, ret: *ret })
                    }
                    _ => None,
                };
                for (index, arg) in args.iter_mut().enumerate() {
                    self.expr(arg, signature.as_ref().and_then(|s| s.params.get(index)))?;
                }
                Ok(signature.map(|s| s.ret))
            }
            Expr::Field { expr, field, .. } => {
                let source = self.expr(expr, None)?;
                Ok(source
                    .as_ref()
                    .and_then(|t| self.field_type(t, &field.name)))
            }
            Expr::Index { expr, index, .. } => {
                let source = self.expr(expr, None)?;
                self.expr(index, None)?;
                Ok(source.and_then(|t| match t {
                    Type::Array { elem, .. } | Type::Pointer(elem, _) | Type::Slice(elem, _) => {
                        Some(*elem)
                    }
                    _ => None,
                }))
            }
            Expr::Ref { mutable, expr, .. } => {
                let inner_expected = expected.and_then(pointee);
                Ok(self.expr(expr, inner_expected)?.map(|t| Type::Ref {
                    mutable: *mutable,
                    lifetime: None,
                    inner: Box::new(t),
                    span,
                }))
            }
            Expr::Deref { expr, .. } => {
                Ok(self.expr(expr, None)?.and_then(|t| pointee(&t).cloned()))
            }
            Expr::Cast { expr, ty, .. } => {
                *ty = self.ty(ty, &self.bindings.clone())?;
                self.expr(expr, None)?;
                Ok(Some(ty.clone()))
            }
            Expr::Unary { expr, .. } => self.expr(expr, expected),
            Expr::Binary {
                left, op, right, ..
            } => {
                let comparison = matches!(
                    op,
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
                );
                let hint = self.hint(left).or_else(|| self.hint(right)).or_else(|| {
                    if comparison {
                        None
                    } else {
                        expected.cloned()
                    }
                });
                let lhs = self.expr(left, hint.as_ref())?;
                let rhs_expected = lhs
                    .as_ref()
                    .filter(|t| !matches!(t, Type::Pointer(..) | Type::Ref { .. }));
                self.expr(right, rhs_expected)?;
                Ok(
                    if matches!(
                        op,
                        BinOp::Eq
                            | BinOp::Ne
                            | BinOp::Lt
                            | BinOp::Le
                            | BinOp::Gt
                            | BinOp::Ge
                            | BinOp::And
                            | BinOp::Or
                    ) {
                        Some(named("bool", span))
                    } else {
                        lhs
                    },
                )
            }
            Expr::Block(block) | Expr::Unsafe { body: block, .. } => self.block(block, expected),
            Expr::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                self.expr(cond, Some(&named("bool", span)))?;
                let then_ty = self.block(then_block, expected)?;
                let other = if let Some(block) = else_block {
                    self.block(block, expected.or(then_ty.as_ref()))?
                } else {
                    Some(Type::Unit(span))
                };
                Ok(then_ty.filter(|t| !matches!(t, Type::Never(_))).or(other))
            }
            Expr::Match { expr, arms, .. } => {
                let source = self.expr(expr, None)?;
                let saved = self.locals.clone();
                let mut result = expected.cloned();
                for arm in arms {
                    self.locals = saved.clone();
                    self.pattern(&mut arm.pattern, source.as_ref())?;
                    if let Some(guard) = &mut arm.guard {
                        self.expr(guard, Some(&named("bool", span)))?;
                    }
                    let ty = self.expr(&mut arm.body, expected.or(result.as_ref()))?;
                    if result.is_none() {
                        result = ty.filter(|t| !matches!(t, Type::Never(_)));
                    }
                }
                self.locals = saved;
                Ok(result)
            }
            Expr::StructLit { name, fields, .. } => {
                let Some(template) = self.structs.get(&name.name).cloned() else {
                    for (_, value) in fields {
                        self.expr(value, None)?;
                    }
                    return Ok(Some(named(&name.name, span)));
                };
                if !template.type_params.is_empty() || !template.generic_params.is_empty() {
                    let params =
                        type_params(&template.generic_params, &template.type_params, span)?;
                    let mut bindings = Bindings::new();
                    if let Some(want) = expected {
                        self.unify(
                            &Type::Generic(
                                name.name.clone(),
                                params.iter().map(|p| named(&p.name, p.span)).collect(),
                                span,
                            ),
                            want,
                            &params,
                            &mut bindings,
                            span,
                        )?;
                    }
                    for (field, value) in fields.iter() {
                        if let (Some(ft), Some(actual)) = (
                            template.fields.iter().find(|f| f.name.name == field.name),
                            self.hint(value),
                        ) {
                            self.unify(&ft.ty, &actual, &params, &mut bindings, span)?;
                        }
                    }
                    for (field, value) in fields.iter_mut() {
                        if let Some(ft) = template.fields.iter().find(|f| f.name.name == field.name)
                        {
                            let want = self.partial_type(&ft.ty, &params, &bindings)?;
                            if let Some(actual) = self.expr(value, want.as_ref())? {
                                self.unify(&ft.ty, &actual, &params, &mut bindings, span)?;
                            }
                        } else {
                            self.expr(value, None)?;
                        }
                    }
                    let args = required_args(&params, &bindings, &name.name, span)?;
                    if let Type::Named(concrete, _) =
                        self.instantiate_type(&name.name, args, span)?
                    {
                        name.name = concrete;
                    }
                } else {
                    for (field, value) in fields.iter_mut() {
                        let want = template
                            .fields
                            .iter()
                            .find(|f| f.name.name == field.name)
                            .map(|f| f.ty.clone());
                        self.expr(value, want.as_ref())?;
                    }
                }
                Ok(Some(named(&name.name, span)))
            }
            Expr::Array { elements, .. } => {
                let element = match expected {
                    Some(Type::Array { elem, .. }) | Some(Type::Slice(elem, _)) => {
                        Some((**elem).clone())
                    }
                    _ => elements.iter().find_map(|e| self.hint(e)),
                };
                let mut inferred = element;
                for value in elements.iter_mut() {
                    let ty = self.expr(value, inferred.as_ref())?;
                    if inferred.is_none() {
                        inferred = ty;
                    }
                }
                Ok(inferred.map(|ty| Type::Array {
                    elem: Box::new(ty),
                    size: elements.len(),
                    span,
                }))
            }
            Expr::Tuple { elements, .. } => {
                if elements.is_empty() {
                    return Ok(Some(Type::Unit(span)));
                }
                let mut types = Vec::new();
                for (index, value) in elements.iter_mut().enumerate() {
                    let want = match expected {
                        Some(Type::Tuple(ts, _)) => ts.get(index),
                        _ => None,
                    };
                    if let Some(ty) = self.expr(value, want)? {
                        types.push(ty);
                    }
                }
                Ok(if types.len() == elements.len() {
                    Some(Type::Tuple(types, span))
                } else {
                    None
                })
            }
            Expr::Loop { body, .. } => {
                self.block(body, None)?;
                Ok(Some(Type::Unit(span)))
            }
            Expr::While { cond, body, .. } => {
                self.expr(cond, Some(&named("bool", span)))?;
                self.block(body, None)?;
                Ok(Some(Type::Unit(span)))
            }
            Expr::For {
                var, iter, body, ..
            } => {
                self.expr(iter, None)?;
                let saved = self.locals.clone();
                self.locals.insert(var.name.clone(), named("i64", var.span));
                self.block(body, None)?;
                self.locals = saved;
                Ok(Some(Type::Unit(span)))
            }
            Expr::Range { start, end, .. } => {
                if let Some(e) = start {
                    self.expr(e, None)?;
                }
                if let Some(e) = end {
                    self.expr(e, None)?;
                }
                Ok(None)
            }
            Expr::Try { expr, .. } => {
                let t = self.expr(expr, None)?;
                Ok(t.and_then(|t| {
                    self.enum_variant_fields(&t, "Ok")
                        .and_then(|v| v.into_iter().next())
                }))
            }
            Expr::MethodCall { expr, args, .. } => {
                self.expr(expr, None)?;
                for arg in args {
                    self.expr(arg, None)?;
                }
                Ok(None)
            }
            Expr::Asm { operands, .. } => {
                for operand in operands {
                    if let Some(e) = &mut operand.expr {
                        self.expr(e, None)?;
                    }
                }
                Ok(Some(Type::Unit(span)))
            }
            Expr::Closure {
                params,
                ret_type,
                body,
                ..
            } => {
                let saved = self.locals.clone();
                for p in params.iter_mut() {
                    if let Some(t) = &mut p.ty {
                        *t = self.ty(t, &self.bindings.clone())?;
                        self.locals.insert(p.name.name.clone(), t.clone());
                    }
                }
                if let Some(t) = ret_type {
                    *t = self.ty(t, &self.bindings.clone())?;
                }
                self.expr(body, ret_type.as_ref())?;
                self.locals = saved;
                Ok(None)
            }
        }
    }

    fn infer_arguments(
        &mut self,
        types: &[Type],
        args: &mut [Expr],
        params: &[Ident],
        bindings: &mut Bindings,
        span: Span,
    ) -> Result<()> {
        // Infer from typed expressions first: f(1, byte) must choose u8, not i64.
        for (ty, arg) in types.iter().zip(args.iter()) {
            if let Some(actual) = self.hint(arg) {
                self.unify(ty, &actual, params, bindings, span)?;
            }
        }
        let mut processed = vec![false; args.len()];
        // Infer concrete/nested typed arguments before defaulting numeric
        // literals, and defer context-only values such as Option::None until
        // a later argument has supplied their missing type.
        for phase in 0..4 {
            for (index, (ty, arg)) in types.iter().zip(args.iter_mut()).enumerate() {
                if processed[index] {
                    continue;
                }
                let want = self.partial_type(ty, params, bindings)?;
                let ready = match phase {
                    0 => want.is_some(),
                    1 => !weak_numeric(arg) && !self.needs_context(arg),
                    2 => weak_numeric(arg),
                    _ => true,
                };
                if !ready {
                    continue;
                }
                if let Some(actual) = self.expr(arg, want.as_ref())? {
                    self.unify(ty, &actual, params, bindings, span)?;
                }
                processed[index] = true;
            }
        }
        // Do not hide arity errors; semantic analysis owns the final signature.
        for arg in args.iter_mut().skip(types.len()) {
            self.expr(arg, None)?;
        }
        Ok(())
    }

    fn needs_context(&self, expression: &Expr) -> bool {
        let path = match expression {
            Expr::Path { segments, .. } => Some(segments),
            Expr::Call { func, .. } => {
                if let Expr::Path { segments, .. } = &**func {
                    Some(segments)
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(segments) = path {
            if segments.len() == 2 {
                if let Some(definition) = self.enums.get(&segments[0].name) {
                    if let Some(variant) = definition
                        .variants
                        .iter()
                        .find(|v| v.name.name == segments[1].name)
                    {
                        return definition.type_params.iter().any(|parameter| {
                            !variant.fields.iter().any(|ty| {
                                has_unbound(ty, std::slice::from_ref(parameter), &Bindings::new())
                            })
                        });
                    }
                }
            }
        }
        if let Expr::Call { func, .. } = expression {
            if let Expr::Ident(name) = &**func {
                if let Some(function) = self.functions.get(&name.name) {
                    return function.type_params.iter().any(|parameter| {
                        !function.params.iter().any(|p| {
                            has_unbound(&p.ty, std::slice::from_ref(parameter), &Bindings::new())
                        })
                    });
                }
            }
        }
        false
    }

    fn enum_constructor(
        &mut self,
        segments: &mut [Ident],
        args: &mut [Expr],
        expected: Option<&Type>,
        span: Span,
    ) -> Result<(Type, Vec<Type>)> {
        let template = self.enums[&segments[0].name].clone();
        let variant = template
            .variants
            .iter()
            .find(|v| v.name.name == segments[1].name)
            .ok_or_else(|| {
                problem(
                    &format!(
                        "variant '{}' in enum '{}'",
                        segments[1].name, template.name.name
                    ),
                    span,
                )
            })?
            .clone();
        if args.len() != variant.fields.len() {
            return Err(Error::ArgCountMismatch {
                func_name: format!("{}::{}", template.name.name, variant.name.name),
                expected: variant.fields.len(),
                got: args.len(),
                span,
            });
        }
        if !template.generic_params.is_empty() || !template.type_params.is_empty() {
            let params = type_params(&template.generic_params, &template.type_params, span)?;
            let mut bindings = Bindings::new();
            if let Some(want) = expected {
                self.unify(
                    &Type::Generic(
                        template.name.name.clone(),
                        params.iter().map(|p| named(&p.name, p.span)).collect(),
                        span,
                    ),
                    want,
                    &params,
                    &mut bindings,
                    span,
                )?;
            }
            self.infer_arguments(&variant.fields, args, &params, &mut bindings, span)?;
            let arguments = required_args(&params, &bindings, &template.name.name, span)?;
            let ty = self.instantiate_type(&template.name.name, arguments, span)?;
            if let Type::Named(name, _) = &ty {
                segments[0].name = name.clone();
            }
            let fields = self
                .enum_variant_fields(&ty, &variant.name.name)
                .unwrap_or_default();
            Ok((ty, fields))
        } else {
            for (arg, field) in args.iter_mut().zip(&variant.fields) {
                self.expr(arg, Some(field))?;
            }
            Ok((named(&template.name.name, span), variant.fields))
        }
    }

    fn pattern(&mut self, pattern: &mut Pattern, source: Option<&Type>) -> Result<()> {
        match pattern {
            Pattern::Binding { name, .. } => {
                if let Some(t) = source {
                    self.locals.insert(name.name.clone(), t.clone());
                }
            }
            Pattern::Variant {
                enum_name,
                variant,
                fields,
                span,
            } => {
                if let Some(t) = source {
                    if let Some(actual) = nominal_name(t) {
                        if let Some(original) = enum_name {
                            let base = self
                                .origins
                                .get(actual)
                                .map(|x| x.0.as_str())
                                .unwrap_or(actual);
                            if original.name != actual && original.name != base {
                                return Err(problem(
                                    &format!(
                                        "pattern for enum '{actual}', not '{}'",
                                        original.name
                                    ),
                                    *span,
                                ));
                            }
                            original.name = actual.to_string();
                        } else {
                            *enum_name = Some(Ident {
                                name: actual.to_string(),
                                span: *span,
                            });
                        }
                        if let Some(types) = self.enum_variant_fields(t, &variant.name) {
                            for (field, ty) in fields.iter_mut().zip(types) {
                                self.pattern(field, Some(&ty))?;
                            }
                        }
                    }
                }
            }
            Pattern::Tuple { elements, .. } => {
                if let Some(Type::Tuple(types, _)) = source {
                    for (p, t) in elements.iter_mut().zip(types) {
                        self.pattern(p, Some(t))?;
                    }
                }
            }
            Pattern::Struct { name, fields, .. } => {
                if let Some(t) = source {
                    if let Some(actual) = nominal_name(t) {
                        name.name = actual.to_string();
                    }
                    for (field, p) in fields {
                        let ty = self.field_type(t, &field.name);
                        self.pattern(p, ty.as_ref())?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn field_type(&self, source: &Type, field: &str) -> Option<Type> {
        let name = nominal_name(source)?;
        self.structs
            .get(name)?
            .fields
            .iter()
            .find(|f| f.name.name == field)
            .map(|f| f.ty.clone())
    }
    fn enum_variant_fields(&self, source: &Type, variant: &str) -> Option<Vec<Type>> {
        self.enums
            .get(nominal_name(source)?)?
            .variants
            .iter()
            .find(|v| v.name.name == variant)
            .map(|v| v.fields.clone())
    }

    /// A non-mutating hint deliberately excludes unsuffixed numeric literals.
    fn hint(&self, expr: &Expr) -> Option<Type> {
        match expr {
            Expr::Ident(id) => self.locals.get(&id.name).cloned(),
            Expr::Cast { ty, .. } => Some(substitute(ty, &self.bindings)),
            Expr::Layout { span, .. } => Some(named("usize", *span)),
            Expr::Literal(Literal::Bool(_, span)) => Some(named("bool", *span)),
            Expr::Literal(Literal::String(_, span)) => {
                Some(Type::Pointer(Box::new(named("u8", *span)), *span))
            }
            Expr::Literal(Literal::Char(_, span)) => Some(named("char", *span)),
            Expr::Ref {
                expr,
                mutable,
                span,
            } => self.hint(expr).map(|t| Type::Ref {
                mutable: *mutable,
                lifetime: None,
                inner: Box::new(t),
                span: *span,
            }),
            Expr::Deref { expr, .. } => self.hint(expr).and_then(|t| pointee(&t).cloned()),
            Expr::Field { expr, field, .. } => self
                .hint(expr)
                .and_then(|t| self.field_type(&t, &field.name)),
            Expr::Index { expr, .. } => self.hint(expr).and_then(|t| match t {
                Type::Array { elem, .. } | Type::Pointer(elem, _) | Type::Slice(elem, _) => {
                    Some(*elem)
                }
                _ => None,
            }),
            Expr::Call { func, .. } => {
                if let Expr::Ident(id) = &**func {
                    self.signatures.get(&id.name).map(|s| s.ret.clone())
                } else {
                    None
                }
            }
            Expr::Unary { expr, .. } => self.hint(expr),
            Expr::Binary {
                left,
                op,
                right,
                span,
            } => {
                if matches!(
                    op,
                    BinOp::Eq
                        | BinOp::Ne
                        | BinOp::Lt
                        | BinOp::Le
                        | BinOp::Gt
                        | BinOp::Ge
                        | BinOp::And
                        | BinOp::Or
                ) {
                    Some(named("bool", *span))
                } else {
                    self.hint(left).or_else(|| self.hint(right))
                }
            }
            _ => None,
        }
    }

    fn partial_type(
        &mut self,
        ty: &Type,
        params: &[Ident],
        bindings: &Bindings,
    ) -> Result<Option<Type>> {
        if has_unbound(ty, params, bindings) {
            Ok(None)
        } else {
            self.ty(ty, bindings).map(Some)
        }
    }

    fn unify(
        &mut self,
        template: &Type,
        actual: &Type,
        params: &[Ident],
        bindings: &mut Bindings,
        span: Span,
    ) -> Result<()> {
        self.expansion_depth += 1;
        if self.expansion_depth > 128 {
            self.expansion_depth -= 1;
            return Err(problem("finite generic inference (recursive alias)", span));
        }
        let result = self.unify_inner(template, actual, params, bindings, span);
        self.expansion_depth -= 1;
        result
    }

    fn unify_inner(
        &mut self,
        template: &Type,
        actual: &Type,
        params: &[Ident],
        bindings: &mut Bindings,
        span: Span,
    ) -> Result<()> {
        // Generic aliases are transparent to inference, including aliases in
        // parameter positions (e.g. Link<T> = *Node<T>).
        if let Type::Generic(name, args, _) = template {
            if let Some(alias) = self.aliases.get(name).cloned() {
                let alias_bindings = bind_args(&alias.type_params, args, name, span)?;
                let expanded = substitute(&alias.ty, &alias_bindings);
                if type_key(&expanded) == type_key(template) {
                    return Err(problem("non-recursive generic alias", span));
                }
                return self.unify(&expanded, actual, params, bindings, span);
            }
        }
        if let Type::Named(name, _) = template {
            if !params.iter().any(|p| p.name == *name) {
                if let Some(alias) = self.aliases.get(name).cloned() {
                    if alias.type_params.is_empty() {
                        let concrete = self.ty(&alias.ty, &Bindings::new())?;
                        return self.unify(&concrete, actual, params, bindings, span);
                    }
                }
            }
            if params.iter().any(|p| p.name == *name) {
                let actual = self.ty(actual, &self.bindings.clone())?;
                if let Some(previous) = bindings.get(name) {
                    if type_key(previous) != type_key(&actual) {
                        return Err(Error::TypeMismatch {
                            expected: format!("{} for generic {name}", type_key(previous)),
                            got: type_key(&actual),
                            span,
                        });
                    }
                } else {
                    bindings.insert(name.clone(), actual);
                }
                return Ok(());
            }
        }
        match (template, actual) {
            (Type::Generic(base, args, _), Type::Named(name, _)) => {
                if let Some((origin, actual_args)) = self.origins.get(name).cloned() {
                    if *base == origin {
                        for (a, b) in args.iter().zip(&actual_args) {
                            self.unify(a, b, params, bindings, span)?;
                        }
                    }
                }
            }
            (Type::Generic(base, args, _), Type::Generic(actual_base, actual_args, _))
                if base == actual_base =>
            {
                for (a, b) in args.iter().zip(actual_args) {
                    self.unify(a, b, params, bindings, span)?;
                }
            }
            (Type::Pointer(a, _), Type::Pointer(b, _))
            | (Type::Slice(a, _), Type::Slice(b, _))
            | (Type::Volatile(a, _), Type::Volatile(b, _)) => {
                self.unify(a, b, params, bindings, span)?
            }
            (Type::Ref { inner: a, .. }, Type::Ref { inner: b, .. })
            | (Type::Owned { inner: a, .. }, Type::Owned { inner: b, .. }) => {
                self.unify(a, b, params, bindings, span)?
            }
            (Type::Array { elem: a, .. }, Type::Array { elem: b, .. }) => {
                self.unify(a, b, params, bindings, span)?
            }
            (Type::Tuple(a, _), Type::Tuple(b, _)) => {
                for (a, b) in a.iter().zip(b) {
                    self.unify(a, b, params, bindings, span)?;
                }
            }
            (
                Type::Function {
                    params: a, ret: ar, ..
                },
                Type::Function {
                    params: b, ret: br, ..
                },
            ) => {
                for (a, b) in a.iter().zip(b) {
                    self.unify(a, b, params, bindings, span)?;
                }
                self.unify(ar, br, params, bindings, span)?;
            }
            _ => {}
        }
        // Non-generic compatibility and arity remain semantic analysis' job.
        Ok(())
    }
}

fn intrinsic_type(ty: &crate::types::ResolvedType) -> Option<Type> {
    use crate::types::{PrimitiveType as P, ResolvedType as R};
    let span = Span::dummy();
    Some(match ty {
        R::Primitive(P::Unit) => Type::Unit(span),
        R::Primitive(P::Never) => Type::Never(span),
        R::Primitive(primitive) => named(&format!("{primitive:?}").to_lowercase(), span),
        R::Pointer(inner) => Type::Pointer(Box::new(intrinsic_type(inner)?), span),
        R::Vector(inner,lanes) => match inner.as_ref() {
            R::Primitive(primitive) => named(&format!("{primitive:?}x{lanes}").to_lowercase(),span),
            _ => return None,
        },
        _ => return None,
    })
}

fn weak_numeric(expression: &Expr) -> bool {
    match expression {
        Expr::Literal(Literal::Int(..) | Literal::Float(..)) => true,
        Expr::Unary { expr, .. } => weak_numeric(expr),
        Expr::Binary { left, right, .. } => weak_numeric(left) && weak_numeric(right),
        Expr::Call { args, .. } => !args.is_empty() && args.iter().all(weak_numeric),
        _ => false,
    }
}
fn problem(message: &str, span: Span) -> Error {
    Error::semantic_expected("E2100", message, span)
}
fn named(name: &str, span: Span) -> Type {
    Type::Named(name.to_string(), span)
}
fn pointee(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Pointer(t, _)
        | Type::Ref { inner: t, .. }
        | Type::Owned { inner: t, .. }
        | Type::Volatile(t, _) => Some(t),
        _ => None,
    }
}
fn nominal_name(ty: &Type) -> Option<&str> {
    match ty {
        Type::Named(n, _) => Some(n),
        Type::Pointer(t, _) | Type::Ref { inner: t, .. } | Type::Owned { inner: t, .. } => {
            nominal_name(t)
        }
        _ => None,
    }
}
fn is_numeric(ty: &Type) -> bool {
    matches!(ty, Type::Named(n, _) if matches!(n.as_str(), "i8"|"u8"|"i16"|"u16"|"i32"|"u32"|"i64"|"u64"|"isize"|"usize"|"f32"|"f64"))
}
fn is_float(ty: &Type) -> bool {
    matches!(ty, Type::Named(n, _) if matches!(n.as_str(), "f32"|"f64"))
}
fn type_params(generic: &[GenericParam], legacy: &[Ident], span: Span) -> Result<Vec<Ident>> {
    if generic.is_empty() {
        return Ok(legacy.to_vec());
    }
    generic
        .iter()
        .map(|p| match p {
            GenericParam::Type(id) => Ok(id.clone()),
            GenericParam::Const { .. } => Err(problem(
                "type-only generics; const generic parameters are not implemented",
                span,
            )),
        })
        .collect()
}
fn bind_args(params: &[Ident], args: &[Type], base: &str, span: Span) -> Result<Bindings> {
    if params.len() != args.len() {
        return Err(problem(
            &format!(
                "{} type arguments for '{base}', got {}",
                params.len(),
                args.len()
            ),
            span,
        ));
    }
    Ok(params
        .iter()
        .zip(args)
        .map(|(p, t)| (p.name.clone(), t.clone()))
        .collect())
}
fn required_args(
    params: &[Ident],
    bindings: &Bindings,
    base: &str,
    span: Span,
) -> Result<Vec<Type>> {
    params
        .iter()
        .map(|p| {
            bindings.get(&p.name).cloned().ok_or_else(|| {
                problem(
                    &format!(
                        "a concrete type for '{}' in '{base}'; add a type annotation",
                        p.name
                    ),
                    span,
                )
            })
        })
        .collect()
}
fn instance_key(base: &str, args: &[Type]) -> String {
    format!(
        "{base}<{}>",
        args.iter().map(type_key).collect::<Vec<_>>().join(",")
    )
}
fn type_key(ty: &Type) -> String {
    match ty {
        Type::Named(n, _) => format!("n{}_{}", n.len(), n),
        Type::Generic(n, ts, _) => format!(
            "g{}_{}_{}",
            n.len(),
            n,
            ts.iter().map(type_key).collect::<Vec<_>>().join("_")
        ),
        Type::Pointer(t, _) => format!("p{}", type_key(t)),
        Type::Ref { inner, mutable, .. } => {
            format!("r{}{}", if *mutable { "m" } else { "c" }, type_key(inner))
        }
        Type::Array { elem, size, .. } => format!("a{size}_{}", type_key(elem)),
        Type::Slice(t, _) => format!("s{}", type_key(t)),
        Type::Tuple(ts, _) => format!(
            "t{}_{}",
            ts.len(),
            ts.iter().map(type_key).collect::<Vec<_>>().join("_")
        ),
        Type::Function { params, ret, .. } => format!(
            "f{}_{}_{}",
            params.len(),
            params.iter().map(type_key).collect::<Vec<_>>().join("_"),
            type_key(ret)
        ),
        Type::Unit(_) => "unit".to_string(),
        Type::Never(_) => "never".to_string(),
        Type::Infer(_) => "infer".to_string(),
        Type::Owned {
            inner, ownership, ..
        } => format!("o{ownership:?}_{}", type_key(inner)),
        Type::Volatile(t, _) => format!("v{}", type_key(t)),
        Type::GenericWithArgs { name, args, .. } => format!("mixed{name}{args:?}"),
    }
}
fn has_unbound(ty: &Type, params: &[Ident], bindings: &Bindings) -> bool {
    match ty {
        Type::Named(n, _) => params.iter().any(|p| p.name == *n) && !bindings.contains_key(n),
        Type::Generic(_, ts, _) | Type::Tuple(ts, _) => {
            ts.iter().any(|t| has_unbound(t, params, bindings))
        }
        Type::GenericWithArgs { args, .. } => args
            .iter()
            .any(|a| matches!(a, GenericArg::Type(t) if has_unbound(t, params, bindings))),
        Type::Pointer(t, _)
        | Type::Ref { inner: t, .. }
        | Type::Array { elem: t, .. }
        | Type::Slice(t, _)
        | Type::Owned { inner: t, .. }
        | Type::Volatile(t, _) => has_unbound(t, params, bindings),
        Type::Function {
            params: ps, ret, ..
        } => {
            ps.iter().any(|t| has_unbound(t, params, bindings))
                || has_unbound(ret, params, bindings)
        }
        _ => false,
    }
}
fn substitute(ty: &Type, bindings: &Bindings) -> Type {
    match ty {
        Type::Named(n, _) => bindings.get(n).cloned().unwrap_or_else(|| ty.clone()),
        Type::Pointer(t, s) => Type::Pointer(Box::new(substitute(t, bindings)), *s),
        Type::Ref {
            inner,
            mutable,
            lifetime,
            span,
        } => Type::Ref {
            inner: Box::new(substitute(inner, bindings)),
            mutable: *mutable,
            lifetime: lifetime.clone(),
            span: *span,
        },
        Type::Generic(n, ts, s) => Type::Generic(
            n.clone(),
            ts.iter().map(|t| substitute(t, bindings)).collect(),
            *s,
        ),
        Type::Array { elem, size, span } => Type::Array {
            elem: Box::new(substitute(elem, bindings)),
            size: *size,
            span: *span,
        },
        Type::Tuple(types, span) => Type::Tuple(
            types.iter().map(|t| substitute(t, bindings)).collect(),
            *span,
        ),
        Type::Slice(inner, span) => Type::Slice(Box::new(substitute(inner, bindings)), *span),
        Type::Volatile(inner, span) => Type::Volatile(Box::new(substitute(inner, bindings)), *span),
        Type::Owned {
            inner,
            ownership,
            span,
        } => Type::Owned {
            inner: Box::new(substitute(inner, bindings)),
            ownership: *ownership,
            span: *span,
        },
        Type::Function { params, ret, span } => Type::Function {
            params: params.iter().map(|t| substitute(t, bindings)).collect(),
            ret: Box::new(substitute(ret, bindings)),
            span: *span,
        },
        _ => ty.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::{lexer::Lexer, parser::Parser};
    fn compile(source: &str) -> Result<Program> {
        specialize(Parser::new(Lexer::new(source, 0)).parse_program()?)
    }
    fn function<'a>(program: &'a Program, prefix: &str) -> &'a Function {
        program
            .items
            .iter()
            .find_map(|item| {
                if let Item::Function(f) = item {
                    f.name.name.starts_with(prefix).then_some(f)
                } else {
                    None
                }
            })
            .unwrap()
    }
    #[test]
    fn distinct_concrete_types_and_contextual_function_results() {
        let p = compile("struct Box<T> { value: T } fn make<T>(x: T) -> Box<T> { Box { value: x } } fn main() { let a: Box<i64> = make(17); let b: Box<u8> = make(7); }").unwrap();
        let defs: Vec<_> = p
            .items
            .iter()
            .filter_map(|i| {
                if let Item::Struct(s) = i {
                    Some(s)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(defs.len(), 2);
        assert!(defs
            .iter()
            .any(|d| matches!(&d.fields[0].ty, Type::Named(n,_) if n == "i64")));
        assert!(defs
            .iter()
            .any(|d| matches!(&d.fields[0].ty, Type::Named(n,_) if n == "u8")));
        assert!(defs
            .iter()
            .all(|d| d.type_params.is_empty() && d.generic_params.is_empty()));
        assert!(function(&p, "make__spec_n3_i64").type_params.is_empty());
        assert!(function(&p, "make__spec_n2_u8").type_params.is_empty());
    }
    #[test]
    fn later_typed_argument_guides_earlier_integer_literal() {
        let p = compile("fn choose<T>(a: T, b: T) -> T { a } fn main() { let byte: u8 = 9; let x: u8 = choose(1, byte); }").unwrap();
        let f = function(&p, "choose__spec_");
        assert!(matches!(&f.params[0].ty, Type::Named(n, _) if n == "u8"));
    }
    #[test]
    fn recursive_generic_function_uses_one_cached_instance() {
        let p = compile("fn again<T>(x: T, n: i64) -> T { if n == 0 { return x; } again(x, n - 1) } fn main() -> i64 { again(42, 5) }").unwrap();
        assert_eq!(
            p.items
                .iter()
                .filter(
                    |i| matches!(i, Item::Function(f) if f.name.name.starts_with("again__spec_"))
                )
                .count(),
            1
        );
    }
    #[test]
    fn enum_constructor_and_pattern_retain_real_payload_types() {
        let p = compile("enum Result<T,E> { Ok(T), Err(E) } fn read<T>(r: Result<T,i32>, fallback: T) -> T { match r { Result::Ok(v) => v, Result::Err(e) => fallback, } } fn main() -> i64 { let r: Result<i64,i32> = Result::Ok(41); read(r, 7) }").unwrap();
        let f = function(&p, "read__spec_");
        let Stmt::Expr(Expr::Match { arms, .. }) = &f.body.stmts[0] else {
            panic!("match retained")
        };
        assert!(
            matches!(&arms[0].pattern, Pattern::Variant { enum_name: Some(n), .. } if n.name.starts_with("Result__spec_"))
        );
        assert!(p.items.iter().any(|i| matches!(i, Item::Enum(e) if e.type_params.is_empty() && matches!(&e.variants[1].fields[0], Type::Named(n,_) if n=="i32"))));
    }
    #[test]
    fn layout_operands_are_specialized() {
        let p = compile("fn bytes<T>(p: *T) -> usize { size_of(T) } fn main() -> i64 { let p: *u8 = 0 as *u8; bytes(p) as i64 }").unwrap();
        let f = function(&p, "bytes__spec_");
        assert!(
            matches!(&f.body.stmts[0], Stmt::Expr(Expr::Layout { ty: Type::Named(n,_), .. }) if n=="u8")
        );
    }
    #[test]
    fn missing_and_conflicting_arguments_are_diagnostics() {
        let missing = compile("enum Result<T,E> { Ok(T), Err(E) } fn main() { Result::Ok(1); }")
            .unwrap_err()
            .to_string();
        assert!(missing.contains("concrete type for 'E'"), "{missing}");
        let conflict = compile(
            "fn choose<T>(a:T,b:T)->T { a } fn main() { let a:u8=1; let b:i64=2; choose(a,b); }",
        )
        .unwrap_err()
        .to_string();
        assert!(conflict.contains("Type mismatch"), "{conflict}");
    }
    #[test]
    fn aliases_and_recursive_pointer_structs_are_concrete() {
        let p = compile("struct Node<T> { value:T, next:*Node<T> } type Link<T> = *Node<T>; fn main() { let a:Link<u8> = 0 as Link<u8>; }").unwrap();
        assert_eq!(
            p.items
                .iter()
                .filter(|i| matches!(i, Item::Struct(_)))
                .count(),
            1
        );
    }
    #[test]
    fn comparisons_do_not_pass_bool_context_into_generic_operands() {
        let p = compile("fn id<T>(x:T)->T { x } fn main()->i32 { if id(7) != 7 { return 1; } 0 }")
            .unwrap();
        let f = function(&p, "id__spec_");
        assert!(matches!(&f.params[0].ty, Type::Named(n, _) if n == "i64"));
    }
    #[test]
    fn transparent_alias_parameters_infer_their_element_type() {
        let p = compile("struct Box<T>{value:T} type Link<T> = *Box<T>; fn read<T>(p:Link<T>)->T { p.value } fn main()->i64 { let value:Box<i64> = Box{value:7}; read(&value as *Box<i64>) }").unwrap();
        assert!(
            matches!(&function(&p, "read__spec_").ret_type, Some(Type::Named(n,_)) if n=="i64")
        );
    }
    #[test]
    fn a_local_value_shadows_a_generic_function() {
        let p = compile("fn id<T>(value:T)->T{value} fn main()->i64{let id:i64=7; id}").unwrap();
        assert!(!p
            .items
            .iter()
            .any(|i| matches!(i, Item::Function(f) if f.name.name.starts_with("id__spec_"))));
    }
    #[test]
    fn unit_payload_literal_and_unit_type_share_one_instance() {
        let p = compile("enum Option<T> { Some(T), None } fn main() { let value:Option<()> = Option::Some(()); }").unwrap();
        assert_eq!(
            p.items
                .iter()
                .filter(|i| matches!(i, Item::Enum(_)))
                .count(),
            1
        );
        assert!(p.items.iter().any(
            |i| matches!(i, Item::Enum(e) if matches!(&e.variants[0].fields[0], Type::Unit(_)))
        ));
    }
    #[test]
    fn generated_names_do_not_collide_with_existing_abi_symbols() {
        let p=compile("extern \"C\" { fn id__spec_n3_i64(value:i64)->i64; } fn id<T>(value:T)->T{value} fn main()->i64{id(7)}").unwrap();
        assert_eq!(function(&p, "id__spec_").name.name, "id__spec_n3_i64_d1");
    }
    #[test]
    fn delayed_constructor_and_nested_literals_use_other_arguments() {
        let p=compile("enum Option<T>{Some(T),None} fn fallback<T>(value:Option<T>,other:T)->T{other} fn id<T>(value:T)->T{value} fn choose<T>(a:T,b:T)->T{a} fn main(){fallback(Option::None, 7); choose(id(1), id(7 as u8));}").unwrap();
        assert!(matches!(&function(&p,"choose__spec_").ret_type,Some(Type::Named(n,_)) if n=="u8"));
        assert!(
            matches!(&function(&p,"fallback__spec_").ret_type,Some(Type::Named(n,_)) if n=="i64")
        );
    }
    #[test]
    fn memory_intrinsic_signatures_inform_generic_calls() {
        let p=compile("fn id<T>(value:T)->T{value} fn main()->i32{let mut value:u64=1; if id(__atomic_load_u64(&value as *u64)) != 1 {return 1;} 0}").unwrap();
        assert!(matches!(&function(&p,"id__spec_").ret_type, Some(Type::Named(n,_)) if n=="u64"));
    }
}
