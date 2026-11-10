//! Expressions: the language of bound slots (vision doc 02 §4).
//!
//! An expression is data walked by a fixed interpreter. Its dependency set is
//! syntactic, which is what lets the reactive graph be built statically
//! (`graph.rs`). The surface syntax exists so a bound slot has a readable
//! form in files and views; parser and printer are inverses and tested so.
//!
//! ```text
//!   #3.slider.value * 300
//!   #3.slider.value > 0.5 ? 1 : 0.25
//!   clamp(#3.slider.value * 2, 0, 1)
//! ```
//!
//! Absent on purpose: calls that leave the process. Those arrive with the
//! cost-tagged invocation node. `Get` (`beam.waist`) reads a record's field.
//!
//! Series and functions (plans/specs/plot.md §1): floor functions and
//! operators broadcast over series, and `x => body` is a function value.
//! A lambda's parameters are locals (`Var`), never dependencies; its
//! references are dependencies of the binding that holds it, and
//! evaluating the lambda closes it over their current values (`Closure`).

use crate::facet::LeafPath;
use crate::id::NodeId;
use crate::value::Value;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// A leaf address: the unit of dependency.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Leaf {
    pub node: NodeId,
    pub path: LeafPath,
}

impl fmt::Display for Leaf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.node, self.path)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Lit(Value),
    /// A named constant (`pi`). Kept symbolic so it prints as written.
    Const(Constant),
    Ref(Leaf),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    /// A field of a record value: `expr.field` (doc 02 §4).
    Get(Box<Expr>, &'static str),
    /// A lambda's parameter, read inside its body.
    Var(&'static str),
    /// `x => body`, `(x, y) => body`: evaluates to a [`Closure`].
    Lambda(Vec<&'static str>, Box<Expr>),
    /// A function value called: `f(x)`, where `f` is a parameter or a leaf
    /// holding a function.
    Apply(Box<Expr>, Vec<Expr>),
    /// A colour token, `@series-2` (palette.rs): the document token of that
    /// name, in the theme in use. The graph resolves it to the token's
    /// `token.color` leaf when it builds and evaluates.
    Token(String),
}

thread_local! {
    /// The tokens `@name` reads, by name, while the graph evaluates.
    static TOKENS: std::cell::RefCell<BTreeMap<String, Leaf>> = const { std::cell::RefCell::new(BTreeMap::new()) };
}

/// The tokens formulas resolve while this lives (the graph's evaluation).
pub struct TokenScope(BTreeMap<String, Leaf>);

impl TokenScope {
    pub fn new(tokens: BTreeMap<String, Leaf>) -> TokenScope {
        TokenScope(TOKENS.with(|t| std::mem::replace(&mut *t.borrow_mut(), tokens)))
    }
}

impl Drop for TokenScope {
    fn drop(&mut self) {
        let previous = std::mem::take(&mut self.0);
        TOKENS.with(|t| *t.borrow_mut() = previous);
    }
}

/// A function value: parameters and a body with no references left in it
/// (each was replaced by its value when the lambda was evaluated), so
/// applying it reads nothing from the document.
#[derive(Clone, Debug, PartialEq)]
pub struct Closure {
    pub params: Vec<&'static str>,
    pub body: Expr,
}

/// Most samples one function call may produce.
pub const MAX_SAMPLES: usize = 100_000;

/// Most work one evaluation of a binding may do: elements produced and
/// function applications, summed over every call inside it (nested `map`s
/// multiply), so no document or agent edit can hang the engine.
pub const MAX_WORK: u64 = 5_000_000;

/// How deep functions may call functions (`(f => f(f))(f => f(f))` recurses
/// forever; the stack would overflow first).
pub const MAX_DEPTH: u32 = 64;

thread_local! {
    /// (evaluations in progress, apply depth, work done) for the outermost
    /// evaluation on this thread.
    static BUDGET: std::cell::Cell<(u32, u32, u64)> = const { std::cell::Cell::new((0, 0, 0)) };
}

/// Count `n` units of work against the current evaluation's budget.
pub(crate) fn charge(n: usize) -> Result<(), EvalError> {
    BUDGET.with(|b| {
        let (nest, depth, work) = b.get();
        let work = work.saturating_add(n as u64);
        b.set((nest, depth, work));
        if work > MAX_WORK { Err(EvalError::Type(format!("too much work: more than {MAX_WORK} elements or calls in one binding"))) } else { Ok(()) }
    })
}

impl Closure {
    pub fn apply(&self, args: &[Value]) -> Result<Value, EvalError> {
        if args.len() != self.params.len() {
            return Err(EvalError::Arity { func: self.to_string(), expected: self.params.len(), got: args.len() });
        }
        charge(1)?;
        let depth = BUDGET.with(|b| {
            let (nest, depth, work) = b.get();
            b.set((nest, depth + 1, work));
            depth + 1
        });
        let out = if depth > MAX_DEPTH {
            Err(EvalError::Type(format!("functions nested more than {MAX_DEPTH} deep (does one call itself?)")))
        } else {
            let env: Vec<(&'static str, Value)> = self.params.iter().copied().zip(args.iter().cloned()).collect();
            // Closed: the body reads no leaf.
            self.body.eval_in(&|_| None, &env)
        };
        BUDGET.with(|b| {
            let (nest, depth, work) = b.get();
            b.set((nest, depth.saturating_sub(1), work));
        });
        out
    }
}

impl fmt::Display for Closure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_params(&self.params, f)?;
        f.write_str(" => ")?;
        self.body.write(f, PREC_TERNARY)
    }
}

fn write_params(params: &[&'static str], f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match params {
        [one] => f.write_str(one),
        many => write!(f, "({})", many.join(", ")),
    }
}

/// Named numeric constants of the floor. One arm each, like functions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Constant {
    Pi,
}

impl Constant {
    pub const ALL: &'static [Constant] = &[Constant::Pi];

    pub fn name(self) -> &'static str {
        match self {
            Constant::Pi => "pi",
        }
    }

    pub fn from_name(s: &str) -> Option<Constant> {
        Constant::ALL.iter().copied().find(|c| c.name() == s)
    }

    pub fn value(self) -> f64 {
        match self {
            Constant::Pi => std::f64::consts::PI,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

impl BinOp {
    fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::And => "&&",
            BinOp::Or => "||",
        }
    }

    /// Binding power; higher binds tighter. All left-associative.
    fn precedence(self) -> u8 {
        match self {
            BinOp::Or => 1,
            BinOp::And => 2,
            BinOp::Eq | BinOp::Ne => 3,
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => 4,
            BinOp::Add | BinOp::Sub => 5,
            BinOp::Mul | BinOp::Div | BinOp::Mod => 6,
        }
    }
}

const PREC_TERNARY: u8 = 0;
const PREC_UNARY: u8 = 7;
/// Field access binds tightest: `-a.b` is `-(a.b)`.
const PREC_GET: u8 = 8;

impl Expr {
    /// Every leaf this expression reads. Static: no evaluation needed.
    pub fn deps(&self) -> BTreeSet<Leaf> {
        let mut out = BTreeSet::new();
        self.collect_deps(&mut out);
        out
    }

    /// The colour tokens (`@name`) this reads.
    pub fn tokens(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.walk(&mut |e| {
            if let Expr::Token(n) = e {
                out.push(n.clone());
            }
        });
        out
    }

    /// Visit this and every subexpression.
    pub fn walk(&self, f: &mut dyn FnMut(&Expr)) {
        f(self);
        match self {
            Expr::Neg(e) | Expr::Not(e) | Expr::Get(e, _) => e.walk(f),
            Expr::Bin(_, a, b) => {
                a.walk(f);
                b.walk(f);
            }
            Expr::If(c, a, b) => {
                c.walk(f);
                a.walk(f);
                b.walk(f);
            }
            Expr::Call(_, args) => args.iter().for_each(|a| a.walk(f)),
            Expr::Lambda(_, body) => body.walk(f),
            Expr::Apply(g, args) => {
                g.walk(f);
                args.iter().for_each(|a| a.walk(f));
            }
            Expr::Lit(_) | Expr::Const(_) | Expr::Ref(_) | Expr::Var(_) | Expr::Token(_) => {}
        }
    }

