//! Raster layers (plans/specs/render-cache.md §5): runs of a view's cold
//! chunks drawn from cached tiles instead of being encoded again.
//!
//! Render world only. The main world marks which layers may be cached
//! (`VectorLayer::cache`, a chunk's stable key); the rest is decided here,
//! each time a view's scene is built:
//!
//! - **Layerization.** A chunk is hot for [`HOT`] after its recording last
//!   changes, and always when it cannot be baked (it draws a texture, a
//!   filter, or a blend that reads the backdrop). A run of cold chunks is a
//!   *group*, drawn from tiles; hot chunks draw live between groups. A view
//!   whose cold content is cheap to encode ([`LIVE_OK`]) draws everything
//!   live, as before.
//! - **Tiles.** A group's pixels at one scale and sub-pixel phase ([`At`]),
//!   cut into [`TILE`]-px squares of *bucket px* (list units × scale, plus
//!   the phase) and kept on shared atlas pages. At rest the scale is the
//!   view's own and the phase its placement's fraction of a pixel, so a tile
//!   maps texel for pixel. While the zoom changes, or a group moves by a
//!   fraction of a pixel (a chunk recorded in its parent group's space,
//!   placed by the layer's transform: a dragged group moves rigidly), the
//!   tiles it has are drawn resampled, up to 2× stretched, and the exact
//!   tiling is baked once the motion settles.
//! - **Fallbacks.** A square with no tile is drawn from another scale's
//!   tiles, or else live, clipped to the square. Nothing is ever missing or
//!   stale on screen (architecture.md §7).
//! - **Damage.** When a chunk's recording changes, the tiles under the
//!   commands that changed are dropped, in every group that holds it.
//! - **Transitions.** When the grouping changes (a chunk turns hot or cold),
//!   the old groups keep drawing until the new ones cover the view. Their
//!   damaged squares are drawn live meanwhile.
//!
//! Baking is budgeted ([`BAKE_MS`] of encode a frame, visible tiles first,
//! nearest the centre first). Each tile is encoded into a scene of its own
//! slot's size, so Vello culls everything outside it, then rendered and
//! copied into its slot: a scene the size of a page would make every tile
//! pay for strips across the whole page.

use bevy::platform::collections::{HashMap, HashSet};
use bevy::platform::time::Instant;
use bevy::prelude::Entity;
use kurbo::{Affine, BezPath, Rect, Shape as _};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use vello_common::TextureId;
use vello_common::geometry::RectU16;
use vello_common::paint::{Image as VelloImage, ImageSource};
use vello_gpu::{ClearSettings, RenderSize, Renderer, Resources, Scene, TargetInit, TextureBindings};

/// Baked tile scenes kept for reuse; more than a frame's bakes need.
const POOL: usize = 16;

use super::display_list::{DrawCmd, Recorded};

/// A tile's side, in bucket px.
pub const TILE: i32 = 254;
/// Texels of neighbouring content around each tile on its page, so bilinear
/// sampling at a tile's edge reads the right colours, not the next slot's.
const GUTTER: i32 = 1;
const SLOT: i32 = TILE + 2 * GUTTER;
/// An atlas page's side: 16 MB of RGBA8, 64 slots.
const PAGE: i32 = 2048;
const ROW: i32 = PAGE / SLOT;
const SLOTS: usize = (ROW * ROW) as usize;
/// Texture memory all pages may take, by default.
const RASTER_BUDGET: u64 = 256 << 20;
/// The budget in force, in bytes (`__iron.raster(true, mb)` lowers it, to
/// exercise eviction).
static BUDGET: AtomicU64 = AtomicU64::new(RASTER_BUDGET);
const PAGE_BYTES: u64 = PAGE as u64 * PAGE as u64 * 4;

/// Seconds a chunk stays live after its recording changes.
const HOT: f64 = 0.5;
/// Seconds after the view's scale last changed that it still counts as
/// zooming: tiles are stretched, not rebaked.
const ZOOM_SETTLE: f64 = 0.15;
/// A view rasters once its cold visible content has more commands than
/// this, about 4 ms of encode, and stops below [`LIVE_OFF`].
const LIVE_OK: usize = 2000;
const LIVE_OFF: usize = 1000;
/// A group with fewer visible commands is cheaper drawn live.
const MIN_GROUP: usize = 64;
/// Milliseconds of baking a frame may spend on work nothing waits for:
/// prefetch, a grouping change, a stretched square's sharper tile. A tile
/// whose estimate alone is over it bakes only in a frame where its view is
/// otherwise idle. A visible square nothing else covers bakes regardless,
/// since drawing it live would cost the same.
const BAKE_MS: f32 = 6.0;
/// The magnification range within which a zoom gesture stretches an
/// existing tiling instead of baking one.
const STRETCH: (f64, f64) = (0.5, 2.0);

/// Texture ids of atlas pages, as Vello knows them: page `i` is `BASE + i`.
const PAGE_TEXTURE_BASE: u64 = 0x7261_7374_6572_0000;

/// Some view has raster work left (tiles to bake, a chunk about to turn
/// cold, a zoom about to settle): the app keeps rendering frames until it
/// is done (`decide_redraw`).
static PENDING: AtomicBool = AtomicBool::new(false);

pub fn pending() -> bool {
    PENDING.load(Ordering::Relaxed)
}

/// Off draws every view live, as before raster layers: the reference the
/// raster output is compared against (`__iron.raster(false)`).
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Turn raster tiles on or off, and set the atlas budget (`0`: the
/// default, [`RASTER_BUDGET`]).
pub fn set_enabled(enabled: bool, budget_mb: u64) {
    ENABLED.store(enabled, Ordering::Relaxed);
    BUDGET.store(if budget_mb == 0 { RASTER_BUDGET } else { budget_mb << 20 }, Ordering::Relaxed);
}

/// What the last frames did, for the render inspector and the benchmarks
/// (`__iron.raster()`).
static REPORT: Mutex<Option<serde_json::Value>> = Mutex::new(None);
/// Counters since the last [`report`] that reset them.
static TOTALS: Mutex<Totals> = Mutex::new(Totals { baked: 0, bake_ms: 0.0, live_squares: 0, stretched_squares: 0, evicted: 0 });

#[derive(Clone, Copy, Default)]
struct Totals {
    baked: u32,
    bake_ms: f32,
    live_squares: u32,
    stretched_squares: u32,
    evicted: u32,
}

/// The atlas and every view's raster state, plus the counters and the
/// last frame's per-view picture. `reset` clears the counters.
pub fn report(reset: bool) -> serde_json::Value {
    let mut out = REPORT.lock().ok().and_then(|r| r.clone()).unwrap_or_else(|| serde_json::json!({}));
    if let Ok(mut t) = TOTALS.lock() {
        out["baked"] = t.baked.into();
        out["bake_ms"] = ((t.bake_ms as f64 * 100.0).round() / 100.0).into();
        out["live_squares"] = t.live_squares.into();
        out["stretched_squares"] = t.stretched_squares.into();
        out["evicted"] = t.evicted.into();
        if reset {
            *t = Totals::default();
        }
    }
    out
}

/// How one square of a group was drawn in a view's last build, for the
/// render inspector (render-inspector.md §2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SquareKind {
    /// From its tile at the view's own scale.
    Tile,
    /// From tiles of another scale, stretched: while zooming, or until the
    /// view's own scale is baked.
    Stretched,
    /// Live, clipped: nothing cached covered it.
    Live,
    /// Split around a hot member drawn live between its neighbours' tiles.
    Split,
}

