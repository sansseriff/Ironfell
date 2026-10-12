use bevy::input::mouse::MouseButtonInput;
use bevy::prelude::*;

use crate::bevy_app::AccumulatedCursorDelta;

/// Where the last press happened (physical px), set by the shell with the
/// press; consumed by the frame that sees the press.
#[derive(Resource, Default)]
pub struct PressPosition(pub Option<Vec2>);

// Collect pointer state from input events and accumulated deltas.
pub fn pointer_collect_system(
    mut cursor_events: MessageReader<CursorMoved>,
    mut button_events: MessageReader<MouseButtonInput>,
    keys: Res<ButtonInput<KeyCode>>,
    accumulated: Res<AccumulatedCursorDelta>,
    shell: Res<crate::bevy_app::select::ShellModifiers>,
    mut press: ResMut<PressPosition>,
    mut held_back: Local<Option<Vec2>>,
    mut pointer: ResMut<crate::PointerState>,
) {
    // Update position from the last cursor event this frame (if any); a
    // position held back by last frame's press comes first.
    if let Some(p) = held_back.take() {
        pointer.screen = p;
    }
    let moved_to = cursor_events.read().last().map(|e| e.position);
    if let Some(p) = moved_to {
        pointer.screen = p;
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

    // A press is resolved where it happened: moves that arrived after it in
    // the same frame wait for the next one.
    if pointer.just_pressed_left
        && let Some(p) = press.0.take()
    {
        *held_back = moved_to.filter(|m| *m != p);
        pointer.screen = p;
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
