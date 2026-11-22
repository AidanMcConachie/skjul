# 6. Control-flow flattening

**Goal:** hide the order in which a function's statements run.

## The idea

Normally a function reads top to bottom: statement 1, then 2, then 3. That order
is obvious and easy to follow. Flattening destroys that readability by turning
the function into a *state machine* — a loop with a `switch` that jumps between
numbered "states", where the real order is hidden in the jumps rather than in
the layout.

## Before and after

A straight-line body like this:

```c
<statement 0>
<statement 1>
<statement 2>
```

becomes a dispatcher like this:

```c
int _cf0 = 7;            // id of the first state
while (_cf0 != 2) {      // 2 means "done"
    switch (_cf0) {
        case 4: { <statement 1> _cf0 = 9; break; }
        case 7: { <statement 0> _cf0 = 4; break; }
        case 9: { <statement 2> _cf0 = 2; break; }
    }
}
```

Each original statement becomes one `case`. The important tricks:

- The state ids (`4`, `7`, `9`, …) are **shuffled** random numbers, not `1, 2,
  3`.
- The `case` arms are emitted in **shuffled order**, so their position in the
  `switch` tells you nothing about execution order.
- The only thing that reveals the true order is the chain of `_cf0 = <next id>`
  assignments — and that chain preserves the original order *exactly*, so
  behaviour is unchanged.

A reader now has to trace the `_cf0` assignments by hand to reconstruct what the
function really does.

## What stays intact

Compound statements — an `if`, a `for`, a `while`, a nested block, a whole inner
`switch` — are kept whole inside their single state. Their internal logic,
including any `break`, `continue`, or `return`, is left untouched.

## Playing it safe

The pass leaves a function unchanged whenever flattening could change behaviour:

- A body with fewer than two top-level statements (nothing to gain).
- A top-level jump or label (`break`, `continue`, `goto`, a label, or a
  `case`/`default` prefix), since splitting these across states would break the
  jump.
- A declaration whose initializer can't be safely separated (an aggregate
  `{ … }` initializer, or an array with an initializer).

Local declarations that *can* be handled are hoisted above the loop (so the
variable names stay in scope for every state), and their simple initializers
become ordinary assignments inside the right state — preserving both scope and
the order of any side effects.

*Source: [`src/transformation/flatten.rs`](../src/transformation/flatten.rs).*
