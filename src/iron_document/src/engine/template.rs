//! Templates (plans/specs/engine.md §2.3; EN-12, EN-13): what an owner
//! derives, compiled once and shared via `Arc<Template>` across every owner
//! made from it, and [`Template::sparse_facets`], which allocates only the
//! facets of a view that differ from the shared template (rewritten refs,
//! `in` port signal bindings, or lexical overrides).
//!
//! Three kinds, by what is derived:
//! - a **body**, per definition: its port_specs (as ports), equations,
//!   inner entities, connections, channels, machines and the rest
//!   (`expand::is_body`), then each machine's states, transitions and tweens;
//! - a **picture**, per (definition, notation): the template's subtree,
//!   down to and including each `use` (an inner picture is its own owner);
//! - a **collection**, per repeat or conditional: its children that are
//!   not items, with their subtrees. An authored repeat's is compiled from
//!   the document; a derived one's (a repeat in a picture, or in another
//!   repeat's items) is a sub-template of the template that copies it
//!   ([`Template::sub`]), shared by every copy.
//!
//! Templates are cached in [`TemplateCache`], indexed by the structural
//! [`Key`]s recorded while compiling each entry (`Tree::recording`). Writes
//! notify the cache via [`TemplateCache::note_touched`], returning the evicted
//! [`CacheKey`]s so owners subscribed to them re-derive; a small transaction
//! journal (`begin`/`commit`/`rollback`) restores the exact committed cache
//! on rollback without losing `by_key` subscriptions.

use crate::definition;
use crate::document::{RelKind, Relation};
use crate::expand;
use crate::expr::{Expr, Leaf};
use crate::facet::{Facet, FacetKind, LeafPath};
use crate::id::NodeId;
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::structure::{Key, Recorder};
use crate::value::{Slot, Value};
use crate::Tree;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

/// How a node a template refers to is read from one owner's virtual copy.
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
    /// The node copied: an owner's copy is `NodeId::expanded(owner, local)`.
    pub(crate) local: NodeId,
    /// Its parent in the source; the source's root stands for the owner.
    pub(crate) parent: NodeId,
    /// The copy's type: a port_spec's copy is a port.
    pub(crate) ty: TypeId,
    pub(crate) order: OrderKey,
    /// Facets sorted by `FacetKind`. Unpatched constant facets are borrowed
    /// directly from here by virtual views with zero per-instance allocation.
    pub(crate) facets: Vec<Facet>,
    /// Parallel to `facets`: whether that facet contains any ref or binding
    /// that may need relocation per environment.
    facet_rewrites: Vec<bool>,
    /// Indices into `Template::nodes` of direct children of this node,
    /// sorted by `(order, local)`.
    pub(crate) children: Vec<usize>,
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
    /// entities).
    pub(crate) figures: Option<Relation>,
    pub(crate) own: bool,
}

impl TNode {
    fn new(local: NodeId, parent: NodeId, ty: TypeId, order: OrderKey, mut facets: Vec<Facet>) -> TNode {
        facets.sort_by_key(Facet::kind);
        let facet_rewrites = vec![false; facets.len()];
        TNode {
            local,
            parent,
            ty,
            order,
            facets,
            facet_rewrites,
            children: Vec::new(),
            arg: None,
            target: local,
            rels: Vec::new(),
            top: false,
            figures: None,
            own: false,
        }
    }

    pub(crate) fn facet(&self, kind: FacetKind) -> Option<&Facet> {
        self.facets.iter().find(|f| f.kind() == kind)
    }
}

