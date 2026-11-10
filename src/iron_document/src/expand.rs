//! Expansion (component-model.md §3, §6): the nodes every linked instance
//! derives from its definition, kept in the document beside the authored
//! nodes and never stored.
//!
//! A linked `entity` (`instance.mode = linked`) derives its definition's
//! interface and behaviour: one `port` per `port_spec` and one `equation`
//! per equation, as its children. An `in` port reads the entity's argument
//! (`#entity.args.<name>`) unless a connection feeds it. Each `use` that
//! figures a linked entity derives the definition's template for its
//! notation, as its children. References inside the copies follow: to a
//! `port_spec` or equation, the entity's copy; to a template node, the
//! `use`'s copy.
//!
//! An expanded node's id is `NodeId::expanded(owner, local)`, the owner
//! being the entity or `use` and `local` the definition node it copies, so
//! re-expanding gives every node the id it had and references to expanded
//! nodes (a connection to `#m/7`, a readout bound to `#m/7.port.signal`)
//! survive edits to the definition, undo and reload.
//!
//! [`expand`] computes the whole wanted set and reconciles the document with
//! it through the op applier, returning the ops it applied so consumers that
//! follow ops (the Bevy bridge, geometry revisions) see the change. They are
//! not history: undo and redo re-expand from the authored state.

use crate::definition;
use crate::document::{Document, RelKind, Relation};
use crate::expr::{Expr, Leaf};
use crate::facet::{Facet, FacetKind, Instance, LeafPath, LeafStruct, Override, Use};
use crate::id::{NodeId, RelationId, Version};
use crate::op::{self, Op};
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::{Slot, Value};
use std::collections::{BTreeMap, BTreeSet};

/// One node an instance should have.
struct Want {
    id: NodeId,
    ty: TypeId,
    parent: NodeId,
    order: OrderKey,
    facets: Vec<Facet>,
    /// Whether what this node derives in turn (an instance's ports, a
    /// picture's template) was expanded by whoever wanted it. The scope
    /// pass (`want_scopes`) expands the rest.
    scoped: bool,
}

/// Something expansion could not do, or did past a limit, on the node it
/// concerns (reactivity.md §4.3): shown on the node, never silent.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub node: NodeId,
    /// `depth`, `depth_blocked`, `budget`, `truncated`, `expansion`, `binding`.
    pub kind: &'static str,
    pub message: String,
}

/// Scopes (instances, repeat items) nested deeper than this are reported.
pub const DEPTH_REPORTED: usize = 16;
/// Scopes nested deeper than this are not expanded.
pub const DEPTH_MAX: usize = 128;
/// The most derived nodes a document may have.
pub const NODE_BUDGET: usize = 50_000;

/// The definition a linked entity instantiates, if it is one.
pub fn linked_definition(doc: &Document, entity: NodeId) -> Option<NodeId> {
    let inst = doc.get::<Instance>(entity)?;
    if inst.mode.constant().and_then(|v| v.as_str()) != Some("linked") {
        return None;
    }
    let def = inst.def.constant()?.as_ref()?;
    doc.live(def).filter(|n| n.ty.name() == "definition").map(|_| def)
}

/// A *reference* (instance-cache.md §3, phase D): a `use` whose own
/// `instance` facet names a definition, `linked`, with no entity figured.
/// It is drawn from its definition's template, wherever that template is,
/// and never expanded: its picture costs no nodes. SVG import makes one for
/// every repeated piece of a drawing. Its template, for `notation`, if `u`
/// is one.
pub fn reference_template(doc: &Document, u: NodeId) -> Option<NodeId> {
    if doc.live(u)?.ty.name() != "use" || doc.relations_from(u).any(|r| r.rel == RelKind::Figures) {
        return None;
    }
    let inst = doc.get::<Instance>(u)?;
    if inst.mode.constant().and_then(|v| v.as_str()) != Some("linked") {
        return None;
    }
    let def = inst.def.constant()?.as_ref()?;
    doc.live(def).filter(|n| n.ty.name() == "definition")?;
    let notation = doc.get::<Use>(u).and_then(|x| x.notation.constant()).and_then(|v| v.as_str()).unwrap_or("free");
    definition::template_of(doc, def, notation)
}

/// The definition a `use` is the main of (component-authoring.md §1): its
/// `instance` facet says `main`. A main figures no entity; edits inside it
/// are edits to the definition.
pub fn main_definition(doc: &Document, u: NodeId) -> Option<NodeId> {
    let inst = doc.get::<Instance>(u)?;
    if inst.mode.constant().and_then(|v| v.as_str()) != Some("main") {
        return None;
    }
    let def = inst.def.constant()?.as_ref()?;
    doc.live(def).filter(|n| n.ty.name() == "definition").map(|_| def)
}

/// The template a main shows, for its notation.
pub fn main_template(doc: &Document, u: NodeId) -> Option<NodeId> {
    let def = main_definition(doc, u)?;
    let notation = doc.get::<Use>(u)?.notation.constant()?.as_str()?.to_owned();
    definition::template_of(doc, def, &notation)
}

/// The entity a `use` figures.
pub fn use_entity(doc: &Document, u: NodeId) -> Option<NodeId> {
    doc.relations_from(u).find(|r| r.rel == RelKind::Figures && doc.is_live(r.to)).map(|r| r.to)
}

/// One relation an instance should have: a derived connection's ends, a
/// derived picture's `figures`.
struct RelWant {
    rel: Relation,
}

/// What every scope derives, nodes parents first, then relations.
#[derive(Default)]
struct Wanted {
    nodes: Vec<Want>,
    /// Where each wanted node is in `nodes`.
    index: BTreeMap<NodeId, usize>,
    rels: Vec<RelWant>,
    problems: Vec<Problem>,
    full: bool,
}

impl Wanted {
    /// Want a node, within the document's budget of derived nodes.
    fn push(&mut self, w: Want) -> bool {
        if self.nodes.len() >= NODE_BUDGET {
            if !self.full {
                self.full = true;
                self.problems.push(Problem { node: w.parent, kind: "budget", message: format!("the document derives more than {NODE_BUDGET} nodes; the rest are left out, from {} on", w.id) });
            }
            return false;
        }
        self.index.insert(w.id, self.nodes.len());
        self.nodes.push(w);
        true
    }

    fn get(&self, id: NodeId) -> Option<&Want> {
        self.index.get(&id).map(|&i| &self.nodes[i])
    }

