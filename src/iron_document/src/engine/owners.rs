//! Recorded-read owners and virtual view derivation (plans/specs/engine.md §2.3,
//! §3.1; handoff-engine.md EN-13).
//!
//! Each top-level authored owner (`entity`, `use`, or `repeat`/`when` group)
//! derives its [`OwnerPlan`] (`Vec<Arc<View>>`, `ItemTable` records, and derived
//! relations) through a recording [`Tree`]. Structural writes report touched
//! [`Key`]s through one channel; [`Owners::note_touched`] evicts dirty templates
//! in [`TemplateCache`] and wakes only the roots and top-level owners whose
//! recorded keys or subscribed templates were touched. No static op classifier
//! (`affected`), no `by_def`/`readers`/`made` tables, and no `Docs::copies`.

use crate::document::{RelKind, Relation};
use crate::engine::template::{CacheKey, Env, Item, Template, TemplateCache, Templates};
use crate::engine::views::{ItemRecord, OwnerPlan, View, ViewDelta, VirtualViews};
use crate::expand::{self, Items, Problem, DEPTH_MAX, DEPTH_REPORTED, NODE_BUDGET};
use crate::facet::{Facet, FacetKind, Instance, Use};
use crate::id::{NodeId, RelationId};
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::structure::{Key, Recorder};
use crate::tree::Docs;
use crate::Tree;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub(crate) struct Owners {
    /// Each owner's expansion problems.
    problems: BTreeMap<NodeId, Vec<Problem>>,
    /// Shared compiled templates, invalidated via recorded reads.
    templates: TemplateCache,
    /// Per-top-level-root discovery of candidate authored owners.
    roots_init: bool,
    roots_dirty: bool,
    dirty_roots: BTreeSet<NodeId>,
    root_owners: BTreeMap<NodeId, BTreeSet<NodeId>>,
    /// Per-top-owner recorded structural keys and subscribed template keys.
    owner_deps: HashMap<NodeId, Vec<Key>>,
    by_key: HashMap<Key, Vec<NodeId>>,
    owner_templates: HashMap<NodeId, Vec<CacheKey>>,
    by_template: HashMap<CacheKey, Vec<NodeId>>,
    /// Top-level owners marked dirty by `note_touched`.
    dirty_owners: BTreeSet<NodeId>,
    /// Saved dirty flags while a transaction journal is open (`begin_tx`).
    tx_dirty_backup: Option<(bool, BTreeSet<NodeId>, BTreeSet<NodeId>)>,
}

/// Owner metadata computed during a pass; committed only once the enclosing
/// transaction succeeds (`Owners::apply`).
pub(crate) struct Delta {
    all: bool,
    root_owners: BTreeMap<NodeId, Option<BTreeSet<NodeId>>>,
    problems: BTreeMap<NodeId, Vec<Problem>>,
    deps: BTreeMap<NodeId, (Vec<Key>, Vec<CacheKey>)>,
    removed_owners: Vec<NodeId>,
}

impl Owners {
    pub(crate) fn begin_tx(&mut self) {
        self.templates.begin();
        self.tx_dirty_backup = Some((self.roots_dirty, self.dirty_roots.clone(), self.dirty_owners.clone()));
    }

    pub(crate) fn commit_tx(&mut self) {
        self.templates.commit();
        self.tx_dirty_backup = None;
    }

    pub(crate) fn rollback_tx(&mut self) {
        self.templates.rollback();
        if let Some((rd, dr, do_)) = self.tx_dirty_backup.take() {
            self.roots_dirty = rd;
            self.dirty_roots = dr;
            self.dirty_owners = do_;
        }
    }

    /// Notify `TemplateCache` (authored keys only) and recorded-read owner
    /// subscriptions (authored + virtual/scoped keys) of touched structural keys.
    pub(crate) fn note_touched(&mut self, authored: &[Key], derived: &[Key]) {
        if !authored.is_empty() {
            let evicted = self.templates.note_touched(authored);
            for ck in evicted {
                if let Some(owners) = self.by_template.get(&ck) {
                    self.dirty_owners.extend(owners.iter().copied());
                }
            }
        }
        if authored.is_empty() && derived.is_empty() {
            return;
        }
        let entity_ty = TypeId::lookup("entity").expect("registered");
        let use_ty = TypeId::lookup("use").expect("registered");
        for k in authored.iter().chain(derived.iter()) {
            match k {
                Key::Roots => self.roots_dirty = true,
                Key::In(root, ty) if *ty == entity_ty || *ty == use_ty => {
                    self.dirty_roots.insert(*root);
                }
                Key::With(root, kind) if expand::owns_items(*kind) => {
                    self.dirty_roots.insert(*root);
                }
                _ => {}
            }
            if let Some(owners) = self.by_key.get(k) {
                self.dirty_owners.extend(owners.iter().copied());
            }
        }
    }

