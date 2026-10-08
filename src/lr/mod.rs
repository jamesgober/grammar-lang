//! The parser generator: LALR(1) parse tables from a context-free grammar.
//!
//! Construction runs in three stages:
//!
//! 1. [`automaton`] builds the LR(0) automaton — the canonical collection of
//!    item sets, kept as kernels.
//! 2. [`lookahead`] computes LALR(1) lookahead sets with DeRemer and
//!    Pennello's relations (*Efficient Computation of LALR(1) Look-Ahead
//!    Sets*, 1982), which propagate terminal sets along the automaton's
//!    nonterminal transitions instead of building LR(1) item sets.
//! 3. [`table`] fills dense ACTION and GOTO tables, resolving shift/reduce
//!    conflicts with yacc's precedence and associativity rules and reporting
//!    every other conflict as an error.

mod automaton;
mod lookahead;
pub(crate) mod table;

use alloc::{vec, vec::Vec};

pub(crate) use table::{Conflict, Table};

use crate::util::BitMatrix;

/// How operators of one precedence level group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Associativity {
    Left,
    Right,
    NonAssoc,
}

/// A precedence level (higher binds tighter) and its associativity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Prec {
    pub(crate) level: u32,
    pub(crate) assoc: Associativity,
}

/// One production, `lhs → rhs`.
#[derive(Clone, Debug)]
pub(crate) struct Prod {
    pub(crate) lhs: u32,
    /// Symbols: below [`Spec::terms`] a terminal, otherwise
    /// `terms + nonterminal`.
    pub(crate) rhs: Vec<u32>,
    pub(crate) prec: Option<Prec>,
}

/// A grammar in the generator's numbering.
///
/// Terminal `terms - 1` is end of input. Nonterminal 0 is the augmented start
/// symbol, and production 0 is `0 → start`; reducing it on end of input
/// accepts.
#[derive(Clone, Debug)]
pub(crate) struct Spec {
    pub(crate) terms: usize,
    pub(crate) nonterms: usize,
    pub(crate) prods: Vec<Prod>,
    pub(crate) term_prec: Vec<Option<Prec>>,
}

/// Why no table could be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LrError {
    /// A conflict precedence does not resolve.
    Conflict(Conflict),
    /// The automaton or its tables exceed the size limits.
    TooLarge,
}

/// The most cells the ACTION and GOTO tables may hold together: 256 MiB of
/// `u32`. Real grammars need a small fraction of this.
const CELL_LIMIT: usize = 1 << 26;

/// Builds the LALR(1) tables for `spec`.
pub(crate) fn build(spec: &Spec) -> Result<Table, LrError> {
    let grammar = Analysis::new(spec);
    let automaton = automaton::build(&grammar).ok_or(LrError::TooLarge)?;
    let lookaheads = lookahead::compute(&grammar, &automaton);
    table::build(&grammar, &automaton, &lookaheads)
}

/// A [`Spec`] with the derived facts every stage needs.
pub(crate) struct Analysis<'a> {
    pub(crate) spec: &'a Spec,
    /// Each nonterminal's productions.
    pub(crate) prods_of: Vec<Vec<u32>>,
    /// Item `i` is production `item_prod[i]` with its dot at
    /// `i - item_base[item_prod[i]]`.
    pub(crate) item_base: Vec<u32>,
    pub(crate) item_prod: Vec<u32>,
    /// Which nonterminals derive the empty string.
    pub(crate) nullable: Vec<bool>,
    /// Row `A`: every nonterminal whose productions join an LR(0) closure that
    /// `A` joins — `A` itself and every left corner reachable from it.
    pub(crate) left_closure: BitMatrix,
}

impl<'a> Analysis<'a> {
    fn new(spec: &'a Spec) -> Self {
        let mut prods_of = vec![Vec::new(); spec.nonterms];
        let mut item_base = Vec::with_capacity(spec.prods.len());
        let mut item_prod = Vec::new();
        for (p, prod) in spec.prods.iter().enumerate() {
            prods_of[prod.lhs as usize].push(p as u32);
            item_base.push(item_prod.len() as u32);
            item_prod.extend(core::iter::repeat_n(p as u32, prod.rhs.len() + 1));
        }

        let mut nullable = vec![false; spec.nonterms];
        let mut changed = true;
        while changed {
            changed = false;
            for prod in &spec.prods {
                if !nullable[prod.lhs as usize]
                    && prod
                        .rhs
                        .iter()
                        .all(|&s| (s as usize) >= spec.terms && nullable[s as usize - spec.terms])
                {
                    nullable[prod.lhs as usize] = true;
                    changed = true;
                }
            }
        }

        // Left corners by depth-first search from each nonterminal.
        let n = spec.nonterms;
        let mut left_closure = BitMatrix::new(n, n);
        let mut stack = Vec::new();
        for a in 0..n {
            left_closure.insert(a, a);
            stack.push(a);
            while let Some(b) = stack.pop() {
                for &p in &prods_of[b] {
                    if let Some(&first) = spec.prods[p as usize].rhs.first() {
                        let first = first as usize;
                        if first >= spec.terms {
                            let c = first - spec.terms;
                            if !left_closure.contains(a, c) {
                                left_closure.insert(a, c);
                                stack.push(c);
                            }
                        }
                    }
                }
            }
        }

        Self {
            spec,
            prods_of,
            item_base,
            item_prod,
            nullable,
            left_closure,
        }
    }

    /// Whether symbol `sym` is a terminal.
    #[inline]
    pub(crate) fn is_term(&self, sym: u32) -> bool {
        (sym as usize) < self.spec.terms
    }

    /// The production and dot position of item `item`.
    #[inline]
    pub(crate) fn item(&self, item: u32) -> (u32, usize) {
        let prod = self.item_prod[item as usize];
        (prod, (item - self.item_base[prod as usize]) as usize)
    }

    /// The symbol after the dot of `item`, if the dot is not at the end.
    #[inline]
    pub(crate) fn next_symbol(&self, item: u32) -> Option<u32> {
        let (prod, dot) = self.item(item);
        self.spec.prods[prod as usize].rhs.get(dot).copied()
    }
}

#[cfg(test)]
mod tests;
