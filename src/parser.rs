//! [`Parser`]: a built grammar, and the table-driven driver that runs it.

use alloc::{boxed::Box, string::String, vec, vec::Vec};
use core::{fmt, iter::FusedIterator};

use token_lang::{Span, Token};

use crate::lex::Dfa;
use crate::lr::table::{ACCEPT, REDUCE, SHIFT};
use crate::tree::{Tree, TreeBuilder};
use crate::{Kind, ParseError};

/// The longest token text quoted in an error message.
const QUOTE_LIMIT: usize = 32;

/// A parser generated from a [`Grammar`](crate::Grammar).
///
/// A `Parser` holds a minimized DFA for the tokens and LALR(1) tables for the
/// rules. Parsing is a single left-to-right pass: the lexer finds each token
/// by longest match as the parser asks for it, skipped tokens are passed
/// over, and every step of the parser is one table lookup. Time is linear in
/// the input, and the stacks grow with nesting depth only, so left-recursive
/// lists of any length parse in constant stack space.
///
/// There are two ways to consume a parse:
///
/// - [`parse`](Parser::parse) builds a concrete syntax [`Tree`]: a node per
///   rule match and a leaf per token, each with its span.
/// - [`parse_with`](Parser::parse_with) runs your own [`Actions`] instead,
///   building whatever the language needs — an AST, a value, nothing — with
///   no intermediate tree.
///
/// A `Parser` is immutable, `Send`, and `Sync`; one instance can serve any
/// number of parses, on any number of threads.
///
/// # Examples
///
/// ```
/// use grammar_lang::Grammar;
///
/// let parser = Grammar::new()
///     .literal("[")
///     .literal("]")
///     .literal(",")
///     .pattern("num", "[0-9]+")
///     .skip("space", " +")
///     .rule("list", &["[", "]"])
///     .rule("list", &["[", "items", "]"])
///     .rule("items", &["items", ",", "num"])
///     .rule("items", &["num"])
///     .build()?;
///
/// let tree = parser.parse("[1, 2, 3]").unwrap();
/// let nums = parser.kind("num").unwrap();
/// let count = tree.root().descendants().filter(|n| n.kind() == nums).count();
/// assert_eq!(count, 3);
/// # Ok::<(), grammar_lang::GrammarError>(())
/// ```
#[derive(Clone)]
pub struct Parser {
    /// Every token and rule name, by kind index.
    pub(crate) names: Box<[Box<str>]>,
    /// Whether each kind is a literal token.
    pub(crate) literal: Box<[bool]>,
    /// Kind indices sorted by name.
    pub(crate) by_name: Box<[u32]>,
    /// The number of parsed (not skipped) tokens; also the end-of-input
    /// terminal.
    pub(crate) parsed: u32,
    /// The number of tokens, skipped ones included.
    pub(crate) lexed: u32,
    pub(crate) dfa: Dfa,
    pub(crate) action: Box<[u32]>,
    pub(crate) goto: Box<[u32]>,
    pub(crate) terms: u32,
    pub(crate) nonterms: u32,
    /// By generator production number.
    pub(crate) prods: Box<[ProdInfo]>,
}

/// What the driver needs to know about a production.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ProdInfo {
    pub(crate) len: u32,
    pub(crate) lhs: u32,
    pub(crate) production: Production,
}

/// A production of the grammar, as reported to [`Actions::reduce`].
///
/// Productions are numbered from 0 in the order they were added with
/// [`Grammar::rule`](crate::Grammar::rule), across all rules, so
/// [`index`](Production::index) identifies the alternative that matched.
///
/// # Examples
///
/// ```
/// # use grammar_lang::{Actions, Grammar, Kind, Production, Span, Token};
/// # use std::vec::Drain;
/// struct Record(Vec<usize>);
///
/// impl<'s> Actions<'s> for Record {
///     type Value = ();
///     fn token(&mut self, _: Token<Kind>, _: &'s str) {}
///     fn reduce(&mut self, production: Production, _: Span, _: Drain<'_, ()>) {
///         self.0.push(production.index());
///     }
/// }
///
/// let parser = Grammar::new()
///     .literal("a")
///     .literal("b")
///     .rule("s", &["a"]) // production 0
///     .rule("s", &["b"]) // production 1
///     .build()?;
///
/// let mut record = Record(Vec::new());
/// parser.parse_with("b", &mut record).unwrap();
/// assert_eq!(record.0, [1]);
/// # Ok::<(), grammar_lang::GrammarError>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Production {
    index: u32,
    rule: Kind,
}

