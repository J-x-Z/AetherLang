//! IR Generator - AST to Aether IR
//!
//! Converts the typed AST into three-address code IR.

use std::collections::HashMap;
use crate::frontend::ast::{
    self, Program, Item, Block, Stmt, Expr, Literal, Ident, Type as AstType,
    Function, Param, Ownership,
};
use crate::middle::ir::{
    IRModule, IRFunction, IRType, BlockId, Register,
    Instruction, Terminator, Value, Constant, UnaryOp,
    BinOp as IRBinOp,
};
use crate::utils::Result;

/// IR Generator
pub struct IRGenerator {
    /// Current module being built
    module: IRModule,
    /// Current function being built
    current_fn: Option<IRFunction>,
    /// Current block ID
    current_block: BlockId,
    /// Register counter
    next_register: usize,
    /// Variable to register mapping
    locals: HashMap<String, Register>,
}

impl IRGenerator {
    pub fn new(module_name: &str) -> Self {
        Self {
            module: IRModule::new(module_name),
            current_fn: None,
            current_block: BlockId(0),
            next_register: 0,
            locals: HashMap::new(),
        }
    }

    /// Generate IR for a program
    pub fn generate(&mut self, program: &Program) -> Result<IRModule> {
        for item in &program.items {
            self.generate_item(item)?;
        }
        Ok(self.module.clone())
    }

    /// Generate IR for a top-level item
    fn generate_item(&mut self, item: &Item) -> Result<()> {
        match item {
            Item::Function(func) => self.generate_function(func),
            Item::Struct(_) => Ok(()), // Structs don't generate IR directly
            Item::Enum(_) => Ok(()),   // Enums don't generate IR directly
            Item::Impl(impl_block) => {
                for method in &impl_block.methods {
                    self.generate_function(method)?;
                }
                Ok(())
            }
            Item::Interface(_) => Ok(()), // Interfaces don't generate IR
            Item::Const(_) => Ok(()),     // TODO: Handle constants
        }
    }

    /// Generate IR for a function
    fn generate_function(&mut self, func: &ast::Function) -> Result<()> {
        // Convert parameters
        let params: Vec<(String, IRType)> = func.params
            .iter()
            .map(|p| (p.name.name.clone(), self.ast_type_to_ir(&p.ty)))
            .collect();

        let ret_type = func.ret_type
            .as_ref()
            .map(|t| self.ast_type_to_ir(t))
            .unwrap_or(IRType::Void);

        // Create function
        let mut ir_func = IRFunction::new(&func.name.name, params.clone(), ret_type);
        
        // Reset state
        self.next_register = 0;
        self.locals.clear();
        self.current_block = ir_func.add_block("entry");

        // Add parameters to locals
        for (i, (name, _ty)) in params.iter().enumerate() {
            let reg = self.alloc_register();
            self.emit(&mut ir_func, Instruction::Assign {
                dest: reg,
                value: Value::Parameter(i),
            });
            self.locals.insert(name.clone(), reg);
        }

        self.current_fn = Some(ir_func);

        // Generate body
        self.generate_block(&func.body)?;

        // Add implicit return if needed
        if let Some(ref mut ir_func) = self.current_fn {
            if let Some(block) = ir_func.get_block_mut(self.current_block) {
                if block.terminator.is_none() {
                    block.set_terminator(Terminator::Return { value: None });
                }
            }
        }

        // Finalize function
        if let Some(ir_func) = self.current_fn.take() {
            self.module.functions.push(ir_func);
        }

        Ok(())
    }

    /// Generate IR for a block
    fn generate_block(&mut self, block: &ast::Block) -> Result<Option<Value>> {
        let mut last_value = None;
        for stmt in &block.stmts {
            last_value = self.generate_stmt(stmt)?;
        }
        Ok(last_value)
    }

    /// Generate IR for a statement
    fn generate_stmt(&mut self, stmt: &ast::Stmt) -> Result<Option<Value>> {
        match stmt {
            Stmt::Let { name, value, .. } => {
                let reg = self.alloc_register();
                
                if let Some(expr) = value {
                    let val = self.generate_expr(expr)?;
                    self.emit_current(Instruction::Assign { dest: reg, value: val });
                }
                
                self.locals.insert(name.name.clone(), reg);
                Ok(None)
            }

            Stmt::Expr(expr) => {
                let val = self.generate_expr(expr)?;
                Ok(Some(val))
            }

            Stmt::Return { value, .. } => {
                let ret_val = if let Some(expr) = value {
                    Some(self.generate_expr(expr)?)
                } else {
                    None
                };
                
                self.set_terminator_current(Terminator::Return { value: ret_val });
                Ok(None)
            }

            Stmt::Break { .. } => {
                // TODO: Track loop context for break
                Ok(None)
            }

            Stmt::Continue { .. } => {
                // TODO: Track loop context for continue
                Ok(None)
            }

            Stmt::Empty { .. } => Ok(None),
        }
    }

