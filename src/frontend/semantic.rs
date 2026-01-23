//! Semantic Analysis for AetherLang
//!
//! Performs:
//! - Symbol table management (scopes, definitions)
//! - Type checking
//! - Type checking
//! - Ownership analysis (own/ref/mut)
#![allow(dead_code)]

use std::collections::HashMap;
use crate::frontend::ast::*;
use crate::frontend::module::ModuleLoader;
use crate::types::*;
use crate::types::type_system::ConstBinOp;
use crate::utils::{Span, Error, Result};

// ==================== Symbol Table ====================

/// Unique identifier for a scope
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(usize);

/// Symbol information
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub ty: ResolvedType,
    pub span: Span,
    pub mutable: bool,
}

/// Kind of symbol
#[derive(Debug, Clone)]
pub enum SymbolKind {
    Variable,
    Function { params: Vec<ResolvedType>, ret: ResolvedType, type_params: Vec<String>, const_params: Vec<(String, ResolvedType)> },
    Struct { fields: Vec<(String, ResolvedType)>, type_params: Vec<String>, const_params: Vec<(String, ResolvedType)> },
    Enum { variants: Vec<String>, type_params: Vec<String>, const_params: Vec<(String, ResolvedType)> },
    Param { ownership: Ownership },
    TypeParam,
    /// Const generic parameter (e.g., N in `const N: usize`)
    ConstParam { ty: ResolvedType },
    TypeAlias { target: ResolvedType },
}

/// A scope containing symbols
#[derive(Debug)]
struct Scope {
    parent: Option<ScopeId>,
    symbols: HashMap<String, Symbol>,
}

/// Symbol table with nested scopes
pub struct SymbolTable {
    scopes: Vec<Scope>,
    current: ScopeId,
}

impl SymbolTable {
    pub fn new() -> Self {
        // Create global scope
        let global = Scope {
            parent: None,
            symbols: HashMap::new(),
        };
        Self {
            scopes: vec![global],
            current: ScopeId(0),
        }
    }

    /// Enter a new scope
    pub fn enter_scope(&mut self) -> ScopeId {
        let id = ScopeId(self.scopes.len());
        self.scopes.push(Scope {
            parent: Some(self.current),
            symbols: HashMap::new(),
        });
        self.current = id;
        id
    }

    /// Exit the current scope
    pub fn exit_scope(&mut self) {
        if let Some(parent) = self.scopes[self.current.0].parent {
            self.current = parent;
        }
    }

    /// Define a symbol in the current scope
    pub fn define(&mut self, symbol: Symbol) -> Result<()> {
        let scope = &mut self.scopes[self.current.0];
        if let Some(existing) = scope.symbols.get(&symbol.name) {
            // Allow extern functions to override builtin function definitions
            if matches!(existing.kind, SymbolKind::Function { .. }) 
               && matches!(symbol.kind, SymbolKind::Function { .. }) {
                // Silently replace - extern declaration overrides builtin
                scope.symbols.insert(symbol.name.clone(), symbol);
                return Ok(());
            }
            return Err(Error::DuplicateDefinition {
                name: symbol.name.clone(),
                span: symbol.span,
            });
        }
        scope.symbols.insert(symbol.name.clone(), symbol);
        Ok(())
    }

    /// Look up a symbol, searching from current scope upward
    pub fn lookup(&self, name: &str) -> Option<&Symbol> {
        let mut scope_id = Some(self.current);
        while let Some(id) = scope_id {
            if let Some(symbol) = self.scopes[id.0].symbols.get(name) {
                return Some(symbol);
            }
            scope_id = self.scopes[id.0].parent;
        }
        None
    }