    /// Reconcile every dirty root and dirty top-level owner against `docs`,
    /// running waves automatically when one owner's virtual delta touches keys
    /// read by another owner (e.g. a picture figuring a derived entity).
    pub(crate) fn sync_dirty(&mut self, docs: &mut Docs, counts: &Items) -> (ViewDelta, Option<Delta>) {
        let (ak, dk) = docs.drain_view_touched();
        self.note_touched(&ak, &dk);

        if !self.roots_init {
            return self.sync_all(docs, counts);
        }

        let mut root_updates: BTreeMap<NodeId, Option<BTreeSet<NodeId>>> = BTreeMap::new();
        let mut todo: BTreeSet<NodeId> = std::mem::take(&mut self.dirty_owners);

        // Reconcile root-level owner discovery if `Key::Roots` or any root's
        // `In(entity|use)` / `With(Repeat|When)` fired. Do NOT mark vanished
        // root members as `removed_owners` here: a live owner reparented to
        // another root leaves its old root while remaining live in the new one.
        // `reconcile_worklist` classifies dead or non-owner nodes once live
        // status is checked.
        if self.roots_dirty {
            self.roots_dirty = false;
            let current: BTreeSet<NodeId> = Tree::of(docs).roots().iter().copied().collect();
            for &r in self.root_owners.keys() {
                if !current.contains(&r) {
                    if let Some(old) = self.root_owners.get(&r) {
                        todo.extend(old.iter().copied());
                    }
                    root_updates.insert(r, None);
                    self.dirty_roots.remove(&r);
                }
            }
            for &r in &current {
                if !self.root_owners.contains_key(&r) {
                    self.dirty_roots.insert(r);
                }
            }
        }
        let dirty_roots = std::mem::take(&mut self.dirty_roots);
        for root in dirty_roots {
            let tree = Tree::of(docs);
            if !tree.is_live(root) || tree.parent_of(root).is_some() {
                if let Some(old) = self.root_owners.get(&root) {
                    todo.extend(old.iter().copied());
                }
                root_updates.insert(root, None);
                continue;
            }
            let now = discover_owners_in_root(tree, root);
            let old = self.root_owners.get(&root);
            for &o in &now {
                if old.is_none_or(|s| !s.contains(&o)) {
                    todo.insert(o);
                }
            }
            if let Some(old) = old {
                for &o in old {
                    if !now.contains(&o) {
                        todo.insert(o);
                    }
                }
            }
            root_updates.insert(root, Some(now));
        }

        if todo.is_empty() && root_updates.is_empty() {
            return (ViewDelta::default(), None);
        }

        let (vdelta, delta) = self.reconcile_worklist(docs, counts, todo, false, root_updates);
        (vdelta, Some(delta))
    }

    /// Reconcile the top-level owners of `repeats` whose item keys changed
    /// during `settle_repeats`. Surviving views reuse their `Arc<View>` in O(1).
    pub(crate) fn sync_items(&mut self, docs: &mut Docs, counts: &Items, repeats: &BTreeSet<NodeId>) -> (ViewDelta, Delta) {
        let (ak, dk) = docs.drain_view_touched();
        self.note_touched(&ak, &dk);
        let mut todo: BTreeSet<NodeId> = std::mem::take(&mut self.dirty_owners);
        for &r in repeats {
            todo.insert(r.expanded_root().unwrap_or(r));
        }
        self.reconcile_worklist(docs, counts, todo, false, BTreeMap::new())
    }

    /// Rebuild every top-level owner in id order (load or differential
    /// full-view reference) through the same worklist and per-owner cap as
    /// incremental updates.
    pub(crate) fn sync_all(&mut self, docs: &mut Docs, counts: &Items) -> (ViewDelta, Option<Delta>) {
        let (ak, dk) = docs.drain_view_touched();
        self.note_touched(&ak, &dk);
        self.roots_dirty = false;
        self.dirty_roots.clear();
        self.dirty_owners.clear();

        let tree = Tree::of(docs);
        let mut root_owners: BTreeMap<NodeId, Option<BTreeSet<NodeId>>> = BTreeMap::new();
        let mut owners: BTreeSet<NodeId> = BTreeSet::new();
        for &root in tree.roots() {
            let found = discover_owners_in_root(tree, root);
            owners.extend(found.iter().copied());
            root_owners.insert(root, Some(found));
        }
        // Include any stale owner still holding views so it gets torn down.
        owners.extend(docs.views.top_owners_held());

        let (vdelta, delta) = self.reconcile_worklist(docs, counts, owners, true, root_owners);
        (vdelta, Some(delta))
    }

