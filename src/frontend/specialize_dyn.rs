//! Closed-world borrowed interface objects. The concrete tag/payload is private
//! to lowering; calls dispatch at run time and retain ordinary reference facts.
use super::*;

#[derive(Clone)]
pub(super) struct DynamicObject {
    pub interface: String,
    pub mutable: bool,
    implementations: Vec<String>,
    methods: HashMap<String, DynamicMethod>,
}
#[derive(Clone)]
struct DynamicMethod {
    symbol: String,
    mutable: bool,
    signature: Signature,
}
fn invalid(message: impl Into<String>, span: Span) -> Error {
    Error::Semantic {
        code: "E2407",
        message: message.into(),
        span,
    }
}
fn contains_self(ty: &Type) -> bool {
    match ty {
        Type::Named(n, _) => n == "Self",
        Type::Pointer(t, _)
        | Type::Volatile(t, _)
        | Type::Slice(t, _)
        | Type::Ref { inner: t, .. }
        | Type::Owned { inner: t, .. }
        | Type::Array { elem: t, .. }
        | Type::ArrayExpr { elem: t, .. } => contains_self(t),
        Type::Tuple(ts, _) | Type::Generic(_, ts, _) => ts.iter().any(contains_self),
        Type::GenericWithArgs { args, .. } => args
            .iter()
            .any(|a| matches!(a,GenericArg::Type(t) if contains_self(t))),
        Type::Function { params, ret, .. } => {
            params.iter().any(contains_self) || contains_self(ret)
        }
        _ => false,
    }
}
impl Specializer {
    pub(super) fn contains_dynamic(&self, ty: &Type) -> bool {
        fn contains(this: &Specializer, ty: &Type, seen: &mut HashSet<String>) -> bool {
            match ty {
                Type::Dyn(..) => true,
                Type::Named(name, _) => {
                    this.dynamic_objects.contains_key(name)
                        || (seen.insert(name.clone())
                            && (this
                                .aliases
                                .get(name)
                                .is_some_and(|a| contains(this, &a.ty, seen))
                                || this.structs.get(name).is_some_and(|s| {
                                    s.fields.iter().any(|f| contains(this, &f.ty, seen))
                                })
                                || this.unions.get(name).is_some_and(|u| {
                                    u.fields.iter().any(|f| contains(this, &f.ty, seen))
                                })
                                || this.enums.get(name).is_some_and(|e| {
                                    e.variants
                                        .iter()
                                        .any(|v| v.fields.iter().any(|t| contains(this, t, seen)))
                                })))
                }
                Type::Pointer(t, _)
                | Type::Volatile(t, _)
                | Type::Slice(t, _)
                | Type::Ref { inner: t, .. }
                | Type::Owned { inner: t, .. }
                | Type::Array { elem: t, .. }
                | Type::ArrayExpr { elem: t, .. } => contains(this, t, seen),
                Type::Tuple(ts, _) | Type::Generic(_, ts, _) => {
                    ts.iter().any(|t| contains(this, t, seen))
                }
                Type::GenericWithArgs { args, .. } => args
                    .iter()
                    .any(|a| matches!(a,GenericArg::Type(t) if contains(this,t,seen))),
                Type::Function { params, ret, .. } => {
                    params.iter().any(|t| contains(this, t, seen)) || contains(this, ret, seen)
                }
                _ => false,
            }
        }
        contains(self, ty, &mut HashSet::new())
    }
    pub(super) fn validate_dynamic_abi(
        &self,
        params: &[Param],
        ret: Option<&Type>,
        span: Span,
    ) -> Result<()> {
        if params.iter().any(|p| self.contains_dynamic(&p.ty))
            || ret.is_some_and(|t| self.contains_dynamic(t))
        {
            return Err(invalid("dynamic interface objects have a closed-world internal layout and cannot appear in a C ABI declaration or export",span));
        }
        Ok(())
    }

