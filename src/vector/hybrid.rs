//! Sparse-strips backend, via `vello_hybrid`.
//!
//! The second implementation of the display-list seam, and the reason the seam
//! exists. Nothing in `src/bevy_app/` changed to add it.
//!
//! # How it differs from the classic backend
//!
//! Vello classic keeps a resolution-independent encoding and flattens paths on
//! the GPU. Vello Hybrid flattens, tiles and computes antialiasing coverage on
//! the **CPU**, in **device space**, and uses only vertex/fragment passes. Three
//! consequences drive the design here:
//!
//! - **One `Scene` for the whole view.** Every public `render()` clears its
//!   target, so layers cannot be composited as separate scenes. All layers merge
//!   into a single `Scene`, sorted by [`VectorLayer::order`] — which is why that
//!   ordering had to become explicit before this backend could exist.
//! - **The `Scene` is pixel-sized and screen-space.** That inverts the classic
//!   bias: screen layers pass straight through, and *world* layers are the ones
//!   needing a camera transform.
//! - **Rebuilding is the expensive part.** Regenerating strips for 10k+ paths is
//!   milliseconds. So the `Scene` is retained and only rebuilt when some layer's
//!   commands actually changed — roughly a 10x saving on idle frames, and the
//!   single most valuable behaviour in this file.
//!
//! # Where future caching plugs in
//!
//! [`emit_layer`] is the substitution point. Today every layer emits live
//! commands. A layer whose content is static and expensive is exactly the thing
//! that should instead be rasterized once into a texture and drawn as a single
//! quad via `Scene::draw_texture_rect`, which `vello_hybrid` 0.2 supports
//! against any externally-owned `wgpu::TextureView` — including a Bevy one.
//!
//! The structure needed to decide *which* layers deserve that already exists:
//! [`DisplayList::bounds`] gives the raster size and placement, and
//! [`VectorStats`] records how often each layer rebuilds and how many commands
//! it carries. A layer that is large, command-heavy and rarely rebuilt is a
//! rasterization candidate; one that rebuilds every frame is not. That decision
//! should be driven by those measurements rather than by guesswork, which is why
//! the instrumentation lands before the cache.
//!
//! Note the granularity this implies: **the layer is the natural cache unit**,
//! because it is already the unit of change detection, painter order and bounds.
//! A finer unit (per-shape) has no invalidation story; a coarser one (the whole
//! view) is what the tile cache in `plans/vision/01` describes and needs spatial
//! indexing this backend does not have.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::image::{Image, ImageSampler};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::{
    Extract, Render, RenderApp, RenderSystems,
    render_asset::RenderAssets,
    renderer::{RenderDevice, RenderQueue},
    texture::GpuImage,
};
use bevy::window::PrimaryWindow;
use std::sync::Mutex;
use kurbo::{Affine, BezPath, PathEl, Rect, Shape as _};
use peniko::Fill;
use vello_hybrid::{
    LayersConfig, MemorySettings, RenderSize, RenderSettings, RenderTargetConfig, Renderer,
    Resources, Scene, SizeU16,
};
use wgpu::TextureFormat;

use super::backend::{LayerSpace, VectorLayer};
use super::composite::{VECTOR_LAYER, VectorCamera};
use super::composite_material::{VectorCompositeMaterial, fullscreen_quad};
use super::display_list::{Brush, DisplayList, DrawCmd, Shape};

/// Format of the texture the backend renders into and Bevy samples.
///
/// `render()` clears whatever view it is given, so the backend must own its
/// target rather than draw into a populated `ViewTarget`. The cost is one
/// full-window texture; the alternative is erasing everything Bevy drew first.
///
/// UNORM, not sRGB: Vello writes sRGB-encoded *premultiplied* bytes, and letting
/// the hardware decode them would darken every antialiased edge. The composite
/// shader un-premultiplies and converts instead — see `composite_material`.
const TARGET_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

/// Tolerance for converting analytic shapes to paths, in device pixels.
///
/// Only reached for shapes `vello_hybrid` has no direct entry point for
/// (rounded rects, circles, lines); rects keep their fast path.
const PATH_TOLERANCE: f64 = 0.1;

