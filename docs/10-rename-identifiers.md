# 10. Rename identifiers

**Goal:** replace every meaningful name with a confusing look-alike.

## Why names matter

Names are the single biggest aid to reading code. `totalScore`, `playerCount`,
`isValid` — each tells you what a variable is *for*. Strip those away and the
code becomes far harder to follow, even though it runs identically (the compiler
doesn't care what names you use).

## The look-alike glyph trick

skjul replaces each name with one drawn from a tiny alphabet of characters that
are nearly impossible to tell apart by eye:

- lowercase L (`l`)
- uppercase i (`I`)
- uppercase o (`O`)
- lowercase o (`o`)
- digit zero (`0`)

Every generated name starts with `_`, so results look like `_l`, `_I`, `_O`,
`_0`, `_lI`, … In many fonts these are almost indistinguishable, so a reader
can't reliably tell `_l0` from `_I0` from `_lO`.

The naming is done by counting: the *n*-th renamed identifier gets a name
derived from *n* written in that 5-character alphabet. Because that encoding is
one-to-one, two different original names never accidentally collide. And the
mapping is **consistent per name** — every use of `totalScore` becomes the same
glyph name, so the program still links up correctly.

## What is left alone

Some names *must* keep their spelling or the code won't compile. These are never
renamed:

- **C keywords** the lexer doesn't tokenize on its own (`char`, `if`, `for`,
  `while`, `struct`, `sizeof`, …).
- **The entry point `main`** and `NULL`.
- **Standard-library symbols** the program calls (`printf`, `malloc`, `strlen`,
  `fopen`, and many more).
- **Any name mentioned in a preprocessor directive** — macro names, macro
  parameters, anything a surviving `#define` or header pulls in. Directives are
  passed through verbatim, so renaming a *use* of such a name in the code body
  would break its link to the directive (for example, the call `SQ(i)` and its
  `#define SQ`).

Everything else — your variables, your functions, your parameters — gets the
glyph treatment.

*Source: [`src/main.rs`](../src/main.rs) (`rename_identifiers`, `opaque_name`,
`is_reserved`).*
