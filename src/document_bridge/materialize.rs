//! Node → entity. One function per node type, and a reconcile pass that
//! spawns, despawns, or rewrites the entities for a set of touched nodes.

use super::{NodeMap, Provenance};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use iron_document::{ComponentKind, LeafPath, NodeId, Store, Value};
use std::collections::BTreeSet;

use crate::bevy_app::scene3d::{ActiveState, Shape};

/// Drawable 2D node: what the document layer emits for it. Geometry is in the
/// node's local space; placement comes from the entity's `GlobalTransform`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Doc2d {
    pub shape: Shape2d,
    pub fill: Option<[f32; 4]>,
    /// Colour and width.
    pub stroke: Option<([f32; 4], f32)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape2d {
    /// Spans `(0, 0)` to `(w, h)`: the node origin is the top-left corner.
    Rect { w: f32, h: f32 },
    /// Centred on the node origin.
    Circle { r: f32 },
    /// A scalar-on-a-track control (doc 02 §9.3): a `w`×`h` track with the
    /// knob at fraction `t` along it.
    Slider { w: f32, h: f32, t: f32 },
}

impl Shape2d {
    pub fn contains_local(&self, p: Vec2) -> bool {
        match *self {
            Shape2d::Rect { w, h } | Shape2d::Slider { w, h, .. } => p.x >= 0.0 && p.y >= 0.0 && p.x <= w && p.y <= h,
            Shape2d::Circle { r } => p.length_squared() <= r * r,
        }
    }
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
    commands: &mut Commands,
    assets: &MeshAssets,
) {
    let doc = store.document();
    let _ = ComponentKind::ALL; // keep the import meaningful for readers

    // Pass 1: existence and values. Hierarchy waits until every entity exists.
    for &id in affected {
        match (doc.live(id), map.entity(id)) {
            (Some(_), None) => {
                if let Some(e) = spawn(store, id, commands, assets) {
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
                    write_values(store, id, &mut ec, assets);
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

/// Spawn the entity for a live node, or `None` for node types that have no
/// runtime form yet (slider, bar, clip, timeline).
fn spawn(store: &Store, id: NodeId, commands: &mut Commands, assets: &MeshAssets) -> Option<Entity> {
    let node = store.document().live(id)?;
    let mut ec = match node.ty.name() {
        "group" => commands.spawn((Provenance(id), Name::new(format!("group {id}")), Visibility::default())),
        "rect" | "circle" | "bar" | "slider" => {
            commands.spawn((Provenance(id), Name::new(format!("{} {id}", node.ty)), Visibility::default()))
        }
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
                RenderLayers::layer(0),
            ))
        }
        _ => return None,
    };
    write_values(store, id, &mut ec, assets);
    Some(ec.id())
}

/// Rewrite every derived component from the node's current *resolved* values:
/// a bound slot contributes what its binding evaluates to, a previewed slot
/// what the gesture says. Authored constants are read the same way.
fn write_values(store: &Store, id: NodeId, ec: &mut EntityCommands, assets: &MeshAssets) {
    let Some(node) = store.document().live(id) else { return };
    let r = Reader { store, id };
    match node.ty.name() {
        "group" => {
            ec.insert(r.transform_2d());
        }
        "rect" | "circle" | "bar" | "slider" => {
            ec.insert(r.transform_2d());
            let shape = match node.ty.name() {
                "circle" => Shape2d::Circle { r: r.num("radius.r", 50.0) },
                "slider" => {
                    let (min, max) = (r.num("slider.min", 0.0), r.num("slider.max", 1.0));
                    let value = r.num("slider.value", 0.0);
                    let t = if max > min { ((value - min) / (max - min)).clamp(0.0, 1.0) } else { 0.0 };
                    Shape2d::Slider { w: r.num("size.w", 200.0), h: r.num("size.h", 24.0), t }
                }
                _ => Shape2d::Rect { w: r.num("size.w", 100.0), h: r.num("size.h", 100.0) },
            };
            let fill = r.color("fill.color").map(|mut c| {
                c[3] *= r.num("fill.opacity", 1.0);
                c
            });
            let stroke = r.color("stroke.color").map(|c| (c, r.num("stroke.width", 1.0)));
            ec.insert(Doc2d { shape, fill, stroke });
        }
        "mesh" => {
            ec.insert(r.transform_3d());
            let asset = r.str("mesh.asset").unwrap_or_else(|| "torus".to_owned());
            if asset != "torus" {
                warn!("{id}: unknown mesh asset {asset:?}; showing torus");
            }
            ec.insert(Mesh3d(assets.torus.clone()));
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
