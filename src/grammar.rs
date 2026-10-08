//! [`Grammar`]: the description a parser is built from.

use alloc::{boxed::Box, collections::BTreeMap, string::String, vec, vec::Vec};

use crate::lex::{self, LexError, regex};
use crate::lr::{self, Associativity, Conflict, LrError, Prec, Prod, Spec};
use crate::parser::{ProdInfo, Production};
use crate::{GrammarError, Kind, Parser};

/// A grammar: the tokens of a language and the rules that arrange them.
///
/// A grammar is described with a by-value builder and turned into a
/// [`Parser`] by [`build`](Grammar::build), which checks it and generates a
/// lexer and an LALR(1) parse table. Nothing is checked while describing;
/// every problem surfaces as one [`GrammarError`] from `build`.
///
/// **Tokens** come in three forms. A [`literal`](Grammar::literal) matches its
/// text exactly and is named by it, so rules can say `"+"` or `"if"`. A
/// [`pattern`](Grammar::pattern) matches a regular expression. A
/// [`skip`](Grammar::skip) token is a pattern the lexer matches and the
/// parser never sees — whitespace and comments. The lexer always takes the
/// longest match; between matches of equal length a literal beats a pattern,
/// and otherwise the token declared first wins. So `literal("if")` and
/// `pattern("ident", "[a-z]+")` lex `if` as the keyword and `iffy` as an
/// identifier.
///
/// **Rules** are written one production at a time: each
/// [`rule`](Grammar::rule) call adds an alternative to the named rule, as a
/// sequence of token and rule names. The first rule declared is the start
/// rule, which a parse must match against the whole input.
///
/// **Precedence** settles the ambiguity of operator grammars the way yacc
/// does. [`left`](Grammar::left), [`right`](Grammar::right), and
/// [`nonassoc`](Grammar::nonassoc) each declare one level, binding tighter
/// than the levels before it. A production takes the precedence of its last
/// token — none, if that token has none — unless [`prec`](Grammar::prec)
/// names another. These are yacc's and Bison's rules, so their grammars carry
/// over unchanged.
///
/// # Examples
///
/// Arithmetic with the usual precedence:
///
/// ```
/// use grammar_lang::Grammar;
///
/// let parser = Grammar::new()
///     .literal("+")
///     .literal("*")
///     .literal("(")
///     .literal(")")
///     .pattern("num", "[0-9]+")
///     .skip("space", r"\s+")
///     .left(&["+"])
///     .left(&["*"])
///     .rule("expr", &["expr", "+", "expr"])
///     .rule("expr", &["expr", "*", "expr"])
///     .rule("expr", &["(", "expr", ")"])
///     .rule("expr", &["num"])
///     .build()?;
///
/// let tree = parser.parse("1 + 2 * 3").unwrap();
/// assert_eq!(
///     tree.to_string(),
///     r#"(expr (expr (num "1")) "+" (expr (expr (num "2")) "*" (expr (num "3"))))"#,
/// );
/// # Ok::<(), grammar_lang::GrammarError>(())
/// ```
#[derive(Clone, Debug, Default)]
pub struct Grammar {
    tokens: Vec<TokenDecl>,
    prods: Vec<ProdDecl>,
    levels: Vec<LevelDecl>,
    misplaced_prec: Option<Box<str>>,
}

/// A declared token.
#[derive(Clone, Debug)]
struct TokenDecl {
    name: Box<str>,
    /// The regular expression, or `None` for a literal (matching `name`).
    regex: Option<Box<str>>,
    skip: bool,
}

/// One production of a rule.
#[derive(Clone, Debug)]
struct ProdDecl {
    rule: Box<str>,
    symbols: Box<[Box<str>]>,
    prec: Option<Box<str>>,
}

/// One precedence level.
#[derive(Clone, Debug)]
struct LevelDecl {
    assoc: Associativity,
    names: Box<[Box<str>]>,
}

