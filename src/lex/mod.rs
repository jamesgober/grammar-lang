//! The lexer generator: token patterns to one minimized, table-driven DFA.
//!
//! Every token — literal or pattern — becomes a fragment of a single NFA,
//! which is determinized and minimized into a [`Dfa`] that finds the longest
//! match at a position in one pass. When matches tie on length, the token
//! with the lower rank wins: literals before patterns, then declaration order.

mod dfa;
mod nfa;
pub(crate) mod regex;
mod utf8;

use alloc::vec::Vec;

pub(crate) use dfa::Dfa;
use dfa::DfaError;
use regex::Hir;

/// One token to compile.
pub(crate) struct Spec {
    /// The token's pattern.
    pub(crate) hir: Hir,
    /// Tie-break priority; lower wins.
    pub(crate) rank: u32,
    /// Declaration order, for choosing which error to report.
    pub(crate) order: u32,
}

/// Why the lexer could not be built. Token numbers index the spec slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LexError {
    /// Token `0`'s pattern alone needs too many NFA states.
    PatternTooLarge(u32),
    /// The combined automaton is too large.
    TooLarge,
    /// Token `0` matches the empty string.
    EmptyMatch(u32),
    /// Token `token` never wins a match over `by`.
    Shadowed { token: u32, by: u32 },
}

/// Compiles `tokens` into a scanning DFA. A match reports the token's index in
/// `tokens`.
pub(crate) fn build(tokens: &[Spec]) -> Result<Dfa, LexError> {
    let mut builder = nfa::Builder::new();
    let mut matches = Vec::with_capacity(tokens.len());
    for (i, token) in tokens.iter().enumerate() {
        let accept = builder
            .add(&token.hir, i as u32)
            .map_err(|_| LexError::PatternTooLarge(i as u32))?;
        matches.push(accept);
    }
    let nfa = builder.finish().map_err(|_| LexError::TooLarge)?;
    let rank: Vec<u32> = tokens.iter().map(|t| t.rank).collect();
    let order: Vec<u32> = tokens.iter().map(|t| t.order).collect();
    dfa::build(&nfa, &rank, &order, &matches).map_err(|e| match e {
        DfaError::TooLarge => LexError::TooLarge,
        DfaError::EmptyMatch(t) => LexError::EmptyMatch(t),
        DfaError::Shadowed { token, by } => LexError::Shadowed { token, by },
    })
}

#[cfg(test)]
mod tests {
    use alloc::{
        string::{String, ToString},
        vec,
    };

    use super::*;

    fn lexer(patterns: &[&str]) -> Dfa {
        let specs: Vec<Spec> = patterns
            .iter()
            .enumerate()
            .map(|(i, p)| Spec {
                hir: regex::parse(p).unwrap(),
                rank: i as u32,
                order: i as u32,
            })
            .collect();
        build(&specs).unwrap()
    }

    fn tokens(dfa: &Dfa, text: &str) -> Vec<(u32, String)> {
        let mut out = vec![];
        let mut pos = 0;
        while pos < text.len() {
            let (len, token) = dfa.longest_match(&text.as_bytes()[pos..]);
            assert!(len > 0, "stuck at {pos} in {text:?}");
            out.push((token, text[pos..pos + len].to_string()));
            pos += len;
        }
        out
    }

    #[test]
    fn longest_match_then_rank() {
        let dfa = lexer(&["if", "[a-z]+", "[0-9]+", " +", "=|=="]);
        let toks = tokens(&dfa, "if iffy == 42");
        assert_eq!(
            toks,
            vec![
                (0, "if".into()),
                (3, " ".into()),
                (1, "iffy".into()),
                (3, " ".into()),
                (4, "==".into()),
                (3, " ".into()),
                (2, "42".into()),
            ]
        );
    }

    #[test]
    fn unicode_classes_match_whole_characters() {
        let dfa = lexer(&["[α-ω]+", "[^α-ω]"]);
        let toks = tokens(&dfa, "αβγ€δ😀");
        assert_eq!(
            toks,
            vec![
                (0, "αβγ".into()),
                (1, "€".into()),
                (0, "δ".into()),
                (1, "😀".into()),
            ]
        );
    }

    #[test]
    fn no_match_reports_zero_length() {
        let dfa = lexer(&["a+"]);
        assert_eq!(dfa.longest_match(b"b"), (0, 0));
        assert_eq!(dfa.longest_match(b""), (0, 0));
        assert_eq!(dfa.longest_match(b"aab"), (2, 0));
    }

    #[test]
    fn backs_off_to_the_last_accepting_position() {
        // "/*" starts a comment that never closes; the scanner must fall back
        // to the "/" token rather than fail.
        let dfa = lexer(&[r"/\*([^*]|\*+[^*/])*\*+/", "/", r"\*"]);
        assert_eq!(dfa.longest_match(b"/* open"), (1, 1));
        assert_eq!(dfa.longest_match(b"/* c */x"), (7, 0));
    }

    #[test]
    fn minimization_merges_equivalent_states() {
        // (a|b)*abb has a four-state minimal DFA (plus the dead row).
        let dfa = lexer(&["(a|b)*abb"]);
        assert_eq!(dfa.states(), 5);
        // Equivalent spellings collapse to the same automaton.
        assert_eq!(lexer(&["a+|a+a"]).states(), lexer(&["a+"]).states());
    }

    #[test]
    fn errors() {
        let build_err = |patterns: &[&str]| {
            let specs: Vec<Spec> = patterns
                .iter()
                .enumerate()
                .map(|(i, p)| Spec {
                    hir: regex::parse(p).unwrap(),
                    rank: i as u32,
                    order: i as u32,
                })
                .collect();
            build(&specs).unwrap_err()
        };
        assert_eq!(build_err(&["a", "b*"]), LexError::EmptyMatch(1));
        assert_eq!(
            build_err(&["[a-z]+", "abc"]),
            LexError::Shadowed { token: 1, by: 0 }
        );
        assert_eq!(build_err(&["(a|b)*a(a|b){16}"]), LexError::TooLarge);
        assert_eq!(
            build_err(&["((a{1000}){1000}){2}"]),
            LexError::PatternTooLarge(0)
        );
    }
}