    fn collect_deps(&self, out: &mut BTreeSet<Leaf>) {
        match self {
            Expr::Lit(_) | Expr::Const(_) => {}
            Expr::Ref(l) => {
                out.insert(*l);
            }
            Expr::Neg(e) | Expr::Not(e) => e.collect_deps(out),
            Expr::Bin(_, a, b) => {
                a.collect_deps(out);
                b.collect_deps(out);
            }
            Expr::If(c, a, b) => {
                c.collect_deps(out);
                a.collect_deps(out);
                b.collect_deps(out);
            }
            Expr::Call(_, args) => {
                for a in args {
                    a.collect_deps(out);
                }
            }
            Expr::Get(e, _) => e.collect_deps(out),
            Expr::Var(_) | Expr::Token(_) => {}
            Expr::Lambda(_, body) => body.collect_deps(out),
            Expr::Apply(f, args) => {
                f.collect_deps(out);
                for a in args {
                    a.collect_deps(out);
                }
            }
        }
    }

    /// The same expression with every reference passed through `f`. Used
    /// when a template is copied: its references move to the copy.
    pub fn map_refs(&self, f: &dyn Fn(Leaf) -> Leaf) -> Expr {
        match self {
            // A node named as a value follows too (`list(#3, #4)` in a
            // template names each copy's own nodes).
            Expr::Lit(Value::Ref(n)) => Expr::Lit(Value::Ref(f(Leaf { node: *n, path: LeafPath::new(crate::facet::FacetKind::Name, "text").expect("declared") }).node)),
            Expr::Lit(_) | Expr::Const(_) => self.clone(),
            Expr::Ref(l) => Expr::Ref(f(*l)),
            Expr::Neg(e) => Expr::Neg(Box::new(e.map_refs(f))),
            Expr::Not(e) => Expr::Not(Box::new(e.map_refs(f))),
            Expr::Bin(op, a, b) => Expr::Bin(*op, Box::new(a.map_refs(f)), Box::new(b.map_refs(f))),
            Expr::If(c, a, b) => Expr::If(Box::new(c.map_refs(f)), Box::new(a.map_refs(f)), Box::new(b.map_refs(f))),
            Expr::Call(name, args) => Expr::Call(name.clone(), args.iter().map(|a| a.map_refs(f)).collect()),
            Expr::Get(e, field) => Expr::Get(Box::new(e.map_refs(f)), field),
            Expr::Var(_) | Expr::Token(_) => self.clone(),
            Expr::Lambda(ps, body) => Expr::Lambda(ps.clone(), Box::new(body.map_refs(f))),
            Expr::Apply(g, args) => Expr::Apply(Box::new(g.map_refs(f)), args.iter().map(|a| a.map_refs(f)).collect()),
        }
    }

    /// The same expression with each reference replaced by whatever `f`
    /// returns for it: another reference, or a literal (`copy` freezes a
    /// read it cannot keep).
    pub fn replace_refs(&self, f: &dyn Fn(Leaf) -> Expr) -> Expr {
        match self {
            Expr::Lit(_) | Expr::Const(_) => self.clone(),
            Expr::Ref(l) => f(*l),
            Expr::Neg(e) => Expr::Neg(Box::new(e.replace_refs(f))),
            Expr::Not(e) => Expr::Not(Box::new(e.replace_refs(f))),
            Expr::Bin(op, a, b) => Expr::Bin(*op, Box::new(a.replace_refs(f)), Box::new(b.replace_refs(f))),
            Expr::If(c, a, b) => Expr::If(Box::new(c.replace_refs(f)), Box::new(a.replace_refs(f)), Box::new(b.replace_refs(f))),
            Expr::Call(name, args) => Expr::Call(name.clone(), args.iter().map(|a| a.replace_refs(f)).collect()),
            Expr::Get(e, field) => Expr::Get(Box::new(e.replace_refs(f)), field),
            Expr::Var(_) | Expr::Token(_) => self.clone(),
            Expr::Lambda(ps, body) => Expr::Lambda(ps.clone(), Box::new(body.replace_refs(f))),
            Expr::Apply(g, args) => Expr::Apply(Box::new(g.replace_refs(f)), args.iter().map(|a| a.replace_refs(f)).collect()),
        }
    }

