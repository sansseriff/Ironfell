//! The reactive graph: bound slots, their dependencies, and their resolved
//! values.
//!
//! Evaluation is height-ordered with equality cutoff, so a diamond evaluates
//! each node once and an unchanged intermediate stops propagation. Structure
//! is rebuilt only when a transaction changed bindings or nodes; a plain
//! value change walks the reverse edges from the changed leaves.
//!
//! The graph also holds the *gesture overlay*: values previewed during a
//! drag, read in place of authored constants by every evaluation, and turned
//! into one transaction on commit. This is doc 04 §3.5's coalescing buffer
//! living in the store, so the manipulation loop never leaves the process
//! (doc 02 invariant 8).
//!
//! Dependencies are static (`Expr::deps`), with one exception: a **tracked
//! read**, `deref(node, 'facet.field')`, whose target is a value
//! (plans/specs/reactivity.md §2). A binding that holds one records what it
//! read each time it is evaluated; those leaves are its dependencies until
//! the next evaluation, as Svelte's and Solid's are. A leaf it starts to
//! read may be evaluated after it in the same pass, in which case it is
//! evaluated again; a tracked read that reaches its own binding is an
//! error on that leaf, since the applier cannot refuse it ahead of time.

use crate::document::Document;
use crate::expr::{Expr, Leaf};
use crate::value::Value;
use std::collections::{BTreeMap, BTreeSet};

/// A leaf whose resolved value changed. Consumers (the reconciler) rewrite
/// whatever they derived from it.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedChange {
    pub leaf: Leaf,
    pub value: Value,
}

/// Who publishes a runtime signal. Three producers are built in; a host
/// registers more through `Store::register_producer`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProducerId(pub u32);

impl ProducerId {
    /// Clips sampled at the playhead (`time.rs`).
    pub const CLIPS: ProducerId = ProducerId(0);
    /// The across/through network's solution (`Store::resolve_physics`).
    pub const NETWORK: ProducerId = ProducerId(1);
    /// Where groups' layouts place their children (`layout.rs`).
    pub const LAYOUT: ProducerId = ProducerId(2);
    /// Component state machines' channels (`machine.rs`).
    pub const MACHINES: ProducerId = ProducerId(3);
    /// Colour tokens' values for the theme in use (palette.rs).
    pub const THEME: ProducerId = ProducerId(4);
    /// Repeat items' values and indices (`Store::settle_repeats`).
    pub const ITEMS: ProducerId = ProducerId(5);
    /// Session state cells (`let.keep = session`; `Store::set_state`).
    pub const STATE: ProducerId = ProducerId(6);
    /// What is selected, on nodes that ask (`selection` facet).
    pub const SELECTION: ProducerId = ProducerId(7);
    /// The first id a host producer receives.
    /// Traces and accumulators (step.rs).
    pub const STEPS: ProducerId = ProducerId(8);
    /// Queries' status and answers (query.rs).
    pub const QUERIES: ProducerId = ProducerId(9);
    /// Items entering and leaving (`Store::advance_presence`).
    pub const PRESENCE: ProducerId = ProducerId(10);
    /// What the last drag or wheel event carried, on its `on` nodes.
    pub const EVENTS: ProducerId = ProducerId(11);
    pub const FIRST_HOST: ProducerId = ProducerId(12);
}

/// One published value: who published it and at which of their frames.
#[derive(Clone, Debug, PartialEq)]
pub struct Signal {
    pub value: Value,
    pub producer: ProducerId,
    pub stamp: u64,
}

