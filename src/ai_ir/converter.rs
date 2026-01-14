//! AST to AI-IR Converter
//!
//! Converts the typed AST into an AI-IR representation for AI consumption.

use crate::frontend::ast::*;
use super::*;

/// Converter from AST to AI-IR
pub struct AIIRConverter {
    module: AIIRModule,
    next_constraint_id: usize,
}

impl AIIRConverter {
    /// Create a new converter
    pub fn new(module_name: String) -> Self {
        Self {
            module: AIIRModule::new(module_name),
            next_constraint_id: 0,
        }
    }
    
    /// Convert a program to AI-IR
    pub fn convert(mut self, program: &Program) -> AIIRModule {
        for item in &program.items {
            self.convert_item(item);
        }
        self.module
    }
    
    /// Convert a single item
    fn convert_item(&mut self, item: &Item) {
        match item {
            Item::Function(func) => self.convert_function(func),
            Item::Struct(s) => self.convert_struct(s),
            Item::Enum(e) => self.convert_enum(e),
            _ => {} // TODO: Impl, Interface, Const
        }
    }
    
    /// Convert a function to AI-IR nodes and edges
    fn convert_function(&mut self, func: &Function) {
        // Create function node
        let params: Vec<(String, String)> = func.params.iter()
            .map(|p| (p.name.name.clone(), format!("{:?}", p.ty)))
            .collect();
        
        let func_id = self.module.graph.add_node(
            NodeKind::Function {
                params,
                return_type: func.ret_type.as_ref().map(|t| format!("{:?}", t)),
                effects: func.effects.clone(),
                is_pure: func.effects.is_pure,
            },
            func.name.name.clone(),
            func.span,
        );
        
        // Convert contracts to constraints
        for contract in &func.contracts {
            let constraint_id = ConstraintId(self.next_constraint_id);
            self.next_constraint_id += 1;
            
            let constraint = match contract.kind {
                ContractKind::Requires => Constraint::precondition(
                    constraint_id,
                    func_id,
                    format!("{:?}", contract.condition),
                    contract.span,
                ),
                ContractKind::Ensures => Constraint::postcondition(
                    constraint_id,
                    func_id,
                    format!("{:?}", contract.condition),
                    contract.span,
                ),
                ContractKind::Invariant => Constraint {
                    id: constraint_id,
                    target: func_id,
                    kind: ConstraintKind::Invariant { 
                        expr: format!("{:?}", contract.condition) 
                    },
                    source: ConstraintSource::Explicit { span: contract.span },
                    verification: VerificationStrategy::Hybrid,
                },
            };
            self.module.constraints.push(constraint);
        }
        
        // Analyze function body for call edges
        self.analyze_block(&func.body, func_id);
    }
    
    /// Convert a struct to AI-IR
    fn convert_struct(&mut self, s: &StructDef) {
        let fields: Vec<(String, String)> = s.fields.iter()
            .map(|f| (f.name.name.clone(), format!("{:?}", f.ty)))
            .collect();
        
        let _struct_id = self.module.graph.add_node(
            NodeKind::Type {
                type_kind: TypeNodeKind::Struct,
                fields,
            },
            s.name.name.clone(),
            s.span,
        );
    }
    
    /// Convert an enum to AI-IR
    fn convert_enum(&mut self, e: &EnumDef) {
        let fields: Vec<(String, String)> = e.variants.iter()
            .map(|v| (v.name.name.clone(), "variant".to_string()))
            .collect();
        
        let _enum_id = self.module.graph.add_node(
            NodeKind::Type {
                type_kind: TypeNodeKind::Enum,
                fields,
            },
            e.name.name.clone(),
            e.span,
        );
    }
    
    /// Analyze a block for call edges
    fn analyze_block(&mut self, block: &Block, parent_func: NodeId) {
        for stmt in &block.stmts {
            self.analyze_stmt(stmt, parent_func);
        }
    }
    
    /// Analyze a statement for call edges  
    fn analyze_stmt(&mut self, stmt: &Stmt, parent_func: NodeId) {
        match stmt {
            Stmt::Expr(expr) => self.analyze_expr(expr, parent_func),
            Stmt::Let { value, .. } => {
                if let Some(v) = value {
                    self.analyze_expr(v, parent_func);
                }
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    self.analyze_expr(v, parent_func);
                }
            }
            _ => {}
        }
    }
    
    /// Analyze an expression for call edges
    fn analyze_expr(&mut self, expr: &Expr, parent_func: NodeId) {
        match expr {
            Expr::Call { func, args, .. } => {
                // If calling a named function, create a Calls edge
                if let Expr::Ident(ident) = func.as_ref() {
                    if let Some(callee_id) = self.module.graph.lookup(&ident.name) {
                        self.module.graph.add_edge(parent_func, callee_id, EdgeKind::Calls);
                    }
                }
                // Analyze arguments
                for arg in args {
                    self.analyze_expr(arg, parent_func);
                }
            }
            Expr::Binary { left, right, .. } => {
                self.analyze_expr(left, parent_func);
                self.analyze_expr(right, parent_func);
            }
            Expr::Unary { expr: inner, .. } => {
                self.analyze_expr(inner, parent_func);
            }
            Expr::If { cond, then_block, else_block, .. } => {
                self.analyze_expr(cond, parent_func);
                self.analyze_block(then_block, parent_func);
                if let Some(eb) = else_block {
                    self.analyze_block(eb, parent_func);
                }
            }
            Expr::Block(b) => self.analyze_block(b, parent_func),
            Expr::Try { expr, .. } => self.analyze_expr(expr, parent_func),
            _ => {}
        }
    }
}
