//! Concrete closure environments. Captures are ordinary value fields, so the
//! existing aggregate loan/escape analysis remains the authority on lifetimes.
//! A closure call is a direct call with a borrowed environment; it does not
//! change the representation or calling convention of C function pointers.
use super::*;

#[derive(Clone)]
pub(super) struct ClosureInfo {
    invoke: String,
    mutable: bool,
    signature: Signature,
}

fn invalid(message: &str, span: Span) -> Error {
    Error::Semantic {
        code: "E2403",
        message: message.into(),
        span,
    }
}

impl Specializer {
    pub(super) fn closure_info(&self, ty: &Type) -> Option<&ClosureInfo> {
        match ty {
            Type::Named(name, _) => self.closures.get(name),
            Type::Ref { inner, .. } => self.closure_info(inner),
            _ => None,
        }
    }

    pub(super) fn contains_closure(&self, ty: &Type) -> bool {
        match ty {
            Type::Named(name, _) => {
                self.closures.contains_key(name)
                    || self.structs.get(name).is_some_and(|d| {
                        d.fields
                            .iter()
                            .any(|f| self.closure_field_type(&f.ty, &mut HashSet::new()))
                    })
            }
            Type::Ref { inner, .. } | Type::Array { elem: inner, .. } => {
                self.contains_closure(inner)
            }
            Type::Tuple(fields, _) => fields.iter().any(|t| self.contains_closure(t)),
            _ => false,
        }
    }

    fn closure_field_type(&self, ty: &Type, seen: &mut HashSet<String>) -> bool {
        match ty {
            Type::Named(name, _) => {
                self.closures.contains_key(name)
                    || (seen.insert(name.clone())
                        && self.structs.get(name).is_some_and(|d| {
                            d.fields
                                .iter()
                                .any(|f| self.closure_field_type(&f.ty, seen))
                        }))
            }
            Type::Ref { inner, .. } | Type::Array { elem: inner, .. } => {
                self.closure_field_type(inner, seen)
            }
            Type::Tuple(fields, _) => fields.iter().any(|t| self.closure_field_type(t, seen)),
            _ => false,
        }
    }

    pub(super) fn known_closure_method(&self, ty: &Type, method: &Ident) -> Option<Signature> {
        let mut ty = ty;
        while let Type::Ref { inner, .. } | Type::Pointer(inner, _) = ty {
            ty = inner;
        }
        let name = nominal_name(ty)?;
        let signature = self
            .closure_method_signatures
            .get(&(name.to_string(), method.name.clone()))?;
        let mut binding = Bindings::new();
        binding.insert("Self".into(), ty.clone());
        Some(Signature {
            params: signature
                .params
                .iter()
                .map(|t| substitute(t, &binding))
                .collect(),
            ret: substitute(&signature.ret, &binding),
        })
    }

    fn closure_call_writes(&self, expression: &Expr) -> bool {
        match expression {
            Expr::Call { func, .. } => self
                .hint(func)
                .as_ref()
                .and_then(|t| self.closure_info(t))
                .is_some_and(|c| c.mutable),
            Expr::MethodCall { expr, method, .. } => {
                let Some(ty) = self.hint(expr) else {
                    return true;
                };
                if let Some(mutable)=self.dynamic_method_mutable(&ty,method) {return mutable;}
                if let Some(field) = self.field_type(&ty, &method.name) {
                    if let Some(closure) = self.closure_info(&field) {
                        return closure.mutable;
                    }
                }
                let mut ty = &ty;
                while let Type::Ref { inner, .. } = ty {
                    ty = inner;
                }
                if matches!(ty, Type::Pointer(_, _)) && method.name == "add" {
                    return false;
                }
                nominal_name(ty)
                    .and_then(|name| {
                        self.closure_method_modes
                            .get(&(name.to_string(), method.name.clone()))
                    })
                    .copied()
                    .unwrap_or(true)
            }
            _ => false,
        }
    }

    fn closure_name(&mut self, kind: &str) -> String {
        let mut n = self.names.len();
        loop {
            let name = format!("__aeth_closure_{kind}_{n}");
            if !self.locals.contains_key(&name) && self.names.insert(name.clone()) {
                return name;
            }
            n += 1;
        }
    }

