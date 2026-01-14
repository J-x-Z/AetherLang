//! Semantic Graph: Core data structure for AI-IR
//!
//! The semantic graph represents code as a graph where nodes are code entities
//! (functions, types, variables, expressions) and edges are relationships
//! between them (calls, data flow, type relationships).

use crate::utils::Span;
use crate::frontend::ast::{Ownership, EffectSet};
use super::{NodeId, EdgeId};
use std::collections::HashMap;

// ==================== Semantic Graph ====================

/// The semantic graph containing all nodes and edges
#[derive(Debug, Clone, Default)]
pub struct SemanticGraph {
    /// All nodes in the graph
    nodes: Vec<SemanticNode>,
    
    /// All edges in the graph
    edges: Vec<SemanticEdge>,
    
    /// Index: node name → node ID (for quick lookup)
    name_index: HashMap<String, NodeId>,
    
    /// Index: source node → edges from it
    edges_from_index: HashMap<NodeId, Vec<EdgeId>>,
    
    /// Index: target node → edges to it
    edges_to_index: HashMap<NodeId, Vec<EdgeId>>,
}

// ==================== Semantic Node ====================

/// A node in the semantic graph representing a code entity
#[derive(Debug, Clone)]
pub struct SemanticNode {
    pub id: NodeId,
    pub kind: NodeKind,
    pub span: Span,
    /// Human-readable name (for AI understanding)
    pub name: String,
    /// Intent annotation (what this code does at high level)
    pub intent: Option<super::intent::Intent>,
}

/// The kind of semantic node
#[derive(Debug, Clone)]
pub enum NodeKind {
    /// A function definition
    Function {
        params: Vec<(String, String)>,  // (name, type_name)
        return_type: Option<String>,
        effects: EffectSet,
        is_pure: bool,
    },
    
    /// A type definition (struct, enum)
    Type {
        type_kind: TypeNodeKind,
        fields: Vec<(String, String)>,  // (field_name, type_name)
    },
    
    /// A variable binding
    Variable {
        type_name: String,
        ownership: Ownership,
        is_mutable: bool,
    },
    
    /// An expression
    Expression {
        expr_kind: ExprNodeKind,
        type_name: String,
    },
    
    /// A code block
    Block {
        stmt_count: usize,
    },
}

#[derive(Debug, Clone)]
pub enum TypeNodeKind {
    Struct,
    Enum,
    Alias,
}

#[derive(Debug, Clone)]
pub enum ExprNodeKind {
    Literal,
    BinaryOp,
    UnaryOp,
    Call,
    FieldAccess,
    If,
    Match,
    Loop,
}

// ==================== Semantic Edge ====================

/// An edge in the semantic graph representing a relationship
#[derive(Debug, Clone)]
pub struct SemanticEdge {
    pub id: EdgeId,
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
}

/// The kind of relationship between nodes
#[derive(Debug, Clone)]
pub enum EdgeKind {
    /// Function calls another function
    Calls,
    
    /// Data flows from one node to another
    DataFlow {
        ownership_transfer: bool,
    },
    
    /// Control flow edge
    ControlFlow,
    
    /// Variable/expression has type
    TypeOf,
    
    /// Node depends on another (e.g., uses its value)
    DependsOn,
    
    /// Type implements interface/trait
    Implements,
    
    /// Node is constrained by another
    ConstrainedBy,
    
    /// Ownership relationship
    Owns,
    
    /// Borrow relationship
    Borrows { mutable: bool },
}

// ==================== SemanticGraph Implementation ====================

impl SemanticGraph {
    /// Create a new empty semantic graph
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Add a node to the graph
    pub fn add_node(&mut self, kind: NodeKind, name: String, span: Span) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(SemanticNode {
            id,
            kind,
            span,
            name: name.clone(),
            intent: None,
        });
        self.name_index.insert(name, id);
        id
    }
    
    /// Add an edge to the graph
    pub fn add_edge(&mut self, from: NodeId, to: NodeId, kind: EdgeKind) -> EdgeId {
        let id = EdgeId(self.edges.len());
        self.edges.push(SemanticEdge { id, from, to, kind });
        
        // Update indices
        self.edges_from_index.entry(from).or_default().push(id);
        self.edges_to_index.entry(to).or_default().push(id);
        
        id
    }
    
    /// Get a node by ID
    pub fn get_node(&self, id: NodeId) -> Option<&SemanticNode> {
        self.nodes.get(id.0)
    }
    
    /// Get a mutable node by ID
    pub fn get_node_mut(&mut self, id: NodeId) -> Option<&mut SemanticNode> {
        self.nodes.get_mut(id.0)
    }
    
    /// Look up a node by name
    pub fn lookup(&self, name: &str) -> Option<NodeId> {
        self.name_index.get(name).copied()
    }
    
    /// Get all edges from a node
    pub fn edges_from(&self, id: NodeId) -> Vec<&SemanticEdge> {
        self.edges_from_index
            .get(&id)
            .map(|ids| ids.iter().filter_map(|eid| self.edges.get(eid.0)).collect())
            .unwrap_or_default()
    }
    
    /// Get all edges to a node
    pub fn edges_to(&self, id: NodeId) -> Vec<&SemanticEdge> {
        self.edges_to_index
            .get(&id)
            .map(|ids| ids.iter().filter_map(|eid| self.edges.get(eid.0)).collect())
            .unwrap_or_default()
    }
    
    /// Get all nodes of a specific kind
    pub fn nodes_of_kind<F>(&self, predicate: F) -> Vec<&SemanticNode>
    where
        F: Fn(&NodeKind) -> bool,
    {
        self.nodes.iter().filter(|n| predicate(&n.kind)).collect()
    }
    
    /// Get all function nodes
    pub fn functions(&self) -> Vec<&SemanticNode> {
        self.nodes_of_kind(|k| matches!(k, NodeKind::Function { .. }))
    }
    
    /// Get all type nodes
    pub fn types(&self) -> Vec<&SemanticNode> {
        self.nodes_of_kind(|k| matches!(k, NodeKind::Type { .. }))
    }
    
    /// Count total nodes
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    
    /// Count total edges
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}
