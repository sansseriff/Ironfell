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

use crate::engine::template::{Env, Pieces, Template, TemplateCache};
use std::sync::Arc;
use crate::expand::{self, Items, Problem};
use crate::facet::FacetKind;
use crate::op::Op;
use crate::tree::Docs;
use crate::{NodeId, RelKind, Tree, Version};
use std::collections::{BTreeMap, BTreeSet};

/// Which owners to expand again.
pub(crate) enum Which {
    /// Every owner: on load, and on what this layer does not map (a reparent, a definition made or removed).
    All,
    Some(BTreeSet<NodeId>),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Owners {
    /// Each owner's expansion problems.
    problems: BTreeMap<NodeId, Vec<Problem>>,
    /// How many derived nodes each owner wanted: the budget's share.
    counts: crate::work::Counted<BTreeMap<NodeId, usize>, { crate::work::OWNERS_COUNTS }>,
    /// The sum of `counts`, kept as they change (DS-3b: no sum over every
    /// owner per expansion).
    held: usize,
    /// Definition -> owners whose expansion read it. Only grows.
    by_def: crate::work::Counted<BTreeMap<NodeId, BTreeSet<NodeId>>, { crate::work::OWNERS_BY_DEF }>,
    /// Owner -> owners whose pictures figure its derived entities. Only
    /// grows.
    readers: BTreeMap<NodeId, BTreeSet<NodeId>>,
    /// The templates compiled so far. Each knows whether it is current, so
    /// a refused transaction leaves none to put back.
    templates: TemplateCache,
    /// The copies a constraint cluster holds: expansion leaves the leaves
    /// the solver writes on them as they are (`solve::writes`).
    solved: BTreeSet<NodeId>,
    /// The solver leaves of copies the current transaction's own ops set:
    /// always written, whatever was wanted before (`Owners::edit`).
    edited: BTreeSet<crate::Leaf>,
    /// What each piece's last accepted expansion copied: a copy it records
    /// as current is kept as it is (2c stage 3, step 2). An entry outlives
    /// its owner; a copy is kept only when it is there.
    made: Made,
}

#[derive(Clone, Default)]
struct Made(Pieces);

impl std::fmt::Debug for Made {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Made({} pieces)", self.0.len())
    }
}

/// What an expansion would change here, applied only once the transaction
/// that made it is accepted.
pub(crate) struct Delta {
    all: bool,
    problems: BTreeMap<NodeId, Vec<Problem>>,
    counts: BTreeMap<NodeId, usize>,
    defs: Vec<(NodeId, NodeId)>,
    reads: Vec<(NodeId, NodeId)>,
    pieces: Vec<(NodeId, Arc<Template>, Arc<Env>)>,
}

impl Delta {
    fn add(&mut self, d: expand::OwnersExpanded) {
        self.problems.extend(d.problems);
        self.counts.extend(d.counts);
        self.defs.extend(d.defs);
        self.reads.extend(d.reads);
        self.pieces.extend(d.pieces);
    }

    /// Fold in a later delta: its owners' entries replace these.
    fn add_delta(&mut self, d: Delta) {
        self.all |= d.all;
        self.problems.extend(d.problems);
        self.counts.extend(d.counts);
        self.defs.extend(d.defs);
        self.reads.extend(d.reads);
        self.pieces.extend(d.pieces);
    }

    fn has_budget_problem(&self) -> bool {
        self.problems.values().flatten().any(|p| p.kind == "budget")
    }
}

impl Owners {
    /// Expand `which` again in `doc`: the ops applied, and what to record.
    pub(crate) fn expand(&mut self, docs: &mut Docs, stamp: Version, counts: &Items, which: Which) -> (Vec<Op>, Delta) {
        let Which::Some(mut todo) = which else { return self.expand_all(docs, stamp, counts) };
        // Owners whose last expansion failed in part are tried again: what
        // failed may depend on something outside them that has changed.
        todo.extend(self.problems.iter().filter(|(_, ps)| ps.iter().any(|p| matches!(p.kind, "expansion" | "budget"))).map(|(o, _)| *o));
        let mut delta = Delta { all: false, problems: BTreeMap::new(), counts: BTreeMap::new(), defs: Vec::new(), reads: Vec::new(), pieces: Vec::new() };
        let mut ops = Vec::new();
        let mut done: BTreeSet<NodeId> = BTreeSet::new();
        // Each wave expands the owners whose pictures read what the last one
        // changed.
        while !todo.is_empty() {
            // What the owners not in this wave hold, as this call has
            // left them so far.
            let mut held = self.held - todo.iter().filter_map(|o| self.counts.get(o)).sum::<usize>();
            for (o, &n) in delta.counts.iter().filter(|(o, _)| !todo.contains(o)) {
                held = (held + n).saturating_sub(self.counts.get(o).copied().unwrap_or(0));
            }
            let held = held.min(expand::NODE_BUDGET);
            let troubled: BTreeSet<NodeId> = self.problems.keys().copied().collect();
            let wave = expand::expand_owners(docs, &mut self.templates, stamp, counts, &todo, expand::NODE_BUDGET.saturating_sub(held), (&self.solved, &self.edited), &self.made.0, &troubled);
            ops.extend(std::mem::take(&mut { wave.ops.clone() }));
            delta.add(expand::OwnersExpanded { ops: Vec::new(), ..wave });
            done.extend(todo.iter().copied());
            let reading = |o: &NodeId| self.readers.get(o).into_iter().flatten().copied().chain(delta.reads.iter().filter(move |(r, _)| r == o).map(|(_, w)| *w)).collect::<Vec<_>>();
            todo = done.iter().flat_map(reading).filter(|o| !done.contains(o)).collect();
        }
        // Past the budget the cut must not depend on edit history: expand
        // everything, in id order.
        if delta.has_budget_problem() || self.problems.values().flatten().any(|p| p.kind == "budget") {
            let (more, all) = self.expand_all(docs, stamp, counts);
            ops.extend(more);
            return (ops, all);
        }
        (ops, delta)
    }

