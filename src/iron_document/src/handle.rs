//! Drag handles: how a gesture on a leaf is realised (roadmap stage A,
//! "linear binding inversion").
//!
//! An authored constant is previewed directly. A bound leaf has no value of
//! its own, but when its expression is linear in exactly one constant leaf
//! (`x = 2·v + 10`, `y = 640 − #slider.value · 300`) the drag can set that
//! leaf exactly by inverting the expression: the gesture previews the source,
//! the binding re-evaluates to the dragged value, and the commit writes the
//! source. Anything non-linear, or depending on several free leaves, is
//! locked until it belongs to a constraint cluster (constraint-system.md).
//! This is doc 02 §8.2's "edit the underlying source if the binding is a
//! direct reference", generalised to linear references.

use crate::document::Document;
use crate::expr::{BinOp, Expr, Leaf};
use crate::graph::Reactive;
use crate::value::{Value, ValueKind};

#[derive(Clone, Debug, PartialEq)]
pub enum Handle {
    /// An authored constant: preview it directly.
    Direct,
    /// Bound to `a·source + b` with `source` a constant leaf. A target value
    /// `y` previews `(y − b) / a` on the source.
    Linear { source: Leaf, a: f64, b: f64 },
    /// No handle. The reason is for logs and the inspector.
    Locked(String),
}

impl Handle {
    pub fn is_locked(&self) -> bool {
        matches!(self, Handle::Locked(_))
    }
}

/// The handle for `leaf` now: roles are read at the moment of the grab, so a
/// leaf that a clip drives at this playhead is locked even though it is an
/// authored constant.
pub fn handle(doc: &Document, reactive: &Reactive, leaf: Leaf) -> Handle {
    let Some(slot) = doc.leaf(leaf.node, leaf.path) else {
        return Handle::Locked(format!("{leaf} does not exist"));
    };
    if reactive.animated().contains_key(&leaf) {
        return Handle::Locked(format!("{leaf} is driven by a clip at the current time"));
    }
    let Some(expr) = slot.expr() else {
        return Handle::Direct;
    };
    let inlined = inline(doc, expr);
    let read = |l: Leaf| reactive.read(doc, l);
    // Candidates: constant number leaves the (inlined) expression reads.
    // Clip-driven and non-number leaves are knowns for the drag.
    let free: Vec<Leaf> = inlined
        .deps()
        .into_iter()
        .filter(|d| {
            !reactive.animated().contains_key(d)
                && doc.leaf(d.node, d.path).and_then(|s| s.constant()).is_some_and(|v| v.kind() == ValueKind::Number)
        })
        .collect();
    match free.as_slice() {
        [] => Handle::Locked(format!("{leaf} is bound and reads no free leaf")),
        [source] => match linear(&inlined, *source, &read) {
            Some((a, b)) if a != 0.0 && a.is_finite() && b.is_finite() => Handle::Linear { source: *source, a, b },
            _ => Handle::Locked(format!("{leaf} is bound non-linearly to {source}")),
        },
        many => {
            let names: Vec<String> = many.iter().map(|l| l.to_string()).collect();
            Handle::Locked(format!("{leaf} is bound to {} free leaves ({})", many.len(), names.join(", ")))
        }
    }
}

/// Replace every reference to a bound leaf with that leaf's expression, so
/// the result reads only constants and clip-driven leaves. Bindings form a
/// DAG (checked at apply time), so this terminates.
fn inline(doc: &Document, expr: &Expr) -> Expr {
    match expr {
        Expr::Ref(l) => match doc.leaf(l.node, l.path).and_then(|s| s.expr()) {
            Some(inner) => inline(doc, inner),
            None => expr.clone(),
        },
        Expr::Lit(_) | Expr::Const(_) => expr.clone(),
        Expr::Neg(e) => Expr::Neg(Box::new(inline(doc, e))),
        Expr::Not(e) => Expr::Not(Box::new(inline(doc, e))),
        Expr::Bin(op, a, b) => Expr::Bin(*op, Box::new(inline(doc, a)), Box::new(inline(doc, b))),
        Expr::If(c, a, b) => Expr::If(Box::new(inline(doc, c)), Box::new(inline(doc, a)), Box::new(inline(doc, b))),
        Expr::Call(name, args) => Expr::Call(name.clone(), args.iter().map(|a| inline(doc, a)).collect()),
        Expr::Get(e, f) => Expr::Get(Box::new(inline(doc, e)), f),
    }
}

