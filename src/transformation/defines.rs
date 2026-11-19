//! Inline object-like `#define` constants and drop the directive
//! (define inlining).
//!
//! Like `#include`, a `#define` is a *preprocessor* directive: it stands alone
//! on its own physical line and survives every structural pass as a verbatim
//! banner. `#define CLASS_SIZE 8` leaks twice — once as the directive itself,
//! and again as the readable name `CLASS_SIZE` at every use, where the constant
//! `8` is carried as a symbol the later integer-disguise pass never sees.
//!
//! This pass closes both leaks for the common case: an *object-like* macro whose
//! replacement is a single integer constant. It substitutes the value at every
//! use (turning `CLASS_SIZE` into the literal `8`, which the token-level
//! literal pass then disguises like any other number) and removes the directive.
//! Nothing symbolic survives, and the `8` is hidden alongside every other
//! literal in the output.
//!
//! # Conservatism
//!
//! Matching the rest of the module, the pass only ever removes information and
//! never risks miscompilation. It inlines a `#define` only when it can be sure
//! the substitution is exact and self-contained:
//!
//! * **Integer object-like macros only.** The body must be a single integer
//!   constant (decimal, `0x` hex, `0b` binary or leading-zero octal, with an
//!   optional sign). A function-like macro (`SQ(x)`), a macro whose body is an
//!   expression, a string, a float, or another macro, and a bodyless
//!   `#define FLAG` are all left untouched.
//! * **No conditional compilation.** If the unit contains any `#if`/`#ifdef`/
//!   `#ifndef`/`#elif`/`#else`/`#endif` directive, a define might be guarded and
//!   we cannot tell which branch is live, so the whole pass stands down and
//!   changes nothing.
//! * **Defined once, used by no other directive.** A name is inlined only when
//!   exactly one directive mentions it — its own `#define`. A redefinition, a
//!   `#undef`, or a name referenced from another directive (a macro chain like
//!   `#define TOTAL (CLASS_SIZE * 2)`) disqualifies it, so a kept directive is
//!   never left referring to a name this pass removed.
//!
//! Anything the pass does not recognise is left exactly as it was.

use std::collections::HashMap;

use crate::parsing::ast::*;

/// Inline every object-like integer `#define` that is safe to inline, replacing
/// each use with the constant and removing the directive.
pub fn inline_defines(mut unit: TranslationUnit) -> TranslationUnit {
    // Conditional compilation puts defines on branches we cannot reason about;
    // if any is present, leave the whole unit untouched.
    if unit
        .items
        .iter()
        .any(|it| matches!(it, Item::Directive(line) if is_conditional(line)))
    {
        return unit;
    }

    // Candidate object-like integer defines, as (item index, name, value).
    let candidates: Vec<(usize, String, i64)> = unit
        .items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            let Item::Directive(line) = item else {
                return None;
            };
            let (name, body) = parse_object_define(line)?;
            let value = parse_int_body(body)?;
            Some((i, name.to_string(), value))
        })
        .collect();

    // A name is inlinable only when exactly one directive mentions it (its own
    // `#define`). Any other mention — a redefinition, an `#undef`, or a macro
    // chain — means removing the directive could strand a kept one, so we skip.
    let mut macros: HashMap<String, i64> = HashMap::new();
    let mut remove: Vec<usize> = Vec::new();
    for (idx, name, value) in &candidates {
        let mentions = unit
            .items
            .iter()
            .filter(|it| matches!(it, Item::Directive(line) if mentions_name(line, name)))
            .count();
        if mentions == 1 {
            macros.insert(name.clone(), *value);
            remove.push(*idx);
        }
    }

    if macros.is_empty() {
        return unit;
    }

    // Substitute uses in real parsed code, then drop the inlined directives.
    for item in &mut unit.items {
        match item {
            Item::Function(func) => {
                for p in &mut func.params {
                    subst_dims(&mut p.array_dims, &macros);
                }
                if let Some(body) = &mut func.body {
                    subst_block(body, &macros);
                }
            }
            Item::Declaration(decl) => subst_declaration(decl, &macros),
            Item::Directive(_) | Item::Raw(_) => {}
        }
    }
    for idx in remove {
        unit.items[idx] = Item::Raw(String::new());
    }

    unit
}

/// Whether a directive is a conditional-compilation line, whose presence makes
/// the whole pass stand down.
fn is_conditional(line: &str) -> bool {
    let rest = line.trim_start();
    let Some(rest) = rest.strip_prefix('#') else {
        return false;
    };
    let rest = rest.trim_start();
    ["if", "ifdef", "ifndef", "elif", "else", "endif"]
        .iter()
        .any(|kw| match rest.strip_prefix(*kw) {
            // The keyword must be whole: `#if`/`#ifdef` but not `#iffoo`, and
            // `#else`/`#endif` which take nothing after them.
            Some(after) => after.is_empty() || after.starts_with([' ', '\t']),
            None => false,
        })
}

