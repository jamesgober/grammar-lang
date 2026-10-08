//! Rendering parse errors with `diag-lang`.
//!
//! A `ParseError` converts into a `diag_lang::Diagnostic`, which the family's
//! renderer prints against the source with the offending span underlined.
//! The error lists exactly the tokens that could have come next.
//!
//! ```text
//! cargo run --example diagnostics
//! ```

use diag_lang::{Renderer, SourceMap};
use grammar_lang::{Grammar, Parser};

/// A small configuration language: `key = value` lines in `[sections]`.
fn config() -> Parser {
    let built = Grammar::new()
        .literal("[")
        .literal("]")
        .literal("=")
        .literal("true")
        .literal("false")
        .pattern("key", "[a-z][a-z0-9_.-]*")
        .pattern("string", r#""[^"\n]*""#)
        .pattern("number", "-?[0-9]+")
        .skip("space", "[ \t\r\n]+")
        .skip("comment", "#[^\n]*")
        .rule("file", &["file", "item"])
        .rule("file", &[])
        .rule("item", &["[", "key", "]"])
        .rule("item", &["key", "=", "value"])
        .rule("value", &["string"])
        .rule("value", &["number"])
        .rule("value", &["true"])
        .rule("value", &["false"])
        .build();
    match built {
        Ok(parser) => parser,
        Err(err) => panic!("the configuration grammar is invalid: {err}"),
    }
}

const FILES: &[(&str, &str)] = &[
    (
        "server.conf",
        "# Server settings\n[server]\nhost = \"example.org\"\nport = 8080\ntls = true\n",
    ),
    ("missing-value.conf", "[server]\nhost =\nport = 8080\n"),
    ("unclosed.conf", "[server\nhost = \"x\"\n"),
    ("stray.conf", "[server]\nport = 80 80\n"),
    ("bad-char.conf", "[server]\nport = @80\n"),
    ("unterminated.conf", "[server]\nname = \"half\n"),
];

fn main() {
    let parser = config();
    let renderer = Renderer::new();
    for &(name, text) in FILES {
        let mut map = SourceMap::new();
        if let Err(err) = map.add(name, text) {
            println!("{name}: cannot register source: {err:?}");
            continue;
        }
        match parser.parse(text) {
            Ok(tree) => {
                let items = tree
                    .root()
                    .descendants()
                    .filter(|n| n.name() == "item")
                    .count();
                println!("{name}: ok, {items} items\n");
            }
            Err(err) => println!("{}", renderer.render(&err.to_diagnostic(), &map)),
        }
    }
}
