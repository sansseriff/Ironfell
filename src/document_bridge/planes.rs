//! 2D compositions inside 3D ones: a textured plane (ECM §7.4,
//! plans/specs/view-textures.md migration step 4).
//!
//! An embed of a 2D composition placed in a 3D composition is a plane. Its
//! extent is the embed's frame, or the bounds of the composition's content,
//! at [`PX_PER_UNIT`] document px per world unit; the content's top-left sits
//! at the embed's origin, x to the right and y down the plane, facing +Z.
//!
//! The content renders into an off-screen vector view: the same draw walk as
//! a panel (highlights, embeds and all), into a texture whose density is
//! derived from how large the plane appears. Each camera that sees the plane
//! projects its corners; the largest on-screen texels-per-px wins, stepped in
//! powers of √2 so a slow orbit does not reallocate every frame. When the
//! texture's content changes, the 3D views that show the plane wake up.

use super::{DocumentStore, InComposition, nest};
use crate::vector::{DisplayList, Offscreen, VectorLayer, VectorView, ViewTarget2d, order};
use bevy::asset::{RenderAssetUsages, uuid_handle};
use bevy::camera::visibility::RenderLayers;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;
use iron_document::NodeId;
use iron_document::composition::{self, Dims};

/// Document px per world unit on a plane.
pub const PX_PER_UNIT: f64 = 100.0;
/// Bounds on texels per document px, and the largest texture side.
const MIN_DENSITY: f64 = 0.25;
const MAX_DENSITY: f64 = 4.0;
const MAX_SIDE: u32 = 4096;

const PLANE_SHADER: Handle<Shader> = uuid_handle!("2a7c9e14-5b3d-4f6a-8e21-c0d9b7a4f512");

// WESL (Bevy 0.20).
const PLANE_WGSL: &str = r#"
import bevy_pbr::render::forward_io::VertexOutput;

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var plane_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var plane_sampler: sampler;
// xy: the part of the texture the content fills (it is bucket-sized).
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var<uniform> uv_scale: vec4<f32>;

fn linear_from_srgb(srgb: vec3<f32>) -> vec3<f32> {
    return select(srgb / 12.92, pow((srgb + 0.055) / 1.055, vec3(2.4)), srgb > vec3(0.04045));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Vello's bytes: premultiplied and sRGB-encoded, in a UNORM texture.
    let c = textureSample(plane_texture, plane_sampler, in.uv * uv_scale.xy);
    if (c.a <= 0.0) {
        discard;
    }
    let straight = linear_from_srgb(c.rgb / c.a);
    return vec4(straight * c.a, c.a);
}
"#;

/// A 2D composition's view texture on a plane: unlit (a diagram should look
/// as it does in 2D), double-sided, premultiplied.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct PlaneMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub texture: Handle<Image>,
    #[uniform(2)]
    pub uv_scale: Vec4,
}

impl Material for PlaneMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(PLANE_SHADER)
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

pub(super) struct PlanePlugin;

impl Plugin for PlanePlugin {
    fn build(&self, app: &mut App) {
        let _ = app.world_mut().resource_mut::<Assets<Shader>>().insert(PLANE_SHADER.id(), Shader::from_wesl(PLANE_WGSL, file!()));
        app.add_plugins(MaterialPlugin::<PlaneMaterial>::default());
    }
}

/// On the 3D entity of an embed that shows a 2D composition as a plane.
#[derive(Component, Debug)]
pub struct EmbedPlane {
    /// The off-screen 2D view its content renders into.
    pub view: Entity,
    pub source: NodeId,
    /// The part of the composition shown, document px.
    pub region: kurbo::Rect,
    /// Texels per document px.
    pub density: f64,
    material: Handle<PlaneMaterial>,
    mesh_region: kurbo::Rect,
}

