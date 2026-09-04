use bevy::prelude::*;
use kurbo;
use peniko;

use crate::panels::{Panels, TIMELINE_PANEL};
use crate::vector::{DisplayList, DisplayListRebuild, VectorLayer, order};

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

/// Marker component for the timeline background layer
#[derive(Component)]
pub struct TimelineBackgroundLayer;

/// Marker component for the timeline grid layer
#[derive(Component)]
pub struct TimelineGridLayer;

/// Marker component for the timeline playhead layer
#[derive(Component)]
pub struct TimelinePlayheadLayer;

fn setup_timeline_scenes(mut commands: Commands) {
    commands.spawn((
        DisplayList::default(),
        VectorLayer::screen(order::TIMELINE_BACKGROUND),
        TimelineBackgroundLayer,
    ));
    commands.spawn((
        DisplayList::default(),
        VectorLayer::screen(order::TIMELINE_GRID),
        TimelineGridLayer,
    ));
    commands.spawn((
        DisplayList::default(),
        VectorLayer::screen(order::TIMELINE_PLAYHEAD),
        TimelinePlayheadLayer,
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
    mut bg: Query<
        &mut DisplayList,
        (
            With<TimelineBackgroundLayer>,
            Without<TimelineGridLayer>,
            Without<TimelinePlayheadLayer>,
        ),
    >,
    mut grid: Query<&mut DisplayList, (With<TimelineGridLayer>, Without<TimelinePlayheadLayer>)>,
    mut playhead: Query<&mut DisplayList, (With<TimelinePlayheadLayer>, Without<TimelineGridLayer>)>,
    timeline: Res<TimelineState>,
    panels: Res<Panels>,
) {
    let rect = panels.rect(TIMELINE_PANEL);

    // Background (replaces the old timeline camera's clear color)
    if let Ok(mut list) = bg.single_mut() {
        list.rebuild(|b| {
            if let Some(rect) = rect {
                b.fill(
                    kurbo::Affine::IDENTITY,
                    peniko::Color::new([0.145, 0.145, 0.152, 1.0]),
                    rect.to_kurbo(),
                );
            }
        });
    }

    // With no timeline panel there is nothing to draw; emitting empty lists keeps
    // the layers consistent rather than leaving stale content on screen.
    let Some(rect) = rect else {
        for mut list in grid.iter_mut() {
            list.rebuild(|_| {});
        }
        for mut list in playhead.iter_mut() {
            list.rebuild(|_| {});
        }
        return;
    };

    let clip = rect.to_kurbo();
    let left = rect.x as f64;
    let top = rect.y as f64;
    let bottom = (rect.y + rect.h) as f64;
    let width = rect.w as f64;
    let time_per_pixel: f64 = timeline.duration / width;

    if let Ok(mut list) = grid.single_mut() {
        list.rebuild(|b| {
            b.clipped(kurbo::Affine::IDENTITY, clip, |b| {
                const MAJOR_STEP: f64 = 5.0;
                const MINOR_STEP: f64 = 1.0;

                let mut time: f64 = 0.0;
                while time <= timeline.duration {
                    let x: f64 = left + (time / time_per_pixel);
                    let line = kurbo::Line::new((x, top), (x, bottom));

                    if (time % MAJOR_STEP).abs() < 0.01 {
                        b.stroke(
                            kurbo::Affine::IDENTITY,
                            kurbo::Stroke::new(2.0),
                            peniko::Color::new([0.5, 0.5, 0.5, 1.0]),
                            line,
                        );
                    } else if (time % MINOR_STEP).abs() < 0.01 {
                        b.stroke(
                            kurbo::Affine::IDENTITY,
                            kurbo::Stroke::new(1.0),
                            peniko::Color::new([0.3, 0.3, 0.3, 1.0]),
                            line,
                        );
                    }

                    time += 0.5;
                }
            });
        });
    }

    if let Ok(mut list) = playhead.single_mut() {
        list.rebuild(|b| {
            b.clipped(kurbo::Affine::IDENTITY, clip, |b| {
                let playhead_x: f64 = left + (timeline.current_time / time_per_pixel);
                b.stroke(
                    kurbo::Affine::IDENTITY,
                    kurbo::Stroke::new(3.0),
                    peniko::Color::new([1.0, 0.2, 0.2, 1.0]),
                    kurbo::Line::new((playhead_x, top), (playhead_x, bottom)),
                );

                // Handle: a triangle hanging from the panel's top edge.
                let handle_size = 8.0;
                let mut handle = kurbo::BezPath::new();
                handle.move_to((playhead_x, top + handle_size));
                handle.line_to((playhead_x - handle_size, top));
                handle.line_to((playhead_x + handle_size, top));
                handle.close_path();

                b.fill(
                    kurbo::Affine::IDENTITY,
                    peniko::Color::new([1.0, 0.2, 0.2, 1.0]),
                    handle,
                );
            });
        });
    }
}
