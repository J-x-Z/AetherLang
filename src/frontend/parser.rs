//! Parser for AetherLang
//!
//! Recursive descent parser with Pratt parsing for expressions.

use crate::frontend::token::{Token, TokenKind};
use crate::frontend::ast::*;
use crate::frontend::lexer::Lexer;
use crate::utils::{Span, Error, Result};

/// The parser
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    /// Create a new parser from a lexer
    pub fn new(mut lexer: Lexer) -> Self {
        Self {
            tokens: lexer.tokenize(),
            pos: 0,
        }
    }

    /// Create a parser from pre-tokenized input
    pub fn from_tokens(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    // ==================== Helper Methods ====================

    fn current(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or_else(|| {
            self.tokens.last().expect("tokens should not be empty")
        })
    }

    fn current_kind(&self) -> &TokenKind {
        &self.current().kind
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos + 1)
    }

    fn advance(&mut self) -> Token {
        let token = self.current().clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        token
    }

    fn check(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(self.current_kind()) == std::mem::discriminant(kind)
    }

    fn is_at_end(&self) -> bool {
        matches!(self.current_kind(), TokenKind::Eof)
    }

    fn expect(&mut self, expected: TokenKind) -> Result<Token> {
        if self.check(&expected) {
            Ok(self.advance())
        } else {
            Err(Error::UnexpectedToken {
                expected: format!("{:?}", expected),
                got: format!("{:?}", self.current_kind()),
                span: self.current().span,
            })
        }
    }

    fn consume(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    // ==================== Parsing Methods ====================

    /// Parse a complete program
    pub fn parse_program(&mut self) -> Result<Program> {
        let mut items = Vec::new();

        while !self.is_at_end() {
            items.push(self.parse_item()?);
        }

        Ok(Program { items })
    }

    /// Parse a top-level item
    fn parse_item(&mut self) -> Result<Item> {
        match self.current_kind() {
            TokenKind::Fn => Ok(Item::Function(self.parse_function()?)),
            TokenKind::Struct => Ok(Item::Struct(self.parse_struct()?)),
            TokenKind::Enum => Ok(Item::Enum(self.parse_enum()?)),
            TokenKind::Impl => Ok(Item::Impl(self.parse_impl()?)),
            TokenKind::Interface => Ok(Item::Interface(self.parse_interface()?)),
            TokenKind::Const => Ok(Item::Const(self.parse_const()?)),
            _ => Err(Error::UnexpectedToken {
                expected: "item (fn, struct, enum, impl, interface, const)".to_string(),
                got: format!("{:?}", self.current_kind()),
                span: self.current().span,
            }),
        }
    }

    /// Parse a function definition
    fn parse_function(&mut self) -> Result<Function> {
        let start = self.current().span;
        self.expect(TokenKind::Fn)?;

        let name = self.parse_ident()?;

        self.expect(TokenKind::LParen)?;
        let params = self.parse_params()?;
        self.expect(TokenKind::RParen)?;

        let ret_type = if self.consume(&TokenKind::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };

        let body = self.parse_block()?;

        Ok(Function {
            name,
            params,
            ret_type,
            body,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
        })
    }

    fn parse_params(&mut self) -> Result<Vec<Param>> {
        let mut params = Vec::new();

        while !self.check(&TokenKind::RParen) && !self.is_at_end() {
            params.push(self.parse_param()?);
            if !self.consume(&TokenKind::Comma) {
                break;
            }
        }

        Ok(params)
    }

    fn parse_param(&mut self) -> Result<Param> {
        let start = self.current().span;
        let name = self.parse_ident()?;
        self.expect(TokenKind::Colon)?;

        let ownership = if self.consume(&TokenKind::Own) {
            Ownership::Own
        } else if self.consume(&TokenKind::Ref) {
            Ownership::Ref
        } else if self.consume(&TokenKind::Mut) {
            Ownership::Mut
        } else {
            Ownership::Own
        };

        let ty = self.parse_type()?;

        Ok(Param {
            name,
            ownership,
            ty,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
        })
    }

    fn parse_ident(&mut self) -> Result<Ident> {
        let token = self.current().clone();
        match &token.kind {
            TokenKind::Ident(name) => {
                self.advance();
                Ok(Ident {
                    name: name.clone(),
                    span: token.span,
                })
            }
            _ => Err(Error::ExpectedIdent { span: token.span }),
        }
    }

    fn parse_type(&mut self) -> Result<Type> {
        let start = self.current().span;

        // Pointer type
        if self.consume(&TokenKind::Star) {
            let inner = self.parse_type()?;
            return Ok(Type::Pointer(
                Box::new(inner),
                start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
            ));
        }

        // Reference type
        if self.consume(&TokenKind::And) {
            let mutable = self.consume(&TokenKind::Mut);
            let inner = self.parse_type()?;
            return Ok(Type::Ref {
                mutable,
                inner: Box::new(inner),
                span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
            });
        }

        // Array or slice
        if self.consume(&TokenKind::LBracket) {
            let elem = self.parse_type()?;
            if self.consume(&TokenKind::Semicolon) {
                let size = match self.current_kind() {
                    TokenKind::IntLit(n) => {
                        let n = *n as usize;
                        self.advance();
                        n
                    }
                    _ => return Err(Error::ExpectedArraySize { span: self.current().span }),
                };
                self.expect(TokenKind::RBracket)?;
                return Ok(Type::Array {
                    elem: Box::new(elem),
                    size,
                    span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
                });
            } else {
                self.expect(TokenKind::RBracket)?;
                return Ok(Type::Slice(
                    Box::new(elem),
                    start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
                ));
            }
        }

        // Unit or tuple
        if self.consume(&TokenKind::LParen) {
            if self.consume(&TokenKind::RParen) {
                return Ok(Type::Unit(start.merge(&self.tokens[self.pos.saturating_sub(1)].span)));
            }

            let first = self.parse_type()?;
            if self.consume(&TokenKind::Comma) {
                let mut types = vec![first];
                loop {
                    if self.check(&TokenKind::RParen) {
                        break;
                    }
                    types.push(self.parse_type()?);
                    if !self.consume(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::RParen)?;
                return Ok(Type::Tuple(
                    types,
                    start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
                ));
            }
            self.expect(TokenKind::RParen)?;
            return Ok(first);
        }

        // Named type
        if let TokenKind::Ident(name) = self.current_kind().clone() {
            self.advance();
            return Ok(Type::Named(name, start));
        }

        Err(Error::ExpectedType { span: self.current().span })
    }

    fn parse_block(&mut self) -> Result<Block> {
        let start = self.current().span;
        self.expect(TokenKind::LBrace)?;

        let mut stmts = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            stmts.push(self.parse_stmt()?);
        }

        self.expect(TokenKind::RBrace)?;

        Ok(Block {
            stmts,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
        })
    }

    fn parse_stmt(&mut self) -> Result<Stmt> {
        match self.current_kind() {
            TokenKind::Let => self.parse_let_stmt(),
            TokenKind::Return => self.parse_return_stmt(),
            TokenKind::Break => {
                let span = self.current().span;
                self.advance();
                Ok(Stmt::Break { span })
            }
            TokenKind::Continue => {
                let span = self.current().span;
                self.advance();
                Ok(Stmt::Continue { span })
            }
            TokenKind::Semicolon => {
                let span = self.current().span;
                self.advance();
                Ok(Stmt::Empty { span })
            }
            _ => {
                let expr = self.parse_expr()?;
                self.consume(&TokenKind::Semicolon);
                Ok(Stmt::Expr(expr))
            }
        }
    }

    fn parse_let_stmt(&mut self) -> Result<Stmt> {
        let start = self.current().span;
        self.expect(TokenKind::Let)?;

        let mutable = self.consume(&TokenKind::Mut);
        let name = self.parse_ident()?;

        let ty = if self.consume(&TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };

        let value = if self.consume(&TokenKind::Eq) {
            Some(self.parse_expr()?)
        } else {
            None
        };

        Ok(Stmt::Let {
            name,
            mutable,
            ty,
            value,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
        })
    }

    fn parse_return_stmt(&mut self) -> Result<Stmt> {
        let start = self.current().span;
        self.expect(TokenKind::Return)?;

        let value = if !self.check(&TokenKind::Semicolon)
            && !self.check(&TokenKind::RBrace)
            && !self.is_at_end()
        {
            Some(self.parse_expr()?)
        } else {
            None
        };

        Ok(Stmt::Return {
            value,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
        })
    }

    // ==================== Expression Parsing (Pratt) ====================

    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_expr_bp(0)
    }

    /// Parse expression with binding power (Pratt parsing)
    fn parse_expr_bp(&mut self, min_bp: u8) -> Result<Expr> {
        let mut left = self.parse_primary()?;

        loop {
            let op_token = self.current().clone();
            let Some(bp) = op_token.kind.binary_precedence() else {
                break;
            };

            if bp < min_bp {
                break;
            }

            self.advance();
            let op = Self::token_to_binop(&op_token.kind)?;

            // Right-associative for assignment
            let next_bp = if matches!(
                op,
                BinOp::Assign | BinOp::AddAssign | BinOp::SubAssign | BinOp::MulAssign | BinOp::DivAssign
            ) {
                bp
            } else {
                bp + 1
            };

            let right = self.parse_expr_bp(next_bp)?;
            let span = left.span().merge(&right.span());

            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
                span,
            };
        }

        Ok(left)
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        let token = self.current().clone();

        let expr = match &token.kind {
            // Literals
            TokenKind::IntLit(n) => {
                self.advance();
                Expr::Literal(Literal::Int(*n, token.span))
            }
            TokenKind::FloatLit(n) => {
                self.advance();
                Expr::Literal(Literal::Float(*n, token.span))
            }
            TokenKind::StringLit(s) => {
                self.advance();
                Expr::Literal(Literal::String(s.clone(), token.span))
            }
            TokenKind::CharLit(c) => {
                self.advance();
                Expr::Literal(Literal::Char(*c, token.span))
            }
            TokenKind::True => {
                self.advance();
                Expr::Literal(Literal::Bool(true, token.span))
            }
            TokenKind::False => {
                self.advance();
                Expr::Literal(Literal::Bool(false, token.span))
            }

            // Identifier or struct literal
            TokenKind::Ident(_) => {
                let ident = self.parse_ident()?;
                
                // Check if this is a struct literal: TypeName { field: value, ... }
                if self.check(&TokenKind::LBrace) {
                    let start_span = ident.span;
                    self.advance(); // consume '{'
                    
                    let mut fields = Vec::new();
                    while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
                        // Parse field name
                        let field_name = self.parse_ident()?;
                        self.expect(TokenKind::Colon)?;
                        let field_value = self.parse_expr()?;
                        fields.push((field_name, field_value));
                        
                        if !self.consume(&TokenKind::Comma) {
                            break;
                        }
                    }
                    
                    let end_token = self.expect(TokenKind::RBrace)?;
                    return Ok(Expr::StructLit {
                        name: ident,
                        fields,
                        span: start_span.merge(&end_token.span),
                    });
                }
                
                Expr::Ident(ident)
            }


            // Parenthesized or tuple
            TokenKind::LParen => {
                self.advance();
                if self.check(&TokenKind::RParen) {
                    self.advance();
                    return Ok(Expr::Tuple {
                        elements: Vec::new(),
                        span: token.span.merge(&self.tokens[self.pos.saturating_sub(1)].span),
                    });
                }

                let first = self.parse_expr()?;
                if self.consume(&TokenKind::Comma) {
                    let mut elements = vec![first];
                    while !self.check(&TokenKind::RParen) && !self.is_at_end() {
                        elements.push(self.parse_expr()?);
                        if !self.consume(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(TokenKind::RParen)?;
                    return Ok(Expr::Tuple {
                        elements,
                        span: token.span.merge(&self.tokens[self.pos.saturating_sub(1)].span),
                    });
                }

                self.expect(TokenKind::RParen)?;
                first
            }

            // Block
            TokenKind::LBrace => Expr::Block(self.parse_block()?),

            // If
            TokenKind::If => self.parse_if_expr()?,

            // Match  
            TokenKind::Match => self.parse_match_expr()?,

            // Loop
            TokenKind::Loop => {
                self.advance();
                let body = self.parse_block()?;
                Expr::Loop {
                    span: token.span.merge(&body.span),
                    body,
                }
            }

            // While
            TokenKind::While => {
                self.advance();
                let cond = self.parse_expr()?;
                let body = self.parse_block()?;
                Expr::While {
                    cond: Box::new(cond),
                    span: token.span.merge(&body.span),
                    body,
                }
            }

            // For
            TokenKind::For => {
                self.advance();
                let var = self.parse_ident()?;
                self.expect(TokenKind::In)?;
                let iter = self.parse_expr()?;
                let body = self.parse_block()?;
                Expr::For {
                    var,
                    iter: Box::new(iter),
                    span: token.span.merge(&body.span),
                    body,
                }
            }

            // Unary operators
            TokenKind::Minus => {
                self.advance();
                let expr = self.parse_primary()?;
                Expr::Unary {
                    op: UnOp::Neg,
                    span: token.span.merge(&expr.span()),
                    expr: Box::new(expr),
                }
            }
            TokenKind::Not => {
                self.advance();
                let expr = self.parse_primary()?;
                Expr::Unary {
                    op: UnOp::Not,
                    span: token.span.merge(&expr.span()),
                    expr: Box::new(expr),
                }
            }
            TokenKind::Star => {
                self.advance();
                let expr = self.parse_primary()?;
                Expr::Deref {
                    span: token.span.merge(&expr.span()),
                    expr: Box::new(expr),
                }
            }
            TokenKind::And => {
                self.advance();
                let mutable = self.consume(&TokenKind::Mut);
                let expr = self.parse_primary()?;
                Expr::Ref {
                    mutable,
                    span: token.span.merge(&expr.span()),
                    expr: Box::new(expr),
                }
            }

            // Unsafe
            TokenKind::Unsafe => {
                self.advance();
                let body = self.parse_block()?;
                Expr::Unsafe {
                    span: token.span.merge(&body.span),
                    body,
                }
            }

            // Array literal
            TokenKind::LBracket => {
                self.advance();
                let mut elements = Vec::new();
                while !self.check(&TokenKind::RBracket) && !self.is_at_end() {
                    elements.push(self.parse_expr()?);
                    if !self.consume(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::RBracket)?;
                Expr::Array {
                    elements,
                    span: token.span.merge(&self.tokens[self.pos.saturating_sub(1)].span),
                }
            }

            _ => return Err(Error::ExpectedExpr { span: token.span }),
        };

        self.parse_postfix(expr)
    }

    fn parse_postfix(&mut self, mut expr: Expr) -> Result<Expr> {
        loop {
            if self.consume(&TokenKind::LParen) {
                // Function call
                let mut args = Vec::new();
                while !self.check(&TokenKind::RParen) && !self.is_at_end() {
                    args.push(self.parse_expr()?);
                    if !self.consume(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::RParen)?;
                expr = Expr::Call {
                    span: expr.span().merge(&self.tokens[self.pos.saturating_sub(1)].span),
                    func: Box::new(expr),
                    args,
                };
            } else if self.consume(&TokenKind::Dot) {
                let field = self.parse_ident()?;
                if self.consume(&TokenKind::LParen) {
                    // Method call
                    let mut args = Vec::new();
                    while !self.check(&TokenKind::RParen) && !self.is_at_end() {
                        args.push(self.parse_expr()?);
                        if !self.consume(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(TokenKind::RParen)?;
                    expr = Expr::MethodCall {
                        span: expr.span().merge(&self.tokens[self.pos.saturating_sub(1)].span),
                        expr: Box::new(expr),
                        method: field,
                        args,
                    };
                } else {
                    // Field access
                    expr = Expr::Field {
                        span: expr.span().merge(&field.span),
                        expr: Box::new(expr),
                        field,
                    };
                }
            } else if self.consume(&TokenKind::LBracket) {
                // Index
                let index = self.parse_expr()?;
                self.expect(TokenKind::RBracket)?;
                expr = Expr::Index {
                    span: expr.span().merge(&self.tokens[self.pos.saturating_sub(1)].span),
                    expr: Box::new(expr),
                    index: Box::new(index),
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_if_expr(&mut self) -> Result<Expr> {
        let start = self.current().span;
        self.expect(TokenKind::If)?;

        let cond = self.parse_expr()?;
        let then_block = self.parse_block()?;

        let else_block = if self.consume(&TokenKind::Else) {
            Some(self.parse_block()?)
        } else {
            None
        };

        let end = else_block.as_ref().map(|b| b.span).unwrap_or(then_block.span);

        Ok(Expr::If {
            cond: Box::new(cond),
            then_block,
            else_block,
            span: start.merge(&end),
        })
    }

    fn parse_match_expr(&mut self) -> Result<Expr> {
        let start = self.current().span;
        self.expect(TokenKind::Match)?;

        let expr = self.parse_expr()?;
        self.expect(TokenKind::LBrace)?;

        let mut arms = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            arms.push(self.parse_match_arm()?);
        }

        self.expect(TokenKind::RBrace)?;

        Ok(Expr::Match {
            expr: Box::new(expr),
            arms,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
        })
    }

    fn parse_match_arm(&mut self) -> Result<MatchArm> {
        let start = self.current().span;
        let pattern = self.parse_pattern()?;

        self.expect(TokenKind::FatArrow)?;
        let body = self.parse_expr()?;

        self.consume(&TokenKind::Comma);

        Ok(MatchArm {
            pattern,
            guard: None,
            body,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
        })
    }

    fn parse_pattern(&mut self) -> Result<Pattern> {
        let token = self.current().clone();

        match &token.kind {
            TokenKind::Ident(name) if name == "_" => {
                self.advance();
                Ok(Pattern::Wildcard { span: token.span })
            }
            TokenKind::Ident(name) => {
                self.advance();
                Ok(Pattern::Binding {
                    name: Ident { name: name.clone(), span: token.span },
                    mutable: false,
                    span: token.span,
                })
            }
            TokenKind::IntLit(n) => {
                self.advance();
                Ok(Pattern::Literal(Literal::Int(*n, token.span)))
            }
            TokenKind::StringLit(s) => {
                self.advance();
                Ok(Pattern::Literal(Literal::String(s.clone(), token.span)))
            }
            TokenKind::True => {
                self.advance();
                Ok(Pattern::Literal(Literal::Bool(true, token.span)))
            }
            TokenKind::False => {
                self.advance();
                Ok(Pattern::Literal(Literal::Bool(false, token.span)))
            }
            _ => Err(Error::ExpectedPattern { span: token.span }),
        }
    }

    // ==================== Struct, Enum, Impl, Interface, Const ====================

    fn parse_struct(&mut self) -> Result<StructDef> {
        let start = self.current().span;
        self.expect(TokenKind::Struct)?;

        let name = self.parse_ident()?;
        self.expect(TokenKind::LBrace)?;

        let mut fields = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let field_name = self.parse_ident()?;
            self.expect(TokenKind::Colon)?;
            let ty = self.parse_type()?;

            fields.push(Field {
                span: field_name.span.merge(&ty.span()),
                name: field_name,
                ty,
            });

            self.consume(&TokenKind::Comma);
        }

        self.expect(TokenKind::RBrace)?;

        Ok(StructDef {
            name,
            fields,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
        })
    }

    fn parse_enum(&mut self) -> Result<EnumDef> {
        let start = self.current().span;
        self.expect(TokenKind::Enum)?;

        let name = self.parse_ident()?;
        self.expect(TokenKind::LBrace)?;

        let mut variants = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let var_name = self.parse_ident()?;

            let mut fields = Vec::new();
            if self.consume(&TokenKind::LParen) {
                while !self.check(&TokenKind::RParen) && !self.is_at_end() {
                    fields.push(self.parse_type()?);
                    if !self.consume(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::RParen)?;
            }

            variants.push(Variant {
                span: var_name.span,
                name: var_name,
                fields,
            });

            self.consume(&TokenKind::Comma);
        }

        self.expect(TokenKind::RBrace)?;

        Ok(EnumDef {
            name,
            variants,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
        })
    }

    fn parse_impl(&mut self) -> Result<ImplBlock> {
        let start = self.current().span;
        self.expect(TokenKind::Impl)?;

        let first = self.parse_ident()?;

        let (interface, target) = if self.consume(&TokenKind::For) {
            (Some(first), self.parse_ident()?)
        } else {
            (None, first)
        };

        self.expect(TokenKind::LBrace)?;

        let mut methods = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            methods.push(self.parse_function()?);
        }

        self.expect(TokenKind::RBrace)?;

        Ok(ImplBlock {
            target,
            interface,
            methods,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
        })
    }

    fn parse_interface(&mut self) -> Result<InterfaceDef> {
        let start = self.current().span;
        self.expect(TokenKind::Interface)?;

        let name = self.parse_ident()?;
        self.expect(TokenKind::LBrace)?;

        let mut methods = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            methods.push(self.parse_fn_sig()?);
        }

        self.expect(TokenKind::RBrace)?;

        Ok(InterfaceDef {
            name,
            methods,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
        })
    }

    fn parse_fn_sig(&mut self) -> Result<FunctionSig> {
        let start = self.current().span;
        self.expect(TokenKind::Fn)?;

        let name = self.parse_ident()?;
        self.expect(TokenKind::LParen)?;
        let params = self.parse_params()?;
        self.expect(TokenKind::RParen)?;

        let ret_type = if self.consume(&TokenKind::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };

        Ok(FunctionSig {
            name,
            params,
            ret_type,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
        })
    }

    fn parse_const(&mut self) -> Result<ConstDef> {
        let start = self.current().span;
        self.expect(TokenKind::Const)?;

        let name = self.parse_ident()?;

        let ty = if self.consume(&TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };

        self.expect(TokenKind::Eq)?;
        let value = self.parse_expr()?;

        Ok(ConstDef {
            name,
            ty,
            value,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
        })
    }

    fn token_to_binop(kind: &TokenKind) -> Result<BinOp> {
        match kind {
            TokenKind::Plus => Ok(BinOp::Add),
            TokenKind::Minus => Ok(BinOp::Sub),
            TokenKind::Star => Ok(BinOp::Mul),
            TokenKind::Slash => Ok(BinOp::Div),
            TokenKind::Percent => Ok(BinOp::Mod),
            TokenKind::EqEq => Ok(BinOp::Eq),
            TokenKind::Ne => Ok(BinOp::Ne),
            TokenKind::Lt => Ok(BinOp::Lt),
            TokenKind::Le => Ok(BinOp::Le),
            TokenKind::Gt => Ok(BinOp::Gt),
            TokenKind::Ge => Ok(BinOp::Ge),
            TokenKind::AndAnd => Ok(BinOp::And),
            TokenKind::OrOr => Ok(BinOp::Or),
            TokenKind::And => Ok(BinOp::BitAnd),
            TokenKind::Or => Ok(BinOp::BitOr),
            TokenKind::Caret => Ok(BinOp::BitXor),
            TokenKind::Shl => Ok(BinOp::Shl),
            TokenKind::Shr => Ok(BinOp::Shr),
            TokenKind::Eq => Ok(BinOp::Assign),
            TokenKind::PlusEq => Ok(BinOp::AddAssign),
            TokenKind::MinusEq => Ok(BinOp::SubAssign),
            TokenKind::StarEq => Ok(BinOp::MulAssign),
            TokenKind::SlashEq => Ok(BinOp::DivAssign),
            _ => Err(Error::InvalidOperator { span: Span::dummy() }),
        }
    }
}

// Helper for Expr span
impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Literal(lit) => lit.span(),
            Expr::Ident(ident) => ident.span,
            Expr::Binary { span, .. } => *span,
            Expr::Unary { span, .. } => *span,
            Expr::Call { span, .. } => *span,
            Expr::Field { span, .. } => *span,
            Expr::MethodCall { span, .. } => *span,
            Expr::Index { span, .. } => *span,
            Expr::Block(block) => block.span,
            Expr::If { span, .. } => *span,
            Expr::Match { span, .. } => *span,
            Expr::Loop { span, .. } => *span,
            Expr::While { span, .. } => *span,
            Expr::For { span, .. } => *span,
            Expr::StructLit { span, .. } => *span,
            Expr::Array { span, .. } => *span,
            Expr::Tuple { span, .. } => *span,
            Expr::Ref { span, .. } => *span,
            Expr::Deref { span, .. } => *span,
            Expr::Cast { span, .. } => *span,
            Expr::Range { span, .. } => *span,
            Expr::Unsafe { span, .. } => *span,
            Expr::Asm { span, .. } => *span,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Result<Program> {
        let lexer = Lexer::new(source, 0);
        let mut parser = Parser::new(lexer);
        parser.parse_program()
    }

    #[test]
    fn test_empty_function() {
        let program = parse("fn main() {}").unwrap();
        assert_eq!(program.items.len(), 1);
    }

    #[test]
    fn test_function_with_return() {
        let program = parse("fn add(a: i32, b: i32) -> i32 { return a + b }").unwrap();
        assert_eq!(program.items.len(), 1);
    }

    #[test]
    fn test_let_statement() {
        let program = parse("fn main() { let x = 42 }").unwrap();
        assert_eq!(program.items.len(), 1);
    }

    #[test]
    fn test_struct_def() {
        let program = parse("struct Point { x: i32, y: i32 }").unwrap();
        assert_eq!(program.items.len(), 1);
    }

    #[test]
    fn test_if_expr() {
        let program = parse("fn main() { if x > 0 { return 1 } else { return 0 } }").unwrap();
        assert_eq!(program.items.len(), 1);
    }
}
