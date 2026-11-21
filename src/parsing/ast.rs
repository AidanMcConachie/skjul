//! The abstract syntax tree for the subset of C this obfuscator understands.
//!
//! The lexer only distinguishes `int` and `return` as keywords; every other
//! C keyword (`char`, `if`, `for`, …) reaches us as an ordinary identifier,
//! and preprocessor lines arrive as opaque strings. The parser therefore
//! recognises the remaining keywords by their spelling and keeps directives
//! verbatim. This tree reflects that: it is deliberately a *practical subset*,
//! enough to carry structural obfuscation passes, not a complete C grammar.
//!
//! Anything the parser cannot model (function-pointer declarators, bitfields,
//! designated initialisers, K&R parameter lists, …) is reported as a parse
//! error rather than mis-modelled, so a caller can fall back to the token-level
//! passes when a program steps outside the subset.

/// A whole translation unit: the top-level items of one source file, in order.
#[derive(Debug, Clone)]
pub struct TranslationUnit {
    pub items: Vec<Item>,
}

/// One top-level item.
#[derive(Debug, Clone)]
pub enum Item {
    /// A preprocessor directive, kept verbatim from `#` to end of line.
    Directive(String),
    /// A function definition, or a prototype when `body` is `None`.
    Function(Function),
    /// A file-scope declaration, e.g. `int g = 3;`.
    Declaration(Declaration),
    /// Pre-rendered top-level text emitted verbatim by an obfuscation pass
    /// (e.g. an injected runtime helper and its supporting globals). Like
    /// [`Expr::Raw`], it is the producer's job to make the text a
    /// self-contained, well-formed sequence of top-level C.
    Raw(String),
}

#[derive(Debug, Clone)]
pub struct Function {
    pub return_type: Type,
    pub name: String,
    pub params: Vec<Param>,
    /// Whether the parameter list ends in `...`.
    pub variadic: bool,
    /// `Some` for a definition, `None` for a prototype (`int f(void);`).
    pub body: Option<Block>,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub ty: Type,
    pub name: Option<String>,
    /// Array dimensions written in `[]` form, e.g. the `[]` in `int a[]` or the
    /// `[4]` in `int a[4]`. Empty for a plain or pointer parameter. In C an
    /// array parameter decays to a pointer, but we keep its written shape so the
    /// printer reproduces the source faithfully.
    pub array_dims: Vec<Option<Expr>>,
}

/// A type: its specifier words in source order plus a pointer level.
///
/// Specifiers are kept as plain words (`["unsigned", "int"]`, `["struct",
/// "Point"]`) rather than interpreted, which is all the obfuscation passes
/// need — they rename and move code, they do not reason about types.
#[derive(Debug, Clone)]
pub struct Type {
    pub specifiers: Vec<String>,
    pub pointers: usize,
    /// The inline body of a `struct`/`union`/`enum` definition, e.g. the
    /// `{ int x; int y; }` in `struct Point { int x; int y; }`. `None` for a
    /// plain type or one that only names a tag (`struct Point`). The passes do
    /// not reason about layout; they carry the body through so the printer can
    /// reproduce the definition.
    pub body: Option<AggregateBody>,
}

impl Type {
    /// A plain type from specifier words, with no pointers and no inline body.
    pub fn plain(specifiers: Vec<String>) -> Self {
        Type {
            specifiers,
            pointers: 0,
            body: None,
        }
    }
}

/// The inline body of an aggregate type definition.
#[derive(Debug, Clone)]
pub enum AggregateBody {
    /// `struct`/`union` members, each a declaration (`int x;`, `char *s;`).
    Fields(Vec<Declaration>),
    /// `enum` constants, each a name with an optional explicit value.
    Enumerators(Vec<Enumerator>),
}

/// One `enum` constant: a name and, optionally, the `= value` that sets it.
#[derive(Debug, Clone)]
pub struct Enumerator {
    pub name: String,
    pub value: Option<Expr>,
}

/// One or more declarators sharing a type, e.g. `int *a = f(), b[3];`.
#[derive(Debug, Clone)]
pub struct Declaration {
    pub ty: Type,
    pub declarators: Vec<Declarator>,
}

