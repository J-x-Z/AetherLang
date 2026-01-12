//! Abstract Syntax Tree definitions for AetherLang

use crate::utils::Span;

/// A complete program (compilation unit)
#[derive(Debug, Clone)]
pub struct Program {
    pub items: Vec<Item>,
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
}

/// Function definition
#[derive(Debug, Clone)]
pub struct Function {
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret_type: Option<Type>,
    pub body: Block,
    pub span: Span,
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
    /// Owned value (default)
    Own,
    /// Immutable borrow
    Ref,
    /// Mutable borrow
    Mut,
}

impl Default for Ownership {
    fn default() -> Self {
        Ownership::Own
    }
}

/// Struct definition
#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: Ident,
    pub fields: Vec<Field>,
    pub span: Span,
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

/// Interface definition
#[derive(Debug, Clone)]
pub struct InterfaceDef {
    pub name: Ident,
    pub methods: Vec<FunctionSig>,
    pub span: Span,
}

/// Function signature (for interfaces)
#[derive(Debug, Clone)]
pub struct FunctionSig {
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret_type: Option<Type>,
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
    /// Unsafe block
    Unsafe {
        body: Block,
        span: Span,
    },
    /// Inline assembly
    Asm {
        template: String,
        operands: Vec<AsmOperand>,
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
    pub constraint: String,
    pub expr: Expr,
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
    /// Named type (i32, String, MyStruct)
    Named(String, Span),
    /// Pointer type (*T)
    Pointer(Box<Type>, Span),
    /// Reference type (&T or &mut T)
    Ref {
        mutable: bool,
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
}

impl Type {
    pub fn span(&self) -> Span {
        match self {
            Type::Named(_, s) => *s,
            Type::Pointer(_, s) => *s,
            Type::Ref { span, .. } => *span,
            Type::Array { span, .. } => *span,
            Type::Slice(_, s) => *s,
            Type::Tuple(_, s) => *s,
            Type::Function { span, .. } => *span,
            Type::Never(s) => *s,
            Type::Unit(s) => *s,
            Type::Infer(s) => *s,
        }
    }
}
