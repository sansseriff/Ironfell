//! Scripted time (vision doc 05, timeline.md §12.1): clips of keyframes,
//! sampled at a playhead.
//!
//! A clip is a node with `Timing { start }` and `Clip { prop }`, linked to its
//! targets by `animates` relations whose `data.offset` staggers it, holding
//! `key` children (`t` after the start, `value`, `easing` into the next).
//! Sampling produces, for one time, the value each targeted leaf shows. Those
//! values sit in the reactive graph's animated layer, beneath the gesture
//! overlay and above the authored constant, so bindings that read an animated
//! leaf follow it.
//!
//! Before the first key the leaf shows its authored value; between keys it
//! interpolates with the left key's easing; after the last it holds that
//! key's value. Targets must be constants: a bound leaf has no slot for a
//! sampled value, and the conflict is reported rather than resolved.

use crate::facet::{Clip, Key, LeafPath, Timing};
use crate::document::{Document, RelKind};
use crate::expr::Leaf;
use crate::id::NodeId;
use crate::registry::TypeId;
use crate::value::Value;
use std::collections::BTreeMap;

/// One time's worth of animated values, plus what could not be sampled.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Sample {
    pub values: BTreeMap<Leaf, Value>,
    /// Keyed by clip node.
    pub errors: BTreeMap<NodeId, String>,
}

pub fn ease(name: &str, u: f64) -> f64 {
    curve(name).map_or(u.clamp(0.0, 1.0), |c| c.at(u))
}

/// An easing curve (plans/specs/wires-and-formulas.md §5): one vocabulary
/// for keys, `ease()` in formulas, transitions and layout glides.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Linear,
    /// Today's quadratics, kept so files read the same.
    In,
    Out,
    InOut,
    /// CSS's `cubic-bezier(x1, y1, x2, y2)`.
    Bezier([f64; 4]),
    /// 0 until the end, then 1.
    Hold,
}

/// The named curves, with what each is.
pub const CURVES: &[(&str, &str)] = &[
    ("linear", "constant speed"),
    ("ease_in", "starts slowly"),
    ("ease_out", "ends slowly"),
    ("ease_in_out", "starts and ends slowly"),
    ("ease", "CSS's ease, cubic-bezier(0.25, 0.1, 0.25, 1)"),
    ("easy_ease", "After Effects' Easy Ease, cubic-bezier(0.33, 0, 0.67, 1)"),
    ("hold", "jumps at the end"),
];

/// A curve by name, or `cubic-bezier(x1, y1, x2, y2)` with x1, x2 in 0..1.
pub fn curve(name: &str) -> Result<Curve, String> {
    let name = name.trim();
    Ok(match name {
        "linear" => Curve::Linear,
        "ease_in" => Curve::In,
        "ease_out" => Curve::Out,
        "ease_in_out" => Curve::InOut,
        "ease" => Curve::Bezier([0.25, 0.1, 0.25, 1.0]),
        "easy_ease" => Curve::Bezier([0.33, 0.0, 0.67, 1.0]),
        "hold" => Curve::Hold,
        _ => {
            let inner = name.strip_prefix("cubic-bezier(").and_then(|r| r.strip_suffix(')')).ok_or_else(|| {
                format!("unknown easing {name:?}; use one of {} or cubic-bezier(x1, y1, x2, y2)", CURVES.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", "))
            })?;
            let n: Vec<f64> = inner.split(',').map(|p| p.trim().parse::<f64>()).collect::<Result<_, _>>().map_err(|_| format!("{name}: four numbers"))?;
            let [x1, y1, x2, y2] = n[..] else { return Err(format!("{name}: four numbers")) };
            if !(0.0..=1.0).contains(&x1) || !(0.0..=1.0).contains(&x2) || ![y1, y2].iter().all(|v| v.is_finite()) {
                return Err(format!("{name}: x1 and x2 must be within 0..1"));
            }
            Curve::Bezier([x1, y1, x2, y2])
        }
    })
}

