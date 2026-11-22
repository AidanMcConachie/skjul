# 5. String encryption

**Goal:** make sure no readable text survives in the compiled program.

## The problem

Even after the later literal-disguise step re-encodes a string's bytes, the
actual text still sits intact inside the compiled binary. Anyone can run the
standard `strings` tool on the executable and read every message, prompt, and
error. That often gives away exactly what a program does.

## The fix

This pass replaces each string with a tiny function call that **rebuilds** the
text at run time. The plaintext is never stored directly; instead skjul stores a
scrambled (encrypted) version and a small decoder function. Simplified, it
injects something like this once, at the top of the file:

```c
char *_sk_dec(char *src, char *out, int n, int key) {
    for (int i = 0; i < n; i++)
        out[i] = src[i] ^ (key + i);   // undo the scramble
    out[n] = 0;
    return out;
}
static char _sk_c0[] = { /* scrambled bytes of "Hello" */ };
static char _sk_b0[6];                 // scratch space for the result
```

and then, wherever `"Hello"` appeared, it writes:

```c
(_sk_dec(_sk_c0, _sk_b0, 5, 37))
```

So the binary contains only the scrambled bytes; the readable word "Hello" is
reconstructed in memory only while the program runs.

## How the scramble works

The cipher is a simple "rolling XOR": each byte is combined with `key + index`.
XOR is reversible — applying the same operation again gives back the original
byte — which is why the decoder is just the same loop. The key comes from
skjul's shared random generator, so the output is different per string but still
deterministic (the same input always produces the same output).

Each string decodes into its **own** separate buffer, which keeps two things
correct:

- A string used inside a loop decodes correctly every iteration (decoding always
  starts from the pristine scrambled copy).
- Two different strings never share a buffer, so `printf("%s %s", a, b)` still
  prints both correctly.

## Playing it safe

Only strings in *executable* positions (where a function call is allowed) are
encrypted. A string used in a declaration's initializer — like
`char buf[] = "x";` — or as the operand of `sizeof` or `&` is left alone,
because a function call can't legally stand in for the literal there.

*Source: [`src/transformation/strings.rs`](../src/transformation/strings.rs).*