    /// Generate IR for an expression
    fn generate_expr(&mut self, expr: &ast::Expr) -> Result<Value> {
        match expr {
            Expr::Literal(lit) => Ok(self.generate_literal(lit)),

            Expr::Ident(ident) => {
                if let Some(&reg) = self.locals.get(&ident.name) {
                    Ok(Value::Register(reg))
                } else {
                    // Could be a global or function
                    Ok(Value::Global(ident.name.clone()))
                }
            }

            Expr::Binary { left, op, right, .. } => {
                let left_val = self.generate_expr(left)?;
                let right_val = self.generate_expr(right)?;
                
                // Handle assignment specially
                if matches!(op, ast::BinOp::Assign) {
                    if let Expr::Ident(ident) = left.as_ref() {
                        if let Some(&reg) = self.locals.get(&ident.name) {
                            self.emit_current(Instruction::Assign {
                                dest: reg,
                                value: right_val.clone(),
                            });
                            return Ok(right_val);
                        }
                    }
                }
                
                let ir_op = self.ast_binop_to_ir(*op);
                let dest = self.alloc_register();
                
                self.emit_current(Instruction::BinOp {
                    dest,
                    op: ir_op,
                    left: left_val,
                    right: right_val,
                });
                
                Ok(Value::Register(dest))
            }

            Expr::Unary { op, expr: inner, .. } => {
                let val = self.generate_expr(inner)?;
                let ir_op = match op {
                    ast::UnOp::Neg => UnaryOp::Neg,
                    ast::UnOp::Not => UnaryOp::Not,
                    ast::UnOp::BitNot => UnaryOp::BitNot,
                };
                
                let dest = self.alloc_register();
                self.emit_current(Instruction::UnaryOp { dest, op: ir_op, value: val });
                Ok(Value::Register(dest))
            }

            Expr::Call { func, args, .. } => {
                // Get function name
                let func_name = if let Expr::Ident(ident) = func.as_ref() {
                    ident.name.clone()
                } else {
                    // For complex expressions, generate them
                    let _func_val = self.generate_expr(func)?;
                    "indirect_call".to_string()
                };

                // Generate arguments
                let arg_vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.generate_expr(a))
                    .collect::<Result<Vec<_>>>()?;

                let dest = self.alloc_register();
                self.emit_current(Instruction::Call {
                    dest: Some(dest),
                    func: func_name,
                    args: arg_vals,
                });
                
                Ok(Value::Register(dest))
            }

            Expr::If { cond, then_block, else_block, .. } => {
                let cond_val = self.generate_expr(cond)?;

                let then_id = self.add_block("then");
                let else_id = self.add_block("else");
                let merge_id = self.add_block("merge");

                self.set_terminator_current(Terminator::Branch {
                    cond: cond_val,
                    then_target: then_id,
                    else_target: else_id,
                });

                // Generate then block
                self.current_block = then_id;
                let then_result = self.generate_block(then_block)?;
                
                let then_jumps_to_merge = if self.get_current_terminator().is_none() {
                    self.set_terminator_current(Terminator::Jump { target: merge_id });
                    true
                } else {
                    false
                };
                let then_exit = self.current_block;

                // Generate else block
                self.current_block = else_id;
                let else_result = if let Some(eb) = else_block {
                    self.generate_block(eb)?
                } else {
                    Some(Value::Unit)
                };
                
                let else_jumps_to_merge = if self.get_current_terminator().is_none() {
                    self.set_terminator_current(Terminator::Jump { target: merge_id });
                    true
                } else {
                    false
                };
                let else_exit = self.current_block;

                // Merge block with phi node
                self.current_block = merge_id;
                
                if then_result.is_some() || else_result.is_some() {
                    let dest = self.alloc_register();
                    let mut incoming = Vec::new();
                    
                    if then_jumps_to_merge {
                        if let Some(tv) = then_result {
                            incoming.push((tv, then_exit));
                        } else {
                            incoming.push((Value::Unit, then_exit));
                        }
                    }
                    
                    if else_jumps_to_merge {
                        if let Some(ev) = else_result {
                            incoming.push((ev, else_exit));
                        } else {
                            incoming.push((Value::Unit, else_exit));
                        }
                    }
                    
                    if !incoming.is_empty() {
                        self.emit_current(Instruction::Phi { dest, incoming });
                        return Ok(Value::Register(dest));
                    }
                }

                Ok(Value::Unit)

            }

            Expr::Loop { body, .. } => {
                let loop_id = self.add_block("loop");
                let exit_id = self.add_block("loop_exit");

                self.set_terminator_current(Terminator::Jump { target: loop_id });
                self.current_block = loop_id;

                self.generate_block(body)?;
                
                // If no break, loop forever
                if self.get_current_terminator().is_none() {
                    self.set_terminator_current(Terminator::Jump { target: loop_id });
                }

                self.current_block = exit_id;
                Ok(Value::Unit)
            }

            Expr::While { cond, body, .. } => {
                let cond_id = self.add_block("while_cond");
                let body_id = self.add_block("while_body");
                let exit_id = self.add_block("while_exit");

                self.set_terminator_current(Terminator::Jump { target: cond_id });

                // Condition check
                self.current_block = cond_id;
                let cond_val = self.generate_expr(cond)?;
                self.set_terminator_current(Terminator::Branch {
                    cond: cond_val,
                    then_target: body_id,
                    else_target: exit_id,
                });

                // Body
                self.current_block = body_id;
                self.generate_block(body)?;
                self.set_terminator_current(Terminator::Jump { target: cond_id });

                self.current_block = exit_id;
                Ok(Value::Unit)
            }

            Expr::Block(block) => {
                self.generate_block(block)?;
                Ok(Value::Unit)
            }

            Expr::Array { elements, .. } => {
                // For now, just return first element or unit
                if let Some(first) = elements.first() {
                    self.generate_expr(first)
                } else {
                    Ok(Value::Unit)
                }
            }

            Expr::Tuple { elements, .. } => {
                // For now, just return first element or unit
                if let Some(first) = elements.first() {
                    self.generate_expr(first)
                } else {
                    Ok(Value::Unit)
                }
            }

            // Placeholder implementations
            Expr::Field { expr, field, .. } => {
                let _base = self.generate_expr(expr)?;
                Ok(Value::Global(field.name.clone()))
            }

            Expr::MethodCall { expr, method, args, .. } => {
                let _receiver = self.generate_expr(expr)?;
                let _arg_vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.generate_expr(a))
                    .collect::<Result<Vec<_>>>()?;
                
                let dest = self.alloc_register();
                self.emit_current(Instruction::Call {
                    dest: Some(dest),
                    func: method.name.clone(),
                    args: Vec::new(), // TODO: Pass receiver and args
                });
                Ok(Value::Register(dest))
            }

