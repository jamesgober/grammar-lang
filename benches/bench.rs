//! Criterion benchmarks: generating parsers, lexing, and parsing.
//!
//! ```text
//! cargo bench
//! ```

use std::hint::black_box;
use std::vec::Drain;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use grammar_lang::{Actions, Grammar, Kind, Parser, Production, Span, Token};

fn json_grammar() -> Grammar {
    Grammar::new()
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
        .rule("value", &["object"])
        .rule("value", &["array"])
        .rule("value", &["string"])
        .rule("value", &["number"])
        .rule("value", &["true"])
        .rule("value", &["false"])
        .rule("value", &["null"])
        .rule("object", &["{", "}"])
        .rule("object", &["{", "members", "}"])
        .rule("members", &["members", ",", "member"])
        .rule("members", &["member"])
        .rule("member", &["string", ":", "value"])
        .rule("array", &["[", "]"])
        .rule("array", &["[", "elements", "]"])
        .rule("elements", &["elements", ",", "value"])
        .rule("elements", &["value"])
}

fn expression_grammar() -> Grammar {
    Grammar::new()
        .literal("+")
        .literal("-")
        .literal("*")
        .literal("/")
        .literal("(")
        .literal(")")
        .pattern("num", "[0-9]+")
        .skip("space", " +")
        .left(&["+", "-"])
        .left(&["*", "/"])
        .right(&["NEG"])
        .rule("e", &["e", "+", "e"])
        .rule("e", &["e", "-", "e"])
        .rule("e", &["e", "*", "e"])
        .rule("e", &["e", "/", "e"])
        .rule("e", &["-", "e"])
        .prec("NEG")
        .rule("e", &["(", "e", ")"])
        .rule("e", &["num"])
}

