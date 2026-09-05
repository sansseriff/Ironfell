//! Compositing material for backends that render into their own texture.
//!
//! # Why this exists rather than a plain `Sprite`
//!
//! `vello_hybrid` renders with `BlendState::PREMULTIPLIED_ALPHA_BLENDING` and
//! stores colours premultiplied, so its output texture holds **premultiplied**
//! RGBA. Bevy's 2D pipeline blends with `BlendState::ALPHA_BLENDING`, which is
//! *straight* alpha — it multiplies the source by alpha again on the way in:
//!
//! ```text
//!   correct   dst = src        + dst · (1 − α)
//!   straight  dst = src · α    + dst · (1 − α)
//! ```
//!
//! Where α is 1 the two agree, which is why flat interiors looked perfect and
//! only antialiased boundaries were wrong: every partial-coverage pixel came out
//! too dark and too thin, so edges read as if a sharpening filter had been
//! applied. A few pixels wide, present on every shape, invisible in a diff of
//! solid areas.
//!
//! Overriding the blend state requires a `Material2d`, so this is the smallest
//! material that does it. `bevy_vello` reaches the same conclusion for the same
//! reason and sets its own blend state.
//!
//! # Why the shader un-premultiplies before converting
//!
//! The texture holds premultiplied colour that is *also* sRGB-encoded. sRGB
//! decode is nonlinear, so decoding a premultiplied value is not the same as
//! decoding the colour and then premultiplying:
//!
//! ```text
//!   decode(C · α)  ≠  decode(C) · α        for α < 1
//! ```
//!
//! Letting the hardware decode an sRGB-typed texture therefore darkens exactly
//! the partial-coverage pixels — at α = 0.24 the source contribution lands
//! roughly 5× too low — while leaving α = 1 interiors perfect. The visible
//! result is that every antialiased boundary loses most of its colour and the
//! shape looks as though a sharpening filter has been applied.
//!
//! So the texture is sampled as UNORM (no hardware decode) and the shader
//! un-premultiplies, converts, and re-premultiplies. `bevy_vello` can decode
//! directly because vello *classic* emits straight alpha; sparse strips does
//! not.
//!
//! # Why the mesh is in clip space
//!
//! The vertex shader passes positions through untouched, and the fragment shader
//! derives its UV from the fragment coordinate and the viewport. The composite is
//! therefore an exact 1:1 texel-to-pixel blit by construction, rather than
//! something that happens to line up if the camera projection, the window scale
//! factor and the quad size all agree. That removes sub-pixel misalignment as a
//! possible cause of edge artifacts instead of leaving it to be ruled out later.

use bevy::asset::{Asset, RenderAssetUsages, uuid_handle};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{AsBindGroup, BlendState, RenderPipelineDescriptor};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPlugin};

const COMPOSITE_SHADER: Handle<Shader> = uuid_handle!("4f2f0a52-9d61-4f0e-9a3e-6f3f9a1c77b1");

const COMPOSITE_WGSL: &str = r#"
#import bevy_render::view::{View, frag_coord_to_uv}
#import bevy_sprite::mesh2d_vertex_output::VertexOutput

@group(0) @binding(0) var<uniform> view: View;
@group(2) @binding(0) var composite_texture: texture_2d<f32>;
@group(2) @binding(1) var composite_sampler: sampler;

struct Vertex {
    @location(0) position: vec3<f32>,
};

@vertex
fn vertex(in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    // Already clip-space: no view or model transform, so one texel maps to one
    // pixel exactly.
    out.position = vec4<f32>(in.position, 1.0);
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
    let uv = frag_coord_to_uv(in.position.xy, view.viewport);
    // UNORM texture: these are raw sRGB-encoded, premultiplied bytes.
    let c = textureSample(composite_texture, composite_sampler, uv);
    if (c.a <= 0.0) {
        return vec4(0.0);
    }
    // Un-premultiply, convert in the space the values are actually in, then
    // re-premultiply for the premultiplied blend state.
    let straight = linear_from_srgb(c.rgb / c.a);
    return vec4(straight * c.a, c.a);
}
"#;

/// Draws a backend's offscreen output over the scene.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct VectorCompositeMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub texture: Handle<Image>,
}

impl Material2d for VectorCompositeMaterial {
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(COMPOSITE_SHADER)
    }

    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(COMPOSITE_SHADER)
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        // The entire point of this material. `AlphaMode2d::Blend` gets us onto
        // the alpha-blended phase; this corrects which blend that phase uses.
        if let Some(fragment) = descriptor.fragment.as_mut()
            && let Some(Some(target)) = fragment.targets.first_mut()
        {
            target.blend = Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        }
        Ok(())
    }
}

pub(super) struct CompositeMaterialPlugin;

impl Plugin for CompositeMaterialPlugin {
    fn build(&self, app: &mut App) {
        // Inlined rather than loaded from `assets/`: it is a dozen lines that
        // must stay in lockstep with this file, and shipping it as a loose asset
        // would mean a missing-file failure at runtime instead of at build time.
        let _ = app
            .world_mut()
            .resource_mut::<Assets<Shader>>()
            .insert(
                COMPOSITE_SHADER.id(),
                Shader::from_wgsl(COMPOSITE_WGSL, file!()),
            );
        app.add_plugins(Material2dPlugin::<VectorCompositeMaterial>::default());
    }
}

/// A quad covering clip space, for use with [`VectorCompositeMaterial`].
pub(super) fn fullscreen_quad() -> Mesh {
    use bevy::mesh::{Indices, PrimitiveTopology};

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
        ],
    );
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}
