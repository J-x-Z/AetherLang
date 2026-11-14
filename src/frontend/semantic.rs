//! Semantic Analysis for AetherLang
//!
//! Performs:
//! - Symbol table management (scopes, definitions)
//! - Type checking
//! - Type checking
//! - Ownership analysis (own/ref/mut)
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use crate::frontend::ast::*;
use crate::frontend::module::ModuleLoader;
use crate::types::*;
use crate::types::type_system::ConstBinOp;
use crate::utils::{Span, Error, Result};

#[path = "semantic_constants.rs"]
mod constants;
pub use constants::{ScalarConstant, SemanticFacts, TypedConstant};

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
    Function { params: Vec<ResolvedType>, ret: ResolvedType, type_params: Vec<String>, const_params: Vec<(String, ResolvedType)>, effects: EffectSet },
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
            if existing.span == Span::dummy()
               && matches!(existing.kind, SymbolKind::Function { .. })
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

            // Also extract associated functions from impl blocks
            if let Item::Impl(impl_block) = item {
                let type_name = impl_block.target.name.clone();

                for method in &impl_block.methods {
                    if method.is_pub {
                        let params: Vec<ResolvedType> = method.params.iter()
                            .map(|p| self.ast_type_to_resolved(&p.ty))
                            .collect();
                        let ret = method.ret_type.as_ref()
                            .map(|t| self.ast_type_to_resolved(t))
                            .unwrap_or(ResolvedType::unit());

                        // Register as Type::method (e.g., IRGenerator::new)
                        let qualified_name = format!("{}::{}", type_name, method.name.name);
                        symbols.push(Symbol {
                            name: qualified_name,
                            kind: SymbolKind::Function {
                                params: params.clone(),
                                ret: ret.clone(),
                                type_params: vec![],
                                const_params: vec![],
                                effects: EffectSet::default()
                            },
                            ty: ResolvedType::Function { params, ret: Box::new(ret) },
                            span,
                            mutable: false,
                        });
                    }
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
                    kind: SymbolKind::Function { params: params.clone(), ret: ret.clone(), type_params: vec![], const_params: vec![], effects: EffectSet::default() },
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
    current_return: Option<ResolvedType>,
    loop_depth: usize,
    loop_breaks: Vec<bool>,
    variadic_functions: HashSet<String>,
    declared_types: HashMap<String, ResolvedType>,
    type_aliases: HashMap<String, Type>,
    constant_defs: HashMap<String, ConstDef>,
    facts: SemanticFacts,
    foreign_signatures: HashMap<String, String>,
    check_entrypoint: bool,
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
            current_return: None,
            loop_depth: 0,
            loop_breaks: Vec::new(),
            variadic_functions: HashSet::new(),
            declared_types: HashMap::new(),
            type_aliases: HashMap::new(),
            constant_defs: HashMap::new(),
            facts: SemanticFacts::default(),
            foreign_signatures: HashMap::new(),
            check_entrypoint: true,
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
        // Create effect sets for builtins
        let io_effects = EffectSet { is_pure: false, effects: vec![Effect::IO] };
        let alloc_effects = EffectSet { is_pure: false, effects: vec![Effect::Alloc] };
        let panic_effects = EffectSet { is_pure: false, effects: vec![Effect::Panic] };

        // I/O functions - require effect[io]
        self.define_builtin_with_effects("print", vec![ResolvedType::Pointer(Box::new(ResolvedType::U8))], ResolvedType::unit(), io_effects.clone());
        self.define_builtin_with_effects("println", vec![ResolvedType::Pointer(Box::new(ResolvedType::U8))], ResolvedType::unit(), io_effects.clone());
        self.define_builtin_with_effects("puts", vec![ResolvedType::Pointer(Box::new(ResolvedType::U8))], ResolvedType::I32, io_effects.clone());
        self.define_builtin_with_effects("print_i64", vec![ResolvedType::I64], ResolvedType::unit(), io_effects.clone());
        self.define_builtin_with_effects("println_i64", vec![ResolvedType::I64], ResolvedType::unit(), io_effects.clone());

        // Memory functions - require effect[alloc]
        self.define_builtin_with_effects("alloc", vec![ResolvedType::U64],
            ResolvedType::Pointer(Box::new(ResolvedType::U8)), alloc_effects.clone());
        self.define_builtin_with_effects("malloc", vec![ResolvedType::U64],
            ResolvedType::Pointer(Box::new(ResolvedType::U8)), alloc_effects.clone());
        self.define_builtin_with_effects("free",
            vec![ResolvedType::Pointer(Box::new(ResolvedType::U8))],
            ResolvedType::unit(), alloc_effects.clone());

        // C library functions for self-hosting (pure - no side effects)
        self.define_builtin("atof", vec![ResolvedType::Pointer(Box::new(ResolvedType::U8))],
            ResolvedType::F64);
        self.define_builtin("strcmp", vec![
            ResolvedType::Pointer(Box::new(ResolvedType::U8)),
            ResolvedType::Pointer(Box::new(ResolvedType::U8)),
        ], ResolvedType::I32);

        // Process control - require effect[panic] (never returns)
        self.define_builtin_with_effects("exit", vec![ResolvedType::I32], ResolvedType::never(), panic_effects.clone());

        // Debug - require effect[panic]
        self.define_builtin_with_effects("assert", vec![ResolvedType::BOOL], ResolvedType::UNIT, panic_effects.clone());
        
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
        self.define_builtin_with_effects(name, params, ret, EffectSet::default());
    }

    /// Define a built-in function with specific effects
    fn define_builtin_with_effects(&mut self, name: &str, params: Vec<ResolvedType>, ret: ResolvedType, effects: EffectSet) {
        let symbol = Symbol {
            name: name.to_string(),
            kind: SymbolKind::Function { params, ret, type_params: vec![], const_params: vec![], effects },
            ty: ResolvedType::Unknown,
            span: Span::dummy(),
            mutable: false,
        };
        let _ = self.symbols.define(symbol);
    }

    /// Analyze a program
    pub fn analyze(&mut self, program: &Program) -> Result<()> {
        self.check_entrypoint = !program.inner_attrs.iter().any(|attribute| attribute.name.name == "no_main");
        // Nominal names are known before resolving any signatures. This supports
        // forward and pointer-recursive structs without accepting invented types.
        for item in &program.items {
            let declaration = match item {
                Item::Struct(def) => Some((&def.name, ResolvedType::Struct { name: def.name.name.clone(), fields: vec![] })),
                Item::Enum(def) => Some((&def.name, ResolvedType::Enum { name: def.name.name.clone() })),
                Item::Union(def) => Some((&def.name, ResolvedType::Struct { name: def.name.name.clone(), fields: vec![] })),
                Item::Const(definition) => { self.constant_defs.insert(definition.name.name.clone(), definition.clone()); None }
                Item::TypeAlias(alias) => {
                    if !alias.type_params.is_empty() { return Err(Error::Expected("non-generic type alias; generic aliases are not implemented".into(), alias.span)); }
                    if self.type_aliases.insert(alias.name.name.clone(), alias.ty.clone()).is_some() {
                        return Err(Error::DuplicateDefinition { name: alias.name.name.clone(), span: alias.span });
                    }
                    None
                }
                _ => None,
            };
            if let Some((name, ty)) = declaration {
                if self.declared_types.insert(name.name.clone(), ty).is_some() {
                    return Err(Error::DuplicateDefinition { name: name.name.clone(), span: name.span });
                }
            }
        }
        for name in self.type_aliases.keys() {
            self.validate_alias(name, &mut HashSet::new())?;
        }
        self.validate_struct_layouts(program)?;
        // Pass 1: Collect all top-level definitions
        for item in &program.items {
            self.collect_definition(item)?;
        }

        self.prepare_constant_facts()?;

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

    fn register_foreign(&mut self, name: &str, signature: String) -> Result<bool> {
        if let Some(previous) = self.foreign_signatures.get(name) {
            if previous != &signature { return Err(Error::ModuleError(format!("conflicting foreign declarations for '{}'", name))); }
            return Ok(false);
        }
        self.foreign_signatures.insert(name.into(), signature);
        Ok(true)
    }

    fn type_identity(ty: &ResolvedType) -> String {
        match ty {
            ResolvedType::Struct { name, .. } => format!("struct {}", name),
            ResolvedType::Enum { name } => format!("enum {}", name),
            ResolvedType::Pointer(inner) => format!("*{}", Self::type_identity(inner)),
            ResolvedType::Reference { mutable, inner } => format!("&{}{}", mutable, Self::type_identity(inner)),
            ResolvedType::Array { elem, size } => format!("[{};{}]", Self::type_identity(elem), size),
            ResolvedType::Slice(inner) => format!("[{}]", Self::type_identity(inner)),
            ResolvedType::Tuple(types) => format!("({})", types.iter().map(Self::type_identity).collect::<Vec<_>>().join(",")),
            ResolvedType::Function { params, ret } => format!("fn({})->{}", params.iter().map(Self::type_identity).collect::<Vec<_>>().join(","), Self::type_identity(ret)),
            _ => format!("{:?}", ty),
        }
    }

    fn define_type_parameters(&mut self, params: &[Ident]) -> Result<()> {
        for param in params {
            self.symbols.define(Symbol { name: param.name.clone(), kind: SymbolKind::TypeParam,
                ty: ResolvedType::GenericParam(param.name.clone()), span: param.span, mutable: false })?;
        }
        Ok(())
    }

    fn type_names(ty: &Type, behind_pointer: bool, names: &mut Vec<String>) {
        match ty {
            Type::Named(name, _) => names.push(name.clone()),
            Type::Pointer(inner, _) | Type::Slice(inner, _) | Type::Ref { inner, .. } => {
                if behind_pointer { Self::type_names(inner, true, names); }
            }
            Type::Array { elem, .. } | Type::Owned { inner: elem, .. } | Type::Volatile(elem, _) => Self::type_names(elem, behind_pointer, names),
            Type::Tuple(elements, _) => for elem in elements { Self::type_names(elem, behind_pointer, names); },
            Type::Generic(name, args, _) => {
                names.push(name.clone());
                if behind_pointer { for arg in args { Self::type_names(arg, true, names); } }
            }
            Type::GenericWithArgs { name, args, .. } => {
                names.push(name.clone());
                if behind_pointer { for arg in args { if let GenericArg::Type(ty) = arg { Self::type_names(ty, true, names); } } }
            }
            Type::Function { params, ret, .. } if behind_pointer => {
                for param in params { Self::type_names(param, true, names); }
                Self::type_names(ret, true, names);
            }
            _ => {}
        }
    }

    fn validate_alias(&self, name: &str, visiting: &mut HashSet<String>) -> Result<()> {
        let Some(ty) = self.type_aliases.get(name) else { return Ok(()); };
        if !visiting.insert(name.into()) { return Err(Error::Expected(format!("non-recursive type alias '{}'", name), ty.span())); }
        let mut dependencies = Vec::new();
        Self::type_names(ty, true, &mut dependencies);
        for dependency in dependencies { self.validate_alias(&dependency, visiting)?; }
        visiting.remove(name);
        Ok(())
    }

    fn validate_struct_layouts(&self, program: &Program) -> Result<()> {
        let mut graph: HashMap<String, (Vec<String>, Span)> = HashMap::new();
        fn dependencies(analyzer: &SemanticAnalyzer, ty: &Type, result: &mut Vec<String>) {
            if let Type::Named(name, _) = ty {
                if let Some(alias) = analyzer.type_aliases.get(name) { dependencies(analyzer, alias, result); return; }
            }
            let mut names = Vec::new();
            SemanticAnalyzer::type_names(ty, false, &mut names);
            for name in names {
                if let Some(alias) = analyzer.type_aliases.get(&name) { dependencies(analyzer, alias, result); }
                else { result.push(name); }
            }
        }
        for item in &program.items {
            let (name, fields, span) = match item {
                Item::Struct(def) => (&def.name.name, &def.fields, def.span),
                Item::Union(def) => (&def.name.name, &def.fields, def.span),
                _ => continue,
            };
            let mut deps = Vec::new();
            for field in fields { dependencies(self, &field.ty, &mut deps); }
            graph.insert(name.clone(), (deps, span));
        }
        fn visit(name: &str, graph: &HashMap<String, (Vec<String>, Span)>, visiting: &mut HashSet<String>, done: &mut HashSet<String>) -> Result<()> {
            if done.contains(name) { return Ok(()); }
            let Some((deps, span)) = graph.get(name) else { return Ok(()); };
            if !visiting.insert(name.into()) { return Err(Error::Expected(format!("finite layout for '{}'; use a pointer for recursive fields", name), *span)); }
            for dependency in deps { visit(dependency, graph, visiting, done)?; }
            visiting.remove(name); done.insert(name.into()); Ok(())
        }
        let mut done = HashSet::new();
        for name in graph.keys() { visit(name, &graph, &mut HashSet::new(), &mut done)?; }
        Ok(())
    }

    /// Collect a top-level definition
    fn collect_definition(&mut self, item: &Item) -> Result<()> {
        match item {
            Item::Function(func) => {
                self.symbols.enter_scope();
                self.define_type_parameters(&func.type_params)?;
                let params: Vec<ResolvedType> = func.params.iter()
                    .map(|p| self.resolve_type(&p.ty))
                    .collect::<Result<Vec<_>>>()?;
                let ret = func.ret_type.as_ref()
                    .map(|t| self.resolve_type(t))
                    .transpose()?
                    .unwrap_or(ResolvedType::unit());
                self.symbols.exit_scope();

                self.symbols.define(Symbol {
                    name: func.name.name.clone(),
                    kind: SymbolKind::Function { params: params.clone(), ret: ret.clone(), type_params: func.type_params.iter().map(|p| p.name.clone()).collect(), const_params: vec![], effects: func.effects.clone() },
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
                for foreign_item in &ext.items {
                    match foreign_item {
                        ForeignItem::Fn { name, params, ret_type, variadic, .. } => {
                            let param_types = params.iter().map(|param| self.resolve_type(&param.ty)).collect::<Result<Vec<_>>>()?;
                            let ret = ret_type.as_ref().map(|ty| self.resolve_type(ty)).transpose()?.unwrap_or(ResolvedType::UNIT);
                            let signature = format!("{}:fn({})->{}:{}", ext.abi.as_deref().unwrap_or("C"),
                                param_types.iter().map(Self::type_identity).collect::<Vec<_>>().join(","), Self::type_identity(&ret), variadic);
                            if !self.register_foreign(&name.name, signature)? { continue; }
                            if *variadic { self.variadic_functions.insert(name.name.clone()); }
                            self.symbols.define(Symbol {
                                name: name.name.clone(),
                                kind: SymbolKind::Function { params: param_types.clone(), ret: ret.clone(), type_params: vec![], const_params: vec![], effects: EffectSet::default() },
                                ty: ResolvedType::Function { params: param_types, ret: Box::new(ret) },
                                span: name.span, mutable: false,
                            })?;
                        }
                        ForeignItem::Static { name, ty, is_mut, .. } => {
                            let resolved = self.resolve_type(ty)?;
                            let signature = format!("static:{}:{}", Self::type_identity(&resolved), is_mut);
                            if !self.register_foreign(&name.name, signature)? { continue; }
                            self.symbols.define(Symbol { name: name.name.clone(), kind: SymbolKind::Variable,
                                ty: resolved, span: name.span, mutable: *is_mut })?;
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

                    // Register symbols for both qualified and simple name lookup
                    for (name, symbol) in &symbols {
                        // Register simple name (e.g., "Span") for direct use
                        let _ = self.symbols.define(symbol.clone());

                        // Also register qualified name (e.g., "span::Span")
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
                        effects: EffectSet::default(),
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
                if let Some(ty) = &c.ty {
                    let expected = self.resolve_type(ty)?;
                    self.check_expected_expr(&c.value, &expected)?;
                } else { self.check_expr(&c.value)?; }
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
        self.define_type_parameters(&func.type_params)?;
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
        
        if self.check_entrypoint && func.name.name == "main" {
            if func.ret_type.is_some() && return_type != ResolvedType::I32 {
                return Err(Error::Expected("main returning i32 (omit the return annotation for automatic status 0)".into(), func.span));
            }
            let params = func.params.iter().map(|param| self.resolve_type(&param.ty)).collect::<Result<Vec<_>>>()?;
            let argv = ResolvedType::Pointer(Box::new(ResolvedType::Pointer(Box::new(ResolvedType::U8))));
            if !params.is_empty() && params != vec![ResolvedType::I32, argv] {
                return Err(Error::Expected("main() or main(argc: i32, argv: **u8)".into(), func.span));
            }
        }

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

        // Return checks share the same contextual typing as arguments and assignments.
        self.current_return = Some(return_type.clone());
        self.loop_depth = 0;
        self.check_function_body(&func.body, &return_type)?;
        self.current_return = None;
        
        // Clear effect context
        self.current_effects = None;

        self.symbols.exit_scope();
        Ok(())
    }

    /// Type check an impl block
    fn check_impl(&mut self, impl_block: &ImplBlock) -> Result<()> {
        let target = self.resolve_type(&Type::Named(impl_block.target.name.clone(), impl_block.target.span))?;
        self.symbols.enter_scope();
        self.symbols.define(Symbol { name: "Self".into(), kind: SymbolKind::TypeAlias { target: target.clone() },
            ty: target, span: impl_block.span, mutable: false })?;
        for method in &impl_block.methods { self.check_function(method)?; }
        self.symbols.exit_scope();
        Ok(())
    }

    /// Type check a block
    fn check_block(&mut self, block: &Block) -> Result<ResolvedType> {
        let mut last_ty = ResolvedType::unit();

        for stmt in &block.stmts {
            let ty = self.check_stmt(stmt)?;
            // A trailing semicolon or unreachable statement cannot make a return fall through.
            if last_ty != ResolvedType::NEVER && !matches!(stmt, Stmt::Empty { .. }) {
                last_ty = ty;
            }
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

                let final_ty = match (declared_ty, value) {
                    (Some(expected), Some(value)) => {
                        self.check_expected_expr(value, &expected)?;
                        expected
                    }
                    (Some(expected), None) => expected,
                    (None, Some(value)) => self.check_expr(value)?,
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
            Stmt::Semi(expr) => {
                let ty = self.check_expr(expr)?;
                Ok(if ty == ResolvedType::NEVER { ty } else { ResolvedType::UNIT })
            }
            Stmt::Return { value, span } => {
                let expected = self.current_return.clone().ok_or_else(||
                    Error::Expected("return inside a function".into(), *span))?;
                if let Some(value) = value {
                    if expected == ResolvedType::Unknown {
                        self.current_return = Some(self.check_expr(value)?);
                    } else { self.check_expected_expr(value, &expected)?; }
                } else {
                    self.require_type(&expected, &ResolvedType::UNIT, *span)?;
                    if expected == ResolvedType::Unknown { self.current_return = Some(ResolvedType::UNIT); }
                }
                Ok(ResolvedType::NEVER)
            }
            Stmt::Break { span } | Stmt::Continue { span } => {
                if self.loop_depth == 0 {
                    return Err(Error::Expected("break/continue inside a loop".into(), *span));
                }
                if matches!(stmt, Stmt::Break { .. }) {
                    if let Some(has_break) = self.loop_breaks.last_mut() { *has_break = true; }
                }
                Ok(ResolvedType::NEVER)
            }
            Stmt::Empty { .. } => Ok(ResolvedType::UNIT),
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
                if matches!(op, BinOp::Assign | BinOp::AddAssign | BinOp::SubAssign | BinOp::MulAssign | BinOp::DivAssign) {
                    self.check_assignable(left)?;
                    let right_ty = if matches!(left_ty, ResolvedType::Pointer(_)) && matches!(op, BinOp::AddAssign | BinOp::SubAssign) {
                        self.check_expr(right)?
                    } else { self.check_expected_expr(right, &left_ty)? };
                    if *op != BinOp::Assign {
                        let arithmetic = match op {
                            BinOp::AddAssign => BinOp::Add,
                            BinOp::SubAssign => BinOp::Sub,
                            BinOp::MulAssign => BinOp::Mul,
                            _ => BinOp::Div,
                        };
                        self.check_binary_op(&left_ty, arithmetic, &right_ty, *span)?;
                    }
                    return Ok(ResolvedType::UNIT);
                }
                let right_ty = self.check_expr(right)?;
                let (left_ty, right_ty) = self.common_operand_types(left, left_ty, right, right_ty);
                self.check_binary_op(&left_ty, *op, &right_ty, *span)
            }

            Expr::Try { expr, .. } => {
                // Determine the error type (basic check: expr must be Result)
                let _ty = self.check_expr(expr)?;
                // TODO: Verify type is Result<T, E> and return T
                // For now, return the inner type blindly or assume ok
                Ok(_ty) 
            }

            Expr::Unary { op, expr, span } => {
                let ty = self.check_expr(expr)?;
                self.check_unary_op(*op, &ty, *span)
            }

            Expr::Call { func, args, span } => {
                let func_ty = self.check_expr(func)?;

                // P5.2: Effect propagation - HARD ERROR if caller doesn't declare required effects
                if let Some(ref caller_effects) = self.current_effects {
                    // Get callee's effects from symbol table
                    if let Expr::Ident(ident) = func.as_ref() {
                        if let Some(symbol) = self.symbols.lookup(&ident.name) {
                            if let SymbolKind::Function { effects: callee_effects, .. } = &symbol.kind {
                                // Check each effect the callee requires
                                for effect in &callee_effects.effects {
                                    // If caller is pure, it cannot call any effectful function
                                    if caller_effects.is_pure {
                                        return Err(Error::EffectViolation {
                                            message: format!(
                                                "pure function cannot call '{}' which has effect {:?}",
                                                ident.name, effect
                                            ),
                                            span: *span,
                                        });
                                    }
                                    // If caller doesn't have this effect, error
                                    if !caller_effects.effects.contains(effect) {
                                        return Err(Error::EffectViolation {
                                            message: format!(
                                                "function with effect[{:?}] called from function without that effect. Add 'effect[{:?}]' to caller's signature.",
                                                effect, effect
                                            ),
                                            span: *span,
                                        });
                                    }
                                }
                                // If callee is pure but caller is not - that's fine (pure is subset of any effect)
                            }
                        } else {
                            // Check built-in impure functions
                            let io_builtins = ["print", "println", "print_i64", "println_i64", "puts", "printf", "exit"];
                            let alloc_builtins = ["malloc", "free", "realloc", "alloc"];

                            if caller_effects.is_pure {
                                if io_builtins.contains(&ident.name.as_str()) || alloc_builtins.contains(&ident.name.as_str()) {
                                    return Err(Error::EffectViolation {
                                        message: format!("pure function cannot call impure builtin '{}'", ident.name),
                                        span: *span,
                                    });
                                }
                            } else {
                                // Check specific effects for builtins
                                if io_builtins.contains(&ident.name.as_str()) && !caller_effects.effects.contains(&Effect::IO) {
                                    return Err(Error::EffectViolation {
                                        message: format!("calling '{}' requires effect[IO]. Add 'effect[io]' to function signature.", ident.name),
                                        span: *span,
                                    });
                                }
                                if alloc_builtins.contains(&ident.name.as_str()) && !caller_effects.effects.contains(&Effect::Alloc) {
                                    return Err(Error::EffectViolation {
                                        message: format!("calling '{}' requires effect[Alloc]. Add 'effect[alloc]' to function signature.", ident.name),
                                        span: *span,
                                    });
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
                        let is_variadic = matches!(func.as_ref(), Expr::Ident(ident) if self.variadic_functions.contains(&ident.name));
                        
                        if args.len() < expected_args || (!is_variadic && args.len() != expected_args) {
                            // Debug: print function name for better error messages
                            let func_name = match func.as_ref() {
                                Expr::Ident(ident) => ident.name.clone(),
                                Expr::Path { segments, .. } => segments.iter().map(|s| s.name.clone()).collect::<Vec<_>>().join("::"),
                                _ => "unknown".to_string(),
                            };
                            return Err(Error::ArgCountMismatch {
                                func_name,
                                expected: expected_args,
                                got: args.len(),
                                span: *span,
                            });
                        }
                        
                        let mut type_substitutions: HashMap<String, ResolvedType> = HashMap::new();
                        for (index, arg) in args.iter().enumerate() {
                            if let Some(param_ty) = params.get(index) {
                                let expected = self.substitute_type(param_ty, &type_substitutions);
                                if let ResolvedType::GenericParam(name) = &expected {
                                    let arg_ty = self.check_expr(arg)?;
                                    type_substitutions.insert(name.clone(), arg_ty);
                                } else {
                                    self.check_expected_expr(arg, &expected)?;
                                }
                            } else {
                                // Variadic arguments still need semantic checking.
                                self.check_expr(arg)?;
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
                    ResolvedType::Struct { name, fields } => {
                        let fields = match self.symbols.lookup(name) {
                            Some(Symbol { kind: SymbolKind::Struct { fields, .. }, .. }) => fields,
                            _ => fields,
                        };
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
                    let else_ty = self.check_block(else_block)?;
                    self.symbols.exit_scope();
                    if then_ty == ResolvedType::NEVER { return Ok(else_ty); }
                    self.require_type(&then_ty, &else_ty, else_block.span)?;
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
                let has_break = self.check_loop_body(body)?;
                Ok(if has_break { ResolvedType::UNIT } else { ResolvedType::NEVER })
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
                self.check_loop_body(body)?;
                Ok(ResolvedType::unit())
            }

            Expr::For { var, iter, body, span } => {
                let Expr::Range { start: Some(start), end: Some(end), .. } = iter.as_ref() else {
                    return Err(Error::Expected("bounded integer range (start..end)".into(), iter.span()));
                };
                let start_ty = self.check_expr(start)?;
                let end_ty = self.check_expr(end)?;
                let (start_ty, end_ty) = self.common_operand_types(start, start_ty, end, end_ty);
                if !Self::is_integer(&start_ty) {
                    return Err(Error::TypeMismatch { expected: "integer range".into(), got: format!("{:?}", start_ty), span: iter.span() });
                }
                self.require_type(&start_ty, &end_ty, iter.span())?;
                self.symbols.enter_scope();
                self.symbols.define(Symbol {
                    name: var.name.clone(), kind: SymbolKind::Variable, ty: start_ty,
                    span: *span, mutable: false,
                })?;
                self.check_loop_body(body)?;
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
                    self.check_expected_expr(elem, &first_ty)?;
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
                
                if *mutable { self.check_assignable(expr)?; }
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

                if !Self::is_integer(&index_ty) {
                    return Err(Error::TypeMismatch {
                        expected: "integer".to_string(), got: format!("{:?}", index_ty), span: index.span(),
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

            Expr::Match { expr, arms, span } => self.check_match(expr, arms, *span, None),

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
                                return Err(Error::ArgCountMismatch { func_name: "ptr.add".to_string(), expected: 1, got: args.len(), span: *span });
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
                    
                    let mut seen = HashSet::new();
                    for (fname, _) in fields {
                        if !seen.insert(fname.name.clone()) {
                            return Err(Error::DuplicateDefinition { name: fname.name.clone(), span: fname.span });
                        }
                    }
                    for (field, _) in def_fields {
                        if !seen.contains(field) {
                            return Err(Error::Expected(format!("initializer for field {}", field), *span));
                        }
                    }
                    for (fname, fvalue) in fields {
                        // Check each initializer exactly once: checking a borrow mutates ownership state.
                        // Find definition
                        if let Some((_, def_ty)) = def_fields.iter().find(|(n, _)| n == &fname.name) {
                             // Unify def_ty and fvalue_ty
                             if let ResolvedType::GenericParam(p_name) = def_ty {
                                 inferred_params.insert(p_name.clone(), self.check_expr(fvalue)?);
                             } else if let ResolvedType::Generic(g_name, g_args) = def_ty {
                                 let fvalue_ty = self.check_expr(fvalue)?;
                                 if let ResolvedType::Generic(v_name, v_args) = &fvalue_ty {
                                     if g_name == v_name && g_args.len() == v_args.len() {
                                         for (g_arg, v_arg) in g_args.iter().zip(v_args.iter()) {
                                              if let ResolvedType::GenericParam(p_name) = g_arg {
                                                  inferred_params.insert(p_name.clone(), v_arg.clone());
                                              }
                                         }
                                     }
                                 }
                             } else {
                                 self.check_expected_expr(fvalue, def_ty)?;
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
                        if (p1.is_integer() || p1.is_float()) && (p2.is_integer() || p2.is_float()) => true,
                    
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
                
                // Closures form their own return and loop-control boundary.
                let declared = ret_type.as_ref().map(|ty| self.resolve_type(ty)).transpose()?;
                let outer_return = self.current_return.replace(declared.clone().unwrap_or(ResolvedType::Unknown));
                let outer_loop_depth = std::mem::replace(&mut self.loop_depth, 0);
                let outer_loop_breaks = std::mem::take(&mut self.loop_breaks);
                let body_result = if let Some(expected) = &declared {
                    self.check_expected_expr(body, expected)
                } else { self.check_expr(body) };
                let inferred_return = self.current_return.take().unwrap_or(ResolvedType::Unknown);
                self.current_return = outer_return;
                self.loop_depth = outer_loop_depth;
                self.loop_breaks = outer_loop_breaks;
                let body_ty = body_result?;
                let ret_ty = declared.unwrap_or_else(|| {
                    if body_ty == ResolvedType::NEVER && inferred_return != ResolvedType::Unknown { inferred_return }
                    else { body_ty }
                });

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


    fn is_integer(ty: &ResolvedType) -> bool {
        matches!(ty, ResolvedType::Primitive(p) if p.is_integer())
    }

    fn is_numeric(ty: &ResolvedType) -> bool {
        matches!(ty, ResolvedType::Primitive(p) if p.is_integer() || p.is_float())
            || matches!(ty, ResolvedType::Vector(_, _))
    }

    fn require_type(&self, expected: &ResolvedType, got: &ResolvedType, span: Span) -> Result<()> {
        if self.types_compatible(expected, got) { return Ok(()); }
        Err(Error::TypeMismatch { expected: format!("{:?}", expected), got: format!("{:?}", got), span })
    }

    /// Unsuffixed literals may adopt a context type; typed values never silently narrow.
    fn integer_constant(expr: &Expr) -> Option<i128> {
        match expr {
            Expr::Literal(Literal::Int(value, _)) => Some(*value as i128),
            Expr::Unary { op: UnOp::Neg, expr, .. } => Self::integer_constant(expr)?.checked_neg(),
            Expr::Binary { left, op, right, .. } => {
                let a = Self::integer_constant(left)?;
                let b = Self::integer_constant(right)?;
                match op {
                    BinOp::Add => a.checked_add(b), BinOp::Sub => a.checked_sub(b),
                    BinOp::Mul => a.checked_mul(b), BinOp::Div => a.checked_div(b),
                    BinOp::Mod => a.checked_rem(b), _ => None,
                }
            }
            _ => None,
        }
    }

    fn literal_fits(expr: &Expr, expected: &ResolvedType) -> bool {
        if let Some(value) = Self::integer_constant(expr) {
            if let ResolvedType::Primitive(primitive) = expected {
                if primitive.is_integer() {
                    let bits = primitive.size_of() * 8;
                    return if primitive.is_signed() {
                        value >= -(1i128 << (bits - 1)) && value < (1i128 << (bits - 1))
                    } else {
                        value >= 0 && value < (1i128 << bits)
                    };
                }
            }
        }
        fn float_constant(expr: &Expr) -> bool {
            match expr {
                Expr::Literal(Literal::Float(_, _)) => true,
                Expr::Unary { op: UnOp::Neg, expr, .. } => float_constant(expr),
                Expr::Binary { left, op: BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div, right, .. } => float_constant(left) && float_constant(right),
                _ => false,
            }
        }
        float_constant(expr) && matches!(expected, ResolvedType::Primitive(PrimitiveType::F32 | PrimitiveType::F64))
    }

    fn common_operand_types(&self, left: &Expr, left_ty: ResolvedType, right: &Expr, right_ty: ResolvedType) -> (ResolvedType, ResolvedType) {
        if left_ty != right_ty {
            if Self::literal_fits(right, &left_ty) { return (left_ty.clone(), left_ty); }
            if Self::literal_fits(left, &right_ty) { return (right_ty.clone(), right_ty); }
        }
        (left_ty, right_ty)
    }

    fn check_expected_expr(&mut self, expr: &Expr, expected: &ResolvedType) -> Result<ResolvedType> {
        // Propagate context into aggregate elements and branch values.
        match (expr, expected) {
            (Expr::Match { expr: value, arms, span }, _) => {
                return self.check_match(value, arms, *span, Some(expected));
            }
            (Expr::Array { elements, span }, ResolvedType::Array { elem, size }) => {
                if elements.len() != *size {
                    return Err(Error::TypeMismatch { expected: format!("array of length {}", size), got: format!("array of length {}", elements.len()), span: *span });
                }
                for value in elements { self.check_expected_expr(value, elem)?; }
                return Ok(expected.clone());
            }
            (Expr::Tuple { elements, .. }, ResolvedType::Tuple(types)) if elements.len() == types.len() => {
                for (value, ty) in elements.iter().zip(types) { self.check_expected_expr(value, ty)?; }
                return Ok(expected.clone());
            }
            (Expr::If { cond, then_block, else_block: Some(else_block), .. }, _) => {
                self.check_expected_expr(cond, &ResolvedType::BOOL)?;
                for block in [then_block, else_block] {
                    self.symbols.enter_scope();
                    let result = self.check_value_block(block, expected);
                    self.symbols.exit_scope();
                    result?;
                }
                return Ok(expected.clone());
            }
            (Expr::Block(block), _) | (Expr::Unsafe { body: block, .. }, _) => {
                self.symbols.enter_scope();
                let result = self.check_value_block(block, expected);
                self.symbols.exit_scope();
                result?;
                return Ok(expected.clone());
            }
            _ => {}
        }
        let got = self.check_expr(expr)?;
        if Self::literal_fits(expr, expected) { return Ok(expected.clone()); }
        self.require_type(expected, &got, expr.span())?;
        Ok(got)
    }

    fn check_match(&mut self, expr: &Expr, arms: &[MatchArm], span: Span, expected: Option<&ResolvedType>) -> Result<ResolvedType> {
        let scrutinee = self.check_expr(expr)?;
        let mut catch_all = false;
        let mut true_case = false;
        let mut false_case = false;
        let mut result = ResolvedType::NEVER;
        let mut first_value: Option<&Expr> = None;
        for arm in arms {
            self.symbols.enter_scope();
            let arm_result = (|| {
                match &arm.pattern {
                    Pattern::Wildcard { .. } => { if arm.guard.is_none() { catch_all = true; } }
                    Pattern::Binding { name, mutable, span } => {
                        self.symbols.define(Symbol { name: name.name.clone(), kind: SymbolKind::Variable,
                            ty: scrutinee.clone(), span: *span, mutable: *mutable })?;
                        if arm.guard.is_none() { catch_all = true; }
                    }
                    Pattern::Literal(literal) => {
                        if matches!(literal, Literal::String(_, _) | Literal::Float(_, _)) {
                            return Err(Error::Expected("integer, bool, or char match pattern".into(), literal.span()));
                        }
                        self.check_expected_expr(&Expr::Literal(literal.clone()), &scrutinee)?;
                        if arm.guard.is_none() {
                            if let Literal::Bool(value, _) = literal { if *value { true_case = true; } else { false_case = true; } }
                        }
                    }
                    _ => return Err(Error::Expected("scalar literal, binding, or wildcard pattern; aggregate/enum patterns are not implemented".into(), arm.span)),
                }
                if let Some(guard) = &arm.guard { self.check_expected_expr(guard, &ResolvedType::BOOL)?; }
                if let Some(expected) = expected { self.check_expected_expr(&arm.body, expected) }
                else { self.check_expr(&arm.body) }
            })();
            self.symbols.exit_scope();
            let arm_ty = arm_result?;
            if arm_ty == ResolvedType::NEVER { continue; }
            if result == ResolvedType::NEVER {
                result = arm_ty;
                first_value = Some(&arm.body);
            } else {
                let (left, right) = self.common_operand_types(first_value.unwrap(), result, &arm.body, arm_ty);
                self.require_type(&left, &right, arm.span)?;
                result = left;
            }
        }
        if !catch_all && !(scrutinee == ResolvedType::BOOL && true_case && false_case) {
            return Err(Error::Expected("exhaustive match: add an unguarded wildcard or binding arm".into(), span));
        }
        Ok(result)
    }

    fn check_value_block(&mut self, body: &Block, expected: &ResolvedType) -> Result<()> {
        if *expected == ResolvedType::UNIT {
            let got = self.check_block(body)?;
            self.require_type(expected, &got, body.span)
        } else { self.check_function_body(body, expected) }
    }

    fn check_function_body(&mut self, body: &Block, expected: &ResolvedType) -> Result<()> {
        let last = body.stmts.iter().rposition(|stmt| !matches!(stmt, Stmt::Empty { .. }));
        let mut diverges = false;
        let mut last_ty = ResolvedType::UNIT;
        for (index, stmt) in body.stmts.iter().enumerate() {
            let ty = if !diverges && Some(index) == last && *expected != ResolvedType::UNIT {
                if let Stmt::Expr(expr) = stmt { self.check_expected_expr(expr, expected)? }
                else { self.check_stmt(stmt)? }
            } else { self.check_stmt(stmt)? };
            if ty == ResolvedType::NEVER { diverges = true; }
            if Some(index) == last { last_ty = ty; }
        }
        if !diverges && *expected != ResolvedType::UNIT {
            self.require_type(expected, &last_ty, body.span)?;
        }
        Ok(())
    }

    fn check_loop_body(&mut self, body: &Block) -> Result<bool> {
        self.symbols.enter_scope();
        self.loop_depth += 1;
        self.loop_breaks.push(false);
        let result = self.check_block(body);
        let has_break = self.loop_breaks.pop().unwrap_or(false);
        self.loop_depth -= 1;
        self.symbols.exit_scope();
        result.map(|_| has_break)
    }

    fn check_assignable(&mut self, expr: &Expr) -> Result<()> {
        match expr {
            Expr::Ident(ident) => {
                if self.symbols.lookup(&ident.name).is_some_and(|symbol| symbol.mutable) { return Ok(()); }
                Err(Error::Expected(format!("mutable binding for '{}'", ident.name), ident.span))
            }
            Expr::Deref { expr: pointer, span } => match self.check_expr(pointer)? {
                ResolvedType::Pointer(_) | ResolvedType::Reference { mutable: true, .. } => Ok(()),
                _ => Err(Error::Expected("mutable reference or raw pointer".into(), *span)),
            },
            Expr::Field { expr: base, .. } | Expr::Index { expr: base, .. } => {
                match self.check_expr(base)? {
                    ResolvedType::Pointer(_) | ResolvedType::Reference { mutable: true, .. } => Ok(()),
                    ResolvedType::Reference { mutable: false, .. } => Err(Error::Expected("mutable reference".into(), expr.span())),
                    _ => self.check_assignable(base),
                }
            }
            _ => Err(Error::Expected("assignable place".into(), expr.span())),
        }
    }

    fn check_binary_op(&self, left: &ResolvedType, op: BinOp, right: &ResolvedType, span: Span) -> Result<ResolvedType> {
        // Raw pointer arithmetic advances by pointee elements, like ptr.add().
        if matches!(left, ResolvedType::Pointer(_)) && Self::is_integer(right)
            && matches!(op, BinOp::Add | BinOp::Sub) {
            return Ok(left.clone());
        }
        let compatible = self.types_compatible(left, right);
        let valid = match op {
            BinOp::And | BinOp::Or => *left == ResolvedType::BOOL && *right == ResolvedType::BOOL,
            BinOp::Eq | BinOp::Ne => compatible && (Self::is_numeric(left) || matches!(left, ResolvedType::Pointer(_) | ResolvedType::Enum { .. }) || *left == ResolvedType::BOOL),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => compatible && Self::is_numeric(left),
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr | BinOp::Mod => compatible && Self::is_integer(left),
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => compatible && Self::is_numeric(left),
            _ => false,
        };
        if !valid {
            return Err(Error::TypeMismatch { expected: format!("valid operands for {:?}", op), got: format!("{:?} and {:?}", left, right), span });
        }
        if matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::And | BinOp::Or) {
            Ok(ResolvedType::BOOL)
        } else { Ok(left.clone()) }
    }

    fn check_unary_op(&self, op: UnOp, ty: &ResolvedType, span: Span) -> Result<ResolvedType> {
        let valid = match op {
            UnOp::Neg => Self::is_numeric(ty),
            UnOp::Not => *ty == ResolvedType::BOOL,
            UnOp::BitNot => Self::is_integer(ty),
        };
        if !valid { return Err(Error::TypeMismatch { expected: format!("valid operand for {:?}", op), got: format!("{:?}", ty), span }); }
        Ok(ty.clone())
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
            Type::Named(name, span) => {
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
                    "void" => Ok(ResolvedType::UNIT),
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
                        if let Some(symbol) = self.symbols.lookup(name) {
                            match &symbol.kind {
                                SymbolKind::TypeParam => return Ok(ResolvedType::GenericParam(name.clone())),
                                SymbolKind::TypeAlias { target } => return Ok(target.clone()),
                                SymbolKind::Struct { .. } | SymbolKind::Enum { .. } => return Ok(symbol.ty.clone()),
                                _ => {}
                            }
                        }
                        if let Some(alias) = self.type_aliases.get(name) { return self.resolve_type(alias); }
                        self.declared_types.get(name).cloned().ok_or_else(|| Error::UndefinedType { name: name.clone(), span: *span })
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
            Type::Generic(name, args, span) => {
                self.resolve_type(&Type::Named(name.clone(), *span))?;
                let resolved_args: Vec<ResolvedType> = args.iter()
                    .map(|arg| self.resolve_type(arg))
                    .collect::<Result<Vec<_>>>()?;
                Ok(ResolvedType::Generic(name.clone(), resolved_args))
            }
            Type::GenericWithArgs { name, args, span } => {
                self.resolve_type(&Type::Named(name.clone(), *span))?;
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
            Type::Infer(span) => Err(Error::Expected("concrete type annotation; placeholder '_' inference is not implemented".into(), *span)),
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
        if *got == ResolvedType::NEVER { return true; }
        // Unknown types remain limited to unresolved generic/method paths.
        if matches!(expected, ResolvedType::Unknown) || matches!(got, ResolvedType::Unknown) {
            return true;
        }
        
        // Strict equality - no implicit conversions between numeric types
        match (expected, got) {
            (ResolvedType::Primitive(a), ResolvedType::Primitive(b)) => a == b,
            (ResolvedType::Pointer(a), ResolvedType::Pointer(b)) => self.types_compatible(a, b),
            (ResolvedType::Reference { mutable: ma, inner: ia, .. }, 
             ResolvedType::Reference { mutable: mb, inner: ib, .. }) => {
                // Mutable reference can be used where immutable is expected
                (!*ma || *mb) && self.types_compatible(ia, ib)
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
        // P5.1: AetherLang requires explicit type annotations
        let result = analyze("fn main() { let x: i32 = 42 }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_undefined_variable() {
        let result = analyze("fn main() { return y }");
        assert!(result.is_err());
    }
    fn accepts(source: &str) {
        assert!(analyze(source).is_ok(), "unexpected rejection: {:?}\n{}", analyze(source), source);
    }

    fn rejects(source: &str) {
        assert!(analyze(source).is_err(), "unexpected acceptance: {}", source);
    }

    #[test]
    fn calls_check_parameter_types_and_variadic_prefix() {
        accepts("fn id(x: i32) -> i32 { x } fn main() { id(12); }");
        rejects("fn id(x: i32) -> i32 { x } fn main() { id(true); }");
        accepts("extern \"C\" { fn log(fmt: *u8, ...) -> i32; } fn main() { log(\"x\", 1, true); }");
        rejects("extern \"C\" { fn log(fmt: *u8, ...) -> i32; } fn main() { log(); }");
        rejects("extern \"C\" { fn log(fmt: *u8, ...) -> i32; } fn main() { log(true); }");
        rejects("extern \"C\" { fn printf(x: i32); } fn main() { printf(1, 2); }");
        rejects("extern \"C\" { fn log(fmt: *u8, ...) -> i32; } fn main() { log(\"x\", missing); }");
    }

    #[test]
    fn return_values_and_fallthrough_follow_function_signature() {
        accepts("fn f() -> i32 { return 7; }");
        accepts("fn f() -> i32 { 1 + 2 }");
        accepts("fn f(b: bool) -> i32 { if b { return 1; } else { return 2; } }");
        accepts("fn f(b: bool) -> i32 { if b { 1 } else { 2 } }");
        accepts("fn f() -> i32 { loop {} }");
        rejects("fn f() -> i32 { return true; }");
        rejects("fn f() -> i32 { return; }");
        rejects("fn f() { return 1; }");
        rejects("fn f() { return { 1 }; }");
        rejects("fn f() { return if true { 1 } else { 2 }; }");
        rejects("fn f() -> i32 { 1; }");
        rejects("fn f() -> i32 { let x: i32 = 1; }");
        rejects("fn f(b: bool) -> i32 { if b { return 1; } }");
        rejects("fn f() -> i32 { loop { break; } }");
    }

    #[test]
    fn integer_literals_adopt_context_without_narrowing_variables() {
        accepts("fn f() { let x: u8 = 255; let y: i8 = -128; let z: usize = 3; }");
        accepts("fn f() { let x: i32 = 1 + 2; let y: i32 = x + 4; let z: i32 = 4 + x; }");
        rejects("fn f() { let x: u8 = 256; }");
        rejects("fn f() { let x: u8 = -1; }");
        rejects("fn f() { let x: i8 = -129; }");
        rejects("fn f() { let x: i64 = 1; let y: i32 = x; }");
        rejects("fn f() { let x: i32 = 1; let y: u32 = x; }");
        accepts("fn f() { let x: i64 = 1; let y: i32 = x as i32; }");
    }

    #[test]
    fn operators_require_valid_compatible_operands() {
        rejects("fn f() { let x: bool = true + false; }");
        rejects("fn f() { let x: bool = 1 && 2; }");
        rejects("fn f() { let x: bool = 1 == true; }");
        rejects("fn f() { let x: bool = !1; }");
        rejects("fn f() { let x: bool = -true; }");
        accepts("fn f() { let x: bool = !false && true; let y: i32 = 1; let z: bool = y < 2; }");
        accepts("fn f() { let x: f64 = -1.5; let y: f64 = x + 2.5; let z: f64 = 1.5 + 2.5; }");
        rejects("fn f() { let x: f32 = 1.5; let y: f64 = x; }");
        accepts("fn f() { let x: f32 = 1.5; let y: f64 = x as f64; }");
    }

    #[test]
    fn assignments_require_mutable_places_and_matching_values() {
        accepts("fn f() { let mut x: i32 = 1; x = 2; x += 3; }");
        rejects("fn f() { let x: i32 = 1; x = 2; }");
        rejects("fn f() { let mut x: i32 = 1; x = true; }");
        rejects("fn f() { 1 = 2; }");
        rejects("fn f() { let mut x: bool = true; x += false; }");
        accepts("fn f(p: *i32) { *p = 2; let q: *i32 = p + 1; let mut r: *i32 = q; r -= 1; }");
        rejects("fn f(p: &i32) { *p = 2; }");
        accepts("fn f(p: &mut i32) { *p = 2; }");
    }

    #[test]
    fn references_preserve_mutability_direction() {
        accepts("fn f(p: &mut i32) { let q: &i32 = p; }");
        rejects("fn f(p: &i32) { let q: &mut i32 = p; }");
        rejects("fn f() { let x: i32 = 1; let p: &mut i32 = &mut x; }");
        accepts("fn f() { let mut x: i32 = 1; let p: &mut i32 = &mut x; }");
    }

    #[test]
    fn arrays_and_structs_check_each_element_or_field() {
        accepts("fn f() { let mut a: [i32; 2] = [1, 2]; let i: u64 = 1; a[i] = 3; }");
        rejects("fn f() { let a: [i32; 2] = [1, true]; }");
        rejects("fn f() { let a: [i32; 2] = [1]; }");
        rejects("fn f() { let a: [i32; 2] = [1, 2]; a[true]; }");
        rejects("struct S { x: i32 } fn f() { let s: S = S { x: true }; }");
        rejects("struct S { x: i32, y: i32 } fn f() { let s: S = S { x: 1 }; }");
        rejects("struct S { x: i32 } fn f() { let s: S = S { x: 1, x: 2 }; }");
        accepts("struct S { x: i32 } fn f() { let s: S = S { x: 1 }; }");
        accepts("struct S { p: &mut i32 } fn f() { let mut x: i32 = 1; let s: S = S { p: &mut x }; }");
    }

    #[test]
    fn loop_control_and_ranges_are_checked() {
        rejects("fn f() { break; }");
        rejects("fn f() { continue; }");
        accepts("fn f() { loop { break; } while false { continue; } for i in 0..2 { let x: i64 = i; } }");
        rejects("fn f() { for i in false..true {} }");
        accepts("fn f(n: u64) { for i in 0..n { let x: u64 = i; } }");
    }

    #[test]
    fn declarations_do_not_silently_replace_functions_or_types() {
        rejects("fn f() {} fn f() {}");
        rejects("const X: bool = 1;");
        accepts("extern \"C\" { fn puts(s: *u8) -> i32; } fn main() { puts(\"ok\"); }");
    }

    #[test]
    fn scalar_matches_check_guards_bindings_and_exhaustiveness() {
        accepts("fn f(x: i32) -> i32 { match x { 0 => 1, n if n > 0 => n, _ => 2 } }");
        accepts("fn f(x: bool) -> i32 { match x { true => 1, false => 0 } }");
        rejects("fn f(x: i32) -> i32 { match x { 0 => 1 } }");
        rejects("fn f(x: bool) -> i32 { match x { true if false => 1, false => 0 } }");
        rejects("fn f(x: i32) -> i32 { match x { _ if 1 => 2, _ => 3 } }");
        rejects("fn f(x: i32) -> i32 { match x { true => 1, _ => 0 } }");
        rejects("fn f(x: i32) -> i32 { match x { 0 => true, _ => 1 } }");
        rejects("fn f(x: i32) { match x { n => n }; let y: i32 = n; }");
    }

    #[test]
    fn named_types_are_declared_and_forward_pointer_recursion_is_valid() {
        rejects("fn f(x: Missing) {}");
        rejects("fn f(x: T) {}");
        rejects("fn f() { let x: i32 = 1; let y: x = 2; }");
        rejects("struct S { field: Missing }");
        accepts("fn f(x: *Node) -> i32 { x.next.value } struct Node { next: *Node, value: i32 }");
        accepts("struct A { b: *B } struct B { a: *A } fn f(a: *A) -> *A { a.b.a }");
        accepts("fn id<Value>(value: Value) -> Value { value } fn f() -> i32 { id(7 as i32) }");
        rejects("struct A { a: A }");
        rejects("struct A { b: B } struct B { a: A }");
        rejects("type A = B; type B = A;");
        accepts("type Count = i64; fn f(x: Count) -> Count { x }");
    }

    #[test]
    fn unresolved_types_and_conflicting_canonical_foreign_declarations_fail() {
        rejects("fn f(x: _) {}");
        rejects("fn f() { let x: _ = 1; }");
        rejects("fn f() -> _ { 1 }");
        accepts("type Count = i64; extern \"C\" { fn count(x: Count) -> Count; } extern \"C\" { fn count(y: i64) -> i64; }");
        rejects("extern \"C\" { fn count(x: i32); } extern \"C\" { fn count(x: i64); }");
    }

    #[test]
    fn typed_constants_share_canonical_facts_and_reject_invalid_evaluation() {
        let source = "type Count = i32; const A: Count = 20 + 2; const B: Count = A * 2; const F: f64 = 1.5 + 0.25; fn f() -> Count { B }";
        let program = Parser::new(Lexer::new(source, 0)).parse_program().unwrap();
        let mut analyzer = SemanticAnalyzer::new();
        analyzer.analyze(&program).unwrap();
        assert_eq!(analyzer.facts().aliases["Count"], ResolvedType::I32);
        assert!(matches!(analyzer.facts().constants["B"].value, ScalarConstant::Int(44)));
        assert_eq!(analyzer.facts().constants["B"].ty, ResolvedType::I32);
        assert!(matches!(analyzer.facts().constants["F"].value, ScalarConstant::Float(value) if value == 1.75));
        accepts("const A: i32 = B + 1; const B: i32 = 4; fn f() -> i32 { A }");
        rejects("const A: i32 = B; const B: i32 = A;");
        rejects("const A: i32 = 1 / 0;");
        rejects("const A: i8 = 127 + 1;");
        rejects("const A: i64 = 9223372036854775807 + 1;");
        rejects("const A: i64 = (-9223372036854775807 - 1) % -1;");
        rejects("fn f() -> i32 { 1 } const A: i32 = f();");
        accepts("const A: u8 = 257 as u8;");
    }

    #[test]
    fn native_main_has_a_defined_process_abi() {
        accepts("fn main() -> i32 { 0 }");
        accepts("fn main(argc: i32, argv: **u8) -> i32 { 0 }");
        rejects("fn main() -> f64 { 0.0 }");
        rejects("fn main() -> void {}");
        rejects("fn main(argc: i64) -> i32 { 0 }");
        accepts("#![no_main] fn main() -> f64 { 0.0 }");
    }

}