    /// Whether `owner`'s scope may be expanded: not when the budget is
    /// spent or it is nested past `DEPTH_MAX`. Past `DEPTH_REPORTED` it is
    /// expanded and reported, once where the nesting crosses the line.
    fn enter(&mut self, owner: NodeId) -> bool {
        if self.full {
            return false;
        }
        let depth = owner.nesting() + 1;
        if depth > DEPTH_MAX {
            self.problems.push(Problem { node: owner, kind: "depth_blocked", message: format!("nested more than {DEPTH_MAX} deep: not expanded (does something contain itself?)") });
            return false;
        }
        if depth == DEPTH_REPORTED + 1 {
            self.problems.push(Problem { node: owner, kind: "depth", message: format!("nested more than {DEPTH_REPORTED} deep") });
        }
        true
    }
}

/// `n`, a node of a definition's content, as instance `owner` derives it:
/// `owner`'s path followed by `n`'s.
fn relocate(owner: NodeId, n: NodeId) -> Option<NodeId> {
    let mut path = owner.path();
    path.extend(n.path());
    NodeId::from_path(&path)
}

/// A definition's own content an entity derives: its port_specs,
/// equations, inner entities and connections.
fn is_body(doc: &Document, def: NodeId, node: NodeId) -> bool {
    doc.node(node).is_some_and(|n| n.parent == Some(def) && matches!(n.ty.name(), "port_spec" | "equation" | "entity" | "connection" | "channel" | "machine" | "let" | "trace" | "accumulate"))
}

/// The definition an entity instantiates and is linked to, read from an
/// authored or derived entity's facets.
fn linked_def_of(doc: &Document, facets: &[Facet]) -> Option<NodeId> {
    facets.iter().find_map(|f| match f {
        Facet::Instance(inst) if inst.mode.constant().and_then(|v| v.as_str()) == Some("linked") => inst.def.constant()?.as_ref(),
        _ => None,
    }).filter(|&d| doc.live(d).is_some_and(|n| n.ty.name() == "definition"))
}

/// The items each repeat and conditional has now, by key, in order.
pub type Items = BTreeMap<NodeId, Vec<String>>;

/// Whether a facet makes its node's children a template whose copies are
/// its items: a repeat, or a conditional (`when`).
fn owns_items(kind: FacetKind) -> bool {
    matches!(kind, FacetKind::Repeat | FacetKind::When)
}

/// Whether `id` is a repeat or a conditional.
pub fn is_item_owner(doc: &Document, id: NodeId) -> bool {
    doc.facet(id, FacetKind::Repeat).is_some() || doc.facet(id, FacetKind::When).is_some()
}

/// Every expanded node and relation a document should have, with
/// `counts[r]` the items of each repeat and conditional `r`.
fn wanted(doc: &Document, counts: &Items) -> Wanted {
    let mut out = Wanted::default();
    let entity_ty = TypeId::lookup("entity").expect("registered");
    for entity in doc.nodes_of_type(entity_ty).filter(|&e| doc.is_live(e) && !e.is_expanded()).collect::<Vec<_>>() {
        if let Some(def) = linked_definition(doc, entity) {
            want_entity(doc, entity, def, &mut out);
        }
    }
    let use_ty = TypeId::lookup("use").expect("registered");
    for u in doc.nodes_of_type(use_ty).filter(|&u| doc.is_live(u) && !u.is_expanded()).collect::<Vec<_>>() {
        if let Some(def) = main_definition(doc, u) {
            if let Some(notation) = doc.get::<Use>(u).and_then(|x| x.notation.constant()).and_then(|v| v.as_str()).map(str::to_owned) {
                want_use(doc, u, doc.facet(u, FacetKind::Size).is_some(), None, def, &notation, &mut out);
            }
            continue;
        }
        let Some(entity) = use_entity(doc, u) else { continue };
        let Some(def) = linked_definition(doc, entity) else { continue };
        let Some(notation) = doc.get::<Use>(u).and_then(|x| x.notation.constant()).and_then(|v| v.as_str()).map(str::to_owned) else { continue };
        want_use(doc, u, doc.facet(u, FacetKind::Size).is_some(), Some(entity), def, &notation, &mut out);
    }
    want_scopes(doc, counts, &mut out);
    out
}

/// Whether `id` is part of a repeat's template (plans/specs/repeat.md):
/// inside a group with a `repeat` facet but in none of its items. A
/// template is never drawn; its copies are.
pub fn in_repeat_template(doc: &Document, id: NodeId) -> bool {
    let mut at = doc.node(id).and_then(|n| n.parent);
    while let Some(p) = at {
        let Some(node) = doc.node(p) else { return false };
        if is_item_owner(doc, p) && id.item_of(p).is_none() {
            return true;
        }
        at = node.parent;
    }
    false
}

/// One node of a repeat's template: id, parent, type, order, facets.
type TemplateNode = (NodeId, NodeId, TypeId, OrderKey, Vec<Facet>);

/// Whether `id` holds a repeat that draws: a group with a `repeat` facet,
/// outside the library (a definition's template copies it into each
/// picture, and the picture's copy is the one that repeats).
pub(crate) fn draws_repeat(doc: &Document, id: NodeId, facets: &[Facet]) -> bool {
    facets.iter().any(|f| owns_items(f.kind())) && !id.path().first().is_some_and(|&root| in_library(doc, root))
}

/// Every drawn repeat of a document: the authored ones and the derived
/// ones it has now.
pub(crate) fn drawn_repeats(doc: &Document) -> Vec<NodeId> {
    let group = TypeId::lookup("group").expect("registered");
    doc.nodes_of_type(group)
        .filter(|&g| doc.is_live(g) && is_item_owner(doc, g) && !g.path().first().is_some_and(|&root| in_library(doc, root)) && !in_repeat_template(doc, g))
        .collect()
}

/// Whether `id`, a node of the document or one wanted, is part of some
/// repeat's template: under a repeat, in none of its items. A template is
/// what items copy; it derives nothing itself, or every level of nesting
/// would double what is derived.
fn in_template(doc: &Document, out: &Wanted, id: NodeId) -> bool {
    let parent_of = |n: NodeId| out.get(n).map(|w| w.parent).or_else(|| doc.node(n).and_then(|x| x.parent));
    let is_repeat = |n: NodeId| match out.get(n) {
        Some(w) => w.facets.iter().any(|f| owns_items(f.kind())),
        None => is_item_owner(doc, n),
    };
    let mut at = parent_of(id);
    while let Some(p) = at {
        if is_repeat(p) && id.item_of(p).is_none() {
            return true;
        }
        at = parent_of(p);
    }
    false
}

