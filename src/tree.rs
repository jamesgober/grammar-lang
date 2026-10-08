//! [`Tree`] and [`Node`]: the concrete syntax tree [`Parser::parse`] builds.

use alloc::{vec, vec::Vec};
use core::fmt;

use token_lang::{Span, Token};

use crate::{Actions, Kind, Parser, Production};

/// A concrete syntax tree: a node for every production a parse used and a
/// leaf for every token it consumed.
///
/// The tree borrows the parser (for names) and the input (for text), and
/// stores its nodes flat — in one array, in the order the parser completed
/// them — so building it is a push per node and dropping it is two
/// deallocations, however deep it is. Skipped tokens are not in the tree;
/// their text is still in the input, between the spans of the leaves.
///
/// `Display` prints the tree as an S-expression: a rule as `(name child...)`,
/// a literal token as its quoted text, and any other token as
/// `(name "text")`. The alternate form, `{:#}`, puts each child on its own
/// indented line.
///
/// # Examples
///
/// ```
/// use grammar_lang::Grammar;
///
/// let parser = Grammar::new()
///     .literal("(")
///     .literal(")")
///     .pattern("atom", "[a-z]+")
///     .skip("space", " +")
///     .rule("list", &["(", "items", ")"])
///     .rule("items", &[])
///     .rule("items", &["items", "item"])
///     .rule("item", &["atom"])
///     .rule("item", &["list"])
///     .build()?;
///
/// let tree = parser.parse("(a (b))").unwrap();
/// assert_eq!(
///     tree.to_string(),
///     r#"(list "(" (items (items (items) (item (atom "a"))) (item (list "(" (items (items) (item (atom "b"))) ")"))) ")")"#,
/// );
/// assert_eq!(
///     format!("{tree:#}").lines().next(),
///     Some("(list"),
/// );
/// # Ok::<(), grammar_lang::GrammarError>(())
/// ```
pub struct Tree<'a> {
    parser: &'a Parser,
    source: &'a str,
    nodes: Vec<NodeData>,
    children: Vec<u32>,
    root: u32,
}

/// One node, stored flat.
#[derive(Clone, Copy, Debug)]
struct NodeData {
    kind: Kind,
    span: Span,
    /// The node's children are `children[first..first + count]`.
    first: u32,
    count: u32,
}

impl<'a> Tree<'a> {
    /// The node for the start rule, covering the whole parse.
    ///
    /// # Examples
    ///
    /// ```
    /// # use grammar_lang::Grammar;
    /// let parser = Grammar::new()
    ///     .pattern("word", "[a-z]+")
    ///     .skip("space", " +")
    ///     .rule("pair", &["word", "word"])
    ///     .build()?;
    /// let tree = parser.parse(" hello world ").unwrap();
    /// let root = tree.root();
    /// assert_eq!(root.name(), "pair");
    /// assert_eq!(root.text(), "hello world");
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn root(&self) -> Node<'_> {
        Node {
            tree: self,
            index: self.root,
        }
    }

    /// The input the tree was parsed from.
    ///
    /// # Examples
    ///
    /// ```
    /// # use grammar_lang::Grammar;
    /// let parser = Grammar::new().literal("x").rule("s", &["x"]).build()?;
    /// assert_eq!(parser.parse("x").unwrap().source(), "x");
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn source(&self) -> &'a str {
        self.source
    }

    fn data(&self, index: u32) -> NodeData {
        self.nodes[index as usize]
    }

    /// Writes the S-expression for the subtree at `root`, iteratively.
    fn write_sexp(&self, f: &mut fmt::Formatter<'_>, root: u32) -> fmt::Result {
        let pretty = f.alternate();
        let mut stack: Vec<(u32, u32)> = Vec::new();
        if self.open(f, root)? {
            stack.push((root, 0));
        }
        while let Some(top) = stack.last_mut() {
            let (node, next) = *top;
            let data = self.data(node);
            if next == data.count {
                f.write_str(")")?;
                stack.truncate(stack.len() - 1);
                continue;
            }
            top.1 += 1;
            let child = self.children[(data.first + next) as usize];
            if pretty {
                f.write_str("\n")?;
                for _ in 0..stack.len() {
                    f.write_str("  ")?;
                }
            } else {
                f.write_str(" ")?;
            }
            if self.open(f, child)? {
                stack.push((child, 0));
            }
        }
        Ok(())
    }

    /// Writes a leaf, or the opening of a rule node; returns whether the node
    /// needs closing.
    fn open(&self, f: &mut fmt::Formatter<'_>, node: u32) -> Result<bool, fmt::Error> {
        let data = self.data(node);
        let name = self.parser.name(data.kind);
        if data.kind.is_rule() {
            write!(f, "({name}")?;
            return Ok(true);
        }
        let text = self.text(data.span);
        if self.parser.is_literal(data.kind) {
            write!(f, "{text:?}")?;
        } else {
            write!(f, "({name} {text:?})")?;
        }
        Ok(false)
    }

    fn text(&self, span: Span) -> &'a str {
        self.source
            .get(span.start().to_usize()..span.end().to_usize())
            .unwrap_or("")
    }
}

