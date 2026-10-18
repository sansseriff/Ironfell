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
use crate::graph::{ProducerId, Reactive};
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
        // A leaf of a facet the node may have but lacks (a rect's
        // `corners`): a gesture drives it directly and its commit adds the
        // facet (`Store::commit_gesture`).
        if doc.live(leaf.node).is_some_and(|n| n.ty.spec().allows(leaf.path.facet)) && doc.facet(leaf.node, leaf.path.facet).is_none() {
            return Handle::Direct;
        }
        return Handle::Locked(format!("{leaf} does not exist"));
    };
    if let Some(signal) = reactive.signal(leaf) {
        return Handle::Locked(match signal.producer {
            ProducerId::CLIPS => format!("{leaf} is driven by a clip at the current time"),
            ProducerId::NETWORK => format!("{leaf} is solved by its across/through network; change a source or a parameter instead"),
            p => format!("{leaf} is published by producer {} at runtime; freeze it to edit", p.0),
        });
    }
    // A connection's in-port has no binding of its own but resolves through
    // its partner (`port::derived_signals`), so it inverts like one.
    let Some(expr) = slot.expr().or_else(|| reactive.expr_of(leaf)) else {
        return Handle::Direct;
    };
    let inlined = inline(doc, reactive, expr);
    let read = |l: Leaf| reactive.read(doc, l);
    // Candidates: constant number leaves the (inlined) expression reads.
    // Clip-driven and non-number leaves are knowns for the drag.
    let free: Vec<Leaf> = inlined
        .deps()
        .into_iter()
        .filter(|d| {
            !reactive.is_driven(*d)
                && doc.leaf(d.node, d.path).and_then(|s| s.constant()).is_some_and(|v| matches!(v.kind(), ValueKind::Number | ValueKind::Quantity))
        })
        .collect();
    match free.as_slice() {
        [] => Handle::Locked(format!("{leaf} is bound and reads no free leaf")),
        [source] => match linear(&inlined, *source, &read) {
            Some((a, b)) if a != 0.0 && a.is_finite() && b.is_finite() => Handle::Linear { source: *source, a, b },
            _ => Handle::Locked(format!("{leaf} is bound non-linearly to {source}")),
        },
        many => {
            // Several free leaves, but if the value is linear in exactly one
            // of them (a laser's power, where a fiber's length and loss sit
            // in an exponent), that one is the handle: the others are held.
            let linear_in: Vec<(Leaf, f64, f64)> = many
                .iter()
                .filter_map(|&s| linear(&inlined, s, &read).filter(|(a, b)| *a != 0.0 && a.is_finite() && b.is_finite()).map(|(a, b)| (s, a, b)))
                .collect();
            if let [(source, a, b)] = linear_in.as_slice() {
                return Handle::Linear { source: *source, a: *a, b: *b };
            }
            let names: Vec<String> = many.iter().map(|l| l.to_string()).collect();
            Handle::Locked(format!("{leaf} is bound to {} free leaves ({})", many.len(), names.join(", ")))
        }
    }
}

/// Replace every reference to a bound leaf with that leaf's expression, so
/// the result reads only constants and clip-driven leaves. Bindings form a
/// DAG (checked at apply time), so this terminates.
fn inline(doc: &Document, reactive: &Reactive, expr: &Expr) -> Expr {
    let i = |e: &Expr| Box::new(inline(doc, reactive, e));
    match expr {
        Expr::Ref(l) => match doc.leaf(l.node, l.path).and_then(|s| s.expr()).or_else(|| reactive.expr_of(*l)) {
            Some(inner) => inline(doc, reactive, inner),
            None => expr.clone(),
        },
        Expr::Lit(_) | Expr::Const(_) => expr.clone(),
        Expr::Neg(e) => Expr::Neg(i(e)),
        Expr::Not(e) => Expr::Not(i(e)),
        Expr::Bin(op, a, b) => Expr::Bin(*op, i(a), i(b)),
        Expr::If(c, a, b) => Expr::If(i(c), i(a), i(b)),
        Expr::Call(name, args) => Expr::Call(name.clone(), args.iter().map(|a| inline(doc, reactive, a)).collect()),
        Expr::Get(e, f) => Expr::Get(i(e), f),
    }
}

/// A number's value, or a quantity's magnitude in SI units: the scale a
/// linear handle works in.
fn magnitude(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => Some(*n),
        Value::Quantity { si, .. } => Some(*si),
        _ => None,
    }
}

/// `expr = a·var + b` over numbers, with every other leaf read through
/// `read`. `None` when the expression is not linear in `var` (or not a
/// number at all). Symbolic, so a function that happens to be straight on
/// the sampled interval is still rejected.
pub fn linear(expr: &Expr, var: Leaf, read: &dyn Fn(Leaf) -> Option<Value>) -> Option<(f64, f64)> {
    let known = |e: &Expr| -> Option<(f64, f64)> { magnitude(&e.eval(read).ok()?).map(|v| (0.0, v)) };
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
            let y = magnitude(&value).ok_or_else(|| format!("{leaf}: a linear handle takes a number or quantity, got {}", value.kind().name()))?;
            // The source keeps its own kind: a quantity stays one, in SI.
            let x = (y - b) / a;
            let v = match doc.leaf(source.node, source.path).and_then(|s| s.constant()) {
                Some(Value::Quantity { dim, .. }) => Value::Quantity { si: x, dim: *dim },
                _ => Value::Number(x),
            };
            Ok((source, v))
        }
        Handle::Locked(reason) => Err(reason),
    }
}

