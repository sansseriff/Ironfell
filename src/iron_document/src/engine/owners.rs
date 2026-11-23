//! Owners (engine.md §3.1, §5; EN-5b): each authored entity, `use`, and drawn
//! repeat or conditional owns the derived nodes whose path starts with it.
//!
//! Static `Owners::made` and the hand-written op classifier `Owners::affected`
//! are gone (`EN-5b`): each owner's expansion records its structural and patch
//! [`Key`] reads (including its compiled templates' recorded reads), and writes
//! wake only the owners whose recorded reads were touched (plus newly created
//! owners in touched roots).

use crate::engine::template::TemplateCache;
use crate::expand::{self, Items, Problem};
use crate::facet::FacetKind;
use crate::op::Op;
use crate::registry::TypeId;
use crate::structure::Key;
use crate::tree::Docs;
use crate::value::Slot;
use crate::{Leaf, NodeId, Tree, Version};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Which owners to expand again.
pub(crate) enum Which {
    /// Every owner: on load, or when budget cuts require whole-document order.
    All,
    Some(BTreeSet<NodeId>),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Owners {
    /// Each owner's expansion problems.
    problems: BTreeMap<NodeId, Vec<Problem>>,
    /// How many derived nodes each owner wanted: the budget's share.
    counts: crate::work::Counted<BTreeMap<NodeId, usize>, { crate::work::OWNERS_COUNTS }>,
    /// The sum of `counts`, kept as they change.
    held: usize,
    /// Owner -> owners whose pictures figure its derived entities.
    readers: BTreeMap<NodeId, BTreeSet<NodeId>>,
    /// Compiled templates invalidated by recorded reads (`EN-5a`).
    templates: TemplateCache,
    /// Recorded structural & patch reads per top-level owner (`EN-5b`).
    owner_deps: HashMap<NodeId, Vec<Key>>,
    /// Reverse index: `Key` -> top-level owners that read it (`EN-5b`).
    owner_by_key: HashMap<Key, Vec<NodeId>>,
    /// Top-level owners currently tracked by `owner_deps`.
    known_owners: BTreeSet<NodeId>,
    /// Top-level roots already scanned for owners.
    known_roots: BTreeSet<NodeId>,
    /// Owners dirtied by `note_touched` since last `take_dirty`.
    dirty_owners: BTreeSet<NodeId>,
    /// Roots whose owner set (`In(_, entity|use)`, `With(_, Repeat|When)`) changed.
    dirty_roots: BTreeSet<NodeId>,
    /// Whether `Key::Roots` was touched (new top-level root may hold owners).
    roots_dirty: bool,
    /// The copies a constraint cluster holds (temporary until `EN-5c`).
    solved: BTreeSet<NodeId>,
    /// The solver leaves of copies the current transaction's own ops set (`EN-5c` deletes).
    edited: BTreeSet<Leaf>,
    /// Last accepted wanted solver-leaf values for held copies (`EN-5c` deletes).
    held_wanted: HashMap<Leaf, Slot>,
}

/// What an expansion would change here, applied only once the transaction
/// that made it is accepted.
pub(crate) struct Delta {
    all: bool,
    problems: BTreeMap<NodeId, Vec<Problem>>,
    counts: BTreeMap<NodeId, usize>,
    reads: Vec<(NodeId, NodeId)>,
    deps: BTreeMap<NodeId, Vec<Key>>,
    extra_deps: BTreeMap<NodeId, Vec<Key>>,
    roots: Vec<NodeId>,
    held_wanted: HashMap<Leaf, Slot>,
}

impl Delta {
    fn add(&mut self, d: expand::OwnersExpanded) {
        self.problems.extend(d.problems);
        self.counts.extend(d.counts);
        self.reads.extend(d.reads);
        self.deps.extend(d.deps);
        self.held_wanted.extend(d.held_wanted);
    }

    fn add_delta(&mut self, d: Delta) {
        self.all |= d.all;
        self.problems.extend(d.problems);
        self.counts.extend(d.counts);
        self.reads.extend(d.reads);
        self.deps.extend(d.deps);
        for (o, ks) in d.extra_deps {
            self.extra_deps.entry(o).or_default().extend(ks);
        }
        self.roots.extend(d.roots);
        self.held_wanted.extend(d.held_wanted);
    }

