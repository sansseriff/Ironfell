//! Bevy app module
//! Splits 3D scene setup, 2D overlay, and shared types/systems into submodules.

mod input_accum;
mod interaction;
mod overlay2d;
mod picking;
mod pointer;
mod scene3d;
mod timeline;
mod ui_panels;
mod vello_world_demo;

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::prelude::*;
use bevy::camera::visibility::RenderLayers;
use bevy::transform::TransformSystems;
use crate::vector::{ActiveBackends, ClassicBackendCamera, VectorPlugin};

pub use input_accum::*;
// Bring required items into scope from submodules
use interaction::{
    drag_apply_system, interaction_decide_system, outbound_hover_system, outbound_selection_system,
    selection_reflect_system,
};
use overlay2d::{
    DraggableSquare, SimpleMouseState, animate_2d_overlay, render_draggable_square,
    setup_2d_overlay, simple_mouse_state_system, update_draggable_square_state,
    update_mini_square_entities, render_mini_squares, render_selection_marquee
};
use picking::{pick_overlay_2d_system, pick_world_3d_system, resolve_primary_hit_system};
use pointer::pointer_collect_system;
use scene3d::{render_active_shapes, rotate_3d_shapes, setup_3d_scene, update_aabbes};
use timeline::TimelinePlugin;
use vello_world_demo::{animate_world_space_demo, setup_world_space_demo};

use crate::{
    WorkerApp,
    asset_reader::WebAssetPlugin,
    camera_controller::CameraControllerPlugin,
    ffi_inspector_bridge::{InspectorStreamingState, inspector_continuous_streaming_system},
    fps_overlay::FPSOverlayPlugin,
    // tracking_circle::TrackingCircle,
};
use bevy_remote_inspector::RemoteInspectorPlugin;

const MAX_HISTORY_LENGTH: usize = 200;

/// Perf-grid variant flags, passed from JS via `init_bevy_app(variant_flags)`.
/// Selected with the `?bevy=` URL param (see wasm_loader.ts) so one wasm artifact
/// serves every grid cell. flags == 0 runs whatever this file currently enables.
pub const VARIANT_NO_LOG: u32 = 1 << 0;
pub const VARIANT_MIN_PLUGINS: u32 = 1 << 1;
pub const VARIANT_EMPTY: u32 = 1 << 2;

pub(crate) fn init_app(variant_flags: u32) -> WorkerApp {
    let no_log = variant_flags & VARIANT_NO_LOG != 0;
    let min_plugins = variant_flags & VARIANT_MIN_PLUGINS != 0;
    let empty = variant_flags & VARIANT_EMPTY != 0;

    let mut app = App::new();

    app.insert_resource(ClearColor(Color::srgb(0.97, 0.97, 0.97)));

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
        return WorkerApp::new(app);
    }

    let mut default_plugins = DefaultPlugins.set(ImagePlugin::default_nearest());

    // By default, a primary window gets spawned by `WindowPlugin`, contained in `DefaultPlugins`
    // Do NOT create an implicit primary window; all windows are created explicitly
    // from JS via create_window_by_offscreen_canvas with deterministic IDs.
    default_plugins = default_plugins.set(bevy::window::WindowPlugin {
        primary_window: None,
        ..default()
    });

    // No WinitPlugin to disable here: bevy_vello `main` no longer force-enables bevy's
    // `bevy_winit` feature, so it is not in the graph at all. Winit could not run in a
    // worker anyway (the frame loop is driven from JS via enter_frame), and windows come
    // from our own canvas bootstrap. If a future dependency drags bevy_winit back in,
    // this is where it must be disabled again.
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
        return WorkerApp::new(app);
    }

    app.add_plugins((
        // WebAssetPlugin::default(),
        default_plugins,
        // TrackingCircle,
        // Owns the display-list seam and every backend that can realize it.
        // Which backend is live is an `ActiveBackends` resource, changeable at
        // runtime; two can run at once for a side-by-side comparison.
        VectorPlugin {
            backends: ActiveBackends::default(),
        },
        FPSOverlayPlugin,
        FrameTimeDiagnosticsPlugin {
            max_history_length: MAX_HISTORY_LENGTH,
            smoothing_factor: 2.0 / (MAX_HISTORY_LENGTH as f64 + 1.0),
        },
        CameraControllerPlugin,
        RemoteInspectorPlugin,
        TimelinePlugin,
    ));

    init_shared_resources(&mut app);

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

    // --- STEP 3: 3D scene + viewport camera ----------------------------------
    // MainCamera3D (viewport-scoped, driven by the "viewer" panel rect) + meshes.
    app.add_systems(Startup, setup_3d_scene);
    app.add_systems(Update, (
        apply_viewer_viewport, 
        rotate_3d_shapes,
    ));

    // --- STEP 4: 2D overlay + UI panels + remaining Update systems -----------
    app.add_systems(
        Startup,
        (
            setup_2d_overlay,
            ui_panels::setup_ui_panels,
            // World-space counterpart to the screen-space overlays above; see
            // vello_world_demo for what the two coordinate spaces imply.
            setup_world_space_demo,
        ),
    );
    app.add_systems(
        Update,
        (
            ui_panels::render_ui_panels,
            animate_world_space_demo,
            inspector_continuous_streaming_system,
            animate_2d_overlay, // TODO: refactor overlay interaction to new picking path
            simple_mouse_state_system,
            update_draggable_square_state.after(simple_mouse_state_system),
            render_draggable_square.after(update_draggable_square_state),
            update_mini_square_entities.after(simple_mouse_state_system),
            render_mini_squares.after(update_mini_square_entities),
            render_selection_marquee.after(update_mini_square_entities),
        ),
    );

    // --- STEP 5: input/picking/interaction pipelines --------------------------
    app.add_systems(
        PreUpdate,
        (
            accumulate_cursor_delta_system,
            accumulate_custom_scroll_system,
            pointer_collect_system.after(accumulate_cursor_delta_system),
            pick_overlay_2d_system.after(pointer_collect_system),
            pick_world_3d_system.after(pointer_collect_system),
            resolve_primary_hit_system
                .after(pick_overlay_2d_system)
                .after(pick_world_3d_system),
        ),
    );
    app.add_systems(
        PostUpdate,
        (
            interaction_decide_system,
            // Must land before propagation: bevy_vello extracts scenes via
            // GlobalTransform, so a correction written after this would be a frame late.
            drag_apply_system
                .after(interaction_decide_system)
                .before(TransformSystems::Propagate),
            update_aabbes.after(drag_apply_system),
            selection_reflect_system.after(interaction_decide_system),
            outbound_hover_system.after(interaction_decide_system),
            outbound_selection_system.after(interaction_decide_system),
            render_active_shapes
                .after(drag_apply_system)
                .after(selection_reflect_system),
        ),
    );
    // ========================== END RE-ENABLE LADDER ===========================

    WorkerApp::new(app)
}

