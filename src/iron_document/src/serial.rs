//! Canonical JSON.
//!
//! Flat tables keyed by id, sorted keys, one record per line, a schema
//! version at the top. Nesting would encode location, and identity is
//! separate from location (plans/document-spine.md §7). The layout is chosen
//! so that a git diff of a saved file reads as an op log.

use crate::component::{Component, ComponentKind};
use crate::document::{Document, Node, Relation};
use crate::id::{NodeId, RelationId, Version};
use crate::order::OrderKey;
use crate::registry::TypeId;
use serde_json::{Map, Value as J, json};
use std::collections::BTreeMap;
use std::fmt;

pub const SCHEMA: u64 = 1;

#[derive(Debug, Clone, PartialEq)]
pub struct LoadError(pub String);

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot load document: {}", self.0)
    }
}
impl std::error::Error for LoadError {}

fn node_json(n: &Node) -> J {
    let mut m = Map::new();
    m.insert("ty".into(), json!(n.ty.name()));
    m.insert(
        "parent".into(),
        n.parent.map(|p| json!(p.to_string())).unwrap_or(J::Null),
    );
    m.insert("order".into(), json!(n.order.as_str()));
    if let Some(v) = n.tombstoned {
        m.insert("tombstoned".into(), json!(v.0));
    }
    J::Object(m)
}

/// Serialise to the canonical text form.
pub fn save(doc: &Document) -> String {
    let (next_id, next_rel) = doc.raw_next();
    let mut out = String::new();
    out.push_str(&format!(
        "{{\"schema\": {SCHEMA}, \"next_id\": {next_id}, \"next_rel\": {next_rel}, \"version\": {},\n",
        doc.version().0
    ));

    out.push_str(" \"nodes\": {");
    let mut first = true;
    for (id, n) in doc.raw_nodes() {
        out.push_str(if first { "\n" } else { ",\n" });
        first = false;
        out.push_str(&format!("  \"{id}\": {}", n_compact(&node_json(n))));
    }
    out.push_str("\n },\n");

    out.push_str(" \"components\": {");
    let mut first_kind = true;
    for (kind, table) in doc.raw_components() {
        if table.is_empty() {
            continue;
        }
        out.push_str(if first_kind { "\n" } else { ",\n" });
        first_kind = false;
        out.push_str(&format!("  \"{}\": {{", kind.name()));
        let mut first = true;
        for (id, c) in table {
            out.push_str(if first { "\n" } else { ",\n" });
            first = false;
            out.push_str(&format!("   \"{id}\": {}", n_compact(&c.to_json())));
        }
        out.push_str("\n  }");
    }
    out.push_str("\n },\n");

    out.push_str(" \"relations\": [");
    let mut first = true;
    for r in doc.relations() {
        out.push_str(if first { "\n" } else { ",\n" });
        first = false;
        out.push_str(&format!(
            "  {}",
            n_compact(&serde_json::to_value(r).expect("relation serialises"))
        ));
    }
    out.push_str("\n ]}\n");
    out
}

/// serde_json's compact form has sorted keys because the maps are BTreeMaps.
fn n_compact(v: &J) -> String {
    serde_json::to_string(v).expect("json serialises")
}

