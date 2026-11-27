//! "Ask the node" (`EN-14`, patches.md §5): one read-only answer to why a
//! node looks and moves as it does, for the inspector and the agent's
//! `<look connections="#…"/>`.
//!
//! - **Where it is and what it derives from:** its parents up to its page,
//!   and for a part of an instance, the instance, its definition and the
//!   template node it comes from, out to the authored instance.
//! - **Why each leaf has its value:** the layers it is read through, weakest
//!   first, in the store's fixed order (`graph::read_with`): the facet's
//!   default, the template's value, each override reaching the instance,
//!   the authored value, a persisted solve, the producer that drives it,
//!   its binding, a live solve and a gesture's preview. The last one wins.
//! - **What drives it:** what its bindings read, the producers driving its
//!   leaves (and those waiting), its constraint clusters, the layout of its
//!   parent, and the relations pointing at it.
//! - **What it drives:** every binding and engine computation reading one
//!   of its leaves, and the relations it holds.
//!
//! Everything comes from what the store already records (the graph's reads,
//! the producers' claims, the overrides, the relations), so nothing that
//! affects a node can be missing from its answer. It reads the store as it
//! is: a caller asking about a resting page pulls it first
//! (`Store::pull_nodes`).

use crate::facet::{FacetKind, LeafPath};
use crate::graph::{CellId, NativeKey, ProducerId};
use crate::{Leaf, NodeId, StoreCore, Tree, Value};
use serde::Serialize;
use std::collections::BTreeSet;

/// A node, for a reader: its id, type and name.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NodeRef {
    pub id: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub name: Option<String>,
}

/// One layer a leaf is read through.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Layer {
    /// `default`, `template`, `override`, `authored`, `solved`, `producer`,
    /// `binding`, `live solve` or `preview`.
    pub kind: &'static str,
    /// Who: the override node, the producer, the template node.
    pub from: Option<String>,
    /// Its value, or a binding's formula.
    pub value: String,
}

/// Why one leaf has its value.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Why {
    pub leaf: String,
    pub value: Option<String>,
    /// Weakest first; the last is what is read.
    pub layers: Vec<Layer>,
}

/// Something that drives the node, or that it drives.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Link {
    /// `binding`, `producer`, `waiting`, `cluster`, `layout`, a relation's
    /// kind (`animates`, `constrains`, …), or an engine computation
    /// (`network`, `repeat`, `constraints`, `pins`).
    pub kind: String,
    /// The other node.
    pub node: String,
    /// The leaf of this node it concerns.
    pub leaf: Option<String>,
    /// More: the other end's leaf, a producer's name.
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Inspection {
    pub node: NodeRef,
    /// Its parents, outermost first, ending at its page or root.
    pub path: Vec<NodeRef>,
    /// What it derives from, nearest first: one line each.
    pub derives: Vec<String>,
    /// The leaves whose value comes from more than their facet's default.
    pub why: Vec<Why>,
    pub driven_by: Vec<Link>,
    pub drives: Vec<Link>,
}

fn name(doc: Tree<'_>, id: NodeId) -> Option<String> {
    doc.get::<crate::facet::Name>(id).and_then(|n| n.text.constant()).and_then(|v| v.as_str()).map(str::to_owned)
}

fn node_ref(doc: Tree<'_>, id: NodeId) -> NodeRef {
    NodeRef { id: id.to_string(), ty: doc.ty_of(id).map_or("?", |t| t.name()).to_owned(), name: name(doc, id) }
}

/// The definition `id` is an instance of.
fn def_of(doc: Tree<'_>, id: NodeId) -> Option<NodeId> {
    doc.get::<crate::facet::Instance>(id).and_then(|i| i.def.constant()).and_then(|v| v.as_ref())
}

/// `#5 "meter"`, or `#5`.
fn named(doc: Tree<'_>, id: NodeId) -> String {
    match name(doc, id) {
        Some(n) => format!("{id} \"{n}\""),
        None => id.to_string(),
    }
}

pub fn producer_name(p: ProducerId) -> String {
    match p {
        ProducerId::CLIPS => "clips".into(),
        ProducerId::NETWORK => "network".into(),
        ProducerId::LAYOUT => "layout".into(),
        ProducerId::MACHINES => "state machine".into(),
        ProducerId::THEME => "theme".into(),
        ProducerId::ITEMS => "repeat items".into(),
        ProducerId::STATE => "session state".into(),
        ProducerId::SELECTION => "selection".into(),
        ProducerId::STEPS => "steps".into(),
        ProducerId::QUERIES => "queries".into(),
        ProducerId::PRESENCE => "presence".into(),
        ProducerId::EVENTS => "events".into(),
        ProducerId(n) => format!("host {n}"),
    }
}

fn slot_text(slot: &crate::Slot) -> String {
    match slot.expr() {
        Some(e) => format!("{{= {e}}}"),
        None => slot.constant().map_or_else(|| "?".into(), Value::to_string),
    }
}