/// Expand every scope nobody has expanded yet (reactivity.md §3.1), until
/// none is left: a repeat's items, and inside whatever those derive, the
/// ports of an instance and the picture of a `use`. So a component in a
/// repeat's template, a repeat in a component's, and repeats in repeats
/// all expand, each scope under the one that made it.
fn want_scopes(doc: &Document, counts: &Items, out: &mut Wanted) {
    let mut repeats: Vec<NodeId> = drawn_repeats(doc).into_iter().filter(|g| !g.is_expanded()).collect();
    let mut done = BTreeSet::new();
    let mut seen = 0;
    loop {
        while seen < out.nodes.len() {
            let w = &out.nodes[seen];
            seen += 1;
            let (id, scoped) = (w.id, w.scoped);
            if draws_repeat(doc, id, &w.facets) {
                repeats.push(id);
            }
            if scoped || in_template(doc, out, id) {
                continue;
            }
            match w.ty.name() {
                "entity" => {
                    if let Some(def) = linked_def_of(doc, &w.facets) {
                        want_entity(doc, id, def, out);
                    }
                }
                "use" => {
                    let sized = w.facets.iter().any(|f| matches!(f, Facet::Size(_)));
                    let notation = w.facets.iter().find_map(|f| match f {
                        Facet::Use(x) => x.notation.constant().and_then(|v| v.as_str()).map(str::to_owned),
                        _ => None,
                    });
                    // The entity it pictures: one this expansion derives
                    // (its own item's), or an authored one.
                    let entity = out.rels.iter().find(|r| r.rel.from == id && r.rel.rel == RelKind::Figures).map(|r| r.rel.to);
                    let def = entity.and_then(|e| match out.get(e) {
                        Some(copy) => linked_def_of(doc, &copy.facets),
                        None => linked_definition(doc, e),
                    });
                    if let (Some(entity), Some(def), Some(notation)) = (entity, def, notation) {
                        want_use(doc, id, sized, Some(entity), def, &notation, out);
                    }
                }
                _ => {}
            }
        }
        let Some(r) = repeats.pop() else { break };
        if done.insert(r) && !in_template(doc, out, r) {
            want_items(doc, r, counts.get(&r).map_or(&[][..], Vec::as_slice), out);
        }
    }
}

/// Repeat `r`'s template: its children that are not its items, and their
/// subtrees, parents first, with the relations from them. A derived
/// repeat's come from what is wanted, an authored one's from the document.
fn template_of_repeat(doc: &Document, r: NodeId, out: &Wanted) -> (Vec<TemplateNode>, Vec<Relation>) {
    let mut nodes = Vec::new();
    let mut rels = Vec::new();
    if r.is_expanded() {
        let mut parents = BTreeSet::from([r]);
        for w in &out.nodes {
            if parents.contains(&w.parent) && w.id.item_of(r).is_none() && w.ty.name() != "override" {
                parents.insert(w.id);
                nodes.push((w.id, w.parent, w.ty, w.order.clone(), w.facets.clone()));
            }
        }
        rels.extend(out.rels.iter().filter(|x| parents.contains(&x.rel.from) && x.rel.from != r).map(|x| x.rel.clone()));
    } else {
        // An override is the repeat's own opinion about one item's copy,
        // not part of what items copy.
        let mut stack: Vec<NodeId> = doc.children(r).iter().rev().copied().filter(|c| !c.is_expanded()).collect();
        while let Some(n) = stack.pop() {
            let Some(node) = doc.live(n).filter(|x| x.ty.name() != "override") else { continue };
            nodes.push((n, node.parent.unwrap_or(r), node.ty, node.order.clone(), doc.facets_of(n).cloned().collect()));
            rels.extend(doc.relations_from(n).filter(|x| !x.id.is_expanded()).cloned());
            stack.extend(doc.children(n).iter().rev().copied().filter(|c| !c.is_expanded()));
        }
    }
    (nodes, rels)
}

/// The items of repeat (or conditional) `r`, by key. Each is a scope: a
/// derived group `#r/~key` with an `item` facet, holding a copy of the
/// template, with the repeat's overrides of that item applied. In a
/// copy, references to the template follow to the item's own copies, and
/// `#r.repeat.item` / `.index` read the item's `item.value` / `.index`,
/// which the store publishes (`Store::settle_repeats`): so an item
/// re-evaluates when its own element changes, not when any does.
fn want_items(doc: &Document, r: NodeId, keys: &[String], out: &mut Wanted) {
    if keys.is_empty() || !out.enter(r) {
        return;
    }
    let (template, rels) = template_of_repeat(doc, r, out);
    let path = |f: &str| LeafPath::new(FacetKind::Repeat, f).expect("declared leaf");
    let item_path = |f: &str| LeafPath::new(FacetKind::Item, f).expect("declared leaf");
    let any = LeafPath::new(FacetKind::Name, "text").expect("declared");
    let inside: BTreeSet<NodeId> = template.iter().map(|t| t.0).collect();
    let group = TypeId::lookup("group").expect("registered");
    let mut key = template.iter().filter(|t| t.1 == r).map(|t| t.3.clone()).max().unwrap_or_else(OrderKey::first);
    for k in keys {
        let k = k.as_str();
        let Some(scope) = NodeId::item_scope(r, k) else { continue };
        key = OrderKey::after(&key);
        if !out.push(Want { id: scope, ty: group, parent: r, order: key.clone(), facets: vec![FacetKind::Item.default_facet()], scoped: true }) {
            return;
        }
        let follow = |leaf: Leaf| -> Leaf {
            if leaf.node == r && leaf.path == path("item") {
                return Leaf { node: scope, path: item_path("value") };
            }
            if leaf.node == r && leaf.path == path("index") {
                return Leaf { node: scope, path: item_path("index") };
            }
            match inside.contains(&leaf.node).then(|| NodeId::item(r, k, leaf.node)).flatten() {
                Some(node) => Leaf { node, path: leaf.path },
                None => leaf,
            }
        };
        for (id, parent, ty, order, facets) in &template {
            let (Some(copy), Some(parent_copy)) = (NodeId::item(r, k, *id), if *parent == r { Some(scope) } else { NodeId::item(r, k, *parent) }) else { continue };
            let mut facets: Vec<Facet> = facets
                .iter()
                .map(|f| {
                    let mut f = f.clone();
                    for field in f.field_names() {
                        match f.get(field) {
                            Some(Slot::Bound(e)) => {
                                let e = e.map_refs(&follow);
                                f.set(field, Slot::Bound(e)).expect("field exists");
                            }
                            Some(Slot::Const(Value::Ref(to))) => {
                                let to = follow(Leaf { node: *to, path: any }).node;
                                f.set(field, Slot::Const(Value::Ref(to))).expect("field exists");
                            }
                            _ => {}
                        }
                    }
                    f
                })
                .collect();
            if let Some(&local) = id.path().last() {
                apply_overrides(doc, scope, local, *ty, &mut facets);
            }
            if !out.push(Want { id: copy, ty: *ty, parent: parent_copy, order: order.clone(), facets, scoped: false }) {
                return;
            }
        }
        // The template's relations, between the item's copies: a picture
        // in the template figures its item's entity.
        for rel in &rels {
            let Some(from) = NodeId::item(r, k, rel.from) else { continue };
            let to = follow(Leaf { node: rel.to, path: any }).node;
            out.rels.push(RelWant { rel: Relation { id: RelationId::expanded(scope, rel.id), from, to, rel: rel.rel, data: rel.data.clone() } });
        }
    }
}

