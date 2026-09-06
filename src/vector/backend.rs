//! Backend selection and the contract every 2D vector backend must satisfy.
//!
//! There is deliberately no `trait VectorBackend` here. Backends differ in
//! *where in Bevy they live*, not just in what they do: the classic path writes
//! components in the main world, while the sparse-strips path owns render-world
//! resources. A single object-safe trait would force both into a shape that
//! fits neither.
//!
//! The seam is the data, not a trait. A backend is a Bevy plugin that consumes
//! [`DisplayList`](super::DisplayList) components on entities carrying
//! [`VectorLayer`]. The app installs exactly one backend, so an inactive
//! renderer pays no initialization cost.
//!
//! (The `VectorRasterizer` trait sketched in the vision document is a different
//! thing: an interface for rasterizing a *bounded* workload into a *target*. That
//! becomes meaningful once there is a tile cache to rasterize into. Introducing
//! it now would be a trait with one implementation and no callers.)

use bevy::prelude::*;

/// Which renderer realizes a [`DisplayList`](super::DisplayList).
///
/// `ClassicVello` exists only under the `classic` feature: it is not merely
/// unused without it, the dependency is absent entirely.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum BackendKind {
    /// `vello` classic, via `bevy_vello`. GPU compute flattening; WebGPU only.
    #[cfg(feature = "classic")]
    ClassicVello,
    /// `vello_hybrid` sparse strips. CPU strip generation, no compute shaders.
    Hybrid,
}

/// The coordinate space a layer's commands are authored in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayerSpace {
    /// Window pixels, top-left origin, y-down. What the overlays and panels use.
    Screen,
    /// World units, y-up, positioned by the entity's `Transform`.
    World,
}

/// Marks an entity as a 2D vector layer and fixes its place in painter order.
///
/// `order` is explicit because the previous arrangement left every screen-space
/// scene at `z = 0`, where relative draw order was undefined and depended on
/// entity iteration. Two layers must not share an order.
#[derive(Component, Clone, Copy, Debug)]
#[require(Transform)]
pub struct VectorLayer {
    pub space: LayerSpace,
    pub order: i32,
}

impl VectorLayer {
    pub fn screen(order: i32) -> Self {
        Self {
            space: LayerSpace::Screen,
            order,
        }
    }

    pub fn world(order: i32) -> Self {
        Self {
            space: LayerSpace::World,
            order,
        }
    }
}

/// Painter order for the layers this app draws.
///
/// Kept in one place so the stacking is reviewable, rather than being implied by
/// spawn order across five modules.
pub mod order {
    pub const UI_PANELS: i32 = 0;
    pub const TIMELINE_BACKGROUND: i32 = 10;
    pub const TIMELINE_GRID: i32 = 11;
    pub const TIMELINE_PLAYHEAD: i32 = 12;
    pub const WORLD_DEMO: i32 = 20;
    pub const OVERLAY_ANIMATED: i32 = 30;
    pub const OVERLAY_BEZIER: i32 = 31;
    pub const MINI_SQUARES: i32 = 40;
    pub const DRAGGABLE: i32 = 50;
    pub const SELECTION_MARQUEE: i32 = 60;
    /// Alpha stress fixture (`?bevy=alpha`), drawn above everything else.
    pub const ALPHA_STRESS_STATIC: i32 = 70;
    pub const ALPHA_STRESS_ANIMATED: i32 = 71;
}
