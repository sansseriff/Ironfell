//! Sparse-strips backend, via `vello_gpu` (`vello_hybrid` before 0.3).
//!
//! Vello Hybrid realizes the app's renderer-independent display lists. It
//! flattens, tiles and computes antialiasing coverage on
//! the **CPU**, in **device space**, and uses only vertex/fragment passes. Three
//! consequences drive the design here:
//!
//! - **One `Scene` for the whole view.** Every public `render()` clears its
//!   target, so layers cannot be composited as separate scenes. All layers merge
//!   into a single `Scene`, sorted by [`VectorLayer::order`] — which is why that
//!   ordering had to become explicit before this backend could exist.
//! - **The `Scene` is pixel-sized and screen-space.** Screen layers pass straight
//!   through, and *world* layers are the ones needing a camera transform.
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
use kurbo::{Affine, BezPath, PathEl, Rect, Shape as _};
use peniko::Fill;
use std::sync::Mutex;
use vello_common::TextureId;
use vello_common::geometry::{RectU16, SizeU16};
use vello_common::paint::{Image as VelloImage, ImageSource};
use vello_gpu::{
    ClearSettings, LayersConfig, MemorySettings, RenderSettings, RenderSize, RenderTargetConfig, Renderer,
    Resources, Scene, TargetInit,
};
use wgpu::TextureFormat;