// ---------------------------------------------------------------------------
// Main-world side
// ---------------------------------------------------------------------------

/// The offscreen image this backend renders into, and the quad that composites it.
#[derive(Resource, Debug, Clone)]
pub struct HybridTarget {
    pub image: Handle<Image>,
    /// Entity holding the clip-space quad and its compositing material.
    pub composite: Entity,
    pub size: UVec2,
}

/// Per-layer counters, for deciding what to cache later and for telling apart
/// "the scene rebuilt" from "the scene rendered" when reading a frame profile.
///
/// This is a **render-world** resource, so main-world systems cannot read it
/// directly; each rebuild is logged at `debug!` instead. If it ever needs to
/// drive main-world behaviour, mirror it back with a system rather than moving
/// it — the counters are produced where the work happens.
#[derive(Resource, Default, Debug, Clone)]
pub struct VectorStats {
    /// Scene rebuilds since startup. On a well-behaved idle frame this stops
    /// climbing; if it tracks the frame counter, something is dirtying a layer
    /// every frame and change detection is being defeated.
    pub scene_rebuilds: u64,
    /// Layers skipped by the viewport test during the last rebuild.
    pub layers_culled: u32,
    /// Commands emitted into the scene during the last rebuild.
    pub commands_emitted: u32,
    /// Per-layer rebuild counts, keyed by entity. High command count plus a low
    /// rebuild count is the signature of a good rasterization candidate.
    pub layer_rebuilds: HashMap<Entity, u64>,
}

pub struct HybridBackend;

impl Plugin for HybridBackend {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, setup_target)
            .add_systems(PostUpdate, sync_target);

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<ExtractedLayers>()
            .init_resource::<VectorStats>()
            .add_systems(ExtractSchedule, (extract_target, extract_layers))
            .add_systems(
                Render,
                (prepare_scene, render_scene)
                    .chain()
                    .in_set(RenderSystems::Render)
                    .run_if(resource_exists::<RenderDevice>),
            );
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.init_resource::<HybridRenderer>();
    }
}

/// Create the offscreen target and the sprite that composites it.
fn setup_target(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<VectorCompositeMaterial>>,
    camera: Res<VectorCamera>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let size = windows
        .single()
        .map(|w| {
            UVec2::new(
                w.resolution.physical_width().max(1),
                w.resolution.physical_height().max(1),
            )
        })
        .unwrap_or(UVec2::splat(1));

    let image = images.add(new_target_image(size));
    // A clip-space quad, not a sprite: see `composite_material` for why the
    // blend state and the 1:1 mapping both have to be ours.
    let mesh = meshes.add(fullscreen_quad());
    let material = materials.add(VectorCompositeMaterial {
        texture: image.clone(),
    });
    let composite = commands
        .spawn((
            Mesh2d(mesh),
            MeshMaterial2d(material),
            Transform::default(),
            RenderLayers::layer(VECTOR_LAYER),
            // The quad ignores the view transform, so it can never be culled by
            // bounds that assume otherwise.
            bevy::camera::visibility::NoFrustumCulling,
            Visibility::Visible,
            Name::new("Hybrid Backend Composite"),
        ))
        .id();

    commands.insert_resource(HybridTarget {
        image,
        composite,
        size,
    });
    let _ = camera;
}

