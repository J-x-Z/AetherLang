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
use crate::types::*;
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
    Function { params: Vec<ResolvedType>, ret: ResolvedType },
    Struct { fields: Vec<(String, ResolvedType)>, type_params: Vec<String> },
    Enum { variants: Vec<String> },
    Param { ownership: Ownership },
    TypeParam,
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
        if scope.symbols.contains_key(&symbol.name) {
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
}

impl SemanticAnalyzer {
    pub fn new() -> Self {
        let mut analyzer = Self {
            symbols: SymbolTable::new(),
            errors: Vec::new(),
            ownership: OwnershipState::new(),
            current_effects: None,
            strict_mode: false, // Default: lenient mode
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
        self.define_builtin("free", 
            vec![ResolvedType::Pointer(Box::new(ResolvedType::U8))], 
            ResolvedType::unit());
        
        // Process control
        self.define_builtin("exit", vec![ResolvedType::I32], ResolvedType::never());
        
        // Debug
        self.define_builtin("assert", vec![ResolvedType::BOOL], ResolvedType::UNIT);
    }
    
    /// Define a built-in function
    fn define_builtin(&mut self, name: &str, params: Vec<ResolvedType>, ret: ResolvedType) {
        let symbol = Symbol {
            name: name.to_string(),
            kind: SymbolKind::Function { params, ret },
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
                    kind: SymbolKind::Function { params: params.clone(), ret: ret.clone() },
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
                for param in &s.type_params {
                     self.symbols.define(Symbol {
                         name: param.name.clone(),
                         kind: SymbolKind::TypeParam,
                         ty: ResolvedType::GenericParam(param.name.clone()),
                         span: param.span,
                         mutable: false,
                     })?;
                }
                
                let fields: Vec<(String, ResolvedType)> = s.fields.iter()
                    .map(|f| Ok((f.name.name.clone(), self.resolve_type(&f.ty)?)))
                    .collect::<Result<Vec<_>>>()?;
                
                self.symbols.exit_scope();

                self.symbols.define(Symbol {
                    name: s.name.name.clone(),
                    kind: SymbolKind::Struct { 
                        fields: fields.clone(),
                        type_params: s.type_params.iter().map(|p| p.name.clone()).collect(),
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
                for param in &e.type_params {
                     self.symbols.define(Symbol {
                         name: param.name.clone(),
                         kind: SymbolKind::TypeParam,
                         ty: ResolvedType::GenericParam(param.name.clone()),
                         span: param.span,
                         mutable: false,
                     })?;
                }
                
                let variants: Vec<String> = e.variants.iter()
                    .map(|v| v.name.name.clone())
                    .collect();
                
                self.symbols.exit_scope();

                self.symbols.define(Symbol {
                    name: e.name.name.clone(),
                    kind: SymbolKind::Enum { variants },
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
                                kind: SymbolKind::Function { params: param_types.clone(), ret: ret.clone() },
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
            _ => {} // Impl and Interface handled separately
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
                    if let SymbolKind::Function { params, ret } = &symbol.kind {
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
                        for (arg, _param_ty) in args.iter().zip(params.iter()) {
                            let _arg_ty = self.check_expr(arg)?;
                            // TODO: Check arg_ty matches param_ty
                        }
                        Ok(*ret)
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

                if let SymbolKind::Struct { fields: def_fields, type_params } = &symbol.kind {
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
        }
    }

    /// Get the type of a literal
    fn literal_type(&self, lit: &Literal) -> ResolvedType {
        match lit {
            Literal::Int(_, _) => ResolvedType::Primitive(PrimitiveType::I64), // Default to i64
            Literal::Float(_, _) => ResolvedType::Primitive(PrimitiveType::F64),
            Literal::String(_, _) => ResolvedType::Pointer(Box::new(ResolvedType::Primitive(PrimitiveType::U8))), // C-style string pointer
            Literal::Char(_, _) => ResolvedType::Primitive(PrimitiveType::Char),
            Literal::Bool(_, _) => ResolvedType::Primitive(PrimitiveType::Bool),
        }
    }


    /// Check binary operation and return result type
    fn check_binary_op(&self, left: &ResolvedType, op: BinOp, _right: &ResolvedType, _span: Span) -> Result<ResolvedType> {
        // TODO: Proper type checking
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
            // Arithmetic and bitwise return the operand type
            _ => Ok(left.clone()),
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
                    _ => {
                        // Look up in symbol table
                        if let Some(sym) = self.symbols.lookup(name) {
                            Ok(sym.ty.clone())
                        } else {
                            Ok(ResolvedType::Struct { 
                                name: name.clone(), 
                                fields: Vec::new() 
                            })
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
                        (I32, I64) | (U32, I64) | (U64, I64)
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
