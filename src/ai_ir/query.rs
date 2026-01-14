//! Query API: AI-friendly interface for querying the semantic graph
//!
//! This module provides query methods for AI to understand code structure,
//! relationships, and constraints.

use super::{NodeId, AIIRModule, EdgeKind, Constraint};

/// Query result for callers of a function
#[derive(Debug, Clone)]
pub struct CallersResult {
    pub callers: Vec<NodeId>,
}

/// Query result for callees of a function  
#[derive(Debug, Clone)]
pub struct CalleesResult {
    pub callees: Vec<NodeId>,
}

/// Query result for data flow
#[derive(Debug, Clone)]
pub struct DataFlowResult {
    pub sources: Vec<NodeId>,
    pub sinks: Vec<NodeId>,
}

/// Query API implementation for AIIRModule
impl AIIRModule {
    // === Relationship Queries ===
    
    /// Get all functions that call this function
    pub fn get_callers(&self, func: NodeId) -> CallersResult {
        let callers = self.graph.edges_to(func)
            .iter()
            .filter(|e| matches!(e.kind, EdgeKind::Calls))
            .map(|e| e.from)
            .collect();
        CallersResult { callers }
    }
    
    /// Get all functions called by this function
    pub fn get_callees(&self, func: NodeId) -> CalleesResult {
        let callees = self.graph.edges_from(func)
            .iter()
            .filter(|e| matches!(e.kind, EdgeKind::Calls))
            .map(|e| e.to)
            .collect();
        CalleesResult { callees }
    }
    
    /// Get data flow information for a node
    pub fn get_dataflow(&self, node: NodeId) -> DataFlowResult {
        let sources = self.graph.edges_to(node)
            .iter()
            .filter(|e| matches!(e.kind, EdgeKind::DataFlow { .. }))
            .map(|e| e.from)
            .collect();
        let sinks = self.graph.edges_from(node)
            .iter()
            .filter(|e| matches!(e.kind, EdgeKind::DataFlow { .. }))
            .map(|e| e.to)
            .collect();
        DataFlowResult { sources, sinks }
    }
    
    // === Type Queries ===
    
    /// Get the type of a node
    pub fn get_type_of(&self, node: NodeId) -> Option<NodeId> {
        self.graph.edges_from(node)
            .iter()
            .find(|e| matches!(e.kind, EdgeKind::TypeOf))
            .map(|e| e.to)
    }
    
    /// Get all nodes of a given type
    pub fn nodes_of_type(&self, type_name: &str) -> Vec<NodeId> {
        self.graph.lookup(type_name)
            .map(|type_id| {
                self.graph.edges_to(type_id)
                    .iter()
                    .filter(|e| matches!(e.kind, EdgeKind::TypeOf))
                    .map(|e| e.from)
                    .collect()
            })
            .unwrap_or_default()
    }
    
    // === Constraint Queries ===
    
    /// Get all preconditions for a function
    pub fn get_preconditions(&self, func: NodeId) -> Vec<&Constraint> {
        self.constraints.iter()
            .filter(|c| c.target == func)
            .filter(|c| matches!(c.kind, super::constraint::ConstraintKind::Precondition { .. }))
            .collect()
    }
    
    /// Get all postconditions for a function
    pub fn get_postconditions(&self, func: NodeId) -> Vec<&Constraint> {
        self.constraints.iter()
            .filter(|c| c.target == func)
            .filter(|c| matches!(c.kind, super::constraint::ConstraintKind::Postcondition { .. }))
            .collect()
    }
    
    // === Summary Queries ===
    
    /// Get summary statistics
    pub fn summary(&self) -> ModuleSummary {
        let functions = self.graph.functions().len();
        let types = self.graph.types().len();
        ModuleSummary {
            node_count: self.graph.node_count(),
            edge_count: self.graph.edge_count(),
            function_count: functions,
            type_count: types,
            constraint_count: self.constraints.len(),
        }
    }
}

/// Summary statistics for a module
#[derive(Debug, Clone)]
pub struct ModuleSummary {
    pub node_count: usize,
    pub edge_count: usize,
    pub function_count: usize,
    pub type_count: usize,
    pub constraint_count: usize,
}

// ==================== API Discoverability ====================

/// An operation that can be performed on a type
/// This is the core anti-hallucination feature: AI can query valid operations
#[derive(Debug, Clone)]
pub struct Operation {
    /// Operation name (method/function name)
    pub name: String,
    
    /// Parameter types
    pub params: Vec<OperationParam>,
    
    /// Return type (None = void)
    pub return_type: Option<String>,
    
    /// Effects (pure, io, alloc, etc.)
    pub effects: Vec<String>,
    
    /// Preconditions (requires clauses)
    pub preconditions: Vec<String>,
    
    /// Is this a method (first param is self)?
    pub is_method: bool,
    
    /// Source node ID for reference  
    pub source_node: NodeId,
}

