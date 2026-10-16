//! Node → entity. One function per node type, and a reconcile pass that
//! spawns, despawns, or rewrites the entities for a set of touched nodes.

use super::{CompositionLayers, InComposition, NodeMap, Provenance};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use iron_document::{ComponentKind, LeafPath, NodeId, Store, TypeId, Value};
use std::collections::BTreeSet;

use crate::bevy_app::scene3d::{ActiveState, Shape};

/// Drawable 2D node: what the document layer emits for it. Geometry is in the
/// node's local space; placement comes from the entity's `GlobalTransform`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Doc2d {
    pub shape: Shape2d,
    pub fill: Option<[f32; 4]>,
    /// Colour, and width, joins, caps and dashes.
    pub stroke: Option<([f32; 4], kurbo::Stroke)>,
    pub shadow: Option<Shadow2d>,
}

/// A drop shadow (the `shadow` component): offset in the parent's space,
/// and the blur's standard deviation (half the authored blur radius).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow2d {
    pub color: [f32; 4],
    pub dx: f32,
    pub dy: f32,
    pub std_dev: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Shape2d {
    /// Spans `(0, 0)` to `(w, h)`: the node origin is the top-left corner.
    /// `radii` round its corners, clockwise from the top-left.
    Rect { w: f32, h: f32, radii: [f32; 4] },
    /// Centred on the node origin, with radii along its x and y; `arc`
    /// (start, sweep, inner) keeps part of it.
    Ellipse { rx: f32, ry: f32, arc: Option<[f32; 3]> },
    /// A scalar-on-a-track control (doc 02 §9.3): a `w`×`h` track with the
    /// knob at fraction `t` along it.
    Slider { w: f32, h: f32, t: f32 },
    /// A path through its contours' points (stage E), in the path's local
    /// space: `outline` is every contour (what is stroked), `fill` the
    /// closed ones (what is filled; none when no contour closes), filled by
    /// the even-odd rule or the non-zero one.
    Path { outline: std::sync::Arc<kurbo::BezPath>, fill: Option<std::sync::Arc<kurbo::BezPath>>, even_odd: bool },
}

/// How near (local px) the pointer must come to a path's line to hit it.
const PATH_HIT: f64 = 5.0;

impl Shape2d {
    pub fn contains_local(&self, p: Vec2) -> bool {
        match self {
            &Shape2d::Rect { w, h, radii } if radii.iter().any(|&r| r > 0.0) => {
                use kurbo::Shape;
                super::nest::rounded_rect(w, h, radii).contains(kurbo::Point::new(p.x as f64, p.y as f64))
            }
            &Shape2d::Rect { w, h, .. } | &Shape2d::Slider { w, h, .. } => p.x >= 0.0 && p.y >= 0.0 && p.x <= w && p.y <= h,
            &Shape2d::Ellipse { rx, ry, arc: None } => (p / Vec2::new(rx, ry).max(Vec2::splat(1e-6))).length_squared() <= 1.0,
            Shape2d::Ellipse { .. } => {
                use kurbo::Shape;
                super::nest::shape_path(self).winding(kurbo::Point::new(p.x as f64, p.y as f64)) != 0
            }
            Shape2d::Path { outline, fill, even_odd } => {
                use kurbo::{ParamCurveNearest, Shape};
                let q = kurbo::Point::new(p.x as f64, p.y as f64);
                if let Some(fill) = fill {
                    let w = fill.winding(q);
                    if if *even_odd { w % 2 != 0 } else { w != 0 } {
                        return true;
                    }
                }
                outline.segments().any(|s| s.nearest(q, 0.01).distance_sq <= PATH_HIT * PATH_HIT)
            }
        }
    }
}

/// A path node's outline: every contour, in the path's space
/// (`iron_document::contour`).
pub fn path_outline(store: &Store, id: NodeId) -> kurbo::BezPath {
    iron_document::contour::outline(&iron_document::contour::contours(store, id))
}

