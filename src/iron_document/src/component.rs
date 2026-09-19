//! Typed authored components and leaf paths.
//!
//! Components are Rust structs, not JSON bags, because the reconciler, the
//! floor and the layout solver all need typed access. Every field is a
//! [`Slot`], so any field can later become bound or animated without changing
//! its address. A leaf is addressed as `component.field`, e.g. `transform2d.x`
//! (plans/document-spine.md §5.3).
//!
//! The file format stays generic JSON regardless: each component serialises as
//! an object of its fields.

use crate::value::{Slot, Value, ValueKind};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

/// Implemented by every component struct. `KIND` and `FIELDS` are what the
/// registry, the validator and the view renderer read.
pub trait LeafStruct: Sized + Default + Clone {
    const KIND: ComponentKind;
    const FIELDS: &'static [(&'static str, ValueKind)];
    fn get(&self, field: &str) -> Option<&Slot>;
    fn get_mut(&mut self, field: &str) -> Option<&mut Slot>;
    fn wrap(self) -> Component;
    fn unwrap_ref(c: &Component) -> Option<&Self>;
}

macro_rules! components {
    ( $( $(#[$m:meta])* $variant:ident / $json:literal => $name:ident {
          $( $field:ident : $vk:ident = $default:expr ),* $(,)?
      } )* ) => {
        $(
            $(#[$m])*
            #[derive(Clone, Debug, PartialEq)]
            pub struct $name { $( pub $field: Slot, )* }

            impl Default for $name {
                fn default() -> Self {
                    Self { $( $field: Slot::Const($default), )* }
                }
            }

            impl LeafStruct for $name {
                const KIND: ComponentKind = ComponentKind::$variant;
                const FIELDS: &'static [(&'static str, ValueKind)] =
                    &[ $( (stringify!($field), ValueKind::$vk), )* ];
                fn get(&self, field: &str) -> Option<&Slot> {
                    match field { $( stringify!($field) => Some(&self.$field), )* _ => None }
                }
                fn get_mut(&mut self, field: &str) -> Option<&mut Slot> {
                    match field { $( stringify!($field) => Some(&mut self.$field), )* _ => None }
                }
                fn wrap(self) -> Component { Component::$variant(self) }
                fn unwrap_ref(c: &Component) -> Option<&Self> {
                    match c { Component::$variant(v) => Some(v), _ => None }
                }
            }
        )*

        /// The kind of a component, i.e. which struct it is.
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum ComponentKind { $( $variant, )* }

        impl ComponentKind {
            pub const ALL: &'static [ComponentKind] = &[ $( ComponentKind::$variant, )* ];

            pub fn name(self) -> &'static str {
                match self { $( ComponentKind::$variant => $json, )* }
            }

            pub fn from_name(s: &str) -> Option<ComponentKind> {
                match s { $( $json => Some(ComponentKind::$variant), )* _ => None }
            }

            pub fn fields(self) -> &'static [(&'static str, ValueKind)] {
                match self { $( ComponentKind::$variant => <$name as LeafStruct>::FIELDS, )* }
            }

            pub fn default_component(self) -> Component {
                match self { $( ComponentKind::$variant => Component::$variant($name::default()), )* }
            }
        }

        /// One authored component of a node.
        #[derive(Clone, Debug, PartialEq)]
        pub enum Component { $( $variant($name), )* }

        impl Component {
            pub fn kind(&self) -> ComponentKind {
                match self { $( Component::$variant(_) => ComponentKind::$variant, )* }
            }

            pub fn get(&self, field: &str) -> Option<&Slot> {
                match self { $( Component::$variant(v) => v.get(field), )* }
            }

            fn get_mut(&mut self, field: &str) -> Option<&mut Slot> {
                match self { $( Component::$variant(v) => v.get_mut(field), )* }
            }
        }
    };
}

