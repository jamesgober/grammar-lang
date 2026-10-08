<h1 align="center">
    <img width="90px" height="auto" src="https://raw.githubusercontent.com/jamesgober/jamesgober/main/media/icons/hexagon-3.svg" alt="Triple Hexagon">
    <br><b>CHANGELOG</b>
</h1>
<p>
  All notable changes to <code>grammar-lang</code> will be documented in this file. The format is based on <a href="https://keepachangelog.com/en/1.1.0/">Keep a Changelog</a>,
  and this project adheres to <a href="https://semver.org/spec/v2.0.0.html/">Semantic Versioning</a>.
</p>

---

## [Unreleased]

---

## [1.0.0] - 2026-10-07

The API freeze. The 0.2.0 surface is now the stable `1.x` contract; the code
is unchanged.

### Added

- `docs/API.md#stability`: the frozen surface, the behavioural contract
  (lexer tie-breaking, Bison-compatible conflict resolution, production
  numbering, tree shape and text form, error position and expected sets,
  termination), what is not promised, `token-lang` 1 and `diag-lang` 1 as
  public dependencies, and the MSRV policy, recorded as the SemVer promise.
- A Stability section in the crate documentation.

### Changed

- Version 1.0.0. `README.md` and `docs/API.md` mark the API stable.

---

## [0.2.0] - 2026-10-07

The core: a parser generator that runs at runtime. A grammar of tokens, rules,
and precedence builds a minimized DFA lexer and an LALR(1) parser, which parses
text into a concrete syntax tree or drives the caller's semantic actions.

### Added

- `Grammar`: a by-value builder — `literal`, `pattern`, `skip`, `rule`,
  `prec`, `left`, `right`, `nonassoc` — and `build`, which checks the grammar
  and returns a `Parser` or the first problem as a `GrammarError`.
- Lexer generation: token patterns in the syntax of Rust's `regex` crate
  (literals, escapes, `.`, classes, ASCII `\d` `\w` `\s`, groups, alternation,
  `*` `+` `?` `{n,m}`), compiled by Thompson's construction into one NFA over
  UTF-8 bytes, determinized over byte equivalence classes, minimized with
  Hopcroft's algorithm, and laid out as a flat scanning table whose
  transitions carry "accepting" and "final" flags. Longest match wins; on a
  tie a literal beats a pattern, then the earlier declaration.
- Parser generation: the LR(0) automaton, LALR(1) lookaheads by DeRemer and
  Pennello's relations, and dense ACTION/GOTO tables. Conflicts are settled
  exactly as GNU Bison settles them — a production's precedence is its last
  token's unless `prec` names another, precedence is applied before
  reduce/reduce conflicts are judged, and states only a disabled shift reaches
  are dropped — and every conflict precedence leaves standing is an error
  naming the token and productions.
- `Parser`: `parse` into a `Tree`, `parse_with` driving `Actions`, `tokens`
  for the lexer alone, and `kind`/`name` to resolve names. Immutable, `Send`,
  `Sync`, and `Clone`.
- `Actions` and `Production`: semantic actions in postorder, with children
  moved out of the parser's stack and token text borrowed from the input.
- `Tree` and `Node`: a flat concrete syntax tree with spans, text, children,
  preorder descendants, and S-expression `Display` (`{:#}` indented).
- `Kind`: the token or rule a token or node is; implements
  `token_lang::TokenKind`, with skipped tokens as trivia.
- `Tokens`: the lexer as an iterator, which reports unmatched characters and
  carries on.
- `GrammarError` (`#[non_exhaustive]`, sixteen variants) and `ParseError`, whose
  expected-token set is exact: on an error the parser replays to the stack
  it had when the bad token arrived and checks every token, so LALR(1)
  lookahead merging never lists a token that cannot follow.
  `ParseError::to_diagnostic` produces a `diag_lang::Diagnostic`.
- Grammars in which a rule can derive itself alone are refused
  (`GrammarError::Cycle`), so every parse terminates even when a precedence
  would hide the cycle's conflict.
- Re-exports of `Span`, `Token`, and `TokenKind` from `token-lang`.
- Examples `calculator`, `json`, `ast` (an `ast-lang` arena AST),
  `diagnostics`, and `highlight`.
- Unit, integration, and property tests — the generator against a canonical
  LR(1) construction merged by core, the parser and its errors against an
  Earley recognizer, patterns against a reference matcher — and Criterion
  benchmarks for generation, lexing, and parsing. `README.md` and
  `docs/API.md` examples run as doctests.

### Changed

- Wired `token-lang` 1 and `diag-lang` 1. `lexer-lang`, `parser-lang`, and
  `ast-lang` are deliberately not wired; `dev/ROADMAP.md` records why.
- `Cargo.toml` description and keywords describe the crate.
- Removed the scaffold's `serde` feature and `loom` dev-dependency, which had
  no code behind them.
- CI also runs clippy and tests without default features and builds the
  examples.

### Fixed

- `Cargo.toml` listed `keywords` and `categories` unquoted, so the manifest
  did not parse.
- `clippy.toml` declared MSRV 1.87 against the crate's 1.85.
- `deny.toml` named another project in its header.
- `dev/ROADMAP.md` and `docs/API.md` carried byte-order marks.
- The README linked a `dev/DIRECTIVES.md` that does not exist.

---

## [0.1.0] - 2026-06-18

Initial scaffold and repository bootstrap. No domain logic yet &mdash; this release establishes the structure, tooling, and quality gates the implementation will be built on.

### Added

- `Cargo.toml` with crate metadata, Rust 2024 edition, MSRV 1.85.
- Dual `Apache-2.0 OR MIT` license files.
- `README.md`, `CHANGELOG.md`, and a documentation skeleton.
- `REPS.md` compliance baseline.
- `.github/workflows/ci.yml` CI matrix; `deny.toml`, `clippy.toml`, `rustfmt.toml`.
- `dev/DIRECTIVES.md` and `dev/ROADMAP.md` (committed engineering standards + plan).

[Unreleased]: https://github.com/jamesgober/grammar-lang/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/jamesgober/grammar-lang/compare/v0.2.0...v1.0.0
[0.2.0]: https://github.com/jamesgober/grammar-lang/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/grammar-lang/releases/tag/v0.1.0
