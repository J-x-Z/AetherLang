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
        
        let span = func.span().merge(&self.tokens[self.pos.saturating_sub(1)].span);
        Ok(Expr::Call {
            func: Box::new(func),
            args,
            span,
        })
    }