/// Resources the FFI layer touches (Option-guarded there); initialized for every
/// perf-grid variant so FFI behavior is uniform across cells.
fn init_shared_resources(app: &mut App) {
    app.init_resource::<AccumulatedCursorDelta>();
    app.init_resource::<AccumulatedScroll>();
    app.init_resource::<InspectorStreamingState>();
    app.init_resource::<crate::panels::Panels>();
    // New interaction resources
    app.insert_resource(crate::ActivityControl::new());
    app.init_resource::<crate::PointerState>();
    app.init_resource::<crate::PointerHits>();
    app.init_resource::<crate::SelectionState>();
    app.init_resource::<crate::DragState>();
    // Overlay interaction resources
    app.init_resource::<DraggableSquare>();
    app.init_resource::<SimpleMouseState>();
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
    camera: Option<Res<ClassicBackendCamera>>,
) {
    // Optional so that selecting a backend which does not provide this camera is
    // a warning rather than a panic — but loud, because the symptom otherwise is
    // "the FPS overlay silently vanished".
    let Some(camera) = camera else {
        warn!("no vector backend camera; bevy_ui has nothing to draw into");
        return;
    };
    commands.entity(camera.0).insert(bevy::ui::IsDefaultUiCamera);
}

/// Mirror the "viewer" panel rect (posted from JS) onto the 3D camera's viewport.
/// The camera stays inactive until the panel exists; the rect is clamped to the
/// window so resize races can never produce an out-of-bounds viewport.
fn apply_viewer_viewport(
    panels: Res<crate::panels::Panels>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut cameras: Query<&mut Camera, With<scene3d::MainCamera3D>>,
) {
    let Ok(mut camera) = cameras.single_mut() else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };

    let Some(rect) = panels.rect(crate::panels::VIEWER_PANEL) else {
        if camera.is_active {
            camera.is_active = false;
        }
        return;
    };

    let win_w = window.resolution.physical_width();
    let win_h = window.resolution.physical_height();
    if win_w == 0 || win_h == 0 {
        return;
    }

    let x = (rect.x.max(0.0) as u32).min(win_w.saturating_sub(1));
    let y = (rect.y.max(0.0) as u32).min(win_h.saturating_sub(1));
    let w = (rect.w.max(1.0) as u32).min(win_w - x).max(1);
    let h = (rect.h.max(1.0) as u32).min(win_h - y).max(1);

    let viewport = bevy::camera::Viewport {
        physical_position: UVec2::new(x, y),
        physical_size: UVec2::new(w, h),
        ..default()
    };

    let changed = match &camera.viewport {
        Some(v) => v.physical_position != viewport.physical_position
            || v.physical_size != viewport.physical_size,
        None => true,
    };
    if changed {
        camera.viewport = Some(viewport);
    }
    if !camera.is_active {
        camera.is_active = true;
    }
}