    pub(super) fn lower_closure(
        &mut self,
        expression: &mut Expr,
        expected: Option<&Type>,
    ) -> Result<Option<Type>> {
        let Expr::Closure {
            params,
            ret_type,
            body,
            span,
        } = expression
        else {
            unreachable!()
        };
        let span = *span;
        if matches!(expected, Some(Type::Function { .. })) {
            return Err(invalid("a captured closure has a concrete environment type; use a generic parameter instead of a C function pointer", span));
        }
        for param in params.iter_mut() {
            let ty = param.ty.as_ref().ok_or_else(|| {
                invalid("closure parameters require explicit types", param.name.span)
            })?;
            param.ty = Some(self.ty(ty, &self.bindings.clone())?);
        }
        if let Some(ty) = ret_type {
            *ty = self.ty(ty, &self.bindings.clone())?;
        }
        let mut bound: HashSet<String> = params.iter().map(|p| p.name.name.clone()).collect();
        let mut captures = Vec::<(Ident, Type, bool)>::new();
        // Scan the source before lowering nested closures. In particular, an
        // inner closure's capture initializer is still a use in this scope.
        walk_free(
            body,
            &mut bound,
            false,
            &|call| self.closure_call_writes(call),
            &mut |id, write, binding| {
                if !binding && self.local_names.contains(&id.name) {
                    if let Some((_, _, mutable)) =
                        captures.iter_mut().find(|(n, _, _)| n.name == id.name)
                    {
                        *mutable |= write;
                    } else if let Some(ty) = self.locals.get(&id.name) {
                        captures.push((id.clone(), ty.clone(), write));
                    }
                }
                None
            },
        );
        let environment = self.closure_name("env");
        let invoke = self.closure_name("invoke");
        // The environment parameter must not shadow any identifier in the
        // body, including local declarations that hide an outer capture.
        let mut identifiers = HashSet::new();
        walk_free(
            body,
            &mut HashSet::new(),
            false,
            &|_| false,
            &mut |id, _, _| {
                identifiers.insert(id.name.clone());
                None
            },
        );
        identifiers.extend(params.iter().map(|p| p.name.name.clone()));
        let mut env_parameter = self.closure_name("receiver");
        while identifiers.contains(&env_parameter) {
            env_parameter = self.closure_name("receiver");
        }
        let mutable = captures.iter().any(|(_, _, write)| *write);
        let saved_locals = self.locals.clone();
        let saved_names = self.local_names.clone();
        let saved_return = self.return_type.clone();
        for param in params.iter() {
            self.locals
                .insert(param.name.name.clone(), param.ty.clone().unwrap());
            self.local_names.insert(param.name.name.clone());
        }
        self.return_type = ret_type.clone();
        let body_type = self.expr(body, ret_type.as_ref());
        self.locals = saved_locals;
        self.local_names = saved_names;
        self.return_type = saved_return;
        let result = ret_type.clone().or(body_type?).ok_or_else(|| {
            invalid(
                "an explicit closure return type when its body cannot be inferred",
                span,
            )
        })?;
        if matches!(result, Type::Never(_)) && ret_type.is_none() {
            return Err(invalid(
                "an explicit closure return type for a body containing return",
                span,
            ));
        }
        let capture_fields: HashMap<String, String> = captures
            .iter()
            .enumerate()
            .map(|(i, (id, _, _))| (id.name.clone(), format!("capture_{i}")))
            .collect();
        let mut bound = params.iter().map(|p| p.name.name.clone()).collect();
        walk_free(
            body,
            &mut bound,
            false,
            &|_| false,
            &mut |id, _, binding| {
                if binding {
                    return None;
                }
                capture_fields.get(&id.name).map(|field| Expr::Field {
                    expr: Box::new(Expr::Ident(Ident {
                        name: env_parameter.clone(),
                        // The generated base and source field have different
                        // canonical types. Their fact keys must not alias.
                        span: Span::new(id.span.start,id.span.start,id.span.file_id),
                    })),
                    field: Ident {
                        name: field.clone(),
                        span: id.span,
                    },
                    span: id.span,
                })
            },
        );
        let definition = StructDef {
            name: Ident {
                name: environment.clone(),
                span,
            },
            fields: captures
                .iter()
                .enumerate()
                .map(|(i, (id, ty, _))| Field {
                    name: Ident {
                        name: format!("capture_{i}"),
                        span: id.span,
                    },
                    ty: ty.clone(),
                    span: id.span,
                })
                .collect(),
            span,
            annotations: vec![],
            invariants: vec![],
            is_pub: false,
            generic_params: vec![],
            type_params: vec![],
        };
        let parameter_types: Vec<Type> = params.iter().map(|p| p.ty.clone().unwrap()).collect();
        let mut parameters = vec![Param {
            mutable: false,
            name: Ident {
                name: env_parameter,
                span,
            },
            ownership: Ownership::Own,
            ty: Type::Ref {
                mutable,
                lifetime: None,
                inner: Box::new(named(&environment, span)),
                span,
            },
            span,
        }];
        parameters.extend(params.iter().map(|p| Param {
            mutable: false,
            name: p.name.clone(),
            ownership: Ownership::Own,
            ty: p.ty.clone().unwrap(),
            span: p.name.span,
        }));
        let body = match body.as_ref() {
            Expr::Block(block) => block.clone(),
            value => Block {
                stmts: vec![Stmt::Expr(value.clone())],
                span: value.span(),
            },
        };
        let mut function = Function {
            name: Ident {
                name: invoke.clone(),
                span,
            },
            generic_params: vec![],
            params: parameters,
            ret_type: Some(result.clone()),
            body,
            span,
            annotations: vec![],
            contracts: vec![],
            effects: EffectSet::default(),
            is_pub: false,
            type_params: vec![],
        };
        self.structs.insert(environment.clone(), definition.clone());
        self.signature(&mut function, &Bindings::new())?;
        self.closures.insert(
            environment.clone(),
            ClosureInfo {
                invoke,
                mutable,
                signature: Signature {
                    params: parameter_types,
                    ret: result,
                },
            },
        );
        self.module_owners.insert(span, self.current_owner);
        self.output.push(Item::Struct(definition));
        self.output.push(Item::Function(function));
        *expression = Expr::StructLit {
            name: Ident {
                name: environment.clone(),
                span,
            },
            fields: captures
                .into_iter()
                .enumerate()
                .map(|(i, (id, _, _))| {
                    (
                        Ident {
                            name: format!("capture_{i}"),
                            span: id.span,
                        },
                        Expr::Ident(id),
                    )
                })
                .collect(),
            span,
        };
        Ok(Some(named(&environment, span)))
    }