#[derive(Clone, Debug)]
struct BoundNode {
    expr: Expr,
    deps: BTreeSet<Leaf>,
    /// What each `@name` it reads resolved to when the graph was built.
    ctx: BTreeMap<String, Leaf>,
    /// Whether the expression holds a tracked read (`deref`).
    tracks: bool,
    /// What its tracked reads read at the last evaluation, beyond `deps`.
    tracked: BTreeSet<Leaf>,
    height: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Reactive {
    nodes: BTreeMap<Leaf, BoundNode>,
    /// Any leaf -> the bound leaves that read it.
    dependents: BTreeMap<Leaf, BTreeSet<Leaf>>,
    resolved: BTreeMap<Leaf, Value>,
    overlay: BTreeMap<Leaf, Value>,
    /// The runtime layer (vision 10 §4): named signals published by
    /// producers, read beneath bindings and above authored constants, never
    /// serialised. Clips (`time.rs`) and the across/through network
    /// (`Store::resolve_physics`) are two producers; a host system (a Bevy
    /// simulation, a Model) is another. A leaf claimed by two producers keeps
    /// the first claim and reports the second (`signal_errors`).
    runtime: BTreeMap<Leaf, Signal>,
    /// Which leaves each producer currently holds, for whole-set republish.
    held: BTreeMap<ProducerId, BTreeSet<Leaf>>,
    /// Leaves a producer asked for while another held them, by producer.
    signal_errors: BTreeMap<Leaf, String>,
    changes: BTreeMap<Leaf, Value>,
    /// Every leaf whose resolved value moved since `take_touched`. Unlike
    /// `changes`, which the reconciler drains, this feeds the store's
    /// geometry revisions (`geom::Revisions`), so the two consumers never
    /// starve each other.
    touched: BTreeSet<Leaf>,
    /// Bindings that failed, by leaf, with why. A leaf in error keeps its
    /// last good value for whoever draws it (`read`), but a binding that
    /// reads it fails too, "depends on …, which failed" (reactivity.md
    /// §4.1): nothing is computed from a failed input as if it were good.
    pub errors: BTreeMap<Leaf, String>,
    /// Expressions evaluated since construction. Tests use it to check that
    /// the schedule is glitch-free and the cutoff works.
    pub evaluations: u64,
}

impl Reactive {
    /// Value of any leaf as a reader sees it: the gesture overlay first, then
    /// the resolved value of a binding, then the authored constant.
    pub fn read(&self, doc: &Document, leaf: Leaf) -> Option<Value> {
        read_with(&self.overlay, &self.runtime, &self.resolved, &self.nodes, doc, leaf)
    }

    /// The runtime layer as published.
    pub fn signals(&self) -> &BTreeMap<Leaf, Signal> {
        &self.runtime
    }

    /// The signal on a leaf, if a producer holds it.
    pub fn signal(&self, leaf: Leaf) -> Option<&Signal> {
        self.runtime.get(&leaf)
    }

    /// Whether some producer currently drives this leaf.
    pub fn is_driven(&self, leaf: Leaf) -> bool {
        self.runtime.contains_key(&leaf)
    }

    /// Whether this producer currently drives this leaf.
    pub fn driven_by(&self, leaf: Leaf, producer: ProducerId) -> bool {
        self.runtime.get(&leaf).is_some_and(|s| s.producer == producer)
    }

    /// The leaves a producer holds, with their values.
    pub fn published_by(&self, producer: ProducerId) -> BTreeMap<Leaf, Value> {
        self.held.get(&producer).map(|ls| ls.iter().filter_map(|l| self.runtime.get(l).map(|s| (*l, s.value.clone()))).collect()).unwrap_or_default()
    }

    /// Leaves a producer asked for while another held them, with why.
    pub fn signal_errors(&self) -> &BTreeMap<Leaf, String> {
        &self.signal_errors
    }

    /// Replace everything `producer` publishes with `values`: leaves it no
    /// longer names are released, leaves another producer holds are refused
    /// and reported. Leaves whose readable value moved are noted and their
    /// dependents re-evaluated, as for a constant that was set.
    pub fn publish(&mut self, doc: &Document, producer: ProducerId, values: BTreeMap<Leaf, Value>, stamp: u64) {
        let mut changed: Vec<Leaf> = Vec::new();
        let was: BTreeSet<Leaf> = self.held.remove(&producer).unwrap_or_default();
        let mut now: BTreeSet<Leaf> = BTreeSet::new();
        for (leaf, value) in values {
            if self.nodes.contains_key(&leaf) {
                self.signal_errors.insert(leaf, format!("{leaf} is bound; producer {} cannot drive it", producer.0));
                continue;
            }
            match self.runtime.get(&leaf) {
                Some(s) if s.producer != producer => {
                    self.signal_errors.insert(leaf, format!("{leaf} is already driven by producer {}; producer {} cannot also drive it", s.producer.0, producer.0));
                    continue;
                }
                Some(s) if s.value == value => {
                    self.runtime.insert(leaf, Signal { value, producer, stamp });
                }
                _ => {
                    self.runtime.insert(leaf, Signal { value, producer, stamp });
                    changed.push(leaf);
                }
            }
            self.signal_errors.remove(&leaf);
            now.insert(leaf);
        }
        for leaf in was.difference(&now) {
            if self.runtime.get(leaf).is_some_and(|s| s.producer == producer) {
                self.runtime.remove(leaf);
                changed.push(*leaf);
            }
        }
        // Another producer's errors on leaves this one released may now be
        // stale; they clear on that producer's next publish.
        if !now.is_empty() {
            self.held.insert(producer, now);
        }
        if changed.is_empty() {
            return;
        }
        for &l in &changed {
            // Only visible when no gesture is previewing this leaf.
            if !self.overlay.contains_key(&l)
                && let Some(v) = self.read(doc, l)
            {
                self.note(l, v);
            }
        }
        self.invalidate(doc, changed);
    }

