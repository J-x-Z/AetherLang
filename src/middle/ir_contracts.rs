//! Contracts are executable control flow, shared by all backends and opt levels.
use super::*;

#[derive(Default)]
pub(super) struct ReturnContracts {
    ensures: Vec<Expr>,
    // Capture lexical identities, not values: mutable parameters retain their
    // final value, while a body-local shadow cannot capture the predicate.
    parameters: HashMap<String, (Value, IRType)>,
    addresses: HashMap<String, Value>,
}

impl IRGenerator {
    pub(super) fn begin_contracts(&mut self, function: &ast::Function) -> Result<()> {
        self.return_contracts = ReturnContracts {
            ensures: function.contracts.iter().filter(|c| c.kind == ast::ContractKind::Ensures)
                .map(|c| c.condition.clone()).collect(),
            parameters: self.locals.clone(),
            addresses: self.local_addresses.clone(),
        };
        for contract in &function.contracts {
            if contract.kind == ast::ContractKind::Requires { self.emit_contract_check(&contract.condition)?; }
        }
        Ok(())
    }

    fn emit_contract_check(&mut self, predicate: &Expr) -> Result<()> {
        let condition = self.generate_expr_expected(predicate, Some(&IRType::Bool))?;
        if self.get_current_terminator().is_some() {
            return Err(crate::utils::Error::CodeGen("checked contract predicate unexpectedly diverged".into()));
        }
        let valid = self.add_block("contract_pass");
        let invalid = self.add_block("contract_fail");
        self.set_terminator_current(Terminator::Branch { cond: condition, then_target: valid, else_target: invalid });
        self.current_block = invalid;
        self.set_terminator_current(Terminator::Trap);
        self.current_block = valid;
        Ok(())
    }

    /// `value` is already evaluated. Both explicit/implicit returns and `?`
    /// early returns use this path; a diverging expression has no normal exit.
    pub(super) fn emit_checked_return(&mut self, value: Option<Value>) -> Result<()> {
        if self.get_current_terminator().is_some() { return Ok(()); }
        if !self.return_contracts.ensures.is_empty() {
            let saved_locals = std::mem::replace(&mut self.locals, self.return_contracts.parameters.clone());
            let saved_addresses = std::mem::replace(&mut self.local_addresses, self.return_contracts.addresses.clone());
            let ty = self.current_fn.as_ref().unwrap().ret_type.clone();
            let ty = if ty == IRType::Void { IRType::Unit } else { ty };
            let result_address = self.allocate_local(&ty, Some(value.clone().unwrap_or(Value::Unit)));
            self.locals.insert("result".into(), (result_address.clone(), ty));
            self.local_addresses.insert("result".into(), result_address);
            let checked: Result<()> = (|| {
                for predicate in self.return_contracts.ensures.clone() { self.emit_contract_check(&predicate)?; }
                Ok(())
            })();
            self.locals = saved_locals;
            self.local_addresses = saved_addresses;
            checked?;
        }
        self.set_terminator_current(Terminator::Return { value });
        Ok(())
    }
}
