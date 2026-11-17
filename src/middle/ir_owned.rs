//! Explicit destructor calls on normal lexical exits, shared by C and LLVM.
use super::*;
use crate::utils::Span;

#[derive(Clone)]
struct Resource {
    address: Value,
    flag: Value,
    ty: IRType,
    destructor: String,
    depth: usize,
    temporary: bool,
}
#[derive(Default)]
pub(super) struct OwnedScopes {
    resources: Vec<Resource>,
    bindings: HashMap<Span, usize>,
    temporaries: HashMap<Register, usize>,
    pub depth: usize,
    pub loops: Vec<usize>,
    destructor_parameter: Option<Span>,
}

impl IRGenerator {
    fn owned_destructor(&self, ty: &ResolvedType) -> Option<String> {
        let ResolvedType::Owned(inner) = ty else {
            return None;
        };
        self.semantic_facts
            .as_ref()?
            .drop_functions
            .get(&crate::frontend::semantic::SemanticAnalyzer::type_identity(
                inner,
            ))
            .cloned()
    }

    pub(super) fn begin_owned(&mut self, function: &ast::Function) {
        self.owned = OwnedScopes::default();
        if function.annotations.iter().any(|a| a.name.name == "drop") {
            self.owned.destructor_parameter = function.params.first().map(|p| p.name.span);
        }
    }

    fn owned_binding_type(&self, span: Span) -> Option<ResolvedType> {
        let key = (self.current_fn.as_ref()?.name.clone(), span);
        self.semantic_facts
            .as_ref()?
            .owned_bindings
            .get(&key)
            .cloned()
    }

    // An exit can follow a branch where the initializer never ran. Both the
    // liveness flag and storage address must dominate that cleanup CFG.
    pub(super) fn allocate_owned_storage(&mut self, ty: &IRType, value: Option<Value>) -> Value {
        let dest = self.alloc_register();
        self.reg_types
            .insert(dest, IRType::Ptr(Box::new(ty.clone())));
        self.current_fn.as_mut().unwrap().blocks[0]
            .instructions
            .insert(
                0,
                Instruction::Alloca {
                    dest,
                    ty: ty.clone(),
                },
            );
        let address = Value::Register(dest);
        if let Some(value) = value {
            self.emit_current(Instruction::Store {
                ptr: address.clone(),
                value,
            });
        }
        address
    }

    pub(super) fn owner_local_storage(
        &mut self,
        span: Span,
        ty: &IRType,
        value: Option<Value>,
    ) -> Value {
        if self
            .owned_binding_type(span)
            .as_ref()
            .and_then(|t| self.owned_destructor(t))
            .is_some()
        {
            self.allocate_owned_storage(ty, value)
        } else {
            self.allocate_local(ty, value)
        }
    }

    fn new_resource(
        &mut self,
        address: Value,
        ty: IRType,
        destructor: String,
        temporary: bool,
    ) -> usize {
        let flag = self.allocate_owned_storage(&IRType::Bool, None);
        // Initialize in entry, including resources declared only on one path.
        self.current_fn.as_mut().unwrap().blocks[0]
            .instructions
            .insert(
                1,
                Instruction::Store {
                    ptr: flag.clone(),
                    value: Value::Constant(Constant::Bool(false)),
                },
            );
        let index = self.owned.resources.len();
        self.owned.resources.push(Resource {
            address,
            flag,
            ty,
            destructor,
            depth: self.owned.depth,
            temporary,
        });
        index
    }

    fn set_owned_live(&mut self, index: usize, live: bool) {
        self.emit_current(Instruction::Store {
            ptr: self.owned.resources[index].flag.clone(),
            value: Value::Constant(Constant::Bool(live)),
        });
    }

    pub(super) fn bind_owned(
        &mut self,
        span: Span,
        address: Value,
        initialized: bool,
    ) -> Result<()> {
        if self.owned.destructor_parameter == Some(span) {
            return Ok(());
        }
        let Some(ty) = self.owned_binding_type(span) else {
            return Ok(());
        };
        let Some(destructor) = self.owned_destructor(&ty) else {
            return Ok(());
        };
        let ty = Self::resolved_type_to_ir(&ty)?;
        let index = self.new_resource(address, ty, destructor, false);
        self.owned.bindings.insert(span, index);
        self.set_owned_live(index, initialized);
        Ok(())
    }