    fn has_budget_problem(&self) -> bool {
        self.problems.values().flatten().any(|p| p.kind == "budget")
    }
}

impl Owners {
    /// Notify both `TemplateCache` and recorded-read owner index of touched
    /// structural/patch keys (`EN-5a`, `EN-5b`).
    pub(crate) fn note_touched(&mut self, keys: &[Key]) {
        if keys.is_empty() {
            return;
        }
        self.templates.note_touched(keys);
        let entity_ty = TypeId::lookup("entity");
        let use_ty = TypeId::lookup("use");
        for k in keys {
            if let Some(owners) = self.owner_by_key.get(k) {
                self.dirty_owners.extend(owners.iter().copied());
            }
            match *k {
                Key::Roots => {
                    self.roots_dirty = true;
                }
                Key::In(root, ty) if Some(ty) == entity_ty || Some(ty) == use_ty => {
                    self.dirty_roots.insert(root);
                }
                Key::With(root, FacetKind::Repeat | FacetKind::When) => {
                    self.dirty_roots.insert(root);
                }
                _ => {}
            }
        }
    }

    /// Clear uncommitted dirty state and templates after a transaction rollback
    /// (`EN-5a`, `EN-5b`). Committed `owner_deps`/`known_owners` stay intact.
    pub(crate) fn clear_on_rollback(&mut self) {
        self.templates.clear();
        self.dirty_owners.clear();
        self.dirty_roots.clear();
        self.roots_dirty = false;
    }

    /// Drain touched keys from `docs` and return the dirty owners that must
    /// re-expand (`EN-5b`: replaces hand-written `Owners::affected`).
    pub(crate) fn take_dirty(&mut self, docs: &mut Docs) -> Option<Which> {
        let tkeys = docs.drain_template_touched();
        self.note_touched(&tkeys);
        let mut todo = std::mem::take(&mut self.dirty_owners);
        let doc = Tree::of(docs);
        if self.roots_dirty {
            self.roots_dirty = false;
            for &r in doc.roots().iter() {
                if !self.known_roots.contains(&r) {
                    self.dirty_roots.insert(r);
                }
            }
        }
        if !self.dirty_roots.is_empty() {
            let entity_ty = TypeId::lookup("entity").expect("registered");
            let use_ty = TypeId::lookup("use").expect("registered");
            for root in std::mem::take(&mut self.dirty_roots) {
                for id in doc
                    .nodes_of_type_in(root, entity_ty)
                    .into_iter()
                    .chain(doc.nodes_of_type_in(root, use_ty))
                    .chain(doc.nodes_with_in(root, FacetKind::Repeat))
                    .chain(doc.nodes_with_in(root, FacetKind::When))
                {
                    if !id.is_expanded() && !id.is_session() && !self.known_owners.contains(&id) {
                        todo.insert(id);
                    }
                }
            }
        }
        for (o, ps) in &self.problems {
            if ps.iter().any(|p| matches!(p.kind, "expansion" | "budget")) {
                todo.insert(*o);
            }
        }
        todo.retain(|&o| !o.is_expanded() && !o.is_session());
        (!todo.is_empty()).then_some(Which::Some(todo))
    }

