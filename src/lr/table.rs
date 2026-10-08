//! Dense ACTION and GOTO tables, with conflict resolution.
//!
//! An ACTION cell is one `u32`: 0 is a syntax error, and otherwise the low two
//! bits tag the action and the rest is its operand — a target state for a
//! shift, a production for a reduce. Dense rows make the parser's inner loop a
//! single indexed load per step.
//!
//! Conflicts are settled exactly as Bison settles them, so a grammar written
//! for yacc or Bison means the same thing here — except that a conflict
//! precedence does not resolve is an error, never a silent default.

use alloc::{boxed::Box, vec, vec::Vec};
use core::cmp::Ordering;

use super::automaton::Automaton;
use super::lookahead::Lookaheads;
use super::{Analysis, Associativity, CELL_LIMIT, LrError};
use crate::util::bits;

/// Shift: the operand is the state to push.
pub(crate) const SHIFT: u32 = 1;
/// Reduce: the operand is the production.
pub(crate) const REDUCE: u32 = 2;
/// Accept (the whole cell).
pub(crate) const ACCEPT: u32 = 3;

/// The parse tables.
#[derive(Clone, Debug)]
pub(crate) struct Table {
    /// `state * terms + terminal` → action.
    pub(crate) action: Box<[u32]>,
    /// `state * nonterms + nonterminal` → state.
    pub(crate) goto: Box<[u32]>,
}

/// A conflict precedence could not resolve. Productions and terminals are in
/// the generator's numbering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Conflict {
    /// On `term`, the parser could shift — continuing the item `shift`, a
    /// `(production, dot)` pair — or reduce by `reduce`.
    ShiftReduce {
        term: u32,
        shift: (u32, usize),
        reduce: u32,
    },
    /// On `term`, the parser could reduce by either production.
    ReduceReduce { term: u32, first: u32, second: u32 },
}

