//! Lexer for AetherLang
//! 
//! Converts source code into a stream of tokens.
#![allow(dead_code)]

use crate::frontend::token::{Token, TokenKind};
use crate::utils::Span;

/// The lexer state
pub struct Lexer {
    /// Source code as bytes
    source: Vec<char>,
    /// Current position in source
    pos: usize,
    /// Start position of current token
    start: usize,
    /// File ID for span tracking
    file_id: usize,
}

impl Lexer {
    /// Create a new lexer for the given source code
    pub fn new(source: &str, file_id: usize) -> Self {
        Self {
            source: source.chars().collect(),
            pos: 0,
            start: 0,
            file_id,
        }
    }
    
    /// Get the current character without advancing
    fn peek(&self) -> Option<char> {
        self.source.get(self.pos).copied()
    }
    
    /// Get the next character without advancing
    fn peek_next(&self) -> Option<char> {
        self.source.get(self.pos + 1).copied()
    }
    
    /// Advance to the next character
    fn advance(&mut self) -> Option<char> {
        let c = self.peek();
        self.pos += 1;
        c
    }
    
    /// Check if we've reached the end of input
    fn is_at_end(&self) -> bool {
        self.pos >= self.source.len()
    }
    
    /// Create a span from start to current position
    fn make_span(&self) -> Span {
        Span::new(self.start, self.pos, self.file_id)
    }
    
    /// Create a token with the current span
    fn make_token(&self, kind: TokenKind) -> Token {
        Token::new(kind, self.make_span())
    }
    