    pub(super) fn move_owned_binding(&mut self, expr: &Expr) {
        let Some(function) = self.current_fn.as_ref() else {
            return;
        };
        let binding = self
            .semantic_facts
            .as_ref()
            .and_then(|f| f.owned_moves.get(&(function.name.clone(), expr.span())))
            .copied();
        if let Some(index) = binding.and_then(|span| self.owned.bindings.get(&span).copied()) {
            self.set_owned_live(index, false);
        }
    }

    pub(super) fn initialize_owned_place(&mut self, address: &Value) {
        let Value::Register(register) = address else {
            return;
        };
        let index = self.owned.resources.iter().position(|r| {
            !r.temporary && matches!(r.address,Value::Register(value) if value==*register)
        });
        if let Some(index) = index {
            self.set_owned_live(index, true);
        }
    }

    pub(super) fn remember_owned_temporary(
        &mut self,
        value: &Value,
        ty: &ResolvedType,
    ) -> Result<()> {
        let Some(destructor) = self.owned_destructor(ty) else {
            return Ok(());
        };
        let Value::Register(register) = value else {
            return Err(crate::utils::Error::CodeGen(
                "owned aggregate did not produce a register value".into(),
            ));
        };
        let index = if let Some(index) = self.owned.temporaries.get(register).copied() {
            self.owned.resources[index].depth = self.owned.depth;
            index
        } else {
            let ty = Self::resolved_type_to_ir(ty)?;
            let address = self.allocate_owned_storage(&ty, Some(value.clone()));
            let index = self.new_resource(address, ty, destructor, true);
            self.owned.temporaries.insert(*register, index);
            index
        };
        self.set_owned_live(index, true);
        Ok(())
    }

    pub(super) fn transfer_owned_value(&mut self, value: &Value) {
        if let Value::Register(register) = value {
            if let Some(index) = self.owned.temporaries.get(register).copied() {
                self.set_owned_live(index, false);
            }
        }
    }

    pub(super) fn promote_owned_value(&mut self, value: &Value) {
        if let Value::Register(register) = value {
            if let Some(index) = self.owned.temporaries.get(register).copied() {
                self.owned.resources[index].depth = self.owned.depth.saturating_sub(1);
            }
        }
    }

    fn emit_resource_drop(&mut self, index: usize) -> Result<()> {
        if self.get_current_terminator().is_some() {
            return Ok(());
        }
        let resource = self.owned.resources[index].clone();
        let condition = self.load_address(resource.flag.clone(), IRType::Bool);
        let drop = self.add_block("owner_drop");
        let next = self.add_block("owner_drop_done");
        self.set_terminator_current(Terminator::Branch {
            cond: condition,
            then_target: drop,
            else_target: next,
        });
        self.current_block = drop;
        // Clear before entering user code, so a destructor cannot re-enter this
        // same cleanup with the slot still claiming ownership.
        self.set_owned_live(index, false);
        let value = self.load_address(resource.address, resource.ty);
        self.emit_current_with_type(
            Instruction::Call {
                dest: None,
                func: resource.destructor,
                args: vec![value],
            },
            IRType::Unit,
        );
        self.set_terminator_current(Terminator::Jump { target: next });
        self.current_block = next;
        Ok(())
    }

    pub(super) fn cleanup_owned_scope(&mut self, depth: usize) -> Result<()> {
        for index in (0..self.owned.resources.len()).rev() {
            if self.owned.resources[index].depth >= depth {
                self.emit_resource_drop(index)?;
            }
        }
        Ok(())
    }

    pub(super) fn cleanup_owned_temporaries(
        &mut self,
        start: usize,
        depth: usize,
        keep: Option<&Value>,
    ) -> Result<()> {
        let keep = keep.and_then(|value| {
            if let Value::Register(register) = value {
                self.owned.temporaries.get(register).copied()
            } else {
                None
            }
        });
        for index in (start..self.owned.resources.len()).rev() {
            let resource = &self.owned.resources[index];
            if resource.temporary && resource.depth >= depth && keep != Some(index) {
                self.emit_resource_drop(index)?;
            }
        }
        Ok(())
    }

    pub(super) fn owned_checkpoint(&self) -> usize {
        self.owned.resources.len()
    }
}
