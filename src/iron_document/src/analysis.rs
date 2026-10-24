//! Render analysis (component-model.md §11, instance-cache.md §1): what each
//! part of an instance's picture depends on, and so how it can be drawn.
//!
//! Dependencies are static (`Expr::deps`), and the reactive graph holds every
//! one: stored bindings, and those it derives (a connected `in` port reads
//! its partner, an anchor's position, a follow anchor's place). For each part
//! of a `use`'s picture and each **channel** — shape (what it draws),
//! placement (where: its transform and its ancestors'), effects (opacity,
//! shadow) — [`classify`] follows the graph from the part's copy until it
//! reaches one of the instance's `in` ports (an *input*) or a constant.
//! Anything else is *opaque*: a producer's signal (a clip, the across/through
//! network, a host system), an animated slot, a constraint, or a read
//! outside the instance (a follow anchor reading its partner).
//!
//! Each input is then constant at this instance or wired (bound in the
//! parent scope, connected, driven by a producer). A part's channel is
//! **static** (depends on nothing: one drawing serves every instance),
//! **keyed** (depends only on inputs constant here, or is overridden: one
//! drawing serves instances whose relevant values are equal) or **live**.
//! [`runs`] cuts the drawing parts, in painter order, into contiguous runs of
//! one class, which is what a renderer bakes.
//!
//! This is analysis only. Nothing here changes what is drawn; the render
//! track consumes it (instance-cache.md phase C).

use crate::definition;
use crate::document::{Document, RelKind};
use crate::expand;
use crate::expr::Leaf;
use crate::facet::{Args, FacetKind, LeafPath};
use crate::graph::Reactive;
use crate::id::NodeId;
use crate::value::{Slot, Value};
use std::collections::{BTreeMap, BTreeSet};

/// A part of a picture's drawing: what it draws, where, and how.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Channel {
    Shape,
    Placement,
    Effects,
}

impl Channel {
    pub const ALL: [Channel; 3] = [Channel::Shape, Channel::Placement, Channel::Effects];

    pub fn name(self) -> &'static str {
        match self {
            Channel::Shape => "shape",
            Channel::Placement => "placement",
            Channel::Effects => "effects",
        }
    }

    /// The channel a facet's values feed, or `None` for a facet that draws
    /// nothing (a name, a port, an anchor's bookkeeping).
    pub fn of(kind: FacetKind) -> Option<Channel> {
        use FacetKind as K;
        match kind {
            K::Transform2d | K::Transform3d => Some(Channel::Placement),
            K::Appearance | K::Shadow => Some(Channel::Effects),
            K::Name | K::Host | K::Anchor | K::Instance | K::Override | K::Use | K::Template | K::Import | K::Port | K::Equation | K::Connection | K::Composition | K::Constraint | K::Timing | K::Clip | K::Camera | K::EmbedCamera => None,
            _ if kind.is_dynamic() => None,
            _ => Some(Channel::Shape),
        }
    }
}

/// What one channel of a part reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Deps {
    /// The definition's `in` port_specs it reads, through any bindings.
    pub inputs: BTreeSet<NodeId>,
    /// Why it also reads something no input explains.
    pub opaque: BTreeSet<String>,
}

impl Deps {
    fn absorb(&mut self, other: &Deps) {
        self.inputs.extend(other.inputs.iter().copied());
        self.opaque.extend(other.opaque.iter().cloned());
    }
}

/// Node types whose children are part of their own shape, not parts.
fn folds_children(ty: &str) -> bool {
    matches!(ty, "path" | "contour" | "use")
}

fn draws(ty: &str) -> bool {
    matches!(ty, "rect" | "ellipse" | "text" | "path" | "mesh" | "slider" | "embed" | "use")
}

/// A template child that is part of the picture: authored, live, and not
/// bookkeeping (an override, a constraint).
fn part_node(doc: &Document, n: NodeId) -> bool {
    !n.is_expanded() && doc.live(n).is_some_and(|x| !matches!(x.ty.name(), "override" | "constraint" | "clip" | "timeline"))
}

