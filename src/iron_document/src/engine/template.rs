//! Templates (reactive-engine.md §5.4; handoff §4.6, stage 2): what an
//! owner copies, compiled once per expansion pass and shared by every
//! owner made from it, and [`Template::materialise`], one owner's copy of
//! one template node.
//!
//! Three kinds, by what is copied:
//! - a **body**, per definition: its port_specs (as ports), equations,
//!   inner entities, connections, channels, machines and the rest
//!   (`expand::is_body`), then each machine's states, transitions and tweens;
//! - a **picture**, per (definition, notation): the template's subtree,
//!   down to and including each `use` (an inner picture is its own owner);
//! - a **collection**, per repeat or conditional: its children that are
//!   not items, with their subtrees. An authored repeat's is compiled from
//!   the document; a derived one's (a repeat in a picture, or in another
//!   repeat's items) is a sub-template of the template that copies it
//!   ([`Template::sub`]), shared by every copy, never read from copies.
//!
//! Every node a template's facets or relations refer to is classified once
//! ([`Ref`]); an owner resolves each class against its own environment
//! ([`Env`]): its id, the entity a picture figures, whether a picture is
//! sized, an item's key, and the overrides that reach it. An item of a
//! derived repeat carries the environment of the owner that copies the
//! repeat ([`Item::outer`]): what its own template does not resolve, that
//! owner's does, and that owner's overrides apply before the item's. So a picture
//! template serves a sized and an unsized `use`, a main and a placed
//! instance alike: the spec's (definition, notation, sized, main) key is
//! (definition, notation) here, with sized and main in the environment.
//!
//! Templates are kept between passes ([`TemplateCache`], in `Owners`),
//! indexed by the structural [`Key`]s recorded while compiling each entry
//! (`Tree::recording`, EN-5a). Writes notify the cache via
//! [`TemplateCache::note_touched`], evicting only entries whose recorded
//! reads were touched; a rolled-back transaction clears the cache so
//! templates compiled from uncommitted state never linger.

use crate::definition;
use crate::document::{RelKind, Relation};
use crate::expand;
use crate::expr::{Expr, Leaf};
use crate::facet::{Facet, FacetKind, LeafPath};
use crate::id::NodeId;
use crate::order::OrderKey;
use crate::patch::{PatchKey, PatchValue};
use crate::registry::TypeId;
use crate::structure::{Key, Recorder};
use crate::value::{Slot, Value};
use crate::Tree;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

/// How a node a template refers to is read from one owner's copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ref {
    /// A node of this template, or one derived under one: the owner's own
    /// copy (`expand::relocate`), or for a collection the item's.
    Inside,
    /// A picture's read of its definition's body: the figured entity's
    /// copy, or the definition's own node in a main.
    Body,
    /// A picture's template root: in a sized `use`, its `size` is the
    /// `use`'s.
    Root,
}

/// What a template is compiled from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Source {
    /// A definition's body.
    Body(NodeId),
    /// A definition's template node for one notation.
    Picture(NodeId),
    /// A repeat or conditional, as the source has it: an authored one, or
    /// the definition-local node a derived one copies.
    Collection(NodeId),
}

/// One node of a template, as the source has it.
#[derive(PartialEq)]
pub(crate) struct TNode {
    /// The node copied: an owner's copy is `NodeId::expanded(owner, local)`,
    /// an item's `NodeId::item(repeat, key, local)`.
    pub(crate) local: NodeId,
    /// Its parent in the source; the source's root stands for the owner.
    pub(crate) parent: NodeId,
    /// The copy's type: a port_spec's copy is a port.
    pub(crate) ty: TypeId,
    pub(crate) order: OrderKey,
    pub(crate) facets: Vec<Facet>,
    /// Whether any facet holds a reference to resolve.
    pub(crate) rewrite: bool,
    /// An `in` port's argument: its signal reads the owner's.
    pub(crate) arg: Option<LeafPath>,
    /// What an override of this node's copy names as its target.
    pub(crate) target: NodeId,
    /// Relations from this node that each owner derives: a connection's
    /// ends, a port's `exposes`, a template's relations.
    pub(crate) rels: Vec<Relation>,
    /// A body: a child of the definition (a machine's parts are not).
    pub(crate) top: bool,
    /// A picture's `use`: the inner entity it figures, and whether this
    /// picture expands it (a picture of one of the definition's own inner
    /// entities). That entity's definition is read when the picture is
    /// expanded: the entity may be outside the template's scope.
    pub(crate) figures: Option<Relation>,
    pub(crate) own: bool,
}