impl Production {
    #[inline]
    pub(crate) const fn new(index: u32, rule: Kind) -> Self {
        Self { index, rule }
    }

    /// The production's number: its position among every
    /// [`rule`](crate::Grammar::rule) call, from 0.
    #[inline]
    #[must_use]
    pub const fn index(self) -> usize {
        self.index as usize
    }

    /// The kind of the rule the production belongs to.
    #[inline]
    #[must_use]
    pub const fn rule(self) -> Kind {
        self.rule
    }
}

/// Semantic actions: what to build as the parser recognizes the input.
///
/// [`Parser::parse_with`] calls [`token`](Actions::token) for each token the
/// parser consumes, and [`reduce`](Actions::reduce) each time a production is
/// complete, handing it the values already built for the production's symbols.
/// The calls arrive in postorder — children before their parent, left to
/// right — and the value returned for the start rule is the result of the
/// parse. This is the place to build an AST directly, evaluate as you go, or
/// collect only what a tool needs.
///
/// The `'s` lifetime is the input's: token text is borrowed from it, so values
/// can hold `&'s str` slices without copying.
///
/// # Examples
///
/// A calculator that evaluates while it parses:
///
/// ```
/// use grammar_lang::{Actions, Grammar, Kind, Production, Span, Token};
/// use std::vec::Drain;
///
/// struct Eval;
///
/// impl<'s> Actions<'s> for Eval {
///     type Value = i64;
///
///     fn token(&mut self, _: Token<Kind>, text: &'s str) -> i64 {
///         text.parse().unwrap_or(0) // operators evaluate to 0, unused
///     }
///
///     fn reduce(&mut self, p: Production, _: Span, mut children: Drain<'_, i64>) -> i64 {
///         match p.index() {
///             0 => {
///                 let (a, _, b) = (children.next(), children.next(), children.next());
///                 a.unwrap_or(0) + b.unwrap_or(0)
///             }
///             1 => {
///                 let (a, _, b) = (children.next(), children.next(), children.next());
///                 a.unwrap_or(0) * b.unwrap_or(0)
///             }
///             _ => children.next().unwrap_or(0),
///         }
///     }
/// }
///
/// let parser = Grammar::new()
///     .literal("+")
///     .literal("*")
///     .pattern("num", "[0-9]+")
///     .left(&["+"])
///     .left(&["*"])
///     .rule("e", &["e", "+", "e"]) // 0
///     .rule("e", &["e", "*", "e"]) // 1
///     .rule("e", &["num"])         // 2
///     .build()?;
///
/// assert_eq!(parser.parse_with("2+3*4", &mut Eval), Ok(14));
/// # Ok::<(), grammar_lang::GrammarError>(())
/// ```
pub trait Actions<'s> {
    /// What each token and each completed production becomes.
    type Value;

    /// Builds the value of a token the parser consumed.
    ///
    /// # Parameters
    ///
    /// - `token`: the token's kind and span. Skipped tokens never arrive here.
    /// - `text`: the token's text, borrowed from the input.
    fn token(&mut self, token: Token<Kind>, text: &'s str) -> Self::Value;

    /// Builds the value of a completed production.
    ///
    /// # Parameters
    ///
    /// - `production`: which production matched, and its rule.
    /// - `span`: the input the production covers. An empty production covers
    ///   an empty span just after the previous token.
    /// - `children`: the values of the production's symbols, in order, moved
    ///   out of the parser's value stack. Its length is the length of the
    ///   production. Values not taken are dropped.
    fn reduce(
        &mut self,
        production: Production,
        span: Span,
        children: alloc::vec::Drain<'_, Self::Value>,
    ) -> Self::Value;
}

