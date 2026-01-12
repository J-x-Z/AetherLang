//! Token definitions for AetherLang

use crate::utils::Span;
/// A token produced by the lexer
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
    
    pub fn eof(span: Span) -> Self {
        Self { kind: TokenKind::Eof, span }
    }
}

/// Token kinds
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // ============ Keywords ============
    /// fn
    Fn,
    /// let
    Let,
    /// mut
    Mut,
    /// if
    If,
    /// else
    Else,
    /// loop
    Loop,
    /// while
    While,
    /// for
    For,
    /// in
    In,
    /// return
    Return,
    /// match
    Match,
    /// struct
    Struct,
    /// impl
    Impl,
    /// enum
    Enum,
    /// interface
    Interface,
    /// own
    Own,
    /// ref
    Ref,
    /// const
    Const,
    /// unsafe
    Unsafe,
    /// break
    Break,
    /// continue
    Continue,
    /// true
    True,
    /// false
    False,
    /// asm
    Asm,
    
    // ============ Identifiers and Literals ============
    /// Identifier (variable name, function name, etc.)
    Ident(String),
    /// Integer literal
    IntLit(i64),
    /// Floating-point literal
    FloatLit(f64),
    /// String literal
    StringLit(String),
    /// Character literal
    CharLit(char),
    
    // ============ Operators ============
    /// +
    Plus,
    /// -
    Minus,
    /// *
    Star,
    /// /
    Slash,
    /// %
    Percent,
    /// =
    Eq,
    /// ==
    EqEq,
    /// !=
    Ne,
    /// <
    Lt,
    /// <=
    Le,
    /// >
    Gt,
    /// >=
    Ge,
    /// &&
    AndAnd,
    /// ||
    OrOr,
    /// !
    Not,
    /// &
    And,
    /// |
    Or,
    /// ^
    Caret,
    /// <<
    Shl,
    /// >>
    Shr,
    /// +=
    PlusEq,
    /// -=
    MinusEq,
    /// *=
    StarEq,
    /// /=
    SlashEq,
    /// ->
    Arrow,
    /// =>
    FatArrow,
    /// ..
    DotDot,
    /// ::
    ColonColon,
    
    // ============ Delimiters ============
    /// (
    LParen,
    /// )
    RParen,
    /// {
    LBrace,
    /// }
    RBrace,
    /// [
    LBracket,
    /// ]
    RBracket,
    /// ,
    Comma,
    /// .
    Dot,
    /// :
    Colon,
    /// ;
    Semicolon,
    
    // ============ Special ============
    /// End of file
    Eof,
    /// Unknown/invalid character
    Unknown(char),
}

impl TokenKind {
    /// Check if this token is a keyword
    pub fn is_keyword(&self) -> bool {
        matches!(
            self,
            TokenKind::Fn
                | TokenKind::Let
                | TokenKind::Mut
                | TokenKind::If
                | TokenKind::Else
                | TokenKind::Loop
                | TokenKind::While
                | TokenKind::For
                | TokenKind::In
                | TokenKind::Return
                | TokenKind::Match
                | TokenKind::Struct
                | TokenKind::Impl
                | TokenKind::Enum
                | TokenKind::Interface
                | TokenKind::Own
                | TokenKind::Ref
                | TokenKind::Const
                | TokenKind::Unsafe
                | TokenKind::Break
                | TokenKind::Continue
                | TokenKind::True
                | TokenKind::False
                | TokenKind::Asm
        )
    }

    /// Try to convert an identifier to a keyword
    pub fn keyword_from_str(s: &str) -> Option<TokenKind> {
        match s {
            "fn" => Some(TokenKind::Fn),
            "let" => Some(TokenKind::Let),
            "mut" => Some(TokenKind::Mut),
            "if" => Some(TokenKind::If),
            "else" => Some(TokenKind::Else),
            "loop" => Some(TokenKind::Loop),
            "while" => Some(TokenKind::While),
            "for" => Some(TokenKind::For),
            "in" => Some(TokenKind::In),
            "return" => Some(TokenKind::Return),
            "match" => Some(TokenKind::Match),
            "struct" => Some(TokenKind::Struct),
            "impl" => Some(TokenKind::Impl),
            "enum" => Some(TokenKind::Enum),
            "interface" => Some(TokenKind::Interface),
            "own" => Some(TokenKind::Own),
            "ref" => Some(TokenKind::Ref),
            "const" => Some(TokenKind::Const),
            "unsafe" => Some(TokenKind::Unsafe),
            "break" => Some(TokenKind::Break),
            "continue" => Some(TokenKind::Continue),
            "true" => Some(TokenKind::True),
            "false" => Some(TokenKind::False),
            "asm" => Some(TokenKind::Asm),
            _ => None,
        }
    }
    
    /// Get the precedence of a binary operator (for Pratt parsing)
    /// Returns None if not a binary operator
    pub fn binary_precedence(&self) -> Option<u8> {
        match self {
            // Assignment (lowest)
            TokenKind::Eq | TokenKind::PlusEq | TokenKind::MinusEq 
                | TokenKind::StarEq | TokenKind::SlashEq => Some(1),
            
            // Logical OR
            TokenKind::OrOr => Some(2),
            
            // Logical AND
            TokenKind::AndAnd => Some(3),
            
            // Bitwise OR
            TokenKind::Or => Some(4),
            
            // Bitwise XOR
            TokenKind::Caret => Some(5),
            
            // Bitwise AND
            TokenKind::And => Some(6),
            
            // Equality
            TokenKind::EqEq | TokenKind::Ne => Some(7),
            
            // Comparison
            TokenKind::Lt | TokenKind::Le | TokenKind::Gt | TokenKind::Ge => Some(8),
            
            // Shift
            TokenKind::Shl | TokenKind::Shr => Some(9),
            
            // Additive
            TokenKind::Plus | TokenKind::Minus => Some(10),
            
            // Multiplicative (highest for binary)
            TokenKind::Star | TokenKind::Slash | TokenKind::Percent => Some(11),
            
            _ => None,
        }
    }
}
