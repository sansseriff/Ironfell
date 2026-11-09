//! Declared types at a component's boundary (plans/specs/types-and-units.md):
//! what an input or output port carries, written in `port.datatype` as a small
//! grammar (`nm`, `length`, `choice(hidden|shown)`, `like(xs) | auto`, …).
//!
//! Strict at the boundary, loose inside: formulas stay dynamically typed;
//! a declared type checks a value set on an argument, a value arriving at a
//! port, and a connection between two declared ports. An undeclared port is
//! *loose* and never an obstacle (§3.5, gradual typing): the empty text.

use crate::units::{self, Dim};
use crate::value::Value;
use std::fmt;

/// What a port carries.
#[derive(Clone, Debug, PartialEq)]
pub enum Base {
    /// Anything (also what an undeclared port is).
    Any,
    /// A plain number.
    Number,
    /// A quantity of one dimension, shown in `unit` when given.
    Quantity { dim: Dim, unit: Option<String> },
    /// A quantity of the dimension another input's value has (`like(xs)`:
    /// a plot's range is in its data's unit, whatever that is).
    Like(String),
    /// A series, of one dimension when given.
    Series(Option<Dim>),
    /// One of these words.
    Choice(Vec<String>),
    Bool,
    Color,
    Text,
    /// A function of this many arguments.
    Fn(usize),
}

/// A declared type: a base, and whether `auto` (stored as NaN) is allowed.
#[derive(Clone, Debug, PartialEq)]
pub struct Type {
    pub base: Base,
    pub auto: bool,
}

/// Named dimensions (the coherent units' quantities).
const DIMS: &[(&str, &str)] = &[
    ("length", "m"),
    ("mass", "kg"),
    ("time", "s"),
    ("current", "A"),
    ("temperature", "K"),
    ("amount", "mol"),
    ("luminosity", "cd"),
    ("frequency", "Hz"),
    ("force", "N"),
    ("pressure", "Pa"),
    ("energy", "J"),
    ("power", "W"),
    ("charge", "C"),
    ("voltage", "V"),
    ("capacitance", "F"),
    ("resistance", "Ω"),
    ("area", "m2"),
    ("volume", "m3"),
    ("speed", "m/s"),
];

fn dim_name(dim: Dim) -> String {
    DIMS.iter()
        .find(|(_, u)| units::parse_unit(u).is_ok_and(|x| x.dim == dim))
        .map(|(n, _)| (*n).to_owned())
        .unwrap_or_else(|| format!("quantity in {}", dim.canonical_unit()))
}

/// A dimension by name (`length`) or by any unit of it (`nm`).
fn parse_dim(text: &str) -> Option<(Dim, Option<String>)> {
    let t = text.trim();
    if let Some((_, u)) = DIMS.iter().find(|(n, _)| *n == t) {
        return Some((units::parse_unit(u).ok()?.dim, None));
    }
    let u = units::parse_unit(t).ok()?;
    (!u.dim.is_none()).then(|| (u.dim, Some(t.to_owned())))
}

impl Type {
    pub const ANY: Type = Type { base: Base::Any, auto: false };