    /// Look up a symbol only in the current scope
    pub fn lookup_local(&self, name: &str) -> Option<&Symbol> {
        self.scopes[self.current.0].symbols.get(name)
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== Ownership State ====================

/// Tracks ownership state of variables
#[derive(Debug, Clone)]
pub struct OwnershipState {
    /// Variables that are currently owned
    owned: HashMap<String, Span>,
    /// Variables that have been moved
    moved: HashMap<String, Span>,
    /// Variables that are immutably borrowed (with count)
    borrowed: HashMap<String, usize>,
    /// Variables that are mutably borrowed
    mut_borrowed: HashMap<String, Span>,
}

impl OwnershipState {
    pub fn new() -> Self {
        Self {
            owned: HashMap::new(),
            moved: HashMap::new(),
            borrowed: HashMap::new(),
            mut_borrowed: HashMap::new(),
        }
    }

    /// Mark a variable as owned
    pub fn add_owned(&mut self, name: String, span: Span) {
        self.owned.insert(name, span);
    }

    /// Check if a variable is owned and not moved
    pub fn is_available(&self, name: &str) -> bool {
        self.owned.contains_key(name) && !self.moved.contains_key(name)
    }

    /// Move a variable (transfer ownership)
    pub fn move_var(&mut self, name: &str, span: Span) -> Result<()> {
        if self.moved.contains_key(name) {
            return Err(Error::UseAfterMove {
                var: name.to_string(),
                span,
            });
        }
        if self.borrowed.contains_key(name) || self.mut_borrowed.contains_key(name) {
            return Err(Error::CannotMoveWhileBorrowed {
                var: name.to_string(),
                span,
            });
        }
        self.moved.insert(name.to_string(), span);
        Ok(())
    }

    /// Borrow a variable immutably
    pub fn borrow(&mut self, name: &str, span: Span) -> Result<()> {
        if self.moved.contains_key(name) {
            return Err(Error::UseAfterMove {
                var: name.to_string(),
                span,
            });
        }
        if self.mut_borrowed.contains_key(name) {
            return Err(Error::CannotBorrowWhileMutBorrowed {
                var: name.to_string(),
                span,
            });
        }
        *self.borrowed.entry(name.to_string()).or_insert(0) += 1;
        Ok(())
    }

    /// Borrow a variable mutably
    pub fn borrow_mut(&mut self, name: &str, span: Span) -> Result<()> {
        if self.moved.contains_key(name) {
            return Err(Error::UseAfterMove {
                var: name.to_string(),
                span,
            });
        }
        if self.borrowed.contains_key(name) {
            return Err(Error::CannotMutBorrowWhileBorrowed {
                var: name.to_string(),
                span,
            });
        }
        if self.mut_borrowed.contains_key(name) {
            return Err(Error::CannotMutBorrowTwice {
                var: name.to_string(),
                span,
            });
        }
        self.mut_borrowed.insert(name.to_string(), span);
        Ok(())
    }

    /// Release an immutable borrow
    pub fn release_borrow(&mut self, name: &str) {
        if let Some(count) = self.borrowed.get_mut(name) {
            *count -= 1;
            if *count == 0 {
                self.borrowed.remove(name);
            }
        }
    }

    /// Release a mutable borrow
    pub fn release_mut_borrow(&mut self, name: &str) {
        self.mut_borrowed.remove(name);
    }
}

impl Default for OwnershipState {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== Module System ====================

use std::path::PathBuf;

/// Module search paths for resolving use statements
pub struct ModuleResolver {
    /// Paths to search for modules
    search_paths: Vec<PathBuf>,
    /// Cached module symbols by module name
    cached_modules: HashMap<String, Vec<Symbol>>,
    /// Module loader for parsing modules
    loader: ModuleLoader,
}

impl ModuleResolver {
    pub fn new() -> Self {
        Self {
            search_paths: vec![
                PathBuf::from("."),
                PathBuf::from("src_aether"),
                PathBuf::from("stdlib"),
            ],
            cached_modules: HashMap::new(),
            loader: ModuleLoader::new(),
        }
    }
    
    /// Find module file by name
    pub fn find_module(&self, name: &str) -> Option<PathBuf> {
        self.loader.find_module_file(name)
    }
    
    /// Load a module and return its public items as symbols
    pub fn load_module_symbols(&mut self, module_name: &str, span: Span) -> Result<Vec<(String, Symbol)>> {
        // Check cache first
        if let Some(cached) = self.cached_modules.get(module_name) {
            return Ok(cached.iter().map(|s| (s.name.clone(), s.clone())).collect());
        }
        
        // Load and parse the module
        let parsed = self.loader.load_module(module_name)?;
        
        // Clone the items to avoid borrow conflict
        let items: Vec<Item> = parsed.items.clone();
        
        // Convert public items to symbols
        let mut symbols = Vec::new();
        for item in &items {
            if Self::is_item_public(&item) {
                if let Some(symbol) = self.item_to_symbol(&item, span) {
                    symbols.push(symbol);
                }
            }
        }
        
        // Cache the symbols
        self.cached_modules.insert(module_name.to_string(), symbols.clone());
        
        Ok(symbols.iter().map(|s| (s.name.clone(), s.clone())).collect())
    }
    
    /// Check if an item is public
    fn is_item_public(item: &Item) -> bool {
        match item {
            Item::Function(f) => f.is_pub,
            Item::Struct(s) => s.is_pub,
            Item::Enum(_) => true,
            Item::Const(_) => true,
            _ => false,
        }
    }

    
    /// Convert an AST item to a symbol
    fn item_to_symbol(&self, item: &Item, span: Span) -> Option<Symbol> {
        match item {
            Item::Function(f) => {
                let params: Vec<ResolvedType> = f.params.iter()
                    .map(|p| self.ast_type_to_resolved(&p.ty))
                    .collect();
                let ret = f.ret_type.as_ref()
                    .map(|t| self.ast_type_to_resolved(t))
                    .unwrap_or(ResolvedType::unit());
                Some(Symbol {
                    name: f.name.name.clone(),
                    kind: SymbolKind::Function { params: params.clone(), ret: ret.clone(), type_params: vec![], const_params: vec![] },
                    ty: ResolvedType::Function { params, ret: Box::new(ret) },
                    span,
                    mutable: false,
                })
            }
            Item::Struct(s) => {
                let fields: Vec<(String, ResolvedType)> = s.fields.iter()
                    .map(|f| (f.name.name.clone(), self.ast_type_to_resolved(&f.ty)))
                    .collect();
                Some(Symbol {
                    name: s.name.name.clone(),
                    kind: SymbolKind::Struct { fields: fields.clone(), type_params: vec![], const_params: vec![] },
                    ty: ResolvedType::Struct { name: s.name.name.clone(), fields },
                    span,
                    mutable: false,
                })
            }
            Item::Enum(e) => {
                let variants: Vec<String> = e.variants.iter()
                    .map(|v| v.name.name.clone())
                    .collect();
                Some(Symbol {
                    name: e.name.name.clone(),
                    kind: SymbolKind::Enum { variants, type_params: vec![], const_params: vec![] },
                    ty: ResolvedType::Enum { name: e.name.name.clone() },
                    span,
                    mutable: false,
                })
            }
            Item::Const(c) => {
                let ty = c.ty.as_ref()
                    .map(|t| self.ast_type_to_resolved(t))
                    .unwrap_or(ResolvedType::Unknown);
                Some(Symbol {
                    name: c.name.name.clone(),
                    kind: SymbolKind::Variable,
                    ty,
                    span,
                    mutable: false,
                })
            }
            _ => None,
        }
    }
    
    /// Convert AST type to resolved type (simplified)
    fn ast_type_to_resolved(&self, ty: &Type) -> ResolvedType {
        match ty {
            Type::Named(name, _) => {
                match name.as_str() {
                    "i8" => ResolvedType::Primitive(PrimitiveType::I8),
                    "i16" => ResolvedType::Primitive(PrimitiveType::I16),
                    "i32" => ResolvedType::Primitive(PrimitiveType::I32),
                    "i64" => ResolvedType::Primitive(PrimitiveType::I64),
                    "u8" => ResolvedType::Primitive(PrimitiveType::U8),
                    "u16" => ResolvedType::Primitive(PrimitiveType::U16),
                    "u32" => ResolvedType::Primitive(PrimitiveType::U32),
                    "u64" => ResolvedType::Primitive(PrimitiveType::U64),
                    "f32" => ResolvedType::Primitive(PrimitiveType::F32),
                    "f64" => ResolvedType::Primitive(PrimitiveType::F64),
                    "bool" => ResolvedType::Primitive(PrimitiveType::Bool),
                    "void" => ResolvedType::UNIT,
                    // SIMD vector types
                    "f32x4" => ResolvedType::Vector(Box::new(ResolvedType::Primitive(PrimitiveType::F32)), 4),
                    "f32x8" => ResolvedType::Vector(Box::new(ResolvedType::Primitive(PrimitiveType::F32)), 8),
                    "f64x2" => ResolvedType::Vector(Box::new(ResolvedType::Primitive(PrimitiveType::F64)), 2),
                    "f64x4" => ResolvedType::Vector(Box::new(ResolvedType::Primitive(PrimitiveType::F64)), 4),
                    "i32x4" => ResolvedType::Vector(Box::new(ResolvedType::Primitive(PrimitiveType::I32)), 4),
                    "i32x8" => ResolvedType::Vector(Box::new(ResolvedType::Primitive(PrimitiveType::I32)), 8),
                    "i64x2" => ResolvedType::Vector(Box::new(ResolvedType::Primitive(PrimitiveType::I64)), 2),
                    "i64x4" => ResolvedType::Vector(Box::new(ResolvedType::Primitive(PrimitiveType::I64)), 4),
                    _ => ResolvedType::Struct { name: name.clone(), fields: vec![] },
                }
            }
            Type::Pointer(inner, _) => {
                ResolvedType::Pointer(Box::new(self.ast_type_to_resolved(inner)))
            }
            Type::Ref { mutable, inner, .. } => {
                ResolvedType::Reference {
                    mutable: *mutable,
                    inner: Box::new(self.ast_type_to_resolved(inner)),
                }
            }
            Type::Array { elem, size, .. } => {
                ResolvedType::Array {
                    elem: Box::new(self.ast_type_to_resolved(elem)),
                    size: *size,
                }
            }
            Type::Unit(_) => ResolvedType::UNIT,
            Type::Never(_) => ResolvedType::never(),
            _ => ResolvedType::Unknown,
        }
    }
}

// ==================== Semantic Analyzer ====================

/// Semantic analyzer
pub struct SemanticAnalyzer {
    pub symbols: SymbolTable,
    pub errors: Vec<Error>,
    ownership: OwnershipState,
    // AI-Native extensions
    /// Current function's declared effects (for effect propagation checking)
    current_effects: Option<EffectSet>,
    /// Whether we're in strict mode (@production) or lenient mode (@prototype)
    strict_mode: bool,
    /// Module resolver for use statements
    module_resolver: ModuleResolver,
    /// Imported modules: module_name -> Vec<(symbol_name, Symbol)>
    pub imported_modules: HashMap<String, Vec<(String, Symbol)>>,
}

impl SemanticAnalyzer {
    pub fn new() -> Self {
        let mut analyzer = Self {
            symbols: SymbolTable::new(),
            errors: Vec::new(),
            ownership: OwnershipState::new(),
            current_effects: None,
            strict_mode: false, // Default: lenient mode
            module_resolver: ModuleResolver::new(),
            imported_modules: HashMap::new(),
        };
        analyzer.register_builtins();
        analyzer
    }
    
    /// Set strict mode for production-level checking
    pub fn set_strict_mode(&mut self, strict: bool) {
        self.strict_mode = strict;
    }
    
    /// Register built-in functions
    fn register_builtins(&mut self) {
        // I/O functions
        self.define_builtin("print", vec![ResolvedType::String], ResolvedType::unit());
        self.define_builtin("println", vec![ResolvedType::String], ResolvedType::unit());
        self.define_builtin("puts", vec![ResolvedType::Pointer(Box::new(ResolvedType::U8))], ResolvedType::I32);
        self.define_builtin("print_i64", vec![ResolvedType::I64], ResolvedType::unit());
        self.define_builtin("println_i64", vec![ResolvedType::I64], ResolvedType::unit());
        
        // Memory functions
        self.define_builtin("alloc", vec![ResolvedType::U64], 
            ResolvedType::Pointer(Box::new(ResolvedType::U8)));
        self.define_builtin("malloc", vec![ResolvedType::U64], 
            ResolvedType::Pointer(Box::new(ResolvedType::U8)));
        self.define_builtin("free", 
            vec![ResolvedType::Pointer(Box::new(ResolvedType::U8))], 
            ResolvedType::unit());
        
        // C library functions for self-hosting
        self.define_builtin("atof", vec![ResolvedType::Pointer(Box::new(ResolvedType::U8))], 
            ResolvedType::F64);
        self.define_builtin("strcmp", vec![
            ResolvedType::Pointer(Box::new(ResolvedType::U8)),
            ResolvedType::Pointer(Box::new(ResolvedType::U8)),
        ], ResolvedType::I32);
        
        // Process control
        self.define_builtin("exit", vec![ResolvedType::I32], ResolvedType::never());
        
        // Debug
        self.define_builtin("assert", vec![ResolvedType::BOOL], ResolvedType::UNIT);
        
        // SIMD intrinsics for f32x4
        let f32x4 = ResolvedType::Vector(Box::new(ResolvedType::Primitive(PrimitiveType::F32)), 4);
        self.define_builtin("f32x4_splat", vec![ResolvedType::Primitive(PrimitiveType::F32)], f32x4.clone());
        self.define_builtin("f32x4_add", vec![f32x4.clone(), f32x4.clone()], f32x4.clone());
        self.define_builtin("f32x4_sub", vec![f32x4.clone(), f32x4.clone()], f32x4.clone());
        self.define_builtin("f32x4_mul", vec![f32x4.clone(), f32x4.clone()], f32x4.clone());
        self.define_builtin("f32x4_div", vec![f32x4.clone(), f32x4.clone()], f32x4.clone());
        self.define_builtin("f32x4_sum", vec![f32x4.clone()], ResolvedType::Primitive(PrimitiveType::F32));
        
        // __simd_* prefixed versions for simd.aeth
        let f32_ty = ResolvedType::Primitive(PrimitiveType::F32);
        let f64_ty = ResolvedType::Primitive(PrimitiveType::F64);
        let i32_ty = ResolvedType::Primitive(PrimitiveType::I32);
        let f32_ptr = ResolvedType::Pointer(Box::new(f32_ty.clone()));
        
        // f32x4 intrinsics
        self.define_builtin("__simd_f32x4_new", vec![f32_ty.clone(), f32_ty.clone(), f32_ty.clone(), f32_ty.clone()], f32x4.clone());
        self.define_builtin("__simd_f32x4_splat", vec![f32_ty.clone()], f32x4.clone());
        self.define_builtin("__simd_f32x4_add", vec![f32x4.clone(), f32x4.clone()], f32x4.clone());
        self.define_builtin("__simd_f32x4_sub", vec![f32x4.clone(), f32x4.clone()], f32x4.clone());
        self.define_builtin("__simd_f32x4_mul", vec![f32x4.clone(), f32x4.clone()], f32x4.clone());
        self.define_builtin("__simd_f32x4_div", vec![f32x4.clone(), f32x4.clone()], f32x4.clone());
        self.define_builtin("__simd_f32x4_sum", vec![f32x4.clone()], f32_ty.clone());
        self.define_builtin("__simd_f32x4_load", vec![f32_ptr.clone()], f32x4.clone());
        self.define_builtin("__simd_f32x4_store", vec![f32_ptr.clone(), f32x4.clone()], ResolvedType::unit());
        
        // SIMD intrinsics for f64x2
        let f64x2 = ResolvedType::Vector(Box::new(ResolvedType::Primitive(PrimitiveType::F64)), 2);
        self.define_builtin("f64x2_splat", vec![ResolvedType::Primitive(PrimitiveType::F64)], f64x2.clone());
        self.define_builtin("f64x2_add", vec![f64x2.clone(), f64x2.clone()], f64x2.clone());
        self.define_builtin("f64x2_mul", vec![f64x2.clone(), f64x2.clone()], f64x2.clone());
        
        // __simd_* prefixed versions for f64x2
        self.define_builtin("__simd_f64x2_new", vec![f64_ty.clone(), f64_ty.clone()], f64x2.clone());
        self.define_builtin("__simd_f64x2_splat", vec![f64_ty.clone()], f64x2.clone());
        self.define_builtin("__simd_f64x2_add", vec![f64x2.clone(), f64x2.clone()], f64x2.clone());
        self.define_builtin("__simd_f64x2_mul", vec![f64x2.clone(), f64x2.clone()], f64x2.clone());
        
        // SIMD intrinsics for i32x4
        let i32x4 = ResolvedType::Vector(Box::new(ResolvedType::Primitive(PrimitiveType::I32)), 4);
        self.define_builtin("i32x4_splat", vec![ResolvedType::Primitive(PrimitiveType::I32)], i32x4.clone());
        self.define_builtin("i32x4_add", vec![i32x4.clone(), i32x4.clone()], i32x4.clone());
        self.define_builtin("i32x4_mul", vec![i32x4.clone(), i32x4.clone()], i32x4.clone());
        
        // __simd_* prefixed versions for i32x4
        self.define_builtin("__simd_i32x4_new", vec![i32_ty.clone(), i32_ty.clone(), i32_ty.clone(), i32_ty.clone()], i32x4.clone());
        self.define_builtin("__simd_i32x4_splat", vec![i32_ty.clone()], i32x4.clone());
        self.define_builtin("__simd_i32x4_add", vec![i32x4.clone(), i32x4.clone()], i32x4.clone());
        self.define_builtin("__simd_i32x4_mul", vec![i32x4.clone(), i32x4.clone()], i32x4.clone());
    }
    
    /// Define a built-in function
    fn define_builtin(&mut self, name: &str, params: Vec<ResolvedType>, ret: ResolvedType) {
        let symbol = Symbol {
            name: name.to_string(),
            kind: SymbolKind::Function { params, ret, type_params: vec![], const_params: vec![] },
            ty: ResolvedType::Unknown, // Function type handled by kind
            span: Span::dummy(), // Built-in, no source location
            mutable: false,
        };
        let _ = self.symbols.define(symbol);
    }

    /// Analyze a program
    pub fn analyze(&mut self, program: &Program) -> Result<()> {
        // Pass 1: Collect all top-level definitions
        for item in &program.items {
            self.collect_definition(item)?;
        }

        // Pass 2: Type check all items
        for item in &program.items {
            self.check_item(item)?;
        }

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors[0].clone())
        }
    }

    /// Collect a top-level definition
    fn collect_definition(&mut self, item: &Item) -> Result<()> {
        match item {
            Item::Function(func) => {
                let params: Vec<ResolvedType> = func.params.iter()
                    .map(|p| self.resolve_type(&p.ty))
                    .collect::<Result<Vec<_>>>()?;
                let ret = func.ret_type.as_ref()
                    .map(|t| self.resolve_type(t))
                    .transpose()?
                    .unwrap_or(ResolvedType::unit());

                self.symbols.define(Symbol {
                    name: func.name.name.clone(),
                    kind: SymbolKind::Function { params: params.clone(), ret: ret.clone(), type_params: func.type_params.iter().map(|p| p.name.clone()).collect(), const_params: vec![] },
                    ty: ResolvedType::Function {
                        params,
                        ret: Box::new(ret),
                    },
                    span: func.span,
                    mutable: false,
                })?;
            }
            Item::Struct(s) => {
                self.symbols.enter_scope();

                // Collect type params and const params separately
                let mut type_params = Vec::new();
                let mut const_params = Vec::new();

                for param in &s.generic_params {
                    match param {
                        crate::frontend::ast::GenericParam::Type(ident) => {
                            type_params.push(ident.name.clone());
                            self.symbols.define(Symbol {
                                name: ident.name.clone(),
                                kind: SymbolKind::TypeParam,
                                ty: ResolvedType::GenericParam(ident.name.clone()),
                                span: ident.span,
                                mutable: false,
                            })?;
                        }
                        crate::frontend::ast::GenericParam::Const { name, ty } => {
                            let resolved_ty = self.resolve_type(ty)?;
                            const_params.push((name.name.clone(), resolved_ty.clone()));
                            self.symbols.define(Symbol {
                                name: name.name.clone(),
                                kind: SymbolKind::ConstParam { ty: resolved_ty.clone() },
                                ty: resolved_ty,
                                span: name.span,
                                mutable: false,
                            })?;
                        }
                    }
                }

                // Also handle legacy type_params field for backward compatibility
                for param in &s.type_params {
                    if !type_params.contains(&param.name) {
                        type_params.push(param.name.clone());
                        self.symbols.define(Symbol {
                            name: param.name.clone(),
                            kind: SymbolKind::TypeParam,
                            ty: ResolvedType::GenericParam(param.name.clone()),
                            span: param.span,
                            mutable: false,
                        })?;
                    }
                }

                let fields: Vec<(String, ResolvedType)> = s.fields.iter()
                    .map(|f| Ok((f.name.name.clone(), self.resolve_type(&f.ty)?)))
                    .collect::<Result<Vec<_>>>()?;

                self.symbols.exit_scope();

                self.symbols.define(Symbol {
                    name: s.name.name.clone(),
                    kind: SymbolKind::Struct {
                        fields: fields.clone(),
                        type_params,
                        const_params,
                    },
                    ty: ResolvedType::Struct {
                        name: s.name.name.clone(),
                        fields, // Use generic fields
                    },
                    span: s.span,
                    mutable: false,
                })?;
            }
            Item::Enum(e) => {
                self.symbols.enter_scope();

                // Collect type params and const params separately
                let mut type_params = Vec::new();
                let mut const_params = Vec::new();

                for param in &e.generic_params {
                    match param {
                        crate::frontend::ast::GenericParam::Type(ident) => {
                            type_params.push(ident.name.clone());
                            self.symbols.define(Symbol {
                                name: ident.name.clone(),
                                kind: SymbolKind::TypeParam,
                                ty: ResolvedType::GenericParam(ident.name.clone()),
                                span: ident.span,
                                mutable: false,
                            })?;
                        }
                        crate::frontend::ast::GenericParam::Const { name, ty } => {
                            let resolved_ty = self.resolve_type(ty)?;
                            const_params.push((name.name.clone(), resolved_ty.clone()));
                            self.symbols.define(Symbol {
                                name: name.name.clone(),
                                kind: SymbolKind::ConstParam { ty: resolved_ty.clone() },
                                ty: resolved_ty,
                                span: name.span,
                                mutable: false,
                            })?;
                        }
                    }
                }

                // Also handle legacy type_params field for backward compatibility
                for param in &e.type_params {
                    if !type_params.contains(&param.name) {
                        type_params.push(param.name.clone());
                        self.symbols.define(Symbol {
                            name: param.name.clone(),
                            kind: SymbolKind::TypeParam,
                            ty: ResolvedType::GenericParam(param.name.clone()),
                            span: param.span,
                            mutable: false,
                        })?;
                    }
                }

                let variants: Vec<String> = e.variants.iter()
                    .map(|v| v.name.name.clone())
                    .collect();

                self.symbols.exit_scope();

                self.symbols.define(Symbol {
                    name: e.name.name.clone(),
                    kind: SymbolKind::Enum { variants, type_params, const_params },
                    ty: ResolvedType::Enum { name: e.name.name.clone() },
                    span: e.span,
                    mutable: false,
                })?;
            }
            Item::Const(c) => {
                let ty = c.ty.as_ref()
                    .map(|t| self.resolve_type(t))
                    .transpose()?
                    .unwrap_or(ResolvedType::Unknown);

                self.symbols.define(Symbol {
                    name: c.name.name.clone(),
                    kind: SymbolKind::Variable,
                    ty,
                    span: c.span,
                    mutable: false,
                })?;
            }
            Item::Extern(ext) => {
                // Register extern functions in symbol table
                for foreign_item in &ext.items {
                    match foreign_item {
                        crate::frontend::ast::ForeignItem::Fn { name, params, ret_type, .. } => {
                            let param_types: Vec<ResolvedType> = params.iter()
                                .map(|p| self.resolve_type(&p.ty))
                                .collect::<Result<Vec<_>>>()?;
                            let ret = ret_type.as_ref()
                                .map(|t| self.resolve_type(t))
                                .transpose()?
                                .unwrap_or(ResolvedType::unit());

                            self.symbols.define(Symbol {
                                name: name.name.clone(),
                                kind: SymbolKind::Function { params: param_types.clone(), ret: ret.clone(), type_params: vec![], const_params: vec![] },
                                ty: ResolvedType::Function {
                                    params: param_types,
                                    ret: Box::new(ret),
                                },
                                span: name.span,
                                mutable: false,
                            })?;
                        }
                        crate::frontend::ast::ForeignItem::Static { name, ty, .. } => {
                            let resolved_ty = self.resolve_type(ty)?;
                            self.symbols.define(Symbol {
                                name: name.name.clone(),
                                kind: SymbolKind::Variable,
                                ty: resolved_ty,
                                span: name.span,
                                mutable: false,
                            })?;
                        }
                    }
                }
            }
            Item::TypeAlias(alias) => {
                // Resolve the target type
                let target = self.resolve_type(&alias.ty)?;
                self.symbols.define(Symbol {
                    name: alias.name.name.clone(),
                    kind: SymbolKind::TypeAlias { target: target.clone() },
                    ty: target,
                    span: alias.span,
                    mutable: false,
                })?;
            }
            Item::Use(use_decl) => {
                // Resolve use declaration by loading module symbols
                self.resolve_use_decl(use_decl)?;
            }
            _ => {} // Impl and Interface handled separately
        }
        Ok(())
    }
    
    /// Resolve a use declaration by importing symbols from the target module
    fn resolve_use_decl(&mut self, use_decl: &UseDecl) -> Result<()> {
        // Get module name from path (first segment)
        if use_decl.path.is_empty() {
            return Ok(());
        }
        
        let module_name = use_decl.path[0].name.clone();
        
        // Check if already imported
        if self.imported_modules.contains_key(&module_name) {
            return Ok(());
        }
        
        // Try to load the actual module file
        if self.module_resolver.find_module(&module_name).is_some() {
            // Load module and get symbols
            match self.module_resolver.load_module_symbols(&module_name, use_decl.span) {
                Ok(symbols) => {
                    // Store in imported_modules for qualified name lookup
                    self.imported_modules.insert(module_name.clone(), symbols.clone());
                    
                    // Also register symbols directly for simple imports
                    for (name, symbol) in &symbols {
                        // Create qualified name: module::symbol
                        let qualified_name = format!("{}::{}", module_name, name);
                        let mut qualified_symbol = symbol.clone();
                        qualified_symbol.name = qualified_name;
                        let _ = self.symbols.define(qualified_symbol);
                    }
                    return Ok(());
                }
                Err(_) => {
                    // Fall back to placeholders
                }
            }
        }
        
        // Module not found or failed to load - register placeholders for self-hosting
        self.register_module_placeholder(&module_name, use_decl)?;
        
        Ok(())
    }
    
    /// Register placeholder symbols from a module
    fn register_module_placeholder(&mut self, module_name: &str, use_decl: &UseDecl) -> Result<()> {
        // Handle common self-hosting module types
        match module_name {
            "span" => {
                // Register Span struct
                self.symbols.define(Symbol {
                    name: "Span".to_string(),
                    kind: SymbolKind::Struct {
                        fields: vec![
                            ("file_id".to_string(), ResolvedType::Primitive(PrimitiveType::U64)),
                            ("start".to_string(), ResolvedType::Primitive(PrimitiveType::U64)),
                            ("end".to_string(), ResolvedType::Primitive(PrimitiveType::U64)),
                        ],
                        type_params: vec![],
                        const_params: vec![],
                    },
                    ty: ResolvedType::Struct {
                        name: "Span".to_string(),
                        fields: vec![
                            ("file_id".to_string(), ResolvedType::Primitive(PrimitiveType::U64)),
                            ("start".to_string(), ResolvedType::Primitive(PrimitiveType::U64)),
                            ("end".to_string(), ResolvedType::Primitive(PrimitiveType::U64)),
                        ],
                    },
                    span: use_decl.span,
                    mutable: false,
                })?;
            }
            "string" => {
                // Register String struct
                self.symbols.define(Symbol {
                    name: "String".to_string(),
                    kind: SymbolKind::Struct {
                        fields: vec![
                            ("data".to_string(), ResolvedType::Pointer(Box::new(ResolvedType::Primitive(PrimitiveType::U8)))),
                            ("len".to_string(), ResolvedType::Primitive(PrimitiveType::U64)),
                            ("cap".to_string(), ResolvedType::Primitive(PrimitiveType::U64)),
                        ],
                        type_params: vec![],
                        const_params: vec![],
                    },
                    ty: ResolvedType::Struct {
                        name: "String".to_string(),
                        fields: vec![
                            ("data".to_string(), ResolvedType::Pointer(Box::new(ResolvedType::Primitive(PrimitiveType::U8)))),
                            ("len".to_string(), ResolvedType::Primitive(PrimitiveType::U64)),
                            ("cap".to_string(), ResolvedType::Primitive(PrimitiveType::U64)),
                        ],
                    },
                    span: use_decl.span,
                    mutable: false,
                })?;
            }
            "vec" => {
                // Register Vec struct as placeholder
                self.symbols.define(Symbol {
                    name: "Vec".to_string(),
                    kind: SymbolKind::Struct {
                        fields: vec![
                            ("data".to_string(), ResolvedType::Pointer(Box::new(ResolvedType::Primitive(PrimitiveType::U8)))),
                            ("len".to_string(), ResolvedType::Primitive(PrimitiveType::U64)),
                            ("cap".to_string(), ResolvedType::Primitive(PrimitiveType::U64)),
                        ],
                        type_params: vec!["T".to_string()],
                        const_params: vec![],
                    },
                    ty: ResolvedType::Struct {
                        name: "Vec".to_string(),
                        fields: vec![
                            ("data".to_string(), ResolvedType::Pointer(Box::new(ResolvedType::Primitive(PrimitiveType::U8)))),
                            ("len".to_string(), ResolvedType::Primitive(PrimitiveType::U64)),
                            ("cap".to_string(), ResolvedType::Primitive(PrimitiveType::U64)),
                        ],
                    },
                    span: use_decl.span,
                    mutable: false,
                })?;
            }
            "token" => {
                // Register Token and TokenKind as placeholders
                self.symbols.define(Symbol {
                    name: "Token".to_string(),
                    kind: SymbolKind::Struct {
                        fields: vec![
                            ("kind".to_string(), ResolvedType::Enum { name: "TokenKind".to_string() }),
                            ("span".to_string(), ResolvedType::Struct { name: "Span".to_string(), fields: vec![
                                ("file_id".to_string(), ResolvedType::Primitive(PrimitiveType::U64)),
                                ("start".to_string(), ResolvedType::Primitive(PrimitiveType::U64)),
                                ("end".to_string(), ResolvedType::Primitive(PrimitiveType::U64)),
                            ] }),
                        ],
                        type_params: vec![],
                        const_params: vec![],
                    },
                    ty: ResolvedType::Struct {
                        name: "Token".to_string(),
                        fields: vec![
                            ("kind".to_string(), ResolvedType::Enum { name: "TokenKind".to_string() }),
                            ("span".to_string(), ResolvedType::Struct { name: "Span".to_string(), fields: vec![] }),
                        ],
                    },
                    span: use_decl.span,
                    mutable: false,
                })?;
                self.symbols.define(Symbol {
                    name: "TokenKind".to_string(),
                    kind: SymbolKind::Enum { variants: vec![], type_params: vec![], const_params: vec![] },
                    ty: ResolvedType::Enum { name: "TokenKind".to_string() },
                    span: use_decl.span,
                    mutable: false,
                })?;
                // Register keyword_from_str function
                self.symbols.define(Symbol {
                    name: "keyword_from_str".to_string(),
                    kind: SymbolKind::Function {
                        params: vec![ResolvedType::Reference {
                            mutable: false,
                            inner: Box::new(ResolvedType::Struct { name: "String".to_string(), fields: vec![] }),
                        }],
                        ret: ResolvedType::Enum { name: "TokenKind".to_string() },
                        type_params: vec![],
                        const_params: vec![],
                    },
                    ty: ResolvedType::Function {
                        params: vec![ResolvedType::Reference {
                            mutable: false,
                            inner: Box::new(ResolvedType::Struct { name: "String".to_string(), fields: vec![] }),
                        }],
                        ret: Box::new(ResolvedType::Enum { name: "TokenKind".to_string() }),
                    },
                    span: use_decl.span,
                    mutable: false,
                })?;
            }
            "core" => {
                // core module - printf already registered as builtin
            }
            _ => {
                // Unknown module - ignore for now
            }
        }
        Ok(())
    }

    /// Type check an item
    fn check_item(&mut self, item: &Item) -> Result<()> {
        match item {
            Item::Function(func) => self.check_function(func),
            Item::Struct(_) => Ok(()), // Already collected
            Item::Enum(_) => Ok(()),   // Already collected
            Item::Impl(impl_block) => self.check_impl(impl_block),
            Item::Interface(_) => Ok(()), // Already collected
            Item::Const(c) => {
                let _expr_ty = self.check_expr(&c.value)?;
                // TODO: Check that expr_ty matches declared type
                Ok(())
            }
            Item::Macro(_) => Ok(()), // Macro expansion handled elsewhere
            Item::Module(m) => {
                // Recursively check module items
                if let Some(items) = &m.items {
                    for item in items {
                        self.check_item(item)?;
                    }
                }
                Ok(())
            }
            Item::Use(_) => Ok(()), // Import resolution handled elsewhere
            // Phase 8: FFI and System features
            Item::Extern(_) => Ok(()), // FFI - external symbols registered elsewhere
            Item::Static(_) => Ok(()), // TODO: Check static variable type
            Item::Union(_) => Ok(()), // TODO: Check union field types
            Item::Trait(_) => Ok(()), // Trait definitions checked in collect phase
            Item::TypeAlias(_) => Ok(()), // Type aliases resolved in collect phase
        }
    }

    /// Type check a function
    fn check_function(&mut self, func: &Function) -> Result<()> {
        self.symbols.enter_scope();
        self.ownership = OwnershipState::new();
        
        // Set effect context for this function (for effect propagation checking)
        self.current_effects = Some(func.effects.clone());

        // Add parameters to scope
        for param in &func.params {
            let ty = self.resolve_type(&param.ty)?;
            self.symbols.define(Symbol {
                name: param.name.name.clone(),
                kind: SymbolKind::Param { ownership: param.ownership },
                ty: ty.clone(),
                span: param.span,
                mutable: param.ownership == Ownership::Mut,
            })?;
            self.ownership.add_owned(param.name.name.clone(), param.span);
        }
        
        // Resolve return type for 'result' variable in ensures contracts
        let return_type = if let Some(ref ret_ty) = func.ret_type {
            self.resolve_type(ret_ty)?
        } else {
            ResolvedType::UNIT
        };
        
        // Check contract expressions (requires, ensures)
        for contract in &func.contracts {
            // For 'ensures' contracts, add 'result' variable to scope
            // This allows postconditions to reference the return value
            let is_ensures = matches!(contract.kind, crate::frontend::ast::ContractKind::Ensures);
            
            if is_ensures && return_type != ResolvedType::UNIT {
                // Temporarily add 'result' variable for ensures checking
                self.symbols.enter_scope();
                self.symbols.define(Symbol {
                    name: "result".to_string(),
                    kind: SymbolKind::Variable,
                    ty: return_type.clone(),
                    span: func.span,
                    mutable: false,
                })?;
            }
            
            let contract_ty = self.check_expr(&contract.condition)?;
            
            // Contract expressions must be boolean
            if contract_ty != ResolvedType::BOOL && contract_ty != ResolvedType::Unknown {
                if self.strict_mode {
                    if is_ensures && return_type != ResolvedType::UNIT {
                        self.symbols.exit_scope();
                    }
                    return Err(Error::TypeMismatch {
                        expected: "bool".to_string(),
                        got: format!("{:?}", contract_ty),
                        span: contract.span,
                    });
                } else {
                    // In lenient mode, just warn (add to errors but don't fail)
                    self.errors.push(Error::TypeMismatch {
                        expected: "bool".to_string(),
                        got: format!("{:?}", contract_ty),
                        span: contract.span,
                    });
                }
            }
            
            if is_ensures && return_type != ResolvedType::UNIT {
                self.symbols.exit_scope();
            }
        }

        // Check function body
        self.check_block(&func.body)?;
        
        // Clear effect context
        self.current_effects = None;

        self.symbols.exit_scope();
        Ok(())
    }

    /// Type check an impl block
    fn check_impl(&mut self, impl_block: &ImplBlock) -> Result<()> {
        for method in &impl_block.methods {
            self.check_function(method)?;
        }
        Ok(())
    }

    /// Type check a block
    fn check_block(&mut self, block: &Block) -> Result<ResolvedType> {
        let mut last_ty = ResolvedType::unit();

        for stmt in &block.stmts {
            last_ty = self.check_stmt(stmt)?;
        }

        Ok(last_ty)
    }

    /// Type check a statement
    fn check_stmt(&mut self, stmt: &Stmt) -> Result<ResolvedType> {
        match stmt {
            Stmt::Let { name, mutable, ty, value, span } => {
                let declared_ty = ty.as_ref()
                    .map(|t| self.resolve_type(t))
                    .transpose()?;

                let value_ty = value.as_ref()
                    .map(|e| self.check_expr(e))
                    .transpose()?;

                let final_ty = match (declared_ty, value_ty) {
                    (Some(d), Some(v)) => {
                        // Strict Type System: No implicit conversions allowed
                        if !self.types_compatible(&d, &v) {
                            return Err(Error::TypeMismatch {
                                expected: format!("{:?}", d),
                                got: format!("{:?}", v),
                                span: *span,
                            });
                        }
                        d
                    }
                    (Some(d), None) => d,
                    (None, Some(v)) => v,
                    (None, None) => ResolvedType::Unknown,
                };

                self.symbols.define(Symbol {
                    name: name.name.clone(),
                    kind: SymbolKind::Variable,
                    ty: final_ty,
                    span: *span,
                    mutable: *mutable,
                })?;

                self.ownership.add_owned(name.name.clone(), *span);

                Ok(ResolvedType::unit())
            }
            Stmt::Expr(expr) => self.check_expr(expr),
            Stmt::Return { value, .. } => {
                if let Some(expr) = value {
                    self.check_expr(expr)
                } else {
                    Ok(ResolvedType::unit())
                }
            }
            Stmt::Break { .. } | Stmt::Continue { .. } | Stmt::Empty { .. } => {
                Ok(ResolvedType::unit())
            }
        }
    }

    /// Type check an expression
    fn check_expr(&mut self, expr: &Expr) -> Result<ResolvedType> {
        match expr {
            Expr::Literal(lit) => Ok(self.literal_type(lit)),
            
            Expr::Ident(ident) => {
                if let Some(symbol) = self.symbols.lookup(&ident.name) {
                    // For functions, return the function type from SymbolKind
                    if let SymbolKind::Function { params, ret, .. } = &symbol.kind {
                        Ok(ResolvedType::Function {
                            params: params.clone(),
                            ret: Box::new(ret.clone()),
                        })
                    } else {
                        Ok(symbol.ty.clone())
                    }
                } else {
                    Err(Error::UndefinedVariable {
                        name: ident.name.clone(),
                        span: ident.span,
                    })
                }
            }


            Expr::Path { segments, span } => {
                // Phase 11: Basic path resolution for Enum constructors and Struct static methods
                if segments.len() >= 2 {
                    let type_name = &segments[0].name;
                    let symbol_name = &segments[1].name;
                    
                    // Check if this is an imported module symbol (e.g., helper::greet)
                    if let Some(module_symbols) = self.imported_modules.get(type_name) {
                        for (name, symbol) in module_symbols {
                            if name == symbol_name {
                                // Found the symbol in the imported module
                                // For functions, ensure we return a Function type
                                match &symbol.kind {
                                    SymbolKind::Function { params, ret, .. } => {
                                        return Ok(ResolvedType::Function {
                                            params: params.clone(),
                                            ret: Box::new(ret.clone()),
                                        });
                                    }
                                    _ => return Ok(symbol.ty.clone()),
                                }
                            }
                        }
                    }
                    
                    // Also check qualified name lookup (module::symbol registered in symbol table)
                    let qualified_name = format!("{}::{}", type_name, symbol_name);
                    if let Some(symbol) = self.symbols.lookup(&qualified_name) {
                        return Ok(symbol.ty.clone());
                    }
                    
                    if let Some(symbol) = self.symbols.lookup(type_name) {
                         // Enum variant (e.g., TokenKind::Eof)
                         if matches!(symbol.kind, SymbolKind::Enum { .. }) {
                             return Ok(ResolvedType::Unknown);
                         }
                         // Struct static method (e.g., String::new)
                         if matches!(symbol.kind, SymbolKind::Struct { .. }) {
                             return Ok(ResolvedType::Unknown);
                         }
                    }
                }
                
                Err(Error::UndefinedVariable {
                    name: segments.iter().map(|i| i.name.clone()).collect::<Vec<_>>().join("::"),
                    span: *span,
                })
            }


            Expr::Binary { left, op, right, span } => {
                let left_ty = self.check_expr(left)?;
                let right_ty = self.check_expr(right)?;
                self.check_binary_op(&left_ty, *op, &right_ty, *span)
            }
            
            Expr::Try { expr, .. } => {
                // Determine the error type (basic check: expr must be Result)
                let _ty = self.check_expr(expr)?;
                // TODO: Verify type is Result<T, E> and return T
                // For now, return the inner type blindly or assume ok
                Ok(_ty) 
            }

            Expr::Unary { op, expr, .. } => {
                let ty = self.check_expr(expr)?;
                self.check_unary_op(*op, &ty)
            }

            Expr::Call { func, args, span } => {
                let func_ty = self.check_expr(func)?;
                
                // Effect propagation: check if calling from pure context
                if let Some(ref effects) = self.current_effects {
                    if effects.is_pure {
                        // Pure functions cannot call impure functions
                        // Check if calling a known impure builtin
                        if let Expr::Ident(ident) = func.as_ref() {
                            let impure_builtins = ["print", "println", "print_i64", "println_i64", "exit", "alloc", "free"];
                            if impure_builtins.contains(&ident.name.as_str()) {
                                let err = Error::EffectViolation {
                                    message: format!("pure function cannot call impure builtin '{}'", ident.name),
                                    span: *span,
                                };
                                if self.strict_mode {
                                    return Err(err);
                                } else {
                                    self.errors.push(err);
                                }
                            }
                        }
                    }
                }
                
                match func_ty {
                    ResolvedType::Function { params, ret } => {
                        // For method calls (func is field access), skip the self parameter
                        let expected_args = if matches!(func.as_ref(), Expr::Field { .. }) && !params.is_empty() {
                            params.len() - 1  // Exclude self parameter
                        } else {
                            params.len()
                        };
                        
                        // Skip arg count check for variadic C functions
                        let is_variadic = matches!(func.as_ref(), Expr::Ident(ident) if ident.name == "printf");
                        
                        if !is_variadic && args.len() != expected_args {
                            return Err(Error::ArgCountMismatch {
                                expected: expected_args,
                                got: args.len(),
                                span: *span,
                            });
                        }
                        
                        // Infer generic type parameters from arguments
                        let mut type_substitutions: HashMap<String, ResolvedType> = HashMap::new();
                        for (arg, param_ty) in args.iter().zip(params.iter()) {
                            let arg_ty = self.check_expr(arg)?;
                            // If param is a generic type, bind it to the actual arg type
                            if let ResolvedType::GenericParam(name) = param_ty {
                                type_substitutions.insert(name.clone(), arg_ty);
                            }
                        }
                        
                        // Substitute generic params in return type
                        let actual_ret = self.substitute_type(&ret, &type_substitutions);
                        Ok(actual_ret)
                    }
                    ResolvedType::Unknown => {
                        // Allow calling unknown functions (e.g. enum constructors for now)
                        for arg in args {
                            let _ = self.check_expr(arg)?;
                        }
                        Ok(ResolvedType::Unknown)
                    }
                    _ => Err(Error::NotCallable { span: *span }),
                }
            }

            Expr::Field { expr, field, span } => {
                let expr_ty = self.check_expr(expr)?;
                
                let struct_ty = if let ResolvedType::Pointer(inner) = &expr_ty {
                    inner.as_ref()
                } else if let ResolvedType::Reference { inner, .. } = &expr_ty {
                    inner.as_ref()
                } else {
                    &expr_ty
                };

                match struct_ty {
                    ResolvedType::Struct { name: _, fields } => {
                        for (fname, fty) in fields {
                            if fname == &field.name {
                                return Ok(fty.clone());
                            }
                        }
                        Err(Error::UnknownField {
                            field: field.name.clone(),
                            span: *span,
                        })
                    }
                    _ => Err(Error::NotAStruct { span: *span }),
                }
            }

            Expr::If { cond, then_block, else_block, .. } => {
                let cond_ty = self.check_expr(cond)?;
                if cond_ty != ResolvedType::bool() && cond_ty != ResolvedType::Unknown {
                    self.errors.push(Error::TypeMismatch {
                        expected: "bool".to_string(),
                        got: format!("{:?}", cond_ty),
                        span: cond.span(),
                    });
                }

                self.symbols.enter_scope();
                let then_ty = self.check_block(then_block)?;
                self.symbols.exit_scope();

                if let Some(else_block) = else_block {
                    self.symbols.enter_scope();
                    let _else_ty = self.check_block(else_block)?;
                    self.symbols.exit_scope();
                    // TODO: Check then_ty == else_ty
                    Ok(then_ty)
                } else {
                    Ok(ResolvedType::unit())
                }
            }

            Expr::Block(block) => {
                self.symbols.enter_scope();
                let ty = self.check_block(block)?;
                self.symbols.exit_scope();
                Ok(ty)
            }

            Expr::Loop { body, .. } => {
                self.symbols.enter_scope();
                self.check_block(body)?;
                self.symbols.exit_scope();
                Ok(ResolvedType::never())
            }

            Expr::While { cond, body, .. } => {
                let cond_ty = self.check_expr(cond)?;
                if cond_ty != ResolvedType::bool() && cond_ty != ResolvedType::Unknown {
                    self.errors.push(Error::TypeMismatch {
                        expected: "bool".to_string(),
                        got: format!("{:?}", cond_ty),
                        span: cond.span(),
                    });
                }
                self.symbols.enter_scope();
                self.check_block(body)?;
                self.symbols.exit_scope();
                Ok(ResolvedType::unit())
            }

            Expr::For { var, iter, body, span } => {
                let _iter_ty = self.check_expr(iter)?;
                // TODO: Get element type from iterator

                self.symbols.enter_scope();
                self.symbols.define(Symbol {
                    name: var.name.clone(),
                    kind: SymbolKind::Variable,
                    ty: ResolvedType::Unknown, // Would be element type
                    span: *span,
                    mutable: false,
                })?;
                self.check_block(body)?;
                self.symbols.exit_scope();
                Ok(ResolvedType::unit())
            }

            Expr::Array { elements, .. } => {
                if elements.is_empty() {
                    return Ok(ResolvedType::Array {
                        elem: Box::new(ResolvedType::Unknown),
                        size: 0,
                    });
                }
                let first_ty = self.check_expr(&elements[0])?;
                for elem in elements.iter().skip(1) {
                    let _elem_ty = self.check_expr(elem)?;
                    // TODO: Check all elements have same type
                }
                Ok(ResolvedType::Array {
                    elem: Box::new(first_ty),
                    size: elements.len(),
                })
            }

            Expr::Tuple { elements, .. } => {
                let types: Vec<ResolvedType> = elements.iter()
                    .map(|e| self.check_expr(e))
                    .collect::<Result<Vec<_>>>()?;
                Ok(ResolvedType::Tuple(types))
            }

            Expr::Ref { mutable, expr, span } => {
                let inner_ty = self.check_expr(expr)?;
                
                // Check ownership for borrowing
                if let Expr::Ident(ident) = expr.as_ref() {
                    if *mutable {
                        self.ownership.borrow_mut(&ident.name, *span)?;
                    } else {
                        self.ownership.borrow(&ident.name, *span)?;
                    }
                }

                Ok(ResolvedType::Reference {
                    mutable: *mutable,
                    inner: Box::new(inner_ty),
                })
            }

            Expr::Deref { expr, span } => {
                let ty = self.check_expr(expr)?;
                match ty {
                    ResolvedType::Pointer(inner) => Ok(*inner),
                    ResolvedType::Reference { inner, .. } => Ok(*inner),
                    _ => Err(Error::CannotDeref { span: *span }),
                }
            }

            Expr::Index { expr, index, span } => {
                let expr_ty = self.check_expr(expr)?;
                let index_ty = self.check_expr(index)?;

                // Check index is integer
                if !matches!(index_ty, ResolvedType::Primitive(PrimitiveType::Usize) 
                    | ResolvedType::Primitive(PrimitiveType::I32)
                    | ResolvedType::Primitive(PrimitiveType::I64)) {
                    self.errors.push(Error::TypeMismatch {
                        expected: "integer".to_string(),
                        got: format!("{:?}", index_ty),
                        span: index.span(),
                    });
                }

                match expr_ty {
                    ResolvedType::Array { elem, .. } => Ok(*elem),
                    ResolvedType::Slice(elem) => Ok(*elem),
                    // Support pointer indexing: ptr[i] dereferences and offsets
                    ResolvedType::Pointer(elem) => Ok(*elem),
                    _ => Err(Error::NotIndexable { span: *span }),
                }
            }

            Expr::Match { expr, arms, .. } => {
                let _expr_ty = self.check_expr(expr)?;
                
                let mut result_ty = None;
                for arm in arms {
                    // TODO: Check pattern against expr_ty
                    let arm_ty = self.check_expr(&arm.body)?;
                    if result_ty.is_none() {
                        result_ty = Some(arm_ty);
                    }
                    // TODO: Check all arms have same type
                }
                Ok(result_ty.unwrap_or(ResolvedType::unit()))
            }

            Expr::Unsafe { body, .. } => {
                self.symbols.enter_scope();
                let ty = self.check_block(body)?;
                self.symbols.exit_scope();
                Ok(ty)
            }

            Expr::MethodCall { expr, method, args, span } => {
                let receiver_ty = self.check_expr(expr)?;
                
                match &receiver_ty {
                    ResolvedType::Pointer(inner) => {
                        if method.name == "add" {
                            if args.len() != 1 {
                                return Err(Error::ArgCountMismatch { expected: 1, got: args.len(), span: *span });
                            }
                            let offset_ty = self.check_expr(&args[0])?;
                            // Check offset is integer
                            match offset_ty {
                                ResolvedType::Primitive(p) if p.is_integer() => {},
                                _ => return Err(Error::TypeMismatch { 
                                    expected: "integer".to_string(), 
                                    got: format!("{:?}", offset_ty), 
                                    span: args[0].span() 
                                }),
                            }
                            // Returns same pointer type
                            Ok(ResolvedType::Pointer(inner.clone()))
                        } else {
                            Ok(ResolvedType::Unknown)
                        }
                    },
                    _ => Ok(ResolvedType::Unknown)
                }
            }
            
            Expr::StructLit { name, fields, span } => {
                // First, check struct exists and get field info
                let symbol = self.symbols.lookup(&name.name)
                    .ok_or(Error::UndefinedType { name: name.name.clone(), span: *span })
                    .cloned()?;

                if let SymbolKind::Struct { fields: def_fields, type_params, .. } = &symbol.kind {
                    let mut inferred_params = std::collections::HashMap::new();
                    
                    // Check each field
                    for (fname, fvalue) in fields {
                        let fvalue_ty = self.check_expr(fvalue)?;
                        
                        // Find definition
                        if let Some((_, def_ty)) = def_fields.iter().find(|(n, _)| n == &fname.name) {
                             // Unify def_ty and fvalue_ty
                             if let ResolvedType::GenericParam(p_name) = def_ty {
                                 inferred_params.insert(p_name.clone(), fvalue_ty.clone());
                             } else if let ResolvedType::Generic(g_name, g_args) = def_ty {
                                 if let ResolvedType::Generic(v_name, v_args) = &fvalue_ty {
                                     if g_name == v_name && g_args.len() == v_args.len() {
                                         for (g_arg, v_arg) in g_args.iter().zip(v_args.iter()) {
                                              if let ResolvedType::GenericParam(p_name) = g_arg {
                                                  inferred_params.insert(p_name.clone(), v_arg.clone());
                                              }
                                         }
                                     }
                                 }
                             }
                        } else {
                            return Err(Error::UnknownField { field: fname.name.clone(), span: *span });
                        }
                    }
                    
                    // Construct Result
                    if !type_params.is_empty() {
                        let mut args = Vec::new();
                        for param in type_params {
                             if let Some(ty) = inferred_params.get(param) {
                                 args.push(ty.clone());
                             } else {
                                  args.push(ResolvedType::Unknown);
                             }
                        }
                        return Ok(ResolvedType::Generic(name.name.clone(), args));
                    }

                    // Return struct type (Value, not Pointer)
                    Ok(ResolvedType::Struct {
                        name: name.name.clone(),
                        fields: def_fields.clone(),
                    })
                } else {
                    Err(Error::NotAStruct { span: *span })
                }
            }


            Expr::Cast { expr, ty, span } => {
                let source_ty = self.check_expr(expr)?;
                let target_ty = self.resolve_type(ty)?;
                
                // Allow explicit casts in the following cases:
                // 1. Integer to Integer (any size)
                // 2. Integer to Pointer (for raw memory access)
                // 3. Pointer to Integer (address extraction)
                // 4. Pointer to Pointer (reinterpret)
                // 5. Same type (no-op)
                
                let is_valid_cast = match (&source_ty, &target_ty) {
                    // Same type
                    (s, t) if s == t => true,
                    
                    // Int to Int
                    (ResolvedType::Primitive(p1), ResolvedType::Primitive(p2)) 
                        if p1.is_integer() && p2.is_integer() => true,
                    
                    // Int to Pointer
                    (ResolvedType::Primitive(p), ResolvedType::Pointer(_)) => {
                        p.is_integer()
                    },
                    
                    // Pointer to Int
                    (ResolvedType::Pointer(_), ResolvedType::Primitive(p))
                        if p.is_integer() => true,
                    
                    // Pointer to Pointer
                    (ResolvedType::Pointer(_), ResolvedType::Pointer(_)) => true,
                    
                    // Reference to Pointer (same inner type or compatible)
                    (ResolvedType::Reference { inner, .. }, ResolvedType::Pointer(ptr_inner)) => {
                        inner.as_ref() == ptr_inner.as_ref()
                    },
                    
                    // Unknown source (permissive for now)
                    (ResolvedType::Unknown, _) => true,
                    
                    _ => false,
                };
                
                if !is_valid_cast {
                    return Err(Error::TypeMismatch {
                        expected: format!("{:?}", target_ty),
                        got: format!("{:?}", source_ty),
                        span: *span,
                    });
                }
                
                Ok(target_ty)
            }
            Expr::Range { .. } => Ok(ResolvedType::Unknown),
            Expr::Asm { .. } => Ok(ResolvedType::unit()),
            
            Expr::Closure { params, ret_type, body, .. } => {
                // Enter a new scope for closure parameters
                self.symbols.enter_scope();
                
                // Add parameters to scope
                let mut param_types = Vec::new();
                for param in params {
                    let ty = if let Some(t) = &param.ty {
                        self.resolve_type(t)?
                    } else {
                        // Infer type from usage (for now, default to i64)
                        ResolvedType::Primitive(PrimitiveType::I64)
                    };
                    param_types.push(ty.clone());
                    self.symbols.define(Symbol {
                        name: param.name.name.clone(),
                        kind: SymbolKind::Variable,
                        ty,
                        span: param.name.span,
                        mutable: false,
                    })?;
                }
                
                // Check body and determine return type
                let body_ty = self.check_expr(body)?;
                
                let ret_ty = if let Some(t) = ret_type {
                    self.resolve_type(t)?
                } else {
                    body_ty
                };
                
                self.symbols.exit_scope();
                
                Ok(ResolvedType::Function {
                    params: param_types,
                    ret: Box::new(ret_ty),
                })
            }
        }
    }

    /// Get the type of a literal
    fn literal_type(&self, lit: &Literal) -> ResolvedType {
        match lit {
            Literal::Int(_, _) => ResolvedType::Primitive(PrimitiveType::I64), // Default to i64
            Literal::Float(_, _) => ResolvedType::Primitive(PrimitiveType::F32),
            Literal::String(_, _) => ResolvedType::Pointer(Box::new(ResolvedType::Primitive(PrimitiveType::U8))), // C-style string pointer
            Literal::Char(_, _) => ResolvedType::Primitive(PrimitiveType::Char),
            Literal::Bool(_, _) => ResolvedType::Primitive(PrimitiveType::Bool),
        }
    }


    /// Check binary operation and return result type
    fn check_binary_op(&self, left: &ResolvedType, op: BinOp, right: &ResolvedType, _span: Span) -> Result<ResolvedType> {
        match op {
            // Comparison operators return bool
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                Ok(ResolvedType::bool())
            }
            // Logical operators return bool
            BinOp::And | BinOp::Or => {
                Ok(ResolvedType::bool())
            }
            // Assignment returns unit
            BinOp::Assign | BinOp::AddAssign | BinOp::SubAssign | BinOp::MulAssign | BinOp::DivAssign => {
                Ok(ResolvedType::unit())
            }
            // Arithmetic and bitwise: handle F32/F64 mixed operations
            _ => {
                use crate::types::type_system::PrimitiveType;
                // For F32/F64 mixed operations, promote to F64
                if let (ResolvedType::Primitive(PrimitiveType::F32), ResolvedType::Primitive(PrimitiveType::F64)) = (left, right) {
                    Ok(ResolvedType::Primitive(PrimitiveType::F64))
                } else if let (ResolvedType::Primitive(PrimitiveType::F64), ResolvedType::Primitive(PrimitiveType::F32)) = (left, right) {
                    Ok(ResolvedType::Primitive(PrimitiveType::F64))
                } else {
                    Ok(left.clone())
                }
            }
        }
    }

