//! End-to-end behaviour-preservation tests.
//!
//! Each corpus program is run through the full obfuscation pipeline
//! ([`crate::obfuscate`]) and then compiled and executed with the system `gcc`,
//! alongside the original. The test asserts that the obfuscated program
//! compiles and produces byte-identical stdout and the same exit status as the
//! original — the property every pass in this crate must preserve.
//!
//! The corpus deliberately spans the pipeline's branches: programs well inside
//! the parser subset (which get control-flow flattening and opaque predicates),
//! programs that parse but defeat flattening per-function (array-with-
//! initialiser bodies), and programs outside the subset entirely (an inline
//! `struct` body) that fall back to the token-level passes.
//!
//! If `gcc` is not on `PATH` the test skips rather than fails, so the suite
//! still runs in environments without a C compiler.
//!
//! The lexer handles decimal, hex (`0x…`), binary (`0b…`) and octal integers,
//! integer suffixes (`100UL`), floating-point constants, and the full set of
//! compound assignments including `<<= >>= &= |= ^=`. A leading-dot float
//! (`.5`) still lexes as `. 5` but survives the round trip.

use crate::obfuscate;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `(name, source)` for each self-contained, deterministic C program. They use
/// only `<stdio.h>`, take no input, and avoid undefined behaviour.
const CORPUS: &[(&str, &str)] = &[
    (
        "arithmetic_loop",
        r#"
#include <stdio.h>
int main() {
    long sum = 0;
    for (int i = 1; i <= 100; i++) {
        sum += (long)i * i;
    }
    printf("sum=%ld\n", sum);
    return 0;
}
"#,
    ),
    (
        "recursion",
        r#"
#include <stdio.h>
int fib(int n) {
    if (n < 2) return n;
    return fib(n - 1) + fib(n - 2);
}
long fact(int n) {
    long r = 1;
    while (n > 1) {
        r *= n;
        n--;
    }
    return r;
}
int main() {
    for (int i = 0; i < 10; i++) {
        printf("%d ", fib(i));
    }
    printf("\n%ld\n", fact(10));
    return 0;
}
"#,
    ),
    (
        "control_flow",
        r#"
#include <stdio.h>
int main() {
    int total = 0;
    for (int i = 0; i < 20; i++) {
        if (i % 3 == 0) {
            total += i;
        } else if (i % 2 == 0) {
            total -= 1;
        } else {
            total++;
        }
    }
    int n = total;
    int steps = 0;
    do {
        n /= 2;
        steps++;
    } while (n > 1);
    printf("total=%d steps=%d\n", total, steps);
    return 0;
}
"#,
    ),
    (
        "switch_goto",
        r#"
#include <stdio.h>
int main() {
    int acc = 0;
    int n = 0;
loop:
    switch (n % 4) {
        case 0: acc += 1; break;
        case 1: acc += 10; break;
        case 2: acc += 100; break;
        default: acc += 1000; break;
    }
    n++;
    if (n < 9) goto loop;
    printf("acc=%d\n", acc);
    return 0;
}
"#,
    ),
    (
        "pointers_arrays",
        r#"
#include <stdio.h>
int main() {
    int a[8];
    for (int i = 0; i < 8; i++) {
        a[i] = i * i;
    }
    int *p = a;
    int sum = 0;
    for (int i = 0; i < 8; i++) {
        sum += *(p + i);
    }
    printf("sum=%d first=%d last=%d\n", sum, a[0], a[7]);
    return 0;
}
"#,
    ),
    (
        "strings_chars",
        r#"
#include <stdio.h>
int main() {
    char msg[] = "Hello";
    int len = 0;
    while (msg[len] != '\0') {
        len++;
    }
    printf("msg=%s len=%d tab[\t]end\n", msg, len);
    putchar('A');
    putchar('\n');
    return 0;
}
"#,
    ),
    (
        // Stresses string encryption: a literal reused across loop iterations,
        // two literals live at once in one `printf`, a literal returned from a
        // function, a literal indexed directly, and literals carrying escapes.
        "string_encryption",
        r#"
#include <stdio.h>
const char *label(int ok) {
    if (ok) return "yes";
    return "no";
}
int main() {
    for (int i = 0; i < 3; i++) {
        printf("row %d: %s\n", i, "repeated");
    }
    printf("%s / %s\n", "left", "right");
    printf("tab\tnewline-escaped\n");
    printf("first char of 'skjul' is %c\n", "skjul"[0]);
    printf("%s %s empty[%s]\n", label(1), label(0), "");
    return 0;
}
"#,
    ),
    (
        "expressions",
        r#"
#include <stdio.h>
int main() {
    unsigned x = 43981;
    int y = (int)(x % 256);
    int z = (y > 100) ? (y - 100) : (y + 100);
    z *= 4;
    z = z + 85;
    printf("y=%d z=%d max=%d\n", y, z, (y > z) ? y : z);
    return 0;
}
"#,
    ),
    (
        // Inline `struct` body is outside the parser subset: this exercises the
        // token-level fallback path end to end.
        "struct_fallback",
        r#"
#include <stdio.h>
struct Point { int x; int y; };
int dist2(struct Point p) {
    return p.x * p.x + p.y * p.y;
}
int main() {
    struct Point pts[3];
    for (int i = 0; i < 3; i++) {
        pts[i].x = i;
        pts[i].y = i + 1;
    }
    int total = 0;
    for (int i = 0; i < 3; i++) {
        total += dist2(pts[i]);
    }
    printf("total=%d\n", total);
    return 0;
}
"#,
    ),
    (
        // Exercises the lexer's numeric and compound-assignment coverage end
        // to end: hex, octal and binary integer bases, an integer suffix, a
        // float (including a leading-dot form), and the `<<= >>= &= |= ^=`
        // compound assignments. Each base matters — `010` is 8, not 10 — so
        // byte-identical output proves they survive verbatim.
        "number_bases_and_compound_assign",
        r#"
#include <stdio.h>
int main() {
    unsigned long mask = 0xFF;
    int oct = 010;
    int bin = 0b1010;
    unsigned long big = 100UL;
    int x = 1;
    x <<= 4;
    x |= 0b0011;
    x &= 0xFE;
    x ^= 5;
    int y = 64;
    y >>= 2;
    double d = 3.5 + 1e1 + .5;
    printf("mask=%lu oct=%d bin=%d big=%lu x=%d y=%d d=%.1f\n",
           mask, oct, bin, big, x, y, d);
    return 0;
}
"#,
    ),
    (
        "nonzero_exit",
        r#"
#include <stdio.h>
int main() {
    int code = 0;
    for (int i = 0; i < 5; i++) {
        code += i;
    }
    printf("code=%d\n", code);
    return code % 7;
}
"#,
    ),
];

