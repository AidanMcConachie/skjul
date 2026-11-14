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

    /// Append characters to `out` while `pred` holds, stopping at the first
    /// one that does not (or at end of input).
    fn take_while(&mut self, out: &mut String, pred: impl Fn(char) -> bool) {
        while let Some(c) = self.peek() {
            if pred(c) {
                out.push(c);
                self.advance();
            } else {
                break;
            }
        }
    }

    /// Consume a C integer suffix — any run of `u`/`l` in either case, such as
    /// `U`, `ll`, `UL` — appending it to `out`. Returns whether one was found.
    /// The run is taken permissively: an invalid combination is left for the C
    /// compiler to reject, the same as it would in the original source.
    fn take_int_suffix(&mut self, out: &mut String) -> bool {
        let start = out.len();
        while let Some(c @ ('u' | 'U' | 'l' | 'L')) = self.peek() {
            out.push(c);
            self.advance();
        }
        out.len() != start
    }

    /// Read a numeric literal. A plain decimal integer with no suffix is parsed
    /// into [`TokenType::INTEGER`] so the literal-disguise pass can rewrite its
    /// value. Every other form — hex, binary and octal integers, floats, and
    /// any suffixed integer — is captured verbatim as [`TokenType::NUMBER`],
    /// because its base or suffix carries meaning that reparsing to a bare
    /// decimal would discard.
    fn read_number(&mut self) -> Result<TokenType, String> {
        let (line, column) = (self.line, self.column);
        let mut lexeme = String::new();

        // Prefixed integers: hex (`0x…`) and binary (`0b…`, a GCC extension).
        if self.peek() == Some('0') {
            match self.peek_next() {
                Some('x' | 'X') => {
                    lexeme.push(self.advance().unwrap()); // 0
                    lexeme.push(self.advance().unwrap()); // x
                    self.take_while(&mut lexeme, |c| c.is_ascii_hexdigit());
                    self.take_int_suffix(&mut lexeme);
                    return Ok(TokenType::NUMBER(lexeme));
                }
                Some('b' | 'B') => {
                    lexeme.push(self.advance().unwrap()); // 0
                    lexeme.push(self.advance().unwrap()); // b
                    self.take_while(&mut lexeme, |c| c == '0' || c == '1');
                    self.take_int_suffix(&mut lexeme);
                    return Ok(TokenType::NUMBER(lexeme));
                }
                _ => {}
            }
        }

        // Integer part (decimal digits).
        self.take_while(&mut lexeme, |c| c.is_ascii_digit());

        // A fractional part, an exponent, or a float suffix makes this a float.
        let mut is_float = false;
        if self.peek() == Some('.') {
            is_float = true;
            lexeme.push(self.advance().unwrap());
            self.take_while(&mut lexeme, |c| c.is_ascii_digit());
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            is_float = true;
            lexeme.push(self.advance().unwrap());
            if matches!(self.peek(), Some('+' | '-')) {
                lexeme.push(self.advance().unwrap());
            }
            self.take_while(&mut lexeme, |c| c.is_ascii_digit());
        }
        if matches!(self.peek(), Some('f' | 'F')) {
            lexeme.push(self.advance().unwrap());
            return Ok(TokenType::NUMBER(lexeme));
        }
        if is_float {
            // Trailing `l`/`L` marks a `long double`.
            if matches!(self.peek(), Some('l' | 'L')) {
                lexeme.push(self.advance().unwrap());
            }
            return Ok(TokenType::NUMBER(lexeme));
        }

        // Plain decimal integer. A type suffix (`100UL`) or a leading-zero
        // octal base (`0755`, which is *not* 755) must survive verbatim; only a
        // bare decimal is parsed so its value can be disguised.
        let had_suffix = self.take_int_suffix(&mut lexeme);
        let is_octal = lexeme.len() > 1 && lexeme.starts_with('0');
        if had_suffix || is_octal {
            return Ok(TokenType::NUMBER(lexeme));
        }

        lexeme
            .parse::<i64>()
            .map(TokenType::INTEGER)
            .map_err(|e| format!("{line}:{column}: invalid integer '{lexeme}': {e}"))
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
            '<' if eat(self, '<') => {
                if eat(self, '=') {
                    TokenType::LEFTSHIFTEQUALS
                } else {
                    TokenType::LEFTSHIFT
                }
            }
            '<' => TokenType::LESS,
            '>' if eat(self, '=') => TokenType::GREATEREQUALS,
            '>' if eat(self, '>') => {
                if eat(self, '=') {
                    TokenType::RIGHTSHIFTEQUALS
                } else {
                    TokenType::RIGHTSHIFT
                }
            }
            '>' => TokenType::GREATER,
            '&' if eat(self, '&') => TokenType::AMPAMP,
            '&' if eat(self, '=') => TokenType::AMPEQUALS,
            '&' => TokenType::AMP,
            '|' if eat(self, '|') => TokenType::PIPEPIPE,
            '|' if eat(self, '=') => TokenType::PIPEEQUALS,
            '|' => TokenType::PIPE,
            '^' if eat(self, '=') => TokenType::CARETEQUALS,
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
