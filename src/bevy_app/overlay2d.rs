use bevy::input::mouse::MouseButtonInput; // added for button event reader
use bevy::prelude::*;
use kurbo;
use peniko;

use crate::vector::{DisplayList, DisplayListRebuild, VectorLayer, order};
// Bring kurbo trait methods into scope for PathSeg operations (arclen, inv_arclen, etc.)
use kurbo::{ParamCurve, ParamCurveArclen};

use crate::panels::{Panels, VIEWER_PANEL, overlay_affine, overlay_world_from_screen};

// -------------------------------------------------------------------------------------------------
// Overlay 2D content, drawn in "overlay world" coordinates (viewer panel center origin,
// y-up) and mapped into screen space + clipped to the viewer panel rect at render time.
// -------------------------------------------------------------------------------------------------

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
pub(crate) struct DraggableOverlayLayer; // Separate Vello scene so it isn't affected by the animated transform

#[derive(Component)]
pub(crate) struct AnimatedOverlayLayer; // Marker for animated overlay scene (needs Transform)

#[derive(Component)]
pub(crate) struct AnimatedBezierStrokeLayer; // Marker for animated bezier stroke scene

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
pub(crate) struct MiniSquaresLayer; // single batched scene for all mini squares

#[derive(Resource, Default)]
pub(crate) struct SelectionMarquee {
    start: Option<Vec2>,
    current: Option<Vec2>,
}

#[derive(Component)]
pub(crate) struct SelectionMarqueeLayer;

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
    mut events: MessageReader<MouseButtonInput>,
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
) {
    // All overlay layers are screen-space; panel placement and clipping are baked
    // into the emitted command transforms by the render systems below. Painter
    // order is explicit (see `vector::order`) rather than left to spawn order.

    commands.spawn((
        DisplayList::default(),
        VectorLayer::screen(order::OVERLAY_ANIMATED),
        AnimatedOverlayLayer,
    ));

    commands.spawn((
        DisplayList::default(),
        VectorLayer::screen(order::DRAGGABLE),
        DraggableOverlayLayer,
    ));

    if existing_bezier.is_none() {
        commands.insert_resource(AnimatedBezierPath::generate());
    }
    commands.spawn((
        DisplayList::default(),
        VectorLayer::screen(order::OVERLAY_BEZIER),
        AnimatedBezierStrokeLayer,
    ));

    // Mini squares are individual entities; one shared layer batches their draws.
    let mut seed: u32 = 0x91E2_33AB;
    fn next(seed: &mut u32) -> f32 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 17;
        *seed ^= *seed << 5;
        (*seed as f32) / (u32::MAX as f32)
    }
    let mini_size = 120.0 / 4.0;
    for _ in 0..20 {
        let x = (next(&mut seed) - 0.5) * 1900.0;
        let y = (next(&mut seed) - 0.5) * 1200.0;
        let r = next(&mut seed);
        let g = next(&mut seed);
        let b = next(&mut seed);
        commands.spawn((
            Transform::from_translation(Vec3::new(x, y, 0.0)),
            GlobalTransform::default(),
            MiniSquare { size: mini_size, base_color: [r, g, b] },
            MiniSquareState {
                final_color: [r, g, b, 1.0],
                ..Default::default()
            },
        ));
    }
    commands.spawn((
        DisplayList::default(),
        VectorLayer::screen(order::MINI_SQUARES),
        MiniSquaresLayer,
    ));

    commands.insert_resource(SelectionMarquee::default());
    commands.spawn((
        DisplayList::default(),
        VectorLayer::screen(order::SELECTION_MARQUEE),
        SelectionMarqueeLayer,
    ));
}

