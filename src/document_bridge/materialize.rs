//! Node → entity. One function per node type, and a reconcile pass that
//! spawns, despawns, or rewrites the entities for a set of touched nodes.

use super::{NodeMap, Provenance};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use iron_document::component::{Fill, Mesh as MeshComp, Radius, Size, Stroke, Transform2d, Transform3d};
use iron_document::{Document, NodeId, Store};
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
}

impl Shape2d {
    pub fn contains_local(&self, p: Vec2) -> bool {
        match *self {
            Shape2d::Rect { w, h } => p.x >= 0.0 && p.y >= 0.0 && p.x <= w && p.y <= h,
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

    // Pass 1: existence and values. Hierarchy waits until every entity exists.
    for &id in affected {
        match (doc.live(id), map.entity(id)) {
            (Some(_), None) => {
                if let Some(e) = spawn(doc, id, commands, assets) {
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
                    write_values(doc, id, &mut ec, assets);
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
fn spawn(doc: &Document, id: NodeId, commands: &mut Commands, assets: &MeshAssets) -> Option<Entity> {
    let node = doc.live(id)?;
    let mut ec = match node.ty.name() {
        "group" => commands.spawn((Provenance(id), Name::new(format!("group {id}")), Visibility::default())),
        "rect" | "circle" => commands.spawn((Provenance(id), Name::new(format!("{} {id}", node.ty)), Visibility::default())),
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
    write_values(doc, id, &mut ec, assets);
    Some(ec.id())
}

/// Rewrite every derived component from the node's current authored values.
fn write_values(doc: &Document, id: NodeId, ec: &mut EntityCommands, assets: &MeshAssets) {
    let Some(node) = doc.live(id) else { return };
    match node.ty.name() {
        "group" => {
            ec.insert(transform_2d(doc.get::<Transform2d>(id)));
        }
        "rect" | "circle" => {
            ec.insert(transform_2d(doc.get::<Transform2d>(id)));
            let shape = if node.ty.name() == "rect" {
                let s = doc.get::<Size>(id);
                Shape2d::Rect { w: num(s.map(|s| &s.w), 100.0), h: num(s.map(|s| &s.h), 100.0) }
            } else {
                Shape2d::Circle { r: num(doc.get::<Radius>(id).map(|r| &r.r), 50.0) }
            };
            let fill = doc.get::<Fill>(id).map(|f| {
                let mut c = color(&f.color);
                c[3] *= num(Some(&f.opacity), 1.0);
                c
            });
            let stroke = doc.get::<Stroke>(id).map(|s| (color(&s.color), num(Some(&s.width), 1.0)));
            ec.insert(Doc2d { shape, fill, stroke });
        }
        "mesh" => {
            ec.insert(transform_3d(doc.get::<Transform3d>(id)));
            let asset = doc.get::<MeshComp>(id).and_then(|m| m.asset.constant()).and_then(|v| v.as_str()).unwrap_or("torus");
            if asset != "torus" {
                warn!("{id}: unknown mesh asset {asset:?}; showing torus");
            }
            ec.insert(Mesh3d(assets.torus.clone()));
        }
        _ => {}
    }
}

fn num(slot: Option<&iron_document::Slot>, default: f32) -> f32 {
    slot.and_then(|s| s.constant()).and_then(|v| v.as_f64()).map(|n| n as f32).unwrap_or(default)
}

fn color(slot: &iron_document::Slot) -> [f32; 4] {
    slot.constant().and_then(|v| v.as_color()).unwrap_or([0.5, 0.5, 0.5, 1.0])
}

fn vec3(slot: &iron_document::Slot, default: Vec3) -> Vec3 {
    slot.constant().and_then(|v| v.as_vec3()).map(|[x, y, z]| Vec3::new(x as f32, y as f32, z as f32)).unwrap_or(default)
}

/// Document 2D space is y-down with the origin at the viewer panel's top-left;
/// the entity transform is that space verbatim, and the layer renderer applies
/// the panel offset when it emits commands.
pub fn transform_2d(t: Option<&Transform2d>) -> Transform {
    let Some(t) = t else { return Transform::IDENTITY };
    Transform {
        translation: Vec3::new(num(Some(&t.x), 0.0), num(Some(&t.y), 0.0), 0.0),
        rotation: Quat::from_rotation_z(num(Some(&t.rot), 0.0)),
        scale: Vec3::new(num(Some(&t.sx), 1.0), num(Some(&t.sy), 1.0), 1.0),
    }
}

pub fn transform_3d(t: Option<&Transform3d>) -> Transform {
    let Some(t) = t else { return Transform::IDENTITY };
    let r = vec3(&t.rot, Vec3::ZERO);
    Transform {
        translation: vec3(&t.pos, Vec3::ZERO),
        rotation: Quat::from_euler(EulerRot::XYZ, r.x, r.y, r.z),
        scale: vec3(&t.scale, Vec3::ONE),
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
