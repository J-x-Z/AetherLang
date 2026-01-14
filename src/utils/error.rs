//! Error handling for AetherLang

use crate::utils::Span;
use thiserror::Error;

/// Result type alias
pub type Result<T> = std::result::Result<T, Error>;

/// Compiler error
#[derive(Error, Debug, Clone)]
#[allow(dead_code)]
pub enum Error {
    // ==================== Parser Errors ====================
    
    #[error("Unexpected token: expected {expected}, got {got}")]
    UnexpectedToken {
        expected: String,
        got: String,
        span: Span,
    },

    #[error("Expected {0}")]
    Expected(String, Span),

    
    #[error("Expected identifier")]
    ExpectedIdent { span: Span },
    
    #[error("Expected type")]
    ExpectedType { span: Span },
    
    #[error("Expected expression")]
    ExpectedExpr { span: Span },
    
    #[error("Expected pattern")]
    ExpectedPattern { span: Span },
    
    #[error("Expected array size")]
    ExpectedArraySize { span: Span },
    
    #[error("Invalid operator")]
    InvalidOperator { span: Span },
    
    // ==================== Semantic Errors ====================
    
    #[error("Undefined variable: {name}")]
    UndefinedVariable { name: String, span: Span },
    
    #[error("Duplicate definition: {name}")]
    DuplicateDefinition { name: String, span: Span },
    
    #[error("Type mismatch: expected {expected}, got {got}")]
    TypeMismatch {
        expected: String,
        got: String,
        span: Span,
    },
    
    #[error("Argument count mismatch: expected {expected}, got {got}")]
    ArgCountMismatch {
        expected: usize,
        got: usize,
        span: Span,
    },
    
    #[error("Expression is not callable")]
    NotCallable { span: Span },
    
    #[error("Expression is not a struct")]
    NotAStruct { span: Span },
    
    #[error("Undefined type: {name}")]
    UndefinedType { name: String, span: Span },
    
    #[error("Unknown field: {field}")]
    UnknownField { field: String, span: Span },
    
    #[error("Cannot dereference this type")]
    CannotDeref { span: Span },
    
    #[error("Expression is not indexable")]
    NotIndexable { span: Span },
    
    // ==================== Ownership Errors ====================
    
    #[error("Use of moved value: {var}")]
    UseAfterMove { var: String, span: Span },
    
    #[error("Cannot move {var} while it is borrowed")]
    CannotMoveWhileBorrowed { var: String, span: Span },
    
    #[error("Cannot borrow {var} mutably while it is borrowed immutably")]
    CannotMutBorrowWhileBorrowed { var: String, span: Span },
    
    #[error("Cannot borrow {var} while it is mutably borrowed")]
    CannotBorrowWhileMutBorrowed { var: String, span: Span },
    
    #[error("Cannot borrow {var} mutably more than once")]
    CannotMutBorrowTwice { var: String, span: Span },
    
    #[error("Cannot move out of borrowed value: {var}")]
    CannotMoveOutOfBorrow { var: String, span: Span },
    
    #[error("Cannot borrow mutably: {var}")]
    CannotBorrowMutably { var: String, span: Span },
    
    // ==================== AI-Native: Effect Errors ====================
    
    #[error("Effect violation: {message}")]
    EffectViolation { message: String, span: Span },
    
    #[error("IO error: {0}")]
    Io(String),
    
    #[error("LLVM error: {0}")]
    Llvm(String),
    
    #[error("Code generation error: {0}")]
    CodeGen(String),
}

impl Error {
    /// Get the span associated with this error
    pub fn span(&self) -> Option<Span> {
        match self {
            Self::UnexpectedToken { span, .. } => Some(*span),
            Self::Expected(_, span) => Some(*span),
            Self::ExpectedIdent { span } => Some(*span),
            Self::ExpectedType { span } => Some(*span),
            Self::ExpectedExpr { span } => Some(*span),
            Self::ExpectedPattern { span } => Some(*span),
            Self::ExpectedArraySize { span } => Some(*span),
            Self::InvalidOperator { span } => Some(*span),
            Self::UndefinedVariable { span, .. } => Some(*span),
            Self::DuplicateDefinition { span, .. } => Some(*span),
            Self::TypeMismatch { span, .. } => Some(*span),
            Self::ArgCountMismatch { span, .. } => Some(*span),
            Self::NotCallable { span } => Some(*span),
            Self::NotAStruct { span } => Some(*span),
            Self::UndefinedType { span, .. } => Some(*span),
            Self::UnknownField { span, .. } => Some(*span),
            Self::CannotDeref { span } => Some(*span),
            Self::NotIndexable { span } => Some(*span),
            Self::UseAfterMove { span, .. } => Some(*span),
            Self::CannotMoveWhileBorrowed { span, .. } => Some(*span),
            Self::CannotMutBorrowWhileBorrowed { span, .. } => Some(*span),
            Self::CannotBorrowWhileMutBorrowed { span, .. } => Some(*span),
            Self::CannotMutBorrowTwice { span, .. } => Some(*span),
            Self::CannotMoveOutOfBorrow { span, .. } => Some(*span),
            Self::CannotBorrowMutably { span, .. } => Some(*span),
            Self::EffectViolation { span, .. } => Some(*span),
            Self::Io(_) | Self::Llvm(_) | Self::CodeGen(_) => None,
        }
    }
}
