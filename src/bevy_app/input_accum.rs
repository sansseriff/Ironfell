use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;

#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct AccumulatedCursorDelta {
    pub delta: Vec2,
    last_position: Option<Vec2>,
}

/// Log zoom per window px of wheel travel, and the most one wheel event may
/// zoom: a wheel click (about 10 px) is ~5%, and the bound
/// keeps an accelerated burst of clicks (100 px or more each on macOS)
/// to ~6.7% apiece. Raise ZOOM_STEP to speed up fast spins; ZOOM_PER_PX
/// tunes the small-delta response, including trackpad pinch.
const ZOOM_PER_PX: f32 = 0.005;
const ZOOM_STEP: f32 = 0.065;

#[derive(Resource, Debug, Clone, Copy)]
pub struct AccumulatedScroll {
    pub delta: Vec2,
    pub unit: MouseScrollUnit,
    /// This frame's wheel as log zoom, each event bounded (`ZOOM_STEP`);
    /// positive zooms out.
    pub zoom: f32,
}
impl Default for AccumulatedScroll {
    fn default() -> Self {
        Self {
            delta: Vec2::ZERO,
            unit: MouseScrollUnit::Line,
            zoom: 0.0,
        }
    }
}

pub(crate) fn accumulate_cursor_delta_system(
    mut cursor_moved_events: MessageReader<CursorMoved>,
    mut accumulated_delta: ResMut<AccumulatedCursorDelta>,
) {
    accumulated_delta.delta = Vec2::ZERO;
    for event in cursor_moved_events.read() {
        if let Some(last_pos) = accumulated_delta.last_position {
            let current_delta = event.position - last_pos;
            accumulated_delta.delta += current_delta;
        }
        accumulated_delta.last_position = Some(event.position);
    }
}

pub(crate) fn accumulate_custom_scroll_system(
    mut scroll_events: MessageReader<MouseWheel>,
    mut accumulated_scroll: ResMut<AccumulatedScroll>,
) {
    accumulated_scroll.delta = Vec2::ZERO;
    accumulated_scroll.zoom = 0.0;
    for event in scroll_events.read() {
        accumulated_scroll.delta += Vec2::new(event.x, event.y);
        accumulated_scroll.unit = event.unit;
        let px = match event.unit {
            MouseScrollUnit::Line => event.y * 16.0,
            MouseScrollUnit::Pixel => event.y,
        };
        accumulated_scroll.zoom += (px * ZOOM_PER_PX).clamp(-ZOOM_STEP, ZOOM_STEP);
    }
}
