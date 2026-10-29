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

/// Build an integer constant expression that is always non-zero (true in a
/// C condition). All operands are small literals, so none of the arithmetic
/// can overflow.
fn opaque_true(rng: &mut Rng) -> Expr {
    let a = rng.range(1, 1000);
    let b = rng.range(1, 1000);
    match rng.below(4) {
        // a*a + b*b >= 0  — a sum of squares is never negative.
        0 => ge(add(mul(int(a), int(a)), mul(int(b), int(b))), int(0)),
        // ((a | 1) & 1) == 1  — setting then testing the low bit.
        1 => eq(band(bor(int(a), int(1)), int(1)), int(1)),
        // (a * (a + 1)) % 2 == 0  — a product of consecutive integers is even.
        2 => eq(modulo(mul(int(a), add(int(a), int(1))), int(2)), int(0)),
        // a * a >= a  — holds for every a >= 1.
        _ => ge(mul(int(a), int(a)), int(a)),
    }
}

// Small expression constructors, kept local to keep `opaque_true` readable.

fn int(n: i64) -> Expr {
    Expr::Int(n)
}

fn bin(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::Binary {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

fn add(l: Expr, r: Expr) -> Expr {
    bin(BinOp::Add, l, r)
}
fn mul(l: Expr, r: Expr) -> Expr {
    bin(BinOp::Mul, l, r)
}
fn modulo(l: Expr, r: Expr) -> Expr {
    bin(BinOp::Mod, l, r)
}
fn ge(l: Expr, r: Expr) -> Expr {
    bin(BinOp::Ge, l, r)
}
fn eq(l: Expr, r: Expr) -> Expr {
    bin(BinOp::Eq, l, r)
}
fn band(l: Expr, r: Expr) -> Expr {
    bin(BinOp::BitAnd, l, r)
}
fn bor(l: Expr, r: Expr) -> Expr {
    bin(BinOp::BitOr, l, r)
}