impl Parser {
    /// Parses `text` into a concrete syntax tree.
    ///
    /// The tree has a node for every production the parse used and a leaf
    /// for every token, skipped tokens aside; see [`Tree`].
    ///
    /// # Errors
    ///
    /// Returns a [`ParseError`] at the first token the grammar does not allow,
    /// at a premature end of input, or at text no token matches. Input of
    /// 4 GiB or more is rejected, since spans are 32-bit.
    ///
    /// # Examples
    ///
    /// ```
    /// use grammar_lang::Grammar;
    ///
    /// let parser = Grammar::new()
    ///     .literal("=")
    ///     .pattern("ident", "[a-z]+")
    ///     .pattern("num", "[0-9]+")
    ///     .skip("space", " +")
    ///     .rule("assign", &["ident", "=", "num"])
    ///     .build()?;
    ///
    /// let tree = parser.parse("x = 42").unwrap();
    /// let root = tree.root();
    /// assert_eq!(root.name(), "assign");
    /// assert_eq!(root.child(2).map(|n| n.text()), Some("42"));
    ///
    /// assert!(parser.parse("x = y").is_err());
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    pub fn parse<'a>(&'a self, text: &'a str) -> Result<Tree<'a>, ParseError> {
        let mut builder = TreeBuilder::new(text.len());
        let root = self.parse_with(text, &mut builder)?;
        Ok(builder.finish(self, text, root))
    }