impl EmbedPlane {
    /// A point on the plane (its local space, world units) as document px
    /// in the composition, if it falls inside the region.
    pub fn plane_to_doc(&self, local: Vec3) -> Option<kurbo::Point> {
        let p = kurbo::Point::new(self.region.x0 + local.x as f64 * PX_PER_UNIT, self.region.y0 - local.y as f64 * PX_PER_UNIT);
        self.region.contains(p).then_some(p)
    }
}

/// The off-screen view of a plane: which composition, what region, at what
/// density. The 2D layer draws it (`render_document_layer`).
#[derive(Component, Debug, Clone, PartialEq)]
pub struct PlaneLayer {
    pub embed: NodeId,
    pub source: NodeId,
    pub region: kurbo::Rect,
    pub density: f64,
}

impl PlaneLayer {
    /// Composition px → texture px.
    pub fn affine(&self) -> kurbo::Affine {
        kurbo::Affine::scale(self.density) * kurbo::Affine::translate((-self.region.x0, -self.region.y0))
    }
}

fn plane_mesh(region: kurbo::Rect) -> Mesh {
    use bevy::mesh::{Indices, PrimitiveTopology};
    let (w, h) = ((region.width() / PX_PER_UNIT) as f32, (region.height() / PX_PER_UNIT) as f32);
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0, 0.0, 0.0], [w, 0.0, 0.0], [w, -h, 0.0], [0.0, -h, 0.0]]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    mesh.insert_indices(Indices::U32(vec![0, 2, 1, 0, 3, 2]));
    mesh
}

/// Texels per px the plane needs: across the cameras that see it, the largest
/// on-screen length of one document px along either axis, at any corner.
fn density_needed(
    cameras: &Query<(&Camera, &GlobalTransform, &RenderLayers)>,
    layers: &RenderLayers,
    global: &GlobalTransform,
    region: kurbo::Rect,
) -> Option<f64> {
    let (w, h) = ((region.width() / PX_PER_UNIT) as f32, (region.height() / PX_PER_UNIT) as f32);
    let corners = [Vec3::ZERO, Vec3::new(w, 0.0, 0.0), Vec3::new(w, -h, 0.0), Vec3::new(0.0, -h, 0.0)].map(|c| global.transform_point(c));
    let mut best: Option<f64> = None;
    for (camera, cam_t, cam_layers) in cameras {
        // Every camera that renders this composition counts, asleep or not:
        // a retained view still shows the plane at its last density.
        if !cam_layers.intersects(layers) {
            continue;
        }
        let Some(px) = corners.iter().map(|&c| camera.world_to_viewport(cam_t, c).ok()).collect::<Option<Vec<Vec2>>>() else { continue };
        // Edge lengths on screen over edge lengths in document px.
        let along_x = px[0].distance(px[1]).max(px[3].distance(px[2])) as f64 / region.width().max(1.0);
        let along_y = px[0].distance(px[3]).max(px[1].distance(px[2])) as f64 / region.height().max(1.0);
        let d = along_x.max(along_y);
        best = Some(best.map_or(d, |b: f64| b.max(d)));
    }
    best
}

/// Round a density up to a power of √2 (so small changes reuse the texture).
fn step(d: f64) -> f64 {
    let k = (d.clamp(MIN_DENSITY, MAX_DENSITY).ln() / std::f64::consts::SQRT_2.ln()).ceil();
    std::f64::consts::SQRT_2.powf(k).clamp(MIN_DENSITY, MAX_DENSITY)
}