/// A `stroke` component's style as kurbo strokes: Iron's names are SVG's.
pub fn stroke_style(store: &Store, id: NodeId) -> kurbo::Stroke {
    let r = Reader { store, id };
    let word = |leaf: &str, default: &str| r.str(leaf).unwrap_or_else(|| default.to_owned());
    let join = match word("stroke.join", "round").as_str() {
        "miter" => kurbo::Join::Miter,
        "bevel" => kurbo::Join::Bevel,
        _ => kurbo::Join::Round,
    };
    let cap = match word("stroke.cap", "round").as_str() {
        "butt" => kurbo::Cap::Butt,
        "square" => kurbo::Cap::Square,
        _ => kurbo::Cap::Round,
    };
    let dash: Vec<f64> = word("stroke.dash", "").split(|c: char| c == ',' || c.is_whitespace()).filter_map(|v| v.parse().ok()).collect();
    let solid = dash.is_empty() || dash.iter().any(|&d| d < 0.0 || !d.is_finite()) || dash.iter().all(|&d| d == 0.0);
    let style = kurbo::Stroke::new(r.num("stroke.width", 1.0) as f64).with_join(join).with_caps(cap).with_miter_limit(r.num("stroke.miter", 4.0) as f64);
    if solid { style } else { style.with_dashes(r.num("stroke.dash_offset", 0.0) as f64, dash) }
}

/// Marker for materialised 3D nodes.
#[derive(Component, Debug, Clone, Copy)]
pub struct Doc3d;

/// Shared handles for `mesh` nodes. Meshes are looked up by the node's
/// `mesh.asset` string; only `torus` exists today.
#[derive(Resource)]
pub struct MeshAssets {
    pub torus: Handle<Mesh>,
    pub torus_bounds: Cuboid,
    pub material: Handle<StandardMaterial>,
}

pub(super) fn setup_mesh_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let material = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(uv_debug_texture())),
        ..default()
    });
    let torus = meshes.add(Torus::default().mesh().major_resolution(8).minor_resolution(6));
    commands.insert_resource(MeshAssets {
        torus,
        torus_bounds: Cuboid::from_size(Vec3::new(1.75, 0.52, 1.75)),
        material,
    });
}

pub(super) fn reconcile(
    store: &Store,
    affected: &BTreeSet<NodeId>,
    map: &mut NodeMap,
    layers: &mut CompositionLayers,
    commands: &mut Commands,
    assets: &MeshAssets,
) {
    let doc = store.document();
    let _ = ComponentKind::ALL; // keep the import meaningful for readers
    // A contour or a point has no entity of its own: its path is redrawn
    // instead. Tombstoned nodes still name their parent, so a deleted point
    // finds its path.
    let parts = ["point", "contour"].map(|t| TypeId::lookup(t).expect("registered"));
    let path_ty = TypeId::lookup("path").expect("registered");
    let mut with_paths = affected.clone();
    for &id in affected {
        let mut cur = doc.node(id).filter(|n| parts.contains(&n.ty)).and_then(|n| n.parent);
        while let Some(p) = cur.and_then(|p| doc.node(p)) {
            if p.ty == path_ty {
                with_paths.insert(p.id);
                break;
            }
            cur = p.parent;
        }
    }
    let affected = &with_paths;

    // Pass 1: existence and values. Hierarchy waits until every entity exists.
    for &id in affected {
        match (doc.live(id), map.entity(id)) {
            (Some(_), None) => {
                if drawable(doc, id)
                    && let Some(e) = spawn(store, id, layers, commands, assets)
                {
                    map.insert(id, e);
                }
            }
            (None, Some(_)) => {
                let e = map.remove(id).expect("checked");
                if let Ok(mut ec) = commands.get_entity(e) {
                    ec.try_despawn();
                }
            }
            (Some(_), Some(e)) => {
                if let Ok(mut ec) = commands.get_entity(e) {
                    write_values(store, id, layers, &mut ec, assets);
                }
            }
            (None, None) => {}
        }
    }

    // Pass 2: parent links, for every touched node that is materialised.
    for &id in affected {
        let (Some(node), Some(e)) = (doc.live(id), map.entity(id)) else { continue };
        let Ok(mut ec) = commands.get_entity(e) else { continue };
        match node.parent.and_then(|p| map.entity(p)) {
            Some(pe) => {
                ec.insert(ChildOf(pe));
            }
            None => {
                ec.remove::<ChildOf>();
            }
        }
    }
}

/// A node under a `library` is a definition's template, shown only when
/// instantiated; nothing in a library is materialised.
fn drawable(doc: &iron_document::Document, id: NodeId) -> bool {
    let library = TypeId::lookup("library").expect("registered");
    let mut cur = Some(id);
    while let Some(n) = cur.and_then(|c| doc.live(c)) {
        if n.ty == library {
            return false;
        }
        cur = n.parent;
    }
    true
}

