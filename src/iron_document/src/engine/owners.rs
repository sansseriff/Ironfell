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
}

/// What an expansion would change here, applied only once the transaction
/// that made it is accepted.
pub(crate) struct Delta {
    all: bool,
    problems: BTreeMap<NodeId, Vec<Problem>>,
    counts: BTreeMap<NodeId, usize>,
    defs: Vec<(NodeId, NodeId)>,
}

impl Owners {
    /// Expand `which` again in `doc`: the ops applied, and what to record.
    pub(crate) fn expand(&self, doc: &mut Document, stamp: Version, counts: &Items, which: Which) -> (Vec<Op>, Delta) {
        let (all, owners) = match which {
            Which::All => (true, expand::top_owners(Tree::over(doc))),
            Which::Some(set) => (false, set),
        };
        // Owners may derive the budget less what the others hold, taken in
        // id order. Past the budget the cut falls per owner, not in the old
        // engine's global order: a difference only past 50,000 derived
        // nodes, recorded in the handoff (§4.4, stage 2) for the user.
        let held: usize = if all { 0 } else { self.counts.iter().filter(|(o, _)| !owners.contains(o)).map(|(_, n)| n).sum() };
        let done = expand::expand_owners(doc, stamp, counts, &owners, expand::NODE_BUDGET.saturating_sub(held));
        let delta = Delta { all, problems: done.problems, counts: done.counts, defs: done.defs };
        (done.ops, delta)
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
            if expand::in_library(doc, id) {
                match expand::definition_of(doc, id) {
                    Some(def) => out.extend(self.by_def.get(&def).into_iter().flatten().copied()),
                    // The library itself, or something outside any definition.
                    None => return true,
                }
            }
            false
        };
        // An entity's pictures follow it.
        let pictures = |entity: NodeId, out: &mut BTreeSet<NodeId>| {
            out.extend(doc.relations_to(entity).filter(|r| r.rel == RelKind::Figures).map(|r| r.from));
        };
        let is_structure_type = |id: NodeId| doc.node(id).is_some_and(|n| matches!(n.ty.name(), "definition" | "library"));
        let mut all = false;
        for op in ops {
            match op {
                Op::Create { id, .. } | Op::Delete { id } => {
                    all |= is_structure_type(*id);
                    all |= touch(*id, &mut out);
                    pictures(*id, &mut out);
                }
                Op::AddFacet { id, facet } => {
                    all |= expand::owns_items(facet.kind()) || is_structure_type(*id);
                    all |= touch(*id, &mut out);
                    pictures(*id, &mut out);
                }
                Op::RemoveFacet { id, kind } => {
                    all |= expand::owns_items(*kind) || is_structure_type(*id);
                    all |= touch(*id, &mut out);
                    pictures(*id, &mut out);
                }
                Op::Set { id, path, .. } => {
                    let shapes = matches!(path.facet, FacetKind::Instance | FacetKind::Use | FacetKind::Override) || expand::in_library(doc, *id) || expand::in_repeat_template(doc, *id);
                    if shapes {
                        all |= touch(*id, &mut out);
                        if path.facet == FacetKind::Instance {
                            pictures(*id, &mut out);
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
                }
                Op::Link { rel } if !rel.id.is_expanded() => {
                    all |= touch(rel.from, &mut out);
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