/// An engine computation, as a link: its kind and node.
fn native(k: NativeKey) -> (&'static str, NodeId) {
    match k {
        NativeKey::Island(l) => ("network", l.node),
        NativeKey::Layout(n) => ("layout", n),
        NativeKey::Pins(n) => ("pins", n),
        NativeKey::Repeat(n) => ("repeat", n),
        NativeKey::Component(n) | NativeKey::Cluster(n) => ("constraints", n),
        #[cfg(test)]
        NativeKey::Test(n, _) => ("test", n),
    }
}

impl StoreCore {
    /// Ask the node (module docs). `None` for a node that is not live.
    pub fn inspect(&self, id: NodeId) -> Option<Inspection> {
        let doc = self.tree();
        let node = doc.live(id)?;
        let graph = self.reactive();

        let mut path = Vec::new();
        let mut at = node.parent;
        while let Some(p) = at {
            path.push(node_ref(doc, p));
            at = doc.live(p).and_then(|n| n.parent);
        }
        path.reverse();

        // What it derives from: out through each instance it is a part of.
        let mut derives = Vec::new();
        let mut cur = id;
        while let Some((owner, local)) = cur.expanded_parts() {
            if crate::expand::is_item_owner(doc, owner) {
                derives.push(format!("an item of {}, from {}", named(doc, owner), named(doc, local)));
            } else {
                // A `use` shows its entity's definition.
                let entity = crate::expand::use_entity(doc, owner).unwrap_or(owner);
                match def_of(doc, entity) {
                    Some(def) => derives.push(format!("part {} of {}, an instance of {}", named(doc, local), named(doc, owner), named(doc, def))),
                    None => derives.push(format!("part {} of {}", named(doc, local), named(doc, owner))),
                }
            }
            cur = owner;
        }
        if let Some(def) = def_of(doc, id) {
            derives.push(format!("an instance of {}", named(doc, def)));
        }
        if !id.is_expanded()
            && let Some(t) = path.iter().rev().find(|p| p.ty == "template")
        {
            let def = t.id.parse::<NodeId>().ok().and_then(|t| doc.live(t)?.parent);
            if let Some(def) = def {
                derives.push(format!("in the template of {}", named(doc, def)));
            }
        }

        // The overrides reaching this part, weakest first.
        let overrides = id.expanded_parts().map(|(owner, local)| {
            crate::expand::overrides(doc, owner).into_iter().filter(move |(_, target, ..)| *target == local).collect::<Vec<_>>()
        });

        let mut why = Vec::new();
        let mut driven_by: BTreeSet<Link> = BTreeSet::new();
        let mut drives: BTreeSet<Link> = BTreeSet::new();
        for facet in doc.facets_of(id) {
            let kind = facet.kind();
            // Its name is in `node`.
            if kind == FacetKind::Name {
                continue;
            }
            for field in facet.field_names() {
                let Ok(lp) = LeafPath::new(kind, field) else { continue };
                let leaf = Leaf { node: id, path: lp };
                let leaf_text = lp.to_string();
                let mut layers = Vec::new();
                let default = kind.default_facet().get(field).and_then(|s| s.constant().cloned());
                if let Some(d) = &default {
                    layers.push(Layer { kind: "default", from: None, value: d.to_string() });
                }
                match id.expanded_parts() {
                    Some((_, local)) => {
                        if let Some(s) = doc.leaf(local, lp) {
                            layers.push(Layer { kind: "template", from: Some(local.to_string()), value: slot_text(s) });
                        }
                        for (o, _, l, v) in overrides.iter().flatten() {
                            if *l == lp {
                                layers.push(Layer { kind: "override", from: Some(o.to_string()), value: v.to_string() });
                            }
                        }
                    }
                    // A bound leaf's formula is its binding layer, below.
                    None => {
                        if let Some(s) = doc.leaf(id, lp).filter(|s| s.expr().is_none()) {
                            layers.push(Layer { kind: "authored", from: None, value: slot_text(s) });
                        }
                    }
                }
                if let Some(s) = doc.solve_leaf(leaf) {
                    layers.push(Layer { kind: "solved", from: None, value: slot_text(&s) });
                }
                if let Some(sig) = graph.signal(leaf) {
                    layers.push(Layer { kind: "producer", from: Some(producer_name(sig.producer)), value: sig.value.to_string() });
                    let by = match sig.producer {
                        ProducerId::LAYOUT => node.parent.unwrap_or(id),
                        _ => id,
                    };
                    driven_by.insert(Link { kind: "producer".into(), node: by.to_string(), leaf: Some(leaf_text.clone()), detail: Some(producer_name(sig.producer)) });
                }
                for p in graph.waiting_on(leaf) {
                    driven_by.insert(Link { kind: "waiting".into(), node: id.to_string(), leaf: Some(leaf_text.clone()), detail: Some(producer_name(p)) });
                }
                if let Some(e) = graph.expr_of(leaf) {
                    layers.push(Layer { kind: "binding", from: None, value: format!("{{= {e}}}") });
                    for d in graph.deps_of(leaf) {
                        driven_by.insert(Link { kind: "binding".into(), node: d.node.to_string(), leaf: Some(leaf_text.clone()), detail: Some(d.path.to_string()) });
                    }
                }
                if let Some(v) = graph.live_solve_preview().get(&leaf) {
                    layers.push(Layer { kind: "live solve", from: None, value: v.to_string() });
                }
                if let Some(v) = graph.gesture_preview().get(&leaf) {
                    layers.push(Layer { kind: "preview", from: None, value: v.to_string() });
                }
                if let Some(&c) = self.clusters().var_cluster.get(&leaf) {
                    driven_by.insert(Link { kind: "cluster".into(), node: c.to_string(), leaf: Some(leaf_text.clone()), detail: None });
                }
                for reader in graph.readers_of_leaf(leaf) {
                    let link = match reader {
                        CellId::Leaf(l) => Link { kind: "binding".into(), node: l.node.to_string(), leaf: Some(leaf_text.clone()), detail: Some(l.path.to_string()) },
                        CellId::Native(k) => {
                            let (kind, n) = native(k);
                            Link { kind: kind.into(), node: n.to_string(), leaf: Some(leaf_text.clone()), detail: None }
                        }
                    };
                    drives.insert(link);
                }
                // Only what comes from more than its default is worth a line.
                let plain = layers.iter().all(|l| matches!(l.kind, "default" | "template" | "authored")) && layers.iter().filter(|l| l.kind != "default").all(|l| default.as_ref().is_some_and(|d| d.to_string() == l.value));
                if !plain {
                    let value = graph.read(doc, leaf).map(|v| v.to_string());
                    why.push(Why { leaf: leaf_text, value, layers });
                }
            }
        }
        if let Some(p) = node.parent
            && doc.facet(p, FacetKind::Layout).is_some()
        {
            driven_by.insert(Link { kind: "layout".into(), node: p.to_string(), leaf: None, detail: None });
        }
        // The session's own relations (what it selects, hovers) are not the
        // document's.
        for r in doc.relations_to(id).filter(|r| !r.from.is_session()) {
            driven_by.insert(Link { kind: format!("{:?}", r.rel).to_lowercase(), node: r.from.to_string(), leaf: None, detail: None });
        }
        for r in doc.relations_from(id).filter(|r| !r.to.is_session()) {
            drives.insert(Link { kind: format!("{:?}", r.rel).to_lowercase(), node: r.to.to_string(), leaf: None, detail: None });
        }
        // What it reads of itself is neither.
        driven_by.retain(|l| !(l.kind == "binding" && l.node == id.to_string()));
        drives.retain(|l| !(l.kind == "binding" && l.node == id.to_string()));
        Some(Inspection { node: node_ref(doc, id), path, derives, why, driven_by: driven_by.into_iter().collect(), drives: drives.into_iter().collect() })
    }
}

