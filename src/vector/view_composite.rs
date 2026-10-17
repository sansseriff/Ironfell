//! Pasting a view's texture into a rectangle of the window, texel for pixel
//! (plans/specs/view-textures.md §2).
//!
//! The generalisation of [`super::composite_material`]: that one blits a
//! window-sized texture over the whole window; this one blits the top-left
//! `size` texels of a (bucket-sized) texture into a window rectangle. Both
//! derive the UV from the fragment coordinate, so the mapping is exact by
//! construction rather than by agreement between a camera, a quad and a
//! scale factor.
//!
//! Two encodings arrive here, and blending must treat them differently:
//!
//! - `MODE_PREMULTIPLIED_RAW`: `vello_hybrid` output, premultiplied and
//!   sRGB-encoded, in a UNORM texture (see `composite_material` for why the
//!   shader un-premultiplies before converting).
//! - `MODE_STRAIGHT_SRGB`: a Bevy camera's output in an sRGB texture, so the
//!   sampler already decodes to linear; straight alpha, premultiplied here.

use bevy::asset::{Asset, RenderAssetUsages, uuid_handle};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{AsBindGroup, BlendState, RenderPipelineDescriptor};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPlugin};

pub const MODE_PREMULTIPLIED_RAW: f32 = 0.0;
pub const MODE_STRAIGHT_SRGB: f32 = 1.0;

const VIEW_COMPOSITE_SHADER: Handle<Shader> = uuid_handle!("8c1d7f0e-3b5a-4e8f-9d2c-6a1b4c7e9f30");

// WESL (Bevy 0.20); the vertex output is this shader's own.
const VIEW_COMPOSITE_WGSL: &str = r#"
import bevy_render::view::View;

@group(0) @binding(0) var<uniform> view: View;
@group(2) @binding(0) var view_texture: texture_2d<f32>;
@group(2) @binding(1) var view_sampler: sampler;
// rect: x, y, w, h of the window rectangle (physical px).
@group(2) @binding(2) var<uniform> rect: vec4<f32>;
// params: texture width, texture height, mode, unused.
@group(2) @binding(3) var<uniform> params: vec4<f32>;

struct Vertex {
    @location(0) position: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vertex(in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    // `position` is a unit square; place it over the rectangle in clip space.
    let px = rect.xy + in.position.xy * rect.zw;
    let ndc = (px - view.viewport.xy) / view.viewport.zw * 2.0 - 1.0;
    out.position = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    return out;
}

fn linear_from_srgb(srgb: vec3<f32>) -> vec3<f32> {
    return select(
        srgb / 12.92,
        pow((srgb + 0.055) / 1.055, vec3(2.4)),
        srgb > vec3(0.04045)
    );
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Texel for pixel: the fragment's offset into the rectangle, over the
    // texture's full size (the view fills only its top-left corner).
    let uv = (in.position.xy - rect.xy) / params.xy;
    let c = textureSample(view_texture, view_sampler, uv);
    if (c.a <= 0.0) {
        return vec4(0.0);
    }
    if (params.z < 0.5) {
        let straight = linear_from_srgb(c.rgb / c.a);
        return vec4(straight * c.a, c.a);
    }
    return vec4(c.rgb * c.a, c.a);
}
"#;

/// One view's texture, pasted into a window rectangle.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct ViewCompositeMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub texture: Handle<Image>,
    /// Window rectangle: x, y, w, h in physical px.
    #[uniform(2)]
    pub rect: Vec4,
    /// Texture width and height, mode, unused.
    #[uniform(3)]
    pub params: Vec4,
}

impl Material2d for ViewCompositeMaterial {
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(VIEW_COMPOSITE_SHADER)
    }

    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(VIEW_COMPOSITE_SHADER)
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(
        _pipeline: &bevy::sprite_render::Material2dPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        if let Some(fragment) = descriptor.fragment.as_mut()
            && let Some(Some(target)) = fragment.targets.first_mut()
        {
            target.blend = Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        }
        Ok(())
    }
}

pub(super) struct ViewCompositePlugin;

impl Plugin for ViewCompositePlugin {
    fn build(&self, app: &mut App) {
        let _ = app.world_mut().resource_mut::<Assets<Shader>>().insert(
            VIEW_COMPOSITE_SHADER.id(),
            Shader::from_wesl(VIEW_COMPOSITE_WGSL, file!()),
        );
        app.add_plugins(Material2dPlugin::<ViewCompositeMaterial>::default());
    }
}

/// The unit square the vertex shader places over a rectangle.
pub fn unit_quad() -> Mesh {
    use bevy::mesh::{Indices, PrimitiveTopology};
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]]);
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}

/// Texture sizes round up to 64 px, so small changes in a view's size (a
/// pane being dragged) reuse the texture (plans/specs/view-textures.md §3).
pub fn bucket(size: UVec2) -> UVec2 {
    UVec2::new(size.x.max(1).div_ceil(64) * 64, size.y.max(1).div_ceil(64) * 64)
}
