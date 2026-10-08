//! Property tests against independent reference implementations.
//!
//! - Random grammars are checked against a canonical LR(1) construction with
//!   states merged by core — the definition of LALR(1) — so the generator's
//!   DeRemer–Pennello lookaheads must report a conflict exactly when the
//!   reference does. For grammars that build, every short input is checked
//!   against an Earley recognizer, and every tree against the grammar.
//! - Random patterns are checked against a naive matcher that interprets the
//!   pattern's structure directly.
//! - Arbitrary input never makes a parser panic, and the token stream always
//!   tiles the input.

use std::collections::{BTreeMap, BTreeSet};

use grammar_lang::{Grammar, GrammarError, Node, Parser, TokenKind};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Random grammars
// ---------------------------------------------------------------------------

/// A symbol of a test grammar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Sym {
    T(usize),
    N(usize),
}

/// A small grammar: terminals are the literals `a`, `b`, `c`...; nonterminals
/// are `S` (the start), `A`, `B`, ...; `prods[i]` is `(lhs, rhs)`.
#[derive(Clone, Debug)]
struct TestGrammar {
    terms: usize,
    nonterms: usize,
    prods: Vec<(usize, Vec<Sym>)>,
}

const TERM_NAMES: [&str; 3] = ["a", "b", "c"];
const NONTERM_NAMES: [&str; 4] = ["S", "A", "B", "C"];

impl TestGrammar {
    fn build(&self) -> Result<Parser, GrammarError> {
        let mut grammar = Grammar::new();
        for name in &TERM_NAMES[..self.terms] {
            grammar = grammar.literal(name);
        }
        grammar = grammar.skip("space", " +");
        for (lhs, rhs) in &self.prods {
            let symbols: Vec<&str> = rhs
                .iter()
                .map(|s| match *s {
                    Sym::T(t) => TERM_NAMES[t],
                    Sym::N(n) => NONTERM_NAMES[n],
                })
                .collect();
            grammar = grammar.rule(NONTERM_NAMES[*lhs], &symbols);
        }
        grammar.build()
    }

    fn reachable(&self) -> Vec<bool> {
        let mut seen = vec![false; self.nonterms];
        seen[0] = true;
        let mut stack = vec![0];
        while let Some(n) = stack.pop() {
            for (lhs, rhs) in &self.prods {
                if *lhs != n {
                    continue;
                }
                for s in rhs {
                    if let Sym::N(m) = *s {
                        if !seen[m] {
                            seen[m] = true;
                            stack.push(m);
                        }
                    }
                }
            }
        }
        seen
    }

    fn productive(&self) -> Vec<bool> {
        let mut ok = vec![false; self.nonterms];
        loop {
            let mut changed = false;
            for (lhs, rhs) in &self.prods {
                if !ok[*lhs]
                    && rhs.iter().all(|s| match *s {
                        Sym::T(_) => true,
                        Sym::N(m) => ok[m],
                    })
                {
                    ok[*lhs] = true;
                    changed = true;
                }
            }
            if !changed {
                return ok;
            }
        }
    }

    fn nullable(&self) -> Vec<bool> {
        let mut null = vec![false; self.nonterms];
        loop {
            let mut changed = false;
            for (lhs, rhs) in &self.prods {
                if !null[*lhs] && rhs.iter().all(|s| matches!(*s, Sym::N(m) if null[m])) {
                    null[*lhs] = true;
                    changed = true;
                }
            }
            if !changed {
                return null;
            }
        }
    }

    /// Whether a reachable nonterminal derives itself alone (`A ⇒+ A`).
    fn has_cycle(&self, reachable: &[bool]) -> bool {
        let nullable = self.nullable();
        let mut edges = vec![Vec::new(); self.nonterms];
        for (lhs, rhs) in &self.prods {
            for (i, s) in rhs.iter().enumerate() {
                if let Sym::N(b) = *s {
                    let rest = rhs
                        .iter()
                        .enumerate()
                        .all(|(j, s)| j == i || matches!(*s, Sym::N(m) if nullable[m]));
                    if rest {
                        edges[*lhs].push(b);
                    }
                }
            }
        }
        (0..self.nonterms).filter(|&a| reachable[a]).any(|a| {
            let mut seen = vec![false; self.nonterms];
            let mut stack = edges[a].clone();
            while let Some(n) = stack.pop() {
                if n == a {
                    return true;
                }
                if !seen[n] {
                    seen[n] = true;
                    stack.extend(edges[n].iter().copied());
                }
            }
            false
        })
    }