use super::backend::{LayerSpace, Offscreen, Overlay, VectorLayer, VectorView};
use super::view_composite::{MODE_PREMULTIPLIED_RAW, ViewCompositeMaterial, bucket, unit_quad};
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
    /// The texture's size: the window's while any layer draws into it, 1×1
    /// once every layer has a view of its own.
    pub size: UVec2,
    /// The window's size, which the renderer is configured for.
    pub window: UVec2,
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
            // Before the asset events, so a texture they make is on the GPU
            // the frame it is made (see `sync_view_images`).
            .add_systems(PostUpdate, (sync_target, sync_views).before(bevy::asset::AssetEventSystems));

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<ExtractedLayers>()
            .init_resource::<VectorStats>()
            .init_resource::<ExtractedViews>()
            .add_systems(ExtractSchedule, (extract_target, extract_views, extract_layers).chain())
            .add_systems(
                Render,
                (prepare_scene, render_scene)
                    .chain()
                    .in_set(RenderSystems::Render)
                    // Before Bevy runs its cameras and composites the window:
                    // unordered, the window could paste last frame's 2D.
                    .before(bevy::render::renderer::render_system)
                    .run_if(resource_exists::<RenderDevice>),
            )
            // Views that draw a 3D view's texture render inside the window
            // camera's schedule: after every 3D camera has rendered this
            // frame, before the window composite, in the same submission.
            .add_systems(
                bevy::core_pipeline::Core2d,
                render_textured_scenes
                    .before(bevy::core_pipeline::Core2dSystems::MainPass)
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
        window: size,
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
    // into visible stair-stepping along every antialiased edge. Linear degrades
    // gracefully instead.
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
    layers: Query<&VectorLayer>,
    mut visibility: Query<&mut Visibility>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let window = UVec2::new(
        window.resolution.physical_width().max(1),
        window.resolution.physical_height().max(1),
    );
    if target.window != window {
        target.window = window;
    }
    // A window-sized texture, rendered and pasted every time it changes,
    // only while some layer still draws into it; otherwise a 1×1 stub and a
    // hidden quad (plans/specs/view-textures.md).
    let used = layers.iter().any(|l| l.view.is_none());
    let size = if used { window } else { UVec2::ONE };
    if let Ok(mut v) = visibility.get_mut(target.composite) {
        let want = if used { Visibility::Visible } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }

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

/// A view's texture and the quad that pastes it into the window.
#[derive(Component, Debug)]
pub struct ViewTarget2d {
    pub image: Handle<Image>,
    /// The texture's size; the view fills its top-left `VectorView::size`.
    pub bucket: UVec2,
    material: Handle<ViewCompositeMaterial>,
}

/// The view whose texture a composite quad pastes.
#[derive(Component, Debug)]
struct ViewCompositeOf(Entity);

/// Give every [`VectorView`] a texture (bucketed, so a pane drag rarely
/// reallocates) and a composite quad over its rectangle, and retire both
/// with the view.
fn sync_views(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ViewCompositeMaterial>>,
    mut quad: Local<Option<Handle<Mesh>>>,
    mut views: Query<(Entity, Ref<VectorView>, Option<&mut ViewTarget2d>, Has<Offscreen>, Has<Overlay>)>,
    composites: Query<(Entity, &ViewCompositeOf)>,
) {
    let quad = quad.get_or_insert_with(|| meshes.add(unit_quad())).clone();
    for (entity, view, target, offscreen, overlay) in &mut views {
        let size = view.size.max(UVec2::ONE);
        let want = bucket(size);
        let rect = Vec4::new(view.pos.x as f32, view.pos.y as f32, size.x as f32, size.y as f32);
        let params = Vec4::new(want.x as f32, want.y as f32, MODE_PREMULTIPLIED_RAW, 0.0);
        match target {
            Some(mut t) => {
                if t.bucket != want {
                    t.image = images.add(new_target_image(want));
                    t.bucket = want;
                    if let Some(mut m) = materials.get_mut(&t.material) {
                        m.texture = t.image.clone();
                        m.params = params;
                        m.rect = rect;
                    }
                } else if view.is_changed()
                    && let Some(mut m) = materials.get_mut(&t.material)
                {
                    m.rect = rect;
                }
            }
            None => {
                let image = images.add(new_target_image(want));
                let material = materials.add(ViewCompositeMaterial { texture: image.clone(), rect, params });
                commands.entity(entity).insert(ViewTarget2d { image, bucket: want, material: material.clone() });
                if offscreen {
                    continue;
                }
                commands.spawn((
                    Mesh2d(quad.clone()),
                    MeshMaterial2d(material.clone()),
                    // Over 3D views (z = -1) and the window-sized vector
                    // texture (z = 0); editor feedback over the content.
                    Transform::from_xyz(0.0, 0.0, if overlay { 0.6 } else { 0.5 }),
                    RenderLayers::layer(VECTOR_LAYER),
                    bevy::camera::visibility::NoFrustumCulling,
                    Visibility::Visible,
                    ViewCompositeOf(entity),
                    Name::new("vector view composite"),
                ));
            }
        }
    }
    for (e, of) in &composites {
        if views.get(of.0).is_err() {
            commands.entity(e).despawn();
        }
    }
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
    /// The view whose texture it renders into; `None` is the window target.
    view: Option<Entity>,
}

/// Where a target's scene goes: the window-sized texture (`None`) or one
/// view's own texture (plans/specs/view-textures.md).
type TargetKey = Option<Entity>;

/// Render-world mirror of every vector layer.
///
/// A mirror rather than a per-frame copy: display lists hold `BezPath`s, and
/// cloning all of them every frame would throw away exactly the work that
/// change detection just saved. Only layers that actually changed are copied.
#[derive(Resource, Default)]
struct ExtractedLayers {
    layers: HashMap<Entity, ExtractedLayer>,
    /// Targets whose layers changed; cleared as each scene is rebuilt.
    dirty: bevy::platform::collections::HashSet<TargetKey>,
    target_size: UVec2,
    /// The window's size, which the renderer is configured for.
    window_size: UVec2,
    /// Last camera mapping used to build the mirror; see `extract_layers`.
    view: Option<ViewMapping>,
}

/// One view's texture, mirrored for the render world.
#[derive(Clone, Debug, PartialEq)]
struct ExtractedView {
    /// Window px of the view's top-left: screen layers are offset by it.
    pos: UVec2,
    /// The texture's (bucketed) size, which is the scene's size.
    bucket: UVec2,
    image: Handle<Image>,
    /// Editor feedback ([`Overlay`]): timed apart from content encoding.
    overlay: bool,
}

#[derive(Resource, Default)]
struct ExtractedViews(HashMap<Entity, ExtractedView>);

/// Mirror of [`HybridTarget::image`], so the render world can find the texture.
#[derive(Resource)]
struct HybridTargetView(Handle<Image>);

fn extract_target(mut commands: Commands, target: Extract<Option<Res<HybridTarget>>>) {
    if let Some(target) = target.as_ref() {
        commands.insert_resource(HybridTargetView(target.image.clone()));
    }
}

fn extract_views(
    mut extracted: ResMut<ExtractedViews>,
    mut layers: ResMut<ExtractedLayers>,
    views: Extract<Query<(Entity, &VectorView, &ViewTarget2d, Has<Overlay>)>>,
) {
    let mut seen = 0usize;
    for (e, view, target, overlay) in views.iter() {
        seen += 1;
        let next = ExtractedView { pos: view.pos, bucket: target.bucket, image: target.image.clone(), overlay };
        if extracted.0.get(&e) != Some(&next) {
            // Moved, resized or re-targeted: its scene's offset or size, or
            // the texture it goes into, changed.
            extracted.0.insert(e, next);
            layers.dirty.insert(Some(e));
        }
    }
    if seen != extracted.0.len() {
        let live: bevy::platform::collections::HashSet<Entity> = views.iter().map(|(e, ..)| e).collect();
        extracted.0.retain(|e, _| live.contains(e));
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
    let timing = crate::perf::span(crate::perf::Phase::Extract);
    let mut copied = false;
    if let Some(target) = target.as_ref()
        && extracted.target_size != target.size
    {
        extracted.target_size = target.size;
        extracted.dirty.insert(None);
    }
    if let Some(target) = target.as_ref() {
        extracted.window_size = target.window;
    }

    // World layers need the camera mapping, which depends on the target size, so
    // it is resolved once here rather than per layer.
    let camera_translation = camera
        .as_ref()
        .and_then(|c| camera_transforms.get(c.0).ok())
        .map(|t| t.translation().truncate())
        .unwrap_or(Vec2::ZERO);
    let view = ViewMapping {
        half: extracted.window_size.as_vec2() * 0.5,
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
        let moved = layer.space == LayerSpace::World && (global.is_changed() || view_moved);
        let retargeted = extracted.layers.get(&entity).is_some_and(|l| l.view != layer.view);
        // `is_added` covers the first frame, when the mirror has no entry yet.
        if !list.is_changed() && !moved && !retargeted && extracted.layers.contains_key(&entity) {
            continue;
        }
        if let Some(old) = extracted.layers.get(&entity) {
            let old_view = old.view;
            extracted.dirty.insert(old_view);
        }
        extracted.layers.insert(
            entity,
            ExtractedLayer {
                order: layer.order,
                transform: layer_transform(layer.space, &global, view),
                cmds: list.cmds().to_vec(),
                bounds: list.bounds(),
                view: layer.view,
            },
        );
        extracted.dirty.insert(layer.view);
        *stats.layer_rebuilds.entry(entity).or_insert(0) += 1;
        copied = true;
    }
    if !copied {
        timing.discard();
    }

    // Drop layers whose entities are gone. Comparing counts first keeps the
    // common case to a single integer compare.
    if seen != extracted.layers.len() {
        let live: bevy::platform::collections::HashSet<Entity> =
            layers.iter().map(|(e, ..)| e).collect();
        let gone: Vec<TargetKey> = extracted.layers.iter().filter(|(e, _)| !live.contains(*e)).map(|(_, l)| l.view).collect();
        extracted.dirty.extend(gone);
        extracted.layers.retain(|e, _| live.contains(e));
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

/// The renderer, its persistent resources, and one retained scene per target.
///
/// Behind a `Mutex` because `vello_hybrid::Scene` holds `RefCell`/`OnceCell`
/// internally and so is `Send` but not `Sync`, while Bevy resources must be
/// both. There is no contention to speak of: only the two render-world systems
/// below ever take the lock, and they are ordered.
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
    /// not exist when the plugin is built. One renderer serves every target:
    /// `render` takes the size per call.
    inner: Option<(Renderer, Resources)>,
    /// The window-sized target's size, which the renderer was configured for.
    size: UVec2,
    targets: HashMap<TargetKey, TargetScene>,
    /// Scratch path buffer, so shape conversion allocates once rather than
    /// once per shape per rebuild.
    scratch: BezPath,
    /// The pass that turns a Bevy camera's image into what Vello samples.
    convert: Option<Convert>,
    /// Converted copies of other views' textures, by source image.
    converted: HashMap<AssetId<Image>, (wgpu::Texture, wgpu::TextureView, UVec2)>,
}

/// One target's retained scene.
struct TargetScene {
    scene: Scene,
    size: UVec2,
    /// The texture holds the current scene: with retention, nothing to do.
    rendered: bool,
    /// Other views' textures this scene draws, bound at render time.
    textures: Vec<(TextureId, Handle<Image>, UVec2)>,
    /// A target that draws textures renders inside the render graph, in its
    /// shared submission, so it has its own renderer: the queue writes a
    /// renderer stages for its buffers land before the whole submission, and
    /// two targets sharing one would overwrite each other's.
    own: Option<(Renderer, Resources)>,
}

/// Rebuild each target's scene, but only when one of its layers changed.
fn prepare_scene(
    renderer: Res<HybridRenderer>,
    mut extracted: ResMut<ExtractedLayers>,
    views: Res<ExtractedViews>,
    mut stats: ResMut<VectorStats>,
) {
    let size = extracted.target_size;
    let window = extracted.window_size;
    if size.x == 0 || size.y == 0 || window.x == 0 || window.y == 0 {
        return;
    }
    // Content and editor feedback are timed apart, so a hover that only
    // re-encodes the overlay shows as zero content encodes.
    let mut content_ms: Option<f32> = None;
    let mut overlay_ms: Option<f32> = None;
    let mut state = renderer.0.lock().expect("hybrid renderer lock");

    if state.size != window {
        state.size = window;
        // The renderer is built against a fixed target config, and the view
        // belongs to a texture that no longer exists, so a resize retires both.
        state.inner = None;
        extracted.dirty.insert(None);
    }
    // Targets that went away.
    state.targets.retain(|k, _| k.is_none_or(|e| views.0.contains_key(&e)));

    let mut keys: Vec<TargetKey> = vec![None];
    keys.extend(views.0.keys().map(|&e| Some(e)));
    let mut culled = 0_u32;
    let mut emitted = 0_u32;
    for key in keys {
        let (offset, tsize, overlay) = match key {
            None => (UVec2::ZERO, size, false),
            Some(e) => {
                let v = &views.0[&e];
                (v.pos, v.bucket, v.overlay)
            }
        };
        let fresh = !state.targets.contains_key(&key);
        if !fresh && !extracted.dirty.contains(&key) {
            continue;
        }
        extracted.dirty.remove(&key);
        let started = bevy::platform::time::Instant::now();
        // `Scene::new` takes u16; a 5K display is well inside that, but a bad
        // size should clamp rather than wrap.
        let (w, h) = (tsize.x.min(u16::MAX as u32) as u16, tsize.y.min(u16::MAX as u32) as u16);
        let HybridInner { targets, scratch, .. } = &mut *state;
        let target = targets.entry(key).or_insert_with(|| TargetScene { scene: Scene::new(w, h), size: tsize, rendered: false, textures: Vec::new(), own: None });
        // Reuse the allocation; `reset_and_resize` keeps its buffers.
        target.scene.reset_and_resize(w, h);
        target.size = tsize;
        target.rendered = false;
        target.textures.clear();

        // Painter order across layers is explicit; within a layer it is
        // emission order.
        let mut ordered: Vec<&ExtractedLayer> = extracted.layers.values().filter(|l| l.view == key).collect();
        ordered.sort_by_key(|l| l.order);
        // A view's scene starts at its own top-left: screen layers, authored
        // in window px, shift by the view's position.
        let to_target = Affine::translate((-(offset.x as f64), -(offset.y as f64)));
        let viewport = Rect::new(0.0, 0.0, tsize.x as f64, tsize.y as f64);
        for layer in ordered {
            // A layer whose content cannot touch the viewport costs nothing.
            if let Some(bounds) = layer.bounds {
                let device_bounds = (to_target * layer.transform).transform_rect_bbox(bounds);
                if device_bounds.intersect(viewport).is_zero_area() {
                    culled += 1;
                    continue;
                }
            }
            let n = emit_layer(&mut target.scene, &mut target.textures, layer, to_target, scratch);
            if !overlay {
                emitted += n;
            }
        }
        let ms = started.elapsed().as_secs_f32() * 1000.0;
        let total = if overlay { &mut overlay_ms } else { &mut content_ms };
        *total = Some(total.unwrap_or(0.0) + ms);
    }
    if let Some(ms) = overlay_ms {
        crate::perf::record(crate::perf::Phase::OverlayEncode, ms);
    }
    if let Some(ms) = content_ms {
        crate::perf::record(crate::perf::Phase::Encode, ms);
        stats.scene_rebuilds += 1;
        stats.layers_culled = culled;
        stats.commands_emitted = emitted;
        debug!("hybrid rebuild #{}: {emitted} commands, {culled} layers culled", stats.scene_rebuilds);
        crate::perf::set_encode_counts(emitted, culled);
    }
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
fn emit_layer(scene: &mut Scene, textures: &mut Vec<(TextureId, Handle<Image>, UVec2)>, layer: &ExtractedLayer, to_target: Affine, scratch: &mut BezPath) -> u32 {
    let base = to_target * layer.transform;
    let mut emitted = 0;
    for cmd in &layer.cmds {
        match cmd {
            DrawCmd::Fill {
                transform,
                brush,
                fill_rule,
                shape,
            } => {
                scene.set_transform(base * *transform);
                scene.set_fill_rule(*fill_rule);
                match brush { Brush::Solid(c) => scene.set_paint(*c), Brush::Gradient(g) => scene.set_paint(g.clone()) };
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
                scene.set_transform(base * *transform);
                match brush { Brush::Solid(c) => scene.set_paint(*c), Brush::Gradient(g) => scene.set_paint(g.clone()) };
                scene.set_stroke(style.clone());
                match shape {
                    Shape::Rect(r) => scene.stroke_rect(r),
                    other => {
                        fill_scratch(scratch, other);
                        scene.stroke_path(scratch);
                    }
                }
            }
            DrawCmd::BlurredRect { transform, color, rect, radius, std_dev } => {
                scene.set_transform(base * *transform);
                scene.set_paint(*color);
                scene.fill_blurred_rounded_rect(rect, *radius as f32, *std_dev as f32, false);
            }
            DrawCmd::PushShadow { transform, color, offset, std_dev, .. } => {
                use vello_common::filter_effects::{EdgeMode, Filter, FilterPrimitive};
                // The filter reads its offset and blur in the transform
                // current when the layer is pushed.
                scene.set_transform(base * *transform);
                scene.push_filter_layer(Filter::from_primitive(FilterPrimitive::DropShadow {
                    dx: offset.x as f32,
                    dy: offset.y as f32,
                    std_deviation: *std_dev as f32,
                    color: *color,
                    edge_mode: EdgeMode::None,
                }));
            }
            DrawCmd::PushClip { transform, shape } => {
                scene.set_transform(base * *transform);
                fill_scratch(scratch, shape);
                scene.push_clip_layer(scratch);
            }
            DrawCmd::PushOpacity { opacity, .. } => scene.push_opacity_layer(*opacity),
            DrawCmd::PushBlend { mode } => scene.push_blend_layer(*mode),
            DrawCmd::PopLayer => scene.pop_layer(),
            DrawCmd::Texture { transform, image, size, .. } => {
                let id = texture_id(image);
                if !textures.iter().any(|(t, ..)| *t == id) {
                    textures.push((id, image.clone(), *size));
                }
                scene.set_transform(base * *transform);
                let (w, h) = (size.x.clamp(1, u16::MAX as u32) as u16, size.y.clamp(1, u16::MAX as u32) as u16);
                // 0.3: an external texture is a paint; the rect it fills is
                // its whole source region in local px (as `draw_texture_rects`
                // drew in 0.2). The view's texture is premultiplied and may
                // be transparent outside its content.
                scene.set_paint(VelloImage {
                    image: ImageSource::external_texture(id, RectU16::new(0, 0, w, h), true),
                    sampler: peniko::ImageSampler::default(),
                });
                scene.fill_rect(&Rect::new(0.0, 0.0, w as f64, h as f64));
            }
        }
        emitted += 1;
    }
    // Leaving state set would leak into the next retained command stream.
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

/// Render each target's scene into its texture: the window-sized one, and
/// every view's own. With retention, a target whose scene did not change
/// keeps last frame's pixels and costs nothing.
fn render_scene(
    renderer: Res<HybridRenderer>,
    target: Option<Res<HybridTargetView>>,
    views: Res<ExtractedViews>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let mut state = renderer.0.lock().expect("hybrid renderer lock");
    let size = state.size;
    if size.x == 0 || size.y == 0 {
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
            "vello_gpu renderer {}x{}, intermediate layer textures capped at {device_max}",
            size.x, size.y
        );
        state.inner = Some(Renderer::new_with(
            device.wgpu_device(),
            &RenderTargetConfig {
                format: TARGET_FORMAT,
                width: size.x.min(u16::MAX as u32) as u16,
                height: size.y.min(u16::MAX as u32) as u16,
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
    let timing = crate::perf::span(crate::perf::Phase::Render);
    let mut rendered = false;
    let HybridInner { inner, targets, convert, converted, .. } = &mut *state;
    let (renderer_inner, resources) = inner.as_mut().expect("created above");
    let convert = convert.get_or_insert_with(|| Convert::new(device.wgpu_device()));
    // Converted copies nobody draws any more.
    converted.retain(|id, _| targets.values().any(|t| t.textures.iter().any(|(_, h, _)| h.id() == *id)));
    let _ = (&convert, &converted);
    for (key, t) in targets.iter_mut() {
        // Targets that draw other views' textures render in the graph
        // (`render_textured_scenes`), once those views have.
        if t.rendered || !t.textures.is_empty() {
            continue;
        }
        let bindings = vello_gpu::TextureBindings::new();
        let handle = match key {
            None => target.as_ref().map(|t| &t.0),
            Some(e) => views.0.get(e).map(|v| &v.image),
        };
        let Some(gpu_image) = handle.and_then(|h| gpu_images.get(h)) else {
            // The image lands in the render world an asset-extraction tick
            // after it is created; skipping until then is normal.
            continue;
        };
        // One encoder and submit per target: consecutive renders reuse the
        // renderer's upload buffers.
        let mut encoder = device
            .wgpu_device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("vello_gpu") });
        // Note: `render` clears `view` before drawing. That is why this backend
        // owns its targets instead of writing into Bevy's `ViewTarget`.
        if let Err(err) = renderer_inner.render(
            &t.scene,
            resources,
            device.wgpu_device(),
            queue.as_ref(),
            &mut encoder,
            &RenderSize { width: t.size.x.min(u16::MAX as u32) as u16, height: t.size.y.min(u16::MAX as u32) as u16 },
            &gpu_image.texture_view,
            None,
            &bindings,
            TargetInit::Clear(ClearSettings::default()),
        ) {
            error!("vello_gpu render failed: {err}");
            continue;
        }
        queue.submit([encoder.finish()]);
        t.rendered = true;
        rendered = true;
    }
    if !rendered {
        timing.discard();
    }
}

/// A stable Vello texture id for a Bevy image.
fn texture_id(image: &Handle<Image>) -> TextureId {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    image.id().hash(&mut h);
    TextureId(h.finish())
}

/// sRGB texture (as a Bevy camera writes it) → UNORM texture of sRGB-encoded,
/// premultiplied bytes (as Vello samples it). One full-target triangle per
/// texture, run whenever a scene that draws it re-renders.
struct Convert {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

const CONVERT_WGSL: &str = r#"
@group(0) @binding(0) var src: texture_2d<f32>;

struct VOut { @builtin(position) pos: vec4<f32> };

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VOut {
    var p = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    var o: VOut;
    o.pos = vec4(p[i], 0.0, 1.0);
    return o;
}

fn encode(c: vec3<f32>) -> vec3<f32> {
    return select(c * 12.92, 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, c > vec3(0.0031308));
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    // Texel for pixel; the sRGB source decodes to linear on load.
    let c = textureLoad(src, vec2<i32>(in.pos.xy), 0);
    return vec4(encode(c.rgb) * c.a, c.a);
}
"#;

impl Convert {
    fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("view texture convert"),
            source: wgpu::ShaderSource::Wgsl(CONVERT_WGSL.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("view texture convert"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("view texture convert"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("view texture convert"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState { format: TARGET_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Convert { pipeline, layout }
    }

    fn target(&self, device: &wgpu::Device, size: UVec2) -> (wgpu::Texture, wgpu::TextureView, UVec2) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("view texture (converted)"),
            size: wgpu::Extent3d { width: size.x.max(1), height: size.y.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view, size)
    }

    fn record(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, src: &wgpu::TextureView, dst: &wgpu::TextureView) {
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("view texture convert"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src) }],
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("view texture convert"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dst,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

/// Render the targets that draw other views' textures (3D compositions seen
/// in 2D), inside the window camera's schedule: every 3D camera has recorded
/// its pass by now, so the texture is this frame's and the 2D view that draws
/// it shows it this frame. Each texture is converted for Vello first (a Bevy
/// camera writes sRGB that samples as linear; Vello reads raw sRGB-encoded,
/// premultiplied bytes), all into the graph's encoder.
fn render_textured_scenes(
    renderer: Res<HybridRenderer>,
    views: Res<ExtractedViews>,
    target: Option<Res<HybridTargetView>>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut ctx: bevy::render::renderer::RenderContext,
) {
    let mut state = renderer.0.lock().expect("hybrid renderer lock");
    let window = state.size;
    if window.x == 0 || window.y == 0 {
        return;
    }
    let HybridInner { targets, convert, converted, .. } = &mut *state;
    let convert = convert.get_or_insert_with(|| Convert::new(device.wgpu_device()));
    converted.retain(|id, _| targets.values().any(|t| t.textures.iter().any(|(_, h, _)| h.id() == *id)));
    for (key, t) in targets.iter_mut() {
        if t.rendered || t.textures.is_empty() {
            continue;
        }
        let handle = match key {
            None => target.as_ref().map(|t| &t.0),
            Some(e) => views.0.get(e).map(|v| &v.image),
        };
        let Some(gpu_image) = handle.and_then(|h| gpu_images.get(h)) else { continue };
        let mut bindings = vello_gpu::TextureBindings::new();
        let mut missing = false;
        for (id, image, size) in &t.textures {
            let Some(src) = gpu_images.get(image) else {
                missing = true;
                continue;
            };
            let entry = converted.entry(image.id()).or_insert_with(|| convert.target(device.wgpu_device(), *size));
            if entry.2 != *size {
                *entry = convert.target(device.wgpu_device(), *size);
            }
            convert.record(device.wgpu_device(), ctx.command_encoder(), &src.texture_view, &entry.1);
            bindings.insert(*id, entry.1.clone());
        }
        if missing {
            // A texture not on the GPU yet; try again next frame.
            continue;
        }
        let (own, resources) = t.own.get_or_insert_with(|| {
            Renderer::new_with(
                device.wgpu_device(),
                &RenderTargetConfig { format: TARGET_FORMAT, width: window.x.min(u16::MAX as u32) as u16, height: window.y.min(u16::MAX as u32) as u16 },
                RenderSettings::default(),
            )
        });
        if let Err(err) = own.render(
            &t.scene,
            resources,
            device.wgpu_device(),
            queue.as_ref(),
            ctx.command_encoder(),
            &RenderSize { width: t.size.x.min(u16::MAX as u32) as u16, height: t.size.y.min(u16::MAX as u32) as u16 },
            &gpu_image.texture_view,
            None,
            &bindings,
            TargetInit::Clear(ClearSettings::default()),
        ) {
            error!("vello_gpu render failed: {err}");
            continue;
        }
        t.rendered = true;
    }
}
