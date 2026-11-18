//! Cooperative, concrete async frames. Suspensions occur at statement boundaries.
//! Ordinary source chunks retain the existing type/effect/borrow checks; only
//! generated frame transfers are unsafe. Empty/Full slots never fabricate T.
use super::*;
use crate::frontend::{lexer::Lexer, parser::Parser};

pub(super) const AWAIT: &str = "@async_await";
#[derive(Clone)]
struct Task {
    frame: String,
    poll: String,
    output: Type,
    function: Function,
}
#[derive(Clone)]
struct Slot {
    name: String,
    slot: String,
    field: String,
    ty: Type,
    raw: Type,
    mutable: bool,
    owned: bool,
}
struct Boundary {
    expression: Expr,
    output: Option<usize>,
    child: Option<(Task, usize)>,
}
struct Segment {
    statements: Vec<Stmt>,
    incoming: Vec<usize>,
    outgoing: Vec<usize>,
    boundary: Option<Boundary>,
}
fn invalid(message: impl Into<String>, span: Span) -> Error {
    Error::semantic_expected("E2406", message, span)
}
fn unit(span: Span) -> Expr {
    Expr::Tuple {
        elements: vec![],
        span,
    }
}
fn await_value(expr: &Expr) -> Option<&Expr> {
    if let Expr::Call { func, args, .. } = expr {
        if matches!(&**func,Expr::Ident(name) if name.name==AWAIT) {
            return args.first();
        }
    }
    None
}
fn asynchronous(f: &Function) -> bool {
    f.annotations.iter().any(|a| a.name.name == "async")
}

/// Run after name resolution, before generic expansion. Every emitted function
/// is ordinary Aether AST and shares both existing backends and ownership flow.
pub(super) fn lower(mut program: Program) -> Result<Program> {
    let mut tasks = HashMap::new();
    let mut occupied = program.reserved_symbols.clone();
    for item in &program.items {
        match item {
            Item::Function(f) => {
                occupied.insert(f.name.name.clone());
            }
            Item::Struct(d) => {
                occupied.insert(d.name.name.clone());
            }
            Item::Enum(d) => {
                occupied.insert(d.name.name.clone());
            }
            _ => {}
        }
    }
    let mut serial = 0usize;
    for item in &program.items {
        let Item::Function(f) = item else { continue };
        if !asynchronous(f) {
            continue;
        }
        if f.name.name == "main" {
            return Err(invalid(
                "ordinary synchronous main; drive async tasks explicitly with poll",
                f.span,
            ));
        }
        if !f.type_params.is_empty() || !f.generic_params.is_empty() {
            return Err(invalid(
                "non-generic async function in the current concrete-frame implementation",
                f.span,
            ));
        }
        if !f.contracts.is_empty()
            || f.effects.is_pure
            || !f.effects.effects.is_empty()
            || f.annotations.iter().any(|a| a.name.name != "async")
        {
            return Err(invalid("async functions cannot carry contracts, effect declarations or native entry attributes; effects belong to generated polling",f.span));
        }
        let frame = loop {
            let n = format!("__aeth_async_frame_{serial}");
            serial += 1;
            if !occupied
                .iter()
                .any(|old| old == &n || old.starts_with(&(n.clone() + "_")))
            {
                occupied.insert(n.clone());
                break n;
            }
        };
        let poll = format!("{frame}_Poll");
        tasks.insert(
            f.name.name.clone(),
            Task {
                frame,
                poll,
                output: f.ret_type.clone().unwrap_or(Type::Unit(f.span)),
                function: f.clone(),
            },
        );
    }
    if tasks.is_empty() {
        return Ok(program);
    }
    // Direct child construction gives an acyclic, statically sized frame graph.
    fn visit(
        name: &str,
        tasks: &HashMap<String, Task>,
        active: &mut HashSet<String>,
        done: &mut HashSet<String>,
    ) -> Result<()> {
        if done.contains(name) {
            return Ok(());
        }
        if !active.insert(name.into()) {
            return Err(invalid(
                "recursive async frames require an indirection and are not implemented",
                tasks[name].function.span,
            ));
        }
        if active.len() > 32 {
            return Err(invalid(
                "async nesting depth at most 32",
                tasks[name].function.span,
            ));
        }
        let mut f = tasks[name].function.clone();
        let mut calls = Vec::new();
        walk_block(&mut f.body, &mut |e| {
            if let Some(Expr::Call { func, .. }) = await_value(e) {
                if let Expr::Ident(n) = &**func {
                    if tasks.contains_key(&n.name) {
                        calls.push(n.name.clone());
                    }
                }
            }
            Ok(())
        })?;
        for child in calls {
            visit(&child, tasks, active, done)?;
        }
        active.remove(name);
        done.insert(name.into());
        Ok(())
    }
    let mut done = HashSet::new();
    for name in tasks.keys() {
        visit(name, &tasks, &mut HashSet::new(), &mut done)?;
    }
    let aliases: HashMap<_, _> = program
        .items
        .iter()
        .filter_map(|i| {
            if let Item::TypeAlias(a) = i {
                Some((a.name.name.clone(), a.clone()))
            } else {
                None
            }
        })
        .collect();
    let mut output = Vec::new();
    for item in std::mem::take(&mut program.items) {
        if let Item::Function(f) = &item {
            if let Some(task) = tasks.get(&f.name.name) {
                let owner = program.module_owners.get(&f.span).copied().unwrap_or(0);
                let mut builder = Builder::new(task.clone(), &tasks, &aliases, serial)?;
                serial += 1;
                let items = builder.generate()?;
                for i in &items {
                    let span = match i {
                        Item::Function(f) => f.span,
                        Item::Struct(d) => d.span,
                        Item::Enum(d) => d.span,
                        Item::Impl(d) => d.span,
                        _ => continue,
                    };
                    program.module_owners.insert(span, owner);
                    if let Item::Impl(d) = i {
                        for f in &d.methods {
                            program.module_owners.insert(f.span, owner);
                        }
                    }
                }
                output.extend(items);
                continue;
            }
        }
        output.push(item);
    }
    program.items = output;
    Ok(program)
}

