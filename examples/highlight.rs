//! A syntax highlighter from the token stream alone.
//!
//! `Parser::tokens` runs only the generated lexer: every token in order,
//! comments and whitespace included, and an error for each character no token
//! matches, after which it carries on. That is exactly what an editor's
//! highlighter needs — it never stops at the first mistake.
//!
//! ```text
//! cargo run --example highlight
//! ```

use grammar_lang::{Grammar, Parser};

fn language() -> Parser {
    let built = Grammar::new()
        .literal("fn")
        .literal("let")
        .literal("return")
        .literal("(")
        .literal(")")
        .literal("{")
        .literal("}")
        .literal(";")
        .literal("=")
        .literal("+")
        .literal("*")
        .pattern("ident", "[a-zA-Z_][a-zA-Z0-9_]*")
        .pattern("number", "[0-9]+")
        .pattern("string", r#""([^"\\]|\\.)*""#)
        .skip("space", r"\s+")
        .skip("comment", "//[^\n]*")
        .rule("program", &["program", "ident"])
        .rule("program", &[])
        .build();
    match built {
        Ok(parser) => parser,
        Err(err) => panic!("the grammar is invalid: {err}"),
    }
}

/// The highlight class of a token, by its name.
fn class(name: &str) -> &'static str {
    match name {
        "fn" | "let" | "return" => "keyword",
        "ident" => "name",
        "number" | "string" => "literal",
        "comment" => "comment",
        "space" => "space",
        _ => "punct",
    }
}

const SOURCE: &str = r#"// Squares a number.
fn square(x) { return x * x; }
let greeting = "hi" + 2 # 1;
"#;

fn main() {
    let parser = language();
    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    let mut line = String::new();
    for item in parser.tokens(SOURCE) {
        match item {
            Ok(token) => {
                let span = token.span();
                let text = &SOURCE[span.start().to_usize()..span.end().to_usize()];
                let class = class(parser.name(*token.kind()));
                *counts.entry(class).or_default() += 1;
                if token.is_trivia() && text.contains('\n') {
                    println!("{line}");
                    line.clear();
                } else if !token.is_trivia() {
                    line.push_str(&format!("<{class}>{text}</> "));
                } else if class == "comment" {
                    line.push_str(&format!("<comment>{text}</> "));
                }
            }
            Err(err) => {
                *counts.entry("error").or_default() += 1;
                line.push_str(&format!("<error title=\"{err}\">?</> "));
            }
        }
    }
    if !line.is_empty() {
        println!("{line}");
    }
    println!("\ntoken classes: {counts:?}");
}
