# 7. Opaque predicates

**Goal:** wrap real code in `if` statements that *look* conditional but are
always true.

## What is an opaque predicate?

A *predicate* is just a condition — the test inside an `if`. An **opaque**
predicate is one whose value is fixed (here, always true) but isn't obvious at a
glance. For example, `if (x*x >= 0)` is always true for any integer `x`, but a
reader has to stop and reason about it to be sure.

## What this pass does

For each statement that is safe to wrap, with a 50/50 chance, skjul surrounds it
with an always-true guard:

```c
// before
total = total + 1;

// after
if (P) { total = total + 1; }
```

where `P` is an expression that always evaluates to true but isn't plainly a
constant. The guarded statement still always runs, so behaviour is unchanged —
but a human reader (or a naive analysis tool) now has to evaluate `P` just to
confirm the branch is unconditional. Scattering these throughout the code buries
the real logic in a thicket of fake decisions.

The predicates are built from small literals, chosen so they can't overflow and
are provably non-zero (true). The always-true and always-false predicate
builders live in
[`src/transformation/predicates.rs`](../src/transformation/predicates.rs); the
always-false ones are used by the next pass, **Dead-code insertion**.

## Playing it safe

Only statements that are safe to wrap are touched:

- **Declarations are never wrapped.** Moving `int x = …;` inside an `if` block
  would scope `x` to that block and hide it from the statements that follow,
  changing the program.
- **Labels, `case`/`default` prefixes, and `goto` are left alone**, so the
  program's jump structure is preserved.

*Source: [`src/transformation/opaque.rs`](../src/transformation/opaque.rs).*
