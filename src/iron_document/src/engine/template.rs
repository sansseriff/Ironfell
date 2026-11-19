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
//!   not items, with their subtrees.
//!
//! Every node a template's facets or relations refer to is classified once
//! ([`Ref`]); an owner resolves each class against its own environment
//! ([`Env`]): its id, the entity a picture figures, whether a picture is
//! sized, an item's key, and the overrides that reach it. So a picture
//! template serves a sized and an unsized `use`, a main and a placed
//! instance alike: the spec's (definition, notation, sized, main) key is
//! (definition, notation) here, with sized and main in the environment.
//!
//! A template lives for one pass: nothing is edited while a pass wants its
//! copies, and the next pass compiles again from the document. Stage 3's
//! slots will need templates kept between passes, with a version.

use crate::definition;
use crate::document::{RelKind, Relation};
use crate::expand;
use crate::expr::{Expr, Leaf};
use crate::facet::{Facet, FacetKind, Instance, LeafPath};
use crate::id::NodeId;
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::{Slot, Value};
use crate::Tree;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    /// A definition's body.
    Body(NodeId),
    /// A definition's template node for one notation.
    Picture(NodeId),
    /// A repeat or conditional.
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
    /// A picture's `use`: the inner entity it figures, whether this
    /// picture expands it (a picture of one of the definition's own inner
    /// entities) and that entity's definition.
    pub(crate) figures: Option<Relation>,
    pub(crate) own: bool,
    pub(crate) inner_def: Option<NodeId>,
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
    /// A collection's items are ordered after this key.
    pub(crate) after: OrderKey,
}

