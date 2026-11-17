//! Resolve a compilation unit once, before semantic analysis and IR lowering.
//!
//! Imported definitions keep module-private names. Only explicit public imports
//! enter the caller's scope; both backends receive the same fully resolved AST.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use super::ast::*;
use super::{lexer::Lexer, parser::Parser, token::TokenKind};
use crate::utils::{Error, Result, Span};

#[derive(Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    pub text: String,
}

#[derive(Clone)]
struct ResolvedName {
    emitted: String,
    foreign_declaration: Option<Span>,
}

#[derive(Clone, Default)]
struct Exports {
    symbols: HashMap<String, ResolvedName>,
    modules: HashMap<String, Exports>,
}

/// Shared command-line/editor lookup. Exact bindings do not make their source
/// directories global search roots for unrelated modules.
#[derive(Clone, Default)]
pub struct ModuleSearch {
    pub paths: Vec<PathBuf>,
    exact_modules: HashMap<String, PathBuf>,
}

impl ModuleSearch {
    pub fn new(paths: Vec<PathBuf>) -> Self { Self { paths, exact_modules: HashMap::new() } }

    pub fn add_module(&mut self, specification: &str) -> std::result::Result<(), String> {
        let (name, filename) = specification.split_once('=')
            .ok_or_else(|| format!("invalid --module '{specification}'; expected NAME=FILE"))?;
        let tokens = Lexer::new(name, 0).tokenize();
        if tokens.len() != 2 || !matches!(&tokens[0].kind, TokenKind::Ident(identifier) if identifier == name)
            || tokens[0].span.start != 0 || tokens[0].span.end != name.len()
            || !matches!(tokens[1].kind, TokenKind::Eof) {
            return Err(format!("invalid module name '{name}'; expected one language identifier"));
        }
        if self.exact_modules.contains_key(name) { return Err(format!("duplicate explicit module '{name}'")); }
        if filename.is_empty() { return Err(format!("explicit module '{name}' requires a source file")); }
        let file = Path::new(filename);
        if file.extension().is_none_or(|extension| extension != "aeth") {
            return Err(format!("explicit module '{name}' requires an exact .aeth source file"));
        }
        let path = std::fs::canonicalize(file)
            .map_err(|error| format!("explicit module '{name}': {}: {error}", file.display()))?;
        if !path.is_file() || path.extension().is_none_or(|extension| extension != "aeth") {
            return Err(format!("explicit module '{name}' requires an exact .aeth source file: {}", path.display()));
        }
        self.exact_modules.insert(name.into(), path);
        Ok(())
    }
}

pub struct Resolver {
    pub sources: Vec<SourceFile>,
    search: ModuleSearch,
    loaded: HashMap<PathBuf, Exports>,
    loading: Vec<PathBuf>,
    items: Vec<Item>,
    next_module: usize,
    module_owners: HashMap<Span, usize>,
    foreign_uses: HashMap<Span, Span>,
    entrypoint_enabled: bool,
    overlays: HashMap<PathBuf, String>,
    namespace: String,
    source_identifiers: HashSet<String>,
}

impl Resolver {
    pub fn new(search: ModuleSearch) -> Self {
        Self { sources: vec![], search, loaded: HashMap::new(), loading: vec![], items: vec![], next_module: 0, module_owners: HashMap::new(), foreign_uses: HashMap::new(), entrypoint_enabled: true, overlays: HashMap::new(), namespace: "__aeth_m".into(), source_identifiers: HashSet::new() }
    }

    pub fn with_overlays(mut self, overlays: HashMap<PathBuf, String>) -> Self {
        self.overlays = overlays.into_iter().map(|(path, text)| (Self::source_path(&path), text)).collect();
        self
    }