/// What entity `entity` (authored, or derived for an outer instance)
/// derives from `def`: its ports and equations, and its inner entities and
/// connections, each inner entity in turn deriving its own.
fn want_entity(doc: &Document, entity: NodeId, def: NodeId, out: &mut Wanted) {
    if !out.enter(entity) {
        return;
    }
    let inputs = definition::inputs_of(doc, def).unwrap_or_default();
    let follow = |leaf: Leaf| -> Leaf {
        match leaf.node.path().first().copied().filter(|&root| is_body(doc, def, root)).and_then(|_| relocate(entity, leaf.node)) {
            Some(n) => Leaf { node: n, path: leaf.path },
            None => leaf,
        }
    };
    for &child in doc.children(def) {
        let Some(node) = doc.live(child) else { continue };
        let ty = match node.ty.name() {
            "port_spec" => TypeId::lookup("port").expect("registered"),
            "equation" | "entity" | "connection" | "channel" | "machine" | "let" | "trace" | "accumulate" => node.ty,
            _ => continue,
        };
        let Some(id) = NodeId::expanded(entity, child) else { continue };
        let mut facets = rewritten(doc, child, &follow);
        if let Some(input) = inputs.iter().find(|i| i.spec == child) {
            let arg = Leaf { node: entity, path: LeafPath::new(FacetKind::Args, input.name).expect("argument path") };
            for f in &mut facets {
                if let Facet::Port(p) = f {
                    p.signal = Slot::Bound(Expr::Ref(arg));
                }
            }
        }
        apply_overrides(doc, entity, child, ty, &mut facets);
        let inner_def = (node.ty.name() == "entity").then(|| linked_def_of(doc, &facets)).flatten();
        if !out.push(Want { id, ty, parent: entity, order: node.order.clone(), facets, scoped: true }) {
            return;
        }
        // A connection's ends, a port's `exposes`: re-pointed for this entity.
        if matches!(node.ty.name(), "connection" | "port_spec") {
            for r in doc.relations_from(child) {
                let to = follow(Leaf { node: r.to, path: LeafPath::new(FacetKind::Name, "text").expect("declared") }).node;
                out.rels.push(RelWant { rel: Relation { id: RelationId::expanded(entity, r.id), from: id, to, rel: r.rel, data: r.data.clone() } });
            }
        }
        if let Some(inner) = inner_def {
            want_entity(doc, id, inner, out);
        }
    }
    // A machine's states, transitions and tweens, copied for this instance
    // (machines.md §9 Q12): their timing may read the instance's inputs,
    // and the instance may override any of it. After the ports they read.
    for &machine in doc.children(def).iter().filter(|&&c| doc.live(c).is_some_and(|n| n.ty.name() == "machine")) {
        let mut stack: Vec<NodeId> = doc.children(machine).iter().rev().copied().filter(|c| !c.is_expanded()).collect();
        while let Some(local) = stack.pop() {
            let Some(n) = doc.live(local) else { continue };
            let (Some(copy), Some(parent)) = (NodeId::expanded(entity, local), n.parent.and_then(|p| NodeId::expanded(entity, p))) else { continue };
            let mut facets = rewritten(doc, local, &follow);
            apply_overrides(doc, entity, local, n.ty, &mut facets);
            if !out.push(Want { id: copy, ty: n.ty, parent, order: n.order.clone(), facets, scoped: true }) {
                return;
            }
            stack.extend(doc.children(local).iter().rev().copied().filter(|c| !c.is_expanded()));
        }
    }
}

