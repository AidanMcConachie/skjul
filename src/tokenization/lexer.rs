use super::token::{Token, TokenType};

struct Lexer {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    column: usize,
}

impl Lexer {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(c)
    }

    fn skip_whitespace_and_comments(&mut self) -> Result<(), String> {
        loop {
            match (self.peek(), self.peek_next()) {
                (Some(c), _) if c.is_whitespace() => {
                    self.advance();
                }
                (Some('/'), Some('/')) => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                (Some('/'), Some('*')) => {
                    let (line, column) = (self.line, self.column);
                    self.advance();
                    self.advance();
                    loop {
                        match self.advance() {
                            Some('*') if self.peek() == Some('/') => {
                                self.advance();
                                break;
                            }
                            Some(_) => {}
                            None => {
                                return Err(format!("{line}:{column}: unterminated block comment"));
                            }
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn read_word(&mut self) -> TokenType {
        let mut word = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '_' {
                word.push(c);
                self.advance();
            } else {
                break;
            }
        }

        match word.as_str() {
            "int" => TokenType::INT,
            "return" => TokenType::RETURN,
            _ => TokenType::IDENTIFIER(word),
        }
    }

    /// Read a preprocessor directive, from the `#` at the cursor to the end
    /// of the line. A backslash-newline continues the directive onto the next
    /// line, matching C's line-splicing, so multi-line macros survive intact.
    fn read_preprocessor(&mut self) -> TokenType {
        let mut line = String::new();
        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            if c == '\\' && self.peek_next() == Some('\n') {
                line.push(self.advance().unwrap()); // backslash
                line.push(self.advance().unwrap()); // newline
                continue;
            }
            line.push(self.advance().unwrap());
        }
        TokenType::PREPROCESSOR(line)
    }

    fn read_number(&mut self) -> Result<TokenType, String> {
        let (line, column) = (self.line, self.column);
        let mut digits = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                digits.push(c);
                self.advance();
            } else {
                break;
            }
        }

        digits
            .parse::<i64>()
            .map(TokenType::INTEGER)
            .map_err(|e| format!("{line}:{column}: invalid integer '{digits}': {e}"))
    }

    /// Read a `"..."` string literal. The opening quote is at the cursor.
    /// The content is stored exactly as written (escapes kept verbatim) so
    /// re-emitting it reproduces the source literal character for character.
    fn read_string(&mut self) -> Result<TokenType, String> {
        let (line, column) = (self.line, self.column);
        self.advance(); // opening quote
        let mut content = String::new();
        loop {
            match self.advance() {
                Some('"') => return Ok(TokenType::STRING(content)),
                Some('\\') => {
                    content.push('\\');
                    match self.advance() {
                        Some(escaped) => content.push(escaped),
                        None => return Err(format!("{line}:{column}: unterminated string literal")),
                    }
                }
                Some(c) => content.push(c),
                None => return Err(format!("{line}:{column}: unterminated string literal")),
            }
        }
    }

    /// Read a `'...'` character literal, following the same verbatim-content
    /// rule as [`read_string`](Self::read_string).
    fn read_character(&mut self) -> Result<TokenType, String> {
        let (line, column) = (self.line, self.column);
        self.advance(); // opening quote
        let mut content = String::new();
        loop {
            match self.advance() {
                Some('\'') => return Ok(TokenType::CHARACTER(content)),
                Some('\\') => {
                    content.push('\\');
                    match self.advance() {
                        Some(escaped) => content.push(escaped),
                        None => return Err(format!("{line}:{column}: unterminated character literal")),
                    }
                }
                Some(c) => content.push(c),
                None => return Err(format!("{line}:{column}: unterminated character literal")),
            }
        }
    }

    fn read_symbol(&mut self) -> Result<TokenType, String> {
        let (line, column) = (self.line, self.column);
        let Some(c) = self.advance() else {
            return Err(format!("{line}:{column}: unexpected end of input"));
        };

        // Longest-match first: consume a trailing character when it forms a
        // two-character operator, otherwise fall back to the single one.
        let eat = |lexer: &mut Self, expected: char| {
            if lexer.peek() == Some(expected) {
                lexer.advance();
                true
            } else {
                false
            }
        };

        let kind = match c {
            '+' if eat(self, '+') => TokenType::PLUSPLUS,
            '+' if eat(self, '=') => TokenType::PLUSEQUALS,
            '+' => TokenType::PLUS,
            '-' if eat(self, '-') => TokenType::MINUSMINUS,
            '-' if eat(self, '=') => TokenType::MINUSEQUALS,
            '-' if eat(self, '>') => TokenType::ARROW,
            '-' => TokenType::MINUS,
            '*' if eat(self, '=') => TokenType::STAREQUALS,
            '*' => TokenType::STAR,
            '/' if eat(self, '=') => TokenType::SLASHEQUALS,
            '/' => TokenType::SLASH,
            '%' if eat(self, '=') => TokenType::PERCENTEQUALS,
            '%' => TokenType::PERCENT,
            '=' if eat(self, '=') => TokenType::EQUALSEQUALS,
            '=' => TokenType::EQUALS,
            '!' if eat(self, '=') => TokenType::BANGEQUALS,
            '!' => TokenType::BANG,
            '<' if eat(self, '=') => TokenType::LESSEQUALS,
            '<' if eat(self, '<') => TokenType::LEFTSHIFT,
            '<' => TokenType::LESS,
            '>' if eat(self, '=') => TokenType::GREATEREQUALS,
            '>' if eat(self, '>') => TokenType::RIGHTSHIFT,
            '>' => TokenType::GREATER,
            '&' if eat(self, '&') => TokenType::AMPAMP,
            '&' => TokenType::AMP,
            '|' if eat(self, '|') => TokenType::PIPEPIPE,
            '|' => TokenType::PIPE,
            '^' => TokenType::CARET,
            '~' => TokenType::TILDE,
            '?' => TokenType::QUESTION,
            ':' => TokenType::COLON,
            '.' => TokenType::DOT,
            ',' => TokenType::COMMA,
            ';' => TokenType::SEMICOLON,
            '(' => TokenType::LEFTPAREN,
            ')' => TokenType::RIGHTPAREN,
            '{' => TokenType::LEFTBRACE,
            '}' => TokenType::RIGHTBRACE,
            '[' => TokenType::LEFTBRACKET,
            ']' => TokenType::RIGHTBRACKET,
            other => return Err(format!("{line}:{column}: unexpected character '{other}'")),
        };

        Ok(kind)
    }
}

pub fn tokenize(source: &str) -> Result<Vec<Token>, String> {
    let mut lexer = Lexer {
        chars: source.chars().collect(),
        pos: 0,
        line: 1,
        column: 1,
    };
    let mut tokens = Vec::new();

    loop {
        lexer.skip_whitespace_and_comments()?;

        let (line, column) = (lexer.line, lexer.column);
        let Some(c) = lexer.peek() else {
            break;
        };

        let kind = if c == '#' {
            lexer.read_preprocessor()
        } else if c == '"' {
            lexer.read_string()?
        } else if c == '\'' {
            lexer.read_character()?
        } else if c.is_ascii_alphabetic() || c == '_' {
            lexer.read_word()
        } else if c.is_ascii_digit() {
            lexer.read_number()?
        } else {
            lexer.read_symbol()?
        };

        tokens.push(Token { kind, line, column });
    }

    tokens.push(Token {
        kind: TokenType::EOF,
        line: lexer.line,
        column: lexer.column,
    });

    Ok(tokens)
}