pub(super) fn contains_task(specializer: &Specializer, ty: &Type) -> bool {
    match ty {
        Type::Owned { inner, .. } | Type::Ref { inner, .. } => contains_task(specializer, inner),
        Type::Named(name, _) => {
            specializer
                .structs
                .get(name)
                .is_some_and(|s| s.annotations.iter().any(|a| a.name.name == "async_frame"))
                || specializer.enums.get(name).is_some_and(|e| {
                    e.name.name.starts_with("__aeth_async_frame_") && e.name.name.ends_with("_Poll")
                })
        }
        _ => false,
    }
}

fn collect_bindings(block: &Block, names: &mut HashSet<String>) {
    for s in &block.stmts {
        if let Stmt::Let { name, .. } = s {
            names.insert(name.name.clone());
        }
    }
}
fn pattern_bindings(p: &Pattern, names: &mut HashSet<String>) {
    match p {
        Pattern::Binding { name, .. } => {
            names.insert(name.name.clone());
        }
        Pattern::Tuple { elements, .. }
        | Pattern::Variant {
            fields: elements, ..
        } => {
            for p in elements {
                pattern_bindings(p, names)
            }
        }
        Pattern::Struct { fields, .. } => {
            for (_, p) in fields {
                pattern_bindings(p, names)
            }
        }
        _ => {}
    }
}

