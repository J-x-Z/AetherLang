use super::token::Span;

#[derive(Debug, Clone)]
pub struct ScriptModule {
    pub stmts: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    FunctionDef(FunctionDef),
    If(IfStmt),
    While(WhileStmt),
    For(ForStmt),
    Return(ReturnStmt),
    Expr(Expr),
    Assign(AssignStmt),
    Pass,
    Break,
    Continue,
    // Add ClassDef, Import, etc. later
}

#[derive(Debug, Clone)]
pub struct FunctionDef {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<TypeHint>,
    pub body: Vec<Stmt>,
    pub is_comptime: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub type_hint: Option<TypeHint>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TypeHint {
    pub name: String,
    pub generics: Vec<TypeHint>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct IfStmt {
    pub condition: Expr,
    pub then_block: Vec<Stmt>,
    pub else_block: Option<Vec<Stmt>>, // For elif/else
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct WhileStmt {
    pub condition: Expr,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ForStmt {
    pub var: String,
    pub iterable: Expr,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ReturnStmt {
    pub value: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct AssignStmt {
    pub target: Expr,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Identifier { name: String, span: Span },
    Integer { value: i64, span: Span },
    Boolean { value: bool, span: Span },
    Unary { op: UnaryOp, expr: Box<Expr>, span: Span },
    Float { value: f64, span: Span },
    String { value: String, span: Span },
    Binary { left: Box<Expr>, op: BinOp, right: Box<Expr>, span: Span },
    Call { func: Box<Expr>, args: Vec<Expr>, span: Span },
    FieldAccess { target: Box<Expr>, field: String, span: Span },
    List { elements: Vec<Expr>, span: Span },
}

#[derive(Debug, Clone)]
pub enum BinOp {
    Add, Sub, Mul, Div, Mod,
    Eq, Ne, Lt, Gt, Le, Ge,
    And, Or,
}

#[derive(Debug, Clone)]
pub enum UnaryOp { Neg, Not }

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Self::Identifier { span, .. } | Self::Integer { span, .. } | Self::Boolean { span, .. }
            | Self::Unary { span, .. } | Self::Float { span, .. } | Self::String { span, .. }
            | Self::Binary { span, .. } | Self::Call { span, .. } | Self::FieldAccess { span, .. }
            | Self::List { span, .. } => *span,
        }
    }
}