/// What `use` `u`, figuring `entity` (an instance of `def`), derives: the
/// definition's template for `notation`, as its children. A `use` inside
/// the template pictures one of the definition's inner entities, so its
/// copy figures that entity's copy and derives that entity's picture. A
/// main has no entity: references to the definition's body stay on it.
fn want_use(doc: &Document, u: NodeId, sized: bool, entity: Option<NodeId>, def: NodeId, notation: &str, out: &mut Wanted) {
    if !out.enter(u) {
        return;
    }
    let Some(template) = definition::template_of(doc, def, notation) else { return };
    let inside: BTreeSet<NodeId> = doc.descendants(template).into_iter().filter(|d| !d.is_expanded()).collect();
    // A sized picture (plans/specs/plot.md §3): the template's own `size`,
    // which its parts read, is this use's (`sized`: the use has a size;
    // an inner use's copy may not exist yet, so the caller says).
    let follow = |leaf: Leaf| -> Leaf {
        if leaf.node == template {
            return if sized && leaf.path.facet == FacetKind::Size { Leaf { node: u, path: leaf.path } } else { leaf };
        }
        let root = leaf.node.path()[0];
        let owner = if inside.contains(&root) {
            u
        } else if is_body(doc, def, root) {
            match entity {
                Some(e) => e,
                None => return leaf,
            }
        } else {
            return leaf;
        };
        match relocate(owner, leaf.node) {
            Some(n) => Leaf { node: n, path: leaf.path },
            None => leaf,
        }
    };
    // Preorder, so every parent is wanted before its children. A template's
    // own derived children (its uses' previews) and overrides are not copied.
    let copied = |n: NodeId| !n.is_expanded() && doc.live(n).is_some_and(|x| x.ty.name() != "override");
    let mut stack: Vec<NodeId> = doc.children(template).iter().rev().copied().filter(|&c| copied(c)).collect();
    while let Some(local) = stack.pop() {
        let Some(node) = doc.live(local) else { continue };
        let parent = match node.parent {
            Some(p) if p == template => Some(u),
            Some(p) => NodeId::expanded(u, p),
            None => None,
        };
        let (Some(id), Some(parent)) = (NodeId::expanded(u, local), parent) else { continue };
        let mut facets = rewritten(doc, local, &follow);
        apply_overrides(doc, u, local, node.ty, &mut facets);
        // An entity in a template (a repeat's item instance) and a picture
        // of one are scopes the scope pass expands; a picture of one of
        // the definition's own inner entities is expanded here.
        let inner = (node.ty.name() == "use").then(|| doc.relations_from(local).find(|r| r.rel == RelKind::Figures)).flatten();
        let own = inner.is_some_and(|r| !inside.contains(&r.to.path()[0]));
        if !out.push(Want { id, ty: node.ty, parent, order: node.order.clone(), facets: facets.clone(), scoped: !matches!(node.ty.name(), "entity" | "use") || own }) {
            return;
        }
        if node.ty.name() == "use" {
            // An inner picture: it figures the inner entity's copy, and
            // derives that entity's picture in turn.
            if let Some(r) = doc.relations_from(local).find(|r| r.rel == RelKind::Figures) {
                let inner_entity = follow(Leaf { node: r.to, path: LeafPath::new(FacetKind::Name, "text").expect("declared") }).node;
                out.rels.push(RelWant { rel: Relation { id: RelationId::expanded(u, r.id), from: id, to: inner_entity, rel: RelKind::Figures, data: r.data.clone() } });
                let inner_notation = facets.iter().find_map(|f| match f {
                    Facet::Use(x) => x.notation.constant().and_then(|v| v.as_str()).map(str::to_owned),
                    _ => None,
                });
                let inner_def = doc.node(r.to).and_then(|_| doc.get::<Instance>(r.to)).and_then(|i| {
                    (i.mode.constant().and_then(|v| v.as_str()) == Some("linked")).then(|| i.def.constant().and_then(|v| v.as_ref())).flatten()
                });
                if let (Some(n), Some(d)) = (inner_notation, inner_def)
                    && own
                {
                    want_use(doc, id, facets.iter().any(|f| matches!(f, Facet::Size(_))), Some(inner_entity), d, &n, out);
                }
            }
            continue;
        }
        stack.extend(doc.children(local).iter().rev().copied().filter(|&c| copied(c)));
    }
}

/// The overrides that reach `owner`'s copies, as (node, target, leaf, value)
/// with `target` the definition-local node copied, weakest first
/// (§5.3, "Nested targets"). `owner`'s path runs from an authored node
/// through the definition-local nodes each enclosing instance copies; each
/// step's own overrides apply when their target path is the rest of the
/// way down. So the innermost definition's opinion comes first and the
/// outermost instance's last, and the last applied wins.
fn overrides(doc: &Document, owner: NodeId) -> Vec<(NodeId, NodeId, LeafPath, Value)> {
    let path = owner.path();
    let mut out = Vec::new();
    for j in (0..path.len()).rev() {
        let rest = &path[j + 1..];
        for (o, target, leaf, value) in own_overrides(doc, path[j]) {
            let steps = target.path();
            if steps.len() == rest.len() + 1 && steps[..rest.len()] == *rest {
                out.push((o, steps[rest.len()], leaf, value));
            }
        }
    }
    out
}

/// The overrides reaching `owner`'s copies, as (definition-local target,
/// leaf, value), weakest first: for render analysis (`analysis.rs`).
pub fn overrides_of(doc: &Document, owner: NodeId) -> Vec<(NodeId, LeafPath, Value)> {
    overrides(doc, owner).into_iter().map(|(_, t, l, v)| (t, l, v)).collect()
}

fn own_overrides(doc: &Document, owner: NodeId) -> Vec<(NodeId, NodeId, LeafPath, Value)> {
    doc.children(owner)
        .iter()
        .copied()
        .filter(|&c| doc.live(c).is_some_and(|n| n.ty.name() == "override"))
        .filter_map(|c| {
            let o = doc.get::<Override>(c)?;
            let target = o.target.constant()?.as_ref()?;
            let leaf: LeafPath = o.leaf.constant()?.as_str()?.parse().ok()?;
            Some((c, target, leaf, o.value.constant()?.clone()))
        })
        .collect()
}

/// Apply `owner`'s overrides of `local` to the facets its copy will have
/// (component-model.md §5.3): each sets one leaf to a constant, adding the
/// facet when the node type admits it and the copy lacks it.
fn apply_overrides(doc: &Document, owner: NodeId, local: NodeId, ty: TypeId, facets: &mut Vec<Facet>) {
    for (_, target, leaf, value) in overrides(doc, owner) {
        if target != local {
            continue;
        }
        if !facets.iter().any(|f| f.kind() == leaf.facet) {
            if !ty.spec().allows(leaf.facet) {
                continue;
            }
            facets.push(leaf.facet.default_facet());
        }
        if let Some(f) = facets.iter_mut().find(|f| f.kind() == leaf.facet) {
            let _ = f.set(leaf.field, Slot::Const(value));
        }
    }
}

/// Overrides that apply nothing (component-model.md §5.3): their instance
/// is not linked, their target is gone from the definition, or their leaf
/// is not one the target can have. Kept, inert, and listed with why.
pub fn stale_overrides(doc: &Document) -> Vec<(NodeId, String)> {
    let ty = TypeId::lookup("override").expect("registered");
    let mut out = Vec::new();
    for o in doc.nodes_of_type(ty).filter(|&o| doc.is_live(o)) {
        let Some(owner) = doc.live(o).and_then(|n| n.parent) else { continue };
        // A repeat's (or conditional's) override of one item's copy: stale
        // while that item is not there.
        if is_item_owner(doc, owner) {
            let target = doc.get::<Override>(o).and_then(|f| f.target.constant().and_then(|v| v.as_ref()));
            let derived = target.and_then(|t| NodeId::from_path(&[owner.path(), t.path()].concat()));
            if !derived.is_some_and(|d| doc.is_live(d)) {
                out.push((o, format!("{owner} has no such item now")));
            }
            continue;
        }
        let entity = if doc.live(owner).is_some_and(|n| n.ty.name() == "use") { use_entity(doc, owner) } else { Some(owner) };
        let Some(def) = entity.and_then(|e| linked_definition(doc, e)) else {
            out.push((o, format!("{owner} is not a linked instance")));
            continue;
        };
        let Some(facet) = doc.get::<Override>(o) else { continue };
        let target = facet.target.constant().and_then(|v| v.as_ref());
        let leaf = facet.leaf.constant().and_then(|v| v.as_str()).and_then(|l| l.parse::<LeafPath>().ok());
        let (Some(target), Some(leaf)) = (target, leaf) else {
            out.push((o, "its target or leaf is not well formed".into()));
            continue;
        };
        let derived = NodeId::from_path(&[owner.path(), target.path()].concat()).filter(|&d| doc.is_live(d));
        match derived {
            None => out.push((o, format!("{def} has no {target} for {owner} to derive"))),
            Some(d) if doc.leaf(d, leaf).is_none() => out.push((o, format!("{target} has no {leaf}"))),
            Some(_) => {}
        }
    }
    out
}

