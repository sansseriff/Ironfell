use crate::camera_controller::CameraController;
use bevy::math::bounding::{Aabb3d, Bounded3d};
use bevy::prelude::*;
use bevy::camera::visibility::RenderLayers;
use std::ops::Deref;
use bevy::core_pipeline::tonemapping::Tonemapping;

/// The camera of one 3D composition panel (ECM §7.1). It renders the
/// scenery layer (0) and its composition's layer into the panel's rect.
#[derive(Component, Debug)]
pub(crate) struct PanelCamera {
    pub panel: String,
}

/// A marker component for our shapes so we can query them separately from the ground plane
#[derive(Component, Clone)]
pub(crate) enum Shape {
    Box(Cuboid),
}

#[derive(Component, Default)]
pub(crate) struct ActiveState {
    pub hover: bool,
    pub selected: bool,
}
impl ActiveState {
    pub(crate) fn is_active(&self) -> bool {
        self.hover || self.selected
    }
}

#[derive(Component, Debug)]
pub(crate) struct Despawnable;

#[derive(Component, Debug)]
pub(crate) struct CurrentVolume(pub Aabb3d);
impl Deref for CurrentVolume {
    type Target = Aabb3d;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Lights and ground, on the scenery layer every 3D panel renders. Content
/// (the torus) is a document node, materialised by `document_bridge`, and
/// cameras come and go with panels (`sync_panel_cameras`).
pub(crate) fn setup_3d_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Lights
    commands.spawn((
        PointLight {
            shadow_maps_enabled: false,
            intensity: 07_000_000.,
            range: 100.0,
            ..default()
        },
        Transform::from_xyz(8.0, 9.0, 16.0),
    ));
    commands.spawn((
        PointLight {
            shadow_maps_enabled: false,
            intensity: 2_000_000.,
            range: 100.0,
            ..default()
        },
        Transform::from_xyz(-8.0, 9.0, -10.0),
    ));

    // Ground
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(50.0, 50.0).subdivisions(10))),
        MeshMaterial3d(materials.add(Color::srgb(0.75, 0.75, 0.75))),
    ));

}