    /// Single worklist used by incremental updates, repeat settling, and full
    /// rebuilds. Orders each wave `(is_use, owner_id)` so entities and repeats
    /// settle before pictures that figure them, and caps each top-level owner
    /// independently at `NODE_BUDGET` (`EN-13+EN-7`).
    fn reconcile_worklist(
        &mut self,
        docs: &mut Docs,
        counts: &Items,
        initial: BTreeSet<NodeId>,
        full: bool,
        root_owners: BTreeMap<NodeId, Option<BTreeSet<NodeId>>>,
    ) -> (ViewDelta, Delta) {
        const MAX_WAVES: usize = 8;
        let mut delta = Delta {
            all: full,
            root_owners,
            problems: BTreeMap::new(),
            deps: BTreeMap::new(),
            removed_owners: Vec::new(),
        };
        let mut vdelta = ViewDelta::default();
        let mut staged_by_key: HashMap<Key, Vec<NodeId>> = HashMap::new();
        let mut todo = initial;
        let mut waves = 0usize;
        let use_ty = TypeId::lookup("use").expect("registered");

        while !todo.is_empty() && waves < MAX_WAVES {
            waves += 1;
            let mut ordered: Vec<(bool, NodeId)> = todo
                .iter()
                .map(|&o| (Tree::of(docs).ty_of(o) == Some(use_ty), o))
                .collect();
            ordered.sort_unstable();

            let mut next_wave: BTreeSet<NodeId> = BTreeSet::new();
            for (_, owner) in ordered {
                next_wave.remove(&owner);
                let BuiltOwner { plan, problems, keys: rec_keys, templates: used_t } = self.build_one(docs, owner, counts, NODE_BUDGET);
                delta.problems.insert(owner, problems);

                // Replace any previous staged subscription for `owner`.
                if let Some((prev_k, _)) = delta.deps.remove(&owner) {
                    unindex_list(&mut staged_by_key, &prev_k, owner);
                }
                let tree = Tree::of(docs);
                if is_top_level_owner(tree, owner) {
                    let (uniq_k, uniq_t) = dedupe_deps(rec_keys, used_t);
                    index_list(&mut staged_by_key, &uniq_k, owner);
                    delta.deps.insert(owner, (uniq_k, uniq_t));
                } else {
                    delta.removed_owners.push(owner);
                }

                let vd = docs.reconcile_owner(owner, plan);
                vdelta.extend(vd);

                // Drain only this owner's view mutations and wake dependent
                // owners (excluding `owner` itself) across both committed and
                // staged subscriptions. Virtual view writes never mutate
                // authored templates, so `TemplateCache` is not invalidated.
                let (_, vk) = docs.drain_view_touched();
                if !vk.is_empty() {
                    let doc = Tree::of(docs);
                    let mut wake = |o: NodeId, committed: bool| {
                        if o == owner || !doc.is_live(o) {
                            return;
                        }
                        if committed && (full || delta.deps.contains_key(&o)) {
                            return;
                        }
                        next_wave.insert(o);
                    };
                    for k in &vk {
                        for &o in self.by_key.get(k).into_iter().flatten() {
                            wake(o, true);
                        }
                        for &o in staged_by_key.get(k).into_iter().flatten() {
                            wake(o, false);
                        }
                    }
                }
            }
            todo = next_wave;
        }

        // Report explicitly if wave limit was hit instead of silently dropping work.
        if !todo.is_empty() {
            for owner in todo {
                delta.problems.entry(owner).or_default().push(Problem {
                    node: owner,
                    kind: "depth_blocked",
                    message: format!("owner expansion did not settle within {MAX_WAVES} waves"),
                });
            }
        }

        (vdelta, delta)
    }

    fn build_one(&mut self, docs: &Docs, owner: NodeId, counts: &Items, room: usize) -> BuiltOwner {
        let rec = Recorder::default();
        let mut cache = Templates::new(&mut self.templates);
        let b = build_owner(Tree::of(docs).recording(&rec), &docs.views, owner, counts, room, &mut cache);
        let used_t = cache.take_used();
        BuiltOwner { plan: b.plan, problems: b.problems, keys: rec.into_inner(), templates: used_t }
    }

    /// Commit what an accepted pass found.
    pub(crate) fn apply(&mut self, d: Delta) {
        self.roots_init = true;
        if d.all {
            self.problems.clear();
            self.root_owners.clear();
            self.owner_deps.clear();
            self.by_key.clear();
            self.owner_templates.clear();
            self.by_template.clear();
        }
        for (root, set) in d.root_owners {
            match set {
                Some(s) => {
                    self.root_owners.insert(root, s);
                }
                None => {
                    self.root_owners.remove(&root);
                }
            }
        }
        for (o, p) in d.problems {
            if p.is_empty() {
                self.problems.remove(&o);
            } else {
                self.problems.insert(o, p);
            }
        }
        for o in d.removed_owners {
            if !d.deps.contains_key(&o) {
                self.unindex_owner(o);
            }
        }
        for (o, (keys, tkeys)) in d.deps {
            self.set_owner_deps(o, keys, tkeys);
        }
    }