/// A compiled template.
pub(crate) struct Template {
    pub(crate) source: Source,
    /// Parents first.
    pub(crate) nodes: Vec<TNode>,
    /// A collection's relations among its nodes: an item's are linked
    /// after all its nodes.
    pub(crate) rels: Vec<Relation>,
    refs: HashMap<NodeId, Ref>,
    /// Each node's `local`, for asking whether a node is the template's.
    locals: HashSet<NodeId>,
    /// A collection's items are ordered after this key.
    pub(crate) after: OrderKey,
    /// Cache revision assigned by [`TemplateCache`]; ties sub-templates
    /// to the exact compiled outer template instance (EN-5a).
    pub(crate) rev: u64,
}

/// One owner, as its template's references resolve against it. Two equal
/// environments resolve every reference of a template alike: what a copy
/// is depends on its template node and its environment alone
/// (`Template::materialise` reads nothing else).
#[derive(PartialEq)]
pub(crate) struct Env {
    /// The entity, `use` or item scope.
    pub(crate) owner: NodeId,
    /// The entity a picture figures; `None` in a main.
    pub(crate) entity: Option<NodeId>,
    pub(crate) sized: bool,
    /// A collection item.
    pub(crate) item: Option<Item>,
    /// Enclosing definition scopes with path start offsets `(scope, start)`
    /// (outermost first), for derived scope-chain layer precedence (`EN-5b`).
    pub(crate) scopes: Vec<(NodeId, usize)>,
    /// The patch overrides reaching the owner's copies: `(target, key, value)`,
    /// resolved from `PatchTable` at `(global, ViewCoord)` (`EN-5b`).
    pub(crate) overrides: Vec<(NodeId, PatchKey, PatchValue)>,
}

/// A collection item's part of its environment.
pub(crate) struct Item {
    /// The repeat whose item this is: a derived one is a copy of the
    /// template's [`Source::Collection`] node.
    pub(crate) repeat: NodeId,
    pub(crate) key: String,
    /// A derived repeat's: the template that copies it and the
    /// environment of that copy's owner.
    pub(crate) outer: Option<(Arc<Template>, Arc<Env>)>,
}

impl PartialEq for Item {
    fn eq(&self, other: &Item) -> bool {
        self.repeat == other.repeat
            && self.key == other.key
            && match (&self.outer, &other.outer) {
                (None, None) => true,
                (Some((t, e)), Some((u, f))) => Arc::ptr_eq(t, u) && e == f,
                _ => false,
            }
    }
}

fn any_leaf() -> LeafPath {
    LeafPath::new(FacetKind::Name, "text").expect("declared")
}

/// The nodes `facets` refer to, in bindings and as constants; whether
/// there are any.
fn refs_of(facets: &[Facet], out: &mut Vec<NodeId>) -> bool {
    let mut any = false;
    for f in facets {
        for field in f.field_names() {
            match f.get(field) {
                Some(Slot::Bound(e)) => {
                    any = true;
                    e.walk(&mut |x| match x {
                        Expr::Ref(l) => out.push(l.node),
                        Expr::Lit(Value::Ref(n)) => out.push(*n),
                        _ => {}
                    });
                }
                Some(Slot::Const(Value::Ref(n))) => {
                    any = true;
                    out.push(*n);
                }
                _ => {}
            }
        }
    }
    any
}

impl TNode {
    fn new(local: NodeId, parent: NodeId, ty: TypeId, order: OrderKey, facets: Vec<Facet>) -> TNode {
        TNode { local, parent, ty, order, facets, rewrite: false, arg: None, target: local, rels: Vec::new(), top: false, figures: None, own: false }
    }
}

