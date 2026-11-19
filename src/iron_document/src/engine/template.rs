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
//! each with the stamp its scope had when it was compiled: a body's and a
//! picture's scope is the definition, an authored repeat's the repeat, a
//! sub-template's its outer template's. A template is current while its
//! scope's stamp is unchanged (`Document::scope_stamp`): every write under
//! a definition or repeat stamps it again, a rollback included, and nothing
//! a compiler reads is outside its scope. So a refused transaction needs
//! nothing put back here: what it compiled no longer matches.

use crate::definition;
use crate::document::{RelKind, Relation};
use crate::expand;
use crate::expr::{Expr, Leaf};
use crate::facet::{Facet, FacetKind, LeafPath};
use crate::id::NodeId;
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::{Slot, Value};
use crate::Tree;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;
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
pub(crate) struct TNode {
    /// The node copied: an owner's copy is `NodeId::expanded(owner, local)`,
    /// an item's `NodeId::item(repeat, key, local)`.
    pub(crate) local: NodeId,
    /// Its parent in the source; the source's root stands for the owner.
    pub(crate) parent: NodeId,
    /// The copy's type: a port_spec's copy is a port.
    pub(crate) ty: TypeId,
    pub(crate) order: OrderKey,
    facets: Vec<Facet>,
    /// Whether any facet holds a reference to resolve.
    rewrite: bool,
    /// An `in` port's argument: its signal reads the owner's.
    arg: Option<LeafPath>,
    /// What an override of this node's copy names as its target.
    target: NodeId,
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
    /// What the template is compiled from (a definition, or an authored
    /// repeat), and its stamp then.
    scope: NodeId,
    stamp: u64,
}

/// One owner, as its template's references resolve against it.
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
pub(crate) struct Item {
    /// The repeat whose item this is: a derived one is a copy of the
    /// template's [`Source::Collection`] node.
    pub(crate) repeat: NodeId,
    pub(crate) key: String,
    /// A derived repeat's: the template that copies it and the
    /// environment of that copy's owner.
    pub(crate) outer: Option<(Arc<Template>, Rc<Env>)>,
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
        Template { source, nodes, rels, refs, locals, after, scope: NodeId::from_raw(0), stamp: 0 }
    }

    /// Definition `def`'s body (`expand::want_entity`'s copies).
    pub(crate) fn body(doc: Tree<'_>, def: NodeId) -> Template {
        let inputs = definition::inputs_of(doc, def).unwrap_or_default();
        let port = TypeId::lookup("port").expect("registered");
        let mut nodes = Vec::new();
        for &child in doc.children(def).iter() {
            // A session node gets no copy (`NodeId::expanded`).
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
        // A machine's states, transitions and tweens (machines.md §9 Q12),
        // after the ports they read.
        // A session node gets no copy, so nothing under it does either.
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
        Template::classify(Source::Body(def), nodes, Vec::new(), OrderKey::first(), |id| expand::is_body(doc, def, id.path()[0]).then_some(Ref::Inside)).in_scope(doc, def)
    }

    fn in_scope(mut self, doc: Tree<'_>, scope: NodeId) -> Template {
        self.scope = scope;
        self.stamp = doc.scope_stamp(scope);
        self
    }

    /// Whether the document is as it was when this was compiled.
    fn current(&self, doc: Tree<'_>) -> bool {
        doc.scope_stamp(self.scope) == self.stamp
    }

    /// Definition `def`'s picture from its template node `template`
    /// (`expand::want_use`'s copies): preorder, each `use` without its
    /// subtree. A template's own derived children and its overrides are
    /// not copied, nor a session node or anything under it (it gets no
    /// copy, `NodeId::expanded`).
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
        .in_scope(doc, def)
    }

    /// Authored repeat (or conditional) `r`'s template: its children that
    /// are not items, and their subtrees, parents first, with the
    /// relations from them. An override is the repeat's own opinion about
    /// one item's copy, not part of what items copy; a session node gets no
    /// copy, nor anything under it, and does not order the items.
    pub(crate) fn repeat(doc: Tree<'_>, r: NodeId) -> Template {
        let mut nodes = Vec::new();
        let mut rels = Vec::new();
        let copied = |c: &NodeId| !c.is_expanded() && !c.is_session();
        let mut stack: Vec<NodeId> = doc.children(r).iter().rev().copied().filter(copied).collect();
        while let Some(n) = stack.pop() {
            let Some(node) = doc.live(n).filter(|x| x.ty.name() != "override") else { continue };
            nodes.push(TNode::new(n, node.parent.unwrap_or(r), node.ty, node.order.clone(), doc.facets_of(n).cloned().collect()));
            rels.extend(doc.relations_from(n).filter(|x| !x.id.is_expanded()).cloned());
            stack.extend(doc.children(n).iter().rev().copied().filter(copied));
        }
        Template::collection(r, nodes, rels).in_scope(doc, r)
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
        Template { scope: outer.scope, stamp: outer.stamp, ..Template::collection(l, nodes, rels) }
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

/// `env`'s overrides of node `n`'s copy: an item's outer owner's first.
fn apply_overrides(t: &Template, n: &TNode, env: &Env, facets: &mut Vec<Facet>) {
    if let Some(item @ Item { outer: Some((outer_t, outer)), .. }) = &env.item {
        apply_overrides(outer_t, n, outer, facets);
        // An outer override naming a node under this template's, as the
        // outer owner copies it, names the item's copy
        // (`Template::into_item`).
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
    for (target, leaf, value) in &env.overrides {
        if *target != n.target {
            continue;
        }
        if !facets.iter().any(|f| f.kind() == leaf.facet) {
            if !n.ty.spec().allows(leaf.facet) {
                continue;
            }
            facets.push(leaf.facet.default_facet());
        }
        if let Some(f) = facets.iter_mut().find(|f| f.kind() == leaf.facet) {
            let _ = f.set(leaf.field, Slot::Const(value.clone()));
        }
    }
}

/// Whether an override reaching `env` (or an owner it is an item of) sets
/// node `n`'s copy to a node.
fn outer_sets_a_node(n: &TNode, env: &Env) -> bool {
    env.overrides.iter().any(|(target, _, v)| *target == n.target && matches!(v, Value::Ref(_)))
        || matches!(&env.item, Some(Item { outer: Some((_, outer)), .. }) if outer_sets_a_node(n, outer))
}

/// The templates compiled so far, kept between passes: each is used while
/// it is current ([`Template::current`]) and compiled again when not.
#[derive(Clone, Default)]
pub(crate) struct TemplateCache {
    bodies: HashMap<NodeId, Arc<Template>>,
    /// A definition without a template for a notation is cached too, with
    /// the definition's stamp.
    pictures: HashMap<(NodeId, String), (u64, Option<Arc<Template>>)>,
    /// Each authored repeat's or conditional's.
    repeats: HashMap<NodeId, Arc<Template>>,
    /// Each sub-template, by the template it is part of (its source and
    /// scope) and its repeat there: a repeat in a definition outside a
    /// library is drawn in place and copied into pictures, two templates
    /// whose sub-templates have the same source and differ in scope.
    subs: HashMap<(Source, NodeId, NodeId), Arc<Template>>,
}

impl std::fmt::Debug for TemplateCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TemplateCache({} bodies, {} pictures, {} repeats, {} sub-templates)", self.bodies.len(), self.pictures.len(), self.repeats.len(), self.subs.len())
    }
}

