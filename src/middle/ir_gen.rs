//! IR Generator - AST to Aether IR
//!
//! Converts the typed AST into three-address code IR.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use crate::frontend::ast::{
    self, Program, Item, Stmt, Expr, Type as AstType,
};
use crate::middle::ir::{
    IRModule, IRFunction, IRType, BlockId, Register,
    Instruction, Terminator, Value, Constant, UnaryOp, IRGlobal, IRInitializer, IRLayoutQuery,
    BinOp as IRBinOp, IRAsmOperand, IRAsmOperandKind, IRExtern,
};
use crate::utils::Result;
use crate::frontend::semantic::{SemanticFacts, ScalarConstant, TypedInitializer, InitializerValue, ResolvedMethodCall, ReceiverAdjustment};
use crate::types::type_system::{ResolvedType, PrimitiveType};

#[path = "ir_arrays.rs"]
mod arrays;
#[path = "ir_enum.rs"]
mod enums;
#[path = "ir_memory.rs"]
mod memory;
#[path = "ir_simd.rs"]
mod simd;
#[path = "ir_methods.rs"]
mod methods;
#[path = "ir_contracts.rs"]
mod contracts;
#[path = "ir_tuple.rs"]
mod tuples;
#[path = "ir_owned.rs"]
mod owned;
#[path = "ir_host.rs"]
mod host;

/// IR Generator
pub struct IRGenerator {
    owned: owned::OwnedScopes,
    emission_error: Option<crate::utils::Error>,
    semantic_facts: Option<SemanticFacts>,
    nominal_types: HashSet<String>,
    resolved_aliases: HashMap<String, IRType>,
    constants: HashMap<String, (IRType, Constant)>,
    /// Current module being built
    module: IRModule,
    /// Current function being built
    current_fn: Option<IRFunction>,
    /// Current block ID
    current_block: BlockId,
    /// Innermost loop (continue target, break target).
    loop_targets: Vec<(BlockId, BlockId)>,
    /// Register counter
    next_register: usize,
    /// Variable to value mapping (with type)
    locals: HashMap<String, (Value, IRType)>,
    /// Addressable storage for lexical locals.
    local_addresses: HashMap<String, Value>,
    /// Register type mapping (for temporaries)
    reg_types: HashMap<Register, IRType>,
    /// Struct definitions (name -> fields)
    struct_defs: HashMap<String, Vec<(String, IRType)>>,
    /// Pre-scanned semantic return types, independent of target calling convention.
    function_signatures: HashMap<String, IRType>,
    function_parameters: HashMap<String, Vec<IRType>>,
    global_types: HashMap<String, IRType>,
    return_contracts: contracts::ReturnContracts,
    /// Layout cache is populated by immutable type conversion as well as values.
    tuple_layouts: std::cell::RefCell<std::collections::BTreeMap<String, Vec<IRType>>>,
}

impl IRGenerator {
    pub fn new(module_name: &str) -> Self {
        Self {
            owned: owned::OwnedScopes::default(),
            emission_error: None,
            module: IRModule::new(module_name),
            semantic_facts: None,
            nominal_types: HashSet::new(),
            resolved_aliases: HashMap::new(),
            constants: HashMap::new(),
            current_fn: None,
            current_block: BlockId(0),
            next_register: 0,
            loop_targets: Vec::new(),
            locals: HashMap::new(),
            local_addresses: HashMap::new(),
            reg_types: HashMap::new(),
            struct_defs: HashMap::new(),
            function_signatures: HashMap::new(),
            function_parameters: HashMap::new(),
            global_types: HashMap::new(),
            return_contracts: contracts::ReturnContracts::default(),
            tuple_layouts: std::cell::RefCell::new(std::collections::BTreeMap::new()),
        }
    }

    pub fn with_semantic_facts(mut self, facts: SemanticFacts) -> Self {
        self.module.pointer_width = facts.pointer_width;
        self.semantic_facts = Some(facts);
        self
    }

    /// Generate IR for a program
    pub fn generate(mut self, program: &Program) -> Result<IRModule> {
        for item in &program.items {
            let name = match item {
                Item::Struct(def) => &def.name, Item::Union(def) => &def.name,
                Item::Enum(def) => &def.name, _ => continue,
            };
            self.nominal_types.insert(name.name.clone());
        }
        self.register_resolved_tuple_layouts()?;
        if let Some(facts) = &self.semantic_facts {
            for (name, ty) in &facts.aliases {
                self.resolved_aliases.insert(name.clone(), Self::resolved_type_to_ir(ty)?);
            }
            for (name, ty) in &facts.global_types {
                self.global_types.insert(name.clone(), Self::resolved_type_to_ir(ty)?);
            }
            for (name, constant) in &facts.constants {
                let ty = Self::resolved_type_to_ir(&constant.ty)?;
                let value = match &constant.value {
                    ScalarConstant::Int(value) => Constant::Int(*value),
                    ScalarConstant::Float(value) => Constant::Float(*value),
                    ScalarConstant::Bool(value) => Constant::Bool(*value),
                    ScalarConstant::Char(value) => Constant::Int(*value as i64),
                    ScalarConstant::String(value) => Constant::String(value.clone()),
                };
                self.constants.insert(name.clone(), (ty, value));
            }
        }
        // Process inner attributes: #![no_std], #![no_main]
        for attr in &program.inner_attrs {
            match attr.name.name.as_str() {
                "no_std" => self.module.no_std = true,
                "no_main" => self.module.no_main = true,
                _ => {}
            }
        }
        
        // Phase 1: Collect all function signatures (for forward reference)
        for item in &program.items {
            self.collect_signatures(item)?;
        }
        
        // Layouts and external declarations are order independent, like function signatures.
        for item in &program.items {
            if matches!(item, Item::Struct(_) | Item::Union(_) | Item::Static(_) | Item::Enum(_) | Item::Extern(_)) { self.generate_item(item)?; }
        }
        for item in &program.items {
            if !matches!(item, Item::Struct(_) | Item::Union(_) | Item::Static(_) | Item::Enum(_) | Item::Extern(_)) { self.generate_item(item)?; }
        }
        self.validate_hosted_abi()?;
        self.coalesce_defined_externs()?;
        self.finish_tuple_layouts()?;
        self.order_struct_layouts()?;
        Ok(self.module)
    }

    fn register_global(&mut self, global: IRGlobal) -> Result<()> {
        if let Some(previous) = self.module.globals.iter_mut().find(|previous| previous.name == global.name) {
            if previous.ty != global.ty || previous.mutable != global.mutable || previous.exported != global.exported {
                return Err(crate::utils::Error::CodeGen(format!("inconsistent static declaration and definition for '{}'", global.name)));
            }
            if previous.initializer.is_some() && global.initializer.is_some() {
                return Err(crate::utils::Error::CodeGen(format!("duplicate static definition for '{}'", global.name)));
            }
            if global.initializer.is_some() { *previous = global; }
        } else {
            self.module.globals.push(global);
        }
        Ok(())
    }

    fn function_has_c_abi(func: &ast::Function, name: &str) -> bool {
        name == "main" || func.annotations.iter().any(|annotation| matches!(annotation.name.name.as_str(), "no_mangle" | "interrupt" | "naked"))
    }

    /// Unit remains a language value, but a C ABI function has no result slot
    /// for it. Omitted and explicit unit returns must produce the same signature.
    fn function_return_type(&self, ty: Option<&AstType>, c_abi: bool) -> Result<IRType> {
        let ty = ty.map(|ty| self.ast_type_to_ir(ty)).transpose()?.unwrap_or(IRType::Void);
        Ok(if c_abi && ty == IRType::Unit { IRType::Void } else { ty })
    }

    /// Semantic analysis permits compatible declarations plus one C ABI body.
    /// Emit one definition, retaining a consistency check at the IR boundary so
    /// a stale signature (including an implicit libc default) cannot be hidden.
    fn coalesce_defined_externs(&mut self) -> Result<()> {
        let mut definitions = HashMap::new();
        for function in self.module.functions.iter().filter(|function| function.c_abi) {
            let signature = (function.params.iter().map(|(_, ty)| ty.clone()).collect::<Vec<_>>(), function.ret_type.clone());
            if definitions.insert(function.name.clone(), signature).is_some() {
                return Err(crate::utils::Error::CodeGen(format!("duplicate C ABI definition for '{}'", function.name)));
            }
        }
        for declaration in &self.module.externs {
            if let Some((params, ret)) = definitions.get(&declaration.name) {
                if declaration.variadic || declaration.ret_type != *ret
                    || !declaration.params.iter().map(|(_, ty)| ty).eq(params.iter()) {
                    return Err(crate::utils::Error::CodeGen(format!(
                        "inconsistent C ABI declaration and definition for '{}'", declaration.name)));
                }
            }
        }
        self.module.externs.retain(|declaration| !definitions.contains_key(&declaration.name));
        Ok(())
    }
    