/// One `Camera3d` per 3D composition panel: spawned when the panel appears,
/// its viewport and layers kept equal to the panel's, despawned with it.
/// The first camera gets the free-look controller (one at a time: the
/// controller drives a single camera).
pub(crate) fn sync_panel_cameras(
    mut commands: Commands,
    views: Res<crate::document_bridge::PanelViews>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut cameras: Query<(Entity, &PanelCamera, &mut Camera, &mut RenderLayers, Option<&mut ScreenRect>)>,
    others: Query<&Camera, Without<PanelCamera>>,
    controlled: Query<(), With<CameraController>>,
) {
    use iron_document::composition::Dims;
    let Ok(window) = windows.single() else { return };
    let (win_w, win_h) = (window.resolution.physical_width(), window.resolution.physical_height());
    if win_w == 0 || win_h == 0 {
        return;
    }
    let wanted: Vec<_> = views.0.iter().filter(|v| v.dims == Dims::Three).collect();
    for (e, pc, ..) in &cameras {
        if !wanted.iter().any(|v| v.panel == pc.panel) {
            commands.entity(e).despawn();
        }
    }
    let mut has_controller = !controlled.is_empty();
    let mut used: Vec<isize> = others.iter().map(|c| c.order).chain(cameras.iter().map(|(_, _, c, ..)| c.order)).collect();
    for view in &wanted {
        // Clamped to the window so resize races can never produce an
        // out-of-bounds viewport.
        let r = view.rect;
        let x = (r.x.max(0.0) as u32).min(win_w.saturating_sub(1));
        let y = (r.y.max(0.0) as u32).min(win_h.saturating_sub(1));
        let w = (r.w.max(1.0) as u32).min(win_w - x).max(1);
        let h = (r.h.max(1.0) as u32).min(win_h - y).max(1);
        let viewport = bevy::camera::Viewport {
            physical_position: UVec2::new(x, y),
            physical_size: UVec2::new(w, h),
            ..default()
        };
        let layers = RenderLayers::from_layers(&[0, view.layer]);
        if let Some((_, _, mut camera, mut current, screen)) = cameras.iter_mut().find(|(_, pc, ..)| pc.panel == view.panel) {
            place(&mut camera, screen, &viewport);
            if *current != layers {
                *current = layers;
            }
            continue;
        }
        let mut ec = commands.spawn((
            Camera3d::default(),
            bevy::render::view::Msaa::Off,
            Tonemapping::BlenderFilmic,
            // No clear: a clear load op wipes the whole shared target, not
            // just this viewport, erasing panels drawn before it. The
            // background camera (order -10) clears the window once.
            Camera { order: free_order(&mut used), ..default() },
            ScreenRect::of(&viewport),
            PanelCamera { panel: view.panel.clone() },
            layers,
            Projection::Perspective(PerspectiveProjection {
                fov: 60.0_f32.to_radians(),
                near: 0.1,
                far: 1000.0,
                ..default()
            }),
            Transform::from_xyz(0.0, 7.0, 9.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
            Name::new(format!("panel camera {}", view.panel)),
        ));
        if !has_controller {
            ec.insert(CameraController::default());
            has_controller = true;
        }
    }
}

pub(crate) fn render_active_shapes(
    mut gizmos: Gizmos,
    query: Query<(&Shape, &Transform, &ActiveState)>,
) {
    use bevy::color::palettes::css::BLANCHED_ALMOND;
    use bevy::color::palettes::tailwind::BLUE_400;
    for (shape, transform, active_state) in query.iter() {
        if !active_state.is_active() {
            continue;
        }
        let color = if active_state.selected {
            BLUE_400
        } else {
            BLANCHED_ALMOND
        };
        let translation = transform.translation.xyz();
        match shape {
            Shape::Box(cuboid) => {
                gizmos.primitive_3d(
                    cuboid,
                    Isometry3d::new(translation, transform.rotation),
                    color,
                );
            }
        }
    }
}

pub(crate) fn update_aabbes(
    mut commands: Commands,
    mut config_store: ResMut<bevy::gizmos::config::GizmoConfigStore>,
    query: Query<(Entity, &Shape, &Transform), Or<(Changed<Shape>, Changed<Transform>)>>,
) {
    for (_, config, _) in config_store.iter_mut() {
        config.line.width = 3.;
    }
    for (entity, shape, transform) in query.iter() {
        let translation = transform.translation;
        let rotation = transform.rotation;
        let aabb = match shape {
            Shape::Box(b) => b.aabb_3d(Isometry3d::new(translation, rotation)),
        };
        commands.entity(entity).insert(CurrentVolume(aabb));
    }
}

/// "Show in" (ECM §7.2): frame each requested node in every panel that
/// shows it. A 2D panel pans so the node is centred, wherever it appears
/// in the panel: drawn directly, or through embeds (a node in an embedded
/// 3D composition centres that embed's view). A 3D panel showing the node's
/// composition keeps its viewing direction and backs off until the node's
/// bounds fill about half the view. Presentation only: nothing here
/// touches the document, and an authored camera never moves.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_focus(
    mut requests: ResMut<crate::document_bridge::FocusRequests>,
    store: Res<crate::document_bridge::DocumentStore>,
    views: Res<crate::document_bridge::PanelViews>,
    map: Res<crate::document_bridge::NodeMap>,
    spatial: Res<crate::document_bridge::SpatialScene>,
    nodes: Query<(&'static crate::document_bridge::Doc2d, &'static GlobalTransform)>,
    globals: Query<&'static GlobalTransform>,
    mut view_cameras: ResMut<crate::document_bridge::ViewCameras>,
    volumes: Query<(&GlobalTransform, Option<&CurrentVolume>)>,
    mut cameras: Query<(&PanelCamera, &mut Transform, &Projection, Option<&mut CameraController>)>,
) {
    use iron_document::composition::{self, Dims};
    for id in requests.0.drain(..) {
        let doc = store.0.document();
        let target = composition::composition_of(doc, id);
        for view in &views.0 {
            match view.dims {
                Dims::Two => {
                    // `locate` answers in the panel's document space, so
                    // the pan is absolute; the zoom is kept.
                    let Some(r) = crate::document_bridge::nest::locate(&store.0, &map, &nodes, &globals, &spatial, view, id) else { continue };
                    let centre = Vec2::new(r.center().x as f32, r.center().y as f32);
                    let camera = view_cameras.0.entry(view.panel.clone()).or_default();
                    camera.pan = Vec2::new(view.rect.w, view.rect.h) * 0.5 - centre * camera.zoom;
                    info!("focus {id}: panel {} pans to centre it", view.panel);
                }
                Dims::Three if view.composition == target => {
                    let Some((global, volume)) = map.entity(id).and_then(|e| volumes.get(e).ok()) else { continue };
                    let (centre, radius) = match volume {
                        Some(v) => {
                            let (min, max) = (Vec3::from(v.min), Vec3::from(v.max));
                            ((min + max) * 0.5, ((max - min) * 0.5).length().max(0.1))
                        }
                        None => (global.translation(), 1.0),
                    };
                    for (pc, mut transform, projection, controller) in &mut cameras {
                        if pc.panel != view.panel {
                            continue;
                        }
                        let fov = match projection {
                            Projection::Perspective(p) => p.fov,
                            _ => 60f32.to_radians(),
                        };
                        let dist = (radius / (fov * 0.5).tan() * 2.0).max(radius * 2.0);
                        let forward = transform.forward().as_vec3();
                        *transform = Transform::from_translation(centre - forward * dist).looking_at(centre, Vec3::Y);
                        if let Some(mut c) = controller {
                            // The controller caches yaw and pitch; re-read them.
                            c.initialized = false;
                        }
                        info!("focus {id}: panel {} camera frames it", view.panel);
                    }
                }
                Dims::Three => {}
            }
        }
    }
}

