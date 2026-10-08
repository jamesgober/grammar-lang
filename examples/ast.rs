//! Building a typed AST in an `ast-lang` arena, straight from the parse.
//!
//! The grammar describes a tiny statement language. Semantic actions allocate
//! each node in an `ast_lang::Arena` as its production completes, so the
//! parse produces the language's own AST with no intermediate tree; then
//! `ast_lang::walk` runs a visitor over it.
//!
//! ```text
//! cargo run --example ast
//! ```

use std::vec::Drain;

use ast_lang::{Arena, Flow, Id, Node, Visitor, walk};
use grammar_lang::{Actions, Grammar, Kind, Parser, Production, Span, Token};

/// The AST: one enum, children by arena handle.
#[derive(Debug)]
enum Ast {
    Program(Vec<Id<Ast>>, Span),
    Let(Box<str>, Id<Ast>, Span),
    Print(Id<Ast>, Span),
    Binary(char, Id<Ast>, Id<Ast>, Span),
    Number(f64, Span),
    Name(Box<str>, Span),
}

impl Node for Ast {
    fn span(&self) -> Span {
        match self {
            Ast::Program(_, s)
            | Ast::Let(_, _, s)
            | Ast::Print(_, s)
            | Ast::Binary(_, _, _, s)
            | Ast::Number(_, s)
            | Ast::Name(_, s) => *s,
        }
    }

    fn each_child(&self, f: &mut dyn FnMut(Id<Self>)) {
        match self {
            Ast::Program(items, _) => items.iter().copied().for_each(f),
            Ast::Let(_, value, _) | Ast::Print(value, _) => f(*value),
            Ast::Binary(_, a, b, _) => {
                f(*a);
                f(*b);
            }
            Ast::Number(..) | Ast::Name(..) => {}
        }
    }

    fn map_children(&self, f: &mut dyn FnMut(Id<Self>) -> Id<Self>) -> Self {
        match self {
            Ast::Program(items, s) => Ast::Program(items.iter().map(|&i| f(i)).collect(), *s),
            Ast::Let(name, value, s) => Ast::Let(name.clone(), f(*value), *s),
            Ast::Print(value, s) => Ast::Print(f(*value), *s),
            Ast::Binary(op, a, b, s) => Ast::Binary(*op, f(*a), f(*b), *s),
            Ast::Number(v, s) => Ast::Number(*v, *s),
            Ast::Name(n, s) => Ast::Name(n.clone(), *s),
        }
    }
}

fn language() -> Parser {
    let built = Grammar::new()
        .literal("let")
        .literal("print")
        .literal("=")
        .literal(";")
        .literal("+")
        .literal("-")
        .literal("*")
        .literal("/")
        .literal("(")
        .literal(")")
        .pattern("name", "[a-zA-Z_][a-zA-Z0-9_]*")
        .pattern("number", r"[0-9]+(\.[0-9]+)?")
        .skip("space", r"\s+")
        .skip("comment", "//[^\n]*")
        .left(&["+", "-"])
        .left(&["*", "/"])
        .rule("program", &["program", "stmt"]) // 0
        .rule("program", &["stmt"]) // 1
        .rule("stmt", &["let", "name", "=", "expr", ";"]) // 2
        .rule("stmt", &["print", "expr", ";"]) // 3
        .rule("expr", &["expr", "+", "expr"]) // 4
        .rule("expr", &["expr", "-", "expr"]) // 5
        .rule("expr", &["expr", "*", "expr"]) // 6
        .rule("expr", &["expr", "/", "expr"]) // 7
        .rule("expr", &["(", "expr", ")"]) // 8
        .rule("expr", &["number"]) // 9
        .rule("expr", &["name"]) // 10
        .build();
    match built {
        Ok(parser) => parser,
        Err(err) => panic!("the grammar is invalid: {err}"),
    }
}

/// What actions hand each other: arena nodes, the growing statement list,
/// or raw token text.
enum Item<'s> {
    Node(Id<Ast>),
    Stmts(Vec<Id<Ast>>),
    Text(&'s str),
}

struct Lower<'a> {
    arena: &'a mut Arena<Ast>,
}

impl Lower<'_> {
    fn node(item: Item<'_>) -> Option<Id<Ast>> {
        match item {
            Item::Node(id) => Some(id),
            _ => None,
        }
    }

    fn text(item: Option<Item<'_>>) -> Box<str> {
        match item {
            Some(Item::Text(t)) => t.into(),
            _ => "".into(),
        }
    }
}

