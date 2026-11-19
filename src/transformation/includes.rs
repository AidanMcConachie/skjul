//! Replace `#include` directives with inline `extern` declarations
//! (include elision).
//!
//! A `#include` is a *preprocessor* directive, and the preprocessor is
//! line-based: the directive must stand alone on its own physical line, so it
//! can never be folded into the surrounding code the way the structural passes
//! fold statements. That line survives every other pass untouched — a tidy,
//! legible banner at the top of the output announcing exactly which libraries
//! the program leans on. This pass removes it.
//!
//! The trick is not to *disguise* the directive but to *eliminate* it. For a
//! recognised system header we know the prototypes of the standard functions it
//! declares, so we can drop the `#include` and splice in `extern` declarations
//! for just the symbols the program actually uses. Those declarations are
//! ordinary top-level C: they scatter in among the obfuscated code instead of
//! sitting in a labelled block, and the identifier-renaming and minifying passes
//! run over them like anything else.
//!
//! # Conservatism
//!
//! The pass only ever *removes* information, never risks miscompilation. A
//! directive is elided only when every standard symbol the program uses from
//! that header has a prototype we can spell using primitive types alone. If the
//! program touches a part of the header we cannot model without its own
//! typedefs — `FILE`, or a `size_t`-typed *object* (as opposed to a parameter) —
//! the directive is kept verbatim. Anything the pass does not recognise is left
//! exactly as it was, matching the rest of the transformation module.
//!
//! # Portability note
//!
//! The size-of-object parameters (`malloc`, `memcpy`, `strlen`, …) are declared
//! with `unsigned long` in place of `size_t`. That is exact on LP64 targets
//! (Linux and macOS, 64-bit), where `size_t` *is* `unsigned long`. On an LLP64
//! target (64-bit Windows) `size_t` is wider than `unsigned long`; there the
//! elided prototypes would not match the real ones, so this pass assumes an LP64
//! toolchain.

use std::collections::HashSet;

use crate::parsing::ast::*;

/// One recognised system header and everything we know about it.
struct KnownHeader {
    /// The spelling inside the `#include`, e.g. `stdio.h`.
    name: &'static str,
    /// Standard functions we can declare with primitive types only, as
    /// `(symbol, full extern declaration)` pairs.
    protos: &'static [(&'static str, &'static str)],
    /// Names from this header we recognise but cannot safely inline — they
    /// require a library typedef (`FILE`, `size_t` as an object type, …). If the
    /// program uses any of these, the `#include` is kept so the header still
    /// provides them.
    keep_if_used: &'static [&'static str],
}