impl Grammar {
    /// An empty grammar.
    ///
    /// # Examples
    ///
    /// ```
    /// use grammar_lang::{Grammar, GrammarError};
    ///
    /// assert_eq!(Grammar::new().build().unwrap_err(), GrammarError::NoRules);
    /// ```
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares a token that matches `text` exactly, named by its text.
    ///
    /// Rules refer to it by that same text: after `literal("while")`, a rule
    /// can use `"while"`. A literal beats any pattern matching the same
    /// characters, which is how keywords take priority over identifiers.
    ///
    /// # Parameters
    ///
    /// - `text`: the exact characters to match, and the token's name. An
    ///   empty literal is rejected by [`build`](Grammar::build).
    ///
    /// # Examples
    ///
    /// ```
    /// use grammar_lang::Grammar;
    ///
    /// let parser = Grammar::new()
    ///     .literal("let")
    ///     .literal("=")
    ///     .literal(";")
    ///     .pattern("ident", "[a-z]+")
    ///     .skip("space", " +")
    ///     .rule("stmt", &["let", "ident", "=", "ident", ";"])
    ///     .build()?;
    ///
    /// assert!(parser.parse("let x = y;").is_ok());
    /// // `let` is a keyword: it cannot name a variable.
    /// assert!(parser.parse("let let = y;").is_err());
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[must_use]
    pub fn literal(mut self, text: &str) -> Self {
        self.tokens.push(TokenDecl {
            name: text.into(),
            regex: None,
            skip: false,
        });
        self
    }

    /// Declares a token named `name` that matches the regular expression
    /// `regex`.
    ///
    /// The syntax is the one Rust's `regex` crate uses, minus what has no
    /// meaning for a token that always takes the longest match: literals and
    /// escapes (`\n`, `\t`, `\x7F`, `\u{1F600}`, and any escaped punctuation),
    /// `.` (any character but a newline), classes such as `[a-z_]` and
    /// `[^"\\]`, the ASCII classes `\d`, `\w`, and `\s` and their negations,
    /// groups `(...)` and `(?:...)`, alternation `|`, and repetition `*`, `+`,
    /// `?`, `{n}`, `{n,}`, and `{n,m}`. Matching is by Unicode scalar value.
    /// Anchors, lazy repetition, and flags are rejected.
    ///
    /// # Parameters
    ///
    /// - `name`: how rules and error messages refer to the token.
    /// - `regex`: the pattern. It must not match the empty string.
    ///
    /// # Examples
    ///
    /// ```
    /// use grammar_lang::Grammar;
    ///
    /// let parser = Grammar::new()
    ///     .pattern("string", r#""([^"\\]|\\.)*""#)
    ///     .rule("value", &["string"])
    ///     .build()?;
    ///
    /// assert!(parser.parse(r#""say \"hi\"""#).is_ok());
    /// assert!(parser.parse(r#""unterminated"#).is_err());
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    ///
    /// Unicode classes match whole characters:
    ///
    /// ```
    /// use grammar_lang::Grammar;
    ///
    /// let parser = Grammar::new()
    ///     .pattern("greek", "[α-ω]+")
    ///     .rule("word", &["greek"])
    ///     .build()?;
    /// assert!(parser.parse("λογος").is_ok());
    /// // `ό` (U+03CC) lies past `ω` (U+03C9).
    /// assert!(parser.parse("λόγος").is_err());
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[must_use]
    pub fn pattern(mut self, name: &str, regex: &str) -> Self {
        self.tokens.push(TokenDecl {
            name: name.into(),
            regex: Some(regex.into()),
            skip: false,
        });
        self
    }