impl Curve {
    /// Progress `u` (clamped to 0..1) shaped by the curve.
    pub fn at(self, u: f64) -> f64 {
        let u = if u.is_nan() { 0.0 } else { u.clamp(0.0, 1.0) };
        match self {
            Curve::Linear => u,
            Curve::In => u * u,
            Curve::Out => 1.0 - (1.0 - u) * (1.0 - u),
            Curve::InOut => {
                if u < 0.5 {
                    2.0 * u * u
                } else {
                    1.0 - (-2.0 * u + 2.0).powi(2) / 2.0
                }
            }
            Curve::Hold => {
                if u >= 1.0 { 1.0 } else { 0.0 }
            }
            Curve::Bezier([x1, y1, x2, y2]) => {
                // x(s) = u for s by Newton's method, bisection when it
                // stalls (as browsers do), then y(s).
                let b = |a1: f64, a2: f64, s: f64| 3.0 * (1.0 - s) * (1.0 - s) * s * a1 + 3.0 * (1.0 - s) * s * s * a2 + s * s * s;
                let db = |a1: f64, a2: f64, s: f64| 3.0 * (1.0 - s) * (1.0 - s) * a1 + 6.0 * (1.0 - s) * s * (a2 - a1) + 3.0 * s * s * (1.0 - a2);
                let mut s = u;
                for _ in 0..8 {
                    let (x, d) = (b(x1, x2, s) - u, db(x1, x2, s));
                    if x.abs() < 1e-7 {
                        return b(y1, y2, s);
                    }
                    if d.abs() < 1e-6 {
                        break;
                    }
                    s = (s - x / d).clamp(0.0, 1.0);
                }
                let (mut lo, mut hi) = (0.0, 1.0);
                for _ in 0..50 {
                    s = (lo + hi) / 2.0;
                    if b(x1, x2, s) < u { lo = s } else { hi = s }
                }
                b(y1, y2, s)
            }
        }
    }
}

/// Interpolate two values of one kind. Discrete kinds step at the end.
pub fn lerp(from: &Value, to: &Value, u: f64) -> Option<Value> {
    use Value::*;
    let l = |a: f64, b: f64| a + (b - a) * u;
    Some(match (from, to) {
        (Number(a), Number(b)) => Number(l(*a, *b)),
        (Vec2(a), Vec2(b)) => Vec2([l(a[0], b[0]), l(a[1], b[1])]),
        (Vec3(a), Vec3(b)) => Vec3([l(a[0], b[0]), l(a[1], b[1]), l(a[2], b[2])]),
        (Quantity { si: a, dim: da }, Quantity { si: b, dim: db }) if da == db => Quantity { si: l(*a, *b), dim: *da },
        (Color(a), Color(b)) => Color([
            l(a[0] as f64, b[0] as f64) as f32,
            l(a[1] as f64, b[1] as f64) as f32,
            l(a[2] as f64, b[2] as f64) as f32,
            l(a[3] as f64, b[3] as f64) as f32,
        ]),
        (a, b) if a.kind() == b.kind() => {
            if u < 1.0 {
                a.clone()
            } else {
                b.clone()
            }
        }
        _ => return None,
    })
}

/// One key of a clip, as stored.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyAt {
    pub id: NodeId,
    pub t: f64,
    pub value: Value,
    pub easing: String,
}