fn new_target_image(size: UVec2) -> Image {
    // `vello_hybrid::Renderer::render` clears the complete target before every
    // draw. Supplying initialized pixels here would allocate and upload a
    // full-window zero buffer that can never be observed, on startup and on
    // every resize. Keep this GPU-only target data-less instead.
    let mut image = Image::new_uninit(
        wgpu::Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        wgpu::TextureDimension::D2,
        TARGET_FORMAT,
        // The CPU never reads this back; keeping it GPU-only avoids a
        // full-window shadow copy in main memory.
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage = wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_DST
        | wgpu::TextureUsages::RENDER_ATTACHMENT;
    // The app installs `ImagePlugin::default_nearest()` for pixel-art assets,
    // and this composite would otherwise inherit it. At an exact 1:1 blit
    // nearest and linear agree, but any sub-pixel misalignment turns nearest
    // into visible stair-stepping along every antialiased edge — which is what
    // separates this path from `bevy_vello`'s, whose fullscreen-triangle
    // material maps fragment coordinates straight to UVs and so is 1:1 by
    // construction. Linear degrades gracefully instead.
    image.sampler = ImageSampler::linear();
    image
}

/// Track the window size and replace the offscreen target after a resize.
fn sync_target(
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<VectorCompositeMaterial>>,
    mut target: ResMut<HybridTarget>,
    composites: Query<&MeshMaterial2d<VectorCompositeMaterial>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let size = UVec2::new(
        window.resolution.physical_width().max(1),
        window.resolution.physical_height().max(1),
    );

    let Ok(material) = composites.get(target.composite) else {
        return;
    };
    if target.size == size {
        return;
    }

    let image = images.add(new_target_image(size));
    if let Some(mut slot) = materials.get_mut(&material.0) {
        slot.texture = image.clone();
    }
    target.image = image;
    target.size = size;
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

/// One layer's contribution, mirrored into the render world.
struct ExtractedLayer {
    order: i32,
    /// Layer space already folded into the transform; see [`layer_transform`].
    transform: Affine,
    cmds: Vec<DrawCmd>,
    bounds: Option<Rect>,
}

/// Render-world mirror of every vector layer.
///
/// A mirror rather than a per-frame copy: display lists hold `BezPath`s, and
/// cloning all of them every frame would throw away exactly the work that
/// change detection just saved. Only layers that actually changed are copied.
#[derive(Resource, Default)]
struct ExtractedLayers {
    layers: HashMap<Entity, ExtractedLayer>,
    /// Set when the mirror changed; cleared once the scene is rebuilt.
    dirty: bool,
    target_size: UVec2,
    /// Last camera mapping used to build the mirror; see `extract_layers`.
    view: Option<ViewMapping>,
}

/// Mirror of [`HybridTarget::image`], so the render world can find the texture.
#[derive(Resource)]
struct HybridTargetView(Handle<Image>);

fn extract_target(mut commands: Commands, target: Extract<Option<Res<HybridTarget>>>) {
    if let Some(target) = target.as_ref() {
        commands.insert_resource(HybridTargetView(target.image.clone()));
    }
}

fn extract_layers(
    mut extracted: ResMut<ExtractedLayers>,
    target: Extract<Option<Res<HybridTarget>>>,
    layers: Extract<Query<(Entity, &VectorLayer, Ref<DisplayList>, Ref<GlobalTransform>)>>,
    camera: Extract<Option<Res<VectorCamera>>>,
    camera_transforms: Extract<Query<&GlobalTransform, With<Camera>>>,
    mut stats: ResMut<VectorStats>,
) {
    if let Some(target) = target.as_ref()
        && extracted.target_size != target.size
    {
        extracted.target_size = target.size;
        extracted.dirty = true;
    }

    // World layers need the camera mapping, which depends on the target size, so
    // it is resolved once here rather than per layer.
    let camera_translation = camera
        .as_ref()
        .and_then(|c| camera_transforms.get(c.0).ok())
        .map(|t| t.translation().truncate())
        .unwrap_or(Vec2::ZERO);
    let view = ViewMapping {
        half: extracted.target_size.as_vec2() * 0.5,
        camera: camera_translation,
    };

    // A camera move changes every world layer's device-space transform without
    // touching any display list, so it has to invalidate the mirror on its own.
    let view_moved = extracted.view != Some(view);
    extracted.view = Some(view);

    let mut seen = 0_usize;
    for (entity, layer, list, global) in layers.iter() {
        seen += 1;
        // Three ways a mirrored layer can go stale, and only the first is
        // obvious: its commands changed, the entity moved, or the camera moved
        // under a world layer. Gating on the display list alone would silently
        // freeze a layer that only ever moves — which is exactly what a pan does.
        // Screen layers are authored directly in the scene's own space, so
        // `layer_transform` discards their transform entirely — a screen layer
        // that "moves" changes nothing about what this backend draws. Only
        // world layers care, and restricting the test to them is not an
        // optimisation but a correctness statement about what feeds the output.
        //
        // It also keeps this backend out of the blast radius of anything that
        // writes screen-layer transforms unconditionally, which the classic
        // backend's `sync_screen_space_transforms` is entitled to do.
        let moved = layer.space == LayerSpace::World && (global.is_changed() || view_moved);
        // `is_added` covers the first frame, when the mirror has no entry yet.
        if !list.is_changed() && !moved && extracted.layers.contains_key(&entity) {
            continue;
        }
        extracted.layers.insert(
            entity,
            ExtractedLayer {
                order: layer.order,
                transform: layer_transform(layer.space, &global, view),
                cmds: list.cmds().to_vec(),
                bounds: list.bounds(),
            },
        );
        extracted.dirty = true;
        *stats.layer_rebuilds.entry(entity).or_insert(0) += 1;
    }

    // Drop layers whose entities are gone. Comparing counts first keeps the
    // common case to a single integer compare.
    if seen != extracted.layers.len() {
        let live: bevy::platform::collections::HashSet<Entity> =
            layers.iter().map(|(e, ..)| e).collect();
        extracted.layers.retain(|e, _| live.contains(e));
        extracted.dirty = true;
    }

}

/// How the composite camera maps world units onto the scene's pixels.
#[derive(Clone, Copy, PartialEq)]
struct ViewMapping {
    /// Half the target size: world origin sits at the centre of the view.
    half: Vec2,
    /// Camera position in world units.
    camera: Vec2,
}

/// Resolve a layer's own transform into the scene's device space.
///
/// Screen layers are authored in window pixels, which *is* the scene's space, so
/// they need nothing.
///
/// World layers are the interesting case, and the one that is silent when wrong.
/// Their content is authored in y-up world units around the entity's origin, and
/// the scene is y-down pixels from the window's top-left. Three things compose,
/// outermost first:
///
/// 1. move the world origin to the centre of the view,
/// 2. flip y,
/// 3. apply the entity's own world transform.
///
/// The flip must stay *outside* the entity transform. Commands inside a layer
/// are authored in the same y-up space as the entity — `vello_world_demo` cancels
/// its own translation that way — so flipping first would break that
/// cancellation while still looking plausible on symmetric shapes.
///
/// Assumes the composite camera is unscaled (1 world unit = 1 pixel, which is
/// `Camera2d`'s default). If it ever zooms, the scale belongs here too.
fn layer_transform(space: LayerSpace, global: &GlobalTransform, view: ViewMapping) -> Affine {
    match space {
        LayerSpace::Screen => Affine::IDENTITY,
        LayerSpace::World => {
            let m = global.to_matrix();
            let entity = Affine::new([
                m.x_axis.x as f64,
                m.x_axis.y as f64,
                m.y_axis.x as f64,
                m.y_axis.y as f64,
                m.w_axis.x as f64,
                m.w_axis.y as f64,
            ]);
            let to_screen = Affine::new([
                1.0,
                0.0,
                0.0,
                -1.0,
                (view.half.x - view.camera.x) as f64,
                (view.half.y + view.camera.y) as f64,
            ]);
            to_screen * entity
        }
    }
}

// ---------------------------------------------------------------------------
// Render world
// ---------------------------------------------------------------------------

/// The renderer, its persistent resources, and the retained scene.
///
/// Behind a `Mutex` because `vello_hybrid::Scene` holds `RefCell`/`OnceCell`
/// internally and so is `Send` but not `Sync`, while Bevy resources must be
/// both. `bevy_vello` wraps its renderer the same way. There is no contention
/// to speak of: only the two render-world systems below ever take the lock, and
/// they are ordered.
#[derive(Resource)]
struct HybridRenderer(Mutex<HybridInner>);

impl Default for HybridRenderer {
    fn default() -> Self {
        Self(Mutex::new(HybridInner::default()))
    }
}

#[derive(Default)]
struct HybridInner {
    /// Created lazily: constructing a `Renderer` needs the device, which does
    /// not exist when the plugin is built.
    inner: Option<(Renderer, Resources)>,
    scene: Option<Scene>,
    size: UVec2,
    /// Scratch path buffer, so shape conversion allocates once rather than
    /// once per shape per rebuild.
    scratch: BezPath,
}

/// Rebuild the merged scene, but only when a layer actually changed.
fn prepare_scene(
    renderer: Res<HybridRenderer>,
    mut extracted: ResMut<ExtractedLayers>,
    mut stats: ResMut<VectorStats>,
) {
    let size = extracted.target_size;
    if size.x == 0 || size.y == 0 {
        return;
    }
    let mut state = renderer.0.lock().expect("hybrid renderer lock");

    let resized = state.size != size;
    if resized {
        state.size = size;
        // The renderer is built against a fixed target config, and the view
        // belongs to a texture that no longer exists, so a resize retires both.
        state.inner = None;
    }
    if !extracted.dirty && !resized && state.scene.is_some() {
        return;
    }

    // `Scene::new` takes u16; a 5K display is well inside that, but a bad
    // window size should clamp rather than wrap.
    let (w, h) = (
        size.x.min(u16::MAX as u32) as u16,
        size.y.min(u16::MAX as u32) as u16,
    );
    match state.scene.as_mut() {
        // Reuse the allocation; `reset_and_resize` keeps its buffers.
        Some(scene) => scene.reset_and_resize(w, h),
        None => state.scene = Some(Scene::new(w, h)),
    }

    // Painter order across layers is explicit; within a layer it is emission
    // order. Sorting a handful of entries every rebuild is cheaper than keeping
    // a sorted structure correct across insertions and removals.
    let mut ordered: Vec<&ExtractedLayer> = extracted.layers.values().collect();
    ordered.sort_by_key(|l| l.order);

    let viewport = Rect::new(0.0, 0.0, size.x as f64, size.y as f64);
    let HybridInner { scene, scratch, .. } = &mut *state;
    let scene = scene.as_mut().expect("scene created above");

    let mut culled = 0_u32;
    let mut emitted = 0_u32;
    for layer in ordered {
        // A layer whose content cannot touch the viewport costs nothing. This is
        // the cheapest form of the culling described in Tranche C, and it works
        // because `bounds` is conservative.
        if let Some(bounds) = layer.bounds {
            let device_bounds = layer.transform.transform_rect_bbox(bounds);
            if device_bounds.intersect(viewport).is_zero_area() {
                culled += 1;
                continue;
            }
        }
        emitted += emit_layer(scene, layer, scratch);
    }

    extracted.dirty = false;
    stats.scene_rebuilds += 1;
    stats.layers_culled = culled;
    stats.commands_emitted = emitted;
    debug!(
        "hybrid scene rebuild #{}: {emitted} commands, {culled} layers culled",
        stats.scene_rebuilds
    );
}

/// Emit one layer's contribution into the merged scene.
///
/// **This is the seam a raster cache substitutes into.** A layer that has been
/// rasterized would, instead of walking commands, bind its texture and issue a
/// single `scene.draw_texture_rect(ExternalTextureRect { .. })` covering
/// `layer.bounds`. Everything that decision needs — stable identity, bounds,
/// rebuild frequency — is already available at this call site; what is missing
/// is only the atlas and its eviction policy.
///
/// Returns the number of commands emitted.
fn emit_layer(scene: &mut Scene, layer: &ExtractedLayer, scratch: &mut BezPath) -> u32 {
    let mut emitted = 0;
    for cmd in &layer.cmds {
        match cmd {
            DrawCmd::Fill {
                transform,
                brush,
                fill_rule,
                shape,
            } => {
                let Brush::Solid(color) = brush;
                scene.set_transform(layer.transform * *transform);
                scene.set_fill_rule(*fill_rule);
                scene.set_paint(*color);
                match shape {
                    // Rects have a dedicated entry point that skips path
                    // handling entirely — the reason `Shape` keeps them apart.
                    Shape::Rect(r) => scene.fill_rect(r),
                    other => {
                        fill_scratch(scratch, other);
                        scene.fill_path(scratch);
                    }
                }
            }
            DrawCmd::Stroke {
                transform,
                style,
                brush,
                shape,
            } => {
                let Brush::Solid(color) = brush;
                scene.set_transform(layer.transform * *transform);
                scene.set_paint(*color);
                scene.set_stroke(style.clone());
                match shape {
                    Shape::Rect(r) => scene.stroke_rect(r),
                    other => {
                        fill_scratch(scratch, other);
                        scene.stroke_path(scratch);
                    }
                }
            }
            DrawCmd::PushClip { transform, shape } => {
                scene.set_transform(layer.transform * *transform);
                fill_scratch(scratch, shape);
                scene.push_clip_layer(scratch);
            }
            DrawCmd::PopLayer => scene.pop_layer(),
        }
        emitted += 1;
    }
    // Leaving state set would leak into the next layer, which is the classic
    // stateful-API bug.
    scene.reset_transform();
    scene.set_fill_rule(Fill::NonZero);
    emitted
}

/// Convert a shape into the scratch path, reusing its allocation.
fn fill_scratch(scratch: &mut BezPath, shape: &Shape) {
    match shape {
        Shape::Path(p) => {
            // Already a path; clone into scratch rather than borrowing, so the
            // caller has one code path.
            scratch.truncate(0);
            scratch.extend(p.elements().iter().copied());
        }
        Shape::Rect(s) => refill(scratch, s.path_elements(PATH_TOLERANCE)),
        Shape::RoundedRect(s) => refill(scratch, s.path_elements(PATH_TOLERANCE)),
        Shape::Circle(s) => refill(scratch, s.path_elements(PATH_TOLERANCE)),
        Shape::Line(s) => refill(scratch, s.path_elements(PATH_TOLERANCE)),
    }
}

fn refill(scratch: &mut BezPath, els: impl Iterator<Item = PathEl>) {
    scratch.truncate(0);
    scratch.extend(els);
}

/// Render the merged scene into the backend's own texture.
fn render_scene(
    renderer: Res<HybridRenderer>,
    target: Option<Res<HybridTargetView>>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let Some(target) = target else { return };
    let Some(gpu_image) = gpu_images.get(&target.0) else {
        // The image lands in the render world an asset-extraction tick after it
        // is created; skipping until then is normal, not an error.
        return;
    };

    let mut state = renderer.0.lock().expect("hybrid renderer lock");
    let size = state.size;
    if size.x == 0 || size.y == 0 || state.scene.is_none() {
        return;
    }

    if state.inner.is_none() {
        // Clip and blend layers are composited through intermediate textures,
        // and `LayersConfig` caps their size at 4096 by default. A clip wider
        // than that — a full-width panel on a 5K display, say — is rejected
        // outright with "exceeds maximum", so the cap has to follow the device
        // rather than a constant.
        //
        // Only the *maximum* is raised. `min_texture_size` stays at its default:
        // upstream notes that large minimums cost memory and hurt mobile GPUs,
        // and it is a batching hint rather than a correctness limit.
        let device_max = device
            .limits()
            .max_texture_dimension_2d
            .min(u16::MAX as u32) as u16;
        info!(
            "vello_hybrid renderer {}x{}, intermediate layer textures capped at {device_max}",
            size.x, size.y
        );
        state.inner = Some(Renderer::new_with(
            device.wgpu_device(),
            &RenderTargetConfig {
                format: TARGET_FORMAT,
                width: size.x,
                height: size.y,
            },
            RenderSettings {
                memory_settings: MemorySettings {
                    layers_config: LayersConfig {
                        max_texture_size: SizeU16::new(device_max),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
        ));
    }
    let HybridInner { inner, scene, .. } = &mut *state;
    let (renderer_inner, resources) = inner.as_mut().expect("created above");
    let scene = scene.as_ref().expect("checked above");

    let mut encoder =
        device
            .wgpu_device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("vello_hybrid"),
            });
    // Note: `render` clears `view` before drawing. That is why this backend owns
    // an offscreen texture instead of writing into Bevy's `ViewTarget`.
    if let Err(err) = renderer_inner.render(
        scene,
        resources,
        device.wgpu_device(),
        queue.as_ref(),
        &mut encoder,
        &RenderSize {
            width: size.x,
            height: size.y,
        },
        &gpu_image.texture_view,
        &vello_hybrid::TextureBindings::new(),
    ) {
        error!("vello_hybrid render failed: {err}");
        return;
    }
    queue.submit([encoder.finish()]);
}
