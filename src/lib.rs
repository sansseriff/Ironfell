// Exactly one graphics backend, enforced at compile time.
//
// Neither mistake is visible at runtime until the app fails to start, and the
// "both" case is the dangerous one: bevy's WebGL2 support is gated on
// `all(feature = "webgl", ..., not(feature = "webgpu"))`, so a build with both
// features compiles the WebGL2 paths out, asks for `Backends::BROWSER_WEBGPU`,
// and dies looking for an adapter. Cargo features unify across the dependency
// graph, so anything that pulls `bevy/webgpu` transitively could do this to the
// WebGL2 build without touching this crate.
#[cfg(all(feature = "webgpu", feature = "webgl2"))]
compile_error!(
    "features `webgpu` and `webgl2` are mutually exclusive: bevy compiles its \
     WebGL2 paths out when `webgpu` is present, producing a build that cannot \
     acquire an adapter. Build each target separately."
);

// Classic Vello flattens paths in compute shaders. WebGL2 has none, so this is a
// build that could never run rather than one that merely runs slowly.
#[cfg(all(feature = "classic", feature = "webgl2"))]
compile_error!(
    "feature `classic` requires `webgpu`: vello classic flattens paths in \
     compute shaders, which WebGL2 does not provide."
);

#[cfg(not(any(feature = "webgpu", feature = "webgl2")))]
compile_error!(
    "no graphics backend selected: build with `--features webgpu` or \
     `--features webgl2` (see [features] in Cargo.toml)."
);

use bevy::{
    ecs::system::SystemState, prelude::*,
    window::WindowCloseRequested,
};
use std::ops::{Deref, DerefMut};

// original web ffi module
mod web_ffi;
pub use web_ffi::*;

mod canvas_view;

pub mod panels;

// ray_pick legacy module removed (superseded by new picking systems)

pub mod bevy_app; // expose init_app and related types
pub use bevy_app::*; // re-export init_app symbols

mod fps_overlay;

mod tracking_circle;

mod asset_reader; // kept private


// mod asset_loader;

// The 2D vector rendering seam: display lists in, backend-rendered pixels out.
mod vector;

// The authored store's runtime projection: reconciler, provenance, writeback.
pub mod document_bridge;

mod camera_controller;

// Per-phase CPU timings for the render benchmark (`__iron.timings`).
pub mod perf;

pub struct WorkerApp {
    pub app: App,
    /// 手动包装事件需要
    pub window: Entity,
    pub scale_factor: f32,
}

impl Deref for WorkerApp {
    type Target = App;

    fn deref(&self) -> &Self::Target {
        &self.app
    }
}

impl DerefMut for WorkerApp {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.app
    }
}

impl WorkerApp {
    pub fn new(app: App, window: Entity) -> Self {
        Self {
            app,
            window,
            scale_factor: 1.0,
        }
    }

/// One frame. With render on demand (`RedrawNeeded`), the main world runs,
    /// decides at the end of its schedule whether anything the window shows
    /// changed, and only then are the render world's extraction and rendering
    /// run; otherwise the window keeps its last picture. The same steps as
    /// `App::update`, with the sub-apps made conditional.
    pub fn frame(&mut self) {
        let start = bevy::platform::time::Instant::now();
        if !self.app.world().contains_resource::<RedrawNeeded>() {
            self.app.update();
            crate::perf::end_frame(start);
            return;
        }
        let subs = self.app.sub_apps_mut();
        subs.main.run_default_schedule();
        let redraw = subs.main.world().get_resource::<RedrawNeeded>().is_none_or(|r| r.now);
        if redraw {
            for (_, sub) in subs.sub_apps.iter_mut() {
                sub.extract(subs.main.world_mut());
                sub.update();
            }
        }
        subs.main.world_mut().clear_trackers();
        crate::perf::end_frame(start);
    }

    pub fn to_physical_size(&self, x: f32, y: f32) -> Vec2 {
        Vec2::new(x * self.scale_factor, y * self.scale_factor)
    }
}

/// Render on demand (plans/specs/view-textures.md): whether this frame changed
/// anything the window shows. When it did not, the frame loop runs only the
/// main world and the window keeps its last picture, so an idle frame costs
/// no GPU work at any resolution. Only installed with view textures, where
/// every view keeps its own texture between frames.
#[derive(Resource, Debug)]
pub(crate) struct RedrawNeeded {
    pub now: bool,
    /// Frames still to render after the last change: per-view state settles
    /// over a frame or two, as for a woken 3D view.
    linger: u8,
}