impl<'s> Actions<'s> for Lower<'_> {
    type Value = Item<'s>;

    fn token(&mut self, _token: Token<Kind>, text: &'s str) -> Item<'s> {
        Item::Text(text)
    }

    fn reduce(&mut self, p: Production, span: Span, mut kids: Drain<'_, Item<'s>>) -> Item<'s> {
        let ast = match p.index() {
            0 => {
                let mut stmts = match kids.next() {
                    Some(Item::Stmts(s)) => s,
                    _ => Vec::new(),
                };
                stmts.extend(kids.next().and_then(Self::node));
                return Item::Stmts(stmts);
            }
            1 => return Item::Stmts(kids.next().and_then(Self::node).into_iter().collect()),
            2 => {
                let name = Self::text(kids.nth(1));
                let Some(value) = kids.nth(1).and_then(Self::node) else {
                    return Item::Text("");
                };
                Ast::Let(name, value, span)
            }
            3 => {
                let Some(value) = kids.nth(1).and_then(Self::node) else {
                    return Item::Text("");
                };
                Ast::Print(value, span)
            }
            4..=7 => {
                let a = kids.next().and_then(Self::node);
                let op = Self::text(kids.next()).chars().next().unwrap_or('?');
                let b = kids.next().and_then(Self::node);
                let (Some(a), Some(b)) = (a, b) else {
                    return Item::Text("");
                };
                Ast::Binary(op, a, b, span)
            }
            8 => return kids.nth(1).unwrap_or(Item::Text("")),
            9 => Ast::Number(Self::text(kids.next()).parse().unwrap_or(0.0), span),
            _ => Ast::Name(Self::text(kids.next()), span),
        };
        Item::Node(self.arena.alloc(ast))
    }
}

/// Lists every variable read, in source order.
struct Reads(Vec<String>);

impl Visitor<Ast> for Reads {
    fn enter(&mut self, _: &Arena<Ast>, _: Id<Ast>, node: &Ast) -> Flow {
        if let Ast::Name(name, _) = node {
            self.0.push(name.to_string());
        }
        Flow::Continue
    }
}

/// Evaluates the program, printing as `print` statements run.
fn run(arena: &Arena<Ast>, program: Id<Ast>) {
    use std::collections::HashMap;
    fn eval(arena: &Arena<Ast>, id: Id<Ast>, vars: &HashMap<Box<str>, f64>) -> f64 {
        match arena.get(id) {
            Some(Ast::Number(v, _)) => *v,
            Some(Ast::Name(n, _)) => vars.get(n).copied().unwrap_or(f64::NAN),
            Some(Ast::Binary(op, a, b, _)) => {
                let (a, b) = (eval(arena, *a, vars), eval(arena, *b, vars));
                match op {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    _ => a / b,
                }
            }
            _ => f64::NAN,
        }
    }
    let mut vars = HashMap::new();
    if let Some(Ast::Program(stmts, _)) = arena.get(program) {
        for &stmt in stmts {
            match arena.get(stmt) {
                Some(Ast::Let(name, value, _)) => {
                    let v = eval(arena, *value, &vars);
                    vars.insert(name.clone(), v);
                }
                Some(Ast::Print(value, span)) => {
                    println!("  print at {span}: {}", eval(arena, *value, &vars));
                }
                _ => {}
            }
        }
    }
}

const PROGRAM: &str = "
    // Compute a few things.
    let width = 12;
    let height = width / 4 + 1;
    print width * height;
    print (width - height) * 2;
";

fn main() {
    let parser = language();
    let mut arena = Arena::new();
    let stmts = match parser.parse_with(PROGRAM, &mut Lower { arena: &mut arena }) {
        Ok(Item::Stmts(stmts)) => stmts,
        Ok(_) => Vec::new(),
        Err(err) => {
            println!("parse error at {}: {err}", err.span());
            return;
        }
    };
    let program = arena.alloc(Ast::Program(stmts, Span::new(0, PROGRAM.len() as u32)));
    println!("{} AST nodes", arena.len());

    let mut reads = Reads(Vec::new());
    walk(&arena, program, &mut reads);
    println!("variables read: {:?}", reads.0);

    println!("running:");
    run(&arena, program);
}