components! {
    /// A human-readable name. Rendered as the `name` attribute in views.
    Name / "name" => Name { text: Str = Value::Str(String::new()) }

    /// Which renderer materialises this node: `canvas` or `dom`
    /// (plans/document-spine.md §4). Absent means the type's default.
    Host / "host" => Host { mode: Enum = Value::Enum("canvas".to_owned()) }

    /// Document 2D space: y-down, origin top-left, matching SVG and the
    /// DisplayList seam. Scalars, not a vector, because they are the leaves a
    /// binding or a slider most often addresses individually.
    Transform2d / "transform2d" => Transform2d {
        x: Number = Value::Number(0.0),
        y: Number = Value::Number(0.0),
        rot: Number = Value::Number(0.0),
        sx: Number = Value::Number(1.0),
        sy: Number = Value::Number(1.0),
    }

    /// 3D placement as vectors: a viewport drag writes one leaf.
    Transform3d / "transform3d" => Transform3d {
        pos: Vec3 = Value::Vec3([0.0, 0.0, 0.0]),
        rot: Vec3 = Value::Vec3([0.0, 0.0, 0.0]),
        scale: Vec3 = Value::Vec3([1.0, 1.0, 1.0]),
    }

    Size / "size" => Size {
        w: Number = Value::Number(100.0),
        h: Number = Value::Number(100.0),
    }

    Radius / "radius" => Radius { r: Number = Value::Number(50.0) }

    Fill / "fill" => Fill {
        color: Color = Value::Color([0.5, 0.5, 0.5, 1.0]),
        opacity: Number = Value::Number(1.0),
    }

    Stroke / "stroke" => Stroke {
        color: Color = Value::Color([0.0, 0.0, 0.0, 1.0]),
        width: Number = Value::Number(1.0),
    }

    /// Which mesh asset a `mesh` node shows.
    Mesh / "mesh" => Mesh { asset: Str = Value::Str("torus".to_owned()) }

    /// A scalar-on-a-track control (doc 02 §9.3). `value` is the leaf other
    /// properties will bind to.
    Slider / "slider" => Slider {
        min: Number = Value::Number(0.0),
        max: Number = Value::Number(1.0),
        step: Number = Value::Number(0.01),
        value: Number = Value::Number(0.5),
    }

    /// Scripted time (doc 05 §2): a start and a duration on a track.
    Timing / "timing" => Timing {
        start: Number = Value::Number(0.0),
        dur: Number = Value::Number(1.0),
        easing: Enum = Value::Enum("linear".to_owned()),
    }

    /// What a clip does to its target. Endpoints stay literal (doc 05 §2.1).
    /// The target itself is a relation, not a field (doc 05 §3).
    Clip / "clip" => Clip {
        prop: Str = Value::Str(String::new()),
        from: Number = Value::Number(0.0),
        to: Number = Value::Number(1.0),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeafError {
    UnknownComponent(String),
    UnknownField {
        component: ComponentKind,
        field: String,
    },
    TypeMismatch {
        path: LeafPath,
        expected: ValueKind,
        got: ValueKind,
    },
}

impl fmt::Display for LeafError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LeafError::UnknownComponent(c) => write!(f, "unknown component {c:?}"),
            LeafError::UnknownField { component, field } => {
                let available: Vec<&str> = component.fields().iter().map(|(n, _)| *n).collect();
                write!(
                    f,
                    "component {:?} has no field {:?}; available: {}",
                    component.name(),
                    field,
                    available.join(", ")
                )
            }
            LeafError::TypeMismatch {
                path,
                expected,
                got,
            } => {
                write!(
                    f,
                    "{path} expects {} but got {}",
                    expected.name(),
                    got.name()
                )
            }
        }
    }
}
impl std::error::Error for LeafError {}

impl Component {
    pub fn fields(&self) -> &'static [(&'static str, ValueKind)] {
        self.kind().fields()
    }

    /// Replace one field's slot, checking the value kind. Returns the previous
    /// slot, which is what an inverse op needs.
    pub fn set(&mut self, field: &str, slot: Slot) -> Result<Slot, LeafError> {
        let kind = self.kind();
        let expected = kind
            .fields()
            .iter()
            .find(|(n, _)| *n == field)
            .map(|(_, k)| *k)
            .ok_or_else(|| LeafError::UnknownField {
                component: kind,
                field: field.to_owned(),
            })?;
        // A binding's kind is only known when it is evaluated; the graph
        // reports a mismatch then. Constants are checked here.
        if let Some(v) = slot.constant()
            && v.kind() != expected
        {
            let path = LeafPath::new(kind, field).expect("field checked above");
            return Err(LeafError::TypeMismatch {
                path,
                expected,
                got: v.kind(),
            });
        }
        let target = self.get_mut(field).expect("field checked above");
        Ok(std::mem::replace(target, slot))
    }

    /// Object of `field: slot`, the file and op shape.
    pub fn to_json(&self) -> serde_json::Value {
        let mut m = serde_json::Map::new();
        for (name, _) in self.fields() {
            let slot = self.get(name).expect("declared field");
            m.insert(
                (*name).to_owned(),
                serde_json::to_value(slot).expect("slot serialises"),
            );
        }
        serde_json::Value::Object(m)
    }

    /// Inverse of [`Component::to_json`]. Missing fields take defaults;
    /// unknown fields are an error rather than silently dropped.
    pub fn from_json(kind: ComponentKind, v: &serde_json::Value) -> Result<Component, String> {
        let obj = v
            .as_object()
            .ok_or_else(|| format!("component {:?} must be an object", kind.name()))?;
        let mut c = kind.default_component();
        for (field, raw) in obj {
            // Through `Slot`, not `Value`, so `{"bind": …}` becomes a binding.
            let slot: Slot = serde_json::from_value(raw.clone())
                .map_err(|e| format!("{}.{field}: {e}", kind.name()))?;
            c.set(field, slot).map_err(|e| e.to_string())?;
        }
        Ok(c)
    }
}

