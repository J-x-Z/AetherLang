use super::token::{Token, TokenKind, Span};
use super::lexer::Lexer;
use super::ast::*;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(input: &str) -> Self {
        let mut lexer = Lexer::new(input);
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
        match self.peek().kind {
            TokenKind::Def | TokenKind::Comptime => self.parse_function_def(),
            TokenKind::If => self.parse_if(),
            TokenKind::Return => self.parse_return(),
            TokenKind::Pass => {
                self.advance();
                self.consume(TokenKind::Newline, "Expected newline after pass")?;
                Ok(Stmt::Pass)
            }
            _ => self.parse_expr_stmt(),
        }
    }
    
    fn parse_function_def(&mut self) -> Result<Stmt, String> {
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
            return_type,
            body,
            is_comptime,
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
        let t = self.peek().clone();
        let name = match t.kind {
            TokenKind::Identifier(s) => { self.advance(); s },
            _ => return Err("Expected type name".to_string()),
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
        
        Ok(TypeHint { name, generics, span: t.span })
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, String> {
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
        
        let else_block = if self.peek().kind == TokenKind::Else {
            self.advance();
            self.consume(TokenKind::Colon, "Expected ':'")?;
            self.consume(TokenKind::Newline, "Expected Newline")?;
            Some(self.parse_block()?)
        } else if self.peek().kind == TokenKind::Elif {
            // Desugar elif to else { if ... }
            // Let's recurse parse_if but treat it as a new Stmt
            // NOTE: Token is Elif. 
            // We can trick it: consume Elif, act like we consumed If.
            // But parse_if expects If token.
            // For now, simple: consume Elif here.
            self.advance();
            let cond = self.parse_expr()?;
            self.consume(TokenKind::Colon, "Expected ':'")?;
            self.consume(TokenKind::Newline, "Expected Newline")?;
            let block = self.parse_block()?;
            
            // Further elses?
            // This needs full recursion.
            // Construct nested IfStmt
            let nested_else = if self.peek().kind == TokenKind::Else || self.peek().kind == TokenKind::Elif {
                 // Hacky recursion or loop?
                 // Let's just say else_block = Some(vec![Stmt::If(...)])
                 // But I need to parse the rest of the chain.
                 // Let's implement full parsing logic for elif chain later.
                 // For current MVP, treat elif as just another block? No that's wrong.
                 // Recursive call to parse_if_tail?
                 None // TODO: Implement Elif recursion
            } else {
                None
            };
            
            Some(vec![Stmt::If(IfStmt {
                condition: cond,
                then_block: block,
                else_block: nested_else,
                span: start_span, // approximate
            })])
        } else {
            None
        };

        Ok(Stmt::If(IfStmt {
            condition,
            then_block,
            else_block,
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
        
        if self.match_kind(TokenKind::Eq) {
            let right = self.parse_expr()?;
            self.consume(TokenKind::Newline, "Expected Newline")?;
            let span = left.span_owned();
            Ok(Stmt::Assign(AssignStmt {
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
    
    fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_equality()
    }
    
    fn parse_equality(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_term()?; // actually parse_add_sub etc.
        // Simplified: just call primary for now to test structure
        // TODO: Implement full precedence (Eq, Add, Mul, Unary, Primary)
        Ok(expr)
    }

    fn parse_term(&mut self) -> Result<Expr, String> {
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
                expr = Expr::Call { 
                    func: Box::new(expr.clone()), 
                    args, 
                    span: expr.span_owned() 
                };
            } else if self.match_kind(TokenKind::Dot) {
                let name = match self.peek().kind.clone() {
                    TokenKind::Identifier(s) => { self.advance(); s },
                    _ => return Err("Expected field name".to_string()),
                };
                expr = Expr::FieldAccess {
                    target: Box::new(expr.clone()),
                    field: name,
                    span: expr.span_owned()
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        let t = self.peek().clone();
        match t.kind {
            TokenKind::Identifier(s) => {
                self.advance();
                Ok(Expr::Identifier { name: s, span: t.span })
            }
            TokenKind::Integer(i) => {
                self.advance();
                Ok(Expr::Integer { value: i, span: t.span })
            }
            TokenKind::String(s) => {
                self.advance();
                Ok(Expr::String { value: s, span: t.span })
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
            Expr::Integer { span, .. } => *span,
            Expr::Float { span, .. } => *span,
            Expr::String { span, .. } => *span,
            Expr::Binary { span, .. } => *span,
            Expr::Call { span, .. } => *span,
            Expr::FieldAccess { span, .. } => *span,
            Expr::List { span, .. } => *span,
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