    /// Expand `which` again in `docs`: the ops applied, and what to record.
    pub(crate) fn expand(&mut self, docs: &mut Docs, stamp: Version, counts: &Items, which: Which) -> (Vec<Op>, Delta) {
        let tkeys = docs.drain_template_touched();
        self.note_touched(&tkeys);
        let Which::Some(mut todo) = which else { return self.expand_all(docs, stamp, counts) };
        todo.extend(self.problems.iter().filter(|(_, ps)| ps.iter().any(|p| matches!(p.kind, "expansion" | "budget"))).map(|(o, _)| *o));
        let mut delta = Delta {
            all: false,
            problems: BTreeMap::new(),
            counts: BTreeMap::new(),
            reads: Vec::new(),
            deps: BTreeMap::new(),
            extra_deps: BTreeMap::new(),
            roots: Tree::of(docs).roots().to_vec(),
            held_wanted: HashMap::new(),
        };
        let mut ops = Vec::new();
        let mut done: BTreeSet<NodeId> = BTreeSet::new();
        while !todo.is_empty() {
            let mut held = self.held - todo.iter().filter_map(|o| self.counts.get(o)).sum::<usize>();
            for (o, &n) in delta.counts.iter().filter(|(o, _)| !todo.contains(o)) {
                held = (held + n).saturating_sub(self.counts.get(o).copied().unwrap_or(0));
            }
            let held = held.min(expand::NODE_BUDGET);
            let wave = expand::expand_owners(docs, &mut self.templates, stamp, counts, &todo, expand::NODE_BUDGET.saturating_sub(held), (&self.solved, &self.edited), &self.held_wanted);
            ops.extend(std::mem::take(&mut { wave.ops.clone() }));
            delta.add(expand::OwnersExpanded { ops: Vec::new(), ..wave });
            done.extend(todo.iter().copied());
            let reading = |o: &NodeId| self.readers.get(o).into_iter().flatten().copied().chain(delta.reads.iter().filter(move |(r, _)| r == o).map(|(_, w)| *w)).collect::<Vec<_>>();
            todo = done.iter().flat_map(reading).filter(|o| !done.contains(o)).collect();
        }
        if delta.has_budget_problem() || self.problems.values().flatten().any(|p| p.kind == "budget") {
            let (more, all) = self.expand_all(docs, stamp, counts);
            ops.extend(more);
            return (ops, all);
        }
        (ops, delta)
    }

