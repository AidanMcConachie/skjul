//! Dead-code insertion.
//!
//! Junk statements are spliced between the real statements of a block, each
//! hidden behind an always-false opaque predicate:
//!
//! ```c
//! if (<opaque false>) {
//!     int _dc0 = 37;
//!     int _dc1 = 12;
//!     _dc0 = ((_dc0 ^ 90) + 15) & 255;
//!     _dc1 = ((_dc1 + _dc0) ^ 46) & 255;
//! }
//! ```
//!
//! The guard is provably false (see [`super::predicates::opaque_false`]), so
//! the body never runs and behaviour is unchanged. A reader, though, must
//! evaluate the predicate to know that, and the junk adds plausible-looking
//! statements and extra identifiers for the later rename pass to blur into the
//! real ones.
//!
//! The pass is safe by construction in two independent ways:
//!
//! * The junk is wholly self-contained. Every variable it uses it also
//!   declares, inside the guard's own block, so it can be spliced into any
//!   scope without colliding with — or depending on — the surrounding code,
//!   and it produces no output and no effect reachable from outside the block.
//! * Even in the impossible event the guard evaluated true, the junk still
//!   could not misbehave: every value is masked back into `0..=255` after each
//!   step, so the arithmetic can neither overflow nor divide, and the only
//!   state it touches is its own locals.
//!
//! Junk is only spliced between the statements of a genuine `{ … }` block, and
//! never directly inside a `switch` body, where a statement sitting between the
//! `case` labels would land in a case's fallthrough (harmless, since the guard
//! is false, but needlessly fragile). The bodies *inside* each `case` are still
//! visited, so dead code reaches those too.

use super::predicates::{add, band, bxor, int, opaque_false};
use super::Rng;
use crate::parsing::ast::*;

/// Name stem for the throwaway variables a junk block declares. Renaming
/// happens later over the whole token stream, so this only needs to be
/// unlikely to clash with user code; each junk block scopes its own copies.
const JUNK_VAR: &str = "_dc";

/// Roughly one splice point in [`DEAD_ODDS`] is taken, keeping the inserted
/// bulk moderate even as it compounds with the opaque-predicate pass.
const DEAD_ODDS: u64 = 3;

/// Sprinkle dead code through every function body in the unit.
pub fn insert_dead_code(unit: TranslationUnit, rng: &mut Rng) -> TranslationUnit {
    let items = unit
        .items
        .into_iter()
        .map(|item| match item {
            Item::Function(mut f) => {
                if let Some(body) = f.body.take() {
                    f.body = Some(transform_block(body, rng));
                }
                Item::Function(f)
            }
            other => other,
        })
        .collect();
    TranslationUnit { items }
}

/// Rewrite a block: recurse into each statement's children, then splice a junk
/// block before it with odds `1/DEAD_ODDS`. A final splice point after the last
/// statement keeps the tail eligible too.
fn transform_block(block: Block, rng: &mut Rng) -> Block {
    let mut stmts = Vec::with_capacity(block.stmts.len());
    for stmt in block.stmts {
        if rng.below(DEAD_ODDS) == 0 {
            stmts.push(dead_block(rng));
        }
        stmts.push(transform_children(stmt, rng));
    }
    if rng.below(DEAD_ODDS) == 0 {
        stmts.push(dead_block(rng));
    }
    Block { stmts }
}

/// Recurse into the sub-statements of `stmt` so dead code reaches every nested
/// block, without splicing a junk statement in place of `stmt` itself.
///
/// A `switch` body is visited with [`transform_switch_body`] so junk is not
/// spliced among its `case` labels; every other compound statement recurses
/// straight through, and a nested `{ … }` block becomes a fresh splice site.
fn transform_children(stmt: Stmt, rng: &mut Rng) -> Stmt {
    match stmt {
        Stmt::Block(b) => Stmt::Block(transform_block(b, rng)),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => Stmt::If {
            cond,
            then_branch: Box::new(transform_children(*then_branch, rng)),
            else_branch: else_branch.map(|e| Box::new(transform_children(*e, rng))),
        },
        Stmt::While { cond, body } => Stmt::While {
            cond,
            body: Box::new(transform_children(*body, rng)),
        },
        Stmt::DoWhile { body, cond } => Stmt::DoWhile {
            body: Box::new(transform_children(*body, rng)),
            cond,
        },
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => Stmt::For {
            init,
            cond,
            step,
            body: Box::new(transform_children(*body, rng)),
        },
        Stmt::Switch { cond, body } => Stmt::Switch {
            cond,
            body: Box::new(transform_switch_body(*body, rng)),
        },
        Stmt::Case { value, body } => Stmt::Case {
            value,
            body: Box::new(transform_children(*body, rng)),
        },
        Stmt::Default { body } => Stmt::Default {
            body: Box::new(transform_children(*body, rng)),
        },
        Stmt::Label { name, body } => Stmt::Label {
            name,
            body: Box::new(transform_children(*body, rng)),
        },
        other => other,
    }
}

