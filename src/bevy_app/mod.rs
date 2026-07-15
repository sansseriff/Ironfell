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

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::prelude::*;
use bevy::render::view::RenderLayers;
use bevy_vello::{VelloPlugin, prelude::*};

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

pub(crate) fn init_app() -> WorkerApp {
    let mut app = App::new();

    let mut default_plugins = DefaultPlugins.set(ImagePlugin::default_nearest());

    // By default, a primary window gets spawned by `WindowPlugin`, contained in `DefaultPlugins`
    // Do NOT create an implicit primary window; all windows are created explicitly
    // from JS via create_window_by_offscreen_canvas with deterministic IDs.
    default_plugins = default_plugins.set(bevy::window::WindowPlugin {
        primary_window: None,
        ..default()
    });

    // crates.io bevy_vello enables bevy's `bevy_winit` feature, which puts WinitPlugin
    // into DefaultPlugins. Winit cannot run in a worker (and we drive the loop from JS
    // via enter_frame anyway), so strip it and use our own canvas window bootstrap.
    let default_plugins = default_plugins
        .build()
        .disable::<bevy::winit::WinitPlugin>().disable::<bevy::log::LogPlugin>();

    app.insert_resource(ClearColor(Color::srgb(0.97, 0.97, 0.97)));

    app.add_plugins((
        // WebAssetPlugin::default(),
        default_plugins,
        // TrackingCircle,
        VelloPlugin {
            canvas_render_layers: RenderLayers::layer(1),
            use_cpu: false,
            antialiasing: vello::AaConfig::Area,
        },
        FPSOverlayPlugin,
        FrameTimeDiagnosticsPlugin {
            max_history_length: MAX_HISTORY_LENGTH,
            smoothing_factor: 2.0 / (MAX_HISTORY_LENGTH as f64 + 1.0),
        },
        // CameraControllerPlugin,
        // RemoteInspectorPlugin,
        // TimelinePlugin,
    ));

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

    // app.add_systems(
    //     Startup,
    //     (
    //         setup_cameras,
    //         // setup_3d_scene,
    //         // setup_2d_overlay,
    //         // ui_panels::setup_ui_panels,
    //     ),
    // );
        // .add_systems(
        //     Update,
        //     (
        //         // apply_viewer_viewport,
        //         ui_panels::render_ui_panels,
        //         update_aabbes,
        //         // inspector_continuous_streaming_system,
        //         // animate_2d_overlay, // TODO: refactor overlay interaction to new picking path
        //         rotate_3d_shapes,
        //         simple_mouse_state_system,
        //         // update_draggable_square_state,
        //         // render_draggable_square,
        //         // update_mini_square_entities,
        //         // render_mini_squares,
        //         // render_selection_marquee
        //     ),
        // )
        // .add_systems(
        //     PreUpdate,
        //     (
        //         accumulate_cursor_delta_system,
        //         accumulate_custom_scroll_system,
        //         pointer_collect_system,
        //         // pick_overlay_2d_system,
        //         pick_world_3d_system,
        //         resolve_primary_hit_system,
        //     ),
        // )
        // .add_systems(
        //     PostUpdate,
        //     (
        //         interaction_decide_system,
        //         drag_apply_system,
        //         selection_reflect_system,
        //         outbound_hover_system,
        //         outbound_selection_system,
        //         render_active_shapes,
        //     ),
        // );

    WorkerApp::new(app)
}

/// Full-window helper cameras for the single-canvas architecture:
/// - background camera (order -10) clears the whole window to the app background color
/// - vello camera (order 10) draws the full-window vello texture on top, no clear
///
/// The 3D viewer camera (order 0, viewport-scoped) is spawned in `setup_3d_scene`.
fn setup_cameras(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        Camera {
            order: -10,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.97, 0.97, 0.97)),
            ..default()
        },
        RenderLayers::none(),
        Name::new("Background Camera"),
    ));

    commands.spawn((
        Camera2d,
        Camera {
            order: 10,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        RenderLayers::layer(1),
        VelloView,
        bevy::ui::IsDefaultUiCamera,
        Name::new("Vello Camera"),
    ));
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

    let viewport = bevy::render::camera::Viewport {
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