    /// Reconcile only the items of `repeats`, the repeats a settle round
    /// found with other items (reactive-engine handoff §4.4, stage 3b), or
    /// `None`, with nothing applied, when that cannot be done alone and
    /// the caller expands their owners whole: an owner with problems
    /// recorded, an owner another owner's pictures read, or what the items
    /// want meets the budget or a nesting limit. An op that fails to apply
    /// is repaired by expanding its owner whole, in the same call.
    pub(crate) fn expand_items(&mut self, docs: &mut Docs, stamp: Version, counts: &Items, repeats: &BTreeSet<NodeId>) -> Option<(Vec<Op>, Delta)> {
        // The whole-document rules (`expand`): any owner's failed or cut
        // expansion is tried again on every update, and past the budget
        // everything expands in id order. Neither is this path's.
        if self.problems.values().flatten().any(|p| matches!(p.kind, "expansion" | "budget")) {
            return None;
        }
        // A repeat inside another one that changed is expanded with it.
        let outer: BTreeSet<NodeId> = repeats.iter().copied().filter(|&r| !repeats.iter().any(|&o| o != r && r.item_of(o).is_some())).collect();
        let mut by_owner: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::new();
        for &r in &outer {
            by_owner.entry(*r.path().first()?).or_default().insert(r);
        }
        if by_owner.keys().any(|o| self.problems.contains_key(o) || self.readers.get(o).is_some_and(|rs| !rs.is_empty())) {
            return None;
        }
        // Each owner's whole count must be known, to keep the budget's share.
        if by_owner.keys().any(|o| !self.counts.contains_key(o)) {
            return None;
        }
        // What every owner holds, kept as each one's items change, so two
        // owners never spend the same room.
        let mut held: usize = self.held;
        let mut delta = Delta { all: false, problems: BTreeMap::new(), counts: BTreeMap::new(), defs: Vec::new(), reads: Vec::new(), pieces: Vec::new() };
        let mut ops = Vec::new();
        let mut repair: BTreeSet<NodeId> = BTreeSet::new();
        for (owner, rs) in &by_owner {
            let room = expand::NODE_BUDGET.saturating_sub(held);
            // Wanting is tried before anything applies, so a `None` from
            // the first owner leaves the document as it was. A later
            // owner's `None` is repaired by expanding it whole.
            let room = room + self.held_under(Tree::of(docs), rs);
            let Some((mut done, problems)) = expand::expand_items(docs, &mut self.templates, stamp, counts, *owner, rs, room, (&self.solved, &self.edited), &self.made.0) else {
                if ops.is_empty() {
                    return None;
                }
                repair.insert(*owner);
                continue;
            };
            ops.extend(done.ops);
            let whole = (self.counts[owner] + done.wanted).saturating_sub(done.held);
            held = (held + whole).saturating_sub(self.counts[owner]);
            delta.counts.insert(*owner, whole);
            delta.defs.extend(done.defs.iter().map(|&d| (d, *owner)));
            delta.reads.extend(done.reads.iter().map(|&r| (r, *owner)));
            delta.pieces.append(&mut done.pieces);
            if !problems.is_empty() {
                repair.insert(*owner);
            }
        }
        if !repair.is_empty() {
            let (more, d) = self.expand(docs, stamp, counts, Which::Some(repair));
            ops.extend(more);
            delta.add_delta(d);
        }
        Some((ops, delta))
    }

    /// How many derived nodes the items of `repeats` hold now.
    fn held_under(&self, tree: Tree<'_>, repeats: &BTreeSet<NodeId>) -> usize {
        repeats.iter().map(|&r| tree.descendants(r).into_iter().filter(|d| d.item_of(r).is_some()).count()).sum()
    }

