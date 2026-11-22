# 12. Minify

**Goal:** strip the code down to the minimum whitespace, so it becomes one dense,
unreadable blob.

## What minifying does

Humans rely on indentation, blank lines, and spacing to see a program's
structure. Minifying throws all of that away: every space, newline, and comment
the author wrote is removed. The result is a wall of text that is valid C and
compiles fine, but is miserable to read:

```c
int _l(int _I){int _O=(23^29);while(_O!=2){switch(_O){case 4:...
```

## Keeping it correct

You can't *blindly* delete all whitespace, because sometimes a space is the only
thing keeping two tokens apart. The minifier inserts the minimum separators
needed to preserve meaning. It adds a single space only when removing it would
let two tokens fuse into a *different* token:

- **Two word-like tokens:** `int x` must not become `intx` (one identifier).
- **Symbol pairs that would form a longer operator or a comment:** `a / /b` must
  not become `a//b` (a line comment), and `+ +` must not become `++`
  (increment). The guarded pairs are `//`, `/*`, `*/`, `++`, and `--`.

Everywhere else, the space is dropped.

## Directives get their own line

The C preprocessor is line-based, so a directive (like a surviving `#include` or
`#define`) must stand alone on its own line. The minifier gives each directive a
newline before and after, so code never trails onto a directive's line and the
directive never shares a line with the code around it.

## The end of the pipeline

This is the last step. After minifying, skjul hands back the finished string —
either printed to the terminal (CLI mode) or shown in the web page. The program
inside is behaviourally identical to what you put in, but about as hard to read
as skjul can make it.

*Source: [`src/main.rs`](../src/main.rs) (`minify`, `needs_separator`).*