/// Keep every plane's view, density, mesh and material current.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_planes(
    mut commands: Commands,
    store: Res<DocumentStore>,
    spatial: Res<super::SpatialScene>,
    cameras: Query<(&Camera, &GlobalTransform, &RenderLayers)>,
    mut planes: Query<(Entity, &crate::document_bridge::Provenance, &GlobalTransform, &RenderLayers, Option<&mut EmbedPlane>), With<InComposition>>,
    mut views: Query<(Entity, &mut PlaneLayer, &mut VectorView, Option<&ViewTarget2d>, Ref<DisplayList>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PlaneMaterial>>,
    mut active: Query<&mut crate::bevy_app::ActiveStateMarker>,
) {
    let doc = store.0.document();
    for (entity, prov, global, layers, plane) in &mut planes {
        let id = prov.0;
        if !nest::is_embed(doc, id) {
            continue;
        }
        let Some(source) = composition::embedded(doc, id).filter(|&s| composition::dims(doc, s) == Some(Dims::Two)) else { continue };
        let region = match doc.get::<iron_document::facet::Frame>(id) {
            Some(f) => {
                let n = |s: &iron_document::Slot| s.constant().and_then(|v| v.as_f64()).unwrap_or(0.0);
                kurbo::Rect::new(n(&f.x0), n(&f.y0), n(&f.x1), n(&f.y1))
            }
            None => nest::content_bounds(&spatial, source).unwrap_or(kurbo::Rect::new(0.0, 0.0, 100.0, 100.0)),
        };
        if region.width() < 1.0 || region.height() < 1.0 {
            continue;
        }
        match plane {
            None => {
                // A new plane: its view (drawn by the 2D layer), mesh and
                // material; the texture arrives once the view has one.
                let density = 1.0;
                let layer = PlaneLayer { embed: id, source, region, density };
                let size = UVec2::new(((region.width() * density).ceil() as u32).clamp(1, MAX_SIDE), ((region.height() * density).ceil() as u32).clamp(1, MAX_SIDE));
                let view = commands.spawn((DisplayList::default(), VectorView { pos: UVec2::ZERO, size }, Offscreen, layer, Name::new(format!("plane view {id}")))).id();
                commands.entity(view).insert(VectorLayer::screen(order::DOCUMENT_2D).in_view(view));
                let material = materials.add(PlaneMaterial { texture: Handle::default(), uv_scale: Vec4::ONE });
                commands.entity(entity).insert((
                    Mesh3d(meshes.add(plane_mesh(region))),
                    MeshMaterial3d(material.clone()),
                    EmbedPlane { view, source, region, density, material, mesh_region: region },
                ));
            }
            Some(mut plane) => {
                let Ok((_, mut layer, mut vview, target, list)) = views.get_mut(plane.view) else { continue };
                // Density from how large it appears; kept when nothing sees it.
                let density = density_needed(&cameras, layers, global, region).map(step).unwrap_or(plane.density);
                if plane.region != region || plane.density != density || plane.source != source {
                    plane.region = region;
                    plane.density = density;
                    plane.source = source;
                    let next = PlaneLayer { embed: id, source, region, density };
                    if *layer != next {
                        *layer = next;
                    }
                    let size = UVec2::new(((region.width() * density).ceil() as u32).clamp(1, MAX_SIDE), ((region.height() * density).ceil() as u32).clamp(1, MAX_SIDE));
                    if vview.size != size {
                        vview.size = size;
                    }
                }
                if plane.mesh_region != region {
                    plane.mesh_region = region;
                    commands.entity(entity).insert(Mesh3d(meshes.add(plane_mesh(region))));
                }
                if let Some(t) = target {
                    let uv = Vec4::new(vview.size.x as f32 / t.bucket.x as f32, vview.size.y as f32 / t.bucket.y as f32, 0.0, 0.0);
                    let stale = materials.get(&plane.material).is_some_and(|m| m.texture != t.image || m.uv_scale != uv);
                    if stale && let Some(mut m) = materials.get_mut(&plane.material) {
                        m.texture = t.image.clone();
                        m.uv_scale = uv;
                    }
                }
                // New content on the plane: the 3D views showing it re-render.
                if list.is_changed()
                    && let Ok(mut a) = active.get_mut(entity)
                {
                    a.set_changed();
                }
            }
        }
    }
    // Views of planes that went away.
    let live: Vec<Entity> = planes.iter().filter_map(|(.., p)| p.map(|p| p.view)).collect();
    for (e, ..) in views.iter() {
        if !live.contains(&e) {
            commands.entity(e).despawn();
        }
    }
}
