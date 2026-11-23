//! EN-3 Prototype & Benchmark Spike: Sparse non-journaled `Resolved` store vs
//! Fully-virtual `Tree` resolution (plans/specs/engine.md §2.3–2.4,
//! plans/handoff-engine.md EN-3).
//!
//! Covers all 4 required EN-3 scenarios and measures exact heap allocations,
//! read latency, patch-edit vs constant-edit latency, and resident heap bytes
//! at 700 instances (43,400 virtual nodes, matching `instance_cost.rs`).
//!
//! Run with:
//!   cargo test -p iron_document --target aarch64-apple-darwin --test views_spike -- --nocapture --test-threads=1

use iron_document::expr::{Expr, Leaf};
use iron_document::facet::{Facet, FacetKind, Fill, LeafPath, LeafStruct, Size, Transform2d};
use iron_document::id::NodeId;
use iron_document::registry::TypeId;
use iron_document::value::{Slot, Value};
use std::alloc::{GlobalAlloc, Layout, System};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};
use std::time::Instant;

struct CountingAlloc;
static LIVE_BYTES: AtomicIsize = AtomicIsize::new(0);
static ALLOC_OPS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE_BYTES.fetch_add(l.size() as isize, Ordering::Relaxed);
        ALLOC_OPS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE_BYTES.fetch_sub(l.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        LIVE_BYTES.fetch_add(new as isize - l.size() as isize, Ordering::Relaxed);
        ALLOC_OPS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(p, l, new) }
    }
}

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn reset_alloc_ops() {
    ALLOC_OPS.store(0, Ordering::SeqCst);
}
fn alloc_ops() -> usize {
    ALLOC_OPS.load(Ordering::SeqCst)
}
fn live_bytes() -> isize {
    LIVE_BYTES.load(Ordering::SeqCst)
}

// ---------------------------------------------------------------------------
// Minimal Patch + Template + Virtual View model over real `iron_document` types
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum EffectiveKey {
    Node(NodeId),
    Children(NodeId),
    FacetSet(NodeId),
    Facet(NodeId, FacetKind),
    Leaf(Leaf),
    In(NodeId, TypeId),
    With(NodeId, FacetKind),
}

/// One node in a shared compiled template (no `scope_stamp`, mirrors
/// `engine::template::TNode` in `iron_document`).
#[derive(Clone, Debug)]
struct TemplateNode {
    local: NodeId,
    parent: Option<NodeId>,
    ty: TypeId,
    facets: Vec<Facet>,
    /// True if any slot references another template/body node and must be
    /// rewritten per instance (e.g. needle reading its entity's port).
    rewrite: bool,
    /// Optional reference target (body port) that `rewrite` relocates.
    body_ref_field: Option<(LeafPath, NodeId)>,
}

/// Shared compiled template for one `(definition, notation)` or body.
#[derive(Clone, Debug)]
struct CompiledTemplate {
    nodes: Vec<TemplateNode>,
    by_local: HashMap<NodeId, usize>,
    by_type: BTreeMap<TypeId, Vec<NodeId>>,
    by_facet: BTreeMap<FacetKind, Vec<NodeId>>,
}

impl CompiledTemplate {
    fn new(nodes: Vec<TemplateNode>) -> Arc<Self> {
        let mut by_local = HashMap::new();
        let mut by_type: BTreeMap<TypeId, Vec<NodeId>> = BTreeMap::new();
        let mut by_facet: BTreeMap<FacetKind, Vec<NodeId>> = BTreeMap::new();
        for (i, n) in nodes.iter().enumerate() {
            by_local.insert(n.local, i);
            by_type.entry(n.ty).or_default().push(n.local);
            for f in &n.facets {
                by_facet.entry(f.kind()).or_default().push(n.local);
            }
        }
        Arc::new(CompiledTemplate { nodes, by_local, by_type, by_facet })
    }
}

