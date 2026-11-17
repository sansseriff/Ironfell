//! Owners (reactive-engine.md §5.3, §7.2; handoff §4.4, stage 2): each
//! authored entity, `use` and drawn repeat or conditional owns the derived
//! nodes whose path starts with it. An op expands again only the owners it
//! can change, where the old engine re-expanded the whole document.
//!
//! What an owner holds is read from the document (its derived
//! descendants), so a refused transaction, rolled back by the journal,
//! leaves nothing here to undo but what `Delta` would have applied. The
//! index from definitions to owners only grows: an owner it names that no
//! longer reads the definition is expanded again for nothing.

use crate::expand::{self, Items, Problem};
use crate::facet::FacetKind;
use crate::op::Op;
use crate::{Document, NodeId, RelKind, Tree, Version};
use std::collections::{BTreeMap, BTreeSet};

/// Which owners to expand again.
pub(crate) enum Which {
    /// Every owner, as the old engine does on load and on what this layer
    /// does not map (a reparent, a definition made or removed).
    All,
    Some(BTreeSet<NodeId>),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Owners {
    /// Each owner's expansion problems.
    problems: BTreeMap<NodeId, Vec<Problem>>,
    /// How many derived nodes each owner wanted: the budget's share.
    counts: BTreeMap<NodeId, usize>,
    /// Definition -> owners whose expansion read it. Only grows.
    by_def: BTreeMap<NodeId, BTreeSet<NodeId>>,
    /// Owner -> owners whose pictures figure its derived entities. Only
    /// grows.
    readers: BTreeMap<NodeId, BTreeSet<NodeId>>,
}

/// What an expansion would change here, applied only once the transaction
/// that made it is accepted.
pub(crate) struct Delta {
    all: bool,
    problems: BTreeMap<NodeId, Vec<Problem>>,
    counts: BTreeMap<NodeId, usize>,
    defs: Vec<(NodeId, NodeId)>,
    reads: Vec<(NodeId, NodeId)>,
}

impl Delta {
    fn add(&mut self, d: expand::OwnersExpanded) {
        self.problems.extend(d.problems);
        self.counts.extend(d.counts);
        self.defs.extend(d.defs);
        self.reads.extend(d.reads);
    }

    fn has_budget_problem(&self) -> bool {
        self.problems.values().flatten().any(|p| p.kind == "budget")
    }
}

impl Owners {
    /// Expand `which` again in `doc`: the ops applied, and what to record.
    pub(crate) fn expand(&self, doc: &mut Document, stamp: Version, counts: &Items, which: Which) -> (Vec<Op>, Delta) {
        let Which::Some(mut todo) = which else { return self.expand_all(doc, stamp, counts) };
        // Owners whose last expansion failed in part are tried again: what
        // failed may depend on something outside them that has changed.
        todo.extend(self.problems.iter().filter(|(_, ps)| ps.iter().any(|p| matches!(p.kind, "expansion" | "budget"))).map(|(o, _)| *o));
        let mut delta = Delta { all: false, problems: BTreeMap::new(), counts: BTreeMap::new(), defs: Vec::new(), reads: Vec::new() };
        let mut ops = Vec::new();
        let mut done: BTreeSet<NodeId> = BTreeSet::new();
        // Each wave expands the owners whose pictures read what the last one
        // changed.
        while !todo.is_empty() {
            let held: usize = self.counts.iter().chain(delta.counts.iter()).filter(|(o, _)| !todo.contains(o)).map(|(o, n)| delta.counts.get(o).copied().unwrap_or(*n)).sum::<usize>();
            let held = held.min(expand::NODE_BUDGET);
            let wave = expand::expand_owners(doc, stamp, counts, &todo, expand::NODE_BUDGET.saturating_sub(held));
            ops.extend(std::mem::take(&mut { wave.ops.clone() }));
            delta.add(expand::OwnersExpanded { ops: Vec::new(), ..wave });
            done.extend(todo.iter().copied());
            let reading = |o: &NodeId| self.readers.get(o).into_iter().flatten().copied().chain(delta.reads.iter().filter(move |(r, _)| r == o).map(|(_, w)| *w)).collect::<Vec<_>>();
            todo = done.iter().flat_map(reading).filter(|o| !done.contains(o)).collect();
        }
        // Past the budget the cut must not depend on edit history: expand
        // everything, in id order.
        if delta.has_budget_problem() || self.problems.values().flatten().any(|p| p.kind == "budget") {
            let (more, all) = self.expand_all(doc, stamp, counts);
            ops.extend(more);
            return (ops, all);
        }
        (ops, delta)
    }

    /// Every owner, in id order.
    fn expand_all(&self, doc: &mut Document, stamp: Version, counts: &Items) -> (Vec<Op>, Delta) {
        let owners = expand::top_owners(Tree::over(doc));
        let mut done = expand::expand_owners(doc, stamp, counts, &owners, expand::NODE_BUDGET);
        let ops = std::mem::take(&mut done.ops);
        let mut delta = Delta { all: true, problems: BTreeMap::new(), counts: BTreeMap::new(), defs: Vec::new(), reads: Vec::new() };
        delta.add(done);
        (ops, delta)
    }