    /// FIRST sets of the nonterminals (terminal indices).
    fn first(&self, nullable: &[bool]) -> Vec<BTreeSet<usize>> {
        let mut first = vec![BTreeSet::new(); self.nonterms];
        loop {
            let mut changed = false;
            for (lhs, rhs) in &self.prods {
                for s in rhs {
                    match *s {
                        Sym::T(t) => {
                            changed |= first[*lhs].insert(t);
                            break;
                        }
                        Sym::N(m) => {
                            let add: Vec<usize> = first[m].iter().copied().collect();
                            for t in add {
                                changed |= first[*lhs].insert(t);
                            }
                            if !nullable[m] {
                                break;
                            }
                        }
                    }
                }
            }
            if !changed {
                return first;
            }
        }
    }
}

/// The reference verdict: does the canonical LR(1) automaton, merged by
/// core, have a conflict? Production `usize::MAX` is the augmented
/// `S' → S`, and terminal `terms` is end of input.
fn lalr_by_merging_lr1(g: &TestGrammar) -> bool {
    let eof = g.terms;
    let nullable = g.nullable();
    let first = g.first(&nullable);
    const AUG: usize = usize::MAX;
    let rhs_of = |p: usize| -> Vec<Sym> {
        if p == AUG {
            vec![Sym::N(0)]
        } else {
            g.prods[p].1.clone()
        }
    };
    // FIRST of a symbol string followed by a lookahead.
    let first_seq = |seq: &[Sym], la: usize| -> BTreeSet<usize> {
        let mut out = BTreeSet::new();
        for s in seq {
            match *s {
                Sym::T(t) => {
                    out.insert(t);
                    return out;
                }
                Sym::N(m) => {
                    out.extend(first[m].iter().copied());
                    if !nullable[m] {
                        return out;
                    }
                }
            }
        }
        out.insert(la);
        out
    };
    type Item = (usize, usize, usize); // (prod, dot, lookahead)
    let closure = |kernel: BTreeSet<Item>| -> BTreeSet<Item> {
        let mut set = kernel;
        let mut work: Vec<Item> = set.iter().copied().collect();
        while let Some((p, dot, la)) = work.pop() {
            let rhs = rhs_of(p);
            if let Some(&Sym::N(b)) = rhs.get(dot) {
                for lookahead in first_seq(&rhs[dot + 1..], la) {
                    for (q, (lhs, _)) in g.prods.iter().enumerate() {
                        if *lhs == b && set.insert((q, 0, lookahead)) {
                            work.push((q, 0, lookahead));
                        }
                    }
                }
            }
        }
        set
    };

    let start = closure(BTreeSet::from([(AUG, 0, eof)]));
    let mut states: Vec<BTreeSet<Item>> = vec![start.clone()];
    let mut index: BTreeMap<BTreeSet<Item>, usize> = BTreeMap::from([(start, 0)]);
    let mut i = 0;
    while i < states.len() {
        let mut moves: BTreeMap<Sym, BTreeSet<Item>> = BTreeMap::new();
        for &(p, dot, la) in &states[i] {
            if let Some(&s) = rhs_of(p).get(dot) {
                moves.entry(s).or_default().insert((p, dot + 1, la));
            }
        }
        for (_, kernel) in moves {
            let next = closure(kernel);
            if !index.contains_key(&next) {
                index.insert(next.clone(), states.len());
                states.push(next);
            }
        }
        i += 1;
    }

    // Merge by core and look for conflicts.
    let mut merged: BTreeMap<BTreeSet<(usize, usize)>, BTreeSet<Item>> = BTreeMap::new();
    for state in states {
        let core: BTreeSet<(usize, usize)> = state.iter().map(|&(p, d, _)| (p, d)).collect();
        merged.entry(core).or_default().extend(state);
    }
    for items in merged.values() {
        let mut reduce_on: BTreeMap<usize, usize> = BTreeMap::new();
        let mut shift_on: BTreeSet<usize> = BTreeSet::new();
        for &(p, dot, la) in items {
            let rhs = rhs_of(p);
            match rhs.get(dot) {
                Some(&Sym::T(t)) => {
                    shift_on.insert(t);
                }
                Some(&Sym::N(_)) => {}
                None => {
                    if let Some(&other) = reduce_on.get(&la) {
                        if other != p {
                            return true;
                        }
                    }
                    reduce_on.insert(la, p);
                }
            }
        }
        if reduce_on.keys().any(|t| shift_on.contains(t)) {
            return true;
        }
    }
    false
}

