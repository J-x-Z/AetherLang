//! Abstract Syntax Tree definitions for AetherLang
//!
//! Note: Many fields are reserved for future features (LLVM backend, advanced analysis).
#![allow(dead_code)]

use crate::utils::Span;

// ==================== Generic Parameters (Const Generics Support) ====================

/// A generic parameter: either a type parameter or a const parameter
#[derive(Debug, Clone)]
pub enum GenericParam {
    /// Type parameter: `T`, `U`
    Type(Ident),
    /// Const parameter: `const N: usize`
    Const {
        name: Ident,
        ty: Box<Type>,
    },
}

impl GenericParam {
    pub fn name(&self) -> &Ident {
        match self {
            GenericParam::Type(name) => name,
            GenericParam::Const { name, .. } => name,
        }
    }
}

/// A generic argument: either a type or a const value
#[derive(Debug, Clone)]
pub enum GenericArg {
    /// Type argument: `i32`, `String`
    Type(Type),
    /// Const argument: `3`, `N`, `{N + 1}`
    Const(Expr),
}

/// A complete program (compilation unit)
#[derive(Debug, Clone)]
pub struct Program {
    pub items: Vec<Item>,
    /// Inner attributes: #![no_std], #![no_main], etc.
    pub inner_attrs: Vec<Annotation>,
}

/// Top-level items
#[derive(Debug, Clone)]
pub enum Item {
    Function(Function),
    Struct(StructDef),
    Enum(EnumDef),
    Impl(ImplBlock),
    Interface(InterfaceDef),
    Const(ConstDef),
    /// Macro definition
    Macro(MacroDef),
    /// Module definition
    Module(ModuleDef),
    /// Use/import statement
    Use(UseDecl),
    /// Extern block (FFI)
    Extern(ExternBlock),
    /// Static variable (global)
    Static(StaticDef),
    /// Union definition
    Union(UnionDef),
    /// Trait definition (uses InterfaceDef as underlying structure)
    Trait(InterfaceDef),
    /// Type alias (type Foo = Bar)
    TypeAlias(TypeAliasDef),
}

/// Function definition
#[derive(Debug, Clone)]
pub struct Function {
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret_type: Option<Type>,
    pub body: Block,
    pub span: Span,
    // AI-Native extensions
    pub annotations: Vec<Annotation>,
    pub contracts: Vec<Contract>,
    pub effects: EffectSet,
    pub is_pub: bool,
    pub type_params: Vec<Ident>,
}

/// Function parameter
#[derive(Debug, Clone)]
pub struct Param {
    pub name: Ident,
    pub ownership: Ownership,
    pub ty: Type,
    pub span: Span,
}

/// Ownership modifier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    /// Owned value (default, move semantics)
    Own,
    /// Immutable borrow
    Ref,
    /// Mutable borrow
    Mut,
    /// Shared ownership (reference counted)
    Shared,
}

impl Default for Ownership {
    fn default() -> Self {
        Ownership::Own
    }
}

// ==================== AI-Native AST Extensions ====================

/// Annotation (e.g., @inline, @test, @static)
#[derive(Debug, Clone)]
pub struct Annotation {
    pub name: Ident,
    pub args: Vec<Expr>,
    pub span: Span,
}

/// Contract clause (requires/ensures/invariant)
#[derive(Debug, Clone)]
pub struct Contract {
    pub kind: ContractKind,
    pub condition: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractKind {
    /// Precondition: caller must satisfy
    Requires,
    /// Postcondition: function guarantees
    Ensures,
    /// Type invariant: always holds
    Invariant,
}

/// Effect set for a function
#[derive(Debug, Clone, Default)]
pub struct EffectSet {
    pub is_pure: bool,
    pub effects: Vec<Effect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Read,
    Write,
    IO,
    Alloc,
    Panic,
}


/// Struct definition
#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: Ident,
    pub fields: Vec<Field>,
    pub span: Span,
    // AI-Native extensions
    pub annotations: Vec<Annotation>,
    pub invariants: Vec<Contract>,
    pub is_pub: bool,
    /// Generic parameters including const generics: `<T, const N: usize>`
    pub generic_params: Vec<GenericParam>,
    /// Legacy type_params for backward compatibility
    pub type_params: Vec<Ident>,
}

/// Struct field
#[derive(Debug, Clone)]
pub struct Field {
    pub name: Ident,
    pub ty: Type,
    pub span: Span,
}

/// Enum definition
#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: Ident,
    pub variants: Vec<Variant>,
    pub span: Span,
    /// Generic parameters including const generics
    pub generic_params: Vec<GenericParam>,
    /// Legacy type_params for backward compatibility
    pub type_params: Vec<Ident>,
}