impl Serialize for ComponentKind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for ComponentKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        ComponentKind::from_name(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown component kind {s:?}")))
    }
}

/// A component in an op serialises as `{"<kind>": {fields}}`.
impl Serialize for Component {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut m = s.serialize_map(Some(1))?;
        m.serialize_entry(self.kind().name(), &self.to_json())?;
        m.end()
    }
}

impl<'de> Deserialize<'de> for Component {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        let obj = raw
            .as_object()
            .filter(|o| o.len() == 1)
            .ok_or_else(|| serde::de::Error::custom("component must be a single-key object"))?;
        let (k, v) = obj.iter().next().expect("len 1");
        let kind = ComponentKind::from_name(k)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown component kind {k:?}")))?;
        Component::from_json(kind, v).map_err(serde::de::Error::custom)
    }
}

/// `component.field`, the address of one editable value.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LeafPath {
    pub component: ComponentKind,
    pub field: &'static str,
}

impl LeafPath {
    pub fn new(component: ComponentKind, field: &str) -> Result<LeafPath, LeafError> {
        component
            .fields()
            .iter()
            .find(|(n, _)| *n == field)
            .map(|(n, _)| LeafPath {
                component,
                field: n,
            })
            .ok_or_else(|| LeafError::UnknownField {
                component,
                field: field.to_owned(),
            })
    }

    pub fn kind(&self) -> ValueKind {
        self.component
            .fields()
            .iter()
            .find(|(n, _)| *n == self.field)
            .map(|(_, k)| *k)
            .expect("constructed from a declared field")
    }
}

impl FromStr for LeafPath {
    type Err = LeafError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (c, f) = s
            .split_once('.')
            .ok_or_else(|| LeafError::UnknownComponent(s.to_owned()))?;
        let component =
            ComponentKind::from_name(c).ok_or_else(|| LeafError::UnknownComponent(c.to_owned()))?;
        LeafPath::new(component, f)
    }
}

impl fmt::Display for LeafPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.component.name(), self.field)
    }
}

impl fmt::Debug for LeafPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Serialize for LeafPath {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for LeafPath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_checks_kind_and_returns_old() {
        let mut t = Transform2d::default().wrap();
        let old = t.set("x", Slot::Const(Value::Number(4.0))).unwrap();
        assert_eq!(old, Slot::Const(Value::Number(0.0)));
        assert_eq!(t.get("x"), Some(&Slot::Const(Value::Number(4.0))));
        let err = t.set("x", Slot::Const(Value::Bool(true))).unwrap_err();
        assert!(matches!(err, LeafError::TypeMismatch { .. }));
        assert!(matches!(
            t.set("nope", Slot::Const(Value::Number(1.0))),
            Err(LeafError::UnknownField { .. })
        ));
    }

    #[test]
    fn json_roundtrip() {
        let mut f = Fill::default().wrap();
        f.set("opacity", Value::Number(0.25).into()).unwrap();
        let j = serde_json::to_string(&f).unwrap();
        assert_eq!(
            j,
            r#"{"fill":{"color":{"rgba":[0.5,0.5,0.5,1.0]},"opacity":0.25}}"#
        );
        let back: Component = serde_json::from_str(&j).unwrap();
        assert_eq!(back, f);
        assert!(
            Component::from_json(ComponentKind::Fill, &serde_json::json!({"nope": 1})).is_err()
        );
    }

    #[test]
    fn leaf_path_parses() {
        let p: LeafPath = "transform2d.x".parse().unwrap();
        assert_eq!(p.component, ComponentKind::Transform2d);
        assert_eq!(p.kind(), ValueKind::Number);
        assert_eq!(p.to_string(), "transform2d.x");
        assert!("transform2d.q".parse::<LeafPath>().is_err());
        assert!("nope.x".parse::<LeafPath>().is_err());
        assert!("x".parse::<LeafPath>().is_err());
    }
}
