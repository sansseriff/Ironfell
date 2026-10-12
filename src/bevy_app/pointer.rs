use bevy::input::mouse::MouseButtonInput;
use bevy::prelude::*;

use crate::bevy_app::AccumulatedCursorDelta;

// Collect pointer state from input events and accumulated deltas.
pub fn pointer_collect_system(
    mut cursor_events: MessageReader<CursorMoved>,
    mut button_events: MessageReader<MouseButtonInput>,
    keys: Res<ButtonInput<KeyCode>>,
    accumulated: Res<AccumulatedCursorDelta>,
    shell: Res<crate::bevy_app::select::ShellModifiers>,
    mut pointer: ResMut<crate::PointerState>,
) {
    // Update position from the last cursor event this frame (if any)
    if let Some(last) = cursor_events.read().last() {
        // reads & drains for this system only
        pointer.screen = last.position;
    }

    // Apply accumulated delta (already zeroed if no movement this frame)
    pointer.delta = accumulated.delta;

    // Edges come from the events, not from comparing state at frame
    // boundaries: a slow frame (a shader compiling) can hold a whole click,
    // press and release, and that must still be a click.
    pointer.just_pressed_left = false;
    pointer.just_released_left = false;
    for ev in button_events.read() {
        // independent reader
        match ev.button {
            MouseButton::Left => {
                let down = ev.state.is_pressed();
                if down && !pointer.buttons.left {
                    pointer.just_pressed_left = true;
                }
                if !down && (pointer.buttons.left || pointer.just_pressed_left) {
                    pointer.just_released_left = true;
                }
                pointer.buttons.left = down;
            }
            MouseButton::Right => pointer.buttons.right = ev.state.is_pressed(),
            MouseButton::Middle => pointer.buttons.middle = ev.state.is_pressed(),
            _ => {}
        }
    }

    // Modifiers: keys Bevy saw, or what the shell reports (it alone sees
    // Alt and Cmd).
    use crate::bevy_app::select::{MOD_ALT, MOD_CTRL, MOD_META, MOD_SHIFT};
    use KeyCode::*;
    let m = shell.0;
    pointer.modifiers.shift = keys.pressed(ShiftLeft) || keys.pressed(ShiftRight) || m & MOD_SHIFT != 0;
    pointer.modifiers.ctrl = keys.pressed(ControlLeft) || keys.pressed(ControlRight) || m & MOD_CTRL != 0;
    pointer.modifiers.alt = keys.pressed(AltLeft) || keys.pressed(AltRight) || m & MOD_ALT != 0;
    pointer.modifiers.meta = keys.pressed(SuperLeft) || keys.pressed(SuperRight) || m & MOD_META != 0;
}