/// Enum variant
#[derive(Debug, Clone)]
pub struct Variant {
    pub name: Ident,
    pub fields: Vec<Type>,
    pub span: Span,
}

/// Impl block
#[derive(Debug, Clone)]
pub struct ImplBlock {
    pub target: Ident,
    pub interface: Option<Ident>,
    pub methods: Vec<Function>,
    pub span: Span,
}

/// Trait definition (interface with optional default implementations)
#[derive(Debug, Clone)]
pub struct InterfaceDef {
    pub name: Ident,
    /// Type parameters (generics)
    pub type_params: Vec<Ident>,
    /// Method signatures (without default implementation)
    pub methods: Vec<FunctionSig>,
    /// Methods with default implementations
    pub default_methods: Vec<Function>,
    /// Associated types
    pub associated_types: Vec<AssociatedType>,
    /// Supertraits (traits this trait extends)
    pub supertraits: Vec<Type>,
    pub span: Span,
    pub is_pub: bool,
}

/// Associated type in a trait
#[derive(Debug, Clone)]
pub struct AssociatedType {
    pub name: Ident,
    /// Optional default type
    pub default_ty: Option<Type>,
    /// Trait bounds on the associated type
    pub bounds: Vec<Type>,
    pub span: Span,
}

/// Function signature (for traits/interfaces)
#[derive(Debug, Clone)]
pub struct FunctionSig {
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret_type: Option<Type>,
    /// Effect annotations
    pub effects: EffectSet,
    /// Contract clauses
    pub contracts: Vec<Contract>,
    pub span: Span,
}

/// Constant definition
#[derive(Debug, Clone)]
pub struct ConstDef {
    pub name: Ident,
    pub ty: Option<Type>,
    pub value: Expr,
    pub span: Span,
}

/// Type alias definition (type Foo = Bar)
#[derive(Debug, Clone)]
pub struct TypeAliasDef {
    pub name: Ident,
    /// Type parameters for generic aliases
    pub type_params: Vec<Ident>,
    /// The type this alias refers to
    pub ty: Type,
    pub is_pub: bool,
    pub span: Span,
}

// ==================== Macro System ====================

/// Macro definition
#[derive(Debug, Clone)]
pub struct MacroDef {
    pub name: Ident,
    pub kind: MacroKind,
    pub span: Span,
    pub is_pub: bool,
}

/// Kind of macro
#[derive(Debug, Clone)]
pub enum MacroKind {
    /// Declarative macro (pattern matching)
    Declarative {
        rules: Vec<MacroRule>,
    },
    /// Procedural macro (code transformation)
    Procedural {
        /// Function to call for transformation
        handler: Ident,
    },
}

/// A single macro rule (pattern => template)
#[derive(Debug, Clone)]
pub struct MacroRule {
    /// Pattern to match
    pub pattern: MacroPattern,
    /// Template to expand
    pub template: MacroTemplate,
    pub span: Span,
}

/// Macro pattern (simplified)
#[derive(Debug, Clone)]
pub struct MacroPattern {
    pub tokens: Vec<MacroToken>,
}

/// Macro template (simplified)
#[derive(Debug, Clone)]
pub struct MacroTemplate {
    pub tokens: Vec<MacroToken>,
}

/// Token in a macro pattern/template
#[derive(Debug, Clone)]
pub enum MacroToken {
    /// Literal token
    Literal(String),
    /// Variable (e.g., $expr, $ident)
    Variable { name: String, kind: String },
    /// Repetition (e.g., $($x:expr),*)
    Repetition { pattern: Vec<MacroToken>, separator: Option<String> },
}

// ==================== Module System ====================

/// Module definition
#[derive(Debug, Clone)]
pub struct ModuleDef {
    pub name: Ident,
    /// Inline module items (if Some) or external file (if None)
    pub items: Option<Vec<Item>>,
    pub span: Span,
    pub is_pub: bool,
}

/// Use/import declaration
#[derive(Debug, Clone)]
pub struct UseDecl {
    /// Path to import (e.g., std::io::File)
    pub path: Vec<Ident>,
    /// Kind of import
    pub kind: UseKind,
    pub span: Span,
    pub is_pub: bool,
}

/// Kind of use declaration
#[derive(Debug, Clone)]
pub enum UseKind {
    /// Import single item (use foo::bar)
    Simple,
    /// Import with alias (use foo::bar as baz)
    Alias(Ident),
    /// Import all (use foo::*)
    Glob,
    /// Import multiple (use foo::{a, b, c})
    Group(Vec<UseDecl>),
}

// ==================== FFI System (Phase 8) ====================

/// Extern block for FFI declarations
#[derive(Debug, Clone)]
pub struct ExternBlock {
    /// ABI specification (e.g., "C", "stdcall")
    pub abi: Option<String>,
    /// Foreign function declarations
    pub items: Vec<ForeignItem>,
    pub span: Span,
}

