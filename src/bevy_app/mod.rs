//! Bevy app module
//! Splits 3D scene setup, 2D overlay, and shared types/systems into submodules.

mod input_accum;
mod interaction;
mod picking;
mod pointer;
pub(crate) mod select;
mod pen;
pub(crate) mod scene3d;
mod timeline;
mod ui_panels;
mod alpha_stress;

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use crate::vector::{VectorCamera, VectorPlugin};
use crate::document_bridge::{DocumentPlugin, reconcile_gesture};

/// The drag's preview, reconcile and command flush, in that order.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GestureSystems;

pub use input_accum::*;
pub(crate) use pointer::PressPosition;
pub(crate) use scene3d::ScreenRect as ScreenRectMarker;
pub(crate) use scene3d::ActiveState as ActiveStateMarker;
// Bring required items into scope from submodules
use interaction::{
    PendingCommit, drag_apply_system, drag_commit_system,
    outbound_hover_system, outbound_selection_system, selection_reflect_system,
};
use picking::{pick_document_2d_system, pick_world_3d_system, pointer_panel_system, resolve_primary_hit_system};
use pointer::pointer_collect_system;
use scene3d::{apply_focus, render_active_shapes, setup_3d_scene, sync_embed_cameras, sync_panel_cameras, sync_view_images, update_aabbes};
use timeline::TimelinePlugin;

use crate::{
    WorkerApp,
    asset_reader::WebAssetPlugin,
    canvas_view::{ViewObj, announce_canvas_window, spawn_canvas_window},
    camera_controller::CameraControllerPlugin,
    fps_overlay::FPSOverlayPlugin,
    // tracking_circle::TrackingCircle,
};

const MAX_HISTORY_LENGTH: usize = 200;

/// Perf-grid variant flags, passed from JS via `init_bevy_app_with_canvas`.
/// Selected with the `?bevy=` URL param (see wasm_loader.ts) so one wasm artifact
/// serves every grid cell. flags == 0 runs whatever this file currently enables.
pub const VARIANT_NO_LOG: u32 = 1 << 0;
pub const VARIANT_MIN_PLUGINS: u32 = 1 << 1;
pub const VARIANT_EMPTY: u32 = 1 << 2;
/// Add the alpha-blending stress fixture (`?bevy=alpha`). Combines with
/// the other diagnostic variants.
pub const VARIANT_ALPHA_STRESS: u32 = 1 << 4;
/// Explicitly initialize classic Vello instead of Hybrid. Requires the `classic`
/// feature (WebGPU builds only); ignored otherwise. The ordinary app defaults to
/// a Hybrid-only plugin set so classic's eager compute-pipeline creation cannot
/// affect cold start.
pub const VARIANT_CLASSIC_BACKEND: u32 = 1 << 5;

/// The canvas background in the light and dark themes: behind 2D panels and
/// in 3D views. Both match the shell's `--bg` (src-ui/app.css), so the
/// active tab joins the canvas beneath it.
pub const CANVAS_LIGHT: Color = Color::WHITE;
pub const CANVAS_DARK: Color = Color::srgb_u8(0x12, 0x13, 0x14);
/// Render every frame (`?bevy=always`): render on demand off, for comparing
/// frame pacing and latency against it on a real display.
pub const VARIANT_ALWAYS_RENDER: u32 = 1 << 6;