/// Earley's item sets for `input` (terminal indices), from `S`. Set `k` is
/// non-empty exactly when `input[..k]` is a prefix of some sentence (every
/// reachable rule is productive in the grammars this is used on).
fn earley(g: &TestGrammar, input: &[usize]) -> Vec<BTreeSet<(usize, usize, usize)>> {
    let nullable = g.nullable();
    type Item = (usize, usize, usize); // (prod, dot, origin); prod == len = S'
    let aug = g.prods.len();
    let rhs_of = |p: usize| -> &[Sym] {
        if p == aug {
            &[Sym::N(0)]
        } else {
            &g.prods[p].1
        }
    };
    let mut sets: Vec<BTreeSet<Item>> = vec![BTreeSet::new(); input.len() + 1];
    sets[0].insert((aug, 0, 0));
    for k in 0..=input.len() {
        loop {
            let items: Vec<Item> = sets[k].iter().copied().collect();
            let before = sets[k].len();
            for (p, dot, origin) in items {
                let rhs = rhs_of(p);
                match rhs.get(dot) {
                    Some(&Sym::N(b)) => {
                        for (q, (lhs, _)) in g.prods.iter().enumerate() {
                            if *lhs == b {
                                sets[k].insert((q, 0, k));
                            }
                        }
                        if nullable[b] {
                            sets[k].insert((p, dot + 1, origin));
                        }
                    }
                    Some(&Sym::T(t)) => {
                        if k < input.len() && input[k] == t {
                            sets[k + 1].insert((p, dot + 1, origin));
                        }
                    }
                    None => {
                        let lhs = if p == aug { usize::MAX } else { g.prods[p].0 };
                        let parents: Vec<Item> = sets[origin].iter().copied().collect();
                        for (pp, pd, po) in parents {
                            if rhs_of(pp).get(pd) == Some(&Sym::N(lhs)) {
                                sets[k].insert((pp, pd + 1, po));
                            }
                        }
                    }
                }
            }
            if sets[k].len() == before {
                break;
            }
        }
    }
    sets
}

fn earley_accepts(g: &TestGrammar, input: &[usize]) -> bool {
    earley(g, input)[input.len()].contains(&(g.prods.len(), 1, 0))
}

fn earley_viable(g: &TestGrammar, prefix: &[usize]) -> bool {
    !earley(g, prefix)[prefix.len()].is_empty()
}

fn grammar_strategy() -> impl Strategy<Value = TestGrammar> {
    (1usize..=3, 1usize..=4).prop_flat_map(|(terms, nonterms)| {
        let sym = prop_oneof![(0..terms).prop_map(Sym::T), (0..nonterms).prop_map(Sym::N),];
        let prod = proptest::collection::vec(sym, 0..=3);
        // Every nonterminal gets one to three productions.
        proptest::collection::vec(proptest::collection::vec(prod, 1..=3), nonterms).prop_map(
            move |per_nt| TestGrammar {
                terms,
                nonterms,
                prods: per_nt
                    .into_iter()
                    .enumerate()
                    .flat_map(|(lhs, prods)| prods.into_iter().map(move |rhs| (lhs, rhs)))
                    .collect(),
            },
        )
    })
}

/// Every input over `terms` terminals up to `max` long.
fn all_inputs(terms: usize, max: usize) -> Vec<Vec<usize>> {
    let mut out = vec![vec![]];
    let mut frontier = vec![vec![]];
    for _ in 0..max {
        let mut next = vec![];
        for s in &frontier {
            for t in 0..terms {
                let mut s2: Vec<usize> = s.clone();
                s2.push(t);
                next.push(s2);
            }
        }
        out.extend(next.iter().cloned());
        frontier = next;
    }
    out
}