    /// Release everything `producer` publishes.
    pub fn retract(&mut self, doc: &Document, producer: ProducerId) {
        self.publish(doc, producer, BTreeMap::new(), 0);
    }

    /// Whether any binding reads `leaf`.
    pub fn is_read(&self, leaf: Leaf) -> bool {
        self.dependents.get(&leaf).is_some_and(|d| !d.is_empty())
    }

    pub fn is_bound(&self, leaf: Leaf) -> bool {
        self.nodes.contains_key(&leaf)
    }

    /// The expression a leaf resolves through: its binding, or the signal a
    /// connection derives for it.
    pub fn expr_of(&self, leaf: Leaf) -> Option<&Expr> {
        self.nodes.get(&leaf).map(|n| &n.expr)
    }

    /// Every leaf `leaf` reads, directly or through other bindings.
    pub fn transitive_deps(&self, leaf: Leaf) -> BTreeSet<Leaf> {
        let mut out = BTreeSet::new();
        let mut stack = vec![leaf];
        while let Some(l) = stack.pop() {
            if let Some(n) = self.nodes.get(&l) {
                for d in n.deps.iter().chain(&n.tracked) {
                    if out.insert(*d) {
                        stack.push(*d);
                    }
                }
            }
        }
        out
    }

    pub fn height(&self, leaf: Leaf) -> Option<u32> {
        self.nodes.get(&leaf).map(|n| n.height)
    }

    pub fn overlay(&self) -> &BTreeMap<Leaf, Value> {
        &self.overlay
    }

    /// Rebuild the structure from the document and evaluate every binding.
    /// Called after transactions that created, deleted, or rebound anything.
    pub fn rebuild(&mut self, doc: &Document) {
        let old_bound: BTreeSet<Leaf> = self.nodes.keys().copied().collect();
        self.nodes.clear();
        self.dependents.clear();
        // Stored bindings, then the signals connections derive (`port`):
        // an in-port reads its partner, evaluated like any binding; then
        // anchors' positions and followers' placements (`anchor`).
        let mut derived = crate::port::derived_signals(doc);
        derived.extend(crate::anchor::derived(doc));
        let tokens = crate::palette::leaves(doc);
        for (leaf, expr) in doc.bound_slots().chain(derived.iter().map(|(l, e)| (*l, e))) {
            let mut deps = expr.deps();
            // `@name` reads the nearest provider above this node, a colour
            // token at the root (palette.rs).
            let ctx: BTreeMap<String, Leaf> =
                expr.tokens().into_iter().filter_map(|n| crate::palette::provided(doc, leaf.node, &n, &tokens).map(|l| (n, l))).collect();
            deps.extend(ctx.values().copied());
            for d in &deps {
                self.dependents.entry(*d).or_default().insert(leaf);
            }
            let mut tracks = false;
            expr.walk(&mut |e| tracks |= matches!(e, Expr::Call(name, _) if name == "deref"));
            self.nodes.insert(
                leaf,
                BoundNode {
                    expr: expr.clone(),
                    deps,
                    ctx,
                    tracks,
                    tracked: BTreeSet::new(),
                    height: 0,
                },
            );
        }
        self.compute_heights();
        // A slot that stopped being bound reads as its constant again; say so.
        for leaf in old_bound {
            if !self.nodes.contains_key(&leaf) {
                self.resolved.remove(&leaf);
                self.errors.remove(&leaf);
                if let Some(v) = self.read(doc, leaf) {
                    self.note(leaf, v);
                }
            }
        }
        let all: BTreeSet<Leaf> = self.nodes.keys().copied().collect();
        self.evaluate(doc, &all, &all);
    }

