//! The bridge between the authored store and Bevy.
//!
//! ```text
//!   PendingTransactions ──sync_document──▶ Store ──reconcile──▶ entities
//!        ▲                                                        │
//!        │  one Set transaction on release                        │ optimistic
//!        └────────────── drag_commit ◀── drag_apply ◀─────────────┘ Transform
//! ```
//!
//! Three rules, from `plans/specs/document-spine.md` §10:
//!
//! - The store is authority. Bevy holds a projection of it: every materialised
//!   entity carries [`Provenance`] naming its node, and nothing else about the
//!   entity is durable.
//! - Gestures are intents. A drag moves the entity's `Transform` each frame for
//!   latency and emits one labelled transaction on release. The reconciler
//!   then rewrites the entity from the store, which is the same value.
//! - The reconciler consumes transactions, not diffs. The ids an applied
//!   transaction touched are read off its ops and inverse; only those nodes
//!   are re-examined.

mod demo;
mod highlight;
mod materialize;
pub mod nest;
mod panel_views;
pub mod planes;
mod render;
mod spatial;
pub mod stress;

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use iron_document::{Document, NodeId, Op, Store, TransactionInput, TypeId};
use std::collections::BTreeSet;

pub use highlight::{Highlights, update_highlights};
pub use nest::{EmbedImage, EmbedImages, EmbedView, EmbedViews};
pub use materialize::{Doc2d, Doc3d, MeshAssets, Shape2d, path_outline};
pub use panel_views::{CompositionLayers, FocusRequests, InComposition, ViewCameras, PanelView, PanelViews};
pub use render::{DocumentPanelLayer, render_document_layer};
pub use spatial::SpatialScene;

/// The authored store, as a resource. Reads go through `.0.document()`;
/// writes go through [`PendingTransactions`], never through here.
#[derive(Resource)]
pub struct DocumentStore(pub Store);

/// Transactions queued for the next sync. Anything that wants to change
/// authored state pushes here: gestures, keyboard commands, the FFI, the
/// model surface.
#[derive(Resource, Default)]
pub struct PendingTransactions(pub Vec<TransactionInput>);

impl PendingTransactions {
    pub fn push(&mut self, label: impl Into<String>, actor: iron_document::Actor, ops: Vec<Op>) {
        self.0.push(TransactionInput {
            label: label.into(),
            actor,
            ts: 0, // stamped from the clock at sync
            idempotency_key: None,
            ops,
            exact: false,
        });
    }
}

/// Names the document node an entity materialises.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Provenance(pub NodeId);

/// `NodeId ↔ Entity`, plus each materialised node's painter index (its
/// position in the document's preorder), which picking uses as depth.
#[derive(Resource, Default)]
pub struct NodeMap {
    to_entity: HashMap<NodeId, Entity>,
    to_node: HashMap<Entity, NodeId>,
    paint_index: HashMap<NodeId, u32>,
}

impl NodeMap {
    pub fn entity(&self, id: NodeId) -> Option<Entity> {
        self.to_entity.get(&id).copied()
    }

    pub fn node(&self, e: Entity) -> Option<NodeId> {
        self.to_node.get(&e).copied()
    }

    pub fn paint_index(&self, id: NodeId) -> Option<u32> {
        self.paint_index.get(&id).copied()
    }

    fn insert(&mut self, id: NodeId, e: Entity) {
        self.to_entity.insert(id, e);
        self.to_node.insert(e, id);
    }

    fn remove(&mut self, id: NodeId) -> Option<Entity> {
        let e = self.to_entity.remove(&id)?;
        self.to_node.remove(&e);
        self.paint_index.remove(&id);
        Some(e)
    }

    fn drain(&mut self) -> Vec<Entity> {
        self.to_node.clear();
        self.paint_index.clear();
        self.to_entity.drain().map(|(_, e)| e).collect()
    }
}

/// End of a value gesture (a slider scrub): turn the store's previews into
/// one transaction, or drop them.
#[derive(Debug, Clone, PartialEq)]
pub enum GestureRequest {
    Commit(String),
    Cancel,
}

#[derive(Resource, Default)]
pub struct GestureRequests(pub Vec<GestureRequest>);

/// A document to replace the current one, already parsed and validated by
/// the FFI. Applied by the sync before that frame's transactions; history is
/// discarded with the old document.
#[derive(Resource, Default)]
pub struct PendingLoad(pub Option<Document>);