/// Visit a `switch` body without splicing junk among the `case` labels
/// themselves. The direct children (the labels and their trailing `break`s) are
/// recursed into so dead code still reaches each case's own statements, but no
/// new statement is inserted at this level.
fn transform_switch_body(stmt: Stmt, rng: &mut Rng) -> Stmt {
    match stmt {
        Stmt::Block(b) => {
            let stmts = b
                .stmts
                .into_iter()
                .map(|s| transform_children(s, rng))
                .collect();
            Stmt::Block(Block { stmts })
        }
        // An unbraced single-statement switch body: nothing to splice into, so
        // just recurse as usual.
        other => transform_children(other, rng),
    }
}

/// Build one `if (<opaque false>) { <junk> }` dead-code block.
fn dead_block(rng: &mut Rng) -> Stmt {
    Stmt::If {
        cond: opaque_false(rng),
        then_branch: Box::new(Stmt::Block(junk_body(rng))),
        else_branch: None,
    }
}

/// The body of a junk block: declare two throwaway `int`s, then mutate them
/// with masked bitwise/additive arithmetic that always stays in `0..=255`.
fn junk_body(rng: &mut Rng) -> Block {
    let a = format!("{JUNK_VAR}0");
    let b = format!("{JUNK_VAR}1");

    let mut stmts = vec![
        declare_int(&a, rng.range(0, 255)),
        declare_int(&b, rng.range(0, 255)),
        // _dc0 = ((_dc0 ^ k) + k) & 255;
        assign(
            &a,
            masked(add(
                bxor(ident(&a), int(rng.range(0, 255))),
                int(rng.range(0, 255)),
            )),
        ),
        // _dc1 = ((_dc1 + _dc0) ^ k) & 255;
        assign(
            &b,
            masked(bxor(add(ident(&b), ident(&a)), int(rng.range(0, 255)))),
        ),
    ];

    // A third, optional step keeps the junk's length from being a tell.
    if rng.chance() {
        stmts.push(assign(
            &a,
            masked(bxor(ident(&a), add(ident(&b), int(rng.range(0, 255))))),
        ));
    }

    Block { stmts }
}

/// `expr & 255` — clamp a value into `0..=255` so later steps cannot overflow.
fn masked(expr: Expr) -> Expr {
    band(expr, int(255))
}

fn ident(name: &str) -> Expr {
    Expr::Ident(name.to_string())
}

/// `int <name> = <value>;`
fn declare_int(name: &str, value: i64) -> Stmt {
    Stmt::Declaration(Declaration {
        ty: Type {
            specifiers: vec!["int".to_string()],
            pointers: 0,
        },
        declarators: vec![Declarator {
            name: name.to_string(),
            pointers: 0,
            array_dims: Vec::new(),
            init: Some(Expr::Int(value)),
        }],
    })
}

/// `<name> = <value>;`
fn assign(name: &str, value: Expr) -> Stmt {
    Stmt::Expr(Expr::Assign {
        op: AssignOp::Assign,
        target: Box::new(ident(name)),
        value: Box::new(value),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::parser::parse;
    use crate::parsing::printer::print_unit;
    use crate::tokenization::lexer::tokenize;

    /// Run the pass over `source` and return the printed C.
    fn inserted(source: &str, seed: u64) -> String {
        let unit = parse(tokenize(source).expect("lex")).expect("parse");
        let mut rng = Rng::new(seed);
        print_unit(&insert_dead_code(unit, &mut rng))
    }

    #[test]
    fn splices_guarded_junk_into_a_body() {
        // A body with several statements gives many splice points; across the
        // seed at least one should be taken.
        let source = r#"
int main() {
    int a = 1;
    a = a + 2;
    a = a * 3;
    a = a - 4;
    a = a + 5;
    return a;
}
"#;
        let out = inserted(source, 0x9E37_79B9_7F4A_7C15);

        // The junk declares its throwaway variable and hides behind an `if`.
        assert!(out.contains("_dc0"), "junk variable should appear:\n{out}");
        assert!(out.contains("if("), "junk must be guarded by an if:\n{out}");
        // The mask constant is a fingerprint of the junk arithmetic.
        assert!(out.contains("255"), "masked arithmetic should appear:\n{out}");
    }

    #[test]
    fn is_deterministic() {
        let source = "int main() { int a = 1; a = a + 2; return a; }";
        let first = inserted(source, 7);
        let again = inserted(source, 7);
        assert_eq!(first, again, "same seed must yield the same output");
    }

    #[test]
    fn leaves_the_real_statements_intact() {
        // Whatever junk is spliced in, the original statements survive in order.
        let source = "int main() { int a = 1; a = a + 2; return a; }";
        let out = inserted(source, 3);
        assert!(out.contains("return"), "the real return must remain:\n{out}");
    }
}