/// Follows the graph from an instance's copies to its inputs, memoised per
/// leaf.
struct Resolver<'a> {
    doc: &'a Document,
    reactive: &'a Reactive,
    /// The `use` and, for a linked instance, its entity: reads inside
    /// either are the instance's own.
    owners: Vec<NodeId>,
    /// The leaf each input arrives on, to its port_spec: the entity's `in`
    /// port copies, or for a main the definition's own port_specs.
    inputs: BTreeMap<Leaf, NodeId>,
    memo: BTreeMap<Leaf, Deps>,
    visiting: BTreeSet<Leaf>,
}

impl Resolver<'_> {
    /// Each channel's dependencies from a node's own facets and the
    /// relations that drive it.
    fn node_channels(&mut self, node: NodeId) -> BTreeMap<Channel, Deps> {
        let mut out: BTreeMap<Channel, Deps> = BTreeMap::new();
        let facets: Vec<(FacetKind, Vec<&'static str>)> = self.doc.facets_of(node).map(|f| (f.kind(), f.field_names())).collect();
        for (kind, fields) in facets {
            let Some(channel) = Channel::of(kind) else { continue };
            for field in fields {
                let Ok(path) = LeafPath::new(kind, field) else { continue };
                let d = self.leaf(Leaf { node, path });
                out.entry(channel).or_default().absorb(&d);
            }
        }
        for r in self.doc.relations_to(node) {
            let why = match r.rel {
                RelKind::Animates => "animated by a clip",
                RelKind::Constrains => "placed by a constraint",
                _ => continue,
            };
            out.entry(Channel::Placement).or_default().opaque.insert(why.to_owned());
            out.entry(Channel::Shape).or_default().opaque.insert(why.to_owned());
        }
        out
    }

    fn leaf(&mut self, leaf: Leaf) -> Deps {
        if let Some(d) = self.memo.get(&leaf) {
            return d.clone();
        }
        if !self.visiting.insert(leaf) {
            return Deps::default();
        }
        let d = self.compute(leaf);
        self.visiting.remove(&leaf);
        self.memo.insert(leaf, d.clone());
        d
    }

    fn compute(&mut self, leaf: Leaf) -> Deps {
        let mut d = Deps::default();
        if let Some(&spec) = self.inputs.get(&leaf) {
            d.inputs.insert(spec);
            return d;
        }
        if self.reactive.is_driven(leaf) {
            d.opaque.insert(format!("driven by a producer ({leaf})"));
            return d;
        }
        if let Some(expr) = self.reactive.expr_of(leaf).cloned() {
            if !self.inside(leaf.node) {
                d.opaque.insert(format!("reads {leaf}, outside the instance"));
                return d;
            }
            for dep in expr.deps() {
                let x = self.leaf(dep);
                d.absorb(&x);
            }
            return d;
        }
        match self.doc.facet(leaf.node, leaf.path.facet).and_then(|f| f.get(leaf.path.field)) {
            Some(Slot::Animated(_)) => {
                d.opaque.insert(format!("animated ({leaf})"));
            }
            // A constant inside the instance is fixed by its definition or
            // an override; outside it, by whatever authored it, which the
            // instance cannot see change.
            Some(Slot::Const(_)) | Some(Slot::Bound(_)) | None if self.inside(leaf.node) => {}
            _ => {
                d.opaque.insert(format!("reads {leaf}, outside the instance"));
            }
        }
        d
    }

    /// Whether `node` is the instance's own: one of its owners or derived
    /// under one.
    fn inside(&self, node: NodeId) -> bool {
        let mut at = Some(node);
        while let Some(n) = at {
            if self.owners.contains(&n) {
                return true;
            }
            at = self.doc.node(n).and_then(|x| x.parent);
        }
        false
    }
}

/// How an instance can draw one channel of a part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    /// Depends on nothing: one drawing serves every instance.
    Static,
    /// Depends only on values constant at this instance: one drawing serves
    /// every instance with equal values.
    Keyed,
    /// Depends on something that may change here: drawn live.
    Live,
}

impl Class {
    pub fn name(self) -> &'static str {
        match self {
            Class::Static => "static",
            Class::Keyed => "keyed",
            Class::Live => "live",
        }
    }
}

/// One of a definition's inputs as an instance sees it.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Constant(Value),
    /// May change here; says why ("bound to …", "connected").
    Wired(String),
}

