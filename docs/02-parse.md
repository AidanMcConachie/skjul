# 2. Parse

**Goal:** turn the flat list of tokens into an *AST* — a tree that captures the
structure of the program.

## From a list to a tree

Tokens on their own are just a sequence; they don't say which tokens belong to
which function, or which statements are inside a loop. Parsing adds that
structure. Take this code:

```c
int main() {
    int x = 5;
    return x;
}
```

The parser recognises "a function named `main`, whose body is a block
containing two statements: a declaration and a return." That nested
relationship — a function *containing* a block *containing* statements — is the
tree, formally called an **Abstract Syntax Tree (AST)**.

## Why a tree?

The powerful structural passes (flattening, opaque predicates, dead code) need
to understand the *shape* of your program, not just its words. You can't safely
move a statement into a new `switch` arm, or wrap it in an `if`, unless you know
exactly where statements begin and end and how they nest. The AST gives you
that.

## The supported subset

skjul understands a large but not complete slice of C. It handles:

- functions and prototypes,
- the full statement and expression grammar,
- array-syntax parameters like `int marks[]` or `int grid[4]` (their written
  shape is preserved),
- inline `struct` / `union` / `enum` definitions,
- `typedef`s (both defining a type and later using the new name),
- `sizeof`.

## The fallback

If your program uses something outside that subset — bitfields, or
function-pointer declarators, for example — the parser **can't** build a tree
for it. Rather than fail, skjul gives up on the structural passes and jumps
straight to step 9 (**Print & re-lex**), obfuscating the original tokens
directly. This is why skjul always returns *something*: the token-level passes
don't need the tree.

*Source: [`src/parsing/`](../src/parsing/) (`parser.rs`, `ast.rs`).*
