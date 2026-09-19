//! Leaf values and the slots that hold them.
//!
//! A [`Slot`] is what a leaf path resolves to. Today every slot is a constant.
//! The `Bound` and `Animated` variants are reserved so that adding the
//! evaluator later touches the evaluator, not every read site
//! (plans/document-spine.md §5.4). They carry [`Reserved`], which cannot be
//! constructed outside this crate and has no constructor inside it either.

use crate::id::NodeId;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    Bool(bool),
    Str(String),
    /// Linear RGBA, each channel in `0..=1`.
    Color([f32; 4]),
    Vec2([f64; 2]),
    Vec3([f64; 3]),
    /// One of a declared set of alternatives. Enumerated choice is a leaf kind
    /// in its own right (vision doc 06 §3).
    Enum(String),
    Ref(NodeId),
    // `Quantity { value, unit }` is deferred (doc 06 §5) and intentionally
    // absent rather than reserved: adding a variant here is cheap, and a
    // reserved variant would have to be handled by every match below.
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ValueKind {
    Number,
    Bool,
    Str,
    Color,
    Vec2,
    Vec3,
    Enum,
    Ref,
}

impl ValueKind {
    pub fn name(self) -> &'static str {
        match self {
            ValueKind::Number => "number",
            ValueKind::Bool => "bool",
            ValueKind::Str => "string",
            ValueKind::Color => "color",
            ValueKind::Vec2 => "vec2",
            ValueKind::Vec3 => "vec3",
            ValueKind::Enum => "enum",
            ValueKind::Ref => "ref",
        }
    }
}

