//! Structural front end: turn the token stream into an [`ast::TranslationUnit`]
//! and render it back to C.
//!
//! This module is the foundation for structural obfuscation (control-flow
//! rewriting, opaque predicates, dead-code insertion), which needs the shape
//! of the program, not just a flat token stream. It is not yet wired into the
//! transform pipeline: the token-level passes always succeed, whereas parsing
//! fails on C outside the supported subset, so the intended integration is a
//! *try-parse, fall back to the token passes* path once transforms exist.
//! Until then these items are unused by `main`, hence the `dead_code` allow.
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
}
