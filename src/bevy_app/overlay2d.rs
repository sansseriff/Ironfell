use bevy::input::mouse::MouseButtonInput; // added for button event reader
use bevy::prelude::*;
use bevy::render::view::RenderLayers;
use bevy_vello::prelude::*;
// Bring kurbo trait methods into scope for PathSeg operations (arclen, inv_arclen, etc.)
use bevy_vello::prelude::kurbo::{ParamCurve, ParamCurveArclen};

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

#[derive(Component)]
pub(crate) struct AnimatedOverlayScene; // Marker for animated overlay scene (needs Transform)

#[derive(Component)]
pub(crate) struct AnimatedBezierStrokeScene; // Marker for animated bezier stroke scene

#[derive(Resource)]
pub(crate) struct AnimatedBezierPath {
    pub path: kurbo::BezPath,
    pub seg_lengths: Vec<f64>,
    pub total_length: f64,
    pub stroke_width: f32,
}

impl AnimatedBezierPath {
    fn generate() -> Self {
        let mut seed: u32 = 0xA1B2B3D4; // xorshift32 deterministic
        fn next(seed: &mut u32) -> f32 {
            *seed ^= *seed << 13;
            *seed ^= *seed >> 17;
            *seed ^= *seed << 5;
            (*seed as f32) / (u32::MAX as f32)
        }
        let mut path = kurbo::BezPath::new();
        let start_y = (next(&mut seed) - 0.5) * 600.0; // -300..300
        path.move_to((-480.0, start_y as f64));
        let mut x = -480.0f64;
        while x < 480.0 {
            let w = 160.0; // width per cubic
            let end_x = (x + w).min(480.0);
            let y0 = (next(&mut seed) - 0.5) * 600.0;
            let y1 = (next(&mut seed) - 0.5) * 600.0;
            let y2 = (next(&mut seed) - 0.5) * 600.0;
            path.curve_to(
                (x + w * 0.33, y0 as f64),
                (x + w * 0.66, y1 as f64),
                (end_x, y2 as f64),
            );
            x += w;
        }
        // Precompute lengths
        let mut seg_lengths = Vec::new();
        let mut total = 0.0;
        for seg in path.segments() {
            let len = seg.arclen(0.5); // moderate accuracy
            seg_lengths.push(len);
            total += len;
        }

        info!("number of segments: {}", seg_lengths.len());
        Self {
            path,
            seg_lengths,
            total_length: total,
            stroke_width: 25.0,
        }
    }
}

#[derive(Resource, Default, Debug)]
pub(crate) struct SimpleMouseState {
    pub left_pressed: bool,
}

pub(crate) fn setup_2d_overlay(
    mut commands: Commands,
    existing_bezier: Option<Res<AnimatedBezierPath>>,
) {
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            // clear_color: ClearColorConfig::None,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.97, 0.97, 0.97)),
            ..default()
        },
        RenderLayers::layer(1),
        OverlayCamera2D,
        VelloView,
    ));
    // Animated demo scene (kept from previous implementation)
    commands.spawn((
        VelloScene::new(),
        Transform::default(),
        GlobalTransform::default(),
        RenderLayers::layer(1),
        AnimatedOverlayScene,
    ));

    // Static scene for draggable square (unaffected by animated transform changes)
    commands.spawn((
        VelloScene::new(),
        DraggableOverlayScene,
        RenderLayers::layer(1),
    ));

    // Animated bezier stroke scene
    if existing_bezier.is_none() {
        commands.insert_resource(AnimatedBezierPath::generate());
    }
    commands.spawn((
        VelloScene::new(),
        RenderLayers::layer(1),
        AnimatedBezierStrokeScene,
    ));
}