    /// Parses `text`, building the result with `actions`.
    ///
    /// See [`Actions`] for the order of calls. Nothing else is allocated
    /// beyond the parser's two stacks, so a parse that only computes a value
    /// costs only the work of recognizing the input.
    ///
    /// # Parameters
    ///
    /// - `text`: the input. Token text handed to `actions` borrows from it.
    /// - `actions`: the semantic actions. Taken by reference so whatever they
    ///   accumulate — an arena, a symbol table, counters — stays available
    ///   after the parse.
    ///
    /// # Errors
    ///
    /// The same as [`parse`](Parser::parse). When the parse fails, `actions`
    /// has seen the calls for the input before the error, and the values
    /// built so far are dropped.
    ///
    /// # Examples
    ///
    /// Counting tokens without building anything:
    ///
    /// ```
    /// use grammar_lang::{Actions, Grammar, Kind, Production, Span, Token};
    /// use std::vec::Drain;
    ///
    /// #[derive(Default)]
    /// struct Count(usize);
    ///
    /// impl<'s> Actions<'s> for Count {
    ///     type Value = ();
    ///     fn token(&mut self, _: Token<Kind>, _: &'s str) {
    ///         self.0 += 1;
    ///     }
    ///     fn reduce(&mut self, _: Production, _: Span, _: Drain<'_, ()>) {}
    /// }
    ///
    /// let parser = Grammar::new()
    ///     .pattern("word", "[a-z]+")
    ///     .skip("space", " +")
    ///     .rule("text", &["text", "word"])
    ///     .rule("text", &["word"])
    ///     .build()?;
    ///
    /// let mut count = Count::default();
    /// parser.parse_with("the quick brown fox", &mut count).unwrap();
    /// assert_eq!(count.0, 4);
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    ///
    /// Building a typed AST with borrowed text:
    ///
    /// ```
    /// use grammar_lang::{Actions, Grammar, Kind, Production, Span, Token};
    /// use std::vec::Drain;
    ///
    /// #[derive(Debug, PartialEq)]
    /// enum Ast<'s> {
    ///     Name(&'s str),
    ///     Call(&'s str, Vec<Ast<'s>>),
    ///     Args(Vec<Ast<'s>>),
    ///     Punct,
    /// }
    ///
    /// struct Build;
    ///
    /// impl<'s> Actions<'s> for Build {
    ///     type Value = Ast<'s>;
    ///
    ///     fn token(&mut self, token: Token<Kind>, text: &'s str) -> Ast<'s> {
    ///         if text.chars().all(char::is_alphabetic) { Ast::Name(text) } else { Ast::Punct }
    ///     }
    ///
    ///     fn reduce(&mut self, p: Production, _: Span, mut children: Drain<'_, Ast<'s>>) -> Ast<'s> {
    ///         match p.index() {
    ///             // call → name "(" args ")"
    ///             0 => match (children.next(), children.nth(1)) {
    ///                 (Some(Ast::Name(f)), Some(Ast::Args(args))) => Ast::Call(f, args),
    ///                 _ => Ast::Punct,
    ///             },
    ///             // call → name "(" ")"
    ///             1 => match children.next() {
    ///                 Some(Ast::Name(f)) => Ast::Call(f, Vec::new()),
    ///                 _ => Ast::Punct,
    ///             },
    ///             // args → args "," name
    ///             2 => match (children.next(), children.nth(1)) {
    ///                 (Some(Ast::Args(mut args)), Some(name)) => {
    ///                     args.push(name);
    ///                     Ast::Args(args)
    ///                 }
    ///                 _ => Ast::Punct,
    ///             },
    ///             // args → name
    ///             _ => Ast::Args(children.collect()),
    ///         }
    ///     }
    /// }
    ///
    /// let parser = Grammar::new()
    ///     .literal("(")
    ///     .literal(")")
    ///     .literal(",")
    ///     .pattern("name", "[a-z]+")
    ///     .rule("call", &["name", "(", "args", ")"])
    ///     .rule("call", &["name", "(", ")"])
    ///     .rule("args", &["args", ",", "name"])
    ///     .rule("args", &["name"])
    ///     .build()?;
    ///
    /// let ast = parser.parse_with("max(a,b)", &mut Build).unwrap();
    /// assert_eq!(ast, Ast::Call("max", vec![Ast::Name("a"), Ast::Name("b")]));
    /// assert_eq!(parser.parse_with("now()", &mut Build), Ok(Ast::Call("now", vec![])));
    /// assert!(parser.parse_with("f(,a)", &mut Build).is_err());
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    pub fn parse_with<'s, A: Actions<'s>>(
        &self,
        text: &'s str,
        actions: &mut A,
    ) -> Result<A::Value, ParseError> {
        if text.len() > u32::MAX as usize {
            return Err(self.too_large(text.len()));
        }
        let bytes = text.as_bytes();
        let terms = self.terms as usize;
        let nonterms = self.nonterms as usize;

        // Each stack entry is a state and the span of the symbol that
        // entered it; values run parallel, one per symbol.
        let mut stack: Vec<(u32, Span)> = Vec::with_capacity(32);
        stack.push((0, Span::empty(0)));
        let mut values: Vec<A::Value> = Vec::with_capacity(32);
        let mut pos = 0usize;
        let mut last_end = 0u32;

        let (mut term, mut span) = match self.next_parsed(bytes, &mut pos) {
            Some(lookahead) => lookahead,
            None => return Err(self.unrecognized(text, pos, Some(&[0]))),
        };
        loop {
            let state = stack.last().map_or(0, |&(s, _)| s as usize);
            let action = self.action[state * terms + term as usize];
            match action & 3 {
                SHIFT => {
                    let slice = &text[span.start().to_usize()..span.end().to_usize()];
                    values.push(actions.token(Token::new(Kind::token(term, false), span), slice));
                    stack.push((action >> 2, span));
                    last_end = span.end().to_u32();
                    (term, span) = match self.next_parsed(bytes, &mut pos) {
                        Some(lookahead) => lookahead,
                        None => {
                            let states: Vec<u32> = stack.iter().map(|&(s, _)| s).collect();
                            return Err(self.unrecognized(text, pos, Some(&states)));
                        }
                    };
                }
                REDUCE => {
                    let info = self.prods[(action >> 2) as usize];
                    let len = info.len as usize;
                    let base = stack.len() - len;
                    let covered = if len == 0 {
                        Span::empty(last_end)
                    } else {
                        let first = stack[base].1;
                        let last = stack[stack.len() - 1].1;
                        Span::new(first.start().to_u32(), last.end().to_u32())
                    };
                    let value = actions.reduce(
                        info.production,
                        covered,
                        values.drain(values.len() - len..),
                    );
                    stack.truncate(base);
                    let below = stack.last().map_or(0, |&(s, _)| s as usize);
                    let next = self.goto[below * nonterms + info.lhs as usize];
                    stack.push((next, covered));
                    values.push(value);
                }
                ACCEPT => {
                    if let Some(value) = values.pop() {
                        return Ok(value);
                    }
                    return Err(self.unexpected(text, term, span));
                }
                _ => return Err(self.unexpected(text, term, span)),
            }
        }
    }