#[derive(Clone, Copy, Debug)]
pub struct Square {
    /// In the view's scene px (window px less the view's position).
    pub rect: Rect,
    pub kind: SquareKind,
    /// The tiling's scale, and the view's.
    pub scale: f64,
    pub view_scale: f64,
}

/// What the raster cache did lately, for the inspector's legend.
#[derive(Clone, Copy, Debug, Default)]
pub struct Summary {
    pub tiles: usize,
    pub pages: usize,
    pub budget_mb: usize,
    pub groups: usize,
    /// Over the last second.
    pub baked: u32,
    pub bake_ms: f32,
    pub live_squares: u32,
    pub stretched_squares: u32,
}

/// Each view's last picture, by its entity, and the summary.
static PICTURES: Mutex<Option<(HashMap<Entity, (bool, Vec<Square>)>, Summary)>> = Mutex::new(None);

/// How `view`'s groups were drawn in its last build, and whether it rasters
/// at all.
pub fn picture(view: Entity) -> Option<(bool, Vec<Square>)> {
    PICTURES.lock().ok()?.as_ref()?.0.get(&view).cloned()
}

pub fn summary() -> Summary {
    PICTURES.lock().ok().and_then(|p| p.as_ref().map(|p| p.1)).unwrap_or_default()
}

/// One layer of a view, as the raster planner sees it.
pub(super) struct Item<'a> {
    /// The chunk's key when it may be cached.
    pub cache: Option<u64>,
    /// Its commands' space to the view's scene px.
    pub base: Affine,
    pub list: &'a Arc<Recorded>,
}

/// Draws a list into a scene under `base`, culled to `cull` (scene px):
/// the backend's `emit_list`. Returns the commands emitted.
pub(super) type Emit<'a> = dyn FnMut(&mut Scene, &Recorded, Affine, Rect) -> u32 + 'a;

/// What a view's build cost.
#[derive(Default)]
pub(super) struct Built {
    pub emitted: u32,
    pub culled: u32,
    /// Of the build's time, what went into baking tiles.
    pub bake_ms: f32,
}

type ViewKey = Option<Entity>;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Slot {
    page: u16,
    slot: u16,
}

#[derive(Clone, Copy, Debug)]
enum Tile {
    /// Nothing of the group draws in this square.
    Empty,
    At(Slot),
}

/// A tiling's map from list space to bucket px: a linear part and a
/// sub-pixel phase, `translate(phase) · linear`. At rest the linear part is
/// the layer's own (its view's zoom, and its frame's rotation or flip, for a
/// chunk recorded in a transformed group) and the phase the fraction of a
/// pixel its placement lands on, so bucket px are scene px shifted by whole
/// pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct At {
    linear: Affine,
    phase: (f64, f64),
}

impl At {
    fn key(self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let [a, b, c, d, ..] = self.linear.as_coeffs();
        (a.to_bits(), b.to_bits(), c.to_bits(), d.to_bits(), self.phase.0.to_bits(), self.phase.1.to_bits()).hash(&mut h);
        h.finish()
    }

    /// Its smallest stretch: bucket px per list unit, at least.
    fn scale(self) -> f64 {
        min_scale(self.linear)
    }

    fn to_bucket(self) -> Affine {
        Affine::translate(self.phase) * self.linear
    }

    /// Bucket px to scene px, for a group whose list maps to scene px by
    /// `base`.
    fn to_scene(self, base: Affine) -> Affine {
        base * self.to_bucket().inverse()
    }

    fn bucket_rect(self, r: Rect) -> Rect {
        self.to_bucket().transform_rect_bbox(r)
    }

    /// Square `t`, in list space.
    fn tile_rect(self, t: (i32, i32)) -> Rect {
        self.to_bucket().inverse().transform_rect_bbox(Rect::new((t.0 * TILE) as f64, (t.1 * TILE) as f64, ((t.0 + 1) * TILE) as f64, ((t.1 + 1) * TILE) as f64))
    }
}

/// The fraction of a pixel `base` places list space's origin at: the phase
/// a tiling at the view's own scale bakes at, so it lands on whole pixels.
/// Exact, not rounded: `v - fract(v)` is then exactly `floor(v)`, and a
/// tile's texels sit on pixels with no resampling at all.
fn phase_of(base: Affine) -> (f64, f64) {
    let [.., x, y] = base.as_coeffs();
    (x - x.floor(), y - y.floor())
}

/// `base` without its translation.
fn linear(base: Affine) -> Affine {
    let [a, b, c, d, ..] = base.as_coeffs();
    Affine::new([a, b, c, d, 0.0, 0.0])
}

/// The tiling `base` draws texel for pixel at rest.
fn exact(base: Affine) -> At {
    At { linear: linear(base), phase: phase_of(base) }
}

/// The smaller singular value of an affine's linear part.
fn min_scale(m: Affine) -> f64 {
    let [a, b, c, d, ..] = m.as_coeffs();
    let (s, t) = (a * a + b * b + c * c + d * d, (a * d - b * c).abs());
    let disc = (s * s - 4.0 * t * t).max(0.0).sqrt();
    ((s - disc) / 2.0).max(0.0).sqrt()
}

/// How `m` relates to `of`, when it is `of` scaled uniformly: the scale.
/// `None` when it also turns, flips or skews it, which a tile cannot be
/// drawn under (its square would no longer be one).
fn ratio(m: Affine, of: Affine) -> Option<f64> {
    if of.determinant().abs() < 1e-18 {
        return None;
    }
    let [a, b, c, d, ..] = (linear(m) * linear(of).inverse()).as_coeffs();
    let tol = 1e-6 * a.abs().max(d.abs());
    (a > 0.0 && b.abs() <= tol && c.abs() <= tol && (a - d).abs() <= tol).then_some(a)
}

/// A group's tiles at one scale and phase.
struct Tiling {
    at: At,
    tiles: HashMap<(i32, i32), Tile>,
}

/// Whether a run's placement is changing: its scale (a zoom) or its phase
/// (a group dragged by fractions of a pixel), either drawing existing tiles
/// resampled instead of baking every frame; or its rotation or skew (a group
/// turned), which draws it live until it settles.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Motion {
    zooming: bool,
    moving: bool,
    turning: bool,
}

impl Motion {
    fn any(self) -> bool {
        self.zooming || self.moving
    }
}

