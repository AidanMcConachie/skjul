//! Constant obfuscation: rewrite the literals in a token stream into forms
//! that compile to exactly the same values but are no longer readable at a
//! glance.
//!
//! Three kinds of literal are handled:
//!
//! * **Strings** are re-encoded so every byte becomes a three-digit octal
//!   escape (`"hi"` → `"\150\151"`). The bytes are identical, so the program
//!   behaves the same; the human-readable text is gone.
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
                    TokenType::STRING(encode_octal(&decode_c_escapes(content)))
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
/// Two shapes are used, chosen by the generator so repeated values look
/// different: a bitwise `(A^B)` and an arithmetic `(A-B)`. Both operands are
/// non-negative literals, so no unary-minus token is introduced and the text
/// drops cleanly into the stream.
fn obfuscate_int(value: i64, state: &mut u64) -> String {
    let mask = (next(state) % 0xFFFF) as i64 + 1; // 1..=0xFFFF

    if next(state) % 2 == 0 {
        // XOR always round-trips for a non-negative value, flipping only the
        // low bits, so `A = value ^ mask` stays non-negative.
        let a = value ^ mask;
        format!("({a}^{mask})")
    } else {
        // Subtraction, but fall back to XOR if `value + mask` would overflow.
        match value.checked_add(mask) {
            Some(a) => format!("({a}-{mask})"),
            None => format!("({}^{mask})", value ^ mask),
        }
    }
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
fn decode_c_escapes(content: &str) -> Vec<u8> {
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

/// Re-encode raw bytes as a string-literal body in which every byte is a
/// three-digit octal escape. Fixed-width escapes are self-delimiting, so the
/// result is unambiguous no matter what bytes follow (`"\1" "2"` ambiguity
/// cannot arise).
fn encode_octal(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        out.push_str(&format!("\\{b:03o}"));
    }
    out
}
