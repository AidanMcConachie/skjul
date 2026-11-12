//! Opaque predicates and the small expression constructors they are built
//! from, shared by the structural passes.
//!
//! An *opaque predicate* is an expression whose value is fixed — always true,
//! or always false — but not obviously so at a glance: a reader (or a naive
//! tool) must evaluate it to learn which. [`opaque_true`] feeds the
//! opaque-predicate pass, which guards real statements with always-true `if`s;
//! [`opaque_false`] feeds the dead-code pass, which hides junk behind
//! always-false `if`s.
//!
//! Every predicate is an integer constant expression built from small literals
//! (at most `1000`), chosen so none of the arithmetic can overflow a 32-bit
//! `int` and so its truth value is provable by inspection.

use super::Rng;
use crate::parsing::ast::*;

/// Build an integer constant expression that is always non-zero — true in a C
/// condition. All operands are small literals, so none of the arithmetic can
/// overflow.
pub fn opaque_true(rng: &mut Rng) -> Expr {
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

/// Build an integer constant expression that is always zero — false in a C
/// condition. Each variant is the exact negation of an [`opaque_true`] one, so
/// it is provably false by the same reasoning and shares its overflow safety.
pub fn opaque_false(rng: &mut Rng) -> Expr {
    let a = rng.range(1, 1000);
    let b = rng.range(1, 1000);
    match rng.below(4) {
        // a*a + b*b < 0  — a sum of squares is never negative.
        0 => lt(add(mul(int(a), int(a)), mul(int(b), int(b))), int(0)),
        // ((a | 1) & 1) == 0  — the low bit was just set, so it is never 0.
        1 => eq(band(bor(int(a), int(1)), int(1)), int(0)),
        // (a * (a + 1)) % 2 == 1  — a product of consecutive integers is even.
        2 => eq(modulo(mul(int(a), add(int(a), int(1))), int(2)), int(1)),
        // a * a < a  — false for every a >= 1.
        _ => lt(mul(int(a), int(a)), int(a)),
    }
}

// Small expression constructors, kept here so both opaque predicates and the
// dead-code junk can build arithmetic without repeating the boilerplate.

pub fn int(n: i64) -> Expr {
    Expr::Int(n)
}

pub fn bin(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::Binary {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

pub fn add(l: Expr, r: Expr) -> Expr {
    bin(BinOp::Add, l, r)
}
pub fn mul(l: Expr, r: Expr) -> Expr {
    bin(BinOp::Mul, l, r)
}
pub fn modulo(l: Expr, r: Expr) -> Expr {
    bin(BinOp::Mod, l, r)
}
pub fn ge(l: Expr, r: Expr) -> Expr {
    bin(BinOp::Ge, l, r)
}
pub fn lt(l: Expr, r: Expr) -> Expr {
    bin(BinOp::Lt, l, r)
}
pub fn eq(l: Expr, r: Expr) -> Expr {
    bin(BinOp::Eq, l, r)
}
pub fn band(l: Expr, r: Expr) -> Expr {
    bin(BinOp::BitAnd, l, r)
}
pub fn bor(l: Expr, r: Expr) -> Expr {
    bin(BinOp::BitOr, l, r)
}
pub fn bxor(l: Expr, r: Expr) -> Expr {
    bin(BinOp::BitXor, l, r)
}
