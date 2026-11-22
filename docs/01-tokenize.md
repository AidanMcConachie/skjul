# 1. Tokenize

**Goal:** turn the raw text of your program into a list of *tokens*.

## What is a token?

A token is the smallest meaningful piece of code — one "word" or symbol. Your
source file is just a long string of characters, but a compiler (and skjul)
doesn't think in characters, it thinks in tokens. For example, this line:

```c
int x = 5;
```

becomes five tokens:

```
int    x    =    5    ;
```

The whitespace between them, and any comments, are thrown away — they only
existed to help humans read the code.

## What the tokenizer (lexer) produces

skjul's lexer walks through the text one character at a time and groups the
characters into tokens of a few kinds:

- **Identifiers** — names like `x`, `main`, `total` (`IDENTIFIER`).
- **Integers** — whole-number literals like `5` or `0xFF` (`INTEGER`).
- **Strings and characters** — text in quotes, like `"hello"` or `'A'`
  (`STRING`, `CHARACTER`).
- **Preprocessor lines** — whole lines starting with `#`, like `#include
  <stdio.h>`, kept as one token (`PREPROCESSOR`). These stay intact because the
  C preprocessor is line-based.
- **Punctuation and operators** — `=`, `;`, `{`, `+`, and so on.

## Why this step matters

Every later step works on tokens, not raw text. Tokenizing is the common
entry point:

- The **Parse** step (next) consumes these tokens to build the program tree.
- If parsing fails, the token-level passes work on this same list directly.

So tokenizing is simply the act of *reading* the program into a form the rest
of the tool can manipulate. It changes nothing about the meaning of the code.

*Source: [`src/tokenization/lexer.rs`](../src/tokenization/lexer.rs).*