    /// Declares a token the lexer matches and the parser skips, such as
    /// whitespace or comments.
    ///
    /// Skipped tokens may appear between any two tokens; rules cannot name
    /// them. [`Parser::tokens`] still reports them, with
    /// [`is_trivia`](crate::TokenKind::is_trivia) set.
    ///
    /// # Parameters
    ///
    /// - `name`: the token's name.
    /// - `regex`: the pattern, in the syntax of [`pattern`](Grammar::pattern).
    ///
    /// # Examples
    ///
    /// ```
    /// use grammar_lang::Grammar;
    ///
    /// let parser = Grammar::new()
    ///     .pattern("num", "[0-9]+")
    ///     .skip("space", r"\s+")
    ///     .skip("comment", "#[^\n]*")
    ///     .rule("nums", &["nums", "num"])
    ///     .rule("nums", &["num"])
    ///     .build()?;
    ///
    /// assert!(parser.parse("1 2 # three\n 4").is_ok());
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[must_use]
    pub fn skip(mut self, name: &str, regex: &str) -> Self {
        self.tokens.push(TokenDecl {
            name: name.into(),
            regex: Some(regex.into()),
            skip: true,
        });
        self
    }

    /// Adds a production to rule `name`: the rule matches `symbols` in order.
    ///
    /// Call it once per alternative. An empty `symbols` makes the rule match
    /// nothing at all, which is how optional parts are written. The first
    /// rule named is the start rule. Productions are numbered from 0 in the
    /// order they are added, across all rules; [`Production::index`] reports
    /// that number to [`Actions`](crate::Actions).
    ///
    /// # Parameters
    ///
    /// - `name`: the rule.
    /// - `symbols`: token and rule names. Left recursion (`list → list item`)
    ///   is the natural form for an LR parser and uses constant stack.
    ///
    /// # Examples
    ///
    /// ```
    /// use grammar_lang::Grammar;
    ///
    /// // args → ε | list;  list → list "," x | x
    /// let parser = Grammar::new()
    ///     .literal(",")
    ///     .literal("x")
    ///     .rule("args", &[])
    ///     .rule("args", &["list"])
    ///     .rule("list", &["list", ",", "x"])
    ///     .rule("list", &["x"])
    ///     .build()?;
    ///
    /// for input in ["", "x", "x,x,x"] {
    ///     assert!(parser.parse(input).is_ok(), "{input:?}");
    /// }
    /// assert!(parser.parse("x,").is_err());
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[must_use]
    pub fn rule(mut self, name: &str, symbols: &[&str]) -> Self {
        self.prods.push(ProdDecl {
            rule: name.into(),
            symbols: symbols.iter().map(|&s| s.into()).collect(),
            prec: None,
        });
        self
    }

    /// Gives the most recently added production the precedence of `name`,
    /// instead of the precedence of its last token.
    ///
    /// `name` is a token or a marker name listed in a precedence level. The
    /// classic use is unary minus, which shares its token with subtraction
    /// but binds tighter.
    ///
    /// # Parameters
    ///
    /// - `name`: a name declared in [`left`](Grammar::left),
    ///   [`right`](Grammar::right), or [`nonassoc`](Grammar::nonassoc).
    ///
    /// # Examples
    ///
    /// ```
    /// use grammar_lang::Grammar;
    ///
    /// let parser = Grammar::new()
    ///     .literal("-")
    ///     .literal("*")
    ///     .pattern("num", "[0-9]+")
    ///     .left(&["-"])
    ///     .left(&["*"])
    ///     .right(&["NEG"])
    ///     .rule("e", &["e", "-", "e"])
    ///     .rule("e", &["e", "*", "e"])
    ///     .rule("e", &["-", "e"])
    ///     .prec("NEG")
    ///     .rule("e", &["num"])
    ///     .build()?;
    ///
    /// // -2 * 3 groups as (-2) * 3.
    /// let tree = parser.parse("-2*3").unwrap();
    /// assert_eq!(
    ///     tree.to_string(),
    ///     r#"(e (e "-" (e (num "2"))) "*" (e (num "3")))"#,
    /// );
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[must_use]
    pub fn prec(mut self, name: &str) -> Self {
        match self.prods.last_mut() {
            Some(prod) => prod.prec = Some(name.into()),
            None => {
                if self.misplaced_prec.is_none() {
                    self.misplaced_prec = Some(name.into());
                }
            }
        }
        self
    }

