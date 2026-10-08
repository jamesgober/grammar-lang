//! [`GrammarError`] and [`ParseError`].

use alloc::boxed::Box;
use core::fmt;

use diag_lang::{Diagnostic, Label, Severity};
use token_lang::Span;

use crate::Kind;

/// Why [`Grammar::build`](crate::Grammar::build) rejected a grammar.
///
/// Building checks the grammar from its names down to its parse table and
/// reports the first problem, in this order: the declarations themselves
/// (names, precedences, the symbols each rule uses), then the token patterns
/// and the lexer they make, then the rules that can be reached from the start
/// rule, and finally the LALR(1) table. Names in the error are the names the
/// grammar declared.
///
/// # Examples
///
/// ```
/// use grammar_lang::{Grammar, GrammarError};
///
/// let err = Grammar::new()
///     .literal("+")
///     .pattern("num", "[0-9]+")
///     .rule("expr", &["expr", "+", "expr"])
///     .rule("expr", &["num"])
///     .build()
///     .unwrap_err();
///
/// // `1 + 2 + 3` could group either way.
/// assert!(matches!(err, GrammarError::ShiftReduce { .. }));
/// assert_eq!(
///     err.to_string(),
///     "shift/reduce conflict on `+`: shift in `expr → expr • + expr`, \
///      or reduce `expr → expr + expr`",
/// );
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GrammarError {
    /// The grammar declares no rules, so there is nothing to parse.
    NoRules,
    /// A name is declared twice: two tokens share it, or a token and a rule
    /// do. (Several [`rule`](crate::Grammar::rule) calls with one name are
    /// alternatives, not duplicates.)
    Duplicate {
        /// The name.
        name: Box<str>,
    },
    /// A rule uses a name that is neither a token nor a rule.
    Undefined {
        /// The unknown name.
        name: Box<str>,
        /// The rule whose production uses it.
        rule: Box<str>,
    },
    /// A rule uses a token declared with [`skip`](crate::Grammar::skip),
    /// which the parser never sees.
    SkipInRule {
        /// The skipped token.
        name: Box<str>,
        /// The rule whose production uses it.
        rule: Box<str>,
    },
    /// [`prec`](crate::Grammar::prec) was called before any production
    /// existed to apply it to.
    MisplacedPrec {
        /// The precedence name passed to `prec`.
        name: Box<str>,
    },
    /// A production's [`prec`](crate::Grammar::prec) names something no
    /// precedence level declares.
    UndefinedPrecedence {
        /// The name.
        name: Box<str>,
    },
    /// A name appears in more than one precedence level, or twice in one.
    DuplicatePrecedence {
        /// The name.
        name: Box<str>,
    },
    /// A precedence level names a rule. Precedence belongs to tokens, and to
    /// marker names used only through [`prec`](crate::Grammar::prec).
    PrecedenceOnRule {
        /// The rule's name.
        name: Box<str>,
    },
    /// A token pattern is not a valid regular expression, or is too large to
    /// compile.
    InvalidPattern {
        /// The token.
        name: Box<str>,
        /// The byte offset in the pattern where the problem was found.
        offset: usize,
        /// What is wrong.
        reason: &'static str,
    },
    /// A token can match the empty string, which would let the lexer produce
    /// tokens forever without consuming input.
    EmptyToken {
        /// The token.
        name: Box<str>,
    },
    /// A token can never be produced: on every input it matches, a longer
    /// match or a token of higher priority wins.
    ShadowedToken {
        /// The token that is never produced.
        name: Box<str>,
        /// A token that wins over it.
        by: Box<str>,
    },
    /// A rule reachable from the start rule cannot derive any finite input:
    /// every one of its productions needs the rule itself again, directly or
    /// through other rules.
    Unproductive {
        /// The rule.
        name: Box<str>,
    },
    /// A rule reachable from the start rule can derive itself and nothing
    /// else — `a → a`, or `a → b c` with `b → a` and `c` able to match
    /// nothing — so some input has infinitely many parse trees.
    Cycle {
        /// A rule on the cycle.
        name: Box<str>,
    },
    /// The grammar is not LALR(1): on `token`, the parser could both shift
    /// and reduce, and no precedence settles which.
    ShiftReduce {
        /// The lookahead token.
        token: Box<str>,
        /// An item that shifts the token, as `rule → before • after`.
        shift: Box<str>,
        /// The production that could be reduced, as `rule → symbols`.
        reduce: Box<str>,
    },
    /// The grammar is not LALR(1): on the same lookahead, the parser could
    /// reduce by two different productions.
    ReduceReduce {
        /// The lookahead token, or `None` for the end of input.
        token: Option<Box<str>>,
        /// One production that could be reduced.
        first: Box<str>,
        /// The other.
        second: Box<str>,
    },
    /// The grammar's lexer or parse tables would exceed the size limits.
    TooLarge,
}

