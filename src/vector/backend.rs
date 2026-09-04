//! Backend selection and the contract every 2D vector backend must satisfy.
//!
//! There is deliberately no `trait VectorBackend` here. Backends differ in
//! *where in Bevy they live*, not just in what they do: the classic path writes
//! components in the main world, while a sparse-strips path will own render-world
//! resources and a render-graph node. A single object-safe trait would force both
//! into a shape that fits neither.
//!
//! The seam is the data, not a trait. A backend is a Bevy plugin that consumes
//! [`DisplayList`](super::DisplayList) components on entities carrying
//! [`VectorLayer`], and its systems are gated on [`backend_active`] so backends
//! can be switched at runtime — or run at the same time, for a side-by-side
//! comparison on identical input.
//!
//! (The `VectorRasterizer` trait sketched in the vision document is a different
//! thing: an interface for rasterizing a *bounded* workload into a *target*. That
//! becomes meaningful once there is a tile cache to rasterize into. Introducing
//! it now would be a trait with one implementation and no callers.)

use bevy::prelude::*;

/// Which renderer realizes a [`DisplayList`](super::DisplayList).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum BackendKind {
    /// `vello` classic, via `bevy_vello`. GPU compute flattening.
    ClassicVello,
    /// `vello_hybrid` sparse strips. CPU strip generation, no compute shaders.
    ///
    /// Not implemented yet; selecting it renders nothing rather than falling
    /// back silently, so a missing backend is visible rather than mysterious.
    #[allow(
        dead_code,
        reason = "the next backend's slot; removing it would hide the plan"
    )]
    Hybrid,
}

/// Which backends are live this frame.
///
/// `compare` exists for A/B work: with two backends active, the same display
/// lists are realized twice, which is the only way to compare them on genuinely
/// identical input rather than on two builds that have drifted.
#[derive(Resource, Clone, Debug)]
pub struct ActiveBackends {
    pub primary: BackendKind,
    pub compare: Option<BackendKind>,
}

impl Default for ActiveBackends {
    fn default() -> Self {
        Self {
            primary: BackendKind::ClassicVello,
            compare: None,
        }
    }
}

impl ActiveBackends {
    pub fn is_active(&self, kind: BackendKind) -> bool {
        self.primary == kind || self.compare == Some(kind)
    }
}

/// Run condition: is `kind` realizing display lists this frame?
pub fn backend_active(kind: BackendKind) -> impl Fn(Res<'_, ActiveBackends>) -> bool + Clone {
    move |backends: Res<'_, ActiveBackends>| backends.is_active(kind)
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
}
