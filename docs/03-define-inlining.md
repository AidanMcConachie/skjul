# 3. Define inlining

**Goal:** remove `#define` constants by folding their value directly into the
code.

## The problem

A `#define` is a *preprocessor* directive — it sits on its own line and skjul
passes it through untouched. That leaks information twice:

```c
#define CLASS_SIZE 8
...
int marks[CLASS_SIZE];
```

1. The directive itself (`#define CLASS_SIZE 8`) survives as a readable banner.
2. The name `CLASS_SIZE` appears at every use, telling a reader what the number
   *means*.

A meaningful name like `CLASS_SIZE` is a gift to anyone reverse-engineering the
code.

## The fix

This pass substitutes the value everywhere the name is used and deletes the
directive:

```c
int marks[8];
```

Now there's no `CLASS_SIZE` name and no directive. The bare `8` is just a
number — and the later **Literal disguise** step (step 11) will scramble it into
something like `(23 ^ 31)`, so even the value is hidden.

## Playing it safe

Like every structural pass, this one only *removes* information and never risks
breaking your program. It inlines a `#define` only when it is completely sure
the swap is exact:

- **Integer constants only.** The macro body must be a single whole number
  (decimal, hex, binary, or octal, with an optional sign). Function-like macros
  (`SQ(x)`), string macros, float macros, and empty `#define FLAG` are all left
  alone.
- **No conditional compilation.** If the file contains any `#if`, `#ifdef`,
  `#ifndef`, `#else`, `#elif`, or `#endif`, a define might live on a branch skjul
  can't reason about, so the whole pass stands down and changes nothing.
- **Defined once, referenced nowhere else.** The name must be mentioned by
  exactly one directive — its own `#define`. A redefinition, an `#undef`, or a
  macro that builds on it (`#define TOTAL (CLASS_SIZE * 2)`) disqualifies it, so
  a surviving directive is never left pointing at a name this pass deleted.

Anything it doesn't recognise is left exactly as it was.

*Source: [`src/transformation/defines.rs`](../src/transformation/defines.rs).*
