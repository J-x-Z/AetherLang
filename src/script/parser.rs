use super::token::{Token, TokenKind, Span};
use super::lexer::Lexer;
use super::ast::*;

#[path = "parser_limits.rs"]
mod limits;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    limits: limits::ParseLimits,
}

impl Parser {
    pub fn new(input: &str) -> Self {
        let normalized = format!("{}\n", input.trim_end_matches('\n'));
        let mut lexer = Lexer::new(&normalized);
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token();
            if token.kind == TokenKind::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }
        
        Self {
            tokens,
            pos: 0,
            limits: limits::ParseLimits::default(),
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }
    
    fn advance(&mut self) -> &Token {
        if self.pos < self.tokens.len() {
            let t = &self.tokens[self.pos];
            self.pos += 1;
            t
        } else {
            // Return EOF token if out of bounds (shouldn't happen with EOF sentinel)
            &self.tokens[self.tokens.len() - 1]
        }
    }
    
    fn match_kind(&mut self, kind: TokenKind) -> bool {
        if self.peek().kind == kind {
            self.advance();
            true
        } else {
            false
        }
    }

    fn consume(&mut self, kind: TokenKind, msg: &str) -> Result<&Token, String> {
        let t = self.peek();
        // Simple discriminant check? TokenKind is PartialEq
        // But TokenKind::Identifier(String) makes strict equality hard if we just want "any Identifier"
        // Need specific checks.
        
        let matches = match (&t.kind, &kind) {
            (TokenKind::Identifier(_), TokenKind::Identifier(_)) => true, // Ignore content
            (TokenKind::String(_), TokenKind::String(_)) => true,
            (TokenKind::Integer(_), TokenKind::Integer(_)) => true,
            (k1, k2) => k1 == k2,
        };
        
        if matches {
            Ok(self.advance())
        } else {
            Err(format!("Expected {:?}, got {:?}: {}", kind, t.kind, msg))
        }
    }
    
    // --- Parsing Methods ---

    pub fn parse(&mut self) -> Result<ScriptModule, String> {
        self.parse_inner().map_err(|message| {
            if message.starts_with("line ") { message } else {
                format!("line {}:{}: {}", self.peek().span.line, self.peek().span.column, message)
            }
        })
    }

    fn parse_inner(&mut self) -> Result<ScriptModule, String> {
        for token in &self.tokens {
            if let TokenKind::Invalid(message) = &token.kind { return Err(format!("line {}:{}: {}", token.span.line, token.span.column, message)); }
        }
        let mut stmts = Vec::new();
        while self.peek().kind != TokenKind::Eof {
            // Skip newlines at top level
            if self.peek().kind == TokenKind::Newline {
                self.advance();
                continue;
            }
            stmts.push(self.parse_stmt()?);
        }
        Ok(ScriptModule { stmts })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        self.limits.enter(self.peek().span)?;
        let result = self.parse_stmt_inner();
        self.limits.leave();
        result
    }

    fn parse_stmt_inner(&mut self) -> Result<Stmt, String> {
        match self.peek().kind {
            TokenKind::From => self.parse_core_import(),
            TokenKind::Def | TokenKind::Comptime | TokenKind::Async => self.parse_function_def(),
            TokenKind::If => self.parse_if(),
            TokenKind::While => self.parse_while(),
            TokenKind::For => self.parse_for(),
            TokenKind::Return => self.parse_return(),
            TokenKind::Break | TokenKind::Continue => {
                let is_break = self.peek().kind == TokenKind::Break;
                self.advance();
                self.consume(TokenKind::Newline, "Expected newline after loop control")?;
                Ok(if is_break { Stmt::Break } else { Stmt::Continue })
            }
            TokenKind::Pass => {
                self.advance();
                self.consume(TokenKind::Newline, "Expected newline after pass")?;
                Ok(Stmt::Pass)
            }
            _ => self.parse_expr_stmt(),
        }
    }
    
    fn parse_core_import(&mut self)->Result<Stmt,String> {
        let span=self.advance().span;
        let mut path=Vec::new();
        loop {
            let TokenKind::Identifier(name)=self.peek().kind.clone() else {return Err("Core module path needs identifiers".into());};
            self.advance();path.push(name);
            if !self.match_kind(TokenKind::Dot) {break;}
        }
        self.consume(TokenKind::Import,"Expected import after Core module path")?;
        let mut names=Vec::new();
        loop {
            let TokenKind::Identifier(name)=self.peek().kind.clone() else {return Err("Core import needs explicit names; wildcard import is not supported".into());};
            self.advance();
            let alias=if self.match_kind(TokenKind::As) {
                let TokenKind::Identifier(alias)=self.peek().kind.clone() else {return Err("Expected import alias".into());};self.advance();alias
            }else{name.clone()};
            names.push((name,alias));
            if !self.match_kind(TokenKind::Comma) {break;}
        }
        self.consume(TokenKind::Newline,"Expected newline after import")?;
        Ok(Stmt::Import(CoreImport{path,names,span}))
    }