/// Checks every rule node against the grammar's productions, and collects
/// the leaves.
fn check_tree(g: &TestGrammar, node: Node<'_>, leaves: &mut Vec<String>) {
    if node.kind().is_token() {
        leaves.push(node.name().to_string());
        return;
    }
    let children: Vec<&str> = node.children().map(|c| c.name()).collect();
    let matches = g.prods.iter().any(|(lhs, rhs)| {
        NONTERM_NAMES[*lhs] == node.name()
            && rhs.len() == children.len()
            && rhs.iter().zip(&children).all(|(s, c)| match *s {
                Sym::T(t) => TERM_NAMES[t] == *c,
                Sym::N(n) => NONTERM_NAMES[n] == *c,
            })
    });
    assert!(
        matches,
        "node {} -> {children:?} is no production",
        node.name()
    );
    let span = node.span();
    for child in node.children() {
        let cs = child.span();
        assert!(span.start() <= cs.start() && cs.end() <= span.end());
        check_tree(g, child, leaves);
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2000, ..ProptestConfig::default() })]

    #[test]
    fn lalr_matches_canonical_lr1_merged_by_core(g in grammar_strategy()) {
        let reachable = g.reachable();
        let productive = g.productive();
        let unproductive = (0..g.nonterms).any(|n| reachable[n] && !productive[n]);
        let result = g.build();
        if unproductive {
            prop_assert!(matches!(result, Err(GrammarError::Unproductive { .. })), "{result:?}");
            return Ok(());
        }
        if g.has_cycle(&reachable) {
            prop_assert!(matches!(result, Err(GrammarError::Cycle { .. })), "{result:?}");
            return Ok(());
        }
        let conflict = lalr_by_merging_lr1(&g);
        match &result {
            Err(GrammarError::ShiftReduce { .. } | GrammarError::ReduceReduce { .. }) => {
                prop_assert!(conflict, "spurious conflict: {result:?}");
            }
            Ok(parser) => {
                prop_assert!(!conflict, "missed conflict");
                for input in all_inputs(g.terms, 5) {
                    let text: Vec<&str> = input.iter().map(|&t| TERM_NAMES[t]).collect();
                    let text = text.join(" ");
                    let ours = parser.parse(&text);
                    let theirs = earley_accepts(&g, &input);
                    prop_assert_eq!(ours.is_ok(), theirs, "input {:?}: {:?}", text, ours.as_ref().err());
                    if let Err(err) = &ours {
                        // The error sits at the first token that cannot
                        // continue the input, and lists exactly the tokens
                        // that could.
                        let k = err.span().start().to_usize().div_ceil(2);
                        let prefix = &input[..k];
                        prop_assert!(earley_viable(&g, prefix), "{text:?}: error too late at {k}");
                        if k < input.len() {
                            prop_assert!(!earley_viable(&g, &input[..=k]), "{text:?}: error too early at {k}");
                        }
                        let mut viable = vec![];
                        for (t, &name) in TERM_NAMES.iter().enumerate().take(g.terms) {
                            let mut longer = prefix.to_vec();
                            longer.push(t);
                            if earley_viable(&g, &longer) {
                                viable.push(name);
                            }
                        }
                        let listed: Vec<&str> = err.expected().iter().map(|&kind| parser.name(kind)).collect();
                        prop_assert_eq!(&listed, &viable, "{:?}: {}", text, err);
                        let message = err.to_string();
                        let wanted = message.split(", found").next().unwrap_or("");
                        prop_assert_eq!(
                            wanted.contains("end of input"),
                            earley_accepts(&g, prefix),
                            "{:?}: {}", text, err
                        );
                    }
                    if let Ok(tree) = ours {
                        prop_assert_eq!(tree.root().name(), "S");
                        let mut leaves = vec![];
                        check_tree(&g, tree.root(), &mut leaves);
                        let expected: Vec<String> =
                            input.iter().map(|&t| TERM_NAMES[t].to_string()).collect();
                        prop_assert_eq!(leaves, expected);
                    }
                }
            }
            Err(other) => prop_assert!(false, "unexpected error {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Random patterns
// ---------------------------------------------------------------------------

/// A pattern over the alphabet `a`, `b`, `c`, kept as a tree so the reference
/// matcher can interpret it without parsing.
#[derive(Clone, Debug)]
enum Re {
    Char(char),
    Class(Vec<char>, bool),
    Any,
    Concat(Vec<Re>),
    Alt(Vec<Re>),
    Repeat(Box<Re>, u32, Option<u32>),
}

impl Re {
    fn render(&self) -> String {
        match self {
            Re::Char(c) => c.to_string(),
            Re::Class(chars, negated) => {
                let body: String = chars.iter().collect();
                format!("[{}{body}]", if *negated { "^" } else { "" })
            }
            Re::Any => ".".into(),
            Re::Concat(parts) => {
                let mut out = String::new();
                for part in parts {
                    out.push_str("(?:");
                    out.push_str(&part.render());
                    out.push(')');
                }
                out
            }
            Re::Alt(parts) => {
                let inner: Vec<String> = parts.iter().map(Re::render).collect();
                format!("(?:{})", inner.join("|"))
            }
            Re::Repeat(inner, min, max) => {
                let q = match (min, max) {
                    (0, None) => "*".to_string(),
                    (1, None) => "+".to_string(),
                    (0, Some(1)) => "?".to_string(),
                    (n, None) => format!("{{{n},}}"),
                    (n, Some(m)) if n == m => format!("{{{n}}}"),
                    (n, Some(m)) => format!("{{{n},{m}}}"),
                };
                format!("(?:{}){q}", inner.render())
            }
        }
    }

    /// The end positions reachable after matching `self` from each position
    /// in `starts`.
    fn ends(&self, text: &[char], starts: &BTreeSet<usize>) -> BTreeSet<usize> {
        match self {
            Re::Char(c) => starts
                .iter()
                .filter(|&&i| text.get(i) == Some(c))
                .map(|&i| i + 1)
                .collect(),
            Re::Class(chars, negated) => starts
                .iter()
                .filter(|&&i| text.get(i).is_some_and(|c| chars.contains(c) != *negated))
                .map(|&i| i + 1)
                .collect(),
            Re::Any => starts
                .iter()
                .filter(|&&i| text.get(i).is_some_and(|&c| c != '\n'))
                .map(|&i| i + 1)
                .collect(),
            Re::Concat(parts) => parts
                .iter()
                .fold(starts.clone(), |acc, p| p.ends(text, &acc)),
            Re::Alt(parts) => parts.iter().flat_map(|p| p.ends(text, starts)).collect(),
            Re::Repeat(inner, min, max) => {
                let mut current = starts.clone();
                for _ in 0..*min {
                    current = inner.ends(text, &current);
                }
                let mut all = current.clone();
                let mut frontier = current;
                let mut count = *min;
                while !frontier.is_empty() && max.is_none_or(|m| count < m) {
                    let next: BTreeSet<usize> = inner
                        .ends(text, &frontier)
                        .difference(&all)
                        .copied()
                        .collect();
                    all.extend(next.iter().copied());
                    frontier = next;
                    count += 1;
                    if max.is_none() && frontier.is_empty() {
                        break;
                    }
                }
                all
            }
        }
    }

    fn full_match(&self, text: &str) -> bool {
        let chars: Vec<char> = text.chars().collect();
        self.ends(&chars, &BTreeSet::from([0]))
            .contains(&chars.len())
    }
}

fn re_strategy() -> impl Strategy<Value = Re> {
    let leaf = prop_oneof![
        prop::sample::select(vec!['a', 'b', 'c', 'é']).prop_map(Re::Char),
        (
            proptest::collection::vec(prop::sample::select(vec!['a', 'b', 'c', 'é']), 1..3),
            any::<bool>()
        )
            .prop_map(|(c, n)| Re::Class(c, n)),
        Just(Re::Any),
    ];
    leaf.prop_recursive(4, 24, 3, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 2..4).prop_map(Re::Concat),
            proptest::collection::vec(inner.clone(), 2..4).prop_map(Re::Alt),
            (inner, 0u32..3, prop::option::of(0u32..3)).prop_map(|(r, min, extra)| {
                Re::Repeat(Box::new(r), min, extra.map(|e| min + e))
            }),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 1000, ..ProptestConfig::default() })]

    #[test]
    fn patterns_match_like_the_reference(
        re in re_strategy(),
        inputs in proptest::collection::vec("[abcé\\n]{0,6}", 1..12),
    ) {
        let pattern = re.render();
        let built = Grammar::new().pattern("t", &pattern).rule("s", &["t"]).build();
        if re.full_match("") {
            prop_assert!(
                matches!(built, Err(GrammarError::EmptyToken { .. })),
                "{pattern}: {built:?}"
            );
            return Ok(());
        }
        let parser = built.map_err(|e| TestCaseError::fail(format!("{pattern}: {e}")))?;
        for text in inputs {
            // A single-token grammar accepts exactly the strings the pattern
            // matches whole: the longest match covers all of a string that
            // matches.
            prop_assert_eq!(
                parser.parse(&text).is_ok(),
                re.full_match(&text),
                "pattern {} on {:?}", pattern, text
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Robustness
// ---------------------------------------------------------------------------

fn json() -> Parser {
    Grammar::new()
        .literal("{")
        .literal("}")
        .literal("[")
        .literal("]")
        .literal(":")
        .literal(",")
        .literal("true")
        .literal("false")
        .literal("null")
        .pattern(
            "string",
            r#""([^"\\\x00-\x1F]|\\["\\/bfnrt]|\\u[0-9a-fA-F]{4})*""#,
        )
        .pattern("number", r"-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?")
        .skip("space", r"[ \t\n\r]+")
        .rule("value", &["object"])
        .rule("value", &["array"])
        .rule("value", &["string"])
        .rule("value", &["number"])
        .rule("value", &["true"])
        .rule("value", &["false"])
        .rule("value", &["null"])
        .rule("object", &["{", "}"])
        .rule("object", &["{", "members", "}"])
        .rule("members", &["members", ",", "member"])
        .rule("members", &["member"])
        .rule("member", &["string", ":", "value"])
        .rule("array", &["[", "]"])
        .rule("array", &["[", "elements", "]"])
        .rule("elements", &["elements", ",", "value"])
        .rule("elements", &["value"])
        .build()
        .unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2000, ..ProptestConfig::default() })]

    #[test]
    fn arbitrary_input_never_panics(text in r#"[\[\]{}:,"a-z0-9 \n\\.-éλ😀]{0,40}"#) {
        let parser = json();
        match parser.parse(&text) {
            Ok(tree) => {
                prop_assert_eq!(tree.root().name(), "value");
                prop_assert_eq!(tree.root().text(), text.trim_matches([' ', '\n']));
            }
            Err(err) => {
                let span = err.span();
                prop_assert!(span.end().to_usize() <= text.len());
                prop_assert!(text.is_char_boundary(span.start().to_usize()));
                prop_assert!(text.is_char_boundary(span.end().to_usize()));
                prop_assert!(!err.to_string().is_empty());
            }
        }
    }

    #[test]
    fn tokens_tile_the_input(text in r#"[\[\]{}:,"a-z0-9 \n\\.-é]{0,40}"#) {
        let parser = json();
        let mut pos = 0;
        for item in parser.tokens(&text) {
            let span = match item {
                Ok(token) => token.span(),
                Err(err) => err.span(),
            };
            prop_assert_eq!(span.start().to_usize(), pos);
            prop_assert!(span.end().to_usize() > pos);
            pos = span.end().to_usize();
        }
        prop_assert_eq!(pos, text.len());
    }

    #[test]
    fn trivia_never_reaches_the_tree(n in 0usize..30) {
        let parser = json();
        let items: Vec<String> = (0..n).map(|i| format!(" {i} ")).collect();
        let text = format!("[{}]", items.join(","));
        let tree = parser.parse(&text).unwrap();
        let tokens = tree.root().descendants().filter(|n| n.kind().is_token());
        for token in tokens {
            prop_assert!(!token.kind().is_trivia());
            prop_assert!(!token.text().contains(' '));
        }
    }
}
