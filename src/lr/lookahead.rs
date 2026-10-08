//! LALR(1) lookahead sets by DeRemer and Pennello's method.
//!
//! Every nonterminal transition `(p, A)` of the LR(0) automaton gets a Follow
//! set of terminals; a completed production's lookahead in state `q` is the
//! union of the Follow sets of the transitions it "looks back" to. The sets
//! come from two relations solved with the same SCC-collapsing traversal:
//!
//! - `DR(p, A)`: terminals that can be shifted right after the transition.
//! - `(p, A) reads (r, C)`: `r = goto(p, A)` and `C` is nullable, so whatever
//!   follows `(r, C)` can follow `(p, A)` too. `Read` is `DR` closed over it.
//! - `(p, A) includes (p', B)`: some `B → β A γ` with `γ` nullable and
//!   `p' --β--> p`, so everything following `B` there follows `A` here.
//!   `Follow` is `Read` closed over it.
//! - `(q, A → ω) lookback (p, A)`: `p --ω--> q`.

use alloc::{vec, vec::Vec};

use super::Analysis;
use super::automaton::Automaton;
use crate::util::BitMatrix;

/// Each reduction's lookahead set, indexed like [`Automaton::reductions`].
pub(crate) struct Lookaheads {
    pub(crate) sets: BitMatrix,
}

/// Computes the LALR(1) lookahead set of every reduction in `automaton`.
pub(crate) fn compute(grammar: &Analysis<'_>, automaton: &Automaton) -> Lookaheads {
    let spec = grammar.spec;
    let terms = spec.terms;

    // Number the nonterminal transitions as `(state, symbol, target)`.
    // `id_of[i]` maps a position in `automaton.trans` to its number.
    let mut id_of = vec![u32::MAX; automaton.trans.len()];
    let mut ids: Vec<(usize, u32, usize)> = Vec::new();
    for state in 0..automaton.states() {
        let start = automaton.trans_start[state] as usize;
        for (i, &(sym, target)) in automaton.transitions(state).iter().enumerate() {
            if !grammar.is_term(sym) {
                id_of[start + i] = ids.len() as u32;
                ids.push((state, sym, target as usize));
            }
        }
    }
    let count = ids.len();
    let nt_id = |state: usize, sym: u32| -> Option<usize> {
        automaton
            .transition_index(state, sym)
            .map(|i| id_of[i] as usize)
    };

    // DR and reads.
    let mut follow = BitMatrix::new(count, terms);
    let mut reads = Relation::new(count);
    for (id, &(_, _, target)) in ids.iter().enumerate() {
        for &(sym, _) in automaton.transitions(target) {
            if grammar.is_term(sym) {
                follow.insert(id, sym as usize);
            } else if grammar.nullable[sym as usize - terms] {
                if let Some(other) = nt_id(target, sym) {
                    reads.push(id, other as u32);
                }
            }
        }
    }
    // End of input follows the start symbol: the augmented production is
    // `0 → start $`, and `$` is never shifted.
    if let Some(start_id) = nt_id(0, spec.prods[0].rhs[0]) {
        follow.insert(start_id, terms - 1);
    }
    digraph(&reads.finish(), &mut follow);

    // includes and lookback, from one walk of each production per transition.
    let mut includes = Relation::new(count);
    let mut lookback: Vec<(u32, u32)> = Vec::new();
    let mut path: Vec<usize> = Vec::new();
    for (id, &(state, sym, _)) in ids.iter().enumerate() {
        let lhs = sym as usize - terms;
        for &p in &grammar.prods_of[lhs] {
            let rhs = &spec.prods[p as usize].rhs;
            path.clear();
            path.push(state);
            let mut at = state;
            for &x in rhs {
                match automaton.goto(at, x) {
                    Some(next) => at = next as usize,
                    None => break,
                }
                path.push(at);
            }
            if path.len() != rhs.len() + 1 {
                continue;
            }
            let mut rest_nullable = true;
            for (i, &x) in rhs.iter().enumerate().rev() {
                if grammar.is_term(x) {
                    break;
                }
                if rest_nullable {
                    if let Some(inner) = nt_id(path[i], x) {
                        includes.push(inner, id as u32);
                    }
                }
                rest_nullable = grammar.nullable[x as usize - terms];
                if !rest_nullable {
                    break;
                }
            }
            if let Some(slot) = automaton.reduction_index(at, p) {
                lookback.push((slot as u32, id as u32));
            }
        }
    }
    digraph(&includes.finish(), &mut follow);

    let mut sets = BitMatrix::new(automaton.reductions.len(), terms);
    for &(slot, id) in &lookback {
        sets.union_from(slot as usize, follow.row(id as usize));
    }
    // The augmented production completes only in the accepting state, on end
    // of input.
    for state in 0..automaton.states() {
        if let Some(slot) = automaton.reduction_index(state, 0) {
            sets.insert(slot, terms - 1);
        }
    }
    Lookaheads { sets }
}