#[derive(Debug, Clone)]
pub struct Declarator {
    pub name: String,
    /// `*` levels on this specific declarator (beyond those in the base type).
    pub pointers: usize,
    /// Array dimensions, outer first. `None` is an unsized `[]` dimension.
    pub array_dims: Vec<Option<Expr>>,
    pub init: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Declaration(Declaration),
    Expr(Expr),
    Return(Option<Expr>),
    If {
        cond: Expr,
        then_branch: Box<Stmt>,
        else_branch: Option<Box<Stmt>>,
    },
    While {
        cond: Expr,
        body: Box<Stmt>,
    },
    DoWhile {
        body: Box<Stmt>,
        cond: Expr,
    },
    For {
        /// A declaration or expression statement, or `None` for an empty slot.
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        step: Option<Expr>,
        body: Box<Stmt>,
    },
    Block(Block),
    Break,
    Continue,
    /// A lone `;`.
    Empty,
    Switch {
        cond: Expr,
        body: Box<Stmt>,
    },
    /// A `case <value>:` label prefixing a statement.
    Case {
        value: Expr,
        body: Box<Stmt>,
    },
    /// A `default:` label prefixing a statement.
    Default {
        body: Box<Stmt>,
    },
    /// A `name:` label prefixing a statement.
    Label {
        name: String,
        body: Box<Stmt>,
    },
    /// `goto name;`
    Goto(String),
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i64),
    /// String-literal content, stored verbatim (escapes kept, no quotes).
    Str(String),
    /// Character-literal content, stored verbatim (escapes kept, no quotes).
    Char(String),
    Ident(String),
    /// Pre-rendered text emitted verbatim by an obfuscation pass. Always
    /// parenthesised by its producer, so it drops into any context safely.
    Raw(String),
    Unary {
        op: UnOp,
        expr: Box<Expr>,
    },
    Postfix {
        op: PostOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Assign {
        op: AssignOp,
        target: Box<Expr>,
        value: Box<Expr>,
    },
    Ternary {
        cond: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },
    Cast {
        ty: Type,
        expr: Box<Expr>,
    },
    /// `sizeof(type)`, e.g. `sizeof(int)` or `sizeof(struct Point)`.
    SizeOfType(Type),
    /// `sizeof expr` or `sizeof(expr)`. The operand is never evaluated; only
    /// its type matters, so obfuscation passes leave it untouched.
    SizeOfValue(Box<Expr>),
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    Member {
        base: Box<Expr>,
        /// `true` for `->`, `false` for `.`.
        arrow: bool,
        field: String,
    },
    Comma {
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// A brace-enclosed initializer list, e.g. `{1, 2, 3}` or `{0}`. Elements
    /// may themselves be lists (nested aggregates). Appears only in an
    /// initializer position; designated initialisers are not modelled.
    InitList(Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    /// `-`
    Neg,
    /// `+`
    Pos,
    /// `!`
    Not,
    /// `~`
    BitNot,
    /// prefix `++`
    PreInc,
    /// prefix `--`
    PreDec,
    /// `*`
    Deref,
    /// `&`
    AddrOf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostOp {
    Inc,
    Dec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Shl,
    Shr,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    BitAnd,
    BitXor,
    BitOr,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    /// `=`
    Assign,
    /// `+=`
    Add,
    /// `-=`
    Sub,
    /// `*=`
    Mul,
    /// `/=`
    Div,
    /// `%=`
    Mod,
    /// `<<=`
    Shl,
    /// `>>=`
    Shr,
    /// `&=`
    BitAnd,
    /// `|=`
    BitOr,
    /// `^=`
    BitXor,
}

impl BinOp {
    /// The source spelling of the operator.
    pub fn text(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::BitAnd => "&",
            BinOp::BitXor => "^",
            BinOp::BitOr => "|",
            BinOp::And => "&&",
            BinOp::Or => "||",
        }
    }
}

impl UnOp {
    pub fn text(self) -> &'static str {
        match self {
            UnOp::Neg => "-",
            UnOp::Pos => "+",
            UnOp::Not => "!",
            UnOp::BitNot => "~",
            UnOp::PreInc => "++",
            UnOp::PreDec => "--",
            UnOp::Deref => "*",
            UnOp::AddrOf => "&",
        }
    }
}

impl PostOp {
    pub fn text(self) -> &'static str {
        match self {
            PostOp::Inc => "++",
            PostOp::Dec => "--",
        }
    }
}

impl AssignOp {
    pub fn text(self) -> &'static str {
        match self {
            AssignOp::Assign => "=",
            AssignOp::Add => "+=",
            AssignOp::Sub => "-=",
            AssignOp::Mul => "*=",
            AssignOp::Div => "/=",
            AssignOp::Mod => "%=",
            AssignOp::Shl => "<<=",
            AssignOp::Shr => ">>=",
            AssignOp::BitAnd => "&=",
            AssignOp::BitOr => "|=",
            AssignOp::BitXor => "^=",
        }
    }
}
