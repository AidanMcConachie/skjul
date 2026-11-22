# 9. Print & re-lex

**Goal:** turn the transformed AST back into C text, then tokenize it again.

## Why this step exists

Steps 3–8 rewrote the *tree* (the AST). But the remaining passes — rename,
literal disguise, minify — work on a flat *token stream*, not a tree. This step
is the bridge between the two halves of skjul:

1. **Print** — walk the transformed AST and write it back out as ordinary C
   source text. This is sometimes called "unparsing": it's the exact reverse of
   the parse step, producing a string of valid C that reflects all the
   structural changes (flattening, opaque predicates, dead code, and so on).
2. **Re-lex** — run that freshly printed C back through the tokenizer (the same
   lexer from step 1), producing a new, clean token stream.

## The two paths meet here

Recall from step 2 that programs can arrive here two ways:

- **Parsed path:** the program fit skjul's C subset, went through all the
  structural passes, and is now printed back to text and re-tokenized.
- **Fallback path:** the program *didn't* fit the subset, so the structural
  passes were skipped entirely — but it was still tokenized back in step 1.

Either way, by the end of this step we have a plain token stream ready for the
token-level passes. Re-lexing guarantees that stream is consistent and
well-formed, regardless of which path the code took to get here.

## Nothing is hidden yet

Importantly, this step changes *presentation*, not *secrecy*. The printed code
is still perfectly readable — the real disguising of names and values happens in
the three passes that follow (steps 10–12).

*Source: printing in [`src/parsing/printer.rs`](../src/parsing/printer.rs),
re-lexing in [`src/tokenization/lexer.rs`](../src/tokenization/lexer.rs); wired
together in [`src/main.rs`](../src/main.rs) (`obfuscate`).*
