use bevy::input::mouse::MouseButtonInput; // added for button event reader
use bevy::prelude::*;
use bevy::render::view::RenderLayers;
use bevy_vello::prelude::*;
use std::ops::DerefMut;

// -------------------------------------------------------------------------------------------------
// Overlay 2D camera + animated demo scene (existing behavior)
// -------------------------------------------------------------------------------------------------

#[derive(Component)]
pub(crate) struct OverlayCamera2D;

// -------------------------------------------------------------------------------------------------
// Draggable square state + marker scene
// -------------------------------------------------------------------------------------------------

#[derive(Resource, Debug)]
pub(crate) struct DraggableSquare {
    pub position: Vec2, // Center position in overlay world space
    pub size: Vec2,     // Width / height
    pub dragging: bool,
    pub hovered: bool,
    drag_offset: Vec2, // Cursor offset captured at drag start
}

impl Default for DraggableSquare {
    fn default() -> Self {
        Self {
            position: Vec2::ZERO,
            size: Vec2::splat(120.0),
            dragging: false,
            hovered: false,
            drag_offset: Vec2::ZERO,
        }
    }
}

#[derive(Component)]
pub(crate) struct DraggableOverlayScene; // Separate Vello scene so it isn't affected by the animated transform

#[derive(Resource, Default, Debug)]
pub(crate) struct SimpleMouseState {
    pub left_pressed: bool,
}

pub(crate) fn setup_2d_overlay(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        RenderLayers::layer(1),
        OverlayCamera2D,
        VelloView,
    ));
    // Animated demo scene (kept from previous implementation)
    commands.spawn((VelloScene::new(), RenderLayers::layer(1)));

    // Static scene for draggable square (unaffected by animated transform changes)
    commands.spawn((
        VelloScene::new(),
        DraggableOverlayScene,
        RenderLayers::layer(1),
    ));
}

pub(crate) fn animate_2d_overlay(
    mut query_scene: Single<(&mut Transform, &mut VelloScene)>,
    time: Res<Time>,
) {
    let sin_time = time.elapsed_secs().sin().mul_add(0.5, 0.5);
    let (transform, scene) = query_scene.deref_mut();

    scene.reset();

    let c = Vec3::lerp(
        Vec3::new(-1.0, 1.0, -1.0),
        Vec3::new(-1.0, 1.0, 1.0),
        sin_time + 0.5,
    );

    scene.fill(
        peniko::Fill::NonZero,
        kurbo::Affine::default(),
        peniko::Color::new([c.x, c.y, c.z, 1.]),
        None,
        &kurbo::RoundedRect::new(-100.0, -100.0, 100.0, 100.0, (sin_time as f64) * 100.0),
    );

    transform.scale = Vec3::lerp(Vec3::ONE * 0.5, Vec3::ONE * 1.0, sin_time);
    transform.translation = Vec3::lerp(Vec3::Y * -900.0, Vec3::Y * 900.0, sin_time);
    transform.rotation = Quat::from_rotation_z(-std::f32::consts::TAU * sin_time);
}

// -------------------------------------------------------------------------------------------------
// Draggable square logic
// -------------------------------------------------------------------------------------------------

pub(crate) fn update_draggable_square_state(
    mut state: ResMut<DraggableSquare>,
    mut cursor_events: EventReader<CursorMoved>,
    mouse: Res<SimpleMouseState>,
    q_cam: Query<(&Camera, &GlobalTransform), With<OverlayCamera2D>>,
) {
    // Follow the pattern in tracking_circle.rs: only act if we have cursor movement events this frame.
    if cursor_events.is_empty() {
        // Still need to handle drag end even without movement.
        if state.dragging && !mouse.left_pressed {
            state.dragging = false;
        }
        return;
    }
    let last_opt = cursor_events.read().last().map(|e| e.position);
    let Some(last_pos) = last_opt else {
        return;
    };
    let (camera, cam_transform) = match q_cam.single() {
        Ok(v) => v,
        Err(_) => return,
    };
    let Ok(world_pos) = camera.viewport_to_world_2d(cam_transform, last_pos) else {
        return;
    };

    // Hover test (AABB of the square)
    let half = state.size * 0.5;
    state.hovered = (world_pos.x >= state.position.x - half.x)
        && (world_pos.x <= state.position.x + half.x)
        && (world_pos.y >= state.position.y - half.y)
        && (world_pos.y <= state.position.y + half.y);

    // Drag start
    if !state.dragging && state.hovered && mouse.left_pressed {
        state.dragging = true;
        state.drag_offset = world_pos - state.position;
    }

    // Drag end
    if state.dragging && !mouse.left_pressed {
        state.dragging = false;
    }

    // Drag move
    if state.dragging && mouse.left_pressed {
        state.position = world_pos - state.drag_offset;
    }
}

pub(crate) fn render_draggable_square(
    mut scenes: Query<&mut VelloScene, With<DraggableOverlayScene>>,
    state: Res<DraggableSquare>,
) {
    let Ok(mut scene) = scenes.single_mut() else {
        return;
    };
    scene.reset();

    // Choose color based on state
    // Dragging: red, Hover: pink, Idle: dark gray
    let (r, g, b) = if state.dragging {
        (1.0, 0.0, 0.0) // red
    } else if state.hovered {
        (1.0, 0.4, 0.7) // pink-ish
    } else {
        (0.2, 0.2, 0.2) // dark gray
    };

    let half = state.size * 0.5;
    let rect = kurbo::Rect::new(
        (state.position.x - half.x) as f64,
        (state.position.y - half.y) as f64,
        (state.position.x + half.x) as f64,
        (state.position.y + half.y) as f64,
    );

    scene.fill(
        peniko::Fill::NonZero,
        kurbo::Affine::default(),
        peniko::Color::new([r, g, b, 1.0]),
        None,
        &rect,
    );
}

// -------------------------------------------------------------------------------------------------
// Notes:
// We leverage Bevy's built-in Input<MouseButton> resource as a lightweight "state machine" for
// mouse buttons normally, but in this environment (custom event injection without winit) we
// instead maintain a minimal `SimpleMouseState` from `MouseButtonInput` events. This reproduces the
// tracking approach used in `tracking_circle.rs`, reacting only to the latest cursor position.
// Cursor position comes from CursorMoved events and is converted to overlay world space via the
// overlay camera (similar to the tracking circle implementation). This pattern is typical in Bevy
// apps: input state is queried each frame rather than building an explicit FSM, unless more complex
// gesture / multi-button / modal behavior is required.
// -------------------------------------------------------------------------------------------------

pub(crate) fn simple_mouse_state_system(
    mut events: EventReader<MouseButtonInput>,
    mut mouse: ResMut<SimpleMouseState>,
) {
    for ev in events.read() {
        if ev.button == MouseButton::Left {
            mouse.left_pressed = ev.state.is_pressed();
        }
    }
}
