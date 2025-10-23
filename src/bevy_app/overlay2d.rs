use bevy::input::mouse::MouseButtonInput; // added for button event reader
use bevy::prelude::*;
use bevy::render::{
    camera::RenderTarget,
    view::RenderLayers,
};
use bevy::window::WindowRef;
use crate::canvas_view::CanvasName;
use bevy_vello::prelude::*;
// Bring kurbo trait methods into scope for PathSeg operations (arclen, inv_arclen, etc.)
use bevy_vello::prelude::kurbo::{ParamCurve, ParamCurveArclen};
use bevy_vello::prelude::VelloScreenSpace;
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
            position: Vec2::new(0.0, -200.0),
            size: Vec2::splat(80.0),
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
    pub just_pressed: bool,
    pub just_released: bool,
}



// -------------------------------------------------------------------------------------------------
// Multiple small selectable squares (batched version)
// -------------------------------------------------------------------------------------------------

#[derive(Component)]
pub(crate) struct MiniSquare {
    size: f32,
    base_color: [f32; 3], // precomputed linear components
}

#[derive(Component)]
pub(crate) struct MiniSquareState {
    selected: bool,
    hovered: bool,
    dragging: bool,
    drag_offset: Vec2,
    final_color: [f32; 4], // rgba ready for render
}

impl Default for MiniSquareState {
    fn default() -> Self {
        Self {
            selected: false,
            hovered: false,
            dragging: false,
            drag_offset: Vec2::ZERO,
            final_color: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

#[derive(Component)]
pub(crate) struct MiniSquaresScene; // single batched scene for all mini squares

#[derive(Resource, Default)]
pub(crate) struct MiniSquaresDirty(pub bool);

#[derive(Resource, Default)]
pub(crate) struct SelectionMarquee {
    start: Option<Vec2>,
    current: Option<Vec2>,
}

#[derive(Component)]
pub(crate) struct SelectionMarqueeScene;

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
    // Reset per-frame transition flags
    mouse.just_pressed = false;
    mouse.just_released = false;
    for ev in events.read() {
        if ev.button == MouseButton::Left {
            if ev.state.is_pressed() {
                if !mouse.left_pressed {
                    mouse.just_pressed = true;
                }
                mouse.left_pressed = true;
            } else {
                if mouse.left_pressed {
                    mouse.just_released = true;
                }
                mouse.left_pressed = false;
            }
        }
    }
}

pub(crate) fn setup_2d_overlay(
    mut commands: Commands,
    existing_bezier: Option<Res<AnimatedBezierPath>>,
    windows: Query<(Entity, Option<&CanvasName>), With<Window>>,
) {
    // Find the viewer window deterministically by CanvasName; fall back to Primary if not yet present
    let mut viewer_window: Option<Entity> = None;
    for (e, name) in windows.iter() {
        if let Some(CanvasName(id)) = name {
            if id == "viewer-canvas" {
                viewer_window = Some(e);
                break;
            }
        }
    }
    let Some(_viewer_window) = viewer_window else {
        // Defer until the viewer window exists
        return;
    };

    // Spawn overlay camera immediately (was previously deferred)
    if let Some(viewer_entity) = viewer_window {
        let camera_target = RenderTarget::Window(WindowRef::Entity(viewer_entity));
        commands.spawn((
            Camera2d,
            Camera {
                order: 1,
                clear_color: ClearColorConfig::Custom(Color::srgb(0.97, 0.97, 0.97)),
                target: camera_target,
                ..default()
            },
            RenderLayers::layer(1),
            OverlayCamera2D,
            VelloView,
        ));
    }
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
        VelloScreenSpace,
    ));

    // SPAWN many mini square entities (NO per-entity VelloScene now)
    let mut seed: u32 = 0x91E2_33AB;
    fn next(seed: &mut u32) -> f32 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 17;
        *seed ^= *seed << 5;
        (*seed as f32) / (u32::MAX as f32)
    }
    let mini_size = 120.0 / 4.0;
    for _ in 0..3000 {
        let x = (next(&mut seed) - 0.5) * 1900.0;
        let y = (next(&mut seed) - 0.5) * 1200.0;
        let r = next(&mut seed);
        let g = next(&mut seed);
        let b = next(&mut seed);
        commands.spawn((
            Transform::from_translation(Vec3::new(x, y, 0.0)),
            GlobalTransform::default(),
            RenderLayers::layer(1),
            MiniSquare { size: mini_size, base_color: [r, g, b] },
            MiniSquareState {
                final_color: [r, g, b, 1.0],
                ..Default::default()
            },
        ));
    }

    // Shared batched scene entity for all mini squares
    commands.spawn((
        VelloScene::new(),
        RenderLayers::layer(1),
        MiniSquaresScene,
    ));
    commands.insert_resource(MiniSquaresDirty(true));

    // Marquee scene + resource (unchanged)
    commands.insert_resource(SelectionMarquee::default());
    commands.spawn((
        VelloScene::new(),
        RenderLayers::layer(1),
        SelectionMarqueeScene,
    ));
}