    /// This lambda body closed: every reference replaced by its value, and
    /// every outer local (not one of `params`) by its value in `env`.
    fn close(&self, read: &dyn Fn(Leaf) -> Option<Value>, env: &[(&'static str, Value)], params: &[&'static str]) -> Result<Expr, EvalError> {
        Ok(match self {
            Expr::Lit(_) | Expr::Const(_) => self.clone(),
            Expr::Ref(l) => Expr::Lit(read(*l).ok_or(EvalError::Unresolved(*l))?),
            Expr::Var(name) if params.contains(name) => self.clone(),
            Expr::Var(name) => Expr::Lit(lookup(env, name)?),
            Expr::Token(_) => Expr::Lit(self.eval_in(read, env)?),
            Expr::Neg(e) => Expr::Neg(Box::new(e.close(read, env, params)?)),
            Expr::Not(e) => Expr::Not(Box::new(e.close(read, env, params)?)),
            Expr::Bin(op, a, b) => Expr::Bin(*op, Box::new(a.close(read, env, params)?), Box::new(b.close(read, env, params)?)),
            Expr::If(c, a, b) => Expr::If(Box::new(c.close(read, env, params)?), Box::new(a.close(read, env, params)?), Box::new(b.close(read, env, params)?)),
            Expr::Call(name, args) => Expr::Call(name.clone(), args.iter().map(|a| a.close(read, env, params)).collect::<Result<_, _>>()?),
            Expr::Get(e, field) => Expr::Get(Box::new(e.close(read, env, params)?), field),
            Expr::Lambda(inner, body) => {
                let all: Vec<&'static str> = params.iter().chain(inner.iter()).copied().collect();
                Expr::Lambda(inner.clone(), Box::new(body.close(read, env, &all)?))
            }
            Expr::Apply(g, args) => Expr::Apply(Box::new(g.close(read, env, params)?), args.iter().map(|a| a.close(read, env, params)).collect::<Result<_, _>>()?),
        })
    }

    /// Evaluate against a resolver for leaf values. Reads go through the
    /// resolver and nowhere else, which is the hook a tracking evaluator
    /// would use later.
    pub fn eval(&self, read: &dyn Fn(Leaf) -> Option<Value>) -> Result<Value, EvalError> {
        // The outermost evaluation starts a fresh budget.
        BUDGET.with(|b| {
            let (nest, depth, work) = b.get();
            b.set(if nest == 0 { (1, 0, 0) } else { (nest + 1, depth, work) });
        });
        let out = self.eval_in(read, &[]);
        BUDGET.with(|b| {
            let (nest, depth, work) = b.get();
            b.set((nest.saturating_sub(1), depth, work));
        });
        out
    }

    /// Evaluate with the locals `env` in scope (innermost last).
    fn eval_in(&self, read: &dyn Fn(Leaf) -> Option<Value>, env: &[(&'static str, Value)]) -> Result<Value, EvalError> {
        match self {
            Expr::Lit(v) => Ok(v.clone()),
            Expr::Const(c) => Ok(Value::Number(c.value())),
            Expr::Ref(l) => read(*l).ok_or(EvalError::Unresolved(*l)),
            Expr::Var(name) => lookup(env, name),
            Expr::Token(name) => {
                let leaf = TOKENS.with(|t| t.borrow().get(name).copied()).ok_or_else(|| EvalError::Type(format!("nothing provides @{name}")))?;
                read(leaf).ok_or(EvalError::Unresolved(leaf))
            }
            Expr::Lambda(params, body) => Ok(Value::Fn(std::sync::Arc::new(Closure { params: params.clone(), body: body.close(read, env, params)? }))),
            Expr::Apply(f, args) => {
                let f = f.eval_in(read, env)?;
                let vals: Vec<Value> = args.iter().map(|a| a.eval_in(read, env)).collect::<Result<_, _>>()?;
                match f {
                    Value::Fn(c) => c.apply(&vals),
                    v => Err(EvalError::Type(format!("cannot call {}; only a function (x => …) can be called", v.kind().name()))),
                }
            }
            Expr::Neg(e) => match e.eval_in(read, env)? {
                Value::Number(n) => Ok(Value::Number(-n)),
                Value::Quantity { si, dim } => Ok(Value::Quantity { si: -si, dim }),
                Value::Series { data, dim } => Ok(Value::series(data.iter().map(|x| -x).collect::<Vec<_>>(), dim)),
                v => Err(EvalError::Type(format!("cannot negate {}", v.kind().name()))),
            },
            Expr::Not(e) => match e.eval_in(read, env)? {
                Value::Bool(b) => Ok(Value::Bool(!b)),
                v => Err(EvalError::Type(format!("cannot negate {}", v.kind().name()))),
            },
            Expr::Bin(op, a, b) => {
                let a = a.eval_in(read, env)?;
                let b = b.eval_in(read, env)?;
                binary(*op, a, b)
            }
            Expr::If(c, a, b) => match c.eval_in(read, env)? {
                Value::Bool(true) => a.eval_in(read, env),
                Value::Bool(false) => b.eval_in(read, env),
                Value::Series { .. } => Err(EvalError::Type("a condition over a series is not one bool; use where(cond, a, b) for each element".into())),
                v => Err(EvalError::Type(format!("condition is {}, not bool", v.kind().name()))),
            },
            Expr::Call(name, args) => {
                let vals: Result<Vec<Value>, EvalError> = args.iter().map(|a| a.eval_in(read, env)).collect();
                let vals = vals?;
                if name == "deref" {
                    return deref(&vals, read);
                }
                floor::call(name, &vals)
            }
            Expr::Get(e, field) => match e.eval_in(read, env)? {
                Value::Record { kind, fields } => fields.get(field).cloned().ok_or_else(|| {
                    let names: Vec<&str> = fields.keys().copied().collect();
                    EvalError::Type(format!("{kind} has no field {field:?}; fields: {}", names.join(", ")))
                }),
                v => Err(EvalError::Type(format!("cannot read field {field:?} of {}", v.kind().name()))),
            },
        }
    }

    pub fn parse(text: &str) -> Result<Expr, ParseError> {
        let tokens = lex(text)?;
        let mut p = Parser { tokens, pos: 0, scope: Vec::new() };
        let e = p.expr(PREC_TERNARY)?;
        if p.pos < p.tokens.len() {
            return Err(ParseError(format!("unexpected {:?} after expression", p.tokens[p.pos])));
        }
        Ok(e)
    }

    fn write(&self, f: &mut fmt::Formatter<'_>, parent: u8) -> fmt::Result {
        match self {
            Expr::Lit(v) => write_lit(v, f),
            Expr::Const(c) => f.write_str(c.name()),
            Expr::Ref(l) => write!(f, "{l}"),
            Expr::Neg(e) => {
                f.write_str("-")?;
                e.write(f, PREC_UNARY)
            }
            Expr::Not(e) => {
                f.write_str("!")?;
                e.write(f, PREC_UNARY)
            }
            Expr::Bin(op, a, b) => {
                let p = op.precedence();
                let paren = p < parent;
                if paren {
                    f.write_str("(")?;
                }
                a.write(f, p)?;
                write!(f, " {} ", op.symbol())?;
                // Left-associative: the right operand needs parens at equal precedence.
                b.write(f, p + 1)?;
                if paren {
                    f.write_str(")")?;
                }
                Ok(())
            }
            Expr::If(c, a, b) => {
                let paren = PREC_TERNARY < parent;
                if paren {
                    f.write_str("(")?;
                }
                c.write(f, PREC_TERNARY + 1)?;
                f.write_str(" ? ")?;
                a.write(f, PREC_TERNARY)?;
                f.write_str(" : ")?;
                b.write(f, PREC_TERNARY)?;
                if paren {
                    f.write_str(")")?;
                }
                Ok(())
            }
            Expr::Call(name, args) => {
                write!(f, "{name}(")?;
                // record(name: value, …) prints as it is written.
                let named = name == "record" && args.len() % 2 == 0 && args.chunks(2).all(|p| matches!(&p[0], Expr::Lit(Value::Str(n)) if is_name(n)));
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(if named && i % 2 == 1 { ": " } else { ", " })?;
                    }
                    match a {
                        Expr::Lit(Value::Str(n)) if named && i % 2 == 0 => f.write_str(n)?,
                        a => a.write(f, PREC_TERNARY)?,
                    }
                }
                f.write_str(")")
            }
            Expr::Get(e, field) => {
                e.write(f, PREC_GET)?;
                write!(f, ".{field}")
            }
            Expr::Var(name) => f.write_str(name),
            Expr::Token(name) => write!(f, "@{name}"),
            Expr::Lambda(params, body) => {
                let paren = PREC_TERNARY < parent;
                if paren {
                    f.write_str("(")?;
                }
                write_params(params, f)?;
                f.write_str(" => ")?;
                body.write(f, PREC_TERNARY)?;
                if paren {
                    f.write_str(")")?;
                }
                Ok(())
            }
            Expr::Apply(g, args) => {
                g.write(f, PREC_GET)?;
                f.write_str("(")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    a.write(f, PREC_TERNARY)?;
                }
                f.write_str(")")
            }
        }
    }
}

/// `deref(node, 'facet.field')`: a leaf of whichever node the first
/// argument names now (plans/specs/reactivity.md §2): a *tracked read*,
/// the one read whose target is a value. The graph records what it read
/// (`graph.rs`), so the binding follows both the reference and the leaf.
/// `deref(node, 'facet.field', fallback)` gives the fallback when there is
/// no such node or leaf.
fn deref(args: &[Value], read: &dyn Fn(Leaf) -> Option<Value>) -> Result<Value, EvalError> {
    let (target, path, fallback) = match args {
        [t, p] => (t, p, None),
        [t, p, f] => (t, p, Some(f)),
        _ => return Err(EvalError::Arity { func: "deref".to_owned(), expected: 2, got: args.len() }),
    };
    let path = path.as_str().ok_or_else(|| EvalError::Type(format!("deref: the leaf is text, 'size.w'; got {}", path.kind().name())))?;
    let path: LeafPath = path.parse().map_err(|e| EvalError::Type(format!("deref: {e}")))?;
    let node = match (target, fallback) {
        (Value::Ref(n), _) => *n,
        (_, Some(f)) => return Ok(f.clone()),
        (v, None) => return Err(EvalError::Type(format!("deref takes a node reference, got {}", v.kind().name()))),
    };
    let leaf = Leaf { node, path };
    match (read(leaf), fallback) {
        (Some(v), _) => Ok(v),
        (None, Some(f)) => Ok(f.clone()),
        (None, None) => Err(EvalError::Unresolved(leaf)),
    }
}

