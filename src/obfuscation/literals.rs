//! Constant obfuscation: rewrite the literals in a token stream into forms
//! that compile to exactly the same values but are no longer readable at a
//! glance.
//!
//! Three kinds of literal are handled:
//!
//! * **Strings** are re-encoded so every byte becomes an escape — a
//!   three-digit octal (`\150`) or two-digit hex (`\x68`) form, chosen per
//!   byte. Every escape begins with a backslash, which terminates any
//!   preceding hex escape, so the two forms mix safely for any bytes. The
//!   bytes are identical, so the program behaves the same; the readable text
//!   is gone.
//! * **Character constants** become integer expressions. A character constant
//!   has type `int` in C, so `'A'` and `65` are interchangeable as values, and
//!   we can hand the result to the same arithmetic scrambler the integers use.
//! * **Integer constants** are replaced by an equivalent arithmetic
//!   expression (`10` → `(23^29)`). Both operands are literals, so the result
//!   is still an integer *constant expression* and remains legal everywhere a
//!   plain literal was — `case` labels, array bounds, enum values and so on.
//!
//! Every rewritten expression is wrapped in parentheses and emitted through
//! [`TokenType::RAW`], which keeps precedence intact and prevents the text
//! from fusing with a neighbouring token when the stream is minified.

use crate::tokenization::token::{Token, TokenType};

/// Rewrite every literal in `tokens` into an equivalent obfuscated form.
///
/// The transformation is deterministic: the same input always yields the same
/// output. A small counter-based generator supplies the masks, so repeated
/// literals still get varied disguises without any external randomness.
pub fn obfuscate_literals(tokens: Vec<Token>) -> Vec<Token> {
    // Fixed seed → deterministic output. The constant is the 64-bit golden
    // ratio, a conventional, well-mixed starting value.
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;

    (0..tokens.len())
        .map(|i| {
            let kind = match &tokens[i].kind {
                TokenType::STRING(content) => {
                    TokenType::STRING(encode_mixed(&decode_c_escapes(content), &mut state))
                }

                TokenType::CHARACTER(content) => {
                    let bytes = decode_c_escapes(content);
                    // Only rewrite a single-byte, plainly-positive constant.
                    // A value ≥ 128 is negative on platforms where `char` is
                    // signed, so turning it into a positive integer literal
                    // would silently change its value; leave those untouched.
                    // Multi-byte constants are implementation-defined too.
                    if bytes.len() == 1 && bytes[0] < 128 {
                        TokenType::RAW(obfuscate_int(bytes[0] as i64, &mut state))
                    } else {
                        tokens[i].kind.clone()
                    }
                }

                TokenType::INTEGER(value) => {
                    // The lexer has no float rule, so `3.14` arrives as
                    // `3 . 14`. Wrapping either integer in parentheses would
                    // turn that into `(3).(14)`, which no longer parses. Leave
                    // an integer alone when it sits against a dot so float
                    // literals keep surviving the round trip.
                    let touches_dot = matches!(neighbour(&tokens, i, -1), Some(TokenType::DOT))
                        || matches!(neighbour(&tokens, i, 1), Some(TokenType::DOT));
                    if touches_dot {
                        tokens[i].kind.clone()
                    } else {
                        TokenType::RAW(obfuscate_int(*value, &mut state))
                    }
                }

                other => other.clone(),
            };

            Token {
                kind,
                line: tokens[i].line,
                column: tokens[i].column,
            }
        })
        .collect()
}

/// The kind of the token `offset` positions away from index `i`, if any.
fn neighbour(tokens: &[Token], i: usize, offset: isize) -> Option<&TokenType> {
    let j = i as isize + offset;
    if j < 0 {
        return None;
    }
    tokens.get(j as usize).map(|t| &t.kind)
}

