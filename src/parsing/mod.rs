//! Structural front end: turn the token stream into an [`ast::TranslationUnit`]
//! and render it back to C.
//!
//! This module is the foundation for structural obfuscation (control-flow
//! rewriting, opaque predicates, dead-code insertion), which needs the shape
//! of the program, not just a flat token stream. `main`'s `obfuscate` runs a
//! *try-parse, fall back to the token passes* path: when the source parses into
//! this subset it is transformed structurally and printed back to C, and when
//! it does not, the token-level passes run over the original source instead.
//!
//! A few constructors and helpers here are kept for completeness even when no
//! pass currently builds them, hence the `dead_code` allow.
#![allow(dead_code)]

pub mod ast;
pub mod parser;
pub mod printer;

#[cfg(test)]
mod tests {
    use super::parser::parse;
    use super::printer::print_unit;
    use crate::tokenization::lexer::tokenize;

    /// Parse `source`, print the AST back to C, and return the result.
    fn roundtrip(source: &str) -> String {
        let tokens = tokenize(source).expect("lexing failed");
        let unit = parse(tokens).expect("parsing failed");
        print_unit(&unit)
    }

    #[test]
    fn roundtrips_a_representative_program() {
        let source = r#"
#include <stdio.h>
#define SQ(x) ((x)*(x))

int g = 3;
int table[3] = {0};

int add(int a, int b);

int add(int a, int b) {
    return a + b;
}

int main() {
    int count = 10;
    char grade = 'A';
    int total = 0;
    for (int i = 0; i < count; i++) {
        if (i % 2 == 0) {
            total += SQ(i);
        } else {
            total = total + add(i, 1);
        }
    }
    int *p = &total;
    while (*p > 1000) {
        *p -= 100;
    }
    int n = 0;
    do {
        table[n % 3] += n;
        n++;
    } while (n < 5);
    int big = (total > 100) ? total : -total;
    unsigned char c = (unsigned char)(grade + 1);
    printf("sum=%d grade=%c next=%c big=%d pi~%d\n", total, grade, c, big, 3);
    puts("line one " "and two");
    return 0;
}
"#;
        let out = roundtrip(source);

        // Directives survive verbatim.
        assert!(out.contains("#include <stdio.h>"));
        assert!(out.contains("#define SQ(x) ((x)*(x))"));
        // Structure is recovered.
        assert!(out.contains("int add("));
        assert!(out.contains("int main("));
        assert!(out.contains("for("));
        assert!(out.contains("while("));
        assert!(out.contains(" else"));

        // Write it out so an external `gcc` round-trip check can compile it.
        if let Ok(path) = std::env::var("ROUNDTRIP_OUT") {
            std::fs::write(path, out).expect("writing round-trip output failed");
        }
    }

    #[test]
    fn roundtrips_aggregate_types_and_sizeof() {
        let source = r#"
typedef struct Point { int x; int y; } Point;
enum Dir { NORTH, EAST = 10, SOUTH };
union Word { int i; char b[4]; };
struct Empty;

int measure(Point p) {
    enum Dir d = EAST;
    union Word w;
    w.i = sizeof(Point) + sizeof(int);
    return p.x + (int)d + w.i + sizeof w;
}
"#;
        let out = roundtrip(source);

        // The inline bodies are reproduced, tag and all.
        assert!(out.contains("struct Point{"));
        assert!(out.contains("enum Dir{"));
        assert!(out.contains("union Word{"));
        // The enumerator's explicit value survives.
        assert!(out.contains("EAST=10"));
        // A bare forward declaration keeps no stray declarator.
        assert!(out.contains("struct Empty;"));
        // The typedef name is recognised as a type in a later use (`Point p`),
        // so the function parses rather than falling back.
        assert!(out.contains("measure("));
        // Both `sizeof` forms print.
        assert!(out.contains("sizeof(Point)"));
        assert!(out.contains("sizeof w"));
    }
}