    /// Reconcile only the items of `repeats` for settle rounds.
    pub(crate) fn expand_items(&mut self, docs: &mut Docs, stamp: Version, counts: &Items, repeats: &BTreeSet<NodeId>) -> Option<(Vec<Op>, Delta)> {
        let tkeys = docs.drain_template_touched();
        self.note_touched(&tkeys);
        if self.problems.values().flatten().any(|p| matches!(p.kind, "expansion" | "budget")) {
            return None;
        }
        let outer: BTreeSet<NodeId> = repeats.iter().copied().filter(|&r| !repeats.iter().any(|&o| o != r && r.item_of(o).is_some())).collect();
        let mut by_owner: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::new();
        for &r in &outer {
            by_owner.entry(*r.path().first()?).or_default().insert(r);
        }
        if by_owner.keys().any(|o| self.problems.contains_key(o) || self.readers.get(o).is_some_and(|rs| !rs.is_empty())) {
            return None;
        }
        if by_owner.keys().any(|o| !self.counts.contains_key(o)) {
            return None;
        }
        let mut held: usize = self.held;
        let mut delta = Delta {
            all: false,
            problems: BTreeMap::new(),
            counts: BTreeMap::new(),
            reads: Vec::new(),
            deps: BTreeMap::new(),
            extra_deps: BTreeMap::new(),
            roots: Vec::new(),
            held_wanted: HashMap::new(),
        };
        let mut ops = Vec::new();
        let mut repair: BTreeSet<NodeId> = BTreeSet::new();
        for (owner, rs) in &by_owner {
            let room = expand::NODE_BUDGET.saturating_sub(held) + self.held_under(Tree::of(docs), rs);
            let Some((done, problems)) = expand::expand_items(docs, &mut self.templates, stamp, counts, *owner, rs, room, (&self.solved, &self.edited), &self.held_wanted) else {
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
            delta.reads.extend(done.reads.iter().map(|&r| (r, *owner)));
            delta.extra_deps.entry(*owner).or_default().extend(done.extra_deps);
            delta.held_wanted.extend(done.held_wanted);
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

    fn held_under(&self, tree: Tree<'_>, repeats: &BTreeSet<NodeId>) -> usize {
        repeats.iter().map(|&r| tree.descendants(r).into_iter().filter(|d| d.item_of(r).is_some()).count()).sum()
    }

    fn expand_all(&mut self, docs: &mut Docs, stamp: Version, counts: &Items) -> (Vec<Op>, Delta) {
        let tree = Tree::of(docs);
        let owners = expand::top_owners(tree);
        let roots = tree.roots().to_vec();
        let mut done = expand::expand_owners(docs, &mut self.templates, stamp, counts, &owners, expand::NODE_BUDGET, (&self.solved, &self.edited), &self.held_wanted);
        let ops = std::mem::take(&mut done.ops);
        let mut delta = Delta {
            all: true,
            problems: BTreeMap::new(),
            counts: BTreeMap::new(),
            reads: Vec::new(),
            deps: BTreeMap::new(),
            extra_deps: BTreeMap::new(),
            roots,
            held_wanted: HashMap::new(),
        };
        delta.add(done);
        (ops, delta)
    }

    fn set_owner_deps(&mut self, owner: NodeId, raw: Vec<Key>) {
        if let Some(old) = self.owner_deps.remove(&owner) {
            for k in old {
                if let Some(list) = self.owner_by_key.get_mut(&k) {
                    list.retain(|&x| x != owner);
                    if list.is_empty() {
                        self.owner_by_key.remove(&k);
                    }
                }
            }
        }
        if raw.is_empty() {
            self.known_owners.remove(&owner);
            return;
        }
        let mut seen = HashSet::with_capacity(raw.len());
        let mut uniq = Vec::with_capacity(raw.len());
        for k in raw {
            if seen.insert(k) {
                uniq.push(k);
            }
        }
        for &k in &uniq {
            self.owner_by_key.entry(k).or_default().push(owner);
        }
        self.owner_deps.insert(owner, uniq);
        self.known_owners.insert(owner);
    }

    fn append_owner_deps(&mut self, owner: NodeId, extra: Vec<Key>) {
        if extra.is_empty() {
            return;
        }
        let list = self.owner_deps.entry(owner).or_default();
        let mut seen: HashSet<Key> = list.iter().copied().collect();
        for k in extra {
            if seen.insert(k) {
                list.push(k);
                self.owner_by_key.entry(k).or_default().push(owner);
            }
        }
        self.known_owners.insert(owner);
    }

    /// Record what an accepted expansion found.
    pub(crate) fn apply(&mut self, d: Delta) {
        if d.all {
            self.problems.clear();
            self.counts.clear();
            self.held = 0;
            self.owner_deps.clear();
            self.owner_by_key.clear();
            self.known_owners.clear();
            self.known_roots.clear();
        }
        self.known_roots.extend(d.roots);
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
        for (read, o) in d.reads {
            self.readers.entry(read).or_default().insert(o);
        }
        for (o, raw) in d.deps {
            self.set_owner_deps(o, raw);
        }
        for (o, extra) in d.extra_deps {
            self.append_owner_deps(o, extra);
        }
        self.held_wanted.extend(d.held_wanted);
    }

    pub(crate) fn templates(&self) -> &TemplateCache {
        &self.templates
    }

    pub(crate) fn wanted_slots(&mut self, doc: Tree<'_>, counts: &Items, leaves: &[Leaf]) -> Vec<(Leaf, Slot)> {
        expand::wanted_slots(doc, &mut self.templates, counts, leaves)
    }

    pub(crate) fn hold(&mut self, doc: Tree<'_>, counts: &Items, solved: BTreeSet<NodeId>) {
        let added: Vec<NodeId> = solved.difference(&self.solved).copied().collect();
        self.solved = solved;
        self.held_wanted.retain(|leaf, _| self.solved.contains(&leaf.node));
        if !added.is_empty() {
            let leaves: Vec<Leaf> = added
                .into_iter()
                .flat_map(|node| {
                    crate::solve::TRANSFORM_LEAVES.map(move |f| Leaf {
                        node,
                        path: crate::facet::LeafPath::new(crate::facet::FacetKind::Transform2d, f).expect("declared"),
                    })
                })
                .collect();
            for (leaf, slot) in expand::wanted_slots(doc, &mut self.templates, counts, &leaves) {
                self.held_wanted.entry(leaf).or_insert(slot);
            }
        }
    }

    pub(crate) fn edit(&mut self, leaves: BTreeSet<Leaf>) {
        self.edited = leaves;
    }

    pub(crate) fn held(&self) -> &BTreeSet<NodeId> {
        &self.solved
    }

    pub(crate) fn problems(&self) -> Vec<Problem> {
        let mut out: Vec<Problem> = self.problems.values().flatten().cloned().collect();
        expand::sort_problems(&mut out);
        out
    }
}
