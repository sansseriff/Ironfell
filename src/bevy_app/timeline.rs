use bevy::prelude::*;
use bevy::render::view::RenderLayers;
use bevy_vello::prelude::*;

use crate::panels::{Panels, TIMELINE_PANEL};

/// Timeline plugin: draws the timeline into its panel rect (screen space, clipped).
/// No dedicated camera/window — the shared full-window vello camera presents it.
pub struct TimelinePlugin;

impl Plugin for TimelinePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TimelineState>()
            .add_systems(Startup, setup_timeline_scenes)
            .add_systems(Update, (update_timeline_view, render_timeline_grid));
    }
}

/// Resource to manage timeline state and configuration
#[derive(Resource, Debug)]
pub struct TimelineState {
    pub zoom: f64,
    pub offset: f64,
    pub duration: f64,
    pub current_time: f64,
    pub playing: bool,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            offset: 0.0,
            duration: 30.0, // 30 seconds default
            current_time: 0.0,
            playing: false,
        }
    }
}

/// Marker component for timeline background scene
#[derive(Component)]
pub struct TimelineBackgroundScene;

/// Marker component for timeline grid scene
#[derive(Component)]
pub struct TimelineGridScene;

/// Marker component for timeline playhead scene
#[derive(Component)]
pub struct TimelinePlayheadScene;

fn setup_timeline_scenes(mut commands: Commands) {
    // Layer 1 = the vello camera's RenderLayers; scenes on other layers are culled.
    commands.spawn((
        VelloScene::new(),
        VelloScreenSpace,
        RenderLayers::layer(1),
        TimelineBackgroundScene,
    ));
    commands.spawn((
        VelloScene::new(),
        VelloScreenSpace,
        RenderLayers::layer(1),
        TimelineGridScene,
    ));
    commands.spawn((
        VelloScene::new(),
        VelloScreenSpace,
        RenderLayers::layer(1),
        TimelinePlayheadScene,
    ));
}

/// Update timeline view based on current state
pub fn update_timeline_view(mut timeline: ResMut<TimelineState>, time: Res<Time>) {
    // Update current time if playing
    if timeline.playing {
        timeline.current_time += time.delta_secs_f64();
        if timeline.current_time > timeline.duration {
            timeline.current_time = timeline.duration;
            timeline.playing = false; // Stop at end
        }
    }
}

/// Render the timeline background, grid and playhead into the timeline panel rect.
pub fn render_timeline_grid(
    mut bg_scene: Query<
        &mut VelloScene,
        (
            With<TimelineBackgroundScene>,
            Without<TimelineGridScene>,
            Without<TimelinePlayheadScene>,
        ),
    >,
    mut grid_scene: Query<
        &mut VelloScene,
        (With<TimelineGridScene>, Without<TimelinePlayheadScene>),
    >,
    mut playhead_scene: Query<
        &mut VelloScene,
        (With<TimelinePlayheadScene>, Without<TimelineGridScene>),
    >,
    timeline: Res<TimelineState>,
    panels: Res<Panels>,
) {
    let rect = panels.rect(TIMELINE_PANEL);

    // Background (replaces the old timeline camera's clear color)
    if let Ok(mut scene) = bg_scene.single_mut() {
        scene.reset();
        if let Some(rect) = rect {
            scene.fill(
                peniko::Fill::NonZero,
                kurbo::Affine::IDENTITY,
                peniko::Color::new([0.145, 0.145, 0.152, 1.0]),
                None,
                &rect.to_kurbo(),
            );
        }
    }

    let Some(rect) = rect else {
        for mut scene in grid_scene.iter_mut() {
            scene.reset();
        }
        for mut scene in playhead_scene.iter_mut() {
            scene.reset();
        }
        return;
    };

    let clip = rect.to_kurbo();
    let left = rect.x as f64;
    let top = rect.y as f64;
    let bottom = (rect.y + rect.h) as f64;
    let width = rect.w as f64;

    // Render grid
    if let Ok(mut scene) = grid_scene.single_mut() {
        scene.reset();
        scene.push_layer(peniko::Mix::Clip, 1.0, kurbo::Affine::IDENTITY, &clip);

        // Draw time grid lines across the panel width
        let time_per_pixel: f64 = timeline.duration / width;
        let major_step: f64 = 5.0; // Major grid line every 5 seconds
        let minor_step: f64 = 1.0; // Minor grid line every 1 second

        let mut time: f64 = 0.0;
        while time <= timeline.duration {
            let x: f64 = left + (time / time_per_pixel);
            let line = kurbo::Line::new((x, top), (x, bottom));

            if (time % major_step).abs() < 0.01 {
                // Major line - thicker and brighter
                scene.stroke(
                    &kurbo::Stroke::new(2.0),
                    kurbo::Affine::IDENTITY,
                    peniko::Color::new([0.5, 0.5, 0.5, 1.0]),
                    None,
                    &line,
                );
            } else if (time % minor_step).abs() < 0.01 {
                // Minor line - thinner and darker
                scene.stroke(
                    &kurbo::Stroke::new(1.0),
                    kurbo::Affine::IDENTITY,
                    peniko::Color::new([0.3, 0.3, 0.3, 1.0]),
                    None,
                    &line,
                );
            }

            time += 0.5; // Check every 0.5 seconds for grid lines
        }
        scene.pop_layer();
    }

    // Render playhead
    if let Ok(mut scene) = playhead_scene.single_mut() {
        scene.reset();
        scene.push_layer(peniko::Mix::Clip, 1.0, kurbo::Affine::IDENTITY, &clip);

        let time_per_pixel: f64 = timeline.duration / width;
        let playhead_x: f64 = left + (timeline.current_time / time_per_pixel);

        // Draw playhead line
        let playhead_line = kurbo::Line::new((playhead_x, top), (playhead_x, bottom));
        scene.stroke(
            &kurbo::Stroke::new(3.0),
            kurbo::Affine::IDENTITY,
            peniko::Color::new([1.0, 0.2, 0.2, 1.0]), // Red playhead
            None,
            &playhead_line,
        );

        // Draw playhead handle (triangle hanging from the panel top edge)
        let handle_size = 8.0;
        let mut handle_path = kurbo::BezPath::new();
        handle_path.move_to((playhead_x, top + handle_size));
        handle_path.line_to((playhead_x - handle_size, top));
        handle_path.line_to((playhead_x + handle_size, top));
        handle_path.close_path();

        scene.fill(
            peniko::Fill::NonZero,
            kurbo::Affine::IDENTITY,
            peniko::Color::new([1.0, 0.2, 0.2, 1.0]), // Red playhead handle
            None,
            &handle_path,
        );
        scene.pop_layer();
    }
}