/// Fills the tables.
///
/// Each state is settled in three steps:
///
/// 1. In production order, every reduction whose production has a precedence
///    is weighed against each shift of a token that has one. The higher level
///    wins; on a tie, left associativity reduces, right shifts, and
///    non-associative does neither and makes the token an explicit error. A
///    losing shift is disabled and a losing reduction drops that lookahead.
/// 2. A lookahead still claimed by a shift and a reduction, or by two
///    reductions, is a conflict.
/// 3. States no input can reach once the disabled shifts are gone are
///    dropped, and only conflicts in reachable states are reported: a state
///    the parser can never enter cannot make it ambiguous.
pub(crate) fn build(
    grammar: &Analysis<'_>,
    automaton: &Automaton,
    lookaheads: &Lookaheads,
) -> Result<Table, LrError> {
    let spec = grammar.spec;
    let (terms, nonterms) = (spec.terms, spec.nonterms);
    let states = automaton.states();
    if states.saturating_mul(terms + nonterms) > CELL_LIMIT {
        return Err(LrError::TooLarge);
    }
    let words = terms.div_ceil(64);
    let mut action = vec![0u32; states * terms];
    let mut conflicts: Vec<(usize, Conflict)> = Vec::new();

    let mut shifts = vec![0u64; words];
    let mut errors = vec![0u64; words];
    let mut seen = vec![0u64; words];
    let mut la: Vec<u64> = Vec::new();
    let mut contested: Vec<usize> = Vec::new();

    for state in 0..states {
        shifts.fill(0);
        errors.fill(0);
        for &(sym, _) in automaton.transitions(state) {
            if grammar.is_term(sym) {
                set(&mut shifts, sym as usize);
            }
        }
        let reductions = automaton.reductions(state);
        let first_slot = automaton.red_start[state] as usize;
        la.clear();
        for i in 0..reductions.len() {
            la.extend_from_slice(lookaheads.sets.row(first_slot + i));
        }

        // 1. Precedence.
        for (i, &prod) in reductions.iter().enumerate() {
            let Some(prod_prec) = spec.prods[prod as usize].prec else {
                continue;
            };
            let row = &mut la[i * words..(i + 1) * words];
            contested.clear();
            contested.extend(bits(row).filter(|&t| has(&shifts, t)));
            for &t in &contested {
                let Some(tok_prec) = spec.term_prec[t] else {
                    continue;
                };
                let (keep_shift, keep_reduce) = match prod_prec.level.cmp(&tok_prec.level) {
                    Ordering::Greater => (false, true),
                    Ordering::Less => (true, false),
                    Ordering::Equal => match tok_prec.assoc {
                        Associativity::Left => (false, true),
                        Associativity::Right => (true, false),
                        Associativity::NonAssoc => (false, false),
                    },
                };
                if !keep_shift {
                    clear(&mut shifts, t);
                }
                if !keep_reduce {
                    clear(row, t);
                }
                if !keep_shift && !keep_reduce {
                    set(&mut errors, t);
                }
            }
        }

        // 2. Conflicts precedence left standing.
        seen.copy_from_slice(&shifts);
        let mut conflict = None;
        for (i, &prod) in reductions.iter().enumerate() {
            let row = &la[i * words..(i + 1) * words];
            if conflict.is_none() {
                if let Some(t) = bits(row).find(|&t| has(&seen, t)) {
                    conflict = Some(if has(&shifts, t) {
                        let shift = automaton
                            .closure(grammar, state)
                            .into_iter()
                            .find(|&item| grammar.next_symbol(item) == Some(t as u32))
                            .map_or((prod, 0), |item| grammar.item(item));
                        Conflict::ShiftReduce {
                            term: t as u32,
                            shift,
                            reduce: prod,
                        }
                    } else {
                        let first = (0..i)
                            .find(|&j| has(&la[j * words..(j + 1) * words], t))
                            .map_or(prod, |j| reductions[j]);
                        Conflict::ReduceReduce {
                            term: t as u32,
                            first,
                            second: prod,
                        }
                    });
                }
            }
            for (s, r) in seen.iter_mut().zip(row) {
                *s |= *r;
            }
        }
        if let Some(conflict) = conflict {
            conflicts.push((state, conflict));
        }

        // The row: reductions (the earliest production taking a contested
        // token), then the shifts precedence kept, then the explicit errors.
        let row = &mut action[state * terms..(state + 1) * terms];
        for (i, &prod) in reductions.iter().enumerate().rev() {
            let cell = if prod == 0 {
                ACCEPT
            } else {
                (prod << 2) | REDUCE
            };
            for t in bits(&la[i * words..(i + 1) * words]) {
                row[t] = cell;
            }
        }
        for &(sym, target) in automaton.transitions(state) {
            if grammar.is_term(sym) && has(&shifts, sym as usize) {
                row[sym as usize] = (target << 2) | SHIFT;
            }
        }
        for t in bits(&errors) {
            row[t] = 0;
        }
    }

    // 3. Reachability through the surviving shifts and every goto.
    let mut reachable = vec![false; states];
    reachable[0] = true;
    let mut queue = vec![0usize];
    while let Some(state) = queue.pop() {
        let shifted = action[state * terms..(state + 1) * terms]
            .iter()
            .filter(|&&a| a & 3 == SHIFT)
            .map(|&a| (a >> 2) as usize);
        let gone = automaton
            .transitions(state)
            .iter()
            .filter(|&&(sym, _)| !grammar.is_term(sym))
            .map(|&(_, target)| target as usize);
        for next in shifted.chain(gone) {
            if !reachable[next] {
                reachable[next] = true;
                queue.push(next);
            }
        }
    }
    if let Some((_, conflict)) = conflicts.into_iter().find(|&(s, _)| reachable[s]) {
        return Err(LrError::Conflict(conflict));
    }

    // Keep the reachable states, renumbered in order; state 0 stays the
    // start.
    let mut renumber = vec![u32::MAX; states];
    let mut kept = 0usize;
    for (old, _) in reachable.iter().enumerate().filter(|&(_, &r)| r) {
        renumber[old] = kept as u32;
        kept += 1;
    }
    let mut compact_action = vec![0u32; kept * terms];
    let mut compact_goto = vec![0u32; kept * nonterms];
    for old in (0..states).filter(|&s| reachable[s]) {
        let new = renumber[old] as usize;
        let src = &action[old * terms..(old + 1) * terms];
        let dst = &mut compact_action[new * terms..(new + 1) * terms];
        for (d, &a) in dst.iter_mut().zip(src) {
            *d = if a & 3 == SHIFT {
                (renumber[(a >> 2) as usize] << 2) | SHIFT
            } else {
                a
            };
        }
        for &(sym, target) in automaton.transitions(old) {
            if !grammar.is_term(sym) {
                let nt = sym as usize - terms;
                compact_goto[new * nonterms + nt] = renumber[target as usize];
            }
        }
    }

    Ok(Table {
        action: compact_action.into_boxed_slice(),
        goto: compact_goto.into_boxed_slice(),
    })
}

#[inline]
fn has(words: &[u64], bit: usize) -> bool {
    words[bit / 64] & (1 << (bit % 64)) != 0
}

#[inline]
fn set(words: &mut [u64], bit: usize) {
    words[bit / 64] |= 1 << (bit % 64);
}

#[inline]
fn clear(words: &mut [u64], bit: usize) {
    words[bit / 64] &= !(1 << (bit % 64));
}
