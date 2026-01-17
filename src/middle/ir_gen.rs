//! IR Generator - AST to Aether IR
//!
//! Converts the typed AST into three-address code IR.
#![allow(dead_code)]

use std::collections::HashMap;
use crate::frontend::ast::{
    self, Program, Item, Stmt, Expr, Type as AstType,
};
use crate::middle::ir::{
    IRModule, IRFunction, IRType, BlockId, Register,
    Instruction, Terminator, Value, Constant, UnaryOp,
    BinOp as IRBinOp, IRAsmOperand, IRAsmOperandKind, IRExtern,
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
    /// Variable to value mapping (with type)
    locals: HashMap<String, (Value, IRType)>,
    /// Register type mapping (for temporaries)
    reg_types: HashMap<Register, IRType>,
    /// Struct definitions (name -> fields)
    struct_defs: HashMap<String, Vec<(String, IRType)>>,
}

impl IRGenerator {
    pub fn new(module_name: &str) -> Self {
        Self {
            module: IRModule::new(module_name),
            current_fn: None,
            current_block: BlockId(0),
            next_register: 0,
            locals: HashMap::new(),
            reg_types: HashMap::new(),
            struct_defs: HashMap::new(),
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
            Item::Struct(struct_def) => {
                let fields: Vec<_> = struct_def.fields.iter()
                    .map(|f| (f.name.name.clone(), self.ast_type_to_ir(&f.ty)))
                    .collect();
                
                // Extract repr from annotations
                let mut repr = crate::middle::ir::StructRepr::Default;
                for ann in &struct_def.annotations {
                    if ann.name.name == "repr" {
                        // Check annotation args for C, packed, transparent
                        if !ann.args.is_empty() {
                            if let Expr::Ident(ident) = &ann.args[0] {
                                match ident.name.as_str() {
                                    "C" => repr = crate::middle::ir::StructRepr::C,
                                    "packed" => repr = crate::middle::ir::StructRepr::Packed,
                                    "transparent" => repr = crate::middle::ir::StructRepr::Transparent,
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                
                self.struct_defs.insert(struct_def.name.name.clone(), fields.clone());
                self.module.add_struct(&struct_def.name.name, fields, repr);
                Ok(())
            }
            Item::Enum(_) => Ok(()),
            Item::Impl(impl_block) => {
                let type_name = &impl_block.target.name;
                for method in &impl_block.methods {
                    self.generate_method(type_name, method)?;
                }
                Ok(())
            }
            Item::Interface(_) => Ok(()),
            Item::Const(_) => Ok(()),
            // Macro/Module/Use are handled at earlier compilation stages
            Item::Macro(_) => Ok(()),
            Item::Module(m) => {
                // Recursively generate module items
                if let Some(items) = &m.items {
                    for item in items {
                        self.generate_item(item)?;
                    }
                }
                Ok(())
            }
            Item::Use(_) => Ok(()),
            Item::Extern(ext) => {
                // Register extern functions in IR module
                for foreign_item in &ext.items {
                    match foreign_item {
                        ast::ForeignItem::Fn { name, params, ret_type, .. } => {
                            let ir_params: Vec<(String, IRType)> = params.iter()
                                .map(|p| (p.name.name.clone(), self.ast_type_to_ir(&p.ty)))
                                .collect();
                            let ir_ret = ret_type.as_ref()
                                .map(|t| self.ast_type_to_ir(t))
                                .unwrap_or(IRType::Void);
                            
                            self.module.externs.push(IRExtern {
                                name: name.name.clone(),
                                params: ir_params,
                                ret_type: ir_ret,
                            });
                        }
                        ast::ForeignItem::Static { .. } => {
                            // TODO: Handle static extern variables
                        }
                    }
                }
                Ok(())
            }
            Item::Static(_) => Ok(()), // TODO: Generate global variable IR
            Item::Union(_) => Ok(()), // TODO: Generate union type IR
        }
    }

    /// Generate IR for a method with Type_method naming convention
    fn generate_method(&mut self, type_name: &str, func: &ast::Function) -> Result<()> {
        // Create a modified function name with type prefix
        let prefixed_name = format!("{}_{}", type_name, func.name.name);
        self.generate_function_with_name(func, &prefixed_name)
    }

    /// Generate IR for a function with a specific name
    fn generate_function_with_name(&mut self, func: &ast::Function, name: &str) -> Result<()> {
        self.next_register = 0;
        self.locals.clear();
        self.reg_types.clear();

        // Convert parameters
        let params: Vec<(String, IRType)> = func.params.iter()
            .map(|p| (p.name.name.clone(), self.ast_type_to_ir(&p.ty)))
            .collect();
            
        let ret_type = if let Some(ref ty) = func.ret_type {
            self.ast_type_to_ir(ty)
        } else {
            IRType::Void
        };

        let mut ir_func = IRFunction::new(name, params.clone(), ret_type);
        let entry_block = ir_func.add_block("entry");
        self.current_block = entry_block;

        // Register parameters
        for (i, (param_name, ty)) in params.iter().enumerate() {
            let reg = self.alloc_register();
            
            // Assign param to register (pseudo-instruction for valid SSA start)
            if let Some(block) = ir_func.get_block_mut(entry_block) {
                block.push(Instruction::Assign {
                    dest: reg,
                    value: Value::Parameter(i),
                });
            }
            
            self.locals.insert(param_name.clone(), (Value::Register(reg), ty.clone()));
            self.reg_types.insert(reg, ty.clone());
        }

        self.current_fn = Some(ir_func);

        // Generate function body
        self.generate_block(&func.body)?;

        // Add implicit return if needed (same as generate_function)
        if let Some(ref mut ir_func) = self.current_fn {
            let ret_type = ir_func.ret_type.clone();
            if let Some(block) = ir_func.get_block_mut(self.current_block) {
                if block.terminator.is_none() {
                    // Only add return void for void functions
                    if ret_type == IRType::Void {
                        block.set_terminator(Terminator::Return { value: None });
                    } else {
                        // For non-void functions without explicit return, add unreachable
                        block.set_terminator(Terminator::Unreachable);
                    }
                }
            }
        }

        // Finalize function
        let ir_func = self.current_fn.take().unwrap();
        self.module.functions.push(ir_func);
        Ok(())
    }

    /// Generate IR for a function
    fn generate_function(&mut self, func: &ast::Function) -> Result<()> {
        self.next_register = 0;
        self.locals.clear();
        self.reg_types.clear();

        // Convert parameters
        let params: Vec<(String, IRType)> = func.params.iter()
            .map(|p| (p.name.name.clone(), self.ast_type_to_ir(&p.ty)))
            .collect();
            
        let ret_type = if let Some(ref ty) = func.ret_type {
            self.ast_type_to_ir(ty)
        } else {
            IRType::Void
        };

        let mut ir_func = IRFunction::new(&func.name.name, params.clone(), ret_type);
        let entry_block = ir_func.add_block("entry");
        self.current_block = entry_block;

        // Register parameters
        for (i, (name, ty)) in params.iter().enumerate() {
            let reg = self.alloc_register();
            
            // Assign param to register (pseudo-instruction for valid SSA start)
            if let Some(block) = ir_func.get_block_mut(entry_block) {
                block.push(Instruction::Assign {
                    dest: reg,
                    value: Value::Parameter(i),
                });
            }
            
            self.locals.insert(name.clone(), (Value::Register(reg), ty.clone()));
            self.reg_types.insert(reg, ty.clone());
        }

        self.current_fn = Some(ir_func);

        // Generate body
        self.generate_block(&func.body)?;

        // Add implicit return if needed
        if let Some(ref mut ir_func) = self.current_fn {
            let ret_type = ir_func.ret_type.clone();
            if let Some(block) = ir_func.get_block_mut(self.current_block) {
                if block.terminator.is_none() {
                    // Only add return void for void functions
                    if ret_type == IRType::Void {
                        block.set_terminator(Terminator::Return { value: None });
                    } else {
                        // For non-void functions without explicit return, add unreachable
                        block.set_terminator(Terminator::Unreachable);
                    }
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
            Stmt::Let { name, value, ty: type_annotation, .. } => {
                let reg = self.alloc_register();
                let mut var_type = IRType::I64;

                if let Some(expr) = value {
                    let val = self.generate_expr(expr)?;
                    if let Some(t) = self.get_value_type(&val) {
                        var_type = t;
                    }
                    self.emit_current(Instruction::Assign { dest: reg, value: val });
                    self.reg_types.insert(reg, var_type.clone());
                } else if let Some(ref ast_ty) = type_annotation {
                    var_type = self.ast_type_to_ir(ast_ty);
                }
                
                self.locals.insert(name.name.clone(), (Value::Register(reg), var_type));
                Ok(None)
            }

            Stmt::Expr(expr) => {
                let val = self.generate_expr(expr)?;
                Ok(Some(val))
            }

            Stmt::Return { value, .. } => {
                let ret_val = if let Some(expr) = value {
                    let mut val = self.generate_expr(expr)?;
                    
                    // Convert to function return type if needed
                    if let Some(func) = &self.current_fn {
                        let expected_ty = func.ret_type.clone();
                        if let Some(actual_ty) = self.get_value_type(&val) {
                            if Self::is_integer_type(&expected_ty) && Self::is_integer_type(&actual_ty) && expected_ty != actual_ty {
                                let cast_dest = self.alloc_register();
                                self.emit_current_with_type(Instruction::Cast {
                                    dest: cast_dest,
                                    value: val,
                                    ty: expected_ty.clone(),
                                }, expected_ty);
                                val = Value::Register(cast_dest);
                            }
                        }
                    }
                    Some(val)
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
                if let Some((val, _ty)) = self.locals.get(&ident.name) {
                    Ok(val.clone())
                } else {
                    Ok(Value::Global(ident.name.clone()))
                }

            }

            Expr::Path { segments, .. } => {
                // Return global path name (e.g. "Option_Some")
                // Phase 11: Simple flattening
                let path_str = segments.iter().map(|s| s.name.clone()).collect::<Vec<_>>().join("_");
                Ok(Value::Global(path_str))
            }

            Expr::Binary { left, op, right, .. } => {
                let left_val = self.generate_expr(left)?;
                let right_val = self.generate_expr(right)?;
                
                // Handle assignment specially
                if matches!(op, ast::BinOp::Assign) {
                    // 1. Assign to Variable
                    if let Expr::Ident(ident) = left.as_ref() {
                        if let Some((dest_val, _)) = self.locals.get(&ident.name) {
                            if let Value::Register(reg) = dest_val {
                                self.emit_current(Instruction::Assign {
                                    dest: *reg,
                                    value: right_val.clone(),
                                });
                                return Ok(right_val);
                            }
                        }
                    } 
                    // 2. Assign to Field
                    else if let Expr::Field { expr: base, field, .. } = left.as_ref() {
                         // We need the address of the field (LValue)
                         let base_val = self.generate_expr(base)?;
                         let base_ty = self.get_value_type(&base_val);
                         
                         if let Some(IRType::Ptr(inner)) = base_ty {
                            if let IRType::Struct(struct_name) = *inner {
                                 let fields = self.struct_defs.get(&struct_name).cloned()
                                     .ok_or_else(|| crate::utils::Error::UndefinedType { 
                                         span: crate::utils::Span::dummy(),
                                         name: struct_name.clone() 
                                     })?;
                                 
                                 let (idx, (_, field_ty)) = fields.iter().enumerate()
                                     .find(|(_, (n, _))| n == &field.name)
                                     .ok_or_else(|| crate::utils::Error::UnknownField { 
                                         span: crate::utils::Span::dummy(),
                                         field: field.name.clone(),
                                     })?;
                                     
                                 let field_ty = field_ty.clone();
                                     
                                 let dest = self.alloc_register();
                                 let idx_val = Value::Constant(Constant::Int(idx as i64));
                                 
                                 self.emit_current_with_type(Instruction::GetElementPtr {
                                     dest,
                                     ptr: base_val,
                                     index: idx_val,
                                 }, IRType::Ptr(Box::new(field_ty.clone())));
                                 
                                 // Store directly to field pointer
                                 self.emit_current(Instruction::Store {
                                     ptr: Value::Register(dest),
                                     value: right_val.clone(),
                                 });
                                 
                                 return Ok(right_val);
                            }
                         }
                    }
                    // 3. Assign to Deref (*ptr = val)
                    else if let Expr::Deref { expr: ptr_expr, .. } = left.as_ref() {
                        let ptr_val = self.generate_expr(ptr_expr)?;
                        self.emit_current(Instruction::Store {
                            ptr: ptr_val,
                            value: right_val.clone(),
                        });
                        return Ok(right_val);
                    }
                    
                    // 4. Fallback: If we get here with Assign, the target is not in locals
                    // This can happen with re-assignment to variables. Handle by storing to the register.
                    if let Expr::Ident(ident) = left.as_ref() {
                        // Variable exists but not in locals - likely needs alloca
                        return Err(crate::utils::Error::CodeGen(
                            format!("Cannot assign to '{}': variable not found in locals. Consider using 'let mut' for mutable variables.", ident.name)
                        ));
                    }
                    
                    // For any other Assign target, return an error
                    return Err(crate::utils::Error::CodeGen(
                        "Invalid assignment target".to_string()
                    ));
                }
                
                let ir_op = self.ast_binop_to_ir(*op);
                let dest = self.alloc_register();
                
                // Unify types for binary operations: convert right to left's type if different integers
                let left_ty = self.get_value_type(&left_val);
                let right_ty = self.get_value_type(&right_val);
                let unified_right = if let (Some(lt), Some(rt)) = (&left_ty, &right_ty) {
                    if Self::is_integer_type(lt) && Self::is_integer_type(rt) && lt != rt {
                        // Cast right to left's type
                        let cast_dest = self.alloc_register();
                        self.emit_current_with_type(Instruction::Cast {
                            dest: cast_dest,
                            value: right_val.clone(),
                            ty: lt.clone(),
                        }, lt.clone());
                        Value::Register(cast_dest)
                    } else {
                        right_val
                    }
                } else {
                    right_val
                };
                
                // Result of binary op is usually primitive or bool (I64/Bool)
                let res_ty = match ir_op {
                    IRBinOp::Eq | IRBinOp::Ne | IRBinOp::Lt | IRBinOp::Le | IRBinOp::Gt | IRBinOp::Ge => IRType::Bool,
                    _ => left_ty.unwrap_or(IRType::I64), // Use left type for arithmetic
                };

                self.emit_current_with_type(Instruction::BinOp {
                    dest,
                    op: ir_op,
                    left: left_val,
                    right: unified_right,
                }, res_ty);
                
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
                let ty = self.get_value_type(&val).unwrap_or(IRType::I64);
                self.emit_current_with_type(Instruction::UnaryOp { dest, op: ir_op, value: val }, ty);
                Ok(Value::Register(dest))
            }

            Expr::Call { func, args, .. } => {
                let func_name = if let Expr::Ident(ident) = func.as_ref() {
                    ident.name.clone()
                } else if let Expr::Path { segments, .. } = func.as_ref() {
                    segments.iter().map(|s| s.name.clone()).collect::<Vec<_>>().join("_")
                } else {
                    let _val = self.generate_expr(func)?;
                    "indirect".to_string()
                };

                // Generate argument values
                let mut arg_vals: Vec<Value> = Vec::new();
                
                // Look up function signature for type conversion
                let param_types: Vec<IRType> = self.module.functions.iter()
                    .find(|f| f.name == func_name)
                    .map(|f| f.params.iter().map(|(_, ty)| ty.clone()).collect())
                    .or_else(|| {
                        self.module.externs.iter()
                            .find(|e| e.name == func_name)
                            .map(|e| e.params.iter().map(|(_, ty)| ty.clone()).collect())
                    })
                    .unwrap_or_default();
                
                for (i, arg) in args.iter().enumerate() {
                    let mut val = self.generate_expr(arg)?;
                    
                    // If we have type info, check and convert if needed
                    if let (Some(expected_ty), Some(actual_ty)) = (param_types.get(i), self.get_value_type(&val)) {
                        // Allow implicit integer conversions (e.g., i64 -> i32)
                        if Self::is_integer_type(expected_ty) && Self::is_integer_type(&actual_ty) && expected_ty != &actual_ty {
                            let dest = self.alloc_register();
                            self.emit_current_with_type(Instruction::Cast {
                                dest,
                                value: val,
                                ty: expected_ty.clone(),
                            }, expected_ty.clone());
                            val = Value::Register(dest);
                        }
                    }
                    arg_vals.push(val);
                }

                // Get return type from function signature if available
                let ret_type = self.module.functions.iter()
                    .find(|f| f.name == func_name)
                    .map(|f| f.ret_type.clone())
                    .or_else(|| {
                        self.module.externs.iter()
                            .find(|e| e.name == func_name)
                            .map(|e| e.ret_type.clone())
                    })
                    .unwrap_or(IRType::I64);
                
                // For void functions, don't allocate a dest register
                if ret_type == IRType::Void {
                    self.emit_current_with_type(Instruction::Call {
                        dest: None,
                        func: func_name,
                        args: arg_vals,
                    }, IRType::Void);
                    Ok(Value::Unit)
                } else {
                    let dest = self.alloc_register();
                    self.emit_current_with_type(Instruction::Call {
                        dest: Some(dest),
                        func: func_name,
                        args: arg_vals,
                    }, ret_type);
                    Ok(Value::Register(dest))
                }
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

                self.current_block = then_id;
                let then_result = self.generate_block(then_block)?;
                
                let then_jumps_to_merge = if self.get_current_terminator().is_none() {
                    self.set_terminator_current(Terminator::Jump { target: merge_id });
                    true
                } else { false };
                let then_exit = self.current_block;

                self.current_block = else_id;
                let else_result = if let Some(eb) = else_block {
                    self.generate_block(eb)?
                } else { None }; // Void if no else
                
                let else_jumps_to_merge = if self.get_current_terminator().is_none() {
                    self.set_terminator_current(Terminator::Jump { target: merge_id });
                    true
                } else { false };
                let else_exit = self.current_block;

                self.current_block = merge_id;
                
                // Phi node logic - only generate if BOTH branches jump to merge AND both have values
                // For statement-level if (no else or no return value), just return Unit
                if then_jumps_to_merge && else_jumps_to_merge {
                    if let (Some(then_val), Some(else_val)) = (&then_result, &else_result) {
                        let dest = self.alloc_register();
                        let mut incoming = Vec::new();
                        let mut phi_ty = IRType::Void;
                        
                        incoming.push((then_val.clone(), then_exit));
                        if let Some(t) = self.get_value_type(then_val) { phi_ty = t; }
                        
                        incoming.push((else_val.clone(), else_exit));
                        
                        self.emit_current_with_type(Instruction::Phi { dest, incoming }, phi_ty);
                        return Ok(Value::Register(dest));
                    }
                }
                Ok(Value::Unit)
            }

            Expr::Block(block) => {
                if let Some(val) = self.generate_block(block)? {
                    Ok(val)
                } else {
                    Ok(Value::Unit)
                }
            }

            Expr::Field { expr: base, field, .. } => {
                let base_val = self.generate_expr(base)?;
                let base_ty = self.get_value_type(&base_val);
                
                if let Some(IRType::Ptr(inner)) = base_ty {
                    if let IRType::Struct(struct_name) = *inner {
                         let fields = self.struct_defs.get(&struct_name).cloned()
                             .ok_or_else(|| crate::utils::Error::UndefinedType { 
                                 span: crate::utils::Span::dummy(),
                                 name: struct_name.clone() 
                             })?;
                         
                         let (idx, (_, field_ty)) = fields.iter().enumerate()
                             .find(|(_, (n, _))| n == &field.name)
                             .ok_or_else(|| crate::utils::Error::UnknownField { 
                                 span: crate::utils::Span::dummy(),
                                 field: field.name.clone(),
                             })?;
                             
                         let field_ty = field_ty.clone();
                             
                         let dest = self.alloc_register();
                         self.emit_current_with_type(Instruction::GetElementPtr {
                             dest,
                             ptr: base_val,
                             index: Value::Constant(Constant::Int(idx as i64)),
                         }, IRType::Ptr(Box::new(field_ty.clone())));
                         
                         // If field is a struct, return the pointer (for chained field access)
                         // Otherwise load the value
                         if let IRType::Struct(_) = &field_ty {
                             // Return pointer to nested struct
                             return Ok(Value::Register(dest));
                         } else {
                             // Load primitive/pointer field value
                             let load_dest = self.alloc_register();
                             self.emit_current_with_type(Instruction::Load {
                                 dest: load_dest,
                                 ptr: Value::Register(dest),
                             }, field_ty.clone());
                             
                             return Ok(Value::Register(load_dest));
                         }
                    }
                }
                Ok(Value::Unit) // Error handling fallback
            }

            Expr::StructLit { name, fields, .. } => {
                let struct_type = IRType::Struct(name.name.clone());
                let ptr = self.alloc_register();
                self.emit_current_with_type(Instruction::Alloca { dest: ptr, ty: struct_type.clone() }, 
                    IRType::Ptr(Box::new(struct_type.clone())));
                
                let struct_fields = self.struct_defs.get(&name.name).cloned().ok_or_else(|| crate::utils::Error::UndefinedType {
                    span: crate::utils::Span::dummy(), name: name.name.clone()
                })?;

                for (field_name, field_expr) in fields {
                     // Find index
                     let (idx, (_, field_ty)) = struct_fields.iter().enumerate()
                         .find(|(_, (n, _))| n == &field_name.name)
                         .ok_or_else(|| crate::utils::Error::UnknownField {
                              span: crate::utils::Span::dummy(),
                              field: field_name.name.clone(),
                          })?;
                          
                     let field_val = self.generate_expr(field_expr)?;
                     let field_ptr = self.alloc_register();
                     
                     self.emit_current_with_type(Instruction::GetElementPtr {
                         dest: field_ptr,
                         ptr: Value::Register(ptr),
                         index: Value::Constant(Constant::Int(idx as i64)),
                     }, IRType::Ptr(Box::new(field_ty.clone())));
                     
                     self.emit_current(Instruction::Store {
                         ptr: Value::Register(field_ptr),
                         value: field_val,
                     });
                }
                Ok(Value::Register(ptr))
            }
            
            // Minimal implementations for others
            Expr::Loop { .. } => Ok(Value::Unit),
            Expr::While { .. } => Ok(Value::Unit),
            Expr::For { .. } => Ok(Value::Unit),
            Expr::Match { .. } => Ok(Value::Unit),
            Expr::Array { .. } => Ok(Value::Unit),
            Expr::Tuple { .. } => Ok(Value::Unit),
            Expr::MethodCall { expr: receiver, method, args, .. } => {
                 if method.name == "add" && args.len() == 1 {
                     let ptr_val = self.generate_expr(receiver)?;
                     let offset_val = self.generate_expr(&args[0])?;
                     
                     // Get pointer type
                     if let Some(IRType::Ptr(inner)) = self.get_value_type(&ptr_val) {
                         let dest = self.alloc_register();
                         // Result is same type as ptr
                         let result_ty = IRType::Ptr(inner.clone());
                         
                         self.emit_current_with_type(Instruction::GetElementPtr {
                             dest,
                             ptr: ptr_val,
                             index: offset_val,
                         }, result_ty);
                         
                         Ok(Value::Register(dest))
                     } else {
                         // Should not happen if Semantic pass passed
                         Ok(Value::Unit) 
                     }
                 } else {
                     Ok(Value::Unit)
                 }
            },
            Expr::Index { .. } => Ok(Value::Unit),
            Expr::Ref { .. } => Ok(Value::Unit),
            Expr::Deref { .. } => Ok(Value::Unit),
            Expr::Unsafe { body, .. } => Ok(self.generate_block(body)?.unwrap_or(Value::Unit)),
            Expr::Cast { expr, ty, .. } => {
                let val = self.generate_expr(expr)?;
                let dest = self.alloc_register();
                let target_ty = self.ast_type_to_ir(ty);
                self.emit_current_with_type(Instruction::Cast {
                    dest,
                    value: val,
                    ty: target_ty.clone(),
                }, target_ty);
                Ok(Value::Register(dest))
            },
            Expr::Range { .. } => Ok(Value::Unit),
            Expr::Asm { template, operands, .. } => {
                let mut ir_operands = Vec::new();
                for op in operands {
                     let input = if let Some(expr) = &op.expr {
                         if matches!(op.kind, ast::AsmOperandKind::Input | ast::AsmOperandKind::InOut) {
                             Some(self.generate_expr(expr)?)
                         } else {
                             None
                         }
                     } else {
                         None
                     };
                     
                     let output = if matches!(op.kind, ast::AsmOperandKind::Output | ast::AsmOperandKind::InOut) {
                          Some(self.alloc_register())
                     } else {
                          None
                     };
                     
                     let kind = match op.kind {
                         ast::AsmOperandKind::Input => IRAsmOperandKind::Input,
                         ast::AsmOperandKind::Output => IRAsmOperandKind::Output,
                         ast::AsmOperandKind::InOut => IRAsmOperandKind::InOut,
                         ast::AsmOperandKind::Clobber => IRAsmOperandKind::Clobber,
                     };
                     
                     ir_operands.push(IRAsmOperand {
                         kind,
                         constraint: op.options.clone(),
                         input,
                         output,
                     });
                }
                
                self.emit_current(Instruction::InlineAsm {
                    template: template.clone(),
                    operands: ir_operands.clone(), // Clone needed because we iterate again? No, we can iterate ast operands and match indices
                });

                // Post-ASM assignments (update variables from output registers)
                for (i, op) in operands.iter().enumerate() {
                     let ir_op = &ir_operands[i];
                     if let Some(reg) = ir_op.output {
                          if let Some(expr) = &op.expr {
                              if let Expr::Ident(ident) = expr {
                                  if let Some((dest_val, _)) = self.locals.get(&ident.name) {
                                      if let Value::Register(dest_reg) = dest_val {
                                           self.emit_current(Instruction::Assign {
                                               dest: *dest_reg,
                                               value: Value::Register(reg)
                                           });
                                      }
                                  }
                              }
                          }
                     }
                }
                
                Ok(Value::Unit)
            }
            Expr::Try { expr, .. } => {
                // Basic error propagation (if err return err, else value)
                // For now just evaluate expression
                self.generate_expr(expr)
            }
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
    
    fn emit_current_with_type(&mut self, inst: Instruction, ty: IRType) {
        if let Instruction::Assign { dest, .. } | 
               Instruction::BinOp { dest, .. } | 
               Instruction::UnaryOp { dest, .. } | 
               Instruction::Call { dest: Some(dest), .. } | 
               Instruction::Alloca { dest, .. } | 
               Instruction::Load { dest, .. } | 
               Instruction::GetElementPtr { dest, .. } | 
               Instruction::Cast { dest, .. } |
               Instruction::Phi { dest, .. } = &inst {
            self.reg_types.insert(*dest, ty);
        }
        self.emit_current(inst);
    }
    
    /// Check if an IR type is an integer type
    fn is_integer_type(ty: &IRType) -> bool {
        matches!(ty, IRType::I8 | IRType::I16 | IRType::I32 | IRType::I64 |
                     IRType::U8 | IRType::U16 | IRType::U32 | IRType::U64)
    }
    
    fn get_value_type(&self, val: &Value) -> Option<IRType> {
        match val {
            Value::Register(reg) => self.reg_types.get(reg).cloned(),
            Value::Parameter(idx) => {
                if let Some(func) = &self.current_fn {
                     if *idx < func.params.len() {
                         return Some(func.params[*idx].1.clone());
                     }
                }
                None
            },
            Value::Constant(c) => Some(match c {
                Constant::Int(_) => IRType::I64,
                Constant::Float(_) => IRType::F64,
                Constant::Bool(_) => IRType::Bool,
                Constant::String(_) => IRType::Ptr(Box::new(IRType::U8)),
                Constant::Null => IRType::Ptr(Box::new(IRType::Void)),
            }),
            Value::Global(_) => Some(IRType::Ptr(Box::new(IRType::Void))), // Unknown global
            Value::Unit => Some(IRType::Void),
        }
    }

    fn set_terminator_current(&mut self, term: Terminator) {
        if let Some(ref mut func) = self.current_fn {
            if let Some(block) = func.get_block_mut(self.current_block) {
                block.set_terminator(term);
            }
        }
    }

    fn get_current_terminator(&self) -> Option<&Terminator> {
        if let Some(ref func) = self.current_fn {
            if let Some(block) = func.blocks.get(self.current_block.0) {
                return block.terminator.as_ref();
            }
        }
        None
    }
    
    fn ast_binop_to_ir(&self, op: ast::BinOp) -> IRBinOp {
        match op {
            ast::BinOp::Add => IRBinOp::Add,
            ast::BinOp::Sub => IRBinOp::Sub,
            ast::BinOp::Mul => IRBinOp::Mul,
            ast::BinOp::Div => IRBinOp::Div,
            ast::BinOp::Mod => IRBinOp::Mod,
            ast::BinOp::Eq => IRBinOp::Eq,
            ast::BinOp::Ne => IRBinOp::Ne,
            ast::BinOp::Lt => IRBinOp::Lt,
            ast::BinOp::Le => IRBinOp::Le,
            ast::BinOp::Gt => IRBinOp::Gt,
            ast::BinOp::Ge => IRBinOp::Ge,
            ast::BinOp::And => IRBinOp::And,
            ast::BinOp::Or => IRBinOp::Or,
            ast::BinOp::BitXor => IRBinOp::Xor,
            ast::BinOp::Shl => IRBinOp::Shl,
            ast::BinOp::Shr => IRBinOp::Shr,
            ast::BinOp::BitAnd => IRBinOp::And,
            ast::BinOp::BitOr => IRBinOp::Or,
            ast::BinOp::Assign 
            | ast::BinOp::AddAssign 
            | ast::BinOp::SubAssign 
            | ast::BinOp::MulAssign 
            | ast::BinOp::DivAssign => panic!("Assignment should be handled separately"),
        }
    }

    fn ast_type_to_ir(&self, ty: &AstType) -> IRType {
        match ty {
            AstType::Named(name, _) => {
                match name.as_str() {
                    "i8" => IRType::I8,
                    "i16" => IRType::I16,
                    "i32" => IRType::I32,
                    "i64" | "int" => IRType::I64,
                    "u8" | "byte" => IRType::U8,
                    "u16" => IRType::U16,
                    "u32" => IRType::U32,
                    "u64" => IRType::U64,
                    "f32" => IRType::F32,
                    "f64" | "float" => IRType::F64,
                    "bool" => IRType::Bool,
                    "void" | "()" => IRType::Void,
                    s => IRType::Struct(s.to_string()),
                }

            }
            AstType::Generic(name, args, _) => {
                // Phase 11: Basic monomorphization stub
                // Map Box<T> to Box for now (incorrect but compiles)
                // Real impl need to mangle names: Box_i64
                let mut mangled = name.clone();
                for arg in args {
                     if let AstType::Named(n, _) = arg {
                         mangled.push_str("_");
                         mangled.push_str(n);
                     }
                }
                IRType::Struct(mangled)
            }
            AstType::Pointer(inner, _) => IRType::Ptr(Box::new(self.ast_type_to_ir(inner))),
            AstType::Array { elem, size: _, .. } => {
                 // Array logic hack
                 IRType::Ptr(Box::new(self.ast_type_to_ir(elem))) 
            }
            AstType::Ref { inner, .. } => {
                // References are implemented as pointers at the IR level
                IRType::Ptr(Box::new(self.ast_type_to_ir(inner)))
            }
            AstType::Tuple(elements, _) => {
                if elements.is_empty() {
                    IRType::Void // Unit tuple ()
                } else {
                    IRType::Void // TODO: proper tuple support
                }
            }
            AstType::Unit(_) => IRType::Void,
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