    /// Parse `datatype` text; empty is loose (`any`).
    pub fn parse(text: &str) -> Result<Type, String> {
        let t = text.trim();
        if t.is_empty() {
            return Ok(Type::ANY);
        }
        // `… | auto`: the last `|` outside parentheses.
        let mut depth = 0i32;
        let mut bar = None;
        for (i, c) in t.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                '|' if depth == 0 => bar = Some(i),
                _ => {}
            }
        }
        let (body, auto) = match bar {
            Some(i) if t[i + 1..].trim() == "auto" => (t[..i].trim(), true),
            Some(_) => return Err(format!("{t:?}: only `| auto` may follow a type")),
            None => (t, false),
        };
        let inner = |name: &str| body.strip_prefix(name).and_then(|r| r.trim().strip_prefix('(')).and_then(|r| r.strip_suffix(')')).map(str::trim);
        let base = match body {
            "any" => Base::Any,
            "number" => Base::Number,
            "bool" => Base::Bool,
            "color" | "colour" => Base::Color,
            "text" => Base::Text,
            "series" => Base::Series(None),
            _ if inner("series").is_some() => {
                let d = inner("series").expect("checked");
                if d == "any" || d.is_empty() { Base::Series(None) } else { Base::Series(Some(parse_dim(d).ok_or_else(|| format!("series({d}): not a dimension or unit"))?.0)) }
            }
            _ if inner("choice").is_some() => {
                let opts: Vec<String> = inner("choice").expect("checked").split('|').map(|o| o.trim().to_owned()).filter(|o| !o.is_empty()).collect();
                if opts.is_empty() {
                    return Err("choice() needs at least one option: choice(a|b)".into());
                }
                Base::Choice(opts)
            }
            _ if inner("fn").is_some() => Base::Fn(inner("fn").expect("checked").parse().map_err(|_| format!("{body}: fn(n) takes a count of arguments"))?),
            _ if inner("like").is_some() => Base::Like(inner("like").expect("checked").to_owned()),
            _ => {
                // `length in nm`, `length`, or a unit (`nm`).
                let (d, u) = match body.split_once(" in ") {
                    Some((d, u)) => {
                        let (dim, _) = parse_dim(d).ok_or_else(|| format!("{d:?} is not a dimension"))?;
                        let unit = units::parse_unit(u).map_err(|e| e.to_string())?;
                        if unit.dim != dim {
                            return Err(format!("{u} is not a unit of {d}"));
                        }
                        (dim, Some(u.trim().to_owned()))
                    }
                    None => parse_dim(body).ok_or_else(|| format!("{body:?} is not a type: number, a unit or dimension (nm, length), series, choice(a|b), bool, color, text, fn(n), like(input), any; add | auto"))?,
                };
                Base::Quantity { dim: d, unit: u }
            }
        };
        Ok(Type { base, auto })
    }

    pub fn is_loose(&self) -> bool {
        self.base == Base::Any && !self.auto
    }

    /// The options of a choice.
    pub fn options(&self) -> Option<&[String]> {
        match &self.base {
            Base::Choice(o) => Some(o),
            _ => None,
        }
    }

    /// The unit a quantity is shown and typed in.
    pub fn display_unit(&self) -> Option<&str> {
        match &self.base {
            Base::Quantity { unit: Some(u), .. } => Some(u),
            _ => None,
        }
    }

    /// `v` as this type takes it, or why not. `like` reads the named
    /// input's value through `other`.
    pub fn check(&self, v: &Value, other: &dyn Fn(&str) -> Option<Value>) -> Result<Value, String> {
        if self.auto && matches!(v, Value::Number(x) if x.is_nan()) {
            return Ok(v.clone());
        }
        if self.auto && v.as_str() == Some("auto") {
            return Ok(Value::Number(f64::NAN));
        }
        let want = || format!("expects {self}, got {}", describe(v));
        match &self.base {
            Base::Any => Ok(v.clone()),
            Base::Number => match v {
                Value::Number(_) => Ok(v.clone()),
                Value::Quantity { dim, .. } if dim.is_none() => Ok(v.clone()),
                _ => Err(want()),
            },
            Base::Quantity { dim, unit } => match v.as_quantity() {
                Some((_, d)) if d == *dim => Ok(v.clone()),
                // A bare number is in the display unit (`1550` into `nm`).
                Some((x, d)) if d.is_none() && matches!(v, Value::Number(_)) => match unit {
                    Some(u) => Ok(Value::quantity(x * units::parse_unit(u).map_err(|e| e.to_string())?.factor, *dim)),
                    None => Err(format!("{}: write it with a unit (3 mm)", want())),
                },
                _ => Err(want()),
            },
            Base::Like(name) => {
                let Some(model) = other(name) else { return Ok(v.clone()) };
                let dim = model.as_series().map(|(_, d)| d).or_else(|| model.as_quantity().map(|(_, d)| d));
                let got = v.as_quantity().map(|(_, d)| d);
                match (dim, got) {
                    // Plain data has no unit to match (a function plot's
                    // range sets its own, beside the default sample).
                    (Some(d), _) if d.is_none() => Ok(v.clone()),
                    (Some(d), Some(g)) if d == g => Ok(v.clone()),
                    (Some(d), Some(_)) => Err(format!("expects a value in {name}'s unit ({}), got {}", if d.is_none() { "a plain number".into() } else { d.canonical_unit() }, describe(v))),
                    (None, _) => Ok(v.clone()),
                    _ => Err(want()),
                }
            }
            Base::Series(dim) => match v.as_series() {
                Some((_, d)) if dim.is_none_or(|w| w == d) => Ok(v.clone()),
                _ => Err(want()),
            },
            Base::Choice(opts) => match v.as_str() {
                Some(s) if opts.iter().any(|o| o == s) => Ok(Value::Enum(s.to_owned())),
                Some(s) => Err(format!("is one of {}, not {s:?}", opts.join(", "))),
                None => Err(want()),
            },
            Base::Bool => match v {
                Value::Bool(_) => Ok(v.clone()),
                _ => match v.as_str() {
                    Some("true") => Ok(Value::Bool(true)),
                    Some("false") => Ok(Value::Bool(false)),
                    _ => Err(want()),
                },
            },
            Base::Color => match v {
                Value::Color(_) => Ok(v.clone()),
                _ => Err(want()),
            },
            Base::Text => match v {
                Value::Str(_) => Ok(v.clone()),
                Value::Enum(s) => Ok(Value::Str(s.clone())),
                _ => Err(want()),
            },
            Base::Fn(n) => match v {
                Value::Fn(c) if c.params.len() == *n => Ok(v.clone()),
                Value::Fn(c) => Err(format!("expects a function of {n}, got one of {}", c.params.len())),
                _ => Err(want()),
            },
        }
    }

    /// Whether an out port of type `self` may feed an in port of type
    /// `into` (§3.5): loose either side always may; two declared types must
    /// agree. `Err` says why not.
    pub fn feeds(&self, into: &Type) -> Result<(), String> {
        use Base::*;
        let ok = match (&self.base, &into.base) {
            (Any, _) | (_, Any) | (Like(_), _) | (_, Like(_)) => true,
            (Quantity { dim: a, .. }, Quantity { dim: b, .. }) => a == b,
            (Number, Quantity { dim, .. }) | (Quantity { dim, .. }, Number) => dim.is_none(),
            (Series(a), Series(b)) => a.is_none() || b.is_none() || a == b,
            (Choice(a), Choice(b)) => a.iter().all(|o| b.contains(o)),
            (Choice(_), Text) => true,
            (Fn(a), Fn(b)) => a == b,
            (a, b) => std::mem::discriminant(a) == std::mem::discriminant(b),
        };
        if ok { Ok(()) } else { Err(format!("{into} cannot take {self}")) }
    }

    /// The type a value suggests (the inspector's "infer from the value").
    pub fn of_value(v: &Value) -> Type {
        let base = match v {
            Value::Number(x) if x.is_nan() => return Type { base: Base::Number, auto: true },
            Value::Number(_) => Base::Number,
            Value::Quantity { dim, .. } => Base::Quantity { dim: *dim, unit: Some(units::display_unit(v.as_quantity().map_or(0.0, |q| q.0), *dim).0) },
            Value::Series { dim, .. } => Base::Series(Some(*dim)),
            Value::Enum(s) => Base::Choice(vec![s.clone()]),
            Value::Str(_) => Base::Text,
            Value::Bool(_) => Base::Bool,
            Value::Color(_) => Base::Color,
            Value::Fn(c) => Base::Fn(c.params.len()),
            _ => Base::Any,
        };
        Type { base, auto: false }
    }
}

