# skjul

A C source-code obfuscator with a small web frontend. Paste in C, get back an equivalent but deliberately hard-to-read program (it can be reverse engineered).

Built with Rust and Axum. The basic implementation is done; several structural passes and other features are still in progress 

## Running

Make sure the C code you provide runs in gcc before obfuscating it, to avoid any errors.

```sh
cargo run
```

Then open <http://127.0.0.1:3000>, paste C into the form, and submit.

## How it obfuscates

The pipeline runs source through structural passes (on the AST) and then
token-level passes (on the re-lexed stream).

1. **Tokenize** — lex the source into a token stream.
2. **Parse** — build an AST. If the source falls outside the supported subset,
   skip to step 8 and obfuscate the original tokens directly.
3. **Include elision** — drop recognised `#include`s and splice in `extern`
   declarations for the standard symbols actually used.
4. **String encryption** — encrypt string literals and inject a runtime
   decoder, so no plaintext survives in the compiled binary.
5. **Control-flow flattening** — rewrite straight-line function bodies into a
   `while`/`switch` state machine.
6. **Opaque predicates** — wrap statements in always-true `if` guards.
7. **Dead-code insertion** — splice self-contained junk behind always-false
   guards.
8. **Print & re-lex** — print the AST back to C and re-tokenize it.
9. **Rename identifiers** — replace names with opaque look-alike glyphs
   (`_l`, `_I`, `_O`, `_0`, …), consistently per source name.
10. **Literal disguise** — rewrite numeric literals into obscured forms.
11. **Minify** — strip the stream down to minimal whitespace.