impl fmt::Display for Tree<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write_sexp(f, self.root)
    }
}

impl fmt::Debug for Tree<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write_sexp(f, self.root)
    }
}

/// A node of a [`Tree`]: a rule match or a token.
///
/// A `Node` is a cheap `Copy` handle — a reference to the tree and an index —
/// so navigating the tree allocates nothing.
///
/// # Examples
///
/// ```
/// # use grammar_lang::Grammar;
/// let parser = Grammar::new()
///     .literal("=")
///     .pattern("key", "[a-z]+")
///     .pattern("value", "[0-9]+")
///     .rule("entry", &["key", "=", "value"])
///     .build()?;
///
/// let tree = parser.parse("port=8080").unwrap();
/// let entry = tree.root();
/// let texts: Vec<&str> = entry.children().map(|n| n.text()).collect();
/// assert_eq!(texts, ["port", "=", "8080"]);
/// assert!(entry.children().all(|n| n.kind().is_token()));
/// # Ok::<(), grammar_lang::GrammarError>(())
/// ```
#[derive(Clone, Copy)]
pub struct Node<'t> {
    tree: &'t Tree<'t>,
    index: u32,
}

impl<'t> Node<'t> {
    /// The node's kind: the token or rule it is.
    #[inline]
    #[must_use]
    pub fn kind(self) -> Kind {
        self.tree.data(self.index).kind
    }

    /// The name of the node's token or rule.
    ///
    /// # Examples
    ///
    /// ```
    /// # use grammar_lang::Grammar;
    /// let parser = Grammar::new().literal("x").rule("s", &["x"]).build()?;
    /// let tree = parser.parse("x").unwrap();
    /// assert_eq!(tree.root().name(), "s");
    /// assert_eq!(tree.root().child(0).map(|n| n.name()), Some("x"));
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn name(self) -> &'t str {
        self.tree.parser.name(self.kind())
    }

    /// The bytes of input the node covers: from the start of its first token
    /// to the end of its last. A node that matched nothing has an empty span
    /// just after the preceding token.
    ///
    /// # Examples
    ///
    /// ```
    /// # use grammar_lang::{Grammar, Span};
    /// let parser = Grammar::new()
    ///     .pattern("n", "[0-9]+")
    ///     .skip("space", " +")
    ///     .rule("s", &["n", "n"])
    ///     .build()?;
    /// let tree = parser.parse("  12 345 ").unwrap();
    /// assert_eq!(tree.root().span(), Span::new(2, 8));
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn span(self) -> Span {
        self.tree.data(self.index).span
    }

    /// The input text the node covers, skipped tokens between its own tokens
    /// included.
    ///
    /// # Examples
    ///
    /// ```
    /// # use grammar_lang::Grammar;
    /// let parser = Grammar::new()
    ///     .pattern("n", "[0-9]+")
    ///     .skip("space", " +")
    ///     .rule("s", &["n", "n"])
    ///     .build()?;
    /// let tree = parser.parse("  12   345 ").unwrap();
    /// assert_eq!(tree.root().text(), "12   345");
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn text(self) -> &'t str {
        self.tree.text(self.span())
    }

    /// The node's children, in input order. A token has none; a rule node
    /// has one per symbol of the production that matched.
    ///
    /// # Examples
    ///
    /// ```
    /// # use grammar_lang::Grammar;
    /// let parser = Grammar::new()
    ///     .literal("+")
    ///     .pattern("n", "[0-9]+")
    ///     .rule("sum", &["n", "+", "n"])
    ///     .build()?;
    /// let tree = parser.parse("1+2").unwrap();
    /// let names: Vec<&str> = tree.root().children().map(|n| n.name()).collect();
    /// assert_eq!(names, ["n", "+", "n"]);
    /// assert_eq!(tree.root().children().rev().next().map(|n| n.text()), Some("2"));
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    pub fn children(self) -> impl DoubleEndedIterator<Item = Node<'t>> + ExactSizeIterator + 't {
        let tree = self.tree;
        let data = tree.data(self.index);
        let start = data.first as usize;
        tree.children[start..start + data.count as usize]
            .iter()
            .map(move |&index| Node { tree, index })
    }

    /// The child at position `index`, if there is one.
    ///
    /// # Examples
    ///
    /// ```
    /// # use grammar_lang::Grammar;
    /// let parser = Grammar::new()
    ///     .literal(":")
    ///     .pattern("w", "[a-z]+")
    ///     .rule("pair", &["w", ":", "w"])
    ///     .build()?;
    /// let tree = parser.parse("a:b").unwrap();
    /// assert_eq!(tree.root().child(2).map(|n| n.text()), Some("b"));
    /// assert!(tree.root().child(3).is_none());
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    #[must_use]
    pub fn child(self, index: usize) -> Option<Node<'t>> {
        let data = self.tree.data(self.index);
        if index >= data.count as usize {
            return None;
        }
        let index = self.tree.children[data.first as usize + index];
        Some(Node {
            tree: self.tree,
            index,
        })
    }

    /// This node and every node below it, in preorder: each node before its
    /// children, children in input order. Iterative, so trees of any depth
    /// are safe to walk.
    ///
    /// # Examples
    ///
    /// ```
    /// # use grammar_lang::Grammar;
    /// let parser = Grammar::new()
    ///     .literal(",")
    ///     .pattern("n", "[0-9]+")
    ///     .rule("list", &["list", ",", "n"])
    ///     .rule("list", &["n"])
    ///     .build()?;
    /// let tree = parser.parse("1,2,3").unwrap();
    /// let n = parser.kind("n").unwrap();
    /// let numbers: Vec<&str> = tree
    ///     .root()
    ///     .descendants()
    ///     .filter(|node| node.kind() == n)
    ///     .map(|node| node.text())
    ///     .collect();
    /// assert_eq!(numbers, ["1", "2", "3"]);
    /// # Ok::<(), grammar_lang::GrammarError>(())
    /// ```
    pub fn descendants(self) -> impl Iterator<Item = Node<'t>> + 't {
        let tree = self.tree;
        let mut stack = vec![self.index];
        core::iter::from_fn(move || {
            let index = stack.pop()?;
            let data = tree.data(index);
            let start = data.first as usize;
            stack.extend(
                tree.children[start..start + data.count as usize]
                    .iter()
                    .rev(),
            );
            Some(Node { tree, index })
        })
    }
}

