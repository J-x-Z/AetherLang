//! Parser for AetherLang
//!
//! Recursive descent parser with Pratt parsing for expressions.
#![allow(dead_code)]

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

    /// Parse generic parameters: <T, U>
    fn parse_generic_params(&mut self) -> Result<Vec<Ident>> {
        self.expect(TokenKind::Lt)?;
        let mut params = Vec::new();
        loop {
            params.push(self.parse_ident()?);
            if self.check(&TokenKind::Gt) {
                break;
            }
            self.expect(TokenKind::Comma)?;
        }
        self.expect(TokenKind::Gt)?;
        Ok(params)
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
        // Collect attributes (#[...])
        let mut attributes = Vec::new();
        while self.check(&TokenKind::Hash) {
            attributes.push(self.parse_attribute()?);
        }
        
        // Handle pub modifier - peek ahead to see what comes next
        match self.current_kind() {
            TokenKind::Pub => {
                // pub can precede fn, struct, enum, impl, interface, etc.
                if let Some(next) = self.peek() {
                    match &next.kind {
                        TokenKind::Fn => Ok(Item::Function(self.parse_function()?)),
                        TokenKind::Struct => Ok(Item::Struct(self.parse_struct_with_attrs(attributes)?)),
                        TokenKind::Enum => {
                            self.advance(); // consume 'pub'
                            Ok(Item::Enum(self.parse_enum()?))
                        },
                        TokenKind::Impl => {
                            self.advance(); // consume 'pub'
                            Ok(Item::Impl(self.parse_impl()?))
                        },
                        TokenKind::Interface => {
                            self.advance(); // consume 'pub'
                            Ok(Item::Interface(self.parse_interface()?))
                        },
                        _ => Err(Error::UnexpectedToken {
                            expected: "fn, struct, enum, impl or interface after pub".to_string(),
                            got: format!("{:?}", next.kind),
                            span: next.span,
                        }),
                    }
                } else {
                    Err(Error::UnexpectedToken {
                        expected: "item after pub".to_string(),
                        got: "EOF".to_string(),
                        span: self.current().span,
                    })
                }
            }
            TokenKind::Fn => Ok(Item::Function(self.parse_function()?)),
            TokenKind::Struct => Ok(Item::Struct(self.parse_struct_with_attrs(attributes)?)),
            TokenKind::Enum => Ok(Item::Enum(self.parse_enum()?)),
            TokenKind::Impl => Ok(Item::Impl(self.parse_impl()?)),
            TokenKind::Interface => Ok(Item::Interface(self.parse_interface()?)),
            TokenKind::Const => Ok(Item::Const(self.parse_const()?)),
            // Phase 8: System features
            TokenKind::Extern => Ok(Item::Extern(self.parse_extern_block()?)),
            TokenKind::Static => Ok(Item::Static(self.parse_static_item()?)),
            TokenKind::Union => Ok(Item::Union(self.parse_union_def()?)),
            _ => Err(Error::UnexpectedToken {
                expected: "item (fn, struct, enum, impl, interface, const, extern, static, union)".to_string(),
                got: format!("{:?}", self.current_kind()),
                span: self.current().span,
            }),
        }
    }
    
    /// Parse an attribute: #[name] or #[name(args)]
    fn parse_attribute(&mut self) -> Result<Annotation> {
        let start_span = self.current().span;
        self.expect(TokenKind::Hash)?;
        self.expect(TokenKind::LBracket)?;
        
        let name = self.parse_ident()?;
        
        let mut args = Vec::new();
        if self.check(&TokenKind::LParen) {
            self.advance(); // consume '('
            while !self.check(&TokenKind::RParen) && !self.is_at_end() {
                args.push(self.parse_expr()?);
                if !self.consume(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(TokenKind::RParen)?;
        }
        
        let end_token = self.expect(TokenKind::RBracket)?;
        let span = start_span.merge(&end_token.span);
        
        Ok(Annotation { name, args, span })
    }

    /// Parse a function definition
    /// Syntax: fn name(params) -> type [requires ..., ensures ...] effect[...] { body }
    fn parse_function(&mut self) -> Result<Function> {
        let start = self.current().span;
        
        // Check for pub
        let is_pub = self.consume(&TokenKind::Pub);
        
        self.expect(TokenKind::Fn)?;

        let name = self.parse_ident()?;

        let type_params = if self.check(&TokenKind::Lt) {
            self.parse_generic_params()?
        } else {
            Vec::new()
        };

        self.expect(TokenKind::LParen)?;
        let params = self.parse_params()?;
        self.expect(TokenKind::RParen)?;

        let ret_type = if self.consume(&TokenKind::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        
        // Parse optional contract block: [requires ..., ensures ...]
        let contracts = if self.check(&TokenKind::LBracket) {
            self.parse_contract_block()?
        } else {
            Vec::new()
        };
        
        // Parse optional effect annotation: pure or effect[...]
        let effects = self.parse_effect_annotation()?;

        let body = self.parse_block()?;

        Ok(Function {
            name,
            type_params,
            params,
            ret_type,
            body,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
            annotations: Vec::new(),
            contracts,
            effects,
            is_pub,
        })
    }
    
    /// Parse contract block: [requires cond1, ensures cond2, ...]
    fn parse_contract_block(&mut self) -> Result<Vec<Contract>> {
        self.expect(TokenKind::LBracket)?;
        let mut contracts = Vec::new();
        
        while !self.check(&TokenKind::RBracket) && !self.is_at_end() {
            let start = self.current().span;
            
            let kind = match self.current_kind() {
                TokenKind::Requires => {
                    self.advance();
                    ContractKind::Requires
                }
                TokenKind::Ensures => {
                    self.advance();
                    ContractKind::Ensures
                }
                TokenKind::Invariant => {
                    self.advance();
                    ContractKind::Invariant
                }
                _ => {
                    return Err(Error::UnexpectedToken {
                        expected: "requires, ensures, or invariant".to_string(),
                        got: format!("{:?}", self.current_kind()),
                        span: self.current().span,
                    });
                }
            };
            
            let condition = self.parse_expr()?;
            let end_span = self.tokens[self.pos.saturating_sub(1)].span;
            
            contracts.push(Contract {
                kind,
                condition,
                span: start.merge(&end_span),
            });
            
            // Optional comma between contracts
            self.consume(&TokenKind::Comma);
        }
        
        self.expect(TokenKind::RBracket)?;
        Ok(contracts)
    }
    
    /// Parse effect annotation: pure or effect[read, write, io, ...]
    fn parse_effect_annotation(&mut self) -> Result<EffectSet> {
        if self.consume(&TokenKind::Pure) {
            return Ok(EffectSet { is_pure: true, effects: Vec::new() });
        }
        
        if self.consume(&TokenKind::Effect) {
            self.expect(TokenKind::LBracket)?;
            let mut effects = Vec::new();
            
            while !self.check(&TokenKind::RBracket) && !self.is_at_end() {
                if let TokenKind::Ident(ref name) = self.current_kind().clone() {
                    let effect = match name.as_str() {
                        "read" => Effect::Read,
                        "write" => Effect::Write,
                        "io" => Effect::IO,
                        "alloc" => Effect::Alloc,
                        "panic" => Effect::Panic,
                        _ => {
                            return Err(Error::UnexpectedToken {
                                expected: "read, write, io, alloc, or panic".to_string(),
                                got: name.clone(),
                                span: self.current().span,
                            });
                        }
                    };
                    self.advance();
                    effects.push(effect);
                    self.consume(&TokenKind::Comma);
                } else {
                    break;
                }
            }
            
            self.expect(TokenKind::RBracket)?;
            return Ok(EffectSet { is_pure: false, effects });
        }
        
        // Default: no effect annotation
        Ok(EffectSet::default())
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

    /// Parse an annotation: @name or @name(args)
    fn parse_annotation(&mut self) -> Result<Annotation> {
        let start = self.current().span;
        self.expect(TokenKind::At)?;
        
        let name = self.parse_ident()?;
        
        // Parse optional arguments: @name(arg1, arg2)
        let args = if self.consume(&TokenKind::LParen) {
            let mut args = Vec::new();
            while !self.check(&TokenKind::RParen) && !self.is_at_end() {
                args.push(self.parse_expr()?);
                if !self.consume(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(TokenKind::RParen)?;
            args
        } else {
            Vec::new()
        };
        
        Ok(Annotation {
            name,
            args,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
        })
    }

    fn parse_type(&mut self) -> Result<Type> {
        let start = self.current().span;

        // Ownership modifiers: own T, ref T, mut T, shared T
        // These create an Owned type wrapper (for future semantic analysis)
        if self.consume(&TokenKind::Own) {
            let inner = self.parse_type()?;
            return Ok(Type::Owned {
                inner: Box::new(inner),
                ownership: Ownership::Own,
                span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
            });
        }
        
        if self.consume(&TokenKind::Shared) {
            let inner = self.parse_type()?;
            return Ok(Type::Owned {
                inner: Box::new(inner),
                ownership: Ownership::Shared,
                span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
            });
        }

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

        // Named type or Generic type
        if let TokenKind::Ident(name) = self.current_kind().clone() {
            self.advance();
            let ty_name = name;

            // Parse generic arguments if present: Foo<i32, String>
            if self.consume(&TokenKind::Lt) {
                 let mut inner_types = Vec::new();
                 loop {
                     inner_types.push(self.parse_type()?);
                     if self.check(&TokenKind::Gt) {
                         break;
                     }
                     self.expect(TokenKind::Comma)?;
                 }
                 self.expect(TokenKind::Gt)?;
                 
                 return Ok(Type::Generic(ty_name, inner_types, start.merge(&self.tokens[self.pos.saturating_sub(1)].span)));
            }

            return Ok(Type::Named(ty_name, start.merge(&self.tokens[self.pos.saturating_sub(1)].span)));
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
            
            // Handle Call: expr(args)
            if op_token.kind == TokenKind::LParen {
                let bp = 20; // Postfix precedence
                if bp < min_bp {
                    break;
                }
                self.advance(); // consume (
                left = self.parse_call_rest(left)?;
                continue;
            }

            let Some(bp) = op_token.kind.binary_precedence() else {
                break;
            };

            if bp < min_bp {
                break;
            }

            // Handle Cast: expr as Type
            if op_token.kind == TokenKind::As {
                self.advance(); // consume 'as'
                let ty = self.parse_type()?;
                let span = left.span().merge(&ty.span()); // assuming Type has span()
                
                left = Expr::Cast {
                    expr: Box::new(left),
                    ty,
                    span,
                };
                continue;
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

    fn parse_call_rest(&mut self, func: Expr) -> Result<Expr> {
        let mut args = Vec::new();
        if !self.check(&TokenKind::RParen) {
            loop {
                args.push(self.parse_expr()?);
                if !self.consume(&TokenKind::Comma) {
                    break;
                }
                if self.check(&TokenKind::RParen) {
                    break;
                }
            }
        }
        self.expect(TokenKind::RParen)?;
        
        // Calculate span
        let start_span = func.span();
        let end_span = self.tokens[self.pos.saturating_sub(1)].span;

        Ok(Expr::Call {
            func: Box::new(func),
            args,
            span: start_span.merge(&end_span),
        })
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
                // Use lookahead: only parse as struct lit if { is followed by ident:
                // This avoids ambiguity with `if cond { ... }`
                let is_struct_lit = if self.check(&TokenKind::LBrace) {
                    // Look ahead: check if next token after { is Ident followed by Colon
                    self.pos + 1 < self.tokens.len() && 
                    matches!(&self.tokens[self.pos + 1].kind, TokenKind::Ident(_)) &&
                    self.pos + 2 < self.tokens.len() &&
                    matches!(&self.tokens[self.pos + 2].kind, TokenKind::Colon)
                } else {
                    false
                };

                if is_struct_lit {
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

                // Check for Path: Ident::Ident
                if self.check(&TokenKind::ColonColon) {
                    let mut segments = vec![ident];
                    while self.consume(&TokenKind::ColonColon) {
                        segments.push(self.parse_ident()?);
                    }
                    let span = segments[0].span.merge(&segments.last().unwrap().span);
                    return Ok(Expr::Path {
                        segments,
                        span,
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

            // Unsafe with optional AI metadata: unsafe(reason="...", verifier=name) { }
            TokenKind::Unsafe => {
                self.advance();
                
                // Parse optional metadata: unsafe(reason = "...", verifier = name)
                let (reason, verifier) = if self.consume(&TokenKind::LParen) {
                    let mut reason = None;
                    let mut verifier = None;
                    
                    while !self.check(&TokenKind::RParen) && !self.is_at_end() {
                        let key = self.parse_ident()?;
                        self.expect(TokenKind::Eq)?;
                        
                        match key.name.as_str() {
                            "reason" => {
                                if let TokenKind::StringLit(s) = self.current_kind() {
                                    reason = Some(s.clone());
                                    self.advance();
                                }
                            }
                            "verifier" => {
                                verifier = Some(self.parse_ident()?);
                            }
                            _ => {}
                        }
                        
                        self.consume(&TokenKind::Comma);
                    }
                    self.expect(TokenKind::RParen)?;
                    (reason, verifier)
                } else {
                    (None, None)
                };
                
                let body = self.parse_block()?;
                Expr::Unsafe {
                    span: token.span.merge(&body.span),
                    body,
                    reason,
                    verifier,
                }
            }
            
            // Inline Assembly: asm!("template", in("reg") val, out("reg") val, clobber("memory"))
            TokenKind::Asm => {
                self.advance();
                self.expect(TokenKind::Not)?; // asm!
                self.expect(TokenKind::LParen)?;
                
                // Parse template string
                let template = if let TokenKind::StringLit(s) = self.current_kind() {
                    let t = s.clone();
                    self.advance();
                    t
                } else {
                    return Err(Error::Expected("string literal".into(), self.current().span));
                };

                let mut operands = Vec::new();
                while self.consume(&TokenKind::Comma) && !self.check(&TokenKind::RParen) {
                    let ident_str = if let TokenKind::In = self.current_kind() {
                        self.advance();
                        "in".to_string()
                    } else if let TokenKind::Ident(name) = self.current_kind() {
                        let s = name.clone();
                        self.advance();
                        s
                    } else {
                         return Err(Error::Expected("asm operand type (in/out/inout/clobber)".into(), self.current().span));
                    };

                    let kind = match ident_str.as_str() {
                        "in" => AsmOperandKind::Input,
                        "out" => AsmOperandKind::Output,
                        "inout" => AsmOperandKind::InOut,
                        "clobber" => AsmOperandKind::Clobber,
                        _ => return Err(Error::Expected("asm operand type (in/out/inout/clobber)".into(), self.tokens[self.pos.saturating_sub(1)].span)),
                    };
                    
                    self.expect(TokenKind::LParen)?;
                    
                    // Parse options (register class or clobber string)
                    let options = if let TokenKind::StringLit(s) = self.current_kind() {
                        let o = s.clone();
                        self.advance();
                        o
                    } else if let TokenKind::Ident(i) = &self.current().kind {
                        let o = i.clone();
                        self.advance();
                        o
                    } else {
                        return Err(Error::Expected("string or identifier".into(), self.current().span));
                    };
                    
                    self.expect(TokenKind::RParen)?;
                    
                    let expr = if kind != AsmOperandKind::Clobber {
                        Some(self.parse_expr()?)
                    } else {
                        None
                    };
                    
                    operands.push(AsmOperand { kind, options, expr });
                }
                
                self.expect(TokenKind::RParen)?;
                
                Expr::Asm {
                    template,
                    operands,
                    span: token.span.merge(&self.tokens[self.pos.saturating_sub(1)].span),
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

            // Closure: |x, y| expr or |x: T| -> T expr
            TokenKind::Or => {
                self.advance(); // consume |
                let mut params = Vec::new();
                
                // Parse parameters: |x, y: T, z|
                while !self.check(&TokenKind::Or) && !self.is_at_end() {
                    let name = self.parse_ident()?;
                    let ty = if self.consume(&TokenKind::Colon) {
                        Some(self.parse_type()?)
                    } else {
                        None
                    };
                    params.push(ClosureParam { name, ty });
                    if !self.consume(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::Or)?; // closing |
                
                // Optional return type: -> T
                let ret_type = if self.consume(&TokenKind::Arrow) {
                    Some(self.parse_type()?)
                } else {
                    None
                };
                
                // Body can be a block { ... } or single expression
                let body = if self.check(&TokenKind::LBrace) {
                    Expr::Block(self.parse_block()?)
                } else {
                    self.parse_expr()?
                };
                let body_span = body.span();
                
                Expr::Closure {
                    params,
                    ret_type,
                    body: Box::new(body),
                    span: token.span.merge(&body_span),
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
            } else if self.consume(&TokenKind::Question) {
                // Error propagation (try operator)
                expr = Expr::Try {
                    span: expr.span().merge(&self.tokens[self.pos.saturating_sub(1)].span),
                    expr: Box::new(expr),
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
            if self.check(&TokenKind::If) {
                // else if: parse as nested if expression wrapped in a block
                let nested_if = self.parse_if_expr()?;
                let span = nested_if.span();
                Some(Block {
                    stmts: vec![Stmt::Expr(nested_if)],
                    span,
                })
            } else {
                Some(self.parse_block()?)
            }
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
                let first_ident = Ident { name: name.clone(), span: token.span };
                
                // Check for path pattern (e.g., Color::Red)
                if self.consume(&TokenKind::ColonColon) {
                    let variant = self.parse_ident()?;
                    let end_span = variant.span;
                    
                    // Check for optional tuple payload (e.g., Some(x))
                    let fields = if self.consume(&TokenKind::LParen) {
                        let mut fields = vec![];
                        if !self.check(&TokenKind::RParen) {
                            fields.push(self.parse_pattern()?);
                            while self.consume(&TokenKind::Comma) {
                                if self.check(&TokenKind::RParen) { break; }
                                fields.push(self.parse_pattern()?);
                            }
                        }
                        self.expect(TokenKind::RParen)?;
                        fields
                    } else {
                        vec![]
                    };
                    
                    Ok(Pattern::Variant {
                        enum_name: Some(first_ident),
                        variant,
                        fields,
                        span: token.span.merge(&end_span),
                    })
                } else {
                    if name == "_" {
                        Ok(Pattern::Wildcard { span: token.span })
                    } else {
                        Ok(Pattern::Binding {
                            name: first_ident,
                            mutable: false,
                            span: token.span,
                        })
                    }
                }
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
        self.parse_struct_with_attrs(Vec::new())
    }
    
    fn parse_struct_with_attrs(&mut self, mut annotations: Vec<Annotation>) -> Result<StructDef> {
        let start = self.current().span;
        // Also collect @-style annotations for backward compatibility
        while self.consume(&TokenKind::At) {
            annotations.push(self.parse_annotation()?);
        }

        let is_pub = self.consume(&TokenKind::Pub);
        self.expect(TokenKind::Struct)?;

        let name = self.parse_ident()?;
        
        let type_params = if self.check(&TokenKind::Lt) {
            self.parse_generic_params()?
        } else {
            Vec::new()
        };

        // Parse optional invariants/contracts before body
        let invariants = if self.check(&TokenKind::LBracket) {
            self.parse_contract_block()?
        } else {
            Vec::new()
        };

        self.expect(TokenKind::LBrace)?;

        let mut fields = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            // Allow optional 'pub' visibility on fields (currently ignored)
            let _is_pub_field = self.consume(&TokenKind::Pub);
            
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
            type_params,
            fields,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
            annotations,
            invariants,
            is_pub,
        })
    }

    fn parse_enum(&mut self) -> Result<EnumDef> {
        let start = self.current().span;
        self.expect(TokenKind::Enum)?;
        let name = self.parse_ident()?;
        
        let type_params = if self.check(&TokenKind::Lt) {
            self.parse_generic_params()?
        } else {
            Vec::new()
        };

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
            type_params,
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
            type_params: Vec::new(),
            methods,
            default_methods: Vec::new(),
            associated_types: Vec::new(),
            supertraits: Vec::new(),
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
            is_pub: false,
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
            effects: EffectSet::default(),
            contracts: Vec::new(),
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

    // ==================== Phase 8: System Features ====================

    /// Parse extern block: extern "C" { fn declarations... }
    fn parse_extern_block(&mut self) -> Result<ExternBlock> {
        let start = self.current().span;
        self.expect(TokenKind::Extern)?;

        // Parse optional ABI string (e.g., "C", "stdcall")
        let abi = if let TokenKind::StringLit(s) = self.current_kind() {
            let abi_str = s.clone();
            self.advance();
            Some(abi_str)
        } else {
            None
        };

        self.expect(TokenKind::LBrace)?;

        let mut items = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            items.push(self.parse_foreign_item()?);
        }

        self.expect(TokenKind::RBrace)?;

        Ok(ExternBlock {
            abi,
            items,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
        })
    }

    /// Parse a foreign item (fn or static) inside extern block
    /// Supports @annotation syntax before fn declarations
    fn parse_foreign_item(&mut self) -> Result<ForeignItem> {
        let start = self.current().span;
        
        // Parse any @annotations before the item
        let mut annotations = Vec::new();
        while self.check(&TokenKind::At) {
            annotations.push(self.parse_annotation()?);
        }

        match self.current_kind() {
            TokenKind::Fn => {
                self.advance();
                let name = self.parse_ident()?;
                self.expect(TokenKind::LParen)?;
                let params = self.parse_params()?;
                self.expect(TokenKind::RParen)?;

                let ret_type = if self.consume(&TokenKind::Arrow) {
                    Some(self.parse_type()?)
                } else {
                    None
                };

                self.consume(&TokenKind::Semicolon);

                Ok(ForeignItem::Fn {
                    name,
                    params,
                    ret_type,
                    annotations,
                    span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
                })
            }
            TokenKind::Static => {
                self.advance();
                let is_mut = self.consume(&TokenKind::Mut);
                let name = self.parse_ident()?;
                self.expect(TokenKind::Colon)?;
                let ty = self.parse_type()?;
                self.consume(&TokenKind::Semicolon);

                Ok(ForeignItem::Static {
                    name,
                    ty,
                    is_mut,
                    span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
                })
            }
            _ => Err(Error::UnexpectedToken {
                expected: "fn or static in extern block".to_string(),
                got: format!("{:?}", self.current_kind()),
                span: self.current().span,
            }),
        }
    }

    /// Parse static variable: static [mut] name: type = value;
    fn parse_static_item(&mut self) -> Result<StaticDef> {
        let start = self.current().span;
        let is_pub = self.consume(&TokenKind::Pub);
        self.expect(TokenKind::Static)?;

        let is_mut = self.consume(&TokenKind::Mut);
        let name = self.parse_ident()?;

        self.expect(TokenKind::Colon)?;
        let ty = self.parse_type()?;

        let value = if self.consume(&TokenKind::Eq) {
            Some(self.parse_expr()?)
        } else {
            None
        };

        self.consume(&TokenKind::Semicolon);

        Ok(StaticDef {
            name,
            ty,
            value,
            is_mut,
            is_pub,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
        })
    }

    /// Parse union definition: union Name { fields... }
    fn parse_union_def(&mut self) -> Result<UnionDef> {
        let start = self.current().span;
        let is_pub = self.consume(&TokenKind::Pub);
        self.expect(TokenKind::Union)?;

        let name = self.parse_ident()?;

        self.expect(TokenKind::LBrace)?;

        let mut fields = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let field_name = self.parse_ident()?;
            self.expect(TokenKind::Colon)?;
            let field_ty = self.parse_type()?;
            self.consume(&TokenKind::Comma);

            fields.push(Field {
                name: field_name.clone(),
                ty: field_ty,
                span: field_name.span,
            });
        }

        self.expect(TokenKind::RBrace)?;

        Ok(UnionDef {
            name,
            fields,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
            is_pub,
            repr: None, // TODO: Parse #[repr(...)] attribute
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
            Expr::Path { span, .. } => *span,
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
            Expr::Try { span, .. } => *span,
            Expr::Closure { span, .. } => *span,
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
    
    #[test]
    fn test_function_with_contracts() {
        let program = parse("fn divide(a: i32, b: i32) -> i32 [requires b != 0] { a / b }").unwrap();
        assert_eq!(program.items.len(), 1);
        if let Item::Function(f) = &program.items[0] {
            assert_eq!(f.contracts.len(), 1);
            assert!(matches!(f.contracts[0].kind, ContractKind::Requires));
        } else {
            panic!("Expected function");
        }
    }
    
    #[test]
    fn test_function_with_effects() {
        let program = parse("fn log(msg: str) effect[io] { }").unwrap();
        assert_eq!(program.items.len(), 1);
        if let Item::Function(f) = &program.items[0] {
            assert!(!f.effects.is_pure);
            assert_eq!(f.effects.effects.len(), 1);
            assert!(matches!(f.effects.effects[0], Effect::IO));
        } else {
            panic!("Expected function");
        }
    }
    
    #[test]
    fn test_pure_function() {
        let program = parse("fn add(a: i32, b: i32) -> i32 pure { a + b }").unwrap();
        assert_eq!(program.items.len(), 1);
        if let Item::Function(f) = &program.items[0] {
            assert!(f.effects.is_pure);
        } else {
            panic!("Expected function");
        }
    }
    
    #[test]
    fn test_pub_function() {
        let program = parse("pub fn main() {}").unwrap();
        assert_eq!(program.items.len(), 1);
        if let Item::Function(f) = &program.items[0] {
            assert!(f.is_pub);
        } else {
            panic!("Expected function");
        }
    }
}
