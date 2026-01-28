//! Aether Script Transpiler
//!
//! Converts ScriptAST to Aether Core source code (.aeth)
//! Follows the rules defined in docs/AETHER_SCRIPT_SPEC.md

use super::ast::*;

pub struct Transpiler {
    indent_level: usize,
    output: String,
    source_file: Option<String>,
    emit_line_directives: bool,
}

impl Transpiler {
    pub fn new() -> Self {
        Self {
            indent_level: 0,
            output: String::new(),
            source_file: None,
            emit_line_directives: false,
        }
    }

    /// Enable source mapping with the given source file name
    pub fn with_source_file(mut self, path: &str) -> Self {
        self.source_file = Some(path.to_string());
        self.emit_line_directives = true;
        self
    }

    /// Transpile a ScriptModule to Aether Core source code
    pub fn transpile(&mut self, module: &ScriptModule) -> String {
        // Generate prelude comments
        self.emit_line("// Auto-generated from Aether Script (.ath)");
        self.emit_line("// DO NOT EDIT - Regenerate from source");
        if let Some(ref src) = self.source_file {
            self.output.push_str(&format!("// Source: {}\n", src));
        }
        self.emit_line("");
        
        // Emit extern declarations for common C functions
        self.emit_line("extern \"C\" {");
        self.emit_line("    fn puts(s: *u8) -> i32;");
        self.emit_line("    fn malloc(size: u64) -> *void;");
        self.emit_line("    fn free(ptr: *void);");
        self.emit_line("}");
        self.emit_line("");

        for stmt in &module.stmts {
            self.transpile_stmt(stmt);
        }

        std::mem::take(&mut self.output)
    }

    /// Emit a source mapping comment for debuggers
    /// Uses comment format since Core parser doesn't have preprocessor
    fn emit_source_line(&mut self, line: usize) {
        if self.emit_line_directives {
            if let Some(ref src) = self.source_file {
                // Use comment-based source mapping that won't break Core parser
                self.output.push_str(&format!("// @source {}:{}\n", src, line));
            }
        }
    }