/// `node`'s facets with every reference passed through `follow`: those in
/// bindings, and node references held as constants (an anchor's `port`).
fn rewritten(doc: &Document, node: NodeId, follow: &dyn Fn(Leaf) -> Leaf) -> Vec<Facet> {
    doc.facets_of(node)
        .map(|c| {
            let mut c = c.clone();
            for field in c.field_names() {
                match c.get(field) {
                    Some(Slot::Bound(expr)) => {
                        let e = expr.map_refs(follow);
                        c.set(field, Slot::Bound(e)).expect("field exists");
                    }
                    Some(Slot::Const(Value::Ref(r))) => {
                        let any = LeafPath::new(FacetKind::Name, "text").expect("declared");
                        let to = follow(Leaf { node: *r, path: any }).node;
                        c.set(field, Slot::Const(Value::Ref(to))).expect("field exists");
                    }
                    _ => {}
                }
            }
            c
        })
        .collect()
}

/// Bring the document's expanded nodes to what its linked instances derive.
/// Returns the ops applied (for consumers that follow ops) and the problems
/// met (a template node the registry no longer admits, say), which leave
/// that node out rather than failing the transaction.
pub fn expand(doc: &mut Document, stamp: Version, counts: &Items) -> (Vec<Op>, Vec<Problem>) {
    let Wanted { nodes: want, rels: want_rels, mut problems, .. } = wanted(doc, counts);
    let wanted_ids: BTreeSet<NodeId> = want.iter().map(|w| w.id).collect();
    let mut ops = Vec::new();
    let mut run = |doc: &mut Document, op: Op, ops: &mut Vec<Op>| match op::apply_op(doc, 0, &op, stamp) {
        Ok(_) => ops.push(op),
        Err(e) => {
            if let Some(node) = op_node(doc, &op) {
                problems.push(Problem { node, kind: "expansion", message: format!("expanding {}: {}", op_target(&op), e.message) });
            }
        }
    };
    // Gone: deepest first, so a parent's delete never meets a dead child.
    let stale: Vec<NodeId> = doc.expanded_ids().filter(|id| doc.is_live(*id) && !wanted_ids.contains(id)).collect();
    let mut stale: Vec<(usize, NodeId)> = stale.into_iter().map(|id| (doc.depth(id), id)).collect();
    stale.sort_by(|a, b| b.cmp(a));
    for (_, id) in stale {
        if doc.is_live(id) {
            run(doc, Op::Delete { id }, &mut ops);
        }
    }
    for w in want {
        let Some(node) = doc.live(w.id) else {
            run(doc, Op::Create { id: w.id, ty: w.ty, parent: Some(w.parent), order: w.order, facets: w.facets }, &mut ops);
            continue;
        };
        if node.ty != w.ty {
            run(doc, Op::Delete { id: w.id }, &mut ops);
            run(doc, Op::Create { id: w.id, ty: w.ty, parent: Some(w.parent), order: w.order, facets: w.facets }, &mut ops);
            continue;
        }
        if node.parent != Some(w.parent) || node.order != w.order {
            run(doc, Op::Reparent { id: w.id, parent: Some(w.parent), order: w.order }, &mut ops);
        }
        let have: BTreeMap<FacetKind, Facet> = doc.facets_of(w.id).map(|f| (f.kind(), f.clone())).collect();
        let mut kept = BTreeSet::new();
        for f in w.facets {
            kept.insert(f.kind());
            match have.get(&f.kind()) {
                None => run(doc, Op::AddFacet { id: w.id, facet: f }, &mut ops),
                Some(old) if *old != f => {
                    if f.kind().is_dynamic() {
                        run(doc, Op::RemoveFacet { id: w.id, kind: f.kind() }, &mut ops);
                        run(doc, Op::AddFacet { id: w.id, facet: f }, &mut ops);
                    } else {
                        for field in f.field_names() {
                            let slot = f.get(field).expect("own field").clone();
                            if old.get(field) != Some(&slot) {
                                let path = LeafPath::new(f.kind(), field).expect("declared field");
                                run(doc, Op::Set { id: w.id, path, slot }, &mut ops);
                            }
                        }
                    }
                }
                Some(_) => {}
            }
        }
        for kind in have.keys().filter(|k| !kept.contains(k)) {
            run(doc, Op::RemoveFacet { id: w.id, kind: *kind }, &mut ops);
        }
    }
    // Relations: stale ones unlinked, changed ones relinked, new ones linked.
    let wanted_rels: BTreeMap<RelationId, Relation> = want_rels.into_iter().map(|w| (w.rel.id, w.rel)).collect();
    let have_rels: Vec<Relation> = doc.relations().filter(|r| r.id.is_expanded()).cloned().collect();
    for r in &have_rels {
        if wanted_rels.get(&r.id) != Some(r) {
            run(doc, Op::Unlink { id: r.id }, &mut ops);
        }
    }
    for (id, rel) in wanted_rels {
        if doc.relation(id) != Some(&rel) && doc.is_live(rel.from) {
            run(doc, Op::Link { rel }, &mut ops);
        }
    }
    (ops, problems)
}

/// The node a failed op's problem is shown on.
fn op_node(doc: &Document, op: &Op) -> Option<NodeId> {
    match op {
        Op::Create { id, parent, .. } => Some(parent.unwrap_or(*id)),
        Op::Delete { id } | Op::Reparent { id, .. } | Op::Reorder { id, .. } | Op::Set { id, .. } | Op::AddFacet { id, .. } | Op::RemoveFacet { id, .. } => Some(*id),
        Op::Link { rel } => Some(rel.from),
        Op::Unlink { id } => doc.relation(*id).map(|r| r.from),
    }
}

