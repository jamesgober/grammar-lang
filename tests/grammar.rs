//! Building grammars: every way `build` can refuse one, and what it accepts.

use grammar_lang::{Grammar, GrammarError, TokenKind};

fn err(grammar: Grammar) -> GrammarError {
    grammar.build().unwrap_err()
}

fn name(s: &str) -> Box<str> {
    s.into()
}

#[test]
fn no_rules() {
    let e = err(Grammar::new().literal("x"));
    assert_eq!(e, GrammarError::NoRules);
    assert_eq!(e.to_string(), "the grammar declares no rules");
}

#[test]
fn duplicate_names() {
    let e = err(Grammar::new().literal("x").literal("x").rule("s", &["x"]));
    assert_eq!(e, GrammarError::Duplicate { name: name("x") });
    assert_eq!(e.to_string(), "`x` is declared more than once");

    let e = err(Grammar::new()
        .pattern("id", "[a-z]+")
        .skip("id", " ")
        .rule("s", &["id"]));
    assert_eq!(e, GrammarError::Duplicate { name: name("id") });

    // A rule may not share a token's name...
    let e = err(Grammar::new().pattern("x", "x").rule("x", &["x"]));
    assert_eq!(e, GrammarError::Duplicate { name: name("x") });

    // ...but repeating a rule adds an alternative.
    assert!(
        Grammar::new()
            .literal("a")
            .literal("b")
            .rule("s", &["a"])
            .rule("s", &["b"])
            .build()
            .is_ok()
    );
}

#[test]
fn undefined_and_skipped_symbols() {
    let e = err(Grammar::new().literal("a").rule("s", &["a", "b"]));
    assert_eq!(
        e,
        GrammarError::Undefined {
            name: name("b"),
            rule: name("s")
        }
    );
    assert_eq!(
        e.to_string(),
        "rule `s` uses `b`, which is not a token or rule"
    );

    let e = err(Grammar::new()
        .literal("a")
        .skip("ws", " +")
        .rule("s", &["a", "ws"]));
    assert_eq!(
        e,
        GrammarError::SkipInRule {
            name: name("ws"),
            rule: name("s")
        }
    );
    assert_eq!(
        e.to_string(),
        "rule `s` uses `ws`, which is a skipped token"
    );

    // Undefined symbols are reported even in rules the start rule never uses.
    let e = err(Grammar::new()
        .literal("a")
        .rule("s", &["a"])
        .rule("unused", &["nope"]));
    assert_eq!(
        e,
        GrammarError::Undefined {
            name: name("nope"),
            rule: name("unused")
        }
    );
}

#[test]
fn precedence_declarations() {
    let e = err(Grammar::new().literal("a").prec("a").rule("s", &["a"]));
    assert_eq!(e, GrammarError::MisplacedPrec { name: name("a") });
    assert_eq!(
        e.to_string(),
        "precedence `a` is applied before any rule is declared"
    );

    let e = err(Grammar::new().literal("a").rule("s", &["a"]).prec("HIGH"));
    assert_eq!(e, GrammarError::UndefinedPrecedence { name: name("HIGH") });
    assert_eq!(
        e.to_string(),
        "precedence `HIGH` is not declared at any level"
    );

    let e = err(Grammar::new()
        .literal("a")
        .left(&["a"])
        .right(&["a"])
        .rule("s", &["a"]));
    assert_eq!(e, GrammarError::DuplicatePrecedence { name: name("a") });
    assert_eq!(e.to_string(), "`a` is given a precedence more than once");

    let e = err(Grammar::new().literal("a").left(&["s"]).rule("s", &["a"]));
    assert_eq!(e, GrammarError::PrecedenceOnRule { name: name("s") });
    assert_eq!(e.to_string(), "`s` is a rule and cannot have a precedence");

    // Marker names that are neither tokens nor rules are fine.
    assert!(
        Grammar::new()
            .literal("a")
            .left(&["MARK"])
            .rule("s", &["a"])
            .prec("MARK")
            .build()
            .is_ok()
    );
}

#[test]
fn invalid_patterns() {
    let e = err(Grammar::new().pattern("num", "[0-9").rule("s", &["num"]));
    assert_eq!(
        e,
        GrammarError::InvalidPattern {
            name: name("num"),
            offset: 0,
            reason: "unclosed class"
        }
    );
    assert_eq!(
        e.to_string(),
        "invalid pattern for `num` at byte 0: unclosed class"
    );

    let e = err(Grammar::new().pattern("x", "ab)").rule("s", &["x"]));
    assert!(
        matches!(e, GrammarError::InvalidPattern { offset: 2, .. }),
        "{e}"
    );

    let e = err(Grammar::new().pattern("x", "a{1000}{2}").rule("s", &["x"]));
    assert!(matches!(e, GrammarError::InvalidPattern { .. }), "{e}");

    let e = err(Grammar::new()
        .pattern("x", "((a{1000}){1000}){2}")
        .rule("s", &["x"]));
    assert_eq!(
        e,
        GrammarError::InvalidPattern {
            name: name("x"),
            offset: 0,
            reason: "the pattern is too large to compile"
        }
    );
}