impl Template {
    /// Classify every node the template's nodes and relations refer to.
    fn classify(source: Source, nodes: Vec<TNode>, rels: Vec<Relation>, after: OrderKey, class: impl Fn(NodeId) -> Option<Ref>) -> Template {
        let mut nodes = nodes;
        let mut seen: Vec<NodeId> = rels.iter().map(|r| r.to).collect();
        for n in &mut nodes {
            n.rewrite = refs_of(&n.facets, &mut seen);
            seen.extend(n.rels.iter().map(|r| r.to));
            seen.extend(n.figures.iter().map(|r| r.to));
        }
        let mut refs = HashMap::new();
        for id in seen {
            if let std::collections::hash_map::Entry::Vacant(e) = refs.entry(id)
                && let Some(c) = class(id)
            {
                e.insert(c);
            }
        }
        let locals = nodes.iter().map(|n| n.local).collect();
        Template { source, nodes, rels, refs, locals, after, rev: 0 }
    }

    /// Definition `def`'s body (`expand::want_entity`'s copies).
    /// Compiled strictly from unpatched base storage so templates are
    /// point-independent (`EN-5b`).
    pub(crate) fn body(doc: Tree<'_>, def: NodeId) -> Template {
        let inputs = definition::inputs_of(doc, def).unwrap_or_default();
        let port = TypeId::lookup("port").expect("registered");
        let mut nodes = Vec::new();
        for &child in doc.base_children(def).iter() {
            // A session node gets no copy (`NodeId::expanded`).
            let Some(node) = doc.base_live(child).filter(|_| !child.is_session()) else { continue };
            let ty = match node.ty.name() {
                "port_spec" => port,
                "equation" | "entity" | "connection" | "channel" | "machine" | "let" | "trace" | "accumulate" | "query" => node.ty,
                _ => continue,
            };
            let mut t = TNode::new(child, def, ty, node.order.clone(), doc.base_facets_of(child).cloned().collect());
            t.top = true;
            t.arg = inputs.iter().find(|i| i.spec == child).map(|i| LeafPath::new(FacetKind::Args, i.name).expect("argument path"));
            if matches!(node.ty.name(), "connection" | "port_spec") {
                t.rels = doc.relations_from(child).cloned().collect();
            }
            nodes.push(t);
        }
        // A machine's states, transitions and tweens (machines.md §9 Q12),
        // after the ports they read.
        // A session node gets no copy, so nothing under it does either.
        let copied = |n: NodeId| !n.is_expanded() && !n.is_session();
        for &machine in doc.base_children(def).iter().filter(|&&c| !c.is_session() && doc.base_live(c).is_some_and(|n| n.ty.name() == "machine")) {
            let mut stack: Vec<NodeId> = doc.base_children(machine).iter().rev().copied().filter(|&c| copied(c)).collect();
            while let Some(local) = stack.pop() {
                let Some(n) = doc.base_live(local) else { continue };
                let Some(parent) = n.parent else { continue };
                nodes.push(TNode::new(local, parent, n.ty, n.order.clone(), doc.base_facets_of(local).cloned().collect()));
                stack.extend(doc.base_children(local).iter().rev().copied().filter(|&c| copied(c)));
            }
        }
        Template::classify(Source::Body(def), nodes, Vec::new(), OrderKey::first(), |id| expand::is_body(doc, def, id.path()[0]).then_some(Ref::Inside))
    }