fn describe(v: &Value) -> String {
    match v {
        Value::Quantity { dim, .. } => format!("{} ({})", dim_name(*dim), v),
        Value::Series { dim, .. } if !dim.is_none() => format!("a series of {}", dim_name(*dim)),
        Value::Enum(s) | Value::Str(s) => format!("{s:?}"),
        v => format!("{} {v}", v.kind().name()),
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.base {
            Base::Any => f.write_str("any")?,
            Base::Number => f.write_str("number")?,
            Base::Quantity { dim, unit: Some(u) } => write!(f, "{} in {u}", dim_name(*dim))?,
            Base::Quantity { dim, unit: None } => f.write_str(&dim_name(*dim))?,
            Base::Like(n) => write!(f, "like({n})")?,
            Base::Series(None) => f.write_str("series")?,
            Base::Series(Some(d)) => write!(f, "series({})", dim_name(*d))?,
            Base::Choice(o) => write!(f, "choice({})", o.join("|"))?,
            Base::Bool => f.write_str("bool")?,
            Base::Color => f.write_str("color")?,
            Base::Text => f.write_str("text")?,
            Base::Fn(n) => write!(f, "fn({n})")?,
        }
        if self.auto {
            f.write_str(" | auto")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(t: &str) -> Value {
        Value::parse_quantity(t).unwrap()
    }

    #[test]
    fn the_grammar_round_trips() {
        for t in ["nm", "length", "length in nm", "choice(hidden|shown)", "like(xs) | auto", "fn(1) | auto", "series(length)", "bool", "text", "color", "number", "any", "series"] {
            let ty = Type::parse(t).unwrap_or_else(|e| panic!("{t}: {e}"));
            assert_eq!(Type::parse(&ty.to_string()).unwrap(), ty, "{t} → {ty}");
        }
        assert!(Type::parse("bogus").is_err());
        assert!(Type::parse("seconds in nm").is_err());
        assert!(Type::parse("").unwrap().is_loose());
    }

    #[test]
    fn checks_and_coercions() {
        let none = |_: &str| None;
        let vis = Type::parse("choice(hidden|shown)").unwrap();
        assert_eq!(vis.check(&Value::Str("hidden".into()), &none), Ok(Value::Enum("hidden".into())));
        assert!(vis.check(&Value::Str("hiden".into()), &none).unwrap_err().contains("hidden, shown"));
        let nm = Type::parse("nm").unwrap();
        assert_eq!(nm.check(&Value::Number(1550.0), &none).unwrap().as_quantity().unwrap().1, Dim::LENGTH, "a bare number is in nm");
        assert!(nm.check(&q("3 s"), &none).is_err());
        let range = Type::parse("like(xs) | auto").unwrap();
        let xs = |n: &str| (n == "xs").then(|| Value::series(vec![0.0, 1.0], Dim::LENGTH));
        assert!(range.check(&q("2 mm"), &xs).is_ok());
        assert!(range.check(&Value::Number(0.0), &xs).unwrap_err().contains("xs's unit"));
        let plain = |n: &str| (n == "xs").then(|| Value::series(vec![0.0, 1.0], Dim::NONE));
        assert!(range.check(&q("2 mm"), &plain).is_ok(), "plain data has no unit to match");
        assert!(range.check(&Value::Number(f64::NAN), &xs).is_ok(), "auto");
        assert!(range.check(&Value::Str("auto".into()), &xs).unwrap().as_f64().unwrap().is_nan());
    }

    #[test]
    fn connections_are_gradual() {
        let t = |s: &str| Type::parse(s).unwrap();
        assert!(Type::ANY.feeds(&t("nm")).is_ok(), "loose into typed: checked on arrival");
        assert!(t("nm").feeds(&Type::ANY).is_ok());
        assert!(t("mm").feeds(&t("length")).is_ok());
        assert!(t("s").feeds(&t("length")).is_err());
        assert!(t("choice(a)").feeds(&t("choice(a|b)")).is_ok());
        assert!(t("choice(a|c)").feeds(&t("choice(a|b)")).is_err());
    }
}

/// The declared type of `leaf`, if it is a boundary leaf with one: an
/// entity's argument (its definition's input `port.datatype`), or a port's
/// signal (the port's own, copied from its spec). The entity whose
/// arguments `like(…)` reads comes with it.
pub fn declared(doc: &crate::document::Document, leaf: crate::expr::Leaf) -> Option<(Type, crate::id::NodeId)> {
    use crate::facet::{FacetKind, Name, Port};
    let parse = |p: &Port| p.datatype.constant().and_then(|v| v.as_str()).filter(|t| !t.trim().is_empty()).and_then(|t| Type::parse(t).ok());
    match leaf.path.facet {
        FacetKind::Args => {
            let def = crate::expand::linked_definition(doc, leaf.node).or_else(|| {
                doc.get::<crate::facet::Instance>(leaf.node).and_then(|i| i.def.constant()).and_then(|v| v.as_ref())
            })?;
            let spec = doc.children(def).iter().copied().find(|&c| {
                doc.get::<Name>(c).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(leaf.path.field) && doc.get::<Port>(c).is_some()
            })?;
            Some((parse(doc.get::<Port>(spec)?)?, leaf.node))
        }
        FacetKind::Port if leaf.path.field == "signal" => {
            let ty = parse(doc.get::<Port>(leaf.node)?)?;
            let owner = doc.live(leaf.node).and_then(|n| n.parent).unwrap_or(leaf.node);
            Some((ty, owner))
        }
        _ => None,
    }
}