fn op_target(op: &Op) -> String {
    match op {
        Op::Create { id, .. } | Op::Delete { id } | Op::Reparent { id, .. } | Op::Reorder { id, .. } | Op::Set { id, .. } | Op::AddFacet { id, .. } | Op::RemoveFacet { id, .. } => id.to_string(),
        Op::Link { rel } => rel.id.to_string(),
        Op::Unlink { id } => id.to_string(),
    }
}

/// Whether a transaction's ops can change what instances derive: anything
/// structural, or a set on an instance, a `use`, or a node of a definition.
pub fn affected_by(doc: &Document, ops: &[Op]) -> bool {
    ops.iter().any(|op| match op {
        Op::Set { id, path, .. } => matches!(path.facet, FacetKind::Instance | FacetKind::Use | FacetKind::Override) || in_library(doc, *id),
        _ => true,
    })
}

fn in_library(doc: &Document, mut id: NodeId) -> bool {
    while let Some(n) = doc.node(id) {
        if n.ty.name() == "library" {
            return true;
        }
        match n.parent {
            Some(p) => id = p,
            None => return false,
        }
    }
    false
}

/// The authored op an edit to an expanded node would need; until edits are
/// translated (component-model.md §7) such ops are refused with this reason.
pub fn refuse(op: &Op) -> Option<String> {
    let target = match op {
        Op::Create { id, parent, .. } => [Some(*id), *parent].into_iter().flatten().find(|n| n.is_expanded()),
        Op::Delete { id } | Op::Reorder { id, .. } | Op::Set { id, .. } | Op::AddFacet { id, .. } | Op::RemoveFacet { id, .. } => Some(*id).filter(|n| n.is_expanded()),
        Op::Reparent { id, parent, .. } => [Some(*id), *parent].into_iter().flatten().find(|n| n.is_expanded()),
        Op::Link { rel } => Some(rel.from).filter(|n| n.is_expanded()),
        Op::Unlink { .. } => None,
    }?;
    let (owner, _) = target.expanded_parts()?;
    Some(format!("{target} is part of instance {owner}, derived from its definition; edit the definition or detach the instance"))
}

/// Translate edits aimed at expanded nodes into authored ones
/// (component-model.md §7), before a transaction applies:
///
/// - a constant set on a leaf bound to something it can be inverted onto
///   (an `in` port's argument, a slider) sets that source instead;
/// - a constant set on a leaf with a value of its own becomes an override
///   on the instance, created or updated;
/// - a binding set on one is refused: a binding from outside may not reach
///   inside an instance; the definition needs an `in` port for it.
///
/// Every other op passes through; `refuse` turns away structural edits.
pub fn translate(doc: &Document, reactive: &crate::graph::Reactive, ops: Vec<Op>) -> Result<Vec<Op>, crate::op::ApplyError> {
    use crate::op::{ApplyError, ErrorKind};
    // A repeat's items are copies of its template. A value set on a copy
    // is the repeat's override of that item, as for an instance's part
    // (below); the item's own node and the copies' structure are not
    // edited: that is refused, naming what to edit.
    for (i, op) in ops.iter().enumerate() {
        let target = match op {
            Op::Create { id, parent, .. } | Op::Reparent { id, parent, .. } => [Some(*id), *parent].into_iter().flatten().find(|n| n.repeat_item().is_some()),
            Op::Delete { id } | Op::Reorder { id, .. } | Op::RemoveFacet { id, .. } => Some(*id),
            Op::Set { id, .. } | Op::AddFacet { id, .. } => Some(*id).filter(|n| n.is_item_scope()),
            Op::Link { rel } => Some(rel.from),
            Op::Unlink { .. } => None,
        };
        if let Some((target, (repeat, k, template))) = target.and_then(|t| t.repeat_item().map(|r| (t, r))) {
            let what = if template == repeat { format!("set {repeat}'s repeat.each, repeat.count or when.is") } else { format!("edit its template, {template}; only a copy's values can differ per item") };
            return Err(ApplyError { op: i, kind: ErrorKind::BadId, message: format!("{target} is item {k} of {repeat}, a copy: {what}") });
        }
    }
    // Fresh ids for created overrides go past anything the transaction itself
    // reserved.
    let mut fresh = doc.next_id().raw().max(
        ops.iter()
            .filter_map(|op| match op {
                Op::Create { id, .. } if !id.is_expanded() && !id.is_session() => Some(id.raw() + 1),
                _ => None,
            })
            .max()
            .unwrap_or(0),
    );
    let mut out: Vec<Op> = Vec::with_capacity(ops.len());
    // An override of `path` on `owner`'s copy of `local`: the one already
    // there (or created earlier in this transaction) updated, else a new one.
    let mut made: Vec<(NodeId, NodeId, LeafPath, NodeId)> = Vec::new();
    let mut override_op = |owner: NodeId, local: NodeId, path: LeafPath, value: Value, out: &mut Vec<Op>| -> Result<(), String> {
        // A part of a nested instance is overridden from the outermost one,
        // its target the path down to the part (§5.3, "Nested targets").
        // Inside a main, the outermost is the definition's own node: the
        // override is the definition's (component-authoring.md §1).
        let mut steps = owner.path();
        if main_definition(doc, steps[0]).is_some() {
            steps.remove(0);
        }
        if steps.is_empty() {
            return Err(format!("{owner}: nothing to override"));
        }
        let owner = steps.remove(0);
        steps.push(local);
        let local = NodeId::from_path(&steps).ok_or_else(|| format!("{owner}: no path to {local}"))?;
        let existing = own_overrides(doc, owner).into_iter().find(|(_, t, l, _)| *t == local && *l == path).map(|(o, ..)| o).or_else(|| made.iter().find(|m| m.0 == owner && m.1 == local && m.2 == path).map(|m| m.3));
        match existing {
            Some(o) => out.push(Op::Set { id: o, path: "override.value".parse().expect("declared leaf"), slot: Slot::Const(value) }),
            None => {
                let o = NodeId::from_raw(fresh);
                fresh += 1;
                let after = doc.children(owner).iter().rev().find(|c| !c.is_expanded()).and_then(|&c| doc.live(c)).map(|n| n.order.clone());
                let mut order = after.map_or_else(OrderKey::first, |k| OrderKey::after(&k));
                for _ in made.iter().filter(|m| m.0 == owner) {
                    order = OrderKey::after(&order);
                }
                let mut f = Override::default();
                f.target = Slot::Const(Value::Ref(local));
                f.leaf = Slot::Const(Value::Str(path.to_string()));
                f.value = Slot::Const(value);
                made.push((owner, local, path, o));
                out.push(Op::Create { id: o, ty: TypeId::lookup("override").expect("registered"), parent: Some(owner), order, facets: vec![f.wrap()] });
            }
        }
        Ok(())
    };
    // Inside a main, a derived node one step down stands for its template
    // node, and the main for the template itself.
    let in_main = |id: NodeId| -> Option<NodeId> {
        let path = id.path();
        (path.len() == 2 && main_definition(doc, path[0]).is_some()).then(|| path[1])
    };
    let container = |p: NodeId| -> NodeId {
        if main_definition(doc, p).is_some() {
            return main_template(doc, p).unwrap_or(p);
        }
        in_main(p).unwrap_or(p)
    };
    for (i, op) in ops.into_iter().enumerate() {
        // Edits inside a main are edits to its definition.
        let op = match op {
            Op::Create { id, ty, parent, order, facets } => Op::Create { id, ty, parent: parent.map(container), order, facets },
            Op::Reparent { id, parent, order } => Op::Reparent { id: in_main(id).unwrap_or(id), parent: parent.map(container), order },
            Op::Delete { id } if in_main(id).is_some() => Op::Delete { id: in_main(id).unwrap_or(id) },
            Op::Reorder { id, order } if in_main(id).is_some() => Op::Reorder { id: in_main(id).unwrap_or(id), order },
            Op::AddFacet { id, facet } if in_main(id).is_some() => Op::AddFacet { id: in_main(id).unwrap_or(id), facet },
            Op::RemoveFacet { id, kind } if in_main(id).is_some() => Op::RemoveFacet { id: in_main(id).unwrap_or(id), kind },
            other => other,
        };
        // A facet added to what an instance derives (an ellipse gaining an
        // arc) is an override of each of its fields.
        if let Op::AddFacet { id, facet } = &op
            && let Some((owner, local)) = id.expanded_parts()
        {
            for field in facet.field_names() {
                if let Some(v) = facet.get(field).and_then(|s| s.constant()) {
                    override_op(owner, local, LeafPath::new(facet.kind(), field).expect("own field"), v.clone(), &mut out).map_err(|m| ApplyError { op: i, kind: ErrorKind::BadId, message: m })?;
                }
            }
            continue;
        }
        let Op::Set { id, path, slot } = &op else {
            out.push(op);
            continue;
        };
        let Some((owner, local)) = id.expanded_parts() else {
            out.push(op);
            continue;
        };
        let err = |m: String| ApplyError { op: i, kind: ErrorKind::BadId, message: m };
        // A binding set inside a main is the definition's own (a link from
        // its input to a part, component-authoring.md §3).
        if let Some(part) = in_main(*id)
            && slot.expr().is_some()
        {
            out.push(Op::Set { id: part, path: *path, slot: slot.clone() });
            continue;
        }
        let value = match slot {
            Slot::Const(v) => v.clone(),
            _ => return Err(err(format!("{id} is inside instance {owner}: a binding from outside cannot reach it; give its definition an `in` port and bind that"))),
        };
        let leaf = Leaf { node: *id, path: *path };
        match crate::handle::handle(doc, reactive, leaf) {
            crate::handle::Handle::Locked(reason) => return Err(err(reason)),
            crate::handle::Handle::Linear { .. } => {
                let (source, v) = crate::handle::resolve_target(doc, reactive, leaf, value).map_err(err)?;
                if source.node.is_expanded() {
                    return Err(err(format!("{leaf} resolves onto {source}, inside an instance")));
                }
                out.push(Op::Set { id: source.node, path: source.path, slot: Slot::Const(v) });
            }
            crate::handle::Handle::Direct => match in_main(*id) {
                Some(part) => out.push(Op::Set { id: part, path: *path, slot: Slot::Const(value) }),
                None => override_op(owner, local, *path, value, &mut out).map_err(err)?,
            },
        }
    }
    Ok(out)
}