impl Default for RedrawNeeded {
    fn default() -> Self {
        // Render the first frames unconditionally while everything appears.
        RedrawNeeded { now: true, linger: 30 }
    }
}

/// Decide, at the end of the main world's frame, whether to render it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn decide_redraw(
    mut redraw: ResMut<RedrawNeeded>,
    texture_cameras: Query<&Camera, With<crate::bevy_app::ScreenRectMarker>>,
    lists: Query<(), Or<(Changed<crate::vector::DisplayList>, Changed<crate::vector::VectorView>)>>,
    ui: Query<(), Or<(Changed<Text>, Changed<TextSpan>, Changed<Node>, Changed<Visibility>)>>,
    cameras: Query<(), Or<(Added<Camera>, Changed<Camera>)>>,
    mut removed_cameras: RemovedComponents<Camera>,
    mut removed_lists: RemovedComponents<crate::vector::DisplayList>,
    panels: Res<crate::panels::Panels>,
    windows: Query<(), Changed<Window>>,
    mut images: MessageReader<AssetEvent<Image>>,
    mut meshes: MessageReader<AssetEvent<Mesh>>,
    mut materials: MessageReader<AssetEvent<crate::vector::view_composite::ViewCompositeMaterial>>,
) {
    let changed = texture_cameras.iter().any(|c| c.is_active)
        || !lists.is_empty()
        || !ui.is_empty()
        || !cameras.is_empty()
        || removed_cameras.read().count() > 0
        || removed_lists.read().count() > 0
        || panels.is_changed()
        || !windows.is_empty()
        || images.read().count() > 0
        || meshes.read().count() > 0
        || materials.read().count() > 0;
    if changed {
        redraw.linger = redraw.linger.max(3);
    }
    redraw.now = redraw.linger > 0;
    redraw.linger = redraw.linger.saturating_sub(1);
}

/// Frame / animation driving data retained from the original ActiveInfo.
/// Interaction (selection / hover / drag) has been moved to dedicated resources in the new picking pipeline.
#[derive(Debug, Resource)]
pub(crate) struct ActivityControl {
    pub is_in_worker: bool,
    pub auto_animate: bool,
    pub remaining_frames: u32,
}

impl ActivityControl {
    pub fn new() -> Self {
        ActivityControl {
            is_in_worker: false,
            auto_animate: true,
            remaining_frames: 0,
        }
    }
}

// -------------------------------------------------------------------------------------------------
// New interaction / picking scaffolding (to be wired in subsequent patches)
// -------------------------------------------------------------------------------------------------