    /// The values of `changed` leaves moved (a constant was set, or the
    /// overlay changed). Re-evaluate what depends on them, in height order.
    pub fn invalidate(&mut self, doc: &Document, changed: impl IntoIterator<Item = Leaf>) {
        let mut primed = BTreeSet::new();
        let mut dirty = BTreeSet::new();
        let mut stack: Vec<Leaf> = changed.into_iter().collect();
        for l in &stack {
            if let Some(ds) = self.dependents.get(l) {
                primed.extend(ds.iter().copied());
            }
        }
        while let Some(l) = stack.pop() {
            if let Some(ds) = self.dependents.get(&l) {
                for d in ds {
                    if dirty.insert(*d) {
                        stack.push(*d);
                    }
                }
            }
        }
        self.evaluate(doc, &dirty, &primed);
    }

    /// Preview a value without writing the document.
    pub fn preview(&mut self, doc: &Document, leaf: Leaf, value: Value) {
        let before = self.read(doc, leaf);
        self.overlay.insert(leaf, value.clone());
        if before.as_ref() != Some(&value) {
            self.note(leaf, value);
            self.invalidate(doc, [leaf]);
        }
    }

    /// Drop one leaf's preview; it falls back to the document.
    pub fn unpreview(&mut self, doc: &Document, leaf: Leaf) {
        if self.overlay.remove(&leaf).is_some() {
            if let Some(v) = self.read(doc, leaf) {
                self.note(leaf, v);
            }
            self.invalidate(doc, [leaf]);
        }
    }

    /// Drop every preview; values fall back to the document.
    pub fn clear_overlay(&mut self, doc: &Document) {
        let leaves: Vec<Leaf> = self.overlay.keys().copied().collect();
        self.overlay.clear();
        for &l in &leaves {
            // A leaf of a facet the node lacks (a gesture that would have
            // added it) reads nothing now: report its default, so whatever
            // drew the preview redraws without it.
            let v = self.read(doc, l).or_else(|| l.path.facet.default_facet().get(l.path.field).and_then(|s| s.constant()).cloned());
            if let Some(v) = v {
                self.note(l, v);
            }
        }
        self.invalidate(doc, leaves);
    }

    pub fn has_changes(&self) -> bool {
        !self.changes.is_empty()
    }

    /// Report a leaf as changed with no change to its value: something it
    /// derives (a text's glyphs, when a font arrives) moved.
    pub(crate) fn mark(&mut self, leaf: Leaf, value: Value) {
        self.note(leaf, value);
    }

    /// The one place a resolved change is recorded.
    fn note(&mut self, leaf: Leaf, value: Value) {
        self.touched.insert(leaf);
        self.changes.insert(leaf, value);
    }

    /// Leaves whose resolved value moved since the last take.
    pub fn take_touched(&mut self) -> BTreeSet<Leaf> {
        std::mem::take(&mut self.touched)
    }

    pub fn drain_changes(&mut self) -> Vec<ResolvedChange> {
        std::mem::take(&mut self.changes)
            .into_iter()
            .map(|(leaf, value)| ResolvedChange { leaf, value })
            .collect()
    }

