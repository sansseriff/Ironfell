use bevy::prelude::*;
use kurbo;
use peniko;

use crate::document_bridge::{DocumentStore, sync_document};
use crate::panels::{Panels, TIMELINE_PANEL};
use crate::vector::{DisplayList, DisplayListRebuild, VectorLayer, order};
use iron_document::component::Timing;
use iron_document::TypeId;

/// Timeline plugin: draws the timeline into its panel rect (screen space, clipped).
/// No dedicated camera/window — the shared full-window vello camera presents it.
pub struct TimelinePlugin;

impl Plugin for TimelinePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TimelineState>()
            .add_systems(Startup, setup_timeline_scenes)
            // The playhead is a runtime source of the store: pushed before the
            // document sync so clip samples land in the same frame.
            .add_systems(PreUpdate, (scrub_timeline, push_time_to_store.after(scrub_timeline)).before(sync_document))
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

/// Clip bars, drawn from the document's `clip` nodes.
#[derive(Component)]
pub struct TimelineClipsLayer;

/// Height of one clip row and the gap above the first, in pixels.
const ROW_HEIGHT: f64 = 22.0;
const ROW_TOP: f64 = 14.0;

/// Pointer down or drag inside the timeline panel moves the playhead.
pub fn scrub_timeline(
    pointer: Res<crate::PointerState>,
    panels: Res<Panels>,
    mut timeline: ResMut<TimelineState>,
) {
    if !pointer.buttons.left {
        return;
    }
    let Some(rect) = panels.rect(TIMELINE_PANEL) else { return };
    if !rect.contains(pointer.screen) {
        return;
    }
    let t = ((pointer.screen.x - rect.x) as f64 / rect.w as f64 * timeline.duration).clamp(0.0, timeline.duration);
    if (timeline.current_time - t).abs() > 1e-6 {
        timeline.current_time = t;
        timeline.playing = false;
    }
}

/// Mirror the panel's playhead into the store, which resamples every clip.
pub fn push_time_to_store(timeline: Res<TimelineState>, mut store: ResMut<DocumentStore>) {
    if (store.0.time() - timeline.current_time).abs() > 1e-9 {
        store.0.set_time(timeline.current_time);
    }
}

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
        VectorLayer::screen(order::TIMELINE_CLIPS),
        TimelineClipsLayer,
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
            Without<TimelineClipsLayer>,
            Without<TimelinePlayheadLayer>,
        ),
    >,
    mut grid: Query<&mut DisplayList, (With<TimelineGridLayer>, Without<TimelinePlayheadLayer>, Without<TimelineClipsLayer>)>,
    mut clips: Query<&mut DisplayList, (With<TimelineClipsLayer>, Without<TimelineGridLayer>, Without<TimelinePlayheadLayer>)>,
    mut playhead: Query<&mut DisplayList, (With<TimelinePlayheadLayer>, Without<TimelineGridLayer>, Without<TimelineClipsLayer>)>,
    timeline: Res<TimelineState>,
    panels: Res<Panels>,
    store: Res<DocumentStore>,
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
        for mut list in clips.iter_mut() {
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

    // One row per clip node, in document order: a bar from start to
    // start + dur. Rows are the timeline's own layout, not document data.
    if let Ok(mut list) = clips.single_mut() {
        let doc = store.0.document();
        let clip_ty = TypeId::lookup("clip").expect("registered");
        let rows: Vec<(f64, f64, bool)> = doc
            .nodes_of_type(clip_ty)
            .filter_map(|id| {
                let t = doc.get::<Timing>(id)?;
                let num = |s: &iron_document::Slot, d: f64| s.constant().and_then(|v| v.as_f64()).unwrap_or(d);
                Some((num(&t.start, 0.0), num(&t.dur, 0.0), store.0.time_errors().contains_key(&id)))
            })
            .collect();
        list.rebuild(|b| {
            b.clipped(kurbo::Affine::IDENTITY, clip, |b| {
                for (i, (start, dur, error)) in rows.iter().enumerate() {
                    let y = top + ROW_TOP + i as f64 * ROW_HEIGHT;
                    let x0 = left + start / time_per_pixel;
                    let x1 = left + (start + dur) / time_per_pixel;
                    let bar = kurbo::RoundedRect::new(x0, y, x1.max(x0 + 4.0), y + ROW_HEIGHT - 6.0, 4.0);
                    let color = if *error { [0.85, 0.3, 0.3, 1.0] } else { [0.35, 0.6, 0.9, 1.0] };
                    b.fill(kurbo::Affine::IDENTITY, peniko::Color::new(color), bar);
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
