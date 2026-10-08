//! Parsing: trees, actions, errors, tokens, and inputs at the extremes.

use std::vec::Drain;

use grammar_lang::{Actions, Grammar, Kind, Parser, Production, Span, Token};

fn calculator() -> Parser {
    Grammar::new()
        .literal("+")
        .literal("-")
        .literal("*")
        .literal("/")
        .literal("^")
        .literal("(")
        .literal(")")
        .pattern("num", "[0-9]+")
        .skip("space", r"\s+")
        .left(&["+", "-"])
        .left(&["*", "/"])
        .right(&["NEG"])
        .right(&["^"])
        .rule("e", &["e", "+", "e"]) // 0
        .rule("e", &["e", "-", "e"]) // 1
        .rule("e", &["e", "*", "e"]) // 2
        .rule("e", &["e", "/", "e"]) // 3
        .rule("e", &["e", "^", "e"]) // 4
        .rule("e", &["-", "e"]) // 5
        .prec("NEG")
        .rule("e", &["(", "e", ")"]) // 6
        .rule("e", &["num"]) // 7
        .build()
        .unwrap()
}

/// Evaluates as it parses.
struct Eval;

impl<'s> Actions<'s> for Eval {
    type Value = i64;

    fn token(&mut self, _: Token<Kind>, text: &'s str) -> i64 {
        text.parse().unwrap_or(0)
    }

    fn reduce(&mut self, p: Production, _: Span, children: Drain<'_, i64>) -> i64 {
        let v: Vec<i64> = children.collect();
        match p.index() {
            0 => v[0] + v[2],
            1 => v[0] - v[2],
            2 => v[0] * v[2],
            3 => v[0] / v[2],
            4 => v[0].pow(v[2] as u32),
            5 => -v[1],
            6 => v[1],
            _ => v[0],
        }
    }
}

#[test]
fn precedence_and_associativity_evaluate_correctly() {
    let parser = calculator();
    let cases = [
        ("1 + 2 * 3", 7),
        ("(1 + 2) * 3", 9),
        ("10 - 4 - 3", 3),
        ("2 ^ 3 ^ 2", 512),
        ("-2 ^ 2", -4),
        ("-2 * 3", -6),
        ("--5", 5),
        ("100 / 10 / 5", 2),
        ("2 * -3 + 1", -5),
    ];
    for (input, expected) in cases {
        assert_eq!(parser.parse_with(input, &mut Eval), Ok(expected), "{input}");
    }
}

#[test]
fn actions_run_in_postorder() {
    #[derive(Default)]
    struct Log(Vec<String>);
    impl<'s> Actions<'s> for Log {
        type Value = ();
        fn token(&mut self, _: Token<Kind>, text: &'s str) {
            self.0.push(text.to_string());
        }
        fn reduce(&mut self, p: Production, span: Span, children: Drain<'_, ()>) {
            self.0
                .push(format!("r{}:{}@{}", p.index(), children.len(), span));
        }
    }
    let parser = Grammar::new()
        .literal("a")
        .literal("b")
        .rule("s", &["x", "b"])
        .rule("x", &["a"])
        .rule("x", &[])
        .build()
        .unwrap();
    let mut log = Log::default();
    parser.parse_with("ab", &mut log).unwrap();
    assert_eq!(log.0, ["a", "r1:1@0..1", "b", "r0:2@0..2"]);

    let mut log = Log::default();
    parser.parse_with("b", &mut log).unwrap();
    assert_eq!(log.0, ["r2:0@0..0", "b", "r0:2@0..1"]);
}