/// Parse a `#define` directive into `(name, body)` for an *object-like* macro.
///
/// Returns `None` for a directive that isn't a `#define`, a function-like macro
/// (`#define SQ(x) ...`, where `(` abuts the name), or a bodyless `#define FLAG`.
fn parse_object_define(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim_start().strip_prefix('#')?.trim_start();
    let rest = rest.strip_prefix("define")?;
    // Require whitespace after `define` so we don't match `#defineX`.
    let rest = rest.strip_prefix([' ', '\t'])?.trim_start();

    let name_end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    let name = &rest[..name_end];
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }

    let after = &rest[name_end..];
    // A `(` immediately after the name marks a function-like macro.
    if after.starts_with('(') {
        return None;
    }
    let body = after.trim();
    if body.is_empty() {
        return None;
    }
    Some((name, body))
}

/// Parse a macro body that is a single integer constant into its value, or
/// `None` if the body is anything else. A trailing `//` line comment is ignored.
/// Suffixed integers (`8UL`) are rejected so the pass never changes a constant's
/// type.
fn parse_int_body(body: &str) -> Option<i64> {
    let body = match body.find("//") {
        Some(i) => &body[..i],
        None => body,
    };
    let body = body.trim();

    let (negative, digits) = match body.strip_prefix('-') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, body.strip_prefix('+').map_or(body, str::trim_start)),
    };

    let magnitude = parse_unsigned(digits)?;
    if negative {
        magnitude.checked_neg()
    } else {
        Some(magnitude)
    }
}

/// Parse an unsigned integer constant (decimal, `0x` hex, `0b` binary, or
/// leading-zero octal) that makes up the *entire* string, with no type suffix.
fn parse_unsigned(s: &str) -> Option<i64> {
    let (radix, digits) = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        (16, hex)
    } else if let Some(bin) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        (2, bin)
    } else if s.len() > 1 && s.starts_with('0') {
        (8, &s[1..])
    } else {
        (10, s)
    };
    if digits.is_empty() {
        return None;
    }
    // `from_str_radix` rejects any non-digit, so a suffix like `U` or stray
    // trailing text fails here and the define is left untouched.
    i64::from_str_radix(digits, radix).ok()
}