    fn unindex_owner(&mut self, owner: NodeId) {
        if let Some(old) = self.owner_deps.remove(&owner) {
            unindex_list(&mut self.by_key, &old, owner);
        }
        if let Some(old) = self.owner_templates.remove(&owner) {
            unindex_list(&mut self.by_template, &old, owner);
        }
    }

    fn set_owner_deps(&mut self, owner: NodeId, uniq_k: Vec<Key>, uniq_t: Vec<CacheKey>) {
        self.unindex_owner(owner);
        index_list(&mut self.by_key, &uniq_k, owner);
        if !uniq_k.is_empty() {
            self.owner_deps.insert(owner, uniq_k);
        }
        index_list(&mut self.by_template, &uniq_t, owner);
        if !uniq_t.is_empty() {
            self.owner_templates.insert(owner, uniq_t);
        }
    }

    pub(crate) fn problems(&self) -> Vec<Problem> {
        let mut out: Vec<Problem> = self.problems.values().flatten().cloned().collect();
        expand::sort_problems(&mut out);
        out
    }

    pub(crate) fn clear_templates(&mut self) {
        self.templates.clear();
    }
}

fn dedupe_deps(raw_keys: Vec<Key>, raw_tkeys: Vec<CacheKey>) -> (Vec<Key>, Vec<CacheKey>) {
    let mut seen_k = HashSet::with_capacity(raw_keys.len());
    let uniq_k = raw_keys.into_iter().filter(|k| seen_k.insert(*k)).collect();
    let mut seen_t = HashSet::with_capacity(raw_tkeys.len());
    let uniq_t = raw_tkeys.into_iter().filter(|ck| seen_t.insert(ck.clone())).collect();
    (uniq_k, uniq_t)
}

fn index_list<K: Eq + std::hash::Hash + Clone>(map: &mut HashMap<K, Vec<NodeId>>, keys: &[K], owner: NodeId) {
    for k in keys {
        map.entry(k.clone()).or_default().push(owner);
    }
}

fn unindex_list<K: Eq + std::hash::Hash>(map: &mut HashMap<K, Vec<NodeId>>, keys: &[K], owner: NodeId) {
    for k in keys {
        if let Some(list) = map.get_mut(k) {
            list.retain(|&x| x != owner);
            if list.is_empty() {
                map.remove(k);
            }
        }
    }
}

/// Discover every candidate top-level authored owner (`entity`, `use`, or
/// `repeat`/`when` group) under top-level document root `root` via scoped
/// indexes (O(owners in root), zero full-doc scan).
fn discover_owners_in_root(doc: Tree<'_>, root: NodeId) -> BTreeSet<NodeId> {
    let mut out = BTreeSet::new();
    for ty_name in ["entity", "use"] {
        let ty = TypeId::lookup(ty_name).expect("registered");
        for id in doc.nodes_of_type_in(root, ty) {
            if !id.is_expanded() && !id.is_session() {
                out.insert(id);
            }
        }
    }
    for kind in [FacetKind::Repeat, FacetKind::When] {
        for id in doc.nodes_with_in(root, kind) {
            if !id.is_expanded() && !id.is_session() && doc.ty_of(id).is_some_and(|t| t.name() == "group") {
                out.insert(id);
            }
        }
    }
    out
}

