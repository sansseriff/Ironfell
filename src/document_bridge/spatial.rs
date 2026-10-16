//! A disposable spatial projection, prepared independently of any view drawing.
//! Bevy supplies resolved globals; this does not propagate a second hierarchy.

use super::{Bounds2d, Doc2d, DocumentChanges, DocumentStore, GeometryRevision, NodeMap, Provenance, Shape2d, nest};
use bevy::prelude::*;
use iron_document::{NodeId, composition};
use iron_spatial::{Bounds, BoundsIndex, Record};
use kurbo::{Point, Rect, Shape as _};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Resource)]
pub struct SpatialScene {
    pub enabled: bool,
    pub generation: u64,
    scopes: BTreeMap<Option<NodeId>, BoundsIndex>,
    owners: BTreeMap<NodeId, Option<NodeId>>,
    chains: BTreeMap<NodeId, Vec<NodeId>>,
    embeds: Vec<NodeId>,
    seen: Option<(u64, iron_document::Version)>,
    pub damage: Vec<Rect>,
}

impl Default for SpatialScene {
    fn default() -> Self {
        Self { enabled: true, generation: 0, scopes: BTreeMap::new(), owners: BTreeMap::new(), chains: BTreeMap::new(), embeds: Vec::new(), seen: None, damage: Vec::new() }
    }
}

impl SpatialScene {
    pub fn len(&self) -> usize { self.owners.len() }
    pub fn record(&self, id: NodeId) -> Option<&Record> {
        self.scopes.get(self.owners.get(&id)?)?.record(id)
    }
    /// Every embed in the document, as of the last structural change.
    pub fn embeds(&self) -> &[NodeId] { &self.embeds }
    pub fn chain(&self, id: NodeId) -> &[NodeId] { self.chains.get(&id).map(Vec::as_slice).unwrap_or(&[]) }
    /// Query a canonical composition once; callers transform into it for each
    /// embed occurrence and retain the occurrence chain in their results.
    pub fn candidates(&self, roots: &[NodeId], doc: &iron_document::Document, point: Option<Point>, rect: Rect) -> Vec<NodeId> {
        let root_set: BTreeSet<_> = roots.iter().copied().collect();
        let scopes: BTreeSet<_> = roots.iter().map(|id| composition::composition_of(doc, *id)).collect();
        let multiple = scopes.len() > 1;
        let mut out = Vec::new();
        for scope in scopes {
            if let Some(index) = self.scopes.get(&scope) {
                let ids = match point { Some(p) => index.at_point(p), None => index.in_rect(rect) };
                out.extend(ids.into_iter().filter(|id| self.chain(*id).first().is_some_and(|root| root_set.contains(root))));
            }
        }
        if multiple { out.sort_by_cached_key(|id| self.record(*id).map_or(0, |r| r.order)); }
        out
    }
    fn upsert(&mut self, id: NodeId, scope: Option<NodeId>, record: Record, doc: &iron_document::Document) {
        if let Some(old) = self.owners.insert(id, scope) && old != scope {
            if let Some(index) = self.scopes.get_mut(&old) { index.remove(id); }
        }
        self.chains.entry(id).or_insert_with(|| nest::lineage(doc, id));
        self.scopes.entry(scope).or_default().upsert(id, record);
    }
}

pub fn prepare_spatial(
    store: Res<DocumentStore>,
    revision: Res<GeometryRevision>,
    changes: Res<DocumentChanges>,
    map: Res<NodeMap>,
    nodes: Query<(&Provenance, Ref<Doc2d>, Ref<GlobalTransform>)>,
    shapes: Query<(&'static Doc2d, &'static GlobalTransform)>,
    globals: Query<&'static GlobalTransform>,
    mut bounds: ResMut<Bounds2d>,
    mut scene: ResMut<SpatialScene>,
) {
    let key = (revision.0, store.0.document().version());
    if scene.seen == Some(key) { return; }
    let _timing = crate::perf::span(crate::perf::Phase::Bounds);
    let doc = store.0.document();
    let full = scene.seen.is_none() || changes.structure;
    let scene = &mut *scene;
    scene.seen = Some(key);
    scene.generation = scene.generation.wrapping_add(1);
    scene.damage.clear();
    for index in scene.scopes.values_mut() { index.begin_changes(); }
    if full {
        scene.damage.extend(scene.scopes.values().flat_map(|s| s.records().map(|(_,r)| r.bounds.painted)));
        scene.scopes.clear(); scene.owners.clear(); scene.chains.clear(); bounds.0.clear();
        scene.embeds = doc.live_nodes().filter(|n| n.ty.name() == "embed").map(|n| n.id).collect();
    }
    let mut updated = 0;
    for (prov, shape, global) in &nodes {
        if !full && !shape.is_changed() && !global.is_changed() { continue; }
        let local = nest::shape_path(&shape.shape).bounding_box();
        let paint_pad = nest::paint_padding(&shape);
        let hit_pad = if matches!(shape.shape, Shape2d::Path { .. }) { 5.0 } else { 0.0 };
        let b = Bounds::transformed(local, nest::affine_of(&global), paint_pad, hit_pad);
        let scope = composition::composition_of(doc, prov.0);
        scene.upsert(prov.0, scope, Record { bounds: b, order: map.paint_index(prov.0).unwrap_or(0) }, doc);
        bounds.0.insert(prov.0, b.painted);
        updated += 1;
    }
    // Embeds are small in number and their default regions depend on a source
    // composition. Refresh them after leaf globals, including hidden sources.
    for id in scene.embeds.clone() {
        let Some(entity) = map.entity(id) else { continue };
        let Ok(global) = globals.get(entity) else { continue };
        let local = if let Some(e) = nest::embed_2d(doc, &map, &globals, &shapes, id) {
            e.region
        } else if let Some((_,_,w,h)) = nest::embed_3d(&store.0, id) {
            Rect::new(0.0,0.0,w,h)
        } else { continue };
        let b = Bounds::transformed(local, nest::affine_of(global), 2.0, 0.0);
        let record = Record { bounds: b, order: map.paint_index(id).unwrap_or(0) };
        if full || scene.record(id) != Some(&record) {
            scene.upsert(id, composition::composition_of(doc,id), record, doc);
            bounds.0.insert(id,b.painted);
            updated += 1;
        }
    }
    for index in scene.scopes.values_mut() {
        index.commit();
        scene.damage.extend_from_slice(index.damage());
    }
    crate::perf::set_bounds_updates(updated);
}