/// Whether `name` appears as a whole identifier token anywhere in `line`.
fn mentions_name(line: &str, name: &str) -> bool {
    let bytes = line.as_bytes();
    let mut from = 0;
    while let Some(rel) = line[from..].find(name) {
        let start = from + rel;
        let end = start + name.len();
        let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
        let after_ok = end == bytes.len() || !is_ident_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

// ---- substitution over the parsed tree ----------------------------------

fn subst_declaration(decl: &mut Declaration, macros: &HashMap<String, i64>) {
    for d in &mut decl.declarators {
        subst_dims(&mut d.array_dims, macros);
        if let Some(init) = &mut d.init {
            subst_expr(init, macros);
        }
    }
}

fn subst_dims(dims: &mut [Option<Expr>], macros: &HashMap<String, i64>) {
    for dim in dims.iter_mut().flatten() {
        subst_expr(dim, macros);
    }
}

fn subst_block(block: &mut Block, macros: &HashMap<String, i64>) {
    for stmt in &mut block.stmts {
        subst_stmt(stmt, macros);
    }
}

fn subst_stmt(stmt: &mut Stmt, macros: &HashMap<String, i64>) {
    match stmt {
        Stmt::Declaration(decl) => subst_declaration(decl, macros),
        Stmt::Expr(expr) => subst_expr(expr, macros),
        Stmt::Return(expr) => {
            if let Some(expr) = expr {
                subst_expr(expr, macros);
            }
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            subst_expr(cond, macros);
            subst_stmt(then_branch, macros);
            if let Some(else_branch) = else_branch {
                subst_stmt(else_branch, macros);
            }
        }
        Stmt::While { cond, body } => {
            subst_expr(cond, macros);
            subst_stmt(body, macros);
        }
        Stmt::DoWhile { body, cond } => {
            subst_stmt(body, macros);
            subst_expr(cond, macros);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(init) = init {
                subst_stmt(init, macros);
            }
            if let Some(cond) = cond {
                subst_expr(cond, macros);
            }
            if let Some(step) = step {
                subst_expr(step, macros);
            }
            subst_stmt(body, macros);
        }
        Stmt::Block(block) => subst_block(block, macros),
        Stmt::Switch { cond, body } => {
            subst_expr(cond, macros);
            subst_stmt(body, macros);
        }
        Stmt::Case { value, body } => {
            subst_expr(value, macros);
            subst_stmt(body, macros);
        }
        Stmt::Default { body } | Stmt::Label { body, .. } => subst_stmt(body, macros),
        Stmt::Break | Stmt::Continue | Stmt::Empty | Stmt::Goto(_) => {}
    }
}

fn subst_expr(expr: &mut Expr, macros: &HashMap<String, i64>) {
    match expr {
        Expr::Ident(name) => {
            if let Some(value) = macros.get(name) {
                *expr = Expr::Int(*value);
            }
        }
        Expr::Int(_) | Expr::Str(_) | Expr::Char(_) | Expr::Raw(_) => {}
        Expr::Unary { expr, .. } | Expr::Postfix { expr, .. } => subst_expr(expr, macros),
        Expr::Binary { lhs, rhs, .. } | Expr::Comma { lhs, rhs } => {
            subst_expr(lhs, macros);
            subst_expr(rhs, macros);
        }
        Expr::Assign { target, value, .. } => {
            subst_expr(target, macros);
            subst_expr(value, macros);
        }
        Expr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => {
            subst_expr(cond, macros);
            subst_expr(then_expr, macros);
            subst_expr(else_expr, macros);
        }
        Expr::Cast { expr, .. } => subst_expr(expr, macros),
        Expr::Call { callee, args } => {
            subst_expr(callee, macros);
            for arg in args {
                subst_expr(arg, macros);
            }
        }
        Expr::Index { base, index } => {
            subst_expr(base, macros);
            subst_expr(index, macros);
        }
        Expr::Member { base, .. } => subst_expr(base, macros),
        Expr::InitList(elems) => {
            for elem in elems {
                subst_expr(elem, macros);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::parser::parse;
    use crate::parsing::printer::print_unit;
    use crate::tokenization::lexer::tokenize;

    /// Run the pass over `source` and return the printed C.
    fn inlined(source: &str) -> String {
        let unit = parse(tokenize(source).expect("lex")).expect("parse");
        print_unit(&inline_defines(unit))
    }

    #[test]
    fn inlines_object_like_integer_and_drops_directive() {
        let out = inlined(
            r#"
#define CLASS_SIZE 8
#define PASS_MARK 50
int main() {
    int marks[CLASS_SIZE];
    int last = marks[CLASS_SIZE - 1];
    if (last >= PASS_MARK) return 1;
    return 0;
}
"#,
        );
        // The directives are gone and no symbolic name survives.
        assert!(
            !out.contains("#define"),
            "directive should be dropped:\n{out}"
        );
        assert!(
            !out.contains("CLASS_SIZE"),
            "name should be inlined:\n{out}"
        );
        assert!(!out.contains("PASS_MARK"), "name should be inlined:\n{out}");
        // The values appear as plain literals the later pass can disguise.
        assert!(out.contains('8'), "value 8 should be inlined:\n{out}");
        assert!(out.contains("50"), "value 50 should be inlined:\n{out}");
    }

    #[test]
    fn parses_hex_octal_binary_and_signed_bodies() {
        assert_eq!(parse_int_body("8"), Some(8));
        assert_eq!(parse_int_body("0x10"), Some(16));
        assert_eq!(parse_int_body("0b101"), Some(5));
        assert_eq!(parse_int_body("0755"), Some(493));
        assert_eq!(parse_int_body("-3"), Some(-3));
        assert_eq!(parse_int_body("  42  // rows"), Some(42));
        // Non-integer or risky bodies are left for the directive to keep.
        assert_eq!(parse_int_body("8UL"), None);
        assert_eq!(parse_int_body("3.5"), None);
        assert_eq!(parse_int_body("(1 + 2)"), None);
        assert_eq!(parse_int_body("OTHER"), None);
    }

    #[test]
    fn leaves_function_like_and_expression_macros_alone() {
        let source = r#"
#define SQ(x) ((x) * (x))
#define TOTAL (3 + 4)
int main() { return SQ(2); }
"#;
        let out = inlined(source);
        assert!(
            out.contains("#define SQ"),
            "function-like macro kept:\n{out}"
        );
        assert!(
            out.contains("#define TOTAL"),
            "expression macro kept:\n{out}"
        );
    }

    #[test]
    fn keeps_a_define_referenced_by_another_directive() {
        // `BASE` is used by the kept `DOUBLE` macro, so removing its own
        // `#define` would strand `DOUBLE`; the pass must leave both in place.
        let source = r#"
#define BASE 10
#define DOUBLE (BASE * 2)
int main() { return DOUBLE; }
"#;
        let out = inlined(source);
        assert!(
            out.contains("#define BASE 10"),
            "chained define kept:\n{out}"
        );
        assert!(
            out.contains("#define DOUBLE"),
            "dependent define kept:\n{out}"
        );
    }

    #[test]
    fn stands_down_on_conditional_compilation() {
        let source = r#"
#define N 4
#ifdef DEBUG
#define M 1
#endif
int main() { return N; }
"#;
        let out = inlined(source);
        // A guarded unit is left entirely untouched.
        assert!(
            out.contains("#define N 4"),
            "nothing inlined under #ifdef:\n{out}"
        );
    }
}
