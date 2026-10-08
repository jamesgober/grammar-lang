//! The LR(0) automaton.
//!
//! A state is identified by its kernel — the items with the dot past the
//! start, plus the initial item — kept sorted so equal kernels compare equal.
//! Closures are never stored: the nonterminals a kernel pulls in come from the
//! precomputed left-corner closure, one bitset union per kernel item.

use alloc::vec::Vec;

use super::{Analysis, CELL_LIMIT};
use crate::util::{BitMatrix, SliceMap, bits};

/// The LR(0) automaton of a grammar.
pub(crate) struct Automaton {
    /// Each state's kernel items.
    pub(crate) kernels: SliceMap,
    /// State `s`'s transitions are `trans[trans_start[s]..trans_start[s + 1]]`,
    /// as `(symbol, target)` sorted by symbol — terminals first.
    pub(crate) trans_start: Vec<u32>,
    pub(crate) trans: Vec<(u32, u32)>,
    /// State `s`'s completed productions are
    /// `reductions[red_start[s]..red_start[s + 1]]`.
    pub(crate) red_start: Vec<u32>,
    pub(crate) reductions: Vec<u32>,
}

impl Automaton {
    /// The number of states.
    #[inline]
    pub(crate) fn states(&self) -> usize {
        self.kernels.len()
    }

    /// State `state`'s transitions.
    #[inline]
    pub(crate) fn transitions(&self, state: usize) -> &[(u32, u32)] {
        &self.trans[self.trans_start[state] as usize..self.trans_start[state + 1] as usize]
    }

    /// The position in [`Automaton::trans`] of `state`'s transition on `sym`.
    #[inline]
    pub(crate) fn transition_index(&self, state: usize, sym: u32) -> Option<usize> {
        let start = self.trans_start[state] as usize;
        self.transitions(state)
            .binary_search_by_key(&sym, |&(s, _)| s)
            .ok()
            .map(|i| start + i)
    }

    /// The state reached from `state` on `sym`.
    #[inline]
    pub(crate) fn goto(&self, state: usize, sym: u32) -> Option<u32> {
        self.transition_index(state, sym).map(|i| self.trans[i].1)
    }

    /// State `state`'s completed productions.
    #[inline]
    pub(crate) fn reductions(&self, state: usize) -> &[u32] {
        &self.reductions[self.red_start[state] as usize..self.red_start[state + 1] as usize]
    }

    /// The position in [`Automaton::reductions`] of `prod` in `state`.
    pub(crate) fn reduction_index(&self, state: usize, prod: u32) -> Option<usize> {
        let start = self.red_start[state] as usize;
        self.reductions(state)
            .iter()
            .position(|&p| p == prod)
            .map(|i| start + i)
    }

    /// Every item of `state`'s closure, kernel first.
    pub(crate) fn closure(&self, grammar: &Analysis<'_>, state: usize) -> Vec<u32> {
        let mut items: Vec<u32> = self.kernels.get(state).to_vec();
        let mut pulled = BitMatrix::new(1, grammar.spec.nonterms);
        for &item in self.kernels.get(state) {
            if let Some(sym) = grammar.next_symbol(item) {
                if !grammar.is_term(sym) {
                    let nt = sym as usize - grammar.spec.terms;
                    pulled.union_from(0, grammar.left_closure.row(nt));
                }
            }
        }
        for nt in bits(pulled.row(0)) {
            for &p in &grammar.prods_of[nt] {
                items.push(grammar.item_base[p as usize]);
            }
        }
        items
    }
}

/// Builds the automaton, or `None` if it outgrows the table limits.
pub(crate) fn build(grammar: &Analysis<'_>) -> Option<Automaton> {
    let spec = grammar.spec;
    let symbols = spec.terms + spec.nonterms;

    let mut kernels = SliceMap::new();
    let _ = kernels.insert(&[grammar.item_base[0]]);
    let mut trans_start = Vec::new();
    let mut trans = Vec::new();
    let mut red_start = Vec::new();
    let mut reductions = Vec::new();

    let mut pulled = BitMatrix::new(1, spec.nonterms);
    let mut buckets: Vec<Vec<u32>> = (0..symbols).map(|_| Vec::new()).collect();
    let mut touched: Vec<u32> = Vec::new();
    let mut kernel: Vec<u32> = Vec::new();

    let mut state = 0;
    while state < kernels.len() {
        kernel.clear();
        kernel.extend_from_slice(kernels.get(state));
        trans_start.push(trans.len() as u32);
        red_start.push(reductions.len() as u32);
        pulled.clear_row(0);

        // Kernel items: advance the dot, or record a completed production.
        for &item in &kernel {
            match grammar.next_symbol(item) {
                Some(sym) => {
                    let bucket = &mut buckets[sym as usize];
                    if bucket.is_empty() {
                        touched.push(sym);
                    }
                    bucket.push(item + 1);
                    if !grammar.is_term(sym) {
                        let nt = sym as usize - spec.terms;
                        pulled.union_from(0, grammar.left_closure.row(nt));
                    }
                }
                None => reductions.push(grammar.item(item).0),
            }
        }
        // Closure items: every production of every pulled-in nonterminal,
        // with the dot at the start.
        for nt in bits(pulled.row(0)) {
            for &p in &grammar.prods_of[nt] {
                let item = grammar.item_base[p as usize];
                match grammar.next_symbol(item) {
                    Some(sym) => {
                        let bucket = &mut buckets[sym as usize];
                        if bucket.is_empty() {
                            touched.push(sym);
                        }
                        bucket.push(item + 1);
                    }
                    None => reductions.push(p),
                }
            }
        }

        // Reductions in production order, which is the order conflicts are
        // resolved and reported in.
        let first_reduction = *red_start.last().unwrap_or(&0) as usize;
        reductions[first_reduction..].sort_unstable();

        touched.sort_unstable();
        for &sym in &touched {
            let bucket = &mut buckets[sym as usize];
            bucket.sort_unstable();
            let (target, _) = kernels.insert(bucket);
            bucket.clear();
            trans.push((sym, target));
        }
        touched.clear();

        if kernels.len().saturating_mul(symbols) > CELL_LIMIT {
            return None;
        }
        state += 1;
    }
    trans_start.push(trans.len() as u32);
    red_start.push(reductions.len() as u32);

    Some(Automaton {
        kernels,
        trans_start,
        trans,
        red_start,
        reductions,
    })
}
