# 4. Include elision

**Goal:** get rid of `#include` lines without breaking the program.

## The problem

An `#include` directive, like `#define`, must sit alone on its own line, so it
survives every other pass as a tidy banner at the top of the file:

```c
#include <stdio.h>
```

That one line announces exactly which libraries the program relies on — a strong
hint about what it does.

## The fix

Instead of *disguising* the directive, this pass *eliminates* it. For a system
header skjul recognises, it already knows the prototypes (the type signatures)
of the standard functions that header declares. So it can:

1. delete the `#include`, and
2. splice in an `extern` declaration for **only** the standard functions the
   program actually uses.

For example, a program that uses `printf` loses `#include <stdio.h>` and gains a
plain `extern int printf(...);` line somewhere in the code. Because that's
ordinary top-level C, it scatters in among everything else and gets renamed and
minified like any other code — no labelled header block pointing the way.

## Playing it safe

The pass only removes information, never risking miscompilation:

- A header is elided only when **every** standard symbol the program uses from
  it can be declared using primitive types alone.
- If the program touches something that needs the library's own typedefs — such
  as `FILE`, or a `size_t`-typed *variable* — the `#include` is kept as-is, so
  those types are still available.

## Portability note

Size-related parameters (for `malloc`, `memcpy`, `strlen`, …) are declared with
`unsigned long` instead of `size_t`. That's exact on 64-bit Linux and macOS
(where `size_t` *is* `unsigned long`). On 64-bit Windows the two differ, so this
pass assumes a Linux/macOS-style (LP64) toolchain.

*Source: [`src/transformation/includes.rs`](../src/transformation/includes.rs).*
