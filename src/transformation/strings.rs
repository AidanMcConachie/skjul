//! String-literal encryption with a runtime decoder.
//!
//! The token-level literal pass only re-encodes a string's bytes as octal/hex
//! escapes: the plaintext still sits intact in the compiled binary, so `strings`
//! recovers every message. This pass removes the plaintext from the binary
//! entirely. Each string literal reached in executable code is replaced by a
//! call to an injected decoder that reconstructs its bytes at run time:
//!
//! ```c
//! // injected once, at file scope:
//! char *_sk_dec(char *_sk_s, char *_sk_o, int _sk_n, int _sk_k) {
//!     int _sk_i;
//!     for (_sk_i = 0; _sk_i < _sk_n; _sk_i++)
//!         _sk_o[_sk_i] = _sk_s[_sk_i] ^ (_sk_k + _sk_i);
//!     _sk_o[_sk_n] = 0;
//!     return _sk_o;
//! }
//! static char _sk_c0[] = { /* ciphertext bytes */ };
//! static char _sk_b0[6];           // plaintext scratch buffer
//!
//! // "Hello" at its use site becomes:
//! (_sk_dec(_sk_c0, _sk_b0, 5, 37))
//! ```
//!
//! The cipher is a per-string rolling XOR (`byte ^ (key + index)`), keyed from
//! the shared [`Rng`] so the output stays deterministic. Decryption always runs
//! from the pristine, `const`-in-spirit ciphertext into a *separate* per-string
//! buffer, so decoding is idempotent (a string used in a loop decodes correctly
//! every iteration) and two distinct strings never share a buffer (`printf("%s
//! %s", a, b)` stays correct). The decoder is not reentrant across threads — the
//! scratch buffers are `static` — which matches the single-threaded scope of the
//! rest of the crate.
//!
//! Like every pass here it is conservative. Only string literals in executable
//! positions are touched; a string in **any** declaration initializer is left
//! alone, because those positions can require a constant expression (`static`
//! and file-scope initializers) or the literal's array type (`char buf[] =
//! "x";`), neither of which a function call can satisfy. The operand of
//! `sizeof` and of a `&` are likewise left intact, since replacing the literal
//! there would change the type's size or make the address-of ill-formed.

use super::Rng;
use crate::obfuscation::literals::decode_c_escapes;
use crate::parsing::ast::*;

/// Name of the injected decoder. Renaming happens later over the whole token
/// stream, so these only need to be unlikely to clash with user code.
const DEC: &str = "_sk_dec";

/// Encrypt every eligible string literal in the unit, injecting the decoder and
/// its supporting globals when at least one string was rewritten.
pub fn encrypt_strings(unit: TranslationUnit, rng: &mut Rng) -> TranslationUnit {
    let mut c = Collector {
        rng,
        entries: Vec::new(),
    };

    let mut items: Vec<Item> = unit
        .items
        .into_iter()
        .map(|item| match item {
            // Only function bodies hold executable code. File-scope
            // declarations and directives are left untouched.
            Item::Function(mut f) => {
                if let Some(body) = f.body.take() {
                    f.body = Some(c.xform_block(body));
                }
                Item::Function(f)
            }
            other => other,
        })
        .collect();

    if c.entries.is_empty() {
        return TranslationUnit { items };
    }

    // Place the decoder and its globals after any leading directives (so an
    // `#include` still comes first) but before the first function that uses
    // them.
    let support = c.render_support();
    let pos = items
        .iter()
        .position(|it| !matches!(it, Item::Directive(_)))
        .unwrap_or(items.len());
    items.insert(pos, Item::Raw(support));

    TranslationUnit { items }
}

struct Collector<'r> {
    rng: &'r mut Rng,
    /// The ciphertext bytes of each encrypted string, in replacement order. The
    /// per-string key is baked straight into the decoder call, so only the
    /// ciphertext needs carrying to [`Collector::render_support`].
    entries: Vec<Vec<u8>>,
}