    fn parse_function_def(&mut self) -> Result<Stmt, String> {
        let is_async=self.match_kind(TokenKind::Async);
        let is_comptime = if self.peek().kind == TokenKind::Comptime {
            self.advance();
            true
        } else {
            false
        };
        
        let start_span = self.peek().span;
        self.consume(TokenKind::Def, "Expected 'def'")?;
        
        // Handle Identifier manually to extract name
        let name_token = self.peek().clone();
        let name = match name_token.kind {
            TokenKind::Identifier(s) => { self.advance(); s },
            _ => return Err("Expected function name".to_string()),
        };
        
        let mut const_params = Vec::new();
        if self.match_kind(TokenKind::LBracket) {
            loop {
                let keyword = self.advance().clone();
                if !matches!(keyword.kind, TokenKind::Identifier(ref name) if name == "const") {
                    return Err("Script generic parameters use 'const N: int'".into());
                }
                let token = self.advance().clone();
                let TokenKind::Identifier(name) = token.kind else { return Err("Expected const parameter name".into()); };
                self.consume(TokenKind::Colon, "Expected ':' in const parameter")?;
                let type_hint = Some(self.parse_type_hint()?);
                const_params.push(Param {name,type_hint,span:token.span});
                if !self.match_kind(TokenKind::Comma) { break; }
            }
            self.consume(TokenKind::RBracket, "Expected ']' after const parameters")?;
        }
        self.consume(TokenKind::LParen, "Expected '('")?;
        let params = self.parse_params()?;
        self.consume(TokenKind::RParen, "Expected ')'")?;
        
        let return_type = if self.match_kind(TokenKind::Arrow) {
            Some(self.parse_type_hint()?)
        } else {
            None
        };
        
        self.consume(TokenKind::Colon, "Expected ':'")?;
        self.consume(TokenKind::Newline, "Expected Newline after function header")?;
        
        let body = self.parse_block()?;
        
        // Simplify span: just start
        Ok(Stmt::FunctionDef(FunctionDef {
            name,
            params,
            const_params,
            return_type,
            body,
            is_comptime,
            is_async,
            span: start_span,
        }))
    }
    
    fn parse_params(&mut self) -> Result<Vec<Param>, String> {
        let mut params = Vec::new();
        if self.peek().kind != TokenKind::RParen {
            loop {
                // Parse Param
                let p_token = self.peek().clone();
                let name = match p_token.kind {
                    TokenKind::Identifier(s) => { self.advance(); s },
                    _ => return Err("Expected parameter name".to_string()),
                };
                
                let type_hint = if self.match_kind(TokenKind::Colon) {
                    Some(self.parse_type_hint()?)
                } else {
                    None
                };
                
                params.push(Param {
                    name,
                    type_hint,
                    span: p_token.span,
                });
                
                if !self.match_kind(TokenKind::Comma) {
                    break;
                }
            }
        }
        Ok(params)
    }
    
    fn parse_type_hint(&mut self) -> Result<TypeHint, String> {
        self.limits.enter(self.peek().span)?;
        let result = self.parse_type_hint_inner();
        self.limits.leave();
        result
    }

