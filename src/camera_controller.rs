//! A freecam-style camera controller plugin.
//!
//! This plugin allows for a free-look and movement style camera control,
//! commonly used in first-person and freecam perspectives.
//!
//! The camera controller responds to mouse and keyboard inputs to provide
//! an intuitive and flexible camera control scheme.
//!
//! # Features
//! - Free-look camera with pitch and yaw control.
//! - Movement in all directions with adjustable speed.
//! - Mouse scroll wheel support to adjust speed.
//! - Cursor grabbing and releasing for seamless camera control.
//!
//! # Controls
//! - **WASD**: Move the camera forward, left, backward, and right.
//! - **Space**: Move the camera up.
//! - **Shift**: Move the camera down.
//! - **Control**: Toggle run mode for faster movement.
//! - **Mouse Movement**: Look around.
//! - **Right Mouse Button**: Grab/Release the cursor.
//! - **G**: Toggle cursor grab mode.
//! - **Scroll Wheel**: Adjust movement speed.
//!
//! # Configuration
//! The camera controller can be configured by modifying the `CameraController`
//! component's fields. This can be done directly or through a custom editor.
//!
//! # Example
//! ```
//! // Spawn a camera with the controller
//! commands.spawn_bundle((
//!     Camera3dBundle::default(),
//!     CameraController::default(),
//! ));
use bevy::{
    input::mouse::{MouseButton, MouseScrollUnit}, // Removed AccumulatedMouseScroll
    prelude::*,
    window::{CursorGrabMode, CursorMoved}, // Added CursorMoved
};
use std::{f32::consts::*, fmt};

// Import your custom accumulator resource for cursor delta from bevy_app
use crate::bevy_app::AccumulatedScroll; // Removed AccumulatedCursorDelta

const RADIANS_PER_DOT: f32 = 1.0 / 180.0;

/// A component for controlling a camera with free-look and movement.
#[derive(Component, Debug, Clone, Copy)]
pub struct CameraController {
    pub enabled: bool,
    pub initialized: bool,
    pub sensitivity: f32,
    pub walk_speed: f32,
    pub run_speed: f32,
    pub friction: f32,
    pub pitch: f32,
    pub yaw: f32,
    pub velocity: Vec3,
    pub scroll_factor: f32,
    pub key_forward: KeyCode,
    pub key_back: KeyCode,
    pub key_left: KeyCode,
    pub key_right: KeyCode,
    pub key_up: KeyCode,
    pub key_down: KeyCode,
    pub key_run: KeyCode,
    pub mouse_key_cursor_grab: MouseButton,
    pub keyboard_key_toggle_cursor_grab: KeyCode,
}

impl Default for CameraController {
    fn default() -> Self {
        Self {
            enabled: true,
            initialized: false,
            sensitivity: 1.0,
            walk_speed: 5.0,
            run_speed: 15.0,
            friction: 0.5,
            pitch: 0.0,
            yaw: 0.0,
            velocity: Vec3::ZERO,
            scroll_factor: 0.1,
            key_forward: KeyCode::KeyW,
            key_back: KeyCode::KeyS,
            key_left: KeyCode::KeyA,
            key_right: KeyCode::KeyD,
            key_up: KeyCode::Space,
            key_down: KeyCode::KeyX,
            key_run: KeyCode::ShiftLeft,
            mouse_key_cursor_grab: MouseButton::Right,
            keyboard_key_toggle_cursor_grab: KeyCode::KeyF,
        }
    }
}

impl fmt::Display for CameraController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Sensitivity: {}\nWalk Speed: {}\nRun Speed: {}\nFriction: {}",
            self.sensitivity, self.walk_speed, self.run_speed, self.friction
        )
    }
}

/// A freecam-style camera controller plugin.
#[derive(Default)]
pub struct CameraControllerPlugin;

impl Plugin for CameraControllerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, run_camera_controller);
    }
}