struct Builder<'a> {
    task: Task,
    tasks: &'a HashMap<String, Task>,
    aliases: &'a HashMap<String, TypeAliasDef>,
    slots: Vec<Slot>,
    segments: Vec<Segment>,
    types: HashMap<String, Type>,
    chunks: HashMap<String, Vec<Stmt>>,
    expressions: HashMap<String, Expr>,
    receiver: String,
    file: usize,
    seq: usize,
    used: HashSet<String>,
}
impl<'a> Builder<'a> {
    fn new(
        task: Task,
        tasks: &'a HashMap<String, Task>,
        aliases: &'a HashMap<String, TypeAliasDef>,
        serial: usize,
    ) -> Result<Self> {
        let mut names = HashSet::new();
        for p in &task.function.params {
            names.insert(p.name.name.clone());
        }
        let mut body = task.function.body.clone();
        collect_bindings(&body, &mut names);
        walk_block(&mut body, &mut |e| {
            if let Expr::Ident(n) = e {
                names.insert(n.name.clone());
            }
            match e {
                Expr::Block(b)
                | Expr::Unsafe { body: b, .. }
                | Expr::Loop { body: b, .. }
                | Expr::While { body: b, .. } => collect_bindings(b, &mut names),
                Expr::For { var, body, .. } => {
                    names.insert(var.name.clone());
                    collect_bindings(body, &mut names);
                }
                Expr::If {
                    then_block,
                    else_block,
                    ..
                } => {
                    collect_bindings(then_block, &mut names);
                    if let Some(b) = else_block {
                        collect_bindings(b, &mut names);
                    }
                }
                Expr::Match { arms, .. } => {
                    for a in arms {
                        pattern_bindings(&a.pattern, &mut names);
                    }
                }
                Expr::Closure { params, .. } => {
                    for p in params {
                        names.insert(p.name.name.clone());
                    }
                }
                _ => {}
            }
            Ok(())
        })?;
        let mut receiver = format!("{}_receiver", task.frame);
        while names.contains(&receiver) {
            receiver.push('_');
        }
        Ok(Self {
            task,
            tasks,
            aliases,
            slots: vec![],
            segments: vec![],
            types: HashMap::new(),
            chunks: HashMap::new(),
            expressions: HashMap::new(),
            receiver,
            file: usize::MAX / 2 + serial * 65536,
            seq: 0,
            used: names,
        })
    }
    fn fresh(&mut self, kind: &str) -> String {
        loop {
            self.seq += 1;
            let name = format!("{}_{}_{}", self.task.frame, kind, self.seq);
            if self.used.insert(name.clone()) {
                return name;
            }
        }
    }
    fn canonical(&self, ty: &Type, seen: &mut HashSet<String>) -> Result<Type> {
        if let Type::Named(name, span) = ty {
            if let Some(alias) = self.aliases.get(name) {
                if alias.type_params.is_empty() {
                    if !seen.insert(name.clone()) {
                        return Err(invalid("acyclic async frame type aliases", *span));
                    }
                    return self.canonical(&alias.ty, seen);
                }
            }
        }
        Ok(ty.clone())
    }
    fn type_name(&mut self, ty: Type) -> String {
        let n = self.fresh("type");
        self.types.insert(n.clone(), ty);
        n
    }
    fn expr_name(&mut self, e: Expr) -> String {
        let n = self.fresh("expr");
        self.expressions.insert(n.clone(), e);
        n
    }
    fn slot(&mut self, name: String, ty: Type, mutable: bool) -> Result<usize> {
        if self.slots.iter().any(|s| s.name == name) {
            return Err(invalid("distinct top-level async binding names; put temporary shadows in a suspension-free block",ty.span()));
        }
        let ty = self.canonical(&ty, &mut HashSet::new())?;
        if matches!(ty, Type::Infer(_)) {
            return Err(invalid(
                "explicit types on async locals that persist across suspension",
                ty.span(),
            ));
        }
        let raw = match &ty {
            Type::Owned {
                inner,
                ownership: Ownership::Own,
                ..
            } => (**inner).clone(),
            _ => ty.clone(),
        };
        let owned = matches!(
            ty,
            Type::Owned {
                ownership: Ownership::Own,
                ..
            }
        );
        let index = self.slots.len();
        let slot = self.fresh("slot");
        let field = format!("field_{index}");
        self.slots.push(Slot {
            name,
            slot,
            field,
            ty,
            raw,
            mutable,
            owned,
        });
        Ok(index)
    }
    fn split(&mut self) -> Result<()> {
        let f = self.task.function.clone();
        let mut active = Vec::new();
        for param in f.params {
            if param.ownership != Ownership::Own {
                return Err(invalid(
                    "by-value async parameters; references cannot survive suspension",
                    param.span,
                ));
            }
            active.push(self.slot(param.name.name, param.ty, false)?);
        }
        let mut incoming = active.clone();
        let mut statements = Vec::new();
        for stmt in f.body.stmts {
            let awaited = match &stmt {
                Stmt::Let { value: Some(e), .. } | Stmt::Semi(e) | Stmt::Expr(e) => {
                    await_value(e).cloned()
                }
                _ => None,
            };
            if let Some(mut expression) = awaited {
                validate_user_expr(&mut expression, 0)?;
                let output = match &stmt {
                    Stmt::Let {
                        name,
                        mutable,
                        ty: Some(ty),
                        ..
                    } => Some(self.slot(name.name.clone(), ty.clone(), *mutable)?),
                    Stmt::Let { span, .. } => {
                        return Err(invalid("explicit type for an await binding", *span))
                    }
                    _ => None,
                };
                let child = if matches!(&expression,Expr::Call{func,args,..} if args.is_empty()&&matches!(&**func,Expr::Ident(n) if n.name=="yield_now"))
                {
                    None
                } else {
                    let Expr::Call { func, .. } = &expression else {
                        return Err(invalid(
                            "await of a direct async constructor or yield_now()",
                            expression.span(),
                        ));
                    };
                    let Expr::Ident(name) = &**func else {
                        return Err(invalid(
                            "direct named async child constructor",
                            expression.span(),
                        ));
                    };
                    let child = self.tasks.get(&name.name).cloned().ok_or_else(|| {
                        invalid(
                            "await of an async function; synchronous calls are not futures",
                            expression.span(),
                        )
                    })?;
                    let hidden = self.fresh("child");
                    let index = self.slot(
                        hidden,
                        Type::Owned {
                            inner: Box::new(Type::Named(child.frame.clone(), expression.span())),
                            ownership: Ownership::Own,
                            span: expression.span(),
                        },
                        true,
                    )?;
                    Some((child, index))
                };
                if self.segments.len() >= 64 {
                    return Err(invalid(
                        "at most 64 suspension points per concrete async function",
                        expression.span(),
                    ));
                }
                self.segments.push(Segment {
                    statements: std::mem::take(&mut statements),
                    incoming,
                    outgoing: active.clone(),
                    boundary: Some(Boundary {
                        expression,
                        output,
                        child,
                    }),
                });
                if let Some(index) = output {
                    active.push(index);
                }
                incoming = active.clone();
            } else {
                if let Stmt::Let {
                    name,
                    ty,
                    value,
                    mutable,
                    span,
                } = &stmt
                {
                    if ty.is_none() || value.is_none() {
                        return Err(invalid(
                            "initialized, explicitly typed top-level async locals",
                            *span,
                        ));
                    }
                    active.push(self.slot(name.name.clone(), ty.clone().unwrap(), *mutable)?);
                }
                statements.push(stmt);
            }
        }
        self.segments.push(Segment {
            statements,
            incoming,
            outgoing: active,
            boundary: None,
        });
        Ok(())
    }
    fn restore(&mut self, index: usize) -> String {
        let s = self.slots[index].clone();
        let ty = self.type_name(s.ty.clone());
        let raw = self.fresh("restored");
        let recv = self.receiver.clone();
        let value = if s.owned {
            format!("{raw} as {ty}")
        } else {
            raw.clone()
        };
        let raw_type = self.type_name(s.raw);
        let mutable = if s.mutable { "mut " } else { "" };
        let (name, field, slot, poll) = (&s.name, &s.field, &s.slot, &self.task.poll);
        format!(
            "let {mutable}{name}: {ty} = unsafe {{
                let {raw}: {raw_type} = match {recv}.{field} {{
                    {slot}::Full(value) => value,
                    {slot}::Empty => {{ return {poll}::Finished; }},
                }};
                {recv}.{field} = {slot}::Empty;
                {value}
            }};\n"
        )
    }
    fn persist(&mut self, index: usize) -> String {
        let s = self.slots[index].clone();
        let value = if s.owned {
            format!("{} as {}", s.name, self.type_name(s.raw))
        } else {
            s.name
        };
        format!(
            "unsafe{{{}.{}={}::Full({value});}}\n",
            self.receiver, s.field, s.slot
        )
    }
    fn finish(&mut self, expr: Expr) -> Result<Vec<Stmt>> {
        let out = self.type_name(self.task.output.clone());
        let value = self.expr_name(expr);
        let result = self.fresh("result");
        let (receiver, poll) = (&self.receiver, &self.task.poll);
        let source = format!(
            "fn temp() {{
                let {result}: {out} = {value}();
                unsafe {{ {receiver}.phase = -1; }}
                return {poll}::Ready({result});
            }}"
        );
        let mut f = self.parse(&source)?.remove(0);
        let Item::Function(ref mut f) = f else {
            unreachable!()
        };
        Ok(std::mem::take(&mut f.body.stmts))
    }
    fn source_chunk(&mut self, mut statements: Vec<Stmt>, terminal: bool) -> Result<String> {
        statements.retain(|s| !matches!(s, Stmt::Empty { .. }));
        validate_user_block(
            &mut Block {
                stmts: statements.clone(),
                span: self.task.function.span,
            },
            0,
        )?;
        let final_return = matches!(statements.last(), Some(Stmt::Return { .. }));
        rewrite_returns(&mut statements, self)?;
        if terminal {
            if let Some(Stmt::Expr(value)) = statements.last().cloned() {
                statements.pop();
                statements.extend(self.finish(value)?);
            } else if !final_return {
                if matches!(&self.task.output, Type::Unit(_))
                    || matches!(&self.task.output,Type::Named(name,_) if name=="void")
                {
                    statements.extend(self.finish(unit(self.task.function.span))?);
                } else {
                    return Err(invalid(
                        "an explicit final return or value expression in an async function",
                        self.task.function.span,
                    ));
                }
            }
        }
        let name = self.fresh("chunk");
        self.chunks.insert(name.clone(), statements);
        Ok(format!("{name}();\n"))
    }
    fn generate(&mut self) -> Result<Vec<Item>> {
        self.split()?;
        let f = self.task.function.clone();
        let mut source = String::new();
        for slot in self.slots.clone() {
            let raw = self.type_name(slot.raw);
            source += &format!("enum {}{{Empty,Full({raw})}}\n", slot.slot);
        }
        let output = self.type_name(self.task.output.clone());
        source += &format!(
            "enum {}{{Pending,Ready({output}),Finished}}\n@async_frame struct {}{{phase:i32,",
            self.task.poll, self.task.frame
        );
        for slot in &self.slots {
            source += &format!("{}:{},", slot.field, slot.slot);
        }
        source += "}\n";
        let param_types: Vec<_> = f
            .params
            .iter()
            .map(|p| format!("{}:{}", p.name.name, self.type_name(p.ty.clone())))
            .collect();
        source += &format!(
            "{}fn {}({})->own {}{{unsafe{{{}{{phase:0,",
            if f.is_pub { "pub " } else { "" },
            f.name.name,
            param_types.join(","),
            self.task.frame,
            self.task.frame
        );
        for (index, s) in self.slots.clone().iter().enumerate() {
            let value = if index < f.params.len() {
                let value = if s.owned {
                    format!("{} as {}", s.name, self.type_name(s.raw.clone()))
                } else {
                    s.name.clone()
                };
                format!("{}::Full({value})", s.slot)
            } else {
                format!("{}::Empty", s.slot)
            };
            source += &format!("{}:{value},", s.field);
        }
        source += &format!("}} as own {}}}}}\n", self.task.frame);
        source += &format!(
            "@drop fn {}_drop(value:own {}){{unsafe{{",
            self.task.frame, self.task.frame
        );
        for s in self.slots.clone().iter().rev().filter(|s| s.owned) {
            let ty = self.type_name(s.ty.clone());
            source += &format!(
                "match value.{}{{{}::Full(raw)=>{{let child:{ty}=raw as {ty};}},{}::Empty=>{{}},}}",
                s.field, s.slot, s.slot
            );
        }
        source += "}}\n";
        let (frame, receiver, poll) = (&self.task.frame, &self.receiver, &self.task.poll);
        source += &format!(
            "impl {frame} {{
                pub fn cancel(self: own {frame}) {{}}
                pub fn is_finished(self: &own {frame}) -> bool {{ self.phase < 0 }}
                pub fn poll({receiver}: &mut own {frame}) -> {poll} {{
                    loop {{\n"
        );
        // Even states execute a source segment; odd states poll its stored child.
        // A Ready child advances in this same call. Pending saves the child once
        // and returns, retaining the parent's values in their existing slots.
        for (number, segment) in std::mem::take(&mut self.segments).into_iter().enumerate() {
            source += &format!("if {}.phase=={}{{\n", self.receiver, number * 2);
            for index in segment.incoming {
                source += &self.restore(index);
            }
            source += &self.source_chunk(segment.statements, segment.boundary.is_none())?;
            if let Some(boundary) = segment.boundary {
                if let Some((_, index)) = &boundary.child {
                    let name = self.slots[*index].name.clone();
                    let ty = self.type_name(self.slots[*index].ty.clone());
                    let mut expression = boundary.expression.clone();
                    rewrite_expr_returns(&mut expression, self)?;
                    let expression = self.expr_name(expression);
                    source += &format!("let mut {name}:{ty}={expression}();\n");
                }
                for index in segment.outgoing {
                    source += &self.persist(index);
                }
                if let Some((_, index)) = &boundary.child {
                    source += &self.persist(*index);
                    source += &format!(
                        "unsafe{{{}.phase={};}}continue;\n",
                        self.receiver,
                        number * 2 + 1
                    );
                } else {
                    if let Some(index) = boundary.output {
                        let s = &self.slots[index];
                        source += &format!(
                            "unsafe{{{}.{}={}::Full(());}}",
                            self.receiver, s.field, s.slot
                        );
                    }
                    source += &format!(
                        "unsafe{{{}.phase={};}}return {}::Pending;\n",
                        self.receiver,
                        (number + 1) * 2,
                        self.task.poll
                    );
                }
                source += "}\n";
                if let Some((child, index)) = boundary.child {
                    source += &format!("if {}.phase=={}{{", self.receiver, number * 2 + 1);
                    source += &self.restore(index);
                    let name = self.slots[index].name.clone();
                    source += &format!("match {name}.poll(){{{}::Pending=>{{", child.poll);
                    source += &self.persist(index);
                    source += &format!(
                        "return {}::Pending;}},{}::Ready(value)=>{{",
                        self.task.poll, child.poll
                    );
                    if let Some(index) = boundary.output {
                        let s = &self.slots[index];
                        source += &format!(
                            "unsafe{{{}.{}={}::Full(value);}}",
                            self.receiver, s.field, s.slot
                        );
                    }
                    let (receiver, child_poll, poll, next) = (
                        &self.receiver,
                        &child.poll,
                        &self.task.poll,
                        (number + 1) * 2,
                    );
                    source += &format!(
                        "unsafe {{ {receiver}.phase = {next}; }}
                         }},
                         {child_poll}::Finished => {{
                             unsafe {{ {receiver}.phase = -1; }}
                             return {poll}::Finished;
                         }},
                         }}
                         continue;
                         }}\n"
                    );
                }
            } else {
                source += "}\n";
            }
        }
        source += &format!("return {}::Finished;}}}}}}", self.task.poll);
        let mut items = self.parse(&source)?;
        // Keep the public constructor anchored at its source definition. Frame
        // shape diagnostics then point at the async function, not a virtual file.
        for item in &mut items {
            if let Item::Function(constructor) = item {
                if constructor.name.name == f.name.name {
                    constructor.name.span = f.name.span;
                    constructor.span = f.span;
                    for (generated, original) in constructor.params.iter_mut().zip(&f.params) {
                        generated.span = original.span;
                        generated.name.span = original.name.span;
                    }
                    if let Some(Type::Owned { span, .. }) = &mut constructor.ret_type {
                        *span = f.span;
                    }
                }
            }
        }
        Ok(items)
    }
    fn parse(&mut self, source: &str) -> Result<Vec<Item>> {
        self.file += 1;
        let mut program = Parser::new(Lexer::new(source, self.file))
            .parse_program()
            .map_err(|e| Error::CodeGen(format!("async lowering generated invalid syntax: {e}")))?;
        for item in &mut program.items {
            replace_item(item, &self.types, &self.expressions, &self.chunks)?;
        }
        Ok(program.items)
    }
}

