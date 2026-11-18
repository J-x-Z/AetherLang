//! A source index shared by the CLI and LSP, built from the resolved program.
use std::collections::HashMap;
use serde::Serialize;
use super::{ast::*, session::{LineIndex, Location, Session}};
use crate::utils::Span;
use crate::types::ResolvedType;
use super::semantic::{SemanticFacts, ResolvedMethodCall};

#[derive(Clone, Serialize)]
pub struct Symbol {
    pub id: usize,
    pub name: String,
    pub kind: String,
    pub type_name: String,
    pub definition: Location,
}
#[derive(Clone, Serialize)]
pub struct Reference { pub symbol: usize, pub location: Location }
#[derive(Default, Serialize)]
pub struct SymbolIndex { pub symbols: Vec<Symbol>, pub references: Vec<Reference> }
struct Builder<'a> {
    index: SymbolIndex,
    session: &'a Session,
    lines: Vec<LineIndex>,
    facts: Option<&'a SemanticFacts>,
    function_name: String,
    binding_types: HashMap<usize, String>,
    scopes: Vec<HashMap<String, usize>>,
    fields: HashMap<String, HashMap<String, usize>>,
    enum_declarations: HashMap<Span, usize>,
    variant_declarations: HashMap<(Span, String), usize>,
}
impl SymbolIndex {
    pub fn new(session: &Session, facts: Option<&SemanticFacts>) -> Self {
        // Only semantic-index consumers pay for this table. It expires with the borrowed session.
        let lines = session.resolver.sources.iter().enumerate()
            .map(|(id, file)| LineIndex::new(session.source(id).unwrap_or(&file.text))).collect();
        let mut builder = Builder { index: Self::default(), session, lines, facts, function_name: String::new(), binding_types: HashMap::new(), scopes: vec![HashMap::new()], fields: HashMap::new(), enum_declarations: HashMap::new(), variant_declarations: HashMap::new() };
        for (name, variants) in &session.generic_enum_names {
            builder.enum_definition(name, variants.iter());
        }
        for item in &session.program.items { builder.item_definition(item); }
        for item in &session.program.items { builder.item_body(item); }
        builder.index
    }
    pub fn at(&self, file: &str, byte: usize) -> Option<&Symbol> {
        let contains = |loc: &Location| loc.file == file && loc.start <= byte && byte < loc.end;
        self.references.iter().filter(|r| contains(&r.location)).min_by_key(|r| r.location.end-r.location.start)
            .and_then(|r| self.symbols.get(r.symbol))
            .or_else(|| self.symbols.iter().filter(|s| contains(&s.definition)).min_by_key(|s| s.definition.end-s.definition.start))
    }
}
impl Builder<'_> {
    fn location(&self, span: Span) -> Option<Location> {
        self.session.indexed_location(span, self.lines.get(span.file_id)?)
    }

    fn display_name(&self, name: &str) -> String {
        if let Some(original) = self.session.resolver.original_name(name) { return original.into(); }
        name.strip_prefix("__aeth_script_arg_").unwrap_or(name).into()
    }
    fn type_name(&self, ty: &Type) -> String {
        match ty {
            Type::Dyn(name, _) => format!("dyn {}",self.display_name(name)),
            Type::Named(name, _) => self.display_name(name), Type::Pointer(ty, _) => format!("*{}", self.type_name(ty)),
            Type::Ref { mutable, inner, .. } => format!("&{}{}", if *mutable { "mut " } else { "" }, self.type_name(inner)),
            Type::ArrayExpr {elem,..} => format!("[{}; const]", self.type_name(elem)),
            Type::Array { elem, size, .. } => format!("[{}; {}]", self.type_name(elem), size),
            Type::Slice(ty, _) => format!("[{}]", self.type_name(ty)),
            Type::Tuple(types, _) => format!("({})", types.iter().map(|ty| self.type_name(ty)).collect::<Vec<_>>().join(", ")),
            Type::Generic(name, args, _) => format!("{}<{}>", self.display_name(name), args.iter().map(|ty| self.type_name(ty)).collect::<Vec<_>>().join(", ")),
            Type::GenericWithArgs { name, args, .. } => format!("{}<{}>", self.display_name(name), args.iter().map(|a| match a { GenericArg::Type(t) => self.type_name(t), GenericArg::Const(e) => format!("{:?}",e) }).collect::<Vec<_>>().join(", ")),
            Type::Function { params, ret, .. } => format!("fn({}) -> {}", params.iter().map(|ty| self.type_name(ty)).collect::<Vec<_>>().join(", "),self.type_name(ret)),
            Type::Unit(_) => "()".into(), Type::Never(_) => "!".into(), Type::Infer(_) => "_".into(),
            Type::Owned { inner, .. } => format!("own {}", self.type_name(inner)), Type::Volatile(inner, _) => format!("*volatile {}", self.type_name(inner)),
        }
    }

    fn define(&mut self, key: &str, name: &str, kind: &str, ty: String, span: Span) -> Option<usize> {
        let mut location = self.location(span)?;
        let name = self.display_name(name);
        // Synthesized Script declarations map to a statement; narrow to the
        // actual source spelling rather than exposing a generated identifier.
        if let Some(text) = self.session.source(span.file_id) {
            if text.get(location.start..location.end) != Some(&name) {
                if let Some(segment) = text.get(location.start..location.end) {
                    if let Some(offset) = segment.find(&name) {
                        location = self.lines[span.file_id].location(std::path::Path::new(&location.file), text, location.start+offset, location.start+offset+name.len());
                    }
                }
            }
        }
        let id = self.index.symbols.len();
        self.index.symbols.push(Symbol { id, name, kind: kind.into(), type_name: ty, definition: location });
        self.scopes.last_mut().unwrap().insert(key.into(), id);
        Some(id)
    }
    fn lookup(&self, name: &str) -> Option<usize> { self.scopes.iter().rev().find_map(|scope| scope.get(name).copied()) }
    fn reference(&mut self, key: &str, span: Span) {
        if let (Some(symbol),Some(location)) = (self.lookup(key),self.location(span)) {
            self.index.references.push(Reference { symbol, location });
        }
    }
    fn signature(&self, f: &Function) -> String {
        format!("fn({}) -> {}", f.params.iter().map(|p| format!("{}: {}",self.display_name(&p.name.name),self.type_name(&p.ty))).collect::<Vec<_>>().join(", "), f.ret_type.as_ref().map(|ty| self.type_name(ty)).unwrap_or("()".into()))
    }
    fn enum_definition<'a>(&mut self, name: &Ident, variants: impl Iterator<Item = &'a Ident>) {
        // Specializations share their declarations with the generic source enum.
        let enum_id = if let Some(id) = self.enum_declarations.get(&name.span).copied() {
            self.scopes[0].insert(name.name.clone(), id);
            id
        } else {
            let Some(id) = self.define(&name.name, &name.name, "enum", self.display_name(&name.name), name.span)
                else { return; };
            self.enum_declarations.insert(name.span, id);
            id
        };
        for variant in variants {
            let key = format!("{}::{}", name.name, variant.name);
            let origin = (variant.span, variant.name.clone());
            if let Some(id) = self.variant_declarations.get(&origin) {
                self.scopes[0].insert(key, *id);
            } else {
                let enum_name = self.index.symbols[enum_id].name.clone();
                if let Some(id) = self.define(&key, &variant.name, "variant", enum_name, variant.span) {
                    self.variant_declarations.insert(origin, id);
                }
            }
        }
    }
    fn item_definition(&mut self, item: &Item) {
        match item {
            Item::Function(f) => { self.define(&f.name.name,&f.name.name,"function",self.signature(f),f.name.span); }
            Item::Struct(s) => {
                self.define(&s.name.name,&s.name.name,"struct",self.display_name(&s.name.name),s.name.span);
                let mut fields = HashMap::new();
                for field in &s.fields {
                    let key = format!("{}::{}",s.name.name,field.name.name);
                    if let Some(id) = self.define(&key,&field.name.name,"field",self.type_name(&field.ty),field.name.span) { fields.insert(field.name.name.clone(),id); }
                }
                self.fields.insert(s.name.name.clone(), fields);
            }
            Item::Enum(e) => self.enum_definition(&e.name, e.variants.iter().map(|v| &v.name)),
            Item::Union(u) => { self.define(&u.name.name,&u.name.name,"union",self.display_name(&u.name.name),u.name.span); }
            Item::Const(c) => { self.define(&c.name.name,&c.name.name,"constant",c.ty.as_ref().map(|ty| self.type_name(ty)).unwrap_or("constant".into()),c.name.span); }
            Item::Static(s) => { self.define(&s.name.name,&s.name.name,"static",self.type_name(&s.ty),s.name.span); }
            Item::TypeAlias(t) => { self.define(&t.name.name,&t.name.name,"type",self.type_name(&t.ty),t.name.span); }
            Item::Impl(i) => for f in &i.methods {
                self.define(&format!("{}::{}",i.target.name,f.name.name),&f.name.name,"method",self.signature(f),f.name.span);
            },
            Item::Extern(ext) => for item in &ext.items { match item {
                ForeignItem::Fn { name, params, ret_type, .. } => { self.define(&name.name,&name.name,"extern function",format!("fn({}) -> {}",params.iter().map(|p|self.type_name(&p.ty)).collect::<Vec<_>>().join(", "),ret_type.as_ref().map(|ty| self.type_name(ty)).unwrap_or("()".into())),name.span); }
                ForeignItem::Static { name, ty, .. } => { self.define(&name.name,&name.name,"extern static",self.type_name(ty),name.span); }
            } },
            _ => {},
        }
    }
    fn item_body(&mut self, item: &Item) {
        match item {
            Item::Function(f) => self.function(f, f.name.name.clone()),
            Item::Impl(i) => for f in &i.methods {
                let key = format!("{}::{}", i.target.name, f.name.name);
                let emitted = self.facts.and_then(|facts| facts.method_signatures.get(&key))
                    .map(|sig| sig.emitted_name.clone()).unwrap_or(key);
                self.function(f, emitted);
            },
            Item::Const(c) => self.expr(&c.value), Item::Static(s) => if let Some(value)=&s.value { self.expr(value); },
            _ => {},
        }
    }
    fn typed_binding(&mut self, name: &Ident, kind: &str, ty: Option<&Type>) {
        let Some(id) = self.define(&name.name, &name.name, kind,
            ty.map(|ty| self.type_name(ty)).unwrap_or_else(|| "inferred".into()), name.span) else { return; };
        let Some(mut base) = ty else { return; };
        while let Type::Ref { inner, .. } | Type::Pointer(inner, _) | Type::Volatile(inner, _) = base {
            base = inner;
        }
        if let Type::Named(key, _) = base { self.binding_types.insert(id, key.clone()); }
    }
    fn function(&mut self, f: &Function, emitted: String) {
        self.function_name = emitted;
        self.scopes.push(HashMap::new());
        for p in &f.params { self.typed_binding(&p.name, "parameter", Some(&p.ty)); }
        self.block(&f.body);
        self.scopes.pop();
    }
    fn block(&mut self, block: &Block) {
        self.scopes.push(HashMap::new());
        for stmt in &block.stmts { match stmt {
            Stmt::Let { name, ty, value, .. } => {
                if let Some(value) = value { self.expr(value); }
                self.typed_binding(name, "variable", ty.as_ref());
            }
            Stmt::Expr(e) | Stmt::Semi(e) => self.expr(e), Stmt::Return { value: Some(e), .. } => self.expr(e), _ => {},
        } }
        self.scopes.pop();
    }
    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Ident(i) => self.reference(&i.name,i.span),
            Expr::Path { segments, .. } => {
                if let Some(name) = segments.last() {
                    let key = segments.iter().map(|i| i.name.as_str()).collect::<Vec<_>>().join("::");
                    self.reference(&key, name.span);
                }
            }
            Expr::Binary { left, right, .. } => { self.expr(left);self.expr(right); }
            Expr::Unary { expr, .. } | Expr::Ref { expr, .. } | Expr::Deref { expr, .. } | Expr::Cast { expr, .. } | Expr::Try { expr, .. } => self.expr(expr),
            Expr::Call { func, args, .. } => { self.expr(func); for a in args {self.expr(a);} }
            Expr::Field { expr, field, .. } => {
                self.expr(expr);
                let ty = self.facts.and_then(|facts| facts.expr_types.get(&(self.function_name.clone(), expr.span())));
                let owner = ty.and_then(|mut base| {
                    while let ResolvedType::Reference { inner, .. } | ResolvedType::Pointer(inner) | ResolvedType::Volatile(inner) = base { base = inner; }
                    match base { ResolvedType::Struct { name, .. } => Some(name), _ => None }
                }).or_else(|| match expr.as_ref() {
                    Expr::Ident(id) => self.lookup(&id.name).and_then(|symbol| self.binding_types.get(&symbol)),
                    _ => None,
                });
                if let Some(id) = owner.and_then(|key| self.fields.get(key)).and_then(|fields| fields.get(&field.name)).copied() {
                    if let Some(location) = self.location(field.span) { self.index.references.push(Reference { symbol: id, location }); }
                }
            }
            Expr::MethodCall { expr, method, args, span } => {
                self.expr(expr);
                if let Some(ResolvedMethodCall::Function { symbol, .. }) = self.facts.and_then(|f| f.method_calls.get(&(self.function_name.clone(), *span))) {
                    self.reference(symbol, method.span);
                }
                for arg in args { self.expr(arg); }
            }
            Expr::Index { expr, index, .. } => { self.expr(expr);self.expr(index); }
            Expr::Block(block) | Expr::Unsafe { body: block, .. } | Expr::Loop { body: block, .. } => self.block(block),
            Expr::If { cond, then_block, else_block, .. } => {self.expr(cond);self.block(then_block);if let Some(b)=else_block{self.block(b);} }
            Expr::While {cond,body,..} => {self.expr(cond);self.block(body);}
            Expr::For {var,iter,body,..} => {
                self.expr(iter);self.scopes.push(HashMap::new());
                self.define(&var.name,&var.name,"variable","iteration value".into(),var.span);self.block(body);self.scopes.pop();
            }
            Expr::Match {expr,arms,..} => {self.expr(expr);for arm in arms {
                self.scopes.push(HashMap::new());self.pattern(&arm.pattern);
                if let Some(guard)=&arm.guard{self.expr(guard);}self.expr(&arm.body);self.scopes.pop();
            }}
            Expr::StructLit {name,fields,..} => {self.reference(&name.name,name.span);for (field,value) in fields {self.reference(&format!("{}::{}", name.name, field.name),field.span);self.expr(value);}}
            Expr::ArrayRepeat {value,count,..} => {self.expr(value);self.expr(count);},
            Expr::Array {elements,..}|Expr::Tuple {elements,..} => for e in elements{self.expr(e);},
            Expr::Range {start,end,..} => {if let Some(e)=start{self.expr(e);}if let Some(e)=end{self.expr(e);}}
            Expr::Closure {body,..} => self.expr(body), _=>{},
        }
    }
    fn pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Binding { name: i, .. } => {self.define(&i.name,&i.name,"binding","pattern value".into(),i.span);}
            Pattern::Struct {fields,..} => for (_,p) in fields {self.pattern(p);},
            Pattern::Variant { enum_name, variant, fields, .. } => {
                if let Some(owner) = enum_name {
                    self.reference(&format!("{}::{}", owner.name, variant.name), variant.span);
                }
                for field in fields { self.pattern(field); }
            }
            Pattern::Tuple { elements, .. } => for p in elements { self.pattern(p); },
            _=>{},
        }
    }
}