/// A run's last placement, to tell motion: its linear part and phase, and
/// when each kind of change last happened.
#[derive(Clone, Copy, Debug)]
struct Placed {
    linear: Affine,
    phase: (f64, f64),
    scale_at: f64,
    phase_at: f64,
    turn_at: f64,
}

/// A group of chunks and its tilings, shared by every view that draws the
/// same chunks in the same order.
struct Group {
    members: Vec<u64>,
    /// By [`At::key`].
    tilings: HashMap<u64, Tiling>,
}

struct Owner {
    group: u64,
    tiling: u64,
    tile: (i32, i32),
}

struct Page {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    owner: Vec<Option<Owner>>,
    /// When each slot was last drawn: eviction takes the oldest, never one
    /// drawn this frame.
    used: Vec<f64>,
}

impl Page {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Page {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("raster atlas page"),
            size: wgpu::Extent3d { width: PAGE as u32, height: PAGE as u32, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Page { texture, view, owner: (0..SLOTS).map(|_| None).collect(), used: vec![f64::NEG_INFINITY; SLOTS] }
    }

    /// A slot's rectangle on the page, gutter included.
    fn rect(slot: u16) -> RectU16 {
        let (x, y) = ((slot as i32 % ROW) * SLOT, (slot as i32 / ROW) * SLOT);
        RectU16::new(x as u16, y as u16, (x + SLOT) as u16, (y + SLOT) as u16)
    }
}

struct ChunkState {
    list: Arc<Recorded>,
    changed_at: f64,
    bakeable: bool,
}

#[derive(Default)]
struct ViewState {
    rastering: bool,
    /// The groups this view drew last, by id.
    groups: Vec<u64>,
    /// Groups it wants but does not draw yet: baking until they cover it.
    wanted: Vec<u64>,
    /// The cold runs of its groups that it drew split around hot members.
    parts: Vec<u64>,
    /// Each run's placement, by its first chunk's key and which run with
    /// that first chunk it is: instance runs equal instances share
    /// (instance-cache.md phase C) start several runs with one key.
    placed: HashMap<(u64, u32), Placed>,
    pending: bool,
    /// The last build's picture, for the inspector.
    picture: Vec<Square>,
}

/// The atlas, the groups and every view's plan.
pub(super) struct Raster {
    origin: Instant,
    now: f64,
    format: wgpu::TextureFormat,
    pages: Vec<Page>,
    max_pages: usize,
    chunks: HashMap<u64, ChunkState>,
    ids: HashMap<Vec<u64>, u64>,
    groups: HashMap<u64, Group>,
    next_id: u64,
    views: HashMap<ViewKey, ViewState>,
    /// Bake time left this frame, and whether anything baked yet.
    bake_left: f32,
    baked_frame: u32,
    /// The view being built changed nothing but its raster work.
    idle: bool,
    /// Measured bake cost per command emitted, in µs: how a tile's cost is
    /// estimated before it is baked.
    us_per_cmd: f64,
    last_bake_cmds: u32,
    totals: Totals,
    /// [`ENABLED`] as of the last frame: a switch rebuilds every view.
    enabled: bool,
    /// This frame's baked tiles, each a slot-sized scene, rendered and
    /// copied into their slots by `render_pages`; and spent scenes.
    bakes: Vec<(Scene, Slot)>,
    pool: Vec<Scene>,
    /// Where a tile renders before it is copied into its slot.
    scratch: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// The last second's frames' counters, for [`summary`].
    recent: std::collections::VecDeque<(f64, Totals)>,
}

impl Raster {
    pub fn new(format: wgpu::TextureFormat) -> Raster {
        Raster {
            origin: Instant::now(),
            now: 0.0,
            format,
            pages: Vec::new(),
            max_pages: (RASTER_BUDGET / PAGE_BYTES) as usize,
            chunks: HashMap::default(),
            ids: HashMap::default(),
            groups: HashMap::default(),
            next_id: 1,
            views: HashMap::default(),
            bake_left: BAKE_MS,
            baked_frame: 0,
            idle: false,
            us_per_cmd: 2.0,
            last_bake_cmds: 0,
            totals: Totals::default(),
            enabled: true,
            bakes: Vec::new(),
            pool: Vec::new(),
            scratch: None,
            recent: std::collections::VecDeque::new(),
        }
    }

