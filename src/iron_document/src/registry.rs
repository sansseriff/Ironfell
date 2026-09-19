//! The node-type registry: one table read by the validator, the reconciler,
//! the model-facing manifest and the DOM host (plans/document-spine.md §5.7).
//!
//! Adding a node type is a Rust change. That is the correct place for it: the
//! vocabulary is code, the bindings over it are data (doc 02 §9.6).

use crate::component::ComponentKind;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TypeId(u16);

/// Which renderer materialises a node by default (§4).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Host {
    Canvas,
    Dom,
}

impl Host {
    pub fn name(self) -> &'static str {
        match self {
            Host::Canvas => "canvas",
            Host::Dom => "dom",
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum ChildPolicy {
    None,
    Any,
    Only(&'static [&'static str]),
}

pub struct TypeSpec {
    pub name: &'static str,
    /// Allowed components, with whether each is required on create.
    pub components: &'static [(ComponentKind, bool)],
    pub default_host: Host,
    pub children: ChildPolicy,
}

use ComponentKind as K;

static TYPES: &[TypeSpec] = &[
    TypeSpec {
        name: "group",
        components: &[(K::Name, false), (K::Transform2d, false), (K::Host, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::Any,
    },
    TypeSpec {
        name: "rect",
        components: &[
            (K::Transform2d, true),
            (K::Size, true),
            (K::Fill, false),
            (K::Stroke, false),
            (K::Name, false),
            (K::Host, false),
        ],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
    TypeSpec {
        name: "circle",
        components: &[
            (K::Transform2d, true),
            (K::Radius, true),
            (K::Fill, false),
            (K::Stroke, false),
            (K::Name, false),
        ],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
    TypeSpec {
        name: "mesh",
        components: &[(K::Transform3d, true), (K::Mesh, true), (K::Name, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
    TypeSpec {
        name: "slider",
        components: &[
            (K::Transform2d, true),
            (K::Slider, true),
            (K::Size, false),
            (K::Name, false),
            (K::Host, false),
        ],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
    TypeSpec {
        name: "bar",
        components: &[
            (K::Transform2d, true),
            (K::Size, true),
            (K::Fill, false),
            (K::Name, false),
        ],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
    TypeSpec {
        name: "timeline",
        components: &[(K::Name, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::Only(&["clip"]),
    },
    TypeSpec {
        name: "clip",
        components: &[(K::Timing, true), (K::Clip, true), (K::Name, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
];

impl TypeId {
    pub fn lookup(name: &str) -> Option<TypeId> {
        TYPES
            .iter()
            .position(|t| t.name == name)
            .map(|i| TypeId(i as u16))
    }

    pub fn spec(self) -> &'static TypeSpec {
        &TYPES[self.0 as usize]
    }

    pub fn name(self) -> &'static str {
        self.spec().name
    }

    pub fn all() -> impl Iterator<Item = TypeId> {
        (0..TYPES.len()).map(|i| TypeId(i as u16))
    }
}

impl TypeSpec {
    pub fn allows(&self, kind: ComponentKind) -> bool {
        self.components.iter().any(|(k, _)| *k == kind)
    }

    pub fn requires(&self, kind: ComponentKind) -> bool {
        self.components.iter().any(|(k, req)| *k == kind && *req)
    }

    pub fn required(&self) -> impl Iterator<Item = ComponentKind> + '_ {
        self.components.iter().filter(|(_, r)| *r).map(|(k, _)| *k)
    }

    pub fn admits_child(&self, child: TypeId) -> bool {
        match self.children {
            ChildPolicy::None => false,
            ChildPolicy::Any => true,
            ChildPolicy::Only(names) => names.contains(&child.name()),
        }
    }
}

/// A JSON description of every type and component: the single source for the
/// model's manifest, its tool schemas, and the DOM host's registry.
pub fn schema_json() -> serde_json::Value {
    use serde_json::{Map, Value, json};
    let mut components = Map::new();
    for kind in ComponentKind::ALL {
        let fields: Map<String, Value> = kind
            .fields()
            .iter()
            .map(|(n, k)| ((*n).to_owned(), json!(k.name())))
            .collect();
        components.insert(kind.name().to_owned(), Value::Object(fields));
    }
    let mut types = Map::new();
    for ty in TypeId::all() {
        let spec = ty.spec();
        let children = match spec.children {
            ChildPolicy::None => json!("none"),
            ChildPolicy::Any => json!("any"),
            ChildPolicy::Only(names) => json!(names),
        };
        types.insert(
            spec.name.to_owned(),
            json!({
                "components": spec.components.iter().map(|(k, req)| json!({"kind": k.name(), "required": req})).collect::<Vec<_>>(),
                "default_host": spec.default_host.name(),
                "children": children,
            }),
        );
    }
    json!({ "components": components, "types": types })
}

impl fmt::Display for TypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl fmt::Debug for TypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TypeId({})", self.name())
    }
}

impl Serialize for TypeId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for TypeId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        TypeId::lookup(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown node type {s:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_consistent() {
        for ty in TypeId::all() {
            let spec = ty.spec();
            assert_eq!(TypeId::lookup(spec.name), Some(ty));
            if let ChildPolicy::Only(names) = spec.children {
                for n in names {
                    assert!(
                        TypeId::lookup(n).is_some(),
                        "{} admits unknown child {n}",
                        spec.name
                    );
                }
            }
        }
        assert!(
            TypeId::lookup("timeline")
                .unwrap()
                .spec()
                .admits_child(TypeId::lookup("clip").unwrap())
        );
        assert!(
            !TypeId::lookup("rect")
                .unwrap()
                .spec()
                .admits_child(TypeId::lookup("rect").unwrap())
        );
        let schema = schema_json();
        assert!(schema["types"]["rect"]["components"].is_array());
        assert_eq!(schema["components"]["transform2d"]["x"], "number");
    }
}
