//! A source index shared by the CLI and LSP, built from the resolved program.
use std::collections::HashMap;
use serde::Serialize;
use super::{ast::*, session::{Location, Session}};
use crate::utils::Span;

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
    scopes: Vec<HashMap<String, usize>>,
    fields: HashMap<String, HashMap<String, usize>>,
}
impl SymbolIndex {
    pub fn new(session: &Session) -> Self {
        let mut builder = Builder { index: Self::default(), session, scopes: vec![HashMap::new()], fields: HashMap::new() };
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
fn display_name(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("__aeth_m") { if let Some((_, original)) = rest.split_once('_') { return original.into(); } }
    name.strip_prefix("__aeth_script_arg_").unwrap_or(name).into()
}
pub fn type_name(ty: &Type) -> String {
    match ty {
        Type::Named(name, _) => display_name(name), Type::Pointer(ty, _) => format!("*{}", type_name(ty)),
        Type::Ref { mutable, inner, .. } => format!("&{}{}", if *mutable { "mut " } else { "" }, type_name(inner)),
        Type::Array { elem, size, .. } => format!("[{}; {}]", type_name(elem), size),
        Type::Slice(ty, _) => format!("[{}]", type_name(ty)),
        Type::Tuple(types, _) => format!("({})", types.iter().map(type_name).collect::<Vec<_>>().join(", ")),
        Type::Generic(name, args, _) => format!("{}<{}>", display_name(name), args.iter().map(type_name).collect::<Vec<_>>().join(", ")),
        Type::GenericWithArgs { name, args, .. } => format!("{}<{}>", display_name(name), args.iter().map(|a| match a { GenericArg::Type(t) => type_name(t), GenericArg::Const(e) => format!("{:?}",e) }).collect::<Vec<_>>().join(", ")),
        Type::Function { params, ret, .. } => format!("fn({}) -> {}", params.iter().map(type_name).collect::<Vec<_>>().join(", "),type_name(ret)),
        Type::Unit(_) => "()".into(), Type::Never(_) => "!".into(), Type::Infer(_) => "_".into(),
        Type::Owned { inner, .. } => format!("own {}", type_name(inner)), Type::Volatile(inner, _) => format!("*volatile {}", type_name(inner)),
    }
}
impl Builder<'_> {
    fn define(&mut self, key: &str, name: &str, kind: &str, ty: String, span: Span) -> Option<usize> {
        let mut location = self.session.location(span)?;
        let name = display_name(name);
        // Synthesized Script declarations map to a statement; narrow to the
        // actual source spelling rather than exposing a generated identifier.
        if let Some(text) = self.session.source(span.file_id) {
            if text.get(location.start..location.end) != Some(&name) {
                if let Some(segment) = text.get(location.start..location.end) {
                    if let Some(offset) = segment.find(&name) {
                        location = super::session::text_location(std::path::Path::new(&location.file), text, location.start+offset, location.start+offset+name.len());
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
        if let (Some(symbol),Some(location)) = (self.lookup(key),self.session.location(span)) {
            self.index.references.push(Reference { symbol, location });
        }
    }
    fn signature(f: &Function) -> String {
        format!("fn({}) -> {}", f.params.iter().map(|p| format!("{}: {}",display_name(&p.name.name),type_name(&p.ty))).collect::<Vec<_>>().join(", "), f.ret_type.as_ref().map(type_name).unwrap_or("()".into()))
    }
    fn item_definition(&mut self, item: &Item) {
        match item {
            Item::Function(f) => { self.define(&f.name.name,&f.name.name,"function",Self::signature(f),f.name.span); }
            Item::Struct(s) => {
                self.define(&s.name.name,&s.name.name,"struct",display_name(&s.name.name),s.name.span);
                let mut fields = HashMap::new();
                for field in &s.fields {
                    let key = format!("{}::{}",s.name.name,field.name.name);
                    if let Some(id) = self.define(&key,&field.name.name,"field",type_name(&field.ty),field.name.span) { fields.insert(field.name.name.clone(),id); }
                }
                self.fields.insert(display_name(&s.name.name), fields);
            }
            Item::Enum(e) => { self.define(&e.name.name,&e.name.name,"enum",display_name(&e.name.name),e.name.span); }
            Item::Union(u) => { self.define(&u.name.name,&u.name.name,"union",display_name(&u.name.name),u.name.span); }
            Item::Const(c) => { self.define(&c.name.name,&c.name.name,"constant",c.ty.as_ref().map(type_name).unwrap_or("constant".into()),c.name.span); }
            Item::Static(s) => { self.define(&s.name.name,&s.name.name,"static",type_name(&s.ty),s.name.span); }
            Item::TypeAlias(t) => { self.define(&t.name.name,&t.name.name,"type",type_name(&t.ty),t.name.span); }
            Item::Impl(i) => for f in &i.methods {
                self.define(&format!("{}_{}",i.target.name,f.name.name),&f.name.name,"method",Self::signature(f),f.name.span);
            },
            Item::Extern(ext) => for item in &ext.items { match item {
                ForeignItem::Fn { name, params, ret_type, .. } => { self.define(&name.name,&name.name,"extern function",format!("fn({}) -> {}",params.iter().map(|p|type_name(&p.ty)).collect::<Vec<_>>().join(", "),ret_type.as_ref().map(type_name).unwrap_or("()".into())),name.span); }
                ForeignItem::Static { name, ty, .. } => { self.define(&name.name,&name.name,"extern static",type_name(ty),name.span); }
            } },
            _ => {},
        }
    }
    fn item_body(&mut self, item: &Item) {
        match item {
            Item::Function(f) => self.function(f), Item::Impl(i) => for f in &i.methods { self.function(f); },
            Item::Const(c) => self.expr(&c.value), Item::Static(s) => if let Some(value)=&s.value { self.expr(value); },
            _ => {},
        }
    }
    fn function(&mut self, f: &Function) {
        self.scopes.push(HashMap::new());
        for p in &f.params { self.define(&p.name.name,&p.name.name,"parameter",type_name(&p.ty),p.name.span); }
        self.block(&f.body);
        self.scopes.pop();
    }
    fn block(&mut self, block: &Block) {
        self.scopes.push(HashMap::new());
        for stmt in &block.stmts { match stmt {
            Stmt::Let { name, ty, value, .. } => {
                if let Some(value) = value { self.expr(value); }
                self.define(&name.name,&name.name,"variable",ty.as_ref().map(type_name).unwrap_or("inferred".into()),name.span);
            }
            Stmt::Expr(e) | Stmt::Semi(e) => self.expr(e), Stmt::Return { value: Some(e), .. } => self.expr(e), _ => {},
        } }
        self.scopes.pop();
    }
    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Ident(i) => self.reference(&i.name,i.span),
            Expr::Path { segments, span } => { let key=segments.iter().map(|i|i.name.as_str()).collect::<Vec<_>>().join("_"); self.reference(&key,*span); }
            Expr::Binary { left, right, .. } => { self.expr(left);self.expr(right); }
            Expr::Unary { expr, .. } | Expr::Ref { expr, .. } | Expr::Deref { expr, .. } | Expr::Cast { expr, .. } | Expr::Try { expr, .. } => self.expr(expr),
            Expr::Call { func, args, .. } => { self.expr(func); for a in args {self.expr(a);} }
            Expr::Field { expr, field, .. } => {
                self.expr(expr);
                if let Expr::Ident(i) = expr.as_ref() {
                    if let Some(id)=self.lookup(&i.name) {
                        let ty=self.index.symbols[id].type_name.trim_start_matches('&').trim_start_matches("mut ").trim_start_matches('*');
                        if let Some(symbol)=self.fields.get(ty).and_then(|f|f.get(&field.name)).copied() {
                            if let Some(location)=self.session.location(field.span) { self.index.references.push(Reference {symbol,location}); }
                        }
                    }
                }
            }
            Expr::MethodCall { expr, args, .. } => { self.expr(expr);for a in args {self.expr(a);} }
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
            Expr::StructLit {name,fields,..} => {self.reference(&name.name,name.span);for (_,v) in fields{self.expr(v);}}
            Expr::Array {elements,..}|Expr::Tuple {elements,..} => for e in elements{self.expr(e);},
            Expr::Range {start,end,..} => {if let Some(e)=start{self.expr(e);}if let Some(e)=end{self.expr(e);}}
            Expr::Closure {body,..} => self.expr(body), _=>{},
        }
    }
    fn pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Binding { name: i, .. } => {self.define(&i.name,&i.name,"binding","pattern value".into(),i.span);}
            Pattern::Struct {fields,..} => for (_,p) in fields {self.pattern(p);},
            Pattern::Tuple {elements,..} | Pattern::Variant {fields:elements,..} => for p in elements {self.pattern(p);},
            _=>{},
        }
    }
}