    fn transpile_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::FunctionDef(f) => self.transpile_function(f),
            Stmt::If(i) => self.transpile_if(i),
            Stmt::While(w) => self.transpile_while(w),
            Stmt::For(f) => self.transpile_for(f),
            Stmt::Return(r) => self.transpile_return(r),
            Stmt::Assign(a) => self.transpile_assign(a),
            Stmt::Expr(e) => {
                self.emit_indent();
                self.emit(&self.transpile_expr(e));
                self.emit(";\n");
            }
            Stmt::Pass => {
                self.emit_indent();
                self.emit_line("// pass");
            }
        }
    }

    fn transpile_function(&mut self, f: &FunctionDef) {
        // Emit source line directive for debuggers
        self.emit_source_line(f.span.line);
        
        // Function signature
        self.emit_indent();

        // Comptime functions get @comptime attribute
        if f.is_comptime {
            self.emit_line("#[comptime]");
            self.emit_indent();
        }

        self.emit("fn ");
        self.emit(&f.name);
        self.emit("(");

        // NOTE: ctx injection removed for MVP - Core compiler doesn't have ScriptContext yet
        // TODO: Re-enable when stdlib/runtime is implemented
        // self.emit("ctx: &mut ScriptContext");

        // User parameters
        for (i, param) in f.params.iter().enumerate() {
            self.emit(&param.name);
            self.emit(": ");
            if let Some(ref hint) = param.type_hint {
                self.emit(&self.map_type(hint));
            } else {
                self.emit("_"); // Inferred type placeholder
            }
            if i < f.params.len() - 1 {
                self.emit(", ");
            }
        }

        self.emit(")");

        // Return type
        if let Some(ref ret) = f.return_type {
            self.emit(" -> ");
            self.emit(&self.map_type(ret));
        } else if f.name == "main" {
            // main function in C/systems needs to return int
            self.emit(" -> i32");
        }

        self.emit(" {\n");

        // Body
        self.indent_level += 1;
        for stmt in &f.body {
            self.transpile_stmt(stmt);
        }
        self.indent_level -= 1;

        self.emit_indent();
        self.emit_line("}");
        self.emit_line("");
    }

    fn transpile_if(&mut self, i: &IfStmt) {
        self.emit_indent();
        self.emit("if ");
        self.emit(&self.transpile_expr(&i.condition));
        self.emit(" {\n");

        self.indent_level += 1;
        for stmt in &i.then_block {
            self.transpile_stmt(stmt);
        }
        self.indent_level -= 1;

        self.emit_indent();
        self.emit("}");

        if let Some(ref else_block) = i.else_block {
            self.emit(" else {\n");
            self.indent_level += 1;
            for stmt in else_block {
                self.transpile_stmt(stmt);
            }
            self.indent_level -= 1;
            self.emit_indent();
            self.emit("}");
        }
        self.emit("\n");
    }

    fn transpile_while(&mut self, w: &WhileStmt) {
        self.emit_indent();
        self.emit("while ");
        self.emit(&self.transpile_expr(&w.condition));
        self.emit(" {\n");

        self.indent_level += 1;
        for stmt in &w.body {
            self.transpile_stmt(stmt);
        }
        self.indent_level -= 1;

        self.emit_indent();
        self.emit_line("}");
    }

    fn transpile_for(&mut self, f: &ForStmt) {
        self.emit_indent();
        // Transpile: for x in iterable -> for x in iterable
        self.emit("for ");
        self.emit(&f.var);
        // P5.1: Add type annotation for loop variable (infer from iterable if possible)
        self.emit(": _");  // Use placeholder, Core compiler will infer
        self.emit(" in ");
        self.emit(&self.transpile_expr(&f.iterable));
        self.emit(" {\n");

        self.indent_level += 1;
        for stmt in &f.body {
            self.transpile_stmt(stmt);
        }
        self.indent_level -= 1;

        self.emit_indent();
        self.emit_line("}");
    }

    fn transpile_return(&mut self, r: &ReturnStmt) {
        self.emit_indent();
        self.emit("return");
        if let Some(ref val) = r.value {
            self.emit(" ");
            self.emit(&self.transpile_expr(val));
        }
        self.emit(";\n");
    }

    fn transpile_assign(&mut self, a: &AssignStmt) {
        self.emit_indent();
        // P5.1: AetherLang requires explicit type annotations
        self.emit("let ");
        self.emit(&self.transpile_expr(&a.target));

        // Infer type from value expression
        let inferred_type = self.infer_type(&a.value);
        self.emit(": ");
        self.emit(&inferred_type);

        self.emit(" = ");
        self.emit(&self.transpile_expr(&a.value));
        self.emit(";\n");
    }

    /// Infer type from expression for P5.1 compliance
    fn infer_type(&self, expr: &Expr) -> String {
        match expr {
            Expr::Integer { .. } => "i64".to_string(),
            Expr::Float { .. } => "f64".to_string(),
            Expr::String { .. } => "*u8".to_string(),
            Expr::Identifier { .. } => "_".to_string(), // Cannot infer, use placeholder
            Expr::Binary { left, op, .. } => {
                // Binary ops preserve type of operands
                match op {
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt |
                    BinOp::Le | BinOp::Ge | BinOp::And | BinOp::Or => "bool".to_string(),
                    _ => self.infer_type(left),
                }
            }
            Expr::Call { func, .. } => {
                // Try to infer from known functions
                if let Expr::Identifier { name, .. } = func.as_ref() {
                    match name.as_str() {
                        "len" => "u64".to_string(),
                        "malloc" => "*void".to_string(),
                        _ => "_".to_string(),
                    }
                } else {
                    "_".to_string()
                }
            }
            Expr::List { elements, .. } => {
                if let Some(first) = elements.first() {
                    format!("*{}", self.infer_type(first))
                } else {
                    "*void".to_string()
                }
            }
            Expr::FieldAccess { .. } => "_".to_string(),
        }
    }

    fn transpile_expr(&self, expr: &Expr) -> String {
        match expr {
            Expr::Identifier { name, .. } => name.clone(),
            Expr::Integer { value, .. } => format!("{}", value),
            Expr::Float { value, .. } => format!("{}", value),
            Expr::String { value, .. } => {
                // Generate null-terminated C string literal
                format!("\"{}\\0\" as *u8", value)
            }
            Expr::Binary { left, op, right, .. } => {
                let l = self.transpile_expr(left);
                let r = self.transpile_expr(right);
                let op_str = match op {
                    BinOp::Add => "+",
                    BinOp::Sub => "-",
                    BinOp::Mul => "*",
                    BinOp::Div => "/",
                    BinOp::Eq => "==",
                    BinOp::Ne => "!=",
                    BinOp::Lt => "<",
                    BinOp::Gt => ">",
                    BinOp::Le => "<=",
                    BinOp::Ge => ">=",
                    BinOp::And => "&&",
                    BinOp::Or => "||",
                };
                format!("({} {} {})", l, op_str, r)
            }
            Expr::Call { func, args, .. } => {
                let func_name = self.transpile_expr(func);
                // Map Script builtins to Core/C equivalents
                let mapped_func = match func_name.as_str() {
                    "print" => "puts",
                    other => other,
                };
                let args_str: Vec<String> = args.iter().map(|a| self.transpile_expr(a)).collect();
                // NOTE: ctx injection removed for MVP
                format!("{}({})", mapped_func, args_str.join(", "))
            }
            Expr::FieldAccess { target, field, .. } => {
                format!("{}.{}", self.transpile_expr(target), field)
            }
            Expr::List { elements, .. } => {
                // List -> Vec::new_in(ctx.allocator) + pushes
                // For simplicity now, use vec! macro (TODO: proper alloc)
                let elems: Vec<String> = elements.iter().map(|e| self.transpile_expr(e)).collect();
                format!("vec![{}]", elems.join(", "))
            }
        }
    }

    /// Map Script type hints to Core types (compatible with .aeth)
    fn map_type(&self, hint: &TypeHint) -> String {
        let base = match hint.name.as_str() {
            "int" => "i64".to_string(),
            "float" => "f64".to_string(),
            "bool" => "bool".to_string(),
            "str" => "*u8".to_string(),  // Use raw C string pointer
            "None" => "void".to_string(),
            "List" => {
                // Use pointer array for lists
                if let Some(inner) = hint.generics.first() {
                    format!("*{}", self.map_type(inner))
                } else {
                    "*void".to_string()
                }
            }
            "i32" => "i32".to_string(),
            "i64" => "i64".to_string(),
            "f32" => "f32".to_string(),
            "f64" => "f64".to_string(),
            "u8" => "u8".to_string(),
            "u32" => "u32".to_string(),
            "u64" => "u64".to_string(),
            other => other.to_string(), // Pass through custom types
        };
        base
    }

    // --- Emit Helpers ---

    fn emit(&mut self, s: &str) {
        self.output.push_str(s);
    }

    fn emit_line(&mut self, s: &str) {
        self.emit_indent();
        self.output.push_str(s);
        self.output.push('\n');
    }

    fn emit_indent(&mut self) {
        for _ in 0..self.indent_level {
            self.output.push_str("    ");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::parser::Parser;

    #[test]
    fn test_transpile_simple_function() {
        let input = "
def greet(name: str) -> str:
    return name
";
        let mut parser = Parser::new(input);
        let module = parser.parse().expect("parse failed");
        
        let mut transpiler = Transpiler::new();
        let output = transpiler.transpile(&module);
        
        assert!(output.contains("fn greet"));
        assert!(!output.contains("ctx")); // MVP: no ctx injection
        assert!(output.contains("name: *u8"));  // str maps to *u8
        assert!(output.contains("-> *u8"));     // return type
        assert!(output.contains("extern \"C\"")); // has extern block
        assert!(output.contains("fn puts"));     // has puts declaration
    }
}