/// Whether a working `gcc` is on `PATH`.
fn gcc_available() -> bool {
    Command::new("gcc")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// A fresh, process-unique scratch directory under the system temp dir.
fn temp_dir() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("skjul_corpus_{}_{}", std::process::id(), nanos));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Compile `source` to an executable, returning its path or the compiler's
/// stderr on failure. `-w` silences the (expected, plentiful) warnings the
/// obfuscated output provokes; it does not affect whether code compiles.
fn compile(dir: &Path, name: &str, source: &str) -> Result<PathBuf, String> {
    let src_path = dir.join(format!("{name}.c"));
    let bin_path = dir.join(name);
    std::fs::write(&src_path, source).expect("write source");

    let output = Command::new("gcc")
        .args(["-std=c11", "-w", "-o"])
        .arg(&bin_path)
        .arg(&src_path)
        .output()
        .expect("run gcc");

    if output.status.success() {
        Ok(bin_path)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

/// Run a compiled program, returning its stdout and exit code.
fn run(bin: &Path) -> (String, Option<i32>) {
    let output = Command::new(bin).output().expect("run program");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.status.code(),
    )
}

#[test]
fn obfuscated_programs_behave_identically() {
    if !gcc_available() {
        eprintln!("skipping corpus test: gcc not found on PATH");
        return;
    }

    let dir = temp_dir();

    for (name, source) in CORPUS {
        // The pipeline is seeded, so obfuscation must be reproducible.
        let obfuscated = obfuscate(source)
            .unwrap_or_else(|e| panic!("[{name}] obfuscation failed: {e}"));
        let again = obfuscate(source).expect("re-obfuscate");
        assert_eq!(obfuscated, again, "[{name}] obfuscation is not deterministic");

        let orig_bin = compile(&dir, &format!("{name}_orig"), source)
            .unwrap_or_else(|e| panic!("[{name}] original program did not compile:\n{e}"));
        let obf_bin = compile(&dir, &format!("{name}_obf"), &obfuscated)
            .unwrap_or_else(|e| {
                panic!("[{name}] obfuscated program did not compile:\n{e}\n--- output ---\n{obfuscated}")
            });

        let (orig_out, orig_code) = run(&orig_bin);
        let (obf_out, obf_code) = run(&obf_bin);

        assert_eq!(
            orig_out, obf_out,
            "[{name}] stdout differs\n--- obfuscated source ---\n{obfuscated}"
        );
        assert_eq!(orig_code, obf_code, "[{name}] exit code differs");
    }

    // Best-effort cleanup; leaving the dir behind on panic aids debugging.
    let _ = std::fs::remove_dir_all(&dir);
}
