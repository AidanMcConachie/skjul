//! A hand-written recursive-descent parser for the C subset described in
//! [`super::ast`].
//!
//! The parser runs over the token stream the lexer already produces. Because
//! the lexer tokenises only `int` and `return` as keywords, the remaining
//! keywords (`if`, `for`, `char`, …) arrive as [`TokenType::IDENTIFIER`]s and
//! are recognised here by their spelling. Preprocessor directives are carried
//! through untouched.
//!
//! Every method returns a `Result<_, String>` carrying a `line:column`
//! message on failure, so a caller can detect a program outside the subset and
//! fall back to the token-level passes rather than emitting wrong code.

use super::ast::*;
use crate::tokenization::token::{Token, TokenType};

/// Parse a full token stream into a translation unit.
pub fn parse(tokens: Vec<Token>) -> Result<TranslationUnit, String> {
    let mut parser = Parser { tokens, pos: 0 };
    let mut items = Vec::new();
    while !parser.at_end() {
        items.push(parser.parse_item()?);
    }
    Ok(TranslationUnit { items })
}

/// Type-specifier keywords. `int` is a keyword token and handled separately;
/// the rest reach the parser as identifiers.
const TYPE_KEYWORDS: &[&str] = &[
    "char", "void", "float", "double", "long", "short", "unsigned", "signed",
    "const", "volatile", "struct", "union", "enum", "static", "extern",
    "register", "auto", "_Bool",
];

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    // ---- token cursor -----------------------------------------------------

    fn peek(&self) -> &TokenType {
        &self.tokens[self.pos].kind
    }

    /// The kind `offset` tokens ahead, saturating at the final `EOF`.
    fn peek_at(&self, offset: usize) -> &TokenType {
        let i = (self.pos + offset).min(self.tokens.len() - 1);
        &self.tokens[i].kind
    }

    fn at_end(&self) -> bool {
        matches!(self.peek(), TokenType::EOF)
    }

    fn advance(&mut self) -> TokenType {
        let kind = self.tokens[self.pos].kind.clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        kind
    }

    /// The `line:column` of the current token, for error messages.
    fn here(&self) -> String {
        let t = &self.tokens[self.pos];
        format!("{}:{}", t.line, t.column)
    }

    fn error<T>(&self, what: &str) -> Result<T, String> {
        Err(format!("{}: {what}", self.here()))
    }

    /// Consume the current token if its kind equals `kind`.
    fn eat(&mut self, kind: &TokenType) -> bool {
        if self.peek() == kind {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: &TokenType, what: &str) -> Result<(), String> {
        if self.eat(kind) {
            Ok(())
        } else {
            self.error(&format!("expected {what}"))
        }
    }

    // ---- keyword / identifier helpers -------------------------------------

    /// Whether the current token is the identifier keyword `kw`.
    fn is_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), TokenType::IDENTIFIER(name) if name == kw)
    }

    /// Consume the current token if it is the identifier keyword `kw`.
    fn eat_kw(&mut self, kw: &str) -> bool {
        if self.is_kw(kw) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect_ident(&mut self) -> Result<String, String> {
        match self.peek().clone() {
            TokenType::IDENTIFIER(name) => {
                self.advance();
                Ok(name)
            }
            _ => self.error("expected identifier"),
        }
    }

    /// Whether the token at `offset` begins a type (the `int` keyword or a
    /// type-specifier identifier).
    fn is_type_start_at(&self, offset: usize) -> bool {
        match self.peek_at(offset) {
            TokenType::INT => true,
            TokenType::IDENTIFIER(name) => TYPE_KEYWORDS.contains(&name.as_str()),
            _ => false,
        }
    }

    // ---- top-level items --------------------------------------------------

    fn parse_item(&mut self) -> Result<Item, String> {
        if let TokenType::PREPROCESSOR(line) = self.peek().clone() {
            self.advance();
            return Ok(Item::Directive(line));
        }

        // A function definition/prototype or a file-scope declaration. Both
        // start with a type and a (possibly pointer-qualified) name; a `(`
        // after the name marks a function.
        let ty = self.parse_type()?;
        let pointers = self.parse_pointers();
        let name = self.expect_ident()?;

        if self.eat(&TokenType::LEFTPAREN) {
            let mut ret = ty;
            ret.pointers += pointers;
            let (params, variadic) = self.parse_params()?;
            self.expect(&TokenType::RIGHTPAREN, "')'")?;
            let body = if self.eat(&TokenType::SEMICOLON) {
                None
            } else {
                Some(self.parse_block()?)
            };
            Ok(Item::Function(Function {
                return_type: ret,
                name,
                params,
                variadic,
                body,
            }))
        } else {
            let decl = self.finish_declaration(ty, pointers, name)?;
            Ok(Item::Declaration(decl))
        }
    }

    /// Parse the specifier words of a type (no pointers). Requires at least
    /// one specifier. A `struct`/`union`/`enum` is followed by its tag name; an
    /// inline `{ … }` body is outside the subset and rejected.
    fn parse_type(&mut self) -> Result<Type, String> {
        if !self.is_type_start_at(0) {
            return self.error("expected a type");
        }
        let mut specifiers = Vec::new();
        loop {
            match self.peek().clone() {
                TokenType::INT => {
                    self.advance();
                    specifiers.push("int".to_string());
                }
                TokenType::IDENTIFIER(name) if TYPE_KEYWORDS.contains(&name.as_str()) => {
                    self.advance();
                    specifiers.push(name.clone());
                    // A tagged type names its tag next, e.g. `struct Point`.
                    if matches!(name.as_str(), "struct" | "union" | "enum") {
                        if let TokenType::IDENTIFIER(tag) = self.peek().clone() {
                            self.advance();
                            specifiers.push(tag);
                        }
                        if self.peek() == &TokenType::LEFTBRACE {
                            return self.error("inline struct/union/enum bodies are unsupported");
                        }
                    }
                }
                _ => break,
            }
        }
        Ok(Type {
            specifiers,
            pointers: 0,
        })
    }

    /// Count and consume a run of `*` pointer markers.
    fn parse_pointers(&mut self) -> usize {
        let mut n = 0;
        while self.eat(&TokenType::STAR) {
            n += 1;
        }
        n
    }

    fn parse_params(&mut self) -> Result<(Vec<Param>, bool), String> {
        let mut params = Vec::new();
        let mut variadic = false;

        if self.peek() == &TokenType::RIGHTPAREN {
            return Ok((params, variadic)); // empty list
        }

        // `(void)` is an empty list spelled out.
        if self.is_kw("void") && self.peek_at(1) == &TokenType::RIGHTPAREN {
            self.advance();
            return Ok((params, variadic));
        }

        loop {
            if self.try_variadic() {
                variadic = true;
                break;
            }
            let mut ty = self.parse_type()?;
            ty.pointers = self.parse_pointers();
            // A parameter name is optional (prototypes may omit it).
            let name = match self.peek() {
                TokenType::IDENTIFIER(n) => {
                    let n = n.clone();
                    self.advance();
                    Some(n)
                }
                _ => None,
            };
            params.push(Param { ty, name });
            if !self.eat(&TokenType::COMMA) {
                break;
            }
        }
        Ok((params, variadic))
    }

    /// Consume a `...` variadic marker (three `.` tokens) if present.
    fn try_variadic(&mut self) -> bool {
        if self.peek() == &TokenType::DOT
            && self.peek_at(1) == &TokenType::DOT
            && self.peek_at(2) == &TokenType::DOT
        {
            self.advance();
            self.advance();
            self.advance();
            true
        } else {
            false
        }
    }

    // ---- declarations -----------------------------------------------------

    /// Finish a declaration whose base type and first declarator's pointers and
    /// name have already been consumed, through to the terminating `;`.
    fn finish_declaration(
        &mut self,
        ty: Type,
        first_pointers: usize,
        first_name: String,
    ) -> Result<Declaration, String> {
        let mut declarators = vec![self.parse_declarator_tail(first_pointers, first_name)?];
        while self.eat(&TokenType::COMMA) {
            let pointers = self.parse_pointers();
            let name = self.expect_ident()?;
            declarators.push(self.parse_declarator_tail(pointers, name)?);
        }
        self.expect(&TokenType::SEMICOLON, "';'")?;
        Ok(Declaration { ty, declarators })
    }

    /// Parse the array dimensions and optional initialiser of one declarator,
    /// given its already-parsed pointers and name.
    fn parse_declarator_tail(
        &mut self,
        pointers: usize,
        name: String,
    ) -> Result<Declarator, String> {
        let mut array_dims = Vec::new();
        while self.eat(&TokenType::LEFTBRACKET) {
            if self.eat(&TokenType::RIGHTBRACKET) {
                array_dims.push(None);
            } else {
                let dim = self.parse_assignment()?;
                self.expect(&TokenType::RIGHTBRACKET, "']'")?;
                array_dims.push(Some(dim));
            }
        }
        let init = if self.eat(&TokenType::EQUALS) {
            Some(self.parse_initializer()?)
        } else {
            None
        };
        Ok(Declarator {
            name,
            pointers,
            array_dims,
            init,
        })
    }

    /// Parse an initializer: either a single assignment-expression or a
    /// brace-enclosed list (which may nest and may end with a trailing comma).
    fn parse_initializer(&mut self) -> Result<Expr, String> {
        if !self.eat(&TokenType::LEFTBRACE) {
            return self.parse_assignment();
        }
        let mut elems = Vec::new();
        while self.peek() != &TokenType::RIGHTBRACE {
            elems.push(self.parse_initializer()?);
            if !self.eat(&TokenType::COMMA) {
                break;
            }
        }
        self.expect(&TokenType::RIGHTBRACE, "'}'")?;
        Ok(Expr::InitList(elems))
    }

    /// Parse a declaration beginning at the current token (type first).
    fn parse_declaration(&mut self) -> Result<Declaration, String> {
        let ty = self.parse_type()?;
        let pointers = self.parse_pointers();
        let name = self.expect_ident()?;
        self.finish_declaration(ty, pointers, name)
    }

    // ---- statements -------------------------------------------------------

    fn parse_block(&mut self) -> Result<Block, String> {
        self.expect(&TokenType::LEFTBRACE, "'{'")?;
        let mut stmts = Vec::new();
        while self.peek() != &TokenType::RIGHTBRACE {
            if self.at_end() {
                return self.error("unterminated block");
            }
            stmts.push(self.parse_stmt()?);
        }
        self.expect(&TokenType::RIGHTBRACE, "'}'")?;
        Ok(Block { stmts })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        match self.peek() {
            TokenType::LEFTBRACE => Ok(Stmt::Block(self.parse_block()?)),
            TokenType::SEMICOLON => {
                self.advance();
                Ok(Stmt::Empty)
            }
            TokenType::RETURN => {
                self.advance();
                if self.eat(&TokenType::SEMICOLON) {
                    Ok(Stmt::Return(None))
                } else {
                    let e = self.parse_expression()?;
                    self.expect(&TokenType::SEMICOLON, "';'")?;
                    Ok(Stmt::Return(Some(e)))
                }
            }
            _ if self.is_kw("if") => self.parse_if(),
            _ if self.is_kw("while") => self.parse_while(),
            _ if self.is_kw("do") => self.parse_do_while(),
            _ if self.is_kw("for") => self.parse_for(),
            _ if self.is_kw("switch") => self.parse_switch(),
            _ if self.is_kw("case") => self.parse_case(),
            _ if self.is_kw("default") => self.parse_default(),
            _ if self.is_kw("goto") => self.parse_goto(),
            _ if self.is_kw("break") => {
                self.advance();
                self.expect(&TokenType::SEMICOLON, "';'")?;
                Ok(Stmt::Break)
            }
            _ if self.is_kw("continue") => {
                self.advance();
                self.expect(&TokenType::SEMICOLON, "';'")?;
                Ok(Stmt::Continue)
            }
            // A label: an identifier immediately followed by `:`. Checked after
            // the keyword statements (which are also identifiers) so only a
            // genuine label name reaches here.
            TokenType::IDENTIFIER(_) if self.peek_at(1) == &TokenType::COLON => {
                let name = self.expect_ident()?;
                self.expect(&TokenType::COLON, "':'")?;
                let body = Box::new(self.parse_stmt()?);
                Ok(Stmt::Label { name, body })
            }
            _ if self.is_type_start_at(0) => Ok(Stmt::Declaration(self.parse_declaration()?)),
            _ => {
                let e = self.parse_expression()?;
                self.expect(&TokenType::SEMICOLON, "';'")?;
                Ok(Stmt::Expr(e))
            }
        }
    }

    fn parse_if(&mut self) -> Result<Stmt, String> {
        self.eat_kw("if");
        self.expect(&TokenType::LEFTPAREN, "'('")?;
        let cond = self.parse_expression()?;
        self.expect(&TokenType::RIGHTPAREN, "')'")?;
        let then_branch = Box::new(self.parse_stmt()?);
        let else_branch = if self.eat_kw("else") {
            Some(Box::new(self.parse_stmt()?))
        } else {
            None
        };
        Ok(Stmt::If {
            cond,
            then_branch,
            else_branch,
        })
    }

    fn parse_while(&mut self) -> Result<Stmt, String> {
        self.eat_kw("while");
        self.expect(&TokenType::LEFTPAREN, "'('")?;
        let cond = self.parse_expression()?;
        self.expect(&TokenType::RIGHTPAREN, "')'")?;
        let body = Box::new(self.parse_stmt()?);
        Ok(Stmt::While { cond, body })
    }

    fn parse_do_while(&mut self) -> Result<Stmt, String> {
        self.eat_kw("do");
        let body = Box::new(self.parse_stmt()?);
        if !self.eat_kw("while") {
            return self.error("expected 'while' after 'do' body");
        }
        self.expect(&TokenType::LEFTPAREN, "'('")?;
        let cond = self.parse_expression()?;
        self.expect(&TokenType::RIGHTPAREN, "')'")?;
        self.expect(&TokenType::SEMICOLON, "';'")?;
        Ok(Stmt::DoWhile { body, cond })
    }

    fn parse_switch(&mut self) -> Result<Stmt, String> {
        self.eat_kw("switch");
        self.expect(&TokenType::LEFTPAREN, "'('")?;
        let cond = self.parse_expression()?;
        self.expect(&TokenType::RIGHTPAREN, "')'")?;
        let body = Box::new(self.parse_stmt()?);
        Ok(Stmt::Switch { cond, body })
    }

    fn parse_case(&mut self) -> Result<Stmt, String> {
        self.eat_kw("case");
        // A case label is a constant expression: conditional-expression level,
        // so it stops before any `,` separating sibling statements.
        let value = self.parse_ternary()?;
        self.expect(&TokenType::COLON, "':'")?;
        let body = Box::new(self.parse_stmt()?);
        Ok(Stmt::Case { value, body })
    }

    fn parse_default(&mut self) -> Result<Stmt, String> {
        self.eat_kw("default");
        self.expect(&TokenType::COLON, "':'")?;
        let body = Box::new(self.parse_stmt()?);
        Ok(Stmt::Default { body })
    }

    fn parse_goto(&mut self) -> Result<Stmt, String> {
        self.eat_kw("goto");
        let name = self.expect_ident()?;
        self.expect(&TokenType::SEMICOLON, "';'")?;
        Ok(Stmt::Goto(name))
    }

    fn parse_for(&mut self) -> Result<Stmt, String> {
        self.eat_kw("for");
        self.expect(&TokenType::LEFTPAREN, "'('")?;

        // init clause (consumes its own `;`)
        let init = if self.eat(&TokenType::SEMICOLON) {
            None
        } else if self.is_type_start_at(0) {
            Some(Box::new(Stmt::Declaration(self.parse_declaration()?)))
        } else {
            let e = self.parse_expression()?;
            self.expect(&TokenType::SEMICOLON, "';'")?;
            Some(Box::new(Stmt::Expr(e)))
        };

        // condition
        let cond = if self.peek() == &TokenType::SEMICOLON {
            None
        } else {
            Some(self.parse_expression()?)
        };
        self.expect(&TokenType::SEMICOLON, "';'")?;

        // step
        let step = if self.peek() == &TokenType::RIGHTPAREN {
            None
        } else {
            Some(self.parse_expression()?)
        };
        self.expect(&TokenType::RIGHTPAREN, "')'")?;

        let body = Box::new(self.parse_stmt()?);
        Ok(Stmt::For {
            init,
            cond,
            step,
            body,
        })
    }

    // ---- expressions (precedence climbing) --------------------------------

    /// Full expression, including the comma operator.
    fn parse_expression(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_assignment()?;
        while self.eat(&TokenType::COMMA) {
            let right = self.parse_assignment()?;
            left = Expr::Comma {
                lhs: Box::new(left),
                rhs: Box::new(right),
            };
        }
        Ok(left)
    }

    /// Assignment is right-associative and sits just above the comma operator.
    fn parse_assignment(&mut self) -> Result<Expr, String> {
        let left = self.parse_ternary()?;
        let op = match self.peek() {
            TokenType::EQUALS => AssignOp::Assign,
            TokenType::PLUSEQUALS => AssignOp::Add,
            TokenType::MINUSEQUALS => AssignOp::Sub,
            TokenType::STAREQUALS => AssignOp::Mul,
            TokenType::SLASHEQUALS => AssignOp::Div,
            TokenType::PERCENTEQUALS => AssignOp::Mod,
            TokenType::LEFTSHIFTEQUALS => AssignOp::Shl,
            TokenType::RIGHTSHIFTEQUALS => AssignOp::Shr,
            TokenType::AMPEQUALS => AssignOp::BitAnd,
            TokenType::PIPEEQUALS => AssignOp::BitOr,
            TokenType::CARETEQUALS => AssignOp::BitXor,
            _ => return Ok(left),
        };
        self.advance();
        let value = self.parse_assignment()?;
        Ok(Expr::Assign {
            op,
            target: Box::new(left),
            value: Box::new(value),
        })
    }

    fn parse_ternary(&mut self) -> Result<Expr, String> {
        let cond = self.parse_binary(0)?;
        if self.eat(&TokenType::QUESTION) {
            let then_expr = self.parse_expression()?;
            self.expect(&TokenType::COLON, "':'")?;
            let else_expr = self.parse_assignment()?;
            Ok(Expr::Ternary {
                cond: Box::new(cond),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            })
        } else {
            Ok(cond)
        }
    }

    /// Binary operators, parsed by precedence climbing. `min_prec` is the
    /// lowest precedence this call will accept; higher binds tighter.
    fn parse_binary(&mut self, min_prec: u8) -> Result<Expr, String> {
        let mut left = self.parse_unary()?;
        while let Some((op, prec)) = binary_op(self.peek()) {
            if prec < min_prec {
                break;
            }
            self.advance();
            // All these operators are left-associative, so the right operand
            // takes only tighter-binding operators (precedence + 1).
            let right = self.parse_binary(prec + 1)?;
            left = Expr::Binary {
                op,
                lhs: Box::new(left),
                rhs: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        // A cast: `(` type `)` unary. Distinguished from a parenthesised
        // expression by a type keyword following the `(`.
        if self.peek() == &TokenType::LEFTPAREN && self.is_type_start_at(1) {
            self.advance(); // (
            let mut ty = self.parse_type()?;
            ty.pointers = self.parse_pointers();
            self.expect(&TokenType::RIGHTPAREN, "')'")?;
            let expr = self.parse_unary()?;
            return Ok(Expr::Cast {
                ty,
                expr: Box::new(expr),
            });
        }

        let op = match self.peek() {
            TokenType::MINUS => UnOp::Neg,
            TokenType::PLUS => UnOp::Pos,
            TokenType::BANG => UnOp::Not,
            TokenType::TILDE => UnOp::BitNot,
            TokenType::PLUSPLUS => UnOp::PreInc,
            TokenType::MINUSMINUS => UnOp::PreDec,
            TokenType::STAR => UnOp::Deref,
            TokenType::AMP => UnOp::AddrOf,
            _ => return self.parse_postfix(),
        };
        self.advance();
        let expr = self.parse_unary()?;
        Ok(Expr::Unary {
            op,
            expr: Box::new(expr),
        })
    }

    fn parse_postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.parse_primary()?;
        loop {
            match self.peek() {
                TokenType::LEFTPAREN => {
                    self.advance();
                    let args = self.parse_args()?;
                    self.expect(&TokenType::RIGHTPAREN, "')'")?;
                    e = Expr::Call {
                        callee: Box::new(e),
                        args,
                    };
                }
                TokenType::LEFTBRACKET => {
                    self.advance();
                    let index = self.parse_expression()?;
                    self.expect(&TokenType::RIGHTBRACKET, "']'")?;
                    e = Expr::Index {
                        base: Box::new(e),
                        index: Box::new(index),
                    };
                }
                TokenType::DOT => {
                    self.advance();
                    let field = self.expect_ident()?;
                    e = Expr::Member {
                        base: Box::new(e),
                        arrow: false,
                        field,
                    };
                }
                TokenType::ARROW => {
                    self.advance();
                    let field = self.expect_ident()?;
                    e = Expr::Member {
                        base: Box::new(e),
                        arrow: true,
                        field,
                    };
                }
                TokenType::PLUSPLUS => {
                    self.advance();
                    e = Expr::Postfix {
                        op: PostOp::Inc,
                        expr: Box::new(e),
                    };
                }
                TokenType::MINUSMINUS => {
                    self.advance();
                    e = Expr::Postfix {
                        op: PostOp::Dec,
                        expr: Box::new(e),
                    };
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn parse_args(&mut self) -> Result<Vec<Expr>, String> {
        let mut args = Vec::new();
        if self.peek() == &TokenType::RIGHTPAREN {
            return Ok(args);
        }
        loop {
            args.push(self.parse_assignment()?);
            if !self.eat(&TokenType::COMMA) {
                break;
            }
        }
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.peek().clone() {
            TokenType::INTEGER(n) => {
                self.advance();
                Ok(Expr::Int(n))
            }
            TokenType::CHARACTER(s) => {
                self.advance();
                Ok(Expr::Char(s))
            }
            TokenType::STRING(first) => {
                self.advance();
                // Adjacent string literals are concatenated in C; fold them
                // into one node so the content stays a single unit.
                let mut content = first;
                while let TokenType::STRING(next) = self.peek().clone() {
                    self.advance();
                    content.push_str(&next);
                }
                Ok(Expr::Str(content))
            }
            TokenType::IDENTIFIER(name) => {
                self.advance();
                Ok(Expr::Ident(name))
            }
            TokenType::LEFTPAREN => {
                self.advance();
                let e = self.parse_expression()?;
                self.expect(&TokenType::RIGHTPAREN, "')'")?;
                Ok(e)
            }
            _ => self.error("expected an expression"),
        }
    }
}

/// Map a token to its binary operator and precedence (higher binds tighter),
/// or `None` if it is not a binary operator handled here.
fn binary_op(kind: &TokenType) -> Option<(BinOp, u8)> {
    let pair = match kind {
        TokenType::STAR => (BinOp::Mul, 10),
        TokenType::SLASH => (BinOp::Div, 10),
        TokenType::PERCENT => (BinOp::Mod, 10),
        TokenType::PLUS => (BinOp::Add, 9),
        TokenType::MINUS => (BinOp::Sub, 9),
        TokenType::LEFTSHIFT => (BinOp::Shl, 8),
        TokenType::RIGHTSHIFT => (BinOp::Shr, 8),
        TokenType::LESS => (BinOp::Lt, 7),
        TokenType::LESSEQUALS => (BinOp::Le, 7),
        TokenType::GREATER => (BinOp::Gt, 7),
        TokenType::GREATEREQUALS => (BinOp::Ge, 7),
        TokenType::EQUALSEQUALS => (BinOp::Eq, 6),
        TokenType::BANGEQUALS => (BinOp::Ne, 6),
        TokenType::AMP => (BinOp::BitAnd, 5),
        TokenType::CARET => (BinOp::BitXor, 4),
        TokenType::PIPE => (BinOp::BitOr, 3),
        TokenType::AMPAMP => (BinOp::And, 2),
        TokenType::PIPEPIPE => (BinOp::Or, 1),
        _ => return None,
    };
    Some(pair)
}
