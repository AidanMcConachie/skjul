//! Structural obfuscation passes that rewrite the [`crate::parsing::ast`] tree
//! before it is printed back to C.
//!
//! Unlike the token-level passes (rename, literal-disguise, minify), these
//! transforms change the *shape* of the program while preserving its
//! behaviour. Each pass is deliberately conservative: when a construct would
//! be unsafe to rewrite it is left untouched, so the output always compiles to
//! an equivalent program.
//!
//! Three passes exist so far:
//!
//! * [`flatten`] turns a function's straight-line body into a
//!   `while`/`switch` state machine (control-flow flattening).
//! * [`opaque`] wraps statements in always-true `if` guards (opaque
//!   predicates).
//! * [`deadcode`] splices self-contained junk behind always-false `if` guards
//!   (dead-code insertion).
//!
//! The opaque-true and opaque-false predicates both passes rely on live in
//! [`predicates`].

pub mod deadcode;
pub mod flatten;
pub mod opaque;
pub mod predicates;

use crate::parsing::ast::TranslationUnit;

/// Run every structural pass over a translation unit, in order.
///
/// Flattening runs first so the dispatcher it introduces is itself eligible
/// for opaque-predicate wrapping. Dead-code insertion runs last so its junk
/// blocks are not themselves wrapped in opaque guards, keeping each pass's
/// contribution to the output distinct.
pub fn transform(unit: TranslationUnit, rng: &mut Rng) -> TranslationUnit {
    let unit = flatten::flatten_unit(unit, rng);
    let unit = opaque::insert_opaque_predicates(unit, rng);
    deadcode::insert_dead_code(unit, rng)
}

/// A tiny counter-based pseudo-random generator (a linear congruential
/// generator with Knuth's MMIX constants). It exists only to vary the shape of
/// the output deterministically — the same seed always yields the same
/// program — not for any statistical or cryptographic quality.
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        // The high bits of an LCG are the well-mixed ones.
        self.state >> 16
    }

    /// A value in `0..n` (`n` must be non-zero).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    /// A coin flip.
    pub fn chance(&mut self) -> bool {
        self.next_u64() % 2 == 0
    }

    /// A value in `lo..=hi` (requires `lo <= hi`).
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        let span = (hi - lo) as u64 + 1;
        lo + (self.next_u64() % span) as i64
    }
}
