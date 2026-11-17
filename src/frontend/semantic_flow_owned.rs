//! Affine transient values use the same declaration identities as lexical owners.
use super::*;

impl Flow<'_> {
    pub(super) fn consume_owned_value(&mut self, value: &Value) {
        for id in &value.owners {
            if let Some(slot) = self.state.slots.get_mut(id) {
                slot.owned_live = false;
            }
        }
    }

    pub(super) fn record_owned_drop(&mut self, id: BindingId) {
        if self.destructor_parameter == Some(self.bindings[id].declaration) {
            return;
        }
        if self
            .state
            .slots
            .get(&id)
            .is_some_and(|slot| slot.owned_live)
        {
            if let Some(function) = self.analyzer.destructor_for(&self.bindings[id].ty) {
                self.drop_calls.insert(function.clone());
            }
        }
    }

    pub(super) fn record_owned_cleanup(&mut self, depth: usize) {
        let live: Vec<_> = self
            .state
            .slots
            .keys()
            .copied()
            .filter(|id| self.bindings[*id].depth >= depth)
            .collect();
        for id in live {
            self.record_owned_drop(id);
        }
    }

    pub(super) fn cleanup_owned_temporaries(&mut self, depth: usize, keep: Option<&Value>) {
        let live: Vec<_> = self
            .state
            .slots
            .keys()
            .copied()
            .filter(|id| {
                self.bindings[*id].depth >= depth
                    && self.bindings[*id].name.starts_with("$owner_temporary_")
                    && !keep.is_some_and(|value| value.owners.contains(id))
            })
            .collect();
        for id in live {
            self.record_owned_drop(id);
            if let Some(slot) = self.state.slots.get_mut(&id) {
                slot.owned_live = false;
            }
        }
    }

    pub(super) fn condition(
        &mut self,
        expression: &Expr,
        live: &HashSet<String>,
    ) -> Result<Option<Value>> {
        let before: HashSet<_> = self.state.slots.keys().copied().collect();
        let result = self.expr(expression, true, live)?;
        if result.is_some() {
            let new: Vec<_> = self
                .state
                .slots
                .keys()
                .copied()
                .filter(|id| {
                    !before.contains(id) && self.bindings[*id].name.starts_with("$owner_temporary_")
                })
                .collect();
            for id in new {
                self.record_owned_drop(id);
                if let Some(slot) = self.state.slots.get_mut(&id) {
                    slot.owned_live = false;
                }
            }
        }
        Ok(result)
    }

    pub(super) fn expr(
        &mut self,
        expression: &Expr,
        consume: bool,
        live: &HashSet<String>,
    ) -> Result<Option<Value>> {
        let Some(mut value) = self.expr_inner(expression, consume, live)? else {
            return Ok(None);
        };
        if consume && matches!(value.ty, ResolvedType::Owned(_)) {
            self.consume_owned_value(&value);
            value.owners.clear();
            let span = expression.span();
            let name = Ident {
                name: format!("$owner_temporary_{}_{}", span.start, span.end),
                span,
            };
            let id = self.declare(
                &name,
                value.ty.clone(),
                Some(Value::empty(value.ty.clone())),
            );
            value.owners.insert(id);
        }
        Ok(Some(value))
    }
}
