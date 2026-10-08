//! Textbook grammars with known LALR(1) outcomes.

use alloc::{vec, vec::Vec};

use super::table::{ACCEPT, REDUCE, SHIFT};
use super::*;

/// Builds a spec from rules written as `(lhs, rhs)` over single characters:
/// uppercase letters are nonterminals (the first rule's left side is the
/// start), anything else is a terminal.
fn spec(rules: &[(&str, &str)]) -> (Spec, Vec<char>) {
    let mut terms: Vec<char> = Vec::new();
    let mut nts: Vec<char> = Vec::new();
    for &(lhs, rhs) in rules {
        for c in lhs.chars().chain(rhs.chars()) {
            let list = if c.is_ascii_uppercase() {
                &mut nts
            } else {
                &mut terms
            };
            if !list.contains(&c) {
                list.push(c);
            }
        }
    }
    let t = terms.len() + 1; // plus end of input
    let sym = |c: char| -> u32 {
        if c.is_ascii_uppercase() {
            (t + 1 + nts.iter().position(|&x| x == c).unwrap()) as u32
        } else {
            terms.iter().position(|&x| x == c).unwrap() as u32
        }
    };
    let mut prods = vec![Prod {
        lhs: 0,
        rhs: vec![t as u32 + 1],
        prec: None,
    }];
    for &(lhs, rhs) in rules {
        prods.push(Prod {
            lhs: sym(lhs.chars().next().unwrap()) - t as u32,
            rhs: rhs.chars().map(sym).collect(),
            prec: None,
        });
    }
    let spec = Spec {
        terms: t,
        nonterms: nts.len() + 1,
        prods,
        term_prec: vec![None; t],
    };
    (spec, terms)
}

/// Runs the table over `input` (terminal characters) and reports acceptance.
fn accepts(spec: &Spec, terms: &[char], table: &Table, input: &str) -> bool {
    let t = spec.terms;
    let mut stack = vec![0usize];
    let mut tokens: Vec<usize> = input
        .chars()
        .map(|c| terms.iter().position(|&x| x == c).unwrap())
        .collect();
    tokens.push(t - 1);
    let mut i = 0;
    loop {
        let state = *stack.last().unwrap();
        let act = table.action[state * t + tokens[i]];
        match act & 3 {
            SHIFT => {
                stack.push((act >> 2) as usize);
                i += 1;
            }
            REDUCE => {
                let prod = &spec.prods[(act >> 2) as usize];
                stack.truncate(stack.len() - prod.rhs.len());
                let top = *stack.last().unwrap();
                stack.push(table.goto[top * spec.nonterms + prod.lhs as usize] as usize);
            }
            _ if act == ACCEPT => return true,
            _ => return false,
        }
    }
}

#[test]
fn lalr_but_not_slr() {
    // Dragon book 4.49: S → L = R | R; L → * R | i; R → L.
    let (spec, terms) = spec(&[
        ("S", "L=R"),
        ("S", "R"),
        ("L", "*R"),
        ("L", "i"),
        ("R", "L"),
    ]);
    let table = build(&spec).unwrap();
    for ok in ["i", "i=i", "*i=**i", "**i"] {
        assert!(accepts(&spec, &terms, &table, ok), "{ok}");
    }
    for bad in ["", "=", "i=", "i=i=i", "*"] {
        assert!(!accepts(&spec, &terms, &table, bad), "{bad}");
    }
}

#[test]
fn lr1_but_not_lalr_is_a_reduce_reduce_conflict() {
    // Merging the LR(1) states for `c` after `a` and after `b` makes the
    // lookaheads of A → c and B → c collide.
    let (spec, _) = spec(&[
        ("S", "aAd"),
        ("S", "bBd"),
        ("S", "aBe"),
        ("S", "bAe"),
        ("A", "c"),
        ("B", "c"),
    ]);
    assert!(matches!(
        build(&spec),
        Err(LrError::Conflict(Conflict::ReduceReduce { .. }))
    ));
}

#[test]
fn nullable_chains_carry_lookaheads() {
    // S → A B c; A → a | ε; B → b | ε. Reducing A → ε must see `b` (DR) and
    // `c` (through the nullable B: reads).
    let (spec, terms) = spec(&[("S", "ABc"), ("A", "a"), ("A", ""), ("B", "b"), ("B", "")]);
    let table = build(&spec).unwrap();
    for ok in ["c", "ac", "bc", "abc"] {
        assert!(accepts(&spec, &terms, &table, ok), "{ok}");
    }
    for bad in ["", "ab", "ca", "aabc"] {
        assert!(!accepts(&spec, &terms, &table, bad), "{bad}");
    }
}

#[test]
fn includes_carries_follow_through_tails() {
    // S → x T; T → U; U → y | ε — reducing U → ε needs end of input, which
    // reaches U only through T (includes).
    let (spec, terms) = spec(&[("S", "xT"), ("T", "U"), ("U", "y"), ("U", "")]);
    let table = build(&spec).unwrap();
    assert!(accepts(&spec, &terms, &table, "x"));
    assert!(accepts(&spec, &terms, &table, "xy"));
    assert!(!accepts(&spec, &terms, &table, "xyy"));
}

#[test]
fn ambiguity_is_a_shift_reduce_conflict() {
    let (spec, _) = spec(&[("E", "E+E"), ("E", "n")]);
    match build(&spec) {
        Err(LrError::Conflict(Conflict::ShiftReduce { reduce, shift, .. })) => {
            assert_eq!(reduce, 1);
            assert_eq!(shift, (1, 1));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn precedence_resolves_ambiguity() {
    let (mut spec, terms) = spec(&[("E", "E+E"), ("E", "E*E"), ("E", "n")]);
    let plus = terms.iter().position(|&c| c == '+').unwrap();
    let times = terms.iter().position(|&c| c == '*').unwrap();
    let left = |level| {
        Some(Prec {
            level,
            assoc: Associativity::Left,
        })
    };
    spec.term_prec[plus] = left(1);
    spec.term_prec[times] = left(2);
    spec.prods[1].prec = left(1);
    spec.prods[2].prec = left(2);
    let table = build(&spec).unwrap();
    assert!(accepts(&spec, &terms, &table, "n+n*n+n"));
    assert!(!accepts(&spec, &terms, &table, "n+*n"));
}

#[test]
fn deep_unit_chains_do_not_overflow() {
    // A chain of 2000 unit rules exercises the iterative digraph.
    let n = 2000;
    let terms = 2; // `x` and end of input
    let mut prods = vec![Prod {
        lhs: 0,
        rhs: vec![terms as u32 + 1],
        prec: None,
    }];
    for i in 1..n {
        prods.push(Prod {
            lhs: i,
            rhs: vec![terms as u32 + i + 1],
            prec: None,
        });
    }
    prods.push(Prod {
        lhs: n,
        rhs: vec![0],
        prec: None,
    });
    let spec = Spec {
        terms,
        nonterms: n as usize + 1,
        prods,
        term_prec: vec![None; terms],
    };
    let table = build(&spec).unwrap();
    assert!(accepts(&spec, &['x'], &table, "x"));
}