/// Spawn the entity for a live node, or `None` for node types that have no
/// runtime form (clip, timeline, entity, the library types).
fn spawn(store: &Store, id: NodeId, layers: &mut CompositionLayers, commands: &mut Commands, assets: &MeshAssets) -> Option<Entity> {
    let node = store.document().live(id)?;
    let mut ec = match node.ty.name() {
        // An embed is placed like a group; what it shows is drawn through it
        // (`nest`), never parented to it.
        // An embed inside a 3D composition is a plane in it (ECM §7.4): 3D
        // content the composition's cameras see; `planes` gives it its mesh
        // and texture.
        "embed" if in_3d(store, id) => commands.spawn((Provenance(id), Doc3d, ActiveState::default(), Name::new(format!("embed plane {id}")), Transform::default(), Visibility::default())),
        "group" | "embed" => commands.spawn((Provenance(id), Name::new(format!("{} {id}", node.ty)), Visibility::default())),
        "rect" | "ellipse" | "slider" | "path" => {
            commands.spawn((Provenance(id), Name::new(format!("{} {id}", node.ty)), Visibility::default()))
        }
        // A camera node is a pose: embeds that name it look through it.
        "camera" => commands.spawn((Provenance(id), Name::new(format!("camera {id}")), Transform::default(), Visibility::default())),
        "mesh" => {
            let bounds = assets.torus_bounds;
            commands.spawn((
                Provenance(id),
                Doc3d,
                Name::new(format!("mesh {id}")),
                Mesh3d(assets.torus.clone()),
                MeshMaterial3d(assets.material.clone()),
                Shape::Box(bounds),
                ActiveState::default(),
            ))
        }
        _ => return None,
    };
    write_values(store, id, layers, &mut ec, assets);
    Some(ec.id())
}

/// Rewrite every derived component from the node's current *resolved* values:
/// a bound slot contributes what its binding evaluates to, a previewed slot
/// what the gesture says. Authored constants are read the same way.
fn write_values(store: &Store, id: NodeId, layers: &mut CompositionLayers, ec: &mut EntityCommands, assets: &MeshAssets) {
    let Some(node) = store.document().live(id) else { return };
    let r = Reader { store, id };
    // Which panel draws it: 2D panels filter on this; 3D content goes on its
    // composition's render layer, which only that composition's cameras see.
    let composition = iron_document::composition::composition_of(store.document(), id);
    ec.insert(InComposition(composition));
    match node.ty.name() {
        "embed" if in_3d(store, id) => {
            ec.insert((r.transform_3d(), RenderLayers::layer(layers.of(composition))));
        }
        "group" | "embed" => {
            ec.insert(r.transform_2d());
        }
        "rect" | "ellipse" | "slider" | "path" => {
            ec.insert(r.transform_2d());
            let shape = match node.ty.name() {
                "ellipse" => {
                    // Present, or previewed by a gesture that will add it.
                    let arc = ["arc.start", "arc.sweep", "arc.inner"].iter().any(|l| r.get(l).is_some()).then(|| [r.num("arc.start", 0.0), r.num("arc.sweep", std::f32::consts::TAU), r.num("arc.inner", 0.0)]);
                    Shape2d::Ellipse { rx: r.num("radius.rx", 50.0), ry: r.num("radius.ry", 50.0), arc }
                }
                "path" => {
                    use iron_document::contour;
                    let contours = contour::contours(store, id);
                    let fill = contour::fill_outline(&contours);
                    let fill = (!fill.elements().is_empty() && (r.color("fill.color").is_some() || r.color("gradient.start").is_some())).then(|| std::sync::Arc::new(fill));
                    let even_odd = contour::fill_rule(store, id) == contour::FillRule::EvenOdd;
                    Shape2d::Path { outline: std::sync::Arc::new(contour::outline(&contours)), fill, even_odd }
                }
                "slider" => {
                    let (min, max) = (r.num("slider.min", 0.0), r.num("slider.max", 1.0));
                    let value = r.num("slider.value", 0.0);
                    let t = if max > min { ((value - min) / (max - min)).clamp(0.0, 1.0) } else { 0.0 };
                    Shape2d::Slider { w: r.num("size.w", 200.0), h: r.num("size.h", 24.0), t }
                }
                _ => Shape2d::Rect { w: r.num("size.w", 100.0), h: r.num("size.h", 100.0), radii: ["tl", "tr", "br", "bl"].map(|c| r.num(&format!("corners.{c}"), 0.0)) },
            };
            let fill = r.color("fill.color").map(|mut c| {
                c[3] *= r.num("fill.opacity", 1.0);
                c
            });
            let stroke = r.color("stroke.color").map(|c| (c, stroke_style(store, id)));
            let shadow = r.color("shadow.color").map(|color| Shadow2d { color, dx: r.num("shadow.dx", 0.0), dy: r.num("shadow.dy", 4.0), std_dev: r.num("shadow.blur", 4.0).max(0.0) * 0.5 });
            ec.insert(Doc2d { shape, fill, stroke, shadow });
        }
        "camera" => {
            ec.insert(r.transform_3d());
        }
        "mesh" => {
            ec.insert(r.transform_3d());
            let asset = r.str("mesh.asset").unwrap_or_else(|| "torus".to_owned());
            if asset != "torus" {
                warn!("{id}: unknown mesh asset {asset:?}; showing torus");
            }
            ec.insert((Mesh3d(assets.torus.clone()), RenderLayers::layer(layers.of(composition))));
        }
        _ => {}
    }
}

