use super::token::{Token, TokenKind, Span};

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
    indent_stack: Vec<usize>,
    pending_dedents: usize, // Number of DEDENT tokens waiting to be emitted
    at_line_start: bool,    // True if we are at the start of a line (before any non-whitespace)
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
            indent_stack: vec![0], // Initial indentation level is 0
            pending_dedents: 0,
            at_line_start: true,
        }
    }

    fn current_char(&self) -> Option<char> {
        if self.pos < self.input.len() {
            Some(self.input[self.pos])
        } else {
            None
        }
    }

    fn advance(&mut self) -> Option<char> {
        if self.pos < self.input.len() {
            let c = self.input[self.pos];
            self.pos += 1;
            if c == '\n' {
                self.line += 1;
                self.col = 1;
                self.at_line_start = true;
            } else {
                self.col += 1;
            }
            Some(c)
        } else {
            None
        }
    }

    fn peek(&self) -> Option<char> {
        self.current_char()
    }
    
    fn peek_next(&self) -> Option<char> {
        if self.pos + 1 < self.input.len() {
            Some(self.input[self.pos + 1])
        } else {
            None
        }
    }

    /// Calculate indentation level of the current line.
    /// Returns indentation width (spaces). Assumes spaces for now. Todo: Handle tabs.
    fn calculate_indentation(&mut self) -> usize {
        let mut indent = 0;
        let mut p = self.pos;
        
        while p < self.input.len() {
            match self.input[p] {
                ' ' => indent += 1,
                '\t' => indent += 4, // Assume 4 spaces for tab
                _ => break,
            }
            p += 1;
        }
        indent
    }
    
    fn skip_whitespace_and_comments(&mut self) {
        while let Some(c) = self.peek() {
            match c {
                ' ' | '\t' | '\r' => {
                    self.advance();
                }
                '#' => {
                    // Comment until end of line
                    while let Some(next) = self.peek() {
                        if next == '\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                _ => break,
            }
        }
    }

    pub fn next_token(&mut self) -> Token {
        // Debug
        // println!("next_token: start={:?}, pending={}, stack={:?}", self.at_line_start, self.pending_dedents, self.indent_stack);

        // 1. Handle pending DEDENTS
        if self.pending_dedents > 0 {
            self.pending_dedents -= 1;
            return self.make_token(TokenKind::Dedent);
        }

        // 2. Handle Indentation at line start
        // 2. Handle Indentation at line start
        if self.at_line_start {
            loop {
                let _start_indent_pos = self.pos;
                let mut current_indent = 0;
                while let Some(c) = self.peek() {
                    match c {
                        ' ' => { current_indent += 1; self.advance(); }
                        '\t' => { current_indent += 4; self.advance(); }
                        _ => break,
                    }
                }
                
                match self.peek() {
                    Some('\n') => {
                        self.advance();
                        self.at_line_start = true;
                        continue; // Empty line, retry next line
                    }
                    Some('#') => {
                         while let Some(c) = self.peek() {
                            if c == '\n' { break; }
                            self.advance();
                         }
                         if let Some('\n') = self.peek() {
                             self.advance();
                             self.at_line_start = true;
                             continue;
                         }
                         // EOF after comment
                    }
                    None => break, // EOF, let the main logic handle dedents
                    Some(_) => {
                        // Found code! current_indent is valid.
                        self.at_line_start = false; // We have processed indentation for this line
                        
                        let last_indent = *self.indent_stack.last().unwrap();
                        
                        if current_indent > last_indent {
                            self.indent_stack.push(current_indent);
                            return self.make_token(TokenKind::Indent);
                        } else if current_indent < last_indent {
                            while self.indent_stack.len() > 1 && current_indent < *self.indent_stack.last().unwrap() {
                                self.indent_stack.pop();
                                self.pending_dedents += 1;
                            }
                            
                            if current_indent != *self.indent_stack.last().unwrap() {
                                // Indentation error!
                                // For now, treat as just dedenting to nearest.
                            }
                            
                            if self.pending_dedents > 0 {
                                self.pending_dedents -= 1;
                                return self.make_token(TokenKind::Dedent);
                            }
                        }
                        // If equal, just proceed to tokenize the code
                        break;
                    }
                }
            }
        }
        
        self.skip_whitespace_and_comments();

        match self.peek() {
            Some('\n') => {
                self.advance();
                self.at_line_start = true;
                self.make_token(TokenKind::Newline)
            }
            Some(c) if c.is_alphabetic() || c == '_' || c == '@' => self.identifier_or_keyword(),
            Some(c) if c.is_digit(10) => self.number(),
            Some('"') => self.string(),
            Some('+') => { self.advance(); self.make_token(TokenKind::Plus) }
            Some('-') => {
                self.advance();
                if self.peek() == Some('>') {
                    self.advance();
                    self.make_token(TokenKind::Arrow)
                } else {
                    self.make_token(TokenKind::Minus)
                }
            }
            Some('*') => { self.advance(); self.make_token(TokenKind::Star) }
            Some('/') => { self.advance(); self.make_token(TokenKind::Slash) }
            Some(':') => { self.advance(); self.make_token(TokenKind::Colon) }
            Some(',') => { self.advance(); self.make_token(TokenKind::Comma) }
            Some('.') => { self.advance(); self.make_token(TokenKind::Dot) }
            Some('(') => { self.advance(); self.make_token(TokenKind::LParen) }
            Some(')') => { self.advance(); self.make_token(TokenKind::RParen) }
            Some('[') => { self.advance(); self.make_token(TokenKind::LBracket) }
            Some(']') => { self.advance(); self.make_token(TokenKind::RBracket) }
            Some('{') => { self.advance(); self.make_token(TokenKind::LBrace) }
            Some('}') => { self.advance(); self.make_token(TokenKind::RBrace) }
            Some('=') => {
                self.advance();
                if self.peek() == Some('=') {
                     self.advance();
                     self.make_token(TokenKind::EqEq)
                } else {
                    self.make_token(TokenKind::Eq)
                }
            }
                    None => {
                        // println!("EOF hit. Stack: {:?}", self.indent_stack);
                         // EOF. Must emit residual DEDENTs if stack > 0 (actually > 1, 0 is base)
                         if self.indent_stack.len() > 1 {
                             self.indent_stack.pop();
                             self.make_token(TokenKind::Dedent)
                         } else {
                             self.make_token(TokenKind::Eof)
                         }
                    }
            Some(c) => {
                self.advance();
                self.make_token(TokenKind::Unknown(c))
            }
        }
    }

    fn identifier_or_keyword(&mut self) -> Token {
        let start = self.pos;
        let mut text = String::new();
        
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' || c == '@' {
                text.push(c);
                self.advance();
            } else {
                break;
            }
        }

        let kind = match text.as_str() {
            "def" => TokenKind::Def,
            "return" => TokenKind::Return,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "elif" => TokenKind::Elif,
            "while" => TokenKind::While,
            "for" => TokenKind::For,
            "in" => TokenKind::In,
            "class" => TokenKind::Class,
            "import" => TokenKind::Import,
            "from" => TokenKind::From,
            "as" => TokenKind::As,
            "pass" => TokenKind::Pass,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "@comptime" => TokenKind::Comptime,
            _ => TokenKind::Identifier(text),
        };

        let span = Span::new(start, self.pos, self.line, self.col - (self.pos - start));
        Token { kind, span }
    }

    fn number(&mut self) -> Token {
        let start = self.pos;
        let mut text = String::new();
        let mut is_float = false;

        while let Some(c) = self.peek() {
            if c.is_digit(10) {
                text.push(c);
                self.advance();
            } else if c == '.' && !is_float {
                is_float = true;
                text.push(c);
                self.advance();
            } else {
                break;
            }
        }

        let kind = if is_float {
            TokenKind::Float(text.parse().unwrap_or(0.0))
        } else {
            TokenKind::Integer(text.parse().unwrap_or(0))
        };

        let span = Span::new(start, self.pos, self.line, self.col - (self.pos - start));
        Token { kind, span }
    }

    fn string(&mut self) -> Token {
        let start = self.pos;
        self.advance(); // consume opening quote
        let mut text = String::new();
        
        while let Some(c) = self.peek() {
            if c == '"' {
                self.advance(); // consume closing
                break;
            } else {
                text.push(c);
                self.advance();
            }
        }

        let span = Span::new(start, self.pos, self.line, self.col - (self.pos - start));
        Token { kind: TokenKind::String(text), span }
    }

    fn make_token(&self, kind: TokenKind) -> Token {
        // Simplified span for single-char or zero-width tokens for now
        // Real logic should track start position more carefully
        Span::new(self.pos, self.pos, self.line, self.col);
        Token { kind, span: Span::new(self.pos, self.pos, self.line, self.col) } 
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_indentation() {
        let input = "
def main():
    print(\"Hello\")
    if x:
        pass
    return 0
";
        let mut lexer = Lexer::new(input);
        
        // Helper to debug
        let mut tokens = Vec::new();
        loop {
            let t = lexer.next_token();
            println!("Token: {:?}", t.kind);
            tokens.push(t.kind.clone());
            if matches!(tokens.last().unwrap(), TokenKind::Eof) { break; }
        }
        
        let mut iter = tokens.into_iter();
        
        macro_rules! check {
            ($p:pat) => {
                let t = iter.next().unwrap();
                assert!(matches!(t, $p), "Expected {}, got {:?}", stringify!($p), t);
            }
        }
        
        // Skip initial newlines? No, my logic consumes them strictly if empty lines.
        // My test string starts with empty line `\n`.
        // Logic: Empty line -> consumed -> loop -> Def.
        // So first token IS Def.
        
        check!(TokenKind::Def);
        check!(TokenKind::Identifier(_)); // main
        check!(TokenKind::LParen);
        check!(TokenKind::RParen);
        check!(TokenKind::Colon);
        check!(TokenKind::Newline);
        
        check!(TokenKind::Indent);
        check!(TokenKind::Identifier(_)); // print
        check!(TokenKind::LParen);
        check!(TokenKind::String(_));
        check!(TokenKind::RParen);
        check!(TokenKind::Newline);
        
        check!(TokenKind::If);
        check!(TokenKind::Identifier(_));
        check!(TokenKind::Colon);
        check!(TokenKind::Newline);
        
        check!(TokenKind::Indent);
        check!(TokenKind::Pass);
        check!(TokenKind::Newline);
        
        check!(TokenKind::Dedent);
        check!(TokenKind::Return);
        check!(TokenKind::Integer(0));
        check!(TokenKind::Newline);
        
        check!(TokenKind::Dedent);
        check!(TokenKind::Eof);
    }
}