/// Foreign item (function or static) declaration
#[derive(Debug, Clone)]
pub enum ForeignItem {
    /// Foreign function with optional contracts
    Fn {
        name: Ident,
        params: Vec<Param>,
        ret_type: Option<Type>,
        /// Annotations for AI understanding (@pure, @reads, @allocs, etc.)
        annotations: Vec<Annotation>,
        /// Whether this function is variadic (has ... at end of params)
        variadic: bool,
        span: Span,
    },
    /// Foreign static variable
    Static {
        name: Ident,
        ty: Type,
        is_mut: bool,
        span: Span,
    },
}

/// Static variable definition (global)
#[derive(Debug, Clone)]
pub struct StaticDef {
    pub name: Ident,
    pub ty: Type,
    pub value: Option<Expr>,
    pub is_mut: bool,
    pub is_pub: bool,
    pub span: Span,
}

/// Union definition (overlapping memory layout)
#[derive(Debug, Clone)]
pub struct UnionDef {
    pub name: Ident,
    pub fields: Vec<Field>,
    pub span: Span,
    pub is_pub: bool,
    /// Memory representation (C, packed, etc.)
    pub repr: Option<Repr>,
}

/// Memory representation attribute
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repr {
    /// C-compatible layout
    C,
    /// Packed (no padding)
    Packed,
    /// Transparent (single-field wrapper)
    Transparent,
}

/// Code block
#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

/// Statement
#[derive(Debug, Clone)]
pub enum Stmt {
    /// let [mut] name [: type] = expr
    Let {
        name: Ident,
        mutable: bool,
        ty: Option<Type>,
        value: Option<Expr>,
        span: Span,
    },
    /// Expression statement
    Expr(Expr),
    /// return [expr]
    Return {
        value: Option<Expr>,
        span: Span,
    },
    /// break
    Break { span: Span },
    /// continue
    Continue { span: Span },
    /// Empty statement (;)
    Empty { span: Span },
}

/// Expression
#[derive(Debug, Clone)]
pub enum Expr {
    /// Literal value
    Literal(Literal),
    /// Identifier
    Ident(Ident),
    /// Path (e.g. Option::Some)
    Path {
        segments: Vec<Ident>,
        span: Span,
    },
    /// Binary operation
    Binary {
        left: Box<Expr>,
        op: BinOp,
        right: Box<Expr>,
        span: Span,
    },
    /// Unary operation
    Unary {
        op: UnOp,
        expr: Box<Expr>,
        span: Span,
    },
    /// Function call
    Call {
        func: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    /// Field access (expr.field)
    Field {
        expr: Box<Expr>,
        field: Ident,
        span: Span,
    },
    /// Method call (expr.method(args))
    MethodCall {
        expr: Box<Expr>,
        method: Ident,
        args: Vec<Expr>,
        span: Span,
    },
    /// Index access (expr[index])
    Index {
        expr: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    /// Block expression
    Block(Block),
    /// If expression
    If {
        cond: Box<Expr>,
        then_block: Block,
        else_block: Option<Block>,
        span: Span,
    },
    /// Match expression
    Match {
        expr: Box<Expr>,
        arms: Vec<MatchArm>,
        span: Span,
    },
    /// Loop
    Loop {
        body: Block,
        span: Span,
    },
    /// While loop
    While {
        cond: Box<Expr>,
        body: Block,
        span: Span,
    },
    /// For loop
    For {
        var: Ident,
        iter: Box<Expr>,
        body: Block,
        span: Span,
    },
    /// Struct literal
    StructLit {
        name: Ident,
        fields: Vec<(Ident, Expr)>,
        span: Span,
    },
    /// Array literal
    Array {
        elements: Vec<Expr>,
        span: Span,
    },
    /// Tuple literal
    Tuple {
        elements: Vec<Expr>,
        span: Span,
    },
    /// Reference (&expr or &mut expr)
    Ref {
        mutable: bool,
        expr: Box<Expr>,
        span: Span,
    },
    /// Dereference (*expr)
    Deref {
        expr: Box<Expr>,
        span: Span,
    },
    /// Cast (expr as Type)
    Cast {
        expr: Box<Expr>,
        ty: Type,
        span: Span,
    },
    /// Range (start..end)
    Range {
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        span: Span,
    },
    /// Unsafe block with optional AI metadata
    Unsafe {
        body: Block,
        /// Optional reason explaining why this is unsafe (for AI understanding)
        reason: Option<String>,
        /// Optional verifier function to call for validation
        verifier: Option<Ident>,
        span: Span,
    },
    /// Inline assembly
    Asm {
        template: String,
        operands: Vec<AsmOperand>,
        span: Span,
    },
    /// Error propagation (expr?)
    Try {
        expr: Box<Expr>,
        span: Span,
    },
    /// Closure/Lambda expression (|x, y| x + y)
    Closure {
        params: Vec<ClosureParam>,
        ret_type: Option<Type>,
        body: Box<Expr>,
        span: Span,
    },
}

/// Match arm
#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Box<Expr>>,
    pub body: Expr,
    pub span: Span,
}

/// Pattern for matching
#[derive(Debug, Clone)]
pub enum Pattern {
    /// Wildcard (_)
    Wildcard { span: Span },
    /// Binding (name)
    Binding { name: Ident, mutable: bool, span: Span },
    /// Literal
    Literal(Literal),
    /// Struct pattern
    Struct {
        name: Ident,
        fields: Vec<(Ident, Pattern)>,
        span: Span,
    },
    /// Tuple pattern
    Tuple { elements: Vec<Pattern>, span: Span },
    /// Enum variant pattern
    Variant {
        enum_name: Option<Ident>,
        variant: Ident,
        fields: Vec<Pattern>,
        span: Span,
    },
}

/// Inline assembly operand
#[derive(Debug, Clone)]
pub struct AsmOperand {
    pub kind: AsmOperandKind,
    pub options: String, // "reg", "memory", etc.
    pub expr: Option<Expr>, // None for clobber
}

/// Closure parameter (optionally typed)
#[derive(Debug, Clone)]
pub struct ClosureParam {
    pub name: Ident,
    pub ty: Option<Type>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AsmOperandKind {
    Input,  // in(reg) val
    Output, // out(reg) val
    InOut,  // inout(reg) val
    Clobber, // clobber("memory")
}

/// Literal value
#[derive(Debug, Clone)]
pub enum Literal {
    Int(i64, Span),
    Float(f64, Span),
    String(String, Span),
    Char(char, Span),
    Bool(bool, Span),
}

impl Literal {
    pub fn span(&self) -> Span {
        match self {
            Literal::Int(_, s) => *s,
            Literal::Float(_, s) => *s,
            Literal::String(_, s) => *s,
            Literal::Char(_, s) => *s,
            Literal::Bool(_, s) => *s,
        }
    }
}

/// Identifier
#[derive(Debug, Clone)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

/// Binary operator
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    // Logical
    And,
    Or,
    // Bitwise
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    // Assignment
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
}

/// Unary operator
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    /// Negation (-)
    Neg,
    /// Logical not (!)
    Not,
    /// Bitwise not (~)
    BitNot,
}

