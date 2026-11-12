//! Opaque-predicate insertion.
//!
//! Each eligible statement inside a block is, with even odds, wrapped in an
//! `if (P) { … }` whose condition `P` is an expression that always evaluates to
//! true but is not obviously constant at a glance. The guarded statement still
//! runs exactly as before, so behaviour is unchanged; a reader (or a naive
//! tool) must evaluate `P` to see that the branch is unconditional.
//!
//! Only statements that are safe to wrap are touched. Declarations are never
//! wrapped — moving `int x = …;` inside an `if` block would scope `x` to that
//! block and hide it from the statements that follow. Labels, `case`/`default`
//! prefixes and `goto` are likewise left alone so jump structure is preserved.
//!
//! Every predicate is a constant integer expression built from small literals,
//! chosen so it cannot overflow and is provably non-zero.

use super::predicates::opaque_true;
use super::Rng;
use crate::parsing::ast::*;

/// Wrap eligible statements throughout every function body with opaque-true
/// `if` guards.
pub fn insert_opaque_predicates(unit: TranslationUnit, rng: &mut Rng) -> TranslationUnit {
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

/// Rewrite a block: recurse into each statement's children, then optionally
/// wrap the whole statement in an opaque guard.
fn transform_block(block: Block, rng: &mut Rng) -> Block {
    let stmts = block
        .stmts
        .into_iter()
        .map(|stmt| {
            let stmt = transform_children(stmt, rng);
            if is_wrappable(&stmt) && rng.chance() {
                wrap_opaque(stmt, rng)
            } else {
                stmt
            }
        })
        .collect();
    Block { stmts }
}

/// Recurse into the sub-statements of `stmt` (loop/branch bodies, nested
/// blocks) without wrapping `stmt` itself.
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
            body: Box::new(transform_children(*body, rng)),
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

/// Whether a statement may be safely wrapped in an `if` guard. Declarations
/// and jump-structure statements are excluded (see the module docs). `return`
/// is excluded too: guarding a function's only `return` would leave a path
/// that falls off the end of a non-void function (correct, since the guard is
/// always true, but it needlessly provokes a compiler warning).
fn is_wrappable(stmt: &Stmt) -> bool {
    matches!(
        stmt,
        Stmt::Expr(_)
            | Stmt::If { .. }
            | Stmt::While { .. }
            | Stmt::DoWhile { .. }
            | Stmt::For { .. }
            | Stmt::Block(_)
            | Stmt::Switch { .. }
    )
}

/// Wrap `stmt` as `if (<opaque true>) { stmt }`.
fn wrap_opaque(stmt: Stmt, rng: &mut Rng) -> Stmt {
    Stmt::If {
        cond: opaque_true(rng),
        then_branch: Box::new(Stmt::Block(Block { stmts: vec![stmt] })),
        else_branch: None,
    }
}