/// A C-like language: 40 keywords and operators, statements, and a stratified
/// expression grammar of ten precedence levels — a realistic generator load.
fn c_like_grammar() -> Grammar {
    let mut g = Grammar::new();
    for kw in [
        "if", "else", "while", "for", "return", "break", "continue", "int", "float", "void",
        "struct",
    ] {
        g = g.literal(kw);
    }
    for op in [
        "(", ")", "{", "}", "[", "]", ";", ",", ".", "=", "+=", "-=", "||", "&&", "|", "^", "&",
        "==", "!=", "<", ">", "<=", ">=", "<<", ">>", "+", "-", "*", "/", "%", "!", "~", "++",
        "--",
    ] {
        g = g.literal(op);
    }
    g = g
        .pattern("ident", "[a-zA-Z_][a-zA-Z0-9_]*")
        .pattern("number", r"[0-9]+(\.[0-9]+)?")
        .pattern("string", r#""([^"\\\n]|\\.)*""#)
        .skip("space", r"\s+")
        .skip("line_comment", "//[^\n]*")
        .skip("block_comment", r"/\*([^*]|\*+[^*/])*\*+/")
        .rule("unit", &["unit", "decl"])
        .rule("unit", &["decl"])
        .rule("decl", &["type", "ident", "(", "params", ")", "block"])
        .rule("decl", &["type", "ident", "(", ")", "block"])
        .rule("decl", &["type", "ident", ";"])
        .rule("decl", &["struct", "ident", "{", "fields", "}", ";"])
        .rule("fields", &["fields", "type", "ident", ";"])
        .rule("fields", &["type", "ident", ";"])
        .rule("type", &["int"])
        .rule("type", &["float"])
        .rule("type", &["void"])
        .rule("type", &["struct", "ident"])
        .rule("params", &["params", ",", "type", "ident"])
        .rule("params", &["type", "ident"])
        .rule("block", &["{", "stmts", "}"])
        .rule("block", &["{", "}"])
        .rule("stmts", &["stmts", "stmt"])
        .rule("stmts", &["stmt"])
        .rule("stmt", &["block"])
        .rule("stmt", &["type", "ident", "=", "expr", ";"])
        .rule("stmt", &["expr", ";"])
        .rule("stmt", &["if", "(", "expr", ")", "stmt"])
        .rule("stmt", &["if", "(", "expr", ")", "stmt", "else", "stmt"])
        .rule("stmt", &["while", "(", "expr", ")", "stmt"])
        .rule(
            "stmt",
            &["for", "(", "expr", ";", "expr", ";", "expr", ")", "stmt"],
        )
        .rule("stmt", &["return", "expr", ";"])
        .rule("stmt", &["return", ";"])
        .rule("stmt", &["break", ";"])
        .rule("stmt", &["continue", ";"])
        .nonassoc(&[")"])
        .nonassoc(&["else"]);
    let levels: [(&str, &str, &[&str]); 9] = [
        ("expr", "assign", &["=", "+=", "-="]),
        ("assign", "or", &["||"]),
        ("or", "and", &["&&"]),
        ("and", "bitor", &["|", "^", "&"]),
        ("bitor", "eq", &["==", "!="]),
        ("eq", "rel", &["<", ">", "<=", ">="]),
        ("rel", "shift", &["<<", ">>"]),
        ("shift", "add", &["+", "-"]),
        ("add", "mul", &["*", "/", "%"]),
    ];
    for (upper, lower, ops) in levels {
        g = g.rule(upper, &[lower]);
        for op in ops {
            g = g.rule(upper, &[upper, op, lower]);
        }
    }
    g.rule("mul", &["unary"])
        .rule("unary", &["-", "unary"])
        .rule("unary", &["!", "unary"])
        .rule("unary", &["~", "unary"])
        .rule("unary", &["++", "unary"])
        .rule("unary", &["--", "unary"])
        .rule("unary", &["postfix"])
        .rule("postfix", &["postfix", "(", "args", ")"])
        .rule("postfix", &["postfix", "(", ")"])
        .rule("postfix", &["postfix", "[", "expr", "]"])
        .rule("postfix", &["postfix", ".", "ident"])
        .rule("postfix", &["postfix", "++"])
        .rule("postfix", &["primary"])
        .rule("args", &["args", ",", "expr"])
        .rule("args", &["expr"])
        .rule("primary", &["ident"])
        .rule("primary", &["number"])
        .rule("primary", &["string"])
        .rule("primary", &["(", "expr", ")"])
}

/// About `bytes` of JSON: an array of small records.
fn json_document(bytes: usize) -> String {
    let mut out = String::from("[\n");
    let mut i = 0usize;
    while out.len() < bytes {
        if i > 0 {
            out.push_str(",\n");
        }
        out.push_str(&format!(
            r#"  {{"id": {i}, "name": "item-{i}", "price": {}.{:02}, "tags": ["a", "bé"], "ok": {}, "next": null}}"#,
            i * 7 % 1000,
            i % 100,
            i % 2 == 0,
        ));
        i += 1;
    }
    out.push_str("\n]\n");
    out
}

struct Noop;

impl<'s> Actions<'s> for Noop {
    type Value = ();
    #[inline]
    fn token(&mut self, _: Token<Kind>, _: &'s str) {}
    #[inline]
    fn reduce(&mut self, _: Production, _: Span, _: Drain<'_, ()>) {}
}

struct Eval;

impl<'s> Actions<'s> for Eval {
    type Value = i64;
    #[inline]
    fn token(&mut self, _: Token<Kind>, text: &'s str) -> i64 {
        text.parse().unwrap_or(0)
    }
    #[inline]
    fn reduce(&mut self, p: Production, _: Span, mut kids: Drain<'_, i64>) -> i64 {
        let a = kids.next().unwrap_or(0);
        match p.index() {
            0 => a + kids.nth(1).unwrap_or(0),
            1 => a - kids.nth(1).unwrap_or(0),
            2 => a.wrapping_mul(kids.nth(1).unwrap_or(0)),
            3 => a / kids.nth(1).unwrap_or(1).max(1),
            4 => -kids.next().unwrap_or(0),
            5 => kids.next().unwrap_or(0),
            _ => a,
        }
    }
}

fn build(c: &mut Criterion) {
    let mut group = c.benchmark_group("build");
    for (name, grammar) in [
        ("expression", expression_grammar()),
        ("json", json_grammar()),
        ("c_like", c_like_grammar()),
    ] {
        assert!(grammar.build().is_ok(), "{name}");
        group.bench_function(name, |b| b.iter(|| black_box(&grammar).build()));
    }
    group.finish();
}

fn lex_and_parse(c: &mut Criterion) {
    let parser: Parser = json_grammar().build().unwrap_or_else(|e| panic!("{e}"));
    let document = json_document(1 << 20);
    assert!(parser.parse(&document).is_ok());

    let mut group = c.benchmark_group("json_1mib");
    group.throughput(Throughput::Bytes(document.len() as u64));
    group.bench_function("tokens", |b| {
        b.iter(|| parser.tokens(black_box(&document)).count())
    });
    group.bench_function("parse_with_noop", |b| {
        b.iter(|| parser.parse_with(black_box(&document), &mut Noop))
    });
    group.bench_function("parse_tree", |b| {
        b.iter_batched(
            || (),
            |()| parser.parse(black_box(&document)).map(|t| t.root().span()),
            BatchSize::SmallInput,
        )
    });
    group.finish();

    let calc = expression_grammar()
        .build()
        .unwrap_or_else(|e| panic!("{e}"));
    let terms: Vec<String> = (0..20_000)
        .map(|i| format!("({} * {} - -{})", i % 97, i % 13 + 1, i % 7))
        .collect();
    let expression = terms.join(" + ");
    let mut group = c.benchmark_group("expression");
    group.throughput(Throughput::Bytes(expression.len() as u64));
    group.bench_function("evaluate", |b| {
        b.iter(|| calc.parse_with(black_box(&expression), &mut Eval))
    });
    group.finish();
}

criterion_group!(benches, build, lex_and_parse);
criterion_main!(benches);
