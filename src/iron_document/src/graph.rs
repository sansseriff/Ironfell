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
//! Dependencies are static today (`Expr::deps`). Every read during
//! evaluation goes through one resolver, which is where tracked reads would
//! hook in if dynamic dependencies are ever needed.

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

#[derive(Clone, Debug)]
struct BoundNode {
    expr: Expr,
    deps: BTreeSet<Leaf>,
    height: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Reactive {
    nodes: BTreeMap<Leaf, BoundNode>,
    /// Any leaf -> the bound leaves that read it.
    dependents: BTreeMap<Leaf, BTreeSet<Leaf>>,
    resolved: BTreeMap<Leaf, Value>,
    overlay: BTreeMap<Leaf, Value>,
    changes: BTreeMap<Leaf, Value>,
    /// Errors from the last evaluation pass, by leaf. A leaf in error keeps
    /// its previous resolved value.
    pub errors: BTreeMap<Leaf, String>,
    /// Expressions evaluated since construction. Tests use it to check that
    /// the schedule is glitch-free and the cutoff works.
    pub evaluations: u64,
}

impl Reactive {
    /// Value of any leaf as a reader sees it: the gesture overlay first, then
    /// the resolved value of a binding, then the authored constant.
    pub fn read(&self, doc: &Document, leaf: Leaf) -> Option<Value> {
        read_with(&self.overlay, &self.resolved, &self.nodes, doc, leaf)
    }

    pub fn is_bound(&self, leaf: Leaf) -> bool {
        self.nodes.contains_key(&leaf)
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
        for (leaf, expr) in doc.bound_slots() {
            let deps = expr.deps();
            for d in &deps {
                self.dependents.entry(*d).or_default().insert(leaf);
            }
            self.nodes.insert(
                leaf,
                BoundNode {
                    expr: expr.clone(),
                    deps,
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
                    self.changes.insert(leaf, v);
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
            self.changes.insert(leaf, value);
            self.invalidate(doc, [leaf]);
        }
    }

    /// Drop every preview; values fall back to the document.
    pub fn clear_overlay(&mut self, doc: &Document) {
        let leaves: Vec<Leaf> = self.overlay.keys().copied().collect();
        self.overlay.clear();
        for &l in &leaves {
            if let Some(v) = self.read(doc, l) {
                self.changes.insert(l, v);
            }
        }
        self.invalidate(doc, leaves);
    }

    pub fn has_changes(&self) -> bool {
        !self.changes.is_empty()
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

    /// Evaluate `dirty` in height order. A node runs only if it is primed:
    /// a leaf it reads changed since the last pass. Primed-ness spreads from
    /// a node to its dependents only when its own value actually changed,
    /// which is the equality cutoff.
    fn evaluate(&mut self, doc: &Document, dirty: &BTreeSet<Leaf>, primed: &BTreeSet<Leaf>) {
        let mut primed = primed.clone();
        let mut order: Vec<(u32, Leaf)> = dirty.iter().map(|l| (self.nodes[l].height, *l)).collect();
        order.sort();
        for (_, leaf) in order {
            if !primed.contains(&leaf) {
                continue;
            }
            self.evaluations += 1;
            let expr = self.nodes[&leaf].expr.clone();
            let result = {
                let overlay = &self.overlay;
                let resolved = &self.resolved;
                let nodes = &self.nodes;
                expr.eval(&|l| read_with(overlay, resolved, nodes, doc, l))
            };
            match result {
                Ok(v) => {
                    self.errors.remove(&leaf);
                    if self.resolved.get(&leaf) != Some(&v) {
                        self.resolved.insert(leaf, v.clone());
                        self.changes.insert(leaf, v);
                        if let Some(ds) = self.dependents.get(&leaf) {
                            primed.extend(ds.iter().copied());
                        }
                    }
                }
                Err(e) => {
                    self.errors.insert(leaf, e.to_string());
                }
            }
        }
    }
}

fn read_with(
    overlay: &BTreeMap<Leaf, Value>,
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
    doc.leaf(leaf.node, leaf.path)?.constant().cloned()
}