#[test]
fn empty_tokens() {
    let e = err(Grammar::new().pattern("ws", " *").rule("s", &["ws"]));
    assert_eq!(e, GrammarError::EmptyToken { name: name("ws") });
    assert_eq!(e.to_string(), "token `ws` matches the empty string");

    let e = err(Grammar::new().literal("").rule("s", &[""]));
    assert_eq!(e, GrammarError::EmptyToken { name: name("") });

    // Skipped tokens too: an empty match would never advance.
    let e = err(Grammar::new()
        .literal("a")
        .skip("ws", "\\s*")
        .rule("s", &["a"]));
    assert_eq!(e, GrammarError::EmptyToken { name: name("ws") });
}

#[test]
fn shadowed_tokens() {
    // Same length, same priority class: the first pattern always wins.
    let e = err(Grammar::new()
        .pattern("word", "[a-z]+")
        .pattern("abc", "abc")
        .rule("s", &["word"]));
    assert_eq!(
        e,
        GrammarError::ShadowedToken {
            name: name("abc"),
            by: name("word")
        }
    );
    assert_eq!(
        e.to_string(),
        "token `abc` is never produced; `word` takes priority over it"
    );

    // A literal beats a pattern regardless of order, so this is fine.
    assert!(
        Grammar::new()
            .pattern("word", "[a-z]+")
            .literal("abc")
            .rule("s", &["word"])
            .build()
            .is_ok()
    );
}

#[test]
fn unproductive_rules() {
    let e = err(Grammar::new().literal("a").rule("s", &["a", "s"]));
    assert_eq!(e, GrammarError::Unproductive { name: name("s") });
    assert_eq!(e.to_string(), "rule `s` cannot derive any finite input");

    // Only rules the start rule reaches matter.
    let ok = Grammar::new()
        .literal("a")
        .rule("s", &["a"])
        .rule("loop", &["loop"])
        .build();
    assert!(ok.is_ok());
}

#[test]
fn conflicts() {
    let e = err(Grammar::new()
        .literal("+")
        .pattern("n", "[0-9]")
        .rule("e", &["e", "+", "e"])
        .rule("e", &["n"]));
    assert_eq!(
        e,
        GrammarError::ShiftReduce {
            token: name("+"),
            shift: name("e → e • + e"),
            reduce: name("e → e + e"),
        }
    );

    let e = err(Grammar::new()
        .literal("x")
        .rule("s", &["a"])
        .rule("s", &["b"])
        .rule("a", &["x"])
        .rule("b", &["x"]));
    assert_eq!(
        e,
        GrammarError::ReduceReduce {
            token: None,
            first: name("a → x"),
            second: name("b → x")
        }
    );
    assert_eq!(
        e.to_string(),
        "reduce/reduce conflict on end of input: reduce `a → x`, or reduce `b → x`"
    );

    let e = err(Grammar::new()
        .literal("x")
        .literal(";")
        .rule("s", &["a", ";"])
        .rule("s", &["b", ";"])
        .rule("a", &[])
        .rule("b", &[]));
    assert_eq!(
        e,
        GrammarError::ReduceReduce {
            token: Some(name(";")),
            first: name("a → ε"),
            second: name("b → ε"),
        }
    );
}

#[test]
fn the_dangling_else_needs_precedence() {
    let base = || {
        Grammar::new()
            .literal("if")
            .literal("then")
            .literal("else")
            .literal("x")
            .skip("ws", " +")
    };
    let rules = |g: Grammar| {
        g.rule("stmt", &["if", "x", "then", "stmt"])
            .rule("stmt", &["if", "x", "then", "stmt", "else", "stmt"])
            .rule("stmt", &["x"])
    };
    let e = err(rules(base()));
    assert!(
        matches!(e, GrammarError::ShiftReduce { ref token, .. } if &**token == "else"),
        "{e}"
    );

    // `else` binds tighter than `then`, so it attaches to the nearest `if`.
    let parser = rules(base().nonassoc(&["then"]).nonassoc(&["else"]))
        .build()
        .unwrap();
    let tree = parser.parse("if x then if x then x else x").unwrap();
    assert_eq!(
        tree.to_string(),
        r#"(stmt "if" "x" "then" (stmt "if" "x" "then" (stmt "x") "else" (stmt "x")))"#
    );
}

#[test]
fn bison_rule_precedence_is_the_last_token() {
    // `e → e + x e`: the last token is `x`, which has no precedence, so the
    // production has none and `+` alone cannot settle the conflict.
    let e = err(Grammar::new()
        .literal("+")
        .literal("x")
        .literal("n")
        .left(&["+"])
        .rule("e", &["e", "+", "x", "e"])
        .rule("e", &["n"]));
    assert!(matches!(e, GrammarError::ShiftReduce { .. }), "{e}");
}

