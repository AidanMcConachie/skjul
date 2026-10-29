//! Control-flow flattening.
//!
//! A function's straight-line body is rewritten into a dispatcher:
//!
//! ```c
//! int _cf0 = 0;
//! while (_cf0 != N) {
//!     switch (_cf0) {
//!         case 0: { <stmt 0> _cf0 = 1; break; }
//!         case 1: { <stmt 1> _cf0 = 2; break; }
//!         ...
//!         case N-1: { <stmt N-1> _cf0 = N; break; }
//!     }
//! }
//! ```
//!
//! Each original top-level statement becomes one state; the state variable
//! threads them together, so execution order is unchanged. Compound
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

fn flatten_function(mut f: Function, _rng: &mut Rng) -> Function {
    let Some(body) = f.body.take() else {
        return f; // a prototype
    };
    // `try_flatten` returns the original block unchanged on a bail-out.
    f.body = Some(try_flatten(body));
    f
}

fn try_flatten(body: Block) -> Block {
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

    let end = states.len() as i64;

    // Each state's block ends by advancing the state variable and breaking out
    // of the dispatch `switch`.
    let cases = states
        .into_iter()
        .enumerate()
        .map(|(i, mut stmts)| {
            let next = i as i64 + 1; // last state advances to `end`, ending the loop
            stmts.push(assign_state(next));
            stmts.push(Stmt::Break);
            Stmt::Case {
                value: Expr::Int(i as i64),
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
    stmts.push(state_declaration());
    stmts.push(dispatcher);
    Block { stmts }
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

/// `int _cf0 = 0;`
fn state_declaration() -> Stmt {
    Stmt::Declaration(Declaration {
        ty: Type {
            specifiers: vec!["int".to_string()],
            pointers: 0,
        },
        declarators: vec![Declarator {
            name: STATE_VAR.to_string(),
            pointers: 0,
            array_dims: Vec::new(),
            init: Some(Expr::Int(0)),
        }],
    })
}
