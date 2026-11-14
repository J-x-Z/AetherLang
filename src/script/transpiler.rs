//! Checked lowering of Aether Script into Aether Core.
//!
//! Script locals are mutable and function-scoped. Definite assignment is
//! checked before lowering, so a branch/zero-iteration loop cannot expose an
//! uninitialized local. The generated program then uses the normal Core pipeline.

use std::collections::{BTreeMap, HashMap, HashSet};
use super::ast::*;

type Result<T> = std::result::Result<T, String>;

pub struct Transpiler {
    output: String,
    pub source_lines: Vec<usize>,
    source_line: usize,
    indent: usize,
    source_file: Option<String>,
    functions: HashMap<String, String>,
    locals: BTreeMap<String, String>,
    assigned: HashSet<String>,
    return_type: String,
    is_main: bool,
    loop_depth: usize,
    temporary: usize,
}

impl Transpiler {
    pub fn new() -> Self {
        Self { output: String::new(), source_lines: Vec::new(), source_line: 1, indent: 0, source_file: None,
            functions: HashMap::new(), locals: BTreeMap::new(), assigned: HashSet::new(),
            return_type: "void".into(), is_main: false, loop_depth: 0, temporary: 0 }
    }

    pub fn with_source_file(mut self, path: &str) -> Self {
        self.source_file = Some(path.into());
        self
    }

    pub fn transpile(&mut self, module: &ScriptModule) -> Result<String> {
        self.output.clear();
        self.source_lines.clear();
        self.functions.clear();
        for statement in &module.stmts {
            let Stmt::FunctionDef(function) = statement else {
                return Err("Script top-level statements must be function definitions".into());
            };
            let ret = if let Some(hint) = &function.return_type { Self::map_type(hint)? }
                else if function.name == "main" { "i32".into() } else { "void".into() };
            if self.functions.insert(function.name.clone(), ret).is_some() {
                return Err(format!("duplicate Script function '{}'", function.name));
            }
        }
        self.line("// Generated Aether Core; source semantics checked before lowering.");
        if let Some(source) = self.source_file.clone() {
            self.line(&format!("// Source: {}", source.replace(['\n', '\r'], " ")));
        }
        self.line("extern \"C\" {");
        self.line("    fn puts(s: *u8) -> i32;");
        self.line("    fn printf(fmt: *u8, ...) -> i32;");
        self.line("}");
        for statement in &module.stmts {
            if let Stmt::FunctionDef(function) = statement { self.function(function)?; }
        }
        Ok(std::mem::take(&mut self.output))
    }

    fn function(&mut self, function: &FunctionDef) -> Result<()> {
        if function.is_comptime { return Err("Script @comptime execution is not implemented".into()); }
        self.source_line = function.span.line;
        self.locals.clear();
        self.assigned.clear();
        self.loop_depth = 0;
        self.is_main = function.name == "main";
        self.return_type = self.functions[&function.name].clone();
        let mut parameters = Vec::new();
        let mut copies = Vec::new();
        for param in &function.params {
            let hint = param.type_hint.as_ref().ok_or_else(|| format!("Script parameter '{}' needs a type annotation", param.name))?;
            let ty = Self::map_type(hint)?;
            if self.locals.insert(param.name.clone(), ty.clone()).is_some() {
                return Err(format!("duplicate parameter '{}'", param.name));
            }
            self.assigned.insert(param.name.clone());
            let internal = format!("__aeth_script_arg_{}", param.name);
            parameters.push(format!("{}: {}", internal, ty));
            copies.push(format!("{} = {};", param.name, internal));
        }
        let outer_output = std::mem::take(&mut self.output);
        let outer_lines = std::mem::take(&mut self.source_lines);
        self.indent = 1;
        for copy in copies { self.line(&copy); }
        self.statements(&function.body)?;
        if self.is_main && !Self::ends_in_return(&function.body) { self.line("return 0;"); }
        let body = std::mem::replace(&mut self.output, outer_output);
        let body_lines = std::mem::replace(&mut self.source_lines, outer_lines);
        self.source_line = function.span.line;
        self.indent = 0;
        self.line(&format!("fn {}({}) -> {} {{", function.name, parameters.join(", "), self.return_type));
        self.indent = 1;
        for (name, ty) in self.locals.clone() { self.line(&format!("let mut {}: {};", name, ty)); }
        self.output.push_str(&body);
        self.source_lines.extend(body_lines);
        self.indent = 0;
        self.line("}");
        Ok(())
    }