fn lookup(env: &[(&'static str, Value)], name: &str) -> Result<Value, EvalError> {
    env.iter().rev().find(|(n, _)| *n == name).map(|(_, v)| v.clone()).ok_or_else(|| EvalError::Type(format!("{name} is not defined here")))
}

/// Whether `s` can be written as a bare field name.
fn is_name(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn write_lit(v: &Value, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match v {
        Value::List(items) => {
            f.write_str("list(")?;
            for (i, v) in items.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write_lit(v, f)?;
            }
            f.write_str(")")
        }
        Value::Record { kind, fields } if *kind == "record" && fields.keys().all(|k| is_name(k)) => {
            f.write_str("record(")?;
            for (i, (k, v)) in fields.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{k}: ")?;
                write_lit(v, f)?;
            }
            f.write_str(")")
        }
        // Single quotes where they can hold the text: formulas sit inside
        // double-quoted attributes in the model's markup.
        Value::Str(s) | Value::Enum(s) if !s.contains(['\'', '\\', '\n', '\t']) => write!(f, "'{s}'"),
        Value::Str(s) | Value::Enum(s) => write!(f, "{:?}", s),
        Value::Vec2([a, b]) => write!(f, "[{}, {}]", crate::value::fmt_num(*a), crate::value::fmt_num(*b)),
        Value::Vec3([a, b, c]) => write!(
            f,
            "[{}, {}, {}]",
            crate::value::fmt_num(*a),
            crate::value::fmt_num(*b),
            crate::value::fmt_num(*c)
        ),
        Value::Color([r, g, b, a]) => write!(f, "rgba({r}, {g}, {b}, {a})"),
        Value::Ref(id) => write!(f, "{id}"),
        // The exact magnitude in the canonical unit, so parse(print(q)) == q.
        Value::Quantity { si, dim } => write!(f, "{} {}", crate::value::fmt_num(*si), dim.canonical_unit()),
        // Exact SI magnitudes, so parse(print(s)) == s: `series(0.1, 0.2)`,
        // or `(series(4e-7, 5e-7) * 1 m)` with a unit.
        Value::Series { data, dim } => {
            let items: Vec<String> = data.iter().map(|x| crate::value::fmt_num(*x)).collect();
            if dim.is_none() { write!(f, "series({})", items.join(", ")) } else { write!(f, "(series({}) * 1 {})", items.join(", "), dim.canonical_unit()) }
        }
        Value::Fn(c) => write!(f, "({c})"),
        other => write!(f, "{other}"),
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write(f, PREC_TERNARY)
    }
}

impl Serialize for Expr {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Expr {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Expr::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EvalError {
    Unresolved(Leaf),
    Type(String),
    UnknownFunction(String),
    Arity { func: String, expected: usize, got: usize },
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::Unresolved(l) => write!(f, "{l} has no value"),
            EvalError::Type(m) => f.write_str(m),
            EvalError::UnknownFunction(n) => write!(f, "unknown function {n:?}; available: {}", floor::NAMES.join(", ")),
            EvalError::Arity { func, expected, got } => write!(f, "{func} takes {expected} arguments, got {got}"),
        }
    }
}
impl std::error::Error for EvalError {}

/// `a <op> b` on two numbers or quantities, dimension-checked: `+ - %` and
/// comparisons need equal dimensions (a plain number is dimensionless, so
/// `9.58 mW + 2` is an error and `9.58 mW * 2` is not); `*` and `/` combine
/// them, and a result whose dimension cancels is a plain number.
fn quantity_op(op: BinOp, a: &Value, b: &Value) -> Option<Result<Value, EvalError>> {
    let (x, dx) = a.as_quantity()?;
    let (y, dy) = b.as_quantity()?;
    let same = || {
        if dx == dy {
            Ok(())
        } else {
            Err(EvalError::Type(format!("cannot combine {} {} {}", a.describe(), op.symbol(), b.describe())))
        }
    };
    Some(match op {
        BinOp::Add => same().map(|_| Value::quantity(x + y, dx)),
        BinOp::Sub => same().map(|_| Value::quantity(x - y, dx)),
        BinOp::Mod => same().map(|_| Value::quantity(x % y, dx)),
        BinOp::Mul => Ok(Value::quantity(x * y, dx.mul(dy))),
        BinOp::Div => Ok(Value::quantity(x / y, dx.div(dy))),
        BinOp::Lt => same().map(|_| Value::Bool(x < y)),
        BinOp::Le => same().map(|_| Value::Bool(x <= y)),
        BinOp::Gt => same().map(|_| Value::Bool(x > y)),
        BinOp::Ge => same().map(|_| Value::Bool(x >= y)),
        _ => return None,
    })
}

fn binary(op: BinOp, a: Value, b: Value) -> Result<Value, EvalError> {
    use Value::*;
    let num = |x: &Value| x.as_f64();
    if matches!(a, Series { .. }) || matches!(b, Series { .. }) {
        if let Some(r) = crate::series::binary(op, &a, &b) {
            return r;
        }
    }
    if let Some(r) = quantity_op(op, &a, &b) {
        return r;
    }
    // Text joins: a choice's value joins like text (`'mode: ' + $self.style`).
    if op == BinOp::Add
        && let (Str(_) | Enum(_), Str(_) | Enum(_)) = (&a, &b)
        && (matches!(a, Str(_)) || matches!(b, Str(_)))
    {
        return Ok(Str(format!("{}{}", a.as_str().expect("text"), b.as_str().expect("text"))));
    }
    Ok(match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => match (&a, &b) {
            (Vec2(x), Vec2(y)) if matches!(op, BinOp::Add | BinOp::Sub) => {
                let s = if op == BinOp::Add { 1.0 } else { -1.0 };
                Vec2([x[0] + s * y[0], x[1] + s * y[1]])
            }
            (Vec3(x), Vec3(y)) if matches!(op, BinOp::Add | BinOp::Sub) => {
                let s = if op == BinOp::Add { 1.0 } else { -1.0 };
                Vec3([x[0] + s * y[0], x[1] + s * y[1], x[2] + s * y[2]])
            }
            (Vec2(x), Number(k)) | (Number(k), Vec2(x)) if op == BinOp::Mul => Vec2([x[0] * k, x[1] * k]),
            (Vec3(x), Number(k)) | (Number(k), Vec3(x)) if op == BinOp::Mul => Vec3([x[0] * k, x[1] * k, x[2] * k]),
            (Str(x), Str(y)) if op == BinOp::Add => Str(format!("{x}{y}")),
            _ => return Err(EvalError::Type(format!("{} {} {}", a.kind().name(), op.symbol(), b.kind().name()))),
        },
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => match (num(&a), num(&b)) {
            (Some(x), Some(y)) => Bool(match op {
                BinOp::Lt => x < y,
                BinOp::Le => x <= y,
                BinOp::Gt => x > y,
                _ => x >= y,
            }),
            _ => return Err(EvalError::Type(format!("{} {} {}", a.kind().name(), op.symbol(), b.kind().name()))),
        },
        // A choice equals text naming it (`$self.style == 'line'`).
        BinOp::Eq => Bool(same(&a, &b)),
        BinOp::Ne => Bool(!same(&a, &b)),
        BinOp::And | BinOp::Or => match (&a, &b) {
            (Bool(x), Bool(y)) => Bool(if op == BinOp::And { *x && *y } else { *x || *y }),
            _ => return Err(EvalError::Type(format!("{} {} {}", a.kind().name(), op.symbol(), b.kind().name()))),
        },
    })
}

/// Equality, with a choice and text equal when they name the same word.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Str(x) | Value::Enum(x), Value::Str(y) | Value::Enum(y)) => x == y,
        _ => a == b,
    }
}

/// A number as text: fixed `digits` after the point, or, without, up to
/// twelve significant digits with trailing zeros dropped.
pub fn format_number(n: f64, digits: Option<usize>) -> String {
    if !n.is_finite() {
        return n.to_string();
    }
    match digits {
        Some(d) => format!("{:.*}", d.min(15), n),
        None => {
            let s = format!("{:.*}", 12usize.saturating_sub(n.abs().log10().max(0.0) as usize), n);
            let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s };
            if s == "-0" { "0".to_owned() } else { s }
        }
    }
}

fn fmt(args: &[Value]) -> Result<String, EvalError> {
    // A unit written '…' lexes as an enum; it means the same as "…".
    let owned: Vec<Value> = args.iter().map(|v| match v { Value::Enum(s) => Value::Str(s.clone()), v => v.clone() }).collect();
    let args = &owned[..];
    if let Some(Value::Series { data, dim }) = args.first() {
        let shown: Vec<String> = data.iter().take(crate::value::SERIES_SHOWN).map(|&x| {
            let mut one = vec![Value::quantity(x, *dim)];
            one.extend(args[1..].iter().cloned());
            fmt(&one)
        }).collect::<Result<_, _>>()?;
        let more = if data.len() > crate::value::SERIES_SHOWN { ", …" } else { "" };
        return Ok(format!("{}{more}", shown.join(", ")));
    }
    let bad = |m: String| EvalError::Type(format!("fmt: {m}"));
    let digits = |v: &Value| v.as_f64().filter(|d| d.is_finite()).map(|d| d.clamp(0.0, 15.0) as usize).ok_or_else(|| bad(format!("digits must be a number, got {}", v.kind().name())));
    match args {
        [v] => Ok(match v {
            Value::Quantity { si, dim } => crate::units::format(*si, *dim),
            other => crate::text::display(other),
        }),
        [v, Value::Str(unit)] | [v, Value::Str(unit), _] => {
            let d = match args.get(2) { Some(d) => Some(digits(d)?), None => None };
            let u = crate::units::parse_unit(unit).map_err(|_| bad(format!("unknown unit {unit:?}")))?;
            let (si, dim) = v.as_quantity().ok_or_else(|| bad(format!("takes a number, got {}", v.kind().name())))?;
            if dim != u.dim {
                return Err(bad(format!("{} is not in {unit}", if dim.is_none() { "a plain number".to_owned() } else { dim.canonical_unit() })));
            }
            Ok(format!("{} {unit}", format_number(crate::units::convert(si, u), d)))
        }
        [v, d] => {
            let d = digits(d)?;
            Ok(match v.as_quantity() {
                Some((si, dim)) if !dim.is_none() => format!("{} {}", format_number(si, Some(d)), dim.canonical_unit()),
                Some((x, _)) => format_number(x, Some(d)),
                None => return Err(bad(format!("takes a number, got {}", v.kind().name()))),
            })
        }
        _ => Err(EvalError::Arity { func: "fmt".to_owned(), expected: 2, got: args.len() }),
    }
}