pub(crate) fn animate_2d_overlay(
    mut animated: Query<
        &mut DisplayList,
        (With<AnimatedOverlayLayer>, Without<AnimatedBezierStrokeLayer>),
    >,
    mut bezier_layer: Query<
        &mut DisplayList,
        (With<AnimatedBezierStrokeLayer>, Without<AnimatedOverlayLayer>),
    >,
    bezier: Option<Res<AnimatedBezierPath>>,
    time: Res<Time>,
    panels: Res<Panels>,
) {
    let Ok(mut animated_list) = animated.single_mut() else {
        return;
    };
    let sin_time = time.elapsed_secs().sin().mul_add(0.5, 0.5);

    let Some(rect) = panels.rect(VIEWER_PANEL) else {
        animated_list.rebuild(|_| {});
        if let Ok(mut list) = bezier_layer.single_mut() {
            list.rebuild(|_| {});
        }
        return;
    };
    let base = overlay_affine(rect);
    let clip = rect.to_kurbo();

    let c = Vec3::lerp(
        Vec3::new(-1.0, 1.0, -1.0),
        Vec3::new(-1.0, 1.0, 1.0),
        sin_time + 0.5,
    );

    // Animation is baked into the emitted affine rather than the entity Transform:
    // translate (world y-up) ∘ rotate ∘ scale, then mapped into screen space.
    let translation = f64::from(Vec3::lerp(Vec3::Y * -900.0, Vec3::Y * 900.0, sin_time).y);
    let rotation = f64::from(-std::f32::consts::TAU * sin_time);
    let scale = f64::from(Vec3::lerp(Vec3::ONE * 0.5, Vec3::ONE * 1.0, sin_time).x);
    let anim = base
        * kurbo::Affine::translate((0.0, translation))
        * kurbo::Affine::rotate(rotation)
        * kurbo::Affine::scale(scale);

    animated_list.rebuild(|b| {
        b.clipped(kurbo::Affine::IDENTITY, clip, |b| {
            b.fill(
                anim,
                peniko::Color::new([c.x, c.y, c.z, 1.]),
                kurbo::RoundedRect::new(-100.0, -100.0, 100.0, 100.0, (sin_time as f64) * 100.0),
            );
        });
    });

    // Progressive bezier stroke reveal.
    let (Ok(mut stroke_list), Some(bezier)) = (bezier_layer.single_mut(), bezier) else {
        return;
    };
    let progress = (time.elapsed_secs() / 6.0).fract().clamp(0.0, 1.0);
    let target_len = bezier.total_length * (progress as f64);
    if target_len <= 0.0 {
        stroke_list.rebuild(|_| {});
        return;
    }

    // Geometry is resolved before the rebuild closure so the closure stays a
    // straight description of drawing intent.
    let full = (target_len - bezier.total_length).abs() < f64::EPSILON;
    let partial = if full {
        bezier.path.clone()
    } else {
        partial_path(&bezier, target_len)
    };
    let head = partial.segments().last().map(|last| match last {
        kurbo::PathSeg::Line(l) => l.p1,
        kurbo::PathSeg::Quad(q) => q.p2,
        kurbo::PathSeg::Cubic(c) => c.p3,
    });
    let stroke_width = bezier.stroke_width as f64;

    stroke_list.rebuild(|b| {
        b.clipped(kurbo::Affine::IDENTITY, clip, |b| {
            b.stroke(
                base,
                kurbo::Stroke::new(stroke_width),
                peniko::Color::new([0.0, 0.6, 1.0, 1.0]),
                partial.clone(),
            );
            // The leading dot only exists while the stroke is still drawing.
            if !full {
                if let Some(head) = head {
                    b.fill(
                        base,
                        peniko::Color::new([0.95, 0.2, 0.4, 1.0]),
                        kurbo::Circle::new(head, stroke_width * 0.55),
                    );
                }
            }
        });
    });
}

