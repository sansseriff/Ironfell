//! What moved since the host last asked (reactive-engine.md §12.2; handoff
//! §4.2, task 4): one set of changes, taken once (`Store::take_changes`),
//! for every consumer. A host with several consumers keeps what each has
//! not yet used (the app's `document_bridge::ChangeInbox`), so one consumer
//! never takes what another still needs.
//!
//! The store fills it from what it already knows: the ops it applied and
//! their inverses, the reactive graph's changed leaves, and the geometry
//! revisions.

use crate::expr::Leaf;
use crate::facet::{Facet, FacetKind, LeafPath};
use crate::geom::GeometryChanges;
use crate::id::{NodeId, Version};
use crate::op::Op;
use crate::tree::Tree;
use std::collections::BTreeSet;

/// A deleted node as it was when it went (`Docs::gone`): what readers
/// after the fact need, since a copy leaves no tombstone.
#[derive(Clone, Debug)]
pub(crate) struct Went {
    pub parent: Option<NodeId>,
    pub depth: usize,
    pub ty: crate::registry::TypeId,
}

/// Every node deleted since the changes were last taken.
pub(crate) type Gone = std::collections::BTreeMap<NodeId, Went>;

/// The revisions a host reports (`ui::publish`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Revisions {
    /// The authored document's version.
    pub authored: Version,
    /// Bumped by every session-tier change.
    pub session: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Changes {
    /// Leaves whose resolved value moved: a binding re-evaluated, a preview,
    /// a publish, a clip.
    pub values: BTreeSet<Leaf>,
    /// Nodes made, authored and derived, parents first.
    pub created: Vec<NodeId>,
    /// Nodes that went, with the parent each had, derived ones included: a
    /// deleted node's derived descendants go with it, and can no longer be
    /// enumerated afterwards.
    pub removed: Vec<(NodeId, Option<NodeId>)>,
    /// Nodes reparented or reordered, with the parent each had.
    pub moved: Vec<(NodeId, Option<NodeId>)>,
    /// The ends of relations linked or unlinked.
    pub relations: Vec<(NodeId, NodeId)>,
    /// Leaves whose slot or facet changed, whatever the value: a `Set`, a
    /// facet added or removed. Editability and render analysis follow a
    /// binding even to an equal value.
    pub slots: BTreeSet<Leaf>,
    /// Leaves reported as changed on request with no change of value: a
    /// table's tracks, a text whose font arrived.
    pub marks: BTreeSet<Leaf>,
    /// Parents whose children changed (`None`: the roots).
    pub structure: BTreeSet<Option<NodeId>>,
    /// Owners loaded and unloaded (reactive-engine.md §8). Empty until
    /// owners can unload (phase 4).
    pub loaded: Vec<NodeId>,
    pub unloaded: Vec<NodeId>,
    /// What moved spatially, classified for the spatial index: structure,
    /// placements, local shapes.
    pub geometry: GeometryChanges,
    pub revisions: Revisions,
    /// How deep each created or removed node was when it was noted: the
    /// order `sorted` puts them in, since a removed node's ancestry can
    /// change after.
    depths: std::collections::BTreeMap<NodeId, usize>,
    /// The ids in `removed`, to find one without a scan (a subtree's delete
    /// notes thousands).
    removed_ids: BTreeSet<NodeId>,
}