/// A relation over transition numbers, collected as pairs and frozen into
/// compressed adjacency lists.
struct Relation {
    nodes: usize,
    pairs: Vec<(u32, u32)>,
}

/// Compressed adjacency lists: node `x`'s successors are
/// `adj[start[x]..start[x + 1]]`.
struct Graph {
    start: Vec<u32>,
    adj: Vec<u32>,
}

impl Relation {
    fn new(nodes: usize) -> Self {
        Self {
            nodes,
            pairs: Vec::new(),
        }
    }

    fn push(&mut self, from: usize, to: u32) {
        self.pairs.push((from as u32, to));
    }

    fn finish(mut self) -> Graph {
        self.pairs.sort_unstable();
        self.pairs.dedup();
        let mut start = vec![0u32; self.nodes + 1];
        for &(from, _) in &self.pairs {
            start[from as usize + 1] += 1;
        }
        for i in 1..start.len() {
            start[i] += start[i - 1];
        }
        let adj = self.pairs.iter().map(|&(_, to)| to).collect();
        Graph { start, adj }
    }
}

/// DeRemer and Pennello's `Digraph`: closes `sets` over `graph`, so each node's
/// set becomes the union of the sets of every node it reaches. Nodes on a
/// cycle share one set. Iterative, so relation chains of any length are safe.
fn digraph(graph: &Graph, sets: &mut BitMatrix) {
    const DONE: u32 = u32::MAX;
    let n = graph.start.len() - 1;
    let mut depth = vec![0u32; n];
    let mut stack: Vec<u32> = Vec::new();
    // Call frames: (node, next edge, depth at entry).
    let mut frames: Vec<(u32, u32, u32)> = Vec::new();
    for root in 0..n {
        if depth[root] != 0 {
            continue;
        }
        stack.push(root as u32);
        depth[root] = stack.len() as u32;
        frames.push((root as u32, graph.start[root], stack.len() as u32));
        while let Some(frame) = frames.last_mut() {
            let (x, edge, entry) = *frame;
            let x = x as usize;
            if edge < graph.start[x + 1] {
                frame.1 += 1;
                let y = graph.adj[edge as usize] as usize;
                if depth[y] == 0 {
                    stack.push(y as u32);
                    depth[y] = stack.len() as u32;
                    frames.push((y as u32, graph.start[y], stack.len() as u32));
                } else {
                    depth[x] = depth[x].min(depth[y]);
                    sets.union_rows(x, y);
                }
                continue;
            }
            let _ = frames.pop();
            if depth[x] == entry {
                // `x` roots a strongly connected component: everything above
                // it on the stack shares its set.
                while let Some(top) = stack.pop() {
                    let top = top as usize;
                    depth[top] = DONE;
                    if top == x {
                        break;
                    }
                    sets.copy_row(top, x);
                }
            }
            if let Some(&(parent, _, _)) = frames.last() {
                let parent = parent as usize;
                depth[parent] = depth[parent].min(depth[x]);
                sets.union_rows(parent, x);
            }
        }
    }
}