/// Camera orders available to 3D views: between the background camera
/// (-10) and the vector composite (10), which must stay on top. Cameras
/// sharing an order make Bevy skip one, so each new one takes the lowest
/// free slot.
fn free_order(used: &mut Vec<isize>) -> isize {
    let o = (-9..=9).find(|o| !used.contains(o)).unwrap_or(9);
    used.push(o);
    o
}

/// The camera of one 3D embed seen in a 2D panel (ECM §7.4), keyed like
/// its `EmbedView`. `framed` is the sphere the derived default view last
/// framed: the view holds still until the content leaves it, so dragging
/// something does not chase the camera.
#[derive(Component, Debug)]
pub(crate) struct EmbedCam {
    pub key: String,
    framed: Option<(Vec3, f32)>,
}

/// The direction the derived default view looks from.
const DEFAULT_VIEW_FROM: Vec3 = Vec3::new(0.0, 0.55, 0.85);
const DEFAULT_FOV: f32 = 0.8;

/// One `Camera3d` per 3D embed view: viewport at the embed's rectangle,
/// rendering scenery plus the embedded composition's layer, posed by the
/// named camera node or by the derived default view that frames all of
/// the composition's meshes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_embed_cameras(
    mut commands: Commands,
    embeds: Res<crate::document_bridge::EmbedViews>,
    store: Res<crate::document_bridge::DocumentStore>,
    map: Res<crate::document_bridge::NodeMap>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    poses: Query<&GlobalTransform, Without<EmbedCam>>,
    volumes: Query<(&CurrentVolume, &crate::document_bridge::InComposition)>,
    planes: Query<(&GlobalTransform, &crate::document_bridge::planes::EmbedPlane, &crate::document_bridge::InComposition), Without<EmbedCam>>,
    mut cams: Query<(Entity, &mut EmbedCam, &mut Camera, &mut RenderLayers, &mut Transform, &mut Projection, Option<&mut ScreenRect>, Option<&mut PointerToCamera>)>,
    others: Query<&Camera, Without<EmbedCam>>,
) {
    let _ = windows;
    for (e, cam, ..) in &cams {
        if !embeds.0.iter().any(|v| v.key == cam.key) {
            commands.entity(e).despawn();
        }
    }
    let mut used: Vec<isize> = others.iter().map(|c| c.order).chain(cams.iter().map(|(_, _, c, ..)| c.order)).collect();
    for view in &embeds.0 {
        // The whole embed at its on-screen scale, into the camera's texture;
        // the 2D view crops it (no reframing to a clipped strip).
        let (w, h) = (view.size.x.max(1), view.size.y.max(1));
        let viewport = bevy::camera::Viewport { physical_position: UVec2::ZERO, physical_size: view.size.max(UVec2::ONE), ..default() };
        let layers = RenderLayers::from_layers(&[0, view.layer]);
        let entity = match cams.iter().find(|(_, c, ..)| c.key == view.key).map(|(e, ..)| e) {
            Some(e) => e,
            None => {
                commands.spawn((
                        Camera3d::default(),
                        bevy::render::view::Msaa::Off,
                        Tonemapping::BlenderFilmic,
                        Camera { order: free_order(&mut used), ..default() },
                        ScreenRect::of(&viewport),
                        PointerToCamera(view.to_texture),
                        EmbedCam { key: view.key.clone(), framed: None },
                        layers.clone(),
                        Projection::Perspective(PerspectiveProjection { fov: DEFAULT_FOV, near: 0.1, far: 1000.0, ..default() }),
                        Transform::from_translation(DEFAULT_VIEW_FROM * 10.0).looking_at(Vec3::ZERO, Vec3::Y),
                        Name::new(format!("embed camera {}", view.key)),
                    ));
                // Posed from next frame, once the components exist.
                continue;
            }
        };
        let Ok((_, mut cam, mut camera, mut current, mut transform, mut projection, screen, pointer)) = cams.get_mut(entity) else { continue };
        place(&mut camera, screen, &viewport);
        if let Some(mut p) = pointer
            && p.0 != view.to_texture
        {
            p.0 = view.to_texture;
        }
        if *current != layers {
            *current = layers;
        }
        let aspect = (w as f32 / h as f32).max(1e-3);
        match view.camera {
            // A camera node: its pose and lens, as authored.
            Some(node) => {
                let Some(pose) = map.entity(node).and_then(|e| poses.get(e).ok()) else { continue };
                let t = pose.compute_transform();
                if *transform != t {
                    *transform = t;
                }
                let fov = "camera.fov".parse().ok().and_then(|p| store.0.resolved(node, p)).and_then(|v| v.as_f64()).unwrap_or(DEFAULT_FOV as f64) as f32;
                if let Projection::Perspective(p) = projection.as_mut()
                    && p.fov != fov
                {
                    p.fov = fov;
                }
                cam.framed = None;
            }
            // The derived default view: frame every mesh of the composition
            // from a fixed direction, re-framing only when the content
            // leaves what was framed.
            None => {
                // Meshes, and the corners of planes (2D compositions in it).
                let plane_corners = planes.iter().filter(|(_, _, within)| within.0 == Some(view.source)).flat_map(|(gt, plane, _)| {
                    let (w, h) = ((plane.region.width() / crate::document_bridge::planes::PX_PER_UNIT) as f32, (plane.region.height() / crate::document_bridge::planes::PX_PER_UNIT) as f32);
                    [Vec3::ZERO, Vec3::new(w, 0.0, 0.0), Vec3::new(w, -h, 0.0), Vec3::new(0.0, -h, 0.0)].map(|c| {
                        let p = gt.transform_point(c);
                        (p, p)
                    })
                });
                let bounds = volumes
                    .iter()
                    .filter(|(_, within)| within.0 == Some(view.source))
                    .map(|(v, _)| (Vec3::from(v.min), Vec3::from(v.max)))
                    .chain(plane_corners)
                    .reduce(|(a0, a1), (b0, b1)| (a0.min(b0), a1.max(b1)));
                let (centre, radius) = match bounds {
                    Some((lo, hi)) => ((lo + hi) * 0.5, ((hi - lo) * 0.5).length().max(0.25)),
                    None => (Vec3::ZERO, 2.0),
                };
                let reframe = match cam.framed {
                    Some((c0, r0)) => centre.distance(c0) + radius > r0 * 1.001 || radius < r0 * 0.5,
                    None => true,
                };
                if reframe {
                    let half_v = DEFAULT_FOV * 0.5;
                    let half_h = (half_v.tan() * aspect).atan();
                    let dist = radius / half_v.min(half_h).sin() * 1.1;
                    *transform = Transform::from_translation(centre + DEFAULT_VIEW_FROM.normalize() * dist).looking_at(centre, Vec3::Y);
                    cam.framed = Some((centre, radius));
                    if let Projection::Perspective(p) = projection.as_mut() {
                        p.fov = DEFAULT_FOV;
                    }
                }
            }
        }
    }
}