#[test]
fn production_reports_its_rule() {
    struct Rules(Vec<Kind>);
    impl<'s> Actions<'s> for Rules {
        type Value = ();
        fn token(&mut self, _: Token<Kind>, _: &'s str) {}
        fn reduce(&mut self, p: Production, _: Span, _: Drain<'_, ()>) {
            self.0.push(p.rule());
        }
    }
    let parser = Grammar::new()
        .literal("a")
        .rule("s", &["t"])
        .rule("t", &["a"])
        .build()
        .unwrap();
    let mut rules = Rules(vec![]);
    parser.parse_with("a", &mut rules).unwrap();
    assert_eq!(
        rules.0,
        [parser.kind("t").unwrap(), parser.kind("s").unwrap()]
    );
}

#[test]
fn trees_record_structure_and_spans() {
    let parser = calculator();
    let tree = parser.parse(" 1 + 2*3 ").unwrap();
    assert_eq!(
        tree.to_string(),
        r#"(e (e (num "1")) "+" (e (e (num "2")) "*" (e (num "3"))))"#
    );
    let root = tree.root();
    assert_eq!(root.span(), Span::new(1, 8));
    assert_eq!(root.text(), "1 + 2*3");
    assert_eq!(tree.source(), " 1 + 2*3 ");
    let rhs = root.child(2).unwrap();
    assert_eq!(rhs.text(), "2*3");
    assert_eq!(rhs.children().len(), 3);
    assert_eq!(format!("{:?}", rhs), "Node(e @ 5..8)");
    assert_eq!(
        format!("{tree:#}"),
        "(e\n  (e\n    (num \"1\"))\n  \"+\"\n  (e\n    (e\n      (num \"2\"))\n    \"*\"\n    (e\n      (num \"3\"))))"
    );
    let leaves: Vec<&str> = root
        .descendants()
        .filter(|n| n.kind().is_token())
        .map(|n| n.text())
        .collect();
    assert_eq!(leaves, ["1", "+", "2", "*", "3"]);
}

#[test]
fn empty_productions_get_empty_spans() {
    let parser = Grammar::new()
        .literal("a")
        .literal(";")
        .skip("ws", " +")
        .rule("s", &["a", "opt", ";"])
        .rule("opt", &[])
        .rule("opt", &["a"])
        .build()
        .unwrap();
    let tree = parser.parse("a  ;").unwrap();
    let opt = tree.root().child(1).unwrap();
    assert_eq!(opt.name(), "opt");
    assert_eq!(opt.span(), Span::empty(1));
    assert_eq!(opt.text(), "");
    assert_eq!(opt.to_string(), "(opt)");
}

#[test]
fn error_messages_list_what_was_expected() {
    let parser = calculator();
    let msg = |input: &str| parser.parse(input).unwrap_err().to_string();
    assert_eq!(
        msg("1 +"),
        "expected one of `-`, `(`, or `num`, found end of input"
    );
    assert_eq!(
        msg("1 2"),
        "expected one of `+`, `-`, `*`, `/`, `^`, or end of input, found `num` `2`"
    );
    assert_eq!(
        msg("(1"),
        "expected one of `+`, `-`, `*`, `/`, `^`, or `)`, found end of input"
    );
    assert_eq!(
        msg("1 @ 2"),
        "unrecognized input `@`; expected one of `+`, `-`, `*`, `/`, `^`, or end of input"
    );
    assert_eq!(msg(")"), "expected one of `-`, `(`, or `num`, found `)`");

    let pair = Grammar::new()
        .literal("a")
        .literal("b")
        .rule("s", &["a", "b"])
        .build()
        .unwrap();
    assert_eq!(
        pair.parse("aa").unwrap_err().to_string(),
        "expected `b`, found `a`"
    );
    let either = Grammar::new()
        .literal("a")
        .rule("s", &["a"])
        .rule("s", &[])
        .build()
        .unwrap();
    assert_eq!(
        either.parse("aa").unwrap_err().to_string(),
        "expected end of input, found `a`"
    );
    let opt = Grammar::new()
        .literal("a")
        .literal("b")
        .rule("s", &["a", "t"])
        .rule("t", &["b"])
        .rule("t", &[])
        .build()
        .unwrap();
    assert_eq!(
        opt.parse("aa").unwrap_err().to_string(),
        "expected `b` or end of input, found `a`"
    );
}

#[test]
fn error_details() {
    let parser = calculator();
    let err = parser.parse("1 + )").unwrap_err();
    assert_eq!(err.span(), Span::new(4, 5));
    assert_eq!(err.found(), parser.kind(")"));
    assert!(!err.at_end());
    let expected: Vec<&str> = err.expected().iter().map(|&k| parser.name(k)).collect();
    assert_eq!(expected, ["-", "(", "num"]);

    let err = parser.parse("1 +").unwrap_err();
    assert!(err.at_end());
    assert_eq!(err.found(), None);
    assert_eq!(err.span(), Span::empty(3));

    let err = parser.parse("1 ? 2").unwrap_err();
    assert_eq!(err.found(), None);
    assert!(!err.at_end());
    assert_eq!(err.span(), Span::new(2, 3));

    // Long token text is shortened in the message.
    let long = format!("1 {}", "9".repeat(100));
    let msg = parser.parse(&long).unwrap_err().to_string();
    assert!(
        msg.ends_with(&format!("found `num` `{}…`", "9".repeat(32))),
        "{msg}"
    );

    // Control characters are escaped.
    let msg = parser.parse("1\u{7}").unwrap_err().to_string();
    assert!(msg.starts_with("unrecognized input `\\u{7}`"), "{msg}");
}

#[test]
fn errors_render_as_diagnostics() {
    use diag_lang::{Renderer, SourceMap};

    let parser = calculator();
    let source = "1 + * 2";
    let err = parser.parse(source).unwrap_err();
    let mut map = SourceMap::new();
    let _ = map.add("calc.txt", source).unwrap();
    let text = Renderer::new().render(&err.to_diagnostic(), &map);
    assert!(
        text.contains("expected one of `-`, `(`, or `num`, found `*`"),
        "{text}"
    );
    assert!(text.contains("unexpected token"), "{text}");
    assert!(text.contains("calc.txt"), "{text}");
}

#[test]
fn tokens_cover_everything_including_trivia_and_errors() {
    let parser = calculator();
    let text = "1 + ?2";
    let items: Vec<(String, &str, bool)> = parser
        .tokens(text)
        .map(|item| match item {
            Ok(t) => {
                let s = t.span();
                (
                    parser.name(*t.kind()).to_string(),
                    &text[s.start().to_usize()..s.end().to_usize()],
                    t.is_trivia(),
                )
            }
            Err(e) => {
                let s = e.span();
                (
                    "error".to_string(),
                    &text[s.start().to_usize()..s.end().to_usize()],
                    false,
                )
            }
        })
        .collect();
    assert_eq!(
        items,
        [
            ("num".to_string(), "1", false),
            ("space".to_string(), " ", true),
            ("+".to_string(), "+", false),
            ("space".to_string(), " ", true),
            ("error".to_string(), "?", false),
            ("num".to_string(), "2", false),
        ]
    );
    let mut tokens = parser.tokens("");
    assert!(tokens.next().is_none());
    assert!(tokens.next().is_none());
    let err = parser.tokens("é").next().unwrap().unwrap_err();
    assert_eq!(err.span(), Span::new(0, 2));
    assert_eq!(err.to_string(), "unrecognized input `é`");
}

#[test]
fn keywords_literals_and_longest_match() {
    let parser = Grammar::new()
        .literal("if")
        .literal("=")
        .literal("==")
        .pattern("ident", "[a-z_][a-z0-9_]*")
        .skip("ws", " +")
        .rule("s", &["s", "item"])
        .rule("s", &["item"])
        .rule("item", &["if"])
        .rule("item", &["ident"])
        .rule("item", &["="])
        .rule("item", &["=="])
        .build()
        .unwrap();
    let kinds: Vec<&str> = parser
        .tokens("if iffy if_ = == === i")
        .filter_map(Result::ok)
        .filter(|t| !t.is_trivia())
        .map(|t| parser.name(*t.kind()))
        .collect();
    assert_eq!(
        kinds,
        ["if", "ident", "ident", "=", "==", "==", "=", "ident"]
    );
}

#[test]
fn unicode_text_and_spans() {
    let parser = Grammar::new()
        .pattern("word", "[a-zA-Zà-öø-ÿα-ωА-я]+")
        .pattern("emoji", "[\u{1F300}-\u{1FAFF}]")
        .skip("ws", " +")
        .rule("s", &["s", "t"])
        .rule("s", &["t"])
        .rule("t", &["word"])
        .rule("t", &["emoji"])
        .build()
        .unwrap();
    let tree = parser.parse("é λ 🎉 Ж").unwrap();
    let leaves: Vec<(&str, Span)> = tree
        .root()
        .descendants()
        .filter(|n| n.kind().is_token())
        .map(|n| (n.text(), n.span()))
        .collect();
    assert_eq!(
        leaves,
        [
            ("é", Span::new(0, 2)),
            ("λ", Span::new(3, 5)),
            ("🎉", Span::new(6, 10)),
            ("Ж", Span::new(11, 13)),
        ]
    );
}

#[test]
fn deep_nesting_is_stack_safe() {
    let parser = calculator();
    let depth = 100_000;
    let input = format!("{}1{}", "(".repeat(depth), ")".repeat(depth));
    let tree = parser.parse(&input).unwrap();
    assert_eq!(tree.root().descendants().count(), 3 * depth + 2);
    let expected = format!(
        "{}(e (num \"1\")){}",
        "(e \"(\" ".repeat(depth),
        " \")\")".repeat(depth)
    );
    assert_eq!(tree.to_string(), expected);
    assert_eq!(parser.parse_with(&input, &mut Eval), Ok(1));
    drop(tree);

    // Right recursion keeps every element on the stack until the end.
    let right = Grammar::new()
        .literal("x")
        .rule("list", &["x", "list"])
        .rule("list", &["x"])
        .build()
        .unwrap();
    let input = "x".repeat(depth);
    let tree = right.parse(&input).unwrap();
    assert_eq!(tree.root().descendants().count(), 2 * depth);
    assert!(tree.to_string().starts_with(r#"(list "x" (list "x""#));
}

#[test]
fn long_lists_parse_in_linear_time() {
    let parser = Grammar::new()
        .literal(",")
        .pattern("n", "[0-9]+")
        .rule("list", &["list", ",", "n"])
        .rule("list", &["n"])
        .build()
        .unwrap();
    let items: Vec<String> = (0..200_000).map(|i| i.to_string()).collect();
    let input = items.join(",");
    let tree = parser.parse(&input).unwrap();
    assert_eq!(tree.root().text().len(), input.len());
}

#[test]
fn the_empty_language_and_empty_input() {
    let parser = Grammar::new().rule("s", &[]).build().unwrap();
    let tree = parser.parse("").unwrap();
    assert_eq!(tree.to_string(), "(s)");
    let err = parser.parse("x").unwrap_err();
    assert_eq!(
        err.to_string(),
        "unrecognized input `x`; expected end of input"
    );
}

#[test]
fn one_parser_serves_many_threads() {
    let parser = calculator();
    std::thread::scope(|scope| {
        for i in 0..8i64 {
            let parser = &parser;
            scope.spawn(move || {
                for j in 0..200i64 {
                    let input = format!("{i} * {j} + 1");
                    assert_eq!(parser.parse_with(&input, &mut Eval), Ok(i * j + 1));
                }
            });
        }
    });
}