/// Produce a parenthesised integer constant expression equal to `value`.
///
/// The expression is built recursively: each operand of the top-level
/// operation is itself an obfuscated sub-expression, down to [`INT_DEPTH`]
/// levels, so a single literal fans out into a small tree of operations.
/// Several shapes are used (`^`, `-`, `+`, `<<`/`+`, `*`), chosen by the
/// generator so repeated values look different.
///
/// Every operand is a non-negative integer, so no unary-minus token is ever
/// introduced (which could fuse with a preceding operator in the minified
/// text), and the whole expression stays a legal integer constant expression —
/// valid anywhere a plain literal was.
fn obfuscate_int(value: i64, state: &mut u64) -> String {
    obfuscate_int_rec(value, INT_DEPTH, state)
}

/// Each recursion level replaces both operands with sub-expressions, so the
/// literal fans out into up to `2^INT_DEPTH` leaves. Kept small to bound the
/// size of the generated text.
const INT_DEPTH: u32 = 2;

fn obfuscate_int_rec(value: i64, depth: u32, state: &mut u64) -> String {
    // Leaf: emit the plain literal. `value` is non-negative at every level, so
    // this never introduces a leading minus.
    if depth == 0 {
        return value.to_string();
    }

    let mask = (next(state) % 0xFFFF) as i64 + 1; // 1..=0xFFFF

    match next(state) % 5 {
        // value = A ^ mask. XOR flips only low bits, so A stays non-negative.
        0 => {
            let a = obfuscate_int_rec(value ^ mask, depth - 1, state);
            let b = obfuscate_int_rec(mask, depth - 1, state);
            format!("({a}^{b})")
        }

        // value = A - mask, with A = value + mask. Fall back to XOR when the
        // addition would overflow i64.
        1 => match value.checked_add(mask) {
            Some(sum) => {
                let a = obfuscate_int_rec(sum, depth - 1, state);
                let b = obfuscate_int_rec(mask, depth - 1, state);
                format!("({a}-{b})")
            }
            None => {
                let a = obfuscate_int_rec(value ^ mask, depth - 1, state);
                let b = obfuscate_int_rec(mask, depth - 1, state);
                format!("({a}^{b})")
            }
        },

        // value = A + B, split into two non-negative parts that sum to it.
        2 => {
            let b_val = (next(state) % (value as u64 + 1)) as i64; // 0..=value
            let a = obfuscate_int_rec(value - b_val, depth - 1, state);
            let b = obfuscate_int_rec(b_val, depth - 1, state);
            format!("({a}+{b})")
        }

        // value = (high << s) + low, the exact shift/remainder split of value.
        3 => {
            let s = (next(state) % 8) as u32 + 1; // 1..=8
            let high = obfuscate_int_rec(value >> s, depth - 1, state);
            let low = obfuscate_int_rec(value & ((1i64 << s) - 1), depth - 1, state);
            format!("(({high}<<{s})+{low})")
        }

        // value = A * B, when a small factor exists; otherwise fall back to XOR.
        _ => match small_factor(value, state) {
            Some(f) => {
                let a = obfuscate_int_rec(f, depth - 1, state);
                let b = obfuscate_int_rec(value / f, depth - 1, state);
                format!("({a}*{b})")
            }
            None => {
                let a = obfuscate_int_rec(value ^ mask, depth - 1, state);
                let b = obfuscate_int_rec(mask, depth - 1, state);
                format!("({a}^{b})")
            }
        },
    }
}

/// Find a small divisor of `value` so it can be written as a product, or
/// `None` when none of the candidates divide it (e.g. a large prime). Zero is
/// reported as divisible by 2 (`2 * 0`). The starting point is drawn from the
/// generator so the chosen factor varies.
fn small_factor(value: i64, state: &mut u64) -> Option<i64> {
    if value == 0 {
        return Some(2);
    }
    const CANDIDATES: [i64; 6] = [2, 3, 5, 7, 11, 13];
    let start = (next(state) % CANDIDATES.len() as u64) as usize;
    for k in 0..CANDIDATES.len() {
        let f = CANDIDATES[(start + k) % CANDIDATES.len()];
        if value % f == 0 {
            return Some(f);
        }
    }
    None
}