    /// Check unary operation
    fn check_unary_op(&self, op: UnOp, ty: &ResolvedType) -> Result<ResolvedType> {
        match op {
            UnOp::Neg => Ok(ty.clone()),
            UnOp::Not => Ok(ResolvedType::bool()),
            UnOp::BitNot => Ok(ty.clone()),
        }
    }
    
    /// Substitute generic type parameters with actual types
    fn substitute_type(&self, ty: &ResolvedType, substitutions: &HashMap<String, ResolvedType>) -> ResolvedType {
        match ty {
            ResolvedType::GenericParam(name) => {
                substitutions.get(name).cloned().unwrap_or_else(|| ty.clone())
            }
            ResolvedType::Pointer(inner) => {
                ResolvedType::Pointer(Box::new(self.substitute_type(inner, substitutions)))
            }
            ResolvedType::Reference { mutable, inner } => {
                ResolvedType::Reference {
                    mutable: *mutable,
                    inner: Box::new(self.substitute_type(inner, substitutions)),
                }
            }
            ResolvedType::Array { elem, size } => {
                ResolvedType::Array {
                    elem: Box::new(self.substitute_type(elem, substitutions)),
                    size: *size,
                }
            }
            ResolvedType::Slice(inner) => {
                ResolvedType::Slice(Box::new(self.substitute_type(inner, substitutions)))
            }
            ResolvedType::Tuple(types) => {
                ResolvedType::Tuple(types.iter().map(|t| self.substitute_type(t, substitutions)).collect())
            }
            ResolvedType::Function { params, ret } => {
                ResolvedType::Function {
                    params: params.iter().map(|p| self.substitute_type(p, substitutions)).collect(),
                    ret: Box::new(self.substitute_type(ret, substitutions)),
                }
            }
            // Other types pass through unchanged
            _ => ty.clone(),
        }
    }

