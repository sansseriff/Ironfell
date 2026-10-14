//! Views: typed, elided reads of the document for a model or a host
//! (plans/specs/document-spine.md §8; doc 04 §6).
//!
//! Three relations (ECM §11): the raw tree; the model (entities, their
//! definitions, parameters and figures); and one composition's layout, with
//! each figure's entity as an `of` attribute. Output is XML-ish because
//! nesting is the data. Elision is always marked: a reader must be able to
//! tell "empty" from "not shown".

use crate::component::{Component, ComponentKind};
use crate::document::Document;
use crate::id::NodeId;
use serde::{Deserialize, Serialize};
use std::fmt::Write;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fidelity {
    /// Types, ids, names and counts. No leaves.
    Skeleton,
    /// Leaves, but large child lists are elided to a count and two samples.
    Summary,
    /// Everything to the depth limit.
    Full,
}

/// Which structure a view walks. JSON: `"tree"`, `"model"`, or
/// `{"composition": "#4"}`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewRelation {
    /// Every node under its parent, as stored.
    #[default]
    Tree,
    /// Entities: what things are, and where each is pictured.
    Model,
    /// One composition's layout tree; a figure names its entity with `of`.
    Composition(NodeId),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewQuery {
    #[serde(default)]
    pub relation: ViewRelation,
    /// `None` renders every root.
    #[serde(default)]
    pub scope: Option<NodeId>,
    pub fidelity: Fidelity,
    /// Levels below the scope to descend. `None` is unlimited.
    #[serde(default)]
    pub depth: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ViewError(pub String);

impl std::fmt::Display for ViewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ViewError {}

/// Child lists longer than this are elided at summary fidelity.
const SUMMARY_MAX_CHILDREN: usize = 8;
const SUMMARY_SAMPLES: usize = 2;

/// Which cluster maintains each solver variable, for `.by` markers.
pub type By = std::collections::BTreeMap<crate::expr::Leaf, NodeId>;

pub fn render_tree(
    doc: &Document,
    reactive: &crate::graph::Reactive,
    by: &By,
    q: &ViewQuery,
) -> Result<String, ViewError> {
    let mut out = String::new();
    match q.relation {
        ViewRelation::Tree => {}
        ViewRelation::Model => return Ok(render_model(doc, reactive, by, q)),
        ViewRelation::Composition(c) => {
            if crate::composition::composition_of(doc, c) != Some(c) {
                return Err(ViewError(format!("{c} is not a live composition")));
            }
            let q = ViewQuery { scope: Some(q.scope.unwrap_or(c)), ..q.clone() };
            if crate::composition::composition_of(doc, q.scope.expect("set")) != Some(c) {
                return Err(ViewError(format!("{} is not in composition {c}", q.scope.expect("set"))));
            }
            writeln!(out, "<composition-view of=\"{c}\" version=\"{}\">", doc.version().0).expect("string write");
            node(doc, reactive, by, &q, q.scope.expect("set"), 0, &mut out);
            out.push_str("</composition-view>\n");
            return Ok(out);
        }
    }
    writeln!(out, "<document version=\"{}\">", doc.version().0).expect("string write");
    let roots: Vec<NodeId> = match q.scope {
        Some(id) => {
            if !doc.is_live(id) {
                return Err(ViewError(format!("{id} is not a live node")));
            }
            vec![id]
        }
        // The session tier is not part of the document a model reads.
        None => doc.roots().iter().copied().filter(|r| !r.is_session()).collect(),
    };
    for id in roots {
        node(doc, reactive, by, q, id, 0, &mut out);
    }
    out.push_str("</document>\n");
    Ok(out)
}

fn node(
    doc: &Document,
    reactive: &crate::graph::Reactive,
    by: &By,
    q: &ViewQuery,
    id: NodeId,
    level: u32,
    out: &mut String,
) {
    let n = doc.live(id).expect("live");
    let indent = "  ".repeat(level as usize + 1);
    let mut attrs = leaf_attrs(doc, reactive, by, q.fidelity, id);
    // In a composition view a figure names what it pictures, in place of
    // the relation element (ECM §11).
    let as_of = matches!(q.relation, ViewRelation::Composition(_));
    if as_of && let Some(entity) = crate::composition::figured_by(doc, id) {
        attrs.push(("of".into(), entity.to_string()));
    }

    let kids = doc.children(id);
    let rels: Vec<_> = doc.relations_from(id).filter(|r| !(as_of && r.rel == crate::document::RelKind::Figures)).collect();
    // The reverse direction: what drives this node (doc 05 §5.3). Bindings
    // already show as `<attr>.bind`; relations arrive from elsewhere.
    let drivers: Vec<_> = doc.relations_to(id).filter(|r| !r.from.is_session()).collect();
    let at_limit = q.depth.is_some_and(|d| level >= d);
    let shown: &[NodeId] = if at_limit {
        &[]
    } else if q.fidelity == Fidelity::Summary && kids.len() > SUMMARY_MAX_CHILDREN {
        &kids[..SUMMARY_SAMPLES]
    } else {
        kids
    };
    let elided = shown.len() < kids.len();
    if !kids.is_empty() || matches!(n.ty.spec().children, crate::registry::ChildPolicy::Any) {
        attrs.push(("count".into(), kids.len().to_string()));
    }
    if elided {
        attrs.push(("elided".into(), String::new()));
    }
    write_open(out, &indent, n.ty.name(), &attrs);
    if shown.is_empty() && rels.is_empty() && drivers.is_empty() {
        out.push_str("/>\n");
        return;
    }
    out.push_str(">\n");
    for r in rels {
        write!(out, "{indent}  <{} to=\"{}\"", r.rel.name(), r.to).expect("string write");
        for (k, v) in &r.data {
            write!(out, " {k}=\"{}\"", escape(&v.to_string())).expect("string write");
        }
        out.push_str("/>\n");
    }
    for r in drivers {
        write!(out, "{indent}  <driven-by ref=\"{}\" rel=\"{}\"", r.from, r.rel.name()).expect("string write");
        if let Some(prop) = doc.get::<crate::component::Clip>(r.from).and_then(|c| c.prop.constant()).and_then(|v| v.as_str()) {
            write!(out, " prop=\"{}\"", escape(prop)).expect("string write");
        }
        for (k, v) in &r.data {
            write!(out, " {k}=\"{}\"", escape(&v.to_string())).expect("string write");
        }
        out.push_str("/>\n");
    }
    for &k in shown {
        node(doc, reactive, by, q, k, level + 1, out);
    }
    writeln!(out, "{indent}</{}>", n.ty.name()).expect("string write");
}

fn write_open(out: &mut String, indent: &str, tag: &str, attrs: &[(String, String)]) {
    write!(out, "{indent}<{tag}").expect("string write");
    for (k, v) in attrs {
        if k == "elided" {
            out.push_str(" elided");
        } else {
            write!(out, " {k}=\"{}\"", escape(v)).expect("string write");
        }
    }
}

/// The model view (ECM §11): every live entity with its definition, its
/// parameters (value, binding, solver marker as in the tree view) and each
/// figure with the composition it is drawn in. Signals arrive with ports.
fn render_model(doc: &Document, reactive: &crate::graph::Reactive, by: &By, q: &ViewQuery) -> String {
    use crate::composition;
    let mut out = String::new();
    writeln!(out, "<model-view version=\"{}\">", doc.version().0).expect("string write");
    let entity = crate::registry::TypeId::lookup("entity").expect("registered");
    let name_of = |n: NodeId| {
        doc.get::<crate::component::Name>(n).and_then(|c| c.text.constant()).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(str::to_owned)
    };
    for id in doc.nodes_of_type(entity).filter(|&e| doc.is_live(e)) {
        if q.scope.is_some_and(|s| s != id) {
            continue;
        }
        let mut attrs = leaf_attrs(doc, reactive, by, q.fidelity, id);
        let def = doc.get::<crate::component::Instance>(id).and_then(|i| i.def.constant()).and_then(|v| match v {
            crate::value::Value::Ref(d) => Some(*d),
            _ => None,
        });
        if let Some(d) = def.and_then(name_of) {
            attrs.push(("definition".into(), d));
        }
        let figures = composition::figures_of(doc, id);
        let ports: Vec<NodeId> = doc.children(id).iter().copied().filter(|&p| doc.live(p).is_some_and(|n| n.ty.name() == "port")).collect();
        if figures.is_empty() && ports.is_empty() {
            write_open(&mut out, "  ", "entity", &attrs);
            out.push_str("/>\n");
            continue;
        }
        write_open(&mut out, "  ", "entity", &attrs);
        out.push_str(">\n");
        // Ports with their live values (ECM §6.1): a one-way kind shows its
        // signal, a pair its across and through.
        for p in ports {
            let port = doc.get::<crate::component::Port>(p);
            let text = |slot: Option<&crate::value::Slot>| slot.and_then(|s| s.constant()).and_then(|v| v.as_str()).unwrap_or("").to_owned();
            let kind = text(port.map(|c| &c.kind));
            let mut pa: Vec<(String, String)> = vec![("id".into(), p.to_string())];
            if let Some(n) = name_of(p) {
                pa.push(("name".into(), n));
            }
            pa.push(("kind".into(), kind.clone()));
            pa.push(("dir".into(), text(port.map(|c| &c.dir))));
            let read = |f: &str| f.parse::<crate::component::LeafPath>().ok().and_then(|path| reactive.read(doc, crate::expr::Leaf { node: p, path })).map(|v| v.to_string());
            let pair = crate::port::kind(&kind).is_some_and(|k| k.flow == crate::port::Flow::Pair);
            for f in if pair { &["across", "through"][..] } else { &["signal"][..] } {
                if let Some(v) = read(&format!("port.{f}")) {
                    pa.push(((*f).into(), v));
                }
            }
            write_open(&mut out, "    ", "port", &pa);
            out.push_str("/>\n");
        }
        for f in figures {
            let ty = doc.live(f).map(|n| n.ty.name()).unwrap_or("?");
            let mut fa: Vec<(String, String)> = vec![("id".into(), f.to_string()), ("type".into(), ty.to_owned())];
            if let Some(n) = name_of(f) {
                fa.push(("name".into(), n));
            }
            if let Some(c) = composition::composition_of(doc, f) {
                fa.push(("in".into(), c.to_string()));
                if let Some(n) = name_of(c) {
                    fa.push(("composition".into(), n));
                }
            }
            write_open(&mut out, "    ", "figure", &fa);
            out.push_str("/>\n");
        }
        out.push_str("  </entity>\n");
    }
    // Connections, as `entity.port` names for both ends.
    let connection = crate::registry::TypeId::lookup("connection").expect("registered");
    let end_name = |p: NodeId| {
        let owner = doc.live(p).and_then(|n| n.parent);
        format!("{}.{}", owner.and_then(name_of).unwrap_or_else(|| owner.map(|o| o.to_string()).unwrap_or_default()), name_of(p).unwrap_or_else(|| p.to_string()))
    };
    for c in doc.nodes_of_type(connection).filter(|&c| doc.is_live(c)) {
        let Some((a, b)) = crate::port::ends(doc, c) else { continue };
        let mut ca: Vec<(String, String)> = vec![("id".into(), c.to_string()), ("a".into(), end_name(a)), ("b".into(), end_name(b))];
        if let Some(k) = doc.get::<crate::component::Port>(a).and_then(|p| p.kind.constant()).and_then(|v| v.as_str()) {
            ca.push(("kind".into(), k.to_owned()));
        }
        write_open(&mut out, "  ", "connection", &ca);
        out.push_str("/>\n");
    }
    out.push_str("</model-view>\n");
    out
}

/// A node's id, name and (unless skeleton) leaves: each as its current
/// value, plus `<attr>.bind` for a binding and `<attr>.by` for a solver
/// variable.
fn leaf_attrs(doc: &Document, reactive: &crate::graph::Reactive, by: &By, fidelity: Fidelity, id: NodeId) -> Vec<(String, String)> {
    let mut attrs: Vec<(String, String)> = vec![("id".into(), id.to_string())];

    let comps: Vec<&Component> = doc.components_of(id).collect();
    if let Some(name) = comps.iter().find(|c| c.kind() == ComponentKind::Name) {
        if let Some(text) = name
            .get("text")
            .and_then(|s| s.constant())
            .and_then(|v| v.as_str())
            && !text.is_empty()
        {
            attrs.push(("name".into(), text.to_owned()));
        }
    }
    if fidelity != Fidelity::Skeleton {
        // Attribute names are bare field names unless two components on this
        // node share one, in which case both get the full `kind.field` path.
        let mut seen: Vec<&str> = Vec::new();
        let mut dup: Vec<&str> = Vec::new();
        for c in &comps {
            for f in c.field_names() {
                if seen.contains(&f) {
                    dup.push(f);
                } else {
                    seen.push(f);
                }
            }
        }
        for c in &comps {
            if c.kind() == ComponentKind::Name {
                continue;
            }
            for f in c.field_names() {
                let slot = c.get(f).expect("declared");
                // A bound slot shows its current value like any other, and
                // its binding as `<attr>.bind`, so a reader sees both what a
                // value is and why.
                let leaf = crate::expr::Leaf {
                    node: id,
                    path: crate::component::LeafPath::new(c.kind(), f).expect("declared"),
                };
                let value = reactive
                    .read(doc, leaf)
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "?".to_owned());
                let key = if dup.contains(&f) {
                    format!("{}.{f}", c.kind().name())
                } else {
                    f.to_owned()
                };
                attrs.push((key.clone(), value));
                if let Some(e) = slot.expr() {
                    attrs.push((format!("{key}.bind"), e.to_string()));
                }
                // A solver variable: writing it directly makes it a driver
                // whose effect spreads through the cluster (CS §9.3).
                if let Some(cluster) = by.get(&leaf) {
                    attrs.push((format!("{key}.by"), cluster.to_string()));
                }
            }
        }
    }

    attrs
}

fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            _ => o.push(c),
        }
    }
    o
}
