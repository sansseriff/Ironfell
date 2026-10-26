//! The node-type registry: one table read by the validator, the reconciler,
//! the model-facing manifest and the DOM host (plans/specs/document-spine.md §5.7).
//!
//! Adding a node type is a Rust change. That is the correct place for it: the
//! vocabulary is code, the bindings over it are data (doc 02 §9.6). The
//! registry holds *primitives only* (entity-composition-model.md §4.2):
//! domain things such as a bar chart, a laser or a fiber are definitions in
//! the document's library, built from these primitives.

use crate::facet::FacetKind;
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
    /// Allowed facets, with whether each is required on create.
    pub facets: &'static [(FacetKind, bool)],
    pub default_host: Host,
    pub children: ChildPolicy,
}

use FacetKind as K;

static TYPES: &[TypeSpec] = &[
    TypeSpec {
        name: "group",
        facets: &[(K::Name, false), (K::Transform2d, false), (K::Appearance, false), (K::GroupClip, false), (K::Host, false), (K::Layout, false), (K::Size, false), (K::LayoutItem, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::Any,
    },
    TypeSpec {
        name: "rect",
        facets: &[
            (K::Transform2d, true),
            (K::Size, true),
            (K::Corners, false),
            (K::Fill, false),
            (K::Gradient, false),
            (K::Appearance, false),
            (K::Stroke, false),
            (K::Shadow, false),
            (K::Name, false),
            (K::Host, false),
            (K::LayoutItem, false),
        ],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
    // Centred on its origin; a circle is an ellipse with equal radii, as in
    // Figma (the type was `circle`, with one radius, before schema 4).
    TypeSpec {
        name: "ellipse",
        facets: &[
            (K::Transform2d, true),
            (K::Radius, true),
            (K::Arc, false),
            (K::Fill, false),
            (K::Gradient, false),
            (K::Appearance, false),
            (K::Stroke, false),
            (K::Shadow, false),
            (K::Name, false),
            (K::Host, false),
            (K::LayoutItem, false),
        ],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
    // Text (plans/specs/text.md): origin at the box's top-left, like a
    // rect. `size` matters only when `text.sizing` is not `auto_width`.
    TypeSpec {
        name: "text",
        facets: &[
            (K::Transform2d, true),
            (K::Text, true),
            (K::Font, true),
            (K::Size, false),
            (K::Fill, false),
            (K::Gradient, false),
            (K::Appearance, false),
            (K::Stroke, false),
            (K::Shadow, false),
            (K::Name, false),
            (K::Host, false),
            (K::LayoutItem, false),
        ],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
    // A table (plans/specs/layout-and-tables.md §3): its rows, each holding
    // its cells (text); it draws its bands and lines under them.
    TypeSpec {
        name: "table",
        facets: &[
            (K::Transform2d, true),
            (K::Table, true),
            (K::Rules, false),
            (K::Fill, false),
            (K::Size, false),
            (K::Appearance, false),
            (K::Name, false),
            (K::Host, false),
            (K::LayoutItem, false),
        ],
        default_host: Host::Canvas,
        children: ChildPolicy::Only(&["row"]),
    },
    // A table's row: its cells, left to right. It has no placement of its
    // own (layout places the cells), so it is never drawn or selected.
    TypeSpec {
        name: "row",
        facets: &[(K::Name, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::Only(&["text"]),
    },
    // A path (roadmap stage E): its outlines are `contour` children, so it
    // can have holes (schema 5; before, its points were its children).
    TypeSpec {
        name: "path",
        facets: &[
            (K::Transform2d, true),
            (K::PathShape, true),
            (K::Fill, false),
            (K::Gradient, false),
            (K::Appearance, false),
            (K::Stroke, false),
            (K::Shadow, false),
            (K::Name, false),
            (K::Host, false),
            (K::LayoutItem, false),
        ],
        default_host: Host::Canvas,
        children: ChildPolicy::Only(&["contour"]),
    },
    // One outline of a path: its points, in order. No transform: its points
    // are in the path's space.
    TypeSpec {
        name: "contour",
        facets: &[(K::Contour, true), (K::Name, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::Only(&["point"]),
    },
    // One anchor of a contour, in the path's space, with its handles.
    TypeSpec {
        name: "point",
        facets: &[(K::Transform2d, true), (K::Handles, false), (K::Name, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
    TypeSpec {
        name: "mesh",
        facets: &[(K::Transform3d, true), (K::Mesh, true), (K::Fill, false), (K::Name, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
    TypeSpec {
        name: "slider",
        facets: &[
            (K::Transform2d, true),
            (K::Slider, true),
            (K::Size, false),
            (K::Name, false),
            (K::Host, false),
            (K::LayoutItem, false),
        ],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
    // Definitions as data (entity-composition-model.md §4): the structural
    // types a library is made of. None of them is drawn.
    TypeSpec {
        name: "library",
        facets: &[(K::Name, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::Only(&["definition"]),
    },
    TypeSpec {
        name: "definition",
        facets: &[(K::Name, true), (K::Import, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::Only(&["port_spec", "equation", "template", "entity", "connection"]),
    },
    // A template: its subtree is what an instance copies.
    TypeSpec {
        name: "template",
        facets: &[(K::Template, true), (K::Name, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::Any,
    },
    // The model tree (ECM §5): what things are, never drawn. A root.
    TypeSpec {
        name: "model",
        facets: &[(K::Name, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::Only(&["entity", "connection"]),
    },
    // A definition's port (ECM §6.1): instantiation copies it onto the
    // entity as a `port`, its behaviour binding rewritten.
    TypeSpec {
        name: "port_spec",
        facets: &[(K::Name, true), (K::Port, true)],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
    TypeSpec {
        name: "port",
        facets: &[(K::Name, true), (K::Port, true)],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
    // A behaviour equation over ports and parameters (ECM §6.1).
    TypeSpec {
        name: "equation",
        facets: &[(K::Equation, true), (K::Name, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
    // Two ports joined: two `connects` relations, ends `a` and `b`.
    TypeSpec {
        name: "connection",
        facets: &[(K::Connection, true), (K::Name, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
    // The session tier (plans/specs/panels-and-session.md): state that is not
    // content. Its ids come from their own range; it is never saved and
    // never in undo, and views of the document leave it out.
    TypeSpec {
        name: "session",
        facets: &[(K::Name, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::Only(&["selection"]),
    },
    // One viewer's selection: `selects` relations to the selected nodes,
    // in pick order.
    TypeSpec {
        name: "selection",
        facets: &[(K::Name, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
    // One diagram (ECM §5, §7): a root whose subtree one panel shows.
    TypeSpec {
        name: "composition",
        facets: &[(K::Name, true), (K::Composition, true)],
        default_host: Host::Canvas,
        children: ChildPolicy::Any,
    },
    // One composition shown inside another (ECM §7.4). Placed by whichever
    // transform matches the parent composition's dimensions.
    TypeSpec {
        name: "embed",
        facets: &[
            // Placement first: a bare x, y, w or h on an embed means where
            // it sits, not its frame (`frame.x`), which is the less common.
            (K::Embed, true),
            (K::Transform2d, false),
            (K::Transform3d, false),
            (K::Size, false),
            (K::EmbedCamera, false),
            (K::Frame, false),
            (K::Name, false),
        ],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
    // A view into a 3D composition, for embeds that name it.
    TypeSpec {
        name: "camera",
        facets: &[(K::Transform3d, true), (K::Camera, true), (K::Name, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
    // A transform with nothing to draw: a pivot children hang off, and,
    // with an `anchor`, where a port sits on a picture.
    TypeSpec {
        name: "empty",
        facets: &[(K::Transform2d, true), (K::Anchor, false), (K::Name, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::Any,
    },
    // A placed picture of a component (component-model.md §5.1): a group
    // whose children are its definition's template, expanded, never stored.
    TypeSpec {
        name: "use",
        facets: &[(K::Use, true), (K::Transform2d, true), (K::Name, false), (K::Appearance, false), (K::Instance, false), (K::LayoutItem, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::Any,
    },
    // A per-instance change to what an instance derives
    // (component-model.md §5.3), a child of its entity or `use`.
    TypeSpec {
        name: "override",
        facets: &[(K::Override, true)],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
    // An instance of a definition; its figures point at it with
    // `figures` relations. Its ports are children; `args` holds a value
    // per `in` port (component-model.md §4).
    TypeSpec {
        name: "entity",
        facets: &[(K::Instance, true), (K::Args, true), (K::Name, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::Only(&["port", "equation", "override", "entity", "connection"]),
    },
    // A relationship between operand nodes, solved in whatever direction a
    // drag chooses (constraint-system.md §4). Never materialised as content;
    // by convention a child of the group that owns the mechanism.
    TypeSpec {
        name: "constraint",
        facets: &[(K::Constraint, true), (K::Name, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::None,
    },
    TypeSpec {
        name: "timeline",
        facets: &[(K::Name, false)],
        default_host: Host::Canvas,
        children: ChildPolicy::Only(&["clip"]),
    },
    TypeSpec {
        name: "clip",
        facets: &[(K::Timing, true), (K::Clip, true), (K::Name, false)],
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
    pub fn allows(&self, kind: FacetKind) -> bool {
        self.facets.iter().any(|(k, _)| *k == kind)
    }

    pub fn requires(&self, kind: FacetKind) -> bool {
        self.facets.iter().any(|(k, req)| *k == kind && *req)
    }

    pub fn required(&self) -> impl Iterator<Item = FacetKind> + '_ {
        self.facets.iter().filter(|(_, r)| *r).map(|(k, _)| *k)
    }

    /// Types that only exist as roots: the model tree and compositions.
    pub fn root_only(&self) -> bool {
        matches!(self.name, "model" | "composition" | "session")
    }

    pub fn admits_child(&self, child: TypeId) -> bool {
        if child.spec().root_only() {
            return false;
        }
        match self.children {
            ChildPolicy::None => false,
            ChildPolicy::Any => true,
            ChildPolicy::Only(names) => names.contains(&child.name()),
        }
    }
}

/// A JSON description of every type and facet: the single source for the
/// model's manifest, its tool schemas, and the DOM host's registry.
pub fn schema_json() -> serde_json::Value {
    use serde_json::{Map, Value, json};
    let mut facets = Map::new();
    for kind in FacetKind::ALL {
        let mut fields: Map<String, Value> = kind
            .fields()
            .iter()
            .map(|(n, k)| ((*n).to_owned(), json!(k.name())))
            .collect();
        if kind.is_dynamic() {
            // Fields come from the definition; `*` tells the model surface
            // to accept any name at kind `any`.
            fields.insert("*".to_owned(), json!("any"));
        }
        facets.insert(kind.name().to_owned(), Value::Object(fields));
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
                "facets": spec.facets.iter().map(|(k, req)| json!({"kind": k.name(), "required": req})).collect::<Vec<_>>(),
                "default_host": spec.default_host.name(),
                "root_only": spec.root_only(),
                "children": children,
            }),
        );
    }
    json!({ "facets": facets, "types": types })
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
        assert!(schema["types"]["rect"]["facets"].is_array());
        assert_eq!(schema["facets"]["transform2d"]["x"], "number");
    }
}
