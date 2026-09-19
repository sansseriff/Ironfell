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
//! Absent on purpose: field access on structured values (`Get`), collection
//! verbs, and calls that leave the process. Those arrive with the floor
//! tranche that needs them and with the cost-tagged invocation node.

use crate::component::LeafPath;
use crate::id::NodeId;
use crate::value::Value;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeSet;
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
    Ref(Leaf),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
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

impl Expr {
    /// Every leaf this expression reads. Static: no evaluation needed.
    pub fn deps(&self) -> BTreeSet<Leaf> {
        let mut out = BTreeSet::new();
        self.collect_deps(&mut out);
        out
    }

    fn collect_deps(&self, out: &mut BTreeSet<Leaf>) {
        match self {
            Expr::Lit(_) => {}
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
        }
    }

    /// Evaluate against a resolver for leaf values. Reads go through the
    /// resolver and nowhere else, which is the hook a tracking evaluator
    /// would use later.
    pub fn eval(&self, read: &dyn Fn(Leaf) -> Option<Value>) -> Result<Value, EvalError> {
        match self {
            Expr::Lit(v) => Ok(v.clone()),
            Expr::Ref(l) => read(*l).ok_or(EvalError::Unresolved(*l)),
            Expr::Neg(e) => match e.eval(read)? {
                Value::Number(n) => Ok(Value::Number(-n)),
                v => Err(EvalError::Type(format!("cannot negate {}", v.kind().name()))),
            },
            Expr::Not(e) => match e.eval(read)? {
                Value::Bool(b) => Ok(Value::Bool(!b)),
                v => Err(EvalError::Type(format!("cannot negate {}", v.kind().name()))),
            },
            Expr::Bin(op, a, b) => {
                let a = a.eval(read)?;
                let b = b.eval(read)?;
                binary(*op, a, b)
            }
            Expr::If(c, a, b) => match c.eval(read)? {
                Value::Bool(true) => a.eval(read),
                Value::Bool(false) => b.eval(read),
                v => Err(EvalError::Type(format!("condition is {}, not bool", v.kind().name()))),
            },
            Expr::Call(name, args) => {
                let vals: Result<Vec<Value>, EvalError> = args.iter().map(|a| a.eval(read)).collect();
                floor::call(name, &vals?)
            }
        }
    }

    pub fn parse(text: &str) -> Result<Expr, ParseError> {
        let tokens = lex(text)?;
        let mut p = Parser { tokens, pos: 0 };
        let e = p.expr(PREC_TERNARY)?;
        if p.pos < p.tokens.len() {
            return Err(ParseError(format!("unexpected {:?} after expression", p.tokens[p.pos])));
        }
        Ok(e)
    }