pub(crate) fn animate_2d_overlay(
    mut query_scene: Query<(&mut Transform, &mut VelloScene), With<AnimatedOverlayScene>>,
    mut bezier_scene: Query<
        &mut VelloScene,
        (
            With<AnimatedBezierStrokeScene>,
            Without<AnimatedOverlayScene>,
        ),
    >,
    bezier: Option<Res<AnimatedBezierPath>>,
    time: Res<Time>,
) {
    let Ok((mut transform, mut scene)) = query_scene.single_mut() else {
        return;
    }; // not ready yet
    let sin_time = time.elapsed_secs().sin().mul_add(0.5, 0.5);
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

    // Animate progressive bezier stroke reveal
    if let (Ok(mut scene_stroke), Some(bezier)) = (bezier_scene.single_mut(), bezier) {
        scene_stroke.reset();
        let progress = (time.elapsed_secs() / 6.0).fract().clamp(0.0, 1.0);
        let target_len = bezier.total_length * (progress as f64);
        if target_len <= 0.0 {
            return;
        }
        if (target_len - bezier.total_length).abs() < f64::EPSILON {
            let stroke_style = kurbo::Stroke::new(bezier.stroke_width as f64);
            scene_stroke.stroke(
                &stroke_style,
                kurbo::Affine::default(),
                peniko::Color::new([0.0, 0.6, 1.0, 1.0]),
                None,
                &bezier.path,
            );
            return;
        }
        let mut partial = kurbo::BezPath::new();
        let mut remaining = target_len;
        let mut idx = 0usize;
        let mut segs = bezier.path.segments();
        if let Some(first) = segs.next() {
            let first_start = match first {
                // extract start point manually
                kurbo::PathSeg::Line(l) => l.p0,
                kurbo::PathSeg::Quad(q) => q.p0,
                kurbo::PathSeg::Cubic(c) => c.p0,
            };
            partial.move_to(first_start);
            let take_seg = |seg: kurbo::PathSeg,
                            partial: &mut kurbo::BezPath,
                            remaining: &mut f64,
                            idx: usize| {
                let seg_len = bezier.seg_lengths[idx];
                if *remaining >= seg_len {
                    partial.push(seg.as_path_el());
                    *remaining -= seg_len;
                    true
                } else {
                    let t = seg.inv_arclen(*remaining, 0.5);
                    let sub = seg.subsegment(0.0..t);
                    partial.push(sub.as_path_el());
                    *remaining = 0.0;
                    false
                }
            };
            take_seg(first, &mut partial, &mut remaining, idx);
            idx += 1;
            for seg in segs {
                if remaining <= 0.0 {
                    break;
                }
                let cont = take_seg(seg, &mut partial, &mut remaining, idx);
                idx += 1;
                if !cont {
                    break;
                }
            }
        }
        let stroke_style = kurbo::Stroke::new(bezier.stroke_width as f64);
        scene_stroke.stroke(
            &stroke_style,
            kurbo::Affine::default(),
            peniko::Color::new([0.0, 0.6, 1.0, 1.0]),
            None,
            &partial,
        );
        if let Some(last) = partial.segments().last() {
            let head = match last {
                kurbo::PathSeg::Line(l) => l.p1,
                kurbo::PathSeg::Quad(q) => q.p2,
                kurbo::PathSeg::Cubic(c) => c.p3,
            };
            scene_stroke.fill(
                peniko::Fill::NonZero,
                kurbo::Affine::default(),
                peniko::Color::new([0.95, 0.2, 0.4, 1.0]),
                None,
                &kurbo::Circle::new(head, (bezier.stroke_width * 0.55) as f64),
            );
        }
    }
}

// -------------------------------------------------------------------------------------------------
// Draggable square logic
// -------------------------------------------------------------------------------------------------

pub(crate) fn update_draggable_square_state(
    mut state: ResMut<DraggableSquare>,
    mut cursor_events: EventReader<CursorMoved>,
    mouse: Res<SimpleMouseState>,
    q_cam: Query<(&Camera, &GlobalTransform), With<OverlayCamera2D>>,
    // Correct Y origin mismatch (browser events usually top-left; Bevy viewport_to_world_2d expects bottom-left)
    q_window: Query<&Window>,
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
    let Some(mut last_pos) = last_opt else {
        return;
    };
    // Correct Y inversion if window events are in a top-left origin space
    if let Ok(window) = q_window.single() {
        // Bevy logical cursor coords use bottom-left origin for viewport_to_world_2d.
        // If our injected events are top-left, flip them.
        last_pos.y = window.height() - last_pos.y;
    }
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