fn run_camera_controller(
    time: Res<Time>,
    mut windows: Query<&mut Window>,
    mut cursor_moved_events: EventReader<CursorMoved>, // Added
    accumulated_scroll: Res<AccumulatedScroll>,
    mouse_button_input: Res<ButtonInput<MouseButton>>,
    key_input: Res<ButtonInput<KeyCode>>,
    mut toggle_cursor_grab: Local<bool>,
    mut mouse_cursor_grab: Local<bool>,
    mut last_mouse_position: Local<Option<Vec2>>, // Added to track mouse delta
    mut query: Query<(&mut Transform, &mut CameraController), With<Camera>>,
) {
    let dt = time.delta_secs();

    let Ok((mut transform, mut controller)) = query.single_mut() else {
        return;
    };

    if !controller.initialized {
        let (yaw, pitch, _roll) = transform.rotation.to_euler(EulerRot::YXZ);
        controller.yaw = yaw;
        controller.pitch = pitch;
        controller.initialized = true;
        info!("{}", *controller);
    }
    if !controller.enabled {
        return;
    }

    let mut scroll_input_amount = 0.0;
    // Use AccumulatedScroll directly
    if accumulated_scroll.delta.y.abs() > 0.0 {
        scroll_input_amount = match accumulated_scroll.unit {
            MouseScrollUnit::Line => accumulated_scroll.delta.y,
            MouseScrollUnit::Pixel => accumulated_scroll.delta.y / 16.0, // Adjust divisor as needed
        };
    }

    if scroll_input_amount.abs() > 0.0 {
        let current_speed = if key_input.pressed(controller.key_run) {
            controller.run_speed
        } else {
            controller.walk_speed
        };
        let new_speed =
            current_speed + scroll_input_amount * controller.scroll_factor * current_speed;
        if key_input.pressed(controller.key_run) {
            controller.run_speed = new_speed.max(0.1); // Ensure speed doesn't go to zero or negative
            controller.walk_speed = controller.run_speed / 3.0;
        } else {
            controller.walk_speed = new_speed.max(0.1); // Ensure speed doesn't go to zero or negative
            controller.run_speed = controller.walk_speed * 3.0;
        }
    }

    // Handle key input
    // This section relies on `ButtonInput<KeyCode>` being populated.
    // Your FFI needs to send `KeyboardInput` events that Bevy's `keyboard_input_system`
    // can process into `ButtonInput<KeyCode>`.
    // Your current web_ffi.rs key_down/key_up updates ActiveInfo, not Bevy events.
    let mut axis_input = Vec3::ZERO;
    if key_input.pressed(controller.key_forward) {
        axis_input.z += 1.0;
    }
    if key_input.pressed(controller.key_back) {
        axis_input.z -= 1.0;
    }
    if key_input.pressed(controller.key_right) {
        axis_input.x += 1.0;
    }
    if key_input.pressed(controller.key_left) {
        axis_input.x -= 1.0;
    }
    if key_input.pressed(controller.key_up) {
        axis_input.y += 1.0;
    }
    if key_input.pressed(controller.key_down) {
        axis_input.y -= 1.0;
    }

    let mut cursor_grab_change = false;
    let prev_cursor_grab = *mouse_cursor_grab || *toggle_cursor_grab;

    // This section relies on `ButtonInput<KeyCode>` and `ButtonInput<MouseButton>`.
    // Your FFI needs to send `KeyboardInput` and `MouseButtonInput` events.
    // Your current web_ffi.rs left_bt_down/up updates ActiveInfo, not Bevy events.
    if key_input.just_pressed(controller.keyboard_key_toggle_cursor_grab) {
        *toggle_cursor_grab = !*toggle_cursor_grab;
        cursor_grab_change = true;
    }
    if mouse_button_input.just_pressed(controller.mouse_key_cursor_grab) {
        *mouse_cursor_grab = true;
        cursor_grab_change = true;
    }
    if mouse_button_input.just_released(controller.mouse_key_cursor_grab) {
        *mouse_cursor_grab = false;
        cursor_grab_change = true;
    }
    let cursor_grab = *mouse_cursor_grab || *toggle_cursor_grab;

    if cursor_grab_change && cursor_grab && !prev_cursor_grab {
        // Just grabbed the cursor, invalidate last_mouse_position to prevent jump
        *last_mouse_position = None;
    }

    // Apply movement update
    if axis_input != Vec3::ZERO {
        let max_speed = if key_input.pressed(controller.key_run) {
            controller.run_speed
        } else {
            controller.walk_speed
        };
        controller.velocity = axis_input.normalize() * max_speed;
    } else {
        let friction = controller.friction.clamp(0.0, 1.0);
        controller.velocity *= 1.0 - friction;
        if controller.velocity.length_squared() < 1e-6 {
            controller.velocity = Vec3::ZERO;
        }
    }
    let forward = *transform.forward();
    let right = *transform.right();
    transform.translation += controller.velocity.x * dt * right
        + controller.velocity.y * dt * Vec3::Y
        + controller.velocity.z * dt * forward;

    // Handle cursor grab
    // Note: Directly manipulating window.cursor_options might need to be
    // handled via JavaScript calls in a WASM/FFI context if this doesn't work as expected.
    if cursor_grab_change {
        if cursor_grab {
            for mut window in &mut windows {
                if !window.focused {
                    continue;
                }

                window.cursor_options.grab_mode = CursorGrabMode::Locked;
                window.cursor_options.visible = false;
            }
        } else {
            for mut window in &mut windows {
                window.cursor_options.grab_mode = CursorGrabMode::None;
                window.cursor_options.visible = true;
            }
        }
    }

    // Handle mouse input for rotation
    let mut mouse_movement_delta = Vec2::ZERO;
    if cursor_grab {
        for event in cursor_moved_events.read() {
            if let Some(last_pos) = *last_mouse_position {
                mouse_movement_delta += event.position - last_pos;
            }
            *last_mouse_position = Some(event.position);
        }
    } else {
        *last_mouse_position = None; // Clear last position if not grabbed
        cursor_moved_events.clear(); // Consume events if not grabbed to prevent buildup
    }

    if mouse_movement_delta != Vec2::ZERO && cursor_grab {
        // Apply look update
        controller.pitch = (controller.pitch
            - mouse_movement_delta.y * RADIANS_PER_DOT * controller.sensitivity)
            .clamp(-PI / 2., PI / 2.);
        controller.yaw -= mouse_movement_delta.x * RADIANS_PER_DOT * controller.sensitivity;
        transform.rotation = Quat::from_euler(EulerRot::ZYX, 0.0, controller.yaw, controller.pitch);
    }
}