    /// Skip whitespace and comments
    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            match c {
                // Whitespace
                ' ' | '\t' | '\r' | '\n' => {
                    self.advance();
                }
                // Line comment
                '/' if self.peek_next() == Some('/') => {
                    // Skip until end of line
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                // Block comment
                '/' if self.peek_next() == Some('*') => {
                    self.advance(); // skip /
                    self.advance(); // skip *
                    let mut depth = 1;
                    while depth > 0 && !self.is_at_end() {
                        match (self.peek(), self.peek_next()) {
                            (Some('*'), Some('/')) => {
                                self.advance();
                                self.advance();
                                depth -= 1;
                            }
                            (Some('/'), Some('*')) => {
                                self.advance();
                                self.advance();
                                depth += 1;
                            }
                            _ => {
                                self.advance();
                            }
                        }
                    }
                }
                _ => break,
            }
        }
    }
    
    /// Read an identifier or keyword
    fn read_identifier(&mut self) -> Token {
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                self.advance();
            } else {
                break;
            }
        }
        
        let text: String = self.source[self.start..self.pos].iter().collect();
        
        // Check if it's a keyword
        let kind = TokenKind::keyword_from_str(&text)
            .unwrap_or_else(|| TokenKind::Ident(text));
        
        self.make_token(kind)
    }
    
    /// Read a number literal (integer or float)
    fn read_number(&mut self) -> Token {
        // Check for hex literal
        if self.peek() == Some('0') && matches!(self.peek_next(), Some('x') | Some('X')) {
            self.advance(); // 0
            self.advance(); // x
            
            while let Some(c) = self.peek() {
                if c.is_ascii_hexdigit() || c == '_' {
                    self.advance();
                } else {
                    break;
                }
            }
            
            let text: String = self.source[self.start..self.pos]
                .iter()
                .filter(|&&c| c != '_')
                .collect();
            
            let value = i64::from_str_radix(&text[2..], 16).unwrap_or(0);
            return self.make_token(TokenKind::IntLit(value));
        }
        
        // Regular decimal number
        let mut is_float = false;
        
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '_' {
                self.advance();
            } else {
                break;
            }
        }
        
        // Check for decimal point
        if self.peek() == Some('.') && self.peek_next().map_or(false, |c| c.is_ascii_digit()) {
            is_float = true;
            self.advance(); // consume '.'
            
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() || c == '_' {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        
        // Check for exponent
        if matches!(self.peek(), Some('e') | Some('E')) {
            is_float = true;
            self.advance();
            
            if matches!(self.peek(), Some('+') | Some('-')) {
                self.advance();
            }
            
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        
        let text: String = self.source[self.start..self.pos]
            .iter()
            .filter(|&&c| c != '_')
            .collect();
        
        if is_float {
            let value = text.parse().unwrap_or(0.0);
            self.make_token(TokenKind::FloatLit(value))
        } else {
            let value = text.parse().unwrap_or(0);
            self.make_token(TokenKind::IntLit(value))
        }
    }
    
    /// Read a string literal
    fn read_string(&mut self) -> Token {
        self.advance(); // consume opening quote
        
        let mut value = String::new();
        
        while let Some(c) = self.peek() {
            if c == '"' {
                self.advance(); // consume closing quote
                break;
            } else if c == '\\' {
                self.advance();
                match self.peek() {
                    Some('n') => { value.push('\n'); self.advance(); }
                    Some('r') => { value.push('\r'); self.advance(); }
                    Some('t') => { value.push('\t'); self.advance(); }
                    Some('\\') => { value.push('\\'); self.advance(); }
                    Some('"') => { value.push('"'); self.advance(); }
                    Some('0') => { value.push('\0'); self.advance(); }
                    Some(c) => { value.push(c); self.advance(); }
                    None => break,
                }
            } else if c == '\n' {
                // Unterminated string
                break;
            } else {
                value.push(c);
                self.advance();
            }
        }
        
        self.make_token(TokenKind::StringLit(value))
    }
    
    /// Read a character literal or lifetime parameter
    fn read_char(&mut self) -> Token {
        self.advance(); // consume opening quote
        
        // Check if this could be a lifetime: 'a followed by non-quote
        // vs char literal: 'a' with closing quote
        let first_char = self.peek();
        
        // If it's alphabetic and followed by something other than quote, it's a lifetime
        if let Some(c) = first_char {
            if c.is_alphabetic() || c == '_' {
                // Collect the lifetime name
                let mut name = String::new();
                while let Some(ch) = self.peek() {
                    if ch.is_alphanumeric() || ch == '_' {
                        name.push(ch);
                        self.advance();
                    } else {
                        break;
                    }
                }
                
                // Check if there's a closing quote (char literal) or not (lifetime)
                if self.peek() == Some('\'') && name.len() == 1 {
                    // It's a character literal like 'a'
                    self.advance(); // consume closing quote
                    return self.make_token(TokenKind::CharLit(name.chars().next().unwrap()));
                } else {
                    // It's a lifetime like 'a or 'static
                    return self.make_token(TokenKind::Lifetime(name));
                }
            }
        }
        
        // Handle escape sequences for char literals
        let c = if self.peek() == Some('\\') {
            self.advance();
            match self.peek() {
                Some('n') => { self.advance(); '\n' }
                Some('r') => { self.advance(); '\r' }
                Some('t') => { self.advance(); '\t' }
                Some('\\') => { self.advance(); '\\' }
                Some('\'') => { self.advance(); '\'' }
                Some('0') => { self.advance(); '\0' }
                Some(c) => { self.advance(); c }
                None => '\0',
            }
        } else {
            self.advance().unwrap_or('\0')
        };
        
        // Consume closing quote
        if self.peek() == Some('\'') {
            self.advance();
        }
        
        self.make_token(TokenKind::CharLit(c))
    }
    
    /// Get the next token
    pub fn next_token(&mut self) -> Token {
        self.skip_whitespace();
        self.start = self.pos;
        
        if self.is_at_end() {
            return Token::eof(self.make_span());
        }
        
        let c = self.advance().unwrap();
        
        // Identifiers and keywords
        if c.is_alphabetic() || c == '_' {
            self.pos -= 1; // back up
            return self.read_identifier();
        }
        
        // Numbers
        if c.is_ascii_digit() {
            self.pos -= 1; // back up
            return self.read_number();
        }
        
        // String literals
        if c == '"' {
            self.pos -= 1; // back up
            return self.read_string();
        }
        
        // Character literals
        if c == '\'' {
            self.pos -= 1; // back up
            return self.read_char();
        }
        
        // Operators and punctuation
        let kind = match c {
            '+' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::PlusEq
                } else {
                    TokenKind::Plus
                }
            }
            '-' => {
                if self.peek() == Some('>') {
                    self.advance();
                    TokenKind::Arrow
                } else if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::MinusEq
                } else {
                    TokenKind::Minus
                }
            }
            '*' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::StarEq
                } else {
                    TokenKind::Star
                }
            }
            '/' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::SlashEq
                } else {
                    TokenKind::Slash
                }
            }
            '%' => TokenKind::Percent,
            '=' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::EqEq
                } else if self.peek() == Some('>') {
                    self.advance();
                    TokenKind::FatArrow
                } else {
                    TokenKind::Eq
                }
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::Ne
                } else {
                    TokenKind::Not
                }
            }
            '<' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::Le
                } else if self.peek() == Some('<') {
                    self.advance();
                    TokenKind::Shl
                } else {
                    TokenKind::Lt
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::Ge
                } else if self.peek() == Some('>') {
                    self.advance();
                    TokenKind::Shr
                } else {
                    TokenKind::Gt
                }
            }
            '&' => {
                if self.peek() == Some('&') {
                    self.advance();
                    TokenKind::AndAnd
                } else {
                    TokenKind::And
                }
            }
            '|' => {
                if self.peek() == Some('|') {
                    self.advance();
                    TokenKind::OrOr
                } else {
                    TokenKind::Or
                }
            }
            '^' => TokenKind::Caret,
            '.' => {
                if self.peek() == Some('.') {
                    self.advance();
                    if self.peek() == Some('.') {
                        self.advance();
                        TokenKind::DotDotDot
                    } else {
                        TokenKind::DotDot
                    }
                } else {
                    TokenKind::Dot
                }
            }
            ':' => {
                if self.peek() == Some(':') {
                    self.advance();
                    TokenKind::ColonColon
                } else {
                    TokenKind::Colon
                }
            }
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            '{' => TokenKind::LBrace,
            '}' => TokenKind::RBrace,
            '[' => TokenKind::LBracket,
            ']' => TokenKind::RBracket,
            ',' => TokenKind::Comma,
            ';' => TokenKind::Semicolon,
            '@' => TokenKind::At,
            '#' => TokenKind::Hash,
            '?' => TokenKind::Question,
            '~' => TokenKind::Tilde,
            _ => TokenKind::Unknown(c),
        };
        
        self.make_token(kind)
    }
    
    /// Tokenize the entire source and return all tokens
    pub fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        loop {
            let token = self.next_token();
            let is_eof = token.kind == TokenKind::Eof;
            tokens.push(token);
            if is_eof {
                break;
            }
        }
        tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_simple_tokens() {
        let mut lexer = Lexer::new("fn main() { }", 0);
        let tokens = lexer.tokenize();
        
        assert!(matches!(tokens[0].kind, TokenKind::Fn));
        assert!(matches!(tokens[1].kind, TokenKind::Ident(ref s) if s == "main"));
        assert!(matches!(tokens[2].kind, TokenKind::LParen));
        assert!(matches!(tokens[3].kind, TokenKind::RParen));
        assert!(matches!(tokens[4].kind, TokenKind::LBrace));
        assert!(matches!(tokens[5].kind, TokenKind::RBrace));
        assert!(matches!(tokens[6].kind, TokenKind::Eof));
    }
    
    #[test]
    fn test_numbers() {
        let mut lexer = Lexer::new("42 3.14 0xFF_FF", 0);
        let tokens = lexer.tokenize();
        
        assert!(matches!(tokens[0].kind, TokenKind::IntLit(42)));
        assert!(matches!(tokens[1].kind, TokenKind::FloatLit(f) if (f - 3.14).abs() < 0.001));
        assert!(matches!(tokens[2].kind, TokenKind::IntLit(0xFFFF)));
    }
    
    #[test]
    fn test_strings() {
        let mut lexer = Lexer::new(r#""hello\nworld""#, 0);
        let tokens = lexer.tokenize();
        
        assert!(matches!(tokens[0].kind, TokenKind::StringLit(ref s) if s == "hello\nworld"));
    }
    
    #[test]
    fn test_keywords() {
        let mut lexer = Lexer::new("let mut own ref", 0);
        let tokens = lexer.tokenize();
        
        assert!(matches!(tokens[0].kind, TokenKind::Let));
        assert!(matches!(tokens[1].kind, TokenKind::Mut));
        assert!(matches!(tokens[2].kind, TokenKind::Own));
        assert!(matches!(tokens[3].kind, TokenKind::Ref));
    }
    
    #[test]
    fn test_ai_native_keywords() {
        let mut lexer = Lexer::new("requires ensures invariant effect pure shared type trait pub where", 0);
        let tokens = lexer.tokenize();
        
        assert!(matches!(tokens[0].kind, TokenKind::Requires));
        assert!(matches!(tokens[1].kind, TokenKind::Ensures));
        assert!(matches!(tokens[2].kind, TokenKind::Invariant));
        assert!(matches!(tokens[3].kind, TokenKind::Effect));
        assert!(matches!(tokens[4].kind, TokenKind::Pure));
        assert!(matches!(tokens[5].kind, TokenKind::Shared));
        assert!(matches!(tokens[6].kind, TokenKind::Type));
        assert!(matches!(tokens[7].kind, TokenKind::Trait));
        assert!(matches!(tokens[8].kind, TokenKind::Pub));
        assert!(matches!(tokens[9].kind, TokenKind::Where));
    }
    
    #[test]
    fn test_new_tokens() {
        let mut lexer = Lexer::new("@test ? ~x", 0);
        let tokens = lexer.tokenize();
        
        assert!(matches!(tokens[0].kind, TokenKind::At));
        assert!(matches!(tokens[1].kind, TokenKind::Ident(ref s) if s == "test"));
        assert!(matches!(tokens[2].kind, TokenKind::Question));
        assert!(matches!(tokens[3].kind, TokenKind::Tilde));
    }
}