    fn ends_in_return(statements: &[Stmt]) -> bool {
        match statements.last() {
            Some(Stmt::Return(_)) => true,
            Some(Stmt::If(branch)) => branch.else_block.as_ref().is_some_and(|other|
                Self::ends_in_return(&branch.then_block) && Self::ends_in_return(other)),
            _ => false,
        }
    }

    fn statements(&mut self, statements: &[Stmt]) -> Result<()> {
        for statement in statements { self.statement(statement)?; }
        Ok(())
    }

    fn statement(&mut self, statement: &Stmt) -> Result<()> {
        self.source_line = match statement {
            Stmt::FunctionDef(x) => x.span.line, Stmt::If(x) => x.span.line,
            Stmt::While(x) => x.span.line, Stmt::For(x) => x.span.line,
            Stmt::Return(x) => x.span.line, Stmt::Assign(x) => x.span.line,
            Stmt::Expr(x) => x.span().line, _ => self.source_line,
        };
        self.statement_inner(statement).map_err(|message| {
            if message.starts_with("line ") { message } else { format!("line {}: {}", self.source_line, message) }
        })
    }

    fn statement_inner(&mut self, statement: &Stmt) -> Result<()> {
        match statement {
            Stmt::Assign(assignment) => {
                let Expr::Identifier { name, .. } = &assignment.target else {
                    return Err("Script assignment currently requires a local variable".into());
                };
                let value = self.expression(&assignment.value)?;
                let ty = self.infer_type(&assignment.value)?;
                if ty == "void" { return Err(format!("cannot assign a void result to '{}'", name)); }
                self.locals.entry(name.clone()).or_insert(ty);
                self.assigned.insert(name.clone());
                self.line(&format!("{} = {};", name, value));
            }
            Stmt::Expr(expr) => { let value = self.expression(expr)?; self.line(&format!("{};", value)); }
            Stmt::Return(ret) => {
                if let Some(value) = &ret.value {
                    if self.return_type == "void" { return Err("a Script function returning a value needs a return type annotation".into()); }
                    let code = self.expression(value)?;
                    // Script int is i64; the native process exit ABI is i32.
                    if self.is_main && self.return_type == "i32" && self.infer_type(value)? == "i64" {
                        self.line(&format!("return ({}) as i32;", code));
                    } else { self.line(&format!("return {};", code)); }
                } else { self.line("return;"); }
            }
            Stmt::If(branch) => {
                let condition = self.expression(&branch.condition)?;
                let before = self.assigned.clone();
                self.line(&format!("if {} {{", condition)); self.indent += 1;
                self.statements(&branch.then_block)?;
                let after_then = self.assigned.clone();
                self.indent -= 1;
                self.assigned = before;
                if let Some(other) = &branch.else_block {
                    self.line("} else {"); self.indent += 1;
                    self.statements(other)?; self.indent -= 1;
                }
                self.assigned = if Self::ends_in_return(&branch.then_block) {
                    self.assigned.clone()
                } else if branch.else_block.as_ref().is_some_and(|other| Self::ends_in_return(other)) {
                    after_then
                } else { after_then.intersection(&self.assigned).cloned().collect() };
                self.line("}");
            }
            Stmt::While(loop_) => {
                let condition = self.expression(&loop_.condition)?;
                let before = self.assigned.clone();
                self.line(&format!("while {} {{", condition)); self.indent += 1; self.loop_depth += 1;
                self.statements(&loop_.body)?;
                self.loop_depth -= 1; self.indent -= 1; self.line("}");
                self.assigned = before;
            }
            Stmt::For(loop_) => {
                let Expr::Call { func, args, .. } = &loop_.iterable else { return Err("Script for requires range(stop) or range(start, stop)".into()); };
                if !matches!(func.as_ref(), Expr::Identifier { name, .. } if name == "range") || !(1..=2).contains(&args.len()) {
                    return Err("Script for requires range(stop) or range(start, stop)".into());
                }
                let start = if args.len() == 2 { self.expression(&args[0])? } else { "0".into() };
                let end = self.expression(args.last().unwrap())?;
                let ty = self.infer_type(args.last().unwrap())?;
                let before = self.assigned.clone();
                self.locals.entry(loop_.var.clone()).or_insert(ty);
                self.assigned.insert(loop_.var.clone());
                self.temporary += 1;
                let internal = format!("__aeth_script_iter_{}", self.temporary);
                self.line(&format!("for {} in {}..{} {{", internal, start, end));
                self.indent += 1; self.loop_depth += 1;
                self.line(&format!("{} = {};", loop_.var, internal));
                self.statements(&loop_.body)?;
                self.loop_depth -= 1; self.indent -= 1; self.line("}");
                self.assigned = before;
            }
            Stmt::Break | Stmt::Continue => {
                if self.loop_depth == 0 { return Err("Script break/continue requires an enclosing loop".into()); }
                self.line(if matches!(statement, Stmt::Break) { "break;" } else { "continue;" });
            }
            Stmt::Pass => {},
            Stmt::FunctionDef(_) => return Err("nested Script functions are not implemented".into()),
        }
        Ok(())
    }