/// What the reconciler rewrote since the 2D layer last drew: the nodes, and
/// whether the tree's shape (hence painter order) changed. Both reconcile
/// passes add to it; rendering takes it, decides what to redraw and reports
/// what it did. Roadmap stage H turns this into culling and damage.
#[derive(Resource, Default, Debug)]
pub struct DocumentChanges {
    pub nodes: BTreeSet<NodeId>,
    pub structure: bool,
}

impl DocumentChanges {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && !self.structure
    }

    pub fn take(&mut self) -> DocumentChanges {
        std::mem::take(self)
    }
}

/// Painted bounds in each node's canonical composition space, kept current
/// by the spatial preparation pass even when no view draws the node.
#[derive(Resource, Default)]
pub struct Bounds2d(pub HashMap<NodeId, kurbo::Rect>);

/// Monotonic projection generation, including resolved previews and document
/// replacement. Unlike DocumentChanges this is never drained by rendering.
/// Consumers can retain derived geometry across unchanged frames.
#[derive(Resource, Default)]
pub struct GeometryRevision(pub u64);

/// The projection generation whose Bevy globals have finished propagation.
/// PreUpdate picking must refresh again after a newly reconciled pose settles.
#[derive(Resource, Default)]
pub struct PropagatedGeometryRevision(pub u64);

fn stamp_propagated_geometry(revision: Res<GeometryRevision>, mut propagated: ResMut<PropagatedGeometryRevision>) {
    if propagated.0 != revision.0 { propagated.0 = revision.0; }
}

/// The last version the reconciler has caught up to. Anything the log holds
/// past it, however it was applied (queued transaction, undo, a synchronous
/// FFI apply from the model surface), is reconciled on the next sync.
#[derive(Resource, Default)]
pub struct SyncedVersion(pub iron_document::Version);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryOp {
    Undo,
    Redo,
}

/// Undo and redo requests, queued by the shell through the FFI and applied
/// by the document sync after that frame's transactions. Undo is itself a
/// transaction on the same log (plans/specs/document-spine.md §6.4), so the
/// reconciler treats it like any other.
#[derive(Resource, Default)]
pub struct HistoryRequests(pub Vec<HistoryOp>);

pub struct DocumentPlugin;

impl Plugin for DocumentPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(DocumentStore(Store::new()))
            .init_resource::<PendingTransactions>()
            .init_resource::<NodeMap>()
            .init_resource::<DocumentChanges>()
            .init_resource::<Bounds2d>()
            .init_resource::<GeometryRevision>()
            .init_resource::<PropagatedGeometryRevision>()
            .init_resource::<SpatialScene>()
            .init_resource::<HistoryRequests>()
            .init_resource::<PendingLoad>()
            .init_resource::<GestureRequests>()
            .init_resource::<SyncedVersion>()
            .init_resource::<PanelViews>()
            .init_resource::<Highlights>()
            .init_resource::<ViewCameras>()
            .init_resource::<EmbedViews>()
            .init_resource::<EmbedImages>()
            .init_resource::<FocusRequests>()
            .init_resource::<CompositionLayers>()
            .add_plugins(planes::PlanePlugin)
            .add_systems(Startup, (materialize::setup_mesh_assets, demo::queue_demo_scene))
            // Before input collection, so a transaction queued last frame is
            // visible to this frame's picking.
            .add_systems(PreUpdate, (sync_document, panel_views::resolve_panel_views).chain())
            .add_systems(
                PostUpdate,
                (
                    render::sync_document_views.before(render::render_document_layer),
                    // Planes: after transforms (they project their corners)
                    // and before the 2D layer draws their views.
                    planes::sync_planes.after(TransformSystems::Propagate).before(render::render_document_layer),
                    spatial::prepare_spatial.after(TransformSystems::Propagate).before(render::render_document_layer).before(crate::bevy_app::select::draw_select_overlay),
                    render::render_document_layer.after(TransformSystems::Propagate),
                    stamp_propagated_geometry.after(TransformSystems::Propagate),
                    // The store's world transforms must agree with Bevy's
                    // (constraint-system.md §4.3); checked whenever one moves.
                    check_world_affine.after(TransformSystems::Propagate),
                    // Where each 3D embed sits on screen this frame; its
                    // camera follows in the same frame (scene3d).
                    nest::collect_embed_views.after(TransformSystems::Propagate),
                ),
            );
    }
}

