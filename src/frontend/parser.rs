//! Parser for AetherLang
//!
//! Recursive descent parser with Pratt parsing for expressions.
#![allow(dead_code)]

use crate::frontend::token::{Token, TokenKind};
use crate::frontend::ast::*;
use crate::frontend::lexer::Lexer;
use crate::utils::{Span, Error, Result};

#[path = "parser_limits.rs"]
mod limits;

/// The parser
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    allow_struct_literal: bool,
    limits: limits::ParseLimits,
}

impl Parser {
    /// Create a new parser from a lexer
    pub fn new(mut lexer: Lexer) -> Self {
        Self {
            tokens: lexer.tokenize(),
            pos: 0,
            allow_struct_literal: true,
            limits: limits::ParseLimits::default(),
        }
    }

    /// Create a parser from pre-tokenized input
    pub fn from_tokens(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0, allow_struct_literal: true, limits: limits::ParseLimits::default() }
    }

    /// Parse generic parameters: <T, U> or <T, const N: usize>
    fn parse_generic_params(&mut self) -> Result<Vec<GenericParam>> {
        self.expect(TokenKind::Lt)?;
        let mut params = Vec::new();
        loop {
            // Check for const generic: `const N: Type`
            if self.check(&TokenKind::Const) {
                self.advance(); // consume 'const'
                let name = self.parse_ident()?;
                self.expect(TokenKind::Colon)?;
                let ty = self.parse_type()?;
                params.push(GenericParam::Const {
                    name,
                    ty: Box::new(ty),
                });
            } else {
                // Regular type parameter
                let name = self.parse_ident()?;
                params.push(GenericParam::Type(name));
            }

            if self.at_type_close() {
                break;
            }
            self.expect(TokenKind::Comma)?;
        }
        self.expect(TokenKind::Gt)?;
        Ok(params)
    }

    /// Parse legacy type-only generic parameters (for backward compatibility)
    fn parse_type_params_legacy(&mut self) -> Result<Vec<Ident>> {
        self.expect(TokenKind::Lt)?;
        let mut params = Vec::new();
        loop {
            params.push(self.parse_ident()?);
            if self.at_type_close() {
                break;
            }
            self.expect(TokenKind::Comma)?;
        }
        self.expect(TokenKind::Gt)?;
        Ok(params)
    }

    /// Parse a generic argument: either a type or a const expression
    /// Examples: `i32`, `String`, `3`, `N`, `{N + 1}`
    fn parse_generic_arg(&mut self) -> Result<GenericArg> {
        // Check for const expression in braces: {N + 1}
        if self.check(&TokenKind::LBrace) {
            self.advance();
            let expr = self.parse_expr()?;
            self.expect(TokenKind::RBrace)?;
            return Ok(GenericArg::Const(expr));
        }

        // Check for integer literal: 3, 42
        if let TokenKind::IntLit(n) = self.current_kind().clone() {
            let span = self.current().span;
            self.advance();
            return Ok(GenericArg::Const(Expr::Literal(Literal::Int(n, span))));
        }

        // Bare identifiers are types. Const arguments use literals or braces;
        // capitalization never changes the meaning of a user-defined type.
        // Default: parse as type
        let ty = self.parse_type()?;
        Ok(GenericArg::Type(ty))
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
        // In a type context one closing angle may be the first byte of >=,
        // >> or >>=. Keep the remaining operator and its exact source span.
        if matches!(expected, TokenKind::Gt) && !self.check(&TokenKind::Gt) {
            let rest = match self.current_kind() {
                TokenKind::Ge => Some(TokenKind::Eq),
                TokenKind::Shr => Some(TokenKind::Gt),
                TokenKind::ShrEq => Some(TokenKind::Ge),
                _ => None,
            };
            if let Some(rest) = rest {
                let mut token = self.current().clone();
                token.kind = TokenKind::Gt;
                token.span.end = token.span.start + 1;
                self.tokens[self.pos].kind = rest;
                self.tokens[self.pos].span.start += 1;
                return Ok(token);
            }
        }
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

    fn at_type_close(&self) -> bool {
        matches!(self.current_kind(), TokenKind::Gt | TokenKind::Ge | TokenKind::Shr | TokenKind::ShrEq)
    }

    fn parse_control_expr(&mut self) -> Result<Expr> {
        let previous = self.allow_struct_literal;
        self.allow_struct_literal = false;
        let result = self.parse_expr();
        self.allow_struct_literal = previous;
        result
    }

    // ==================== Parsing Methods ====================

    /// Parse a complete program
    pub fn parse_program(&mut self) -> Result<Program> {
        if let Some(token) = self.tokens.iter().find(|token| matches!(token.kind, TokenKind::Invalid(_))) {
            if let TokenKind::Invalid(message) = &token.kind {
                return Err(Error::UnexpectedToken { expected: "valid token".into(), got: message.clone(), span: token.span });
            }
        }
        // Parse inner attributes: #![no_std], #![no_main], etc.
        let mut inner_attrs = Vec::new();
        while self.check(&TokenKind::Hash) {
            if let Some(next) = self.peek() {
                if next.kind == TokenKind::Not {
                    inner_attrs.push(self.parse_inner_attribute()?);
                    continue;
                }
            }
            break;
        }
        
        let mut items = Vec::new();
        while !self.is_at_end() {
            items.push(self.parse_item()?);
        }

        Ok(Program { items, inner_attrs, module_owners: Default::default(), foreign_uses: Default::default() })
    }
    
    /// Parse inner attribute: #![name]
    fn parse_inner_attribute(&mut self) -> Result<Annotation> {
        let start_span = self.current().span;
        self.expect(TokenKind::Hash)?;
        self.expect(TokenKind::Not)?;
        self.expect(TokenKind::LBracket)?;
        
        let name = self.parse_ident()?;
        
        // Inner attributes like #![no_std] don't need args
        self.expect(TokenKind::RBracket)?;
        
        Ok(Annotation {
            name,
            args: Vec::new(),
            span: start_span.merge(&self.tokens[self.pos.saturating_sub(1)].span),
        })
    }

    /// Parse a top-level item
    fn parse_item(&mut self) -> Result<Item> {
        self.limits.enter(self.current().span)?;
        let result = self.parse_item_inner();
        self.limits.leave();
        result
    }

    fn parse_item_inner(&mut self) -> Result<Item> {
        // Both annotation spellings may be mixed without changing their meaning.
        let mut attributes = Vec::new();
        while self.check(&TokenKind::Hash) || self.check(&TokenKind::At) {
            attributes.push(if self.check(&TokenKind::Hash) { self.parse_attribute()? } else { self.parse_annotation()? });
        }
        
        // Handle pub modifier - peek ahead to see what comes next
        match self.current_kind() {
            TokenKind::Pub => {
                // pub can precede fn, struct, enum, impl, interface, etc.
                if let Some(next) = self.peek() {
                    match &next.kind {
                        TokenKind::Const => {
                            self.advance();
                            Ok(Item::Const(self.parse_const()?))
                        },
                        TokenKind::Fn => {
                            let mut function = self.parse_function()?;
                            function.annotations = attributes;
                            Ok(Item::Function(function))
                        },
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
                            let mut definition = self.parse_interface()?;
                            definition.is_pub = true;
                            Ok(Item::Interface(definition))
                        },
                        TokenKind::Trait => {
                            self.advance(); // consume 'pub'
                            let mut definition = self.parse_trait_def()?;
                            definition.is_pub = true;
                            Ok(Item::Trait(definition))
                        },
                        TokenKind::Type => {
                            self.advance(); // consume 'pub'
                            let mut alias = self.parse_type_alias()?;
                            alias.is_pub = true;
                            Ok(Item::TypeAlias(alias))
                        },
                        TokenKind::Use => {
                            self.advance();
                            let mut item = self.parse_use()?;
                            item.is_pub = true;
                            Ok(Item::Use(item))
                        },
                        TokenKind::Static => Ok(Item::Static(self.parse_static_item(attributes)?)),
                        TokenKind::Union => Ok(Item::Union(self.parse_union_with_attrs(attributes)?)),
                        _ => Err(Error::UnexpectedToken {
                            expected: "fn, struct, enum, impl, interface or type after pub".to_string(),
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
            TokenKind::Fn => {
                let mut func = self.parse_function()?;
                func.annotations = attributes;
                Ok(Item::Function(func))
            }
            TokenKind::Struct => Ok(Item::Struct(self.parse_struct_with_attrs(attributes)?)),
            TokenKind::Enum => Ok(Item::Enum(self.parse_enum()?)),
            TokenKind::Impl => Ok(Item::Impl(self.parse_impl()?)),
            TokenKind::Interface => Ok(Item::Interface(self.parse_interface()?)),
            TokenKind::Const => Ok(Item::Const(self.parse_const()?)),
            // Phase 8: System features
            TokenKind::Extern => Ok(Item::Extern(self.parse_extern_block()?)),
            TokenKind::Static => Ok(Item::Static(self.parse_static_item(attributes)?)),
            TokenKind::Union => Ok(Item::Union(self.parse_union_with_attrs(attributes)?)),
            // P1 Phase B: Trait and Type alias
            TokenKind::Trait => Ok(Item::Trait(self.parse_trait_def()?)),
            TokenKind::Type => Ok(Item::TypeAlias(self.parse_type_alias()?)),
            // Use/import statement
            TokenKind::Use => Ok(Item::Use(self.parse_use()?)),
            // Module declaration
            TokenKind::Mod => Ok(Item::Module(self.parse_mod()?)),
            _ => Err(Error::UnexpectedToken {
                expected: "item (fn, struct, enum, impl, interface, const, extern, static, union, trait, type, use)".to_string(),
                got: format!("{:?}", self.current_kind()),
                span: self.current().span,
            }),
        }
    }
    
    fn parse_annotation_name(&mut self) -> Result<Ident> {
        if self.current_kind().is_keyword() {
            let name = format!("{:?}", self.current_kind()).to_ascii_lowercase();
            let span = self.current().span;
            self.advance();
            Ok(Ident { name, span })
        } else { self.parse_ident() }
    }

    fn parse_union_with_attrs(&mut self, attributes: Vec<Annotation>) -> Result<UnionDef> {
        let mut definition = self.parse_union_def()?;
        for annotation in attributes {
            if annotation.name.name == "repr" {
                let repr = match annotation.args.as_slice() {
                    [Expr::Ident(name)] if name.name == "C" => Repr::C,
                    [Expr::Ident(name)] if name.name == "packed" => Repr::Packed,
                    _ => return Err(Error::Expected("union repr(C) or repr(packed)".into(), annotation.span)),
                };
                definition.repr = Some(repr);
            }
        }
        Ok(definition)
    }

    /// Parse an attribute: #[name] or #[name(args)]
    fn parse_attribute(&mut self) -> Result<Annotation> {
        let start_span = self.current().span;
        self.expect(TokenKind::Hash)?;
        self.expect(TokenKind::LBracket)?;
        
        let name = self.parse_annotation_name()?;
        
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
            self.parse_generic_params()?.into_iter().map(|parameter| match parameter {
                GenericParam::Type(ident) => Ok(ident),
                GenericParam::Const { name, .. } => Err(Error::Expected("type generic parameter; function const generics are not implemented".into(), name.span)),
            }).collect::<Result<Vec<_>>>()?
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

        while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::DotDotDot) && !self.is_at_end() {
            params.push(self.parse_param()?);
            if !self.consume(&TokenKind::Comma) {
                break;
            }
            // After comma, check if next is ... (variadic indicator)
            if self.check(&TokenKind::DotDotDot) {
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
        
        let name = self.parse_annotation_name()?;
        
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
        self.limits.enter(self.current().span)?;
        let result = self.parse_type_inner();
        self.limits.leave();
        result
    }

    fn parse_type_inner(&mut self) -> Result<Type> {
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
            let volatile = self.consume(&TokenKind::Volatile);
            let inner = self.parse_type()?;
            let span = start.merge(&self.tokens[self.pos.saturating_sub(1)].span);
            return Ok(if volatile { Type::Volatile(Box::new(inner), span) }
                else { Type::Pointer(Box::new(inner), span) });
        }

        // Reference type with optional lifetime: &T or &'a T or &'a mut T
        if self.consume(&TokenKind::And) {
            // Check for optional lifetime: &'a
            let lifetime = if let TokenKind::Lifetime(name) = self.current_kind() {
                let lt = name.clone();
                self.advance();
                Some(lt)
            } else {
                None
            };
            let mutable = self.consume(&TokenKind::Mut);
            let inner = self.parse_type()?;
            return Ok(Type::Ref {
                mutable,
                lifetime,
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
                        let n = usize::try_from(*n).map_err(|_| Error::Expected("array size representable on this host".into(), self.current().span))?;
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
            let mut ty_name = name;
            while self.consume(&TokenKind::ColonColon) {
                ty_name.push_str("::");
                ty_name.push_str(&self.parse_ident()?.name);
            }

            // Parse generic arguments if present: Foo<i32, String> or Foo<i32, 3, 3>
            if self.consume(&TokenKind::Lt) {
                 let mut generic_args = Vec::new();
                 loop {
                     // Try to parse as a const expression first (number literals or identifiers that could be const params)
                     let arg = self.parse_generic_arg()?;
                     generic_args.push(arg);

                     if self.at_type_close() {
                         break;
                     }
                     self.expect(TokenKind::Comma)?;
                 }
                 self.expect(TokenKind::Gt)?;

                 let span = start.merge(&self.tokens[self.pos.saturating_sub(1)].span);
                 if generic_args.iter().any(|arg| matches!(arg, GenericArg::Const(_))) {
                     return Ok(Type::GenericWithArgs { name: ty_name, args: generic_args, span });
                 }
                 let inner_types = generic_args.into_iter().filter_map(|arg| match arg {
                     GenericArg::Type(ty) => Some(ty), GenericArg::Const(_) => None,
                 }).collect();
                 return Ok(Type::Generic(ty_name, inner_types, span));
            }

            return Ok(Type::Named(ty_name, start.merge(&self.tokens[self.pos.saturating_sub(1)].span)));
        }

        if self.check(&TokenKind::Fn) {
            return Err(Error::Semantic { code: "E2403", span: self.current().span,
                message: "function value types and closures are not implemented; use a direct named call".into() });
        }
        Err(Error::ExpectedType { span: self.current().span })
    }

    fn parse_block(&mut self) -> Result<Block> {
        self.limits.enter(self.current().span)?;
        let result = self.parse_block_inner();
        self.limits.leave();
        if let Ok(value) = &result { self.limits.block(value)?; }
        result
    }

    fn parse_block_inner(&mut self) -> Result<Block> {
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
                // A block-like expression at statement position ends at its closing
                // brace. A following `*p = ...` is a new statement, not multiplication.
                // Expression contexts (let/return/arguments) still use full precedence.
                let expr = if matches!(self.current_kind(), TokenKind::If | TokenKind::While | TokenKind::For | TokenKind::Loop | TokenKind::Match | TokenKind::Unsafe | TokenKind::LBrace) {
                    self.parse_primary()?
                } else { self.parse_expr()? };
                if self.consume(&TokenKind::Semicolon) {
                    Ok(Stmt::Semi(expr))
                } else {
                    Ok(Stmt::Expr(expr))
                }
            }
        }
    }

    fn parse_let_stmt(&mut self) -> Result<Stmt> {
        let start = self.current().span;
        self.expect(TokenKind::Let)?;

        let mutable = self.consume(&TokenKind::Mut);
        let name = self.parse_ident()?;

        // P5.1: Type annotation is REQUIRED for let statements
        // AetherLang enforces explicit types to reduce AI hallucinations
        let ty = if self.consume(&TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            return Err(Error::Expected(
                "type annotation required: use `let x: Type = value;` (AetherLang requires explicit types)".into(),
                self.current().span
            ));
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
        self.limits.enter(self.current().span)?;
        let result = self.parse_expr_bp_inner(min_bp);
        self.limits.leave();
        if let Ok(value) = &result { self.limits.expression(value)?; }
        result
    }

    fn parse_expr_bp_inner(&mut self, min_bp: u8) -> Result<Expr> {
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
                self.limits.expression(&left)?;
                continue;
            }

            // Ranges bind below arithmetic/comparison and above assignment.
            if op_token.kind == TokenKind::DotDot {
                let bp = 2;
                if bp < min_bp { break; }
                self.advance();
                let end = self.parse_expr_bp(bp + 1)?;
                let span = left.span().merge(&end.span());
                left = Expr::Range { start: Some(Box::new(left)), end: Some(Box::new(end)), span };
                self.limits.expression(&left)?;
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
                self.limits.expression(&left)?;
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
            self.limits.expression(&left)?;
        }

        Ok(left)
    }

    fn parse_call_rest(&mut self, func: Expr) -> Result<Expr> {
        if let Expr::Ident(name) = &func {
            let query = match name.name.as_str() {
                "size_of" => Some(LayoutQuery::Size), "align_of" => Some(LayoutQuery::Align),
                "offset_of" => Some(LayoutQuery::Offset), _ => None,
            };
            if let Some(query) = query {
                let ty = self.parse_type()?;
                let field = if query == LayoutQuery::Offset { self.expect(TokenKind::Comma)?; Some(self.parse_ident()?) } else { None };
                let end = self.expect(TokenKind::RParen)?.span;
                return Ok(Expr::Layout { query, ty, field, span: name.span.merge(&end) });
            }
        }
        let previous = self.allow_struct_literal;
        self.allow_struct_literal = true;
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
        self.allow_struct_literal = previous;
        
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
        self.limits.enter(self.current().span)?;
        let result = self.parse_primary_inner();
        self.limits.leave();
        if let Ok(value) = &result { self.limits.expression(value)?; }
        result
    }

    // Keep recursive prefix parsing outside the large atom match. In debug
    // builds that match has a sizeable frame even when just parsing a '-'.
    fn parse_primary_inner(&mut self) -> Result<Expr> {
        if !matches!(self.current_kind(), TokenKind::Minus | TokenKind::Tilde | TokenKind::Not | TokenKind::Star | TokenKind::And) {
            return self.parse_atom();
        }
        let token = self.advance();
        let mutable = token.kind == TokenKind::And && self.consume(&TokenKind::Mut);
        let inner = self.parse_primary()?;
        let span = token.span.merge(&inner.span());
        let expr = match token.kind {
            TokenKind::Star => Expr::Deref { expr: Box::new(inner), span },
            TokenKind::And => Expr::Ref { expr: Box::new(inner), mutable, span },
            kind => Expr::Unary {
                op: match kind { TokenKind::Minus => UnOp::Neg, TokenKind::Tilde => UnOp::BitNot, _ => UnOp::Not },
                expr: Box::new(inner), span,
            },
        };
        self.parse_postfix(expr)
    }

    fn parse_atom(&mut self) -> Result<Expr> {
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
                
                // A control-flow head reserves the following brace for its body.
                // Parentheses restore the ordinary expression grammar.
                let is_struct_lit = self.allow_struct_literal && self.check(&TokenKind::LBrace);

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
                    return self.parse_postfix(Expr::Tuple {
                        elements: Vec::new(),
                        span: token.span.merge(&self.tokens[self.pos.saturating_sub(1)].span),
                    });
                }

                let previous = self.allow_struct_literal;
                self.allow_struct_literal = true;
                let first = self.parse_expr()?;
                self.allow_struct_literal = previous;
                if self.consume(&TokenKind::Comma) {
                    let mut elements = vec![first];
                    while !self.check(&TokenKind::RParen) && !self.is_at_end() {
                        elements.push(self.parse_expr()?);
                        if !self.consume(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(TokenKind::RParen)?;
                    return self.parse_postfix(Expr::Tuple {
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
                let cond = self.parse_control_expr()?;
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
                let iter = self.parse_control_expr()?;
                let body = self.parse_block()?;
                Expr::For {
                    var,
                    iter: Box::new(iter),
                    span: token.span.merge(&body.span),
                    body,
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

            // Array literal: [a, b, c] or [expr; N]
            TokenKind::LBracket => {
                self.advance();
                let mut elements = Vec::new();

                // Check for empty array
                if self.check(&TokenKind::RBracket) {
                    self.advance();
                    return Ok(Expr::Array {
                        elements,
                        span: token.span.merge(&self.tokens[self.pos.saturating_sub(1)].span),
                    });
                }

                // Parse first element
                let first = self.parse_expr()?;

                // Check for repeat syntax: [expr; N]
                if self.consume(&TokenKind::Semicolon) {
                    let count = match self.current_kind() {
                        TokenKind::IntLit(n) => {
                            let n = usize::try_from(*n).map_err(|_| Error::Expected("array repeat count representable on this host".into(), self.current().span))?;
                            self.advance();
                            n
                        }
                        _ => return Err(Error::Expected(
                            "integer literal for array repeat count".into(),
                            self.current().span
                        )),
                    };
                    self.expect(TokenKind::RBracket)?;
                    // Repeat the expression N times
                    for _ in 0..count {
                        elements.push(first.clone());
                    }
                } else {
                    // Normal array literal: [a, b, c]
                    elements.push(first);
                    while self.consume(&TokenKind::Comma) {
                        if self.check(&TokenKind::RBracket) {
                            break; // trailing comma
                        }
                        elements.push(self.parse_expr()?);
                    }
                    self.expect(TokenKind::RBracket)?;
                }

                Expr::Array {
                    elements,
                    span: token.span.merge(&self.tokens[self.pos.saturating_sub(1)].span),
                }
            }

            // Closure: |x, y| expr or |x: T| -> T expr
            TokenKind::Or => {
                self.advance(); // consume |
                let mut params = Vec::new();
                
                // Parse parameters: |x: T, y: T| - types are REQUIRED (P5.1)
                while !self.check(&TokenKind::Or) && !self.is_at_end() {
                    let name = self.parse_ident()?;
                    // P5.1: Closure parameters MUST have explicit types
                    let ty = if self.consume(&TokenKind::Colon) {
                        Some(self.parse_type()?)
                    } else {
                        return Err(Error::Expected(
                            "closure parameter requires type annotation: use `|x: Type|` (AetherLang requires explicit types)".into(),
                            self.current().span
                        ));
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
        self.limits.expression(&expr)?;
        loop {
            if self.consume(&TokenKind::LParen) {
                expr = self.parse_call_rest(expr)?;
            } else if self.consume(&TokenKind::Dot) {
                if let TokenKind::IntLit(index) = self.current_kind() {
                    let field = Ident { name: index.to_string(), span: self.current().span };
                    self.advance();
                    expr = Expr::Field { span: expr.span().merge(&field.span), expr: Box::new(expr), field };
                    self.limits.expression(&expr)?;
                    continue;
                }
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
            self.limits.expression(&expr)?;
        }
        Ok(expr)
    }

    fn parse_if_expr(&mut self) -> Result<Expr> {
        self.limits.enter(self.current().span)?;
        let result = self.parse_if_expr_inner();
        self.limits.leave();
        if let Ok(value) = &result { self.limits.expression(value)?; }
        result
    }

    fn parse_if_expr_inner(&mut self) -> Result<Expr> {
        let start = self.current().span;
        self.expect(TokenKind::If)?;

        let cond = self.parse_control_expr()?;
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

        let expr = self.parse_control_expr()?;
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
        let guard = if self.consume(&TokenKind::If) { Some(Box::new(self.parse_expr()?)) } else { None };

        self.expect(TokenKind::FatArrow)?;
        let body = self.parse_expr()?;

        self.consume(&TokenKind::Comma);

        Ok(MatchArm {
            pattern,
            guard,
            body,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
        })
    }

    fn parse_pattern(&mut self) -> Result<Pattern> {
        self.limits.enter(self.current().span)?;
        let result = self.parse_pattern_inner();
        self.limits.leave();
        result
    }

    fn parse_pattern_inner(&mut self) -> Result<Pattern> {
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
            TokenKind::Minus => {
                self.advance();
                let magnitude = self.current().clone();
                if let TokenKind::IntLit(value) = magnitude.kind {
                    self.advance();
                    Ok(Pattern::Literal(Literal::Int(-value, token.span.merge(&magnitude.span))))
                } else {
                    Err(Error::Expected("integer literal after '-' in pattern".into(), magnitude.span))
                }
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
        
        let generic_params = if self.check(&TokenKind::Lt) {
            self.parse_generic_params()?
        } else {
            Vec::new()
        };

        // Extract type-only params for backward compatibility
        let type_params: Vec<Ident> = generic_params.iter()
            .filter_map(|p| match p {
                GenericParam::Type(name) => Some(name.clone()),
                GenericParam::Const { .. } => None,
            })
            .collect();

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
            generic_params,
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
        
        let generic_params = if self.check(&TokenKind::Lt) {
            self.parse_generic_params()?
        } else {
            Vec::new()
        };

        // Extract type-only params for backward compatibility
        let type_params: Vec<Ident> = generic_params.iter()
            .filter_map(|p| match p {
                GenericParam::Type(name) => Some(name.clone()),
                GenericParam::Const { .. } => None,
            })
            .collect();

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
            generic_params,
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
            self.consume(&TokenKind::Semicolon);
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

        let effects = self.parse_effect_annotation()?;
        Ok(FunctionSig {
            name,
            params,
            ret_type,
            effects,
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
        self.consume(&TokenKind::Semicolon);

        Ok(ConstDef {
            name,
            ty,
            value,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
        })
    }

    // ==================== P1 Phase B: Trait and Type Alias ====================
    
    /// Parse trait definition: trait Name { fn method_sig; ... }
    fn parse_trait_def(&mut self) -> Result<InterfaceDef> {
        let start = self.current().span;
        self.expect(TokenKind::Trait)?;

        let name = self.parse_ident()?;
        
        // Optional type parameters: trait Name<T, U>
        let type_params = if self.consume(&TokenKind::Lt) {
            let mut params = Vec::new();
            while !self.at_type_close() && !self.is_at_end() {
                params.push(self.parse_ident()?);
                if !self.consume(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(TokenKind::Gt)?;
            params
        } else {
            Vec::new()
        };
        
        self.expect(TokenKind::LBrace)?;

        let mut methods = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            methods.push(self.parse_fn_sig()?);
            self.consume(&TokenKind::Semicolon);
        }

        self.expect(TokenKind::RBrace)?;

        Ok(InterfaceDef {
            name,
            type_params,
            methods,
            default_methods: Vec::new(),
            associated_types: Vec::new(),
            supertraits: Vec::new(),
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
            is_pub: false,
        })
    }
    
    /// Parse type alias: type Name = Type
    fn parse_type_alias(&mut self) -> Result<TypeAliasDef> {
        let start = self.current().span;
        self.expect(TokenKind::Type)?;
        
        let name = self.parse_ident()?;
        
        // Optional type parameters: type Name<T>
        let type_params = if self.consume(&TokenKind::Lt) {
            let mut params = Vec::new();
            while !self.at_type_close() && !self.is_at_end() {
                params.push(self.parse_ident()?);
                if !self.consume(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(TokenKind::Gt)?;
            params
        } else {
            Vec::new()
        };
        
        self.expect(TokenKind::Eq)?;
        let ty = self.parse_type()?;
        self.consume(&TokenKind::Semicolon);
        
        Ok(TypeAliasDef {
            name,
            type_params,
            ty,
            is_pub: false,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
        })
    }
    
    /// Parse a use tree. Group members keep paths relative to their parent.
    fn parse_use(&mut self) -> Result<UseDecl> {
        let start = self.expect(TokenKind::Use)?.span;
        let mut declaration = self.parse_use_tree()?;
        self.consume(&TokenKind::Semicolon);
        declaration.span = start.merge(&self.tokens[self.pos.saturating_sub(1)].span);
        Ok(declaration)
    }

    fn parse_use_tree(&mut self) -> Result<UseDecl> {
        let start = self.current().span;
        let mut path = Vec::new();
        let kind = loop {
            if self.consume(&TokenKind::Star) {
                break UseKind::Glob;
            }
            if self.consume(&TokenKind::LBrace) {
                let mut children = Vec::new();
                while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
                    children.push(self.parse_use_tree()?);
                    if !self.consume(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::RBrace)?;
                break UseKind::Group(children);
            }
            path.push(self.parse_ident()?);
            if self.consume(&TokenKind::ColonColon) {
                continue;
            }
            if self.consume(&TokenKind::As) {
                break UseKind::Alias(self.parse_ident()?);
            }
            break UseKind::Simple;
        };
        Ok(UseDecl {
            path,
            kind,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
            is_pub: false,
        })
    }

    /// Parse module declaration: mod name { items } or mod name;
    fn parse_mod(&mut self) -> Result<ModuleDef> {
        self.limits.enter(self.current().span)?;
        let result = self.parse_mod_inner();
        self.limits.leave();
        result
    }

    fn parse_mod_inner(&mut self) -> Result<ModuleDef> {
        let start = self.current().span;
        self.expect(TokenKind::Mod)?;
        
        let name = self.parse_ident()?;
        
        // Check for inline module (mod foo { ... }) or external (mod foo;)
        let items = if self.consume(&TokenKind::LBrace) {
            let mut items = Vec::new();
            while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
                items.push(self.parse_item()?);
            }
            self.expect(TokenKind::RBrace)?;
            Some(items)
        } else {
            // External module - file will be loaded by semantic phase
            self.consume(&TokenKind::Semicolon);
            None
        };
        
        Ok(ModuleDef {
            name,
            items,
            span: start.merge(&self.tokens[self.pos.saturating_sub(1)].span),
            is_pub: false,
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
                
                // Check for variadic ... after params
                let variadic = if self.check(&TokenKind::DotDotDot) {
                    self.advance();
                    true
                } else if !params.is_empty() && self.check(&TokenKind::Comma) {
                    // Handle case: fn printf(fmt: *u8, ...)
                    self.advance();
                    if self.check(&TokenKind::DotDotDot) {
                        self.advance();
                        true
                    } else {
                        // Wasn't ..., put comma back conceptually by not consuming
                        // Actually we already consumed it, so this is an error
                        // For now, just treat as non-variadic
                        false
                    }
                } else {
                    false
                };
                
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
                    variadic,
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
    fn parse_static_item(&mut self, annotations: Vec<Annotation>) -> Result<StaticDef> {
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
            annotations,
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
            Expr::Layout { span, .. } => *span,
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
        // P5.1: AetherLang requires explicit type annotations
        let program = parse("fn main() { let x: i32 = 42 }").unwrap();
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
    #[test]
    fn test_use_tree_preserves_groups_aliases_and_globs() {
        let program = parse("use foo::{bar as baz, nested::{x, y}, *};").unwrap();
        let Item::Use(root) = &program.items[0] else { panic!("expected use") };
        assert_eq!(root.path[0].name, "foo");
        let UseKind::Group(children) = &root.kind else { panic!("expected group") };
        assert_eq!(children.len(), 3);
        assert_eq!(children[0].path[0].name, "bar");
        assert!(matches!(&children[0].kind, UseKind::Alias(alias) if alias.name == "baz"));
        assert_eq!(children[1].path[0].name, "nested");
        assert!(matches!(&children[1].kind, UseKind::Group(items) if items.len() == 2));
        assert!(children[2].path.is_empty());
        assert!(matches!(children[2].kind, UseKind::Glob));
        let program = parse("use foo::bar as renamed;").unwrap();
        let Item::Use(root) = &program.items[0] else { panic!("expected use") };
        assert_eq!(root.path.len(), 2);
        assert!(matches!(&root.kind, UseKind::Alias(alias) if alias.name == "renamed"));
        assert!(parse("use foo::{bar baz};").is_err());
    }

    #[test]
    fn test_range_and_semicolon_are_preserved() {
        let program = parse("fn main() { for i in 1 + 2..10 - 1 { i; } 7; 8 }").unwrap();
        let Item::Function(function) = &program.items[0] else { panic!("expected fn") };
        assert!(matches!(function.body.stmts[1], Stmt::Semi(_)));
        assert!(matches!(function.body.stmts[2], Stmt::Expr(_)));
        let Stmt::Expr(Expr::For { iter, .. }) = &function.body.stmts[0] else { panic!("expected for") };
        assert!(matches!(iter.as_ref(), Expr::Range { start: Some(_), end: Some(_), .. }));
    }

    #[test]
    fn test_malformed_literals_fail_before_semantics() {
        for source in ["fn main() { let x: u64 = 18446744073709551616; }", "fn main() { let x: f64 = 1e+; }", "fn main() {} /* unfinished"] {
            assert!(matches!(parse(source), Err(Error::UnexpectedToken { .. })));
        }
    }

    #[test]
    fn integer_boundary_types_are_deferred_and_negative_patterns_preserved() {
        let program = parse("fn f(x:i64)->i64 { let high:u64=18446744073709551615; match x { -9223372036854775808=>1,_=>0 } }").unwrap();
        let Item::Function(function) = &program.items[0] else {panic!("function")};
        let statements: Vec<_> = function.body.stmts.iter().filter(|stmt| !matches!(stmt, Stmt::Empty {..})).collect();
        assert_eq!(statements.len(), 2);
        assert!(matches!(statements[0], Stmt::Let {value:Some(Expr::Literal(Literal::Int(value,_))),..} if *value == u64::MAX as i128));
        assert!(matches!(statements[1], Stmt::Expr(Expr::Match {arms,..}) if matches!(&arms[0].pattern,Pattern::Literal(Literal::Int(value,_)) if *value == i64::MIN as i128)));
    }

    #[test]
    fn block_statements_do_not_consume_following_dereference_assignment() {
        let program = parse("fn f(p: *i32, b: bool) { if b {} *p = 1; while b {} *p = 2; let x: i32 = if b { 1 } else { 2 }; }").unwrap();
        let Item::Function(function) = &program.items[0] else { panic!("expected fn") };
        assert_eq!(function.body.stmts.len(), 6);
        assert!(matches!(function.body.stmts[1], Stmt::Semi(Expr::Binary { op: BinOp::Assign, .. })));
        assert!(matches!(function.body.stmts[3], Stmt::Semi(Expr::Binary { op: BinOp::Assign, .. })));
    }

    #[test]
    fn qualified_types_and_const_generic_arguments_are_preserved() {
        let program = parse("fn f(x: module::Value, y: Array<i32, 4>) {}").unwrap();
        let Item::Function(function) = &program.items[0] else { panic!("expected fn") };
        assert!(matches!(&function.params[0].ty, Type::Named(name, _) if name == "module::Value"));
        assert!(matches!(&function.params[1].ty, Type::GenericWithArgs { args, .. } if args.len() == 2 && matches!(&args[1], GenericArg::Const(Expr::Literal(Literal::Int(4, _))))));
    }

    #[test]
    fn const_declarations_accept_optional_semicolons() {
        assert_eq!(parse("const X: i32 = 1; const Y: i32 = 2 fn f() {}").unwrap().items.len(), 3);
    }

    #[test]
    fn public_type_alias_preserves_export_visibility() {
        let program = parse("pub type Number = i32; type PrivateNumber = i64;").unwrap();
        let Item::TypeAlias(public) = &program.items[0] else { panic!("expected type alias") };
        let Item::TypeAlias(private) = &program.items[1] else { panic!("expected type alias") };
        assert!(public.is_pub);
        assert!(!private.is_pub);
        assert_eq!(public.name.name, "Number");
        assert!(matches!(&public.ty, Type::Named(name, _) if name == "i32"));
    }

    #[test]
    fn system_annotations_and_public_items_keep_their_meaning() {
        let program = parse("@volatile #[no_mangle] pub fn read(p: *volatile i32) -> i32 { return *p; }");
        assert!(program.is_ok());
        let program = parse("#[no_mangle] @volatile pub fn read(p: *volatile i32) -> i32 { return *p; } pub static mut X:i32=0; @repr(C) pub union U { a:i32, b:f32 }").unwrap();
        let Item::Function(f) = &program.items[0] else { panic!("function") };
        assert_eq!(f.annotations.iter().map(|a| a.name.name.as_str()).collect::<Vec<_>>(), vec!["no_mangle","volatile"]);
        assert!(matches!(f.params[0].ty, Type::Volatile(_, _)));
        assert!(matches!(&program.items[1], Item::Static(s) if s.is_pub));
        assert!(matches!(&program.items[2], Item::Union(u) if u.is_pub && u.repr == Some(Repr::C)));
        let program = parse("extern \"C\" { @pure fn abs(x:i32)->i32; }").unwrap();
        let Item::Extern(ext) = &program.items[0] else {panic!("extern")};
        assert!(matches!(&ext.items[0], ForeignItem::Fn { annotations, .. } if annotations[0].name.name == "pure"));
    }

    #[test]
    fn static_annotations_are_retained_for_linkage() {
        let program = parse("#[no_mangle] static mut ABI:i64=7; @no_mangle pub static ALSO:i32=2; pub static PUBLIC:i32=1;").unwrap();
        for item in &program.items[..2] {
            let Item::Static(definition) = item else { panic!("static") };
            assert_eq!(definition.annotations.len(), 1);
            assert_eq!(definition.annotations[0].name.name, "no_mangle");
        }
        assert!(matches!(&program.items[2], Item::Static(definition) if definition.is_pub && definition.annotations.is_empty()));
    }

    #[test]
    fn empty_aggregates_and_control_braces_are_unambiguous() {
        let p = parse("struct Empty {} fn f(b:bool) { let x:Empty=Empty{}; if b {} while b {} if test(Empty{}) {} match x {} if (Empty{}).ok {} }").unwrap();
        let Item::Function(f)=&p.items[1] else {panic!("function")};
        let statements:Vec<_>=f.body.stmts.iter().filter(|s|!matches!(s,Stmt::Empty{..})).collect();
        assert_eq!(statements.len(),6);
        assert!(matches!(statements[1],Stmt::Expr(Expr::If{cond,..}) if matches!(cond.as_ref(),Expr::Ident(_))));
    }

    #[test]
    fn adjacent_generic_closers_do_not_consume_assignment_or_shift() {
        let p=parse("type Link<T>=*T; fn f() { let x:Box<Box<i64>>=make(); let b:bool=4>=3; let n:i64=8>>1; }").unwrap();
        let Item::Function(f)=&p.items[1] else {panic!("function")};
        let statements:Vec<_>=f.body.stmts.iter().filter(|s|!matches!(s,Stmt::Empty{..})).collect();
        assert_eq!(statements.len(),3);
        assert!(matches!(statements[1],Stmt::Let{value:Some(Expr::Binary{op:BinOp::Ge,..}),..}));
        assert!(matches!(statements[2],Stmt::Let{value:Some(Expr::Binary{op:BinOp::Shr,..}),..}));
        assert!(parse("fn f<const N:usize>() {}").is_err());
        let p=parse("fn f(x:Box<UUID>) {}").unwrap();
        let Item::Function(f)=&p.items[0] else {panic!("function")};
        assert!(matches!(&f.params[0].ty,Type::Generic(_,args,_) if matches!(&args[0],Type::Named(name,_) if name=="UUID")));
    }

    #[test]
    fn layout_queries_take_types_not_value_expressions() {
        let program = parse("fn main() { size_of([u8;16]); align_of(*i64); offset_of(Packet, tail); }").unwrap();
        let Item::Function(f)=&program.items[0] else {panic!("function")};
        assert!(matches!(&f.body.stmts[0],Stmt::Semi(Expr::Layout {query:LayoutQuery::Size,ty:Type::Array {size:16,..},..})));
        assert!(matches!(&f.body.stmts[2],Stmt::Semi(Expr::Layout {query:LayoutQuery::Offset,field:Some(field),..}) if field.name=="tail"));
    }

}