    pub fn source_path(path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| {
            if path.is_absolute() { path.into() } else { std::env::current_dir().unwrap_or_default().join(path) }
        })
    }

    pub fn resolve(&mut self, path: &Path, source: String) -> Result<Program> {
        let path = Self::source_path(path);
        // An imported file can introduce an ABI symbol or lexical binding that
        // looks like a name emitted earlier in the traversal. Retry the entire
        // graph with a fresh namespace, using the text snapshots already read.
        // Ordinary source keeps the established __aeth_mN_ spelling.
        loop {
            self.sources.clear();
            self.loaded.clear();
            self.loading.clear();
            self.items.clear();
            self.next_module = 0;
            self.module_owners.clear();
            self.foreign_uses.clear();
            let result = self.resolve_once(&path, source.clone());
            if !self.source_identifiers.iter().any(|name| name.starts_with(&self.namespace)) {
                return result;
            }
            // Snapshot only on the uncommon retry path; ordinary compilation
            // keeps one copy of each source rather than duplicating the graph.
            for source in &self.sources {
                self.overlays.insert(source.path.clone(), source.text.clone());
            }
            let mut suffix = 1usize;
            loop {
                let candidate = format!("__aeth_{suffix}_m");
                if !self.source_identifiers.iter().any(|name| name.starts_with(&candidate)) {
                    self.namespace = candidate;
                    break;
                }
                suffix += 1;
            }
        }
    }

    // Only this compilation's namespace is generated. A user identifier that
    // resembles the default (or an earlier candidate) must retain its spelling.
    pub fn original_name<'a>(&self, name: &'a str) -> Option<&'a str> {
        let (module, original) = name.strip_prefix(&self.namespace)?.split_once('_')?;
        if !module.is_empty() && module.bytes().all(|byte| byte.is_ascii_digit()) { Some(original) } else { None }
    }

    fn resolve_once(&mut self, path: &Path, source: String) -> Result<Program> {
        let program = self.parse(path, source)?;
        let attrs = program.inner_attrs.clone();
        self.entrypoint_enabled = !attrs.iter().any(|attribute| attribute.name.name == "no_main");
        self.loading.push(path.to_path_buf());
        self.expand(program.items, path, true)?;
        self.loading.pop();
        // Struct/type definitions precede functions irrespective of import order.
        // Foreign declarations are shared ABI symbols, not module-local copies.
        self.items.sort_by_key(|i| match i {
            Item::Struct(_) | Item::Enum(_) | Item::Union(_) => 0,
            Item::TypeAlias(_) => 1,
            Item::Extern(_) => 2,
            _ => 3,
        });
        // Foreign declarations are compared after semantic alias resolution.
        // Comparing source spellings here rejects equivalent aliases.
        Ok(Program { items: std::mem::take(&mut self.items), inner_attrs: attrs, module_owners: std::mem::take(&mut self.module_owners), foreign_uses: std::mem::take(&mut self.foreign_uses) })
    }

    fn parse(&mut self, path: &Path, text: String) -> Result<Program> {
        let id = self.sources.len();
        self.sources.push(SourceFile { path: path.into(), text });
        let tokens = Lexer::new(&self.sources[id].text, id).tokenize();
        for token in &tokens {
            if let TokenKind::Ident(name) = &token.kind {
                if name.starts_with("__aeth_") { self.source_identifiers.insert(name.clone()); }
            }
        }
        Parser::from_tokens(tokens).parse_program()
    }

    fn load(&mut self, path: PathBuf) -> Result<Exports> {
        let path = Self::source_path(&path);
        if self.loading.contains(&path) {
            return Err(Error::ModuleError(format!("circular import: {} -> {}",
                self.loading.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(" -> "), path.display())));
        }
        if let Some(exports) = self.loaded.get(&path) { return Ok(exports.clone()); }
        let text = match self.overlays.get(&path) {
            Some(text) => text.clone(),
            None => std::fs::read_to_string(&path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?,
        };
        let program = self.parse(&path, text)?;
        self.loading.push(path.clone());
        let result = self.expand(program.items, &path, false);
        self.loading.pop();
        let exports = result?;
        self.loaded.insert(path, exports.clone());
        Ok(exports)
    }

    fn find(&self, dir: &Path, name: &str) -> Result<PathBuf> {
        if let Some(mapped) = self.search.exact_modules.get(name) {
            for local in [dir.join(format!("{name}.aeth")), dir.join(name).join("mod.aeth")] {
                let canonical = Self::source_path(&local);
                if (local.is_file() || self.overlays.contains_key(&canonical)) && &canonical != mapped {
                    return Err(Error::ModuleError(format!("explicit module '{name}' ({}) conflicts with local module {}", mapped.display(), canonical.display())));
                }
            }
            return Ok(mapped.clone());
        }
        for base in std::iter::once(dir).chain(self.search.paths.iter().map(PathBuf::as_path)) {
            for candidate in [base.join(format!("{name}.aeth")), base.join(name).join("mod.aeth")] {
                if candidate.is_file() || self.overlays.contains_key(&Self::source_path(&candidate)) { return Ok(candidate); }
            }
        }
        Err(Error::ModuleError(format!("module '{name}' not found relative to {} or in module search paths", dir.display())))
    }

    fn expand(&mut self, items: Vec<Item>, path: &Path, root: bool) -> Result<Exports> {
        let id = self.next_module;
        self.next_module += 1;
        let dir = path.parent().unwrap_or(Path::new("."));
        let mut names = HashMap::new();
        let mut exports = Exports::default();
        let mut modules = HashMap::new();
        for item in &items {
            if let Some((name, public)) = definition(item) {
                if crate::intrinsics::is_reserved(name) {
                    return Err(Error::ModuleError(format!("compiler intrinsic '{name}' cannot be redefined in {}", path.display())));
                }
                let preserve_abi = match item {
                    Item::Function(f) => f.annotations.iter().any(|a| a.name.name == "no_mangle"),
                    Item::Static(s) => s.annotations.iter().any(|a| a.name.name == "no_mangle"),
                    _ => false,
                };
                let resolved = if (root && name == "main") || preserve_abi { name.to_owned() } else { format!("{}{id}_{name}", self.namespace) };
                let foreign_declaration = match item {
                    Item::Function(f) if preserve_abi => Some(f.name.span),
                    Item::Static(s) if preserve_abi => Some(s.name.span),
                    _ => None,
                };
                let resolved = ResolvedName { emitted: resolved, foreign_declaration };
                if names.insert(name.to_owned(), resolved.clone()).is_some() {
                    return Err(Error::ModuleError(format!("duplicate definition '{name}' in {}", path.display())));
                }
                if public { exports.symbols.insert(name.into(), resolved); }
            }
        }
        // Foreign declarations keep their linker spelling and are importable
        // from the module that declares them. Compatible repeated ABI
        // declarations are merged later by semantic signature checking.
        for item in &items {
            if let Item::Extern(block) = item {
                for foreign in &block.items {
                    let (ForeignItem::Fn {name,..} | ForeignItem::Static {name,..}) = foreign;
                    if names.get(&name.name).is_some_and(|previous| previous.emitted != name.name) {
                        return Err(Error::ModuleError(format!("foreign declaration '{}' conflicts with a local definition in {}",name.name,path.display())));
                    }
                    let binding = ResolvedName { emitted: name.name.clone(), foreign_declaration: Some(name.span) };
                    names.insert(name.name.clone(), binding.clone());
                    exports.symbols.insert(name.name.clone(), binding);
                }
            }
        }
        // Materialize declared modules before imports so inline modules work too.
        for item in &items {
            if let Item::Module(module) = item {
                let child = if let Some(body) = &module.items {
                    if self.search.exact_modules.contains_key(&module.name.name) {
                        return Err(Error::ModuleError(format!("explicit module '{}' conflicts with inline module in {}", module.name.name, path.display())));
                    }
                    self.expand(body.clone(), path, false)?
                } else {
                    self.load(self.find(dir, &module.name.name)?)?
                };
                if modules.insert(module.name.name.clone(), child.clone()).is_some() {
                    return Err(Error::ModuleError(format!("duplicate module '{}'", module.name.name)));
                }
                if module.is_pub { exports.modules.insert(module.name.name.clone(), child); }
            }
        }
        // Imports load each source once. Importing a module also permits its
        // qualified public paths, but never exposes its private definitions.
        for item in &items {
            if let Item::Use(decl) = item {
                let head = decl.path.first().ok_or_else(|| Error::ModuleError("empty import".into()))?;
                if !modules.contains_key(&head.name) {
                    let child = self.load(self.find(dir, &head.name)?)?;
                    modules.insert(head.name.clone(), child);
                }
                import(decl, &[], &modules, &mut names, &mut exports)?;
            }
        }
        for (module, child) in &modules {
            qualified_names(module, child, &mut names);
        }
        for mut item in items {
            if matches!(item, Item::Use(_) | Item::Module(_)) { continue; }
            Rewriter { names: &names, foreign_uses: &mut self.foreign_uses, modules: modules.keys().cloned().collect(), locals: HashSet::new(), type_locals: HashSet::new(), const_locals: HashSet::new(), normalize_main: root && self.entrypoint_enabled, zero_status_returns: false }.item(&mut item)?;
            match &item {
                Item::Function(function) => { self.module_owners.insert(function.span, id); }
                Item::Impl(block) => { for method in &block.methods { self.module_owners.insert(method.span, id); } }
                _ => {},
            }
            self.items.push(item);
        }
        Ok(exports)
    }
}