    /// Start a frame: note which chunks changed, and drop the tiles their
    /// changes damaged. `layers` is every cacheable layer in the render
    /// world.
    pub fn begin_frame<'a>(&mut self, layers: impl Iterator<Item = (u64, &'a Arc<Recorded>)>) {
        self.now = self.origin.elapsed().as_secs_f64();
        self.bake_left = BAKE_MS;
        self.baked_frame = 0;
        // A lower budget gives up the pages over it, and their tiles.
        self.max_pages = (BUDGET.load(Ordering::Relaxed) / PAGE_BYTES).max(1) as usize;
        if self.pages.len() > self.max_pages {
            for page in self.pages.drain(self.max_pages..) {
                for o in page.owner.into_iter().flatten() {
                    if let Some(t) = self.groups.get_mut(&o.group).and_then(|g| g.tilings.get_mut(&o.tiling)) {
                        t.tiles.remove(&o.tile);
                    }
                }
            }
            for v in self.views.values_mut() {
                v.pending = true;
            }
        }
        let enabled = ENABLED.load(Ordering::Relaxed);
        if enabled != self.enabled {
            self.enabled = enabled;
            for v in self.views.values_mut() {
                v.pending = true;
            }
        }
        let mut seen: HashSet<u64> = HashSet::default();
        let mut damaged: Vec<(u64, Rect)> = Vec::new();
        for (key, list) in layers {
            if !seen.insert(key) {
                continue;
            }
            match self.chunks.get_mut(&key) {
                Some(c) if Arc::ptr_eq(&c.list, list) => {}
                Some(c) => {
                    if let Some(d) = changed_bounds(&c.list, list) {
                        damaged.push((key, d));
                    }
                    c.list = list.clone();
                    c.changed_at = self.now;
                    c.bakeable = bakeable(list);
                }
                None => {
                    self.chunks.insert(key, ChunkState { list: list.clone(), changed_at: self.now, bakeable: bakeable(list) });
                }
            }
        }
        self.chunks.retain(|k, _| seen.contains(k));
        for (key, rect) in damaged {
            self.damage(key, rect);
        }
    }

    /// Drop every tile, in every group holding `key`, that `rect`
    /// (composition space) touches.
    fn damage(&mut self, key: u64, rect: Rect) {
        let mut freed = Vec::new();
        for g in self.groups.values_mut().filter(|g| g.members.contains(&key)) {
            for t in g.tilings.values_mut() {
                let (x0, y0, x1, y1) = tile_range(t.at.bucket_rect(rect).inflate(2.0, 2.0));
                t.tiles.retain(|&(tx, ty), tile| {
                    let hit = (x0..=x1).contains(&tx) && (y0..=y1).contains(&ty);
                    if hit && let Tile::At(s) = tile {
                        freed.push(*s);
                    }
                    !hit
                });
            }
        }
        for s in freed {
            self.pages[s.page as usize].owner[s.slot as usize] = None;
        }
    }

    /// Whether `view` must rebuild this frame though nothing it draws
    /// changed: it has raster work left.
    pub fn wants_rebuild(&self, view: ViewKey) -> bool {
        self.views.get(&view).is_some_and(|v| v.pending)
    }

    /// Forget views that are gone.
    pub fn retain_views(&mut self, live: impl Fn(ViewKey) -> bool) {
        self.views.retain(|k, _| live(*k));
    }

    /// Build one view's scene from its layers in painter order: hot and
    /// uncacheable layers live, cold groups from tiles. `idle`: nothing the
    /// view draws changed; it builds only for its raster work.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_view(&mut self, key: ViewKey, items: &[Item<'_>], viewport: Rect, idle: bool, device: &wgpu::Device, scene: &mut Scene, pages: &mut Vec<u16>, emit: &mut Emit<'_>) -> Built {
        let now = self.now;
        self.idle = idle;
        let mut built = Built::default();
        let mut vs = self.views.remove(&key).unwrap_or_default();
        vs.pending = false;
        vs.picture.clear();
        vs.parts.clear();

        // Runs of cacheable layers that share one pan-and-zoom placement.
        let mut runs: Vec<std::ops::Range<usize>> = Vec::new();
        for (i, it) in items.iter().enumerate() {
            if it.cache.is_none() || min_scale(it.base) < 1e-6 {
                continue;
            }
            match runs.last_mut() {
                Some(r) if r.end == i && items[r.start].base == it.base => r.end = i + 1,
                _ => runs.push(i..i + 1),
            }
        }
        // Each run's motion, by its first chunk: a scale or phase that
        // changed within [`ZOOM_SETTLE`].
        let mut motions: HashMap<usize, Motion> = HashMap::default();
        let mut placed = HashMap::default();
        let mut nth: HashMap<u64, u32> = HashMap::default();
        for r in &runs {
            let base = items[r.start].base;
            let (m, phase) = (linear(base), phase_of(base));
            let first = items[r.start].cache.expect("cacheable");
            let n = nth.entry(first).or_default();
            let key = (first, *n);
            *n += 1;
            let never = f64::NEG_INFINITY;
            let mut p = vs.placed.get(&key).copied().unwrap_or(Placed { linear: m, phase, scale_at: never, phase_at: never, turn_at: never });
            if p.linear != m {
                if ratio(m, p.linear).is_some() {
                    p.scale_at = now;
                } else {
                    p.turn_at = now;
                }
                p.linear = m;
            }
            if p.phase != phase {
                p.phase = phase;
                p.phase_at = now;
            }
            placed.insert(key, p);
            motions.insert(r.start, Motion { zooming: now - p.scale_at < ZOOM_SETTLE, moving: now - p.phase_at < ZOOM_SETTLE, turning: now - p.turn_at < ZOOM_SETTLE });
        }
        vs.placed = placed;
        let motion_at = |i: usize| runs.iter().find(|r| r.contains(&i)).and_then(|r| motions.get(&r.start).copied()).unwrap_or_default();

        // What the view would like: each run of bakeable chunks a group.
        // Its hot chunks stay in it, drawn live where they are, so a chunk
        // turning hot or cold changes no grouping (`draw_group`).
        let visible = |it: &Item<'_>| it.list.bounds().map(|b| !it.base.transform_rect_bbox(b).intersect(viewport).is_zero_area()).unwrap_or(false);
        let mut desired: Vec<std::ops::Range<usize>> = Vec::new();
        let mut cold_cmds = 0_usize;
        let mut hot_soon = false;
        // How often each chunk appears: an instance run equal instances share
        // (instance-cache.md phase C) is one group drawn at each of them, so
        // it is worth baking when all its uses together are.
        let mut uses: HashMap<u64, usize> = HashMap::default();
        for k in items.iter().filter_map(|it| it.cache) {
            *uses.entry(k).or_default() += 1;
        }
        for run in runs.iter().filter(|r| !motions.get(&r.start).is_some_and(|m| m.turning)) {
            let mut start: Option<usize> = None;
            let mut cmds = 0_usize;
            for i in run.clone().chain(std::iter::once(run.end)) {
                let state = (i < run.end).then(|| self.chunks.get(&items[i].cache.unwrap())).flatten();
                if let Some(c) = state.filter(|c| c.bakeable) {
                    start.get_or_insert(i);
                    if now - c.changed_at < HOT {
                        hot_soon = true;
                    } else if visible(&items[i]) {
                        cmds += items[i].list.cmds().len();
                    }
                } else if let Some(s) = start.take() {
                    let shared = uses.get(&items[s].cache.expect("cacheable")).copied().unwrap_or(1);
                    if cmds * shared >= MIN_GROUP {
                        desired.push(s..i);
                        cold_cmds += cmds;
                    }
                    cmds = 0;
                }
            }
        }
        if vs.rastering {
            vs.rastering = cold_cmds >= LIVE_OFF;
        } else {
            vs.rastering = cold_cmds > LIVE_OK;
        }
        if !self.enabled {
            vs.rastering = false;
            vs.groups.clear();
        }
        if !vs.rastering {
            desired.clear();
        }
        vs.pending |= hot_soon && (vs.rastering || cold_cmds > LIVE_OFF);

        // The plan: desired groups that are ready, or that replace nothing;
        // otherwise the groups drawn last time, while they still fit.
        let mut wanted: Vec<(u64, std::ops::Range<usize>)> = Vec::new();
        for r in &desired {
            let id = self.group_id(&items[r.clone()]);
            wanted.push((id, r.clone()));
        }
        let current: Vec<(u64, std::ops::Range<usize>)> = vs.groups.iter().filter_map(|&id| Some((id, self.place(id, items, &runs)?))).collect();
        let overlaps = |a: &std::ops::Range<usize>, b: &std::ops::Range<usize>| a.start < b.end && b.start < a.end;
        let mut plan: Vec<(u64, std::ops::Range<usize>, bool)> = Vec::new();
        for (id, r) in &wanted {
            let kept = current.iter().any(|(c, _)| c == id);
            let replaces = current.iter().any(|(c, cr)| c != id && overlaps(cr, r));
            if kept || !replaces || self.missing(*id, &items[r.clone()], viewport, motion_at(r.start)) == 0 {
                plan.push((*id, r.clone(), true));
            }
        }
        // An old group stays only while a wanted group that would replace
        // it is still baking; one nothing wants any more (the view went live,
        // its chunks were regrouped away) is dropped, and its chunks draw live.
        for (id, r) in &current {
            let replacing = wanted.iter().any(|(w, wr)| w != id && overlaps(wr, r));
            if replacing && !plan.iter().any(|(_, pr, _)| overlaps(pr, r)) {
                plan.push((*id, r.clone(), false));
            }
        }
        plan.sort_by_key(|(_, r, _)| r.start);
        if plan.len() != wanted.len() || plan.iter().any(|(_, _, w)| !w) {
            vs.pending = true;
        }
        vs.groups = plan.iter().map(|(id, ..)| *id).collect();

        // Draw in painter order.
        let mut i = 0;
        let mut next = plan.iter().peekable();
        while i < items.len() {
            if let Some((id, r, wanted)) = next.peek().filter(|(_, r, _)| r.start == i) {
                let (id, r, wanted) = (*id, r.clone(), *wanted);
                next.next();
                built.emitted += self.draw_group(id, &items[r.clone()], viewport, motion_at(r.start), wanted, device, scene, pages, emit, &mut vs, &mut built);
                i = r.end;
                continue;
            }
            let it = &items[i];
            if visible(it) {
                built.emitted += emit(scene, it.list, it.base, viewport);
            } else if it.list.bounds().is_some() {
                built.culled += 1;
            }
            i += 1;
        }

        // Wanted groups waiting to replace what is drawn bake in the
        // background, so the switch can happen.
        vs.wanted.clear();
        for (id, r) in &wanted {
            if plan.iter().any(|(p, ..)| p == id) {
                continue;
            }
            vs.wanted.push(*id);
            let members = &items[r.clone()];
            let base = members[0].base;
            let at = self.choose(*id, base, motion_at(r.start));
            self.groups.get_mut(id).expect("wanted group").tilings.entry(at.key()).or_insert_with(|| Tiling { at, tiles: HashMap::default() });
            self.bake_within_budget(*id, at, squares(members, base, at, viewport), members, device, emit, &mut built);
        }

        // Groups no view draws or wants any more give their tiles back.
        self.views.insert(key, vs);
        self.collect();
        built
    }

    /// Where group `id` sits in `items`: its members, in order, contiguous
    /// within one run; `None` when it no longer fits.
    fn place(&self, id: u64, items: &[Item<'_>], runs: &[std::ops::Range<usize>]) -> Option<std::ops::Range<usize>> {
        let g = self.groups.get(&id)?;
        let start = items.iter().position(|it| it.cache == Some(g.members[0]))?;
        let run = runs.iter().find(|r| r.contains(&start))?;
        let end = start + g.members.len();
        (end <= run.end && items[start..end].iter().zip(&g.members).all(|(it, m)| it.cache == Some(*m))).then_some(start..end)
    }

    /// The tiling a group draws from: the exact one at rest (the view's
    /// scale, its placement's phase); while it zooms or moves, the nearest
    /// it already has within [`STRETCH`], else for a zoom the view's scale
    /// rounded up to a √2 step, so a gesture's bakes are reused, and for a
    /// move the exact one.
    fn choose(&self, id: u64, base: Affine, motion: Motion) -> At {
        let exact = exact(base);
        if !motion.any() {
            return exact;
        }
        let near = self.groups.get(&id).and_then(|g| {
            g.tilings
                .values()
                .filter(|t| !t.tiles.is_empty())
                .filter_map(|t| Some((t.at, ratio(exact.linear, t.at.linear).filter(|r| (STRETCH.0..=STRETCH.1).contains(r))?)))
                .min_by(|x, y| x.1.ln().abs().total_cmp(&y.1.ln().abs()).then((x.0 != exact).cmp(&(y.0 != exact))))
                .map(|(at, _)| at)
        });
        near.unwrap_or_else(|| {
            if motion.zooming {
                let a = exact.scale();
                let step = 2f64.powf((a.log2() * 2.0).ceil() / 2.0);
                At { linear: exact.linear * Affine::scale(step / a), phase: (0.0, 0.0) }
            } else {
                exact
            }
        })
    }

    /// Visible squares group `id` has no tile for, at the scale it would
    /// draw at.
    fn missing(&self, id: u64, members: &[Item<'_>], viewport: Rect, motion: Motion) -> usize {
        let base = members[0].base;
        let at = self.choose(id, base, motion);
        let tiling = self.groups.get(&id).and_then(|g| g.tilings.get(&at.key()));
        squares(members, base, at, viewport).iter().filter(|t| !tiling.is_some_and(|tl| tl.tiles.contains_key(*t))).count()
    }

    /// A group's id, made on first use: groups are shared by every view
    /// that draws the same chunks in the same order.
    fn group_id(&mut self, members: &[Item<'_>]) -> u64 {
        let keys: Vec<u64> = members.iter().map(|m| m.cache.expect("cacheable")).collect();
        if let Some(&id) = self.ids.get(&keys) {
            return id;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.ids.insert(keys.clone(), id);
        self.groups.insert(id, Group { members: keys, tilings: HashMap::default() });
        id
    }

    /// Draw one group. Squares no hot member touches draw from the group's
    /// own tiles. Squares a hot member touches are split: the cold runs
    /// between hot members draw from tiles of their own (baked only for
    /// those squares), with each hot member live in between, in order. So
    /// a dragged shape costs its own encode and the bakes under it, not a
    /// rebake of everything above and below it.
    #[allow(clippy::too_many_arguments)]
    fn draw_group(&mut self, id: u64, members: &[Item<'_>], viewport: Rect, motion: Motion, wanted: bool, device: &wgpu::Device, scene: &mut Scene, pages: &mut Vec<u16>, emit: &mut Emit<'_>, vs: &mut ViewState, built: &mut Built) -> u32 {
        let now = self.now;
        let base = members[0].base;
        let a = exact(base).scale();
        let at = self.choose(id, base, motion);
        // How much the view magnifies the tiling: 1 at rest.
        let stretch = ratio(linear(base), at.linear).unwrap_or(1.0);
        let exact = at == exact(base);
        let todo = squares(members, base, at, viewport);
        let hot: Vec<usize> = (0..members.len()).filter(|&i| members[i].cache.and_then(|k| self.chunks.get(&k)).is_some_and(|c| now - c.changed_at < HOT)).collect();
        let pad = (GUTTER as f64 + 2.0) / at.scale();
        let hot_bounds: Vec<Rect> = hot.iter().filter_map(|&i| members[i].list.bounds()).collect();
        let (split, whole): (Vec<(i32, i32)>, Vec<(i32, i32)>) = todo.iter().partition(|&&t| {
            let r = at.tile_rect(t).inflate(pad, pad);
            hot_bounds.iter().any(|b| !b.intersect(r).is_zero_area())
        });
        let mut picture: Vec<(Rect, SquareKind)> = Vec::new();
        let mut drawn = self.draw_squares(id, members, at, &whole, viewport, wanted, motion.any(), device, scene, pages, emit, vs, built, &mut picture);
        if !split.is_empty() {
            vs.pending = true;
            let mut from = 0;
            for &h in hot.iter().chain(std::iter::once(&members.len())) {
                if from < h {
                    let part = &members[from..h];
                    let pid = self.group_id(part);
                    vs.parts.push(pid);
                    drawn += self.draw_squares(pid, part, at, &split, viewport, wanted, motion.any(), device, scene, pages, emit, vs, built, &mut Vec::new());
                }
                if h < members.len() {
                    let m = &members[h];
                    if m.list.bounds().is_some_and(|b| !m.base.transform_rect_bbox(b).intersect(viewport).is_zero_area()) {
                        drawn += emit(scene, m.list, m.base, viewport);
                    }
                }
                from = h + 1;
            }
            let to_scene = at.to_scene(base);
            picture.extend(split.iter().map(|&t| (device_rect(to_scene, t), SquareKind::Split)));
        }
        // Spare budget prefetches a ring of one tile just outside the view.
        if wanted && hot.is_empty() {
            let w = TILE as f64 * stretch;
            self.bake_within_budget(id, at, squares(members, base, at, viewport.inflate(w, w)), members, device, emit, built);
        }
        if motion.any() || !exact {
            vs.pending = true;
        }
        // A tile of another scale or phase than the view's is drawn
        // resampled.
        vs.picture.extend(picture.into_iter().map(|(rect, kind)| Square { rect, kind: if kind == SquareKind::Tile && !exact { SquareKind::Stretched } else { kind }, scale: at.scale(), view_scale: a }));
        drawn
    }

    /// Draw squares `todo` of group `id` at `scale`: each from its tile;
    /// else baked now if the budget allows; else from another scale's tiles,
    /// stretched; else, when the view wants the group, baked now anyway
    /// (drawing it live would cost the same); else live, clipped.
    #[allow(clippy::too_many_arguments)]
    fn draw_squares(&mut self, id: u64, members: &[Item<'_>], at: At, todo: &[(i32, i32)], viewport: Rect, wanted: bool, settling: bool, device: &wgpu::Device, scene: &mut Scene, pages: &mut Vec<u16>, emit: &mut Emit<'_>, vs: &mut ViewState, built: &mut Built, picture: &mut Vec<(Rect, SquareKind)>) -> u32 {
        let base = members[0].base;
        let key = at.key();
        self.groups.get_mut(&id).expect("drawn group").tilings.entry(key).or_insert_with(|| Tiling { at, tiles: HashMap::default() });
        let to_scene = at.to_scene(base);
        let mut missing = Vec::new();
        let mut drawn = 0_u32;
        for &t in todo {
            let tile = match self.tile(id, key, t) {
                Some(tile) => Some(tile),
                None if wanted && self.affordable(members, t, at) => self.timed_bake(id, at, t, members, device, emit, built),
                None => None,
            };
            match tile {
                Some(tile) => {
                    drawn += self.quad(tile, to_scene, t, viewport, viewport, scene, pages);
                    picture.push((device_rect(to_scene, t), SquareKind::Tile));
                }
                None => missing.push(t),
            }
        }
        if missing.is_empty() {
            return drawn;
        }
        if wanted || settling {
            vs.pending = true;
        }
        // Only scales a stretch away: further, the fallback would show
        // blocky or mushy pixels, so the square bakes (or draws live).
        let mut others: Vec<(u64, At)> = self.groups[&id]
            .tilings
            .iter()
            .filter(|(k, t)| **k != key && !t.tiles.is_empty() && ratio(linear(base), t.at.linear).is_some_and(|r| (STRETCH.0..=STRETCH.1).contains(&r)))
            .map(|(k, t)| (*k, t.at))
            .collect();
        let off = |at: &At| ratio(linear(base), at.linear).unwrap_or(1.0).ln().abs();
        others.sort_by(|x, y| off(&x.1).total_cmp(&off(&y.1)));
        let mut live: Vec<Rect> = Vec::new();
        'square: for &t in &missing {
            let r = device_rect(to_scene, t).intersect(viewport);
            if r.is_zero_area() {
                continue;
            }
            for &(okey, oat) in &others {
                let o_scene = oat.to_scene(base);
                let (x0, y0, x1, y1) = tile_range(o_scene.inverse().transform_rect_bbox(r));
                let cover: Option<Vec<((i32, i32), Tile)>> = (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| (x, y))).map(|ot| Some((ot, self.tile(id, okey, ot)?))).collect();
                if let Some(cover) = cover {
                    for (ot, tile) in cover {
                        drawn += self.quad(tile, o_scene, ot, r, viewport, scene, pages);
                    }
                    self.totals.stretched_squares += 1;
                    picture.push((device_rect(to_scene, t), SquareKind::Stretched));
                    continue 'square;
                }
            }
            if wanted
                && let Some(tile) = self.timed_bake(id, at, t, members, device, emit, built)
            {
                drawn += self.quad(tile, to_scene, t, viewport, viewport, scene, pages);
                picture.push((device_rect(to_scene, t), SquareKind::Tile));
                continue;
            }
            live.push(r);
            picture.push((device_rect(to_scene, t), SquareKind::Live));
        }
        if !live.is_empty() {
            self.totals.live_squares += live.len() as u32;
            let mut clip = BezPath::new();
            let mut cull = live[0];
            for r in &live {
                clip.extend(r.path_elements(0.1));
                cull = cull.union(*r);
            }
            scene.reset_transform();
            scene.push_clip_path(&clip);
            for m in members {
                if m.list.bounds().is_some_and(|b| !m.base.transform_rect_bbox(b).intersect(cull).is_zero_area()) {
                    drawn += emit(scene, m.list, m.base, cull);
                }
            }
            scene.pop_clip();
        }
        drawn
    }

    /// Bake the squares of `todo` that have no tile yet, while this frame's
    /// budget lasts.
    #[allow(clippy::too_many_arguments)]
    fn bake_within_budget(&mut self, id: u64, at: At, todo: Vec<(i32, i32)>, members: &[Item<'_>], device: &wgpu::Device, emit: &mut Emit<'_>, built: &mut Built) {
        for t in todo {
            if self.bake_left <= 0.0 {
                break;
            }
            if self.tile(id, at.key(), t).is_none() && self.affordable(members, t, at) {
                self.timed_bake(id, at, t, members, device, emit, built);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn timed_bake(&mut self, id: u64, at: At, t: (i32, i32), members: &[Item<'_>], device: &wgpu::Device, emit: &mut Emit<'_>, built: &mut Built) -> Option<Tile> {
        let started = Instant::now();
        let tile = self.bake(id, at, t, members, device, emit);
        let ms = started.elapsed().as_secs_f32() * 1000.0;
        if self.last_bake_cmds > 50 {
            let us = ms as f64 * 1000.0 / self.last_bake_cmds as f64;
            self.us_per_cmd = self.us_per_cmd * 0.8 + us * 0.2;
        }
        self.bake_left -= ms;
        self.baked_frame += 1;
        self.totals.baked += 1;
        self.totals.bake_ms += ms;
        built.bake_ms += ms;
        tile
    }

    fn tile(&self, id: u64, key: u64, t: (i32, i32)) -> Option<Tile> {
        self.groups.get(&id)?.tilings.get(&key)?.tiles.get(&t).copied()
    }

    /// Draw one tile as a pixel-snapped rect within `within`, sampled through
    /// its slot. Returns 1 when it drew.
    #[allow(clippy::too_many_arguments)]
    fn quad(&mut self, tile: Tile, to_scene: Affine, t: (i32, i32), within: Rect, viewport: Rect, scene: &mut Scene, pages: &mut Vec<u16>) -> u32 {
        let Tile::At(s) = tile else { return 0 };
        let r = device_rect(to_scene, t).intersect(within).intersect(viewport);
        if r.is_zero_area() {
            return 0;
        }
        let page = &mut self.pages[s.page as usize];
        page.used[s.slot as usize] = self.now;
        if !pages.contains(&s.page) {
            pages.push(s.page);
        }
        let slot = Page::rect(s.slot);
        scene.reset_transform();
        scene.set_paint(VelloImage {
            image: ImageSource::external_texture(TextureId(PAGE_TEXTURE_BASE + s.page as u64), slot, true),
            sampler: peniko::ImageSampler::default(),
        });
        // Image px (from the slot's corner) → bucket px → scene px.
        scene.set_paint_transform(to_scene * Affine::translate(((t.0 * TILE - GUTTER) as f64, (t.1 * TILE - GUTTER) as f64)));
        scene.fill_rect(&r);
        scene.reset_paint_transform();
        1
    }

    /// Whether square `t` would bake within what is left of the budget, by
    /// its commands at the measured rate; any tile, in an idle view's first
    /// bake of the frame.
    fn affordable(&self, members: &[Item<'_>], t: (i32, i32), at: At) -> bool {
        if self.idle && self.baked_frame == 0 {
            return true;
        }
        let limit = (self.bake_left as f64 * 1000.0 / self.us_per_cmd).max(0.0) as usize;
        let pad = (GUTTER as f64 + 2.0) / at.scale();
        let reach = at.tile_rect(t).inflate(pad, pad);
        let mut n = 0;
        for m in members.iter().filter(|m| m.list.bounds().is_some_and(|b| !b.intersect(reach).is_zero_area())) {
            n += m.list.cmd_bounds().iter().filter(|b| b.is_some_and(|b| !b.intersect(reach).is_zero_area())).count();
            if n > limit {
                return false;
            }
        }
        true
    }

    /// Bake square `t` of group `id` at `scale` onto a page. `None` when no
    /// slot is free and none can be evicted.
    fn bake(&mut self, id: u64, at: At, t: (i32, i32), members: &[Item<'_>], device: &wgpu::Device, emit: &mut Emit<'_>) -> Option<Tile> {
        let key = at.key();
        let square = at.tile_rect(t);
        let pad = (GUTTER as f64 + 2.0) / at.scale();
        let reach = square.inflate(pad, pad);
        let touches = |m: &Item<'_>| m.list.bounds().is_some_and(|b| !b.intersect(reach).is_zero_area());
        let mut tile = Tile::Empty;
        if members.iter().any(touches) {
            let mut scene = self.pool.pop().unwrap_or_else(|| Scene::new(SLOT as u16, SLOT as u16));
            // List space → slot px: the square's corner lands inside the
            // gutter.
            let to_slot = Affine::translate(((GUTTER - t.0 * TILE) as f64, (GUTTER - t.1 * TILE) as f64)) * at.to_bucket();
            let cull = Rect::new(0.0, 0.0, SLOT as f64, SLOT as f64).inflate(2.0, 2.0);
            let mut n = 0;
            for m in members.iter().filter(|m| touches(m)) {
                n += emit(&mut scene, m.list, to_slot, cull);
            }
            self.last_bake_cmds = n;
            let slot = if n > 0 { self.alloc(device) } else { None };
            match slot {
                Some(s) => {
                    let page = &mut self.pages[s.page as usize];
                    page.owner[s.slot as usize] = Some(Owner { group: id, tiling: key, tile: t });
                    page.used[s.slot as usize] = self.now;
                    self.bakes.push((scene, s));
                    tile = Tile::At(s);
                }
                None => {
                    scene.reset();
                    self.pool.push(scene);
                    if n > 0 {
                        // Nowhere to put it: the square draws live.
                        return None;
                    }
                }
            }
        }
        self.groups.get_mut(&id)?.tilings.get_mut(&key)?.tiles.insert(t, tile);
        Some(tile)
    }

    /// A free slot: on a page with room, on a new page within the budget, or
    /// the least recently drawn one, taken from its tile.
    fn alloc(&mut self, device: &wgpu::Device) -> Option<Slot> {
        for (p, page) in self.pages.iter().enumerate() {
            if let Some(i) = page.owner.iter().position(Option::is_none) {
                return Some(Slot { page: p as u16, slot: i as u16 });
            }
        }
        if self.pages.len() < self.max_pages {
            self.pages.push(Page::new(device, self.format));
            return Some(Slot { page: (self.pages.len() - 1) as u16, slot: 0 });
        }
        let now = self.now;
        let (p, i) = self
            .pages
            .iter()
            .enumerate()
            .flat_map(|(p, page)| page.used.iter().enumerate().map(move |(i, u)| (p, i, *u)))
            .filter(|(.., u)| *u < now)
            .min_by(|x, y| x.2.total_cmp(&y.2))
            .map(|(p, i, _)| (p, i))?;
        if let Some(o) = self.pages[p].owner[i].take()
            && let Some(t) = self.groups.get_mut(&o.group).and_then(|g| g.tilings.get_mut(&o.tiling))
        {
            t.tiles.remove(&o.tile);
        }
        self.totals.evicted += 1;
        Some(Slot { page: p as u16, slot: i as u16 })
    }

    /// Drop groups that no view draws or wants, and give their slots back.
    fn collect(&mut self) {
        let mut keep: HashSet<u64> = HashSet::default();
        for v in self.views.values() {
            keep.extend(v.groups.iter().chain(&v.wanted).chain(&v.parts).copied());
        }
        let gone: Vec<u64> = self.groups.keys().copied().filter(|id| !keep.contains(id)).collect();
        for id in gone {
            if let Some(g) = self.groups.remove(&id) {
                self.ids.remove(&g.members);
                for t in g.tilings.values() {
                    for tile in t.tiles.values() {
                        if let Tile::At(s) = tile {
                            self.pages[s.page as usize].owner[s.slot as usize] = None;
                        }
                    }
                }
            }
        }
    }

    /// End a frame: publish what is pending and the report.
    pub fn end_frame(&mut self) {
        PENDING.store(self.views.values().any(|v| v.pending), Ordering::Relaxed);
        if let Ok(mut t) = TOTALS.lock() {
            t.baked += self.totals.baked;
            t.bake_ms += self.totals.bake_ms;
            t.live_squares += self.totals.live_squares;
            t.stretched_squares += self.totals.stretched_squares;
            t.evicted += self.totals.evicted;
        }
        self.recent.push_back((self.now, self.totals));
        while self.recent.front().is_some_and(|(t, _)| self.now - t > 1.0) {
            self.recent.pop_front();
        }
        self.totals = Totals::default();
        let tiles: usize = self.pages.iter().map(|p| p.owner.iter().filter(|o| o.is_some()).count()).sum();
        let mut summary = Summary { tiles, pages: self.pages.len(), budget_mb: (BUDGET.load(Ordering::Relaxed) >> 20) as usize, groups: self.groups.len(), ..Default::default() };
        for (_, t) in &self.recent {
            summary.baked += t.baked;
            summary.bake_ms += t.bake_ms;
            summary.live_squares += t.live_squares;
            summary.stretched_squares += t.stretched_squares;
        }
        if let Ok(mut p) = PICTURES.lock() {
            *p = Some((self.views.iter().filter_map(|(k, v)| Some(((*k)?, (v.rastering, v.picture.clone())))).collect(), summary));
        }
        let kind = |k: SquareKind| match k {
            SquareKind::Tile => "tile",
            SquareKind::Stretched => "stretched",
            SquareKind::Live => "live",
            SquareKind::Split => "split",
        };
        let views: serde_json::Map<String, serde_json::Value> = self
            .views
            .iter()
            .map(|(k, v)| {
                let mut counts = serde_json::Map::new();
                for sq in &v.picture {
                    let e = counts.entry(kind(sq.kind)).or_insert(0.into());
                    *e = (e.as_u64().unwrap_or(0) + 1).into();
                }
                (k.map_or("window".into(), |e| e.to_bits().to_string()), serde_json::json!({ "rastering": v.rastering, "pending": v.pending, "squares": counts }))
            })
            .collect();
        if let Ok(mut r) = REPORT.lock() {
            *r = Some(serde_json::json!({
                "pages": self.pages.len(),
                "tiles": tiles,
                "groups": self.groups.len(),
                "mb": self.pages.len() * 16,
                "views": views,
            }));
        }
    }

    /// Render this frame's baked tiles into their slots, before any view
    /// samples them: each into the scratch texture, then copied. One
    /// submission each, since consecutive renders share the renderer's
    /// upload buffers.
    pub fn render_pages(&mut self, renderer: &mut Renderer, resources: &mut Resources, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.bakes.is_empty() {
            return;
        }
        let format = self.format;
        let (scratch, scratch_view) = self.scratch.get_or_insert_with(|| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("raster tile scratch"),
                size: wgpu::Extent3d { width: SLOT as u32, height: SLOT as u32, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            (texture, view)
        });
        for (mut scene, s) in self.bakes.drain(..) {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("raster bake") });
            match renderer.render(
                &scene,
                resources,
                device,
                queue,
                &mut encoder,
                &RenderSize { width: SLOT as u16, height: SLOT as u16 },
                scratch_view,
                None,
                &TextureBindings::new(),
                TargetInit::Clear(ClearSettings::default()),
            ) {
                Ok(()) => {
                    let slot = Page::rect(s.slot);
                    encoder.copy_texture_to_texture(
                        wgpu::TexelCopyTextureInfo { texture: scratch, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                        wgpu::TexelCopyTextureInfo { texture: &self.pages[s.page as usize].texture, mip_level: 0, origin: wgpu::Origin3d { x: slot.x0 as u32, y: slot.y0 as u32, z: 0 }, aspect: wgpu::TextureAspect::All },
                        wgpu::Extent3d { width: SLOT as u32, height: SLOT as u32, depth_or_array_layers: 1 },
                    );
                }
                Err(err) => bevy::log::error!("raster bake failed: {err}"),
            }
            queue.submit([encoder.finish()]);
            scene.reset();
            if self.pool.len() < POOL {
                self.pool.push(scene);
            }
        }
    }

    /// Bind the pages a view's scene samples.
    pub fn bind(&self, pages: &[u16], bindings: &mut TextureBindings) {
        for &p in pages {
            if let Some(page) = self.pages.get(p as usize) {
                bindings.insert(TextureId(PAGE_TEXTURE_BASE + p as u64), page.view.clone());
            }
        }
    }

}

/// Whether a list can be baked: it draws no texture (a 3D view, changing
/// every frame), no filter (a drop shadow reads beyond a tile), and no blend
/// that reads what is under it.
fn bakeable(list: &Recorded) -> bool {
    !list.cmds().iter().any(|c| matches!(c, DrawCmd::Texture { .. } | DrawCmd::PushShadow { .. } | DrawCmd::PushBlend { .. }))
}

/// What a recording change could have repainted, in composition space: the
/// commands that differ, old and new, when the lists line up; the union of
/// both otherwise.
fn changed_bounds(old: &Recorded, new: &Recorded) -> Option<Rect> {
    let whole = || match (old.bounds(), new.bounds()) {
        (Some(a), Some(b)) => Some(a.union(b)),
        (a, b) => a.or(b),
    };
    if old.cmds().len() != new.cmds().len() {
        return whole();
    }
    let mut out: Option<Rect> = None;
    for i in 0..old.cmds().len() {
        if old.cmds()[i] == new.cmds()[i] {
            continue;
        }
        match (old.cmd_bounds()[i], new.cmd_bounds()[i]) {
            (Some(a), Some(b)) => {
                let r = a.union(b);
                out = Some(out.map_or(r, |o| o.union(r)));
            }
            // A scope changed (an opacity): everything inside it may have.
            _ => return whole(),
        }
    }
    out
}

/// The tiles a bucket-px rectangle touches, inclusive.
fn tile_range(r: Rect) -> (i32, i32, i32, i32) {
    let t = TILE as f64;
    ((r.x0 / t).floor() as i32, (r.y0 / t).floor() as i32, ((r.x1 / t).ceil() as i32) - 1, ((r.y1 / t).ceil() as i32) - 1)
}

/// Square `t` in scene px, its edges rounded so neighbours share them.
fn device_rect(to_scene: Affine, t: (i32, i32)) -> Rect {
    let r = to_scene.transform_rect_bbox(Rect::new((t.0 * TILE) as f64, (t.1 * TILE) as f64, ((t.0 + 1) * TILE) as f64, ((t.1 + 1) * TILE) as f64));
    Rect::new(r.x0.round(), r.y0.round(), r.x1.round(), r.y1.round())
}

/// The squares of tiling `at` that show in `viewport` and hold some of the
/// members' content, nearest the viewport's centre first.
fn squares(members: &[Item<'_>], base: Affine, at: At, viewport: Rect) -> Vec<(i32, i32)> {
    let Some(content) = members.iter().filter_map(|m| m.list.bounds()).reduce(|a, b| a.union(b)) else { return Vec::new() };
    let seen = base.inverse().transform_rect_bbox(viewport).intersect(content);
    if seen.is_zero_area() {
        return Vec::new();
    }
    let (x0, y0, x1, y1) = tile_range(at.bucket_rect(seen));
    let centre = at.bucket_rect(base.inverse().transform_rect_bbox(viewport)).center();
    let mut out: Vec<(i32, i32)> = (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| (x, y))).collect();
    let t = TILE as f64;
    out.sort_by(|a, b| {
        let d = |p: &(i32, i32)| ((p.0 as f64 + 0.5) * t - centre.x).hypot((p.1 as f64 + 0.5) * t - centre.y);
        d(a).total_cmp(&d(b))
    });
    out
}