    fn write(&self, f: &mut fmt::Formatter<'_>, parent: u8) -> fmt::Result {
        match self {
            Expr::Lit(v) => write_lit(v, f),
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

fn write_lit(v: &Value, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match v {
        Value::Str(s) => write!(f, "{:?}", s),
        Value::Enum(s) => write!(f, "'{s}'"),
        Value::Vec2([a, b]) => write!(f, "[{}, {}]", crate::value::fmt_num(*a), crate::value::fmt_num(*b)),
        Value::Vec3([a, b, c]) => write!(
            f,
            "[{}, {}, {}]",
            crate::value::fmt_num(*a),
            crate::value::fmt_num(*b),
            crate::value::fmt_num(*c)
        ),
        Value::Color([r, g, b, a]) => write!(f, "rgba({r}, {g}, {b}, {a})"),
        Value::Ref(id) => write!(f, "ref({id})"),
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

fn binary(op: BinOp, a: Value, b: Value) -> Result<Value, EvalError> {
    use Value::*;
    let num = |x: &Value| x.as_f64();
    Ok(match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => match (&a, &b) {
            (Number(x), Number(y)) => Number(match op {
                BinOp::Add => x + y,
                BinOp::Sub => x - y,
                BinOp::Mul => x * y,
                BinOp::Div => x / y,
                _ => x % y,
            }),
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
        BinOp::Eq => Bool(a == b),
        BinOp::Ne => Bool(a != b),
        BinOp::And | BinOp::Or => match (&a, &b) {
            (Bool(x), Bool(y)) => Bool(if op == BinOp::And { *x && *y } else { *x || *y }),
            _ => return Err(EvalError::Type(format!("{} {} {}", a.kind().name(), op.symbol(), b.kind().name()))),
        },
    })
}

/// The floor: pure, total, bounded functions available to expressions
/// (doc 02 §6). Each is one arm; adding one is a Rust change by design.
pub mod floor {
    use super::{EvalError, Value};

    pub const NAMES: &[&str] = &["abs", "floor", "ceil", "round", "min", "max", "clamp", "lerp", "scale"];

    fn nums(func: &str, args: &[Value], n: usize) -> Result<Vec<f64>, EvalError> {
        if args.len() != n {
            return Err(EvalError::Arity { func: func.to_owned(), expected: n, got: args.len() });
        }
        args.iter()
            .map(|v| v.as_f64().ok_or_else(|| EvalError::Type(format!("{func} takes numbers, got {}", v.kind().name()))))
            .collect()
    }

    pub fn call(func: &str, args: &[Value]) -> Result<Value, EvalError> {
        let n = |k| nums(func, args, k);
        Ok(Value::Number(match func {
            "abs" => n(1)?[0].abs(),
            "floor" => n(1)?[0].floor(),
            "ceil" => n(1)?[0].ceil(),
            "round" => n(1)?[0].round(),
            "min" => {
                let a = n(2)?;
                a[0].min(a[1])
            }
            "max" => {
                let a = n(2)?;
                a[0].max(a[1])
            }
            "clamp" => {
                let a = n(3)?;
                a[0].clamp(a[1].min(a[2]), a[2].max(a[1]))
            }
            "lerp" => {
                let a = n(3)?;
                a[0] + (a[1] - a[0]) * a[2]
            }
            // scale(x, d0, d1, r0, r1): linear map of x from [d0, d1] to [r0, r1].
            "scale" => {
                let a = n(5)?;
                let t = if a[2] == a[1] { 0.0 } else { (a[0] - a[1]) / (a[2] - a[1]) };
                a[3] + (a[4] - a[3]) * t
            }
            _ => return Err(EvalError::UnknownFunction(func.to_owned())),
        }))
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
    Str(String),
    Enum(String),
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
}

fn lex(text: &str) -> Result<Vec<Tok>, ParseError> {
    let b = text.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        match c {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'0'..=b'9' | b'.' => {
                let start = i;
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.' || b[i] == b'e' || b[i] == b'E') {
                    i += 1;
                }
                let s = &text[start..i];
                out.push(Tok::Num(s.parse().map_err(|_| ParseError(format!("bad number {s:?}")))?));
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
                    return Err(ParseError("unterminated enum literal".into()));
                }
                out.push(Tok::Enum(text[start..i].to_owned()));
                i += 1;
            }
            b'#' => {
                // #<id>.<component>.<field>
                let start = i + 1;
                i += 1;
                while i < b.len() && b[i].is_ascii_alphanumeric() {
                    i += 1;
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
                if parts.len() != 2 {
                    return Err(ParseError(format!("reference {node} needs .component.field")));
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
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<Expr, ParseError> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(Expr::Lit(Value::Number(n))),
            Some(Tok::Str(s)) => Ok(Expr::Lit(Value::Str(s))),
            Some(Tok::Enum(s)) => Ok(Expr::Lit(Value::Enum(s))),
            Some(Tok::Ref(l)) => Ok(Expr::Ref(l)),
            Some(Tok::LParen) => {
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
            Some(Tok::Ident(name)) => match name.as_str() {
                "true" => Ok(Expr::Lit(Value::Bool(true))),
                "false" => Ok(Expr::Lit(Value::Bool(false))),
                _ => {
                    self.expect(Tok::LParen)?;
                    let mut args = Vec::new();
                    if self.peek() == Some(&Tok::RParen) {
                        self.next();
                        return Ok(Expr::Call(name, args));
                    }
                    loop {
                        args.push(self.expr(PREC_TERNARY)?);
                        match self.next() {
                            Some(Tok::Comma) => continue,
                            Some(Tok::RParen) => break,
                            got => return Err(ParseError(format!("expected , or ) in call, got {got:?}"))),
                        }
                    }
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
            "\"a\" + \"b\"",
            "'linear'",
            "[1, 2] * 2",
            "1 - (2 - 3)",
            "(1 < 2 ? 3 : 4) + 1",
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
}