    /// Declares a precedence level of left-associative operators, binding
    /// tighter than every level declared before it.
    ///
    /// Left associativity groups `a - b - c` as `(a - b) - c`.
    ///
    /// # Parameters
    ///
    /// - `names`: tokens, or marker names for [`prec`](Grammar::prec).
    ///
    /// # Examples
    ///
    /// ```
    /// use grammar_lang::Grammar;
    ///
    /// let parser = Grammar::new()
    ///     .literal("-")
    ///     .pattern("n", "[0-9]")
    ///     .left(&["-"])
    ///     .rule("e", &["e", "-", "e"])
    ///     .rule("e", &["n"])
    ///     .build()?;
    /// assert_eq!(
    ///     parser.parse("1-2-3").unwrap().to_string(),
    ///     r#"(e (e (e (n "1")) "-" (e (n "2"))) "-" (e (n "3")))"#,
    /// );
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[must_use]
    pub fn left(self, names: &[&str]) -> Self {
        self.level(Associativity::Left, names)
    }

    /// Declares a precedence level of right-associative operators, binding
    /// tighter than every level declared before it.
    ///
    /// Right associativity groups `a ^ b ^ c` as `a ^ (b ^ c)`.
    ///
    /// # Parameters
    ///
    /// - `names`: tokens, or marker names for [`prec`](Grammar::prec).
    ///
    /// # Examples
    ///
    /// ```
    /// use grammar_lang::Grammar;
    ///
    /// let parser = Grammar::new()
    ///     .literal("^")
    ///     .pattern("n", "[0-9]")
    ///     .right(&["^"])
    ///     .rule("e", &["e", "^", "e"])
    ///     .rule("e", &["n"])
    ///     .build()?;
    /// assert_eq!(
    ///     parser.parse("1^2^3").unwrap().to_string(),
    ///     r#"(e (e (n "1")) "^" (e (e (n "2")) "^" (e (n "3"))))"#,
    /// );
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[must_use]
    pub fn right(self, names: &[&str]) -> Self {
        self.level(Associativity::Right, names)
    }

    /// Declares a precedence level of non-associative operators, binding
    /// tighter than every level declared before it.
    ///
    /// Chaining two operators of a non-associative level, as in `a < b < c`,
    /// is a syntax error.
    ///
    /// # Parameters
    ///
    /// - `names`: tokens, or marker names for [`prec`](Grammar::prec).
    ///
    /// # Examples
    ///
    /// ```
    /// use grammar_lang::Grammar;
    ///
    /// let parser = Grammar::new()
    ///     .literal("<")
    ///     .pattern("n", "[0-9]")
    ///     .nonassoc(&["<"])
    ///     .rule("e", &["e", "<", "e"])
    ///     .rule("e", &["n"])
    ///     .build()?;
    /// assert!(parser.parse("1<2").is_ok());
    /// assert!(parser.parse("1<2<3").is_err());
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[must_use]
    pub fn nonassoc(self, names: &[&str]) -> Self {
        self.level(Associativity::NonAssoc, names)
    }

    fn level(mut self, assoc: Associativity, names: &[&str]) -> Self {
        self.levels.push(LevelDecl {
            assoc,
            names: names.iter().map(|&s| s.into()).collect(),
        });
        self
    }

