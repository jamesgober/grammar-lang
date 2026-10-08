<h1 align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br>
    <b>grammar-lang</b>
    <br>
    <sub><sup>GRAMMAR & GENERATOR</sup></sub>
</h1>

<div align="center">
    <a href="https://crates.io/crates/grammar-lang"><img alt="Crates.io" src="https://img.shields.io/crates/v/grammar-lang"></a>
    <a href="https://crates.io/crates/grammar-lang"><img alt="Downloads" src="https://img.shields.io/crates/d/grammar-lang?color=%230099ff"></a>
    <a href="https://docs.rs/grammar-lang"><img alt="docs.rs" src="https://img.shields.io/docsrs/grammar-lang"></a>
    <a href="https://github.com/jamesgober/grammar-lang/actions"><img alt="CI" src="https://github.com/jamesgober/grammar-lang/actions/workflows/ci.yml/badge.svg"></a>
    <a href="https://github.com/rust-lang/rfcs/blob/master/text/2495-min-rust-version.md"><img alt="MSRV" src="https://img.shields.io/badge/MSRV-1.85%2B-blue"></a>
</div>

<br>

<div align="left">
    <p>
        <strong>grammar-lang</strong> is a parser generator that runs at runtime. Describe a language's tokens and rules in plain Rust, call <code>build</code>, and get back a parser: a minimized DFA lexer and an LALR(1) parse table, ready to turn text into a concrete syntax tree or to drive your own semantic actions.
    </p>
    <p>
        There is no code generation step, no build script, and no macro. The grammar is data &mdash; it can come from a configuration file, a language schematic, or a test &mdash; and a malformed grammar is a <code>GrammarError</code> value that names the problem, not a compile failure. Precedence and associativity work the way yacc and Bison define them, so operator grammars are written the natural way; conflicts that precedence does not settle are reported with the competing productions, never resolved by a silent default. It is the parser-generator core of the <code>-lang</code> family's language generator.
    </p>
    <br>
    <hr>
    <p>
        <strong>MSRV is 1.85+</strong> (Rust 2024 edition). <code>no_std</code>-compatible (needs only <code>alloc</code>), <code>#![forbid(unsafe_code)]</code>, two dependencies from the family: <a href="https://crates.io/crates/token-lang"><code>token-lang</code></a> and <a href="https://crates.io/crates/diag-lang"><code>diag-lang</code></a>.
    </p>
    <blockquote>
        <strong>1.0.0 is the API freeze.</strong> The public surface is stable and follows Semantic Versioning &mdash; no breaking changes before <code>2.0</code>. See <a href="./docs/API.md#stability"><code>docs/API.md</code></a> for the frozen surface, the behavioural contract, and the SemVer promise, and <a href="./CHANGELOG.md"><code>CHANGELOG.md</code></a>.
    </blockquote>
</div>

<hr>
<br>

## The model

A handful of types, one per job:

- A **[`Grammar`](./docs/API.md#grammar)** describes the language: tokens (**literals**, regular-expression **patterns**, and **skipped** trivia such as whitespace and comments), **rules** written one production at a time, and **precedence levels**. [`build`](./docs/API.md#grammarbuild) checks it and generates the parser.
- A **[`Parser`](./docs/API.md#parser)** is the generated lexer and parse table. It parses text into a **[`Tree`](./docs/API.md#tree)**, runs your **[`Actions`](./docs/API.md#actions)** instead, or just splits text into **[tokens](./docs/API.md#parsertokens)**. It is immutable and shareable across threads.
- A **[`Kind`](./docs/API.md#kind)** names a token or rule; tokens are `token_lang::Token<Kind>`.
- A **[`GrammarError`](./docs/API.md#grammarerror)** says why a grammar was refused; a **[`ParseError`](./docs/API.md#parseerror)** says where input went wrong and lists exactly what would have been accepted, and converts into a `diag_lang::Diagnostic`.

<br>

What it guarantees, and how each guarantee is checked:

| Guarantee | How it is held |
|---|---|
| A grammar builds exactly when it is LALR(1) once precedence is applied. | Property tests compare the generator with a canonical LR(1) construction merged by core, the textbook definition of LALR(1), over thousands of random grammars. |
| Precedence and associativity mean what they mean in Bison. | Random grammars with random precedence levels and `%prec` overrides are run through both this crate and GNU Bison 3.8.2: the conflict verdicts, the accepted inputs, and the exact sequence of reductions agree. |
| The parser accepts exactly the grammar's language. | Every short input of each random grammar is checked against an Earley recognizer, and every tree against the grammar's productions. |
| Errors point at the first bad token and list exactly what could follow. | The same property tests check the error position and the expected set against the Earley recognizer's viable prefixes. |
| Token patterns match what they say. | Random regular expressions, Unicode included, are checked against a reference matcher. |
| Parsing always terminates, on any input. | Grammars that can derive a rule from itself alone are refused at build time, and fuzzed input never panics. |

<hr>
<br>

## Installation

```toml
[dependencies]
grammar-lang = "1"
```

Without the standard library:

```toml
[dependencies]
grammar-lang = { version = "1", default-features = false }
```

<hr>
<br>

## Quick start

An arithmetic grammar, with the operator precedence declared rather than encoded in the rules:

```rust
use grammar_lang::Grammar;

let parser = Grammar::new()
    .literal("+")
    .literal("-")
    .literal("*")
    .literal("/")
    .literal("(")
    .literal(")")
    .pattern("num", r"[0-9]+(\.[0-9]+)?")
    .skip("space", r"\s+")
    .left(&["+", "-"])
    .left(&["*", "/"])
    .rule("expr", &["expr", "+", "expr"])
    .rule("expr", &["expr", "-", "expr"])
    .rule("expr", &["expr", "*", "expr"])
    .rule("expr", &["expr", "/", "expr"])
    .rule("expr", &["(", "expr", ")"])
    .rule("expr", &["num"])
    .build()?;

let tree = parser.parse("1 + 2 * 3").unwrap();
assert_eq!(
    tree.to_string(),
    r#"(expr (expr (num "1")) "+" (expr (expr (num "2")) "*" (expr (num "3"))))"#,
);

let err = parser.parse("1 + * 3").unwrap_err();
assert_eq!(err.to_string(), "expected `(` or `num`, found `*`");
# Ok::<(), grammar_lang::GrammarError>(())
```

### Semantic actions

`parse_with` hands each token and each completed production to your `Actions`, children before parents, so the result can be anything — here, the value itself:

```rust
use grammar_lang::{Actions, Grammar, Kind, Production, Span, Token};
use std::vec::Drain;

struct Eval;

impl<'s> Actions<'s> for Eval {
    type Value = i64;

    fn token(&mut self, _: Token<Kind>, text: &'s str) -> i64 {
        text.parse().unwrap_or(0)
    }

    fn reduce(&mut self, p: Production, _: Span, children: Drain<'_, i64>) -> i64 {
        let v: Vec<i64> = children.collect();
        match p.index() {
            0 => v[0] + v[2], // e → e + e
            1 => v[0] * v[2], // e → e * e
            2 => v[1],        // e → ( e )
            _ => v[0],        // e → num
        }
    }
}

let parser = Grammar::new()
    .literal("+")
    .literal("*")
    .literal("(")
    .literal(")")
    .pattern("num", "[0-9]+")
    .left(&["+"])
    .left(&["*"])
    .rule("e", &["e", "+", "e"])
    .rule("e", &["e", "*", "e"])
    .rule("e", &["(", "e", ")"])
    .rule("e", &["num"])
    .build()?;

assert_eq!(parser.parse_with("2*(3+4)", &mut Eval), Ok(14));
# Ok::<(), grammar_lang::GrammarError>(())
```

### Mistakes caught at build time

Every problem is a value that names the grammar's own symbols:

```rust
use grammar_lang::{Grammar, GrammarError};

// Ambiguous: does `1 - 2 - 3` group to the left or the right?
let err = Grammar::new()
    .literal("-")
    .pattern("n", "[0-9]+")
    .rule("e", &["e", "-", "e"])
    .rule("e", &["n"])
    .build()
    .unwrap_err();
assert_eq!(
    err.to_string(),
    "shift/reduce conflict on `-`: shift in `e → e • - e`, or reduce `e → e - e`",
);

// A keyword declared after a pattern that matches it still wins: literals
// outrank patterns. Two patterns are a different story.
let err = Grammar::new()
    .pattern("ident", "[a-z]+")
    .pattern("kw_if", "if")
    .rule("s", &["ident"])
    .build()
    .unwrap_err();
assert_eq!(err, GrammarError::ShadowedToken { name: "kw_if".into(), by: "ident".into() });
```

### Diagnostics

A `ParseError` becomes a `diag_lang::Diagnostic`, which the family's renderer prints against the source:

```rust
use diag_lang::{Renderer, SourceMap};
use grammar_lang::Grammar;

let parser = Grammar::new()
    .literal("=")
    .pattern("key", "[a-z]+")
    .pattern("num", "[0-9]+")
    .skip("space", r"\s+")
    .rule("entries", &["entries", "entry"])
    .rule("entries", &["entry"])
    .rule("entry", &["key", "=", "num"])
    .build()?;

let source = "port = 80\nhost = x\n";
let err = parser.parse(source).unwrap_err();
let mut map = SourceMap::new();
map.add("server.conf", source).unwrap();
let report = Renderer::new().render(&err.to_diagnostic(), &map);
assert!(report.contains("expected `num`, found `key` `x`"));
assert!(report.contains("server.conf:2:8"));
# Ok::<(), grammar_lang::GrammarError>(())
```

<hr>
<br>

## Examples

Runnable programs in [`examples/`](./examples):

| Example | What it shows |
|---|---|
| [`calculator`](./examples/calculator.rs) | Evaluating while parsing: precedence, right associativity, unary minus with `prec`. `cargo run --example calculator -- "2 ^ 3 ^ 2"` |
| [`json`](./examples/json.rs) | A complete RFC 8259 JSON parser that builds a typed value, borrowing string contents from the input. |
| [`ast`](./examples/ast.rs) | Building a typed AST straight into an `ast-lang` arena, then walking it with an `ast-lang` visitor. |
| [`diagnostics`](./examples/diagnostics.rs) | Rendering parse errors with `diag-lang` for a small configuration language. |
| [`highlight`](./examples/highlight.rs) | A highlighter from the token stream alone, carrying on past characters no token matches. |

<hr>
<br>

## Performance

Parsing is a single pass. The lexer finds each token by longest match in a minimized DFA laid out as one flat table: bytes map to equivalence classes, row offsets are pre-multiplied, and each transition carries flags for "a token ends here" and "nothing can follow", so the inner loop is one load and one test per byte and single-character tokens stop at once. The parser fetches tokens on demand and takes one table load per shift or reduce. Building a tree is a push per node into flat arrays, and actions that build nothing cost nothing. Work on the error path — replaying to find the exact expected tokens — is never paid by a successful parse.

Measured with the benchmarks in [`benches/`](./benches), x86_64, Rust stable, release profile:

| Benchmark | What it measures | Windows | Linux (WSL2) |
|---|---|---:|---:|
| `build/expression` | Generate the parser for a 7-production expression grammar. | ~21 µs | ~9 µs |
| `build/json` | Generate the JSON parser (12 tokens, 16 productions). | ~76 µs | ~57 µs |
| `build/c_like` | Generate a C-like language: 51 tokens, 78 productions, a 10-level expression grammar (161 states). | ~0.43 ms | ~0.33 ms |
| `json_1mib/tokens` | Lex 1 MiB of JSON, whitespace included. | ~690 MiB/s | ~895 MiB/s |
| `json_1mib/parse_with_noop` | Lex and parse 1 MiB of JSON with actions that build nothing. | ~250 MiB/s | ~300 MiB/s |
| `json_1mib/parse_tree` | Lex, parse, and build the full concrete syntax tree. | ~114 MiB/s | ~216 MiB/s |
| `expression/evaluate` | Evaluate a 324 KB arithmetic expression in actions. | ~132 MiB/s | ~156 MiB/s |

Generating even the C-like grammar's parser takes a third of a millisecond, so building grammars at startup — or from data at runtime — costs next to nothing. Tree building is the most allocation-heavy path, which is where the platforms differ most.

```bash
cargo bench --bench bench
```

Criterion writes per-benchmark reports to `target/criterion/`. Numbers vary by CPU; use the trend across runs, not a single absolute.

<hr>
<br>

## Design notes

- **Bison's semantics, stricter defaults.** A production takes the precedence of its last token unless `prec` names another; precedence settles shift/reduce conflicts before reduce/reduce conflicts are judged; and states that only a disabled shift could reach are dropped, conflicts and all. That is exactly what Bison does, which the differential tests confirm. Where Bison then picks a default — shift, or the earlier rule — this crate refuses the grammar and says why.
- **Exact errors from an LALR table.** LALR(1) merges lookaheads, so its tables can report tokens that could never follow, and may reduce before noticing an error. On an error, the parser replays the input to the stack it had when the bad token arrived and asks, token by token, whether each would be shifted. The expected set is then exact, and the cost falls only on input that fails.
- **Termination is a build-time property.** A rule that can derive itself alone makes some input infinitely ambiguous; with a precedence hiding the conflict, a table-driven parser would reduce around the cycle forever. Such grammars are refused, so every parse ends.
- **Tokens are bytes, characters are Unicode.** Patterns are compiled to UTF-8 byte automata, so `[α-ω]` and `.` match whole characters while the scanner never decodes. Every match ends on a character boundary.
- **No recursion anywhere input reaches.** Pattern nesting is bounded, the digraph traversal behind the lookahead sets is iterative, and tree building, display, walking, and dropping are flat. A 100,000-deep nesting parses, prints, and drops without touching the stack limit.

<hr>
<br>

## Testing

The suite runs on Windows, Linux (WSL2 Ubuntu), and macOS through the CI matrix, on stable and the 1.85 MSRV:

```bash
cargo test                       # unit + integration + property + doctests
cargo test --no-default-features # no_std + alloc
cargo clippy --all-targets --all-features -- -D warnings
cargo bench --bench bench
```

The property tests in [`tests/properties.rs`](./tests/properties.rs) hold the generator to independent references written in the test: a canonical LR(1) construction merged by core decides which random grammars are LALR(1); an Earley recognizer decides which inputs each accepts, where the first error must be, and which tokens could follow it; a direct interpreter decides what each random pattern matches. Further properties fuzz the parser with arbitrary text and check that the token stream tiles the input exactly. Every `rust` example in this README and in [`docs/API.md`](./docs/API.md) is compiled and run as a doctest.

<hr>
<br>

## Cross-platform support

- Linux (x86_64, aarch64)
- macOS (x86_64, Apple Silicon)
- Windows (x86_64)

The crate uses no operating-system facilities and no platform-specific code; a grammar builds the same tables, and parses the same way, on every platform.

<hr>
<br>

## Contributing

See [`REPS.md`](./REPS.md) for the engineering standards every change is held to, and [`dev/ROADMAP.md`](./dev/ROADMAP.md) for the roadmap and the additive work planned for `1.x`. Before a PR: `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-features` must be clean.

<br>

<div id="license">
    <h2>License</h2>
    <p>Licensed under either of</p>
    <ul>
        <li><b>Apache License, Version 2.0</b> &mdash; <a href="./LICENSE-APACHE">LICENSE-APACHE</a></li>
        <li><b>MIT License</b> &mdash; <a href="./LICENSE-MIT">LICENSE-MIT</a></li>
    </ul>
    <p>at your option.</p>
</div>

<div align="center">
  <h2></h2>
  <sup>COPYRIGHT <small>&copy;</small> 2026 <strong>James Gober <me@jamesgober.com>.</strong></sup>
</div>
