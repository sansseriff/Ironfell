//! Leaf values and the slots that hold them.
//!
//! A [`Slot`] is what a leaf path resolves to: a constant, or an expression
//! over other leaves that the reactive graph (`graph.rs`) keeps resolved. The
//! `Animated` variant is reserved so that adding clip-driven values later
//! touches the evaluator, not every read site (plans/document-spine.md §5.4).
//! It carries [`Reserved`], which cannot be constructed anywhere.

use crate::expr::Expr;
use crate::id::NodeId;
use crate::sym::intern;
use crate::units::{self, Dim};
use std::collections::BTreeMap;
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
    /// A physical quantity: magnitude in SI base units plus its dimension
    /// (entity-composition-model.md §3.1). The display unit is presentation.
    /// Never dimensionless: that is a plain `Number`; see [`Value::quantity`].
    Quantity { si: f64, dim: Dim },
    /// A small structured value (a beam, a pulse shape), ECM §3.2. `kind`
    /// names its schema; fields are read with `Expr::Get` (`beam.waist`).
    /// Field order is canonical (sorted), so files and views are stable.
    Record { kind: &'static str, fields: BTreeMap<&'static str, Value> },
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
    Quantity,
    Record,
    /// Only as a declared field kind: the field accepts any value. Used by
    /// clip endpoints, whose kind is the target leaf's. No value has it.
    Any,
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
            ValueKind::Quantity => "quantity",
            ValueKind::Record => "record",
            ValueKind::Any => "any",
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
            Value::Quantity { .. } => ValueKind::Quantity,
            Value::Record { .. } => ValueKind::Record,
        }
    }

    /// A record of `kind` with the given fields (names interned).
    pub fn record(kind: &str, fields: impl IntoIterator<Item = (impl AsRef<str>, Value)>) -> Value {
        Value::Record {
            kind: intern(kind),
            fields: fields.into_iter().map(|(k, v)| (intern(k.as_ref()), v)).collect(),
        }
    }

    pub fn field(&self, name: &str) -> Option<&Value> {
        match self {
            Value::Record { fields, .. } => fields.get(name),
            _ => None,
        }
    }

    /// A quantity, or a plain number when the dimension cancels: plain
    /// numbers are dimensionless, so `1 W / 1 W` and `5 deg` are numbers.
    pub fn quantity(si: f64, dim: Dim) -> Value {
        if dim.is_none() { Value::Number(si) } else { Value::Quantity { si, dim } }
    }

    /// Magnitude and dimension of a number (dimensionless) or a quantity.
    pub fn as_quantity(&self) -> Option<(f64, Dim)> {
        match self {
            Value::Number(n) => Some((*n, Dim::NONE)),
            Value::Quantity { si, dim } => Some((*si, *dim)),
            _ => None,
        }
    }

    /// What a type error calls this value: its unit, or "a plain number".
    pub fn describe(&self) -> String {
        match self {
            Value::Number(_) => "a plain number".to_owned(),
            Value::Quantity { dim, .. } => dim.canonical_unit(),
            v => v.kind().name().to_owned(),
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
                    // `{"q": [si, "W"]}` is the file form; `{"q": "9.58 mW"}`
                    // is accepted from the model surface, which does not
                    // carry the unit table.
                    "q" => match inner {
                        J::Array(a) => match a.as_slice() {
                            [n, J::String(u)] => {
                                let si = n.as_f64().ok_or_else(|| "quantity magnitude must be a number".to_owned())?;
                                let unit = units::parse_unit(u).map_err(|e| e.to_string())?;
                                Ok(Value::quantity(si * unit.factor, unit.dim))
                            }
                            _ => Err("q must be [magnitude, \"unit\"]".to_owned()),
                        },
                        J::String(s) => Value::parse_quantity(s),
                        _ => Err("q must be [magnitude, \"unit\"] or \"magnitude unit\"".to_owned()),
                    },
                    // `{"rec": ["kind", {field: value, …}]}`.
                    "rec" => match inner.as_array().map(Vec::as_slice) {
                        Some([J::String(kind), J::Object(fields)]) => {
                            let mut out = BTreeMap::new();
                            for (k, v) in fields {
                                out.insert(intern(k), Value::from_json(v).map_err(|e| format!("{kind}.{k}: {e}"))?);
                            }
                            Ok(Value::Record { kind: intern(kind), fields: out })
                        }
                        _ => Err("rec must be [\"kind\", {fields}]".to_owned()),
                    },
                    other => Err(format!("unknown value tag {other:?}")),
                }
            }
            _ => Err("unrecognised value shape".to_owned()),
        }
    }
}