/// A parameter in an operation
#[derive(Debug, Clone)]
pub struct OperationParam {
    pub name: String,
    pub type_name: String,
    pub ownership: String,  // "own", "ref", "mut", "shared"
}

/// Result of querying available operations
#[derive(Debug, Clone)]
pub struct OperationsResult {
    /// The type being queried
    pub type_name: String,
    
    /// Available operations
    pub operations: Vec<Operation>,
    
    /// Field access operations (for structs)
    pub field_accessors: Vec<FieldAccessor>,
}

/// A field that can be accessed on a type
#[derive(Debug, Clone)]
pub struct FieldAccessor {
    pub name: String,
    pub type_name: String,
    pub is_mutable: bool,
}

/// Query API for API Discoverability
impl AIIRModule {
    /// Get all available operations for a type (CORE ANTI-HALLUCINATION API)
    /// 
    /// This allows AI to query "what can I do with this type?" instead of
    /// hallucinating non-existent methods.
    pub fn get_available_operations(&self, type_name: &str) -> OperationsResult {
        let mut operations = Vec::new();
        let mut field_accessors = Vec::new();
        
        // Look up the type node
        if let Some(type_id) = self.graph.lookup(type_name) {
            if let Some(type_node) = self.graph.get_node(type_id) {
                // Extract field accessors if struct
                if let super::semantic_graph::NodeKind::Type { fields, .. } = &type_node.kind {
                    for (field_name, field_type) in fields {
                        field_accessors.push(FieldAccessor {
                            name: field_name.clone(),
                            type_name: field_type.clone(),
                            is_mutable: true, // Default, could be refined
                        });
                    }
                }
            }
        }
        
        // Find all functions that take this type as first parameter (methods)
        for func in self.graph.functions() {
            if let super::semantic_graph::NodeKind::Function { 
                params, 
                return_type, 
                effects,
                is_pure,
            } = &func.kind {
                // Check if this is a method on the type
                let is_method = params.first()
                    .map(|(_, pt)| {
                        pt == type_name || 
                        pt == &format!("ref {}", type_name) ||
                        pt == &format!("mut {}", type_name) ||
                        pt == &format!("own {}", type_name)
                    })
                    .unwrap_or(false);
                
                // Check if this is a constructor (returns the type)
                let is_constructor = return_type.as_ref()
                    .map(|rt| rt == type_name)
                    .unwrap_or(false);
                
                if is_method || is_constructor {
                    let op_params: Vec<OperationParam> = params.iter()
                        .skip(if is_method { 1 } else { 0 })
                        .map(|(name, type_str)| {
                            let (ownership, actual_type) = parse_ownership_type(type_str);
                            OperationParam {
                                name: name.clone(),
                                type_name: actual_type,
                                ownership,
                            }
                        })
                        .collect();
                    
                    let effect_strs: Vec<String> = if *is_pure {
                        vec!["pure".to_string()]
                    } else {
                        effects.effects.iter()
                            .map(|e| format!("{:?}", e).to_lowercase())
                            .collect()
                    };
                    
                    // Get preconditions
                    let preconditions: Vec<String> = self.get_preconditions(func.id)
                        .iter()
                        .map(|c| format!("{:?}", c.kind))
                        .collect();
                    
                    operations.push(Operation {
                        name: func.name.clone(),
                        params: op_params,
                        return_type: return_type.clone(),
                        effects: effect_strs,
                        preconditions,
                        is_method,
                        source_node: func.id,
                    });
                }
            }
        }
        
        OperationsResult {
            type_name: type_name.to_string(),
            operations,
            field_accessors,
        }
    }
    
    /// Get available operations as JSON (for AI consumption)
    pub fn get_available_operations_json(&self, type_name: &str) -> String {
        let result = self.get_available_operations(type_name);
        serde_json::to_string_pretty(&serde_json::json!({
            "type": result.type_name,
            "operations": result.operations.iter().map(|op| {
                serde_json::json!({
                    "name": op.name,
                    "params": op.params.iter().map(|p| {
                        format!("{}: {} {}", p.name, p.ownership, p.type_name)
                    }).collect::<Vec<_>>(),
                    "returns": op.return_type,
                    "effects": op.effects,
                    "preconditions": op.preconditions,
                    "is_method": op.is_method,
                })
            }).collect::<Vec<_>>(),
            "fields": result.field_accessors.iter().map(|f| {
                serde_json::json!({
                    "name": f.name,
                    "type": f.type_name,
                })
            }).collect::<Vec<_>>(),
        })).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Helper: parse "ref Point" into ("ref", "Point")
fn parse_ownership_type(type_str: &str) -> (String, String) {
    for prefix in ["own ", "ref ", "mut ", "shared "] {
        if type_str.starts_with(prefix) {
            return (
                prefix.trim().to_string(), 
                type_str[prefix.len()..].to_string()
            );
        }
    }
    ("own".to_string(), type_str.to_string())
}
