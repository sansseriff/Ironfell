//! Scripted time (vision doc 05): clips on a track, sampled at a playhead.
//!
//! A clip is a node with `Timing { start, dur, easing }` and
//! `Clip { prop, from, to }`, linked to its targets by `animates` relations
//! whose `data.offset` staggers the start. Sampling produces, for one time,
//! the value each targeted leaf should show. Those values sit in the reactive
//! graph's animated layer, beneath the gesture overlay and above the authored
//! constant, so bindings that read an animated leaf follow it.
//!
//! Before a clip starts the leaf shows its authored value; after the clip
//! ends it holds `to`. Targets must be constants: a bound leaf has no slot
//! for a sampled value, and the conflict is reported rather than resolved.

use crate::facet::{Clip, LeafPath, Timing};
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
    let u = u.clamp(0.0, 1.0);
    match name {
        "ease_in" => u * u,
        "ease_out" => 1.0 - (1.0 - u) * (1.0 - u),
        "ease_in_out" => {
            if u < 0.5 {
                2.0 * u * u
            } else {
                1.0 - (-2.0 * u + 2.0).powi(2) / 2.0
            }
        }
        _ => u,
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

pub fn sample(doc: &Document, t: f64) -> Sample {
    let mut out = Sample::default();
    let Some(clip_ty) = TypeId::lookup("clip") else { return out };
    for clip_id in doc.nodes_of_type(clip_ty) {
        let (Some(timing), Some(clip)) = (doc.get::<Timing>(clip_id), doc.get::<Clip>(clip_id)) else {
            continue;
        };
        let num = |s: &crate::value::Slot, d: f64| s.constant().and_then(|v| v.as_f64()).unwrap_or(d);
        let start = num(&timing.start, 0.0);
        let dur = num(&timing.dur, 0.0);
        let easing = timing.easing.constant().and_then(|v| v.as_str()).unwrap_or("linear").to_owned();
        let prop = clip.prop.constant().and_then(|v| v.as_str()).unwrap_or("");
        let path: LeafPath = match prop.parse() {
            Ok(p) => p,
            Err(e) => {
                out.errors.insert(clip_id, format!("prop {prop:?}: {e}"));
                continue;
            }
        };
        let (Some(from), Some(to)) = (clip.from.constant(), clip.to.constant()) else {
            out.errors.insert(clip_id, "from/to must be constants".into());
            continue;
        };
        for rel in doc.relations_from(clip_id).filter(|r| r.rel == RelKind::Animates) {
            if !doc.is_live(rel.to) {
                continue;
            }
            let offset = rel.data.get("offset").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let s = start + offset;
            if t < s {
                continue;
            }
            let u = if dur <= 0.0 { 1.0 } else { ((t - s) / dur).min(1.0) };
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
                    if from.kind() != to.kind() || Some(from.kind()) != kind {
                        out.errors.insert(
                            clip_id,
                            format!(
                                "from/to are {}/{}, but {leaf} is {}",
                                from.kind().name(),
                                to.kind().name(),
                                kind.map(|k| k.name()).unwrap_or("?")
                            ),
                        );
                        continue;
                    }
                }
            }
            if let Some(v) = lerp(from, to, ease(&easing, u)) {
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