/// Compare `Store::world_affine` with Bevy's `GlobalTransform` for every 2D
/// entity whose transform changed this frame. The native test in `geom.rs`
/// pins the arithmetic; this catches drift in what the bridge feeds it
/// (parent links, missing components, unit conversions). Tolerance is f32
/// precision relative to the largest coefficient.
fn check_world_affine(
    store: Res<DocumentStore>,
    moved: Query<(&Provenance, &GlobalTransform), (With<Doc2d>, Changed<GlobalTransform>)>,
) {
    for (prov, global) in &moved {
        let bevy = iron_document::Affine2(render::affine_of(global).as_coeffs());
        let ours = store.0.world_affine(prov.0);
        let scale = ours.0.iter().fold(1.0f64, |m, v| m.max(v.abs()));
        let diff = ours.max_diff(&bevy);
        if diff > 1e-4 * scale {
            warn!("{}: store world affine {:?} differs from Bevy {:?} by {diff}", prov.0, ours.0, bevy.0);
        }
    }
}

/// Apply every pending transaction and history request, then reconcile the
/// nodes they touched.
pub(crate) fn sync_document(
    mut store: ResMut<DocumentStore>,
    mut pending: ResMut<PendingTransactions>,
    mut history: ResMut<HistoryRequests>,
    mut load: ResMut<PendingLoad>,
    mut gestures: ResMut<GestureRequests>,
    mut synced: ResMut<SyncedVersion>,
    mut map: ResMut<NodeMap>,
    mut changes: ResMut<DocumentChanges>,
    mut geometry_revision: ResMut<GeometryRevision>,
    mut layers: ResMut<CompositionLayers>,
    mut commands: Commands,
    assets: Option<Res<MeshAssets>>,
    time: Res<Time>,
) {
    if pending.0.is_empty()
        && history.0.is_empty()
        && load.0.is_none()
        && gestures.0.is_empty()
        && !store.0.has_changes()
        && store.0.document().version() == synced.0
    {
        return;
    }
    let _timing = crate::perf::span(crate::perf::Phase::Sync);
    let now = time.elapsed().as_millis() as u64;
    let mut affected: BTreeSet<NodeId> = BTreeSet::new();
    let mut structure = false;
    let version_before = synced.0;
    if let Some(doc) = load.0.take() {
        geometry_revision.0 = geometry_revision.0.wrapping_add(1);
        structure = true;
        // Everything materialised belongs to the old document. Tear it all
        // down rather than diffing: ids may coincide with different types.
        for e in map.drain() {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.try_despawn();
            }
        }
        store.0 = Store::with_document(doc);
        synced.0 = store.0.document().version();
        affected.extend(store.0.document().live_nodes().map(|n| n.id));
        info!(
            "loaded document v{} ({} live nodes)",
            store.0.document().version().0,
            affected.len()
        );
    }
    for mut input in pending.0.drain(..) {
        if input.ts == 0 {
            input.ts = now;
        }
        let label = input.label.clone();
        if let Err(errors) = store.0.apply(input) {
            for e in errors {
                warn!("transaction {label:?} rejected: {e}");
            }
        }
    }
    for g in gestures.0.drain(..) {
        match g {
            GestureRequest::Commit(label) => {
                if let Some(Err(errors)) = store.0.commit_gesture(label, iron_document::Actor::Human, now) {
                    for e in errors {
                        warn!("gesture rejected: {e}");
                    }
                }
            }
            GestureRequest::Cancel => store.0.cancel_gesture(),
        }
    }
    for op in history.0.drain(..) {
        let result = match op {
            HistoryOp::Undo => store.0.undo(now),
            HistoryOp::Redo => store.0.redo(now),
        };
        match result {
            None => info!("nothing to {op:?}"),
            Some(Ok(_)) => {}
            Some(Err(errors)) => {
                for e in errors {
                    warn!("{op:?} rejected: {e}");
                }
            }
        }
    }
    // Everything the log gained since the last sync, whoever applied it.
    for tx in store.0.history().log().filter(|tx| tx.version > synced.0) {
        info!("applied {:?} ({} ops, {:?}) -> v{}", tx.label, tx.ops.len(), tx.actor, tx.version.0);
        if let Some(s) = &tx.solver {
            // The cause line the model hears (CS §9.1): who drove what, which
            // clusters re-solved; inferred values are on demand, never pushed.
            let list = |ids: &[NodeId]| ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(" ");
            info!(
                "{:?} by {:?}: cluster {} re-solved; moved {} node(s): {}{}",
                tx.label,
                tx.actor,
                list(&s.clusters),
                s.moved.len(),
                list(&s.moved),
                if s.unmet.is_empty() {
                    String::new()
                } else {
                    format!("; unmet: {}", s.unmet.iter().map(|u| format!("{} wanted {} got {}", u.leaf, u.requested, u.solved)).collect::<Vec<_>>().join(", "))
                }
            );
        }
        for op in tx.ops.iter().chain(tx.derived_ops.iter()).chain(tx.inverse.iter()) {
            structure |= collect_ids(op, &mut affected);
        }
    }
    synced.0 = store.0.document().version();
    // Resolved values that moved: bindings re-evaluated, or a gesture
    // previewed. Their nodes are rewritten like any other change.
    for change in store.0.drain_changes() {
        affected.insert(change.leaf.node);
    }
    for (leaf, err) in &store.0.reactive().errors {
        warn!("binding {leaf}: {err}");
    }
    let version = store.0.document().version();
    if version != version_before {
        crate::web_ffi::send_document_changed_from_worker(version.0 as u32);
    }
    if affected.is_empty() {
        return;
    }
    geometry_revision.0 = geometry_revision.0.wrapping_add(1);
    let Some(assets) = assets else {
        warn!("mesh assets missing; cannot reconcile");
        return;
    };
    materialize::reconcile(&store.0, &affected, &mut map, &mut layers, &mut commands, &assets);
    if structure {
        map.paint_index = paint_order(&store.0);
    }
    changes.nodes.extend(affected);
    changes.structure |= structure;
}

