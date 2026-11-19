//! Structural obfuscation passes that rewrite the [`crate::parsing::ast`] tree
//! before it is printed back to C.
//!
//! Unlike the token-level passes (rename, literal-disguise, minify), these
//! transforms change the *shape* of the program while preserving its
//! behaviour. Each pass is deliberately conservative: when a construct would
//! be unsafe to rewrite it is left untouched, so the output always compiles to
//! an equivalent program.
//!
//! Six passes exist so far:
//!
//! * [`defines`] inlines object-like integer `#define` constants at every use
//!   and drops the directive (define inlining), so neither the macro name nor
//!   its value survives — the value is folded into the code as a plain literal
//!   for the later literal-disguise pass to hide.
//! * [`includes`] drops recognised `#include` directives, splicing in `extern`
//!   declarations for the standard symbols the program uses (include elision),
//!   so the output no longer advertises its dependencies in a header block.
//! * [`strings`] encrypts string literals and injects a runtime decoder, so no
//!   plaintext survives in the compiled binary (string encryption).
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
pub mod defines;
pub mod flatten;
pub mod includes;
pub mod opaque;
pub mod predicates;
pub mod strings;

use crate::parsing::ast::TranslationUnit;

/// Run every structural pass over a translation unit, in order.
///
/// Define inlining and include elision run first, rewriting only top-level
/// directives (and the literal uses of inlined macros) before any
/// statement-level pass touches the tree. String encryption runs next, so the
/// decoder calls it leaves behind are
/// ordinary expressions by the time the later passes run — flattening can split
/// a string-bearing statement into its own state, and opaque predicates can
/// wrap it — while the injected decoder (an [`Item::Raw`] block, not a
/// [`Function`](crate::parsing::ast::Function)) is invisible to all of them and
/// stays intact. Flattening runs next so the dispatcher it introduces is itself
/// eligible for opaque-predicate wrapping. Dead-code insertion runs last so its
/// junk blocks are not themselves wrapped in opaque guards, keeping each pass's
/// contribution to the output distinct.
pub fn transform(unit: TranslationUnit, rng: &mut Rng) -> TranslationUnit {
    // Define inlining and include elision run first: they only rewrite
    // top-level directives (and, for inlining, the macro's literal uses), so
    // doing them up front keeps them clear of the statement-level passes and
    // lets the spliced `extern` declarations and inlined constants flow through
    // those passes like any other code. Inlining runs before elision so a macro
    // used only inside an elided header's prototypes is already resolved.
    let unit = defines::inline_defines(unit);
    let unit = includes::elide_includes(unit);
    let unit = strings::encrypt_strings(unit, rng);
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
        self.next_u64().is_multiple_of(2)
    }

    /// A value in `lo..=hi` (requires `lo <= hi`).
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        let span = (hi - lo) as u64 + 1;
        lo + (self.next_u64() % span) as i64
    }
}