fn attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;")
}

/// The inspection as the agent reads it, after a view.
pub fn render(i: &Inspection) -> String {
    let mut out = format!("<connections id=\"{}\" type=\"{}\"", i.node.id, i.node.ty);
    if let Some(n) = &i.node.name {
        out += &format!(" name=\"{}\"", attr(n));
    }
    out += ">\n";
    if !i.path.is_empty() {
        let p: Vec<String> = i.path.iter().map(|r| match &r.name {
            Some(n) => format!("{} {}", r.id, n),
            None => r.id.clone(),
        }).collect();
        out += &format!("  <in path=\"{}\"/>\n", attr(&p.join(" > ")));
    }
    for d in &i.derives {
        out += &format!("  <derives>{}</derives>\n", attr(d));
    }
    for w in &i.why {
        let stack: Vec<String> = w.layers.iter().map(|l| match &l.from {
            Some(f) => format!("{} {f}: {}", l.kind, l.value),
            None => format!("{}: {}", l.kind, l.value),
        }).collect();
        out += &format!("  <why leaf=\"{}\" value=\"{}\">{}</why>\n", w.leaf, attr(w.value.as_deref().unwrap_or("?")), attr(&stack.join(" → ")));
    }
    let link = |tag: &str, l: &Link| {
        let mut s = format!("  <{tag} kind=\"{}\" node=\"{}\"", l.kind, l.node);
        if let Some(leaf) = &l.leaf {
            s += &format!(" leaf=\"{leaf}\"");
        }
        if let Some(d) = &l.detail {
            s += &format!(" detail=\"{}\"", attr(d));
        }
        s + "/>\n"
    };
    for l in &i.driven_by {
        out += &link("driven-by", l);
    }
    for l in &i.drives {
        out += &link("drives", l);
    }
    out + "</connections>"
}