/// The floor: pure, total, bounded functions available to expressions
/// (doc 02 §6). Each is one arm; adding one is a Rust change by design.
pub mod floor {
    use super::{EvalError, Value};
    use crate::units::Dim;

    pub const NAMES: &[&str] = &[
        "abs", "floor", "ceil", "round", "min", "max", "clamp", "lerp", "scale", "sin", "cos", "tan", "atan2", "sqrt",
        "pow", "hypot", "log10", "ln", "exp", "sinc", "fmt", "sci", "str", "series", "range", "step", "len", "at", "first",
        "last", "sum", "mean", "interp", "ticks", "nice_lo", "nice_hi", "where", "map", "sample", "rgb", "hsl", "mix",
        "spectrum", "points", "smooth", "circles", "segments", "bars", "area", "isnan", "kind", "concat", "unit_of", "in_unit", "sig",
        "ease", "window", "list", "record", "get", "filter", "sort_by", "zip", "enumerate", "unique", "deref",
    ];

    /// The scalar arms of [`call`], for the series module to apply per element.
    pub fn scalar(func: &str, args: &[Value]) -> Result<Value, EvalError> {
        call_scalar(func, args)
    }

    /// Magnitudes and dimensions of `n` number-or-quantity arguments.
    fn args_of(func: &str, args: &[Value], n: usize) -> Result<Vec<(f64, Dim)>, EvalError> {
        if args.len() != n {
            return Err(EvalError::Arity { func: func.to_owned(), expected: n, got: args.len() });
        }
        args.iter()
            .map(|v| v.as_quantity().ok_or_else(|| EvalError::Type(format!("{func} takes numbers, got {}", v.kind().name()))))
            .collect()
    }

    /// Every listed argument must share one dimension; returns it.
    fn same(func: &str, a: &[(f64, Dim)], which: &[usize]) -> Result<Dim, EvalError> {
        let d = a[which[0]].1;
        for &i in which {
            if a[i].1 != d {
                return Err(EvalError::Type(format!(
                    "{func}: arguments must share a dimension, got {} and {}",
                    describe(d),
                    describe(a[i].1)
                )));
            }
        }
        Ok(d)
    }

    fn plain(func: &str, a: &[(f64, Dim)], which: &[usize]) -> Result<(), EvalError> {
        for &i in which {
            if !a[i].1.is_none() {
                return Err(EvalError::Type(format!("{func}: argument {} must be a plain number, got {}", i + 1, describe(a[i].1))));
            }
        }
        Ok(())
    }

    fn describe(d: Dim) -> String {
        if d.is_none() { "a plain number".to_owned() } else { d.canonical_unit() }
    }

    pub fn call(func: &str, args: &[Value]) -> Result<Value, EvalError> {
        // Series, functions, colours and path data (plot.md §1–§2), and
        // broadcasting the scalar functions below over series.
        if let Some(r) = crate::list::call(func, args) {
            return r;
        }
        if let Some(r) = crate::series::call(func, args) {
            return r;
        }
        // Text from values (plans/specs/text.md §6): readouts bind a text
        // node's content to these, so formatting is visible in the binding.
        match func {
            "str" => {
                let [v] = args else { return Err(EvalError::Arity { func: func.to_owned(), expected: 1, got: args.len() }) };
                return Ok(Value::Str(crate::text::display(v)));
            }
            // fmt(x), fmt(x, digits), fmt(q, "unit"), fmt(q, "unit", digits)
            "fmt" => return super::fmt(args).map(Value::Str),
            // sci(x, digits): scientific notation, `digits` after the point.
            "sci" => {
                let a = args_of(func, args, 2)?;
                let digits = a[1].0.clamp(0.0, 15.0) as usize;
                let unit = if a[0].1.is_none() { String::new() } else { format!(" {}", a[0].1.canonical_unit()) };
                return Ok(Value::Str(format!("{:.*e}{unit}", digits, a[0].0)));
            }
            _ => {}
        }
        call_scalar(func, args)
    }

    fn call_scalar(func: &str, args: &[Value]) -> Result<Value, EvalError> {
        let n = |k| args_of(func, args, k);
        let q = Value::quantity;
        Ok(match func {
            // sig(x, n): x rounded to n significant figures, unit kept.
            "sig" => {
                let a = n(2)?;
                plain(func, &a, &[1])?;
                let (x, digits) = (a[0].0, a[1].0.clamp(1.0, 15.0).round() as i32);
                let r = if x == 0.0 || !x.is_finite() {
                    x
                } else {
                    let k = 10f64.powi(digits - 1 - x.abs().log10().floor() as i32);
                    crate::units::round12((x * k).round() / k)
                };
                q(r, a[0].1)
            }
            "log10" | "ln" | "exp" | "sinc" => {
                let a = n(1)?;
                plain(func, &a, &[0])?;
                let x = a[0].0;
                Value::Number(match func {
                    "log10" => x.log10(),
                    "ln" => x.ln(),
                    "exp" => x.exp(),
                    _ => if x == 0.0 { 1.0 } else { x.sin() / x },
                })
            }
            "abs" => {
                let a = n(1)?;
                q(a[0].0.abs(), a[0].1)
            }
            "floor" => {
                let a = n(1)?;
                q(a[0].0.floor(), a[0].1)
            }
            "ceil" => {
                let a = n(1)?;
                q(a[0].0.ceil(), a[0].1)
            }
            "round" => {
                let a = n(1)?;
                q(a[0].0.round(), a[0].1)
            }
            "min" => {
                let a = n(2)?;
                q(a[0].0.min(a[1].0), same(func, &a, &[0, 1])?)
            }
            "max" => {
                let a = n(2)?;
                q(a[0].0.max(a[1].0), same(func, &a, &[0, 1])?)
            }
            "clamp" => {
                let a = n(3)?;
                q(a[0].0.clamp(a[1].0.min(a[2].0), a[2].0.max(a[1].0)), same(func, &a, &[0, 1, 2])?)
            }
            "lerp" => {
                let a = n(3)?;
                plain(func, &a, &[2])?;
                q(a[0].0 + (a[1].0 - a[0].0) * a[2].0, same(func, &a, &[0, 1])?)
            }
            // scale(x, d0, d1, r0, r1): linear map of x from [d0, d1] to [r0, r1].
            "scale" => {
                let a = n(5)?;
                same(func, &a, &[0, 1, 2])?;
                let t = if a[2].0 == a[1].0 { 0.0 } else { (a[0].0 - a[1].0) / (a[2].0 - a[1].0) };
                q(a[3].0 + (a[4].0 - a[3].0) * t, same(func, &a, &[3, 4])?)
            }
            // Trigonometry in radians, matching `transform2d.rot`.
            "sin" | "cos" | "tan" => {
                let a = n(1)?;
                plain(func, &a, &[0])?;
                let x = a[0].0;
                Value::Number(match func {
                    "sin" => x.sin(),
                    "cos" => x.cos(),
                    _ => x.tan(),
                })
            }
            // atan2(y, x), the usual argument order; any shared dimension.
            "atan2" => {
                let a = n(2)?;
                same(func, &a, &[0, 1])?;
                Value::Number(a[0].0.atan2(a[1].0))
            }
            "sqrt" => {
                let a = n(1)?;
                let d = a[0].1.sqrt().ok_or_else(|| EvalError::Type(format!("sqrt of {} has no unit", describe(a[0].1))))?;
                q(a[0].0.sqrt(), d)
            }
            "pow" => {
                let a = n(2)?;
                plain(func, &a, &[1])?;
                let (x, dx) = a[0];
                let e = a[1].0;
                if dx.is_none() {
                    Value::Number(x.powf(e))
                } else if e.fract() == 0.0 && e.abs() <= 127.0 {
                    q(x.powf(e), dx.pow(e as i8))
                } else {
                    return Err(EvalError::Type(format!("pow of {} needs an integer exponent, got {e}", describe(dx))));
                }
            }
            "hypot" => {
                let a = n(2)?;
                q(a[0].0.hypot(a[1].0), same(func, &a, &[0, 1])?)
            }
            _ => return Err(EvalError::UnknownFunction(func.to_owned())),
        })
    }
}