    pub(super) fn lower_closure_call(
        &mut self,
        expression: &mut Expr,
        info: &ClosureInfo,
        callable: &Type,
    ) -> Result<Option<Type>> {
        let Expr::Call { func, args, span } = expression else {
            unreachable!()
        };
        let span = *span;
        for (index, arg) in args.iter_mut().enumerate() {
            self.expr(arg, info.signature.params.get(index))?;
        }
        // Evaluate a temporary callable before its arguments, and keep its
        // storage alive through the call. References returned into that storage
        // must still satisfy the normal block escape check.
        let is_place = matches!(
            func.as_ref(),
            Expr::Ident(_) | Expr::Field { .. } | Expr::Index { .. } | Expr::Deref { .. }
        );
        let mut temporary = None;
        let mut place = func.as_ref().clone();
        if !is_place {
            let name = Ident {
                name: self.closure_name("temporary"),
                span,
            };
            temporary = Some(Stmt::Let {
                name: name.clone(),
                mutable: info.mutable,
                ty: Some(callable.clone()),
                value: Some(place),
                span,
            });
            place = Expr::Ident(name);
        }
        let mut ty = callable;
        while let Type::Ref { inner, .. } = ty {
            place = Expr::Deref {
                expr: Box::new(place),
                span,
            };
            ty = inner;
        }
        let receiver = Expr::Ref {
            mutable: info.mutable,
            expr: Box::new(place),
            span,
        };
        let mut call_args = vec![receiver];
        call_args.append(args);
        let call = Expr::Call {
            func: Box::new(Expr::Ident(Ident {
                name: info.invoke.clone(),
                span,
            })),
            args: call_args,
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
        Ok(Some(info.signature.ret.clone()))
    }
}

fn pattern_names(pattern: &Pattern, names: &mut HashSet<String>) {
    match pattern {
        Pattern::Binding { name, .. } => {
            names.insert(name.name.clone());
        }
        Pattern::Tuple { elements, .. }
        | Pattern::Variant {
            fields: elements, ..
        } => {
            for p in elements {
                pattern_names(p, names)
            }
        }
        Pattern::Struct { fields, .. } => {
            for (_, p) in fields {
                pattern_names(p, names)
            }
        }
        _ => {}
    }
}

fn walk_block(
    block: &mut Block,
    bound: &mut HashSet<String>,
    call_writes: &impl Fn(&Expr) -> bool,
    visit: &mut dyn FnMut(&Ident, bool, bool) -> Option<Expr>,
) {
    let saved = bound.clone();
    for stmt in &mut block.stmts {
        match stmt {
            Stmt::Let { name, value, .. } => {
                if let Some(value) = value {
                    walk_free(value, bound, false, call_writes, visit)
                }
                visit(name, false, true);
                bound.insert(name.name.clone());
            }
            Stmt::Expr(e) | Stmt::Semi(e) => walk_free(e, bound, false, call_writes, visit),
            Stmt::Return { value: Some(e), .. } => walk_free(e, bound, false, call_writes, visit),
            _ => {}
        }
    }
    *bound = saved;
}

/// Visit free value identifiers, in source evaluation order. A write denotes
/// mutation of the captured value itself or a mutable borrow of its storage.
fn walk_free(
    expr: &mut Expr,
    bound: &mut HashSet<String>,
    write: bool,
    call_writes: &impl Fn(&Expr) -> bool,
    visit: &mut dyn FnMut(&Ident, bool, bool) -> Option<Expr>,
) {
    let receiver_write = call_writes(expr);
    match expr {
        Expr::Ident(id) => {
            if !bound.contains(&id.name) {
                if let Some(replacement) = visit(id, write, false) {
                    *expr = replacement
                }
            }
        }
        Expr::Literal(_) | Expr::Path { .. } | Expr::Layout { .. } => {}
        Expr::Unary { expr, .. } | Expr::Cast { expr, .. } | Expr::Try { expr, .. } => {
            walk_free(expr, bound, false, call_writes, visit)
        }
        Expr::Deref { expr, .. } => walk_free(expr, bound, write, call_writes, visit),
        Expr::Ref { expr, mutable, .. } => walk_free(expr, bound, *mutable, call_writes, visit),
        Expr::Field { expr, .. } => walk_free(expr, bound, write, call_writes, visit),
        Expr::Index { expr, index, .. } => {
            walk_free(expr, bound, write, call_writes, visit);
            walk_free(index, bound, false, call_writes, visit)
        }
        Expr::Binary {
            left, op, right, ..
        } => {
            walk_free(
                left,
                bound,
                op.is_assignment(),
                call_writes,
                visit,
            );
            walk_free(right, bound, false, call_writes, visit)
        }
        Expr::Call { func, args, .. } => {
            walk_free(func, bound, receiver_write, call_writes, visit);
            for e in args {
                walk_free(e, bound, false, call_writes, visit)
            }
        }
        Expr::MethodCall { expr, args, .. } => {
            walk_free(expr, bound, receiver_write, call_writes, visit);
            for e in args {
                walk_free(e, bound, false, call_writes, visit)
            }
        }
        Expr::Block(block) | Expr::Unsafe { body: block, .. } | Expr::Loop { body: block, .. } => {
            walk_block(block, bound, call_writes, visit)
        }
        Expr::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            walk_free(cond, bound, false, call_writes, visit);
            walk_block(then_block, bound, call_writes, visit);
            if let Some(b) = else_block {
                walk_block(b, bound, call_writes, visit)
            }
        }
        Expr::While { cond, body, .. } => {
            walk_free(cond, bound, false, call_writes, visit);
            walk_block(body, bound, call_writes, visit)
        }
        Expr::For {
            var, iter, body, ..
        } => {
            walk_free(iter, bound, false, call_writes, visit);
            let saved = bound.clone();
            visit(var, false, true);
            bound.insert(var.name.clone());
            walk_block(body, bound, call_writes, visit);
            *bound = saved;
        }
        Expr::Match { expr, arms, .. } => {
            walk_free(expr, bound, false, call_writes, visit);
            for arm in arms {
                let saved = bound.clone();
                pattern_names(&arm.pattern, bound);
                for name in bound.difference(&saved) {
                    visit(
                        &Ident {
                            name: name.clone(),
                            span: arm.span,
                        },
                        false,
                        true,
                    );
                }
                if let Some(g) = &mut arm.guard {
                    walk_free(g, bound, false, call_writes, visit)
                }
                walk_free(&mut arm.body, bound, false, call_writes, visit);
                *bound = saved;
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, e) in fields {
                walk_free(e, bound, false, call_writes, visit)
            }
        }
        Expr::Array { elements, .. } | Expr::Tuple { elements, .. } => {
            for e in elements {
                walk_free(e, bound, false, call_writes, visit)
            }
        }
        Expr::ArrayRepeat { value, count, .. } => {
            walk_free(value, bound, false, call_writes, visit);
            walk_free(count, bound, false, call_writes, visit)
        }
        Expr::Range { start, end, .. } => {
            if let Some(e) = start {
                walk_free(e, bound, false, call_writes, visit)
            }
            if let Some(e) = end {
                walk_free(e, bound, false, call_writes, visit)
            }
        }
        Expr::Asm { operands, .. } => {
            for o in operands {
                if let Some(e) = &mut o.expr {
                    walk_free(
                        e,
                        bound,
                        matches!(o.kind, AsmOperandKind::Output | AsmOperandKind::InOut),
                        call_writes,
                        visit,
                    )
                }
            }
        }
        Expr::Closure { params, body, .. } => {
            let saved = bound.clone();
            for p in params {
                visit(&p.name, false, true);
                bound.insert(p.name.name.clone());
            }
            // Nested closures capture these values into their own environment.
            // Their writes do not mutate this environment merely by creating it.
            walk_free(body, bound, false, call_writes, &mut |id, _, binding| {
                visit(id, false, binding)
            });
            *bound = saved;
        }
    }
}
