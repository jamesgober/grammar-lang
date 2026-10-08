//! A calculator that evaluates while it parses.
//!
//! The grammar is the textbook ambiguous expression grammar; precedence and
//! associativity declarations settle it, and semantic actions compute each
//! value as soon as its production is recognized — no tree is built.
//!
//! ```text
//! cargo run --example calculator
//! cargo run --example calculator -- "2 ^ 3 ^ 2" "-(1.5 + 2) * 4"
//! ```

use std::vec::Drain;

use grammar_lang::{Actions, Grammar, Kind, Parser, Production, Span, Token};

/// Builds the calculator's parser. Production numbers (the comments) are
/// what `Production::index` reports.
fn calculator() -> Parser {
    let built = Grammar::new()
        .literal("+")
        .literal("-")
        .literal("*")
        .literal("/")
        .literal("%")
        .literal("^")
        .literal("(")
        .literal(")")
        .pattern("number", r"[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?")
        .skip("space", r"\s+")
        .left(&["+", "-"])
        .left(&["*", "/", "%"])
        .right(&["NEG"])
        .right(&["^"])
        .rule("expr", &["expr", "+", "expr"]) // 0
        .rule("expr", &["expr", "-", "expr"]) // 1
        .rule("expr", &["expr", "*", "expr"]) // 2
        .rule("expr", &["expr", "/", "expr"]) // 3
        .rule("expr", &["expr", "%", "expr"]) // 4
        .rule("expr", &["expr", "^", "expr"]) // 5
        .rule("expr", &["-", "expr"]) // 6
        .prec("NEG")
        .rule("expr", &["(", "expr", ")"]) // 7
        .rule("expr", &["number"]) // 8
        .build();
    match built {
        Ok(parser) => parser,
        Err(err) => panic!("the calculator grammar is invalid: {err}"),
    }
}

/// The semantic actions: every token and production becomes an `f64`.
struct Evaluate;

impl<'s> Actions<'s> for Evaluate {
    type Value = f64;

    fn token(&mut self, _token: Token<Kind>, text: &'s str) -> f64 {
        // Operators and parentheses parse as NaN and are never used.
        text.parse().unwrap_or(f64::NAN)
    }

    fn reduce(&mut self, production: Production, _span: Span, mut children: Drain<'_, f64>) -> f64 {
        let mut next = || children.next().unwrap_or(f64::NAN);
        match production.index() {
            0..=5 => {
                let (a, _, b) = (next(), next(), next());
                match production.index() {
                    0 => a + b,
                    1 => a - b,
                    2 => a * b,
                    3 => a / b,
                    4 => a % b,
                    _ => a.powf(b),
                }
            }
            6 => {
                let _minus = next();
                -next()
            }
            7 => {
                let _open = next();
                next()
            }
            _ => next(),
        }
    }
}

fn main() {
    let parser = calculator();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let inputs: Vec<&str> = if args.is_empty() {
        vec![
            "1 + 2 * 3",
            "(1 + 2) * 3",
            "2 ^ 3 ^ 2",
            "-2 ^ 2",
            "10 - 4 - 3",
            "7 % 4 * 2",
            "1.5e3 / (2 - -2)",
            "2 * (3 + )",
        ]
    } else {
        args.iter().map(String::as_str).collect()
    };
    for input in inputs {
        match parser.parse_with(input, &mut Evaluate) {
            Ok(value) => println!("{input:>20}  =  {value}"),
            Err(err) => println!("{input:>20}  !  {err} (at {})", err.span()),
        }
    }
}