impl Value {
    /// `"9.58 mW"`, `"1550nm"`, `"3 kg*m2/s3"`: a magnitude in any unit.
    pub fn parse_quantity(text: &str) -> Result<Value, String> {
        let text = text.trim();
        let split = text
            .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
            .unwrap_or(text.len());
        // `e` may start a unit (eV) rather than an exponent: back off if the
        // number does not parse.
        let (num, unit) = (0..=split)
            .rev()
            .map(|i| text.split_at(i))
            .find(|(n, _)| !n.is_empty() && n.parse::<f64>().is_ok())
            .ok_or_else(|| format!("{text:?} is not a quantity"))?;
        let n: f64 = num.parse().expect("checked");
        let u = units::parse_unit(unit.trim()).map_err(|e| e.to_string())?;
        Ok(Value::quantity(n * u.factor, u.dim))
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
            Value::Quantity { si, dim } => f.write_str(&units::format(*si, *dim)),
            Value::Record { kind, fields } => {
                write!(f, "{kind}{{")?;
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{k}={v}")?;
                }
                f.write_str("}")
            }
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
            Value::Quantity { si, dim } => {
                let mut m = s.serialize_map(Some(1))?;
                m.serialize_entry("q", &(si, dim.canonical_unit()))?;
                m.end()
            }
            Value::Record { kind, fields } => {
                let fields: BTreeMap<&str, &Value> = fields.iter().map(|(k, v)| (*k, v)).collect();
                let mut m = s.serialize_map(Some(1))?;
                m.serialize_entry("rec", &(kind, fields))?;
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
    /// An expression over other leaves (doc 02 §4). Its value is derived and
    /// lives in the reactive graph, never here.
    Bound(Expr),
    /// Reserved: the value is driven by clips through relations (doc 05).
    Animated(Reserved),
}

impl Slot {
    /// The authored constant, if this slot is one. Bound slots answer `None`;
    /// their current value comes from `Store::resolved`.
    pub fn constant(&self) -> Option<&Value> {
        match self {
            Slot::Const(v) => Some(v),
            Slot::Bound(_) => None,
            Slot::Animated(r) => match r._private {},
        }
    }

    pub fn expr(&self) -> Option<&Expr> {
        match self {
            Slot::Bound(e) => Some(e),
            _ => None,
        }
    }
}

impl From<Value> for Slot {
    fn from(v: Value) -> Self {
        Slot::Const(v)
    }
}

/// A constant serialises as its value; a binding as `{"bind": "<expr>"}`.
impl Serialize for Slot {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Slot::Const(v) => v.serialize(s),
            Slot::Bound(e) => {
                let mut m = s.serialize_map(Some(1))?;
                m.serialize_entry("bind", &e.to_string())?;
                m.end()
            }
            Slot::Animated(r) => match r._private {},
        }
    }
}

impl<'de> Deserialize<'de> for Slot {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        if let Some(text) = raw.as_object().filter(|o| o.len() == 1).and_then(|o| o.get("bind")).and_then(|b| b.as_str()) {
            return Expr::parse(text).map(Slot::Bound).map_err(serde::de::Error::custom);
        }
        Value::from_json(&raw).map(Slot::Const).map_err(serde::de::Error::custom)
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
            Value::parse_quantity("9.58 mW").unwrap(),
            Value::parse_quantity("3 kg*m2/s3").unwrap(),
            Value::parse_quantity("1/s2").unwrap(),
            Value::record("gaussian_beam", [("waist", Value::parse_quantity("30 um").unwrap()), ("m2", Value::Number(1.1))]),
        ];
        for v in cases {
            let j = serde_json::to_string(&v).unwrap();
            let back: Value = serde_json::from_str(&j).unwrap();
            assert_eq!(back, v, "via {j}");
            assert_eq!(back.kind(), v.kind());
        }
        assert_eq!(serde_json::to_string(&Value::parse_quantity("9.58 mW").unwrap()).unwrap(), r#"{"q":[0.00958,"W"]}"#);
        let (si, dim) = Value::from_json(&serde_json::json!({"q": "1550 nm"})).unwrap().as_quantity().unwrap();
        assert!((si - 1.55e-6).abs() < 1e-21 && dim == Dim::LENGTH);
        assert_eq!(Value::parse_quantity("90 deg").unwrap(), Value::Number(std::f64::consts::FRAC_PI_2));
        assert_eq!(Value::parse_quantity("1 eV").unwrap().kind(), ValueKind::Quantity);
        assert!(Value::parse_quantity("fast").is_err());
        assert!(Value::parse_quantity("3 parsecs").is_err());
    }

    #[test]
    fn display_is_short() {
        assert_eq!(Value::Number(120.0).to_string(), "120");
        assert_eq!(Value::Number(0.1).to_string(), "0.1");
        assert_eq!(Value::Color([1.0, 0.0, 0.0, 1.0]).to_string(), "#ff0000ff");
        assert_eq!(Value::Vec2([1.0, 2.5]).to_string(), "1,2.5");
        assert_eq!(Value::parse_quantity("0.00958 W").unwrap().to_string(), "9.58 mW");
        assert_eq!(Value::parse_quantity("1550 nm").unwrap().to_string(), "1.55 µm");
        let beam = Value::record("gaussian_beam", [("waist", Value::parse_quantity("30 um").unwrap()), ("m2", Value::Number(1.1))]);
        assert_eq!(beam.to_string(), "gaussian_beam{m2=1.1, waist=30 µm}");
        assert_eq!(serde_json::to_string(&beam).unwrap(), r#"{"rec":["gaussian_beam",{"m2":1.1,"waist":{"q":[0.000029999999999999997,"m"]}}]}"#);
    }
}
