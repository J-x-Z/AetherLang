//! AI-IR Mutation API
//!
//! Provides safe code transformation operations for AI self-iteration.
//! All mutations are validated before application.

use super::{NodeId, AIIRModule};
use super::semantic_graph::NodeKind;

// ==================== Mutation Types ====================

/// Result of a mutation operation
#[derive(Debug, Clone)]
pub struct MutationResult {
    /// Whether the mutation was successful
    pub success: bool,
    
    /// New nodes created by this mutation
    pub new_nodes: Vec<NodeId>,
    
    /// Nodes removed by this mutation
    pub removed_nodes: Vec<NodeId>,
    
    /// Any constraint violations detected
    pub violations: Vec<String>,
    
    /// Warnings (non-fatal issues)
    pub warnings: Vec<String>,
    
    /// Description of the mutation
    pub description: String,
}

impl MutationResult {
    pub fn success(description: &str) -> Self {
        Self {
            success: true,
            new_nodes: vec![],
            removed_nodes: vec![],
            violations: vec![],
            warnings: vec![],
            description: description.to_string(),
        }
    }
    
    pub fn failure(reason: &str) -> Self {
        Self {
            success: false,
            new_nodes: vec![],
            removed_nodes: vec![],
            violations: vec![reason.to_string()],
            warnings: vec![],
            description: format!("Mutation failed: {}", reason),
        }
    }
}

/// A proposed mutation (before execution)
#[derive(Debug, Clone)]
pub struct Mutation {
    pub kind: MutationKind,
    pub target: NodeId,
    pub description: String,
}

/// Types of mutations AI can perform
#[derive(Debug, Clone)]
pub enum MutationKind {
    /// Replace an expression with another
    ReplaceExpression {
        new_value: String,  // Serialized expression
    },
    
    /// Inline a function call at call site
    InlineCall,
    
    /// Extract code into a new function
    ExtractFunction {
        new_name: String,
        param_names: Vec<String>,
    },
    
    /// Rename a symbol
    Rename {
        new_name: String,
    },
    
    /// Add a new node
    AddNode {
        kind: String,
        name: String,
    },
    
    /// Remove a node (if safe)
    RemoveNode,
    
    /// Reorder statements
    Reorder {
        new_position: usize,
    },
}

// ==================== Validation API ====================

/// Validation result for AI-IR
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
}

#[derive(Debug, Clone)]
pub struct ValidationError {
    pub code: String,
    pub message: String,
    pub node: Option<NodeId>,
}

#[derive(Debug, Clone)]
pub struct ValidationWarning {
    pub code: String,
    pub message: String,
    pub node: Option<NodeId>,
}

impl ValidationResult {
    pub fn valid() -> Self {
        Self {
            is_valid: true,
            errors: vec![],
            warnings: vec![],
        }
    }
    
    pub fn invalid(error: &str) -> Self {
        Self {
            is_valid: false,
            errors: vec![ValidationError {
                code: "V0001".to_string(),
                message: error.to_string(),
                node: None,
            }],
            warnings: vec![],
        }
    }
}

// ==================== Mutation API Implementation ====================

impl AIIRModule {
    /// Check if a node is marked as optimizable
    pub fn is_optimizable(&self, node: NodeId) -> bool {
        // Check if the node has an @optimizable annotation
        if let Some(n) = self.graph.get_node(node) {
            // For now, all functions are considered optimizable by default
            // In the future, this will check for @optimizable annotation
            matches!(n.kind, NodeKind::Function { .. })
        } else {
            false
        }
    }
    
    /// Propose a mutation (dry run - doesn't apply)
    pub fn propose_mutation(&self, mutation: &Mutation) -> MutationResult {
        // Validate the mutation
        if !self.is_optimizable(mutation.target) {
            return MutationResult::failure("Target node is not marked as optimizable");
        }
        
        // Check if the mutation is safe
        match &mutation.kind {
            MutationKind::ReplaceExpression { .. } => {
                // Check if target is an expression
                if let Some(node) = self.graph.get_node(mutation.target) {
                    if matches!(node.kind, NodeKind::Expression { .. }) {
                        MutationResult::success("Replace expression is valid")
                    } else {
                        MutationResult::failure("Target is not an expression")
                    }
                } else {
                    MutationResult::failure("Target node not found")
                }
            }
            
            MutationKind::InlineCall => {
                // Check if target is a call expression
                MutationResult::success("Inline call is valid (pending implementation)")
            }
            
            MutationKind::ExtractFunction { new_name, .. } => {
                // Check if name is valid and doesn't conflict
                if self.graph.lookup(new_name).is_some() {
                    MutationResult::failure(&format!("Name '{}' already exists", new_name))
                } else {
                    MutationResult::success("Extract function is valid")
                }
            }
            
            MutationKind::Rename { new_name } => {
                if self.graph.lookup(new_name).is_some() {
                    MutationResult::failure(&format!("Name '{}' already exists", new_name))
                } else {
                    MutationResult::success("Rename is valid")
                }
            }
            
            _ => MutationResult::success("Mutation type is valid (pending implementation)"),
        }
    }
    
