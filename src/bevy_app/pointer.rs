use bevy::input::mouse::MouseButtonInput;
use bevy::prelude::*;

use crate::bevy_app::AccumulatedCursorDelta;

// Collect pointer state from input events and accumulated deltas.
pub fn pointer_collect_system(
    mut cursor_events: EventReader<CursorMoved>,
    mut button_events: EventReader<MouseButtonInput>,
    keys: Res<ButtonInput<KeyCode>>,
    accumulated: Res<AccumulatedCursorDelta>,
    mut pointer: ResMut<crate::PointerState>,
) {
    // Update position from the last cursor event this frame (if any)
    if let Some(last) = cursor_events.read().last() {
        // reads & drains for this system only
        pointer.screen = last.position;
    }

    // Apply accumulated delta (already zeroed if no movement this frame)
    pointer.delta = accumulated.delta;

    // Track previous for just_* flags
    let prev_left = pointer.buttons.left;

    // Process button events for edge detection
    for ev in button_events.read() {
        // independent reader
        match ev.button {
            MouseButton::Left => pointer.buttons.left = ev.state.is_pressed(),
            MouseButton::Right => pointer.buttons.right = ev.state.is_pressed(),
            MouseButton::Middle => pointer.buttons.middle = ev.state.is_pressed(),
            _ => {}
        }
    }

    pointer.just_pressed_left = !prev_left && pointer.buttons.left;
    pointer.just_released_left = prev_left && !pointer.buttons.left;

    // Modifiers (simple logical OR of left/right variants)
    use KeyCode::*;
    pointer.modifiers.shift = keys.pressed(ShiftLeft) || keys.pressed(ShiftRight);
    pointer.modifiers.ctrl = keys.pressed(ControlLeft) || keys.pressed(ControlRight);
    pointer.modifiers.alt = keys.pressed(AltLeft) || keys.pressed(AltRight);
    pointer.modifiers.meta = keys.pressed(SuperLeft) || keys.pressed(SuperRight);
}
