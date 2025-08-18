//! Bevy app module
//! Splits 3D scene setup, 2D overlay, and shared types/systems into submodules.

mod input_accum;
mod interaction;
mod overlay2d;
mod picking;
mod pointer;
mod scene3d;

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
use overlay2d::{animate_2d_overlay, setup_2d_overlay};
use picking::{pick_overlay_2d_system, pick_world_3d_system, resolve_primary_hit_system};
use pointer::pointer_collect_system;
use scene3d::{render_active_shapes, rotate_3d_shapes, setup_3d_scene, update_aabbes};

use crate::{
    WorkerApp,
    asset_reader::WebAssetPlugin,
    camera_controller::CameraControllerPlugin,
    ffi_inspector_bridge::{InspectorStreamingState, inspector_continuous_streaming_system},
    fps_overlay::FPSOverlayPlugin,
    tracking_circle::TrackingCircle,
};
use bevy_remote_inspector::RemoteInspectorPlugin;

const MAX_HISTORY_LENGTH: usize = 200;

pub(crate) fn init_app() -> WorkerApp {
    let mut app = App::new();

    let mut default_plugins = DefaultPlugins.set(ImagePlugin::default_nearest());

    default_plugins = default_plugins.set(bevy::window::WindowPlugin {
        primary_window: Some(bevy::window::Window {
            present_mode: bevy::window::PresentMode::AutoNoVsync,
            ..default()
        }),
        ..default()
    });

    app.insert_resource(ClearColor(Color::srgb(0.97, 0.97, 0.97)));

    app.add_plugins((
        WebAssetPlugin::default(),
        default_plugins,
        TrackingCircle,
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
        CameraControllerPlugin,
        RemoteInspectorPlugin,
    ));

    app.init_resource::<AccumulatedCursorDelta>();
    app.init_resource::<AccumulatedScroll>();
    app.init_resource::<InspectorStreamingState>();
    // New interaction resources
    app.insert_resource(crate::ActivityControl::new());
    app.init_resource::<crate::PointerState>();
    app.init_resource::<crate::PointerHits>();
    app.init_resource::<crate::SelectionState>();
    app.init_resource::<crate::DragState>();

    app.add_systems(Startup, (setup_3d_scene, setup_2d_overlay))
        .add_systems(
            Update,
            (
                update_aabbes,
                inspector_continuous_streaming_system,
                animate_2d_overlay, // TODO: refactor overlay interaction to new picking path
                rotate_3d_shapes,
            ),
        )
        .add_systems(
            PreUpdate,
            (
                accumulate_cursor_delta_system,
                accumulate_custom_scroll_system,
                pointer_collect_system,
                pick_overlay_2d_system,
                pick_world_3d_system,
                resolve_primary_hit_system,
            ),
        )
        .add_systems(
            PostUpdate,
            (
                interaction_decide_system,
                drag_apply_system,
                selection_reflect_system,
                outbound_hover_system,
                outbound_selection_system,
                render_active_shapes,
            ),
        );

    WorkerApp::new(app)
}