fn rewrite_returns(statements: &mut Vec<Stmt>, builder: &mut Builder<'_>) -> Result<()> {
    for statement in statements {
        if let Stmt::Return { value, span } = statement {
            let span = *span;
            let mut value = value.take().unwrap_or_else(|| unit(span));
            rewrite_expr_returns(&mut value, builder)?;
            let stmts = builder.finish(value)?;
            *statement = Stmt::Semi(Expr::Block(Block { stmts, span }));
        } else {
            let expr = match statement {
                Stmt::Expr(e) | Stmt::Semi(e) => Some(e),
                Stmt::Let { value, .. } => value.as_mut(),
                _ => None,
            };
            if let Some(e) = expr {
                rewrite_expr_returns(e, builder)?;
            }
        }
    }
    Ok(())
}
fn rewrite_expr_returns(e: &mut Expr, builder: &mut Builder<'_>) -> Result<()> {
    // A closure's returns belong to its own invocation.
    if matches!(e, Expr::Closure { .. }) {
        return Ok(());
    }
    blocks(e, &mut |b| rewrite_returns(&mut b.stmts, builder))
}

fn validate_user_block(block: &mut Block, depth: usize) -> Result<()> {
    for statement in &mut block.stmts {
        if matches!(statement, Stmt::Break { .. } | Stmt::Continue { .. }) && depth == 0 {
            return Err(invalid(
                "loop control inside a user loop, not the generated async dispatcher",
                block.span,
            ));
        }
        let e = match statement {
            Stmt::Expr(e) | Stmt::Semi(e) => Some(e),
            Stmt::Let { value, .. } | Stmt::Return { value, .. } => value.as_mut(),
            _ => None,
        };
        if let Some(e) = e {
            validate_user_expr(e, depth)?;
        }
    }
    Ok(())
}
fn validate_user_expr(e: &mut Expr, depth: usize) -> Result<()> {
    if await_value(e).is_some() {
        return Err(invalid("await at a top-level async statement or let initializer; nested suspension is not implemented",e.span()));
    }
    if matches!(e, Expr::Try { .. }) {
        return Err(invalid("explicit Result match/return in async functions; async '?' propagation is not implemented",e.span()));
    }
    match e {
        Expr::Loop { body, .. } => validate_user_block(body, depth + 1),
        Expr::While { cond, body, .. } => {
            validate_user_expr(cond, depth)?;
            validate_user_block(body, depth + 1)
        }
        Expr::For { iter, body, .. } => {
            validate_user_expr(iter, depth)?;
            validate_user_block(body, depth + 1)
        }
        Expr::Closure { .. } => walk(e, &mut |child| {
            if await_value(child).is_some() {
                Err(invalid(
                    "async closures are not implemented; use an async function",
                    child.span(),
                ))
            } else {
                Ok(())
            }
        }),
        _ => children(e, &mut |e| validate_user_expr(e, depth), &mut |b| {
            validate_user_block(b, depth)
        }),
    }
}