/// Build the app around an already-created canvas.
///
/// The canvas is a construction parameter rather than something attached later,
/// because `RenderPlugin::build` resolves the GPU adapter and needs the primary
/// window to exist at that moment. On WebGL2 the adapter *is* the canvas's GL
/// context, so there is no adapter to find without it.
pub(crate) fn init_app(variant_flags: u32, view: ViewObj) -> WorkerApp {
    let no_log = variant_flags & VARIANT_NO_LOG != 0;
    let min_plugins = variant_flags & VARIANT_MIN_PLUGINS != 0;
    let empty = variant_flags & VARIANT_EMPTY != 0;
    let alpha_stress = variant_flags & VARIANT_ALPHA_STRESS != 0;
    #[cfg(feature = "classic")]
    let classic_backend = variant_flags & VARIANT_CLASSIC_BACKEND != 0;

    let mut app = App::new();

    app.insert_resource(ClearColor(CANVAS_LIGHT));

    // Before any plugin: `RenderPlugin::build` reads the primary window to pick
    // an adapter, and on WebGL2 finds none without it.
    let window_entity = spawn_canvas_window(&mut app, view);

    if min_plugins {
        // Perf-grid cell B3 (`?bevy=min`): the smallest plugin set that can boot a
        // window and present. Mirrors DefaultPlugins order up through ImagePlugin;
        // omits LogPlugin, WinitPlugin, ScenePlugin, and everything above the
        // renderer (core pipeline, sprite/text/ui/pbr/gltf/gizmos/picking).
        app.add_plugins((
            bevy::app::PanicHandlerPlugin,
            bevy::app::TaskPoolPlugin::default(),
            bevy::diagnostic::FrameCountPlugin,
            bevy::time::TimePlugin,
            bevy::transform::TransformPlugin,
            bevy::diagnostic::DiagnosticsPlugin,
            bevy::input::InputPlugin,
            bevy::window::WindowPlugin {
                primary_window: None,
                ..default()
            },
            bevy::a11y::AccessibilityPlugin,
            bevy::asset::AssetPlugin::default(),
            bevy::render::RenderPlugin::default(),
            ImagePlugin::default_nearest(),
        ));
        init_shared_resources(&mut app);
        announce_canvas_window(&mut app);
        return WorkerApp::new(app, window_entity);
    }

    let mut default_plugins = DefaultPlugins.set(ImagePlugin::default_nearest());

    // By default, a primary window gets spawned by `WindowPlugin`, contained in `DefaultPlugins`
    // Do NOT create an implicit primary window; all windows are created explicitly
    // by `spawn_canvas_window` above, before this plugin set is added.
    default_plugins = default_plugins.set(bevy::window::WindowPlugin {
        primary_window: None,
        ..default()
    });

    // No WinitPlugin is enabled. Winit could not run in a worker anyway (the
    // frame loop is driven from JS via enter_frame), and windows come from our
    // own canvas bootstrap.
    let mut default_plugins = default_plugins.build();

    // Perf-grid cell B2 (`?bevy=nolog`): LogPlugin installs tracing-wasm on the web,
    // which emits performance.mark/measure for every system span every frame.
    if no_log {
        default_plugins = default_plugins.disable::<bevy::log::LogPlugin>();
    }

    if empty {
        // Perf-grid cells B0/B1 (`?bevy=empty`): DefaultPlugins only, no cameras,
        // no app plugins — "rendering absolutely nothing".
        app.add_plugins(default_plugins);
        init_shared_resources(&mut app);
        announce_canvas_window(&mut app);
        return WorkerApp::new(app, window_entity);
    }

    app.add_plugins((
        // WebAssetPlugin::default(),
        default_plugins,
        // TrackingCircle,
        // Exactly one renderer is installed. Classic cannot be added later:
        // `VelloPlugin` builds its compute pipelines in `Plugin::finish`, so a
        // present-but-idle classic backend would still cost cold start. Switching
        // renderers therefore rebuilds the `App` rather than toggling a resource.
        #[cfg(feature = "classic")]
        if classic_backend {
            VectorPlugin::classic_only()
        } else {
            VectorPlugin::hybrid_only()
        },
        #[cfg(not(feature = "classic"))]
        VectorPlugin::hybrid_only(),
        FPSOverlayPlugin,
        FrameTimeDiagnosticsPlugin {
            max_history_length: MAX_HISTORY_LENGTH,
            smoothing_factor: 2.0 / (MAX_HISTORY_LENGTH as f64 + 1.0),
        },
        CameraControllerPlugin,
        TimelinePlugin,
        // The authored store and its projection into entities. Scene content
        // (the rects, the circle, the torus) comes from here as document nodes.
        DocumentPlugin,
    ));

    init_shared_resources(&mut app);
    announce_canvas_window(&mut app);
    // Every view renders into its own texture and the window only composites
    // them (plans/specs/view-textures.md), so a frame that changed nothing visible
    // need not render at all.
    if variant_flags & VARIANT_ALWAYS_RENDER == 0 {
        app.init_resource::<crate::RedrawNeeded>();
        app.add_systems(Last, crate::decide_redraw);
    }

    // ============================ RE-ENABLE LADDER =============================
    // flags=0 escalation for the 5K frame-skip hunt. Uncomment ONE step at a time,
    // rebuild (./build-wasm.sh), run full-screen 5K, record __probeReset()/__probe().
    // The first step where rafDeltaMs.p95 blows up while tickMs stays low is the
    // culprit. Each step assumes the previous steps stay enabled.

    // --- STEP 1 (retired): a background camera ------------------------------
    // The window has one camera now, the vector composite camera, which clears
    // to the background itself (plans/specs/view-textures.md).

    // --- STEP 2: vector backend camera ---------------------------------------
    // The backend spawns and owns its full-window compositing camera (order 10).
    // This step only marks it as the camera bevy_ui draws into, which is what
    // starts the FPS overlay rendering.
    // PostStartup, not Startup: the backend spawns its camera with `Commands`, so
    // the resource naming it does not exist until those commands are applied at
    // the end of Startup.
    app.add_systems(PostStartup, adopt_backend_camera_for_ui);

    // --- STEP 3: 3D scene + panel cameras ------------------------------------
    // Lights and ground; one viewport-scoped camera per 3D composition panel
    // (ECM §7.1). Meshes are document nodes (DocumentPlugin).
    app.add_systems(Startup, setup_3d_scene);
    app.add_systems(Update, (sync_panel_cameras, apply_focus).chain());

    // --- STEP 4: UI panels + remaining Update systems -------------------------
    app.add_systems(Update, ui_panels::render_ui_panels);
    app.add_systems(PostUpdate, crate::panels::sync_panel_vector_views);

    // --- STEP 5: input/picking/interaction pipelines --------------------------
    app.add_systems(
        PreUpdate,
        (
            accumulate_cursor_delta_system,
            accumulate_custom_scroll_system,
            pointer_collect_system.after(accumulate_cursor_delta_system),
            pointer_panel_system.after(pointer_collect_system),
            pick_document_2d_system.after(pointer_panel_system).after(crate::document_bridge::sync_document),
            // After the 2D picker, which finds the 3D embed on top.
            pick_world_3d_system.after(pick_document_2d_system),
            resolve_primary_hit_system
                .after(pick_document_2d_system)
                .after(pick_world_3d_system),
        ),
    );
    app.add_systems(
        PostUpdate,
        (
            select::navigate_views.before(pen::pen_tool_system),
            pen::pen_tool_system.before(select::select_tool_system),
            select::select_tool_system,
            select::sync_session_selection.after(select::select_tool_system),
            // A drag previews into the store, the bridge rewrites the
            // affected entities, and the deferred inserts are applied, all
            // before propagation so transform consumers and the 2D layer see
            // this frame's pointer position (constraint-system.md §8.2).
            (drag_apply_system, reconcile_gesture, ApplyDeferred)
                .chain()
                .in_set(GestureSystems)
                .after(select::select_tool_system)
                .before(TransformSystems::Propagate),
            // Writeback: the released drag becomes one gesture commit,
            // applied by the document sync next frame.
            drag_commit_system.after(GestureSystems),
            update_aabbes.after(GestureSystems),
            // A 3D embed's camera follows its rectangle in the same frame,
            // after the meshes' volumes it may frame are current.
            sync_embed_cameras
                .after(crate::document_bridge::nest::collect_embed_views)
                .after(update_aabbes),
            // Texture-backed cameras (`?bevy=views`): targets, composites and
            // whether each renders this frame. After everything that moves
            // what they show, including the gizmo highlight state.
            sync_view_images
                .after(sync_embed_cameras)
                .after(TransformSystems::Propagate)
                .after(selection_reflect_system)
                // Before frusta and visibility: a camera woken this frame
                // must be culled and lit as an active view this frame.
                .before(bevy::camera::visibility::VisibilitySystems::UpdateFrusta)
                // The 2D layer draws 3D embeds' textures by the versions
                // published here.
                .before(crate::document_bridge::render_document_layer),
            // Highlights before the 2D layer draws (after propagation) and
            // before the 3D outlines read them.
            crate::document_bridge::update_highlights
                .after(select::select_tool_system)
                .before(TransformSystems::Propagate),
            selection_reflect_system.after(crate::document_bridge::update_highlights),
            outbound_hover_system.after(select::select_tool_system),
            outbound_selection_system.after(select::select_tool_system),
            select::draw_select_overlay.after(TransformSystems::Propagate),
            render_active_shapes
                .after(GestureSystems)
                .after(selection_reflect_system),
        ),
    );
    if alpha_stress {
        app.add_systems(Startup, alpha_stress::setup_alpha_stress);
        app.add_systems(
            Update,
            (
                alpha_stress::render_alpha_stress_static,
                alpha_stress::render_alpha_stress_animated,
            ),
        );
    }

    // ========================== END RE-ENABLE LADDER ===========================

    WorkerApp::new(app, window_entity)
}

