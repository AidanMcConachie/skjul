//! Control-flow flattening.
//!
//! A function's straight-line body is rewritten into a dispatcher:
//!
//! ```c
//! int _cf0 = 7;            // id of the first state
//! while (_cf0 != 2) {      // 2 is a distinct "done" id
//!     switch (_cf0) {
//!         case 4: { <stmt 1> _cf0 = 9; break; }
//!         case 7: { <stmt 0> _cf0 = 4; break; }
//!         case 9: { <stmt 2> _cf0 = 2; break; }
//!     }
//! }
//! ```
//!
//! Each original top-level statement becomes one state. The states are given
//! distinct, shuffled ids from a single permutation, and the `case` arms are
//! emitted in a shuffled order, so neither the labels nor their position in
//! the `switch` reveals the real execution order — only the chain of
//! `_cf0 = <next id>` transitions threads the statements together, and it
//! preserves that order exactly. Compound
//! statements (`if`, `for`, `while`, nested blocks, a whole `switch`) are kept
//! intact inside their state — their internal control flow, including any
//! `break`/`continue`/`return`, is untouched.
//!
//! The pass is intentionally conservative and bails out (leaving the function
//! unchanged) whenever flattening could change behaviour:
//!
//! * A body with fewer than two top-level statements — nothing to gain.
//! * A top-level jump or label (`break`, `continue`, `goto`, a label, or a
//!   `case`/`default` prefix) — splitting these across states would break the
//!   jump or the surrounding `switch`.
//! * A declaration whose initialiser cannot be split off safely (an aggregate
//!   `{ … }` initialiser, or an array with an initialiser) — these must stay
//!   part of the declaration.
//!
//! Local declarations that *can* be handled are hoisted above the loop (so the
//! names stay in scope for every state) and their scalar initialisers become
//! ordinary assignments inside the owning state, preserving both scope and the
//! order of any side effects.

use super::Rng;
use crate::parsing::ast::*;

/// Name of the injected state variable. Renaming happens later over the whole
/// token stream, so this only needs to be unlikely to clash with user code.
const STATE_VAR: &str = "_cf0";

/// Flatten every function body in the unit.
pub fn flatten_unit(unit: TranslationUnit, rng: &mut Rng) -> TranslationUnit {
    let items = unit
        .items
        .into_iter()
        .map(|item| match item {
            Item::Function(f) => Item::Function(flatten_function(f, rng)),
            other => other,
        })
        .collect();
    TranslationUnit { items }
}

fn flatten_function(mut f: Function, rng: &mut Rng) -> Function {
    let Some(body) = f.body.take() else {
        return f; // a prototype
    };
    // `try_flatten` returns the original block unchanged on a bail-out.
    f.body = Some(try_flatten(body, rng));
    f
}

fn try_flatten(body: Block, rng: &mut Rng) -> Block {
    if body.stmts.len() < 2 {
        return body;
    }

    // Pre-pass (borrowing): confirm every top-level statement is safe to
    // flatten before consuming anything, so a bail-out can return the block
    // untouched.
    for stmt in &body.stmts {
        match stmt {
            Stmt::Break
            | Stmt::Continue
            | Stmt::Goto(_)
            | Stmt::Label { .. }
            | Stmt::Case { .. }
            | Stmt::Default { .. } => return body,
            Stmt::Declaration(decl) if !is_hoistable(decl) => return body,
            _ => {}
        }
    }

    // Build one state per top-level statement, hoisting declarations.
    let mut hoisted: Vec<Stmt> = Vec::new();
    let mut states: Vec<Vec<Stmt>> = Vec::new();
    for stmt in body.stmts {
        match stmt {
            Stmt::Declaration(decl) => {
                let (hoist_decl, assigns) = hoist_declaration(decl);
                hoisted.push(Stmt::Declaration(hoist_decl));
                states.push(assigns);
            }
            other => states.push(vec![other]),
        }
    }

    // Assign each state position a distinct, shuffled id so the `case` labels
    // and `_cf0 = …` transitions no longer reveal execution order. `ids[pos]`
    // is the id of position `pos`; `end` is a further distinct id that stops
    // the loop. All come from one permutation, so every value is unique — C
    // forbids duplicate `case` labels.
    let n = states.len();
    let perm = random_permutation(n + 1, rng);
    let ids: Vec<i64> = (0..n).map(|pos| perm[pos] as i64).collect();
    let end = perm[n] as i64;
    let entry = ids[0];

    // Emit the `case` arms in a shuffled order too; each carries its own id, so
    // the order they appear in the `switch` is irrelevant to behaviour.
    let mut emit_order: Vec<usize> = (0..n).collect();
    shuffle(&mut emit_order, rng);

    // Each state's block ends by advancing the state variable to the next id
    // (or `end` for the last position) and breaking out of the dispatch
    // `switch`.
    let mut states = states;
    let cases = emit_order
        .into_iter()
        .map(|pos| {
            let mut stmts = std::mem::take(&mut states[pos]);
            let next = if pos + 1 < n { ids[pos + 1] } else { end };
            stmts.push(assign_state(next));
            stmts.push(Stmt::Break);
            Stmt::Case {
                value: Expr::Int(ids[pos]),
                body: Box::new(Stmt::Block(Block { stmts })),
            }
        })
        .collect();

    let switch = Stmt::Switch {
        cond: Expr::Ident(STATE_VAR.to_string()),
        body: Box::new(Stmt::Block(Block { stmts: cases })),
    };

    let dispatcher = Stmt::While {
        cond: Expr::Binary {
            op: BinOp::Ne,
            lhs: Box::new(Expr::Ident(STATE_VAR.to_string())),
            rhs: Box::new(Expr::Int(end)),
        },
        body: Box::new(Stmt::Block(Block {
            stmts: vec![switch],
        })),
    };

    let mut stmts = hoisted;
    stmts.push(state_declaration(entry));
    stmts.push(dispatcher);
    Block { stmts }
}