    fn compute_heights(&mut self) {
        // Relaxation to a fixpoint. The applier keeps the graph acyclic, so
        // this converges within `nodes.len()` rounds; the bound is a guard.
        for _ in 0..=self.nodes.len() {
            let mut changed = false;
            let keys: Vec<Leaf> = self.nodes.keys().copied().collect();
            for k in keys {
                let h = 1 + self.nodes[&k]
                    .deps
                    .iter()
                    .chain(&self.nodes[&k].tracked)
                    .filter_map(|d| self.nodes.get(d).map(|n| n.height))
                    .max()
                    .unwrap_or(0);
                let node = self.nodes.get_mut(&k).expect("key");
                if node.height != h {
                    node.height = h;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Whether `from` reads `target`, directly or through other bindings.
    fn reaches(&self, from: Leaf, target: Leaf) -> bool {
        let mut seen = BTreeSet::new();
        let mut stack = vec![from];
        while let Some(l) = stack.pop() {
            if l == target {
                return true;
            }
            if let Some(n) = self.nodes.get(&l) {
                stack.extend(n.deps.iter().chain(&n.tracked).filter(|d| seen.insert(**d)).copied());
            }
        }
        false
    }

    /// Evaluate `dirty` in height order. A node runs only if it is primed:
    /// a leaf it reads changed since the last pass. Primed-ness spreads from
    /// a node to its dependents only when its own value actually changed,
    /// which is the equality cutoff. A binding with a tracked read may find
    /// it depends on a leaf evaluated after it; when that leaf changes, the
    /// binding is queued again.
    fn evaluate(&mut self, doc: &Document, dirty: &BTreeSet<Leaf>, primed: &BTreeSet<Leaf>) {
        let mut primed = primed.clone();
        let mut queue: BTreeSet<(u32, Leaf)> = dirty.iter().map(|l| (self.nodes[l].height, *l)).collect();
        // Static dependencies never queue a node twice; tracked ones may, a
        // bounded number of times in an acyclic graph. The limit is a guard.
        let mut budget = dirty.len().saturating_mul(4) + 1024;
        while let Some((_, leaf)) = queue.pop_first() {
            if !primed.remove(&leaf) {
                continue;
            }
            if budget == 0 {
                self.errors.insert(leaf, "did not settle: tracked reads keep changing each other".into());
                continue;
            }
            budget -= 1;
            self.evaluations += 1;
            let expr = self.nodes[&leaf].expr.clone();
            let tracks = self.nodes[&leaf].tracks;
            let _ctx = crate::expr::TokenScope::new(self.nodes[&leaf].ctx.clone());
            let reads: std::cell::RefCell<BTreeSet<Leaf>> = Default::default();
            let result = {
                let overlay = &self.overlay;
                let runtime = &self.runtime;
                let resolved = &self.resolved;
                let nodes = &self.nodes;
                let errors = &self.errors;
                // The first failed input this binding read, if any.
                let upstream: std::cell::Cell<Option<Leaf>> = std::cell::Cell::new(None);
                let read = |l| {
                    if tracks {
                        reads.borrow_mut().insert(l);
                    }
                    if errors.contains_key(&l) && !overlay.contains_key(&l) {
                        if upstream.get().is_none() {
                            upstream.set(Some(l));
                        }
                        return None;
                    }
                    read_with(overlay, runtime, resolved, nodes, doc, l)
                };
                let out = expr.eval(&read).and_then(|v| as_leaf_kind(leaf, v)).and_then(|v| match crate::types::declared(doc, leaf) {
                    // A boundary leaf with a declared type checks what it
                    // computes or receives (types-and-units.md §3.3, §3.5).
                    Some((ty, owner)) => ty
                        .check(&v, &|name| crate::facet::LeafPath::new(crate::facet::FacetKind::Args, name).ok().and_then(|path| read(Leaf { node: owner, path })))
                        .map_err(|e| crate::expr::EvalError::Type(format!("{}: {e}", boundary_name(doc, leaf)))),
                    None => Ok(v),
                });
                match (out, upstream.get()) {
                    (Err(_), Some(failed)) => {
                        // Name the input at the root of the failure.
                        let why = errors.get(&failed).map(String::as_str).unwrap_or("");
                        let root = why.strip_prefix("depends on ").unwrap_or("");
                        Err(if root.is_empty() { format!("depends on {failed}, which failed: {why}") } else { format!("depends on {root}") })
                    }
                    (Err(e), None) => Err(e.to_string()),
                    (Ok(v), _) => Ok(v),
                }
            };
            // What the tracked reads read this time becomes what this
            // binding depends on, beyond its static dependencies.
            let mut result = result;
            if tracks {
                let node = &self.nodes[&leaf];
                let tracked: BTreeSet<Leaf> = reads.into_inner().into_iter().filter(|l| !node.deps.contains(l)).collect();
                if tracked != node.tracked {
                    let old = node.tracked.clone();
                    if let Some(own) = tracked.iter().find(|t| **t == leaf || self.reaches(**t, leaf)) {
                        result = Err(format!("reads {own} through deref, which depends on this leaf: a cycle"));
                    } else {
                        for gone in old.difference(&tracked) {
                            if let Some(ds) = self.dependents.get_mut(gone) {
                                ds.remove(&leaf);
                            }
                        }
                        for new in tracked.difference(&old) {
                            self.dependents.entry(*new).or_default().insert(leaf);
                        }
                        // A newly read binding still waiting in this pass:
                        // read it again once it has been evaluated.
                        let waiting = tracked.difference(&old).any(|t| primed.contains(t) && self.nodes.contains_key(t));
                        // Heights move only when it now reads something
                        // at or above its own.
                        let need = 1 + tracked.iter().filter_map(|t| self.nodes.get(t).map(|n| n.height)).max().unwrap_or(0);
                        let node = self.nodes.get_mut(&leaf).expect("bound");
                        node.tracked = tracked;
                        if need > node.height {
                            self.compute_heights();
                        }
                        if waiting {
                            primed.insert(leaf);
                            queue.insert((self.nodes[&leaf].height, leaf));
                            continue;
                        }
                    }
                }
            }
            // Readers run again when this value changes, and when it starts
            // or stops failing (a recovery to the same value still clears
            // their errors).
            let changed = match result {
                Ok(v) => {
                    let recovered = self.errors.remove(&leaf).is_some();
                    if self.resolved.get(&leaf) != Some(&v) {
                        self.resolved.insert(leaf, v.clone());
                        self.note(leaf, v);
                        true
                    } else {
                        recovered
                    }
                }
                Err(e) => self.errors.insert(leaf, e).is_none(),
            };
            if changed && let Some(ds) = self.dependents.get(&leaf) {
                for d in ds {
                    primed.insert(*d);
                    // A reader by a tracked read may already have run.
                    if let Some(n) = self.nodes.get(d) {
                        queue.insert((n.height, *d));
                    }
                }
            }
        }
    }
}

fn read_with(
    overlay: &BTreeMap<Leaf, Value>,
    runtime: &BTreeMap<Leaf, Signal>,
    resolved: &BTreeMap<Leaf, Value>,
    nodes: &BTreeMap<Leaf, BoundNode>,
    doc: &Document,
    leaf: Leaf,
) -> Option<Value> {
    if let Some(v) = overlay.get(&leaf) {
        return Some(v.clone());
    }
    if nodes.contains_key(&leaf) {
        return resolved.get(&leaf).cloned();
    }
    if let Some(s) = runtime.get(&leaf) {
        return Some(s.value.clone());
    }
    doc.leaf(leaf.node, leaf.path)?.constant().cloned()
}

/// A bound value as its leaf holds it: SVG path data on a geometry leaf
/// (`path.d`) becomes geometry (plans/specs/plot.md §2), and a quoted word
/// on a text leaf is text.
fn as_leaf_kind(leaf: Leaf, v: Value) -> Result<Value, crate::expr::EvalError> {
    match v {
        // Text on a path is SVG path data.
        Value::Str(text) | Value::Enum(text) if leaf.path.kind() == crate::value::ValueKind::Geometry => crate::geometry::Geometry::from_svg_path(&text)
            .map(|g| Value::Geometry(std::sync::Arc::new(g)))
            .map_err(crate::expr::EvalError::Type),
        // Text on a choice leaf names the option (`{= 'evenodd'}`), and a
        // choice on a text leaf is its word.
        Value::Str(text) if leaf.path.kind() == crate::value::ValueKind::Enum => Ok(Value::Enum(text)),
        Value::Enum(text) if leaf.path.kind() == crate::value::ValueKind::Str => Ok(Value::Str(text)),
        v => Ok(v),
    }
}

/// How a boundary leaf is named in its type error: an argument by its
/// name, a port's signal by the port's name.
fn boundary_name(doc: &Document, leaf: Leaf) -> String {
    match leaf.path.facet {
        crate::facet::FacetKind::Port => doc.get::<crate::facet::Name>(leaf.node).and_then(|n| n.text.constant()).and_then(|v| v.as_str()).unwrap_or("port").to_owned(),
        _ => leaf.path.field.to_owned(),
    }
}