fn qualified_names(prefix: &str, exports: &Exports, names: &mut HashMap<String, ResolvedName>) {
    for (name, resolved) in &exports.symbols { names.insert(format!("{prefix}::{name}"), resolved.clone()); }
    for (name, child) in &exports.modules { qualified_names(&format!("{prefix}::{name}"), child, names); }
}

fn import(decl: &UseDecl, prefix: &[String], modules: &HashMap<String, Exports>, names: &mut HashMap<String, ResolvedName>, exported: &mut Exports) -> Result<()> {
    let mut path = prefix.to_vec();
    path.extend(decl.path.iter().map(|p| p.name.clone()));
    if let UseKind::Group(children) = &decl.kind {
        for child in children {
            let mut child = child.clone();
            child.is_pub |= decl.is_pub;
            import(&child, &path, modules, names, exported)?;
        }
        return Ok(());
    }
    let fail = || Error::ModuleError(format!("no public import '{}'", path.join("::")));
    let mut scope = modules.get(path.first().ok_or_else(fail)?).ok_or_else(fail)?;
    if matches!(decl.kind, UseKind::Glob) {
        for part in &path[1..] { scope = scope.modules.get(part).ok_or_else(fail)?; }
        for (name, value) in &scope.symbols { bind(names, name, value)?; if decl.is_pub { exported.symbols.insert(name.clone(), value.clone()); } }
    } else if path.len() > 1 {
        for part in &path[1..path.len()-1] { scope = scope.modules.get(part).ok_or_else(fail)?; }
        let name = path.last().unwrap();
        let value = scope.symbols.get(name).ok_or_else(fail)?;
        let alias = match &decl.kind { UseKind::Alias(a) => &a.name, _ => name };
        bind(names, alias, value)?;
        if decl.is_pub { exported.symbols.insert(alias.clone(), value.clone()); }
    } else if let UseKind::Alias(alias) = &decl.kind {
        qualified_names(&alias.name, scope, names);
    }
    Ok(())
}

