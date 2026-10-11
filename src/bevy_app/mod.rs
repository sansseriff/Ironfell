//! Bevy app module
//! Splits 3D scene setup, 2D overlay, and shared types/systems into submodules.

mod input_accum;
mod interaction;
mod picking;
mod pointer;
pub(crate) mod scene3d;
mod timeline;
mod ui_panels;
mod alpha_stress;

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::prelude::*;
use bevy::camera::visibility::RenderLayers;
use bevy::transform::TransformSystems;
use crate::vector::{VectorCamera, VectorPlugin};
use crate::document_bridge::{DocumentPlugin, reconcile_gesture};

/// The drag's preview, reconcile and command flush, in that order.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GestureSystems;

pub use input_accum::*;
// Bring required items into scope from submodules
use interaction::{
    PendingCommit, drag_apply_system, drag_commit_system, interaction_decide_system,
    outbound_hover_system, outbound_selection_system, selection_reflect_system,
};
use picking::{pick_document_2d_system, pick_world_3d_system, pointer_panel_system, resolve_primary_hit_system};
use pointer::pointer_collect_system;
use scene3d::{render_active_shapes, setup_3d_scene, sync_panel_cameras, update_aabbes};
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

    app.insert_resource(ClearColor(Color::srgb(0.97, 0.97, 0.97)));

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

    // ============================ RE-ENABLE LADDER =============================
    // flags=0 escalation for the 5K frame-skip hunt. Uncomment ONE step at a time,
    // rebuild (./build-wasm.sh), run full-screen 5K, record __probeReset()/__probe().
    // The first step where rafDeltaMs.p95 blows up while tickMs stays low is the
    // culprit. Each step assumes the previous steps stay enabled.

    // --- STEP 1: background camera only -------------------------------------
    // One full-window Camera2d. This alone switches the render graph from the
    // trivial no-camera clear to the full camera pipeline at 5K: main-pass
    // texture allocation, MSAA (default = 4x!), tonemapping, final upscale blit.
    // A/B within this step: spawn with `Msaa::Off` (see setup_background_camera)
    // to separate "camera pipeline" from "4x multisampled 5K target".
    app.add_systems(Startup, setup_background_camera);

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
    app.add_systems(Update, sync_panel_cameras);

    // --- STEP 4: UI panels + remaining Update systems -------------------------
    app.add_systems(Startup, ui_panels::setup_ui_panels);
    app.add_systems(Update, ui_panels::render_ui_panels);

    // --- STEP 5: input/picking/interaction pipelines --------------------------
    app.add_systems(
        PreUpdate,
        (
            accumulate_cursor_delta_system,
            accumulate_custom_scroll_system,
            pointer_collect_system.after(accumulate_cursor_delta_system),
            pointer_panel_system.after(pointer_collect_system),
            pick_document_2d_system.after(pointer_panel_system),
            pick_world_3d_system.after(pointer_panel_system),
            resolve_primary_hit_system
                .after(pick_document_2d_system)
                .after(pick_world_3d_system),
        ),
    );
    app.add_systems(
        PostUpdate,
        (
            interaction_decide_system,
            // A drag previews into the store, the bridge rewrites the
            // affected entities, and the deferred inserts are applied, all
            // before propagation so transform consumers and the 2D layer see
            // this frame's pointer position (constraint-system.md §8.2).
            (drag_apply_system, reconcile_gesture, ApplyDeferred)
                .chain()
                .in_set(GestureSystems)
                .after(interaction_decide_system)
                .before(TransformSystems::Propagate),
            // Writeback: the released drag becomes one gesture commit,
            // applied by the document sync next frame.
            drag_commit_system.after(GestureSystems),
            update_aabbes.after(GestureSystems),
            // Highlights before the 2D layer draws (after propagation) and
            // before the 3D outlines read them.
            crate::document_bridge::update_highlights
                .after(interaction_decide_system)
                .before(TransformSystems::Propagate),
            selection_reflect_system.after(crate::document_bridge::update_highlights),
            outbound_hover_system.after(interaction_decide_system),
            outbound_selection_system.after(interaction_decide_system),
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
}

/// Full-window helper cameras for the single-canvas architecture, split so the
/// re-enable ladder can bring them back one at a time:
/// - background camera (order -10) clears the whole window to the app background color
/// - vello camera (order 10) draws the full-window vello texture on top, no clear
///
/// The 3D viewer camera (order 0, viewport-scoped) is spawned in `setup_3d_scene`.
///
/// STEP 1 (ladder): background camera only. For the MSAA A/B, add
/// `bevy::render::view::Msaa::Off` to the spawn below — cameras default to
/// Msaa::Sample4, which at 5K means a 4x multisampled full-window main texture.
fn setup_background_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        bevy::render::view::Msaa::Off,
        Camera {
            order: -10,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.97, 0.97, 0.97)),
            ..default()
        },
        RenderLayers::none(),
        Name::new("Background Camera"),
    ));
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
