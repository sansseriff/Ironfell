//! Canonical JSON.
//!
//! Flat tables keyed by id, sorted keys, one record per line, a schema
//! version at the top. Nesting would encode location, and identity is
//! separate from location (plans/specs/document-spine.md §7). The layout is chosen
//! so that a git diff of a saved file reads as an op log.

use crate::facet::{Facet, FacetKind};
use crate::document::{Document, Node, Relation};
use crate::id::{NodeId, RelationId, Version};
use crate::order::OrderKey;
use crate::registry::TypeId;
use serde_json::{Map, Value as J, json};
use std::collections::BTreeMap;
use std::fmt;

/// Bumped to 2 with definitions, entities, `params`, quantities and records
/// (roadmap stage B), and to 3 when `depiction` became `template` and
/// `depicts` became `figures`, and to 4 when `circle` (one radius `r`)
/// became `ellipse` (`rx`, `ry`), and to 5 when a path's points moved under
/// `contour` children, so a path can have holes, and to 6 when a
/// definition's parameters became `in` ports and an entity's `params` its
/// `args` (component-model.md §4), and the facet table's key became
/// `facets`, and to 7 when instances became linked by default (an entity's
/// `instance.mode`; component-model.md §5.2), and to 8 when a path's
/// outlines could be packed into `path.d`, a geometry value
/// (packed-paths.md; nothing migrates: a schema 7 path is expanded). Older
/// files still load (`renamed`, `migrate_radius`, `migrate_contours`,
/// `migrate_inputs`, `migrate_detached`).
pub const SCHEMA: u64 = 8;
const OLDEST_READABLE: u64 = 1;

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
    // The session tier (selection and the like) is never saved, nor are
    // instances' expanded nodes: they are derived on load.
    for (id, n) in doc.raw_nodes().iter().filter(|(id, _)| !id.is_session() && !id.is_expanded()) {
        out.push_str(if first { "\n" } else { ",\n" });
        first = false;
        out.push_str(&format!("  \"{id}\": {}", n_compact(&node_json(n))));
    }
    out.push_str("\n },\n");

    out.push_str(" \"facets\": {");
    let mut first_kind = true;
    for (kind, table) in doc.raw_facets() {
        if table.keys().all(|id| id.is_session() || id.is_expanded()) {
            continue;
        }
        out.push_str(if first_kind { "\n" } else { ",\n" });
        first_kind = false;
        out.push_str(&format!("  \"{}\": {{", kind.name()));
        let mut first = true;
        for (id, c) in table.iter().filter(|(id, _)| !id.is_session() && !id.is_expanded()) {
            out.push_str(if first { "\n" } else { ",\n" });
            first = false;
            out.push_str(&format!("   \"{id}\": {}", n_compact(&c.to_json())));
        }
        out.push_str("\n  }");
    }
    out.push_str("\n },\n");

    out.push_str(" \"relations\": [");
    let mut first = true;
    // A relation to an expanded node (a connection to an instance's port)
    // is saved by its id, which the instance re-derives on load.
    for r in doc.relations().filter(|r| !r.id.is_session() && !r.from.is_session() && !r.to.is_session() && !r.from.is_expanded() && !r.id.is_expanded()) {
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

/// A type, facet or relation name as this build spells it, for a file
/// written under `schema`. Schema 3 renamed `depiction` (the definition's
/// template holder and its facet) to `template`, and the `depicts`
/// relation to `figures`.
fn renamed(schema: u64, name: &str) -> &str {
    match (schema < 3, schema < 4, name) {
        (true, _, "depiction") => "template",
        (true, _, "depicts") => "figures",
        (_, true, "circle") => "ellipse",
        _ => name,
    }
}

/// Before schema 4 a circle had one radius, `radius.r`. Its value becomes
/// both of the ellipse's radii, and every binding or clip that read
/// `radius.r` reads `radius.rx`.
fn migrate_radius(schema: u64, kind: FacetKind, v: &J) -> J {
    if schema >= 4 {
        return v.clone();
    }
    fn rewrite(v: &J) -> J {
        match v {
            J::String(s) => J::String(rename_leaf(s)),
            J::Object(o) => J::Object(o.iter().map(|(k, x)| (k.clone(), rewrite(x))).collect()),
            J::Array(a) => J::Array(a.iter().map(rewrite).collect()),
            other => other.clone(),
        }
    }
    let mut out = rewrite(v);
    if kind == FacetKind::Radius
        && let Some(o) = out.as_object_mut()
        && let Some(r) = o.remove("r")
    {
        o.insert("rx".into(), r.clone());
        o.insert("ry".into(), r);
    }
    out
}

/// `radius.r` → `radius.rx` wherever it stands as a whole leaf name.
fn rename_leaf(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("radius.r") {
        let end = i + "radius.r".len();
        let whole = !rest[end..].starts_with(|c: char| c.is_alphanumeric() || c == '_');
        out.push_str(&rest[..end]);
        if whole {
            out.push('x');
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Before schema 5 a path's points were its children and `path.closed`
/// said whether it closed. Each such path gets one contour, a new node
/// holding its points and their `closed`, with the next free id. A
/// binding that read `path.closed` no longer resolves; none is known.
fn migrate_contours(root: &mut J) -> Result<(), LoadError> {
    let bad = |m: &str| LoadError(format!("migrating to contours: {m}"));
    let mut next_id = root.get("next_id").and_then(J::as_u64).ok_or_else(|| bad("missing next_id"))?;
    let nodes = root.get("nodes").and_then(J::as_object).ok_or_else(|| bad("missing nodes"))?;
    let paths: Vec<(String, Option<J>)> = nodes
        .iter()
        .filter(|(_, n)| n.get("ty").and_then(J::as_str) == Some("path"))
        .map(|(k, n)| (k.clone(), n.get("tombstoned").cloned()))
        .collect();
    let mut moves: Vec<(String, String)> = Vec::new();
    let mut created: Vec<(String, J)> = Vec::new();
    let mut closed: Vec<(String, String)> = Vec::new();
    for (path, tombstoned) in paths {
        // Ids are written with or without their `#`; compare them parsed.
        let path_id: Option<NodeId> = path.parse().ok();
        let points: Vec<String> = nodes
            .iter()
            .filter(|(_, n)| n.get("ty").and_then(J::as_str) == Some("point") && n.get("parent").and_then(J::as_str).and_then(|p| p.parse::<NodeId>().ok()) == path_id)
            .map(|(k, _)| k.clone())
            .collect();
        if points.is_empty() {
            continue;
        }
        let contour = NodeId::from_raw(next_id).to_string();
        next_id += 1;
        let mut node = json!({"ty": "contour", "parent": path, "order": OrderKey::first().as_str()});
        if let Some(t) = tombstoned {
            node["tombstoned"] = t;
        }
        created.push((contour.clone(), node));
        closed.push((path.clone(), contour.clone()));
        moves.extend(points.into_iter().map(|p| (p, contour.clone())));
    }
    let nodes = root.get_mut("nodes").and_then(J::as_object_mut).expect("checked above");
    for (point, contour) in moves {
        nodes.get_mut(&point).expect("listed above")["parent"] = J::String(contour);
    }
    for (id, node) in created {
        nodes.insert(id, node);
    }
    let key = if root.get("components").is_some() { "components" } else { "facets" };
    let comps = root.get_mut(key).and_then(J::as_object_mut).ok_or_else(|| bad("missing components"))?;
    let mut contours = Map::new();
    if let Some(J::Object(table)) = comps.get_mut("path") {
        for (path, contour) in &closed {
            let was = table.get_mut(path).and_then(J::as_object_mut).and_then(|o| o.remove("closed")).unwrap_or(J::Bool(false));
            contours.insert(contour.clone(), json!({ "closed": was }));
        }
        for v in table.values_mut() {
            if let Some(o) = v.as_object_mut() {
                o.remove("closed");
            }
        }
    }
    for (_, contour) in &closed {
        contours.entry(contour.clone()).or_insert_with(|| json!({ "closed": false }));
    }
    comps.insert("contour".into(), J::Object(contours));
    root["next_id"] = json!(next_id);
    Ok(())
}

/// Before schema 6 a definition declared `param_spec` children (`kind`,
/// `default`) and an entity held `params`. A `param_spec` becomes an `in`
/// `port_spec` of kind `value` whose `port.signal` is the default, `params`
/// becomes `args`, and every reference follows: `#s.param_spec.default` →
/// `#s.port.signal`, `#e.params.x` → `#e.args.x`. Entities made before then
/// have no `in` port nodes; their arguments still drive their figures. The
/// facet table's key `components` becomes `facets`.
fn migrate_inputs(root: &mut J) -> Result<(), LoadError> {
    let bad = |m: &str| LoadError(format!("migrating to inputs: {m}"));
    let mut facets = match root.as_object_mut().and_then(|o| o.remove("components").or_else(|| o.remove("facets"))) {
        Some(J::Object(m)) => m,
        _ => return Err(bad("missing components")),
    };
    if let Some(J::Object(specs)) = facets.remove("param_spec") {
        let ports = facets.entry("port").or_insert_with(|| J::Object(Map::new()));
        let ports = ports.as_object_mut().ok_or_else(|| bad("port must be an object"))?;
        for (id, spec) in specs {
            let default = spec.get("default").cloned().unwrap_or(json!(0.0));
            ports.insert(id, json!({"kind": "value", "dir": {"enum": "in"}, "signal": default}));
        }
    }
    if let Some(args) = facets.remove("params") {
        facets.insert("args".into(), args);
    }
    let facets = rename_refs(&J::Object(facets));
    root["facets"] = facets;
    if let Some(J::Object(nodes)) = root.get_mut("nodes") {
        for n in nodes.values_mut() {
            if n.get("ty").and_then(J::as_str) == Some("param_spec") {
                n["ty"] = json!("port_spec");
            }
        }
    }
    if let Some(rels) = root.get("relations").cloned() {
        root["relations"] = rename_refs(&rels);
    }
    Ok(())
}

/// Before schema 7 every instance was a copy of its definition: its ports
/// and pictures are authored nodes. It loads `detached`, so nothing is
/// derived over them.
fn migrate_detached(root: &mut J) {
    let key = if root.get("components").is_some() { "components" } else { "facets" };
    if let Some(J::Object(table)) = root.get_mut(key).and_then(|f| f.get_mut("instance")) {
        for v in table.values_mut() {
            if let Some(o) = v.as_object_mut() {
                o.insert("mode".into(), json!({"enum": "detached"}));
            }
        }
    }
}

/// Every string's references to a parameter, rewritten for schema 6.
fn rename_refs(v: &J) -> J {
    match v {
        J::String(s) => J::String(rename_param_refs(s)),
        J::Object(o) => J::Object(o.iter().map(|(k, x)| (k.clone(), rename_refs(x))).collect()),
        J::Array(a) => J::Array(a.iter().map(rename_refs).collect()),
        other => other.clone(),
    }
}

/// `param_spec.default` → `port.signal` and `params.` → `args.`, each where
/// it starts a leaf path (after a `.`, or at the start of a bare path).
fn rename_param_refs(s: &str) -> String {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        let starts = i == 0 || s[..i].ends_with('.') || !s[..i].ends_with(ident);
        if starts && rest.starts_with("param_spec.default") && !rest["param_spec.default".len()..].starts_with(ident) {
            out.push_str("port.signal");
            i += "param_spec.default".len();
        } else if starts && rest.starts_with("params.") && rest["params.".len()..].starts_with(ident) {
            out.push_str("args.");
            i += "params.".len();
        } else {
            let c = rest.chars().next().expect("in bounds");
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

pub fn load(text: &str) -> Result<Document, LoadError> {
    let mut root: J = serde_json::from_str(text).map_err(|e| LoadError(e.to_string()))?;
    if root.get("schema").and_then(J::as_u64).is_some_and(|s| s < 5) {
        migrate_contours(&mut root)?;
    }
    if root.get("schema").and_then(J::as_u64).is_some_and(|s| s < 6) {
        migrate_inputs(&mut root)?;
    }
    if root.get("schema").and_then(J::as_u64).is_some_and(|s| s < 7) {
        migrate_detached(&mut root);
    }
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
    if !(OLDEST_READABLE..=SCHEMA).contains(&schema) {
        return Err(LoadError(format!(
            "schema {schema} is not supported (this build reads {OLDEST_READABLE} to {SCHEMA})"
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
        let ty = TypeId::lookup(renamed(schema, ty_name))
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

    let mut facets: BTreeMap<FacetKind, BTreeMap<NodeId, Facet>> = BTreeMap::new();
    let raw_comps = field("facets")?
        .as_object()
        .ok_or_else(|| LoadError("facets must be an object".into()))?;
    for (kname, table) in raw_comps {
        let kind = FacetKind::from_name(renamed(schema, kname))
            .ok_or_else(|| LoadError(format!("unknown component {kname:?}")))?;
        let table = table
            .as_object()
            .ok_or_else(|| LoadError(format!("facets.{kname} must be an object")))?;
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
            let c = Facet::from_json(kind, &migrate_radius(schema, kind, v)).map_err(|e| LoadError(format!("{id}: {e}")))?;
            facets.entry(kind).or_default().insert(id, c);
        }
    }
    for n in nodes.values().filter(|n| n.is_live()) {
        for req in n.ty.spec().required() {
            if !facets.get(&req).is_some_and(|t| t.contains_key(&n.id)) {
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
        let mut v = v.clone();
        if let Some(J::String(kind)) = v.get_mut("rel") {
            *kind = renamed(schema, kind).to_owned();
        }
        let r: Relation =
            serde_json::from_value(v).map_err(|e| LoadError(e.to_string()))?;
        // An expanded endpoint exists once its instance is expanded.
        let known = |id: NodeId| nodes.contains_key(&id) || id.expanded_parts().is_some_and(|(owner, _)| nodes.contains_key(&owner));
        if !known(r.from) || !known(r.to) {
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

    let doc = Document::from_parts(nodes, facets, relations, next_id, next_rel, version);
    doc.check_invariants().map_err(LoadError)?;
    Ok(doc)
}