// (Removed) deferred_overlay_camera_spawn: camera now created in setup_2d_overlay

// Diagnostic: log projection extents for overlay and timeline cameras first few frames
pub(crate) fn camera_projection_diagnostics(
    overlay_cam: Query<&Camera, With<OverlayCamera2D>>,
    timeline_cam: Query<&Camera, (With<crate::bevy_app::timeline::TimelineCamera2D>, Without<OverlayCamera2D>)>,
    mut counter: Local<u32>,
) {
    if *counter >= 120 { return; }
    if let Ok(cam) = overlay_cam.single() {
        if let Some(vp) = &cam.viewport { info!("diag overlay viewport phys={}x{}", vp.physical_size.x, vp.physical_size.y); }
    }
    if let Ok(cam) = timeline_cam.single() {
        if let Some(vp) = &cam.viewport { info!("diag timeline viewport phys={}x{}", vp.physical_size.x, vp.physical_size.y); }
    }
    *counter += 1;
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
    // Normalize cursor Y to bottom-left origin by flipping using the viewport height (if present)
    let (camera, cam_transform) = match q_cam.single() {
        Ok(v) => v,
        Err(_) => return,
    };
    if let Some(vp) = &camera.viewport {
        let h = vp.physical_size.y as f32;
        last_pos.y = h - last_pos.y;
    } else if let Some(rect) = camera.logical_viewport_rect() {
        let h = (rect.max.y - rect.min.y) as f32;
        last_pos.y = h - last_pos.y;
    }
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

// -------------------------------------------------------------------------------------------------
// Multi-square update system
// -------------------------------------------------------------------------------------------------
pub(crate) fn update_mini_square_entities(
    mut q_squares: Query<(&mut Transform, &MiniSquare, &mut MiniSquareState)>,
    mut marquee_res: ResMut<SelectionMarquee>,
    mut cursor_events: EventReader<CursorMoved>,
    mouse: Res<SimpleMouseState>,
    q_cam: Query<(&Camera, &GlobalTransform), With<OverlayCamera2D>>,
    mut dirty: ResMut<MiniSquaresDirty>,
) {
    if cursor_events.is_empty() && !mouse.just_pressed && !mouse.just_released {
        if mouse.just_released {
            for (_, _, mut st) in q_squares.iter_mut() {
                st.dragging = false;
                st.drag_offset = Vec2::ZERO;
            }
            marquee_res.start = None;
            marquee_res.current = None;
            dirty.0 = true;
        }
        return;
    }

    // Latest cursor world position
    let mut world_pos_opt = None;
    if let Some(mut screen) = cursor_events.read().last().map(|e| e.position) {
        if let Ok((cam, tf)) = q_cam.get_single() {
            if let Some(vp) = &cam.viewport {
                let h = vp.physical_size.y as f32;
                screen.y = h - screen.y;
            } else if let Some(rect) = cam.logical_viewport_rect() {
                let h = (rect.max.y - rect.min.y) as f32;
                screen.y = h - screen.y;
            }
            if let Ok(wp) = cam.viewport_to_world_2d(tf, screen) {
                world_pos_opt = Some(wp);
            }
        }
    }
    let Some(world_pos) = world_pos_opt else { return; };

    // Pass 1: hover update + detect any hovered & hovered-selected
    let mut any_hovered = false;
    let mut any_hovered_selected = false;
    for (tr, ms, mut st) in q_squares.iter_mut() {
        let center = tr.translation.truncate();
        let half = ms.size * 0.5;
        let new_hovered = world_pos.x >= center.x - half
            && world_pos.x <= center.x + half
            && world_pos.y >= center.y - half
            && world_pos.y <= center.y + half;
        if new_hovered != st.hovered {
            st.hovered = new_hovered;
            dirty.0 = true;
        }
        if st.hovered {
            any_hovered = true;
            if st.selected {
                any_hovered_selected = true;
            }
        }
    }

    // Mouse press handling (selection / drag start / marquee start)
    if mouse.just_pressed {
        if any_hovered {
            if !any_hovered_selected {
                // Replace selection with hovered set
                for (_, _, mut st) in q_squares.iter_mut() {
                    let new_sel = st.hovered;
                    if new_sel != st.selected {
                        st.selected = new_sel;
                        dirty.0 = true;
                    }
                }
            }
            // Start group drag
            for (tr, _, mut st) in q_squares.iter_mut() {
                if st.selected {
                    st.dragging = true;
                    st.drag_offset = tr.translation.truncate() - world_pos;
                } else {
                    st.dragging = false;
                    st.drag_offset = Vec2::ZERO;
                }
            }
            marquee_res.start = None;
            marquee_res.current = None;
        } else {
            // Empty press: clear selection + start marquee
            for (_, _, mut st) in q_squares.iter_mut() {
                if st.selected || st.dragging {
                    st.selected = false;
                    st.dragging = false;
                    st.drag_offset = Vec2::ZERO;
                    dirty.0 = true;
                }
            }
            marquee_res.start = Some(world_pos);
            marquee_res.current = Some(world_pos);
        }
    }

    // Marquee update
    if mouse.left_pressed {
        if let (Some(start), Some(_)) = (marquee_res.start, marquee_res.current) {
            marquee_res.current = Some(world_pos);
            let min = start.min(world_pos);
            let max = start.max(world_pos);
            for (tr, ms, mut st) in q_squares.iter_mut() {
                let center = tr.translation.truncate();
                let half = ms.size * 0.5;
                let a_min = center - Vec2::splat(half);
                let a_max = center + Vec2::splat(half);
                let intersects = !(a_max.x < min.x || a_min.x > max.x || a_max.y < min.y || a_min.y > max.y);
                if intersects != st.selected {
                    st.selected = intersects;
                    dirty.0 = true;
                }
            }
        }
    }

    // Drag move
    if mouse.left_pressed {
        let mut moved_any = false;
        for (mut tr, _, mut st) in q_squares.iter_mut() {
            if st.dragging {
                let new_x = world_pos.x + st.drag_offset.x;
                let new_y = world_pos.y + st.drag_offset.y;
                if tr.translation.x != new_x || tr.translation.y != new_y {
                    tr.translation.x = new_x;
                    tr.translation.y = new_y;
                    moved_any = true;
                }
            }
        }
        if moved_any {
            dirty.0 = true;
        }
    }

    // Mouse release
    if mouse.just_released {
        marquee_res.start = None;
        marquee_res.current = None;
        for (_, _, mut st) in q_squares.iter_mut() {
            if st.dragging {
                st.dragging = false;
                st.drag_offset = Vec2::ZERO;
                dirty.0 = true;
            }
        }
    }

    // Final color computation (only if something potentially changed)
    if dirty.0 {
        for (_, ms, mut st) in q_squares.iter_mut() {
            let base = ms.base_color;
            let new_color = if st.dragging {
                [base[0] * 0.8, base[1] * 0.2, base[2] * 0.2, 1.0]
            } else if st.selected {
                [base[0] * 0.9, base[1] * 0.9, base[2] * 0.1, 1.0]
            } else if st.hovered {
                [0.0, 0.9, 0.3, 1.0]
            } else {
                [base[0], base[1], base[2], 1.0]
            };
            if new_color != st.final_color {
                st.final_color = new_color;
            }
        }
    }
}

// -------------------------------------------------------------------------------------------------
// Rendering systems
// -------------------------------------------------------------------------------------------------

pub(crate) fn render_draggable_square(
    mut scenes: Query<&mut VelloScene, With<DraggableOverlayScene>>,
    state: Res<DraggableSquare>,
) {
    let Ok(mut scene) = scenes.single_mut() else { return; };
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

pub(crate) fn render_mini_squares(
    mut dirty: ResMut<MiniSquaresDirty>,
    mut q_scene: Query<&mut VelloScene, With<MiniSquaresScene>>,
    q_squares: Query<(&Transform, &MiniSquare, &MiniSquareState)>,
) {
    if !dirty.0 {
        return;
    }
    let Ok(mut scene) = q_scene.single_mut() else { return; };
    scene.reset();

    // Canonical unit rect
    const UNIT_RECT: kurbo::Rect = kurbo::Rect::new(0.0, 0.0, 1.0, 1.0);

    for (tr, sq, st) in q_squares.iter() {
        let center = tr.translation.truncate();
        let half = sq.size * 0.5;
        // let affine = kurbo::Affine::translate((
        //     (center.x - half) as f64,
        //     (center.y - half) as f64,
        // ))
        // .then_scale(sq.size as f64);
        let affine = kurbo::Affine::scale(sq.size as f64).then_translate((
            (center.x - half) as f64,
            (center.y - half) as f64,
        ).into());


        scene.fill(
            peniko::Fill::NonZero,
            affine,
            peniko::Color::new(st.final_color),
            None,
            &UNIT_RECT,
        );
    }

    dirty.0 = false;
}

pub(crate) fn render_selection_marquee(
    marquee_res: Res<SelectionMarquee>,
    mut q_scene: Query<&mut VelloScene, With<SelectionMarqueeScene>>,
) {
    if marquee_res.is_changed() {
        if let Ok(mut scene) = q_scene.single_mut() {
            scene.reset();
            if let (Some(a), Some(b)) = (marquee_res.start, marquee_res.current) {
                let min = a.min(b);
                let max = a.max(b);
                let rect = kurbo::Rect::new(min.x as f64, min.y as f64, max.x as f64, max.y as f64);
                scene.fill(
                    peniko::Fill::NonZero,
                    kurbo::Affine::default(),
                    peniko::Color::new([0.1, 0.4, 1.0, 0.15]),
                    None,
                    &rect,
                );
                let stroke = kurbo::Stroke::new(2.0);
                scene.stroke(
                    &stroke,
                    kurbo::Affine::default(),
                    peniko::Color::new([0.1, 0.4, 1.0, 0.9]),
                    None,
                    &rect,
                );
            }
        }
    }
}