impl Changes {
    /// Nothing moved. Revisions are not changes.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
            && self.created.is_empty()
            && self.removed.is_empty()
            && self.moved.is_empty()
            && self.relations.is_empty()
            && self.slots.is_empty()
            && self.marks.is_empty()
            && self.structure.is_empty()
            && self.loaded.is_empty()
            && self.unloaded.is_empty()
            && self.geometry.is_empty()
    }

    /// Whether the tree's shape changed: a node made, gone or moved.
    pub fn structural(&self) -> bool {
        !self.created.is_empty() || !self.removed.is_empty() || !self.moved.is_empty() || !self.loaded.is_empty() || !self.unloaded.is_empty()
    }

    /// Every node something here names: what a reconciler re-examines.
    pub fn nodes(&self) -> BTreeSet<NodeId> {
        let mut out: BTreeSet<NodeId> = BTreeSet::new();
        out.extend(self.values.iter().chain(&self.slots).chain(&self.marks).map(|l| l.node));
        out.extend(self.created.iter().copied());
        for &(n, p) in self.removed.iter().chain(&self.moved) {
            out.insert(n);
            out.extend(p);
        }
        for &(a, b) in &self.relations {
            out.insert(a);
            out.insert(b);
        }
        out.extend(self.structure.iter().flatten().copied());
        out.extend(self.loaded.iter().chain(&self.unloaded).copied());
        out
    }

    /// In a fixed order, whatever order the writes came in: `created`
    /// parents first, `removed` deepest first, each then by id; `moved` and
    /// `relations` by value (reactive-engine handoff §4.4, stage 1).
    pub(crate) fn sorted(mut self) -> Changes {
        let depth = |d: &std::collections::BTreeMap<NodeId, usize>, id: &NodeId| d.get(id).copied().unwrap_or(0);
        let d = std::mem::take(&mut self.depths);
        self.created.sort_by_key(|id| (depth(&d, id), *id));
        self.removed.sort_by_key(|(id, parent)| (std::cmp::Reverse(depth(&d, id)), *id, *parent));
        self.moved.sort();
        self.relations.sort();
        self.depths = d;
        self
    }

    /// Add `later`, taken after this.
    pub fn merge(&mut self, later: Changes) {
        self.values.extend(later.values);
        self.created.extend(later.created);
        self.removed.extend(later.removed);
        self.removed_ids.extend(later.removed_ids);
        self.moved.extend(later.moved);
        self.relations.extend(later.relations);
        self.slots.extend(later.slots);
        self.marks.extend(later.marks);
        self.structure.extend(later.structure);
        self.loaded.extend(later.loaded);
        self.unloaded.extend(later.unloaded);
        self.geometry.merge(later.geometry);
        self.revisions = later.revisions;
        self.depths.extend(later.depths);
    }

    /// Record `ops`, applied to `doc`, with `inverse`, the inverse of
    /// everything applied with them, derived nodes included: a delete's
    /// inverse names every node it took, deepest first.
    ///
    /// `gone` holds each deleted node's parent and depth as they were when
    /// it went (`Docs::gone`): a copy leaves no tombstone to read them from.
    pub(crate) fn note<'a>(&mut self, doc: Tree<'_>, gone: &Gone, ops: impl IntoIterator<Item = &'a Op>, inverse: &[Op]) {
        for op in ops {
            match op {
                Op::Create { id, parent, .. } => {
                    self.created.push(*id);
                    self.depths.insert(*id, doc.depth(*id));
                    self.structure.insert(*parent);
                }
                Op::Delete { id } => {
                    let parent = match gone.get(id) {
                        Some(w) => w.parent,
                        None => doc.node(*id).and_then(|n| n.parent),
                    };
                    self.remove(doc, gone, *id, parent)
                }
                Op::Reparent { id, parent, .. } => {
                    let old = inverse.iter().find_map(|i| match i {
                        Op::Reparent { id: j, parent: p, .. } if j == id => Some(*p),
                        _ => None,
                    });
                    let old = old.unwrap_or(None);
                    self.moved.push((*id, old));
                    self.structure.insert(old);
                    self.structure.insert(*parent);
                }
                Op::Reorder { id, .. } => {
                    let parent = doc.node(*id).and_then(|n| n.parent);
                    self.moved.push((*id, parent));
                    self.structure.insert(parent);
                }
                Op::Set { id, path, .. } => {
                    self.slots.insert(Leaf { node: *id, path: *path });
                }
                Op::AddFacet { id, facet } => self.slots.extend(leaves(*id, facet)),
                Op::RemoveFacet { id, kind } => {
                    let removed = inverse.iter().find_map(|i| match i {
                        Op::AddFacet { id: j, facet } if j == id && facet.kind() == *kind => Some(facet),
                        _ => None,
                    });
                    match removed {
                        Some(f) => self.slots.extend(leaves(*id, f)),
                        None => self.slots.extend(kind_leaves(*id, *kind)),
                    }
                }
                Op::Link { rel } => self.relations.push((rel.from, rel.to)),
                Op::Unlink { id } => {
                    if let Some((a, b)) = inverse.iter().find_map(|i| match i {
                        Op::Link { rel } if rel.id == *id => Some((rel.from, rel.to)),
                        _ => None,
                    }) {
                        self.relations.push((a, b));
                    }
                }
            }
        }
        // A delete's inverse recreates what it took, deepest first: the
        // nodes no op names (a deleted instance's derived descendants).
        for i in inverse {
            if let Op::Create { id, parent, .. } = i {
                self.remove(doc, gone, *id, *parent);
            }
        }
    }

    fn remove(&mut self, doc: Tree<'_>, gone: &Gone, id: NodeId, parent: Option<NodeId>) {
        if self.removed_ids.insert(id) {
            self.removed.push((id, parent));
            self.depths.insert(id, gone.get(&id).map_or_else(|| doc.depth(id), |w| w.depth));
            self.structure.insert(parent);
        }
    }
}

/// Every leaf of a facet a node has.
fn leaves(id: NodeId, facet: &Facet) -> Vec<Leaf> {
    let kind = facet.kind();
    facet.field_names().into_iter().filter_map(|f| LeafPath::new(kind, f).ok().map(|path| Leaf { node: id, path })).collect()
}

/// Every declared leaf of a facet kind.
fn kind_leaves(id: NodeId, kind: FacetKind) -> Vec<Leaf> {
    kind.fields().iter().filter_map(|(f, _)| LeafPath::new(kind, f).ok().map(|path| Leaf { node: id, path })).collect()
}
