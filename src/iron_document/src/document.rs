//! The authored store: nodes, components, relations, and the derived indexes
//! over them.
//!
//! Every mutating method is `pub(crate)` and reachable only from the op
//! applier. That is the structural form of the rule in
//! plans/document-spine.md §6.1: no code path mutates the store directly.

use crate::component::{Component, ComponentKind, LeafPath, LeafStruct};
use crate::id::{NodeId, RelationId, Version};
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::{Slot, Value};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: NodeId,
    pub ty: TypeId,
    pub parent: Option<NodeId>,
    pub order: OrderKey,
    /// The version at which this node was deleted. Tombstoned nodes keep their
    /// record and components so that references fail loudly and undo restores
    /// the same id (doc 04 §2.4).
    pub tombstoned: Option<Version>,
}

impl Node {
    pub fn is_live(&self) -> bool {
        self.tombstoned.is_none()
    }
}

/// A non-containment edge. First-class storage; `target="…"` in a view is a
/// rendering of one of these (doc 05 §3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Relation {
    pub id: RelationId,
    pub from: NodeId,
    pub to: NodeId,
    pub rel: RelKind,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub data: BTreeMap<String, Value>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelKind {
    /// A clip drives a property of its target.
    Animates,
    After,
    During,
    AlignsTo,
    References,
}

impl RelKind {
    pub fn name(self) -> &'static str {
        match self {
            RelKind::Animates => "animates",
            RelKind::After => "after",
            RelKind::During => "during",
            RelKind::AlignsTo => "aligns_to",
            RelKind::References => "references",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Document {
    nodes: BTreeMap<NodeId, Node>,
    components: BTreeMap<ComponentKind, BTreeMap<NodeId, Component>>,
    relations: BTreeMap<RelationId, Relation>,
    next_id: u64,
    next_rel: u64,
    version: Version,

    // Derived. Rebuilt incrementally by the mutators, wholesale by `reindex`.
    // Only live nodes appear in these.
    children: BTreeMap<NodeId, Vec<NodeId>>,
    roots: Vec<NodeId>,
    by_type: BTreeMap<TypeId, BTreeSet<NodeId>>,
    by_component: BTreeMap<ComponentKind, BTreeSet<NodeId>>,
    by_source: BTreeMap<NodeId, BTreeSet<RelationId>>,
    by_target: BTreeMap<NodeId, BTreeSet<RelationId>>,
}

impl Document {
    pub fn new() -> Document {
        Document {
            next_id: 1,
            next_rel: 1,
            ..Default::default()
        }
    }

    // ---- reads --------------------------------------------------------

    pub fn version(&self) -> Version {
        self.version
    }

    /// The id the next `create` will be given. Peek only; nothing is reserved
    /// until an op is applied. A batch creating several nodes uses
    /// consecutive ids from here, see [`Document::next_ids`].
    pub fn next_id(&self) -> NodeId {
        NodeId::from_raw(self.next_id)
    }

    pub fn next_ids(&self, n: usize) -> Vec<NodeId> {
        (0..n as u64)
            .map(|i| NodeId::from_raw(self.next_id + i))
            .collect()
    }

    pub fn next_relation_id(&self) -> RelationId {
        RelationId::from_raw(self.next_rel)
    }

    /// Any node, live or tombstoned.
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub fn live(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id).filter(|n| n.is_live())
    }

    pub fn is_live(&self, id: NodeId) -> bool {
        self.live(id).is_some()
    }

    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values()
    }