    /// Record what an accepted expansion found.
    pub(crate) fn apply(&mut self, d: Delta) {
        if d.all {
            self.problems.clear();
            self.counts.clear();
        }
        for (o, p) in d.problems {
            if p.is_empty() {
                self.problems.remove(&o);
            } else {
                self.problems.insert(o, p);
            }
        }
        for (o, n) in d.counts {
            if n == 0 {
                self.counts.remove(&o);
            } else {
                self.counts.insert(o, n);
            }
        }
        for (def, o) in d.defs {
            self.by_def.entry(def).or_default().insert(o);
        }
        for (read, o) in d.reads {
            self.readers.entry(read).or_default().insert(o);
        }
    }

    /// Every owner's problems, in the order `expand::sort_problems` gives.
    pub(crate) fn problems(&self) -> Vec<Problem> {
        let mut out: Vec<Problem> = self.problems.values().flatten().cloned().collect();
        expand::sort_problems(&mut out);
        out
    }

    /// Which owners a transaction's applied `ops` can change (the spec's
    /// §7.2, and what `expand::wanted` covers that the table does not), or
    /// `None` when none. `inverse` is everything the ops took or replaced,
    /// derived nodes included.
    pub(crate) fn affected(&self, doc: Tree<'_>, ops: &[Op], inverse: &[Op]) -> Option<Which> {
        let mut out: BTreeSet<NodeId> = BTreeSet::new();
        let owner_kind = |id: NodeId| -> bool {
            doc.node(id).is_some_and(|n| matches!(n.ty.name(), "entity" | "use") || (n.ty.name() == "group" && expand::is_item_owner(doc, id)))
        };
        // The node itself and every enclosing owner (an override's, a
        // repeat template child's), and the owners of the definition it is in.
        let touch = |id: NodeId, out: &mut BTreeSet<NodeId>| -> bool {
            if id.is_expanded() || id.is_session() {
                return false;
            }
            let mut at = Some(id);
            while let Some(n) = at {
                if owner_kind(n) || n == id {
                    out.insert(n);
                }
                at = doc.node(n).and_then(|x| x.parent);
            }
            // A definition anywhere, in a library or at the root.
            match expand::definition_of(doc, id) {
                Some(def) => out.extend(self.by_def.get(&def).into_iter().flatten().copied()),
                // The library itself, or something in it outside any
                // definition.
                None if expand::in_library(doc, id) => return true,
                None => {}
            }
            false
        };
        // An entity's pictures follow it: each picture, and the owners
        // around it (a picture in a repeat's template is copied into its
        // items). A derived picture is its top owner's.
        let pictures = |entity: NodeId, out: &mut BTreeSet<NodeId>| -> bool {
            let mut all = false;
            for u in doc.relations_to(entity).filter(|r| r.rel == RelKind::Figures).map(|r| r.from).collect::<Vec<_>>() {
                if u.is_expanded() {
                    out.extend(u.path().first().copied());
                } else {
                    all |= touch(u, out);
                }
            }
            all
        };
        let is_structure_type = |id: NodeId| doc.node(id).is_some_and(|n| matches!(n.ty.name(), "definition" | "library"));
        let mut all = false;
        for op in ops {
            match op {
                Op::Create { id, .. } | Op::Delete { id } => {
                    all |= is_structure_type(*id);
                    all |= touch(*id, &mut out);
                    all |= pictures(*id, &mut out);
                }
                Op::AddFacet { id, facet } => {
                    all |= expand::owns_items(facet.kind()) || is_structure_type(*id);
                    all |= touch(*id, &mut out);
                    all |= pictures(*id, &mut out);
                }
                Op::RemoveFacet { id, kind } => {
                    all |= expand::owns_items(*kind) || is_structure_type(*id);
                    all |= touch(*id, &mut out);
                    all |= pictures(*id, &mut out);
                }
                Op::Set { id, path, .. } => {
                    let shapes = matches!(path.facet, FacetKind::Instance | FacetKind::Use | FacetKind::Override) || expand::in_library(doc, *id) || expand::in_repeat_template(doc, *id);
                    if shapes {
                        all |= touch(*id, &mut out);
                        if path.facet == FacetKind::Instance {
                            all |= pictures(*id, &mut out);
                        }
                    }
                }
                Op::Reorder { id, .. } => {
                    all |= touch(*id, &mut out);
                }
                // Where a subtree moves can change which owners are in a
                // repeat's template or in the library: expand them all.
                Op::Reparent { .. } => all = true,
                Op::Link { rel } => {
                    all |= touch(rel.from, &mut out);
                }
                Op::Unlink { .. } => {}
            }
        }
        // What the ops took with them: a deleted subtree's nodes (each an
        // owner, maybe), and the relations that went (an unlinked picture).
        for op in inverse {
            match op {
                Op::Create { id, .. } if !id.is_expanded() => {
                    all |= touch(*id, &mut out);
                    all |= pictures(*id, &mut out);
                }
                Op::Link { rel } if !rel.id.is_expanded() => {
                    all |= touch(rel.from, &mut out);
                }
                // A derived relation unlinked: its owner derives it again.
                Op::Link { rel } => out.extend(rel.id.root()),
                _ => {}
            }
        }
        if all {
            return Some(Which::All);
        }
        out.retain(|&o| !o.is_expanded() && !o.is_session());
        (!out.is_empty()).then_some(Which::Some(out))
    }
}