/// The headers this pass understands. Prototypes use only primitive types (with
/// the LP64 `size_t` assumption documented at the module level), so each spliced
/// declaration is self-contained and needs no other header.
const KNOWN_HEADERS: &[KnownHeader] = &[
    KnownHeader {
        name: "stdio.h",
        protos: &[
            ("printf", "extern int printf(const char *, ...);"),
            ("scanf", "extern int scanf(const char *, ...);"),
            ("sprintf", "extern int sprintf(char *, const char *, ...);"),
            (
                "sscanf",
                "extern int sscanf(const char *, const char *, ...);",
            ),
            ("puts", "extern int puts(const char *);"),
            ("putchar", "extern int putchar(int);"),
            ("getchar", "extern int getchar(void);"),
        ],
        // The FILE *-bearing half of <stdio.h>: these need the opaque `FILE`
        // type, which only the header can spell, so their use pins the include.
        keep_if_used: &[
            "FILE", "fprintf", "fscanf", "fopen", "fclose", "fgets", "fputs", "fread", "fwrite",
            "stdin", "stdout", "stderr", "fputc", "fgetc", "getline", "perror", "snprintf",
        ],
    },
    KnownHeader {
        name: "stdlib.h",
        protos: &[
            ("malloc", "extern void *malloc(unsigned long);"),
            (
                "calloc",
                "extern void *calloc(unsigned long, unsigned long);",
            ),
            ("realloc", "extern void *realloc(void *, unsigned long);"),
            ("free", "extern void free(void *);"),
            ("exit", "extern void exit(int);"),
            ("atoi", "extern int atoi(const char *);"),
            ("atof", "extern double atof(const char *);"),
            ("abs", "extern int abs(int);"),
            ("rand", "extern int rand(void);"),
            ("srand", "extern void srand(unsigned int);"),
        ],
        keep_if_used: &["size_t", "qsort", "bsearch", "strtol", "strtod", "atexit"],
    },
    KnownHeader {
        name: "string.h",
        protos: &[
            ("strlen", "extern unsigned long strlen(const char *);"),
            ("strcpy", "extern char *strcpy(char *, const char *);"),
            (
                "strncpy",
                "extern char *strncpy(char *, const char *, unsigned long);",
            ),
            ("strcmp", "extern int strcmp(const char *, const char *);"),
            (
                "strncmp",
                "extern int strncmp(const char *, const char *, unsigned long);",
            ),
            ("strcat", "extern char *strcat(char *, const char *);"),
            (
                "strncat",
                "extern char *strncat(char *, const char *, unsigned long);",
            ),
            ("strchr", "extern char *strchr(const char *, int);"),
            ("strstr", "extern char *strstr(const char *, const char *);"),
            (
                "memcpy",
                "extern void *memcpy(void *, const void *, unsigned long);",
            ),
            (
                "memmove",
                "extern void *memmove(void *, const void *, unsigned long);",
            ),
            ("memset", "extern void *memset(void *, int, unsigned long);"),
        ],
        keep_if_used: &["size_t", "strdup", "strtok", "strerror"],
    },
    KnownHeader {
        name: "math.h",
        protos: &[
            ("pow", "extern double pow(double, double);"),
            ("sqrt", "extern double sqrt(double);"),
            ("fabs", "extern double fabs(double);"),
            ("floor", "extern double floor(double);"),
            ("ceil", "extern double ceil(double);"),
            ("sin", "extern double sin(double);"),
            ("cos", "extern double cos(double);"),
            ("tan", "extern double tan(double);"),
            ("exp", "extern double exp(double);"),
            ("log", "extern double log(double);"),
        ],
        keep_if_used: &[],
    },
];

/// Elide every recognised `#include` whose symbols can all be spelled inline,
/// replacing each with `extern` declarations for the symbols the program uses.
pub fn elide_includes(mut unit: TranslationUnit) -> TranslationUnit {
    let used = collect_used_names(&unit);

    unit.items = unit
        .items
        .into_iter()
        .map(|item| rewrite_item(item, &used))
        .collect();

    unit
}

/// Rewrite one top-level item: an elidable `#include` becomes an [`Item::Raw`]
/// block of `extern` declarations; everything else is returned unchanged.
fn rewrite_item(item: Item, used: &HashSet<String>) -> Item {
    let Item::Directive(line) = &item else {
        return item;
    };
    let Some(header_name) = parse_include(line) else {
        return item; // not an `#include`, or a non-literal one like `#include H`
    };
    let Some(header) = KNOWN_HEADERS.iter().find(|h| h.name == header_name) else {
        return item; // a header we don't model
    };

    // If the program leans on a part of this header we can't inline safely,
    // keep the directive so the header still provides it.
    if header.keep_if_used.iter().any(|sym| used.contains(*sym)) {
        return item;
    }

    // Splice prototypes for exactly the header's symbols the program uses.
    let decls: Vec<&str> = header
        .protos
        .iter()
        .filter(|(sym, _)| used.contains(*sym))
        .map(|(_, decl)| *decl)
        .collect();

    // Nothing from the header is used: drop the directive entirely. (A header
    // included only for a typedef would have been pinned by `keep_if_used`
    // above, so reaching here means the include was genuinely unused.)
    if decls.is_empty() {
        return Item::Raw(String::new());
    }

    Item::Raw(decls.join("\n"))
}

/// Parse an `#include <name>` or `#include "name"` directive, returning the
/// quoted header name. Returns `None` for any other directive, or for a
/// computed include like `#include SOME_MACRO` whose target we cannot resolve.
fn parse_include(line: &str) -> Option<&str> {
    let rest = line.trim_start();
    let rest = rest.strip_prefix('#')?.trim_start();
    let rest = rest.strip_prefix("include")?;

    // Require whitespace after `include` so we don't match an identifier that
    // merely starts with it, then find the delimited header name.
    let rest = rest.strip_prefix([' ', '\t'])?.trim();
    let (open, close) = match rest.chars().next()? {
        '<' => ('<', '>'),
        '"' => ('"', '"'),
        _ => return None,
    };
    let inner = rest.strip_prefix(open)?;
    let end = inner.find(close)?;
    Some(inner[..end].trim())
}

