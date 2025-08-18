use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;

#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct AccumulatedCursorDelta {
    pub delta: Vec2,
    last_position: Option<Vec2>,
}

#[derive(Resource, Debug, Clone, Copy)]
pub struct AccumulatedScroll {
    pub delta: Vec2,
    pub unit: MouseScrollUnit,
}
impl Default for AccumulatedScroll {
    fn default() -> Self {
        Self {
            delta: Vec2::ZERO,
            unit: MouseScrollUnit::Line,
        }
    }
}

pub(crate) fn accumulate_cursor_delta_system(
    mut cursor_moved_events: EventReader<CursorMoved>,
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
    mut scroll_events: EventReader<MouseWheel>,
    mut accumulated_scroll: ResMut<AccumulatedScroll>,
) {
    accumulated_scroll.delta = Vec2::ZERO;
    for event in scroll_events.read() {
        accumulated_scroll.delta += Vec2::new(event.x, event.y);
        accumulated_scroll.unit = event.unit;
    }
}