/// Type representation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// Named type (i32, String)
    Named(String, Span),
    /// Generic type instantiation (Option<T>, Vec<i32>)
    Generic(String, Vec<Type>, Span),
    /// Generic type with mixed type and const args (Matrix<f32, 3, 3>)
    GenericWithArgs {
        name: String,
        args: Vec<GenericArg>,
        span: Span,
    },
    /// Pointer type (*T)
    Pointer(Box<Type>, Span),
    /// Reference type (&T or &mut T or &'a T)
    Ref {
        mutable: bool,
        /// Optional lifetime annotation ('a, 'static)
        lifetime: Option<String>,
        inner: Box<Type>,
        span: Span,
    },
    /// Array type ([T; N])
    Array {
        elem: Box<Type>,
        size: usize,
        span: Span,
    },
    /// Slice type ([T])
    Slice(Box<Type>, Span),
    /// Tuple type ((T1, T2, ...))
    Tuple(Vec<Type>, Span),
    /// Function type (fn(A, B) -> R)
    Function {
        params: Vec<Type>,
        ret: Box<Type>,
        span: Span,
    },
    /// Never type (!)
    Never(Span),
    /// Unit type (())
    Unit(Span),
    /// Inferred type (_)
    Infer(Span),
    /// Owned type with explicit ownership (own T, shared T)
    Owned {
        inner: Box<Type>,
        ownership: Ownership,
        span: Span,
    },
    /// Volatile type (*volatile T) - prevents compiler optimization of memory access
    Volatile(Box<Type>, Span),
}

impl Type {
    pub fn span(&self) -> Span {
        match self {
            Type::Named(_, s) => *s,
            Type::Generic(_, _, s) => *s,
            Type::GenericWithArgs { span, .. } => *span,
            Type::Pointer(_, s) => *s,
            Type::Ref { span, .. } => *span,
            Type::Array { span, .. } => *span,
            Type::Slice(_, s) => *s,
            Type::Tuple(_, s) => *s,
            Type::Function { span, .. } => *span,
            Type::Never(s) => *s,
            Type::Unit(s) => *s,
            Type::Infer(s) => *s,
            Type::Owned { span, .. } => *span,
            Type::Volatile(_, s) => *s,
        }
    }
}
