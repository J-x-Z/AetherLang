//! Script has its own AST before Core lowering, so bound it at construction too.
use super::{Expr, Span};
use std::collections::HashMap;

const MAX_SYNTAX_NESTING: usize = 64;
const MAX_EXPRESSION_DEPTH: usize = 256;

#[derive(Default)]
pub(super) struct ParseLimits {
    nesting: usize,
    expressions: HashMap<(usize, usize), usize>,
}
impl ParseLimits {
    fn error(span: Span, message: String) -> String {
        format!("line {}:{}: {}", span.line, span.column, message)
    }
    pub(super) fn enter(&mut self, span: Span) -> Result<(), String> {
        if self.nesting >= MAX_SYNTAX_NESTING {
            return Err(Self::error(
                span,
                format!("syntax nesting exceeds {MAX_SYNTAX_NESTING} parser levels"),
            ));
        }
        self.nesting += 1;
        Ok(())
    }
    pub(super) fn leave(&mut self) {
        self.nesting -= 1;
    }
    fn depth(&self, expr: &Expr) -> usize {
        let span = expr.span();
        self.expressions
            .get(&(span.start, span.end))
            .copied()
            .unwrap_or(1)
    }
    pub(super) fn expression(&mut self, expr: &Expr) -> Result<(), String> {
        let children = match expr {
            Expr::Lambda {body,..} => self.depth(body),
            Expr::Unary { expr, .. } => self.depth(expr),
            Expr::Binary { left, right, .. } => self.depth(left).max(self.depth(right)),
            Expr::Call { func, args, .. } => args
                .iter()
                .map(|arg| self.depth(arg))
                .fold(self.depth(func), usize::max),
            Expr::FieldAccess { target, .. } => self.depth(target),
            Expr::Index {target,index,..} => self.depth(target).max(self.depth(index)),
            Expr::List { elements, .. } => elements
                .iter()
                .map(|element| self.depth(element))
                .max()
                .unwrap_or(0),
            _ => 0,
        };
        let depth = 1 + children;
        let span = expr.span();
        if depth > MAX_EXPRESSION_DEPTH {
            return Err(Self::error(
                span,
                format!("expression tree depth exceeds {MAX_EXPRESSION_DEPTH}"),
            ));
        }
        self.expressions.insert((span.start, span.end), depth);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::script::parser::Parser;
    #[test]
    fn recursive_syntax_and_constructed_trees_have_limits() {
        let nested = (0..512)
            .map(|level| format!("{}if True:\n", "    ".repeat(level)))
            .collect::<String>()
            + &format!("{}pass\n", "    ".repeat(512));
        for source in [
            format!("value = {}\n", vec!["1"; 4096].join("+")),
            format!("value = root{}\n", ".next".repeat(4096)),
            format!("value = {}1\n", "-".repeat(1024)),
            format!("value = {}1{}\n", "(".repeat(1024), ")".repeat(1024)),
            format!(
                "def f(x: {}i32{}):\n    pass\n",
                "List[".repeat(1024),
                "]".repeat(1024)
            ),
            format!(
                "if False:\n    pass\n{}",
                "elif False:\n    pass\n".repeat(1024)
            ),
            nested,
        ] {
            let error = Parser::new(&source).parse().unwrap_err();
            assert!(
                error.starts_with("line ")
                    && (error.contains("depth exceeds") || error.contains("nesting exceeds")),
                "{error}"
            );
        }
    }
    #[test]
    fn ordinary_expression_depth_and_many_statements_remain_valid() {
        assert!(Parser::new(&format!(
            "value = {}({}){}\n{}",
            "(".repeat(16),
            vec!["1"; 128].join("+"),
            ")".repeat(16),
            "value = value + 1\n".repeat(2048)
        ))
        .parse()
        .is_ok());
    }
}