/// The in-frame half of a gesture (constraint-system.md §8.2). `sync_document`
/// runs in `PreUpdate`, but drags preview in `PostUpdate`; without this pass
/// every previewed value would reach the screen a frame behind the pointer.
/// It drains the resolved changes the previews produced (the dragged leaves
/// and every binding that read them) and rewrites those entities, scheduled
/// before transform propagation so rendering sees them this frame. Values
/// only: a preview never changes the tree, so the paint order stands.
pub(crate) fn reconcile_gesture(
    mut store: ResMut<DocumentStore>,
    mut map: ResMut<NodeMap>,
    mut changes: ResMut<DocumentChanges>,
    mut geometry_revision: ResMut<GeometryRevision>,
    mut layers: ResMut<CompositionLayers>,
    mut commands: Commands,
    assets: Option<Res<MeshAssets>>,
) {
    if !store.0.has_changes() {
        return;
    }
    let _timing = crate::perf::span(crate::perf::Phase::GestureReconcile);
    let affected: BTreeSet<NodeId> = store.0.drain_changes().into_iter().map(|c| c.leaf.node).collect();
    geometry_revision.0 = geometry_revision.0.wrapping_add(1);
    let Some(assets) = assets else { return };
    materialize::reconcile(&store.0, &affected, &mut map, &mut layers, &mut commands, &assets);
    changes.nodes.extend(affected);
}

/// The nodes an op touches. Returns whether it changed the tree's shape
/// (existence, parent, or sibling order), which is what invalidates the
/// painter order.
fn collect_ids(op: &Op, out: &mut BTreeSet<NodeId>) -> bool {
    match op {
        Op::Create { id, parent, .. } | Op::Reparent { id, parent, .. } => {
            out.insert(*id);
            if let Some(p) = parent {
                out.insert(*p);
            }
            true
        }
        Op::Delete { id } | Op::Reorder { id, .. } => {
            out.insert(*id);
            true
        }
        Op::Set { id, .. } | Op::AddComp { id, .. } | Op::RemoveComp { id, .. } => {
            out.insert(*id);
            false
        }
        // A `figures` link changes what a figure highlights with, so both
        // ends redraw. An unlink's inverse is the link, which names them.
        Op::Link { rel } => {
            out.insert(rel.from);
            out.insert(rel.to);
            false
        }
        Op::Unlink { .. } => false,
    }
}

/// Preorder index of every live node: painter order for 2D content.
fn paint_order(store: &Store) -> HashMap<NodeId, u32> {
    let doc = store.document();
    let mut out = HashMap::new();
    let mut stack: Vec<NodeId> = doc.roots().iter().rev().copied().collect();
    let mut i = 0u32;
    while let Some(id) = stack.pop() {
        out.insert(id, i);
        i += 1;
        stack.extend(doc.children(id).iter().rev().copied());
    }
    out
}

/// The node type of a materialised entity, for labels such as "move rect".
pub fn type_of(store: &Store, map: &NodeMap, e: Entity) -> Option<TypeId> {
    let id = map.node(e)?;
    store.document().live(id).map(|n| n.ty)
}
