# skjul documentation

This folder explains, one step at a time, what skjul does to your C code when it
obfuscates it. If you have read the **How it obfuscates** list in the top-level
[README](../README.md) and want more detail, you are in the right place.

## The big picture

skjul takes a working C program and turns it into a different program that does
**exactly the same thing** but is much harder for a human to read. It never
changes what the program *does* — only how it *looks*. (The corpus tests prove
this: they compile the original and the obfuscated version with `gcc` and check
that both print the same output.)

The work happens in two halves:

1. **Structural passes** work on an *AST* — a tree that represents the shape of
   your program (functions, loops, `if` statements, and so on). These passes can
   rearrange and rewrite whole chunks of logic.
2. **Token-level passes** work on a flat *stream of tokens* — the individual
   words and symbols of the code (`int`, `x`, `=`, `5`, `;`). These passes
   rename and disguise things without understanding the structure.

If skjul can't fully understand your program (it only supports a subset of C),
it safely skips the structural passes and still runs the token-level ones, so
you always get *some* obfuscation back.

## The passes, in order

Each file below documents one numbered step from the README list. They are meant
to be read top to bottom, because each step feeds into the next.

| # | Step | What it does |
|---|------|--------------|
| 1 | [Tokenize](01-tokenize.md) | Break the text into tokens |
| 2 | [Parse](02-parse.md) | Build the AST (program tree) |
| 3 | [Define inlining](03-define-inlining.md) | Fold `#define` constants into the code |
| 4 | [Include elision](04-include-elision.md) | Remove `#include`s, add `extern`s |
| 5 | [String encryption](05-string-encryption.md) | Hide text so it isn't in the binary |
| 6 | [Control-flow flattening](06-control-flow-flattening.md) | Scramble statement order into a state machine |
| 7 | [Opaque predicates](07-opaque-predicates.md) | Wrap code in always-true `if`s |
| 8 | [Dead-code insertion](08-dead-code-insertion.md) | Add junk code that never runs |
| 9 | [Print & re-lex](09-print-and-re-lex.md) | Turn the tree back into text and re-tokenize |
| 10 | [Rename identifiers](10-rename-identifiers.md) | Replace names with look-alike glyphs |
| 11 | [Literal disguise](11-literal-disguise.md) | Rewrite numbers and strings |
| 12 | [Minify](12-minify.md) | Strip all unnecessary whitespace |

Steps 1–2 read the code, steps 3–8 are the structural passes, step 9 is the
bridge back to text, and steps 10–12 are the token-level passes.