    /// Checks the grammar and generates its parser.
    ///
    /// Building compiles every token into one minimized DFA and the rules
    /// into an LALR(1) parse table. Rules the start rule cannot reach are
    /// left out of the table, though their names still resolve through
    /// [`Parser::kind`]. The grammar is left untouched, so it can be extended
    /// and built again.
    ///
    /// # Errors
    ///
    /// Returns the first problem found, as a [`GrammarError`]: a malformed
    /// declaration, an invalid or empty-matching or never-produced token, a
    /// rule that cannot derive any input, or a conflict precedence does not
    /// resolve.
    ///
    /// # Examples
    ///
    /// ```
    /// use grammar_lang::{Grammar, GrammarError};
    ///
    /// let grammar = Grammar::new().pattern("num", "[0-9]+");
    /// let err = grammar.clone().rule("sum", &["num", "+", "num"]).build().unwrap_err();
    /// assert_eq!(
    ///     err,
    ///     GrammarError::Undefined { name: "+".into(), rule: "sum".into() },
    /// );
    ///
    /// let parser = grammar.literal("+").rule("sum", &["num", "+", "num"]).build()?;
    /// assert!(parser.parse("1+2").is_ok());
    /// # Ok::<(), GrammarError>(())
    /// ```
    pub fn build(&self) -> Result<Parser, GrammarError> {
        Builder::new(self)?.finish()
    }
}

/// What a name refers to while building.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Entity {
    /// Token by declaration order.
    Token(usize),
    /// Rule by first appearance.
    Rule(usize),
}

/// The name tables `build` works from.
struct Builder<'g> {
    grammar: &'g Grammar,
    /// Rule names by first appearance.
    rules: Vec<&'g str>,
    /// Each production's symbols, resolved.
    resolved: Vec<Vec<Entity>>,
    /// Each production's rule.
    prod_rule: Vec<usize>,
    /// Precedence by name.
    precs: BTreeMap<&'g str, Prec>,
}

impl<'g> Builder<'g> {
    /// Resolves names and checks the declarations.
    fn new(grammar: &'g Grammar) -> Result<Self, GrammarError> {
        let mut names: BTreeMap<&str, Entity> = BTreeMap::new();
        for (i, token) in grammar.tokens.iter().enumerate() {
            if names.insert(&token.name, Entity::Token(i)).is_some() {
                return Err(GrammarError::Duplicate {
                    name: token.name.clone(),
                });
            }
        }
        let mut rules: Vec<&str> = Vec::new();
        let mut prod_rule = Vec::with_capacity(grammar.prods.len());
        for prod in &grammar.prods {
            let id = match names.get(&*prod.rule) {
                Some(Entity::Rule(id)) => *id,
                Some(Entity::Token(_)) => {
                    return Err(GrammarError::Duplicate {
                        name: prod.rule.clone(),
                    });
                }
                None => {
                    let id = rules.len();
                    rules.push(&prod.rule);
                    let _ = names.insert(&prod.rule, Entity::Rule(id));
                    id
                }
            };
            prod_rule.push(id);
        }
        if rules.is_empty() {
            return Err(GrammarError::NoRules);
        }
        if let Some(name) = &grammar.misplaced_prec {
            return Err(GrammarError::MisplacedPrec { name: name.clone() });
        }

        let mut precs: BTreeMap<&str, Prec> = BTreeMap::new();
        for (level, decl) in grammar.levels.iter().enumerate() {
            for name in &decl.names {
                if let Some(Entity::Rule(_)) = names.get(&**name) {
                    return Err(GrammarError::PrecedenceOnRule { name: name.clone() });
                }
                let prec = Prec {
                    level: level as u32 + 1,
                    assoc: decl.assoc,
                };
                if precs.insert(name, prec).is_some() {
                    return Err(GrammarError::DuplicatePrecedence { name: name.clone() });
                }
            }
        }

        let mut resolved = Vec::with_capacity(grammar.prods.len());
        for prod in &grammar.prods {
            let mut symbols = Vec::with_capacity(prod.symbols.len());
            for symbol in &prod.symbols {
                match names.get(&**symbol) {
                    None => {
                        return Err(GrammarError::Undefined {
                            name: symbol.clone(),
                            rule: prod.rule.clone(),
                        });
                    }
                    Some(&Entity::Token(t)) if grammar.tokens[t].skip => {
                        return Err(GrammarError::SkipInRule {
                            name: symbol.clone(),
                            rule: prod.rule.clone(),
                        });
                    }
                    Some(&entity) => symbols.push(entity),
                }
            }
            if let Some(name) = &prod.prec {
                if !precs.contains_key(&**name) {
                    return Err(GrammarError::UndefinedPrecedence { name: name.clone() });
                }
            }
            resolved.push(symbols);
        }

        Ok(Self {
            grammar,
            rules,
            resolved,
            prod_rule,
            precs,
        })
    }

