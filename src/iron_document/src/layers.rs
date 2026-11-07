//! The layers panel's read (plans/specs/layers-panel.md §7.1): one
//! composition's drawing tree as the rows a person can see, flattened.
//!
//! A row is a node *as reached through the composition*: its chain, the nodes
//! from the composition's top level down to it, through every group and
//! embed on the way. That is the select tool's `Reach::chain`, which the
//! session stores on each `selects` relation, so a row's key (the chain's ids
//! joined by spaces) is compared with the selection as a string.
//!
//! Only rows whose parent's key is in `open` are descended into: a closed
//! group of 360 paths costs one row. An embed's children are the embedded
//! composition's top-level nodes. In 2D the rows run topmost first (the last
//! child draws on top, so it is listed first, as Figma lists layers); in 3D
//! they keep the stored order.

use crate::composition::{self, Dims};
use crate::document::Document;
use crate::facet::{Name, Text};
use crate::id::NodeId;
use serde::Serialize;
use std::collections::BTreeSet;

/// How deep embeds are followed (`document_bridge::nest::MAX_DEPTH`).
const MAX_EMBED_DEPTH: u32 = 8;

/// The node types a layer row stands for. Everything else under a
/// composition (a path's points and contours, an instance's overrides,
/// a table's rows, constraints) belongs to the row above it.
const LAYER_TYPES: [&str; 12] = ["group", "rect", "ellipse", "text", "table", "path", "mesh", "slider", "embed", "camera", "empty", "use"];

/// Another composition that pictures the same entity as a row.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Chip {
    pub composition: String,
    pub name: Option<String>,
    pub dims: &'static str,
    /// The first figure of the entity in that composition.
    pub node: String,
    /// Its chain in its own composition, as a key.
    pub chain: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Row {
    /// The chain's ids joined by spaces: the row's identity.
    pub key: String,
    /// 0 for the composition's top level.
    pub depth: u32,
    pub id: String,
    /// The node's type name.
    pub kind: &'static str,
    /// The `name` facet; for text with no name, its first line.
    pub name: Option<String>,
    /// How many layer rows sit directly under this one, open or not.
    pub count: usize,
    /// The dimension of the composition this row is drawn in.
    pub dims: &'static str,
    /// For an embed: the composition it shows, `[id, name]`.
    pub embed: Option<(String, Option<String>)>,
    /// The entity this node pictures, if it is a figure.
    pub entity: Option<String>,
    pub chips: Vec<Chip>,
}

fn is_layer(doc: &Document, id: NodeId) -> bool {
    !id.is_expanded() && !id.is_session() && doc.live(id).is_some_and(|n| LAYER_TYPES.contains(&n.ty.name()))
}

fn dims_str(d: Option<Dims>) -> &'static str {
    match d {
        Some(Dims::Three) => "3d",
        _ => "2d",
    }
}

fn name_of(doc: &Document, id: NodeId) -> Option<String> {
    let named = doc.get::<Name>(id).and_then(|n| n.text.constant()).and_then(|v| v.as_str()).filter(|s| !s.is_empty());
    if let Some(n) = named {
        return Some(n.to_owned());
    }
    let text = doc.get::<Text>(id).and_then(|t| t.content.constant()).and_then(|v| v.as_str())?;
    let line = text.lines().find(|l| !l.trim().is_empty())?.trim();
    Some(if line.chars().count() > 48 { format!("{}…", line.chars().take(47).collect::<String>()) } else { line.to_owned() })
}

/// The rows a node's children make, in panel order: the embedded
/// composition's top level for an embed, the node's own layer children
/// otherwise. `dims` is the dimension they are drawn in.
fn child_rows(doc: &Document, id: NodeId) -> (Vec<NodeId>, Option<Dims>) {
    let (parent, dims) = match composition::embedded(doc, id).filter(|_| doc.live(id).is_some_and(|n| n.ty.name() == "embed")) {
        Some(c) => (c, composition::dims(doc, c)),
        None => (id, composition::composition_of(doc, id).and_then(|c| composition::dims(doc, c))),
    };
    let mut out: Vec<NodeId> = doc.children(parent).iter().copied().filter(|&c| is_layer(doc, c)).collect();
    if dims != Some(Dims::Three) {
        out.reverse();
    }
    (out, dims)
}

/// The chain of `id` in its own composition: its ancestors below the
/// composition, outermost first, then itself.
pub fn lineage(doc: &Document, id: NodeId) -> Vec<NodeId> {
    let mut out = vec![id];
    let mut cur = doc.live(id).and_then(|n| n.parent);
    while let Some(n) = cur.and_then(|c| doc.live(c)) {
        if n.ty.name() == "composition" || n.ty.name() == "template" {
            break;
        }
        out.push(n.id);
        cur = n.parent;
    }
    out.reverse();
    out
}

/// A chain as a row key.
pub fn key(chain: &[NodeId]) -> String {
    chain.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ")
}

fn chips(doc: &Document, entity: NodeId, home: Option<NodeId>) -> Vec<Chip> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for f in composition::figures_of(doc, entity) {
        let Some(c) = composition::composition_of(doc, f) else { continue };
        if Some(c) == home || !seen.insert(c) {
            continue;
        }
        out.push(Chip {
            composition: c.to_string(),
            name: name_of(doc, c),
            dims: dims_str(composition::dims(doc, c)),
            node: f.to_string(),
            chain: key(&lineage(doc, f)),
        });
    }
    out
}

/// The visible rows of `composition`, descending into each row whose key is
/// in `open`.
pub fn rows(doc: &Document, composition: NodeId, open: &BTreeSet<String>) -> Vec<Row> {
    let mut out = Vec::new();
    let (top, dims) = child_rows(doc, composition);
    walk(doc, &top, dims, &mut Vec::new(), 0, 0, open, &mut out);
    out
}

#[allow(clippy::too_many_arguments)]
fn walk(doc: &Document, ids: &[NodeId], dims: Option<Dims>, chain: &mut Vec<NodeId>, depth: u32, embeds: u32, open: &BTreeSet<String>, out: &mut Vec<Row>) {
    for &id in ids {
        let Some(node) = doc.live(id) else { continue };
        chain.push(id);
        let k = key(chain);
        let is_embed = node.ty.name() == "embed";
        let shown = if is_embed { composition::embedded(doc, id) } else { None };
        let (children, child_dims) = if is_embed && (shown.is_none() || embeds >= MAX_EMBED_DEPTH) { (Vec::new(), None) } else { child_rows(doc, id) };
        let entity = composition::figured_by(doc, id);
        out.push(Row {
            key: k.clone(),
            depth,
            id: id.to_string(),
            kind: node.ty.name(),
            name: name_of(doc, id),
            count: children.len(),
            dims: dims_str(dims),
            embed: shown.map(|c| (c.to_string(), name_of(doc, c))),
            entity: entity.map(|e| e.to_string()),
            chips: entity.map(|e| chips(doc, e, composition::composition_of(doc, id))).unwrap_or_default(),
        });
        if !children.is_empty() && open.contains(&k) {
            walk(doc, &children, child_dims, chain, depth + 1, embeds + u32::from(is_embed), open, out);
        }
        chain.pop();
    }
}