    pub fn live_nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values().filter(|n| n.is_live())
    }

    pub fn roots(&self) -> &[NodeId] {
        &self.roots
    }

    /// Live children in sibling order.
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        self.children.get(&id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Live descendants, preorder.
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = self.children(id).iter().rev().copied().collect();
        while let Some(n) = stack.pop() {
            out.push(n);
            stack.extend(self.children(n).iter().rev().copied());
        }
        out
    }

    /// Whether `a` is a strict ancestor of `of`.
    pub fn is_ancestor(&self, a: NodeId, of: NodeId) -> bool {
        let mut cur = self.nodes.get(&of).and_then(|n| n.parent);
        while let Some(p) = cur {
            if p == a {
                return true;
            }
            cur = self.nodes.get(&p).and_then(|n| n.parent);
        }
        false
    }

    pub fn nodes_of_type(&self, ty: TypeId) -> impl Iterator<Item = NodeId> + '_ {
        self.by_type.get(&ty).into_iter().flatten().copied()
    }

    pub fn nodes_with(&self, kind: ComponentKind) -> impl Iterator<Item = NodeId> + '_ {
        self.by_component.get(&kind).into_iter().flatten().copied()
    }

    pub fn component(&self, id: NodeId, kind: ComponentKind) -> Option<&Component> {
        self.components.get(&kind)?.get(&id)
    }

    pub fn get<T: LeafStruct>(&self, id: NodeId) -> Option<&T> {
        self.component(id, T::KIND).and_then(T::unwrap_ref)
    }

    /// All components of a node, in kind order.
    pub fn components_of(&self, id: NodeId) -> impl Iterator<Item = &Component> {
        self.components.values().filter_map(move |m| m.get(&id))
    }

    pub fn leaf(&self, id: NodeId, path: LeafPath) -> Option<&Slot> {
        self.component(id, path.component)?.get(path.field)
    }

    pub fn relation(&self, id: RelationId) -> Option<&Relation> {
        self.relations.get(&id)
    }

    pub fn relations(&self) -> impl Iterator<Item = &Relation> {
        self.relations.values()
    }

    /// Edges leaving `id`.
    pub fn relations_from(&self, id: NodeId) -> impl Iterator<Item = &Relation> {
        self.by_source
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|r| self.relations.get(r))
    }

    /// Edges arriving at `id`: "what drives this node" (doc 05 §3.1).
    pub fn relations_to(&self, id: NodeId) -> impl Iterator<Item = &Relation> {
        self.by_target
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|r| self.relations.get(r))
    }

    // ---- crate-private mutators ---------------------------------------

    pub(crate) fn set_version(&mut self, v: Version) {
        self.version = v;
    }

    pub(crate) fn advance_next_id(&mut self, past: NodeId) {
        self.next_id = self.next_id.max(past.raw() + 1);
    }

    pub(crate) fn advance_next_relation_id(&mut self, past: RelationId) {
        self.next_rel = self.next_rel.max(past.raw() + 1);
    }

    pub(crate) fn insert_node(&mut self, node: Node, comps: Vec<Component>) {
        let id = node.id;
        debug_assert!(node.is_live());
        self.by_type.entry(node.ty).or_default().insert(id);
        self.nodes.insert(id, node);
        self.children.entry(id).or_default();
        for c in comps {
            self.insert_component(id, c);
        }
        self.place(id);
    }

    /// Bring a tombstoned node back, replacing its placement and components
    /// with the given ones.
    pub(crate) fn restore_node(
        &mut self,
        id: NodeId,
        ty: TypeId,
        parent: Option<NodeId>,
        order: OrderKey,
        comps: Vec<Component>,
    ) {
        for kind in ComponentKind::ALL {
            if let Some(m) = self.components.get_mut(kind) {
                m.remove(&id);
            }
        }
        {
            let n = self.nodes.get_mut(&id).expect("restore of known node");
            n.ty = ty;
            n.parent = parent;
            n.order = order;
            n.tombstoned = None;
        }
        self.by_type.entry(ty).or_default().insert(id);
        self.children.entry(id).or_default();
        for c in comps {
            self.insert_component(id, c);
        }
        self.place(id);
    }

    /// Tombstone one node. The caller tombstones descendants separately so
    /// each gets its own inverse op.
    pub(crate) fn tombstone(&mut self, id: NodeId, at: Version) {
        self.unplace(id);
        let n = self.nodes.get_mut(&id).expect("tombstone of known node");
        n.tombstoned = Some(at);
        let ty = n.ty;
        if let Some(s) = self.by_type.get_mut(&ty) {
            s.remove(&id);
        }
        for (kind, m) in &self.components {
            if m.contains_key(&id)
                && let Some(s) = self.by_component.get_mut(kind)
            {
                s.remove(&id);
            }
        }
    }

    pub(crate) fn set_parent(&mut self, id: NodeId, parent: Option<NodeId>, order: OrderKey) {
        self.unplace(id);
        let n = self.nodes.get_mut(&id).expect("known node");
        n.parent = parent;
        n.order = order;
        self.place(id);
    }

    pub(crate) fn set_order(&mut self, id: NodeId, order: OrderKey) {
        self.unplace(id);
        self.nodes.get_mut(&id).expect("known node").order = order;
        self.place(id);
    }

    pub(crate) fn insert_component(&mut self, id: NodeId, c: Component) {
        let kind = c.kind();
        self.components.entry(kind).or_default().insert(id, c);
        if self.is_live(id) {
            self.by_component.entry(kind).or_default().insert(id);
        }
    }

    pub(crate) fn remove_component(
        &mut self,
        id: NodeId,
        kind: ComponentKind,
    ) -> Option<Component> {
        let c = self.components.get_mut(&kind)?.remove(&id)?;
        if let Some(s) = self.by_component.get_mut(&kind) {
            s.remove(&id);
        }
        Some(c)
    }

    pub(crate) fn component_mut(
        &mut self,
        id: NodeId,
        kind: ComponentKind,
    ) -> Option<&mut Component> {
        self.components.get_mut(&kind)?.get_mut(&id)
    }

    pub(crate) fn insert_relation(&mut self, r: Relation) {
        self.by_source.entry(r.from).or_default().insert(r.id);
        self.by_target.entry(r.to).or_default().insert(r.id);
        self.relations.insert(r.id, r);
    }

    pub(crate) fn remove_relation(&mut self, id: RelationId) -> Option<Relation> {
        let r = self.relations.remove(&id)?;
        if let Some(s) = self.by_source.get_mut(&r.from) {
            s.remove(&id);
        }
        if let Some(s) = self.by_target.get_mut(&r.to) {
            s.remove(&id);
        }
        Some(r)
    }

    /// Insert `id` into its parent's child list (or the roots) at the position
    /// its order key dictates. Ties on key break by id so order is total.
    fn place(&mut self, id: NodeId) {
        let (parent, key) = {
            let n = &self.nodes[&id];
            (n.parent, n.order.clone())
        };
        let nodes = &self.nodes;
        let container = match parent {
            Some(p) => self.children.entry(p).or_default(),
            None => &mut self.roots,
        };
        let pos = container.partition_point(|&c| {
            let cn = &nodes[&c];
            (&cn.order, c) < (&key, id)
        });
        container.insert(pos, id);
    }

    fn unplace(&mut self, id: NodeId) {
        let parent = self.nodes[&id].parent;
        let container = match parent {
            Some(p) => match self.children.get_mut(&p) {
                Some(c) => c,
                None => return,
            },
            None => &mut self.roots,
        };
        if let Some(pos) = container.iter().position(|&c| c == id) {
            container.remove(pos);
        }
    }

    // ---- bulk construction (used by load) -----------------------------

    /// Build from raw tables without validation beyond what `reindex` checks.
    /// Used by deserialisation; ops never go through here.
    pub(crate) fn from_parts(
        nodes: BTreeMap<NodeId, Node>,
        components: BTreeMap<ComponentKind, BTreeMap<NodeId, Component>>,
        relations: BTreeMap<RelationId, Relation>,
        next_id: u64,
        next_rel: u64,
        version: Version,
    ) -> Document {
        let mut d = Document {
            nodes,
            components,
            relations,
            next_id,
            next_rel,
            version,
            ..Default::default()
        };
        d.reindex();
        d
    }

    pub(crate) fn raw_nodes(&self) -> &BTreeMap<NodeId, Node> {
        &self.nodes
    }

    pub(crate) fn raw_components(&self) -> &BTreeMap<ComponentKind, BTreeMap<NodeId, Component>> {
        &self.components
    }

    pub(crate) fn raw_next(&self) -> (u64, u64) {
        (self.next_id, self.next_rel)
    }

    fn reindex(&mut self) {
        self.children.clear();
        self.roots.clear();
        self.by_type.clear();
        self.by_component.clear();
        self.by_source.clear();
        self.by_target.clear();
        let live: Vec<NodeId> = self
            .nodes
            .values()
            .filter(|n| n.is_live())
            .map(|n| n.id)
            .collect();
        for &id in &live {
            self.children.entry(id).or_default();
            self.by_type
                .entry(self.nodes[&id].ty)
                .or_default()
                .insert(id);
        }
        for &id in &live {
            self.place(id);
        }
        for (kind, m) in &self.components {
            for id in m.keys() {
                if self.nodes.get(id).is_some_and(|n| n.is_live()) {
                    self.by_component.entry(*kind).or_default().insert(*id);
                }
            }
        }
        let rels: Vec<Relation> = self.relations.values().cloned().collect();
        for r in rels {
            self.by_source.entry(r.from).or_default().insert(r.id);
            self.by_target.entry(r.to).or_default().insert(r.id);
        }
    }

    /// Cross-check the indexes against the tables. Cheap enough to run in
    /// tests after every transaction; not run in production.
    pub fn check_invariants(&self) -> Result<(), String> {
        for n in self.nodes.values() {
            if n.is_live() {
                let container: &[NodeId] = match n.parent {
                    Some(p) => {
                        if !self.is_live(p) {
                            return Err(format!("{} has non-live parent {p}", n.id));
                        }
                        self.children(p)
                    }
                    None => &self.roots,
                };
                if !container.contains(&n.id) {
                    return Err(format!("{} missing from its sibling list", n.id));
                }
                if !self.by_type.get(&n.ty).is_some_and(|s| s.contains(&n.id)) {
                    return Err(format!("{} missing from by_type", n.id));
                }
                if self.is_ancestor(n.id, n.id) {
                    return Err(format!("{} is its own ancestor", n.id));
                }
            } else {
                if self.roots.contains(&n.id) || self.children.values().any(|c| c.contains(&n.id)) {
                    return Err(format!("tombstoned {} still placed", n.id));
                }
                if self.by_type.get(&n.ty).is_some_and(|s| s.contains(&n.id)) {
                    return Err(format!("tombstoned {} still in by_type", n.id));
                }
            }
        }
        for (parent, kids) in &self.children {
            let mut prev: Option<(&OrderKey, NodeId)> = None;
            for &k in kids {
                let kn = &self.nodes[&k];
                if kn.parent != Some(*parent) {
                    return Err(format!(
                        "{k} listed under {parent} but parent is {:?}",
                        kn.parent
                    ));
                }
                if let Some(p) = prev
                    && p >= (&kn.order, k)
                {
                    return Err(format!("children of {parent} out of order at {k}"));
                }
                prev = Some((&kn.order, k));
            }
        }
        for (kind, set) in &self.by_component {
            for id in set {
                if !self.is_live(*id) {
                    return Err(format!("by_component[{}] holds non-live {id}", kind.name()));
                }
                if self.component(*id, *kind).is_none() {
                    return Err(format!(
                        "by_component[{}] holds {id} without the component",
                        kind.name()
                    ));
                }
            }
        }
        for (kind, m) in &self.components {
            for id in m.keys() {
                if !self.nodes.contains_key(id) {
                    return Err(format!("component {} on unknown node {id}", kind.name()));
                }
            }
        }
        for r in self.relations.values() {
            if !self
                .by_source
                .get(&r.from)
                .is_some_and(|s| s.contains(&r.id))
                || !self.by_target.get(&r.to).is_some_and(|s| s.contains(&r.id))
            {
                return Err(format!("relation {} not indexed", r.id));
            }
        }
        Ok(())
    }
}
