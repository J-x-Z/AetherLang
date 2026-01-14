//! Constraint Layer: Explicit and inferred constraints for AI reasoning
//!
//! The constraint layer captures all constraints on code - both explicit
//! (from contracts) and inferred (from type system, effects, etc.)

use crate::utils::Span;
use super::NodeId;

/// A constraint on code
#[derive(Debug, Clone)]
pub struct Constraint {
    pub id: super::ConstraintId,
    pub target: NodeId,
    pub kind: ConstraintKind,
    pub source: ConstraintSource,
    pub verification: VerificationStrategy,
}

/// The kind of constraint
#[derive(Debug, Clone)]
pub enum ConstraintKind {
    /// Precondition (caller must satisfy)
    Precondition { expr: String },
    
    /// Postcondition (function guarantees)
    Postcondition { expr: String },
    
    /// Type invariant (always holds)
    Invariant { expr: String },
    
    /// Type bound (T: Trait)
    TypeBound {
        type_param: String,
        bounds: Vec<String>,
    },
    
    /// Lifetime constraint ('a: 'b)
    Lifetime {
        short: String,
        outlives: String,
    },
    
    /// Effect constraint
    Effect { allowed_effects: Vec<String> },
    
    /// Value range constraint
    ValueRange {
        min: Option<i64>,
        max: Option<i64>,
    },
    
    /// Non-null constraint
    NonNull,
    
    /// Initialized constraint
    Initialized,
}

/// Where the constraint comes from
#[derive(Debug, Clone)]
pub enum ConstraintSource {
    /// Explicitly declared by user
    Explicit { span: Span },
    
    /// Inferred by the compiler
    Inferred { reason: String },
    
    /// Propagated from another constraint
    Propagated { from: NodeId },
}

/// How to verify the constraint
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationStrategy {
    /// Verify at compile time
    Static,
    
    /// Generate runtime assertion
    Runtime,
    
    /// Try static, fall back to runtime
    Hybrid,
    
    /// Documentation only, no verification
    Documentation,
}

impl Constraint {
    /// Create an explicit precondition
    pub fn precondition(id: super::ConstraintId, target: NodeId, expr: String, span: Span) -> Self {
        Self {
            id,
            target,
            kind: ConstraintKind::Precondition { expr },
            source: ConstraintSource::Explicit { span },
            verification: VerificationStrategy::Hybrid,
        }
    }
    
    /// Create an explicit postcondition
    pub fn postcondition(id: super::ConstraintId, target: NodeId, expr: String, span: Span) -> Self {
        Self {
            id,
            target,
            kind: ConstraintKind::Postcondition { expr },
            source: ConstraintSource::Explicit { span },
            verification: VerificationStrategy::Hybrid,
        }
    }
    
    /// Create an inferred constraint
    pub fn inferred(id: super::ConstraintId, target: NodeId, kind: ConstraintKind, reason: &str) -> Self {
        Self {
            id,
            target,
            kind,
            source: ConstraintSource::Inferred { reason: reason.to_string() },
            verification: VerificationStrategy::Static,
        }
    }
}
