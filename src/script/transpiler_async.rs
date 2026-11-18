//! Script async syntax feeds the exact Core frame lowering. The hosted
//! block_on adapter polls a fresh task; it is not a second async interpreter.
use super::*;
const AWAIT: &str = "@async_await";
impl Transpiler {
    fn async_function(&self, name: &str) -> bool {
        self.definitions.get(name).is_some_and(|f| f.is_async)
            || self.imported.async_functions.contains(name)
    }
    fn async_parameters(&self, name: &str) -> Result<Vec<ScriptType>> {
        if let Some(function) = self.definitions.get(name) {
            return function
                .params
                .iter()
                .map(|p| {
                    Self::map_type(
                        p.type_hint
                            .as_ref()
                            .ok_or("async parameters need explicit types")?,
                    )
                })
                .collect();
        }
        self.imported
            .functions
            .get(name)
            .map(|(params, _)| params.clone())
            .ok_or_else(|| format!("unknown async function '{name}'"))
    }
    pub(super) fn register_async_imports(&mut self) {
        let mut names = self
            .imported
            .async_functions
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        names.sort();
        for name in names {
            let helper = self.fresh_name("block_on");
            self.async_helpers.insert(name, helper);
        }
    }
    fn async_constructor<'a>(&self, expr: &'a Expr) -> Result<(&'a str, &'a [Expr])> {
        let Expr::Call { func, args, .. } = expr else {
            return Err("await/block_on requires a direct async constructor".into());
        };
        let Expr::Identifier { name, .. } = &**func else {
            return Err("a direct named async function is required".into());
        };
        if self.locals.contains_key(name) || !self.async_function(name) {
            return Err(format!("'{name}' is not an async function"));
        }
        Ok((name, args))
    }
    fn render_async_constructor(&self, name: &str, args: &[Expr]) -> Result<String> {
        self.check_named_callbacks(name, args)?;
        let expected = self.async_parameters(name)?.len();
        if args.len() != expected {
            return Err(format!(
                "async function '{name}' requires {expected} arguments"
            ));
        }
        Ok(format!(
            "{}({})",
            name,
            args.iter()
                .map(|a| self.expression(a))
                .collect::<Result<Vec<_>>>()?
                .join(", ")
        ))
    }
    pub(super) fn async_expression(&self, expr: &Expr) -> Result<Option<(ScriptType, String)>> {
        let Expr::Call { func, args, .. } = expr else {
            return Ok(None);
        };
        let Expr::Identifier { name, .. } = &**func else {
            return Ok(None);
        };
        if name == AWAIT {
            if !self.current_async {
                return Err("await only inside an async def".into());
            }
            let [value] = &args[..] else {
                return Err("await takes one task expression".into());
            };
            if !self.functions.contains_key("yield_now")
                && !self.locals.contains_key("yield_now")
                && matches!(value,Expr::Call{func,args,..} if args.is_empty()&&matches!(&**func,Expr::Identifier{name,..} if name=="yield_now"))
            {
                return Ok(Some(("void".into(), "await yield_now()".into())));
            }
            let (name, args) = self.async_constructor(value)?;
            return Ok(Some((
                self.functions[name].clone(),
                format!("await {}", self.render_async_constructor(name, args)?),
            )));
        }
        if name == "block_on"
            && !self.functions.contains_key(name)
            && !self.locals.contains_key(name)
        {
            let [value] = &args[..] else {
                return Err("block_on requires one fresh async constructor".into());
            };
            let (name, args) = self.async_constructor(value)?;
            let call = self.render_async_constructor(name, args)?;
            let suffix = call.strip_prefix(name).unwrap();
            return Ok(Some((
                self.functions[name].clone(),
                format!(
                    "{}{suffix}",
                    self.async_helpers
                        .get(name)
                        .ok_or("missing async adapter in the current Script context")?
                ),
            )));
        }
        if !self.locals.contains_key(name) && self.async_function(name) {
            return Err("an async call creates a task; use await in async code or block_on(fresh_task) in Script".into());
        }
        Ok(None)
    }
    pub(super) fn emit_async_helpers(&mut self) -> Result<()> {
        let mut names = self.async_helpers.keys().cloned().collect::<Vec<_>>();
        names.sort();
        for name in names {
            self.source_line = self.definitions.get(&name).map_or(1, |f| f.span.line);
            let mut parameters = Vec::new();
            let mut arguments = Vec::new();
            for ty in self.async_parameters(&name)? {
                let internal = self.fresh_name("async_arg");
                parameters.push(format!("{internal}: {ty}"));
                arguments.push(internal);
            }
            let task = self.fresh_name("async_task");
            let value = self.fresh_name("async_result");
            self.line(&format!(
                "fn {}({})->{} {{",
                self.async_helpers[&name],
                parameters.join(","),
                self.functions[&name]
            ));
            self.indent = 1;
            self.line(&format!(
                "let mut {task}:_={name}({});",
                arguments.join(",")
            ));
            self.line(&format!("loop {{match {task}.poll() {{Pending()=>{{}},Ready({value})=>{{return {value};}},Finished()=>{{exit(70);}},}}}}"));
            self.indent = 0;
            self.line("}");
        }
        Ok(())
    }
}
