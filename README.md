# skjul

A C source-code obfuscator with a small web frontend. Paste in C, get back an equivalent but deliberately hard-to-read program (it can be reverse engineered).

Built with Rust and Axum. The full pipeline is implemented — six structural passes over the AST plus three token-level passes — and every pass preserves behaviour (the corpus tests compile the original and obfuscated programs with `gcc` and assert identical output). Widening the parser's C subset further and adding more passes are ongoing.

## Running

Make sure the C code you provide runs in gcc before obfuscating it, to avoid any errors.

```sh
cargo run
```

Then open <http://127.0.0.1:3000>, paste C into the form, and submit.

### CLI mode

If you pass a file path as an argument, skjul runs as a one-shot CLI instead of serving the web GUI: it obfuscates that file and prints the result to stdout.

```sh
cargo run main.c
```

Redirect stdout to save the output (`cargo run main.c > main.obf.c`). 

## How it obfuscates

The pipeline runs source through structural passes (on the AST) and then
token-level passes (on the re-lexed stream).

1. **Tokenize** — lex the source into a token stream.
2. **Parse** — build an AST. The supported subset covers functions and
   prototypes, the full statement and expression grammar, array-syntax
   parameters (`int marks[]`, `int grid[4]`) whose written shape is preserved,
   inline `struct`/`union`/`enum` definitions, `typedef`s (both the definition
   and later uses of the new type name), and `sizeof`. If the source falls
   outside the supported subset (e.g. bitfields or function-pointer
   declarators), skip to step 9 and obfuscate the original tokens directly.
3. **Define inlining** — substitute object-like integer `#define` constants at
   every use and drop the directive, so neither the macro name nor its value
   survives; the inlined constant is later disguised like any other literal.
4. **Include elision** — drop recognised `#include`s and splice in `extern`
   declarations for the standard symbols actually used.
5. **String encryption** — encrypt string literals and inject a runtime
   decoder, so no plaintext survives in the compiled binary.
6. **Control-flow flattening** — rewrite straight-line function bodies into a
   `while`/`switch` state machine.
7. **Opaque predicates** — wrap statements in always-true `if` guards.
8. **Dead-code insertion** — splice self-contained junk behind always-false
   guards.
9. **Print & re-lex** — print the AST back to C and re-tokenize it.
10. **Rename identifiers** — replace names (including function parameters) with
    opaque look-alike glyphs (`_l`, `_I`, `_O`, `_0`, …), consistently per source
    name.
11. **Literal disguise** — rewrite numeric literals into obscured forms.
12. **Minify** — strip the stream down to minimal whitespace.
