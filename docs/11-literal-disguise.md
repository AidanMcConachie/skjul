# 11. Literal disguise

**Goal:** rewrite the constant values in the code so they're no longer readable
at a glance — while computing to exactly the same values.

A *literal* is a value written directly in the source: a number like `10`, a
character like `'A'`, or a string like `"hi"`. skjul handles all three.

## Integers

An integer literal is replaced by an arithmetic expression that evaluates to the
same number:

```c
10   →   (23 ^ 29)
```

Here `^` is bitwise XOR, and `23 ^ 29` really does equal `10`. A reader now has
to do the math to recover the value. Crucially, **both operands are literals**,
so the whole thing is still a *constant expression* — which means it's legal
everywhere a plain number was allowed: `case` labels, array sizes, enum values,
and so on.

## Characters

A character constant like `'A'` has type `int` in C, so `'A'` and `65` are
interchangeable. skjul converts the character to its numeric value and then runs
it through the same integer scrambler above, so `'A'` might become something
like `(70 ^ 7)`.

One caution: a byte value of 128 or more is negative on platforms where `char`
is signed, so turning it into a positive number would change its meaning. Those
(and multi-byte constants, which are implementation-defined) are left untouched.

## Strings

Each byte of a string is re-encoded as an escape sequence — either three-digit
octal (`\150`) or two-digit hex (`\x68`), chosen per byte:

```c
"hi"   →   "\150\x69"
```

The bytes are identical, so the program prints the same text; only the readable
spelling is gone. (This is a *lighter* disguise than step 5's string encryption,
which removes the plaintext from the binary entirely. Both can apply: encryption
rewrites executable-position strings into decoder calls, and this pass re-encodes
whatever string literals remain.)

## Deterministic and safe

The disguises come from a fixed-seed counter, so the same input always produces
the same output, and repeated literals still get varied forms. Every rewritten
value is wrapped in parentheses and emitted as raw text, which keeps operator
precedence correct and stops the new text from fusing with a neighbouring token
when the stream is minified in the next step.

*Source: [`src/obfuscation/literals.rs`](../src/obfuscation/literals.rs).*