    /// Splits `text` into tokens, skipped tokens included, without parsing.
    ///
    /// The iterator yields every token the lexer finds, in order, each by
    /// longest match. Skipped tokens report
    /// [`is_trivia`](crate::TokenKind::is_trivia) as true. Where no token
    /// matches, it yields an error covering one character and carries on
    /// after it, so a highlighter can show everything around a bad character.
    ///
    /// # Examples
    ///
    /// ```
    /// use grammar_lang::{Grammar, TokenKind};
    ///
    /// let parser = Grammar::new()
    ///     .literal("+")
    ///     .pattern("num", "[0-9]+")
    ///     .skip("space", " +")
    ///     .rule("sum", &["num", "+", "num"])
    ///     .build()?;
    ///
    /// let text = "1 + 22";
    /// let tokens: Vec<_> = parser
    ///     .tokens(text)
    ///     .filter_map(Result::ok)
    ///     .filter(|t| !t.is_trivia())
    ///     .map(|t| (parser.name(*t.kind()), &text[t.span().start().to_usize()..t.span().end().to_usize()]))
    ///     .collect();
    /// assert_eq!(tokens, [("num", "1"), ("+", "+"), ("num", "22")]);
    ///
    /// let errors = parser.tokens("1 ? 2").filter(Result::is_err).count();
    /// assert_eq!(errors, 1);
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[must_use]
    pub fn tokens<'a>(&'a self, text: &'a str) -> Tokens<'a> {
        Tokens {
            parser: self,
            text,
            pos: 0,
        }
    }

    /// The kind of the token or rule named `name`, if the grammar declares
    /// one.
    ///
    /// A literal token's name is its text.
    ///
    /// # Examples
    ///
    /// ```
    /// # use grammar_lang::Grammar;
    /// let parser = Grammar::new()
    ///     .literal("+")
    ///     .pattern("num", "[0-9]+")
    ///     .rule("sum", &["num", "+", "num"])
    ///     .build()?;
    /// assert!(parser.kind("+").is_some());
    /// assert!(parser.kind("sum").is_some_and(|k| k.is_rule()));
    /// assert_eq!(parser.kind("-"), None);
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[must_use]
    pub fn kind(&self, name: &str) -> Option<Kind> {
        let i = self
            .by_name
            .binary_search_by(|&k| (*self.names[k as usize]).cmp(name))
            .ok()?;
        Some(self.kind_at(self.by_name[i]))
    }

    /// The name of `kind`: the name the grammar declared for the token or
    /// rule. Returns an empty string for a kind from a different parser that
    /// is out of this one's range.
    ///
    /// # Examples
    ///
    /// ```
    /// # use grammar_lang::Grammar;
    /// let parser = Grammar::new().literal("x").rule("start", &["x"]).build()?;
    /// let tree = parser.parse("x").unwrap();
    /// assert_eq!(parser.name(tree.root().kind()), "start");
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[must_use]
    pub fn name(&self, kind: Kind) -> &str {
        self.names.get(kind.index()).map_or("", |n| n)
    }

    /// The kind with name-table index `index`.
    #[inline]
    pub(crate) fn kind_at(&self, index: u32) -> Kind {
        if index < self.lexed {
            Kind::token(index, index >= self.parsed)
        } else {
            Kind::rule(index)
        }
    }

    /// Whether `kind` is a literal token.
    #[inline]
    pub(crate) fn is_literal(&self, kind: Kind) -> bool {
        self.literal.get(kind.index()).copied().unwrap_or(false)
    }

    /// The next parsed token at or after `*pos` — skipped tokens passed over
    /// — as `(terminal, span)`; end of input is the terminal after the last
    /// token, with an empty span. `None` means no token matches at `*pos`.
    #[inline]
    fn next_parsed(&self, bytes: &[u8], pos: &mut usize) -> Option<(u32, Span)> {
        loop {
            let start = *pos;
            if start >= bytes.len() {
                return Some((self.parsed, Span::empty(start as u32)));
            }
            let (len, token) = self.dfa.longest_match(&bytes[start..]);
            if len == 0 {
                return None;
            }
            *pos = start + len;
            if token < self.parsed {
                return Some((token, Span::new(start as u32, *pos as u32)));
            }
        }
    }