fn bind(names: &mut HashMap<String, ResolvedName>, name: &str, value: &ResolvedName) -> Result<()> {
    if let Some(old) = names.get(name) {
        if old.emitted != value.emitted { return Err(Error::ModuleError(format!("ambiguous import '{name}'; use an alias"))); }
    }
    names.insert(name.into(), value.clone());
    Ok(())
}

fn definition(item: &Item) -> Option<(&str, bool)> {
    match item {
        Item::Function(x) => Some((&x.name.name, x.is_pub)),
        Item::Struct(x) => Some((&x.name.name, x.is_pub)),
        Item::Enum(x) => Some((&x.name.name, true)),
        Item::Const(x) => Some((&x.name.name, true)),
        Item::Static(x) => Some((&x.name.name, x.is_pub)),
        Item::Union(x) => Some((&x.name.name, x.is_pub)),
        Item::TypeAlias(x) => Some((&x.name.name, x.is_pub)),
        Item::Trait(x) | Item::Interface(x) => Some((&x.name.name, x.is_pub)),
        Item::Macro(x) => Some((&x.name.name, x.is_pub)),
        _ => None,
    }
}

struct Rewriter<'a> { names: &'a HashMap<String, ResolvedName>, foreign_uses: &'a mut HashMap<Span, Span>, modules: HashSet<String>, locals: HashSet<String>, type_locals: HashSet<String>, const_locals: HashSet<String>, normalize_main: bool, zero_status_returns: bool }
impl Rewriter<'_> {
    fn name(&self, name: &mut String) { if let Some(new) = self.names.get(name) { *name = new.emitted.clone(); } }
    fn ident(&self, id: &mut Ident) { self.name(&mut id.name); }
    fn ty(&mut self, ty: &mut Type) -> Result<()> {
        match ty {
            Type::Named(n, _) => if !self.type_locals.contains(n) && !self.const_locals.contains(n) { self.name(n); },
            Type::Generic(n, args, _) => { if !self.type_locals.contains(n) { self.name(n); } for ty in args { self.ty(ty)?; } }
            Type::GenericWithArgs { name, args, .. } => { if !self.type_locals.contains(name) { self.name(name); } for arg in args { match arg { GenericArg::Type(t) => self.ty(t)?, GenericArg::Const(e) => self.expr(e)? } } }
            Type::Pointer(t, _) | Type::Slice(t, _) | Type::Volatile(t, _) => self.ty(t)?,
            Type::Ref { inner, .. } | Type::Owned { inner, .. } => self.ty(inner)?,
            Type::Array { elem, .. } => self.ty(elem)?,
            Type::ArrayExpr {elem,size,..} => {self.ty(elem)?;self.expr(size)?;}
            Type::Tuple(ts, _) => for t in ts { self.ty(t)?; },
            Type::Function { params, ret, .. } => { for t in params { self.ty(t)?; } self.ty(ret)?; }
            _ => {}
        }
        Ok(())
    }
    fn function(&mut self, f: &mut Function, rename: bool) -> Result<()> {
        let saved_zero_status = self.zero_status_returns;
        self.zero_status_returns = rename && self.normalize_main && f.name.name == "main" && f.ret_type.is_none();
        if self.zero_status_returns {
            f.ret_type = Some(Type::Named("i32".into(), f.name.span));
            f.body.stmts.push(Stmt::Return { value: Some(Expr::Literal(Literal::Int(0, f.body.span))), span: f.body.span });
        }
        if rename { self.ident(&mut f.name); }
        let saved = self.locals.clone();
        let saved_types = self.type_locals.clone();
        let saved_consts = self.const_locals.clone();
        self.bind_generics(&f.generic_params, &f.type_params);
        self.generic_types(&mut f.generic_params)?;
        for p in &mut f.params { self.ty(&mut p.ty)?; self.locals.insert(p.name.name.clone()); }
        if let Some(t) = &mut f.ret_type { self.ty(t)?; }
        for c in &mut f.contracts {
            let binding = c.kind == ContractKind::Ensures && self.locals.insert("result".into());
            let checked = self.expr(&mut c.condition);
            if binding { self.locals.remove("result"); }
            checked?;
        }
        self.block(&mut f.body)?;
        self.locals = saved;
        self.type_locals = saved_types;
        self.const_locals = saved_consts;
        self.zero_status_returns = saved_zero_status;
        Ok(())
    }
    fn generic_types(&mut self, parameters: &mut [GenericParam]) -> Result<()> {
        for parameter in parameters { if let GenericParam::Const {ty,..} = parameter { self.ty(ty)?; } }
        Ok(())
    }
    fn bind_generics(&mut self, parameters: &[GenericParam], legacy: &[Ident]) {
        for parameter in legacy { self.type_locals.insert(parameter.name.clone()); }
        for parameter in parameters {
            match parameter {
                GenericParam::Type(name) => { self.type_locals.insert(name.name.clone()); }
                GenericParam::Const { name, .. } => { self.locals.insert(name.name.clone()); self.const_locals.insert(name.name.clone()); }
            }
        }
    }
    fn item(&mut self, item: &mut Item) -> Result<()> {
        let saved_locals = self.locals.clone();
        let saved_types = self.type_locals.clone();
        let saved_consts = self.const_locals.clone();
        match &*item {
            Item::Struct(def) => self.bind_generics(&def.generic_params, &def.type_params),
            Item::Enum(def) => self.bind_generics(&def.generic_params, &def.type_params),
            Item::TypeAlias(def) => self.bind_generics(&[], &def.type_params),
            Item::Trait(def) | Item::Interface(def) => {
                self.bind_generics(&[], &def.type_params);
                self.type_locals.insert("Self".into());
            },
            Item::Impl(_) => { self.type_locals.insert("Self".into()); }
            _ => {},
        }
        match item {
            Item::Function(f) => self.function(f, true)?,
            Item::Struct(s) => { self.generic_types(&mut s.generic_params)?; self.ident(&mut s.name); for f in &mut s.fields { self.ty(&mut f.ty)?; } for i in &mut s.invariants { self.expr(&mut i.condition)?; } }
            Item::Enum(e) => { self.generic_types(&mut e.generic_params)?; self.ident(&mut e.name); for v in &mut e.variants { for t in &mut v.fields { self.ty(t)?; } } }
            Item::Impl(i) => { self.ident(&mut i.target); if let Some(t) = &mut i.interface { self.ident(t); } for f in &mut i.methods { self.function(f, false)?; } }
            Item::Const(c) => { self.ident(&mut c.name); if let Some(t) = &mut c.ty { self.ty(t)?; } self.expr(&mut c.value)?; }
            Item::Static(s) => { self.ident(&mut s.name); self.ty(&mut s.ty)?; if let Some(e) = &mut s.value { self.expr(e)?; } }
            Item::TypeAlias(t) => { self.ident(&mut t.name); self.ty(&mut t.ty)?; }
            Item::Union(u) => { self.ident(&mut u.name); for f in &mut u.fields { self.ty(&mut f.ty)?; } }
            Item::Extern(b) => for i in &mut b.items { match i {
                ForeignItem::Fn { params, ret_type, .. } => { for p in params { self.ty(&mut p.ty)?; } if let Some(t) = ret_type { self.ty(t)?; } }
                ForeignItem::Static { ty, .. } => self.ty(ty)?,
            } },
            Item::Trait(t) | Item::Interface(t) => {
                self.ident(&mut t.name);
                for m in &mut t.methods { for p in &mut m.params { self.ty(&mut p.ty)?; } if let Some(t) = &mut m.ret_type { self.ty(t)?; } }
                for f in &mut t.default_methods { self.function(f, false)?; }
            }
            _ => {}
        }
        self.locals = saved_locals;
        self.type_locals = saved_types;
        self.const_locals = saved_consts;
        Ok(())
    }
    fn block(&mut self, block: &mut Block) -> Result<()> {
        let saved = self.locals.clone();
        for stmt in &mut block.stmts { match stmt {
            Stmt::Let { name, ty, value, .. } => { if let Some(t) = ty { self.ty(t)?; } if let Some(e) = value { self.expr(e)?; } self.locals.insert(name.name.clone()); }
            Stmt::Expr(e) | Stmt::Semi(e) => self.expr(e)?,
            Stmt::Return { value, span } => {
                if value.is_none() && self.zero_status_returns { *value = Some(Expr::Literal(Literal::Int(0, *span))); }
                if let Some(expr) = value { self.expr(expr)?; }
            }
            _ => {}
        } }
        self.locals = saved;
        Ok(())
    }
    fn pattern(&mut self, p: &mut Pattern) {
        match p {
            Pattern::Binding { name, .. } => { self.locals.insert(name.name.clone()); }
            Pattern::Struct { name, fields, .. } => { self.ident(name); for (_, p) in fields { self.pattern(p); } }
            Pattern::Tuple { elements, .. } => for p in elements { self.pattern(p); },
            Pattern::Variant { enum_name, fields, .. } => { if let Some(n) = enum_name { self.ident(n); } for p in fields { self.pattern(p); } }
            _ => {}
        }
    }
    fn expr(&mut self, e: &mut Expr) -> Result<()> {
        match e {
            Expr::Ident(id) => if !self.locals.contains(&id.name) {
                if let Some(binding) = self.names.get(&id.name) {
                    if let Some(declaration) = binding.foreign_declaration {
                        self.foreign_uses.insert(id.span, declaration);
                    }
                    id.name.clone_from(&binding.emitted);
                }
            },
            Expr::Path { segments, span } => {
                let path = segments.iter().map(|i| i.name.as_str()).collect::<Vec<_>>().join("::");
                if let Some(binding) = self.names.get(&path) {
                    if let Some(declaration) = binding.foreign_declaration {
                        self.foreign_uses.insert(*span, declaration);
                    }
                    *e = Expr::Ident(Ident { name: binding.emitted.clone(), span: *span });
                }
                else {
                    let mut found = false;
                    for count in (1..segments.len()).rev() {
                        let prefix = segments[..count].iter().map(|i| i.name.as_str()).collect::<Vec<_>>().join("::");
                        if let Some(new) = self.names.get(&prefix) {
                            let first = Ident { name: new.emitted.clone(), span: segments[0].span };
                            segments.splice(..count, [first]);
                            found = true;
                            break;
                        }
                    }
                    if !found && segments.first().is_some_and(|s| self.modules.contains(&s.name)) {
                        return Err(Error::ModuleError(format!("no public symbol '{path}'")));
                    }
                }
            }
            Expr::Binary { left, right, .. } => { self.expr(left)?; self.expr(right)?; }
            Expr::Unary { expr, .. } | Expr::Deref { expr, .. } | Expr::Ref { expr, .. } | Expr::Try { expr, .. } => self.expr(expr)?,
            Expr::Call { func, args, .. } => { self.expr(func)?; for a in args { self.expr(a)?; } }
            Expr::Field { expr, .. } => self.expr(expr)?,
            Expr::MethodCall { expr, args, .. } => { self.expr(expr)?; for a in args { self.expr(a)?; } }
            Expr::Index { expr, index, .. } => { self.expr(expr)?; self.expr(index)?; }
            Expr::Block(b) | Expr::Loop { body: b, .. } | Expr::Unsafe { body: b, .. } => self.block(b)?,
            Expr::If { cond, then_block, else_block, .. } => { self.expr(cond)?; self.block(then_block)?; if let Some(b) = else_block { self.block(b)?; } }
            Expr::While { cond, body, .. } => { self.expr(cond)?; self.block(body)?; }
            Expr::For { var, iter, body, .. } => { self.expr(iter)?; let saved = self.locals.clone(); self.locals.insert(var.name.clone()); self.block(body)?; self.locals = saved; }
            Expr::Match { expr, arms, .. } => { self.expr(expr)?; for a in arms { let saved = self.locals.clone(); self.pattern(&mut a.pattern); if let Some(g) = &mut a.guard { self.expr(g)?; } self.expr(&mut a.body)?; self.locals = saved; } }
            Expr::StructLit { name, fields, .. } => { self.ident(name); for (_, e) in fields { self.expr(e)?; } }
            Expr::Array { elements, .. } | Expr::Tuple { elements, .. } => for e in elements { self.expr(e)?; },
            Expr::Cast { expr, ty, .. } => { self.expr(expr)?; self.ty(ty)?; }
            Expr::Range { start, end, .. } => { if let Some(e) = start { self.expr(e)?; } if let Some(e) = end { self.expr(e)?; } }
            Expr::Asm { operands, .. } => for op in operands { if let Some(e) = &mut op.expr { self.expr(e)?; } },
            Expr::Closure { params, ret_type, body, .. } => { let saved = self.locals.clone(); let saved_zero_status = self.zero_status_returns; self.zero_status_returns = false; for p in params { self.locals.insert(p.name.name.clone()); if let Some(t) = &mut p.ty { self.ty(t)?; } } if let Some(t) = ret_type { self.ty(t)?; } self.expr(body)?; self.locals = saved; self.zero_status_returns = saved_zero_status; }
            Expr::Layout { ty, .. } => self.ty(ty)?,
            Expr::Literal(_) => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_namespace_collisions_replay_the_complete_overlay_snapshot() {
        let directory = std::env::temp_dir().join(format!("aether-resolver-overlay-{}", std::process::id()));
        let root = directory.join("main.aeth");
        let late = directory.join("late.aeth");
        let last = directory.join("last.aeth");
        let late_text = "use last::other; extern \"C\" { fn __aeth_m0_local() -> i32; } pub fn foreign() -> i32 { __aeth_m0_local() + other() }";
        let last_text = "extern \"C\" { fn __aeth_1_m0_local() -> i32; } pub fn other() -> i32 { __aeth_1_m0_local() }";
        let mut resolver = Resolver::new(ModuleSearch::default()).with_overlays(HashMap::from([
            (late.clone(), late_text.into()), (last.clone(), last_text.into()),
        ]));
        let program = resolver.resolve(&root, "use late::foreign; fn local() -> i32 { 7 } fn main() -> i32 { local() }".into()).unwrap();
        assert_eq!(resolver.sources.len(), 3);
        assert_eq!(resolver.sources.iter().find(|source| source.path == late).unwrap().text, late_text);
        assert_eq!(resolver.sources.iter().find(|source| source.path == last).unwrap().text, last_text);
        let local = program.items.iter().find_map(|item| match item {
            Item::Function(function) if resolver.original_name(&function.name.name) == Some("local") => Some(function), _ => None,
        }).unwrap();
        assert_eq!(local.name.name, "__aeth_2_m0_local");
        assert_eq!(resolver.original_name("__aeth_m0_local"), None);
        assert_eq!(resolver.original_name("__aeth_1_m0_local"), None);
        assert!(program.items.iter().filter_map(|item| match item { Item::Function(function) => Some(function.span.file_id), _ => None }).all(|id| id < 3));
    }

    #[test]
    fn module_mangling_preserves_generic_type_and_const_bindings() {
        let names = [("T", "__module_T"), ("N", "__module_N"), ("identity", "__module_identity"), ("Container", "__module_Container")]
            .into_iter().map(|(key, emitted)| (key.into(), ResolvedName { emitted: emitted.into(), foreign_declaration: None })).collect();
        let mut program = Parser::new(Lexer::new("fn identity<T>(value: T) -> T { value } struct Container<T, const N: usize> { value: T }", 0)).parse_program().unwrap();
        let mut rewriter = Rewriter { names: &names, foreign_uses: &mut HashMap::new(), modules: HashSet::new(), locals: HashSet::new(), type_locals: HashSet::new(), const_locals: HashSet::new(), normalize_main: false, zero_status_returns: false };
        for item in &mut program.items { rewriter.item(item).unwrap(); }
        let Item::Function(function) = &program.items[0] else { panic!("expected fn") };
        assert_eq!(function.name.name, "__module_identity");
        assert!(matches!(&function.params[0].ty, Type::Named(name, _) if name == "T"));
        let Item::Struct(structure) = &program.items[1] else { panic!("expected struct") };
        assert_eq!(structure.name.name, "__module_Container");
        assert!(matches!(&structure.fields[0].ty, Type::Named(name, _) if name == "T"));
    }
    #[test]
    fn default_entry_normalization_does_not_change_closure_returns() {
        let mut program = Parser::new(Lexer::new("fn main() { let callback: i64 = | | { return; }; return; }", 0)).parse_program().unwrap();
        let names = HashMap::new();
        let mut rewriter = Rewriter { names: &names, foreign_uses: &mut HashMap::new(), modules: HashSet::new(), locals: HashSet::new(), type_locals: HashSet::new(), const_locals: HashSet::new(), normalize_main: true, zero_status_returns: false };
        rewriter.item(&mut program.items[0]).unwrap();
        let Item::Function(function) = &program.items[0] else { panic!("expected fn") };
        let Stmt::Let { value: Some(Expr::Closure { body, .. }), .. } = &function.body.stmts[0] else { panic!("expected closure") };
        let Expr::Block(block) = body.as_ref() else { panic!("expected block") };
        assert!(matches!(block.stmts[0], Stmt::Return { value: None, .. }));
        assert!(function.body.stmts.iter().any(|stmt| matches!(stmt, Stmt::Return { value: Some(Expr::Literal(Literal::Int(0, _))), .. })));
    }

}