    fn parse_type_hint_inner(&mut self) -> Result<TypeHint, String> {
        let t = self.peek().clone();
        let (name,const_value) = match t.kind {
            TokenKind::Identifier(s) => { self.advance(); (s,None) },
            TokenKind::Integer(value) => { self.advance(); (String::new(),Some(value)) },
            _ => return Err("Expected type name or array dimension".to_string()),
        };
        
        let mut generics = Vec::new();
        if self.match_kind(TokenKind::LBracket) {
            loop {
                generics.push(self.parse_type_hint()?);
                if !self.match_kind(TokenKind::Comma) {
                    break;
                }
            }
            self.consume(TokenKind::RBracket, "Expected ']'")?;
        }
        
        Ok(TypeHint { name, generics, const_value, span: t.span })
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, String> {
        self.limits.enter(self.peek().span)?;
        let result = self.parse_block_inner();
        self.limits.leave();
        result
    }

    fn parse_block_inner(&mut self) -> Result<Vec<Stmt>, String> {
        self.consume(TokenKind::Indent, "Expected indented block")?;
        let mut stmts = Vec::new();
        while self.peek().kind != TokenKind::Dedent && self.peek().kind != TokenKind::Eof {
             // Handle empty lines/comments if they leaked through? No, lexer handles them?
             // Lexer emits newlines for empty lines sometimes?
             // A newline alone is not a statement?
             // Actually, `parse_stmt` handles statement ending newlines.
             // If we have just newline, skip it.
             if self.peek().kind == TokenKind::Newline {
                 self.advance();
                 continue;
             }
             stmts.push(self.parse_stmt()?);
        }
        self.consume(TokenKind::Dedent, "Expected end of block (Dedent)")?;
        Ok(stmts)
    }
    
    fn parse_if(&mut self) -> Result<Stmt, String> {
        let start_span = self.peek().span;
        self.consume(TokenKind::If, "Expected 'if'")?;
        let condition = self.parse_expr()?;
        self.consume(TokenKind::Colon, "Expected ':'")?;
        self.consume(TokenKind::Newline, "Expected Newline")?;
        let then_block = self.parse_block()?;
        
        let else_block = self.parse_if_tail()?;

        Ok(Stmt::If(IfStmt {
            condition,
            then_block,
            else_block,
            span: start_span,
        }))
    }

    fn parse_if_tail(&mut self) -> Result<Option<Vec<Stmt>>, String> {
        self.limits.enter(self.peek().span)?;
        let result = self.parse_if_tail_inner();
        self.limits.leave();
        result
    }

    fn parse_if_tail_inner(&mut self) -> Result<Option<Vec<Stmt>>, String> {
        if self.match_kind(TokenKind::Else) {
            self.consume(TokenKind::Colon, "Expected ':'")?;
            self.consume(TokenKind::Newline, "Expected newline")?;
            return Ok(Some(self.parse_block()?));
        }
        if self.match_kind(TokenKind::Elif) {
            let span = self.peek().span;
            let condition = self.parse_expr()?;
            self.consume(TokenKind::Colon, "Expected ':'")?;
            self.consume(TokenKind::Newline, "Expected newline")?;
            let then_block = self.parse_block()?;
            let else_block = self.parse_if_tail()?;
            return Ok(Some(vec![Stmt::If(IfStmt { condition, then_block, else_block, span })]));
        }
        Ok(None)
    }

    fn parse_while(&mut self) -> Result<Stmt, String> {
        let start_span = self.peek().span;
        self.consume(TokenKind::While, "Expected 'while'")?;
        let condition = self.parse_expr()?;
        self.consume(TokenKind::Colon, "Expected ':'")?;
        self.consume(TokenKind::Newline, "Expected Newline")?;
        let body = self.parse_block()?;

        Ok(Stmt::While(WhileStmt {
            condition,
            body,
            span: start_span,
        }))
    }

    fn parse_for(&mut self) -> Result<Stmt, String> {
        let start_span = self.peek().span;
        self.consume(TokenKind::For, "Expected 'for'")?;

        // Get loop variable name
        let var_token = self.peek().clone();
        let var = match var_token.kind {
            TokenKind::Identifier(s) => { self.advance(); s },
            _ => return Err("Expected loop variable name".to_string()),
        };

        self.consume(TokenKind::In, "Expected 'in'")?;
        let iterable = self.parse_expr()?;
        self.consume(TokenKind::Colon, "Expected ':'")?;
        self.consume(TokenKind::Newline, "Expected Newline")?;
        let body = self.parse_block()?;

        Ok(Stmt::For(ForStmt {
            var,
            iterable,
            body,
            span: start_span,
        }))
    }

    fn parse_return(&mut self) -> Result<Stmt, String> {
        let span = self.peek().span;
        self.consume(TokenKind::Return, "Expected 'return'")?;
        let value = if self.peek().kind != TokenKind::Newline {
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.consume(TokenKind::Newline, "Expected Newline")?;
        Ok(Stmt::Return(ReturnStmt { value, span }))
    }
    
    fn parse_expr_stmt(&mut self) -> Result<Stmt, String> {
        let left = self.parse_expr()?;
        
        let operator = self.peek().kind.clone();
        if matches!(operator, TokenKind::Eq | TokenKind::Compound(_)) {
            self.advance();
            let compound = match operator { TokenKind::Compound(op) => Some(op), _ => None };
            let right = self.parse_expr()?;
            self.consume(TokenKind::Newline, "Expected Newline")?;
            let span = left.span_owned();
            Ok(Stmt::Assign(AssignStmt {
                compound,
                target: left,
                value: right,
                span,
            }))
        } else {
            self.consume(TokenKind::Newline, "Expected Newline")?;
            Ok(Stmt::Expr(left))
        }
    }
    
    // --- Expression Parsing (Simple precedence) ---
    
    fn parse_expr(&mut self) -> Result<Expr, String> { self.parse_binary(0) }

    fn parse_binary(&mut self, minimum: u8) -> Result<Expr, String> {
        self.limits.enter(self.peek().span)?;
        let result = self.parse_binary_inner(minimum);
        self.limits.leave();
        if let Ok(value) = &result { self.limits.expression(value)?; }
        result
    }

    fn parse_binary_inner(&mut self, minimum: u8) -> Result<Expr, String> {
        let mut left = self.parse_unary()?;
        loop {
            let (precedence, op) = match self.peek().kind {
                TokenKind::Or => (1, BinOp::Or), TokenKind::And => (2, BinOp::And),
                TokenKind::EqEq => (3, BinOp::Eq), TokenKind::NotEq => (3, BinOp::Ne),
                TokenKind::Lt => (4, BinOp::Lt), TokenKind::LtEq => (4, BinOp::Le),
                TokenKind::Gt => (4, BinOp::Gt), TokenKind::GtEq => (4, BinOp::Ge),
                TokenKind::BitOr => (5, BinOp::BitOr), TokenKind::BitXor => (6, BinOp::BitXor),
                TokenKind::BitAnd => (7, BinOp::BitAnd),
                TokenKind::Shl => (8, BinOp::Shl), TokenKind::Shr => (8, BinOp::Shr),
                TokenKind::Plus => (9, BinOp::Add), TokenKind::Minus => (9, BinOp::Sub),
                TokenKind::Star => (10, BinOp::Mul), TokenKind::Slash => (10, BinOp::Div),
                TokenKind::Percent => (10, BinOp::Mod), _ => break,
            };
            if precedence < minimum { break; }
            self.advance();
            let right = self.parse_binary(precedence + 1)?;
            let mut span = left.span_owned();
            span.end = right.span().end;
            left = Expr::Binary { left: Box::new(left), op, right: Box::new(right), span };
            self.limits.expression(&left)?;
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        self.limits.enter(self.peek().span)?;
        let result = self.parse_unary_inner();
        self.limits.leave();
        if let Ok(value) = &result { self.limits.expression(value)?; }
        result
    }

    fn parse_unary_inner(&mut self) -> Result<Expr, String> {
        let token = self.peek().clone();
        if token.kind==TokenKind::Await {
            self.advance();let value=self.parse_call()?;let mut span=token.span;span.end=value.span().end;
            return Ok(Expr::Call{func:Box::new(Expr::Identifier{name:"@async_await".into(),span:token.span}),args:vec![value],span});
        }
        let op = match token.kind { TokenKind::Minus => Some(UnaryOp::Neg), TokenKind::Not => Some(UnaryOp::Not), _ => None };
        if let Some(op) = op {
            self.advance();
            let expr = if matches!(op, UnaryOp::Not) { self.parse_binary(3)? } else { self.parse_unary()? };
            let mut span = token.span;
            span.end = expr.span().end;
            return Ok(Expr::Unary { op, expr: Box::new(expr), span });
        }
        self.parse_call()
    }

    fn parse_call(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_primary()?;
        
        loop {
            if self.match_kind(TokenKind::LParen) {
                let mut args = Vec::new();
                if self.peek().kind != TokenKind::RParen {
                    loop {
                        args.push(self.parse_expr()?);
                        if !self.match_kind(TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.consume(TokenKind::RParen, "Expected ')'")?;
                let mut span = expr.span_owned();
                span.end = self.tokens[self.pos - 1].span.end;
                expr = Expr::Call { func: Box::new(expr), args, span };
            } else if self.match_kind(TokenKind::LBracket) {
                let index = self.parse_expr()?;
                self.consume(TokenKind::RBracket, "Expected ']' after array index")?;
                let mut span = expr.span_owned();
                span.end = self.tokens[self.pos - 1].span.end;
                expr = Expr::Index {target:Box::new(expr),index:Box::new(index),span};
            } else if self.match_kind(TokenKind::Dot) {
                let name = match self.peek().kind.clone() {
                    TokenKind::Identifier(s) => { self.advance(); s },
                    _ => return Err("Expected field name".to_string()),
                };
                let mut span = expr.span_owned();
                span.end = self.tokens[self.pos - 1].span.end;
                expr = Expr::FieldAccess { target: Box::new(expr), field: name, span };
            } else {
                break;
            }
            self.limits.expression(&expr)?;
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        self.limits.enter(self.peek().span)?;
        let result = self.parse_primary_inner();
        self.limits.leave();
        if let Ok(value) = &result { self.limits.expression(value)?; }
        result
    }

    fn parse_primary_inner(&mut self) -> Result<Expr, String> {
        let t = self.peek().clone();
        match t.kind {
            TokenKind::Lambda => {
                self.advance();
                self.consume(TokenKind::LParen,"lambda requires typed parameters in parentheses")?;
                let params=self.parse_params()?;
                self.consume(TokenKind::RParen,"Expected ')' after lambda parameters")?;
                if params.iter().any(|p|p.type_hint.is_none()) {return Err("lambda parameters need explicit type annotations".into());}
                let ret_type=if self.match_kind(TokenKind::Arrow) {Some(self.parse_type_hint()?)}else{None};
                self.consume(TokenKind::Colon,"Expected ':' before lambda expression")?;
                let body=self.parse_expr()?;
                let mut span=t.span;span.end=body.span().end;
                Ok(Expr::Lambda {params,ret_type,body:Box::new(body),span})
            }
            TokenKind::Identifier(s) => {
                self.advance();
                Ok(Expr::Identifier { name: s, span: t.span })
            }
            TokenKind::Integer(i) => {
                self.advance();
                Ok(Expr::Integer { value: i, span: t.span })
            }
            TokenKind::True | TokenKind::False => {
                self.advance();
                Ok(Expr::Boolean { value: t.kind == TokenKind::True, span: t.span })
            }
            TokenKind::Float(value) => {
                self.advance();
                Ok(Expr::Float { value, span: t.span })
            }
            TokenKind::String(s) => {
                self.advance();
                Ok(Expr::String { value: s, span: t.span })
            }
            TokenKind::LBracket => {
                self.advance();
                let mut elements = Vec::new();
                while self.peek().kind != TokenKind::RBracket {
                    elements.push(self.parse_expr()?);
                    if !self.match_kind(TokenKind::Comma) { break; }
                }
                self.consume(TokenKind::RBracket, "Expected ']' after fixed array")?;
                let mut span = t.span; span.end = self.tokens[self.pos - 1].span.end;
                Ok(Expr::List {elements,span})
            }
            TokenKind::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.consume(TokenKind::RParen, "Expected ')'")?;
                Ok(expr)
            }
            _ => Err(format!("Unexpected token in expression: {:?}", t.kind)),
        }
    }
}

// Helpers for Expr span extraction
impl Expr {
    fn span_owned(&self) -> Span {
        match self {
            Expr::Identifier { span, .. } => *span,
            Expr::Integer { span, .. } | Expr::Boolean { span, .. } | Expr::Unary { span, .. } => *span,
            Expr::Float { span, .. } => *span,
            Expr::String { span, .. } => *span,
            Expr::Binary { span, .. } => *span,
            Expr::Call { span, .. } => *span,
            Expr::FieldAccess { span, .. } => *span,
            Expr::Lambda {span,..} | Expr::List { span, .. } | Expr::Index {span,..} => *span,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_func() {
        let input = "
def main(args: List[String]) -> int:
    print(\"Hello\")
    return 0
";
        let mut parser = Parser::new(input);
        let module = parser.parse().expect("Failed to parse");
        
        assert_eq!(module.stmts.len(), 1);
        match &module.stmts[0] {
            Stmt::FunctionDef(f) => {
                assert_eq!(f.name, "main");
                assert_eq!(f.params.len(), 1);
                assert_eq!(f.params[0].name, "args");
                // Check return type
                assert!(f.return_type.is_some());
                
                assert_eq!(f.body.len(), 2);
                match &f.body[0] {
                    Stmt::Expr(Expr::Call { func, args, .. }) => {
                        // Check func name 'print'
                        // Check args "Hello"
                    }
                    _ => panic!("Expected Expr Stmt"),
                }
                match &f.body[1] {
                    Stmt::Return(_) => {}
                    _ => panic!("Expected Return Stmt"),
                }
            }
            _ => panic!("Expected FunctionDef"),
        }
    }
    
    #[test]
    fn test_parse_nested_blocks() {
        let input = "
def test():
    if x:
        y = 1
        if z:
            pass
    return y
";
         let mut parser = Parser::new(input);
         let module = parser.parse().expect("Failed to parse nested");
         // Basic structure check
         if let Stmt::FunctionDef(f) = &module.stmts[0] {
             assert_eq!(f.body.len(), 2); // If, Return
         }
    }
}