/// Collect every identifier and type-specifier word that appears anywhere in the
/// unit, so the rewrite can tell which standard symbols the program relies on.
fn collect_used_names(unit: &TranslationUnit) -> HashSet<String> {
    let mut names = HashSet::new();
    for item in &unit.items {
        match item {
            Item::Function(func) => {
                walk_type(&func.return_type, &mut names);
                for p in &func.params {
                    walk_type(&p.ty, &mut names);
                }
                if let Some(body) = &func.body {
                    walk_block(body, &mut names);
                }
            }
            Item::Declaration(decl) => walk_declaration(decl, &mut names),
            // Directives and already-rendered Raw text are opaque strings; a
            // symbol only "counts as used" when it appears in real parsed code,
            // so we don't scan these.
            Item::Directive(_) | Item::Raw(_) => {}
        }
    }
    names
}

fn walk_type(ty: &Type, names: &mut HashSet<String>) {
    for spec in &ty.specifiers {
        names.insert(spec.clone());
    }
}

fn walk_declaration(decl: &Declaration, names: &mut HashSet<String>) {
    walk_type(&decl.ty, names);
    for d in &decl.declarators {
        for dim in d.array_dims.iter().flatten() {
            walk_expr(dim, names);
        }
        if let Some(init) = &d.init {
            walk_expr(init, names);
        }
    }
}

fn walk_block(block: &Block, names: &mut HashSet<String>) {
    for stmt in &block.stmts {
        walk_stmt(stmt, names);
    }
}

fn walk_stmt(stmt: &Stmt, names: &mut HashSet<String>) {
    match stmt {
        Stmt::Declaration(decl) => walk_declaration(decl, names),
        Stmt::Expr(expr) => walk_expr(expr, names),
        Stmt::Return(expr) => {
            if let Some(expr) = expr {
                walk_expr(expr, names);
            }
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            walk_expr(cond, names);
            walk_stmt(then_branch, names);
            if let Some(else_branch) = else_branch {
                walk_stmt(else_branch, names);
            }
        }
        Stmt::While { cond, body } => {
            walk_expr(cond, names);
            walk_stmt(body, names);
        }
        Stmt::DoWhile { body, cond } => {
            walk_stmt(body, names);
            walk_expr(cond, names);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(init) = init {
                walk_stmt(init, names);
            }
            if let Some(cond) = cond {
                walk_expr(cond, names);
            }
            if let Some(step) = step {
                walk_expr(step, names);
            }
            walk_stmt(body, names);
        }
        Stmt::Block(block) => walk_block(block, names),
        Stmt::Switch { cond, body } => {
            walk_expr(cond, names);
            walk_stmt(body, names);
        }
        Stmt::Case { value, body } => {
            walk_expr(value, names);
            walk_stmt(body, names);
        }
        Stmt::Default { body } | Stmt::Label { body, .. } => walk_stmt(body, names),
        Stmt::Break | Stmt::Continue | Stmt::Empty | Stmt::Goto(_) => {}
    }
}

fn walk_expr(expr: &Expr, names: &mut HashSet<String>) {
    match expr {
        Expr::Ident(name) => {
            names.insert(name.clone());
        }
        Expr::Int(_) | Expr::Str(_) | Expr::Char(_) | Expr::Raw(_) => {}
        Expr::Unary { expr, .. } | Expr::Postfix { expr, .. } => walk_expr(expr, names),
        Expr::Binary { lhs, rhs, .. } | Expr::Comma { lhs, rhs } => {
            walk_expr(lhs, names);
            walk_expr(rhs, names);
        }
        Expr::Assign { target, value, .. } => {
            walk_expr(target, names);
            walk_expr(value, names);
        }
        Expr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => {
            walk_expr(cond, names);
            walk_expr(then_expr, names);
            walk_expr(else_expr, names);
        }
        Expr::Cast { ty, expr } => {
            walk_type(ty, names);
            walk_expr(expr, names);
        }
        Expr::Call { callee, args } => {
            walk_expr(callee, names);
            for arg in args {
                walk_expr(arg, names);
            }
        }
        Expr::Index { base, index } => {
            walk_expr(base, names);
            walk_expr(index, names);
        }
        Expr::Member { base, .. } => walk_expr(base, names),
        Expr::InitList(elems) => {
            for elem in elems {
                walk_expr(elem, names);
            }
        }
    }
}