            Expr::Index { expr, index, .. } => {
                let base = self.generate_expr(expr)?;
                let idx = self.generate_expr(index)?;
                
                let dest = self.alloc_register();
                self.emit_current(Instruction::GetElementPtr {
                    dest,
                    ptr: base,
                    index: idx,
                });
                
                let load_dest = self.alloc_register();
                self.emit_current(Instruction::Load {
                    dest: load_dest,
                    ptr: Value::Register(dest),
                });
                
                Ok(Value::Register(load_dest))
            }

            Expr::Ref { expr: inner, .. } => {
                self.generate_expr(inner)
            }

            Expr::Deref { expr: inner, .. } => {
                let ptr = self.generate_expr(inner)?;
                let dest = self.alloc_register();
                self.emit_current(Instruction::Load { dest, ptr });
                Ok(Value::Register(dest))
            }

            Expr::Unsafe { body, .. } => {
                self.generate_block(body)?;
                Ok(Value::Unit)
            }

            // Not yet implemented
            Expr::Match { .. } => Ok(Value::Unit),
            Expr::For { .. } => Ok(Value::Unit),
            Expr::StructLit { .. } => Ok(Value::Unit),
            Expr::Cast { .. } => Ok(Value::Unit),
            Expr::Range { .. } => Ok(Value::Unit),
            Expr::Asm { .. } => Ok(Value::Unit),
        }
    }

    /// Generate a constant value from a literal
    fn generate_literal(&self, lit: &ast::Literal) -> Value {
        match lit {
            ast::Literal::Int(n, _) => Value::Constant(Constant::Int(*n)),
            ast::Literal::Float(n, _) => Value::Constant(Constant::Float(*n)),
            ast::Literal::Bool(b, _) => Value::Constant(Constant::Bool(*b)),
            ast::Literal::String(s, _) => Value::Constant(Constant::String(s.clone())),
            ast::Literal::Char(c, _) => Value::Constant(Constant::Int(*c as i64)),
        }
    }

    // ==================== Helper Methods ====================

    fn alloc_register(&mut self) -> Register {
        let reg = Register(self.next_register);
        self.next_register += 1;
        reg
    }

    fn add_block(&mut self, label: &str) -> BlockId {
        if let Some(ref mut func) = self.current_fn {
            func.add_block(label)
        } else {
            BlockId(0)
        }
    }

    fn emit(&mut self, func: &mut IRFunction, inst: Instruction) {
        if let Some(block) = func.get_block_mut(self.current_block) {
            block.push(inst);
        }
    }

    fn emit_current(&mut self, inst: Instruction) {
        if let Some(ref mut func) = self.current_fn {
            if let Some(block) = func.get_block_mut(self.current_block) {
                block.push(inst);
            }
        }
    }

    fn set_terminator_current(&mut self, term: Terminator) {
        if let Some(ref mut func) = self.current_fn {
            if let Some(block) = func.get_block_mut(self.current_block) {
                block.set_terminator(term);
            }
        }
    }

    fn get_current_terminator(&self) -> Option<Terminator> {
        if let Some(ref func) = self.current_fn {
            if let Some(block) = func.blocks.get(self.current_block.0) {
                return block.terminator.clone();
            }
        }
        None
    }

    fn ast_binop_to_ir(&self, op: ast::BinOp) -> IRBinOp {
        match op {
            ast::BinOp::Add | ast::BinOp::AddAssign => IRBinOp::Add,
            ast::BinOp::Sub | ast::BinOp::SubAssign => IRBinOp::Sub,
            ast::BinOp::Mul | ast::BinOp::MulAssign => IRBinOp::Mul,
            ast::BinOp::Div | ast::BinOp::DivAssign => IRBinOp::Div,
            ast::BinOp::Mod => IRBinOp::Mod,
            ast::BinOp::Eq => IRBinOp::Eq,
            ast::BinOp::Ne => IRBinOp::Ne,
            ast::BinOp::Lt => IRBinOp::Lt,
            ast::BinOp::Le => IRBinOp::Le,
            ast::BinOp::Gt => IRBinOp::Gt,
            ast::BinOp::Ge => IRBinOp::Ge,
            ast::BinOp::And => IRBinOp::And,
            ast::BinOp::Or => IRBinOp::Or,
            ast::BinOp::BitAnd => IRBinOp::And,
            ast::BinOp::BitOr => IRBinOp::Or,
            ast::BinOp::BitXor => IRBinOp::Xor,
            ast::BinOp::Shl => IRBinOp::Shl,
            ast::BinOp::Shr => IRBinOp::Shr,
            ast::BinOp::Assign => IRBinOp::Add, // Handled specially
        }

    }

    fn ast_type_to_ir(&self, ty: &ast::Type) -> IRType {
        match ty {
            ast::Type::Named(name, _) => match name.as_str() {
                "i8" => IRType::I8,
                "i16" => IRType::I16,
                "i32" => IRType::I32,
                "i64" | "isize" => IRType::I64,
                "u8" => IRType::U8,
                "u16" => IRType::U16,
                "u32" => IRType::U32,
                "u64" | "usize" => IRType::U64,
                "f32" => IRType::F32,
                "f64" => IRType::F64,
                "bool" => IRType::Bool,
                _ => IRType::Struct(name.clone()),
            },
            ast::Type::Pointer(inner, _) => IRType::Ptr(Box::new(self.ast_type_to_ir(inner))),
            ast::Type::Ref { inner, .. } => IRType::Ptr(Box::new(self.ast_type_to_ir(inner))),
            ast::Type::Array { elem, size, .. } => {
                IRType::Array(Box::new(self.ast_type_to_ir(elem)), *size)
            }
            ast::Type::Unit(_) => IRType::Void,
            _ => IRType::Void,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::lexer::Lexer;
    use crate::frontend::parser::Parser;

    fn generate(source: &str) -> Result<IRModule> {
        let lexer = Lexer::new(source, 0);
        let mut parser = Parser::new(lexer);
        let program = parser.parse_program()?;
        let mut gen = IRGenerator::new("test");
        gen.generate(&program)
    }

    #[test]
    fn test_empty_function() {
        let module = generate("fn main() {}").unwrap();
        assert_eq!(module.functions.len(), 1);
        assert_eq!(module.functions[0].name, "main");
    }

    #[test]
    fn test_return_constant() {
        let module = generate("fn foo() -> i32 { return 42 }").unwrap();
        assert_eq!(module.functions.len(), 1);
        assert!(!module.functions[0].blocks.is_empty());
    }

    #[test]
    fn test_binary_expression() {
        let module = generate("fn add() -> i32 { return 1 + 2 }").unwrap();
        assert_eq!(module.functions.len(), 1);
    }

    #[test]
    fn test_if_expression() {
        let module = generate("fn test() { if true { return 1 } else { return 0 } }").unwrap();
        assert_eq!(module.functions.len(), 1);
        // Should have entry, then, else, merge blocks
        assert!(module.functions[0].blocks.len() >= 3);
    }
}