#[derive(Clone, Debug, PartialEq)]
enum PatchEntry {
    Set(Slot),
    Present(bool),
    FacetPresent(FacetKind, bool),
    Notation(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Strength {
    layer: u32,
    rank: u32,
    id: u32,
}

const SESSION_LAYER: u32 = u32::MAX;

#[derive(Clone, Debug, PartialEq)]
struct Patch {
    id: u32,
    layer: u32,
    rank: u32,
    /// Suffix pattern over instance owners (empty = all instances).
    suffix: Vec<NodeId>,
    /// Target node (`local` template node or authored node) + entry key.
    entries: BTreeMap<(NodeId, Option<LeafPath>), PatchEntry>,
}

impl Patch {
    fn strength(&self) -> Strength {
        Strength { layer: self.layer, rank: self.rank, id: self.id }
    }
    fn matches(&self, path: &[NodeId]) -> bool {
        path.len() >= self.suffix.len() && path[path.len() - self.suffix.len()..] == *self.suffix
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ViewSpec {
    /// Owning `use` or `entity` node id.
    owner: NodeId,
    /// Top-level root (composition or model) for scoped `In`/`With` keys.
    top_root: NodeId,
    /// Instance path (enclosing steps + this owner).
    path: Vec<NodeId>,
    /// Definition id this view instantiates.
    def: NodeId,
    /// Base notation (overridable via `PatchEntry::Notation` on `owner`).
    base_notation: String,
    /// Figured entity id (for pictures; body leaves live on `entity`, P20).
    entity: Option<NodeId>,
    /// If nested inside an outer view, its enclosing `(outer_owner, use_local)` (P18).
    enclosing_use: Option<(NodeId, NodeId)>,
}

/// Reversible ops (including dedicated solve-layer ops for solver-free undo/redo/replay).
#[derive(Clone, Debug, PartialEq)]
enum SpikeOp {
    AddPatch(Patch),
    RemovePatch(u32),
    SetPatchEntry { patch: u32, target: (NodeId, Option<LeafPath>), entry: Option<PatchEntry> },
    /// System-owned solve layer (P17): exact virtual/authored leaf, below bindings & producers.
    SetSolve { leaf: Leaf, value: Option<Slot> },
}

#[derive(Clone, Debug, Default)]
struct PatchStore {
    templates: BTreeMap<(NodeId, String), Arc<CompiledTemplate>>,
    views: BTreeMap<NodeId, ViewSpec>,
    patches: BTreeMap<u32, Patch>,
    /// In-memory patch index by target node id (as `EN-4` will maintain).
    patches_by_target: HashMap<NodeId, Vec<u32>>,
    /// Dedicated system-owned solve layer (P17): exact `Leaf`, saved, below bindings/producers.
    solve: BTreeMap<Leaf, Slot>,
    /// Simulated binding & producer overrides for 6-tier precedence checks.
    bindings: BTreeMap<Leaf, Value>,
    producers: BTreeMap<Leaf, Value>,
}

impl PatchStore {
    fn index_patch(&mut self, p: &Patch) {
        for &(target, _) in p.entries.keys() {
            let list = self.patches_by_target.entry(target).or_default();
            if !list.contains(&p.id) {
                list.push(p.id);
            }
        }
    }

    fn unindex_patch(&mut self, p: &Patch) {
        for &(target, _) in p.entries.keys() {
            if let Some(list) = self.patches_by_target.get_mut(&target) {
                list.retain(|&id| id != p.id);
                if list.is_empty() {
                    self.patches_by_target.remove(&target);
                }
            }
        }
    }

    fn apply(&mut self, op: &SpikeOp) -> SpikeOp {
        match op {
            SpikeOp::AddPatch(p) => {
                self.index_patch(p);
                self.patches.insert(p.id, p.clone());
                SpikeOp::RemovePatch(p.id)
            }
            SpikeOp::RemovePatch(id) => {
                let old = self.patches.remove(id).expect("existing patch");
                self.unindex_patch(&old);
                SpikeOp::AddPatch(old)
            }
            SpikeOp::SetPatchEntry { patch, target, entry } => {
                let p = self.patches.get_mut(patch).expect("existing patch");
                let old = match entry {
                    Some(e) => p.entries.insert(target.clone(), e.clone()),
                    None => p.entries.remove(target),
                };
                if entry.is_some() {
                    let list = self.patches_by_target.entry(target.0).or_default();
                    if !list.contains(patch) {
                        list.push(*patch);
                    }
                }
                SpikeOp::SetPatchEntry { patch: *patch, target: target.clone(), entry: old }
            }
            SpikeOp::SetSolve { leaf, value } => {
                let old = match value {
                    Some(v) => self.solve.insert(*leaf, v.clone()),
                    None => self.solve.remove(leaf),
                };
                SpikeOp::SetSolve { leaf: *leaf, value: old }
            }
        }
    }

    fn apply_all(&mut self, ops: &[SpikeOp]) -> Vec<SpikeOp> {
        let mut inv: Vec<SpikeOp> = ops.iter().map(|o| self.apply(o)).collect();
        inv.reverse();
        inv
    }

    fn winning_entry(&self, path: &[NodeId], key: &(NodeId, Option<LeafPath>), session_only: Option<bool>) -> Option<&PatchEntry> {
        let ids = self.patches_by_target.get(&key.0)?;
        let mut best: Option<(Strength, &PatchEntry)> = None;
        for id in ids {
            let Some(p) = self.patches.get(id) else { continue };
            match session_only {
                Some(true) if p.layer != SESSION_LAYER => continue,
                Some(false) if p.layer == SESSION_LAYER => continue,
                _ => {}
            }
            if p.matches(path)
                && let Some(e) = p.entries.get(key)
                && best.is_none_or(|(s, _)| p.strength() > s)
            {
                best = Some((p.strength(), e));
            }
        }
        best.map(|(_, e)| e)
    }

    fn resolved_notation(&self, view: &ViewSpec) -> String {
        // Enclosing path governs `use.notation` on `view.owner` (P18).
        let enc_path = &view.path[..view.path.len().saturating_sub(1)];
        let check_path = if enc_path.is_empty() { &view.path[..] } else { enc_path };
        if let Some(PatchEntry::Notation(n)) = self.winning_entry(check_path, &(view.owner, None), Some(false)) {
            return n.clone();
        }
        if let Some(PatchEntry::Notation(n)) = self.winning_entry(&view.path, &(view.owner, None), Some(false)) {
            return n.clone();
        }
        view.base_notation.clone()
    }

    fn is_view_present(&self, owner: NodeId) -> bool {
        let Some(v) = self.views.get(&owner) else { return false };
        if let Some((outer_owner, use_local)) = v.enclosing_use {
            if !self.is_view_present(outer_owner) {
                return false;
            }
            let outer = &self.views[&outer_owner];
            if let Some(PatchEntry::Present(false)) = self.winning_entry(&outer.path, &(use_local, None), Some(false)) {
                return false;
            }
        }
        true
    }

    fn is_node_present(&self, view: &ViewSpec, tpl: &CompiledTemplate, local: NodeId) -> bool {
        if !self.is_view_present(view.owner) {
            return false;
        }
        let Some(&idx) = tpl.by_local.get(&local) else { return false };
        if let Some(PatchEntry::Present(false)) = self.winning_entry(&view.path, &(local, None), Some(false)) {
            return false;
        }
        match tpl.nodes[idx].parent {
            Some(p) => self.is_node_present(view, tpl, p),
            None => true,
        }
    }

    /// Rewrite a template node's facets for a specific view (when `rewrite == true`).
    fn rewrite_facets(&self, view: &ViewSpec, tnode: &TemplateNode) -> Vec<Facet> {
        let mut facets = tnode.facets.clone();
        if let Some((path, body_port_local)) = tnode.body_ref_field {
            let target_owner = view.entity.unwrap_or(view.owner);
            let target_port = NodeId::expanded(target_owner, body_port_local).unwrap();
            let expr = Expr::parse(&format!("{target_port}.port.signal * 2")).unwrap();
            if let Some(f) = facets.iter_mut().find(|f| f.kind() == path.facet) {
                let _ = f.set(path.field, Slot::Bound(expr));
            }
        }
        facets
    }
}

// ---------------------------------------------------------------------------
// Candidate A: Sparse non-journaled `Resolved` store (Stage 1 eager)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct ResolvedView {
    template: Arc<CompiledTemplate>,
    notation: String,
    absent: BTreeSet<NodeId>,
    /// Virtual nodes under this view that live in `SparseResolvedViews::resolved`.
    resolved_nodes: Vec<NodeId>,
}

#[derive(Default)]
struct SparseResolvedViews {
    views: BTreeMap<NodeId, ResolvedView>,
    /// Sparse non-journaled resolved facet store: ONLY rewritten or patched
    /// virtual nodes live here. Unpatched constant virtual nodes borrow
    /// directly from `ResolvedView::template` with 0 allocations!
    resolved: HashMap<NodeId, Vec<Facet>>,
    /// Index of which virtual nodes have active patches/solve entries, for fast O(1) patch edits.
    patched_by_view: HashMap<NodeId, BTreeSet<NodeId>>,
}

impl SparseResolvedViews {
    /// Stage 1 reconcile: updates affected views and emits exact effective keys.
    fn reconcile(&mut self, store: &PatchStore, dirty_owners: Option<&[NodeId]>) -> BTreeSet<EffectiveKey> {
        let mut emitted = BTreeSet::new();
        let owners: Vec<NodeId> = match dirty_owners {
            Some(list) => list.to_vec(),
            None => store.views.keys().copied().collect(),
        };
        for owner in owners {
            let Some(spec) = store.views.get(&owner) else { continue };
            let notation = store.resolved_notation(spec);
            let tpl = store.templates[&(spec.def, notation.clone())].clone();
            let view_live = store.is_view_present(owner);

            let old = self.views.remove(&owner);
            let old_present: BTreeSet<NodeId> = match &old {
                Some(ov) => ov
                    .template
                    .nodes
                    .iter()
                    .map(|n| n.local)
                    .filter(|l| !ov.absent.contains(l))
                    .collect(),
                None => BTreeSet::new(),
            };
            let old_facets: HashMap<NodeId, Vec<Facet>> = match &old {
                Some(ov) => {
                    let mut m = HashMap::new();
                    for &vid in &ov.resolved_nodes {
                        if let Some(fs) = self.resolved.remove(&vid) {
                            m.insert(vid, fs);
                        }
                    }
                    m
                }
                None => HashMap::new(),
            };

            let mut absent = BTreeSet::new();
            let mut resolved_nodes = Vec::new();
            let mut new_present = BTreeSet::new();
            let mut patched_set = BTreeSet::new();

            for tnode in &tpl.nodes {
                let vid = NodeId::expanded(owner, tnode.local).unwrap();
                let present = view_live && store.is_node_present(spec, &tpl, tnode.local);
                if !present {
                    absent.insert(tnode.local);
                    continue;
                }
                new_present.insert(tnode.local);

                // Check if any patch or solve entry touches this node.
                let mut touched = tnode.rewrite;
                let mut optional_facets: BTreeMap<FacetKind, bool> = BTreeMap::new();
                let mut leaf_overrides: BTreeSet<LeafPath> = BTreeSet::new();
                if let Some(pids) = store.patches_by_target.get(&tnode.local) {
                    for pid in pids {
                        let Some(p) = store.patches.get(pid) else { continue };
                        if !p.matches(&spec.path) {
                            continue;
                        }
                        for ((target, opt_path), entry) in &p.entries {
                            if *target != tnode.local {
                                continue;
                            }
                            match (opt_path, entry) {
                                (None, PatchEntry::FacetPresent(fk, on)) => {
                                    optional_facets.insert(*fk, *on);
                                    touched = true;
                                }
                                (Some(lp), PatchEntry::Set(_)) => {
                                    leaf_overrides.insert(*lp);
                                    touched = true;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                for sl in store.solve.keys() {
                    if sl.node == vid {
                        leaf_overrides.insert(sl.path);
                        touched = true;
                    }
                }

                if touched {
                    patched_set.insert(tnode.local);
                    let mut facets = if tnode.rewrite { store.rewrite_facets(spec, tnode) } else { tnode.facets.clone() };
                    // Apply optional facet presence (P21).
                    for (fk, on) in optional_facets {
                        if on && !facets.iter().any(|f| f.kind() == fk) {
                            facets.push(fk.default_facet());
                        } else if !on {
                            facets.retain(|f| f.kind() != fk);
                        }
                    }
                    // Apply winning authored patch -> solve layer -> session patch onto facets.
                    for lp in leaf_overrides {
                        let key = (tnode.local, Some(lp));
                        let slot = if let Some(PatchEntry::Set(s)) = store.winning_entry(&spec.path, &key, Some(true)) {
                            Some(s.clone())
                        } else if let Some(s) = store.solve.get(&Leaf { node: vid, path: lp }) {
                            Some(s.clone())
                        } else if let Some(PatchEntry::Set(s)) = store.winning_entry(&spec.path, &key, Some(false)) {
                            Some(s.clone())
                        } else {
                            None
                        };
                        if let Some(s) = slot
                            && let Some(f) = facets.iter_mut().find(|f| f.kind() == lp.facet)
                        {
                            let _ = f.set(lp.field, s);
                        }
                    }
                    // Diff against old facets to emit exact effective keys.
                    let changed = old_facets.get(&vid) != Some(&facets) || !old_present.contains(&tnode.local);
                    if changed {
                        for f in &facets {
                            emitted.insert(EffectiveKey::Facet(vid, f.kind()));
                            for field in f.field_names() {
                                if let Ok(lp) = LeafPath::new(f.kind(), field) {
                                    emitted.insert(EffectiveKey::Leaf(Leaf { node: vid, path: lp }));
                                }
                            }
                        }
                    }
                    self.resolved.insert(vid, facets);
                    resolved_nodes.push(vid);
                } else if !old_present.contains(&tnode.local) || old_facets.contains_key(&vid) {
                    // Newly appeared or reverted from resolved back to template borrow.
                    for f in &tnode.facets {
                        emitted.insert(EffectiveKey::Facet(vid, f.kind()));
                        for field in f.field_names() {
                            if let Ok(lp) = LeafPath::new(f.kind(), field) {
                                emitted.insert(EffectiveKey::Leaf(Leaf { node: vid, path: lp }));
                            }
                        }
                    }
                }
            }

            // Emit effective structural keys for appeared / disappeared nodes.
            let notation_changed = old.as_ref().is_none_or(|ov| ov.notation != notation);
            if notation_changed || old_present != new_present {
                emitted.insert(EffectiveKey::Children(owner));
            }
            for &local in old_present.difference(&new_present) {
                let vid = NodeId::expanded(owner, local).unwrap();
                emitted.insert(EffectiveKey::Node(vid));
                emitted.insert(EffectiveKey::FacetSet(vid));
                if let Some(ov) = &old
                    && let Some(&i) = ov.template.by_local.get(&local)
                {
                    let tn = &ov.template.nodes[i];
                    emitted.insert(EffectiveKey::In(spec.top_root, tn.ty));
                    for f in &tn.facets {
                        emitted.insert(EffectiveKey::With(spec.top_root, f.kind()));
                    }
                }
            }
            for &local in new_present.difference(&old_present) {
                let vid = NodeId::expanded(owner, local).unwrap();
                emitted.insert(EffectiveKey::Node(vid));
                emitted.insert(EffectiveKey::FacetSet(vid));
                let tn = &tpl.nodes[tpl.by_local[&local]];
                emitted.insert(EffectiveKey::In(spec.top_root, tn.ty));
                for f in &tn.facets {
                    emitted.insert(EffectiveKey::With(spec.top_root, f.kind()));
                }
            }

            self.patched_by_view.insert(owner, patched_set);
            self.views.insert(owner, ResolvedView { template: tpl, notation, absent, resolved_nodes });
        }
        emitted
    }

    /// Zero-allocation borrowed `&Facet` read (preserves `Tree::facet` / `Tree::get::<T>()`!).
    fn facet(&self, id: NodeId, kind: FacetKind) -> Option<&Facet> {
        let (owner, local) = id.expanded_parts()?;
        let rv = self.views.get(&owner)?;
        if rv.absent.contains(&local) {
            return None;
        }
        if let Some(fs) = self.resolved.get(&id) {
            return fs.iter().find(|f| f.kind() == kind);
        }
        let &idx = rv.template.by_local.get(&local)?;
        rv.template.nodes[idx].facets.iter().find(|f| f.kind() == kind)
    }

    /// Base unpatched template facet for inspector/planner (`Tree::base_facet`).
    fn base_facet(&self, id: NodeId, kind: FacetKind) -> Option<&Facet> {
        let (owner, local) = id.expanded_parts()?;
        let rv = self.views.get(&owner)?;
        let &idx = rv.template.by_local.get(&local)?;
        rv.template.nodes[idx].facets.iter().find(|f| f.kind() == kind)
    }

    /// Zero-allocation borrowed `&Slot` read.
    fn leaf(&self, id: NodeId, path: LeafPath) -> Option<&Slot> {
        self.facet(id, path.facet)?.get(path.field)
    }

    /// Full 6-tier effective value read (engine.md §2.4).
    fn effective_value(&self, store: &PatchStore, id: NodeId, path: LeafPath) -> Option<Value> {
        let (owner, local) = id.expanded_parts()?;
        let rv = self.views.get(&owner)?;
        if rv.absent.contains(&local) || !rv.template.by_local.contains_key(&local) {
            return None;
        }
        // Facet presence check.
        let slot = self.leaf(id, path)?;
        let spec = store.views.get(&owner)?;
        let leaf = Leaf { node: id, path };
        // 1. Session patch layer
        if let Some(PatchEntry::Set(s)) = store.winning_entry(&spec.path, &(local, Some(path)), Some(true)) {
            return s.constant().cloned();
        }
        // 2. Binding value
        if let Some(v) = store.bindings.get(&leaf) {
            return Some(v.clone());
        }
        // 3. Producer signal
        if let Some(v) = store.producers.get(&leaf) {
            return Some(v.clone());
        }
        // 4–6. Solve layer > winning authored patch > base (already merged in `slot`)
        slot.constant().cloned()
    }

    /// Scoped index query `In(root, ty)` without materialising virtual NodeIds in a global BTreeSet.
    fn nodes_of_type_in(&self, store: &PatchStore, root: NodeId, ty: TypeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        for (&owner, rv) in &self.views {
            if store.views.get(&owner).is_none_or(|s| s.top_root != root) {
                continue;
            }
            if let Some(locals) = rv.template.by_type.get(&ty) {
                for &l in locals {
                    if !rv.absent.contains(&l) {
                        out.push(NodeId::expanded(owner, l).unwrap());
                    }
                }
            }
        }
        out
    }

    /// Scoped index query `With(root, kind)` over virtual views.
    fn nodes_with_in(&self, store: &PatchStore, root: NodeId, kind: FacetKind) -> Vec<NodeId> {
        let mut out = Vec::new();
        for (&owner, rv) in &self.views {
            if store.views.get(&owner).is_none_or(|s| s.top_root != root) {
                continue;
            }
            if let Some(locals) = rv.template.by_facet.get(&kind) {
                for &l in locals {
                    if !rv.absent.contains(&l) {
                        out.push(NodeId::expanded(owner, l).unwrap());
                    }
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Candidate B: Fully-virtual per-field resolution on `Tree` (no `Resolved` table)
// ---------------------------------------------------------------------------

struct FullyVirtualViews;

impl FullyVirtualViews {
    /// Facet read must return `Cow<'_, Facet>` (cloning whenever rewritten or patched).
    fn facet<'a>(&self, store: &'a PatchStore, id: NodeId, kind: FacetKind) -> Option<Cow<'a, Facet>> {
        let (owner, local) = id.expanded_parts()?;
        let spec = store.views.get(&owner)?;
        let notation = store.resolved_notation(spec);
        let tpl = store.templates.get(&(spec.def, notation))?;
        if !store.is_node_present(spec, tpl, local) {
            return None;
        }
        let &idx = tpl.by_local.get(&local)?;
        let tnode = &tpl.nodes[idx];

        let mut opt_on: Option<bool> = None;
        let mut patch_fields: Vec<LeafPath> = Vec::new();
        if let Some(pids) = store.patches_by_target.get(&local) {
            for pid in pids {
                let Some(p) = store.patches.get(pid) else { continue };
                if !p.matches(&spec.path) {
                    continue;
                }
                for ((target, opt_path), entry) in &p.entries {
                    if *target != local {
                        continue;
                    }
                    match (opt_path, entry) {
                        (None, PatchEntry::FacetPresent(fk, on)) if *fk == kind => opt_on = Some(*on),
                        (Some(lp), PatchEntry::Set(_)) if lp.facet == kind => {
                            if !patch_fields.contains(lp) {
                                patch_fields.push(*lp);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        for sl in store.solve.keys() {
            if sl.node == id && sl.path.facet == kind && !patch_fields.contains(&sl.path) {
                patch_fields.push(sl.path);
            }
        }

        let base_f = tnode.facets.iter().find(|f| f.kind() == kind);
        if opt_on == Some(false) {
            return None;
        }
        if base_f.is_none() && opt_on != Some(true) {
            return None;
        }
        if !tnode.rewrite && patch_fields.is_empty() && opt_on.is_none() {
            return base_f.map(Cow::Borrowed);
        }
        let mut owned = match base_f {
            Some(f) => f.clone(),
            None => kind.default_facet(),
        };
        if tnode.rewrite
            && let Some((lp, body_port_local)) = tnode.body_ref_field
            && lp.facet == kind
        {
            let target_owner = spec.entity.unwrap_or(spec.owner);
            let target_port = NodeId::expanded(target_owner, body_port_local).unwrap();
            let expr = Expr::parse(&format!("{target_port}.port.signal * 2")).unwrap();
            let _ = owned.set(lp.field, Slot::Bound(expr));
        }
        for lp in patch_fields {
            let key = (local, Some(lp));
            let slot = if let Some(PatchEntry::Set(s)) = store.winning_entry(&spec.path, &key, Some(true)) {
                Some(s.clone())
            } else if let Some(s) = store.solve.get(&Leaf { node: id, path: lp }) {
                Some(s.clone())
            } else if let Some(PatchEntry::Set(s)) = store.winning_entry(&spec.path, &key, Some(false)) {
                Some(s.clone())
            } else {
                None
            };
            if let Some(s) = slot {
                let _ = owned.set(lp.field, s);
            }
        }
        Some(Cow::Owned(owned))
    }

    /// Leaf-level virtual read (avoids cloning the whole `Facet`, still allocates for
    /// notation string + rewritten `Expr`).
    fn leaf<'a>(&self, store: &'a PatchStore, id: NodeId, path: LeafPath) -> Option<Cow<'a, Slot>> {
        let (owner, local) = id.expanded_parts()?;
        let spec = store.views.get(&owner)?;
        let notation = store.resolved_notation(spec);
        let tpl = store.templates.get(&(spec.def, notation))?;
        if !store.is_node_present(spec, tpl, local) {
            return None;
        }
        let &idx = tpl.by_local.get(&local)?;
        let tnode = &tpl.nodes[idx];
        let key = (local, Some(path));
        if let Some(PatchEntry::Set(s)) = store.winning_entry(&spec.path, &key, Some(true)) {
            return Some(Cow::Borrowed(s));
        }
        if let Some(s) = store.solve.get(&Leaf { node: id, path }) {
            return Some(Cow::Borrowed(s));
        }
        if let Some(PatchEntry::Set(s)) = store.winning_entry(&spec.path, &key, Some(false)) {
            return Some(Cow::Borrowed(s));
        }
        if tnode.rewrite
            && let Some((lp, body_port_local)) = tnode.body_ref_field
            && lp == path
        {
            let target_owner = spec.entity.unwrap_or(spec.owner);
            let target_port = NodeId::expanded(target_owner, body_port_local).unwrap();
            let expr = Expr::parse(&format!("{target_port}.port.signal * 2")).unwrap();
            return Some(Cow::Owned(Slot::Bound(expr)));
        }
        tnode.facets.iter().find(|f| f.kind() == path.facet)?.get(path.field).map(Cow::Borrowed)
    }
}

// ---------------------------------------------------------------------------
// Helpers for building test templates
// ---------------------------------------------------------------------------

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}
fn lp(s: &str) -> LeafPath {
    s.parse().unwrap()
}
fn col(r: f32, g: f32, b: f32) -> Value {
    Value::Color([r, g, b, 1.0])
}
fn rect_node(local: NodeId, parent: Option<NodeId>, x: f64, color: Value, rewrite: bool, body_ref: Option<(LeafPath, NodeId)>) -> TemplateNode {
    let mut t = Transform2d::default();
    t.x = Slot::Const(Value::Number(x));
    let mut fill = Fill::default();
    fill.color = Slot::Const(color);
    TemplateNode {
        local,
        parent,
        ty: ty("rect"),
        facets: vec![t.wrap(), Size::default().wrap(), fill.wrap()],
        rewrite,
        body_ref_field: body_ref,
    }
}

// ---------------------------------------------------------------------------
// Scenario 1: Nested notation switch
// ---------------------------------------------------------------------------

#[test]
fn scenario_1_nested_notation_switch() {
    let doc = iron_document::Document::new();
    let ids = doc.next_ids(12);
    let [comp, bench_def, meter_def, bench_use, inner_use, cartoon_face, cartoon_needle, schem_box, schem_pin, body_port, _, _] = ids.try_into().unwrap();

    let mut store = PatchStore::default();
    store.templates.insert(
        (bench_def, "cartoon".into()),
        CompiledTemplate::new(vec![TemplateNode {
            local: inner_use,
            parent: None,
            ty: ty("use"),
            facets: vec![Transform2d::default().wrap()],
            rewrite: false,
            body_ref_field: None,
        }]),
    );
    store.templates.insert(
        (meter_def, "cartoon".into()),
        CompiledTemplate::new(vec![
            rect_node(cartoon_face, None, 0.0, col(0.8, 0.8, 0.8), false, None),
            rect_node(cartoon_needle, Some(cartoon_face), 10.0, col(0.8, 0.0, 0.0), true, Some((lp("transform2d.x"), body_port))),
        ]),
    );
    store.templates.insert(
        (meter_def, "schematic".into()),
        CompiledTemplate::new(vec![
            rect_node(schem_box, None, 0.0, col(0.1, 0.1, 0.1), false, None),
            rect_node(schem_pin, Some(schem_box), 5.0, col(0.0, 0.7, 0.0), false, None),
        ]),
    );

    let nested_owner = NodeId::expanded(bench_use, inner_use).unwrap();
    store.views.insert(
        bench_use,
        ViewSpec {
            owner: bench_use,
            top_root: comp,
            path: vec![bench_use],
            def: bench_def,
            base_notation: "cartoon".into(),
            entity: None,
            enclosing_use: None,
        },
    );
    store.views.insert(
        nested_owner,
        ViewSpec {
            owner: nested_owner,
            top_root: comp,
            path: vec![bench_use, nested_owner],
            def: meter_def,
            base_notation: "cartoon".into(),
            entity: None,
            enclosing_use: Some((bench_use, inner_use)),
        },
    );

    let orange = col(1.0, 0.5, 0.0);
    // Patch targeting schematic's `schem_box` fill color: initially inert while in cartoon notation.
    store.apply(&SpikeOp::AddPatch(Patch {
        id: 1,
        layer: 1,
        rank: 1,
        suffix: vec![nested_owner],
        entries: BTreeMap::from([((schem_box, Some(lp("fill.color"))), PatchEntry::Set(Slot::Const(orange.clone())))]),
    }));

    let mut sparse = SparseResolvedViews::default();
    let virt = FullyVirtualViews;
    sparse.reconcile(&store, None);

    let v_face = NodeId::expanded(nested_owner, cartoon_face).unwrap();
    let v_box = NodeId::expanded(nested_owner, schem_box).unwrap();
    assert!(sparse.facet(v_face, FacetKind::Fill).is_some());
    assert!(sparse.facet(v_box, FacetKind::Fill).is_none());
    assert!(virt.facet(&store, v_box, FacetKind::Fill).is_none());

    // Switch nested `use.notation` from "cartoon" to "schematic" via patch!
    store.apply(&SpikeOp::AddPatch(Patch {
        id: 2,
        layer: 1,
        rank: 2,
        suffix: vec![bench_use],
        entries: BTreeMap::from([((nested_owner, None), PatchEntry::Notation("schematic".into()))]),
    }));
    let emitted = sparse.reconcile(&store, Some(&[nested_owner]));

    // Old cartoon nodes disappeared; new schematic nodes appeared with patch #1 active!
    assert!(sparse.facet(v_face, FacetKind::Fill).is_none());
    assert_eq!(sparse.leaf(v_box, lp("fill.color")).and_then(Slot::constant), Some(&orange));
    assert_eq!(virt.leaf(&store, v_box, lp("fill.color")).as_deref().and_then(Slot::constant), Some(&orange));
    // Exact effective keys emitted for both vanished and revealed nodes.
    assert!(emitted.contains(&EffectiveKey::Children(nested_owner)));
    assert!(emitted.contains(&EffectiveKey::Node(v_face)));
    assert!(emitted.contains(&EffectiveKey::Node(v_box)));
    assert!(emitted.contains(&EffectiveKey::Leaf(Leaf { node: v_box, path: lp("fill.color") })));
    assert!(emitted.contains(&EffectiveKey::In(comp, ty("rect"))));
}

// ---------------------------------------------------------------------------
// Scenario 2: Shared entity with 2 pictures (P20)
// ---------------------------------------------------------------------------

#[test]
fn scenario_2_shared_entity_with_two_pictures() {
    let doc = iron_document::Document::new();
    let ids = doc.next_ids(10);
    let [model, comp1, comp2, meter_def, entity, port_local, pic1, pic2, needle_local, bezel_local] = ids.try_into().unwrap();

    let gray = col(0.2, 0.2, 0.2);
    let blue = col(0.0, 0.3, 1.0);
    let mut store = PatchStore::default();
    // Body template (keyed on entity)
    store.templates.insert(
        (meter_def, "body".into()),
        CompiledTemplate::new(vec![TemplateNode {
            local: port_local,
            parent: None,
            ty: ty("port"),
            facets: vec![iron_document::facet::Port::default().wrap()],
            rewrite: false,
            body_ref_field: None,
        }]),
    );
    // Picture template (keyed on each `use`, reading entity's body port)
    store.templates.insert(
        (meter_def, "cartoon".into()),
        CompiledTemplate::new(vec![
            rect_node(bezel_local, None, 0.0, gray.clone(), false, None),
            rect_node(needle_local, Some(bezel_local), 0.0, col(1.0, 0.0, 0.0), true, Some((lp("transform2d.x"), port_local))),
        ]),
    );

    store.views.insert(entity, ViewSpec { owner: entity, top_root: model, path: vec![entity], def: meter_def, base_notation: "body".into(), entity: None, enclosing_use: None });
    store.views.insert(pic1, ViewSpec { owner: pic1, top_root: comp1, path: vec![pic1], def: meter_def, base_notation: "cartoon".into(), entity: Some(entity), enclosing_use: None });
    store.views.insert(pic2, ViewSpec { owner: pic2, top_root: comp2, path: vec![pic2], def: meter_def, base_notation: "cartoon".into(), entity: Some(entity), enclosing_use: None });

    // Picture-local patch on `pic2` only: changes bezel color without duplicating entity body.
    store.apply(&SpikeOp::AddPatch(Patch {
        id: 1,
        layer: 1,
        rank: 1,
        suffix: vec![pic2],
        entries: BTreeMap::from([((bezel_local, Some(lp("fill.color"))), PatchEntry::Set(Slot::Const(blue.clone())))]),
    }));

    let mut sparse = SparseResolvedViews::default();
    let virt = FullyVirtualViews;
    sparse.reconcile(&store, None);

    // Body port exists ONCE (under `entity`), not duplicated per picture.
    assert_eq!(sparse.nodes_of_type_in(&store, model, ty("port")), vec![NodeId::expanded(entity, port_local).unwrap()]);
    assert_eq!(sparse.nodes_with_in(&store, model, FacetKind::Port), vec![NodeId::expanded(entity, port_local).unwrap()]);
    assert!(sparse.nodes_of_type_in(&store, comp1, ty("port")).is_empty());
    assert!(sparse.nodes_of_type_in(&store, comp2, ty("port")).is_empty());

    // Both pictures' rewritten needles bind to the SAME entity port!
    let n1 = NodeId::expanded(pic1, needle_local).unwrap();
    let n2 = NodeId::expanded(pic2, needle_local).unwrap();
    assert_eq!(sparse.leaf(n1, lp("transform2d.x")), sparse.leaf(n2, lp("transform2d.x")));
    assert_eq!(virt.leaf(&store, n1, lp("transform2d.x")).as_deref(), sparse.leaf(n1, lp("transform2d.x")));

    // Yet the two pictures disagree cleanly on bezel fill color!
    let b1 = NodeId::expanded(pic1, bezel_local).unwrap();
    let b2 = NodeId::expanded(pic2, bezel_local).unwrap();
    assert_eq!(sparse.leaf(b1, lp("fill.color")).and_then(Slot::constant), Some(&gray));
    assert_eq!(sparse.leaf(b2, lp("fill.color")).and_then(Slot::constant), Some(&blue));
    // Unpatched `b1` is NOT in `sparse.resolved` (borrowed directly from template)!
    assert!(!sparse.resolved.contains_key(&b1));
    assert!(sparse.resolved.contains_key(&b2));
}

// ---------------------------------------------------------------------------
// Scenario 3: Initially-absent read woken when revealed
// ---------------------------------------------------------------------------

#[test]
fn scenario_3_initially_absent_read() {
    let doc = iron_document::Document::new();
    let ids = doc.next_ids(6);
    let [comp, def, use1, badge_local, _, _] = ids.try_into().unwrap();
    let green = col(0.0, 0.8, 0.4);

    let mut store = PatchStore::default();
    store.templates.insert((def, "free".into()), CompiledTemplate::new(vec![rect_node(badge_local, None, 10.0, green.clone(), false, None)]));
    store.views.insert(use1, ViewSpec { owner: use1, top_root: comp, path: vec![use1], def, base_notation: "free".into(), entity: None, enclosing_use: None });

    // Initially absent via presence patch + optional Stroke facet absent.
    store.apply(&SpikeOp::AddPatch(Patch {
        id: 1,
        layer: 1,
        rank: 1,
        suffix: vec![use1],
        entries: BTreeMap::from([((badge_local, None), PatchEntry::Present(false))]),
    }));

    let mut sparse = SparseResolvedViews::default();
    let virt = FullyVirtualViews;
    sparse.reconcile(&store, None);

    let v_badge = NodeId::expanded(use1, badge_local).unwrap();
    // Reader reads absent node and records effective keys (Node + Facet + Leaf).
    let reader_keys = BTreeSet::from([
        EffectiveKey::Node(v_badge),
        EffectiveKey::Facet(v_badge, FacetKind::Fill),
        EffectiveKey::Leaf(Leaf { node: v_badge, path: lp("fill.color") }),
    ]);
    assert!(sparse.facet(v_badge, FacetKind::Fill).is_none());
    assert!(virt.facet(&store, v_badge, FacetKind::Fill).is_none());

    // Patch reveals `badge_local` AND adds optional `Stroke` facet (P21)!
    store.apply(&SpikeOp::AddPatch(Patch {
        id: 2,
        layer: 1,
        rank: 2,
        suffix: vec![use1],
        entries: BTreeMap::from([
            ((badge_local, None), PatchEntry::Present(true)),
            ((badge_local, None), PatchEntry::FacetPresent(FacetKind::Stroke, true)),
            ((badge_local, Some(lp("stroke.width"))), PatchEntry::Set(Slot::Const(Value::Number(3.5)))),
        ]),
    }));
    let emitted = sparse.reconcile(&store, Some(&[use1]));

    // Emitted keys wake the reader that subscribed while absent!
    assert!(!emitted.is_disjoint(&reader_keys), "reader must wake on reveal: {emitted:?}");
    assert_eq!(sparse.leaf(v_badge, lp("fill.color")).and_then(Slot::constant), Some(&green));
    assert_eq!(sparse.leaf(v_badge, lp("stroke.width")).and_then(Slot::constant), Some(&Value::Number(3.5)));
    assert_eq!(
        virt.facet(&store, v_badge, FacetKind::Stroke).as_deref().and_then(|f| f.get("width")).and_then(Slot::constant),
        Some(&Value::Number(3.5))
    );
}

// ---------------------------------------------------------------------------
// Scenario 4: Copy-targeted edit + constraint release + solver-free undo/redo/replay
// ---------------------------------------------------------------------------

#[test]
fn scenario_4_copy_targeted_edit_release_and_solver_free_undo_redo_replay() {
    let doc = iron_document::Document::new();
    let ids = doc.next_ids(5);
    let [comp, def, use1, part_local, _] = ids.try_into().unwrap();

    let mut store = PatchStore::default();
    store.templates.insert((def, "free".into()), CompiledTemplate::new(vec![rect_node(part_local, None, 10.0, col(0.3, 0.3, 0.3), false, None)]));
    store.views.insert(use1, ViewSpec { owner: use1, top_root: comp, path: vec![use1], def, base_notation: "free".into(), entity: None, enclosing_use: None });

    let v_part = NodeId::expanded(use1, part_local).unwrap();
    let x_path = lp("transform2d.x");
    let x_leaf = Leaf { node: v_part, path: x_path };
    let mut sparse = SparseResolvedViews::default();
    let virt = FullyVirtualViews;
    sparse.reconcile(&store, None);

    // Authored instance override sets x = 20.
    store.apply(&SpikeOp::AddPatch(Patch {
        id: 1,
        layer: 1,
        rank: 1,
        suffix: vec![use1],
        entries: BTreeMap::from([((part_local, Some(x_path)), PatchEntry::Set(Slot::Const(Value::Number(20.0))))]),
    }));
    sparse.reconcile(&store, Some(&[use1]));
    assert_eq!(sparse.effective_value(&store, v_part, x_path), Some(Value::Number(20.0)));

    // Stage 4 constraint solve writes x = 55 into dedicated solve layer (P17).
    let tx_solve = vec![SpikeOp::SetSolve { leaf: x_leaf, value: Some(Slot::Const(Value::Number(55.0))) }];
    let inv_solve = store.apply_all(&tx_solve);
    sparse.reconcile(&store, Some(&[use1]));
    assert_eq!(sparse.effective_value(&store, v_part, x_path), Some(Value::Number(55.0)));
    assert_eq!(virt.leaf(&store, v_part, x_path).as_deref(), Some(&Slot::Const(Value::Number(55.0))));
    // Base facet is untouched (still template 10.0) for inspector/planner.
    assert_eq!(sparse.base_facet(v_part, FacetKind::Transform2d).and_then(|f| f.get("x")).and_then(Slot::constant), Some(&Value::Number(10.0)));

    // Precedence check (engine.md §2.4): producer > solve, binding > producer, session > binding.
    store.producers.insert(x_leaf, Value::Number(70.0));
    assert_eq!(sparse.effective_value(&store, v_part, x_path), Some(Value::Number(70.0)));
    store.bindings.insert(x_leaf, Value::Number(80.0));
    assert_eq!(sparse.effective_value(&store, v_part, x_path), Some(Value::Number(80.0)));
    store.bindings.remove(&x_leaf);
    store.producers.remove(&x_leaf);

    // Copy-targeted authored edit (x = 35): clears solve entry first as derived op + writes authored patch!
    let tx_edit = vec![
        SpikeOp::SetSolve { leaf: x_leaf, value: None },
        SpikeOp::SetPatchEntry { patch: 1, target: (part_local, Some(x_path)), entry: Some(PatchEntry::Set(Slot::Const(Value::Number(35.0)))) },
    ];
    let inv_edit = store.apply_all(&tx_edit);
    sparse.reconcile(&store, Some(&[use1]));
    assert!(store.solve.is_empty());
    assert_eq!(sparse.effective_value(&store, v_part, x_path), Some(Value::Number(35.0)));

    // Solver-free UNDO: applies recorded inverse ops only -> restores solved pose 55!
    let redo_edit = store.apply_all(&inv_edit);
    sparse.reconcile(&store, Some(&[use1]));
    assert_eq!(sparse.effective_value(&store, v_part, x_path), Some(Value::Number(55.0)));

    // Constraint deletion releases solve entry -> reveals underlying authored patch (20)!
    let tx_release = vec![SpikeOp::SetSolve { leaf: x_leaf, value: None }];
    let inv_release = store.apply_all(&tx_release);
    sparse.reconcile(&store, Some(&[use1]));
    assert_eq!(sparse.effective_value(&store, v_part, x_path), Some(Value::Number(20.0)));

    // Solver-free UNDO of release -> restores 55; REDO of edit -> 35; REPLAY from start matches!
    store.apply_all(&inv_release);
    sparse.reconcile(&store, Some(&[use1]));
    assert_eq!(sparse.effective_value(&store, v_part, x_path), Some(Value::Number(55.0)));
    store.apply_all(&redo_edit);
    sparse.reconcile(&store, Some(&[use1]));
    assert_eq!(sparse.effective_value(&store, v_part, x_path), Some(Value::Number(35.0)));
    store.apply_all(&inv_edit);
    store.apply_all(&inv_solve);
    sparse.reconcile(&store, Some(&[use1]));
    assert_eq!(sparse.effective_value(&store, v_part, x_path), Some(Value::Number(20.0)));
}

// ---------------------------------------------------------------------------
// Benchmark: Sparse `Resolved` store vs Fully-virtual `Tree` at 700 instances
// (each with 60 constant ticks + 1 rewritten needle + 1 patched bezel = 43,400 nodes)
// ---------------------------------------------------------------------------

#[test]
fn benchmark_sparse_resolved_vs_fully_virtual_at_700_instances() {
    const INSTANCES: usize = 700;
    const TICKS: usize = 60;

    let doc = iron_document::Document::new();
    let all_ids = doc.next_ids(5 + TICKS + INSTANCES);
    let (comp, meter_def, body_port, bezel_local, needle_local) = (all_ids[0], all_ids[1], all_ids[2], all_ids[3], all_ids[4]);
    let tick_locals = &all_ids[5..5 + TICKS];
    let owners = &all_ids[5 + TICKS..];

    let mut tpl_nodes = Vec::with_capacity(TICKS + 2);
    tpl_nodes.push(rect_node(bezel_local, None, 0.0, col(0.2, 0.2, 0.2), false, None));
    tpl_nodes.push(rect_node(needle_local, Some(bezel_local), 0.0, col(0.9, 0.1, 0.1), true, Some((lp("transform2d.x"), body_port))));
    for (i, &tid) in tick_locals.iter().enumerate() {
        tpl_nodes.push(rect_node(tid, Some(bezel_local), i as f64 * 2.0, col(0.5, 0.5, 0.5), false, None));
    }
    let tpl = CompiledTemplate::new(tpl_nodes);

    // 1. Measure materialized baseline (copying all 43,400 nodes' facets, like `Docs::copies`).
    let b0 = live_bytes();
    let mut materialized: HashMap<NodeId, Vec<Facet>> = HashMap::with_capacity(INSTANCES * (TICKS + 2));
    for &owner in owners {
        for tn in &tpl.nodes {
            materialized.insert(NodeId::expanded(owner, tn.local).unwrap(), tn.facets.clone());
        }
    }
    let mat_bytes = live_bytes() - b0;
    drop(materialized);

    // 2. Build PatchStore (700 views, 10% = 70 instances have a bezel override patch).
    let b1 = live_bytes();
    let mut store = PatchStore::default();
    store.templates.insert((meter_def, "free".into()), tpl);
    for (i, &owner) in owners.iter().enumerate() {
        store.views.insert(owner, ViewSpec { owner, top_root: comp, path: vec![owner], def: meter_def, base_notation: "free".into(), entity: None, enclosing_use: None });
        if i % 10 == 0 {
            let pid = i as u32 + 1;
            store.apply(&SpikeOp::AddPatch(Patch {
                id: pid,
                layer: 1,
                rank: pid,
                suffix: vec![owner],
                entries: BTreeMap::from([((bezel_local, Some(lp("fill.color"))), PatchEntry::Set(Slot::Const(col(0.0, 0.5, 1.0))))]),
            }));
        }
    }
    let virt_store_bytes = live_bytes() - b1;

    // 3. Build Candidate A (Sparse `Resolved` store).
    let b2 = live_bytes();
    let mut sparse = SparseResolvedViews::default();
    sparse.reconcile(&store, None);
    let sparse_bytes = live_bytes() - b2;
    let virt = FullyVirtualViews;

    let sample_unpatched = NodeId::expanded(owners[1], tick_locals[0]).unwrap();
    let sample_rewritten = NodeId::expanded(owners[1], needle_local).unwrap();
    let sample_patched = NodeId::expanded(owners[0], bezel_local).unwrap();

    // Verify exact parity before timing.
    assert_eq!(sparse.facet(sample_unpatched, FacetKind::Fill), virt.facet(&store, sample_unpatched, FacetKind::Fill).as_deref());
    assert_eq!(sparse.facet(sample_rewritten, FacetKind::Transform2d), virt.facet(&store, sample_rewritten, FacetKind::Transform2d).as_deref());
    assert_eq!(sparse.facet(sample_patched, FacetKind::Fill), virt.facet(&store, sample_patched, FacetKind::Fill).as_deref());

    // 4. Measure read allocations & latency (10,000 reads per category).
    const READS: usize = 10_000;
    fn measure(mut f: impl FnMut()) -> (usize, f64) {
        reset_alloc_ops();
        let t = Instant::now();
        for _ in 0..READS {
            f();
        }
        let ns = t.elapsed().as_nanos() as f64 / READS as f64;
        (alloc_ops() / READS, ns)
    }

    let (sa_u, st_u) = measure(|| { std::hint::black_box(sparse.facet(sample_unpatched, FacetKind::Fill)); });
    let (sa_r, st_r) = measure(|| { std::hint::black_box(sparse.facet(sample_rewritten, FacetKind::Transform2d)); });
    let (sa_p, st_p) = measure(|| { std::hint::black_box(sparse.facet(sample_patched, FacetKind::Fill)); });

    let (va_u, vt_u) = measure(|| { std::hint::black_box(virt.facet(&store, sample_unpatched, FacetKind::Fill)); });
    let (va_r, vt_r) = measure(|| { std::hint::black_box(virt.facet(&store, sample_rewritten, FacetKind::Transform2d)); });
    let (va_p, vt_p) = measure(|| { std::hint::black_box(virt.facet(&store, sample_patched, FacetKind::Fill)); });
    let (vla_r, vlt_r) = measure(|| { std::hint::black_box(virt.leaf(&store, sample_rewritten, lp("transform2d.x"))); });

    // 5. Measure single patch edit (reconcile 1 owner) at 700 meters.
    const EDITS: usize = 500;
    let t_patch = Instant::now();
    for i in 0..EDITS {
        let color = if i % 2 == 0 { col(1.0, 0.0, 1.0) } else { col(0.0, 1.0, 0.7) };
        store.apply(&SpikeOp::SetPatchEntry {
            patch: 1,
            target: (bezel_local, Some(lp("fill.color"))),
            entry: Some(PatchEntry::Set(Slot::Const(color))),
        });
        std::hint::black_box(sparse.reconcile(&store, Some(&[owners[0]])));
    }
    let patch_edit_us = t_patch.elapsed().as_micros() as f64 / EDITS as f64;

    println!("\n=== EN-3 Views Spike (700 meters x 62 nodes = 43,400 virtual nodes, 10% patched) ===");
    println!("Memory:");
    println!("  Materialized baseline (Docs::copies equiv): {:.2} MB", mat_bytes as f64 / 1_048_576.0);
    println!("  Fully-virtual store (views + patches only): {:.2} MB", virt_store_bytes as f64 / 1_048_576.0);
    println!("  Sparse Resolved overlay (only {} / {} nodes): {:.2} MB ({:.1}% of materialized)",
        sparse.resolved.len(), INSTANCES * (TICKS + 2), sparse_bytes as f64 / 1_048_576.0,
        (sparse_bytes as f64 / mat_bytes.max(1) as f64) * 100.0);
    println!("Read cost (allocs/read | ns/read):");
    println!("  Sparse Resolved facet() — unpatched: {sa_u} allocs, {st_u:.1} ns | rewritten: {sa_r} allocs, {st_r:.1} ns | patched: {sa_p} allocs, {st_p:.1} ns");
    println!("  Fully-Virtual  facet() — unpatched: {va_u} allocs, {vt_u:.1} ns | rewritten: {va_r} allocs, {vt_r:.1} ns | patched: {va_p} allocs, {vt_p:.1} ns");
    println!("  Fully-Virtual  leaf()  — rewritten: {vla_r} allocs, {vlt_r:.1} ns");
    println!("Write cost (1 owner reconcile at 700 meters): {:.1} us/edit", patch_edit_us);

    // Core invariants: Sparse Resolved achieves 0 heap allocations on ALL reads and << 10% memory of materialized!
    assert_eq!(sa_u, 0, "sparse unpatched read must allocate 0");
    assert_eq!(sa_r, 0, "sparse rewritten read must allocate 0");
    assert_eq!(sa_p, 0, "sparse patched read must allocate 0");
    assert!(va_r > 0 && va_p > 0, "fully-virtual facet reads must allocate on rewritten/patched nodes");
    assert!(sparse.resolved.len() == INSTANCES + INSTANCES / 10, "only rewritten + patched nodes stored");
    assert!(sparse_bytes * 5 < mat_bytes, "sparse resolved must be <20% of materialized copies memory");
}
