//! # grammar-lang
//!
//! A parser generator that runs at runtime. Describe a language's tokens and
//! rules with [`Grammar`], call [`build`](Grammar::build), and get a
//! [`Parser`]: a minimized DFA lexer and an LALR(1) parse table, ready to
//! parse text into a [`Tree`] or to drive your own [`Actions`].
//!
//! There is no code generation step and no build script. The grammar is data,
//! so it can come from a configuration file, a schematic, or a test, and a
//! malformed one is reported as a [`GrammarError`] value rather than a
//! compile failure — which is what the family's language generators build on.
//!
//! ## Quick start
//!
//! ```
//! use grammar_lang::Grammar;
//!
//! let parser = Grammar::new()
//!     .literal("+")
//!     .literal("-")
//!     .literal("*")
//!     .literal("/")
//!     .literal("(")
//!     .literal(")")
//!     .pattern("num", r"[0-9]+(\.[0-9]+)?")
//!     .skip("space", r"\s+")
//!     .left(&["+", "-"])
//!     .left(&["*", "/"])
//!     .rule("expr", &["expr", "+", "expr"])
//!     .rule("expr", &["expr", "-", "expr"])
//!     .rule("expr", &["expr", "*", "expr"])
//!     .rule("expr", &["expr", "/", "expr"])
//!     .rule("expr", &["(", "expr", ")"])
//!     .rule("expr", &["num"])
//!     .build()?;
//!
//! let tree = parser.parse("2 * (3 + 4)").unwrap();
//! assert_eq!(tree.root().name(), "expr");
//! assert_eq!(tree.root().text(), "2 * (3 + 4)");
//!
//! let err = parser.parse("2 * (3 + )").unwrap_err();
//! assert_eq!(err.to_string(), "expected `(` or `num`, found `)`");
//! # Ok::<(), grammar_lang::GrammarError>(())
//! ```
//!
//! ## How it works
//!
//! [`Grammar::build`] runs two generators.
//!
//! - **Lexer.** Every token's pattern is parsed and compiled, by Thompson's
//!   construction, into one NFA over UTF-8 bytes. Subset construction over
//!   byte equivalence classes makes it deterministic, and Hopcroft's algorithm
//!   minimizes it. The result is a flat transition table that finds the
//!   longest match at a position with one table load per input byte. Tokens
//!   that match the empty string, or that can never win a match, are
//!   rejected.
//! - **Parser.** The rules become an LR(0) automaton, and DeRemer and
//!   Pennello's relations compute its LALR(1) lookahead sets without building
//!   LR(1) item sets. Shift/reduce conflicts between operators are resolved by
//!   the declared precedence and associativity, as in yacc; any other
//!   conflict is an error that names the competing productions, never a
//!   silent default.
//!
//! [`Parser::parse_with`] then runs the classic shift-reduce loop: one ACTION
//! lookup per step, tokens fetched from the DFA on demand, skipped tokens
//! passed over, and [`Actions`] called in postorder. Parsing is linear in the
//! input and needs stack only for nesting, never for the length of a list.
//!
//! ## Family
//!
//! Tokens are [`token_lang::Token<Kind>`](Token), and [`Kind`] implements
//! [`TokenKind`], so lexer output plugs into anything else in the `-lang`
//! family that consumes tokens. A [`ParseError`] converts into a
//! [`diag_lang::Diagnostic`] with [`ParseError::to_diagnostic`].
//!
//! ## Features
//!
//! - `std` (default): forwards to the dependencies' `std` features. Without
//!   it the crate is `no_std` and needs only `alloc`.
//!
//! ## Stability
//!
//! Version 1.0 freezes the public API: every item above, and the behaviour
//! documented for it — how the lexer chooses between matches, how conflicts
//! are settled, how productions are numbered, what a tree looks like, and
//! what an error reports. None of it changes in a breaking way before 2.0.
//! `token-lang` 1 and `diag-lang` 1 are public dependencies. The full promise,
//! and what it leaves out, is in
//! [`docs/API.md`](https://github.com/jamesgober/grammar-lang/blob/main/docs/API.md#stability).

#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]
#![deny(
    warnings,
    missing_docs,
    unsafe_op_in_unsafe_fn,
    unused_must_use,
    unused_results,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unreachable,
    clippy::dbg_macro,
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::undocumented_unsafe_blocks
)]

extern crate alloc;

mod error;
mod grammar;
mod kind;
mod lex;
mod lr;
mod parser;
mod tree;
mod util;

pub use error::{GrammarError, ParseError};
pub use grammar::Grammar;
pub use kind::Kind;
pub use parser::{Actions, Parser, Production, Tokens};
pub use tree::{Node, Tree};

#[doc(no_inline)]
pub use token_lang::{Span, Token, TokenKind};

/// Compiles and runs the `rust` code blocks in `README.md` and `docs/API.md` as
/// part of `cargo test`, so the published examples cannot drift from the API.
///
/// Present only while collecting doctests (`#[cfg(doctest)]`); it is not part of
/// the public surface and does not appear in the built library or its docs.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
#[doc = include_str!("../docs/API.md")]
pub struct MarkdownDocTests;