/// One owner, as its template's references resolve against it.
pub(crate) struct Env<'a> {
    /// The entity, `use` or item scope.
    pub(crate) owner: NodeId,
    /// The entity a picture figures; `None` in a main.
    pub(crate) entity: Option<NodeId>,
    pub(crate) sized: bool,
    /// A collection item's key.
    pub(crate) key: &'a str,
    /// The overrides reaching the owner's copies: (target, leaf, value),
    /// weakest first.
    pub(crate) overrides: Vec<(NodeId, LeafPath, Value)>,
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
        TNode { local, parent, ty, order, facets, rewrite: false, arg: None, target: local, rels: Vec::new(), top: false, figures: None, own: false, inner_def: None }
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
        Template { source, nodes, rels, refs, after }
    }

    /// Definition `def`'s body (`expand::want_entity`'s copies).
    pub(crate) fn body(doc: Tree<'_>, def: NodeId) -> Template {
        let inputs = definition::inputs_of(doc, def).unwrap_or_default();
        let port = TypeId::lookup("port").expect("registered");
        let mut nodes = Vec::new();
        for &child in doc.children(def).iter() {
            let Some(node) = doc.live(child) else { continue };
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
        for &machine in doc.children(def).iter().filter(|&&c| doc.live(c).is_some_and(|n| n.ty.name() == "machine")) {
            let mut stack: Vec<NodeId> = doc.children(machine).iter().rev().copied().filter(|c| !c.is_expanded()).collect();
            while let Some(local) = stack.pop() {
                let Some(n) = doc.live(local) else { continue };
                let Some(parent) = n.parent else { continue };
                nodes.push(TNode::new(local, parent, n.ty, n.order.clone(), doc.facets_of(local).cloned().collect()));
                stack.extend(doc.children(local).iter().rev().copied().filter(|c| !c.is_expanded()));
            }
        }
        Template::classify(Source::Body(def), nodes, Vec::new(), OrderKey::first(), |id| expand::is_body(doc, def, id.path()[0]).then_some(Ref::Inside))
    }

    /// Definition `def`'s picture from its template node `template`
    /// (`expand::want_use`'s copies): preorder, each `use` without its
    /// subtree. A template's own derived children and its overrides are
    /// not copied.
    pub(crate) fn picture(doc: Tree<'_>, def: NodeId, template: NodeId) -> Template {
        let inside: BTreeSet<NodeId> = doc.descendants(template).into_iter().filter(|d| !d.is_expanded()).collect();
        let copied = |n: NodeId| !n.is_expanded() && doc.live(n).is_some_and(|x| x.ty.name() != "override");
        let mut nodes = Vec::new();
        let mut stack: Vec<NodeId> = doc.children(template).iter().rev().copied().filter(|&c| copied(c)).collect();
        while let Some(local) = stack.pop() {
            let Some(node) = doc.live(local) else { continue };
            let Some(parent) = node.parent else { continue };
            let mut t = TNode::new(local, parent, node.ty, node.order.clone(), doc.facets_of(local).cloned().collect());
            if node.ty.name() == "use" {
                if let Some(r) = doc.relations_from(local).find(|r| r.rel == RelKind::Figures) {
                    t.own = !inside.contains(&r.to.path()[0]);
                    t.inner_def = doc.node(r.to).and_then(|_| doc.get::<Instance>(r.to)).and_then(|i| {
                        (i.mode.constant().and_then(|v| v.as_str()) == Some("linked")).then(|| i.def.constant().and_then(|v| v.as_ref())).flatten()
                    });
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

    /// Repeat (or conditional) `r`'s template, from its prototype `nodes`
    /// (id, parent, type, order, facets; parents first) and their
    /// relations (`expand::template_of_repeat`).
    pub(crate) fn collection(r: NodeId, nodes: Vec<expand::TemplateNode>, rels: Vec<Relation>) -> Template {
        let after = nodes.iter().filter(|t| t.1 == r).map(|t| t.3.clone()).max().unwrap_or_else(OrderKey::first);
        let inside: BTreeSet<NodeId> = nodes.iter().map(|t| t.0).collect();
        let out: Vec<TNode> = nodes
            .into_iter()
            .map(|(id, parent, ty, order, facets)| {
                let mut t = TNode::new(id, parent, ty, order, facets);
                // An override of an item's copy names the authored node
                // the template node copies.
                t.target = id.path().last().copied().unwrap_or(id);
                t
            })
            .collect();
        Template::classify(Source::Collection(r), out, rels, after, |id| inside.contains(&id).then_some(Ref::Inside))
    }

    /// How `leaf`, as the template has it, reads from `env`'s copy.
    pub(crate) fn follow(&self, env: &Env<'_>, leaf: Leaf) -> Leaf {
        if let Source::Collection(r) = self.source {
            if leaf.node == r {
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
            return match self.refs.get(&leaf.node) {
                Some(Ref::Inside) => NodeId::item(r, env.key, leaf.node).map_or(leaf, |node| Leaf { node, path: leaf.path }),
                _ => leaf,
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

    /// Where relation `rel`'s far end is for `env`'s copy.
    pub(crate) fn follow_node(&self, env: &Env<'_>, node: NodeId) -> NodeId {
        self.follow(env, Leaf { node, path: any_leaf() }).node
    }

    /// Node `i`'s facets as `env`'s copy has them: references resolved,
    /// an `in` port reading the owner's argument, the owner's overrides
    /// applied (component-model.md §5.3).
    pub(crate) fn materialise(&self, i: usize, env: &Env<'_>) -> Vec<Facet> {
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
        facets
    }
}

/// The templates and overrides one pass has read, shared by its owners.
#[derive(Default)]
pub(crate) struct Templates {
    bodies: HashMap<NodeId, Rc<Template>>,
    pictures: HashMap<(NodeId, String), Option<Rc<Template>>>,
    /// Each node's own overrides (`expand::own_overrides`).
    own: HashMap<NodeId, Rc<Vec<(NodeId, NodeId, LeafPath, Value)>>>,
}

impl Templates {
    pub(crate) fn body(&mut self, doc: Tree<'_>, def: NodeId) -> Rc<Template> {
        self.bodies.entry(def).or_insert_with(|| Rc::new(Template::body(doc, def))).clone()
    }

    pub(crate) fn picture(&mut self, doc: Tree<'_>, def: NodeId, notation: &str) -> Option<Rc<Template>> {
        if let Some(t) = self.pictures.get(&(def, notation.to_owned())) {
            return t.clone();
        }
        let t = definition::template_of(doc, def, notation).map(|template| Rc::new(Template::picture(doc, def, template)));
        self.pictures.insert((def, notation.to_owned()), t.clone());
        t
    }

    /// The overrides that reach `owner`'s copies, as (target, leaf, value),
    /// weakest first (`expand::overrides`).
    pub(crate) fn overrides(&mut self, doc: Tree<'_>, owner: NodeId) -> Vec<(NodeId, LeafPath, Value)> {
        let path = owner.path();
        let mut out = Vec::new();
        for j in (0..path.len()).rev() {
            let rest = &path[j + 1..];
            let own = self.own.entry(path[j]).or_insert_with(|| Rc::new(expand::own_overrides(doc, path[j]))).clone();
            for (_, target, leaf, value) in own.iter() {
                let steps = target.path();
                if steps.len() == rest.len() + 1 && steps[..rest.len()] == *rest {
                    out.push((steps[rest.len()], *leaf, value.clone()));
                }
            }
        }
        out
    }
}