    /// Definition `def`'s picture from its template node `template`
    /// (`expand::want_use`'s copies): preorder, each `use` without its
    /// subtree. Compiled strictly from unpatched base storage so templates are
    /// point-independent (`EN-5b`).
    pub(crate) fn picture(doc: Tree<'_>, def: NodeId, template: NodeId) -> Template {
        let inside: BTreeSet<NodeId> = doc.base_descendants(template).into_iter().filter(|d| !d.is_expanded()).collect();
        let copied = |n: NodeId| !n.is_expanded() && !n.is_session() && doc.base_live(n).is_some();
        let mut nodes = Vec::new();
        let mut stack: Vec<NodeId> = doc.base_children(template).iter().rev().copied().filter(|&c| copied(c)).collect();
        while let Some(local) = stack.pop() {
            let Some(node) = doc.base_live(local) else { continue };
            let Some(parent) = node.parent else { continue };
            let mut t = TNode::new(local, parent, node.ty, node.order.clone(), doc.base_facets_of(local).cloned().collect());
            if node.ty.name() == "use" {
                if let Some(r) = doc.relations_from(local).find(|r| r.rel == RelKind::Figures) {
                    t.own = !inside.contains(&r.to.path()[0]);
                    t.figures = Some(r.clone());
                }
                nodes.push(t);
                continue;
            }
            nodes.push(t);
            stack.extend(doc.base_children(local).iter().rev().copied().filter(|&c| copied(c)));
        }
        Template::classify(Source::Picture(template), nodes, Vec::new(), OrderKey::first(), |id| {
            if id == template {
                return Some(Ref::Root);
            }
            let root = id.path()[0];
            if inside.contains(&root) {
                Some(Ref::Inside)
            } else if expand::is_body(doc, def, root) {
                Some(Ref::Body)
            } else {
                None
            }
        })
    }

    /// Authored repeat (or conditional) `r`'s template: its children that
    /// are not items, and their subtrees, parents first, with the
    /// relations from them. Compiled strictly from unpatched base storage (`EN-5b`).
    pub(crate) fn repeat(doc: Tree<'_>, r: NodeId) -> Template {
        let mut nodes = Vec::new();
        let mut rels = Vec::new();
        let copied = |c: &NodeId| !c.is_expanded() && !c.is_session();
        let mut stack: Vec<NodeId> = doc.base_children(r).iter().rev().copied().filter(copied).collect();
        while let Some(n) = stack.pop() {
            let Some(node) = doc.base_live(n) else { continue };
            nodes.push(TNode::new(n, node.parent.unwrap_or(r), node.ty, node.order.clone(), doc.base_facets_of(n).cloned().collect()));
            rels.extend(doc.relations_from(n).filter(|x| !x.id.is_expanded()).cloned());
            stack.extend(doc.base_children(n).iter().rev().copied().filter(copied));
        }
        Template::collection(r, nodes, rels)
    }

    /// Whether a copy of node `i` derives its own scope (an instance,
    /// picture, or repeat).
    #[allow(dead_code)]
    pub(crate) fn derives(&self, i: usize) -> bool {
        let n = &self.nodes[i];
        matches!(n.ty.name(), "entity" | "use") || n.facets.iter().any(|f| expand::owns_items(f.kind()))
    }

    /// The template of a copy of `outer`'s node `l`, a repeat or
    /// conditional: `outer`'s nodes under `l`, and the relations from
    /// them `outer` derives (a picture's `figures`; all of a collection's).
    pub(crate) fn sub(outer: &Template, l: NodeId) -> Template {
        let mut under = BTreeSet::from([l]);
        let mut nodes = Vec::new();
        let mut rels = Vec::new();
        for n in &outer.nodes {
            if !under.contains(&n.parent) {
                continue;
            }
            under.insert(n.local);
            rels.extend(n.figures.clone());
            nodes.push(TNode::new(n.local, n.parent, n.ty, n.order.clone(), n.facets.clone()));
        }
        rels.extend(outer.rels.iter().filter(|x| x.from != l && under.contains(&x.from)).cloned());
        Template::collection(l, nodes, rels)
    }

    fn collection(r: NodeId, nodes: Vec<TNode>, rels: Vec<Relation>) -> Template {
        let after = nodes.iter().filter(|t| t.parent == r).map(|t| t.order.clone()).max().unwrap_or_else(OrderKey::first);
        let inside: BTreeSet<NodeId> = nodes.iter().map(|t| t.local).collect();
        let nodes = nodes
            .into_iter()
            .map(|mut t| {
                // An override of an item's copy names the authored node
                // the template node copies.
                t.target = t.local.path().last().copied().unwrap_or(t.local);
                t
            })
            .collect();
        // As a picture's: a node's own derivation (an inner picture's
        // parts) is inside with it.
        Template::classify(Source::Collection(r), nodes, rels, after, |id| inside.contains(&id.path()[0]).then_some(Ref::Inside))
    }