/// One part of one instance's picture.
#[derive(Clone, Debug, PartialEq)]
pub struct PartClass {
    /// The template node (definition-local).
    pub node: NodeId,
    /// The instance's copy of it.
    pub copy: NodeId,
    /// Whether it draws something itself (a shape, text, a nested picture),
    /// rather than only placing its children (a group, an empty).
    pub draws: bool,
    pub classes: BTreeMap<Channel, Class>,
    /// The values a keyed part's drawing depends on: input names with their
    /// values, and overridden leaves (`#node.fill.color`).
    pub key: BTreeMap<String, Value>,
    /// Why each non-static channel is what it is.
    pub reasons: Vec<String>,
}

impl PartClass {
    /// The part as a whole: live if any channel is, keyed if any is.
    pub fn class(&self) -> Class {
        self.classes.values().copied().max().unwrap_or(Class::Static)
    }

    /// A part whose drawing is fixed and only its placement changes: baked
    /// once and moved (component-model.md §11, "shape and placement are
    /// separate").
    pub fn movable(&self) -> bool {
        self.classes.get(&Channel::Placement) == Some(&Class::Live) && self.classes.iter().all(|(c, k)| *c == Channel::Placement || *k != Class::Live)
    }
}

/// An instance's analysis: its inputs and every part's class, in painter
/// order. Paths' contours and points, and a nested picture's insides, fold
/// into the part that holds them.
#[derive(Clone, Debug, PartialEq)]
pub struct Analysis {
    pub instance: NodeId,
    pub definition: NodeId,
    pub notation: String,
    pub inputs: BTreeMap<String, Input>,
    pub parts: Vec<PartClass>,
}

