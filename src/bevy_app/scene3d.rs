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
    mut cameras: Query<(Entity, &PanelCamera, &mut Camera, &mut RenderLayers)>,
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
    let mut used: Vec<isize> = others.iter().map(|c| c.order).chain(cameras.iter().map(|(_, _, c, _)| c.order)).collect();
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
        if let Some((_, _, mut camera, mut current)) = cameras.iter_mut().find(|(_, pc, ..)| pc.panel == view.panel) {
            let moved = camera.viewport.as_ref().is_none_or(|v| v.physical_position != viewport.physical_position || v.physical_size != viewport.physical_size);
            if moved {
                camera.viewport = Some(viewport);
            }
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
            Camera { order: free_order(&mut used), clear_color: ClearColorConfig::None, viewport: Some(viewport), ..default() },
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
    bounds: Res<crate::document_bridge::Bounds2d>,
    nodes: Query<(&'static crate::document_bridge::Doc2d, &'static GlobalTransform)>,
    globals: Query<&'static GlobalTransform>,
    mut pans: ResMut<crate::document_bridge::PanelPans>,
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
                    // `locate` answers in the panel's un-panned document
                    // space, so the pan is absolute.
                    let Some(r) = crate::document_bridge::nest::locate(&store.0, &map, &nodes, &globals, &bounds.0, view, id) else { continue };
                    let centre = Vec2::new(r.center().x as f32, r.center().y as f32);
                    pans.0.insert(view.panel.clone(), Vec2::new(view.rect.w, view.rect.h) * 0.5 - centre);
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
    mut cams: Query<(Entity, &mut EmbedCam, &mut Camera, &mut RenderLayers, &mut Transform, &mut Projection)>,
    others: Query<&Camera, Without<EmbedCam>>,
) {
    let Ok(window) = windows.single() else { return };
    let (win_w, win_h) = (window.resolution.physical_width(), window.resolution.physical_height());
    if win_w == 0 || win_h == 0 {
        return;
    }
    for (e, cam, ..) in &cams {
        if !embeds.0.iter().any(|v| v.key == cam.key) {
            commands.entity(e).despawn();
        }
    }
    let mut used: Vec<isize> = others.iter().map(|c| c.order).chain(cams.iter().map(|(_, _, c, ..)| c.order)).collect();
    for view in &embeds.0 {
        let r = view.rect;
        let x = (r.x0.max(0.0) as u32).min(win_w.saturating_sub(1));
        let y = (r.y0.max(0.0) as u32).min(win_h.saturating_sub(1));
        let w = (r.width().max(1.0) as u32).min(win_w - x).max(1);
        let h = (r.height().max(1.0) as u32).min(win_h - y).max(1);
        let viewport = bevy::camera::Viewport { physical_position: UVec2::new(x, y), physical_size: UVec2::new(w, h), ..default() };
        let layers = RenderLayers::from_layers(&[0, view.layer]);
        let entity = match cams.iter().find(|(_, c, ..)| c.key == view.key).map(|(e, ..)| e) {
            Some(e) => e,
            None => {
                let e = commands
                    .spawn((
                        Camera3d::default(),
                        bevy::render::view::Msaa::Off,
                        Tonemapping::BlenderFilmic,
                        // No clear, as for panel cameras: the background
                        // camera clears the window once.
                        Camera { order: free_order(&mut used), clear_color: ClearColorConfig::None, viewport: Some(viewport.clone()), ..default() },
                        EmbedCam { key: view.key.clone(), framed: None },
                        layers.clone(),
                        Projection::Perspective(PerspectiveProjection { fov: DEFAULT_FOV, near: 0.1, far: 1000.0, ..default() }),
                        Transform::from_translation(DEFAULT_VIEW_FROM * 10.0).looking_at(Vec3::ZERO, Vec3::Y),
                        Name::new(format!("embed camera {}", view.key)),
                    ))
                    .id();
                // Posed from next frame, once the components exist.
                let _ = e;
                continue;
            }
        };
        let Ok((_, mut cam, mut camera, mut current, mut transform, mut projection)) = cams.get_mut(entity) else { continue };
        if camera.viewport.as_ref().is_none_or(|v| v.physical_position != viewport.physical_position || v.physical_size != viewport.physical_size) {
            camera.viewport = Some(viewport);
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
                let bounds = volumes
                    .iter()
                    .filter(|(_, within)| within.0 == Some(view.source))
                    .map(|(v, _)| (Vec3::from(v.min), Vec3::from(v.max)))
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