impl Value {
    pub fn kind(&self) -> ValueKind {
        match self {
            Value::Number(_) => ValueKind::Number,
            Value::Bool(_) => ValueKind::Bool,
            Value::Str(_) => ValueKind::Str,
            Value::Color(_) => ValueKind::Color,
            Value::Vec2(_) => ValueKind::Vec2,
            Value::Vec3(_) => ValueKind::Vec3,
            Value::Enum(_) => ValueKind::Enum,
            Value::Ref(_) => ValueKind::Ref,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) | Value::Enum(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_vec2(&self) -> Option<[f64; 2]> {
        match self {
            Value::Vec2(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_vec3(&self) -> Option<[f64; 3]> {
        match self {
            Value::Vec3(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_color(&self) -> Option<[f32; 4]> {
        match self {
            Value::Color(c) => Some(*c),
            _ => None,
        }
    }

    pub fn as_ref(&self) -> Option<NodeId> {
        match self {
            Value::Ref(id) => Some(*id),
            _ => None,
        }
    }

    /// The JSON shape used in files and ops. Scalars and vectors are bare;
    /// colours, enums and refs are single-key objects so the kind survives the
    /// round trip.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("value serialisation is infallible")
    }

    pub fn from_json(v: &serde_json::Value) -> Result<Value, String> {
        use serde_json::Value as J;
        match v {
            J::Number(n) => n
                .as_f64()
                .map(Value::Number)
                .ok_or_else(|| "number out of range".to_owned()),
            J::Bool(b) => Ok(Value::Bool(*b)),
            J::String(s) => Ok(Value::Str(s.clone())),
            J::Array(items) => {
                let nums: Option<Vec<f64>> = items.iter().map(|i| i.as_f64()).collect();
                match nums.as_deref() {
                    Some([a, b]) => Ok(Value::Vec2([*a, *b])),
                    Some([a, b, c]) => Ok(Value::Vec3([*a, *b, *c])),
                    _ => Err("array values must be 2 or 3 numbers".to_owned()),
                }
            }
            J::Object(m) if m.len() == 1 => {
                let (k, inner) = m.iter().next().expect("len 1");
                match k.as_str() {
                    "rgba" => {
                        let ch: Option<Vec<f64>> = inner
                            .as_array()
                            .and_then(|a| a.iter().map(|x| x.as_f64()).collect());
                        match ch.as_deref() {
                            Some([r, g, b, a]) => {
                                Ok(Value::Color([*r as f32, *g as f32, *b as f32, *a as f32]))
                            }
                            _ => Err("rgba must be 4 numbers".to_owned()),
                        }
                    }
                    "enum" => inner
                        .as_str()
                        .map(|s| Value::Enum(s.to_owned()))
                        .ok_or_else(|| "enum must be a string".to_owned()),
                    "ref" => inner
                        .as_str()
                        .ok_or_else(|| "ref must be a string".to_owned())?
                        .parse()
                        .map(Value::Ref)
                        .map_err(|e| e.to_string()),
                    other => Err(format!("unknown value tag {other:?}")),
                }
            }
            _ => Err("unrecognised value shape".to_owned()),
        }
    }
}

impl fmt::Display for Value {
    /// Human and model facing rendering, used in views. Not the file format.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number(n) => write!(f, "{}", fmt_num(*n)),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Str(s) | Value::Enum(s) => write!(f, "{s}"),
            Value::Color(c) => {
                let ch = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
                write!(
                    f,
                    "#{:02x}{:02x}{:02x}{:02x}",
                    ch(c[0]),
                    ch(c[1]),
                    ch(c[2]),
                    ch(c[3])
                )
            }
            Value::Vec2([x, y]) => write!(f, "{},{}", fmt_num(*x), fmt_num(*y)),
            Value::Vec3([x, y, z]) => write!(f, "{},{},{}", fmt_num(*x), fmt_num(*y), fmt_num(*z)),
            Value::Ref(id) => write!(f, "{id}"),
        }
    }
}

/// Shortest faithful rendering: `120` not `120.0`, `0.1` not `0.10000000000000001`.
pub(crate) fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Number(n) => s.serialize_f64(*n),
            Value::Bool(b) => s.serialize_bool(*b),
            Value::Str(t) => s.serialize_str(t),
            Value::Vec2(v) => v.serialize(s),
            Value::Vec3(v) => v.serialize(s),
            Value::Color(c) => {
                let mut m = s.serialize_map(Some(1))?;
                m.serialize_entry("rgba", c)?;
                m.end()
            }
            Value::Enum(e) => {
                let mut m = s.serialize_map(Some(1))?;
                m.serialize_entry("enum", e)?;
                m.end()
            }
            Value::Ref(id) => {
                let mut m = s.serialize_map(Some(1))?;
                m.serialize_entry("ref", id)?;
                m.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        Value::from_json(&raw).map_err(serde::de::Error::custom)
    }
}

/// Unconstructible placeholder for the slot variants that exist only so the
/// enum's shape is settled now. No constructor, public or private.
#[derive(Clone, Debug, PartialEq)]
pub struct Reserved {
    _private: Uninhabited,
}

#[derive(Clone, Debug, PartialEq)]
enum Uninhabited {}

#[derive(Clone, Debug, PartialEq)]
pub enum Slot {
    Const(Value),
    /// Reserved: the value is an expression over other leaves (doc 02 §4).
    Bound(Reserved),
    /// Reserved: the value is driven by clips through relations (doc 05).
    Animated(Reserved),
}

impl Slot {
    pub fn constant(&self) -> Option<&Value> {
        match self {
            Slot::Const(v) => Some(v),
            Slot::Bound(r) | Slot::Animated(r) => match r._private {},
        }
    }

    /// The kind this slot produces. For reserved variants this is the kind of
    /// the value they would resolve to; unreachable today.
    pub fn kind(&self) -> ValueKind {
        match self {
            Slot::Const(v) => v.kind(),
            Slot::Bound(r) | Slot::Animated(r) => match r._private {},
        }
    }
}

impl From<Value> for Slot {
    fn from(v: Value) -> Self {
        Slot::Const(v)
    }
}

impl Serialize for Slot {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Slot::Const(v) => v.serialize(s),
            Slot::Bound(r) | Slot::Animated(r) => match r._private {},
        }
    }
}

impl<'de> Deserialize<'de> for Slot {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d).map(Slot::Const)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip_preserves_kind() {
        let cases = [
            Value::Number(1.5),
            Value::Number(120.0),
            Value::Bool(true),
            Value::Str("hi".into()),
            Value::Color([0.0, 0.5, 1.0, 1.0]),
            Value::Vec2([1.0, 2.0]),
            Value::Vec3([1.0, 2.0, 3.0]),
            Value::Enum("linear".into()),
            Value::Ref(NodeId::from_raw(9)),
        ];
        for v in cases {
            let j = serde_json::to_string(&v).unwrap();
            let back: Value = serde_json::from_str(&j).unwrap();
            assert_eq!(back, v, "via {j}");
            assert_eq!(back.kind(), v.kind());
        }
    }

    #[test]
    fn display_is_short() {
        assert_eq!(Value::Number(120.0).to_string(), "120");
        assert_eq!(Value::Number(0.1).to_string(), "0.1");
        assert_eq!(Value::Color([1.0, 0.0, 0.0, 1.0]).to_string(), "#ff0000ff");
        assert_eq!(Value::Vec2([1.0, 2.5]).to_string(), "1,2.5");
    }
}