#[derive(Default, Debug, Clone, Copy)]
pub struct ButtonSnapshot {
    pub left: bool,
    pub right: bool,
    pub middle: bool,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct ModifierSnapshot {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

#[derive(Resource, Debug, Default)]
pub struct PointerState {
    pub screen: Vec2,
    pub delta: Vec2,
    pub overlay_world: Option<Vec2>,
    pub world_ray: Option<Ray3d>,
    pub buttons: ButtonSnapshot,
    pub modifiers: ModifierSnapshot,
    pub just_pressed_left: bool,
    pub just_released_left: bool,
}

#[derive(Debug, Clone)]
pub struct Hit2D {
    pub entity: Entity,
    /// The hit node as reached from the panel: from the panel composition's
    /// top level down to the node, through every embed on the way
    /// (`bevy_app::select`). Selection rules pick a node along it.
    pub chain: Vec<iron_document::NodeId>,
    /// An embed's own area rather than a shape: it is transparent, so any
    /// shape under the pointer, at any depth, comes before it.
    pub background: bool,
    pub z: f32,
    /// From the panel's document space into the hit node's composition
    /// space: identity unless the node was reached through embeds.
    pub space: kurbo::Affine,
    /// Reached through a plane in 3D (camera, plane entity): the pointer maps
    /// into the composition by a ray, not an affine.
    pub through: Option<(Entity, Entity)>,
}

#[derive(Debug, Clone)]
pub struct Hit3D {
    pub entity: Entity,
    /// As for [`Hit2D::chain`]; through the 3D embed when one was looked into.
    pub chain: Vec<iron_document::NodeId>,
    pub distance: f32,
    /// The camera the ray was cast from: a 3D panel's, or a 3D embed's.
    pub camera: Entity,
}

#[derive(Resource, Debug, Default)]
pub struct PointerHits {
    /// The composition panel under the pointer; hits come only from it.
    pub panel: Option<String>,
    /// The 3D embed under the pointer when nothing 2D is drawn over it there
    /// (its `EmbedView` key): the 3D picker casts through its camera.
    pub embed: Option<String>,
    /// That embed's own chain (it ends with the embed node).
    pub embed_chain: Vec<iron_document::NodeId>,
    pub overlay: Vec<Hit2D>,
    pub world3d: Vec<Hit3D>,
    pub primary: Option<Entity>,
}

/// What the pointer picked, as document nodes (ECM §7.2). Selecting or
/// hovering a figure means its entity; `document_bridge::Highlights` derives
/// every node that lights up for it, in every panel.
#[derive(Resource, Debug, Default)]
pub struct SelectionState {
    /// In pick order: shift-click adds, so a Link knows its two figures.
    pub selected: Vec<iron_document::NodeId>,
    pub hovered: Option<iron_document::NodeId>,
}

#[derive(Debug, Clone, Copy)]
pub enum DragKind {
    Overlay2D,
    World3D,
    Group,
    /// Scrubbing a slider's value: previewed through the store each frame,
    /// committed as one `Set` on release.
    SliderValue,
}

#[derive(Resource, Debug)]
pub struct DragState {
    pub target: Option<Entity>,
    pub kind: Option<DragKind>,
    /// The store's handle for a 2D drag: which mode it runs in (through a
    /// constraint cluster, per axis, or not at all) and the grab point in
    /// the node's local space (constraint-system.md §8).
    pub handle: Option<iron_document::DragHandle>,
    /// The panel the drag started in: its rectangle and camera map the
    /// pointer for the whole gesture, wherever the pointer goes.
    pub panel: Option<String>,
    /// Panel document space → the dragged node's composition space (through
    /// any embeds it was picked through).
    pub space: kurbo::Affine,
    /// For a 3D drag, the camera that maps the pointer to a ray.
    pub camera: Option<Entity>,
    /// For a 2D drag through a plane in 3D: the camera and the plane.
    pub through: Option<(Entity, Entity)>,
    /// Where the press happened (window px). A move or 3D drag previews
    /// nothing until the pointer leaves it, so a click is not a move.
    pub press_screen: Vec2,
    pub plane_origin: Vec3,
    pub plane_normal: Vec3,
    pub grab_offset_world: Vec3,
}

impl Default for DragState {
    fn default() -> Self {
        Self {
            target: None,
            kind: None,
            handle: None,
            panel: None,
            space: kurbo::Affine::IDENTITY,
            camera: None,
            through: None,
            press_screen: Vec2::ZERO,
            plane_origin: Vec3::ZERO,
            plane_normal: Vec3::Y,
            grab_offset_world: Vec3::ZERO,
        }
    }
}

// Marker for a composite vector group (single VelloScene acting as many shapes)
#[derive(Component, Debug)]
pub struct GroupAggregate {
    pub version: u32,
    pub shape_count: u32,
}

impl Default for GroupAggregate {
    fn default() -> Self {
        Self {
            version: 0,
            shape_count: 0,
        }
    }
}

pub(crate) fn close_bevy_window(mut app: Box<WorkerApp>) {
    let mut windows_state: SystemState<Query<(Entity, &mut Window)>> =
        SystemState::from_world(app.world_mut());
    // bevy 0.19: get_mut returns Result. Without the `?`-style unwrap this silently
    // compiles as `Result::iter`, which yields the Query rather than its rows.
    let Ok(windows) = windows_state.get_mut(app.world_mut()) else {
        return;
    };
    let entity = windows.iter().last().map(|(entity, _)| entity);
    if let Some(entity) = entity {
        app.world_mut()
            .write_message(WindowCloseRequested { window: entity });
        windows_state.apply(app.world_mut());
        app.update();
    }
    // Dropping the WorkerApp (and its App) releases the wgpu device/surfaces.
}