impl fmt::Display for Node<'_> {
    /// The node's subtree as an S-expression; see [`Tree`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.tree.write_sexp(f, self.index)
    }
}

impl fmt::Debug for Node<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Node({} @ {})", self.name(), self.span())
    }
}

/// The [`Actions`] behind [`Parser::parse`].
pub(crate) struct TreeBuilder {
    nodes: Vec<NodeData>,
    children: Vec<u32>,
}

impl TreeBuilder {
    /// A builder for input of `len` bytes.
    pub(crate) fn new(len: usize) -> Self {
        // About one node per three bytes of typical source; a cheap estimate
        // that saves most of the regrowth on large inputs.
        let estimate = len / 3 + 8;
        Self {
            nodes: Vec::with_capacity(estimate),
            children: Vec::with_capacity(estimate),
        }
    }

    pub(crate) fn finish<'a>(self, parser: &'a Parser, source: &'a str, root: u32) -> Tree<'a> {
        Tree {
            parser,
            source,
            nodes: self.nodes,
            children: self.children,
            root,
        }
    }

    // Node indices are `u32`: a tree is at most a few nodes per input byte,
    // and input is capped at `u32::MAX` bytes, so a tree that overflowed them
    // would need hundreds of gigabytes of node storage first.
    fn push(&mut self, data: NodeData) -> u32 {
        self.nodes.push(data);
        (self.nodes.len() - 1) as u32
    }
}

impl<'s> Actions<'s> for TreeBuilder {
    type Value = u32;

    #[inline]
    fn token(&mut self, token: Token<Kind>, _text: &'s str) -> u32 {
        self.push(NodeData {
            kind: *token.kind(),
            span: token.span(),
            first: 0,
            count: 0,
        })
    }

    #[inline]
    fn reduce(
        &mut self,
        production: Production,
        span: Span,
        children: alloc::vec::Drain<'_, u32>,
    ) -> u32 {
        let first = self.children.len() as u32;
        let count = children.len() as u32;
        self.children.extend(children);
        self.push(NodeData {
            kind: production.rule(),
            span,
            first,
            count,
        })
    }
}