    /// The parser's state stack as it stood when the token starting at byte
    /// `at` — end of input, when `at` is the input's length — became the
    /// lookahead, before any reduction on it.
    ///
    /// An LALR(1) table may reduce on a lookahead before discovering it is an
    /// error, which would lose some of the tokens that were acceptable. The
    /// parse is deterministic, so replaying it without actions reconstructs
    /// the earlier stack exactly; only error reporting pays for this, never
    /// the parse itself.
    fn replay(&self, bytes: &[u8], at: usize) -> Vec<u32> {
        let terms = self.terms as usize;
        let nonterms = self.nonterms as usize;
        let mut states = vec![0u32];
        let mut pos = 0;
        let Some((mut term, mut span)) = self.next_parsed(bytes, &mut pos) else {
            return states;
        };
        while span.start().to_usize() < at {
            let state = states.last().map_or(0, |&s| s as usize);
            let action = self.action[state * terms + term as usize];
            match action & 3 {
                SHIFT => {
                    states.push(action >> 2);
                    match self.next_parsed(bytes, &mut pos) {
                        Some(lookahead) => (term, span) = lookahead,
                        None => break,
                    }
                }
                REDUCE => {
                    let info = self.prods[(action >> 2) as usize];
                    states.truncate(states.len() - info.len as usize);
                    let below = states.last().map_or(0, |&s| s as usize);
                    states.push(self.goto[below * nonterms + info.lhs as usize]);
                }
                _ => break,
            }
        }
        states
    }

    /// Whether, from the stack `states`, the parser would shift `term` (or
    /// accept, for end of input) after the reductions it triggers.
    ///
    /// The stack is left untouched: reductions pop through it virtually and
    /// push onto `extra`. They always end, because a grammar whose rules
    /// could reduce around a cycle without consuming input is refused when it
    /// is built.
    fn shifts(&self, states: &[u32], term: u32, extra: &mut Vec<u32>) -> bool {
        let terms = self.terms as usize;
        let nonterms = self.nonterms as usize;
        extra.clear();
        let mut base = states.len();
        loop {
            let Some(top) = extra
                .last()
                .copied()
                .or_else(|| base.checked_sub(1).map(|i| states[i]))
            else {
                return false;
            };
            let action = self.action[top as usize * terms + term as usize];
            match action & 3 {
                SHIFT | ACCEPT => return true,
                REDUCE => {
                    let info = self.prods[(action >> 2) as usize];
                    let len = info.len as usize;
                    let from_extra = len.min(extra.len());
                    extra.truncate(extra.len() - from_extra);
                    let Some(rest) = base.checked_sub(len - from_extra) else {
                        return false;
                    };
                    base = rest;
                    let Some(below) = extra
                        .last()
                        .copied()
                        .or_else(|| base.checked_sub(1).map(|i| states[i]))
                    else {
                        return false;
                    };
                    extra.push(self.goto[below as usize * nonterms + info.lhs as usize]);
                }
                _ => return false,
            }
        }
    }

    /// The tokens the parser would accept from the stack `states`, and
    /// whether it would accept the end of input.
    fn expected(&self, states: &[u32]) -> (Box<[Kind]>, bool) {
        let mut extra = Vec::new();
        let kinds = (0..self.parsed)
            .filter(|&t| self.shifts(states, t, &mut extra))
            .map(|t| Kind::token(t, false))
            .collect();
        let end_ok = self.shifts(states, self.parsed, &mut extra);
        (kinds, end_ok)
    }

    /// An unexpected token, or an unexpected end of input.
    fn unexpected(&self, text: &str, term: u32, span: Span) -> ParseError {
        let states = self.replay(text.as_bytes(), span.start().to_usize());
        let (expected, end_ok) = self.expected(&states);
        let mut message = String::new();
        self.describe_expected(&mut message, &expected, end_ok);
        if message.is_empty() {
            message.push_str("unexpected ");
        } else {
            message.push_str(", found ");
        }
        if term == self.parsed {
            message.push_str("end of input");
            return ParseError::new(span, None, true, expected, message.into_boxed_str());
        }
        let kind = Kind::token(term, false);
        self.describe_token(
            &mut message,
            kind,
            &text[span.start().to_usize()..span.end().to_usize()],
        );
        ParseError::new(span, Some(kind), false, expected, message.into_boxed_str())
    }