/// The analysis of `use` `u`, a linked instance or a main; `None` for any
/// other node.
pub fn classify(doc: &Document, reactive: &Reactive, u: NodeId) -> Option<Analysis> {
    let notation = doc.get::<crate::facet::Use>(u)?.notation.constant()?.as_str()?.to_owned();
    let entity = expand::use_entity(doc, u).filter(|&e| expand::linked_definition(doc, e).is_some());
    let def = match entity {
        Some(e) => expand::linked_definition(doc, e)?,
        None => expand::main_definition(doc, u)?,
    };
    let template = definition::template_of(doc, def, &notation)?;
    let specs = definition::inputs_of(doc, def).unwrap_or_default();
    let signal = |node: NodeId| Leaf { node, path: LeafPath::new(FacetKind::Port, "signal").expect("declared field") };
    let mut arrives = BTreeMap::new();
    let mut inputs = BTreeMap::new();
    for spec in &specs {
        let input = match entity {
            None => {
                arrives.insert(signal(spec.spec), spec.spec);
                Input::Constant(spec.default.clone())
            }
            Some(e) => {
                let Some(port) = NodeId::expanded(e, spec.spec) else { continue };
                arrives.insert(signal(port), spec.spec);
                let arg = LeafPath::new(FacetKind::Args, spec.name).ok().map(|path| Leaf { node: e, path });
                let slot = doc.get::<Args>(e).and_then(|a| a.slots.get(spec.name));
                let fed = reactive.expr_of(signal(port)).is_some_and(|x| arg.is_none_or(|a| x.deps() != BTreeSet::from([a])));
                if fed {
                    Input::Wired("connected".to_owned())
                } else if let Some(Slot::Bound(expr)) = slot {
                    Input::Wired(format!("bound to {expr}"))
                } else if arg.is_some_and(|a| reactive.is_driven(a)) || reactive.is_driven(signal(port)) {
                    Input::Wired("driven by a producer".to_owned())
                } else {
                    Input::Constant(slot.and_then(Slot::constant).cloned().unwrap_or_else(|| spec.default.clone()))
                }
            }
        };
        inputs.insert(spec.name.to_owned(), input);
    }
    let name_of = |spec: NodeId| specs.iter().find(|s| s.spec == spec).map_or("?", |s| s.name);
    let mut r = Resolver { doc, reactive, owners: [Some(u), entity].into_iter().flatten().collect(), inputs: arrives, memo: BTreeMap::new(), visiting: BTreeSet::new() };
    let overrides = expand::overrides_of(doc, u);
    let mut parts = Vec::new();
    // (template node, inherited placement, effects, and container shape)
    let fresh = || (Deps::default(), Deps::default(), Deps::default());
    let mut stack: Vec<(NodeId, (Deps, Deps, Deps))> = doc.children(template).iter().rev().filter(|c| part_node(doc, **c)).map(|&c| (c, fresh())).collect();
    while let Some((local, (placement_in, effects_in, shape_in))) = stack.pop() {
        let (Some(n), Some(copy)) = (doc.live(local), NodeId::expanded(u, local)) else { continue };
        if !doc.is_live(copy) {
            continue;
        }
        let ty = n.ty.name();
        let mut own = r.node_channels(copy);
        if folds_children(ty) {
            for d in doc.descendants(copy).into_iter().skip(1) {
                if doc.is_live(d) {
                    for deps in r.node_channels(d).into_values() {
                        own.entry(Channel::Shape).or_default().absorb(&deps);
                    }
                }
            }
        }
        let mut placement = placement_in;
        let mut effects = effects_in;
        let mut shape = shape_in.clone();
        placement.absorb(own.get(&Channel::Placement).unwrap_or(&Deps::default()));
        effects.absorb(own.get(&Channel::Effects).unwrap_or(&Deps::default()));
        shape.absorb(own.get(&Channel::Shape).unwrap_or(&Deps::default()));
        if !folds_children(ty) {
            // A container's own shape facets (its clip) bound its children's.
            let inherited = if draws(ty) { shape_in } else { shape.clone() };
            for &c in doc.children(local).iter().rev().filter(|c| part_node(doc, **c)) {
                stack.push((c, (placement.clone(), effects.clone(), inherited.clone())));
            }
        }
        let mut classes = BTreeMap::new();
        let mut key = BTreeMap::new();
        let mut reasons = Vec::new();
        for (channel, deps) in [(Channel::Shape, &shape), (Channel::Placement, &placement), (Channel::Effects, &effects)] {
            let mut class = Class::Static;
            for why in &deps.opaque {
                class = Class::Live;
                reasons.push(format!("{} {why}", channel.name()));
            }
            let mut constant = Vec::new();
            for &spec in &deps.inputs {
                let name = name_of(spec);
                match inputs.get(name) {
                    Some(Input::Wired(why)) => {
                        class = Class::Live;
                        reasons.push(format!("{} depends on `{name}`, {why}", channel.name()));
                    }
                    Some(Input::Constant(v)) => {
                        class = class.max(Class::Keyed);
                        key.insert(name.to_owned(), v.clone());
                        constant.push(format!("`{name}`"));
                    }
                    None => {}
                }
            }
            if !constant.is_empty() {
                reasons.push(format!("{} depends on {}", channel.name(), constant.join(", ")));
            }
            classes.insert(channel, class);
        }
        // Overrides on this part (or a node it folds in) key its channel.
        for (target, leaf, value) in overrides.iter().filter(|(t, _, _)| *t == local || (folds_children(ty) && descends(doc, *t, local))) {
            let Some(channel) = Channel::of(leaf.facet) else { continue };
            let class = classes.entry(channel).or_insert(Class::Static);
            *class = (*class).max(Class::Keyed);
            key.insert(format!("{target}.{leaf}"), value.clone());
            reasons.push(format!("{} overridden ({leaf})", channel.name()));
        }
        parts.push(PartClass { node: local, copy, draws: draws(ty), classes, key, reasons });
    }
    Some(Analysis { instance: u, definition: def, notation, inputs, parts })
}

/// Whether `n` is a descendant of `of`.
fn descends(doc: &Document, n: NodeId, of: NodeId) -> bool {
    let mut at = doc.node(n).and_then(|x| x.parent);
    while let Some(p) = at {
        if p == of {
            return true;
        }
        at = doc.node(p).and_then(|x| x.parent);
    }
    false
}

/// A contiguous run of drawing parts, in painter order, of one class.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub class: Class,
    pub parts: Vec<NodeId>,
}

/// The drawing parts cut into runs at every change of class. A static run
/// next to a keyed one stays separate: they are shared by different sets of
/// instances.
pub fn runs(analysis: &Analysis) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for p in analysis.parts.iter().filter(|p| p.draws) {
        let class = p.class();
        match out.last_mut() {
            Some(r) if r.class == class => r.parts.push(p.node),
            _ => out.push(Run { class, parts: vec![p.node] }),
        }
    }
    out
}