/// A compiled template, shared via `Arc<Template>` across every virtual view.
pub(crate) struct Template {
    pub(crate) source: Source,
    /// Parents first.
    pub(crate) nodes: Vec<TNode>,
    /// Indices into `nodes` of direct children of `source_root()`, sorted by
    /// `(order, local)`.
    pub(crate) top_children: Vec<usize>,
    /// O(1) lookup from `local: NodeId` to index in `nodes`.
    pub(crate) local_index: HashMap<NodeId, usize>,
    /// Precomputed `(node_idx, FacetKind)` pairs holding at least one
    /// `Slot::Bound(_)` (or `in`-port signal binding), so `Tree::bound_slots`
    /// skips static constant nodes.
    pub(crate) bound_facets: Vec<(usize, FacetKind)>,
    /// Indices into `nodes` of nested owners (`entity`, `use`, or nodes with a
    /// template-declared `Repeat` or `When` facet), ascending.
    pub(crate) nested_owners: Vec<usize>,
    /// Indices into `nodes` where `!n.rels.is_empty()`, ascending.
    pub(crate) rel_nodes: Vec<usize>,
    /// Indices into `nodes` with at least one `facet_rewrites` entry or an
    /// `in`-port `arg` binding, ascending.
    pub(crate) rewrite_nodes: Vec<usize>,
    /// `n.target -> ascending node indices` for O(|overrides|) sparse facet
    /// resolution instead of scanning `0..limit`.
    pub(crate) target_index: HashMap<NodeId, Vec<usize>>,
    /// A collection's relations among its nodes: an item's are linked
    /// after all its nodes.
    pub(crate) rels: Vec<Relation>,
    refs: HashMap<NodeId, Ref>,
    /// A collection's items are ordered after this key.
    pub(crate) after: OrderKey,
    /// Monotonic revision assigned when compiled into [`TemplateCache`], tying
    /// sub-templates to their exact outer template instance.
    pub(crate) rev: u64,
}

/// One owner, as its template's references resolve against it. Two equal
/// environments resolve every reference and override of a template alike.
#[derive(Clone, PartialEq)]
pub(crate) struct Env {
    /// The entity, `use` or item scope.
    pub(crate) owner: NodeId,
    /// The entity a picture figures; `None` in a main.
    pub(crate) entity: Option<NodeId>,
    pub(crate) sized: bool,
    /// A collection item.
    pub(crate) item: Option<Item>,
    /// The overrides reaching the owner's copies: (target, leaf, value),
    /// weakest first.
    pub(crate) overrides: Vec<(NodeId, LeafPath, Value)>,
}

/// A collection item's part of its environment.
#[derive(Clone)]
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

/// Collect the nodes `f` refers to (in bindings and as constant refs),
/// returning `(has_any_ref_or_binding, has_bound_slot)`.
fn scan_facet(f: &Facet, out: &mut Vec<NodeId>) -> (bool, bool) {
    let mut has_ref = false;
    let mut has_bound = false;
    for field in f.field_names() {
        match f.get(field) {
            Some(Slot::Bound(e)) => {
                has_ref = true;
                has_bound = true;
                e.walk(&mut |x| match x {
                    Expr::Ref(l) => out.push(l.node),
                    Expr::Lit(Value::Ref(n)) => out.push(*n),
                    _ => {}
                });
            }
            Some(Slot::Const(Value::Ref(n))) => {
                has_ref = true;
                out.push(*n);
            }
            _ => {}
        }
    }
    (has_ref, has_bound)
}

impl Template {
    pub(crate) fn source_root(&self) -> NodeId {
        match self.source {
            Source::Body(d) | Source::Picture(d) | Source::Collection(d) => d,
        }
    }