    /// Text no token matches, at byte `at`. `states` is the parser's stack,
    /// when there is one, for the expected set.
    fn unrecognized(&self, text: &str, at: usize, states: Option<&[u32]>) -> ParseError {
        let c = text[at..].chars().next().unwrap_or('\u{FFFD}');
        let span = Span::new(at as u32, (at + c.len_utf8()) as u32);
        let mut message = String::from("unrecognized input `");
        message.extend(c.escape_debug());
        message.push('`');
        let (expected, end_ok) = match states {
            Some(states) => self.expected(states),
            None => (Box::default(), false),
        };
        if !expected.is_empty() || end_ok {
            message.push_str("; ");
            self.describe_expected(&mut message, &expected, end_ok);
        }
        ParseError::new(span, None, false, expected, message.into_boxed_str())
    }

    /// Input too long for 32-bit spans.
    fn too_large(&self, len: usize) -> ParseError {
        let mut message = String::from("the input is too large: ");
        push_number(&mut message, len);
        message.push_str(" bytes, where spans address at most 4294967295");
        ParseError::new(
            Span::empty(0),
            None,
            false,
            Box::default(),
            message.into_boxed_str(),
        )
    }

    /// Appends "expected `a`", "expected `a` or `b`", or "expected one of
    /// `a`, `b`, or `c`"; nothing when nothing is expected.
    fn describe_expected(&self, out: &mut String, expected: &[Kind], end_ok: bool) {
        let count = expected.len() + usize::from(end_ok);
        if count == 0 {
            return;
        }
        out.push_str(if count > 2 {
            "expected one of "
        } else {
            "expected "
        });
        let items = expected
            .iter()
            .map(|&k| Some(self.name(k)))
            .chain(end_ok.then_some(None));
        for (i, item) in items.enumerate() {
            if i > 0 {
                out.push_str(match (count, i + 1 == count) {
                    (2, _) => " or ",
                    (_, true) => ", or ",
                    _ => ", ",
                });
            }
            match item {
                Some(name) => {
                    out.push('`');
                    out.push_str(name);
                    out.push('`');
                }
                None => out.push_str("end of input"),
            }
        }
    }

    /// Appends a token as an error message shows it: a literal as its text,
    /// any other token as its name and text.
    fn describe_token(&self, out: &mut String, kind: Kind, text: &str) {
        out.push('`');
        out.push_str(self.name(kind));
        out.push('`');
        if self.is_literal(kind) {
            return;
        }
        out.push_str(" `");
        let mut chars = text.chars();
        for c in chars.by_ref().take(QUOTE_LIMIT) {
            out.extend(c.escape_debug());
        }
        if chars.next().is_some() {
            out.push('…');
        }
        out.push('`');
    }
}

/// Appends `n` in decimal.
fn push_number(out: &mut String, mut n: usize) {
    let mut digits = [0u8; 20];
    let mut i = digits.len();
    loop {
        i -= 1;
        digits[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    for &d in &digits[i..] {
        out.push(d as char);
    }
}

impl fmt::Debug for Parser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Parser")
            .field("tokens", &self.lexed)
            .field("rules", &(self.names.len() - self.lexed as usize))
            .field("productions", &(self.prods.len() - 1))
            .field("states", &(self.action.len() / self.terms as usize))
            .finish()
    }
}

/// The tokens of a text, from [`Parser::tokens`].
///
/// Yields `Ok` for each token, skipped tokens included, and `Err` for each
/// character no token matches, then resumes after it. Fused: once it returns
/// `None` it always does.
#[derive(Clone, Debug)]
pub struct Tokens<'a> {
    parser: &'a Parser,
    text: &'a str,
    pos: usize,
}

impl Iterator for Tokens<'_> {
    type Item = Result<Token<Kind>, ParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        let start = self.pos;
        if start >= self.text.len() {
            return None;
        }
        if self.text.len() > u32::MAX as usize {
            self.pos = self.text.len();
            return Some(Err(self.parser.too_large(self.text.len())));
        }
        let (len, token) = self
            .parser
            .dfa
            .longest_match(&self.text.as_bytes()[start..]);
        if len == 0 {
            let err = self.parser.unrecognized(self.text, start, None);
            self.pos = err.span().end().to_usize();
            return Some(Err(err));
        }
        self.pos = start + len;
        let kind = Kind::token(token, token >= self.parser.parsed);
        Some(Ok(Token::new(
            kind,
            Span::new(start as u32, self.pos as u32),
        )))
    }
}

impl FusedIterator for Tokens<'_> {}
