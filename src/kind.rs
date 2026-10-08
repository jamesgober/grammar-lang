//! [`Kind`]: what a token or tree node is.

use core::fmt;

use token_lang::TokenKind;

/// Set on the kinds of rules.
const RULE: u32 = 1 << 31;
/// Set on the kinds of skipped tokens.
const TRIVIA: u32 = 1 << 30;
/// The bits that hold the index.
const INDEX: u32 = TRIVIA - 1;

/// What a token or a tree node is: one of the grammar's tokens or rules.
///
/// Every name a [`Grammar`](crate::Grammar) declares gets a `Kind` when the
/// grammar is built. Look one up by name with
/// [`Parser::kind`](crate::Parser::kind), and turn one back into its name with
/// [`Parser::name`](crate::Parser::name). A `Kind` is a plain `Copy` value, so
/// the usual way to dispatch on it is to look the interesting kinds up once and
/// compare.
///
/// `Kind` implements [`TokenKind`]: [`is_trivia`](TokenKind::is_trivia) is
/// true for tokens declared with [`Grammar::skip`](crate::Grammar::skip),
/// which [`Parser::tokens`](crate::Parser::tokens) reports and the parser
/// passes over.
///
/// Kinds belong to the parser that issued them; a kind from a different
/// parser names an unrelated token or rule.
///
/// # Examples
///
/// ```
/// use grammar_lang::{Grammar, TokenKind};
///
/// let parser = Grammar::new()
///     .pattern("word", "[a-z]+")
///     .skip("space", " +")
///     .rule("words", &["words", "word"])
///     .rule("words", &["word"])
///     .build()?;
///
/// let word = parser.kind("word").unwrap();
/// let space = parser.kind("space").unwrap();
/// let words = parser.kind("words").unwrap();
///
/// assert!(word.is_token() && !word.is_trivia());
/// assert!(space.is_token() && space.is_trivia());
/// assert!(words.is_rule());
/// assert_eq!(parser.name(words), "words");
/// # Ok::<(), grammar_lang::GrammarError>(())
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Kind(u32);

impl Kind {
    /// The kind of token `index`.
    #[inline]
    pub(crate) const fn token(index: u32, trivia: bool) -> Self {
        Self(index | if trivia { TRIVIA } else { 0 })
    }

    /// The kind of rule `index`.
    #[inline]
    pub(crate) const fn rule(index: u32) -> Self {
        Self(index | RULE)
    }

    /// The position of this kind in its parser's name table.
    #[inline]
    pub(crate) const fn index(self) -> usize {
        (self.0 & INDEX) as usize
    }

    /// Whether this is the kind of a token.
    ///
    /// # Examples
    ///
    /// ```
    /// # use grammar_lang::Grammar;
    /// let parser = Grammar::new().literal("x").rule("start", &["x"]).build()?;
    /// assert!(parser.kind("x").unwrap().is_token());
    /// assert!(!parser.kind("start").unwrap().is_token());
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_token(self) -> bool {
        self.0 & RULE == 0
    }

    /// Whether this is the kind of a rule.
    ///
    /// # Examples
    ///
    /// ```
    /// # use grammar_lang::Grammar;
    /// let parser = Grammar::new().literal("x").rule("start", &["x"]).build()?;
    /// assert!(parser.kind("start").unwrap().is_rule());
    /// assert!(!parser.kind("x").unwrap().is_rule());
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_rule(self) -> bool {
        self.0 & RULE != 0
    }
}

impl TokenKind for Kind {
    #[inline]
    fn is_trivia(&self) -> bool {
        self.0 & TRIVIA != 0
    }
}

impl fmt::Debug for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = if self.is_rule() {
            "rule"
        } else if self.is_trivia() {
            "skip"
        } else {
            "token"
        };
        write!(f, "Kind({what} {})", self.index())
    }
}

#[cfg(test)]
mod tests {
    use alloc::format;

    use super::*;

    #[test]
    fn flags_and_index() {
        let t = Kind::token(3, false);
        let s = Kind::token(4, true);
        let r = Kind::rule(5);
        assert!(t.is_token() && !t.is_rule() && !t.is_trivia());
        assert!(s.is_token() && s.is_trivia());
        assert!(r.is_rule() && !r.is_token() && !r.is_trivia());
        assert_eq!((t.index(), s.index(), r.index()), (3, 4, 5));
        assert_eq!(
            format!("{t:?} {s:?} {r:?}"),
            "Kind(token 3) Kind(skip 4) Kind(rule 5)"
        );
    }
}