    /// Classify every node the template's nodes and relations refer to, and
    /// precompute parent→children adjacency, `local_index`, and `bound_facets`.
    fn classify(source: Source, mut nodes: Vec<TNode>, rels: Vec<Relation>, after: OrderKey, class: impl Fn(NodeId) -> Option<Ref>) -> Template {
        let root = match source {
            Source::Body(d) | Source::Picture(d) | Source::Collection(d) => d,
        };
        let mut seen: Vec<NodeId> = rels.iter().map(|r| r.to).collect();
        let mut local_index = HashMap::with_capacity(nodes.len());
        let mut bound_facets = Vec::new();
        let mut nested_owners = Vec::new();
        let mut rel_nodes = Vec::new();
        let mut rewrite_nodes = Vec::new();
        let mut target_index: HashMap<NodeId, Vec<usize>> = HashMap::new();
        for (i, n) in nodes.iter_mut().enumerate() {
            local_index.insert(n.local, i);
            target_index.entry(n.target).or_default().push(i);
            n.facet_rewrites.clear();
            n.facet_rewrites.reserve(n.facets.len());
            let mut node_has_rewrite = n.arg.is_some();
            let mut has_repeat_or_when = false;
            for f in &n.facets {
                let (has_ref, has_bound) = scan_facet(f, &mut seen);
                n.facet_rewrites.push(has_ref);
                if has_ref {
                    node_has_rewrite = true;
                }
                if has_bound || (n.arg.is_some() && f.kind() == FacetKind::Port) {
                    bound_facets.push((i, f.kind()));
                }
                if matches!(f.kind(), FacetKind::Repeat | FacetKind::When) {
                    has_repeat_or_when = true;
                }
            }
            if node_has_rewrite {
                rewrite_nodes.push(i);
            }
            if matches!(n.ty.name(), "entity" | "use") || has_repeat_or_when {
                nested_owners.push(i);
            }
            if !n.rels.is_empty() {
                rel_nodes.push(i);
            }
            seen.extend(n.rels.iter().map(|r| r.to));
            seen.extend(n.figures.iter().map(|r| r.to));
        }
        let mut top_children = Vec::new();
        for i in 0..nodes.len() {
            let p = nodes[i].parent;
            if p == root {
                top_children.push(i);
            } else if let Some(&pi) = local_index.get(&p) {
                nodes[pi].children.push(i);
            }
        }
        // Sort sibling child index lists by `(order, local)`.
        top_children.sort_by(|&a, &b| (&nodes[a].order, nodes[a].local).cmp(&(&nodes[b].order, nodes[b].local)));
        for i in 0..nodes.len() {
            if nodes[i].children.len() > 1 {
                let mut ch = std::mem::take(&mut nodes[i].children);
                ch.sort_by(|&a, &b| (&nodes[a].order, nodes[a].local).cmp(&(&nodes[b].order, nodes[b].local)));
                nodes[i].children = ch;
            }
        }
        let mut refs = HashMap::new();
        for id in seen {
            if let std::collections::hash_map::Entry::Vacant(e) = refs.entry(id)
                && let Some(c) = class(id)
            {
                e.insert(c);
            }
        }
        Template {
            source,
            nodes,
            top_children,
            local_index,
            bound_facets,
            nested_owners,
            rel_nodes,
            rewrite_nodes,
            target_index,
            rels,
            refs,
            after,
            rev: 0,
        }
    }

    /// Definition `def`'s body (`entity` ports, equations, inner entities,
    /// connections, channels, machines and their states/transitions/tweens).
    pub(crate) fn body(doc: Tree<'_>, def: NodeId) -> Template {
        let inputs = definition::inputs_of(doc, def).unwrap_or_default();
        let port = TypeId::lookup("port").expect("registered");
        let mut nodes = Vec::new();
        for &child in doc.children(def).iter() {
            let Some(node) = doc.live(child).filter(|_| !child.is_session()) else { continue };
            let ty = match node.ty.name() {
                "port_spec" => port,
                "equation" | "entity" | "connection" | "channel" | "machine" | "let" | "trace" | "accumulate" | "query" => node.ty,
                _ => continue,
            };
            let mut t = TNode::new(child, def, ty, node.order.clone(), doc.facets_of(child).cloned().collect());
            t.top = true;
            t.arg = inputs.iter().find(|i| i.spec == child).map(|i| LeafPath::new(FacetKind::Args, i.name).expect("argument path"));
            if matches!(node.ty.name(), "connection" | "port_spec") {
                t.rels = doc.relations_from(child).cloned().collect();
            }
            nodes.push(t);
        }
        let copied = |n: NodeId| !n.is_expanded() && !n.is_session();
        for &machine in doc.children(def).iter().filter(|&&c| !c.is_session() && doc.live(c).is_some_and(|n| n.ty.name() == "machine")) {
            let mut stack: Vec<NodeId> = doc.children(machine).iter().rev().copied().filter(|&c| copied(c)).collect();
            while let Some(local) = stack.pop() {
                let Some(n) = doc.live(local) else { continue };
                let Some(parent) = n.parent else { continue };
                nodes.push(TNode::new(local, parent, n.ty, n.order.clone(), doc.facets_of(local).cloned().collect()));
                stack.extend(doc.children(local).iter().rev().copied().filter(|&c| copied(c)));
            }
        }
        Template::classify(Source::Body(def), nodes, Vec::new(), OrderKey::first(), |id| expand::is_body(doc, def, id.path()[0]).then_some(Ref::Inside))
    }