fn is_top_level_owner(doc: Tree<'_>, id: NodeId) -> bool {
    if id.is_expanded() || id.is_session() {
        return false;
    }
    let Some(node) = doc.live(id) else {
        return false;
    };
    match node.ty.name() {
        "entity" | "use" => true,
        "group" => doc.has(id, FacetKind::Repeat) || doc.has(id, FacetKind::When),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Per-owner virtual plan builder (`build_owner`)

struct BuiltOwner {
    plan: OwnerPlan,
    problems: Vec<Problem>,
    keys: Vec<Key>,
    templates: Vec<CacheKey>,
}

struct BuildOut {
    plan: OwnerPlan,
    problems: Vec<Problem>,
}

struct PlanBuilder<'e> {
    top_owner: NodeId,
    existing: &'e VirtualViews,
    views: Vec<Arc<View>>,
    /// Index into `views` by piece owner for O(1) lookup within this plan.
    view_idx: HashMap<NodeId, usize>,
    items: Vec<(NodeId, Arc<[ItemRecord]>)>,
    /// O(1) membership index of planned item scopes for `is_live_in_plan`.
    item_scopes: HashSet<NodeId>,
    rels: Vec<Relation>,
    /// O(1) lookup for `RelKind::Figures` emitted inside this plan (`from -> to`).
    rel_figures: HashMap<NodeId, NodeId>,
    problems: Vec<Problem>,
    count: usize,
    cap: usize,
    full: bool,
    /// Derived repeat/when node -> `(outer_template, outer_env)` that copied it.
    origins: HashMap<NodeId, (Arc<Template>, Arc<Env>)>,
    /// Queue of virtual nodes `(id, view_pos, idx_in_view, scoped)` to check for
    /// nested `entity` / `use` / `repeat` scopes.
    pending_nodes: Vec<(NodeId, usize, usize, bool)>,
    pending_repeats: Vec<NodeId>,
}

impl<'e> PlanBuilder<'e> {
    fn new(top_owner: NodeId, existing: &'e VirtualViews, cap: usize) -> PlanBuilder<'e> {
        PlanBuilder {
            top_owner,
            existing,
            views: Vec::new(),
            view_idx: HashMap::new(),
            items: Vec::new(),
            item_scopes: HashSet::new(),
            rels: Vec::new(),
            rel_figures: HashMap::new(),
            problems: Vec::new(),
            count: 0,
            cap,
            full: false,
            origins: HashMap::new(),
            pending_nodes: Vec::new(),
            pending_repeats: Vec::new(),
        }
    }

    fn push_rel(&mut self, rel: Relation) {
        if rel.rel == RelKind::Figures {
            self.rel_figures.entry(rel.from).or_insert(rel.to);
        }
        self.rels.push(rel);
    }

    fn enter(&mut self, owner: NodeId) -> bool {
        if self.full {
            return false;
        }
        let depth = owner.nesting() + 1;
        if depth > DEPTH_MAX {
            self.problems.push(Problem {
                node: owner,
                kind: "depth_blocked",
                message: format!("nested more than {DEPTH_MAX} deep: not expanded (does something contain itself?)"),
            });
            return false;
        }
        if depth == DEPTH_REPORTED + 1 {
            self.problems.push(Problem {
                node: owner,
                kind: "depth",
                message: format!("nested more than {DEPTH_REPORTED} deep"),
            });
        }
        true
    }

    /// Admit up to `n` nodes from `t` under `owner`, returning how many fit.
    /// Reports `"budget"` once on the first dropped node's parent.
    fn admit_template(&mut self, owner: NodeId, t: &Template) -> usize {
        let avail = self.cap.saturating_sub(self.count);
        let limit = t.nodes.len().min(avail);
        self.count += limit;
        if limit < t.nodes.len() && !self.full {
            self.full = true;
            let n = &t.nodes[limit];
            let parent = if n.parent == t.source_root() {
                owner
            } else {
                NodeId::expanded(owner, n.parent).unwrap_or(owner)
            };
            let id = NodeId::expanded(owner, n.local).unwrap_or(owner);
            self.problems.push(Problem {
                node: parent,
                kind: "budget",
                message: format!("the document derives more than {NODE_BUDGET} nodes; the rest are left out, from {id} on"),
            });
        }
        limit
    }

    fn admit_one(&mut self, parent: NodeId, id: NodeId) -> bool {
        if self.count >= self.cap {
            if !self.full {
                self.full = true;
                self.problems.push(Problem {
                    node: parent,
                    kind: "budget",
                    message: format!("the document derives more than {NODE_BUDGET} nodes; the rest are left out, from {id} on"),
                });
            }
            return false;
        }
        self.count += 1;
        true
    }

    /// Reuse an existing `Arc<View>` in O(1) when `(template, env, limit)` match;
    /// otherwise build sparse facets once.
    fn make_view(&mut self, owner: NodeId, template: Arc<Template>, env: Arc<Env>, limit: usize) -> usize {
        let view = if let Some(prev) = self.existing.views.get(&owner)
            && prev.top_owner == self.top_owner
            && prev.limit == limit
            && Arc::ptr_eq(&prev.template, &template)
            && *prev.env == *env
        {
            Arc::clone(prev)
        } else {
            Arc::new(View::new(owner, self.top_owner, template, env, limit))
        };
        let pos = self.views.len();
        self.view_idx.insert(owner, pos);
        self.views.push(view);
        pos
    }

    fn locate_in_plan(&self, id: NodeId) -> Option<(&View, usize)> {
        let (owner, local) = id.expanded_parts()?;
        let &pos = self.view_idx.get(&owner)?;
        let v = &self.views[pos];
        let &idx = v.template.local_index.get(&local)?;
        (idx < v.limit).then_some((v.as_ref(), idx))
    }

    fn in_template(&self, doc: Tree<'_>, id: NodeId) -> bool {
        let parent_of = |n: NodeId| -> Option<NodeId> {
            if let Some((v, i)) = self.locate_in_plan(n) {
                return Some(v.parent_of_idx(i));
            }
            if n.is_item_scope() {
                return n.expanded_parts().map(|(r, _)| r);
            }
            doc.parent_of(n)
        };
        let is_repeat = |n: NodeId| -> bool {
            if let Some((v, i)) = self.locate_in_plan(n) {
                return v.has_at(i, FacetKind::Repeat) || v.has_at(i, FacetKind::When);
            }
            expand::is_item_owner(doc, n)
        };
        let mut at = parent_of(id);
        while let Some(p) = at {
            if is_repeat(p) && id.item_of(p).is_none() {
                return true;
            }
            at = parent_of(p);
        }
        false
    }

    fn is_live_in_plan(&self, id: NodeId) -> bool {
        self.item_scopes.contains(&id) || self.locate_in_plan(id).is_some()
    }
}

struct Picture {
    entity: Option<NodeId>,
    def: NodeId,
    notation: String,
    sized: bool,
}

fn picture_of(doc: Tree<'_>, u: NodeId) -> Option<Picture> {
    let notation = doc.get::<Use>(u).and_then(|x| x.notation.constant()).and_then(|v| v.as_str()).map(str::to_owned)?;
    let sized = doc.has(u, FacetKind::Size);
    if let Some(def) = expand::main_definition(doc, u) {
        return Some(Picture { entity: None, def, notation, sized });
    }
    let entity = expand::use_entity(doc, u)?;
    let def = expand::linked_definition(doc, entity)?;
    Some(Picture { entity: Some(entity), def, notation, sized })
}

fn linked_def_of_facet(doc: Tree<'_>, f: Option<&Facet>) -> Option<NodeId> {
    let Facet::Instance(inst) = f? else { return None };
    if inst.mode.constant().and_then(|v| v.as_str()) != Some("linked") {
        return None;
    }
    let d = inst.def.constant()?.as_ref()?;
    if d.is_expanded() {
        return None;
    }
    doc.live(d).filter(|n| n.ty.name() == "definition").map(|_| d)
}

fn linked_def_of_node(doc: Tree<'_>, e: NodeId) -> Option<NodeId> {
    doc.node(e)
        .and_then(|_| doc.get::<Instance>(e))
        .and_then(|i| (i.mode.constant().and_then(|v| v.as_str()) == Some("linked")).then(|| i.def.constant().and_then(|v| v.as_ref())).flatten())
        .filter(|d| !d.is_expanded())
}

fn build_owner(
    doc: Tree<'_>,
    existing: &VirtualViews,
    owner: NodeId,
    counts: &Items,
    cap: usize,
    cache: &mut Templates<'_>,
) -> BuildOut {
    let mut out = PlanBuilder::new(owner, existing, cap);
    let Some(node) = doc.live(owner).filter(|_| !owner.is_expanded() && !owner.is_session()) else {
        return BuildOut { plan: OwnerPlan::default(), problems: Vec::new() };
    };
    match node.ty.name() {
        "entity" => {
            if let Some(def) = expand::linked_definition(doc, owner) {
                want_entity(doc, owner, def, &mut out, cache);
            }
        }
        "use" => {
            if let Some(p) = picture_of(doc, owner) {
                want_use(doc, owner, p.sized, p.entity, p.def, &p.notation, &mut out, cache);
            }
        }
        "group" => {
            if expand::is_item_owner(doc, owner) && !expand::in_library(doc, owner) && !expand::in_repeat_template(doc, owner) {
                want_items(doc, owner, counts.get(&owner).map_or(&[][..], Vec::as_slice), &mut out, cache);
            }
        }
        _ => {}
    }
    want_scopes(doc, counts, &mut out, cache);

    // Filter derived relations to match `op::link`: `from` must be live, and
    // `to` must be live (in plan or doc) or have a known authored expanded root.
    let raw_rels = std::mem::take(&mut out.rels);
    let mut rels = Vec::with_capacity(raw_rels.len());
    let mut seen_rids = HashSet::with_capacity(raw_rels.len());
    for rel in raw_rels {
        if !seen_rids.insert(rel.id) {
            continue;
        }
        let from_live = out.is_live_in_plan(rel.from) || (!rel.from.is_expanded() && doc.is_live(rel.from));
        if !from_live {
            continue;
        }
        let to_ok = match rel.to.expanded_root() {
            Some(root) => doc.node(root).is_some(),
            None => doc.is_live(rel.to),
        };
        if to_ok {
            rels.push(rel);
        }
    }
    // Reuse the existing `Arc<[Relation]>` when equal so `reconcile_top_owner`
    // skips relation index teardown & reinstall in O(1) via `Arc::ptr_eq`.
    let rels: Arc<[Relation]> = match existing.rels_by_top.get(&owner) {
        Some(prev) if prev.as_ref() == rels.as_slice() => Arc::clone(prev),
        _ => Arc::from(rels),
    };

    expand::sort_problems(&mut out.problems);
    BuildOut {
        plan: OwnerPlan { views: out.views, items: out.items, rels },
        problems: out.problems,
    }
}

/// Indices in `view` (< `limit`) that are either template-declared nested owners
/// (`entity`/`use`/`Repeat`/`When`) or gained `Repeat`/`When` via lexical override.
fn view_nested_indices(view: &View, limit: usize) -> Vec<usize> {
    let mut extra: Vec<usize> = view
        .extra_facets
        .iter()
        .filter(|&(&idx, ks)| idx < limit && ks.iter().any(|k| matches!(k, FacetKind::Repeat | FacetKind::When)))
        .map(|(&idx, _)| idx)
        .collect();
    if extra.is_empty() {
        return Vec::new();
    }
    extra.extend(view.template.nested_owners.iter().copied().take_while(|&i| i < limit));
    extra.sort_unstable();
    extra.dedup();
    extra
}

fn want_entity(doc: Tree<'_>, entity: NodeId, def: NodeId, out: &mut PlanBuilder<'_>, cache: &mut Templates<'_>) {
    if !out.enter(entity) {
        return;
    }
    let t = cache.body(doc, def);
    let env = Arc::new(Env {
        owner: entity,
        entity: Some(entity),
        sized: false,
        item: None,
        overrides: cache.overrides(doc, entity),
    });
    let limit = out.admit_template(entity, &t);
    let vpos = out.make_view(entity, t.clone(), env.clone(), limit);
    for &idx in &t.rel_nodes {
        if idx >= limit {
            break;
        }
        let n = &t.nodes[idx];
        let Some(id) = NodeId::expanded(entity, n.local) else { continue };
        for r in &n.rels {
            let to = t.follow_node(&env, r.to);
            out.push_rel(Relation {
                id: RelationId::expanded(entity, r.id),
                from: id,
                to,
                rel: r.rel,
                data: r.data.clone(),
            });
        }
    }
    let mut inner_entities: Vec<(NodeId, NodeId)> = Vec::new();
    for &idx in &t.nested_owners {
        if idx >= limit {
            break;
        }
        let n = &t.nodes[idx];
        if n.top && n.ty.name() == "entity" {
            let Some(id) = NodeId::expanded(entity, n.local) else { continue };
            let f = out.views[vpos].facet_at(idx, FacetKind::Instance);
            if let Some(inner_def) = linked_def_of_facet(doc, f) {
                inner_entities.push((id, inner_def));
            }
        }
    }
    for (id, inner_def) in inner_entities {
        want_entity(doc, id, inner_def, out, cache);
    }
}

#[allow(clippy::too_many_arguments)]
fn want_use(
    doc: Tree<'_>,
    u: NodeId,
    sized: bool,
    entity: Option<NodeId>,
    def: NodeId,
    notation: &str,
    out: &mut PlanBuilder<'_>,
    cache: &mut Templates<'_>,
) {
    if !out.enter(u) {
        return;
    }
    let Some(t) = cache.picture(doc, def, notation) else { return };
    let limit = out.admit_template(u, &t);
    let env = Arc::new(Env {
        owner: u,
        entity,
        sized,
        item: None,
        overrides: cache.overrides(doc, u),
    });
    let vpos = out.make_view(u, t.clone(), env.clone(), limit);
    let mut inner_uses: Vec<(NodeId, bool, NodeId, NodeId, String)> = Vec::new();
    let in_lib = expand::in_library(doc, out.top_owner);
    let merged = view_nested_indices(&out.views[vpos], limit);
    let idxs: &[usize] = if merged.is_empty() { &t.nested_owners } else { &merged };
    for &idx in idxs {
        if idx >= limit {
            break;
        }
        let n = &t.nodes[idx];
        let Some(id) = NodeId::expanded(u, n.local) else { continue };
        let view = &out.views[vpos];
        let repeats = !in_lib && (view.has_at(idx, FacetKind::Repeat) || view.has_at(idx, FacetKind::When));
        let is_use = n.ty.name() == "use";
        let scoped = !matches!(n.ty.name(), "entity" | "use") || n.own;
        if repeats {
            out.origins.insert(id, (t.clone(), env.clone()));
            out.pending_repeats.push(id);
        }
        if !scoped {
            out.pending_nodes.push((id, vpos, idx, false));
        }
        if is_use
            && let Some(r) = &n.figures
        {
            let inner_entity = t.follow_node(&env, r.to);
            let inner_sized = view.has_at(idx, FacetKind::Size);
            let inner_notation = if n.own {
                view.facet_at(idx, FacetKind::Use).and_then(|f| match f {
                    Facet::Use(x) => x.notation.constant().and_then(|v| v.as_str()).map(str::to_owned),
                    _ => None,
                })
            } else {
                None
            };
            out.push_rel(Relation {
                id: RelationId::expanded(u, r.id),
                from: id,
                to: inner_entity,
                rel: RelKind::Figures,
                data: r.data.clone(),
            });
            if n.own
                && let (Some(not), Some(d)) = (inner_notation, linked_def_of_node(doc, r.to))
            {
                inner_uses.push((id, inner_sized, inner_entity, d, not));
            }
        }
    }
    for (id, inner_sized, inner_entity, d, not) in inner_uses {
        want_use(doc, id, inner_sized, Some(inner_entity), d, &not, out, cache);
    }
}

fn want_items(doc: Tree<'_>, r: NodeId, keys: &[String], out: &mut PlanBuilder<'_>, cache: &mut Templates<'_>) {
    if keys.is_empty() || !out.enter(r) {
        return;
    }
    let (t, outer) = if !r.is_expanded() {
        (cache.repeat(doc, r), None)
    } else {
        let Some((outer_t, outer_env)) = out.origins.get(&r).cloned() else { return };
        let Some((_, local)) = r.expanded_parts() else { return };
        (cache.sub(&outer_t, local), Some((outer_t, outer_env)))
    };
    let in_lib = expand::in_library(doc, out.top_owner);
    let mut order_key = t.after.clone();
    let mut recs = Vec::with_capacity(keys.len());
    for k in keys {
        let Some(scope) = NodeId::item_scope(r, k) else { continue };
        order_key = OrderKey::after(&order_key);
        if !out.admit_one(r, scope) {
            break;
        }
        out.item_scopes.insert(scope);
        recs.push(ItemRecord {
            scope,
            repeat: r,
            key: k.clone(),
            order: order_key.clone(),
        });
        let limit = out.admit_template(scope, &t);
        let env = Arc::new(Env {
            owner: scope,
            entity: None,
            sized: false,
            item: Some(Item { repeat: r, key: k.clone(), outer: outer.clone() }),
            overrides: cache.overrides(doc, scope),
        });
        let vpos = out.make_view(scope, t.clone(), env.clone(), limit);
        let merged = view_nested_indices(&out.views[vpos], limit);
        let idxs: &[usize] = if merged.is_empty() { &t.nested_owners } else { &merged };
        for &idx in idxs {
            if idx >= limit {
                break;
            }
            let n = &t.nodes[idx];
            let Some(copy) = NodeId::expanded(scope, n.local) else { continue };
            let view = &out.views[vpos];
            let repeats = !in_lib && (view.has_at(idx, FacetKind::Repeat) || view.has_at(idx, FacetKind::When));
            if repeats {
                out.origins.insert(copy, (t.clone(), env.clone()));
                out.pending_repeats.push(copy);
            }
            if matches!(n.ty.name(), "entity" | "use") {
                out.pending_nodes.push((copy, vpos, idx, false));
            }
        }
        for rel in &t.rels {
            let Some(from) = NodeId::item(r, k, rel.from) else { continue };
            let to = t.follow_node(&env, rel.to);
            out.push_rel(Relation {
                id: RelationId::expanded(scope, rel.id),
                from,
                to,
                rel: rel.rel,
                data: rel.data.clone(),
            });
        }
        if out.full {
            break;
        }
    }
    if !recs.is_empty() {
        let arc_recs: Arc<[ItemRecord]> = match out.existing.items.get(&r) {
            Some(prev) if prev.as_ref() == recs.as_slice() => Arc::clone(prev),
            _ => Arc::from(recs),
        };
        out.items.push((r, arc_recs));
    }
}

fn want_scopes(doc: Tree<'_>, counts: &Items, out: &mut PlanBuilder<'_>, cache: &mut Templates<'_>) {
    let mut done_repeats: BTreeSet<NodeId> = BTreeSet::new();
    let mut seen = 0;
    loop {
        while seen < out.pending_nodes.len() {
            let (id, vpos, idx, scoped) = out.pending_nodes[seen];
            seen += 1;
            if scoped || out.in_template(doc, id) {
                continue;
            }
            let ty_name = out.views[vpos].template.nodes[idx].ty.name();
            match ty_name {
                "entity" => {
                    let f = out.views[vpos].facet_at(idx, FacetKind::Instance);
                    if let Some(def) = linked_def_of_facet(doc, f) {
                        want_entity(doc, id, def, out, cache);
                    }
                }
                "use" => {
                    let sized = out.views[vpos].has_at(idx, FacetKind::Size);
                    let notation = out.views[vpos].facet_at(idx, FacetKind::Use).and_then(|f| match f {
                        Facet::Use(x) => x.notation.constant().and_then(|v| v.as_str()).map(str::to_owned),
                        _ => None,
                    });
                    let entity = out.rel_figures.get(&id).copied();
                    let def = entity.and_then(|e| match out.locate_in_plan(e) {
                        Some((v, i)) => linked_def_of_facet(doc, v.facet_at(i, FacetKind::Instance)),
                        None => expand::linked_definition(doc, e),
                    });
                    if let (Some(entity), Some(def), Some(notation)) = (entity, def, notation) {
                        want_use(doc, id, sized, Some(entity), def, &notation, out, cache);
                    }
                }
                _ => {}
            }
        }
        let Some(r) = out.pending_repeats.pop() else { break };
        if done_repeats.insert(r) && !out.in_template(doc, r) {
            want_items(doc, r, counts.get(&r).map_or(&[][..], Vec::as_slice), out, cache);
        }
    }
}