    fn infer_type(&self, expr: &Expr) -> Result<String> {
        Ok(match expr {
            Expr::Integer { .. } => "i64".into(), Expr::Float { .. } => "f64".into(),
            Expr::Boolean { .. } => "bool".into(), Expr::String { .. } => "*u8".into(),
            Expr::Identifier { name, .. } => self.locals.get(name).cloned().ok_or_else(|| format!("unknown Script variable '{}'", name))?,
            Expr::Unary { op: UnaryOp::Not, .. } => "bool".into(),
            Expr::Unary { expr, .. } => self.infer_type(expr)?,
            Expr::Binary { op: BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::And | BinOp::Or, .. } => "bool".into(),
            Expr::Binary { left, .. } => self.infer_type(left)?,
            Expr::Call { func, .. } => {
                let Expr::Identifier { name, .. } = func.as_ref() else { return Err("Script calls require a named function".into()); };
                if name == "print" { "i32".into() }
                else { self.functions.get(name).cloned().ok_or_else(|| format!("unknown Script function '{}'", name))? }
            }
            Expr::List { .. } | Expr::FieldAccess { .. } => return Err("Script lists and object fields are not implemented".into()),
        })
    }

    fn expression(&self, expr: &Expr) -> Result<String> {
        Ok(match expr {
            Expr::Identifier { name, .. } => {
                if !self.assigned.contains(name) { return Err(format!("Script variable '{}' may be used before assignment", name)); }
                name.clone()
            }
            Expr::Integer { value, .. } => value.to_string(),
            Expr::Float { value, .. } => format!("{:?}", value),
            Expr::Boolean { value, .. } => value.to_string(),
            Expr::String { value, .. } => format!("\"{}\"", Self::escape_string(value)),
            Expr::Unary { op, expr, .. } => format!("({}{})", if matches!(op, UnaryOp::Not) { "!" } else { "-" }, self.expression(expr)?),
            Expr::Binary { left, op, right, .. } => {
                let operator = match op {
                    BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*", BinOp::Div => "/", BinOp::Mod => "%",
                    BinOp::Eq => "==", BinOp::Ne => "!=", BinOp::Lt => "<", BinOp::Le => "<=", BinOp::Gt => ">", BinOp::Ge => ">=",
                    BinOp::And => "&&", BinOp::Or => "||",
                };
                format!("({} {} {})", self.expression(left)?, operator, self.expression(right)?)
            }
            Expr::Call { func, args, .. } => {
                let Expr::Identifier { name, .. } = func.as_ref() else { return Err("Script calls require a named function".into()); };
                let values = args.iter().map(|arg| self.expression(arg)).collect::<Result<Vec<_>>>()?;
                if name == "print" {
                    if args.len() != 1 { return Err("Script print takes exactly one argument".into()); }
                    let ty = self.infer_type(&args[0])?;
                    match ty.as_str() {
                        "*u8" => format!("puts({})", values[0]),
                        "f32" | "f64" => format!("printf(\"%g\\n\", ({}) as f64)", values[0]),
                        "bool" => return Err("Script print currently accepts strings, integers, and floats".into()),
                        _ => format!("printf(\"%lld\\n\", ({}) as i64)", values[0]),
                    }
                } else {
                    if !self.functions.contains_key(name) { return Err(format!("unknown Script function '{}'", name)); }
                    format!("{}({})", name, values.join(", "))
                }
            }
            Expr::List { .. } | Expr::FieldAccess { .. } => return Err("Script lists and object fields are not implemented".into()),
        })
    }

