//! A complete JSON parser in about forty lines of grammar.
//!
//! Semantic actions build a typed `Json` value directly; string contents are
//! borrowed from the input when they hold no escapes. Malformed documents
//! report exactly what was expected where.
//!
//! ```text
//! cargo run --example json
//! cargo run --example json -- path/to/file.json
//! ```

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::vec::Drain;

use grammar_lang::{Actions, Grammar, Kind, Parser, Production, Span, Token};

/// A JSON value. Strings borrow from the input unless they had escapes.
#[derive(Debug, PartialEq)]
enum Json<'s> {
    Null,
    Bool(bool),
    Number(f64),
    String(Cow<'s, str>),
    Array(Vec<Json<'s>>),
    Object(BTreeMap<Cow<'s, str>, Json<'s>>),
}

/// The JSON grammar (RFC 8259). Production numbers are in the comments.
fn json() -> Parser {
    let built = Grammar::new()
        .literal("{")
        .literal("}")
        .literal("[")
        .literal("]")
        .literal(":")
        .literal(",")
        .literal("true")
        .literal("false")
        .literal("null")
        .pattern(
            "string",
            r#""([^"\\\x00-\x1F]|\\["\\/bfnrt]|\\u[0-9a-fA-F]{4})*""#,
        )
        .pattern("number", r"-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?")
        .skip("whitespace", r"[ \t\n\r]+")
        .rule("value", &["object"]) // 0
        .rule("value", &["array"]) // 1
        .rule("value", &["string"]) // 2
        .rule("value", &["number"]) // 3
        .rule("value", &["true"]) // 4
        .rule("value", &["false"]) // 5
        .rule("value", &["null"]) // 6
        .rule("object", &["{", "}"]) // 7
        .rule("object", &["{", "members", "}"]) // 8
        .rule("members", &["members", ",", "member"]) // 9
        .rule("members", &["member"]) // 10
        .rule("member", &["string", ":", "value"]) // 11
        .rule("array", &["[", "]"]) // 12
        .rule("array", &["[", "elements", "]"]) // 13
        .rule("elements", &["elements", ",", "value"]) // 14
        .rule("elements", &["value"]) // 15
        .build();
    match built {
        Ok(parser) => parser,
        Err(err) => panic!("the JSON grammar is invalid: {err}"),
    }
}

/// What actions pass between each other: finished values, plus the
/// in-progress pieces of objects and arrays.
enum Part<'s> {
    Value(Json<'s>),
    Member(Cow<'s, str>, Json<'s>),
    Members(BTreeMap<Cow<'s, str>, Json<'s>>),
    Elements(Vec<Json<'s>>),
    Text(&'s str),
}

struct Build;

impl<'s> Actions<'s> for Build {
    type Value = Part<'s>;

    fn token(&mut self, _token: Token<Kind>, text: &'s str) -> Part<'s> {
        Part::Text(text)
    }

    fn reduce(&mut self, p: Production, _span: Span, mut kids: Drain<'_, Part<'s>>) -> Part<'s> {
        // `take(n)` passes over `n` children (punctuation) and returns the
        // next one.
        let mut take = |n: usize| kids.nth(n).unwrap_or(Part::Value(Json::Null));
        match p.index() {
            // value → object | array
            0 | 1 => take(0),
            2 => match take(0) {
                Part::Text(raw) => Part::Value(Json::String(unescape(raw))),
                other => other,
            },
            3 => match take(0) {
                Part::Text(raw) => Part::Value(Json::Number(raw.parse().unwrap_or(f64::NAN))),
                other => other,
            },
            4 => Part::Value(Json::Bool(true)),
            5 => Part::Value(Json::Bool(false)),
            6 => Part::Value(Json::Null),
            7 => Part::Value(Json::Object(BTreeMap::new())),
            8 => match take(1) {
                Part::Members(map) => Part::Value(Json::Object(map)),
                other => other,
            },
            9 => match (take(0), take(1)) {
                (Part::Members(mut map), Part::Member(key, value)) => {
                    map.insert(key, value);
                    Part::Members(map)
                }
                (first, _) => first,
            },
            10 => match take(0) {
                Part::Member(key, value) => Part::Members(BTreeMap::from([(key, value)])),
                other => other,
            },
            11 => match (take(0), take(1)) {
                (Part::Text(raw), Part::Value(value)) => Part::Member(unescape(raw), value),
                (first, _) => first,
            },
            12 => Part::Value(Json::Array(Vec::new())),
            13 => match take(1) {
                Part::Elements(items) => Part::Value(Json::Array(items)),
                other => other,
            },
            14 => match (take(0), take(1)) {
                (Part::Elements(mut items), Part::Value(value)) => {
                    items.push(value);
                    Part::Elements(items)
                }
                (first, _) => first,
            },
            _ => match take(0) {
                Part::Value(value) => Part::Elements(vec![value]),
                other => other,
            },
        }
    }
}

/// Strips the quotes and decodes escapes, borrowing when there are none.
fn unescape(raw: &str) -> Cow<'_, str> {
    let body = &raw[1..raw.len() - 1];
    if !body.contains('\\') {
        return Cow::Borrowed(body);
    }
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                let code = u32::from_str_radix(&hex, 16).unwrap_or(0xFFFD);
                out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    Cow::Owned(out)
}

impl fmt::Display for Json<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Json::Null => f.write_str("null"),
            Json::Bool(b) => write!(f, "{b}"),
            Json::Number(n) => write!(f, "{n}"),
            Json::String(s) => write!(f, "{s:?}"),
            Json::Array(items) => {
                f.write_str("[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }
            Json::Object(map) => {
                f.write_str("{")?;
                for (i, (key, value)) in map.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{key:?}: {value}")?;
                }
                f.write_str("}")
            }
        }
    }
}

const SAMPLE: &str = r#"
{
    "name": "grammar-lang",
    "tags": ["parser", "lalr", "dfa"],
    "version": 0.2,
    "stable": false,
    "notes": "tab:\there, snowman: ☃",
    "deps": { "token-lang": "1", "diag-lang": "1" },
    "empty": [],
    "nothing": null
}
"#;

fn main() {
    let parser = json();
    let text = match std::env::args().nth(1) {
        Some(path) => match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) => {
                eprintln!("cannot read {path}: {err}");
                std::process::exit(1);
            }
        },
        None => SAMPLE.to_string(),
    };
    match parser.parse_with(&text, &mut Build) {
        Ok(Part::Value(value)) => println!("{value}"),
        Ok(_) => println!("(not a value)"),
        Err(err) => println!("error at {}: {err}", err.span()),
    }

    for broken in [r#"{"a": 1,}"#, r#"[1 2]"#, r#"{"a" 1}"#, r#"["open"#] {
        match parser.parse(broken) {
            Ok(_) => println!("{broken:>12}  parsed"),
            Err(err) => println!("{broken:>12}  {err}"),
        }
    }
}
