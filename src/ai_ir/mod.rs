//! AI-IR: AI-Readable Intermediate Representation
//!
//! This module provides data structures and APIs for AI to understand,
//! query, and modify code at a semantic level.
#![allow(dead_code, unused_imports)]

pub mod semantic_graph;
pub mod intent;
pub mod constraint;
pub mod query;
pub mod converter;
pub mod mutation;

pub use semantic_graph::*;
pub use constraint::*;


// ==================== Core Types ====================

/// Unique identifier for nodes in the semantic graph
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub usize);

/// Unique identifier for edges in the semantic graph
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EdgeId(pub usize);

/// Unique identifier for constraints
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConstraintId(pub usize);

// ==================== AI-IR Module ====================

/// The complete AI-IR for a compilation unit
#[derive(Debug, Clone)]
pub struct AIIRModule {
    /// Module name
    pub name: String,
    
    /// Semantic graph containing all nodes and edges
    pub graph: SemanticGraph,
    
    /// All constraints (explicit and inferred)
    pub constraints: Vec<Constraint>,
    
    /// Optimization hints
    pub hints: Vec<OptimizationHint>,
    
    /// Metadata
    pub metadata: ModuleMetadata,
}

/// Module metadata for AI tracking
#[derive(Debug, Clone, Default)]
pub struct ModuleMetadata {
    /// Strictness level
    pub strict_mode: bool,
    
    /// Version counter for iterative optimization
    pub version: u64,
    
    /// Last modifier (AI model identifier)
    pub last_modified_by: Option<String>,
}

/// Optimization hint for AI
#[derive(Debug, Clone)]
pub struct OptimizationHint {
    pub target: NodeId,
    pub kind: OptimizationHintKind,
    pub priority: u8,
}

#[derive(Debug, Clone)]
pub enum OptimizationHintKind {
    /// Mark as hot code path
    Hotspot { estimated_calls: u64 },
    /// Performance bottleneck
    Bottleneck { issue: String },
    /// Can be inlined
    Inlinable,
    /// Can be parallelized
    Parallelizable,
    /// Loop can be unrolled
    LoopUnrollable { factor: usize },
}

impl AIIRModule {
    /// Create a new empty AI-IR module
    pub fn new(name: String) -> Self {
        Self {
            name,
            graph: SemanticGraph::new(),
            constraints: Vec::new(),
            hints: Vec::new(),
            metadata: ModuleMetadata::default(),
        }
    }
    
    /// Get a node by ID
    pub fn get_node(&self, id: NodeId) -> Option<&SemanticNode> {
        self.graph.get_node(id)
    }
    
    /// Get all edges from a node
    pub fn get_edges_from(&self, id: NodeId) -> Vec<&SemanticEdge> {
        self.graph.edges_from(id)
    }
    
    /// Get all edges to a node
    pub fn get_edges_to(&self, id: NodeId) -> Vec<&SemanticEdge> {
        self.graph.edges_to(id)
    }
    
    /// Get constraints for a node
    pub fn get_constraints(&self, id: NodeId) -> Vec<&Constraint> {
        self.constraints.iter()
            .filter(|c| c.target == id)
            .collect()
    }
}