    /// By-value fields need complete layouts; pointer cycles need only names.
    fn order_struct_layouts(&mut self) -> Result<()> {
        fn dependencies(ty: &IRType, output: &mut Vec<String>) {
            match ty {
                IRType::Struct(name) => output.push(name.clone()),
                IRType::Array(element, _) | IRType::Volatile(element) => dependencies(element, output),
                _ => {}
            }
        }
        fn visit(name: &str, definitions: &HashMap<String, crate::middle::ir::IRStruct>,
            visiting: &mut std::collections::HashSet<String>, visited: &mut std::collections::HashSet<String>,
            output: &mut Vec<crate::middle::ir::IRStruct>) -> Result<()> {
            if visited.contains(name) { return Ok(()); }
            if !visiting.insert(name.to_string()) {
                return Err(crate::utils::Error::CodeGen(format!("recursive by-value struct layout involving '{}'", name)));
            }
            let definition = definitions.get(name).ok_or_else(|| crate::utils::Error::CodeGen(format!("missing by-value layout for '{}'", name)))?;
            let mut required = Vec::new();
            for (_, ty) in &definition.fields { dependencies(ty, &mut required); }
            for dependency in required { visit(&dependency, definitions, visiting, visited, output)?; }
            visiting.remove(name); visited.insert(name.to_string()); output.push(definition.clone());
            Ok(())
        }
        let definitions: HashMap<_, _> = self.module.structs.iter().map(|s| (s.name.clone(), s.clone())).collect();
        let mut output = Vec::new();
        let mut visiting = std::collections::HashSet::new();
        let mut visited = std::collections::HashSet::new();
        for definition in &self.module.structs { visit(&definition.name, &definitions, &mut visiting, &mut visited, &mut output)?; }
        self.module.structs = output;
        Ok(())
    }

