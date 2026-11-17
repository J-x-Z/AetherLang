//! Bound recursive syntax and the actual expression tree before later compiler
//! passes can recurse into it. Width is unrestricted: a large flat array or block
//! does not consume a depth budget. Cached child depths keep this check linear.
use super::{Block, Expr, Stmt};
use crate::utils::{Error, Result, Span};
use std::collections::HashMap;

pub(super) const MAX_SYNTAX_NESTING: usize = 64;
pub(super) const MAX_EXPRESSION_DEPTH: usize = 256;

#[derive(Default)]
pub(super) struct ParseLimits {
    nesting: usize,
    expressions: HashMap<Span, usize>,
}

impl ParseLimits {
    pub(super) fn enter(&mut self, span: Span) -> Result<()> {
        if self.nesting >= MAX_SYNTAX_NESTING {
            return Err(Error::Semantic {
                code: "E1002",
                message: format!("syntax nesting exceeds {MAX_SYNTAX_NESTING} parser levels"),
                span,
            });
        }
        self.nesting += 1;
        Ok(())
    }
    pub(super) fn leave(&mut self) {
        self.nesting -= 1;
    }

    fn depth(&self, expression: &Expr) -> usize {
        // Every completed child has already passed through a parser wrapper.
        // Leaves may also be synthesized directly (e.g. a literal match arm).
        self.expressions
            .get(&expression.span())
            .copied()
            .unwrap_or(1)
    }
    fn block_depth(&self, block: &Block) -> usize {
        1 + block
            .stmts
            .iter()
            .map(|statement| match statement {
                Stmt::Expr(expr) | Stmt::Semi(expr) => self.depth(expr),
                Stmt::Let { value, .. } | Stmt::Return { value, .. } => {
                    value.as_ref().map(|expr| self.depth(expr)).unwrap_or(0)
                }
                _ => 0,
            })
            .max()
            .unwrap_or(0)
    }
    fn check_depth(depth: usize, span: Span) -> Result<()> {
        if depth > MAX_EXPRESSION_DEPTH {
            return Err(Error::Semantic {
                code: "E1002",
                message: format!("expression tree depth exceeds {MAX_EXPRESSION_DEPTH}"),
                span,
            });
        }
        Ok(())
    }
    pub(super) fn block(&self, block: &Block) -> Result<()> {
        Self::check_depth(self.block_depth(block), block.span)
    }
    pub(super) fn expression(&mut self, expression: &Expr) -> Result<()> {
        let children = match expression {
            Expr::Literal(_) | Expr::Ident(_) | Expr::Path { .. } | Expr::Layout { .. } => 0,
            Expr::Binary { left, right, .. } => self.depth(left).max(self.depth(right)),
            Expr::Unary { expr, .. }
            | Expr::Ref { expr, .. }
            | Expr::Deref { expr, .. }
            | Expr::Cast { expr, .. }
            | Expr::Field { expr, .. }
            | Expr::Try { expr, .. } => self.depth(expr),
            Expr::Call { func, args, .. } => args
                .iter()
                .map(|arg| self.depth(arg))
                .fold(self.depth(func), usize::max),
            Expr::MethodCall { expr, args, .. } => args
                .iter()
                .map(|arg| self.depth(arg))
                .fold(self.depth(expr), usize::max),
            Expr::Index { expr, index, .. } => self.depth(expr).max(self.depth(index)),
            Expr::Block(block)
            | Expr::Unsafe { body: block, .. }
            | Expr::Loop { body: block, .. } => self.block_depth(block),
            Expr::If {
                cond,
                then_block,
                else_block,
                ..
            } => self.depth(cond).max(self.block_depth(then_block)).max(
                else_block
                    .as_ref()
                    .map(|block| self.block_depth(block))
                    .unwrap_or(0),
            ),
            Expr::While { cond, body, .. } => self.depth(cond).max(self.block_depth(body)),
            Expr::For { iter, body, .. } => self.depth(iter).max(self.block_depth(body)),
            Expr::Match { expr, arms, .. } => arms
                .iter()
                .map(|arm| {
                    self.depth(&arm.body)
                        .max(arm.guard.as_ref().map(|expr| self.depth(expr)).unwrap_or(0))
                })
                .fold(self.depth(expr), usize::max),
            Expr::StructLit { fields, .. } => fields
                .iter()
                .map(|(_, expr)| self.depth(expr))
                .max()
                .unwrap_or(0),
            Expr::ArrayRepeat {value,count,..} => self.depth(value).max(self.depth(count)),
            Expr::Array { elements, .. } | Expr::Tuple { elements, .. } => elements
                .iter()
                .map(|expr| self.depth(expr))
                .max()
                .unwrap_or(0),
            Expr::Range { start, end, .. } => start
                .as_ref()
                .map(|expr| self.depth(expr))
                .unwrap_or(0)
                .max(end.as_ref().map(|expr| self.depth(expr)).unwrap_or(0)),
            Expr::Asm { operands, .. } => operands
                .iter()
                .filter_map(|operand| operand.expr.as_ref())
                .map(|expr| self.depth(expr))
                .max()
                .unwrap_or(0),
            Expr::Closure { body, .. } => self.depth(body),
        };
        let depth = 1 + children;
        Self::check_depth(depth, expression.span())?;
        self.expressions.insert(expression.span(), depth);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::frontend::{lexer::Lexer, parser::Parser};
    use crate::utils::Error;

    fn parse(source: &str) -> crate::utils::Result<crate::frontend::ast::Program> {
        Parser::new(Lexer::new(source, 0)).parse_program()
    }
    fn assert_limit(source: &str) {
        match parse(source) {
            Err(Error::Semantic {
                code: "E1002",
                span,
                ..
            }) => {
                assert_eq!(span.file_id, 0);
                assert!(span.start < source.len());
                assert!(span.end <= source.len());
            }
            other => panic!("expected a bounded source diagnostic, got {other:?}"),
        }
    }
    #[test]
    fn deep_left_and_postfix_trees_stop_while_being_built() {
        assert_limit(&format!(
            "fn main()->i32{{return {};}}",
            vec!["1"; 4096].join("+")
        ));
        assert_limit(&format!(
            "fn main()->i32{{return value{};}}",
            ".next".repeat(4096)
        ));
    }
    macro_rules! syntax_limit_test {
        ($name:ident, $source:expr) => {
            #[test]
            fn $name() {
                assert_limit(&$source);
            }
        };
    }
    syntax_limit_test!(
        prefix_nesting_is_bounded,
        format!("fn f(){{{}1;}}", "-".repeat(1024))
    );
    syntax_limit_test!(
        parentheses_are_bounded,
        format!("fn f(){{{}1{};}}", "(".repeat(1024), ")".repeat(1024))
    );
    syntax_limit_test!(
        assignment_nesting_is_bounded,
        format!("fn f(){{{}1;}}", "x=".repeat(1024))
    );
    syntax_limit_test!(
        type_nesting_is_bounded,
        format!("fn f(x:{}i32){{}}", "*".repeat(1024))
    );
    syntax_limit_test!(
        block_nesting_is_bounded,
        format!("fn f(){{{}{}}}", "{".repeat(1024), "}".repeat(1024))
    );
    syntax_limit_test!(
        else_if_nesting_is_bounded,
        format!("fn f(){{{}if true{{}}}}", "if false{}else ".repeat(1024))
    );
    syntax_limit_test!(
        module_nesting_is_bounded,
        format!(
            "{}fn f(){{}}{}",
            "mod nested{".repeat(1024),
            "}".repeat(1024)
        )
    );
    syntax_limit_test!(
        pattern_nesting_is_bounded,
        format!(
            "fn f(){{match x{{{}true{}=>0,_=>1}}}}",
            "Option::Some(".repeat(1024),
            ")".repeat(1024)
        )
    );
    #[test]
    fn normal_depth_and_large_flat_lists_are_not_total_node_limits() {
        assert!(parse(&format!(
            "fn f(){{{}1{};{};}}",
            "(".repeat(16),
            ")".repeat(16),
            vec!["1"; 128].join("+")
        ))
        .is_ok());
        assert!(parse(&format!(
            "fn f(){{[{}];{}}}",
            vec!["1"; 4096].join(","),
            "f();".repeat(4096)
        ))
        .is_ok());
    }
}