    fn dynamic_name(&mut self, kind: &str) -> String {
        let mut id = self.names.len();
        loop {
            let name = format!("__aeth_dyn_{kind}_{id}");
            if self.names.insert(name.clone()) {
                return name;
            }
            id += 1;
        }
    }
    pub(super) fn dynamic_type(
        &mut self,
        interface: &str,
        mutable: bool,
        span: Span,
    ) -> Result<Type> {
        if let Some(name) = self.dynamic_cache.get(&(interface.to_string(), mutable)) {
            return Ok(named(name, span));
        }
        let declaration = self
            .dynamic_interfaces
            .get(interface)
            .cloned()
            .ok_or_else(|| {
                invalid(
                    format!("declared interface after dyn, got '{interface}'"),
                    span,
                )
            })?;
        if !declaration.type_params.is_empty()
            || !declaration.associated_types.is_empty()
            || !declaration.supertraits.is_empty()
            || !declaration.default_methods.is_empty()
        {
            return Err(invalid("dyn requires an abstract non-generic interface without associated types, defaults or supertraits",span));
        }
        for method in &declaration.methods {
            if !matches!(method.params.first().map(|p|&p.ty),Some(Type::Ref{inner,..}) if matches!(inner.as_ref(),Type::Named(n,_) if n=="Self"))
                || method.params.iter().skip(1).any(|p| contains_self(&p.ty))
                || method.ret_type.as_ref().is_some_and(contains_self)
            {
                return Err(invalid("dynamic methods require &Self or &mut Self first, and no other Self-dependent parameter or result",method.span));
            }
        }
        let implementations = self
            .dynamic_implementations
            .get(interface)
            .cloned()
            .unwrap_or_default();
        if implementations.is_empty() {
            return Err(invalid("a closed-world dynamic interface needs at least one concrete implementation in this module graph",span));
        }
        let name = self.dynamic_name(if mutable { "mut" } else { "shared" });
        self.dynamic_cache
            .insert((interface.to_string(), mutable), name.clone());
        let targets = implementations
            .iter()
            .map(|b| self.ty(&named(&b.target.name, b.target.span), &Bindings::new()))
            .collect::<Result<Vec<_>>>()?;
        let target_names = targets
            .iter()
            .map(|t| {
                nominal_name(t)
                    .map(str::to_string)
                    .ok_or_else(|| invalid("nominal concrete implementation target", t.span()))
            })
            .collect::<Result<Vec<_>>>()?;
        self.dynamic_objects.insert(
            name.clone(),
            DynamicObject {
                interface: interface.into(),
                mutable,
                implementations: target_names,
                methods: HashMap::new(),
            },
        );
        let enumeration = EnumDef {
            name: Ident {
                name: name.clone(),
                span,
            },
            variants: implementations
                .iter()
                .enumerate()
                .map(|(i, b)| Variant {
                    name: Ident {
                        name: format!("Impl{i}"),
                        span: b.span,
                    },
                    fields: vec![Type::Ref {
                        mutable,
                        lifetime: None,
                        inner: Box::new(targets[i].clone()),
                        span: b.span,
                    }],
                    span: b.span,
                })
                .collect(),
            span,
            generic_params: vec![],
            type_params: vec![],
        };
        self.enums.insert(name.clone(), enumeration.clone());
        self.output.push(Item::Enum(enumeration));
        for method in &declaration.methods {
            let requires_mut = matches!(
                method.params.first().map(|p| &p.ty),
                Some(Type::Ref { mutable: true, .. })
            );
            if requires_mut && !mutable {
                continue;
            }
            let args = method
                .params
                .iter()
                .skip(1)
                .map(|p| self.ty(&p.ty, &Bindings::new()))
                .collect::<Result<Vec<_>>>()?;
            let ret = method
                .ret_type
                .as_ref()
                .map(|t| self.ty(t, &Bindings::new()))
                .transpose()?
                .unwrap_or(Type::Unit(method.span));
            let dispatch = self.dynamic_name("dispatch");
            let receiver = Ident {
                name: self.dynamic_name("object"),
                span: method.params[0].name.span,
            };
            let arg_names = method
                .params
                .iter()
                .skip(1)
                .map(|p| Ident {
                    name: self.dynamic_name("argument"),
                    span: p.name.span,
                })
                .collect::<Vec<_>>();
            let mut dispatch_params = vec![Param {
                name: receiver.clone(),
                ownership: Ownership::Own,
                ty: Type::Ref {
                    mutable: requires_mut,
                    lifetime: None,
                    inner: Box::new(named(&name, span)),
                    span: method.span,
                },
                span: method.span,
            }];
            dispatch_params.extend(arg_names.iter().zip(&args).map(|(n, t)| Param {
                name: n.clone(),
                ownership: Ownership::Own,
                ty: t.clone(),
                span: n.span,
            }));
            let mut arms = Vec::new();
            for (index, implementation) in implementations.iter().enumerate() {
                let thunk = self.dynamic_name("implementation");
                let implementation_method = implementation
                    .methods
                    .iter()
                    .find(|m| m.name.name == method.name.name);
                let value_span = implementation_method
                    .and_then(|m| m.params.first())
                    .map(|p| p.name.span)
                    .unwrap_or(implementation.target.span);
                let value = Ident {
                    name: self.dynamic_name("value"),
                    span: value_span,
                };
                let params = std::iter::once(Param {
                    name: value.clone(),
                    ownership: Ownership::Own,
                    ty: Type::Ref {
                        mutable: requires_mut,
                        lifetime: None,
                        inner: Box::new(targets[index].clone()),
                        span: method.span,
                    },
                    span: method.span,
                })
                .chain(arg_names.iter().zip(&args).map(|(n, t)| Param {
                    name: n.clone(),
                    ownership: Ownership::Own,
                    ty: t.clone(),
                    span: n.span,
                }))
                .collect();
                let call = Expr::Call {
                    func: Box::new(Expr::Path {
                        segments: vec![implementation.target.clone(), method.name.clone()],
                        span: method.name.span,
                    }),
                    args: std::iter::once(Expr::Ident(value.clone()))
                        .chain(arg_names.iter().cloned().map(Expr::Ident))
                        .collect(),
                    span: implementation_method
                        .map(|m| m.body.span)
                        .unwrap_or(implementation.span),
                };
                let owner = implementation
                    .methods
                    .first()
                    .and_then(|m| self.module_owners.get(&m.span))
                    .copied()
                    .unwrap_or(0);
                // Use the concrete method span so module-private access is
                // checked in its own module, not the caller's or trait's.
                let thunk_span = implementation
                    .methods
                    .iter()
                    .find(|m| m.name.name == method.name.name)
                    .map(|m| m.span)
                    .unwrap_or(implementation.span);
                self.push_dynamic_function(
                    thunk.clone(),
                    params,
                    ret.clone(),
                    call,
                    thunk_span,
                    owner,
                )?;
                let binding = Ident {
                    name: self.dynamic_name("payload"),
                    span: implementation.target.span,
                };
                let call = Expr::Call {
                    func: Box::new(Expr::Ident(Ident {
                        name: thunk,
                        span: method.span,
                    })),
                    args: std::iter::once(Expr::Ident(binding.clone()))
                        .chain(arg_names.iter().cloned().map(Expr::Ident))
                        .collect(),
                    span: implementation.span,
                };
                arms.push(MatchArm {
                    pattern: Pattern::Variant {
                        enum_name: Some(Ident {
                            name: name.clone(),
                            span,
                        }),
                        variant: Ident {
                            name: format!("Impl{index}"),
                            span,
                        },
                        fields: vec![Pattern::Binding {
                            name: binding,
                            mutable: false,
                            span,
                        }],
                        span,
                    },
                    guard: None,
                    body: call,
                    span: method.span,
                });
            }
            let body = Expr::Match {
                expr: Box::new(Expr::Deref {
                    expr: Box::new(Expr::Ident(receiver)),
                    span: method.params[0].span,
                }),
                arms,
                span: method.span,
            };
            let owner = self.module_owners.get(&method.span).copied().unwrap_or(0);
            self.push_dynamic_function(
                dispatch.clone(),
                dispatch_params,
                ret.clone(),
                body,
                method.span,
                owner,
            )?;
            self.dynamic_objects.get_mut(&name).unwrap().methods.insert(
                method.name.name.clone(),
                DynamicMethod {
                    symbol: dispatch,
                    mutable: requires_mut,
                    signature: Signature { params: args, ret },
                },
            );
        }
        Ok(named(&name, span))
    }
    fn push_dynamic_function(
        &mut self,
        name: String,
        params: Vec<Param>,
        ret: Type,
        body: Expr,
        span: Span,
        owner: usize,
    ) -> Result<()> {
        let mut function = Function {
            name: Ident {
                name: name.clone(),
                span,
            },
            generic_params: vec![],
            params,
            ret_type: Some(ret),
            body: Block {
                stmts: vec![Stmt::Expr(body)],
                span,
            },
            span,
            annotations: vec![],
            contracts: vec![],
            effects: EffectSet::default(),
            is_pub: false,
            type_params: vec![],
        };
        self.signature(&mut function, &Bindings::new())?;
        self.dynamic_functions.insert(name);
        self.module_owners.insert(span, owner);
        self.output.push(Item::Function(function));
        Ok(())
    }
    pub(super) fn dynamic_info(&self, ty: &Type) -> Option<&DynamicObject> {
        match ty {
            Type::Named(name, _) => self.dynamic_objects.get(name),
            Type::Ref { inner, .. } => self.dynamic_info(inner),
            _ => None,
        }
    }
    pub(super) fn dynamic_method_mutable(&self, source: &Type, method: &Ident) -> Option<bool> {
        self.dynamic_info(source)?
            .methods
            .get(&method.name)
            .map(|m| m.mutable)
    }
    pub(super) fn dynamic_cast(
        &mut self,
        expression: &mut Expr,
        target: &Type,
    ) -> Result<Option<Type>> {
        let Expr::Cast { expr, span, .. } = expression else {
            unreachable!()
        };
        let span = *span;
        let info = self.dynamic_info(target).cloned().expect("dynamic target");
        let source = self
            .expr(expr, None)?
            .ok_or_else(|| invalid("a typed concrete reference for dynamic conversion", span))?;
        let Type::Ref { inner, mutable, .. } = &source else {
            return Err(invalid(
                "dynamic conversion requires a safe reference to a concrete implementation",
                span,
            ));
        };
        if info.mutable && !*mutable {
            return Err(invalid(
                "&mut dyn requires an exclusive concrete reference",
                span,
            ));
        }
        let target_name = nominal_name(inner)
            .ok_or_else(|| invalid("a nominal concrete implementation for dyn", span))?;
        let index = info
            .implementations
            .iter()
            .position(|name| name == target_name)
            .ok_or_else(|| {
                invalid(
                    format!("'{target_name}' does not implement '{}'", info.interface),
                    span,
                )
            })?;
        let name = nominal_name(target).unwrap();
        *expression = Expr::Call {
            func: Box::new(Expr::Path {
                segments: vec![
                    Ident {
                        name: name.into(),
                        span,
                    },
                    Ident {
                        name: format!("Impl{index}"),
                        span,
                    },
                ],
                span,
            }),
            args: vec![expr.as_ref().clone()],
            span,
        };
        Ok(Some(target.clone()))
    }
    pub(super) fn dynamic_call(
        &mut self,
        expression: &mut Expr,
        source: &Type,
    ) -> Result<Option<Type>> {
        let Expr::MethodCall {
            expr,
            method,
            args,
            span,
        } = expression
        else {
            unreachable!()
        };
        let span = *span;
        let info = self.dynamic_info(source).unwrap();
        let entry = info.methods.get(&method.name).cloned().ok_or_else(|| {
            invalid(
                format!(
                    "method '{}' must exist on this interface and be callable through {} dyn",
                    method.name,
                    if info.mutable { "exclusive" } else { "shared" }
                ),
                method.span,
            )
        })?;
        for (index, arg) in args.iter_mut().enumerate() {
            self.expr(arg, entry.signature.params.get(index))?;
        }
        let mut place = expr.as_ref().clone();
        let mut temporary = None;
        if !matches!(
            place,
            Expr::Ident(_) | Expr::Field { .. } | Expr::Index { .. } | Expr::Deref { .. }
        ) {
            let name = Ident {
                name: self.dynamic_name("temporary"),
                span,
            };
            temporary = Some(Stmt::Let {
                name: name.clone(),
                mutable: entry.mutable,
                ty: Some(source.clone()),
                value: Some(place),
                span,
            });
            place = Expr::Ident(name);
        }
        let mut ty = source;
        while let Type::Ref { inner, .. } = ty {
            place = Expr::Deref {
                expr: Box::new(place),
                span,
            };
            ty = inner;
        }
        let receiver = Expr::Ref {
            mutable: entry.mutable,
            expr: Box::new(place),
            span,
        };
        let call = Expr::Call {
            func: Box::new(Expr::Ident(Ident {
                name: entry.symbol,
                span,
            })),
            args: std::iter::once(receiver)
                .chain(std::mem::take(args))
                .collect(),
            span,
        };
        *expression = if let Some(temporary) = temporary {
            Expr::Block(Block {
                stmts: vec![temporary, Stmt::Expr(call)],
                span,
            })
        } else {
            call
        };
        Ok(Some(entry.signature.ret))
    }
}