    /// Collect function signatures for forward reference
    fn collect_signatures(&mut self, item: &Item) -> Result<()> {
        match item {
            Item::Function(func) => {
                if !func.generic_params.is_empty() || !func.type_params.is_empty() {
                    return Err(crate::utils::Error::CodeGen("generic function reached IR before specialization".into()));
                }
                let func_name = func.name.name.clone();
                let ret_type = self.function_return_type(func.ret_type.as_ref(), Self::function_has_c_abi(func, &func_name))?;
                
                self.function_parameters.insert(func_name.clone(), func.params.iter().map(|p| self.ast_type_to_ir(&p.ty)).collect::<Result<_>>()?);
                self.function_signatures.insert(func_name, ret_type);
            }
            Item::Impl(impl_block) => {
                for method in &impl_block.methods {
                    let signature = self.resolved_method_signature(&impl_block.target.name, &method.name.name)?;
                    let params = signature.params.iter().map(Self::resolved_type_to_ir).collect::<Result<Vec<_>>>()?;
                    let ret_type = if method.ret_type.is_none() { IRType::Void } else {
                        let ty = Self::resolved_type_to_ir(&signature.ret)?;
                        if Self::function_has_c_abi(method, &signature.emitted_name) && ty == IRType::Unit { IRType::Void } else { ty }
                    };
                    self.function_parameters.insert(signature.emitted_name.clone(), params);
                    self.function_signatures.insert(signature.emitted_name, ret_type);
                }
            }
            Item::Module(m) => {
                if let Some(items) = &m.items {
                    for sub_item in items {
                        self.collect_signatures(sub_item)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Generate IR for a top-level item
    fn generate_item(&mut self, item: &Item) -> Result<()> {
        match item {
            Item::Function(func) => self.generate_function(func),
            Item::Struct(struct_def) => {
                if !struct_def.generic_params.is_empty() || !struct_def.type_params.is_empty() {
                    return Err(crate::utils::Error::CodeGen("generic struct specialization is not implemented".into()));
                }
                let fields: Vec<_> = struct_def.fields.iter()
                    .map(|f| Ok((f.name.name.clone(), self.ast_type_to_ir(&f.ty)?)))
                    .collect::<Result<_>>()?;
                
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
            Item::Enum(enum_def) => self.register_enum(enum_def),

            Item::Impl(impl_block) => {
                let type_name = &impl_block.target.name;
                for method in &impl_block.methods {
                    self.generate_method(type_name, method)?;
                }
                Ok(())
            }
            Item::Interface(_) => Ok(()),
            Item::Const(value) => {
                if self.constants.contains_key(&value.name.name) { Ok(()) }
                else { Err(crate::utils::Error::CodeGen(format!("constant '{}' has no validated scalar value", value.name.name))) }
            },
            // Macro/Module/Use are handled at earlier compilation stages
            Item::Macro(_) => Ok(()),
            Item::Module(_) | Item::Use(_) => Err(crate::utils::Error::CodeGen(
                "unresolved module reached IR generation".into())),
            Item::Extern(ext) => {
                // Register extern functions in IR module
                for foreign_item in &ext.items {
                    match foreign_item {
                        ast::ForeignItem::Fn { name, params, ret_type, variadic, .. } => {
                            let ir_params: Vec<(String, IRType)> = params.iter()
                                .map(|p| Ok((p.name.name.clone(), self.ast_type_to_ir(&p.ty)?)))
                    .collect::<Result<_>>()?;
                            let ir_ret = self.function_return_type(ret_type.as_ref(), true)?;
                            
                            if let Some(existing) = self.module.externs.iter_mut().find(|ext| ext.name == name.name) {
                                // Explicit validated ABI declarations override implicit libc defaults.
                                *existing = IRExtern { name: name.name.clone(), variadic: *variadic, params: ir_params, ret_type: ir_ret };
                            } else {
                                self.module.externs.push(IRExtern { name: name.name.clone(), variadic: *variadic, params: ir_params, ret_type: ir_ret });
                            }
                        }
                        ast::ForeignItem::Static { name, ty, is_mut, .. } => {
                            let ty = self.ast_type_to_ir(ty)?;
                            self.global_types.insert(name.name.clone(), ty.clone());
                            self.register_global(IRGlobal { name: name.name.clone(), ty, mutable: *is_mut, exported: true, initializer: None })?;
                        }
                    }
                }
                Ok(())
            }
            Item::Static(definition) => {
                let initializer = self.semantic_facts.as_ref().and_then(|facts| facts.globals.get(&definition.name.name)).cloned()
                    .ok_or_else(|| crate::utils::Error::CodeGen(format!("global '{}' has no validated initializer", definition.name.name)))?;
                let ty = self.ast_type_to_ir(&definition.ty)?;
                self.global_types.insert(definition.name.name.clone(), ty.clone());
                self.register_global(IRGlobal { name: definition.name.name.clone(), ty, mutable: definition.is_mut,
                    exported: definition.is_pub || definition.annotations.iter().any(|annotation| annotation.name.name == "no_mangle"),
                    initializer: Some(self.lower_initializer(&initializer)?) })?;
                Ok(())
            }
            Item::Union(definition) => {
                let fields = definition.fields.iter().map(|field| Ok((field.name.name.clone(), self.ast_type_to_ir(&field.ty)?))).collect::<Result<Vec<_>>>()?;
                self.struct_defs.insert(definition.name.name.clone(), fields.clone());
                self.module.structs.push(crate::middle::ir::IRStruct { name: definition.name.name.clone(), fields, is_union: true,
                    repr: if definition.repr == Some(ast::Repr::Packed) { crate::middle::ir::StructRepr::Packed } else { crate::middle::ir::StructRepr::C } });
                Ok(())
            }
            Item::Trait(_) => Ok(()), // Trait definitions don't generate IR directly
            Item::TypeAlias(alias) => {
                if self.resolved_aliases.contains_key(&alias.name.name) { Ok(()) }
                else { Err(crate::utils::Error::CodeGen(format!("type alias '{}' has no resolved semantic type", alias.name.name))) }
            },
        }
    }
    
    /// The semantic signature owns both the emitted symbol and resolved Self.
    fn generate_method(&mut self, type_name: &str, func: &ast::Function) -> Result<()> {
        let signature = self.resolved_method_signature(type_name, &func.name.name)?;
        let self_type = Self::resolved_type_to_ir(&signature.target)?;
        let previous = self.resolved_aliases.insert("Self".into(), self_type);
        let result = self.generate_function_with_name(func, &signature.emitted_name);
        if let Some(previous) = previous { self.resolved_aliases.insert("Self".into(), previous); }
        else { self.resolved_aliases.remove("Self"); }
        result
    }

    fn generate_function(&mut self, func: &ast::Function) -> Result<()> {
        self.generate_function_with_name(func, &func.name.name)
    }

    /// Function IR retains language values. ABI coercion belongs to the backend.
    fn generate_function_with_name(&mut self, func: &ast::Function, name: &str) -> Result<()> {
        if !func.generic_params.is_empty() || !func.type_params.is_empty() { return Err(crate::utils::Error::CodeGen("generic function reached IR before specialization".into())); }
        self.next_register = 0;
        self.locals.clear();
        self.local_addresses.clear();
        self.reg_types.clear();
        self.loop_targets.clear();
        self.begin_owned(func);
        let parameter_types = self.function_parameters.get(name).ok_or_else(|| crate::utils::Error::CodeGen(format!("missing collected signature for '{}'", name)))?;
        if parameter_types.len() != func.params.len() { return Err(crate::utils::Error::CodeGen(format!("parameter count changed after checking '{}'", name))); }
        let params: Vec<_> = func.params.iter().zip(parameter_types).map(|(p, ty)| (p.name.name.clone(), ty.clone())).collect();
        let ret_type = self.function_signatures.get(name).cloned().ok_or_else(|| crate::utils::Error::CodeGen(format!("missing collected return type for '{}'", name)))?;
        let mut ir_func = IRFunction::new(name, params.clone(), ret_type);
        for annotation in &func.annotations {
            match annotation.name.name.as_str() {
                "no_mangle" => ir_func.c_abi = true,
                "simd" => ir_func.simd = true,
                "naked" => { ir_func.naked = true; ir_func.c_abi = true; },
                "interrupt" => { ir_func.interrupt = true; ir_func.c_abi = true; },
                "volatile" => ir_func.volatile = true,
                _ => {}
            }
        }
        self.current_block = ir_func.add_block("entry");
        self.current_fn = Some(ir_func);
        for (index, (name, ty)) in params.into_iter().enumerate() {
            let reg = self.alloc_register();
            self.emit_current_with_type(Instruction::Assign { dest: reg, value: Value::Parameter(index) }, ty.clone());
            let address = self.owner_local_storage(func.params[index].name.span, &ty, Some(Value::Register(reg)));
            self.bind_owned(func.params[index].name.span,address.clone(),true)?;
            self.locals.insert(name.clone(), (address.clone(), ty));
            self.local_addresses.insert(name, address);
        }
        self.begin_contracts(func)?;
        let last_value = self.generate_block(&func.body)?;
        let last_value = self.prepare_implicit_return(last_value)?;
        if self.get_current_terminator().is_none() {
            let ty = self.current_fn.as_ref().unwrap().ret_type.clone();
            if self.current_fn.as_ref().unwrap().naked {
                self.set_terminator_current(Terminator::Unreachable);
            } else if ty == IRType::Void {
                self.emit_checked_return(None)?;
            } else if let Some(value) = last_value {
                self.emit_checked_return(Some(value))?;
            } else {
                self.set_terminator_current(Terminator::Unreachable);
            }
        }
        if let Some(error) = self.emission_error.take() { return Err(error); }
        let mut function = self.current_fn.take().unwrap();
        function.register_types = std::mem::take(&mut self.reg_types);
        self.module.functions.push(function);
        Ok(())
    }

    /// Generate IR for a block
    fn generate_block(&mut self, block: &ast::Block) -> Result<Option<Value>> {
        self.owned.depth+=1;
        let mut last_value = None;
        let saved_locals = self.locals.clone();
        let saved_addresses = self.local_addresses.clone();
        for (index,stmt) in block.stmts.iter().enumerate() {
            if self.get_current_terminator().is_some() { break; }
            last_value = self.generate_stmt(stmt)?;
            self.cleanup_owned_temporaries(0,self.owned.depth,if index+1==block.stmts.len(){last_value.as_ref()}else{None})?;
        }
        if let Some(value)=&last_value {self.promote_owned_value(value);}
        self.cleanup_owned_scope(self.owned.depth)?;
        self.owned.depth-=1;
        self.locals = saved_locals;
        self.local_addresses = saved_addresses;
        Ok(last_value)
    }

    /// Generate IR for a statement
    fn generate_stmt(&mut self, stmt: &ast::Stmt) -> Result<Option<Value>> {
        match stmt {
            Stmt::Let { name, value, ty: type_annotation, .. } => {
                let declared = type_annotation.as_ref().map(|ty| self.ast_type_to_ir(ty)).transpose()?;
                let val = value.as_ref().map(|expr| self.generate_expr_expected(expr, declared.as_ref())).transpose()?;
                if self.get_current_terminator().is_some() { return Ok(None); }
                let var_type = declared.or_else(|| val.as_ref().and_then(|v| self.get_value_type(v)))
                    .ok_or_else(|| crate::utils::Error::CodeGen(format!("missing type for local '{}'", name.name)))?;
                if let Some(value)=&val {self.transfer_owned_value(value);}
                let initialized=val.is_some();
                let address = self.owner_local_storage(name.span,&var_type,val);
                self.bind_owned(name.span,address.clone(),initialized)?;
                self.locals.insert(name.name.clone(), (address.clone(), var_type));
                self.local_addresses.insert(name.name.clone(), address);
                Ok(None)
            }

            Stmt::Semi(expr) => {
                self.generate_expr(expr)?;
                Ok(None)
            }

            Stmt::Expr(expr) => {
                let val = self.generate_expr(expr)?;
                Ok(Some(val))
            }

            Stmt::Return { value, .. } => {
                let expected = self.current_fn.as_ref().unwrap().ret_type.clone();
                let ret_val = value.as_ref().map(|expr| self.generate_expr_expected(expr, Some(&expected))).transpose()?;
                let ret_val = if expected == IRType::Void { None } else if expected == IRType::Unit { Some(ret_val.unwrap_or(Value::Unit)) } else { ret_val };
                self.emit_checked_return(ret_val)?;
                Ok(None)
            }

            Stmt::Break { .. } => {
                let (_, target) = self.loop_targets.last().copied()
                    .ok_or_else(|| crate::utils::Error::CodeGen("break outside a loop".into()))?;
                self.cleanup_owned_scope(*self.owned.loops.last().unwrap())?;
                self.set_terminator_current(Terminator::Jump { target });
                Ok(None)
            }

            Stmt::Continue { .. } => {
                let (target, _) = self.loop_targets.last().copied()
                    .ok_or_else(|| crate::utils::Error::CodeGen("continue outside a loop".into()))?;
                self.cleanup_owned_scope(*self.owned.loops.last().unwrap())?;
                self.set_terminator_current(Terminator::Jump { target });
                Ok(None)
            }

            Stmt::Empty { .. } => Ok(None),
        }

    }

    /// Generate IR for an expression
    fn generate_expr(&mut self, expr: &ast::Expr) -> Result<Value> {
        if self.get_current_terminator().is_some() { return Ok(Value::Unit); }
        let value = self.generate_expr_inner(expr)?;
        if let Some(ty @ ResolvedType::Owned(_))=self.checked_expr_type(expr) {
            let ty = ty.clone();
            self.move_owned_binding(expr);
            self.remember_owned_temporary(&value,&ty)?;
        }
        let checked_type = self.checked_expr_type(expr);
        if checked_type == Some(&ResolvedType::NEVER) {
            if self.get_current_terminator().is_none() { self.set_terminator_current(Terminator::Unreachable); }
            return Ok(Value::Unit);
        }
        // IR constants store i64/f64 bits. Materialize their language types
        // before a parent instruction chooses precision, signedness or promotion.
        if let Some(ty @ ResolvedType::Primitive(primitive)) = checked_type {
            if primitive.is_integer() || primitive.is_float() || *primitive == PrimitiveType::Char {
                let ty = Self::resolved_type_to_ir(ty)?;
                return Ok(self.coerce_value(value, &ty));
            }
        }
        Ok(value)
    }

    fn generate_expr_inner(&mut self, expr: &ast::Expr) -> Result<Value> {
        match expr {
            Expr::Layout { query, ty, field, .. } => {
                let ty = self.ast_type_to_ir(ty)?;
                let field = match (&ty, field) {
                    (IRType::Struct(name), Some(field)) => Some(self.struct_defs.get(name).and_then(|fields| fields.iter().position(|(name, _)| name == &field.name))
                        .ok_or_else(|| crate::utils::Error::UnknownField { span: field.span, field: field.name.clone() })?),
                    (_, None) => None,
                    _ => return Err(crate::utils::Error::CodeGen("offset_of requires a struct or union field".into())),
                };
                let query = match query { ast::LayoutQuery::Size => IRLayoutQuery::Size, ast::LayoutQuery::Align => IRLayoutQuery::Align, ast::LayoutQuery::Offset => IRLayoutQuery::Offset };
                let dest = self.alloc_register();
                self.emit_current_with_type(Instruction::Layout { dest, query, ty, field }, Self::pointer_integer(self.pointer_width(), false));
                Ok(Value::Register(dest))
            }
            Expr::Literal(lit) => Ok(self.generate_literal(lit)),

            Expr::Ident(ident) => {
                if let Some((val, ty)) = self.locals.get(&ident.name).cloned() {
                    if let Some(address) = self.local_addresses.get(&ident.name).cloned() {
                        return Ok(self.load_address(address, ty));
                    }
                    Ok(val)
                } else if let Some(ty) = self.global_types.get(&ident.name).cloned() {
                    Ok(self.load_address(Value::Global(ident.name.clone()), ty))
                } else if let Some((ty, value)) = self.constants.get(&ident.name).cloned() {
                    let value = Value::Constant(value);
                    if self.get_value_type(&value).as_ref() == Some(&ty) { Ok(value) }
                    else {
                        let dest = self.alloc_register();
                        self.emit_current_with_type(Instruction::Cast { dest, value, ty: ty.clone() }, ty);
                        Ok(Value::Register(dest))
                    }
                } else { Err(crate::utils::Error::CodeGen(format!("unresolved value '{}'", ident.name))) }

            }

            Expr::Path { segments, .. } => {
                if let Some((definition,variant))=self.enum_variant_path(segments) {
                    self.construct_enum(&definition,variant,&[])
                } else { Err(crate::utils::Error::CodeGen("unresolved value path".into())) }
            },
            Expr::Binary { left, op, right, .. } => {
                if matches!(op, ast::BinOp::And | ast::BinOp::Or) {
                    return self.generate_short_circuit(left, *op, right);
                }
                if matches!(op, ast::BinOp::Assign | ast::BinOp::AddAssign | ast::BinOp::SubAssign | ast::BinOp::MulAssign | ast::BinOp::DivAssign) {
                    // Resolve the address first and exactly once. In particular,
                    // p[next()] = value must neither load p[next()] nor call next twice.
                    let (address, ty) = self.generate_place(left)?;
                    let old_value = if *op == ast::BinOp::Assign { None }
                        else { Some(self.load_address(address.clone(), ty.clone())) };
                    let value = self.generate_expr(right)?;
                    let mut value = self.coerce_value(value, &ty);
                    if let Some(old) = old_value {
                        let operation = match op {
                            ast::BinOp::AddAssign => IRBinOp::Add,
                            ast::BinOp::SubAssign => IRBinOp::Sub,
                            ast::BinOp::MulAssign => IRBinOp::Mul,
                            ast::BinOp::DivAssign => IRBinOp::Div,
                            _ => unreachable!(),
                        };
                        let dest = self.alloc_register();
                        self.emit_current_with_type(Instruction::BinOp { dest, op: operation, left: old, right: value }, ty.unqualified().clone());
                        value = Value::Register(dest);
                    }
                    self.transfer_owned_value(&value);
                    self.initialize_owned_place(&address);
                    self.emit_current(Instruction::Store { ptr: address, value: value.clone() });
                    return Ok(value);
                }
                let left_val = self.generate_expr(left)?;
                let right_val = self.generate_expr(right)?;

                // Check for pointer arithmetic: ptr + offset or ptr - offset
                let left_ty = self.require_value_type(&left_val)?;
                if let IRType::Ptr(inner) = &left_ty {
                    if *op == ast::BinOp::Add || *op == ast::BinOp::Sub {
                        // This is pointer arithmetic, use GEP instead of add
                        let dest = self.alloc_register();
                        let offset_val = if *op == ast::BinOp::Sub {
                            // For subtraction, negate the offset
                            let neg_dest = self.alloc_register();
                            let zero = Value::Constant(Constant::Int(0));
                            self.emit_current_with_type(Instruction::BinOp {
                                dest: neg_dest,
                                op: IRBinOp::Sub,
                                left: zero,
                                right: right_val.clone(),
                            }, IRType::I64);
                            Value::Register(neg_dest)
                        } else {
                            right_val
                        };
                        
                        self.emit_current_with_type(Instruction::GetElementPtr {
                            dest,
                            ptr: left_val,
                            index: offset_val,
                            elem_ty: (**inner).clone(),
                        }, IRType::Ptr(inner.clone()));
                        
                        return Ok(Value::Register(dest));
                    }
                }
                
                let ir_op = self.ast_binop_to_ir(*op);
                let dest = self.alloc_register();
                
                let unified_right = self.coerce_value(right_val, &left_ty);

                // Result of binary op is usually primitive or bool (I64/Bool)
                let res_ty = match ir_op {
                    IRBinOp::Eq | IRBinOp::Ne | IRBinOp::Lt | IRBinOp::Le | IRBinOp::Gt | IRBinOp::Ge => IRType::Bool,
                    _ => left_ty,
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
                let ty = self.require_value_type(&val)?;
                if let IRType::Vector(element, _) = &ty {
                    use crate::intrinsics::VectorKind;
                    let zero = if matches!(element.as_ref(), IRType::F32 | IRType::F64) { Value::Constant(Constant::Float(-0.0)) } else { Value::Constant(Constant::Int(0)) };
                    let zero = self.coerce_value(zero, element);
                    let splat = self.alloc_register();
                    self.emit_current_with_type(Instruction::Vector { dest: Some(splat), kind: VectorKind::Splat, ty: ty.clone(), args: vec![zero] }, ty.clone());
                    self.emit_current_with_type(Instruction::Vector { dest: Some(dest), kind: VectorKind::Sub, ty: ty.clone(), args: vec![Value::Register(splat), val] }, ty);
                } else {
                    self.emit_current_with_type(Instruction::UnaryOp { dest, op: ir_op, value: val }, ty);
                }
                Ok(Value::Register(dest))
            }

            Expr::Call { func, args, span } => {
                if let Some(builtin) = self.checked_host_call(*span) {
                    return self.generate_host_call(builtin, args);
                }
                if let Expr::Path {segments,..}=func.as_ref() {
                    if let Some((definition,variant))=self.enum_variant_path(segments) {
                        return self.construct_enum(&definition,variant,args);
                    }
                }

                let func_name = if let Expr::Ident(ident) = func.as_ref() {
                    ident.name.clone()
                } else if let Expr::Path { segments, .. } = func.as_ref() {
                    let symbol = segments.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join("::");
                    self.semantic_facts.as_ref().and_then(|facts| facts.method_signatures.get(&symbol))
                        .map(|signature| signature.emitted_name.clone())
                        .unwrap_or_else(|| segments.iter().map(|s| s.name.clone()).collect::<Vec<_>>().join("_"))
                } else {
                    return Err(crate::utils::Error::CodeGen("missing resolved direct callee".into()));
                };

                if let Some(op) = crate::intrinsics::platform(&func_name) {
                    if !args.is_empty() { return Err(crate::utils::Error::CodeGen("platform intrinsic arity mismatch".into())); }
                    let dest = self.alloc_register();
                    self.emit_current_with_type(Instruction::Platform { dest, op }, Self::resolved_type_to_ir(&op.result())?);
                    return Ok(Value::Register(dest));
                }
                if crate::intrinsics::memory(&func_name).is_some() {
                    return self.generate_memory_call(&func_name, args, *span);
                }

                if crate::intrinsics::vector(&func_name).is_some() { return self.generate_vector_call(&func_name,args); }

                // Generate argument values
                let mut arg_vals: Vec<Value> = Vec::new();
                
                let external = self.module.externs.iter().find(|e| e.name == func_name).cloned();
                let param_types = self.function_parameters.get(&func_name).cloned()
                    .or_else(|| external.as_ref().map(|e| e.params.iter().map(|(_, ty)| ty.clone()).collect()))
                    .unwrap_or_default();
                let variadic = external.as_ref().map(|e| e.variadic).unwrap_or(false);
                for (i, arg) in args.iter().enumerate() {
                    let mut val = self.generate_expr_expected(arg, param_types.get(i))?;
                    if self.get_current_terminator().is_some() { return Ok(Value::Unit); }
                    if let Some(expected) = param_types.get(i) {
                        val = self.coerce_value(val, expected);
                    } else if variadic {
                        // C default argument promotions are part of the foreign ABI.
                        let promoted = match self.get_value_type(&val) {
                            Some(IRType::F32) => Some(IRType::F64),
                            Some(IRType::I8 | IRType::I16 | IRType::U8 | IRType::U16 | IRType::Bool) => Some(IRType::I32),
                            _ => None,
                        };
                        if let Some(ty) = promoted {
                            let dest = self.alloc_register();
                            self.emit_current_with_type(Instruction::Cast { dest, value: val, ty: ty.clone() }, ty);
                            val = Value::Register(dest);
                        }
                    }
                    arg_vals.push(val);
                }

                for value in &arg_vals {self.transfer_owned_value(value);}
                // Resolve the semantic return type.
                let ret_type = self.function_signatures.get(&func_name).cloned()
                    .or_else(|| external.map(|function| function.ret_type))
                    .ok_or_else(|| crate::utils::Error::CodeGen(format!("missing signature for call to '{func_name}'")))?;

                if ret_type == IRType::Void {
                    // Regular void function
                    self.emit_current_with_type(Instruction::Call {
                        dest: None,
                        func: func_name,
                        args: arg_vals,
                    }, IRType::Void);
                    Ok(Value::Unit)
                } else {
                    // Regular non-void function
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
                let temporary_start=self.owned_checkpoint();
                let cond_val = self.generate_expr(cond)?;
                self.cleanup_owned_temporaries(temporary_start,self.owned.depth,None)?;
                if self.get_current_terminator().is_some() { return Ok(Value::Unit); }

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
                    if let Some(value)=&then_result {self.transfer_owned_value(value);}
                    self.set_terminator_current(Terminator::Jump { target: merge_id });
                    true
                } else { false };
                let then_exit = self.current_block;

                self.current_block = else_id;
                let else_result = if let Some(eb) = else_block {
                    self.generate_block(eb)?
                } else { None }; // Void if no else
                
                let else_jumps_to_merge = if self.get_current_terminator().is_none() {
                    if let Some(value)=&else_result {self.transfer_owned_value(value);}
                    self.set_terminator_current(Terminator::Jump { target: merge_id });
                    true
                } else { false };
                let else_exit = self.current_block;

                self.current_block = merge_id;
                
                // Phi node logic - only generate if BOTH branches jump to merge AND both have values
                // For statement-level if (no else or no return value), just return Unit
                if !then_jumps_to_merge && !else_jumps_to_merge {
                    self.set_terminator_current(Terminator::Unreachable);
                }
                if then_jumps_to_merge && !else_jumps_to_merge { return Ok(then_result.unwrap_or(Value::Unit)); }
                if else_jumps_to_merge && !then_jumps_to_merge { return Ok(else_result.unwrap_or(Value::Unit)); }
                if then_jumps_to_merge && else_jumps_to_merge {
                    if let (Some(then_val), Some(else_val)) = (&then_result, &else_result) {
                        if matches!(then_val, Value::Unit) || matches!(else_val, Value::Unit) { return Ok(Value::Unit); }
                        let dest = self.alloc_register();
                        let phi_ty = self.require_value_type(then_val)?;
                        let incoming = vec![(then_val.clone(), then_exit), (else_val.clone(), else_exit)];
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

            Expr::Field { .. } | Expr::Index { .. } | Expr::Deref { .. } => {
                let (address, ty) = self.generate_place(expr)?;
                Ok(self.load_address(address, ty))
            }

            Expr::StructLit { name, fields, .. } => {
                let struct_type = IRType::Struct(name.name.clone());
                
                // Construct in private storage, then produce a true aggregate value.
                let ptr_val = self.allocate_local(&struct_type, None);
                self.emit_current(Instruction::ZeroInitialize { ptr: ptr_val.clone(), ty: struct_type.clone() });

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
                          
                     let field_val = self.generate_expr_expected(field_expr, Some(field_ty))?;
                     if self.get_current_terminator().is_some() { return Ok(Value::Unit); }
                     let field_ptr = self.alloc_register();
                     
                     self.emit_current_with_type(Instruction::GetFieldPtr {
                         dest: field_ptr,
                         ptr: ptr_val.clone(),
                         field: idx,
                         struct_ty: struct_type.clone(),
                     }, IRType::Ptr(Box::new(field_ty.clone())));
                     
                     self.emit_current(Instruction::Store {
                         ptr: Value::Register(field_ptr),
                         value: field_val,
                     });
                }
                
                Ok(self.load_address(ptr_val, struct_type))
            }
            
            Expr::Loop { body, .. } => {
                let body_block = self.add_block("loop_body");
                let exit_block = self.add_block("loop_exit");
                self.set_terminator_current(Terminator::Jump { target: body_block });
                self.current_block = body_block;
                self.loop_targets.push((body_block, exit_block));
                self.owned.loops.push(self.owned.depth+1);
                self.generate_block(body)?;
                self.loop_targets.pop();
                self.owned.loops.pop();
                if self.get_current_terminator().is_none() {
                    self.set_terminator_current(Terminator::Jump { target: body_block });
                }
                self.current_block = exit_block;
                Ok(Value::Unit)
            }
            Expr::While { cond, body, .. } => {
                let cond_block = self.add_block("while_cond");
                self.set_terminator_current(Terminator::Jump { target: cond_block });
                self.current_block = cond_block;
                let temporary_start=self.owned_checkpoint();
                let cond_val = self.generate_expr(cond)?;
                self.cleanup_owned_temporaries(temporary_start,self.owned.depth,None)?;
                if self.get_current_terminator().is_some() { return Ok(Value::Unit); }
                let body_block = self.add_block("while_body");
                let exit_block = self.add_block("while_exit");
                self.set_terminator_current(Terminator::Branch {
                    cond: cond_val, then_target: body_block, else_target: exit_block,
                });
                self.current_block = body_block;
                self.loop_targets.push((cond_block, exit_block));
                self.owned.loops.push(self.owned.depth+1);
                self.generate_block(body)?;
                self.loop_targets.pop();
                self.owned.loops.pop();
                if self.get_current_terminator().is_none() {
                    self.set_terminator_current(Terminator::Jump { target: cond_block });
                }
                self.current_block = exit_block;
                Ok(Value::Unit)
            }
            Expr::For { var, iter, body, .. } => {
                let (start, end) = match iter.as_ref() {
                    Expr::Range { start: Some(start), end: Some(end), .. } => (start, end),
                    _ => return Err(crate::utils::Error::CodeGen(
                        "for requires a bounded integer range (start..end)".into())),
                };
                // Evaluate each bound exactly once before entering the loop.
                let temporary_start=self.owned_checkpoint();
                let start_val = self.generate_expr(start)?;
                let end_val = self.generate_expr(end)?;
                self.cleanup_owned_temporaries(temporary_start,self.owned.depth,None)?;
                if self.get_current_terminator().is_some() { return Ok(Value::Unit); }
                let ty = self.require_value_type(&start_val)?;
                let end_val = self.coerce_value(end_val, &ty);
                let counter = self.allocate_local(&ty, Some(start_val));
                let previous = self.locals.insert(var.name.clone(), (counter.clone(), ty.clone()));
                let previous_address = self.local_addresses.insert(var.name.clone(), counter.clone());
                let cond_block = self.add_block("for_cond");
                let body_block = self.add_block("for_body");
                let step_block = self.add_block("for_step");
                let exit_block = self.add_block("for_exit");
                self.set_terminator_current(Terminator::Jump { target: cond_block });
                self.current_block = cond_block;
                let current = self.load_address(counter.clone(), ty.clone());
                let cond = self.alloc_register();
                self.emit_current_with_type(Instruction::BinOp {
                    dest: cond, op: IRBinOp::Lt, left: current, right: end_val,
                }, IRType::Bool);
                self.set_terminator_current(Terminator::Branch {
                    cond: Value::Register(cond), then_target: body_block, else_target: exit_block,
                });
                self.current_block = body_block;
                self.loop_targets.push((step_block, exit_block));
                self.owned.loops.push(self.owned.depth+1);
                self.generate_block(body)?;
                self.loop_targets.pop();
                self.owned.loops.pop();
                if self.get_current_terminator().is_none() {
                    self.set_terminator_current(Terminator::Jump { target: step_block });
                }
                self.current_block = step_block;
                let current = self.load_address(counter.clone(), ty.clone());
                let next = self.alloc_register();
                let one = self.coerce_value(Value::Constant(Constant::Int(1)), &ty);
                self.emit_current_with_type(Instruction::BinOp {
                    dest: next, op: IRBinOp::Add, left: current, right: one,
                }, ty);
                self.emit_current(Instruction::Store { ptr: counter, value: Value::Register(next) });
                self.set_terminator_current(Terminator::Jump { target: cond_block });
                self.current_block = exit_block;
                if let Some(old) = previous { self.locals.insert(var.name.clone(), old); }
                else { self.locals.remove(&var.name); }
                if let Some(old) = previous_address { self.local_addresses.insert(var.name.clone(), old); }
                else { self.local_addresses.remove(&var.name); }
                Ok(Value::Unit)
            }
            Expr::Match { expr, arms, .. } => self.generate_match_value(expr, arms),

            Expr::ArrayRepeat {value,count,..} => self.generate_array_repeat(expr,value,count,None),
            Expr::Array { elements, .. } => self.generate_array(elements, None),
            Expr::Tuple { elements, .. } => {
                let expected = self.checked_expr_type(expr).map(Self::resolved_type_to_ir).transpose()?;
                self.generate_tuple(elements, expected.as_ref())
            }

            Expr::MethodCall { expr: receiver, args, span, .. } => self.generate_method_call(receiver, args, *span),
            Expr::Ref { expr: inner, .. } => {
                let (address, _) = self.generate_place(inner)?;
                Ok(address)
            },
            Expr::Unsafe { body, .. } => Ok(self.generate_block(body)?.unwrap_or(Value::Unit)),
            Expr::Cast { expr, ty, .. } => {
                let val = self.generate_expr(expr)?;
                let target_ty = self.ast_type_to_ir(ty)?;
                self.transfer_owned_value(&val);
                if self.get_value_type(&val).as_ref() == Some(&target_ty) { return Ok(val); }
                let dest = self.alloc_register();
                self.emit_current_with_type(Instruction::Cast {
                    dest,
                    value: val,
                    ty: target_ty.clone(),
                }, target_ty);
                Ok(Value::Register(dest))
            },
            Expr::Range { .. } => Err(crate::utils::Error::CodeGen("range values are only supported as for-loop iterators".into())),
            Expr::Asm { template, operands, .. } => {
                let mut ir_operands = Vec::new();
                let mut destinations = Vec::new();
                for operand in operands {
                    let mut input = None;
                    let mut output = None;
                    if matches!(operand.kind, ast::AsmOperandKind::Output | ast::AsmOperandKind::InOut) {
                        let expr = operand.expr.as_ref().ok_or_else(|| crate::utils::Error::CodeGen("asm output needs an assignment place".into()))?;
                        let (address, ty) = self.generate_place(expr)?;
                        if matches!(operand.kind, ast::AsmOperandKind::InOut) { input = Some(self.load_address(address.clone(), ty.clone())); }
                        let reg = self.alloc_register();
                        self.reg_types.insert(reg, ty);
                        output = Some(reg);
                        destinations.push((address, reg));
                    } else if matches!(operand.kind, ast::AsmOperandKind::Input) {
                        if let Some(expr) = &operand.expr { input = Some(self.generate_expr(expr)?); }
                    }
                    ir_operands.push(IRAsmOperand {
                        kind: match operand.kind {
                            ast::AsmOperandKind::Input => IRAsmOperandKind::Input,
                            ast::AsmOperandKind::Output => IRAsmOperandKind::Output,
                            ast::AsmOperandKind::InOut => IRAsmOperandKind::InOut,
                            ast::AsmOperandKind::Clobber => IRAsmOperandKind::Clobber,
                        },
                        constraint: operand.options.clone(), input, output,
                    });
                }
                self.emit_current(Instruction::InlineAsm { template: template.clone(), operands: ir_operands });
                for (address, reg) in destinations {
                    self.emit_current(Instruction::Store { ptr: address, value: Value::Register(reg) });
                }
                Ok(Value::Unit)
            }
            Expr::Try { expr, .. } => self.generate_try_value(expr),
            Expr::Closure { .. } => Err(crate::utils::Error::CodeGen("closure lowering is not implemented".into())),

        }
    }

    /// Generate a constant value from a literal
    fn generate_literal(&self, lit: &ast::Literal) -> Value {
        match lit {
            ast::Literal::Int(n, _) => Value::Constant(Constant::Int(*n as i64)),
            ast::Literal::Float(n, _) => Value::Constant(Constant::Float(*n)),
            ast::Literal::Bool(b, _) => Value::Constant(Constant::Bool(*b)),
            ast::Literal::String(s, _) => Value::Constant(Constant::String(s.clone())),
            ast::Literal::Char(c, _) => Value::Constant(Constant::Int(*c as i64)),
        }
    }

    // ==================== Helper Methods ====================

    fn checked_expr_type(&self, expr: &Expr) -> Option<&ResolvedType> {
        let function = self.current_fn.as_ref()?;
        self.semantic_facts.as_ref()?.expr_types.get(&(function.name.clone(), expr.span()))
    }

    fn generate_expr_expected(&mut self, expr: &Expr, expected: Option<&IRType>) -> Result<Value> {
        if let Expr::Tuple { elements, .. } = expr {
            if expected.is_some() { return self.generate_tuple(elements, expected); }
        }
        if let (Expr::ArrayRepeat {value,count,..},Some(IRType::Array(element,_)))=(expr,expected){
            return self.generate_array_repeat(expr,value,count,Some((**element).clone()));
        }
        if let (Expr::Array { elements, .. }, Some(IRType::Array(element, size))) = (expr, expected) {
            if elements.len() != *size { return Err(crate::utils::Error::CodeGen("array length mismatch".into())); }
            return self.generate_array(elements, Some((**element).clone()));
        }
        let value = self.generate_expr(expr)?;
        if self.get_current_terminator().is_some() { return Ok(Value::Unit); }
        Ok(match expected { Some(ty) => self.coerce_value(value, ty), None => value })
    }

    fn generate_array(&mut self, elements: &[Expr], expected: Option<IRType>) -> Result<Value> {
        let mut values = Vec::new();
        for element in elements {
            values.push(self.generate_expr_expected(element, expected.as_ref())?);
            if self.get_current_terminator().is_some() { return Ok(Value::Unit); }
        }
        let ty = expected.or_else(|| values.first().and_then(|v| self.get_value_type(v)))
            .ok_or_else(|| crate::utils::Error::CodeGen("empty array requires an element type".into()))?;
        let array_ty = IRType::Array(Box::new(ty.clone()), values.len());
        let storage = self.allocate_local(&array_ty, None);
        for (index, value) in values.into_iter().enumerate() {
            let value = self.coerce_value(value, &ty);
            let dest = self.alloc_register();
            self.emit_current_with_type(Instruction::ArrayElementPtr {
                dest, ptr: storage.clone(), index: Value::Constant(Constant::Int(index as i64)), array_ty: array_ty.clone(),
            }, IRType::Ptr(Box::new(ty.clone())));
            self.emit_current(Instruction::Store { ptr: Value::Register(dest), value });
        }
        Ok(self.load_address(storage, array_ty))
    }

    fn prepare_implicit_return(&mut self, value: Option<Value>) -> Result<Option<Value>> {
        if self.get_current_terminator().is_some() { return Ok(value); }
        let ty = self.current_fn.as_ref().unwrap().ret_type.clone();
        if ty == IRType::Unit { return Ok(Some(value.unwrap_or(Value::Unit))); }
        Ok(value.map(|value| self.coerce_value(value, &ty)))
    }

    fn allocate_local(&mut self, ty: &IRType, value: Option<Value>) -> Value {
        let dest = self.alloc_register();
        self.emit_current_with_type(Instruction::Alloca { dest, ty: ty.clone() }, IRType::Ptr(Box::new(ty.clone())));
        let address = Value::Register(dest);
        if let Some(value) = value { self.emit_current(Instruction::Store { ptr: address.clone(), value }); }
        address
    }

    fn load_address(&mut self, ptr: Value, ty: IRType) -> Value {
        let dest = self.alloc_register();
        let value_ty = ty.unqualified().clone();
        self.emit_current_with_type(Instruction::Load { dest, ptr, ty }, value_ty);
        Value::Register(dest)
    }

    /// One lowering path for reads, writes and references. The address is
    /// evaluated once, with the actual element or field layout.
    fn generate_place(&mut self, expr: &Expr) -> Result<(Value, IRType)> {
        match expr {
            Expr::Ident(name) => {
                if let Some(ty) = self.global_types.get(&name.name) { return Ok((Value::Global(name.name.clone()), ty.clone())); }
                let address = self.local_addresses.get(&name.name).cloned()
                    .ok_or_else(|| crate::utils::Error::CodeGen(format!("'{}' has no addressable storage", name.name)))?;
                let ty = self.locals.get(&name.name).unwrap().1.clone();
                Ok((address, ty))
            }
            Expr::Deref { expr, .. } => {
                let ptr = self.generate_expr(expr)?;
                if self.get_current_terminator().is_some() { return Ok((Value::Unit, IRType::Unit)); }
                match self.get_value_type(&ptr) {
                    Some(IRType::Ptr(ty)) => Ok((ptr, *ty)),
                    _ => Err(crate::utils::Error::CodeGen("dereference requires a pointer".into())),
                }
            }
            Expr::Index { expr, index, .. } => {
                let (mut base, mut base_ty) = self.address_or_temporary(expr)?;
                // References and raw pointers share an IR representation, but
                // indexing has different source semantics: &array indexes its
                // elements, while *[T; N] advances by a whole array. Preserve
                // that distinction using the checked expression type.
                let mut source_ty = self.checked_expr_type(expr).cloned();
                while let Some(ResolvedType::Reference { inner, .. }) = source_ty {
                    let pointee = match base_ty.unqualified() {
                        IRType::Ptr(inner) => (**inner).clone(),
                        _ => return Err(crate::utils::Error::CodeGen("reference index base lost its pointer representation".into())),
                    };
                    base = self.load_address(base, base_ty);
                    base_ty = pointee;
                    source_ty = Some(*inner);
                }
                // Snapshot a pointer value before evaluating the index, which
                // may itself mutate the variable holding that pointer.
                if matches!(base_ty.unqualified(), IRType::Ptr(_)) {
                    base = self.load_address(base, base_ty.clone());
                }
                let index = self.generate_expr(index)?;
                let dest = self.alloc_register();
                let base_volatile = matches!(base_ty, IRType::Volatile(_));
                let (instruction, ty) = match base_ty.unqualified().clone() {
                    IRType::Array(element, size) => {
                        let ty = *element;
                        (Instruction::ArrayElementPtr { dest, ptr: base, index, array_ty: IRType::Array(Box::new(ty.clone()), size) }, if base_volatile { IRType::Volatile(Box::new(ty)) } else { ty })
                    }
                    IRType::Ptr(element) => {
                        let ty = *element;
                        (Instruction::GetElementPtr { dest, ptr: base, index, elem_ty: ty.clone() }, ty)
                    }
                    _ => return Err(crate::utils::Error::CodeGen("index requires an array or pointer".into())),
                };
                self.emit_current_with_type(instruction, IRType::Ptr(Box::new(ty.clone())));
                Ok((Value::Register(dest), ty))
            }
            Expr::Field { expr: base, field, .. } => {
                let (mut ptr, mut ty) = self.address_or_temporary(base)?;
                while let IRType::Ptr(inner) = ty.unqualified().clone() {
                    ptr = self.load_address(ptr, ty.clone());
                    ty = *inner;
                }
                let volatile = matches!(ty, IRType::Volatile(_));
                let name = match ty.unqualified().clone() {
                    IRType::Struct(name) => name,
                    _ => return Err(crate::utils::Error::CodeGen("field access requires a struct".into())),
                };
                let tuple_fields = self.tuple_fields(&name);
                let fields = tuple_fields.as_ref().or_else(|| self.struct_defs.get(&name));
                let requested = if tuple_fields.is_some() { format!("_{}", field.name) } else { field.name.clone() };
                let (index, ty) = fields
                    .and_then(|fields| fields.iter().enumerate().find(|(_, (n, _))| *n == requested))
                    .map(|(index, (_, ty))| (index, ty.clone()))
                    .ok_or_else(|| crate::utils::Error::UnknownField { span: field.span, field: field.name.clone() })?;
                let ty = if volatile { IRType::Volatile(Box::new(ty)) } else { ty };
                let dest = self.alloc_register();
                self.emit_current_with_type(Instruction::GetFieldPtr {
                    dest, ptr, field: index, struct_ty: IRType::Struct(name),
                }, IRType::Ptr(Box::new(ty.clone())));
                Ok((Value::Register(dest), ty))
            }
            _ => Err(crate::utils::Error::CodeGen("expression is not an assignment place".into())),
        }
    }

    /// Addressable expressions retain their storage; temporaries get a private value copy.
    fn address_or_temporary(&mut self, expr: &Expr) -> Result<(Value, IRType)> {
        if matches!(expr, Expr::Ident(_) | Expr::Deref { .. } | Expr::Index { .. } | Expr::Field { .. }) {
            return self.generate_place(expr);
        }
        let value = self.generate_expr(expr)?;
        let ty = self.get_value_type(&value).ok_or_else(|| crate::utils::Error::CodeGen("unknown temporary type".into()))?;
        Ok((self.allocate_local(&ty, Some(value)), ty))
    }

    /// Preserve the declared scalar width through IR, rather than letting the
    /// backends independently infer a local's type from an untyped literal.
    fn coerce_value(&mut self, value: Value, ty: &IRType) -> Value {
        let ty = ty.unqualified();
        if let Some(actual) = self.get_value_type(&value) {

            if let (IRType::Array(from, n), IRType::Array(to, m)) = (&actual, ty) {
                if n == m && from != to {
                    let source = self.allocate_local(&actual, Some(value));
                    let target = self.allocate_local(ty, None);
                    for i in 0..*n {
                        let src = self.alloc_register();
                        self.emit_current_with_type(Instruction::ArrayElementPtr { dest: src, ptr: source.clone(), index: Value::Constant(Constant::Int(i as i64)), array_ty: actual.clone() }, IRType::Ptr(from.clone()));
                        let element = self.load_address(Value::Register(src), (**from).clone());
                        let element = self.coerce_value(element, to);
                        let dst = self.alloc_register();
                        self.emit_current_with_type(Instruction::ArrayElementPtr { dest: dst, ptr: target.clone(), index: Value::Constant(Constant::Int(i as i64)), array_ty: ty.clone() }, IRType::Ptr(to.clone()));
                        self.emit_current(Instruction::Store { ptr: Value::Register(dst), value: element });
                    }
                    return self.load_address(target, ty.clone());
                }
            }
            if actual != *ty && ((Self::is_integer_type(&actual) && Self::is_integer_type(ty))
                || matches!((&actual, ty), (IRType::F32 | IRType::F64, IRType::F32 | IRType::F64))) {
                let dest = self.alloc_register();
                self.emit_current_with_type(Instruction::Cast { dest, value, ty: ty.clone() }, ty.clone());
                return Value::Register(dest);
            }
        }
        value
    }

    fn generate_short_circuit(&mut self, left: &Expr, op: ast::BinOp, right: &Expr) -> Result<Value> {
        let left_value = self.generate_expr(left)?;
        if self.get_current_terminator().is_some() { return Ok(Value::Unit); }
        let left_exit = self.current_block;
        let rhs_block = self.add_block("logical_rhs");
        let merge_block = self.add_block("logical_merge");
        let is_and = op == ast::BinOp::And;
        self.set_terminator_current(Terminator::Branch {
            cond: left_value,
            then_target: if is_and { rhs_block } else { merge_block },
            else_target: if is_and { merge_block } else { rhs_block },
        });
        self.current_block = rhs_block;
        let right_value = self.generate_expr(right)?;
        let right_exit = self.current_block;
        let mut incoming = vec![(Value::Constant(Constant::Bool(!is_and)), left_exit)];
        if self.get_current_terminator().is_none() {
            self.set_terminator_current(Terminator::Jump { target: merge_block });
            incoming.push((right_value, right_exit));
        }
        self.current_block = merge_block;
        let dest = self.alloc_register();
        self.emit_current_with_type(Instruction::Phi { dest, incoming }, IRType::Bool);
        Ok(Value::Register(dest))
    }

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
        if self.get_current_terminator().is_some() { return; }
        use crate::intrinsics::{MemoryKind, MemoryOrder};
        let function_volatile = self.current_fn.as_ref().is_some_and(|function| function.volatile);
        let inst = match inst {
            Instruction::BinOp { dest, op, left, right } if matches!(self.get_value_type(&left), Some(IRType::Vector(..))) => {
                use crate::intrinsics::VectorKind as K;
                let kind = match op { IRBinOp::Add => K::Add, IRBinOp::Sub => K::Sub, IRBinOp::Mul => K::Mul, IRBinOp::Div => K::Div, _ => unreachable!("checked vector arithmetic") };
                Instruction::Vector { dest: Some(dest), kind, ty: self.get_value_type(&left).unwrap(), args: vec![left, right] }
            }
            Instruction::Load { dest, ptr, ty } if function_volatile || matches!(ty, IRType::Volatile(_)) => Instruction::Memory {
                dest: Some(dest), kind: MemoryKind::VolatileRead, ty: ty.unqualified().clone(), args: vec![ptr],
                order: MemoryOrder::SeqCst, failure_order: MemoryOrder::SeqCst,
            },
            Instruction::Store { ptr, value } => {
                let Some(IRType::Ptr(ty)) = self.get_value_type(&ptr) else {
                    self.emission_error.get_or_insert_with(|| crate::utils::Error::CodeGen(
                        format!("missing pointer type for store destination {ptr:?}")));
                    return;
                };
                if function_volatile || matches!(*ty, IRType::Volatile(_)) { Instruction::Memory {
                    dest: None, kind: MemoryKind::VolatileWrite, ty: ty.unqualified().clone(), args: vec![ptr, value],
                    order: MemoryOrder::SeqCst, failure_order: MemoryOrder::SeqCst,
                } } else { Instruction::Store { ptr, value } }
            }
            other => other,
        };
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
               Instruction::Memory { dest: Some(dest), .. } |
               Instruction::Vector { dest: Some(dest), .. } |
               Instruction::Platform { dest, .. } |
               Instruction::Alloca { dest, .. } | 
               Instruction::Load { dest, .. } | 
               Instruction::GetElementPtr { dest, .. } |
               Instruction::ArrayElementPtr { dest, .. } |
               Instruction::GetFieldPtr { dest, .. } |
               Instruction::Layout { dest, .. } |
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
    
    fn require_value_type(&self, val: &Value) -> Result<IRType> {
        self.get_value_type(val)
            .ok_or_else(|| crate::utils::Error::CodeGen(format!("missing type for IR value {val:?}")))
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
            Value::Global(name) => self.global_types.get(name).map(|ty| IRType::Ptr(Box::new(ty.clone()))),
            Value::Unit => Some(IRType::Unit),
        }
    }

    fn set_terminator_current(&mut self, term: Terminator) {
        if self.get_current_terminator().is_some() { return; }
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

    fn lower_initializer(&self, initializer: &TypedInitializer) -> Result<IRInitializer> {
        Ok(match &initializer.value {
            InitializerValue::Zero => IRInitializer::Zero,
            InitializerValue::Scalar(value) => IRInitializer::Scalar(match value {
                ScalarConstant::Int(value) => Constant::Int(*value), ScalarConstant::Float(value) => Constant::Float(*value),
                ScalarConstant::Bool(value) => Constant::Bool(*value), ScalarConstant::Char(value) => Constant::Int(*value as i64),
                ScalarConstant::String(value) => Constant::String(value.clone()),
            }),
            InitializerValue::Repeat(value,count)=>IRInitializer::Repeat(Box::new(self.lower_initializer(value)?),*count),
            InitializerValue::Aggregate(values) => IRInitializer::Aggregate(values.iter().map(|(index, value)| Ok((*index, self.lower_initializer(value)?))).collect::<Result<_>>()?),
            InitializerValue::Address { global, path } => IRInitializer::Address { global: global.clone(), path: path.clone() },
            InitializerValue::Layout { query, ty, field } => IRInitializer::Layout {
                query: match query { ast::LayoutQuery::Size => IRLayoutQuery::Size, ast::LayoutQuery::Align => IRLayoutQuery::Align, ast::LayoutQuery::Offset => IRLayoutQuery::Offset },
                ty: Self::resolved_type_to_ir(ty)?, field: *field,
            },
        })
    }

    fn pointer_width(&self) -> crate::types::PointerWidth {
        self.module.pointer_width
    }

    fn pointer_integer(width: crate::types::PointerWidth, signed: bool) -> IRType {
        use crate::types::PointerWidth::*;
        match (width, signed) {
            (W32, true) => IRType::I32, (W32, false) => IRType::U32,
            (W64, true) => IRType::I64, (W64, false) => IRType::U64,
        }
    }

    fn resolved_type_to_ir(ty: &ResolvedType) -> Result<IRType> {
        use PrimitiveType as P;
        Ok(match ty {
            ResolvedType::Primitive(primitive) => match primitive {
                P::I8 => IRType::I8, P::I16 => IRType::I16, P::I32 => IRType::I32, P::I64 => IRType::I64, P::Isize(width) => Self::pointer_integer(*width, true),
                P::U8 => IRType::U8, P::U16 => IRType::U16, P::U32 | P::Char => IRType::U32, P::U64 => IRType::U64, P::Usize(width) => Self::pointer_integer(*width, false),
                P::F32 => IRType::F32, P::F64 => IRType::F64, P::Bool => IRType::Bool, P::Unit => IRType::Unit, P::Never => IRType::Void,
            },
            ResolvedType::Pointer(inner) | ResolvedType::Reference { inner, .. } => IRType::Ptr(Box::new(Self::resolved_type_to_ir(inner)?)),
            ResolvedType::Volatile(inner) => IRType::Volatile(Box::new(Self::resolved_type_to_ir(inner)?)),
            ResolvedType::Owned(inner) => Self::resolved_type_to_ir(inner)?,
            ResolvedType::Array { elem, size } => IRType::Array(Box::new(Self::resolved_type_to_ir(elem)?), *size),
            ResolvedType::Struct { name, .. } | ResolvedType::Enum { name } => IRType::Struct(name.clone()),
            ResolvedType::String => IRType::Ptr(Box::new(IRType::U8)),
            ResolvedType::Vector(inner, lanes) => IRType::Vector(Box::new(Self::resolved_type_to_ir(inner)?), *lanes),
            ResolvedType::Function { params, ret } => IRType::Function { params: params.iter().map(Self::resolved_type_to_ir).collect::<Result<_>>()?, ret: Box::new(Self::resolved_type_to_ir(ret)?) },
            ResolvedType::Tuple(elements) if elements.is_empty() => IRType::Unit,
            ResolvedType::Tuple(elements) => Self::tuple_ir_type(&elements.iter().map(Self::resolved_type_to_ir).collect::<Result<Vec<_>>>()?),
            _ => return Err(crate::utils::Error::CodeGen(format!("lowering resolved type {:?} is not implemented", ty))),
        })
    }

    fn ast_type_to_ir(&self, ty: &AstType) -> Result<IRType> {
        Ok(match ty {
            AstType::Named(name, _) => {
                if let Some(ty) = self.resolved_aliases.get(name) { return Ok(ty.clone()); }
                if let Some(ty) = ResolvedType::core_builtin(name, self.pointer_width()) {
                    return Self::resolved_type_to_ir(&ty);
                }
                if !self.nominal_types.contains(name) {
                    return Err(crate::utils::Error::CodeGen(format!("missing resolved declaration for type '{name}'")));
                }
                IRType::Struct(name.clone())
            }
            AstType::Pointer(inner, _) | AstType::Ref { inner, .. } => IRType::Ptr(Box::new(self.ast_type_to_ir(inner)?)),
            AstType::Volatile(inner, _) => IRType::Ptr(Box::new(IRType::Volatile(Box::new(self.ast_type_to_ir(inner)?)))),
            AstType::Array { elem, size, .. } => IRType::Array(Box::new(self.ast_type_to_ir(elem)?), *size),
            AstType::Tuple(elements, _) if elements.is_empty() => IRType::Unit,
            AstType::Tuple(elements, _) => self.record_tuple_layout(elements.iter().map(|elem| self.ast_type_to_ir(elem)).collect::<Result<_>>()?),
            AstType::Unit(_) => IRType::Unit,
            AstType::Never(_) => IRType::Void,
            AstType::Owned { inner, .. } => return self.ast_type_to_ir(inner),
            other => return Err(crate::utils::Error::CodeGen(format!("type has no concrete representation after checking at {:?}", other.span()))),
        })
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
        let gen = IRGenerator::new("test");
        gen.generate(&program)
    }

    #[test]
    fn static_linkage_and_compatible_foreign_declarations_survive_lowering() {
        let declaration = "type Count=i64; extern \"C\" { static mut ABI:Count; }";
        let definition = "#[no_mangle] static mut ABI:i64=7;";
        for source in [format!("{declaration} {definition}"), format!("{definition} {declaration}")] {
            let source = format!("{source} static PRIVATE:i64=1; pub static PUBLIC:i64=2;");
            let program = Parser::new(Lexer::new(&source, 0)).parse_program().unwrap();
            let mut analyzer = crate::frontend::semantic::SemanticAnalyzer::new();
            analyzer.analyze(&program).unwrap();
            let module = IRGenerator::new("test").with_semantic_facts(analyzer.facts().clone()).generate(&program).unwrap();
            assert_eq!(module.globals.len(), 3);
            let abi = module.globals.iter().find(|global| global.name == "ABI").unwrap();
            assert!(abi.exported && abi.mutable && abi.initializer.is_some());
            assert!(!module.globals.iter().find(|global| global.name == "PRIVATE").unwrap().exported);
            assert!(module.globals.iter().find(|global| global.name == "PUBLIC").unwrap().exported);
        }
    }

    #[test]
    fn c_abi_declaration_and_definition_emit_one_function() {
        let declaration = "extern \"C\" { fn callback(argument: i32) -> i32; fn other() -> i32; }";
        let definition = "#[no_mangle] pub fn callback(value: i32) -> i32 { value + 1 }";
        for source in [format!("{declaration} {definition}"), format!("{definition} {declaration}")] {
            let module = generate(&source).unwrap();
            assert_eq!(module.functions.iter().filter(|function| function.name == "callback").count(), 1);
            assert!(!module.externs.iter().any(|declaration| declaration.name == "callback"));
            assert!(module.externs.iter().any(|declaration| declaration.name == "other"));
            let function = module.functions.iter().find(|function| function.name == "callback").unwrap();
            assert!(function.c_abi);
            assert_eq!(function.params, vec![("value".into(), IRType::I32)]);
            assert_eq!(function.ret_type, IRType::I32);
        }
    }

    #[test]
    fn c_abi_unit_return_spelling_does_not_change_calls_or_declarations() {
        for declaration_return in ["", "-> ()"] {
            for definition_return in ["", "-> ()"] {
                let source = format!(
                    "extern \"C\" {{ fn callback() {declaration_return}; }}
                     #[no_mangle] fn callback() {definition_return} {{ return; }}
                     fn caller() {{ callback(); }} fn language_unit() -> () {{ () }}");
                let module = generate(&source).unwrap();
                assert!(!module.externs.iter().any(|declaration| declaration.name == "callback"));
                let callback = module.functions.iter().find(|function| function.name == "callback").unwrap();
                assert_eq!(callback.ret_type, IRType::Void);
                assert!(callback.blocks.iter().any(|block| matches!(block.terminator, Some(Terminator::Return { value: None }))));
                let caller = module.functions.iter().find(|function| function.name == "caller").unwrap();
                assert!(caller.blocks.iter().flat_map(|block| &block.instructions)
                    .any(|instruction| matches!(instruction, Instruction::Call { dest: None, func, .. } if func == "callback")));
                assert_eq!(module.functions.iter().find(|function| function.name == "language_unit").unwrap().ret_type, IRType::Unit);
            }
        }
        for source in [
            "extern \"C\" { fn callback(); fn callback() -> (); }",
            "extern \"C\" { fn callback() -> (); fn callback(); }",
        ] {
            let module = generate(source).unwrap();
            let declarations: Vec<_> = module.externs.iter().filter(|declaration| declaration.name == "callback").collect();
            assert_eq!(declarations.len(), 1);
            assert_eq!(declarations[0].ret_type, IRType::Void);
        }
    }

    #[test]
    fn inconsistent_c_abi_ir_cannot_hide_behind_a_definition() {
        // These inputs bypass semantic checking deliberately: the IR boundary
        // must reject stale or hand-built signatures rather than erase them.
        for source in [
            "extern \"C\" { fn callback(value: i32) -> i32; } #[no_mangle] fn callback(value: i64) -> i32 { 0 }",
            "extern \"C\" { fn callback() -> i64; } #[no_mangle] fn callback() -> i32 { 0 }",
            "extern \"C\" { fn callback(value: i32, ...) -> i32; } #[no_mangle] fn callback(value: i32) -> i32 { value }",
            "#[no_mangle] fn malloc(size: i32) -> i32 { 0 }",
        ] {
            let error = generate(source).unwrap_err();
            assert!(error.to_string().contains("inconsistent C ABI declaration and definition"), "{error}");
        }
        let error = generate("#[no_mangle] fn callback() {} #[no_mangle] fn callback() {}").unwrap_err();
        assert!(error.to_string().contains("duplicate C ABI definition"), "{error}");
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
    #[test]
    fn error_propagation_rejects_non_result_operands() {
        let error = generate("fn f(x: i32) -> i32 { return x?; }").unwrap_err();
        assert!(error.to_string().contains("error propagation"));
    }

    #[test]
    fn generic_functions_are_not_erased_to_i64() {
        let error = generate("fn identity<T>(value: T) -> T { return value; }").unwrap_err();
        assert!(error.to_string().contains("specialization"));
    }

    #[test]
    fn missing_declarations_and_types_are_ir_errors() {
        for source in ["fn f(x: Missing) {}", "fn f(){ absent(); }", "fn f(x:[i32]){}"] {
            assert!(generate(source).is_err(), "{source}");
        }
    }

    #[test]
    fn tuple_values_are_not_lowered_to_unit() {
        let module = generate("fn f() -> (i64, i64) { (1, 2) }").unwrap();
        let function = module.functions.iter().find(|function| function.name == "f").unwrap();
        let IRType::Struct(name) = &function.ret_type else { panic!("tuple lost its aggregate type") };
        let layout = module.structs.iter().find(|layout| layout.name == *name).unwrap();
        assert_eq!(layout.fields, vec![("_0".into(), IRType::I64), ("_1".into(), IRType::I64)]);
    }

    #[test]
    fn closures_are_not_silently_emitted_without_capture_or_call_support() {
        // Semantic analysis rejects closure values; guard the separate AST
        // lowering entry as well so programmatically created ASTs cannot revive
        // the old placeholder closure implementation.
        let span = crate::utils::Span::dummy();
        let expression = Expr::Closure { params: vec![], ret_type: None,
            body: Box::new(Expr::Literal(ast::Literal::Int(1, span))), span };
        let error = IRGenerator::new("test").generate_expr(&expression).unwrap_err();
        assert!(error.to_string().contains("closure lowering"), "{error}");
    }

}