    /// Definition `def`'s picture from its template node `template`: preorder,
    /// each `use` without its subtree.
    pub(crate) fn picture(doc: Tree<'_>, def: NodeId, template: NodeId) -> Template {
        let inside: BTreeSet<NodeId> = doc.descendants(template).into_iter().filter(|d| !d.is_expanded()).collect();
        let copied = |n: NodeId| !n.is_expanded() && !n.is_session() && doc.live(n).is_some_and(|x| x.ty.name() != "override");
        let mut nodes = Vec::new();
        let mut stack: Vec<NodeId> = doc.children(template).iter().rev().copied().filter(|&c| copied(c)).collect();
        while let Some(local) = stack.pop() {
            let Some(node) = doc.live(local) else { continue };
            let Some(parent) = node.parent else { continue };
            let mut t = TNode::new(local, parent, node.ty, node.order.clone(), doc.facets_of(local).cloned().collect());
            if node.ty.name() == "use" {
                if let Some(r) = doc.relations_from(local).find(|r| r.rel == RelKind::Figures) {
                    t.own = !inside.contains(&r.to.path()[0]);
                    t.figures = Some(r.clone());
                }
                nodes.push(t);
                continue;
            }
            nodes.push(t);
            stack.extend(doc.children(local).iter().rev().copied().filter(|&c| copied(c)));
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

    /// Authored repeat (or conditional) `r`'s template: its non-item children
    /// and their subtrees, parents first, with relations from them.
    pub(crate) fn repeat(doc: Tree<'_>, r: NodeId) -> Template {
        let mut nodes = Vec::new();
        let mut rels = Vec::new();
        let copied = |c: &NodeId| !c.is_expanded() && !c.is_session();
        let mut stack: Vec<NodeId> = doc.authored_children(r).iter().rev().copied().filter(copied).collect();
        while let Some(n) = stack.pop() {
            let Some(node) = doc.live(n).filter(|x| x.ty.name() != "override") else { continue };
            nodes.push(TNode::new(n, node.parent.unwrap_or(r), node.ty, node.order.clone(), doc.facets_of(n).cloned().collect()));
            rels.extend(doc.relations_from(n).filter(|x| !x.id.is_expanded()).cloned());
            stack.extend(doc.authored_children(n).iter().rev().copied().filter(copied));
        }
        Template::collection(r, nodes, rels)
    }

    /// Sub-template of a copy of `outer`'s node `l` (a repeat or conditional).
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
                t.target = t.local.path().last().copied().unwrap_or(t.local);
                t
            })
            .collect();
        Template::classify(Source::Collection(r), nodes, rels, after, |id| inside.contains(&id.path()[0]).then_some(Ref::Inside))
    }

    /// How `leaf`, as the template has it, reads from `env`'s copy.
    pub(crate) fn follow(&self, env: &Env, leaf: Leaf) -> Leaf {
        if let Source::Collection(r) = self.source {
            let item = env.item.as_ref().expect("an item's environment");
            if leaf.node == r || leaf.node == item.repeat {
                let item_leaf = |f: &str| Leaf { node: env.owner, path: LeafPath::new(FacetKind::Item, f).expect("declared leaf") };
                if leaf.path == LeafPath::new(FacetKind::Repeat, "item").expect("declared leaf") {
                    return item_leaf("value");
                }
                if leaf.path == LeafPath::new(FacetKind::Repeat, "index").expect("declared leaf") {
                    return item_leaf("index");
                }
                if expand::owns_items(leaf.path.facet) && matches!(leaf.path.field, "presence" | "leaving") {
                    return item_leaf(leaf.path.field);
                }
            }
            return match (self.refs.get(&leaf.node), &item.outer) {
                (Some(Ref::Inside), _) => NodeId::item_scope(item.repeat, &item.key)
                    .and_then(|scope| expand::relocate(scope, leaf.node))
                    .map_or(leaf, |node| Leaf { node, path: leaf.path }),
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

    /// Where a node the template names is for `env`'s copy.
    pub(crate) fn follow_node(&self, env: &Env, node: NodeId) -> NodeId {
        self.follow(env, Leaf { node, path: any_leaf() }).node
    }

    fn into_item(&self, item: &Item, outer: &Env, node: NodeId) -> NodeId {
        let (o, p) = (outer.owner.path(), node.path());
        if p.len() <= o.len() || p[..o.len()] != o[..] || !self.local_index.contains_key(&p[o.len()]) {
            return node;
        }
        let Some(scope) = NodeId::item_scope(item.repeat, &item.key) else { return node };
        let mut q = scope.path();
        q.extend_from_slice(&p[o.len()..]);
        NodeId::from_path(&q).unwrap_or(node)
    }

    /// Build ONLY the sparse facets for `env` across `self.nodes[..limit]`:
    /// facets whose refs/bindings relocated, `in`-port signal bindings, and
    /// facets touched or added by lexical overrides. Untouched constant facets
    /// are NOT cloned and stay borrowed from `Arc<Template>` (`EN-13`).
    /// Iterates only `self.rewrite_nodes` unioned with override target indices
    /// (`self.target_index`), never walking `0..limit` for constant nodes.
    pub(crate) fn sparse_facets(&self, env: &Env, limit: usize) -> (BTreeMap<(usize, FacetKind), Facet>, BTreeMap<usize, Vec<FacetKind>>) {
        let mut sparse: BTreeMap<(usize, FacetKind), Facet> = BTreeMap::new();
        let mut extra: BTreeMap<usize, Vec<FacetKind>> = BTreeMap::new();
        let has_any_overrides = any_overrides_in_chain(env);
        let mut candidates: Vec<usize> = Vec::new();
        let indices: &[usize] = if !has_any_overrides {
            &self.rewrite_nodes
        } else {
            candidates.extend(self.rewrite_nodes.iter().copied().take_while(|&i| i < limit));
            let mut targets: Vec<NodeId> = Vec::new();
            collect_override_targets(env, &mut targets);
            for t in targets {
                if let Some(idxs) = self.target_index.get(&t) {
                    candidates.extend(idxs.iter().copied().take_while(|&i| i < limit));
                }
            }
            candidates.sort_unstable();
            candidates.dedup();
            &candidates
        };
        for &i in indices {
            if i >= limit {
                break;
            }
            let n = &self.nodes[i];
            // Collect all overrides targeting `n.target` (outer-item first, weakest first).
            let mut ov_kinds: Vec<FacetKind> = Vec::new();
            if has_any_overrides {
                collect_override_kinds(n.target, env, &mut ov_kinds);
            }
            // Existing template facets: only clone if rewritten, port-arg, or overridden.
            for (fi, orig) in n.facets.iter().enumerate() {
                let kind = orig.kind();
                let needs_rewrite = n.facet_rewrites[fi];
                let needs_arg = n.arg.is_some() && kind == FacetKind::Port;
                let needs_ov = ov_kinds.contains(&kind);
                if !needs_rewrite && !needs_arg && !needs_ov {
                    continue;
                }
                let mut f = orig.clone();
                if needs_rewrite {
                    let follow = |leaf: Leaf| self.follow(env, leaf);
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
                if needs_arg
                    && let (Some(arg), Facet::Port(p)) = (n.arg, &mut f)
                {
                    p.signal = Slot::Bound(Expr::Ref(Leaf { node: env.owner, path: arg }));
                }
                if needs_ov {
                    apply_facet_overrides(self, n, env, kind, &mut f);
                }
                if f != *orig {
                    sparse.insert((i, kind), f);
                }
            }
            // Override-added optional facets absent from the template node.
            for kind in ov_kinds {
                if n.facet(kind).is_some() || !n.ty.spec().allows(kind) {
                    continue;
                }
                let mut f = kind.default_facet();
                apply_facet_overrides(self, n, env, kind, &mut f);
                sparse.insert((i, kind), f);
                extra.entry(i).or_default().push(kind);
            }
            if let Some(kinds) = extra.get_mut(&i) {
                kinds.sort();
                kinds.dedup();
            }
        }
        (sparse, extra)
    }
}

fn any_overrides_in_chain(env: &Env) -> bool {
    !env.overrides.is_empty()
        || matches!(&env.item, Some(Item { outer: Some((_, outer)), .. }) if any_overrides_in_chain(outer))
}

fn collect_override_targets(env: &Env, out: &mut Vec<NodeId>) {
    if let Some(Item { outer: Some((_, outer)), .. }) = &env.item {
        collect_override_targets(outer, out);
    }
    for (t, _, _) in &env.overrides {
        if !out.contains(t) {
            out.push(*t);
        }
    }
}

fn collect_override_kinds(target: NodeId, env: &Env, out: &mut Vec<FacetKind>) {
    if let Some(Item { outer: Some((_, outer)), .. }) = &env.item {
        collect_override_kinds(target, outer, out);
    }
    for (t, leaf, _) in &env.overrides {
        if *t == target && !out.contains(&leaf.facet) {
            out.push(leaf.facet);
        }
    }
}

/// Apply all lexical overrides for `(n.target, kind)` onto `f`, outer-item
/// first, then `env.overrides`, weakest first.
fn apply_facet_overrides(t: &Template, n: &TNode, env: &Env, kind: FacetKind, f: &mut Facet) {
    if let Some(item @ Item { outer: Some((outer_t, outer)), .. }) = &env.item {
        apply_facet_overrides(outer_t, n, outer, kind, f);
        if outer_sets_a_node(n, outer, kind) {
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
    for (target, leaf, value) in &env.overrides {
        if *target == n.target && leaf.facet == kind {
            let _ = f.set(leaf.field, Slot::Const(value.clone()));
        }
    }
}

fn outer_sets_a_node(n: &TNode, env: &Env, kind: FacetKind) -> bool {
    env.overrides.iter().any(|(target, leaf, v)| *target == n.target && leaf.facet == kind && matches!(v, Value::Ref(_)))
        || matches!(&env.item, Some(Item { outer: Some((_, outer)), .. }) if outer_sets_a_node(n, outer, kind))
}

/// One cached template entry's lookup key, used by the reverse dependency index
/// and owner subscriptions.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CacheKey {
    Body(NodeId),
    Picture(NodeId, String),
    Repeat(NodeId),
}

#[derive(Clone)]
enum CacheEntry {
    Body(Arc<Template>),
    Picture(Option<Arc<Template>>),
    Repeat(Arc<Template>),
}

#[derive(Clone)]
struct CacheUndo {
    ck: CacheKey,
    prev: Option<(CacheEntry, Vec<Key>, Vec<(u64, NodeId, Arc<Template>)>)>,
}

/// Compiled templates kept between passes and invalidated by recorded
/// structural reads (`Tree::recording` -> `Key` reverse index), with a tiny
/// per-transaction undo journal so rollbacks restore the exact committed cache
/// without losing `by_key` subscriptions.
#[derive(Clone, Default)]
pub(crate) struct TemplateCache {
    next_rev: u64,
    bodies: HashMap<NodeId, Arc<Template>>,
    pictures: HashMap<(NodeId, String), Option<Arc<Template>>>,
    repeats: HashMap<NodeId, Arc<Template>>,
    subs: HashMap<(u64, NodeId), Arc<Template>>,
    subs_by_rev: HashMap<u64, Vec<NodeId>>,
    deps: HashMap<CacheKey, Vec<Key>>,
    by_key: HashMap<Key, Vec<CacheKey>>,
    journal: Option<Vec<CacheUndo>>,
}

impl TemplateCache {
    pub(crate) fn begin(&mut self) {
        self.journal = Some(Vec::new());
    }

    pub(crate) fn commit(&mut self) {
        self.journal = None;
    }

    pub(crate) fn rollback(&mut self) {
        let Some(recs) = self.journal.take() else { return };
        for rec in recs.into_iter().rev() {
            // Remove whatever is currently at `rec.ck` without re-journaling.
            self.evict_raw(&rec.ck);
            if let Some((entry, deps, subs)) = rec.prev {
                match entry {
                    CacheEntry::Body(t) => {
                        let CacheKey::Body(def) = rec.ck else { unreachable!() };
                        self.bodies.insert(def, t);
                    }
                    CacheEntry::Picture(t) => {
                        let CacheKey::Picture(def, ref n) = rec.ck else { unreachable!() };
                        self.pictures.insert((def, n.clone()), t);
                    }
                    CacheEntry::Repeat(t) => {
                        let CacheKey::Repeat(r) = rec.ck else { unreachable!() };
                        self.repeats.insert(r, t);
                    }
                }
                for (rev, l, sub) in subs {
                    self.subs.insert((rev, l), sub);
                    self.subs_by_rev.entry(rev).or_default().push(l);
                }
                self.record_deps(rec.ck, deps);
            }
        }
    }

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

    fn collect_subs(&mut self, root_rev: u64) -> Vec<(u64, NodeId, Arc<Template>)> {
        let mut out = Vec::new();
        let mut stack = vec![root_rev];
        while let Some(r) = stack.pop() {
            if let Some(locals) = self.subs_by_rev.remove(&r) {
                for l in locals {
                    if let Some(sub) = self.subs.remove(&(r, l)) {
                        stack.push(sub.rev);
                        out.push((r, l, sub));
                    }
                }
            }
        }
        out
    }

    fn evict_raw(&mut self, ck: &CacheKey) -> Option<(CacheEntry, Vec<Key>, Vec<(u64, NodeId, Arc<Template>)>)> {
        let entry = match ck {
            CacheKey::Body(def) => self.bodies.remove(def).map(CacheEntry::Body),
            CacheKey::Picture(def, notation) => self.pictures.remove(&(*def, notation.clone())).map(CacheEntry::Picture),
            CacheKey::Repeat(r) => self.repeats.remove(r).map(CacheEntry::Repeat),
        };
        let subs = match &entry {
            Some(CacheEntry::Body(t)) | Some(CacheEntry::Repeat(t)) => self.collect_subs(t.rev),
            Some(CacheEntry::Picture(Some(t))) => self.collect_subs(t.rev),
            _ => Vec::new(),
        };
        let old_deps = self.deps.remove(ck).unwrap_or_default();
        for &k in &old_deps {
            if let Some(list) = self.by_key.get_mut(&k) {
                list.retain(|x| x != ck);
                if list.is_empty() {
                    self.by_key.remove(&k);
                }
            }
        }
        entry.map(|e| (e, old_deps, subs))
    }

    fn evict(&mut self, ck: &CacheKey) {
        let prev = self.evict_raw(ck);
        if let (Some(j), Some(p)) = (&mut self.journal, prev) {
            j.push(CacheUndo { ck: ck.clone(), prev: Some(p) });
        }
    }

    /// Evict every cached template whose recorded reads intersect `keys`,
    /// returning the evicted [`CacheKey`]s so owners subscribed to them wake.
    pub(crate) fn note_touched(&mut self, keys: &[Key]) -> Vec<CacheKey> {
        if self.by_key.is_empty() || keys.is_empty() {
            return Vec::new();
        }
        let mut dirty: Vec<CacheKey> = Vec::new();
        for k in keys {
            if let Some(list) = self.by_key.get(k) {
                dirty.extend(list.iter().cloned());
            }
        }
        if dirty.is_empty() {
            return Vec::new();
        }
        let mut evicted = Vec::new();
        let mut seen = HashSet::with_capacity(dirty.len());
        for ck in dirty {
            if seen.insert(ck.clone()) {
                self.evict(&ck);
                evicted.push(ck);
            }
        }
        evicted
    }

    pub(crate) fn clear(&mut self) {
        self.bodies.clear();
        self.pictures.clear();
        self.repeats.clear();
        self.subs.clear();
        self.subs_by_rev.clear();
        self.deps.clear();
        self.by_key.clear();
    }
}

impl std::fmt::Debug for TemplateCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TemplateCache({} bodies, {} pictures, {} repeats, {} sub-templates)", self.bodies.len(), self.pictures.len(), self.repeats.len(), self.subs.len())
    }
}

/// One pass's view of the templates: caches per-node own overrides and tracks
/// which [`CacheKey`]s each owner read so template evictions wake owners.
pub(crate) struct Templates<'c> {
    cache: &'c mut TemplateCache,
    own: HashMap<NodeId, Rc<expand::IndexedOverrides>>,
    used: Vec<CacheKey>,
}

impl<'c> Templates<'c> {
    pub(crate) fn new(cache: &'c mut TemplateCache) -> Templates<'c> {
        Templates { cache, own: HashMap::new(), used: Vec::new() }
    }

    pub(crate) fn take_used(&mut self) -> Vec<CacheKey> {
        std::mem::take(&mut self.used)
    }

    pub(crate) fn body(&mut self, doc: Tree<'_>, def: NodeId) -> Arc<Template> {
        let ck = CacheKey::Body(def);
        self.used.push(ck.clone());
        if let Some(t) = self.cache.bodies.get(&def) {
            return t.clone();
        }
        let rec = Recorder::default();
        let mut t = Template::body(doc.recording(&rec), def);
        t.rev = self.cache.alloc_rev();
        let t = Arc::new(t);
        if let Some(j) = &mut self.cache.journal {
            j.push(CacheUndo { ck: ck.clone(), prev: None });
        }
        self.cache.bodies.insert(def, t.clone());
        self.cache.record_deps(ck, rec.into_inner());
        t
    }

    pub(crate) fn picture(&mut self, doc: Tree<'_>, def: NodeId, notation: &str) -> Option<Arc<Template>> {
        let ck = CacheKey::Picture(def, notation.to_owned());
        self.used.push(ck.clone());
        let key = (def, notation.to_owned());
        if let Some(t) = self.cache.pictures.get(&key) {
            return t.clone();
        }
        let rec = Recorder::default();
        let rdoc = doc.recording(&rec);
        let t = definition::template_of(rdoc, def, notation).map(|template| {
            let mut compiled = Template::picture(rdoc, def, template);
            compiled.rev = self.cache.alloc_rev();
            Arc::new(compiled)
        });
        if let Some(j) = &mut self.cache.journal {
            j.push(CacheUndo { ck: ck.clone(), prev: None });
        }
        self.cache.pictures.insert(key, t.clone());
        self.cache.record_deps(ck, rec.into_inner());
        t
    }

    /// Authored repeat `r`'s template.
    pub(crate) fn repeat(&mut self, doc: Tree<'_>, r: NodeId) -> Arc<Template> {
        let ck = CacheKey::Repeat(r);
        self.used.push(ck.clone());
        if let Some(t) = self.cache.repeats.get(&r) {
            return t.clone();
        }
        let rec = Recorder::default();
        let mut t = Template::repeat(doc.recording(&rec), r);
        t.rev = self.cache.alloc_rev();
        let t = Arc::new(t);
        if let Some(j) = &mut self.cache.journal {
            j.push(CacheUndo { ck: ck.clone(), prev: None });
        }
        self.cache.repeats.insert(r, t.clone());
        self.cache.record_deps(ck, rec.into_inner());
        t
    }

    /// Sub-template of a copy of `outer`'s repeat `l`, tied to `outer.rev`.
    /// Derived purely from the immutable parent revision, so memoized entries
    /// safely stay warm across rollbacks as long as `outer.rev` stays live.
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

    /// Overrides reaching `owner`'s copies, weakest first, reading and
    /// indexing each enclosing authored node's overrides once per `Templates`
    /// pass (O(1) target-prefix bucket lookup per repeat item).
    pub(crate) fn overrides(&mut self, doc: Tree<'_>, owner: NodeId) -> Vec<(NodeId, LeafPath, Value)> {
        let own = &mut self.own;
        expand::overrides_by_indexed(owner, |n| {
            own.entry(n).or_insert_with(|| Rc::new(expand::IndexedOverrides::from_vec(expand::own_overrides(doc, n)))).clone()
        })
        .into_iter()
        .map(|(_, t, l, v)| (t, l, v))
        .collect()
    }
}