/// A clip's keys with constant values, in time order.
pub fn keys(doc: &Document, clip: NodeId) -> Vec<KeyAt> {
    let mut out: Vec<KeyAt> = doc
        .children(clip)
        .iter()
        .filter_map(|&k| {
            let key = doc.get::<Key>(k)?;
            Some(KeyAt {
                id: k,
                t: key.t.constant().and_then(|v| v.as_f64()).unwrap_or(0.0),
                value: key.value.constant()?.clone(),
                easing: key.easing.constant().and_then(|v| v.as_str()).unwrap_or("linear").to_owned(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.t.total_cmp(&b.t).then(a.id.cmp(&b.id)));
    out
}

/// A clip's start on the clock.
pub fn start(doc: &Document, clip: NodeId) -> f64 {
    doc.get::<Timing>(clip).and_then(|t| t.start.constant()).and_then(|v| v.as_f64()).unwrap_or(0.0)
}

/// The clip's value `u` seconds after its start: `None` before its first key
/// (it does not drive the leaf then) or with no keys.
pub fn value_at(keys: &[KeyAt], u: f64) -> Option<Value> {
    let first = keys.first()?;
    if u < first.t {
        return None;
    }
    let i = keys.iter().rposition(|k| k.t <= u).unwrap_or(0);
    let a = &keys[i];
    let Some(b) = keys.get(i + 1) else { return Some(a.value.clone()) };
    if a.easing == "hold" || b.t <= a.t {
        return Some(a.value.clone());
    }
    lerp(&a.value, &b.value, ease(&a.easing, (u - a.t) / (b.t - a.t)))
}

/// Where a clip's keys end, after its start (0 with none).
pub fn length(keys: &[KeyAt]) -> f64 {
    keys.iter().map(|k| k.t).fold(0.0, f64::max)
}

pub fn sample(doc: &Document, t: f64) -> Sample {
    let mut out = Sample::default();
    let Some(clip_ty) = TypeId::lookup("clip") else { return out };
    for clip_id in doc.nodes_of_type(clip_ty) {
        let Some(clip) = doc.get::<Clip>(clip_id) else { continue };
        let start = start(doc, clip_id);
        let prop = clip.prop.constant().and_then(|v| v.as_str()).unwrap_or("");
        let path: LeafPath = match prop.parse() {
            Ok(p) => p,
            Err(e) => {
                out.errors.insert(clip_id, format!("prop {prop:?}: {e}"));
                continue;
            }
        };
        let keys = keys(doc, clip_id);
        if keys.is_empty() {
            out.errors.insert(clip_id, "a clip needs keys".into());
            continue;
        }
        if let Some(k) = keys.iter().find(|k| k.value.kind() != keys[0].value.kind()) {
            out.errors.insert(clip_id, format!("its keys are {} and {}", keys[0].value.kind().name(), k.value.kind().name()));
            continue;
        }
        for rel in doc.relations_from(clip_id).filter(|r| r.rel == RelKind::Animates) {
            if !doc.is_live(rel.to) {
                continue;
            }
            let offset = rel.data.get("offset").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let leaf = Leaf { node: rel.to, path };
            match doc.leaf(rel.to, path) {
                None => {
                    out.errors.insert(clip_id, format!("{} has no {}", rel.to, path));
                    continue;
                }
                Some(slot) if slot.expr().is_some() => {
                    out.errors.insert(clip_id, format!("{leaf} is bound; a clip cannot also drive it"));
                    continue;
                }
                Some(slot) => {
                    let kind = slot.constant().map(|v| v.kind());
                    if Some(keys[0].value.kind()) != kind {
                        out.errors.insert(clip_id, format!("its keys are {}, but {leaf} is {}", keys[0].value.kind().name(), kind.map(|k| k.name()).unwrap_or("?")));
                        continue;
                    }
                }
            }
            if let Some(v) = value_at(&keys, t - start - offset) {
                out.values.insert(leaf, v);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_endpoints_and_midpoints() {
        for name in ["linear", "ease_in", "ease_out", "ease_in_out"] {
            assert_eq!(ease(name, 0.0), 0.0, "{name}");
            assert_eq!(ease(name, 1.0), 1.0, "{name}");
        }
        assert_eq!(ease("ease_in_out", 0.5), 0.5);
        assert!(ease("ease_in", 0.5) < 0.5);
        assert!(ease("ease_out", 0.5) > 0.5);
    }

    #[test]
    fn lerp_by_kind() {
        assert_eq!(lerp(&Value::Number(0.0), &Value::Number(10.0), 0.25), Some(Value::Number(2.5)));
        assert_eq!(lerp(&Value::Vec3([0.0; 3]), &Value::Vec3([2.0, 4.0, 6.0]), 0.5), Some(Value::Vec3([1.0, 2.0, 3.0])));
        assert_eq!(lerp(&Value::Bool(false), &Value::Bool(true), 0.9), Some(Value::Bool(false)));
        assert_eq!(lerp(&Value::Bool(false), &Value::Bool(true), 1.0), Some(Value::Bool(true)));
        assert_eq!(lerp(&Value::Number(0.0), &Value::Bool(true), 0.5), None);
    }
}