    fn escape_string(value: &str) -> String {
        let mut escaped = String::new();
        for c in value.chars() {
            match c { '\\' => escaped.push_str("\\\\"), '"' => escaped.push_str("\\\""), '\n' => escaped.push_str("\\n"),
                '\r' => escaped.push_str("\\r"), '\t' => escaped.push_str("\\t"), '\0' => escaped.push_str("\\0"), _ => escaped.push(c) }
        }
        escaped
    }

    fn map_type(hint: &TypeHint) -> Result<String> {
        if !hint.generics.is_empty() { return Err("generic Script type hints are not implemented".into()); }
        Ok(match hint.name.as_str() { "int" => "i64", "float" => "f64", "str" => "*u8", "None" => "void", other => other }.into())
    }

    fn line(&mut self, text: &str) {
        self.source_lines.push(self.source_line);
        self.output.push_str(&"    ".repeat(self.indent));
        self.output.push_str(text); self.output.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::parser::Parser;

    fn lower(source: &str) -> Result<String> {
        let module = Parser::new(source).parse()?;
        Transpiler::new().transpile(&module)
    }

    #[test]
    fn script_mutation_arithmetic_and_range_reach_core_semantics() {
        let source = "def square(x: int) -> int:\n    return x * x\ndef main():\n    total = 0\n    for i in range(1, 5):\n        total = total + square(i)\n    print(total)\n    return total - 30\n";
        let core = lower(source).unwrap();
        let mut parser = crate::frontend::parser::Parser::new(crate::frontend::lexer::Lexer::new(&core, 0));
        let program = parser.parse_program().unwrap();
        crate::frontend::semantic::SemanticAnalyzer::new().analyze(&program).unwrap();
        assert_eq!(core.matches("let mut total:").count(), 1);
        assert!(core.contains("fn square("));
    }

    #[test]
    fn definite_assignment_rejects_zero_iteration_and_one_sided_branches() {
        assert!(lower("def main():\n    if True:\n        x = 1\n    return x\n").is_err());
        assert!(lower("def main():\n    while False:\n        x = 1\n    return x\n").is_err());
        assert!(lower("def main():\n    if True:\n        x = 1\n    else:\n        x = 2\n    return x\n").is_ok());
    }

    #[test]
    fn script_elif_chain_boolean_and_escaping_lower_correctly() {
        let source = "def main():\n    x = 2\n    if x < 1:\n        return 1\n    elif x == 1:\n        return 2\n    elif not x > 2 and True:\n        print(\"a\\\"b\\n\")\n        return 0\n    else:\n        return 3\n";
        let core = lower(source).unwrap();
        let mut parser = crate::frontend::parser::Parser::new(crate::frontend::lexer::Lexer::new(&core, 0));
        let program = parser.parse_program().unwrap();
        crate::frontend::semantic::SemanticAnalyzer::new().analyze(&program).unwrap();
    }
}