// ---------------------------------------------------------------------------
// View textures (plans/specs/view-textures.md, spike S1 and S3): a 3D camera
// renders into its own texture sized to its rectangle, pasted into the
// window by a composite quad, and renders only when what it shows changed.

/// Where a texture-backed camera's output goes in the window. Written by the
/// camera sync systems in place of a viewport.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScreenRect {
    pub pos: UVec2,
    pub size: UVec2,
}

impl ScreenRect {
    fn of(v: &bevy::camera::Viewport) -> Self {
        ScreenRect { pos: v.physical_position, size: v.physical_size }
    }
}

/// Keep a camera's screen rectangle current (its texture, viewport and
/// composite follow in `sync_view_images`).
fn place(_camera: &mut Mut<Camera>, screen: Option<Mut<ScreenRect>>, viewport: &bevy::camera::Viewport) {
    if let Some(mut s) = screen {
        let next = ScreenRect::of(viewport);
        if *s != next {
            *s = next;
        }
    }
}

/// A texture-backed camera's target and the quad that composites it.
#[derive(Component, Debug)]
pub(crate) struct ViewImage {
    image: Handle<Image>,
    bucket: UVec2,
    /// The quad pasting it into the window; none for a 3D embed, whose
    /// texture the 2D view that embeds it draws instead.
    composite: Option<Entity>,
    /// Renders this frame: its version moves on, so the 2D view drawing it
    /// re-renders in step (see `EmbedImages`).
    was_active: bool,
    version: u32,
    material: Handle<crate::vector::view_composite::ViewCompositeMaterial>,
    /// Frames still to render whatever happens: a new texture is not on the
    /// GPU for a frame or two.
    fresh: u8,
    /// Frames to stay awake after a change. A view woken for a single frame
    /// can render with per-view state (light clusters, visibility) from
    /// before it slept, so it renders a few frames before sleeping again.
    awake: u8,
}