    /// The repeat or conditional a collection's items are of, as the
    /// source has it.
    pub(crate) fn repeat_node(&self) -> Option<NodeId> {
        match self.source {
            Source::Collection(r) => Some(r),
            _ => None,
        }
    }

    /// How `leaf`, as the template has it, reads from `env`'s copy.
    pub(crate) fn follow(&self, env: &Env, leaf: Leaf) -> Leaf {
        if let Source::Collection(r) = self.source {
            let item = env.item.as_ref().expect("an item's environment");
            // The repeat as the template names it, or as the item's
            // owner has it (a node naming a picture's copy of it).
            if leaf.node == r || leaf.node == item.repeat {
                let item = |f: &str| Leaf { node: env.owner, path: LeafPath::new(FacetKind::Item, f).expect("declared leaf") };
                if leaf.path == LeafPath::new(FacetKind::Repeat, "item").expect("declared leaf") {
                    return item("value");
                }
                if leaf.path == LeafPath::new(FacetKind::Repeat, "index").expect("declared leaf") {
                    return item("index");
                }
                // A repeat's or a conditional's `presence` and `leaving`
                // are each item's own.
                if expand::owns_items(leaf.path.facet) && matches!(leaf.path.field, "presence" | "leaving") {
                    return item(leaf.path.field);
                }
            }
            return match (self.refs.get(&leaf.node), &item.outer) {
                (Some(Ref::Inside), _) => NodeId::item_scope(item.repeat, &item.key).and_then(|scope| expand::relocate(scope, leaf.node)).map_or(leaf, |node| Leaf { node, path: leaf.path }),
                (_, Some((t, outer))) => {
                    let to = t.follow(outer, leaf);
                    Leaf { node: self.into_item(item, outer, to.node), path: to.path }
                }
                (_, None) => leaf,
            };
        }
        let to = match self.refs.get(&leaf.node) {
            None => return leaf,
            Some(Ref::Root) => {
                return if env.sized && leaf.path.facet == FacetKind::Size { Leaf { node: env.owner, path: leaf.path } } else { leaf };
            }
            Some(Ref::Inside) => env.owner,
            Some(Ref::Body) => match env.entity {
                Some(e) => e,
                None => return leaf,
            },
        };
        expand::relocate(to, leaf.node).map_or(leaf, |node| Leaf { node, path: leaf.path })
    }

    /// Where a node the template names (a relation's far end, a node held
    /// as a constant) is for `env`'s copy.
    pub(crate) fn follow_node(&self, env: &Env, node: NodeId) -> NodeId {
        self.follow(env, Leaf { node, path: any_leaf() }).node
    }

    /// `node`, as `item`'s outer owner `outer` names it, as the item names
    /// it: what is under `outer`'s copy of one of this template's nodes is
    /// under the item's copy (the old code copied an item from the outer
    /// copies and moved what was under them). An owner's copy of its
    /// template's node `x` is the owner's path and `x`, for a picture and
    /// for an item alike.
    fn into_item(&self, item: &Item, outer: &Env, node: NodeId) -> NodeId {
        let (o, p) = (outer.owner.path(), node.path());
        if p.len() <= o.len() || p[..o.len()] != o[..] || !self.locals.contains(&p[o.len()]) {
            return node;
        }
        let Some(scope) = NodeId::item_scope(item.repeat, &item.key) else { return node };
        let mut q = scope.path();
        q.extend_from_slice(&p[o.len()..]);
        NodeId::from_path(&q).unwrap_or(node)
    }