    /// Every owner, in id order.
    fn expand_all(&mut self, docs: &mut Docs, stamp: Version, counts: &Items) -> (Vec<Op>, Delta) {
        let owners = expand::top_owners(Tree::of(docs));
        let troubled: BTreeSet<NodeId> = self.problems.keys().copied().collect();
        let mut done = expand::expand_owners(docs, &mut self.templates, stamp, counts, &owners, expand::NODE_BUDGET, (&self.solved, &self.edited), &self.made.0, &troubled);
        let ops = std::mem::take(&mut done.ops);
        let mut delta = Delta { all: true, problems: BTreeMap::new(), counts: BTreeMap::new(), defs: Vec::new(), reads: Vec::new(), pieces: Vec::new() };
        delta.add(done);
        (ops, delta)
    }

    /// Record what an accepted expansion found.
    pub(crate) fn apply(&mut self, d: Delta) {
        if d.all {
            self.problems.clear();
            self.counts.clear();
            self.held = 0;
        }
        for (o, p) in d.problems {
            if p.is_empty() {
                self.problems.remove(&o);
            } else {
                self.problems.insert(o, p);
            }
        }
        for (o, n) in d.counts {
            let was = if n == 0 { self.counts.remove(&o) } else { self.counts.insert(o, n) };
            self.held = self.held + n - was.unwrap_or(0);
        }
        #[cfg(feature = "differential")]
        assert_eq!(self.held, self.counts.values().sum::<usize>());
        for (def, o) in d.defs {
            self.by_def.entry(def).or_default().insert(o);
        }
        for (read, o) in d.reads {
            self.readers.entry(read).or_default().insert(o);
        }
        // A piece of an owner left with problems is not recorded: what it
        // wanted may not be what its copies hold (a failed op).
        for (piece, t, env) in d.pieces {
            if piece.path().first().is_some_and(|o| self.problems.contains_key(o)) {
                continue;
            }
            self.made.0.insert(piece, (t, env));
        }
    }

    /// The templates compiled so far.
    #[cfg(feature = "differential")]
    pub(crate) fn templates(&self) -> &TemplateCache {
        &self.templates
    }

    /// What each of `leaves`, leaves of copies, is as its owner's expansion
    /// wants it now (`expand::wanted_slots`).
    pub(crate) fn wanted_slots(&mut self, doc: Tree<'_>, counts: &Items, leaves: &[crate::Leaf]) -> Vec<(crate::Leaf, crate::Slot)> {
        expand::wanted_slots(doc, &mut self.templates, counts, leaves)
    }

    /// The copies a constraint cluster holds now (`solve::Clusters::copies`).
    pub(crate) fn hold(&mut self, solved: BTreeSet<NodeId>) {
        self.solved = solved;
    }

    /// The solver leaves of copies the transaction being applied sets
    /// itself: an edit of a held copy reaches it even when what is wanted
    /// for the leaf comes out as before (its template's value, or the
    /// override it already had).
    pub(crate) fn edit(&mut self, leaves: BTreeSet<crate::Leaf>) {
        self.edited = leaves;
    }

    /// The copies a constraint cluster held when last told.
    pub(crate) fn held(&self) -> &BTreeSet<NodeId> {
        &self.solved
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
                // repeat's template, a definition or the library: the owners
                // around its new place (here) and its old one (the inverse
                // move, below), and every owner and picture inside it, which
                // may now be nested or top-level (DS-3b: not every owner).
                Op::Reparent { id, .. } => {
                    for n in std::iter::once(*id).chain(doc.descendants(*id)) {
                        all |= is_structure_type(n);
                        if owner_kind(n) && !n.is_expanded() {
                            out.insert(n);
                        }
                        all |= pictures(n, &mut out);
                    }
                    all |= touch(*id, &mut out);
                }
                Op::Link { rel } => {
                    all |= touch(rel.from, &mut out);
                }
                Op::Unlink { .. } | Op::Axis(_) | Op::Solve { .. } => {}
                Op::Patch(p) => {
                    use crate::patch::PatchOp;
                    match p {
                        PatchOp::Add { patch } => {
                            for t in patch.entries.keys() {
                                all |= touch(t.node, &mut out);
                            }
                        }
                        PatchOp::SetEntry { target, .. } => {
                            all |= touch(target.node, &mut out);
                        }
                        PatchOp::Remove { id } | PatchOp::SetRegion { patch: id, .. } | PatchOp::SetRank { patch: id, .. } => {
                            if let Some(patch) = doc.patches().patches.get(id) {
                                for t in patch.entries.keys() {
                                    all |= touch(t.node, &mut out);
                                }
                            }
                        }
                    }
                }
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
                // A move's old place.
                Op::Reparent { parent: Some(p), .. } => {
                    all |= touch(*p, &mut out);
                }
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
