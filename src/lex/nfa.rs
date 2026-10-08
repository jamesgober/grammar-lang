//! Thompson construction: every token pattern becomes a fragment of one
//! byte-level NFA, joined at a shared start state.

use alloc::{vec, vec::Vec};

use super::regex::Hir;
use super::utf8::{self, Sequence};

/// The most NFA states the lexer may need. Patterns stay far below this; the
/// bound exists so a pattern like `(a{1000}){1000}` fails cleanly instead of
/// exhausting memory.
const STATE_LIMIT: usize = 1 << 20;

/// One NFA state.
#[derive(Clone, Debug)]
pub(crate) enum State {
    /// On a byte in `lo..=hi`, move to `next`.
    Range { lo: u8, hi: u8, next: u32 },
    /// Move to any of these states without consuming input.
    Split(Vec<u32>),
    /// The pattern of token `0` has matched.
    Match(u32),
}

/// The combined NFA for every token.
#[derive(Debug)]
pub(crate) struct Nfa {
    pub(crate) states: Vec<State>,
    pub(crate) start: u32,
}

/// The NFA grew past [`STATE_LIMIT`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TooLarge;

/// Builds the NFA one token at a time.
pub(crate) struct Builder {
    states: Vec<State>,
    starts: Vec<u32>,
    seqs: Vec<Sequence>,
}

impl Builder {
    pub(crate) fn new() -> Self {
        Self {
            states: Vec::new(),
            starts: Vec::new(),
            seqs: Vec::new(),
        }
    }

    /// Adds a token whose pattern is `hir`; reaching its end yields
    /// `Match(token)`. Returns the id of that match state.
    pub(crate) fn add(&mut self, hir: &Hir, token: u32) -> Result<u32, TooLarge> {
        let accept = self.push(State::Match(token))?;
        let start = self.compile(hir, accept)?;
        self.starts.push(start);
        Ok(accept)
    }

    /// Joins the tokens under one start state.
    pub(crate) fn finish(mut self) -> Result<Nfa, TooLarge> {
        let starts = core::mem::take(&mut self.starts);
        let start = self.push(State::Split(starts))?;
        Ok(Nfa {
            states: self.states,
            start,
        })
    }

    fn push(&mut self, state: State) -> Result<u32, TooLarge> {
        if self.states.len() >= STATE_LIMIT {
            return Err(TooLarge);
        }
        self.states.push(state);
        Ok((self.states.len() - 1) as u32)
    }

    /// Compiles `hir` so that matching it leads to `next`; returns the
    /// fragment's entry state. Fragments are built back to front, so every
    /// state is created with its successor already known — only the loop of an
    /// unbounded repetition needs a patch.
    fn compile(&mut self, hir: &Hir, next: u32) -> Result<u32, TooLarge> {
        match hir {
            Hir::Empty => Ok(next),
            Hir::Class(ranges) => self.class(ranges, next),
            Hir::Concat(parts) => {
                let mut target = next;
                for part in parts.iter().rev() {
                    target = self.compile(part, target)?;
                }
                Ok(target)
            }
            Hir::Alt(branches) => {
                let mut entries = Vec::with_capacity(branches.len());
                for branch in branches {
                    entries.push(self.compile(branch, next)?);
                }
                self.push(State::Split(entries))
            }
            Hir::Repeat { hir, min, max } => {
                let mut target = match max {
                    None => {
                        // A loop: `entry` either runs the body back into
                        // itself or leaves.
                        let entry = self.push(State::Split(Vec::new()))?;
                        let body = self.compile(hir, entry)?;
                        self.states[entry as usize] = State::Split(vec![body, next]);
                        entry
                    }
                    Some(max) => {
                        // `max - min` optional copies, nested so each may
                        // stop early: (x(x(x)?)?)?
                        let mut target = next;
                        for _ in *min..*max {
                            let body = self.compile(hir, target)?;
                            target = self.push(State::Split(vec![body, next]))?;
                        }
                        target
                    }
                };
                for _ in 0..*min {
                    target = self.compile(hir, target)?;
                }
                Ok(target)
            }
        }
    }

    /// One scalar value from `ranges`, as UTF-8.
    fn class(&mut self, ranges: &[(u32, u32)], next: u32) -> Result<u32, TooLarge> {
        let mut seqs = core::mem::take(&mut self.seqs);
        seqs.clear();
        for &(lo, hi) in ranges {
            utf8::sequences(lo, hi, &mut seqs);
        }
        let mut entries = Vec::with_capacity(seqs.len());
        for seq in &seqs {
            let mut target = next;
            for &(lo, hi) in seq.ranges().iter().rev() {
                target = self.push(State::Range {
                    lo,
                    hi,
                    next: target,
                })?;
            }
            entries.push(target);
        }
        self.seqs = seqs;
        if entries.len() == 1 {
            Ok(entries[0])
        } else {
            self.push(State::Split(entries))
        }
    }
}