/// One pass's view of the templates: the kept ones, and each node's own
/// overrides, read once in the pass.
pub(crate) struct Templates<'c> {
    cache: &'c mut TemplateCache,
    own: HashMap<NodeId, Rc<[expand::OwnOverride]>>,
}

impl<'c> Templates<'c> {
    pub(crate) fn new(cache: &'c mut TemplateCache) -> Templates<'c> {
        Templates { cache, own: HashMap::new() }
    }

    pub(crate) fn body(&mut self, doc: Tree<'_>, def: NodeId) -> Arc<Template> {
        match self.cache.bodies.get(&def) {
            Some(t) if t.current(doc) => t.clone(),
            _ => {
                let t = Arc::new(Template::body(doc, def));
                self.cache.bodies.insert(def, t.clone());
                t
            }
        }
    }

    pub(crate) fn picture(&mut self, doc: Tree<'_>, def: NodeId, notation: &str) -> Option<Arc<Template>> {
        let key = (def, notation.to_owned());
        let stamp = doc.scope_stamp(def);
        if let Some((at, t)) = self.cache.pictures.get(&key)
            && *at == stamp
        {
            return t.clone();
        }
        let t = definition::template_of(doc, def, notation).map(|template| Arc::new(Template::picture(doc, def, template)));
        self.cache.pictures.insert(key, (stamp, t.clone()));
        t
    }

    /// Authored repeat `r`'s template.
    pub(crate) fn repeat(&mut self, doc: Tree<'_>, r: NodeId) -> Arc<Template> {
        match self.cache.repeats.get(&r) {
            Some(t) if t.current(doc) => t.clone(),
            _ => {
                let t = Arc::new(Template::repeat(doc, r));
                self.cache.repeats.insert(r, t.clone());
                t
            }
        }
    }

    /// The template of a copy of `outer`'s repeat `l`, current as `outer`
    /// is.
    pub(crate) fn sub(&mut self, outer: &Template, l: NodeId) -> Arc<Template> {
        let key = (outer.source, outer.scope, l);
        match self.cache.subs.get(&key) {
            Some(t) if t.scope == outer.scope && t.stamp == outer.stamp => t.clone(),
            _ => {
                let t = Arc::new(Template::sub(outer, l));
                self.cache.subs.insert(key, t.clone());
                t
            }
        }
    }

    /// The overrides that reach `owner`'s copies, as (target, leaf, value),
    /// weakest first (`expand::overrides_by`), each node's own read once.
    pub(crate) fn overrides(&mut self, doc: Tree<'_>, owner: NodeId) -> Vec<(NodeId, LeafPath, Value)> {
        let own = &mut self.own;
        expand::overrides_by(owner, |n| own.entry(n).or_insert_with(|| expand::own_overrides(doc, n).into()).clone()).into_iter().map(|(_, t, l, v)| (t, l, v)).collect()
    }
}