    /// Node `i`'s facets as `env`'s copy has them: references resolved,
    /// an `in` port reading the owner's argument, the owner's overrides
    /// applied (component-model.md §5.3).
    pub(crate) fn materialise(&self, i: usize, env: &Env) -> Vec<Facet> {
        let n = &self.nodes[i];
        let mut facets = n.facets.clone();
        if n.rewrite {
            let follow = |leaf: Leaf| self.follow(env, leaf);
            for f in &mut facets {
                for field in f.field_names() {
                    match f.get(field) {
                        Some(Slot::Bound(e)) => {
                            let e = e.map_refs(&follow);
                            f.set(field, Slot::Bound(e)).expect("field exists");
                        }
                        Some(Slot::Const(Value::Ref(to))) => {
                            let to = self.follow_node(env, *to);
                            f.set(field, Slot::Const(Value::Ref(to))).expect("field exists");
                        }
                        _ => {}
                    }
                }
            }
        }
        if let Some(arg) = n.arg {
            for f in &mut facets {
                if let Facet::Port(p) = f {
                    p.signal = Slot::Bound(Expr::Ref(Leaf { node: env.owner, path: arg }));
                }
            }
        }
        apply_overrides(self, n, env, &mut facets);
        facets
    }
}

/// `env`'s patch overrides of node `n`'s copy: an item's outer owner's first.
fn apply_overrides(t: &Template, n: &TNode, env: &Env, facets: &mut Vec<Facet>) {
    if let Some(item @ Item { outer: Some((outer_t, outer)), .. }) = &env.item {
        apply_overrides(outer_t, n, outer, facets);
        // An outer override naming a node under this template's, as the
        // outer owner copies it, names the item's copy (`Template::into_item`).
        if outer_sets_a_node(n, outer) {
            for f in facets.iter_mut() {
                for field in f.field_names() {
                    if let Some(Slot::Const(Value::Ref(to))) = f.get(field) {
                        let copy = t.into_item(item, outer, *to);
                        if copy != *to {
                            f.set(field, Slot::Const(Value::Ref(copy))).expect("field exists");
                        }
                    }
                }
            }
        }
    }
    let mut suppressed: HashSet<FacetKind> = HashSet::new();
    for (target, key, val) in &env.overrides {
        if *target != n.target {
            continue;
        }
        if let (PatchKey::FacetPresent(fk), PatchValue::Bool { v }) = (key, val) {
            if *v {
                if !facets.iter().any(|f| f.kind() == *fk) && n.ty.spec().allows(*fk) {
                    facets.push(fk.default_facet());
                }
            } else {
                suppressed.insert(*fk);
                facets.retain(|f| f.kind() != *fk);
            }
        }
    }
    for (target, key, val) in &env.overrides {
        if *target != n.target {
            continue;
        }
        match (key, val) {
            (PatchKey::Leaf(leaf), PatchValue::Slot { slot }) => {
                // P21: field writes never synthesize missing optional facets implicitly.
                if suppressed.contains(&leaf.facet) {
                    continue;
                }
                if let Some(f) = facets.iter_mut().find(|f| f.kind() == leaf.facet) {
                    let _ = f.set(leaf.field, slot.clone());
                }
            }
            (PatchKey::Notation, PatchValue::Notation { notation }) => {
                if let Some(Facet::Use(u)) = facets.iter_mut().find(|f| matches!(f, Facet::Use(_))) {
                    u.notation = Slot::Const(Value::Str(notation.clone()));
                }
            }
            _ => {}
        }
    }
    // Keep facets in `FacetKind` order (matching `Document::facets_of` / `BTreeMap`).
    facets.sort_by_key(Facet::kind);
}

/// Whether an override reaching `env` (or an owner it is an item of) sets
/// node `n`'s copy to a node reference.
fn outer_sets_a_node(n: &TNode, env: &Env) -> bool {
    env.overrides.iter().any(|(target, _, v)| {
        *target == n.target && matches!(v, PatchValue::Slot { slot: Slot::Const(Value::Ref(_)) })
    }) || matches!(&env.item, Some(Item { outer: Some((_, outer)), .. }) if outer_sets_a_node(n, outer))
}

/// One cached template entry's lookup key, used by the reverse dependency index.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum CacheKey {
    Body(NodeId),
    Picture(NodeId, String),
    Repeat(NodeId),
}