pub fn load(text: &str) -> Result<Document, LoadError> {
    let root: J = serde_json::from_str(text).map_err(|e| LoadError(e.to_string()))?;
    let obj = root
        .as_object()
        .ok_or_else(|| LoadError("top level must be an object".into()))?;
    let field = |k: &str| {
        obj.get(k)
            .ok_or_else(|| LoadError(format!("missing {k:?}")))
    };

    let schema = field("schema")?
        .as_u64()
        .ok_or_else(|| LoadError("schema must be an integer".into()))?;
    if schema != SCHEMA {
        return Err(LoadError(format!(
            "schema {schema} is not supported (this build reads {SCHEMA})"
        )));
    }
    let next_id = field("next_id")?
        .as_u64()
        .ok_or_else(|| LoadError("next_id must be an integer".into()))?;
    let next_rel = field("next_rel")?
        .as_u64()
        .ok_or_else(|| LoadError("next_rel must be an integer".into()))?;
    let version = Version(
        field("version")?
            .as_u64()
            .ok_or_else(|| LoadError("version must be an integer".into()))?,
    );

    let mut nodes: BTreeMap<NodeId, Node> = BTreeMap::new();
    let raw_nodes = field("nodes")?
        .as_object()
        .ok_or_else(|| LoadError("nodes must be an object".into()))?;
    for (k, v) in raw_nodes {
        let id: NodeId = k
            .parse()
            .map_err(|e: crate::id::IdParseError| LoadError(e.to_string()))?;
        let o = v
            .as_object()
            .ok_or_else(|| LoadError(format!("node {id} must be an object")))?;
        let ty_name = o
            .get("ty")
            .and_then(J::as_str)
            .ok_or_else(|| LoadError(format!("node {id}: missing ty")))?;
        let ty = TypeId::lookup(ty_name)
            .ok_or_else(|| LoadError(format!("node {id}: unknown type {ty_name:?}")))?;
        let parent = match o.get("parent") {
            None | Some(J::Null) => None,
            Some(J::String(s)) => Some(
                s.parse()
                    .map_err(|e: crate::id::IdParseError| LoadError(e.to_string()))?,
            ),
            Some(_) => {
                return Err(LoadError(format!(
                    "node {id}: parent must be an id or null"
                )));
            }
        };
        let order = o
            .get("order")
            .and_then(J::as_str)
            .ok_or_else(|| LoadError(format!("node {id}: missing order")))
            .and_then(|s| OrderKey::parse(s).map_err(|e| LoadError(format!("node {id}: {e}"))))?;
        let tombstoned = match o.get("tombstoned") {
            None => None,
            Some(v) => {
                Some(Version(v.as_u64().ok_or_else(|| {
                    LoadError(format!("node {id}: bad tombstoned"))
                })?))
            }
        };
        if id.raw() >= next_id {
            return Err(LoadError(format!(
                "node {id} is at or past next_id {next_id}"
            )));
        }
        nodes.insert(
            id,
            Node {
                id,
                ty,
                parent,
                order,
                tombstoned,
            },
        );
    }
    for n in nodes.values() {
        if let Some(p) = n.parent {
            let pn = nodes
                .get(&p)
                .ok_or_else(|| LoadError(format!("node {} has unknown parent {p}", n.id)))?;
            if n.is_live() && !pn.is_live() {
                return Err(LoadError(format!(
                    "live node {} under tombstoned parent {p}",
                    n.id
                )));
            }
            if n.is_live() && !pn.ty.spec().admits_child(n.ty) {
                return Err(LoadError(format!(
                    "{p} ({}) does not admit {} ({})",
                    pn.ty, n.id, n.ty
                )));
            }
        }
    }
    // Cycle check over parent pointers.
    for n in nodes.values() {
        let mut cur = n.parent;
        let mut steps = 0usize;
        while let Some(p) = cur {
            if p == n.id {
                return Err(LoadError(format!("cycle through {}", n.id)));
            }
            steps += 1;
            if steps > nodes.len() {
                return Err(LoadError(format!("cycle reachable from {}", n.id)));
            }
            cur = nodes.get(&p).and_then(|x| x.parent);
        }
    }

    let mut components: BTreeMap<ComponentKind, BTreeMap<NodeId, Component>> = BTreeMap::new();
    let raw_comps = field("components")?
        .as_object()
        .ok_or_else(|| LoadError("components must be an object".into()))?;
    for (kname, table) in raw_comps {
        let kind = ComponentKind::from_name(kname)
            .ok_or_else(|| LoadError(format!("unknown component {kname:?}")))?;
        let table = table
            .as_object()
            .ok_or_else(|| LoadError(format!("components.{kname} must be an object")))?;
        for (k, v) in table {
            let id: NodeId = k
                .parse()
                .map_err(|e: crate::id::IdParseError| LoadError(e.to_string()))?;
            let node = nodes
                .get(&id)
                .ok_or_else(|| LoadError(format!("{kname} on unknown node {id}")))?;
            if !node.ty.spec().allows(kind) {
                return Err(LoadError(format!(
                    "{} ({}) does not allow {kname}",
                    id, node.ty
                )));
            }
            let c = Component::from_json(kind, v).map_err(|e| LoadError(format!("{id}: {e}")))?;
            components.entry(kind).or_default().insert(id, c);
        }
    }
    for n in nodes.values().filter(|n| n.is_live()) {
        for req in n.ty.spec().required() {
            if !components.get(&req).is_some_and(|t| t.contains_key(&n.id)) {
                return Err(LoadError(format!(
                    "{} ({}) is missing required {}",
                    n.id,
                    n.ty,
                    req.name()
                )));
            }
        }
    }

    let mut relations: BTreeMap<RelationId, Relation> = BTreeMap::new();
    let raw_rels = field("relations")?
        .as_array()
        .ok_or_else(|| LoadError("relations must be an array".into()))?;
    for v in raw_rels {
        let r: Relation =
            serde_json::from_value(v.clone()).map_err(|e| LoadError(e.to_string()))?;
        if !nodes.contains_key(&r.from) || !nodes.contains_key(&r.to) {
            return Err(LoadError(format!(
                "relation {} references an unknown node",
                r.id
            )));
        }
        if r.id.raw() >= next_rel {
            return Err(LoadError(format!(
                "relation {} is at or past next_rel {next_rel}",
                r.id
            )));
        }
        if relations.insert(r.id, r.clone()).is_some() {
            return Err(LoadError(format!("duplicate relation {}", r.id)));
        }
    }

    let doc = Document::from_parts(nodes, components, relations, next_id, next_rel, version);
    doc.check_invariants().map_err(LoadError)?;
    Ok(doc)
}
