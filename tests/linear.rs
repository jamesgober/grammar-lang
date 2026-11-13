//! Lexing stays linear when a token's scan runs far past the token it
//! returns.
//!
//! A block comment that never closes makes every `/` scan to the end of the
//! input before falling back to the one-byte `/` token. Without memoising
//! the failed scans, each `/` rereads the rest of the text and lexing goes
//! quadratic. Each test runs on its own thread and fails at a deadline
//! rather than waiting for a quadratic run to finish.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use grammar_lang::{Grammar, Parser};

/// Generous for a linear pass in a debug build on a busy machine; a
/// quadratic pass over the same input takes many minutes.
const DEADLINE: Duration = Duration::from_secs(30);

fn comments() -> Parser {
    Grammar::new()
        .literal("/")
        .literal("*")
        .pattern("num", "[0-9]+")
        .skip("space", " +")
        .skip("comment", r"/\*([^*]|\*+[^*/])*\*+/")
        .rule("items", &["items", "item"])
        .rule("items", &["item"])
        .rule("item", &["/"])
        .rule("item", &["*"])
        .rule("item", &["num"])
        .build()
        .unwrap()
}

fn runs() -> Parser {
    Grammar::new()
        .literal("a")
        .pattern("ab", "a+b")
        .rule("items", &["items", "item"])
        .rule("items", &["item"])
        .rule("item", &["a"])
        .rule("item", &["ab"])
        .build()
        .unwrap()
}

/// Runs `work` on a thread and fails if it does not finish by the deadline.
fn within_deadline<T: Send + 'static>(what: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(work());
    });
    match rx.recv_timeout(DEADLINE) {
        Ok(value) => value,
        Err(_) => panic!("{what} did not finish within {DEADLINE:?}; lexing is not linear"),
    }
}

#[test]
fn unclosed_comment_openers_lex_in_linear_time() {
    let text = "/* ".repeat(350_000);
    let count = within_deadline("tokens()", move || {
        comments().tokens(&text).filter(Result::is_ok).count()
    });
    // `/` and `*` per repeat; the spaces are skipped tokens and also yielded.
    assert_eq!(count, 3 * 350_000);
}

#[test]
fn unclosed_comment_openers_parse_in_linear_time() {
    let text = "/* ".repeat(350_000);
    let leaves = within_deadline("parse()", move || {
        let parser = comments();
        let tree = parser.parse(&text).unwrap();
        tree.root()
            .descendants()
            .filter(|n| n.kind().is_token())
            .count()
    });
    assert_eq!(leaves, 2 * 350_000);
}

#[test]
fn long_dead_end_prefix_lexes_in_linear_time() {
    // Every `a` scans the rest of the run looking for the `b` of `a+b`,
    // finds none, and falls back to the one-byte `a` token.
    let text = "a".repeat(1_000_000);
    let count = within_deadline("tokens()", move || {
        runs().tokens(&text).filter(Result::is_ok).count()
    });
    assert_eq!(count, 1_000_000);
}

#[test]
fn memoised_scans_still_find_late_matches() {
    // The memo must only cut scans that cannot reach a match: a comment that
    // does close is still one token, after any number of failed openers.
    let parser = comments();
    let mut text = "/* ".repeat(1_000);
    text.push_str("*/ 7");
    let kinds: Vec<&str> = parser
        .tokens(&text)
        .map(|t| parser.name(*t.unwrap().kind()))
        .collect();
    assert_eq!(kinds, ["comment", "space", "num"]);

    let mut text = String::from("1 ");
    for _ in 0..500 {
        text.push_str("/* 2 ");
    }
    text.push_str("*/ 3 / 4");
    let kinds: Vec<&str> = parser
        .tokens(&text)
        .filter_map(|t| {
            let kind = *t.unwrap().kind();
            (parser.name(kind) != "space").then(|| parser.name(kind))
        })
        .collect();
    assert_eq!(kinds, ["num", "comment", "num", "/", "num"]);
}