    /// Compiles the lexer and the parse table.
    fn finish(self) -> Result<Parser, GrammarError> {
        let grammar = self.grammar;
        let tokens = &grammar.tokens;

        // Kinds: parsed tokens first, in declaration order, so a token's kind
        // index is its terminal number; then skipped tokens; then rules.
        let mut kind_of_token = vec![0u32; tokens.len()];
        let mut next = 0u32;
        for skip in [false, true] {
            for (i, token) in tokens.iter().enumerate() {
                if token.skip == skip {
                    kind_of_token[i] = next;
                    next += 1;
                }
            }
        }
        let parsed = tokens.iter().filter(|t| !t.skip).count();
        let lexed = tokens.len();
        if lexed + self.rules.len() >= (1 << 30) || grammar.prods.len() >= (1 << 29) {
            return Err(GrammarError::TooLarge);
        }

        let dfa = self.lexer(&kind_of_token)?;

        let reachable = self.check_rules()?;

        // The generator's numbering: terminal = kind index of a parsed token,
        // then end of input; nonterminal 0 = the augmented start, then the
        // reachable rules in order.
        let terms = parsed + 1;
        let mut nonterm_of = vec![u32::MAX; self.rules.len()];
        let mut rule_of_nonterm = vec![0usize];
        for (rule, _) in reachable.iter().enumerate().filter(|&(_, &r)| r) {
            nonterm_of[rule] = rule_of_nonterm.len() as u32;
            rule_of_nonterm.push(rule);
        }
        let mut term_prec = vec![None; terms];
        for (i, token) in tokens.iter().enumerate() {
            if !token.skip {
                term_prec[kind_of_token[i] as usize] = self.precs.get(&*token.name).copied();
            }
        }
        let symbol = |entity: Entity| -> u32 {
            match entity {
                Entity::Token(t) => kind_of_token[t],
                Entity::Rule(r) => terms as u32 + nonterm_of[r],
            }
        };
        let mut prods = vec![Prod {
            lhs: 0,
            rhs: vec![terms as u32 + 1],
            prec: None,
        }];
        let rule_kind = |rule: usize| Kind::rule((lexed + rule) as u32);
        let mut infos = vec![ProdInfo {
            len: 1,
            lhs: 0,
            production: Production::new(0, rule_kind(0)),
        }];
        for (p, decl) in grammar.prods.iter().enumerate() {
            let rule = self.prod_rule[p];
            if !reachable[rule] {
                continue;
            }
            let rhs: Vec<u32> = self.resolved[p].iter().map(|&e| symbol(e)).collect();
            let prec = match &decl.prec {
                Some(name) => self.precs.get(&**name).copied(),
                None => rhs
                    .iter()
                    .rev()
                    .find(|&&s| (s as usize) < terms)
                    .and_then(|&s| term_prec[s as usize]),
            };
            infos.push(ProdInfo {
                len: rhs.len() as u32,
                lhs: nonterm_of[rule],
                production: Production::new(p as u32, rule_kind(rule)),
            });
            prods.push(Prod {
                lhs: nonterm_of[rule],
                rhs,
                prec,
            });
        }
        let spec = Spec {
            terms,
            nonterms: rule_of_nonterm.len(),
            prods,
            term_prec,
        };

        let table = lr::build(&spec).map_err(|e| match e {
            LrError::TooLarge => GrammarError::TooLarge,
            LrError::Conflict(conflict) => self.conflict(&spec, &rule_of_nonterm, conflict),
        })?;

        // Names in kind order, and an index sorted by name for lookup.
        let mut names: Vec<Box<str>> = vec![Box::from(""); lexed + self.rules.len()];
        let mut literal = vec![false; names.len()];
        for (i, token) in tokens.iter().enumerate() {
            let k = kind_of_token[i] as usize;
            names[k] = token.name.clone();
            literal[k] = token.regex.is_none();
        }
        for (r, &name) in self.rules.iter().enumerate() {
            names[lexed + r] = name.into();
        }
        let mut by_name: Vec<u32> = (0..names.len() as u32).collect();
        by_name.sort_unstable_by(|&a, &b| names[a as usize].cmp(&names[b as usize]));

        Ok(Parser {
            names: names.into_boxed_slice(),
            literal: literal.into_boxed_slice(),
            by_name: by_name.into_boxed_slice(),
            parsed: parsed as u32,
            lexed: lexed as u32,
            dfa,
            action: table.action,
            goto: table.goto,
            terms: terms as u32,
            nonterms: spec.nonterms as u32,
            prods: infos.into_boxed_slice(),
        })
    }