impl fmt::Display for GrammarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRules => f.write_str("the grammar declares no rules"),
            Self::Duplicate { name } => write!(f, "`{name}` is declared more than once"),
            Self::Undefined { name, rule } => {
                write!(
                    f,
                    "rule `{rule}` uses `{name}`, which is not a token or rule"
                )
            }
            Self::SkipInRule { name, rule } => {
                write!(f, "rule `{rule}` uses `{name}`, which is a skipped token")
            }
            Self::MisplacedPrec { name } => {
                write!(
                    f,
                    "precedence `{name}` is applied before any rule is declared"
                )
            }
            Self::UndefinedPrecedence { name } => {
                write!(f, "precedence `{name}` is not declared at any level")
            }
            Self::DuplicatePrecedence { name } => {
                write!(f, "`{name}` is given a precedence more than once")
            }
            Self::PrecedenceOnRule { name } => {
                write!(f, "`{name}` is a rule and cannot have a precedence")
            }
            Self::InvalidPattern {
                name,
                offset,
                reason,
            } => write!(f, "invalid pattern for `{name}` at byte {offset}: {reason}"),
            Self::EmptyToken { name } => write!(f, "token `{name}` matches the empty string"),
            Self::ShadowedToken { name, by } => {
                write!(
                    f,
                    "token `{name}` is never produced; `{by}` takes priority over it"
                )
            }
            Self::Unproductive { name } => {
                write!(f, "rule `{name}` cannot derive any finite input")
            }
            Self::Cycle { name } => write!(
                f,
                "rule `{name}` can derive itself, so some input has infinitely many parse trees"
            ),
            Self::ShiftReduce {
                token,
                shift,
                reduce,
            } => write!(
                f,
                "shift/reduce conflict on `{token}`: shift in `{shift}`, or reduce `{reduce}`"
            ),
            Self::ReduceReduce {
                token,
                first,
                second,
            } => {
                f.write_str("reduce/reduce conflict on ")?;
                match token {
                    Some(token) => write!(f, "`{token}`")?,
                    None => f.write_str("end of input")?,
                }
                write!(f, ": reduce `{first}`, or reduce `{second}`")
            }
            Self::TooLarge => {
                f.write_str("the grammar's lexer or parse tables exceed the size limits")
            }
        }
    }
}

impl core::error::Error for GrammarError {}

/// Why [`Parser::parse`](crate::Parser::parse) rejected its input.
///
/// Parsing stops at the first error. The error records where it happened, the
/// token found there, and the tokens the grammar would have accepted instead,
/// and its message names them the way the grammar does:
///
/// - an unexpected token — [`found`](ParseError::found) is its kind, and the
///   span covers it;
/// - the input ended early — [`at_end`](ParseError::at_end) is true, and the
///   span is empty at the end of the input;
/// - no token matches the text — `found` is `None`, and the span covers the
///   first character no token can start with.
///
/// [`to_diagnostic`](ParseError::to_diagnostic) turns it into a
/// [`diag_lang::Diagnostic`] for the family's renderer.
///
/// # Examples
///
/// ```
/// use grammar_lang::Grammar;
///
/// let parser = Grammar::new()
///     .literal("(")
///     .literal(")")
///     .pattern("num", "[0-9]+")
///     .skip("space", " +")
///     .rule("group", &["(", "num", ")"])
///     .build()?;
///
/// let err = parser.parse("(1 2)").unwrap_err();
/// assert_eq!(err.to_string(), "expected `)`, found `num` `2`");
/// assert_eq!(err.found(), parser.kind("num"));
/// assert_eq!(err.expected(), [parser.kind(")").unwrap()]);
///
/// let err = parser.parse("(1").unwrap_err();
/// assert!(err.at_end());
/// assert_eq!(err.to_string(), "expected `)`, found end of input");
/// # Ok::<(), grammar_lang::GrammarError>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    span: Span,
    found: Option<Kind>,
    at_end: bool,
    expected: Box<[Kind]>,
    message: Box<str>,
    label: &'static str,
}