/// The templates compiled so far, kept between passes and invalidated by
/// recorded structural reads (`Tree::recording` -> `Key` reverse index, EN-5a).
#[derive(Clone, Default)]
pub(crate) struct TemplateCache {
    next_rev: u64,
    bodies: HashMap<NodeId, Arc<Template>>,
    /// A definition without a template for a notation is cached too (`None`),
    /// with its recorded reads so adding that notation invalidates it.
    pictures: HashMap<(NodeId, String), Option<Arc<Template>>>,
    /// Each authored repeat's or conditional's.
    repeats: HashMap<NodeId, Arc<Template>>,
    /// Each sub-template, keyed by its outer template's revision and the
    /// repeat node inside that outer template.
    subs: HashMap<(u64, NodeId), Arc<Template>>,
    /// Sub-template repeat ids owned by each outer revision, for O(subs)
    /// eviction when an outer template is invalidated.
    subs_by_rev: HashMap<u64, Vec<NodeId>>,
    /// Forward recorded-read dependencies per cached entry.
    deps: HashMap<CacheKey, Vec<Key>>,
    /// Reverse index from structural `Key` to cached entries that read it.
    by_key: HashMap<Key, Vec<CacheKey>>,
}

impl TemplateCache {
    fn alloc_rev(&mut self) -> u64 {
        self.next_rev += 1;
        self.next_rev
    }

    fn record_deps(&mut self, ck: CacheKey, raw: Vec<Key>) {
        let mut seen = HashSet::with_capacity(raw.len());
        let mut uniq = Vec::with_capacity(raw.len());
        for k in raw {
            if seen.insert(k) {
                uniq.push(k);
            }
        }
        for &k in &uniq {
            self.by_key.entry(k).or_default().push(ck.clone());
        }
        self.deps.insert(ck, uniq);
    }

    fn evict_rev(&mut self, root_rev: u64) {
        let mut stack = vec![root_rev];
        while let Some(r) = stack.pop() {
            if let Some(locals) = self.subs_by_rev.remove(&r) {
                for l in locals {
                    if let Some(sub) = self.subs.remove(&(r, l)) {
                        stack.push(sub.rev);
                    }
                }
            }
        }
    }

    fn evict(&mut self, ck: &CacheKey) {
        let removed_rev = match ck {
            CacheKey::Body(def) => self.bodies.remove(def).map(|t| t.rev),
            CacheKey::Picture(def, notation) => self.pictures.remove(&(*def, notation.clone())).flatten().map(|t| t.rev),
            CacheKey::Repeat(r) => self.repeats.remove(r).map(|t| t.rev),
        };
        if let Some(r) = removed_rev {
            self.evict_rev(r);
        }
        if let Some(old_deps) = self.deps.remove(ck) {
            for k in old_deps {
                if let Some(list) = self.by_key.get_mut(&k) {
                    list.retain(|x| x != ck);
                    if list.is_empty() {
                        self.by_key.remove(&k);
                    }
                }
            }
        }
    }

    /// Evict every cached template whose recorded reads intersect `keys`.
    pub(crate) fn note_touched(&mut self, keys: &[Key]) {
        if self.by_key.is_empty() || keys.is_empty() {
            return;
        }
        let mut to_evict: Vec<CacheKey> = Vec::new();
        for k in keys {
            if let Some(list) = self.by_key.remove(k) {
                to_evict.extend(list);
            }
        }
        for ck in to_evict {
            self.evict(&ck);
        }
    }

    /// Drop every cached entry (called on transaction rollback so templates
    /// compiled from uncommitted state never survive).
    pub(crate) fn clear(&mut self) {
        self.bodies.clear();
        self.pictures.clear();
        self.repeats.clear();
        self.subs.clear();
        self.subs_by_rev.clear();
        self.deps.clear();
        self.by_key.clear();
    }

    /// Revision of a cached body template, if present (for tests).
    #[allow(dead_code)]
    pub(crate) fn body_rev(&self, def: NodeId) -> Option<u64> {
        self.bodies.get(&def).map(|t| t.rev)
    }

    /// Revision of a cached picture template, if present and `Some` (for tests).
    #[allow(dead_code)]
    pub(crate) fn picture_rev(&self, def: NodeId, notation: &str) -> Option<u64> {
        self.pictures.get(&(def, notation.to_owned())).and_then(|o| o.as_ref()).map(|t| t.rev)
    }