/// Build the leading `target_len` of `bezier`'s path, splitting the segment the
/// reveal currently sits inside.
fn partial_path(bezier: &AnimatedBezierPath, target_len: f64) -> kurbo::BezPath {
    let mut partial = kurbo::BezPath::new();
    let mut remaining = target_len;
    let mut idx = 0usize;
    let mut segs = bezier.path.segments();

    let Some(first) = segs.next() else {
        return partial;
    };
    let first_start = match first {
        kurbo::PathSeg::Line(l) => l.p0,
        kurbo::PathSeg::Quad(q) => q.p0,
        kurbo::PathSeg::Cubic(c) => c.p0,
    };
    partial.move_to(first_start);

    let take_seg = |seg: kurbo::PathSeg, partial: &mut kurbo::BezPath, remaining: &mut f64, idx: usize| {
        let seg_len = bezier.seg_lengths[idx];
        if *remaining >= seg_len {
            partial.push(seg.as_path_el());
            *remaining -= seg_len;
            true
        } else {
            let t = seg.inv_arclen(*remaining, 0.5);
            partial.push(seg.subsegment(0.0..t).as_path_el());
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
    partial
}

// -------------------------------------------------------------------------------------------------
// Draggable square logic
// -------------------------------------------------------------------------------------------------

pub(crate) fn update_draggable_square_state(
    mut state: ResMut<DraggableSquare>,
    mut cursor_events: MessageReader<CursorMoved>,
    mouse: Res<SimpleMouseState>,
    panels: Res<Panels>,
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
    let Some(rect) = panels.rect(VIEWER_PANEL) else {
        return;
    };
    let world_pos = overlay_world_from_screen(rect, last_pos);

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
    mut cursor_events: MessageReader<CursorMoved>,
    mouse: Res<SimpleMouseState>,
    panels: Res<Panels>,
) {
    // Tracks whether anything this frame could change a square's colour. Purely a
    // local optimisation now: the display list decides for itself whether to
    // re-encode, so this no longer has to survive across frames.
    let mut dirty = false;

    if cursor_events.is_empty() && !mouse.just_pressed && !mouse.just_released {
        if mouse.just_released {
            for (_, _, mut st) in q_squares.iter_mut() {
                st.dragging = false;
                st.drag_offset = Vec2::ZERO;
            }
            marquee_res.start = None;
            marquee_res.current = None;
            recompute_final_colors(&mut q_squares);
        }
        return;
    }

    // Latest cursor world position
    let mut world_pos_opt = None;
    if let Some(screen) = cursor_events.read().last().map(|e| e.position) {
        if let Some(rect) = panels.rect(VIEWER_PANEL) {
            world_pos_opt = Some(overlay_world_from_screen(rect, screen));
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
            dirty = true;
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
                        dirty = true;
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
                    dirty = true;
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
                    dirty = true;
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
            dirty = true;
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
                dirty = true;
            }
        }
    }

    // Final color computation (only if something potentially changed)
    if dirty {
        recompute_final_colors(&mut q_squares);
    }
}

/// Resolve each square's display colour from its interaction state.
fn recompute_final_colors(
    q_squares: &mut Query<'_, '_, (&mut Transform, &MiniSquare, &mut MiniSquareState)>,
) {
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

// -------------------------------------------------------------------------------------------------
// Rendering systems
// -------------------------------------------------------------------------------------------------

pub(crate) fn render_draggable_square(
    mut layers: Query<&mut DisplayList, With<DraggableOverlayLayer>>,
    state: Res<DraggableSquare>,
    panels: Res<Panels>,
) {
    let Ok(mut list) = layers.single_mut() else {
        return;
    };
    let Some(panel_rect) = panels.rect(VIEWER_PANEL) else {
        list.rebuild(|_| {});
        return;
    };
    let base = overlay_affine(panel_rect);

    // Dragging: red, hover: pink, idle: dark gray.
    let (r, g, b_) = if state.dragging {
        (1.0, 0.0, 0.0)
    } else if state.hovered {
        (1.0, 0.4, 0.7)
    } else {
        (0.2, 0.2, 0.2)
    };
    let half = state.size * 0.5;
    let rect = kurbo::Rect::new(
        (state.position.x - half.x) as f64,
        (state.position.y - half.y) as f64,
        (state.position.x + half.x) as f64,
        (state.position.y + half.y) as f64,
    );

    list.rebuild(|b| {
        b.clipped(kurbo::Affine::IDENTITY, panel_rect.to_kurbo(), |b| {
            b.fill(base, peniko::Color::new([r, g, b_, 1.0]), rect);
        });
    });
}

pub(crate) fn render_mini_squares(
    mut layers: Query<&mut DisplayList, With<MiniSquaresLayer>>,
    q_squares: Query<(&Transform, &MiniSquare, &MiniSquareState)>,
    panels: Res<Panels>,
) {
    // The old explicit `MiniSquaresDirty` flag is gone: `rebuild` compares the
    // emitted commands, so rebuilding unconditionally re-encodes only on a real
    // change and cannot drift out of sync with the data the way a manual flag can.
    let Ok(mut list) = layers.single_mut() else {
        return;
    };
    let Some(panel_rect) = panels.rect(VIEWER_PANEL) else {
        list.rebuild(|_| {});
        return;
    };
    let base = overlay_affine(panel_rect);
    const UNIT_RECT: kurbo::Rect = kurbo::Rect::new(0.0, 0.0, 1.0, 1.0);

    list.rebuild(|b| {
        b.clipped(kurbo::Affine::IDENTITY, panel_rect.to_kurbo(), |b| {
            for (tr, sq, st) in q_squares.iter() {
                let center = tr.translation.truncate();
                let half = sq.size * 0.5;
                let affine = base
                    * kurbo::Affine::scale(sq.size as f64).then_translate(
                        ((center.x - half) as f64, (center.y - half) as f64).into(),
                    );
                b.fill(affine, peniko::Color::new(st.final_color), UNIT_RECT);
            }
        });
    });
}

pub(crate) fn render_selection_marquee(
    marquee_res: Res<SelectionMarquee>,
    mut layers: Query<&mut DisplayList, With<SelectionMarqueeLayer>>,
    panels: Res<Panels>,
) {
    let Ok(mut list) = layers.single_mut() else {
        return;
    };
    let Some(panel_rect) = panels.rect(VIEWER_PANEL) else {
        list.rebuild(|_| {});
        return;
    };
    let base = overlay_affine(panel_rect);

    list.rebuild(|b| {
        let (Some(a), Some(c)) = (marquee_res.start, marquee_res.current) else {
            return;
        };
        let min = a.min(c);
        let max = a.max(c);
        let rect = kurbo::Rect::new(min.x as f64, min.y as f64, max.x as f64, max.y as f64);
        b.clipped(kurbo::Affine::IDENTITY, panel_rect.to_kurbo(), |b| {
            b.fill(base, peniko::Color::new([0.1, 0.4, 1.0, 0.15]), rect);
            b.stroke(
                base,
                kurbo::Stroke::new(2.0),
                peniko::Color::new([0.1, 0.4, 1.0, 0.9]),
                rect,
            );
        });
    });
}