#[test]
fn precedence_settles_before_reduce_reduce_is_judged() {
    // Two identical productions both lose `a` to a shift by precedence, so
    // neither claims it and there is no reduce/reduce conflict on `a`.
    let parser = Grammar::new()
        .literal("a")
        .literal("b")
        .right(&["a", "b"])
        .rule("s", &["a", "c", "a", "a"])
        .rule("s", &["a"])
        .rule("s", &["a", "b", "a", "a"])
        .rule("c", &["b"])
        .rule("c", &["b"])
        .build()
        .unwrap();
    assert!(parser.parse("abaa").is_ok());
}

#[test]
fn conflicts_in_unreachable_states_are_not_reported() {
    // After the first `a`, `%nonassoc a` turns the shift of a second `a`
    // into an error, so the states after `a a` — where `t → b` and `u → b`
    // collide — can never be entered.
    let grammar = |nonassoc: bool| {
        let g = Grammar::new().literal("a").literal("b");
        let g = if nonassoc { g.nonassoc(&["a"]) } else { g };
        g.rule("s", &["a", "a", "t"])
            .rule("s", &["a"])
            .rule("t", &["s", "a"])
            .rule("t", &["b"])
            .rule("t", &["u"])
            .rule("u", &["b"])
    };
    let e = err(grammar(false));
    assert!(
        matches!(
            e,
            GrammarError::ShiftReduce { .. } | GrammarError::ReduceReduce { .. }
        ),
        "{e}"
    );

    let parser = grammar(true).build().unwrap();
    assert!(parser.parse("a").is_ok());
    assert!(parser.parse("aa").is_err());
}

#[test]
fn derivation_cycles_are_refused() {
    let e = err(Grammar::new()
        .literal("x")
        .rule("s", &["s"])
        .rule("s", &["x"]));
    assert_eq!(e, GrammarError::Cycle { name: name("s") });
    assert_eq!(
        e.to_string(),
        "rule `s` can derive itself, so some input has infinitely many parse trees"
    );

    // Through other rules, and through parts that can match nothing.
    let e = err(Grammar::new()
        .literal("x")
        .rule("s", &["a"])
        .rule("a", &["opt", "b", "opt"])
        .rule("b", &["a"])
        .rule("b", &["x"])
        .rule("opt", &[])
        .rule("opt", &["x"]));
    assert_eq!(e, GrammarError::Cycle { name: name("a") });

    // A precedence that would hide the cycle's conflict changes nothing:
    // the parser would otherwise reduce around the cycle forever.
    let e = err(Grammar::new()
        .literal("x")
        .literal("y")
        .left(&["x"])
        .left(&["HIGH"])
        .rule("s", &["a", "x"])
        .rule("a", &["a"])
        .prec("HIGH")
        .rule("a", &["y"]));
    assert_eq!(e, GrammarError::Cycle { name: name("a") });

    // Recursion that consumes input is not a cycle.
    assert!(
        Grammar::new()
            .literal("x")
            .rule("s", &["s", "x"])
            .rule("s", &["x"])
            .build()
            .is_ok()
    );
}

#[test]
fn build_leaves_the_grammar_reusable() {
    let grammar = Grammar::new().literal("a").rule("s", &["a"]);
    let first = grammar.build().unwrap();
    let second = grammar
        .clone()
        .literal("b")
        .rule("s", &["b"])
        .build()
        .unwrap();
    assert!(first.parse("a").is_ok());
    assert!(first.parse("b").is_err());
    assert!(second.parse("b").is_ok());
}

#[test]
fn names_resolve_to_kinds() {
    let parser = Grammar::new()
        .literal("+")
        .pattern("n", "[0-9]+")
        .skip("ws", " +")
        .rule("sum", &["n", "+", "n"])
        .rule("unused", &["n"])
        .build()
        .unwrap();
    for name in ["+", "n", "ws", "sum", "unused"] {
        let kind = parser.kind(name).unwrap();
        assert_eq!(parser.name(kind), name);
    }
    assert!(parser.kind("ws").unwrap().is_trivia());
    assert!(!parser.kind("n").unwrap().is_trivia());
    assert!(parser.kind("unused").unwrap().is_rule());
    assert_eq!(parser.kind("missing"), None);
    let debug = format!("{parser:?}");
    assert!(debug.starts_with("Parser { tokens: 3, rules: 2"), "{debug}");
}

#[test]
fn errors_are_std_errors() {
    fn assert_error<E: std::error::Error + Send + Sync + 'static>() {}
    assert_error::<GrammarError>();
    assert_error::<grammar_lang::ParseError>();
    fn assert_parser<T: Send + Sync + Clone>() {}
    assert_parser::<grammar_lang::Parser>();
}
