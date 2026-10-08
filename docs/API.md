# grammar-lang &mdash; API Reference

> Complete reference for every public item in `grammar-lang`, with examples.
> **Status: stable (1.0).** The surface below is the `1.0` contract; it follows
> [Semantic Versioning](#stability) and will not change in a breaking way before
> `2.0`. See [`../dev/ROADMAP.md`](../dev/ROADMAP.md).

<sub>Copyright &copy; 2026 <strong>James Gober</strong>.</sub>

## Table of contents

- [Overview](#overview)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Concepts](#concepts)
  - [Tokens and the lexer](#tokens-and-the-lexer)
  - [Rules and productions](#rules-and-productions)
  - [Precedence and associativity](#precedence-and-associativity)
  - [How conflicts are settled](#how-conflicts-are-settled)
  - [What `build` checks](#what-build-checks)
  - [Parse errors and expected tokens](#parse-errors-and-expected-tokens)
- [Pattern syntax](#pattern-syntax)
- [`Grammar`](#grammar)
  - [`Grammar::new`](#grammarnew)
  - [`Grammar::literal`](#grammarliteral)
  - [`Grammar::pattern`](#grammarpattern)
  - [`Grammar::skip`](#grammarskip)
  - [`Grammar::rule`](#grammarrule)
  - [`Grammar::prec`](#grammarprec)
  - [`Grammar::left`](#grammarleft)
  - [`Grammar::right`](#grammarright)
  - [`Grammar::nonassoc`](#grammarnonassoc)
  - [`Grammar::build`](#grammarbuild)
- [`Parser`](#parser)
  - [`Parser::parse`](#parserparse)
  - [`Parser::parse_with`](#parserparse_with)
  - [`Parser::tokens`](#parsertokens)
  - [`Parser::kind`](#parserkind)
  - [`Parser::name`](#parsername)
- [`Actions`](#actions)
- [`Production`](#production)
- [`Tree`](#tree)
- [`Node`](#node)
- [`Kind`](#kind)
- [`Tokens`](#tokens)
- [`GrammarError`](#grammarerror)
- [`ParseError`](#parseerror)
- [Re-exports](#re-exports)
- [Feature flags](#feature-flags)
- [Limits](#limits)
- [Guide: an expression language](#guide-an-expression-language)
- [Guide: porting a yacc or Bison grammar](#guide-porting-a-yacc-or-bison-grammar)
- [Stability](#stability)

## Overview

`grammar-lang` generates parsers at runtime. A [`Grammar`](#grammar) describes
the tokens and rules of a language; [`build`](#grammarbuild) compiles the
tokens into one minimized DFA and the rules into an LALR(1) parse table, and
returns a [`Parser`](#parser). The parser turns text into a concrete syntax
[`Tree`](#tree), or drives your own [`Actions`](#actions) to build an AST or
compute a value directly.

| Item | Kind | Purpose |
|---|---|---|
| [`Grammar`](#grammar) | struct | Describes tokens, rules, and precedence; builds a `Parser`. |
| [`Parser`](#parser) | struct | The generated lexer and tables; parses, tokenizes, resolves names. |
| [`Actions`](#actions) | trait | Semantic actions run by `Parser::parse_with`. |
| [`Production`](#production) | struct | Which production completed, as reported to `Actions::reduce`. |
| [`Tree`](#tree) | struct | A concrete syntax tree from `Parser::parse`. |
| [`Node`](#node) | struct | A node of a `Tree`. |
| [`Kind`](#kind) | struct | The token or rule a token or node is. |
| [`Tokens`](#tokens) | struct | The iterator returned by `Parser::tokens`. |
| [`GrammarError`](#grammarerror) | enum | Why a grammar was refused. |
| [`ParseError`](#parseerror) | struct | Why input was rejected, where, and what was expected. |
| [`Span`, `Token`, `TokenKind`](#re-exports) | re-exports | From `token-lang`. |

## Installation

```toml
[dependencies]
grammar-lang = "1"
```

For `no_std` targets (the crate needs only `alloc`):

```toml
[dependencies]
grammar-lang = { version = "1", default-features = false }
```

## Quick start

```rust
use grammar_lang::Grammar;

let parser = Grammar::new()
    .literal("[")
    .literal("]")
    .literal(",")
    .pattern("num", "-?[0-9]+")
    .skip("space", r"\s+")
    .rule("list", &["[", "]"])
    .rule("list", &["[", "items", "]"])
    .rule("items", &["items", ",", "num"])
    .rule("items", &["num"])
    .build()?;

let tree = parser.parse("[1, -2, 3]").unwrap();
assert_eq!(tree.root().name(), "list");

let num = parser.kind("num").unwrap();
let values: Vec<&str> = tree
    .root()
    .descendants()
    .filter(|node| node.kind() == num)
    .map(|node| node.text())
    .collect();
assert_eq!(values, ["1", "-2", "3"]);

let err = parser.parse("[1, 2,]").unwrap_err();
assert_eq!(err.to_string(), "expected `num`, found `]`");
# Ok::<(), grammar_lang::GrammarError>(())
```

## Concepts

### Tokens and the lexer

A grammar declares three kinds of token:

| Declared with | Matches | Named by | Seen by the parser |
|---|---|---|---|
| [`literal`](#grammarliteral) | its text, exactly | its text | yes |
| [`pattern`](#grammarpattern) | a regular expression ([syntax](#pattern-syntax)) | the name given | yes |
| [`skip`](#grammarskip) | a regular expression | the name given | no — passed over |

The lexer always takes the **longest match** at the current position. When
two tokens match the same longest text, a **literal beats a pattern**, and
otherwise the token **declared first** wins. That makes keywords work with no
special handling: with `literal("if")` and `pattern("ident", "[a-z]+")`, the
text `if` is the keyword and `iffy` is an identifier, whichever was declared
first.

Skipped tokens can appear between any two tokens and never reach the parser
or the tree; [`Parser::tokens`](#parsertokens) still reports them, with
[`is_trivia`](#kind) set.

```rust
use grammar_lang::{Grammar, TokenKind};

let parser = Grammar::new()
    .pattern("ident", "[a-z]+")
    .literal("if")
    .skip("space", " +")
    .rule("words", &["words", "word"])
    .rule("words", &["word"])
    .rule("word", &["if"])
    .rule("word", &["ident"])
    .build()?;

let names: Vec<&str> = parser
    .tokens("if iffy")
    .filter_map(Result::ok)
    .map(|token| parser.name(*token.kind()))
    .collect();
assert_eq!(names, ["if", "space", "ident"]);
# Ok::<(), grammar_lang::GrammarError>(())
```

### Rules and productions

Each [`rule`](#grammarrule) call adds one **production** — one alternative —
to a rule: a sequence of token and rule names. Calling `rule` again with the
same name adds another alternative. An empty sequence matches nothing, which
is how optional parts are written. The **first rule declared is the start
rule**, which must match the whole input.

Productions are numbered from 0 in the order they are added, across every
rule; [`Production::index`](#production) reports that number to
[`Actions`](#actions), which is how actions tell alternatives apart.

Left recursion (`list → list item`) is the natural form for an LR parser: it
parses lists of any length in constant stack. Right recursion works too, but
holds the whole list on the parser's (heap-allocated) stack until its end.

```rust
use grammar_lang::Grammar;

// args → ε | list;  list → list "," x | x
let parser = Grammar::new()
    .literal(",")
    .literal("x")
    .rule("args", &[])               // production 0
    .rule("args", &["list"])         // production 1
    .rule("list", &["list", ",", "x"]) // production 2
    .rule("list", &["x"])            // production 3
    .build()?;

assert_eq!(parser.parse("").unwrap().to_string(), "(args)");
assert_eq!(
    parser.parse("x,x").unwrap().to_string(),
    r#"(args (list (list "x") "," "x"))"#,
);
# Ok::<(), grammar_lang::GrammarError>(())
```

### Precedence and associativity

Operator grammars are ambiguous as written — `1 - 2 - 3` could group either
way, and `1 + 2 * 3` could add first. Rather than encoding priority into a
tower of rules, declare it:

- [`left`](#grammarleft), [`right`](#grammarright), and
  [`nonassoc`](#grammarnonassoc) each declare one **precedence level**, binding
  tighter than every level declared before it, and list the tokens on it.
- A production takes the precedence of its **last token** — none, if that
  token has none — unless [`prec`](#grammarprec) names another. A name used
  only with `prec` (like `NEG` below) is a marker: it needs no token.
- When the parser could either shift a token or reduce a production, and both
  have a precedence, the higher one wins. On a tie, the level's associativity
  decides: **left** reduces (`(a - b) - c`), **right** shifts
  (`a ^ (b ^ c)`), and **non-associative** makes the input an error
  (`a < b < c`).

```rust
use grammar_lang::Grammar;

let parser = Grammar::new()
    .literal("-")
    .literal("*")
    .literal("^")
    .pattern("n", "[0-9]")
    .left(&["-"])
    .left(&["*"])
    .right(&["NEG"])
    .right(&["^"])
    .rule("e", &["e", "-", "e"])
    .rule("e", &["e", "*", "e"])
    .rule("e", &["e", "^", "e"])
    .rule("e", &["-", "e"])
    .prec("NEG")
    .rule("e", &["n"])
    .build()?;

// `-` is left-associative, `^` right, and unary minus binds tighter than `*`
// but looser than `^`.
assert_eq!(
    parser.parse("1-2-3").unwrap().to_string(),
    r#"(e (e (e (n "1")) "-" (e (n "2"))) "-" (e (n "3")))"#,
);
assert_eq!(
    parser.parse("-2^2").unwrap().to_string(),
    r#"(e "-" (e (e (n "2")) "^" (e (n "2"))))"#,
);
# Ok::<(), grammar_lang::GrammarError>(())
```

### How conflicts are settled

[`build`](#grammarbuild) settles each parser state exactly the way GNU Bison
does, so a grammar written for yacc or Bison means the same thing here:

1. In production order, every reduction whose production has a precedence is
   weighed against each shift of a token that has one, by the rules above. A
   shift that loses is disabled; a reduction that loses gives up that
   lookahead; a non-associative tie does both and makes the token an error.
2. Any lookahead still claimed by a shift and a reduction (**shift/reduce**),
   or by two reductions (**reduce/reduce**), is a conflict.
3. States that only a disabled shift could reach are dropped, conflicts and
   all: a state the parser can never enter cannot make it ambiguous.

Where Bison would then resolve a remaining conflict by default — shift, or
the earlier rule — `build` returns
[`GrammarError::ShiftReduce`](#grammarerror) or
[`GrammarError::ReduceReduce`](#grammarerror) instead, naming the token and
the productions involved. This agreement is tested: random grammars with
random precedence declarations are run through both this crate and Bison
3.8.2, and the conflict verdicts, accepted inputs, and reduction sequences
match.

```rust
use grammar_lang::{Grammar, GrammarError};

// The dangling else: which `if` does `else` belong to?
let grammar = || {
    Grammar::new()
        .literal("if")
        .literal("then")
        .literal("else")
        .literal("x")
        .skip("space", " +")
};
let rules = |g: Grammar| {
    g.rule("stmt", &["if", "x", "then", "stmt"])
        .rule("stmt", &["if", "x", "then", "stmt", "else", "stmt"])
        .rule("stmt", &["x"])
};

let err = rules(grammar()).build().unwrap_err();
assert!(matches!(err, GrammarError::ShiftReduce { ref token, .. } if &**token == "else"));

// Give `else` a higher precedence than the `then` that ends the short form,
// and it attaches to the nearest `if`, as in C.
let parser = rules(grammar().nonassoc(&["then"]).nonassoc(&["else"])).build()?;
assert_eq!(
    parser.parse("if x then if x then x else x").unwrap().to_string(),
    r#"(stmt "if" "x" "then" (stmt "if" "x" "then" (stmt "x") "else" (stmt "x")))"#,
);
# Ok::<(), GrammarError>(())
```

### What `build` checks

`build` reports the **first** problem it finds, checking in this order:

| Stage | Checks | Errors |
|---|---|---|
| Declarations | names are unique; there is a rule; `prec` follows a rule; precedence names are declared once and are not rules; every symbol a production uses is a declared token or rule, and not a skipped token; every `prec` name is in some level. | `Duplicate`, `NoRules`, `MisplacedPrec`, `DuplicatePrecedence`, `PrecedenceOnRule`, `Undefined`, `SkipInRule`, `UndefinedPrecedence` |
| Tokens | every pattern parses; no token matches the empty string; every token can win some match. | `InvalidPattern`, `EmptyToken`, `ShadowedToken`, `TooLarge` |
| Rules | every rule the start rule reaches can derive some input, and none can derive itself alone. Unreachable rules are left out of the table. | `Unproductive`, `Cycle` |
| Table | the grammar is LALR(1) once precedence is applied. | `ShiftReduce`, `ReduceReduce`, `TooLarge` |

### Parse errors and expected tokens

Parsing stops at the first error. A [`ParseError`](#parseerror) records where
it happened, what was found there, and **exactly** the tokens that could have
come next: an LALR(1) table on its own may list tokens that cannot actually
follow, so on an error the parser replays the input to the stack it had when
the bad token arrived and asks of every token whether it would be shifted.
The work is done only for input that fails.

The error is always at the **first** token that cannot continue the input: an
LR parser never shifts a token that leads nowhere.

```rust
use grammar_lang::Grammar;

let parser = Grammar::new()
    .literal("(")
    .literal(")")
    .literal("+")
    .pattern("n", "[0-9]+")
    .left(&["+"])
    .rule("e", &["e", "+", "e"])
    .rule("e", &["(", "e", ")"])
    .rule("e", &["n"])
    .build()?;

// A `)` can follow `1` inside parentheses but not at top level, so it is
// neither accepted nor listed here.
assert_eq!(parser.parse("1)").unwrap_err().to_string(), "expected `+` or end of input, found `)`");
assert_eq!(parser.parse("(1").unwrap_err().to_string(), "expected `)` or `+`, found end of input");
# Ok::<(), grammar_lang::GrammarError>(())
```

## Pattern syntax

[`pattern`](#grammarpattern) and [`skip`](#grammarskip) take regular
expressions in the syntax of Rust's `regex` crate, minus what has no meaning
for a token that always takes the longest match.

| Syntax | Matches |
|---|---|
| `a`, `é`, `😀` | that character |
| `\n` `\r` `\t` `\f` `\v` | newline, carriage return, tab, form feed, vertical tab |
| `\x7F`, `\x{1F600}`, `é`, `\u{E9}` | the character with that hex code point |
| `\.` `\*` `\\` `\[` ... | any escaped ASCII punctuation (or space), literally |
| `.` | any character except newline |
| `[abc]`, `[a-z0-9_]`, `[^"\\]` | a character class; `^` first negates it; `]` first or `-` first/last is literal |
| `\d` `\w` `\s` | ASCII digit, word character (`[0-9A-Za-z_]`), whitespace (`[\t\n\v\f\r ]`) |
| `\D` `\W` `\S` | their complements (over all of Unicode) |
| `xy` | `x` then `y` |
| `x\|y` | `x` or `y` |
| `(x)`, `(?:x)` | grouping |
| `x*` `x+` `x?` | zero or more, one or more, zero or one |
| `x{n}` `x{n,}` `x{n,m}` | exactly `n`, at least `n`, between `n` and `m` (counts up to 1000) |

Matching is by Unicode scalar value: classes and `.` match whole characters,
and a match always ends on a character boundary. Rejected, with an
[`InvalidPattern`](#grammarerror) error that gives the byte offset:
anchors (`^`, `$`), lazy or repeated repetition (`x+?`, `x**`), flags and
other group forms (`(?i)`), nested classes, back-references, unknown escapes,
classes that match nothing, and groups nested more than 64 deep.

```rust
use grammar_lang::{Grammar, GrammarError};

let parser = Grammar::new()
    .pattern("string", r#""([^"\\]|\\.)*""#)
    .pattern("float", r"[0-9]+\.[0-9]+([eE][+-]?[0-9]+)?")
    .pattern("hex", "0x[0-9a-fA-F]{1,16}")
    .pattern("greek", "[α-ω]+")
    .skip("space", r"\s+")
    .rule("items", &["items", "item"])
    .rule("items", &["item"])
    .rule("item", &["string"])
    .rule("item", &["float"])
    .rule("item", &["hex"])
    .rule("item", &["greek"])
    .build()?;
assert!(parser.parse(r#" "a \"quoted\" word" 1.5e-3 0xFF λόγ "#).is_err()); // ό is past ω
assert!(parser.parse(r#" "a \"quoted\" word" 1.5e-3 0xFF λογ "#).is_ok());

let err = Grammar::new().pattern("bad", "^abc").rule("s", &["bad"]).build().unwrap_err();
assert!(matches!(err, GrammarError::InvalidPattern { offset: 0, .. }));
# Ok::<(), GrammarError>(())
```

## `Grammar`

```rust,ignore
#[derive(Clone, Debug, Default)]
pub struct Grammar { /* private */ }
```

The description of a language. Every method but [`build`](#grammarbuild)
takes and returns the grammar by value, so a grammar reads as one chain.
Nothing is checked while describing; every problem surfaces from `build`.
`Grammar` is `Clone`, so a common base can be extended into variants.

### `Grammar::new`

```rust,ignore
pub fn new() -> Grammar
```

An empty grammar. Equivalent to `Grammar::default()`.

```rust
use grammar_lang::{Grammar, GrammarError};

let grammar = Grammar::new();
assert_eq!(grammar.build().unwrap_err(), GrammarError::NoRules);
```

```rust
use grammar_lang::Grammar;

// A shared base, extended two ways.
let base = Grammar::new().pattern("n", "[0-9]+").skip("space", " +");
let one = base.clone().rule("s", &["n"]).build()?;
let pair = base.rule("s", &["n", "n"]).build()?;
assert!(one.parse("7").is_ok() && pair.parse("7").is_err());
assert!(pair.parse("7 8").is_ok());
# Ok::<(), grammar_lang::GrammarError>(())
```

### `Grammar::literal`

```rust,ignore
pub fn literal(self, text: &str) -> Grammar
```

Declares a token that matches `text` exactly. The token is **named by its
text**, so rules refer to it the same way: after `literal("while")`, a rule
uses `"while"`. A literal beats any pattern that matches the same characters.

| Parameter | Meaning |
|---|---|
| `text` | The exact characters to match, and the token's name. Must not be empty ([`EmptyToken`](#grammarerror)). |

```rust
use grammar_lang::Grammar;

let parser = Grammar::new()
    .literal("let")
    .literal("=")
    .literal(";")
    .pattern("ident", "[a-z]+")
    .pattern("num", "[0-9]+")
    .skip("space", " +")
    .rule("stmt", &["let", "ident", "=", "num", ";"])
    .build()?;

assert!(parser.parse("let x = 1;").is_ok());
// `let` is a keyword, so it is not an identifier.
assert!(parser.parse("let let = 1;").is_err());
# Ok::<(), grammar_lang::GrammarError>(())
```

```rust
use grammar_lang::Grammar;

// Multi-character operators and their prefixes coexist: longest match wins.
let parser = Grammar::new()
    .literal("=")
    .literal("==")
    .literal("=>")
    .rule("ops", &["ops", "op"])
    .rule("ops", &["op"])
    .rule("op", &["="])
    .rule("op", &["=="])
    .rule("op", &["=>"])
    .build()?;
let names: Vec<&str> = parser
    .tokens("===>")
    .filter_map(Result::ok)
    .map(|t| parser.name(*t.kind()))
    .collect();
assert_eq!(names, ["==", "=>"]);
# Ok::<(), grammar_lang::GrammarError>(())
```

### `Grammar::pattern`

```rust,ignore
pub fn pattern(self, name: &str, regex: &str) -> Grammar
```

Declares a token named `name` that matches the regular expression `regex`;
see [Pattern syntax](#pattern-syntax). Between patterns that match the same
longest text, the one declared first wins; a pattern that can never win any
match is refused as [`ShadowedToken`](#grammarerror).

| Parameter | Meaning |
|---|---|
| `name` | How rules and messages refer to the token. |
| `regex` | The pattern. Must parse, and must not match the empty string. |

```rust
use grammar_lang::Grammar;

let parser = Grammar::new()
    .pattern("number", r"-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?")
    .rule("value", &["number"])
    .build()?;
for ok in ["0", "-12", "3.25", "6.02e23", "1E-9"] {
    assert!(parser.parse(ok).is_ok(), "{ok}");
}
for bad in ["01", "1.", ".5", "--1"] {
    assert!(parser.parse(bad).is_err(), "{bad}");
}
# Ok::<(), grammar_lang::GrammarError>(())
```

```rust
use grammar_lang::{Grammar, GrammarError};

let err = Grammar::new().pattern("spaces", " *").rule("s", &["spaces"]).build().unwrap_err();
assert_eq!(err, GrammarError::EmptyToken { name: "spaces".into() });
```

### `Grammar::skip`

```rust,ignore
pub fn skip(self, name: &str, regex: &str) -> Grammar
```

Declares a token the lexer matches and the parser passes over — whitespace,
comments, anything that may appear between tokens. Rules cannot name skipped
tokens ([`SkipInRule`](#grammarerror)). Their [`Kind`](#kind) reports
[`is_trivia`](#kind), and [`Parser::tokens`](#parsertokens) includes them.

| Parameter | Meaning |
|---|---|
| `name` | The token's name. |
| `regex` | The pattern, in [pattern syntax](#pattern-syntax). |

```rust
use grammar_lang::Grammar;

let parser = Grammar::new()
    .pattern("word", "[a-z]+")
    .skip("space", r"\s+")
    .skip("line_comment", "#[^\n]*")
    .skip("block_comment", r"/\*([^*]|\*+[^*/])*\*+/")
    .rule("words", &["words", "word"])
    .rule("words", &["word"])
    .build()?;

let tree = parser.parse("alpha # note\n /* a\n block */ beta").unwrap();
assert_eq!(tree.to_string(), r#"(words (words (word "alpha")) (word "beta"))"#);
# Ok::<(), grammar_lang::GrammarError>(())
```

### `Grammar::rule`

```rust,ignore
pub fn rule(self, name: &str, symbols: &[&str]) -> Grammar
```

Adds a production to rule `name`. Call once per alternative; an empty
`symbols` makes the alternative match nothing. The first rule named is the
start rule. Productions are numbered from 0, in the order added, across all
rules.

| Parameter | Meaning |
|---|---|
| `name` | The rule. May not share a token's name ([`Duplicate`](#grammarerror)). |
| `symbols` | Token and rule names, in order. Each must be declared somewhere in the grammar, before or after ([`Undefined`](#grammarerror)). |

```rust
use grammar_lang::Grammar;

// key/value pairs separated by commas, with an optional trailing comma.
let parser = Grammar::new()
    .literal("{")
    .literal("}")
    .literal(",")
    .literal(":")
    .pattern("id", "[a-z]+")
    .skip("space", " +")
    .rule("map", &["{", "pairs", "trailing", "}"])
    .rule("map", &["{", "}"])
    .rule("pairs", &["pairs", ",", "pair"])
    .rule("pairs", &["pair"])
    .rule("pair", &["id", ":", "id"])
    .rule("trailing", &[","])
    .rule("trailing", &[])
    .build()?;

for ok in ["{}", "{a: b}", "{a: b, c: d}", "{a: b, c: d,}"] {
    assert!(parser.parse(ok).is_ok(), "{ok}");
}
assert!(parser.parse("{,}").is_err());
# Ok::<(), grammar_lang::GrammarError>(())
```

```rust
use grammar_lang::{Grammar, GrammarError};

let err = Grammar::new().literal("a").rule("s", &["a", "b"]).build().unwrap_err();
assert_eq!(err, GrammarError::Undefined { name: "b".into(), rule: "s".into() });
```

### `Grammar::prec`

```rust,ignore
pub fn prec(self, name: &str) -> Grammar
```

Gives the most recently added production the precedence of `name` instead of
that of its last token. The classic use is unary minus, which shares its token
with subtraction but binds tighter.

| Parameter | Meaning |
|---|---|
| `name` | A token or marker name listed by [`left`](#grammarleft), [`right`](#grammarright), or [`nonassoc`](#grammarnonassoc) ([`UndefinedPrecedence`](#grammarerror) otherwise). Calling `prec` before any `rule` is [`MisplacedPrec`](#grammarerror). |

```rust
use grammar_lang::Grammar;

let parser = Grammar::new()
    .literal("-")
    .literal("*")
    .pattern("n", "[0-9]+")
    .left(&["-"])
    .left(&["*"])
    .right(&["UMINUS"])
    .rule("e", &["e", "-", "e"])
    .rule("e", &["e", "*", "e"])
    .rule("e", &["-", "e"])
    .prec("UMINUS")
    .rule("e", &["n"])
    .build()?;

// -2*3 is (-2)*3, not -(2*3).
assert_eq!(
    parser.parse("-2*3").unwrap().to_string(),
    r#"(e (e "-" (e (n "2"))) "*" (e (n "3")))"#,
);
# Ok::<(), grammar_lang::GrammarError>(())
```

```rust
use grammar_lang::{Grammar, GrammarError};

let err = Grammar::new().literal("a").prec("HIGH").rule("s", &["a"]).build().unwrap_err();
assert_eq!(err, GrammarError::MisplacedPrec { name: "HIGH".into() });
```

### `Grammar::left`

```rust,ignore
pub fn left(self, names: &[&str]) -> Grammar
```

Declares a precedence level of **left-associative** names, binding tighter
than every level declared before it. A tie between operators of the level
reduces, grouping `a - b - c` as `(a - b) - c`.

| Parameter | Meaning |
|---|---|
| `names` | Tokens, or marker names for [`prec`](#grammarprec). Each name may appear in one level only ([`DuplicatePrecedence`](#grammarerror)); rules may not appear at all ([`PrecedenceOnRule`](#grammarerror)). |

```rust
use grammar_lang::Grammar;

let parser = Grammar::new()
    .literal("+")
    .literal("-")
    .pattern("n", "[0-9]")
    .left(&["+", "-"])
    .rule("e", &["e", "+", "e"])
    .rule("e", &["e", "-", "e"])
    .rule("e", &["n"])
    .build()?;
assert_eq!(
    parser.parse("1-2+3").unwrap().to_string(),
    r#"(e (e (e (n "1")) "-" (e (n "2"))) "+" (e (n "3")))"#,
);
# Ok::<(), grammar_lang::GrammarError>(())
```

### `Grammar::right`

```rust,ignore
pub fn right(self, names: &[&str]) -> Grammar
```

Declares a precedence level of **right-associative** names, binding tighter
than every level declared before it. A tie shifts, grouping `a ^ b ^ c` as
`a ^ (b ^ c)` and `a = b = c` as `a = (b = c)`.

| Parameter | Meaning |
|---|---|
| `names` | Tokens, or marker names for [`prec`](#grammarprec). |

```rust
use grammar_lang::Grammar;

let parser = Grammar::new()
    .literal("=")
    .pattern("id", "[a-z]")
    .right(&["="])
    .rule("e", &["e", "=", "e"])
    .rule("e", &["id"])
    .build()?;
assert_eq!(
    parser.parse("a=b=c").unwrap().to_string(),
    r#"(e (e (id "a")) "=" (e (e (id "b")) "=" (e (id "c"))))"#,
);
# Ok::<(), grammar_lang::GrammarError>(())
```

### `Grammar::nonassoc`

```rust,ignore
pub fn nonassoc(self, names: &[&str]) -> Grammar
```

Declares a precedence level of **non-associative** names, binding tighter
than every level declared before it. A tie is a syntax error, so `a < b < c`
is rejected while `a < b` parses.

| Parameter | Meaning |
|---|---|
| `names` | Tokens, or marker names for [`prec`](#grammarprec). |

```rust
use grammar_lang::Grammar;

let parser = Grammar::new()
    .literal("<")
    .literal("==")
    .pattern("n", "[0-9]")
    .nonassoc(&["<", "=="])
    .rule("e", &["e", "<", "e"])
    .rule("e", &["e", "==", "e"])
    .rule("e", &["n"])
    .build()?;
assert!(parser.parse("1<2").is_ok());
assert!(parser.parse("1<2==3").is_err());
assert_eq!(parser.parse("1<2<3").unwrap_err().to_string(), "expected end of input, found `<`");
# Ok::<(), grammar_lang::GrammarError>(())
```

### `Grammar::build`

```rust,ignore
pub fn build(&self) -> Result<Parser, GrammarError>
```

Checks the grammar and generates its parser: every token is compiled into one
minimized DFA, and the rules into an LALR(1) table, with conflicts settled as
described in [How conflicts are settled](#how-conflicts-are-settled). Rules
the start rule cannot reach are left out of the table, though their names
still resolve through [`Parser::kind`](#parserkind). The grammar is borrowed,
so it can be extended and built again.

**Errors.** The first problem found, in the order of
[What `build` checks](#what-build-checks).

```rust
use grammar_lang::{Grammar, GrammarError};

let base = Grammar::new().pattern("num", "[0-9]+");
let err = base.clone().rule("sum", &["num", "+", "num"]).build().unwrap_err();
assert_eq!(err, GrammarError::Undefined { name: "+".into(), rule: "sum".into() });

let parser = base.literal("+").rule("sum", &["num", "+", "num"]).build()?;
assert!(parser.parse("1+2").is_ok());
# Ok::<(), GrammarError>(())
```

```rust
use grammar_lang::{Grammar, GrammarError};

// A rule that can derive itself alone would make some input infinitely
// ambiguous, and is refused.
let err = Grammar::new()
    .literal("x")
    .rule("s", &["s"])
    .rule("s", &["x"])
    .build()
    .unwrap_err();
assert_eq!(err, GrammarError::Cycle { name: "s".into() });
```

## `Parser`

```rust,ignore
#[derive(Clone)]
pub struct Parser { /* private */ }
```

A generated parser: the DFA for the tokens and the LALR(1) tables for the
rules. Parsing is one left-to-right pass, linear in the input: the lexer
finds each token by longest match as the parser asks for it, skipped tokens
are passed over, and each step of the parser is one table lookup. Stack use
grows with nesting depth only, on the heap.

A `Parser` is immutable, `Send`, `Sync`, and `Clone`; one instance can serve
any number of parses on any number of threads. Its `Debug` output summarizes
its size.

```rust
use grammar_lang::Grammar;

let parser = Grammar::new()
    .literal("+")
    .pattern("n", "[0-9]+")
    .left(&["+"])
    .rule("e", &["e", "+", "e"])
    .rule("e", &["n"])
    .build()?;

std::thread::scope(|scope| {
    for i in 0..4 {
        let parser = &parser;
        scope.spawn(move || assert!(parser.parse(&format!("{i}+{i}")).is_ok()));
    }
});
assert!(format!("{parser:?}").starts_with("Parser { tokens: 2, rules: 1, productions: 2"));
# Ok::<(), grammar_lang::GrammarError>(())
```

### `Parser::parse`

```rust,ignore
pub fn parse<'a>(&'a self, text: &'a str) -> Result<Tree<'a>, ParseError>
```

Parses `text` into a concrete syntax [`Tree`](#tree): a node for every
production the parse used and a leaf for every token, skipped tokens aside.

| Parameter | Meaning |
|---|---|
| `text` | The input. The tree borrows it, and the parser, for node text and names. |

**Errors.** A [`ParseError`](#parseerror) at the first token the grammar does
not allow, at a premature end of input, or at text no token matches. Input of
4 GiB or more is rejected, since spans are 32-bit.

```rust
use grammar_lang::Grammar;

let parser = Grammar::new()
    .literal("=")
    .pattern("ident", "[a-z]+")
    .pattern("num", "[0-9]+")
    .skip("space", " +")
    .rule("assign", &["ident", "=", "num"])
    .build()?;

let tree = parser.parse("x = 42").unwrap();
let root = tree.root();
assert_eq!(root.name(), "assign");
assert_eq!(root.child(0).map(|n| n.text()), Some("x"));
assert_eq!(root.child(2).map(|n| n.text()), Some("42"));
# Ok::<(), grammar_lang::GrammarError>(())
```

```rust
use grammar_lang::{Grammar, Span};

let parser = Grammar::new()
    .literal(";")
    .pattern("word", "[a-z]+")
    .skip("space", " +")
    .rule("stmt", &["word", ";"])
    .build()?;

let err = parser.parse("hello world;").unwrap_err();
assert_eq!(err.span(), Span::new(6, 11));
assert_eq!(err.to_string(), "expected `;`, found `word` `world`");
# Ok::<(), grammar_lang::GrammarError>(())
```

### `Parser::parse_with`

```rust,ignore
pub fn parse_with<'s, A: Actions<'s>>(&self, text: &'s str, actions: &mut A)
    -> Result<A::Value, ParseError>
```

Parses `text`, building the result with your [`Actions`](#actions) instead of
a tree: `actions.token` for each token consumed, `actions.reduce` for each
completed production, children before parents. The value returned for the
start rule is the result. Nothing is allocated beyond the parser's two stacks,
so a parse that computes a single value costs only the work of recognizing the
input.

| Parameter | Meaning |
|---|---|
| `text` | The input. Token text handed to the actions borrows from it (`&'s str`). |
| `actions` | The semantic actions, by reference, so whatever they accumulate — an arena, a symbol table — stays available after the parse. |

**Errors.** As for [`parse`](#parserparse). On failure, the actions have seen
the calls for the input before the error, and the values built so far are
dropped.

```rust
use grammar_lang::{Actions, Grammar, Kind, Production, Span, Token};
use std::vec::Drain;

/// Counts tokens without building anything.
#[derive(Default)]
struct Count(usize);

impl<'s> Actions<'s> for Count {
    type Value = ();
    fn token(&mut self, _: Token<Kind>, _: &'s str) {
        self.0 += 1;
    }
    fn reduce(&mut self, _: Production, _: Span, _: Drain<'_, ()>) {}
}

let parser = Grammar::new()
    .pattern("word", "[a-z]+")
    .skip("space", " +")
    .rule("text", &["text", "word"])
    .rule("text", &["word"])
    .build()?;

let mut count = Count::default();
parser.parse_with("the quick brown fox", &mut count).unwrap();
assert_eq!(count.0, 4);
# Ok::<(), grammar_lang::GrammarError>(())
```

```rust
use grammar_lang::{Actions, Grammar, Kind, Production, Span, Token};
use std::vec::Drain;

/// Collects `key=value` pairs as borrowed slices.
struct Pairs;

impl<'s> Actions<'s> for Pairs {
    type Value = Vec<(&'s str, &'s str)>;

    fn token(&mut self, _: Token<Kind>, text: &'s str) -> Self::Value {
        vec![(text, "")]
    }

    fn reduce(&mut self, p: Production, _: Span, mut kids: Drain<'_, Self::Value>) -> Self::Value {
        match p.index() {
            // pairs → pairs pair
            0 => {
                let mut all = kids.next().unwrap_or_default();
                all.extend(kids.next().unwrap_or_default());
                all
            }
            // pairs → pair
            1 => kids.next().unwrap_or_default(),
            // pair → key "=" key
            _ => {
                let key = kids.next().and_then(|v| v.first().map(|p| p.0)).unwrap_or("");
                let value = kids.nth(1).and_then(|v| v.first().map(|p| p.0)).unwrap_or("");
                vec![(key, value)]
            }
        }
    }
}

let parser = Grammar::new()
    .literal("=")
    .pattern("key", "[a-z0-9]+")
    .skip("space", " +")
    .rule("pairs", &["pairs", "pair"])
    .rule("pairs", &["pair"])
    .rule("pair", &["key", "=", "key"])
    .build()?;

let pairs = parser.parse_with("a=1 b=2", &mut Pairs).unwrap();
assert_eq!(pairs, [("a", "1"), ("b", "2")]);
# Ok::<(), grammar_lang::GrammarError>(())
```

### `Parser::tokens`

```rust,ignore
pub fn tokens<'a>(&'a self, text: &'a str) -> Tokens<'a>
```

Splits `text` into tokens without parsing. The [`Tokens`](#tokens) iterator
yields every token in order, skipped tokens included (with
[`is_trivia`](#kind) set), each by longest match. Where no token matches, it
yields an error covering one character and carries on after it — what a
syntax highlighter needs.

| Parameter | Meaning |
|---|---|
| `text` | The input. |

```rust
use grammar_lang::{Grammar, TokenKind};

let parser = Grammar::new()
    .literal("+")
    .pattern("num", "[0-9]+")
    .skip("space", " +")
    .rule("sum", &["num", "+", "num"])
    .build()?;

let text = "1 + 22";
let tokens: Vec<(&str, &str)> = parser
    .tokens(text)
    .filter_map(Result::ok)
    .filter(|t| !t.is_trivia())
    .map(|t| {
        let span = t.span();
        (parser.name(*t.kind()), &text[span.start().to_usize()..span.end().to_usize()])
    })
    .collect();
assert_eq!(tokens, [("num", "1"), ("+", "+"), ("num", "22")]);
# Ok::<(), grammar_lang::GrammarError>(())
```

```rust
use grammar_lang::Grammar;

let parser = Grammar::new().pattern("num", "[0-9]+").rule("s", &["num"]).build()?;
let items: Vec<Result<&str, String>> = parser
    .tokens("12?3")
    .map(|item| item.map(|t| parser.name(*t.kind())).map_err(|e| e.to_string()))
    .collect();
assert_eq!(
    items,
    [Ok("num"), Err("unrecognized input `?`".to_string()), Ok("num")],
);
# Ok::<(), grammar_lang::GrammarError>(())
```

### `Parser::kind`

```rust,ignore
pub fn kind(&self, name: &str) -> Option<Kind>
```

The [`Kind`](#kind) of the token or rule named `name`, or `None` if the
grammar declares no such name. A literal's name is its text. Lookup is a
binary search over the names.

| Parameter | Meaning |
|---|---|
| `name` | A token or rule name. |

```rust
use grammar_lang::Grammar;

let parser = Grammar::new()
    .literal("+")
    .pattern("num", "[0-9]+")
    .rule("sum", &["num", "+", "num"])
    .build()?;
assert!(parser.kind("+").is_some_and(|k| k.is_token()));
assert!(parser.kind("sum").is_some_and(|k| k.is_rule()));
assert_eq!(parser.kind("-"), None);
# Ok::<(), grammar_lang::GrammarError>(())
```

```rust
use grammar_lang::Grammar;

// The usual pattern: look kinds up once, then compare while walking.
let parser = Grammar::new()
    .literal(",")
    .pattern("n", "[0-9]+")
    .rule("list", &["list", ",", "n"])
    .rule("list", &["n"])
    .build()?;
let n = parser.kind("n").unwrap();
let tree = parser.parse("4,5,6").unwrap();
let sum: u32 = tree
    .root()
    .descendants()
    .filter(|node| node.kind() == n)
    .map(|node| node.text().parse::<u32>().unwrap_or(0))
    .sum();
assert_eq!(sum, 15);
# Ok::<(), grammar_lang::GrammarError>(())
```

### `Parser::name`

```rust,ignore
pub fn name(&self, kind: Kind) -> &str
```

The name the grammar declared for `kind`. A kind issued by a different parser
names an unrelated token or rule, or — if out of this parser's range — the
empty string.

| Parameter | Meaning |
|---|---|
| `kind` | A kind from this parser. |

```rust
use grammar_lang::Grammar;

let parser = Grammar::new().literal("x").rule("start", &["x"]).build()?;
let tree = parser.parse("x").unwrap();
assert_eq!(parser.name(tree.root().kind()), "start");
assert_eq!(parser.name(parser.kind("x").unwrap()), "x");
# Ok::<(), grammar_lang::GrammarError>(())
```

## `Actions`

```rust,ignore
pub trait Actions<'s> {
    type Value;
    fn token(&mut self, token: Token<Kind>, text: &'s str) -> Self::Value;
    fn reduce(&mut self, production: Production, span: Span,
              children: alloc::vec::Drain<'_, Self::Value>) -> Self::Value;
}
```

Semantic actions: what [`parse_with`](#parserparse_with) builds as it
recognizes the input. Calls arrive in postorder — each token as it is
consumed, each production once its children are done — and the value of the
start rule is the result.

| Item | Meaning |
|---|---|
| `'s` | The input's lifetime: token text is borrowed from it, so values can hold `&'s str` without copying. |
| `Value` | What every token and completed production becomes. |
| `token(token, text)` | Builds the value of a consumed token. `token` carries the [`Kind`](#kind) and [`Span`](#re-exports); skipped tokens never arrive. |
| `reduce(production, span, children)` | Builds the value of a completed production. `production` identifies the alternative ([`Production`](#production)); `span` is the input it covers (empty, just after the previous token, for an empty production); `children` yields the values of its symbols, in order, moved out of the parser's stack. Values not taken are dropped. |

```rust
use grammar_lang::{Actions, Grammar, Kind, Production, Span, Token};
use std::vec::Drain;

/// Records the order of calls.
#[derive(Default)]
struct Trace(Vec<String>);

impl<'s> Actions<'s> for Trace {
    type Value = ();
    fn token(&mut self, _: Token<Kind>, text: &'s str) {
        self.0.push(text.to_string());
    }
    fn reduce(&mut self, p: Production, span: Span, kids: Drain<'_, ()>) {
        self.0.push(format!("p{} ({} children, {span})", p.index(), kids.len()));
    }
}

let parser = Grammar::new()
    .literal("a")
    .literal("b")
    .rule("s", &["x", "b"]) // p0
    .rule("x", &["a"])      // p1
    .rule("x", &[])         // p2
    .build()?;

let mut trace = Trace::default();
parser.parse_with("ab", &mut trace).unwrap();
assert_eq!(trace.0, ["a", "p1 (1 children, 0..1)", "b", "p0 (2 children, 0..2)"]);

let mut trace = Trace::default();
parser.parse_with("b", &mut trace).unwrap();
assert_eq!(trace.0, ["p2 (0 children, 0..0)", "b", "p0 (2 children, 0..1)"]);
# Ok::<(), grammar_lang::GrammarError>(())
```

## `Production`

```rust,ignore
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Production { /* private */ }

impl Production {
    pub const fn index(self) -> usize;
    pub const fn rule(self) -> Kind;
}
```

A production of the grammar, as reported to
[`Actions::reduce`](#actions).

| Method | Returns |
|---|---|
| `index()` | The production's number: its position among every [`rule`](#grammarrule) call, from 0. |
| `rule()` | The [`Kind`](#kind) of the rule it belongs to. |

```rust
use grammar_lang::{Actions, Grammar, Kind, Production, Span, Token};
use std::vec::Drain;

struct Rules(Vec<(usize, Kind)>);

impl<'s> Actions<'s> for Rules {
    type Value = ();
    fn token(&mut self, _: Token<Kind>, _: &'s str) {}
    fn reduce(&mut self, p: Production, _: Span, _: Drain<'_, ()>) {
        self.0.push((p.index(), p.rule()));
    }
}

let parser = Grammar::new()
    .literal("a")
    .rule("s", &["t"]) // 0
    .rule("t", &["a"]) // 1
    .build()?;
let mut rules = Rules(Vec::new());
parser.parse_with("a", &mut rules).unwrap();
let (s, t) = (parser.kind("s").unwrap(), parser.kind("t").unwrap());
assert_eq!(rules.0, [(1, t), (0, s)]);
# Ok::<(), grammar_lang::GrammarError>(())
```

## `Tree`

```rust,ignore
pub struct Tree<'a> { /* private */ }

impl<'a> Tree<'a> {
    pub fn root(&self) -> Node<'_>;
    pub fn source(&self) -> &'a str;
}
impl Display for Tree<'_> { /* S-expression */ }
impl Debug for Tree<'_> { /* S-expression */ }
```

A concrete syntax tree from [`Parser::parse`](#parserparse): a node for each
production the parse used and a leaf for each token. It borrows the parser and
the input, and stores its nodes flat, so building it is a push per node and
dropping it is two deallocations however deep it is. Skipped tokens are not in
the tree; their text remains in the input between the leaves.

| Method | Returns |
|---|---|
| `root()` | The [`Node`](#node) of the start rule, covering the whole parse. |
| `source()` | The input the tree was parsed from. |

`Display` (and `Debug`) prints an S-expression: a rule as `(name child...)`,
a literal token as its quoted text, any other token as `(name "text")`. The
alternate form `{:#}` puts each child on its own indented line.

```rust
use grammar_lang::Grammar;

let parser = Grammar::new()
    .literal("(")
    .literal(")")
    .pattern("atom", "[a-z]+")
    .skip("space", " +")
    .rule("list", &["(", "items", ")"])
    .rule("items", &[])
    .rule("items", &["items", "item"])
    .rule("item", &["atom"])
    .rule("item", &["list"])
    .build()?;

let tree = parser.parse("(a (b))").unwrap();
assert_eq!(tree.source(), "(a (b))");
assert_eq!(
    tree.to_string(),
    r#"(list "(" (items (items (items) (item (atom "a"))) (item (list "(" (items (items) (item (atom "b"))) ")"))) ")")"#,
);
assert_eq!(format!("{tree:#}").lines().take(3).collect::<Vec<_>>(), [
    "(list",
    "  \"(\"",
    "  (items",
]);
# Ok::<(), grammar_lang::GrammarError>(())
```

## `Node`

```rust,ignore
#[derive(Clone, Copy)]
pub struct Node<'t> { /* private */ }

impl<'t> Node<'t> {
    pub fn kind(self) -> Kind;
    pub fn name(self) -> &'t str;
    pub fn span(self) -> Span;
    pub fn text(self) -> &'t str;
    pub fn children(self) -> impl DoubleEndedIterator<Item = Node<'t>> + ExactSizeIterator + 't;
    pub fn child(self, index: usize) -> Option<Node<'t>>;
    pub fn descendants(self) -> impl Iterator<Item = Node<'t>> + 't;
}
impl Display for Node<'_> { /* the subtree as an S-expression */ }
```

A node of a [`Tree`](#tree): a rule match or a token. A cheap `Copy` handle,
so navigation allocates nothing (except `descendants`, which keeps a small
stack).

| Method | Returns |
|---|---|
| `kind()` | The node's [`Kind`](#kind). |
| `name()` | The name of its token or rule. |
| `span()` | The bytes it covers, from its first token's start to its last token's end. An empty production has an empty span just after the previous token. |
| `text()` | The input it covers, skipped tokens between its tokens included. |
| `children()` | Its children in input order; one per symbol of the production that matched, none for a token. |
| `child(index)` | The child at `index`, if any. |
| `descendants()` | The node and every node below it, in preorder (parents first, children in input order). Iterative, so any depth is safe. |

```rust
use grammar_lang::{Grammar, Span};

let parser = Grammar::new()
    .literal("=")
    .pattern("key", "[a-z]+")
    .pattern("value", "[0-9]+")
    .skip("space", " +")
    .rule("entry", &["key", "=", "value"])
    .build()?;

let tree = parser.parse("  port = 8080 ").unwrap();
let entry = tree.root();
assert_eq!(entry.span(), Span::new(2, 13));
assert_eq!(entry.text(), "port = 8080");
let texts: Vec<&str> = entry.children().map(|n| n.text()).collect();
assert_eq!(texts, ["port", "=", "8080"]);
assert_eq!(entry.children().len(), 3);
assert_eq!(entry.children().next_back().map(|n| n.name()), Some("value"));
assert!(entry.child(3).is_none());
assert_eq!(entry.child(2).map(|n| n.to_string()), Some(r#"(value "8080")"#.to_string()));
# Ok::<(), grammar_lang::GrammarError>(())
```

```rust
use grammar_lang::Grammar;

let parser = Grammar::new()
    .literal(",")
    .pattern("n", "[0-9]+")
    .rule("list", &["list", ",", "n"])
    .rule("list", &["n"])
    .build()?;
let tree = parser.parse("1,2,3").unwrap();
let order: Vec<String> = tree
    .root()
    .descendants()
    .map(|node| if node.kind().is_rule() { node.name().to_string() } else { node.text().to_string() })
    .collect();
assert_eq!(order, ["list", "list", "list", "1", ",", "2", ",", "3"]);
# Ok::<(), grammar_lang::GrammarError>(())
```

## `Kind`

```rust,ignore
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Kind { /* private */ }

impl Kind {
    pub const fn is_token(self) -> bool;
    pub const fn is_rule(self) -> bool;
}
impl TokenKind for Kind { /* is_trivia: true for skipped tokens */ }
```

What a token or tree node is: one of the grammar's tokens or rules. Every
declared name gets a `Kind` when the grammar is built; resolve names with
[`Parser::kind`](#parserkind) and back with [`Parser::name`](#parsername).
Kinds belong to the parser that issued them.

| Method | Returns |
|---|---|
| `is_token()` | Whether this is a token's kind (literal, pattern, or skipped). |
| `is_rule()` | Whether this is a rule's kind. |
| `is_trivia()` *(from [`TokenKind`](#re-exports))* | Whether this is a skipped token's kind. |

```rust
use grammar_lang::{Grammar, TokenKind};

let parser = Grammar::new()
    .pattern("word", "[a-z]+")
    .skip("space", " +")
    .rule("words", &["words", "word"])
    .rule("words", &["word"])
    .build()?;

let word = parser.kind("word").unwrap();
let space = parser.kind("space").unwrap();
let words = parser.kind("words").unwrap();
assert!(word.is_token() && !word.is_trivia());
assert!(space.is_token() && space.is_trivia());
assert!(words.is_rule() && !words.is_token());
assert_ne!(word, words);
# Ok::<(), grammar_lang::GrammarError>(())
```

## `Tokens`

```rust,ignore
#[derive(Clone, Debug)]
pub struct Tokens<'a> { /* private */ }

impl Iterator for Tokens<'_> {
    type Item = Result<Token<Kind>, ParseError>;
}
impl FusedIterator for Tokens<'_> {}
```

The iterator from [`Parser::tokens`](#parsertokens). Yields `Ok` for each
token, skipped tokens included, and `Err` — a [`ParseError`](#parseerror)
covering one character, with an empty expected set — for each character no
token matches, then resumes after it. Once it returns `None` it always does.

```rust
use grammar_lang::Grammar;

let parser = Grammar::new().pattern("x", "x+").rule("s", &["x"]).build()?;
let mut tokens = parser.tokens("xx?x");
assert!(tokens.next().is_some_and(|t| t.is_ok()));
let err = tokens.next().unwrap().unwrap_err();
assert_eq!(err.to_string(), "unrecognized input `?`");
assert!(err.expected().is_empty());
assert!(tokens.next().is_some_and(|t| t.is_ok()));
assert!(tokens.next().is_none());
assert!(tokens.next().is_none());
# Ok::<(), grammar_lang::GrammarError>(())
```

## `GrammarError`

```rust,ignore
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GrammarError { /* variants below */ }
impl Display for GrammarError {}
impl core::error::Error for GrammarError {}
```

Why [`build`](#grammarbuild) refused a grammar. Names in the error are the
grammar's own. The enum is `#[non_exhaustive]`: match it with a wildcard arm.

| Variant | Meaning | Message |
|---|---|---|
| `NoRules` | No rules declared. | the grammar declares no rules |
| `Duplicate { name }` | Two tokens, or a token and a rule, share a name. | `` `x` is declared more than once `` |
| `Undefined { name, rule }` | A production uses an undeclared name. | ``rule `s` uses `b`, which is not a token or rule`` |
| `SkipInRule { name, rule }` | A production uses a skipped token. | ``rule `s` uses `ws`, which is a skipped token`` |
| `MisplacedPrec { name }` | `prec` was called before any `rule`. | ``precedence `p` is applied before any rule is declared`` |
| `UndefinedPrecedence { name }` | A `prec` name is in no level. | ``precedence `p` is not declared at any level`` |
| `DuplicatePrecedence { name }` | A name is in two levels, or twice in one. | `` `p` is given a precedence more than once `` |
| `PrecedenceOnRule { name }` | A level names a rule. | `` `e` is a rule and cannot have a precedence `` |
| `InvalidPattern { name, offset, reason }` | A pattern does not parse, or is too large to compile. | ``invalid pattern for `t` at byte 3: unclosed class`` |
| `EmptyToken { name }` | A token can match the empty string. | ``token `t` matches the empty string`` |
| `ShadowedToken { name, by }` | A token can never win a match. | ``token `t` is never produced; `u` takes priority over it`` |
| `Unproductive { name }` | A reachable rule derives no finite input. | ``rule `r` cannot derive any finite input`` |
| `Cycle { name }` | A reachable rule can derive itself alone. | ``rule `r` can derive itself, so some input has infinitely many parse trees`` |
| `ShiftReduce { token, shift, reduce }` | An unresolved shift/reduce conflict; `shift` is an item `rule → before • after`, `reduce` a production. | ``shift/reduce conflict on `+`: shift in `e → e • + e`, or reduce `e → e + e` `` |
| `ReduceReduce { token, first, second }` | An unresolved reduce/reduce conflict; `token` is `None` for end of input. | ``reduce/reduce conflict on end of input: reduce `a → x`, or reduce `b → x` `` |
| `TooLarge` | The lexer or tables would exceed the [size limits](#limits). | the grammar's lexer or parse tables exceed the size limits |

```rust
use grammar_lang::{Grammar, GrammarError};

fn describe(err: &GrammarError) -> &'static str {
    match err {
        GrammarError::ShiftReduce { .. } | GrammarError::ReduceReduce { .. } => "ambiguous",
        GrammarError::InvalidPattern { .. } | GrammarError::EmptyToken { .. } => "bad token",
        _ => "other",
    }
}

let err = Grammar::new()
    .literal("x")
    .rule("s", &["a"])
    .rule("s", &["b"])
    .rule("a", &["x"])
    .rule("b", &["x"])
    .build()
    .unwrap_err();
assert_eq!(describe(&err), "ambiguous");
assert_eq!(
    err,
    GrammarError::ReduceReduce { token: None, first: "a → x".into(), second: "b → x".into() },
);
```

```rust
use grammar_lang::{Grammar, GrammarError};

let err = Grammar::new().pattern("n", "[0-9").rule("s", &["n"]).build().unwrap_err();
assert_eq!(
    err,
    GrammarError::InvalidPattern { name: "n".into(), offset: 0, reason: "unclosed class" },
);
let boxed: Box<dyn std::error::Error> = Box::new(err);
assert_eq!(boxed.to_string(), "invalid pattern for `n` at byte 0: unclosed class");
```

## `ParseError`

```rust,ignore
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError { /* private */ }

impl ParseError {
    pub const fn span(&self) -> Span;
    pub const fn found(&self) -> Option<Kind>;
    pub const fn at_end(&self) -> bool;
    pub fn expected(&self) -> &[Kind];
    pub fn to_diagnostic(&self) -> diag_lang::Diagnostic;
}
impl Display for ParseError {}
impl core::error::Error for ParseError {}
```

Why input was rejected. Parsing stops at the first error, which is one of:

| Case | `found()` | `at_end()` | `span()` |
|---|---|---|---|
| A token the grammar does not allow here | its `Kind` | `false` | the token |
| The input ended where more was required | `None` | `true` | empty, at the end of the input |
| Text no token matches | `None` | `false` | the first character no token can start with |

The message names tokens as the grammar does — a literal by its text, any
other token by its name and text — and lists what was expected: "expected
`a`", "expected `a` or `b`", or "expected one of `a`, `b`, or `c`", with "end
of input" among them when the input could have ended there.

| Method | Returns |
|---|---|
| `span()` | The byte range the error points at. |
| `found()` | The unexpected token's kind, if the error is one. |
| `at_end()` | Whether the input ended early. |
| `expected()` | The tokens that could have come next, in declaration order. Whether end of input could have is in the message. |
| `to_diagnostic()` | A [`diag_lang::Diagnostic`](https://docs.rs/diag-lang) with the message and a primary label on the span. |

```rust
use grammar_lang::{Grammar, Span};

let parser = Grammar::new()
    .literal("(")
    .literal(")")
    .pattern("num", "[0-9]+")
    .skip("space", " +")
    .rule("group", &["(", "num", ")"])
    .build()?;

let err = parser.parse("(1 2)").unwrap_err();
assert_eq!(err.to_string(), "expected `)`, found `num` `2`");
assert_eq!(err.span(), Span::new(3, 4));
assert_eq!(err.found(), parser.kind("num"));
assert_eq!(err.expected(), [parser.kind(")").unwrap()]);

let err = parser.parse("(1").unwrap_err();
assert!(err.at_end());
assert_eq!(err.to_string(), "expected `)`, found end of input");

let err = parser.parse("(#)").unwrap_err();
assert_eq!(err.found(), None);
assert!(!err.at_end());
assert_eq!(err.to_string(), "unrecognized input `#`; expected `num`");
# Ok::<(), grammar_lang::GrammarError>(())
```

```rust
use diag_lang::{Renderer, Severity, SourceMap};
use grammar_lang::Grammar;

let parser = Grammar::new()
    .literal(";")
    .pattern("word", "[a-z]+")
    .skip("space", r"\s+")
    .rule("stmts", &["stmts", "word", ";"])
    .rule("stmts", &["word", ";"])
    .build()?;

let source = "one;\ntwo three;\n";
let diag = parser.parse(source).unwrap_err().to_diagnostic();
assert_eq!(diag.severity(), Severity::Error);
assert_eq!(diag.message(), "expected `;`, found `word` `three`");
assert_eq!(diag.primary().message(), "unexpected token");

let mut map = SourceMap::new();
map.add("input.txt", source).unwrap();
let report = Renderer::new().render(&diag, &map);
assert!(report.contains("input.txt:2:5"));
# Ok::<(), grammar_lang::GrammarError>(())
```

## Re-exports

```rust,ignore
pub use token_lang::{Span, Token, TokenKind};
```

From [`token-lang`](https://docs.rs/token-lang), so callers name the same
versions this crate uses:

| Item | Use here |
|---|---|
| `Span` | Byte ranges of tokens, nodes, and errors. `start()`/`end()` return positions with `to_usize()`. |
| `Token<K>` | What the lexer yields and `Actions::token` receives, as `Token<Kind>`: `kind()` and `span()`. |
| `TokenKind` | Implemented by [`Kind`](#kind); `is_trivia()` marks skipped tokens. |

```rust
use grammar_lang::{Grammar, Span, Token, TokenKind};

let parser = Grammar::new()
    .pattern("w", "[a-z]+")
    .skip("space", " +")
    .rule("s", &["w"])
    .build()?;
let tokens: Vec<Token<_>> = parser.tokens("hi ").filter_map(Result::ok).collect();
assert_eq!(tokens[0].span(), Span::new(0, 2));
assert!(!tokens[0].is_trivia() && tokens[1].is_trivia());
# Ok::<(), grammar_lang::GrammarError>(())
```

## Feature flags

| Feature | Default | Effect |
|---|---|---|
| `std` | yes | Forwards to `token-lang/std` and `diag-lang/std`. Without it the crate is `no_std` and needs only `alloc`; the API is identical. |

## Limits

Limits exist so that a hostile or accidental grammar fails with an error
instead of exhausting memory; real grammars sit orders of magnitude below them.

| Limit | Value | Error |
|---|---|---|
| Repetition count in `{n,m}` | 1000 | `InvalidPattern` |
| Group nesting in a pattern | 64 | `InvalidPattern` |
| NFA states for one pattern | 1,048,576 | `InvalidPattern` ("too large to compile") |
| Lexer table cells | 4,194,304 (16 MiB) | `TooLarge` |
| ACTION + GOTO table cells | 67,108,864 (256 MiB) | `TooLarge` |
| Input length | `u32::MAX` bytes (4 GiB) | `ParseError` |

## Guide: an expression language

A small but complete language — statements, variables, operators at four
levels, function calls — showing how the pieces fit:

```rust
use grammar_lang::Grammar;

let parser = Grammar::new()
    // Keywords before the identifier pattern is fine: literals win ties.
    .literal("let")
    .literal("print")
    .literal("=")
    .literal(";")
    .literal(",")
    .literal("(")
    .literal(")")
    .literal("||")
    .literal("&&")
    .literal("==")
    .literal("+")
    .literal("-")
    .literal("*")
    .literal("/")
    .pattern("ident", "[A-Za-z_][A-Za-z0-9_]*")
    .pattern("number", r"[0-9]+(\.[0-9]+)?")
    .skip("space", r"\s+")
    .skip("comment", "//[^\n]*")
    // Loosest first.
    .left(&["||"])
    .left(&["&&"])
    .nonassoc(&["=="])
    .left(&["+", "-"])
    .left(&["*", "/"])
    .right(&["NEG"])
    .rule("program", &["program", "stmt"])
    .rule("program", &["stmt"])
    .rule("stmt", &["let", "ident", "=", "expr", ";"])
    .rule("stmt", &["print", "expr", ";"])
    .rule("expr", &["expr", "||", "expr"])
    .rule("expr", &["expr", "&&", "expr"])
    .rule("expr", &["expr", "==", "expr"])
    .rule("expr", &["expr", "+", "expr"])
    .rule("expr", &["expr", "-", "expr"])
    .rule("expr", &["expr", "*", "expr"])
    .rule("expr", &["expr", "/", "expr"])
    .rule("expr", &["-", "expr"])
    .prec("NEG")
    .rule("expr", &["(", "expr", ")"])
    .rule("expr", &["ident", "(", "args", ")"])
    .rule("expr", &["ident", "(", ")"])
    .rule("expr", &["ident"])
    .rule("expr", &["number"])
    .rule("args", &["args", ",", "expr"])
    .rule("args", &["expr"])
    .build()?;

let source = "
    // Totals.
    let total = price * (1 + rate) - discount;
    print max(total, 0) == total && -total == 0 - total;
";
let tree = parser.parse(source).unwrap();
let stmts = tree.root().descendants().filter(|n| n.name() == "stmt").count();
assert_eq!(stmts, 2);

// `==` is non-associative: comparisons do not chain. (`(` is listed because
// `b` could still become a call, `b(...)`.)
let err = parser.parse("print a == b == c;").unwrap_err();
assert_eq!(
    err.to_string(),
    "expected one of `;`, `(`, `||`, `&&`, `+`, `-`, `*`, or `/`, found `==`",
);
# Ok::<(), grammar_lang::GrammarError>(())
```

## Guide: porting a yacc or Bison grammar

The declarations map one to one:

| yacc / Bison | grammar-lang |
|---|---|
| `%token NUM` + a lexer rule | `.pattern("NUM", "[0-9]+")` |
| a literal token `'+'` | `.literal("+")` |
| whitespace and comments the lexer discards | `.skip("ws", r"\s+")` |
| `%left '+' '-'` | `.left(&["+", "-"])` |
| `%right '^'` | `.right(&["^"])` |
| `%nonassoc '<'` | `.nonassoc(&["<"])` |
| `expr: expr '+' expr` | `.rule("expr", &["expr", "+", "expr"])` |
| `\| %empty` | `.rule("name", &[])` |
| `%prec UMINUS` after a rule | `.prec("UMINUS")` after the `rule` call |
| `%start program` | declare `program`'s first production first |
| `{ $$ = $1 + $3; }` | an [`Actions`](#actions) arm for that production's index |

Precedence and conflict resolution follow Bison's rules exactly, so a grammar
Bison accepts **without** conflict warnings builds here unchanged and parses
the same way. A grammar Bison accepts **with** warnings relies on its defaults
— shift for shift/reduce, the earlier rule for reduce/reduce — and `build`
refuses it, naming the conflict; add the precedence declaration that states
the intent.

```rust
use grammar_lang::Grammar;

// Bison:
//   %left '+' '-'
//   %left '*'
//   %precedence NEG
//   exp: exp '+' exp | exp '-' exp | exp '*' exp | '-' exp %prec NEG | NUM ;
let parser = Grammar::new()
    .literal("+")
    .literal("-")
    .literal("*")
    .pattern("NUM", "[0-9]+")
    .left(&["+", "-"])
    .left(&["*"])
    .left(&["NEG"])
    .rule("exp", &["exp", "+", "exp"])
    .rule("exp", &["exp", "-", "exp"])
    .rule("exp", &["exp", "*", "exp"])
    .rule("exp", &["-", "exp"])
    .prec("NEG")
    .rule("exp", &["NUM"])
    .build()?;
assert!(parser.parse("-1*2-3").is_ok());
# Ok::<(), grammar_lang::GrammarError>(())
```

## Stability

`grammar-lang` 1.0 is the API freeze. Everything below is covered by
[Semantic Versioning](https://semver.org/spec/v2.0.0.html): it does not change
in a breaking way before `2.0`.

### The frozen surface

| Item | Frozen |
|---|---|
| `Grammar` | `new`, `literal`, `pattern`, `skip`, `rule`, `prec`, `left`, `right`, `nonassoc`, `build`; `Clone`, `Debug`, `Default`. |
| `Parser` | `parse`, `parse_with`, `tokens`, `kind`, `name`; `Clone`, `Debug`, `Send`, `Sync`. |
| `Actions<'s>` | `Value`, `token(Token<Kind>, &'s str)`, `reduce(Production, Span, Drain<'_, Value>)`. |
| `Production` | `index`, `rule`; `Clone`, `Copy`, `Debug`, `PartialEq`, `Eq`, `Hash`. |
| `Tree<'a>` | `root`, `source`; `Display`, `Debug`. |
| `Node<'t>` | `kind`, `name`, `span`, `text`, `children`, `child`, `descendants`; `Clone`, `Copy`, `Display`, `Debug`. |
| `Kind` | `is_token`, `is_rule`, `TokenKind`; `Clone`, `Copy`, `Debug`, `PartialEq`, `Eq`, `Hash`, `PartialOrd`, `Ord`. |
| `Tokens<'a>` | `Iterator<Item = Result<Token<Kind>, ParseError>>`, `FusedIterator`; `Clone`, `Debug`. |
| `GrammarError` | Every variant and field listed in [`GrammarError`](#grammarerror). The enum is `#[non_exhaustive]`: new variants may be added in minor releases. |
| `ParseError` | `span`, `found`, `at_end`, `expected`, `to_diagnostic`; `Display`, `Error`, `Clone`, `Debug`, `PartialEq`, `Eq`. |
| Re-exports | `Span`, `Token`, `TokenKind` from `token-lang` 1. |
| Feature | `std`, default-on, additive. |

### The behavioural contract

These are part of the API, not implementation details:

- **Lexing.** The longest match wins; on a tie a literal beats a pattern, and
  otherwise the token declared first. The [pattern syntax](#pattern-syntax)
  accepts what it accepts today and means what it means today.
- **Precedence and conflicts.** A production's precedence is its last token's
  unless `prec` names another; conflicts are settled in the order and by the
  rules of [How conflicts are settled](#how-conflicts-are-settled), matching
  GNU Bison; a conflict precedence does not settle is an error.
- **What builds.** A grammar that builds keeps building, with the same parse
  table semantics. The checks of [What `build` checks](#what-build-checks) are
  not tightened in a way that would refuse a grammar 1.0 accepts.
- **Numbering and shape.** Productions are numbered in `rule` call order from
  0. A tree has a node per production used and a leaf per consumed token, with
  the spans documented on [`Node`](#node). `Actions` are called in postorder.
- **The tree's text form.** `Display` for `Tree` and `Node` prints the
  S-expression documented on [`Tree`](#tree); `{:#}` indents children by two
  spaces per level.
- **Errors.** A parse stops at the first token that cannot continue the input;
  `ParseError::expected` lists exactly the tokens that could, in declaration
  order; `found` and `at_end` classify the error as documented.
- **Safety.** The crate is `#![forbid(unsafe_code)]`, does not panic on any
  input, and every parse terminates.

### Not promised

- The exact wording of `GrammarError` and `ParseError` messages, and of `Debug`
  output. They are written for people and may be improved in a minor release;
  match on variants, fields, and accessors instead.
- Which conflict is reported when a grammar has several, beyond being
  deterministic.
- Performance figures, table sizes, and the internal size limits of
  [Limits](#limits), which may be raised.

### Dependencies and MSRV

`token-lang` 1 and `diag-lang` 1 are public dependencies — their types appear
in this crate's API — so moving to a new major version of either is a major
release here. The minimum supported Rust version is **1.85**; raising it is
not considered a breaking change, but happens only in a minor release and is
recorded in [`CHANGELOG.md`](../CHANGELOG.md).