fn replace_type(t: &mut Type, types: &HashMap<String, Type>) {
    if let Type::Named(n, _) = t {
        if let Some(ty) = types.get(n) {
            *t = ty.clone();
            return;
        }
    }
    match t {
        Type::Pointer(t, _)
        | Type::Slice(t, _)
        | Type::Volatile(t, _)
        | Type::Ref { inner: t, .. }
        | Type::Owned { inner: t, .. }
        | Type::Array { elem: t, .. }
        | Type::ArrayExpr { elem: t, .. } => replace_type(t, types),
        Type::Tuple(ts, _) | Type::Generic(_, ts, _) => {
            for t in ts {
                replace_type(t, types)
            }
        }
        Type::GenericWithArgs { args, .. } => {
            for a in args {
                if let GenericArg::Type(t) = a {
                    replace_type(t, types);
                }
            }
        }
        Type::Function { params, ret, .. } => {
            for t in params {
                replace_type(t, types);
            }
            replace_type(ret, types);
        }
        _ => {}
    }
}
fn replace_item(
    item: &mut Item,
    types: &HashMap<String, Type>,
    exprs: &HashMap<String, Expr>,
    chunks: &HashMap<String, Vec<Stmt>>,
) -> Result<()> {
    match item {
        Item::Struct(s) => {
            for f in &mut s.fields {
                replace_type(&mut f.ty, types)
            }
        }
        Item::Enum(e) => {
            for v in &mut e.variants {
                for t in &mut v.fields {
                    replace_type(t, types)
                }
            }
        }
        Item::Function(f) => {
            for p in &mut f.params {
                replace_type(&mut p.ty, types);
            }
            if let Some(t) = &mut f.ret_type {
                replace_type(t, types);
            }
            replace_block(&mut f.body, types, exprs, chunks)?;
        }
        Item::Impl(i) => {
            for f in &mut i.methods {
                for p in &mut f.params {
                    replace_type(&mut p.ty, types);
                }
                if let Some(t) = &mut f.ret_type {
                    replace_type(t, types);
                }
                replace_block(&mut f.body, types, exprs, chunks)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn replace_block(
    b: &mut Block,
    types: &HashMap<String, Type>,
    exprs: &HashMap<String, Expr>,
    chunks: &HashMap<String, Vec<Stmt>>,
) -> Result<()> {
    let mut result = Vec::new();
    for mut s in std::mem::take(&mut b.stmts) {
        if let Stmt::Semi(Expr::Call { func, .. }) = &s {
            if let Expr::Ident(n) = &**func {
                if let Some(stmts) = chunks.get(&n.name) {
                    result.extend(stmts.clone());
                    continue;
                }
            }
        }
        if let Stmt::Let { ty: Some(t), .. } = &mut s {
            replace_type(t, types);
        }
        let e = match &mut s {
            Stmt::Expr(e) | Stmt::Semi(e) => Some(e),
            Stmt::Let { value, .. } | Stmt::Return { value, .. } => value.as_mut(),
            _ => None,
        };
        if let Some(e) = e {
            replace_expr(e, types, exprs, chunks)?;
        }
        result.push(s);
    }
    b.stmts = result;
    Ok(())
}
fn replace_expr(
    e: &mut Expr,
    types: &HashMap<String, Type>,
    exprs: &HashMap<String, Expr>,
    chunks: &HashMap<String, Vec<Stmt>>,
) -> Result<()> {
    if let Expr::Call { func, .. } = e {
        if let Expr::Ident(n) = &**func {
            if let Some(value) = exprs.get(&n.name) {
                *e = value.clone();
                return Ok(());
            }
        }
    }
    match e {
        Expr::Cast { ty, .. } | Expr::Layout { ty, .. } => replace_type(ty, types),
        _ => {}
    }
    children(
        e,
        &mut |e| replace_expr(e, types, exprs, chunks),
        &mut |b| replace_block(b, types, exprs, chunks),
    )
}
fn walk_block(block: &mut Block, visit: &mut impl FnMut(&mut Expr) -> Result<()>) -> Result<()> {
    for s in &mut block.stmts {
        let e = match s {
            Stmt::Expr(e) | Stmt::Semi(e) => Some(e),
            Stmt::Let { value, .. } | Stmt::Return { value, .. } => value.as_mut(),
            _ => None,
        };
        if let Some(e) = e {
            walk(e, visit)?;
        }
    }
    Ok(())
}
fn walk(e: &mut Expr, visit: &mut impl FnMut(&mut Expr) -> Result<()>) -> Result<()> {
    visit(e)?;
    children(e, &mut |e| walk(e, visit), &mut |_| Ok(()))?;
    match e {
        Expr::Block(b)
        | Expr::Unsafe { body: b, .. }
        | Expr::Loop { body: b, .. }
        | Expr::While { body: b, .. }
        | Expr::For { body: b, .. } => walk_block(b, visit)?,
        Expr::If {
            then_block,
            else_block,
            ..
        } => {
            walk_block(then_block, visit)?;
            if let Some(b) = else_block {
                walk_block(b, visit)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn blocks(e: &mut Expr, visit: &mut impl FnMut(&mut Block) -> Result<()>) -> Result<()> {
    if matches!(e, Expr::Closure { .. }) {
        return Ok(());
    }
    children(e, &mut |e| blocks(e, visit), &mut |_| Ok(()))?;
    match e {
        Expr::Block(b)
        | Expr::Unsafe { body: b, .. }
        | Expr::Loop { body: b, .. }
        | Expr::While { body: b, .. }
        | Expr::For { body: b, .. } => visit(b)?,
        Expr::If {
            then_block,
            else_block,
            ..
        } => {
            visit(then_block)?;
            if let Some(b) = else_block {
                visit(b)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn children(
    e: &mut Expr,
    expr: &mut impl FnMut(&mut Expr) -> Result<()>,
    block: &mut impl FnMut(&mut Block) -> Result<()>,
) -> Result<()> {
    match e {
        Expr::Unary { expr: e, .. }
        | Expr::Cast { expr: e, .. }
        | Expr::Try { expr: e, .. }
        | Expr::Deref { expr: e, .. }
        | Expr::Ref { expr: e, .. }
        | Expr::Field { expr: e, .. } => expr(e)?,
        Expr::Binary { left, right, .. } => {
            expr(left)?;
            expr(right)?;
        }
        Expr::Index { expr: e, index, .. } => {
            expr(e)?;
            expr(index)?;
        }
        Expr::Call { func, args, .. } => {
            expr(func)?;
            for a in args {
                expr(a)?;
            }
        }
        Expr::MethodCall { expr: e, args, .. } => {
            expr(e)?;
            for a in args {
                expr(a)?;
            }
        }
        Expr::Block(b) | Expr::Unsafe { body: b, .. } | Expr::Loop { body: b, .. } => block(b)?,
        Expr::If {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr(cond)?;
            block(then_block)?;
            if let Some(b) = else_block {
                block(b)?;
            }
        }
        Expr::While { cond, body, .. } => {
            expr(cond)?;
            block(body)?;
        }
        Expr::For { iter, body, .. } => {
            expr(iter)?;
            block(body)?;
        }
        Expr::Match { expr: e, arms, .. } => {
            expr(e)?;
            for a in arms {
                if let Some(g) = &mut a.guard {
                    expr(g)?;
                }
                expr(&mut a.body)?;
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, e) in fields {
                expr(e)?;
            }
        }
        Expr::Tuple { elements, .. } | Expr::Array { elements, .. } => {
            for e in elements {
                expr(e)?;
            }
        }
        Expr::ArrayRepeat { value, count, .. } => {
            expr(value)?;
            expr(count)?;
        }
        Expr::Range { start, end, .. } => {
            if let Some(e) = start {
                expr(e)?;
            }
            if let Some(e) = end {
                expr(e)?;
            }
        }
        Expr::Closure { body, .. } => expr(body)?,
        Expr::Asm { operands, .. } => {
            for o in operands {
                if let Some(e) = &mut o.expr {
                    expr(e)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