/// Resources the FFI layer touches (Option-guarded there); initialized for every
/// perf-grid variant so FFI behavior is uniform across cells.
fn init_shared_resources(app: &mut App) {
    app.init_resource::<AccumulatedCursorDelta>();
    app.init_resource::<AccumulatedScroll>();
    app.init_resource::<crate::panels::Panels>();
    // New interaction resources
    app.insert_resource(crate::ActivityControl::new());
    app.init_resource::<crate::PointerState>();
    app.init_resource::<crate::PointerHits>();
    app.init_resource::<crate::SelectionState>();
    app.init_resource::<crate::DragState>();
    app.init_resource::<PendingCommit>();
    app.init_resource::<pointer::PressPosition>();
    app.init_resource::<select::SelectTool>();
    app.init_resource::<select::SelectionGeometryCache>();
    app.init_resource::<select::ActiveTool>();
    app.init_resource::<pen::PenTool>();
    app.init_resource::<select::ShellModifiers>();
    app.init_resource::<select::ToolKeys>();
}

/// STEP 2 (ladder): let bevy_ui draw into the backend's compositing camera.
///
/// The camera itself belongs to the vector backend — it is where that backend's
/// output lands — so the app only expresses the policy that UI shares it.
fn adopt_backend_camera_for_ui(
    mut commands: Commands,
    camera: Option<Res<VectorCamera>>,
) {
    // Optional so that selecting a backend which does not provide this camera is
    // a warning rather than a panic — but loud, because the symptom otherwise is
    // "the FPS overlay silently vanished".
    let Some(camera) = camera else {
        warn!("no vector composite camera; bevy_ui has nothing to draw into");
        return;
    };
    commands.entity(camera.0).insert(bevy::ui::IsDefaultUiCamera);
}