/// Fisher–Yates shuffle, driven by the deterministic generator.
fn shuffle<T>(items: &mut [T], rng: &mut Rng) {
    for i in (1..items.len()).rev() {
        let j = rng.below(i as u64 + 1) as usize;
        items.swap(i, j);
    }
}

/// A random permutation of `0..len`.
fn random_permutation(len: usize, rng: &mut Rng) -> Vec<usize> {
    let mut items: Vec<usize> = (0..len).collect();
    shuffle(&mut items, rng);
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::parser::parse;
    use crate::tokenization::lexer::tokenize;

    /// Flatten the first function in `source` and return its new body.
    fn flattened_body(source: &str, seed: u64) -> Block {
        let unit = parse(tokenize(source).expect("lex")).expect("parse");
        let mut rng = Rng::new(seed);
        let unit = flatten_unit(unit, &mut rng);
        match unit.items.into_iter().find_map(|i| match i {
            Item::Function(f) => f.body,
            _ => None,
        }) {
            Some(body) => body,
            None => panic!("no function body"),
        }
    }

    /// The entry id (`_cf0`'s initialiser) and the `case` label ids, read out
    /// of a flattened body in the order the arms are emitted.
    fn dispatcher_ids(body: &Block) -> (i64, Vec<i64>) {
        let entry = body.stmts.iter().find_map(|s| match s {
            Stmt::Declaration(d) if d.declarators[0].name == STATE_VAR => {
                match &d.declarators[0].init {
                    Some(Expr::Int(n)) => Some(*n),
                    _ => None,
                }
            }
            _ => None,
        });
        let switch_body = body.stmts.iter().find_map(|s| match s {
            Stmt::While { body, .. } => match body.as_ref() {
                Stmt::Block(b) => b.stmts.iter().find_map(|s| match s {
                    Stmt::Switch { body, .. } => Some(body.as_ref()),
                    _ => None,
                }),
                _ => None,
            },
            _ => None,
        });
        let cases = match switch_body {
            Some(Stmt::Block(b)) => b
                .stmts
                .iter()
                .filter_map(|s| match s {
                    Stmt::Case {
                        value: Expr::Int(n),
                        ..
                    } => Some(*n),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        (entry.expect("state var"), cases)
    }

    #[test]
    fn flattens_into_a_dispatcher_with_distinct_shuffled_ids() {
        // A straight-line body long enough that a shuffle is observable.
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
        let body = flattened_body(source, 0x9E37_79B9_7F4A_7C15);
        let (entry, cases) = dispatcher_ids(&body);

        // Six top-level statements → six states, each a distinct `case` id.
        assert_eq!(cases.len(), 6, "one case per original statement");
        let mut sorted = cases.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 6, "case ids must be distinct");

        // The emitted ids are genuinely shuffled, not 0,1,2,… in order, and the
        // entry is not the trivial 0.
        let sequential: Vec<i64> = (0..6).collect();
        assert_ne!(cases, sequential, "case ids should not be sequential");
        assert_ne!(entry, 0, "entry id should not be the trivial 0");
    }

    #[test]
    fn leaves_a_trivial_body_untouched() {
        // A single-statement body has nothing to flatten.
        let body = flattened_body("int f() { return 0; }", 1);
        assert_eq!(body.stmts.len(), 1);
        assert!(matches!(body.stmts[0], Stmt::Return(_)));
    }
}

/// Whether every declarator in a declaration can be hoisted: an aggregate
/// `{ … }` initialiser, or an array that carries an initialiser, cannot be
/// turned into a later assignment and so blocks the hoist.
fn is_hoistable(decl: &Declaration) -> bool {
    decl.declarators.iter().all(|d| match &d.init {
        Some(Expr::InitList(_)) => false,
        Some(_) => d.array_dims.is_empty(),
        None => true,
    })
}

/// Split a declaration into a hoisted declaration (initialisers removed) and
/// the assignment statements that reproduce the scalar initialisers in place.
fn hoist_declaration(decl: Declaration) -> (Declaration, Vec<Stmt>) {
    let mut assigns = Vec::new();
    let declarators = decl
        .declarators
        .into_iter()
        .map(|d| {
            if let Some(init) = d.init {
                assigns.push(Stmt::Expr(Expr::Assign {
                    op: AssignOp::Assign,
                    target: Box::new(Expr::Ident(d.name.clone())),
                    value: Box::new(init),
                }));
            }
            Declarator {
                name: d.name,
                pointers: d.pointers,
                array_dims: d.array_dims,
                init: None,
            }
        })
        .collect();
    (
        Declaration {
            ty: decl.ty,
            declarators,
        },
        assigns,
    )
}

/// `_cf0 = value;`
fn assign_state(value: i64) -> Stmt {
    Stmt::Expr(Expr::Assign {
        op: AssignOp::Assign,
        target: Box::new(Expr::Ident(STATE_VAR.to_string())),
        value: Box::new(Expr::Int(value)),
    })
}

/// `int _cf0 = <entry>;`
fn state_declaration(entry: i64) -> Stmt {
    Stmt::Declaration(Declaration {
        ty: Type {
            specifiers: vec!["int".to_string()],
            pointers: 0,
        },
        declarators: vec![Declarator {
            name: STATE_VAR.to_string(),
            pointers: 0,
            array_dims: Vec::new(),
            init: Some(Expr::Int(entry)),
        }],
    })
}