/// `expr = a·var + b` over numbers, with every other leaf read through
/// `read`. `None` when the expression is not linear in `var` (or not a
/// number at all). Symbolic, so a function that happens to be straight on
/// the sampled interval is still rejected.
pub fn linear(expr: &Expr, var: Leaf, read: &dyn Fn(Leaf) -> Option<Value>) -> Option<(f64, f64)> {
    let known = |e: &Expr| -> Option<(f64, f64)> { e.eval(read).ok()?.as_f64().map(|v| (0.0, v)) };
    let depends = |e: &Expr| e.deps().contains(&var);
    if !depends(expr) {
        return known(expr);
    }
    match expr {
        Expr::Ref(l) if *l == var => Some((1.0, 0.0)),
        Expr::Neg(e) => linear(e, var, read).map(|(a, b)| (-a, -b)),
        Expr::Bin(op, x, y) => {
            let (ax, bx) = linear(x, var, read)?;
            let (ay, by) = linear(y, var, read)?;
            match op {
                BinOp::Add => Some((ax + ay, bx + by)),
                BinOp::Sub => Some((ax - ay, bx - by)),
                BinOp::Mul if ax == 0.0 => Some((ay * bx, by * bx)),
                BinOp::Mul if ay == 0.0 => Some((ax * by, bx * by)),
                BinOp::Div if ay == 0.0 && by != 0.0 => Some((ax / by, bx / by)),
                _ => None,
            }
        }
        // The branch must not switch as `var` moves, so the condition may
        // not read it; the taken branch then holds for every value.
        Expr::If(c, x, y) if !depends(c) => match c.eval(read).ok()? {
            Value::Bool(true) => linear(x, var, read),
            Value::Bool(false) => linear(y, var, read),
            _ => None,
        },
        // Two floor functions are linear in their arguments by definition;
        // rewrite them and recurse. Everything else (clamp, trig, …) is not.
        Expr::Call(name, args) => match (name.as_str(), args.as_slice()) {
            ("lerp", [p, q, t]) => {
                let span = Expr::Bin(BinOp::Sub, Box::new(q.clone()), Box::new(p.clone()));
                let e = Expr::Bin(BinOp::Add, Box::new(p.clone()), Box::new(Expr::Bin(BinOp::Mul, Box::new(span), Box::new(t.clone()))));
                linear(&e, var, read)
            }
            ("scale", [x, d0, d1, r0, r1]) => {
                let t = Expr::Bin(
                    BinOp::Div,
                    Box::new(Expr::Bin(BinOp::Sub, Box::new(x.clone()), Box::new(d0.clone()))),
                    Box::new(Expr::Bin(BinOp::Sub, Box::new(d1.clone()), Box::new(d0.clone()))),
                );
                let span = Expr::Bin(BinOp::Sub, Box::new(r1.clone()), Box::new(r0.clone()));
                let e = Expr::Bin(BinOp::Add, Box::new(r0.clone()), Box::new(Expr::Bin(BinOp::Mul, Box::new(span), Box::new(t))));
                linear(&e, var, read)
            }
            _ => None,
        },
        _ => None,
    }
}

/// The leaf a target value for `leaf` would be written to, with the value.
/// `Direct` answers the leaf itself; `Linear` inverts; `Locked` is an error.
pub fn resolve_target(doc: &Document, reactive: &Reactive, leaf: Leaf, value: Value) -> Result<(Leaf, Value), String> {
    match handle(doc, reactive, leaf) {
        Handle::Direct => Ok((leaf, value)),
        Handle::Linear { source, a, b } => {
            let y = value.as_f64().ok_or_else(|| format!("{leaf}: a linear handle takes a number, got {}", value.kind().name()))?;
            Ok((source, Value::Number((y - b) / a)))
        }
        Handle::Locked(reason) => Err(reason),
    }
}