/// Advance the counter-based generator and return the next value. This is an
/// LCG with the constants used by Knuth's MMIX; we only need a well-spread,
/// reproducible sequence, not cryptographic quality.
fn next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    // The high bits of an LCG are the well-mixed ones.
    *state >> 16
}

/// Decode the stored content of a string or character literal into the actual
/// bytes it denotes, resolving C escape sequences. The lexer keeps a literal's
/// content verbatim (backslashes and all), so this is where escapes are
/// finally interpreted.
///
/// Unknown escapes are passed through best-effort (the escaped character is
/// kept literally, matching the common compiler behaviour) rather than
/// rejected, since by this point the source has already lexed cleanly.
pub(crate) fn decode_c_escapes(content: &str) -> Vec<u8> {
    let chars: Vec<char> = content.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c != '\\' {
            push_utf8(&mut out, c);
            i += 1;
            continue;
        }

        // A backslash: consume the escape that follows.
        i += 1;
        let Some(&e) = chars.get(i) else {
            out.push(b'\\'); // stray trailing backslash
            break;
        };

        match e {
            'n' => push_byte(&mut out, b'\n', &mut i),
            't' => push_byte(&mut out, b'\t', &mut i),
            'r' => push_byte(&mut out, b'\r', &mut i),
            '\\' => push_byte(&mut out, b'\\', &mut i),
            '\'' => push_byte(&mut out, b'\'', &mut i),
            '"' => push_byte(&mut out, b'"', &mut i),
            '?' => push_byte(&mut out, b'?', &mut i),
            'a' => push_byte(&mut out, 0x07, &mut i),
            'b' => push_byte(&mut out, 0x08, &mut i),
            'f' => push_byte(&mut out, 0x0c, &mut i),
            'v' => push_byte(&mut out, 0x0b, &mut i),

            // Octal escape: up to three octal digits, starting at `e`.
            '0'..='7' => {
                let mut value: u32 = 0;
                let mut n = 0;
                while n < 3 {
                    match chars.get(i) {
                        Some(d @ '0'..='7') => {
                            value = value * 8 + (*d as u32 - '0' as u32);
                            i += 1;
                            n += 1;
                        }
                        _ => break,
                    }
                }
                out.push((value & 0xFF) as u8);
            }

            // Hex escape: `\x` followed by one or more hex digits.
            'x' => {
                i += 1; // skip the 'x'
                let mut value: u32 = 0;
                let mut any = false;
                while let Some(d) = chars.get(i).and_then(|c| c.to_digit(16)) {
                    value = value * 16 + d;
                    i += 1;
                    any = true;
                }
                if any {
                    out.push((value & 0xFF) as u8);
                } else {
                    out.push(b'x'); // malformed; keep the 'x' literally
                }
            }

            // Unknown escape: keep the character itself.
            other => {
                push_utf8(&mut out, other);
                i += 1;
            }
        }
    }

    out
}

/// Append a single resolved byte and advance the cursor past the escape char.
fn push_byte(out: &mut Vec<u8>, byte: u8, i: &mut usize) {
    out.push(byte);
    *i += 1;
}

/// Append the UTF-8 encoding of `c` to `out`.
fn push_utf8(out: &mut Vec<u8>, c: char) {
    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
}

/// Re-encode raw bytes as a string-literal body in which every byte is an
/// escape sequence, chosen per byte between a three-digit octal (`\150`) and a
/// two-digit hex (`\x68`) form. Both forms begin with a backslash, and a
/// backslash terminates a hex escape, so the hex form never swallows the
/// following byte's digits — the two mix safely for any byte sequence.
fn encode_mixed(bytes: &[u8], state: &mut u64) -> String {
    let mut out = String::new();
    for &b in bytes {
        if next(state) % 2 == 0 {
            out.push_str(&format!("\\{b:03o}"));
        } else {
            out.push_str(&format!("\\x{b:02x}"));
        }
    }
    out
}
