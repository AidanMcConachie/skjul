# 8. Dead-code insertion

**Goal:** add realistic-looking junk code that never actually runs.

## The idea

This pass is the mirror image of the previous one. Where opaque predicates wrap
*real* code in an always-**true** guard, dead-code insertion hides *fake* code
behind an always-**false** guard. The junk looks like genuine computation, so a
reader has to study it before realising it does nothing.

## What gets inserted

Between the real statements of a block, skjul splices blocks like this:

```c
if (<always false>) {
    int _dc0 = 37;
    int _dc1 = 12;
    _dc0 = ((_dc0 ^ 90) + 15) & 255;
    _dc1 = ((_dc1 + _dc0) ^ 46) & 255;
}
```

The guard is provably false, so the body never executes and behaviour is
unchanged. But to *know* it's dead, a reader must evaluate the predicate — and
meanwhile the junk adds plausible statements and extra variable names that the
later **Rename** pass (step 10) will blur together with the real ones, so the
fakes are indistinguishable from the real variables.

Only about one in three candidate spots actually gets junk, keeping the added
bulk moderate (especially since it compounds with the opaque-predicate pass).

## Safe by construction

The inserted code can't cause trouble, for two independent reasons:

- **It's fully self-contained.** Every variable it uses, it also declares inside
  the guard's own block. So it can drop into any scope without clashing with — or
  depending on — the surrounding code, and it produces no output or effect
  visible from outside.
- **Even if the guard somehow ran**, the junk still couldn't misbehave: every
  value is masked back into the range `0..=255` after each step, so the
  arithmetic can't overflow or divide, and it only ever touches its own local
  variables.

## Where it goes

Junk is spliced only between the statements of a genuine `{ … }` block, never
directly inside a `switch` body (where a loose statement between `case` labels
would be needlessly fragile). The code *inside* each `case` is still visited, so
dead code reaches those places too.

*Source: [`src/transformation/deadcode.rs`](../src/transformation/deadcode.rs).*