/// Resolved-value reads for one node, with defaults for absent components.
struct Reader<'a> {
    store: &'a Store,
    id: NodeId,
}

impl Reader<'_> {
    fn get(&self, leaf: &str) -> Option<Value> {
        let path: LeafPath = leaf.parse().expect("declared leaf");
        self.store.resolved(self.id, path)
    }

    fn num(&self, leaf: &str, default: f32) -> f32 {
        self.get(leaf).and_then(|v| v.as_f64()).map(|n| n as f32).unwrap_or(default)
    }

    fn color(&self, leaf: &str) -> Option<[f32; 4]> {
        self.get(leaf).and_then(|v| v.as_color())
    }

    fn str(&self, leaf: &str) -> Option<String> {
        self.get(leaf).and_then(|v| v.as_str().map(str::to_owned))
    }

    fn vec3(&self, leaf: &str, default: Vec3) -> Vec3 {
        self.get(leaf)
            .and_then(|v| v.as_vec3())
            .map(|[x, y, z]| Vec3::new(x as f32, y as f32, z as f32))
            .unwrap_or(default)
    }

    /// Document 2D space is y-down with the origin at the viewer panel's
    /// top-left; the entity transform is that space verbatim, and the layer
    /// renderer applies the panel offset when it emits commands.
    fn transform_2d(&self) -> Transform {
        if self.store.document().component(self.id, ComponentKind::Transform2d).is_none() {
            return Transform::IDENTITY;
        }
        Transform {
            translation: Vec3::new(self.num("transform2d.x", 0.0), self.num("transform2d.y", 0.0), 0.0),
            rotation: Quat::from_rotation_z(self.num("transform2d.rot", 0.0)),
            scale: Vec3::new(self.num("transform2d.sx", 1.0), self.num("transform2d.sy", 1.0), 1.0),
        }
    }

    fn transform_3d(&self) -> Transform {
        if self.store.document().component(self.id, ComponentKind::Transform3d).is_none() {
            return Transform::IDENTITY;
        }
        let r = self.vec3("transform3d.rot", Vec3::ZERO);
        Transform {
            translation: self.vec3("transform3d.pos", Vec3::ZERO),
            rotation: Quat::from_euler(EulerRot::XYZ, r.x, r.y, r.z),
            scale: self.vec3("transform3d.scale", Vec3::ONE),
        }
    }
}

fn uv_debug_texture() -> Image {
    const TEXTURE_SIZE: usize = 8;
    let mut palette: [u8; 32] = [
        255, 102, 159, 255, 255, 159, 102, 255, 236, 255, 102, 255, 121, 255, 102, 255, 102, 255, 198, 255, 102, 198,
        255, 255, 121, 102, 255, 255, 236, 102, 255, 255,
    ];
    let mut texture_data = [0; TEXTURE_SIZE * TEXTURE_SIZE * 4];
    for y in 0..TEXTURE_SIZE {
        let offset = TEXTURE_SIZE * y * 4;
        texture_data[offset..(offset + TEXTURE_SIZE * 4)].copy_from_slice(&palette);
        palette.rotate_right(4);
    }
    Image::new_fill(
        Extent3d { width: TEXTURE_SIZE as u32, height: TEXTURE_SIZE as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &texture_data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// Whether a node sits in a 3D composition.
fn in_3d(store: &Store, id: NodeId) -> bool {
    use iron_document::composition::{self, Dims};
    composition::composition_of(store.document(), id).and_then(|c| composition::dims(store.document(), c)) == Some(Dims::Three)
}