    /// Finds the rules the start rule reaches, and checks that each can
    /// derive some input and none can derive itself.
    fn check_rules(&self) -> Result<Vec<bool>, GrammarError> {
        let count = self.rules.len();
        let mut by_rule: Vec<Vec<usize>> = vec![Vec::new(); count];
        for (p, &rule) in self.prod_rule.iter().enumerate() {
            by_rule[rule].push(p);
        }
        let mut reachable = vec![false; count];
        reachable[0] = true;
        let mut stack = vec![0usize];
        while let Some(rule) = stack.pop() {
            for &p in &by_rule[rule] {
                for &symbol in &self.resolved[p] {
                    if let Entity::Rule(r) = symbol {
                        if !reachable[r] {
                            reachable[r] = true;
                            stack.push(r);
                        }
                    }
                }
            }
        }

        // Productive rules have a production of tokens and productive rules;
        // nullable rules have one of nullable rules alone.
        let fixpoint = |base: fn(Entity) -> bool| -> Vec<bool> {
            let mut holds = vec![false; count];
            let mut changed = true;
            while changed {
                changed = false;
                for (p, symbols) in self.resolved.iter().enumerate() {
                    let rule = self.prod_rule[p];
                    if !holds[rule]
                        && symbols.iter().all(|&s| match s {
                            Entity::Rule(r) => holds[r],
                            token => base(token),
                        })
                    {
                        holds[rule] = true;
                        changed = true;
                    }
                }
            }
            holds
        };
        let productive = fixpoint(|_| true);
        if let Some(rule) = (0..count).find(|&r| reachable[r] && !productive[r]) {
            return Err(GrammarError::Unproductive {
                name: self.rules[rule].into(),
            });
        }

        // A rule that derives itself alone (`a ⇒+ a`) gives some input
        // infinitely many parse trees. It shows up as a conflict unless a
        // precedence hides it, and then the parser would reduce around the
        // cycle forever, so it is refused outright. `a → b` contributes the
        // edge a → b when everything else in the production can vanish.
        let nullable = fixpoint(|_| false);
        let mut unit: Vec<Vec<usize>> = vec![Vec::new(); count];
        for (p, symbols) in self.resolved.iter().enumerate() {
            let rule = self.prod_rule[p];
            if !reachable[rule] {
                continue;
            }
            for (i, &symbol) in symbols.iter().enumerate() {
                let Entity::Rule(target) = symbol else {
                    continue;
                };
                let rest_vanishes = symbols
                    .iter()
                    .enumerate()
                    .all(|(j, &s)| j == i || matches!(s, Entity::Rule(r) if nullable[r]));
                if rest_vanishes {
                    unit[rule].push(target);
                }
            }
        }
        let mut seen = vec![false; count];
        for rule in (0..count).filter(|&r| reachable[r]) {
            seen.fill(false);
            stack.clear();
            stack.extend_from_slice(&unit[rule]);
            while let Some(next) = stack.pop() {
                if next == rule {
                    return Err(GrammarError::Cycle {
                        name: self.rules[rule].into(),
                    });
                }
                if !seen[next] {
                    seen[next] = true;
                    stack.extend_from_slice(&unit[next]);
                }
            }
        }
        Ok(reachable)
    }

