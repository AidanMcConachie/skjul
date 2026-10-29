use axum::{
    extract::Form,
    http::header,
    response::Html,
    routing::{get, post},
    Router,
};

use serde::Deserialize;

use std::collections::HashMap;

mod obfuscation;
mod parsing;
mod tokenization;
mod transformation;

use obfuscation::literals::obfuscate_literals;
use tokenization::token::{Token, TokenType};

#[derive(Deserialize)]
struct Input {
    text: String,
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(index))
        .route("/styles.css", get(styles))
        .route("/transform", post(transform));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();

    axum::serve(listener, app).await.unwrap();
}

async fn index() -> Html<&'static str> {
    Html(include_str!("static/index.html"))
}

async fn styles() -> ([(header::HeaderName, &'static str); 1], &'static str) {
    (
        [(header::CONTENT_TYPE, "text/css")],
        include_str!("static/styles.css"),
    )
}

async fn transform(Form(input): Form<Input>) -> Html<String> {
    let (heading, body) = match obfuscate(&input.text) {
        Ok(code) => ("Obfuscated", code),
        Err(error) => ("Error", error),
    };

    Html(result_page(heading, &body))
}

/// The full obfuscation pipeline.
///
/// When the source parses into the AST subset, the structural passes
/// (control-flow flattening, opaque predicates) rewrite its shape and it is
/// printed back to C. When it does not — a construct outside the subset — we
/// fall back to the original source untouched. Either way the token-level
/// passes finish the job: rename identifiers, disguise every literal, then
/// strip the stream down to minimal whitespace. Each pass preserves the
/// program's behaviour; together they leave little of the original.
fn obfuscate(source: &str) -> Result<String, String> {
    // Deterministic seed → the same source always produces the same output.
    // The constant is the 64-bit golden ratio, a conventional mixing value.
    const SEED: u64 = 0x9E37_79B9_7F4A_7C15;

    let tokens = tokenization::lexer::tokenize(source)?;

    // Structural (AST) path, with a fall back to the source on a parse error.
    let staged = match parsing::parser::parse(tokens) {
        Ok(unit) => {
            let mut rng = transformation::Rng::new(SEED);
            parsing::printer::print_unit(&transformation::transform(unit, &mut rng))
        }
        Err(_) => source.to_string(),
    };

    // Re-lex the staged C and run the token-level passes over it.
    let tokens = tokenization::lexer::tokenize(&staged)?;
    Ok(minify(&obfuscate_literals(rename_identifiers(tokens))))
}

/// Render the obfuscated code (or an error) as a full HTML page. The code is
/// HTML-escaped so characters like `<`, `>` and `&` — common in C and in the
/// directives we pass through — display literally instead of being parsed as
/// markup.
fn result_page(heading: &str, body: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n\
         <meta charset=\"UTF-8\">\n<title>skjul</title>\n\
         <link rel=\"stylesheet\" href=\"/styles.css\">\n</head>\n<body>\n\
         <h1>{heading}</h1>\n\
         <pre class=\"result\">{}</pre>\n\
         <a class=\"back\" href=\"/\">&larr; Obfuscate more</a>\n\
         </body>\n</html>",
        html_escape(body)
    )
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Replace every renamable identifier with an opaque, deliberately
/// hard-to-read name (`_l`, `_I`, `_O`, `_o`, `_0`, `_lI`, …) built from
/// look-alike characters, consistently: the same source name always maps to
/// the same new one.
///
/// Reserved names are left untouched. The lexer only recognises `int` and
/// `return` as keywords, so every other C keyword (`char`, `if`, `for`, …)
/// reaches us as an identifier; renaming those — or libc symbols like
/// `printf`, or the entry point `main` — would stop the code compiling.
fn rename_identifiers(tokens: Vec<Token>) -> Vec<Token> {
    let mut map: HashMap<String, String> = HashMap::new();

    // Names mentioned in a preprocessor directive — macro names, macro
    // parameters, anything a header pulls in. We pass directives through
    // verbatim, so renaming a use of such a name in the code body would break
    // the link to the directive (e.g. the call `SQ(i)` and its `#define SQ`).
    let from_directives = directive_identifiers(&tokens);

    tokens
        .into_iter()
        .map(|mut token| {
            if let TokenType::IDENTIFIER(name) = &token.kind {
                if !is_reserved(name) && !from_directives.contains(name) {
                    let new_name = match map.get(name) {
                        Some(existing) => existing.clone(),
                        None => {
                            let generated = opaque_name(map.len());
                            map.insert(name.clone(), generated.clone());
                            generated
                        }
                    };
                    token.kind = TokenType::IDENTIFIER(new_name);
                }
            }
            token
        })
        .collect()
}

/// Build the opaque replacement name for the `index`-th renamed identifier.
///
/// The names are drawn from a small alphabet of look-alike glyphs (lowercase
/// L, uppercase I and O, lowercase o, digit 0) and always start with `_`, so
/// every result is a valid identifier that is tedious to tell apart from its
/// neighbours by eye. Encoding `index` in this alphabet keeps the mapping
/// injective, so distinct identifiers never collide.
fn opaque_name(index: usize) -> String {
    const ALPHABET: [char; 5] = ['l', 'I', 'O', 'o', '0'];
    let mut n = index;
    let mut name = String::from("_");
    loop {
        name.push(ALPHABET[n % ALPHABET.len()]);
        n /= ALPHABET.len();
        if n == 0 {
            break;
        }
    }
    name
}

/// Collect every identifier-like word appearing in a preprocessor directive,
/// so [`rename_identifiers`] can leave those names untouched in the code body.
fn directive_identifiers(tokens: &[Token]) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    for token in tokens {
        let TokenType::PREPROCESSOR(line) = &token.kind else {
            continue;
        };
        let mut word = String::new();
        for c in line.chars() {
            if c.is_ascii_alphanumeric() || c == '_' {
                word.push(c);
            } else if !word.is_empty() {
                names.insert(std::mem::take(&mut word));
            }
        }
        if !word.is_empty() {
            names.insert(word);
        }
    }
    names
}

/// Names we must never rename: C keywords the lexer doesn't tokenise on its
/// own, the entry point, and common standard-library symbols.
fn is_reserved(name: &str) -> bool {
    matches!(
        name,
        // keywords not already handled by the lexer
        "char" | "void" | "float" | "double" | "long" | "short"
        | "unsigned" | "signed" | "struct" | "union" | "enum"
        | "typedef" | "static" | "const" | "volatile" | "extern"
        | "register" | "auto" | "if" | "else" | "for" | "while"
        | "do" | "switch" | "case" | "default" | "break" | "continue"
        | "goto" | "sizeof"
        // entry point and common libc symbols
        | "main" | "NULL"
        | "printf" | "scanf" | "puts" | "putchar" | "getchar"
        | "fprintf" | "fscanf" | "sprintf" | "sscanf"
        | "fopen" | "fclose" | "fgets" | "fputs" | "fread" | "fwrite"
        | "malloc" | "calloc" | "realloc" | "free"
        | "memcpy" | "memmove" | "memset"
        | "strlen" | "strcpy" | "strncpy" | "strcmp" | "strncmp"
        | "strcat" | "strncat" | "strchr" | "strstr"
        | "exit" | "atoi" | "atof" | "abs" | "pow" | "sqrt"
    )
}

/// Emit the token stream with the minimal whitespace needed to keep it
/// meaning the same, stripping every space, newline and comment the author
/// wrote.
fn minify(tokens: &[Token]) -> String {
    let kinds: Vec<&TokenType> = tokens
        .iter()
        .map(|token| &token.kind)
        .filter(|kind| **kind != TokenType::EOF)
        .collect();

    let mut out = String::new();
    for (i, kind) in kinds.iter().enumerate() {
        let text = kind.text();
        if i > 0 {
            // Directives must stand alone on their own line; the preprocessor
            // is line-based, so a directive can neither share a line with what
            // comes before nor let code trail onto its line.
            let prev_is_directive = matches!(kinds[i - 1], TokenType::PREPROCESSOR(_));
            let this_is_directive = matches!(kind, TokenType::PREPROCESSOR(_));
            if prev_is_directive || this_is_directive {
                out.push('\n');
            } else if needs_separator(&kinds[i - 1].text(), &text) {
                out.push(' ');
            }
        }
        out.push_str(&text);
    }
    out
}

/// A space is required between two tokens only when dropping it would let
/// their text fuse into a different token: two word-like tokens (`int x`
/// must not become `intx`), or symbol pairs that would form a longer
/// operator or a comment opener (`a / /b` must not become `a//b`).
fn needs_separator(prev: &str, next: &str) -> bool {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let (Some(last), Some(first)) = (prev.chars().last(), next.chars().next()) else {
        return false;
    };

    if is_word(last) && is_word(first) {
        return true;
    }

    matches!(
        (last, first),
        ('/', '/') | ('/', '*') | ('*', '/') | ('+', '+') | ('-', '-')
    )
}
