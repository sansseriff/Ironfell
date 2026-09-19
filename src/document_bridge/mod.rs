//! The bridge between the authored store and Bevy.
//!
//! ```text
//!   PendingTransactions ──sync_document──▶ Store ──reconcile──▶ entities
//!        ▲                                                        │
//!        │  one Set transaction on release                        │ optimistic
//!        └────────────── drag_commit ◀── drag_apply ◀─────────────┘ Transform
//! ```
//!
//! Three rules, from `plans/document-spine.md` §10:
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
mod materialize;
mod render;

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use iron_document::{Document, NodeId, Op, Store, TransactionInput, TypeId};
use std::collections::BTreeSet;

pub use materialize::{Doc2d, Doc3d, MeshAssets, Shape2d};
pub use render::DocumentLayer;

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

/// Set when the materialised scene changed shape or values this frame, so
/// the 2D layer rebuilds once rather than every frame.
#[derive(Resource, Default)]
pub struct DocumentDirty(pub bool);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryOp {
    Undo,
    Redo,
}

/// Undo and redo requests, queued by the shell through the FFI and applied
/// by the document sync after that frame's transactions. Undo is itself a
/// transaction on the same log (plans/document-spine.md §6.4), so the
/// reconciler treats it like any other.
#[derive(Resource, Default)]
pub struct HistoryRequests(pub Vec<HistoryOp>);

pub struct DocumentPlugin;

impl Plugin for DocumentPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(DocumentStore(Store::new()))
            .init_resource::<PendingTransactions>()
            .init_resource::<NodeMap>()
            .init_resource::<DocumentDirty>()
            .init_resource::<HistoryRequests>()
            .init_resource::<PendingLoad>()
            .init_resource::<GestureRequests>()
            .add_systems(Startup, (materialize::setup_mesh_assets, render::setup_document_layer, demo::queue_demo_scene))
            // Before input collection, so a transaction queued last frame is
            // visible to this frame's picking.
            .add_systems(PreUpdate, sync_document)
            .add_systems(
                PostUpdate,
                render::render_document_layer.after(TransformSystems::Propagate),
            );
    }
}

/// Apply every pending transaction and history request, then reconcile the
/// nodes they touched.
fn sync_document(
    mut store: ResMut<DocumentStore>,
    mut pending: ResMut<PendingTransactions>,
    mut history: ResMut<HistoryRequests>,
    mut load: ResMut<PendingLoad>,
    mut gestures: ResMut<GestureRequests>,
    mut map: ResMut<NodeMap>,
    mut dirty: ResMut<DocumentDirty>,
    mut commands: Commands,
    assets: Option<Res<MeshAssets>>,
    time: Res<Time>,
) {
    if pending.0.is_empty()
        && history.0.is_empty()
        && load.0.is_none()
        && gestures.0.is_empty()
        && !store.0.has_changes()
    {
        return;
    }
    let now = time.elapsed().as_millis() as u64;
    let mut affected: BTreeSet<NodeId> = BTreeSet::new();
    let version_before = store.0.document().version();
    if let Some(doc) = load.0.take() {
        // Everything materialised belongs to the old document. Tear it all
        // down rather than diffing: ids may coincide with different types.
        for e in map.drain() {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.try_despawn();
            }
        }
        store.0 = Store::with_document(doc);
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
        match store.0.apply(input) {
            Ok(_) => note_applied(&store.0, &mut affected),
            Err(errors) => {
                for e in errors {
                    warn!("transaction {label:?} rejected: {e}");
                }
            }
        }
    }
    for g in gestures.0.drain(..) {
        match g {
            GestureRequest::Commit(label) => match store.0.commit_gesture(label, iron_document::Actor::Human, now) {
                None => {}
                Some(Ok(_)) => note_applied(&store.0, &mut affected),
                Some(Err(errors)) => {
                    for e in errors {
                        warn!("gesture rejected: {e}");
                    }
                }
            },
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
            Some(Ok(_)) => note_applied(&store.0, &mut affected),
            Some(Err(errors)) => {
                for e in errors {
                    warn!("{op:?} rejected: {e}");
                }
            }
        }
    }
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
    let Some(assets) = assets else {
        warn!("mesh assets missing; cannot reconcile");
        return;
    };
    materialize::reconcile(&store.0, &affected, &mut map, &mut commands, &assets);
    map.paint_index = paint_order(&store.0);
    dirty.0 = true;
}

/// Record the nodes the most recently applied transaction touched. Ops name
/// what was asked for; the inverse names everything that actually changed (a
/// subtree delete lists each node).
fn note_applied(store: &Store, affected: &mut BTreeSet<NodeId>) {
    let tx = store.history().log().last().expect("just applied");
    info!("applied {:?} ({} ops) -> v{}", tx.label, tx.ops.len(), tx.version.0);
    for op in tx.ops.iter().chain(tx.inverse.iter()) {
        collect_ids(op, affected);
    }
}

fn collect_ids(op: &Op, out: &mut BTreeSet<NodeId>) {
    match op {
        Op::Create { id, parent, .. } | Op::Reparent { id, parent, .. } => {
            out.insert(*id);
            if let Some(p) = parent {
                out.insert(*p);
            }
        }
        Op::Delete { id }
        | Op::Reorder { id, .. }
        | Op::Set { id, .. }
        | Op::AddComp { id, .. }
        | Op::RemoveComp { id, .. } => {
            out.insert(*id);
        }
        Op::Link { .. } | Op::Unlink { .. } => {}
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