/// The scope rules (component-model.md §8), for a binding written by `op`:
/// inside a definition, a binding reads only that definition's own nodes;
/// outside, a binding reads an instance's insides only through its ports.
pub fn check_scope(doc: &Document, op: &Op) -> Result<(), String> {
    let (node, exprs): (NodeId, Vec<&Expr>) = match op {
        Op::Create { id, facets, .. } => (*id, facets.iter().flat_map(|f| f.field_names().into_iter().filter_map(move |n| f.get(n).and_then(|s| s.expr()))).collect()),
        Op::Set { id, slot, .. } => (*id, slot.expr().into_iter().collect()),
        Op::AddFacet { id, facet } => (*id, facet.field_names().into_iter().filter_map(|n| facet.get(n).and_then(|s| s.expr())).collect()),
        _ => return Ok(()),
    };
    if node.is_expanded() || exprs.is_empty() {
        return Ok(());
    }
    let home = definition_of(doc, node);
    for dep in exprs.iter().flat_map(|e| e.deps()) {
        match home {
            Some(def) => {
                if dep.node != def && !doc.is_ancestor(def, dep.node) {
                    return Err(format!("{node} is part of definition {def}: its bindings read only that definition's own nodes and ports, not {}", dep.node));
                }
            }
            None => {
                // A port, and a port's place on the picture (an anchor's
                // derived position: wires bind to it, schematic.md §1), are
                // the instance's interface; the rest is its inside.
                let place = doc.facet(dep.node, crate::facet::FacetKind::Anchor).is_some()
                    && dep.path.facet == crate::facet::FacetKind::Anchor
                    && matches!(dep.path.field, "wx" | "wy" | "wrot");
                if let Some((owner, _)) = dep.node.expanded_parts()
                    && !place
                    && !doc.live(dep.node).is_some_and(|n| n.ty.name() == "port")
                {
                    return Err(format!("{node} reads {dep}, inside instance {owner}; read one of its ports, or give its definition an `out` port for this"));
                }
            }
        }
    }
    Ok(())
}

/// The definition a node belongs to, if it is part of one.
fn definition_of(doc: &Document, mut id: NodeId) -> Option<NodeId> {
    while let Some(n) = doc.node(id) {
        if n.ty.name() == "definition" {
            return Some(id);
        }
        id = n.parent?;
    }
    None
}