/// For a camera whose texture is drawn inside a 2D view (a 3D embed), the
/// map from window px to its own texture px, through every transform on the
/// way (it may rotate and scale).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct PointerToCamera(pub kurbo::Affine);

/// Window px → this camera's own target px: through the embed's mapping, or
/// offset by the panel rectangle's corner.
pub(crate) fn to_camera_px(screen: Vec2, rect: Option<&ScreenRect>, pointer: Option<&PointerToCamera>) -> Vec2 {
    match pointer {
        Some(p) => {
            let q = p.0 * kurbo::Point::new(screen.x as f64, screen.y as f64);
            Vec2::new(q.x as f32, q.y as f32)
        }
        None => screen - rect.map_or(Vec2::ZERO, |s| s.pos.as_vec2()),
    }
}

/// Keep each texture-backed camera's target, viewport and composite equal to
/// its screen rectangle, and render it only when something it shows changed:
/// its rectangle, its pose or lens, or any 3D content on its layers (moved,
/// highlighted, added or removed).
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_view_images(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<crate::vector::view_composite::ViewCompositeMaterial>>,
    mut quad: Local<Option<Handle<Mesh>>>,
    mut cams: Query<(Entity, Ref<ScreenRect>, &mut Camera, Option<&mut ViewImage>, &RenderLayers, Ref<GlobalTransform>, Ref<Projection>, Option<&EmbedCam>)>,
    content: Query<(&RenderLayers, Ref<GlobalTransform>, Ref<ActiveState>), With<crate::document_bridge::Doc3d>>,
    mut removed: RemovedComponents<crate::document_bridge::Doc3d>,
    composites: Query<(Entity, &ViewCompositeOf)>,
    clear: Res<ClearColor>,
    mut embed_images: ResMut<crate::document_bridge::EmbedImages>,
) {
    let mut published = std::collections::BTreeMap::new();
    use crate::vector::view_composite::{MODE_STRAIGHT_SRGB, ViewCompositeMaterial, bucket, unit_quad};
    let quad = quad.get_or_insert_with(|| meshes.add(unit_quad())).clone();
    let removed_any = removed.read().count() > 0;
    for (entity, screen, mut camera, view, layers, pose, projection, embed) in &mut cams {
        let size = screen.size.max(UVec2::ONE);
        let want = bucket(size);
        let rect = Vec4::new(screen.pos.x as f32, screen.pos.y as f32, size.x as f32, size.y as f32);
        let params = Vec4::new(want.x as f32, want.y as f32, MODE_STRAIGHT_SRGB, 0.0);
        let mut dirty = screen.is_changed() || pose.is_changed() || projection.is_changed() || removed_any;
        let mut awake: Option<u8> = None;
        match view {
            Some(mut v) if v.bucket == want => {
                if screen.is_changed()
                    && let Some(mut m) = materials.get_mut(&v.material)
                {
                    m.rect = rect;
                }
                if v.fresh > 0 {
                    v.fresh -= 1;
                    dirty = true;
                }
                if !dirty {
                    dirty = content.iter().any(|(l, g, a)| l.intersects(layers) && (g.is_changed() || a.is_changed()));
                }
                v.awake = if dirty { 3 } else { v.awake.saturating_sub(1) };
                awake = Some(v.awake);
                v.was_active = dirty || v.awake > 0;
                // Rendering this frame: the 2D view that draws this texture
                // re-renders this frame too, inside the render graph after
                // this camera (`render_textured_scenes`).
                if v.was_active {
                    v.version = v.version.wrapping_add(1);
                }
                if let Some(ec) = embed {
                    published.insert(ec.key.clone(), crate::document_bridge::EmbedImage { image: v.image.clone(), size, version: v.version });
                }
            }
            existing => {
                // A new texture: first use, or the size crossed a bucket.
                let image = images.add(Image::new_target_texture(want.x, want.y, bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb, None));
                match existing {
                    Some(mut v) => {
                        if let Some(mut m) = materials.get_mut(&v.material) {
                            m.texture = image.clone();
                            m.rect = rect;
                            m.params = params;
                        }
                        v.image = image.clone();
                        v.bucket = want;
                        v.fresh = 2;
                        v.was_active = true;
                    }
                    None => {
                        let material = materials.add(ViewCompositeMaterial { texture: image.clone(), rect, params });
                        // A 3D embed's texture is drawn by the 2D view that
                        // embeds it; only panel cameras paste into the window.
                        let composite = embed.is_none().then(|| commands
                            .spawn((
                                Mesh2d(quad.clone()),
                                MeshMaterial2d(material.clone()),
                                // Under the vector composite (z = 0): 2D content
                                // stays on top, as with viewport cameras.
                                Transform::from_xyz(0.0, 0.0, -1.0),
                                RenderLayers::layer(crate::vector::VECTOR_LAYER),
                                bevy::camera::visibility::NoFrustumCulling,
                                Visibility::Visible,
                                ViewCompositeOf(entity),
                                Name::new("view composite"),
                            ))
                            .id());
                        commands.entity(entity).insert(ViewImage { image: image.clone(), bucket: want, composite, material, fresh: 2, awake: 0, was_active: true, version: 0 });
                    }
                }
                commands.entity(entity).insert(bevy::camera::RenderTarget::from(image));
                dirty = true;
            }
        }
        let viewport = bevy::camera::Viewport { physical_position: UVec2::ZERO, physical_size: size, ..default() };
        if camera.viewport.as_ref().is_none_or(|v| v.physical_size != size || v.physical_position != UVec2::ZERO) {
            camera.viewport = Some(viewport);
        }
        // The app's clear colour, which follows the light/dark theme
        // (`web_ffi::set_dark_mode`): a change re-renders the view.
        if !matches!(camera.clear_color, ClearColorConfig::Custom(c) if c == clear.0) {
            camera.clear_color = ClearColorConfig::Custom(clear.0);
            dirty = true;
        }
        let active = dirty || awake.is_some_and(|a| a > 0);
        if camera.is_active != active {
            camera.is_active = active;
        }
    }
    if embed_images.0 != published {
        embed_images.0 = published;
    }
    // A camera that went away takes its composite with it.
    for (e, of) in &composites {
        if cams.get(of.0).is_err() {
            commands.entity(e).despawn();
        }
    }
}

/// The camera whose texture a composite quad pastes.
#[derive(Component, Debug)]
pub(crate) struct ViewCompositeOf(Entity);