    /// Resolve an AST type to a ResolvedType
    fn resolve_type(&self, ty: &Type) -> Result<ResolvedType> {
        match ty {
            Type::Named(name, _) => {
                match name.as_str() {
                    "i8" => Ok(ResolvedType::Primitive(PrimitiveType::I8)),
                    "i16" => Ok(ResolvedType::Primitive(PrimitiveType::I16)),
                    "i32" => Ok(ResolvedType::Primitive(PrimitiveType::I32)),
                    "i64" => Ok(ResolvedType::Primitive(PrimitiveType::I64)),
                    "isize" => Ok(ResolvedType::Primitive(PrimitiveType::Isize)),
                    "u8" => Ok(ResolvedType::Primitive(PrimitiveType::U8)),
                    "u16" => Ok(ResolvedType::Primitive(PrimitiveType::U16)),
                    "u32" => Ok(ResolvedType::Primitive(PrimitiveType::U32)),
                    "u64" => Ok(ResolvedType::Primitive(PrimitiveType::U64)),
                    "usize" => Ok(ResolvedType::Primitive(PrimitiveType::Usize)),
                    "f32" => Ok(ResolvedType::Primitive(PrimitiveType::F32)),
                    "f64" => Ok(ResolvedType::Primitive(PrimitiveType::F64)),
                    "bool" => Ok(ResolvedType::Primitive(PrimitiveType::Bool)),
                    "char" => Ok(ResolvedType::Primitive(PrimitiveType::Char)),
                    // SIMD vector types
                    "f32x4" => Ok(ResolvedType::Vector(Box::new(ResolvedType::Primitive(PrimitiveType::F32)), 4)),
                    "f32x8" => Ok(ResolvedType::Vector(Box::new(ResolvedType::Primitive(PrimitiveType::F32)), 8)),
                    "f64x2" => Ok(ResolvedType::Vector(Box::new(ResolvedType::Primitive(PrimitiveType::F64)), 2)),
                    "f64x4" => Ok(ResolvedType::Vector(Box::new(ResolvedType::Primitive(PrimitiveType::F64)), 4)),
                    "i32x4" => Ok(ResolvedType::Vector(Box::new(ResolvedType::Primitive(PrimitiveType::I32)), 4)),
                    "i32x8" => Ok(ResolvedType::Vector(Box::new(ResolvedType::Primitive(PrimitiveType::I32)), 8)),
                    "i64x2" => Ok(ResolvedType::Vector(Box::new(ResolvedType::Primitive(PrimitiveType::I64)), 2)),
                    "i64x4" => Ok(ResolvedType::Vector(Box::new(ResolvedType::Primitive(PrimitiveType::I64)), 4)),
                    _ => {
                        // Look up in symbol table
                        if let Some(sym) = self.symbols.lookup(name) {
                            // Check if it's a type parameter
                            if matches!(sym.kind, SymbolKind::TypeParam) {
                                Ok(ResolvedType::GenericParam(name.clone()))
                            } else if let SymbolKind::TypeAlias { target } = &sym.kind {
                                // Expand type alias
                                Ok(target.clone())
                            } else {
                                Ok(sym.ty.clone())
                            }
                        } else {
                            // Check if it's a single uppercase letter (common type param convention)
                            if name.len() == 1 && name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                                Ok(ResolvedType::GenericParam(name.clone()))
                            } else {
                                Ok(ResolvedType::Struct { 
                                    name: name.clone(), 
                                    fields: Vec::new() 
                                })
                            }
                        }
                    }
                }
            }
            Type::Pointer(inner, _) => {
                Ok(ResolvedType::Pointer(Box::new(self.resolve_type(inner)?)))
            }
            Type::Ref { mutable, inner, .. } => {
                Ok(ResolvedType::Reference {
                    mutable: *mutable,
                    inner: Box::new(self.resolve_type(inner)?),
                })
            }
            Type::Array { elem, size, .. } => {
                Ok(ResolvedType::Array {
                    elem: Box::new(self.resolve_type(elem)?),
                    size: *size,
                })
            }
            Type::Slice(elem, _) => {
                Ok(ResolvedType::Slice(Box::new(self.resolve_type(elem)?)))
            }
            Type::Tuple(types, _) => {
                let resolved: Vec<ResolvedType> = types.iter()
                    .map(|t| self.resolve_type(t))
                    .collect::<Result<Vec<_>>>()?;
                Ok(ResolvedType::Tuple(resolved))
            }
            Type::Generic(name, args, _) => {
                let resolved_args: Vec<ResolvedType> = args.iter()
                    .map(|arg| self.resolve_type(arg))
                    .collect::<Result<Vec<_>>>()?;
                Ok(ResolvedType::Generic(name.clone(), resolved_args))
            }
            Type::GenericWithArgs { name, args, .. } => {
                // Separate type args and const args
                let mut type_args = Vec::new();
                let mut const_args = Vec::new();

                for arg in args {
                    match arg {
                        crate::frontend::ast::GenericArg::Type(ty) => {
                            type_args.push(self.resolve_type(ty)?);
                        }
                        crate::frontend::ast::GenericArg::Const(expr) => {
                            const_args.push(self.eval_const_expr(expr)?);
                        }
                    }
                }

                // If no const args, use regular Generic
                if const_args.is_empty() {
                    Ok(ResolvedType::Generic(name.clone(), type_args))
                } else {
                    Ok(ResolvedType::GenericWithConsts {
                        name: name.clone(),
                        type_args,
                        const_args,
                    })
                }
            }
            Type::Function { params, ret, .. } => {
                let param_types: Vec<ResolvedType> = params.iter()
                    .map(|t| self.resolve_type(t))
                    .collect::<Result<Vec<_>>>()?;
                Ok(ResolvedType::Function {
                    params: param_types,
                    ret: Box::new(self.resolve_type(ret)?),
                })
            }
            Type::Unit(_) => Ok(ResolvedType::unit()),
            Type::Never(_) => Ok(ResolvedType::never()),
            Type::Infer(_) => Ok(ResolvedType::Unknown),
            // For now, just resolve the inner type (ownership is handled separately)
            Type::Owned { inner, .. } => self.resolve_type(inner),
            // Volatile type (Phase 8) - resolve inner type, volatile semantics handled at IR level
            Type::Volatile(inner, _) => {
                Ok(ResolvedType::Pointer(Box::new(self.resolve_type(inner)?)))
            }
        }
    }

    /// Evaluate a const expression to a ConstValue
    fn eval_const_expr(&self, expr: &Expr) -> Result<ConstValue> {
        match expr {
            Expr::Literal(lit) => {
                match lit {
                    Literal::Int(n, _) => Ok(ConstValue::Int(*n)),
                    Literal::Bool(b, _) => Ok(ConstValue::Bool(*b)),
                    _ => Err(Error::TypeMismatch {
                        expected: "integer or boolean constant".to_string(),
                        got: format!("{:?}", lit),
                        span: lit.span(),
                    }),
                }
            }
            Expr::Ident(ident) => {
                // Check if it's a const parameter
                if let Some(sym) = self.symbols.lookup(&ident.name) {
                    if let SymbolKind::ConstParam { .. } = &sym.kind {
                        Ok(ConstValue::Param(ident.name.clone()))
                    } else {
                        Err(Error::TypeMismatch {
                            expected: "const parameter".to_string(),
                            got: format!("{:?}", sym.kind),
                            span: ident.span,
                        })
                    }
                } else {
                    // Assume it's a const parameter reference
                    Ok(ConstValue::Param(ident.name.clone()))
                }
            }
            Expr::Binary { op, left, right, .. } => {
                let lhs = self.eval_const_expr(left)?;
                let rhs = self.eval_const_expr(right)?;
                let const_op = match op {
                    BinOp::Add => ConstBinOp::Add,
                    BinOp::Sub => ConstBinOp::Sub,
                    BinOp::Mul => ConstBinOp::Mul,
                    BinOp::Div => ConstBinOp::Div,
                    BinOp::Mod => ConstBinOp::Mod,
                    _ => return Err(Error::TypeMismatch {
                        expected: "arithmetic operator".to_string(),
                        got: format!("{:?}", op),
                        span: expr.span(),
                    }),
                };
                Ok(ConstValue::BinOp {
                    op: const_op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                })
            }
            _ => Err(Error::TypeMismatch {
                expected: "const expression".to_string(),
                got: format!("{:?}", expr),
                span: expr.span(),
            }),
        }
    }

    /// Check if two types are compatible (Strict Type System: no implicit conversions)
    fn types_compatible(&self, expected: &ResolvedType, got: &ResolvedType) -> bool {
        // Unknown types are always compatible (inference pending)
        if matches!(expected, ResolvedType::Unknown) || matches!(got, ResolvedType::Unknown) {
            return true;
        }
        
        // Strict equality - no implicit conversions between numeric types
        match (expected, got) {
            (ResolvedType::Primitive(a), ResolvedType::Primitive(b)) => {
                if a == b {
                    true
                } else {
                    // Allow implicit conversion between integer types for bootstrapping
                    use crate::types::type_system::PrimitiveType::*;
                    matches!((a, b), 
                        (I8, U8) | (U8, I8) |
                        (I16, U16) | (U16, I16) |
                        (I32, U32) | (U32, I32) |
                        (I64, U64) | (U64, I64) |
                        // Integer literal (I64) can be assigned to any integer type
                        (I8, I64) | (U8, I64) | (I16, I64) | (U16, I64) |
                        (I32, I64) | (U32, I64) | (U64, I64) |
                        // Allow implicit conversion between F32 and F64 for stdlib compatibility
                        (F32, F64) | (F64, F32)
                    )
                }
            }
            (ResolvedType::Pointer(a), ResolvedType::Pointer(b)) => self.types_compatible(a, b),
            (ResolvedType::Reference { mutable: ma, inner: ia, .. }, 
             ResolvedType::Reference { mutable: mb, inner: ib, .. }) => {
                // Mutable reference can be used where immutable is expected
                (*ma || !*mb) && self.types_compatible(ia, ib)
            }
            (ResolvedType::Array { elem: ea, size: sa, .. },
             ResolvedType::Array { elem: eb, size: sb, .. }) => {
                sa == sb && self.types_compatible(ea, eb)
            }
            (ResolvedType::Tuple(a), ResolvedType::Tuple(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| self.types_compatible(x, y))
            }
            (ResolvedType::Struct { name: na, .. }, ResolvedType::Struct { name: nb, .. }) => na == nb,
            (a, b) => a == b,
        }
    }
}

impl Default for SemanticAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::lexer::Lexer;
    use crate::frontend::parser::Parser;

    fn analyze(source: &str) -> Result<()> {
        let lexer = Lexer::new(source, 0);
        let mut parser = Parser::new(lexer);
        let program = parser.parse_program()?;
        let mut analyzer = SemanticAnalyzer::new();
        analyzer.analyze(&program)
    }

    #[test]
    fn test_simple_function() {
        let result = analyze("fn main() {}");
        assert!(result.is_ok());
    }

    #[test]
    fn test_variable_definition() {
        let result = analyze("fn main() { let x = 42 }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_undefined_variable() {
        let result = analyze("fn main() { return y }");
        assert!(result.is_err());
    }
}