impl ParseError {
    /// Assembles an error; the parser renders the message.
    pub(crate) fn new(
        span: Span,
        found: Option<Kind>,
        at_end: bool,
        expected: Box<[Kind]>,
        message: Box<str>,
    ) -> Self {
        let label = if found.is_some() {
            "unexpected token"
        } else if at_end {
            "input ends here"
        } else {
            "no token matches here"
        };
        Self {
            span,
            found,
            at_end,
            expected,
            message,
            label,
        }
    }

    /// The byte range the error points at: the unexpected token, the
    /// unrecognized character, or an empty span at the end of the input.
    ///
    /// # Examples
    ///
    /// ```
    /// # use grammar_lang::{Grammar, Span};
    /// let parser = Grammar::new()
    ///     .literal("a")
    ///     .skip("space", " +")
    ///     .rule("s", &["a"])
    ///     .build()?;
    /// assert_eq!(parser.parse("a a").unwrap_err().span(), Span::new(2, 3));
    /// assert_eq!(parser.parse("").unwrap_err().span(), Span::new(0, 0));
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[inline]
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }

    /// The kind of the unexpected token, or `None` when the input ended or no
    /// token matched.
    ///
    /// # Examples
    ///
    /// ```
    /// # use grammar_lang::Grammar;
    /// let parser = Grammar::new().literal("a").literal("b").rule("s", &["a"]).build()?;
    /// assert_eq!(parser.parse("b").unwrap_err().found(), parser.kind("b"));
    /// assert_eq!(parser.parse("?").unwrap_err().found(), None);
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[inline]
    #[must_use]
    pub const fn found(&self) -> Option<Kind> {
        self.found
    }

    /// Whether the input ended where the grammar required more.
    ///
    /// # Examples
    ///
    /// ```
    /// # use grammar_lang::Grammar;
    /// let parser = Grammar::new().literal("a").rule("s", &["a", "a"]).build()?;
    /// assert!(parser.parse("a").unwrap_err().at_end());
    /// assert!(!parser.parse("aaa").unwrap_err().at_end());
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[inline]
    #[must_use]
    pub const fn at_end(&self) -> bool {
        self.at_end
    }

    /// The tokens the grammar would have accepted at this point, in
    /// declaration order. Whether the input could have ended here instead is
    /// part of the [message](ParseError#impl-Display-for-ParseError), as it
    /// has no `Kind`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use grammar_lang::Grammar;
    /// let parser = Grammar::new()
    ///     .literal("a")
    ///     .literal("b")
    ///     .literal("c")
    ///     .rule("s", &["a", "b"])
    ///     .rule("s", &["a", "c"])
    ///     .build()?;
    /// let err = parser.parse("aa").unwrap_err();
    /// assert_eq!(err.found(), parser.kind("a"));
    /// let names: Vec<&str> = err.expected().iter().map(|&k| parser.name(k)).collect();
    /// assert_eq!(names, ["b", "c"]);
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn expected(&self) -> &[Kind] {
        &self.expected
    }

    /// This error as a [`Diagnostic`]: the message, and a primary label on
    /// the span.
    ///
    /// # Examples
    ///
    /// ```
    /// use grammar_lang::Grammar;
    /// use diag_lang::Severity;
    ///
    /// let parser = Grammar::new().literal("a").rule("s", &["a"]).build()?;
    /// let diag = parser.parse("b").unwrap_err().to_diagnostic();
    /// assert_eq!(diag.severity(), Severity::Error);
    /// assert_eq!(diag.message(), "unrecognized input `b`; expected `a`");
    /// assert_eq!(diag.primary().message(), "no token matches here");
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[must_use]
    pub fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic::new(
            Severity::Error,
            &*self.message,
            Label::new(self.span, self.label),
        )
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl core::error::Error for ParseError {}