    /// Apply a mutation (modifies the AI-IR)
    pub fn apply_mutation(&mut self, mutation: &Mutation) -> MutationResult {
        // First validate
        let validation = self.propose_mutation(mutation);
        if !validation.success {
            return validation;
        }
        
        // Apply the mutation
        match &mutation.kind {
            MutationKind::Rename { new_name } => {
                if let Some(node) = self.graph.get_node_mut(mutation.target) {
                    let old_name = node.name.clone();
                    node.name = new_name.clone();
                    MutationResult {
                        success: true,
                        new_nodes: vec![],
                        removed_nodes: vec![],
                        violations: vec![],
                        warnings: vec![],
                        description: format!("Renamed '{}' to '{}'", old_name, new_name),
                    }
                } else {
                    MutationResult::failure("Node not found")
                }
            }
            
            // Other mutations are pending full implementation
            _ => MutationResult {
                success: true,
                new_nodes: vec![],
                removed_nodes: vec![],
                violations: vec![],
                warnings: vec![format!("Mutation type {:?} is not fully implemented", mutation.kind)],
                description: mutation.description.clone(),
            },
        }
    }
    
    /// Validate the entire module
    pub fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        
        // Check for orphan nodes (nodes with no edges)
        for func in self.graph.functions() {
            // Functions should have at least a body
            if self.graph.edges_from(func.id).is_empty() && 
               self.graph.edges_to(func.id).is_empty() {
                warnings.push(ValidationWarning {
                    code: "W0001".to_string(),
                    message: format!("Function '{}' has no connections", func.name),
                    node: Some(func.id),
                });
            }
        }
        
        // Check constraint consistency
        for constraint in &self.constraints {
            if let Some(_) = self.graph.get_node(constraint.target) {
                // Constraint target exists - OK
            } else {
                errors.push(ValidationError {
                    code: "V0002".to_string(),
                    message: "Constraint references non-existent node".to_string(),
                    node: Some(constraint.target),
                });
            }
        }
        
        ValidationResult {
            is_valid: errors.is_empty(),
            errors,
            warnings,
        }
    }
    
    /// Validate only the changed nodes (incremental)
    pub fn validate_incremental(&self, changed: &[NodeId]) -> ValidationResult {
        let mut errors = Vec::new();
        let warnings = Vec::new();
        
        for &node_id in changed {
            if self.graph.get_node(node_id).is_none() {
                errors.push(ValidationError {
                    code: "V0003".to_string(),
                    message: format!("Changed node {:?} does not exist", node_id),
                    node: Some(node_id),
                });
                continue;
            }
            
            // Check constraints on this node
            for constraint in &self.constraints {
                if constraint.target == node_id {
                    // Constraint exists for this node - validate
                    // (Full validation would require expression evaluation)
                }
            }
        }
        
        ValidationResult {
            is_valid: errors.is_empty(),
            errors,
            warnings,
        }
    }
}

// ==================== Optimizable Region Tracking ====================

/// Tracks which regions of code are marked as optimizable
#[derive(Debug, Clone, Default)]
pub struct OptimizableRegions {
    /// Node IDs marked as optimizable
    pub nodes: Vec<NodeId>,
    
    /// Forbidden nodes (must not be modified)
    pub frozen: Vec<NodeId>,
}

impl OptimizableRegions {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn mark_optimizable(&mut self, node: NodeId) {
        if !self.frozen.contains(&node) {
            self.nodes.push(node);
        }
    }
    
    pub fn freeze(&mut self, node: NodeId) {
        self.frozen.push(node);
        self.nodes.retain(|&n| n != node);
    }
    
    pub fn is_optimizable(&self, node: NodeId) -> bool {
        self.nodes.contains(&node) && !self.frozen.contains(&node)
    }
}
