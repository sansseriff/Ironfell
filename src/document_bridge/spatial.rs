//! The document's spatial projection, prepared from the store's own
//! revisions, transforms and bounds (vision 10 §3.1): nothing here reads a
//! Bevy transform, and a moved group costs one entry, not one per
//! descendant (`iron_spatial::Scene`). Drawing and picking still take their
//! placements from Bevy globals until phase 2; the store's drift check
//! (`check_world_affine`) keeps the two equal meanwhile.

use super::DocumentStore;
use bevy::prelude::*;
use iron_spatial::Scene;

#[derive(Resource)]
pub struct SpatialScene {
    /// Off: hosts walk the whole tree instead (`__iron.spatial(false)`), a
    /// reference for the regression checks. Bounds are maintained either way.
    pub enabled: bool,
    scene: Scene,
}

impl Default for SpatialScene {
    fn default() -> Self {
        Self { enabled: true, scene: Scene::default() }
    }
}

impl std::ops::Deref for SpatialScene {
    type Target = Scene;
    fn deref(&self) -> &Scene {
        &self.scene
    }
}

pub fn prepare_spatial(mut store: ResMut<DocumentStore>, mut scene: ResMut<SpatialScene>) {
    let timing = crate::perf::span(crate::perf::Phase::Bounds);
    // Change detection by hand: a frame that moves nothing must not mark
    // the scene changed, or every view would rebuild its display list.
    let inner = scene.bypass_change_detection();
    let before = inner.scene.generation;
    inner.scene.update(&mut store.bypass_change_detection().0);
    if inner.scene.generation == before {
        timing.discard();
        return;
    }
    crate::perf::set_bounds_updates(inner.scene.updated);
    scene.set_changed();
}