    /// Whether `(def, notation)` is cached (even as `None`, for tests).
    #[allow(dead_code)]
    pub(crate) fn has_picture_entry(&self, def: NodeId, notation: &str) -> bool {
        self.pictures.contains_key(&(def, notation.to_owned()))
    }
}

impl std::fmt::Debug for TemplateCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TemplateCache({} bodies, {} pictures, {} repeats, {} sub-templates)", self.bodies.len(), self.pictures.len(), self.repeats.len(), self.subs.len())
    }
}

/// One pass's view of the compiled templates, forwarding each template's
/// recorded structural dependencies into any recording [`Tree`] caller (`EN-5b`).
pub(crate) struct Templates<'c> {
    cache: &'c mut TemplateCache,
}

impl<'c> Templates<'c> {
    pub(crate) fn new(cache: &'c mut TemplateCache) -> Templates<'c> {
        Templates { cache }
    }

    pub(crate) fn body(&mut self, doc: Tree<'_>, def: NodeId) -> Arc<Template> {
        let ck = CacheKey::Body(def);
        if let Some(t) = self.cache.bodies.get(&def) {
            if let Some(deps) = self.cache.deps.get(&ck) {
                doc.note_all(deps);
            }
            return t.clone();
        }
        let rec = Recorder::default();
        let mut t = Template::body(doc.recording(&rec), def);
        t.rev = self.cache.alloc_rev();
        let t = Arc::new(t);
        self.cache.bodies.insert(def, t.clone());
        self.cache.record_deps(ck.clone(), rec.into_inner());
        if let Some(deps) = self.cache.deps.get(&ck) {
            doc.note_all(deps);
        }
        t
    }

    pub(crate) fn picture(&mut self, doc: Tree<'_>, def: NodeId, notation: &str) -> Option<Arc<Template>> {
        let key = (def, notation.to_owned());
        let ck = CacheKey::Picture(def, notation.to_owned());
        if let Some(t) = self.cache.pictures.get(&key) {
            if let Some(deps) = self.cache.deps.get(&ck) {
                doc.note_all(deps);
            }
            return t.clone();
        }
        let rec = Recorder::default();
        let rdoc = doc.recording(&rec);
        let t = definition::template_of(rdoc, def, notation).map(|template| {
            let mut compiled = Template::picture(rdoc, def, template);
            compiled.rev = self.cache.alloc_rev();
            Arc::new(compiled)
        });
        self.cache.pictures.insert(key, t.clone());
        self.cache.record_deps(ck.clone(), rec.into_inner());
        if let Some(deps) = self.cache.deps.get(&ck) {
            doc.note_all(deps);
        }
        t
    }

    /// Authored repeat `r`'s template.
    pub(crate) fn repeat(&mut self, doc: Tree<'_>, r: NodeId) -> Arc<Template> {
        let ck = CacheKey::Repeat(r);
        if let Some(t) = self.cache.repeats.get(&r) {
            if let Some(deps) = self.cache.deps.get(&ck) {
                doc.note_all(deps);
            }
            return t.clone();
        }
        let rec = Recorder::default();
        let mut t = Template::repeat(doc.recording(&rec), r);
        t.rev = self.cache.alloc_rev();
        let t = Arc::new(t);
        self.cache.repeats.insert(r, t.clone());
        self.cache.record_deps(ck.clone(), rec.into_inner());
        if let Some(deps) = self.cache.deps.get(&ck) {
            doc.note_all(deps);
        }
        t
    }

    /// The template of a copy of `outer`'s repeat `l`, tied to `outer.rev`.
    pub(crate) fn sub(&mut self, outer: &Template, l: NodeId) -> Arc<Template> {
        let key = (outer.rev, l);
        if let Some(t) = self.cache.subs.get(&key) {
            return t.clone();
        }
        let mut t = Template::sub(outer, l);
        t.rev = self.cache.alloc_rev();
        let t = Arc::new(t);
        self.cache.subs.insert(key, t.clone());
        self.cache.subs_by_rev.entry(outer.rev).or_default().push(l);
        t
    }
}
