#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Structure
    Indent,
    Dedent,
    Newline,
    Eof,

    // Keywords
    Def,
    Return,
    If,
    Else,
    Elif,
    While,
    For,
    In,
    Class,
    Import,
    From,
    As,
    Pass,
    Break,
    Continue,
    
    // Metaprogramming
    Comptime, // @comptime

    // Literals
    Identifier(String),
    Integer(i64),
    Float(f64),
    String(String),

    // Operators
    Plus,       // +
    Minus,      // -
    Star,       // *
    Slash,      // /
    Eq,         // =
    EqEq,       // ==
    NotEq,      // !=
    Lt,         // <
    Gt,         // >
    LtEq,       // <=
    GtEq,       // >=
    Arrow,      // ->
    Colon,      // :
    Comma,      // ,
    Dot,        // .
    LParen,     // (
    RParen,     // )
    LBracket,   // [
    RBracket,   // ]
    LBrace,     // {
    RBrace,     // }
    
    // Comments are usually skipped, but explicit comment token might be useful for some tools?
    // For now, skip comments.
    
    Unknown(char),
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
}

impl Span {
    pub fn new(start: usize, end: usize, line: usize, column: usize) -> Self {
        Self { start, end, line, column }
    }
}
