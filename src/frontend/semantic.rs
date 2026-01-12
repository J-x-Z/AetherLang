//! Semantic Analysis for AetherLang
//!
//! Performs:
//! - Symbol table management (scopes, definitions)
//! - Type checking
//! - Ownership analysis (own/ref/mut)

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
    Struct { fields: Vec<(String, ResolvedType)> },
    Enum { variants: Vec<String> },
    Param { ownership: Ownership },
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
}

impl SemanticAnalyzer {
    pub fn new() -> Self {
        let mut analyzer = Self {
            symbols: SymbolTable::new(),
            errors: Vec::new(),
            ownership: OwnershipState::new(),
        };
        analyzer.register_builtins();
        analyzer
    }
    
    /// Register built-in functions
    fn register_builtins(&mut self) {
        // I/O functions
        self.define_builtin("print", vec![ResolvedType::String], ResolvedType::unit());
        self.define_builtin("println", vec![ResolvedType::String], ResolvedType::unit());
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
        self.define_builtin("assert", vec![ResolvedType::Bool], ResolvedType::unit());
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
                let fields: Vec<(String, ResolvedType)> = s.fields.iter()
                    .map(|f| Ok((f.name.name.clone(), self.resolve_type(&f.ty)?)))
                    .collect::<Result<Vec<_>>>()?;

                self.symbols.define(Symbol {
                    name: s.name.name.clone(),
                    kind: SymbolKind::Struct { fields },
                    ty: ResolvedType::Struct { 
                        name: s.name.name.clone(), 
                        fields: Vec::new() 
                    },
                    span: s.span,
                    mutable: false,
                })?;
            }
            Item::Enum(e) => {
                let variants: Vec<String> = e.variants.iter()
                    .map(|v| v.name.name.clone())
                    .collect();

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
        }
    }

    /// Type check a function
    fn check_function(&mut self, func: &Function) -> Result<()> {
        self.symbols.enter_scope();
        self.ownership = OwnershipState::new();

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

        // Check function body
        self.check_block(&func.body)?;

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
                        // TODO: Check compatibility
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
                    Ok(symbol.ty.clone())
                } else {
                    Err(Error::UndefinedVariable {
                        name: ident.name.clone(),
                        span: ident.span,
                    })
                }
            }

            Expr::Binary { left, op, right, span } => {
                let left_ty = self.check_expr(left)?;
                let right_ty = self.check_expr(right)?;
                self.check_binary_op(&left_ty, *op, &right_ty, *span)
            }

            Expr::Unary { op, expr, .. } => {
                let ty = self.check_expr(expr)?;
                self.check_unary_op(*op, &ty)
            }

            Expr::Call { func, args, span } => {
                let func_ty = self.check_expr(func)?;
                match func_ty {
                    ResolvedType::Function { params, ret } => {
                        if args.len() != params.len() {
                            return Err(Error::ArgCountMismatch {
                                expected: params.len(),
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
                    _ => Err(Error::NotCallable { span: *span }),
                }
            }

            Expr::Field { expr, field, span } => {
                let expr_ty = self.check_expr(expr)?;
                match expr_ty {
                    ResolvedType::Struct { name: _, fields } => {
                        for (fname, fty) in fields {
                            if fname == field.name {
                                return Ok(fty);
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
                if cond_ty != ResolvedType::bool() {
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
                if cond_ty != ResolvedType::bool() {
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

            // Placeholders for unimplemented expressions
            Expr::MethodCall { .. } => Ok(ResolvedType::Unknown),
            Expr::StructLit { .. } => Ok(ResolvedType::Unknown),
            Expr::Cast { ty, .. } => self.resolve_type(ty),
            Expr::Range { .. } => Ok(ResolvedType::Unknown),
            Expr::Asm { .. } => Ok(ResolvedType::unit()),
        }
    }

    /// Get the type of a literal
    fn literal_type(&self, lit: &Literal) -> ResolvedType {
        match lit {
            Literal::Int(_, _) => ResolvedType::Primitive(PrimitiveType::I32),
            Literal::Float(_, _) => ResolvedType::Primitive(PrimitiveType::F64),
            Literal::String(_, _) => ResolvedType::Slice(Box::new(
                ResolvedType::Primitive(PrimitiveType::U8)
            )),
            Literal::Char(_, _) => ResolvedType::Primitive(PrimitiveType::Char),
            Literal::Bool(_, _) => ResolvedType::Primitive(PrimitiveType::Bool),
        }
    }

    /// Check binary operation and return result type
    fn check_binary_op(&self, left: &ResolvedType, op: BinOp, right: &ResolvedType, span: Span) -> Result<ResolvedType> {
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