// ---------------------------------------------------------------------------
// Surface syntax
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot parse expression: {}", self.0)
    }
}
impl std::error::Error for ParseError {}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    /// A quantity literal (a number followed by a unit), already in SI.
    Lit(Value),
    Str(String),
    Token(String),
    Ident(String),
    Ref(Leaf),
    Op(&'static str),
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Question,
    Colon,
    /// `.` before an identifier: field access.
    Dot,
}

fn lex(text: &str) -> Result<Vec<Tok>, ParseError> {
    let b = text.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        match c {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'.' if b.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_') => {
                out.push(Tok::Dot);
                i += 1;
            }
            b'0'..=b'9' | b'.' => {
                let start = i;
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.' || b[i] == b'e' || b[i] == b'E') {
                    i += 1;
                }
                let s = &text[start..i];
                let n: f64 = s.parse().map_err(|_| ParseError(format!("bad number {s:?}")))?;
                // A unit after a number makes a quantity literal: `9.58 mW`,
                // `3 kg*m2/s3`. The longest run of unit characters that
                // parses as a unit is taken; otherwise the number stands
                // alone and whatever follows lexes as usual.
                let mut j = i;
                while j < b.len() && (b[j] == b' ' || b[j] == b'\t') {
                    j += 1;
                }
                let unit_start = j;
                let mut k = unit_start;
                while k < b.len() {
                    let c = text[k..].chars().next().expect("in bounds");
                    if c.is_alphanumeric() || matches!(c, '*' | '·' | '/' | '^' | 'µ' | 'μ' | '°' | 'Ω') || (c == '-' && k > unit_start) {
                        k += c.len_utf8();
                    } else {
                        break;
                    }
                }
                let mut matched = None;
                let mut end = k;
                while end > unit_start {
                    let cand = &text[unit_start..end];
                    if !cand.starts_with(|c: char| c.is_alphabetic() || matches!(c, 'µ' | 'μ' | '°' | 'Ω')) {
                        break;
                    }
                    if !text[end..].trim_start().starts_with('(')
                        && let Ok(u) = crate::units::parse_unit(cand)
                        && !(u.dim.is_none() && u.factor == 1.0 && Constant::from_name(cand).is_some())
                    {
                        matched = Some(u);
                        break;
                    }
                    // Shorten at the previous separator or character boundary.
                    end = cand[..cand.len() - cand.chars().last().map_or(1, char::len_utf8)]
                        .rfind(|c: char| !(c.is_alphanumeric() || matches!(c, 'µ' | 'μ' | '°' | 'Ω')))
                        .map(|p| unit_start + p)
                        .unwrap_or(unit_start);
                    if end == unit_start && cand.len() > 1 {
                        // Last try: the leading identifier alone.
                        let ident_len = cand.find(|c: char| !(c.is_alphabetic() || matches!(c, 'µ' | 'μ' | '°' | 'Ω'))).unwrap_or(cand.len());
                        if ident_len > 0 && ident_len < cand.len() {
                            end = unit_start + ident_len;
                            continue;
                        }
                    }
                }
                match matched {
                    Some(u) => {
                        i = end;
                        out.push(Tok::Lit(Value::quantity(n * u.factor, u.dim)));
                    }
                    None => out.push(Tok::Num(n)),
                }
            }
            b'"' => {
                let mut s = String::new();
                i += 1;
                loop {
                    let Some(&ch) = b.get(i) else { return Err(ParseError("unterminated string".into())) };
                    i += 1;
                    match ch {
                        b'"' => break,
                        b'\\' => {
                            let Some(&esc) = b.get(i) else { return Err(ParseError("bad escape".into())) };
                            i += 1;
                            s.push(match esc {
                                b'n' => '\n',
                                b't' => '\t',
                                other => other as char,
                            });
                        }
                        other => s.push(other as char),
                    }
                }
                out.push(Tok::Str(s));
            }
            b'\'' => {
                let start = i + 1;
                i += 1;
                while i < b.len() && b[i] != b'\'' {
                    i += 1;
                }
                if i >= b.len() {
                    return Err(ParseError("unterminated text literal".into()));
                }
                // Either quote is text (an edit's attributes are in double
                // quotes, so formulas there write single ones). A choice leaf
                // takes text naming one of its options (graph.rs).
                out.push(Tok::Str(text[start..i].to_owned()));
                i += 1;
            }
            // `@series-2`: a colour token's name, letters, digits, `-`, `_`.
            b'@' => {
                let start = i + 1;
                i += 1;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-' || b[i] == b'_') {
                    i += 1;
                }
                if i == start {
                    return Err(ParseError("@ needs a token name: @series-2".into()));
                }
                out.push(Tok::Token(text[start..i].to_owned()));
            }
            b'#' => {
                // #<id>.<facet>.<field>; an expanded id is `#<owner>/<local>`.
                let start = i + 1;
                i += 1;
                while i < b.len() && b[i].is_ascii_alphanumeric() {
                    i += 1;
                }
                // Each further step: a definition-local id, or `~key`, an
                // item of a repeat (letters, digits, `_` and `-`).
                while i + 1 < b.len() && b[i] == b'/' && (b[i + 1].is_ascii_alphanumeric() || b[i + 1] == b'~') {
                    let item = b[i + 1] == b'~';
                    i += 2;
                    while i < b.len() && (b[i].is_ascii_alphanumeric() || (item && matches!(b[i], b'_' | b'-'))) {
                        i += 1;
                    }
                }
                let node: NodeId =
                    text[start..i].parse().map_err(|_| ParseError(format!("bad node id at {start}")))?;
                let mut parts = Vec::new();
                while i < b.len() && b[i] == b'.' && parts.len() < 2 {
                    i += 1;
                    let s = i;
                    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                        i += 1;
                    }
                    parts.push(&text[s..i]);
                }
                // A bare id is the node itself, a reference value:
                // `list(#3, #4)`, `deref(#3, 'size.w')`.
                if parts.is_empty() {
                    out.push(Tok::Lit(Value::Ref(node)));
                    continue;
                }
                if parts.len() != 2 {
                    return Err(ParseError(format!("reference {node} needs .component.field, or nothing after it to mean the node")));
                }
                let path: LeafPath =
                    format!("{}.{}", parts[0], parts[1]).parse().map_err(|e| ParseError(format!("{e}")))?;
                out.push(Tok::Ref(Leaf { node, path }));
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let start = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                out.push(Tok::Ident(text[start..i].to_owned()));
            }
            b'(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            b')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            b'[' => {
                out.push(Tok::LBracket);
                i += 1;
            }
            b']' => {
                out.push(Tok::RBracket);
                i += 1;
            }
            b',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            b'?' => {
                out.push(Tok::Question);
                i += 1;
            }
            b':' => {
                out.push(Tok::Colon);
                i += 1;
            }
            _ => {
                let two = text.get(i..i + 2).unwrap_or("");
                let op: &'static str = match two {
                    "<=" => "<=",
                    ">=" => ">=",
                    "==" => "==",
                    "!=" => "!=",
                    "&&" => "&&",
                    "||" => "||",
                    "=>" => "=>",
                    _ => match c {
                        b'+' => "+",
                        b'-' => "-",
                        b'*' => "*",
                        b'/' => "/",
                        b'%' => "%",
                        b'<' => "<",
                        b'>' => ">",
                        b'!' => "!",
                        other => return Err(ParseError(format!("unexpected {:?} at {i}", other as char))),
                    },
                };
                i += op.len();
                out.push(Tok::Op(op));
            }
        }
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<Tok>,
    pos: usize,
    /// The lambda parameters in scope, innermost last.
    scope: Vec<&'static str>,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.tokens.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn expect(&mut self, t: Tok) -> Result<(), ParseError> {
        match self.next() {
            Some(ref got) if *got == t => Ok(()),
            got => Err(ParseError(format!("expected {t:?}, got {got:?}"))),
        }
    }

    fn expr(&mut self, min_prec: u8) -> Result<Expr, ParseError> {
        let mut lhs = self.unary()?;
        loop {
            let Some(tok) = self.peek() else { break };
            let op = match tok {
                Tok::Op(s) => match *s {
                    "+" => BinOp::Add,
                    "-" => BinOp::Sub,
                    "*" => BinOp::Mul,
                    "/" => BinOp::Div,
                    "%" => BinOp::Mod,
                    "<" => BinOp::Lt,
                    "<=" => BinOp::Le,
                    ">" => BinOp::Gt,
                    ">=" => BinOp::Ge,
                    "==" => BinOp::Eq,
                    "!=" => BinOp::Ne,
                    "&&" => BinOp::And,
                    "||" => BinOp::Or,
                    _ => break,
                },
                Tok::Question if min_prec == PREC_TERNARY => {
                    self.next();
                    let a = self.expr(PREC_TERNARY)?;
                    self.expect(Tok::Colon)?;
                    let b = self.expr(PREC_TERNARY)?;
                    lhs = Expr::If(Box::new(lhs), Box::new(a), Box::new(b));
                    continue;
                }
                _ => break,
            };
            let prec = op.precedence();
            if prec < min_prec.max(1) {
                break;
            }
            self.next();
            let rhs = self.expr(prec + 1)?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr, ParseError> {
        match self.peek() {
            Some(Tok::Op("-")) => {
                self.next();
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Some(Tok::Op("!")) => {
                self.next();
                Ok(Expr::Not(Box::new(self.unary()?)))
            }
            _ => self.postfix(),
        }
    }

    /// A primary followed by any number of `.field` accesses, and calls of
    /// a function value (`f(x)`, `#e.args.f(x)`).
    fn postfix(&mut self) -> Result<Expr, ParseError> {
        let mut e = self.primary()?;
        loop {
            match self.peek() {
                Some(Tok::Dot) => {
                    self.next();
                    match self.next() {
                        Some(Tok::Ident(name)) => e = Expr::Get(Box::new(e), crate::sym::intern(&name)),
                        got => return Err(ParseError(format!("expected a field name after '.', got {got:?}"))),
                    }
                }
                Some(Tok::LParen) if matches!(e, Expr::Var(_) | Expr::Ref(_) | Expr::Get(..) | Expr::Apply(..) | Expr::Lambda(..)) => {
                    self.next();
                    let args = self.args()?;
                    e = Expr::Apply(Box::new(e), args);
                }
                _ => return Ok(e),
            }
        }
    }

    /// Arguments after an opening `(`, through the closing `)`.
    fn args(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();
        if self.peek() == Some(&Tok::RParen) {
            self.next();
            return Ok(args);
        }
        loop {
            args.push(self.expr(PREC_TERNARY)?);
            match self.next() {
                Some(Tok::Comma) => continue,
                Some(Tok::RParen) => return Ok(args),
                got => return Err(ParseError(format!("expected , or ) in call, got {got:?}"))),
            }
        }
    }

    /// A record's fields after an opening `(`, through the closing `)`:
    /// `name: value, …`, as name and value in turn. A field may also be
    /// written as two arguments, `'name', value`.
    fn fields(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();
        if self.peek() == Some(&Tok::RParen) {
            self.next();
            return Ok(args);
        }
        loop {
            if let (Some(Tok::Ident(n)), Some(Tok::Colon)) = (self.tokens.get(self.pos), self.tokens.get(self.pos + 1)) {
                args.push(Expr::Lit(Value::Str(n.clone())));
                self.pos += 2;
            }
            args.push(self.expr(PREC_TERNARY)?);
            match self.next() {
                Some(Tok::Comma) => continue,
                Some(Tok::RParen) => return Ok(args),
                got => return Err(ParseError(format!("expected , or ) in record, got {got:?}"))),
            }
        }
    }

    /// `(a, b) => …` from the `(` at the cursor: the parameter names, or
    /// `None` (cursor unmoved) when this `(` opens something else.
    fn lambda_params(&mut self) -> Option<Vec<String>> {
        let mut i = self.pos + 1;
        let mut names = Vec::new();
        if self.tokens.get(i) == Some(&Tok::RParen) {
            return None;
        }
        loop {
            let Some(Tok::Ident(n)) = self.tokens.get(i) else { return None };
            names.push(n.clone());
            i += 1;
            match self.tokens.get(i) {
                Some(Tok::Comma) => i += 1,
                Some(Tok::RParen) => break,
                _ => return None,
            }
        }
        if self.tokens.get(i + 1) != Some(&Tok::Op("=>")) {
            return None;
        }
        self.pos = i + 2;
        Some(names)
    }

    /// A lambda's body, with `params` in scope; the cursor is past `=>`.
    fn lambda(&mut self, params: Vec<String>) -> Result<Expr, ParseError> {
        let mut interned = Vec::new();
        for p in &params {
            if matches!(p.as_str(), "true" | "false") || Constant::from_name(p).is_some() {
                return Err(ParseError(format!("{p} cannot name a parameter")));
            }
            if interned.contains(&crate::sym::intern(p)) {
                return Err(ParseError(format!("parameter {p} is named twice")));
            }
            interned.push(crate::sym::intern(p));
        }
        let depth = self.scope.len();
        self.scope.extend(interned.iter().copied());
        let body = self.expr(PREC_TERNARY);
        self.scope.truncate(depth);
        Ok(Expr::Lambda(interned, Box::new(body?)))
    }

    fn primary(&mut self) -> Result<Expr, ParseError> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(Expr::Lit(Value::Number(n))),
            Some(Tok::Lit(v)) => Ok(Expr::Lit(v)),
            Some(Tok::Str(s)) => Ok(Expr::Lit(Value::Str(s))),
            Some(Tok::Ref(l)) => Ok(Expr::Ref(l)),
            Some(Tok::Token(name)) => Ok(Expr::Token(name)),
            Some(Tok::LParen) => {
                self.pos -= 1;
                if let Some(params) = self.lambda_params() {
                    return self.lambda(params);
                }
                self.pos += 1;
                let e = self.expr(PREC_TERNARY)?;
                self.expect(Tok::RParen)?;
                Ok(e)
            }
            Some(Tok::LBracket) => {
                let mut items = Vec::new();
                loop {
                    match self.expr(PREC_TERNARY)? {
                        Expr::Lit(Value::Number(n)) => items.push(n),
                        other => return Err(ParseError(format!("vector literals take numbers, got {other}"))),
                    }
                    match self.next() {
                        Some(Tok::Comma) => continue,
                        Some(Tok::RBracket) => break,
                        got => return Err(ParseError(format!("expected , or ] in vector, got {got:?}"))),
                    }
                }
                match items.as_slice() {
                    [a, b] => Ok(Expr::Lit(Value::Vec2([*a, *b]))),
                    [a, b, c] => Ok(Expr::Lit(Value::Vec3([*a, *b, *c]))),
                    _ => Err(ParseError("vector literals have 2 or 3 numbers".into())),
                }
            }
            Some(Tok::Ident(name)) if self.peek() == Some(&Tok::Op("=>")) => {
                self.next();
                self.lambda(vec![name])
            }
            Some(Tok::Ident(name)) if self.scope.iter().any(|p| *p == name) => Ok(Expr::Var(crate::sym::intern(&name))),
            Some(Tok::Ident(name)) => match name.as_str() {
                "true" => Ok(Expr::Lit(Value::Bool(true))),
                "false" => Ok(Expr::Lit(Value::Bool(false))),
                // A bare constant name. `pi(` is still a call, and fails at
                // eval as an unknown function.
                c if self.peek() != Some(&Tok::LParen) && Constant::from_name(c).is_some() => {
                    Ok(Expr::Const(Constant::from_name(c).expect("checked")))
                }
                _ => {
                    if self.peek() != Some(&Tok::LParen) {
                        return Err(ParseError(format!("{name} is not defined here: a function call needs (…), and a name is only defined inside a lambda that takes it (x => …)")));
                    }
                    self.next();
                    let args = if name == "record" { self.fields()? } else { self.args()? };
                    Ok(Expr::Call(name, args))
                }
            },
            got => Err(ParseError(format!("unexpected {got:?}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(id: u64, path: &str) -> Leaf {
        Leaf { node: NodeId::from_raw(id), path: path.parse().unwrap() }
    }

    #[test]
    fn parse_print_roundtrip() {
        let cases = [
            "#3.slider.value * 300",
            "600 - #3.slider.value * 300",
            "(1 + 2) * 3",
            "1 + 2 * 3",
            "-#3.slider.value",
            "#3.slider.value > 0.5 ? 1 : 0.25",
            "clamp(#3.slider.value * 2, 0, 1)",
            "scale(#3.slider.value, 0, 1, 0, 300)",
            "!true && false || 1 < 2",
            "'a' + 'b'",
            "\"it's\" + 'b'",
            "'linear'",
            "[1, 2] * 2",
            "1 - (2 - 3)",
            "(1 < 2 ? 3 : 4) + 1",
            "sin(#3.transform2d.rot) * 120",
            "atan2(#4.transform2d.y, #4.transform2d.x)",
            "hypot(3, 4) + sqrt(2) + pow(2, 10)",
            "cos(pi / 2) + tan(0)",
            "pi * 2",
            "-pi",
            "0.00958 W * 2",
            "1550 m + 2 m",
            "-3 N",
            "3 kg/s3 / 2 s",
            "max(1 W, 2 W) > 0.5 W ? 1 : 0",
            "#3.args.beam.waist * 2",
            "-#3.args.beam.waist",
            "#3.args.beam.m2 + 1",
        ];
        for text in cases {
            let e = Expr::parse(text).unwrap_or_else(|err| panic!("{text}: {err}"));
            let printed = e.to_string();
            let again = Expr::parse(&printed).unwrap_or_else(|err| panic!("{printed}: {err}"));
            assert_eq!(again, e, "{text} -> {printed}");
            assert_eq!(printed, text, "canonical form differs");
        }
        assert!(Expr::parse("#3.slider").is_err());
        assert!(Expr::parse("1 +").is_err());
        assert!(Expr::parse("nope(1)").is_ok(), "unknown functions parse; they fail at eval");
    }

    #[test]
    fn deps_are_static() {
        let e = Expr::parse("#3.slider.value > 0.5 ? #4.size.w : #4.size.h").unwrap();
        let d = e.deps();
        assert_eq!(d.len(), 3);
        assert!(d.contains(&leaf(3, "slider.value")));
        assert!(d.contains(&leaf(4, "size.h")));
    }

    #[test]
    fn eval_reads_through_resolver() {
        let read = |l: Leaf| -> Option<Value> {
            if l == leaf(3, "slider.value") { Some(Value::Number(0.25)) } else { None }
        };
        let ev = |t: &str| Expr::parse(t).unwrap().eval(&read);
        assert_eq!(ev("#3.slider.value * 300").unwrap(), Value::Number(75.0));
        assert_eq!(ev("600 - #3.slider.value * 300").unwrap(), Value::Number(525.0));
        assert_eq!(ev("#3.slider.value > 0.5 ? 1 : 0.25").unwrap(), Value::Number(0.25));
        assert_eq!(ev("clamp(#3.slider.value * 8, 0, 1)").unwrap(), Value::Number(1.0));
        assert_eq!(ev("scale(#3.slider.value, 0, 1, 100, 200)").unwrap(), Value::Number(125.0));
        assert_eq!(ev("[1, 2] * 2").unwrap(), Value::Vec2([2.0, 4.0]));
        assert!(matches!(ev("#9.size.w").unwrap_err(), EvalError::Unresolved(_)));
        assert!(matches!(ev("1 + true").unwrap_err(), EvalError::Type(_)));
        assert!(matches!(ev("nope(1)").unwrap_err(), EvalError::UnknownFunction(_)));
        assert!(matches!(ev("clamp(1)").unwrap_err(), EvalError::Arity { .. }));
    }

    #[test]
    fn trig_roots_and_pi() {
        let read = |_: Leaf| -> Option<Value> { None };
        let ev = |t: &str| Expr::parse(t).unwrap().eval(&read).unwrap().as_f64().unwrap();
        let close = |a: f64, b: f64| (a - b).abs() < 1e-12;
        assert!(close(ev("pi"), std::f64::consts::PI));
        assert!(close(ev("sin(pi / 2)"), 1.0));
        assert!(close(ev("cos(pi)"), -1.0));
        assert!(close(ev("tan(pi / 4)"), 1.0));
        assert!(close(ev("atan2(1, 1)"), std::f64::consts::FRAC_PI_4));
        assert!(close(ev("atan2(1, 0)"), std::f64::consts::FRAC_PI_2), "atan2 takes (y, x)");
        assert!(close(ev("sqrt(16)"), 4.0));
        assert!(close(ev("pow(2, 10)"), 1024.0));
        assert!(close(ev("hypot(3, 4)"), 5.0));
        assert!(Expr::parse("pi").unwrap().deps().is_empty());
        // `pi(` parses as a call and is rejected at eval, like any unknown function.
        assert!(matches!(Expr::parse("pi()").unwrap().eval(&read).unwrap_err(), EvalError::UnknownFunction(_)));
        assert!(Expr::parse("tau").is_err(), "unknown bare identifiers do not parse");
    }

    #[test]
    fn quantities_are_dimension_checked() {
        let read = |_: Leaf| -> Option<Value> { None };
        let ev = |t: &str| Expr::parse(t).unwrap().eval(&read);
        let q = |t: &str| Value::parse_quantity(t).unwrap();
        // The stage B exit: mW + nm is an error, mW * 2 is not.
        let err = ev("9.58 mW + 1 nm").unwrap_err().to_string();
        assert!(err.contains("W") && err.contains("m"), "{err}");
        assert_eq!(ev("9.58 mW * 2").unwrap(), q("19.16 mW"));
        assert_eq!(ev("2 * 9.58 mW").unwrap(), q("0.01916 W"));
        assert!(ev("9.58 mW + 2").unwrap_err().to_string().contains("plain number"));
        assert_eq!(ev("9.58 mW + 0.42 mW").unwrap(), q("10 mW"));
        assert_eq!(ev("1 W / 1 W").unwrap(), Value::Number(1.0), "a cancelled dimension is a plain number");
        assert_eq!(ev("3 V * 2 A").unwrap(), q("6 W"));
        assert_eq!(ev("6 W / 2 A").unwrap(), q("3 V"));
        assert_eq!(ev("1 km > 999 m").unwrap(), Value::Bool(true));
        assert!(ev("1 km > 999 s").is_err());
        assert_eq!(ev("sqrt(4 m*m)").unwrap(), q("2 m"));
        assert!(ev("sqrt(2 m)").is_err());
        assert_eq!(ev("pow(2 m, 3)").unwrap(), q("8 m3"));
        assert!(ev("pow(2 m, 0.5)").is_err());
        assert_eq!(ev("hypot(3 m, 4 m)").unwrap(), q("5 m"));
        assert!(ev("sin(1 m)").is_err());
        assert_eq!(ev("sin(90 deg)").unwrap(), Value::Number(1.0), "angles are plain numbers in radians");
        assert_eq!(ev("clamp(5 W, 0 W, 2 W)").unwrap(), q("2 W"));
        assert!(ev("clamp(5 W, 0, 2)").is_err());
        assert_eq!(ev("lerp(0 W, 10 W, 0.25)").unwrap(), q("2.5 W"));
        assert!(ev("lerp(0 W, 10 W, 1 s)").is_err());
        assert_eq!(ev("scale(5 mW, 0 W, 10 mW, 0, 100)").unwrap(), Value::Number(50.0));
        assert_eq!(ev("abs(-3 N)").unwrap(), q("3 N"));
        // A number before a non-unit identifier is still a syntax error.
        assert!(Expr::parse("2 furlongs").is_err());
        assert!(Expr::parse("2 pi").is_err());
        // Printing uses the canonical unit so the round trip is exact.
        assert_eq!(Expr::parse("9.58 mW").unwrap().to_string(), "0.00958 W");
        assert_eq!(Expr::parse("1 min").unwrap().to_string(), "60 s");
    }

    #[test]
    fn get_reads_record_fields() {
        let beam = Value::record("gaussian_beam", [("waist", Value::parse_quantity("30 um").unwrap()), ("m2", Value::Number(1.1))]);
        let read = |l: Leaf| -> Option<Value> { (l == leaf(3, "args.beam")).then(|| beam.clone()) };
        let ev = |t: &str| Expr::parse(t).unwrap().eval(&read);
        assert_eq!(ev("#3.args.beam.waist * 2").unwrap(), Value::parse_quantity("60 um").unwrap());
        assert_eq!(ev("#3.args.beam.m2").unwrap(), Value::Number(1.1));
        let err = ev("#3.args.beam.power").unwrap_err().to_string();
        assert!(err.contains("no field") && err.contains("waist"), "{err}");
        assert!(ev("#3.args.beam.waist.x").unwrap_err().to_string().contains("cannot read field"));
        // Dependencies are the record leaf, not the field.
        let d = Expr::parse("#3.args.beam.waist").unwrap().deps();
        assert_eq!(d.into_iter().collect::<Vec<_>>(), vec![leaf(3, "args.beam")]);
        assert_eq!(Expr::parse("#3.args.beam.waist").unwrap(), Expr::Get(Box::new(Expr::Ref(leaf(3, "args.beam"))), "waist"));
    }
}