    /// Parses every pattern and builds the scanning DFA.
    fn lexer(&self, kind_of_token: &[u32]) -> Result<lex::Dfa, GrammarError> {
        let tokens = &self.grammar.tokens;
        let mut specs: Vec<Option<lex::Spec>> = (0..tokens.len()).map(|_| None).collect();
        for (i, token) in tokens.iter().enumerate() {
            let hir = match &token.regex {
                None => regex::Hir::literal(&token.name),
                Some(pattern) => {
                    regex::parse(pattern).map_err(|e| GrammarError::InvalidPattern {
                        name: token.name.clone(),
                        offset: e.offset,
                        reason: e.reason,
                    })?
                }
            };
            // Literals outrank patterns; declaration order breaks the rest.
            let rank = if token.regex.is_none() {
                i
            } else {
                tokens.len() + i
            };
            specs[kind_of_token[i] as usize] = Some(lex::Spec {
                hir,
                rank: rank as u32,
                order: i as u32,
            });
        }
        let specs: Vec<lex::Spec> = specs.into_iter().flatten().collect();
        let token_at = |kind: u32| -> &TokenDecl { &tokens[specs[kind as usize].order as usize] };
        lex::build(&specs).map_err(|e| match e {
            LexError::PatternTooLarge(k) => GrammarError::InvalidPattern {
                name: token_at(k).name.clone(),
                offset: 0,
                reason: "the pattern is too large to compile",
            },
            LexError::TooLarge => GrammarError::TooLarge,
            LexError::EmptyMatch(k) => GrammarError::EmptyToken {
                name: token_at(k).name.clone(),
            },
            LexError::Shadowed { token, by } => GrammarError::ShadowedToken {
                name: token_at(token).name.clone(),
                by: token_at(by).name.clone(),
            },
        })
    }

    /// Renders a conflict with the grammar's names.
    fn conflict(&self, spec: &Spec, rule_of_nonterm: &[usize], conflict: Conflict) -> GrammarError {
        let parsed: Vec<&str> = self
            .grammar
            .tokens
            .iter()
            .filter(|d| !d.skip)
            .map(|d| &*d.name)
            .collect();
        let term_name = |t: u32| -> Option<Box<str>> { parsed.get(t as usize).map(|&n| n.into()) };
        let symbol_name = |s: u32| -> &str {
            let s = s as usize;
            if s < spec.terms {
                parsed.get(s).copied().unwrap_or_default()
            } else {
                self.rules[rule_of_nonterm[s - spec.terms]]
            }
        };
        let render = |prod: u32, dot: Option<usize>| -> Box<str> {
            let prod = &spec.prods[prod as usize];
            let mut out = String::new();
            if prod.lhs == 0 {
                out.push_str("$accept");
            } else {
                out.push_str(self.rules[rule_of_nonterm[prod.lhs as usize]]);
            }
            out.push_str(" →");
            for (i, &s) in prod.rhs.iter().enumerate() {
                if dot == Some(i) {
                    out.push_str(" •");
                }
                out.push(' ');
                out.push_str(symbol_name(s));
            }
            if dot == Some(prod.rhs.len()) {
                out.push_str(" •");
            }
            if prod.rhs.is_empty() && dot.is_none() {
                out.push_str(" ε");
            }
            out.into_boxed_str()
        };
        match conflict {
            Conflict::ShiftReduce {
                term,
                shift,
                reduce,
            } => GrammarError::ShiftReduce {
                token: term_name(term).unwrap_or_default(),
                shift: render(shift.0, Some(shift.1)),
                reduce: render(reduce, None),
            },
            Conflict::ReduceReduce {
                term,
                first,
                second,
            } => GrammarError::ReduceReduce {
                token: term_name(term),
                first: render(first, None),
                second: render(second, None),
            },
        }
    }
}
