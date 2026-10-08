# grammar-lang - Roadmap

> Path from scaffold to a stable 1.0. Hard parts are front-loaded; each phase has hard exit criteria.
> Master plan: ../../_strategy/LANG_COLLECTION.md
>
> **Anti-deferral rule:** no listed hard task moves to a later phase unless this file records the move and the reason.

## v0.1.0 - Scaffold (DONE)
Compiles, CI green, structure correct, no domain logic.
- [x] Manifest, README, CHANGELOG, REPS, dual license, CI, deny, clippy, rustfmt.

## v0.2.0 - Core (DONE)
Grammar description plus the parser-generator core - the LexerSketch substrate.
Dependencies (wires token, lexer, ast, parser, diag) are wired here, when first used.
Exit criteria:
- [x] Every public item has rustdoc + a runnable example.
- [x] Core invariants property-tested (full DIRECTIVES + API authored at this stage).

Delivered:
- `Grammar` (by-value builder: `literal`, `pattern`, `skip`, `rule`, `prec`,
  `left`, `right`, `nonassoc`, `build`), `Parser` (`parse`, `parse_with`,
  `tokens`, `kind`, `name`), `Actions` + `Production`, `Tree` + `Node`, `Kind`,
  `Tokens`, `GrammarError`, `ParseError`.
- Lexer generator: regex syntax -> Thompson NFA over UTF-8 bytes -> subset
  construction over byte classes -> Hopcroft minimization -> flat scanning
  table with accept/final flags in the transitions.
- Parser generator: LR(0) automaton -> DeRemer-Pennello LALR(1) lookaheads ->
  dense ACTION/GOTO tables, with conflicts settled exactly as Bison settles
  them (precedence before reduce/reduce judgement, unreachable states dropped)
  and every unresolved conflict reported as an error.
- Exact expected-token sets in errors (replay + per-token simulation on the
  error path only); derivation cycles refused so parsing always terminates.
- Verified against canonical LR(1)-merged-by-core and Earley references
  (property tests), and against GNU Bison 3.8.2 on random grammars with
  precedence (conflict verdicts, acceptance, and reduction sequences).

Dependency wiring (decided here, recorded per the anti-deferral rule):
- **token-lang: wired.** Lexer output and action input are `Token<Kind>`;
  `Kind` implements `TokenKind` (trivia = skipped tokens). `Span` comes from it.
- **diag-lang: wired.** `ParseError::to_diagnostic` produces a `Diagnostic`.
- **lexer-lang: not wired.** Its `Cursor` serves hand-written lexers, one
  character at a time. The generated lexer is a byte-level DFA table; routing
  it through a character cursor would only add a dependency and cost speed.
- **parser-lang: not wired.** It serves hand-written recursive-descent and
  Pratt parsers. A table-driven LALR(1) driver has no use for its `Parser`.
- **ast-lang: not wired.** grammar-lang owns no AST: languages build their own
  through `Actions`. The `ast` example builds an `ast-lang` arena that way,
  with `ast-lang` as a dev-dependency only.
- `serde` feature and `loom` dev-dependency removed: nothing to serialize or
  model-check yet. Serializing built tables is additive 1.x work.

## v1.0.0 - API freeze (DONE)
Public surface stable and frozen until 2.0.
- [x] docs/API.md marked stable; SemVer promise recorded (`docs/API.md#stability`:
  frozen surface, behavioural contract, what is not promised, public
  dependencies, MSRV policy).
- [x] Full test + benchmark suite green on all three platforms (Windows and
  Linux verified locally on stable and 1.85; macOS through the CI matrix).

No code changed at the freeze: the 0.2.0 surface was reviewed and kept as is.

## Additive after 1.0 (not promised, not blocking)
- Error recovery (yacc-style `error` productions) for multi-error parses.
- Serializing a built `Parser` to skip generation at startup.
- Unicode property classes (`\p{L}`) and case-insensitive literals.
- Table compression for very large grammars.