impl Collector<'_> {
    /// Encrypt one string's (escape-decoded) bytes, record it, and return the
    /// decoder-call expression that replaces the literal.
    fn encrypt(&mut self, content: String) -> Expr {
        let plain = decode_c_escapes(&content);
        // A non-zero key, so no byte is ever left as itself at index 0.
        let key = self.rng.range(1, 255) as u8;
        let cipher: Vec<u8> = plain
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ key.wrapping_add(i as u8))
            .collect();

        let idx = self.entries.len();
        let n = cipher.len();
        self.entries.push(cipher);

        // Parenthesised, per the `Expr::Raw` contract, so it drops into any
        // context (indexing, call argument, return value) without re-grouping.
        Expr::Raw(format!("({DEC}(_sk_c{idx},_sk_b{idx},{n},{key}))"))
    }

    /// Render the decoder plus every string's ciphertext and scratch buffer as
    /// one verbatim block of top-level C.
    fn render_support(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "char *{DEC}(char *_sk_s,char *_sk_o,int _sk_n,int _sk_k){{\
int _sk_i;\
for(_sk_i=0;_sk_i<_sk_n;_sk_i++){{_sk_o[_sk_i]=_sk_s[_sk_i]^(_sk_k+_sk_i);}}\
_sk_o[_sk_n]=0;\
return _sk_o;}}\n"
        ));
        for (idx, cipher) in self.entries.iter().enumerate() {
            let n = cipher.len();
            if n == 0 {
                // An empty `{}` initializer is not portable C; give the empty
                // string a one-element array the decoder simply never reads.
                s.push_str(&format!("static char _sk_c{idx}[1]={{0}};\n"));
                s.push_str(&format!("static char _sk_b{idx}[1];\n"));
            } else {
                let bytes = cipher
                    .iter()
                    .map(|b| b.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                s.push_str(&format!("static char _sk_c{idx}[]={{{bytes}}};\n"));
                s.push_str(&format!("static char _sk_b{idx}[{}];\n", n + 1));
            }
        }
        s
    }

    fn xform_block(&mut self, block: Block) -> Block {
        Block {
            stmts: block
                .stmts
                .into_iter()
                .map(|s| self.xform_stmt(s))
                .collect(),
        }
    }

    fn xform_stmt(&mut self, stmt: Stmt) -> Stmt {
        match stmt {
            // Declarations are skipped wholesale: a string in an initializer
            // can require a constant expression or the literal's array type.
            Stmt::Declaration(d) => Stmt::Declaration(d),

            Stmt::Expr(e) => Stmt::Expr(self.xe(e)),
            Stmt::Return(Some(e)) => Stmt::Return(Some(self.xe(e))),
            Stmt::Return(None) => Stmt::Return(None),

            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => Stmt::If {
                cond: self.xe(cond),
                then_branch: Box::new(self.xform_stmt(*then_branch)),
                else_branch: else_branch.map(|b| Box::new(self.xform_stmt(*b))),
            },
            Stmt::While { cond, body } => Stmt::While {
                cond: self.xe(cond),
                body: Box::new(self.xform_stmt(*body)),
            },
            Stmt::DoWhile { body, cond } => Stmt::DoWhile {
                body: Box::new(self.xform_stmt(*body)),
                cond: self.xe(cond),
            },
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => Stmt::For {
                init: init.map(|b| Box::new(self.xform_stmt(*b))),
                cond: cond.map(|e| self.xe(e)),
                step: step.map(|e| self.xe(e)),
                body: Box::new(self.xform_stmt(*body)),
            },
            Stmt::Block(b) => Stmt::Block(self.xform_block(b)),
            Stmt::Switch { cond, body } => Stmt::Switch {
                cond: self.xe(cond),
                body: Box::new(self.xform_stmt(*body)),
            },
            // A `case` value must be a constant expression, so it is left as
            // written; only the labelled statement is transformed.
            Stmt::Case { value, body } => Stmt::Case {
                value,
                body: Box::new(self.xform_stmt(*body)),
            },
            Stmt::Default { body } => Stmt::Default {
                body: Box::new(self.xform_stmt(*body)),
            },
            Stmt::Label { name, body } => Stmt::Label {
                name,
                body: Box::new(self.xform_stmt(*body)),
            },

            other @ (Stmt::Break | Stmt::Continue | Stmt::Empty | Stmt::Goto(_)) => other,
        }
    }

    /// Replace every string literal in an executable expression, leaving the
    /// operands of `sizeof` and `&` intact.
    fn xe(&mut self, e: Expr) -> Expr {
        match e {
            Expr::Str(content) => self.encrypt(content),

            Expr::Int(_) | Expr::Char(_) | Expr::Ident(_) | Expr::Raw(_) | Expr::InitList(_) => e,

            Expr::Unary { op, expr } => {
                // `&"str"` has array type and is well-formed; a decoder call is
                // an rvalue whose address cannot be taken. Leave it alone.
                if op == UnOp::AddrOf && matches!(*expr, Expr::Str(_)) {
                    Expr::Unary { op, expr }
                } else {
                    Expr::Unary {
                        op,
                        expr: Box::new(self.xe(*expr)),
                    }
                }
            }
            Expr::Postfix { op, expr } => Expr::Postfix {
                op,
                expr: Box::new(self.xe(*expr)),
            },
            Expr::Binary { op, lhs, rhs } => Expr::Binary {
                op,
                lhs: Box::new(self.xe(*lhs)),
                rhs: Box::new(self.xe(*rhs)),
            },
            Expr::Assign { op, target, value } => Expr::Assign {
                op,
                target: Box::new(self.xe(*target)),
                value: Box::new(self.xe(*value)),
            },
            Expr::Ternary {
                cond,
                then_expr,
                else_expr,
            } => Expr::Ternary {
                cond: Box::new(self.xe(*cond)),
                then_expr: Box::new(self.xe(*then_expr)),
                else_expr: Box::new(self.xe(*else_expr)),
            },
            Expr::Cast { ty, expr } => Expr::Cast {
                ty,
                expr: Box::new(self.xe(*expr)),
            },
            Expr::Call { callee, args } => {
                // `sizeof("str")` parses as a call here; its operand's type
                // (and thus the computed size) must not change.
                if let Expr::Ident(name) = &*callee
                    && name == "sizeof"
                {
                    return Expr::Call { callee, args };
                }
                Expr::Call {
                    callee: Box::new(self.xe(*callee)),
                    args: args.into_iter().map(|a| self.xe(a)).collect(),
                }
            }
            Expr::Index { base, index } => Expr::Index {
                base: Box::new(self.xe(*base)),
                index: Box::new(self.xe(*index)),
            },
            Expr::Member { base, arrow, field } => Expr::Member {
                base: Box::new(self.xe(*base)),
                arrow,
                field,
            },
            Expr::Comma { lhs, rhs } => Expr::Comma {
                lhs: Box::new(self.xe(*lhs)),
                rhs: Box::new(self.xe(*rhs)),
            },
        }
    }
}
