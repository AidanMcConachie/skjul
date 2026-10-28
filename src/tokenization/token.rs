#[derive(Debug, Clone, PartialEq)]
pub enum TokenType{
    INT,
    RETURN,
    IDENTIFIER(String),
    INTEGER(i64),
    STRING(String),
    CHARACTER(String),

    /// A preprocessor directive, captured verbatim from `#` to end of line
    /// (e.g. `#include <stdio.h>`). Stored whole and re-emitted untouched:
    /// we neither rename identifiers inside it nor strip it.
    PREPROCESSOR(String),

    /// Pre-rendered text produced by an obfuscation pass, emitted verbatim.
    /// Constant-folding passes replace a literal (an integer or character
    /// constant) with an equivalent expression whose text is kept here —
    /// always parenthesised, so it drops into any expression context without
    /// changing precedence and never fuses with a neighbouring token.
    RAW(String),

    // single-character operators and punctuation
    PLUS,
    MINUS,
    STAR,
    SLASH,
    PERCENT,
    EQUALS,
    BANG,
    LESS,
    GREATER,
    AMP,
    PIPE,
    CARET,
    TILDE,
    QUESTION,
    COLON,
    DOT,
    COMMA,
    SEMICOLON,

    // multi-character operators
    PLUSPLUS,
    MINUSMINUS,
    ARROW,
    PLUSEQUALS,
    MINUSEQUALS,
    STAREQUALS,
    SLASHEQUALS,
    PERCENTEQUALS,
    EQUALSEQUALS,
    BANGEQUALS,
    LESSEQUALS,
    GREATEREQUALS,
    AMPAMP,
    PIPEPIPE,
    LEFTSHIFT,
    RIGHTSHIFT,

    LEFTPAREN,
    RIGHTPAREN,
    LEFTBRACE,
    RIGHTBRACE,
    LEFTBRACKET,
    RIGHTBRACKET,

    EOF
}

impl TokenType {
    pub fn text(&self) -> String {
        match self {
            TokenType::INT => "int".to_string(),
            TokenType::RETURN => "return".to_string(),
            TokenType::IDENTIFIER(name) => name.clone(),
            TokenType::INTEGER(value) => value.to_string(),
            TokenType::STRING(content) => format!("\"{content}\""),
            TokenType::CHARACTER(content) => format!("'{content}'"),
            TokenType::PREPROCESSOR(line) => line.clone(),
            TokenType::RAW(text) => text.clone(),
            TokenType::PLUS => "+".to_string(),
            TokenType::MINUS => "-".to_string(),
            TokenType::STAR => "*".to_string(),
            TokenType::SLASH => "/".to_string(),
            TokenType::PERCENT => "%".to_string(),
            TokenType::EQUALS => "=".to_string(),
            TokenType::BANG => "!".to_string(),
            TokenType::LESS => "<".to_string(),
            TokenType::GREATER => ">".to_string(),
            TokenType::AMP => "&".to_string(),
            TokenType::PIPE => "|".to_string(),
            TokenType::CARET => "^".to_string(),
            TokenType::TILDE => "~".to_string(),
            TokenType::QUESTION => "?".to_string(),
            TokenType::COLON => ":".to_string(),
            TokenType::DOT => ".".to_string(),
            TokenType::COMMA => ",".to_string(),
            TokenType::SEMICOLON => ";".to_string(),
            TokenType::PLUSPLUS => "++".to_string(),
            TokenType::MINUSMINUS => "--".to_string(),
            TokenType::ARROW => "->".to_string(),
            TokenType::PLUSEQUALS => "+=".to_string(),
            TokenType::MINUSEQUALS => "-=".to_string(),
            TokenType::STAREQUALS => "*=".to_string(),
            TokenType::SLASHEQUALS => "/=".to_string(),
            TokenType::PERCENTEQUALS => "%=".to_string(),
            TokenType::EQUALSEQUALS => "==".to_string(),
            TokenType::BANGEQUALS => "!=".to_string(),
            TokenType::LESSEQUALS => "<=".to_string(),
            TokenType::GREATEREQUALS => ">=".to_string(),
            TokenType::AMPAMP => "&&".to_string(),
            TokenType::PIPEPIPE => "||".to_string(),
            TokenType::LEFTSHIFT => "<<".to_string(),
            TokenType::RIGHTSHIFT => ">>".to_string(),
            TokenType::LEFTPAREN => "(".to_string(),
            TokenType::RIGHTPAREN => ")".to_string(),
            TokenType::LEFTBRACE => "{".to_string(),
            TokenType::RIGHTBRACE => "}".to_string(),
            TokenType::LEFTBRACKET => "[".to_string(),
            TokenType::RIGHTBRACKET => "]".to_string(),
            TokenType::EOF => String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenType,
    pub line: usize,
    pub column: usize
}
