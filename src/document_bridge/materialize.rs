//! Node → draw data, and node → entity where an entity is due. One function
//! per node type, and a reconcile pass that rewrites what a set of touched
//! nodes derives: every drawable 2D node's [`Doc2d`] in [`Drawables`], and
//! an entity for the nodes Bevy acts on (vision 10 §5): every 3D node, and
//! the 2D nodes in a **Live** subtree ([`Modes`]: the selection's and the
//! hover's). Everything else is drawn, picked and edited from the store and
//! the scene without an entity.

use super::{CompositionLayers, InComposition, NodeMap, Provenance};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use iron_document::{FacetKind, LeafPath, NodeId, Store, TypeId, Value};
use std::collections::BTreeSet;
use bevy::platform::collections::HashMap;

use crate::bevy_app::scene3d::{ActiveState, Shape};

/// Drawable 2D node: what the document layer emits for it. Geometry is in the
/// node's local space; placement is the store's world affine (vision 10 §3:
/// a 2D entity carries no transform, the scene and the store place it).
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
    /// Text (plans/specs/text.md): its box spans `(0, 0)` to `(w, h)`, as a
    /// rect's does, and is what picking and selection use; `glyphs` are
    /// every glyph's outline in the same space, what fill, stroke and
    /// shadow paint. `layout` is kept for the editor's caret.
    /// `paints` are what is drawn (outline glyphs by colour, decorations,
    /// colour glyphs), placed by `offset` like the glyphs.
    Text { w: f32, h: f32, glyphs: std::sync::Arc<kurbo::BezPath>, paints: std::sync::Arc<Vec<iron_text::TextPaint>>, layout: std::sync::Arc<iron_text::TextLayout>, offset: [f32; 2] },
    /// A table (plans/specs/layout-and-tables.md §3): its box spans `(0, 0)`
    /// to `(w, h)`; `decor` is what it draws under its cells, its bands and
    /// lines (`iron_document::table::decor`).
    Table { w: f32, h: f32, decor: std::sync::Arc<iron_document::table::Decor> },
}

/// Every drawable 2D node's draw data, by node, whether or not the node has
/// an entity. The reconciler keeps it current; the draw walk, picking, the
/// selection overlay and the tools read it.
#[derive(Resource, Default)]
pub struct Drawables(pub HashMap<NodeId, Doc2d>);

/// Which 2D subtrees are **Live** (vision 10 §5.2): their nodes have
/// entities, so a Bevy-side system can act on them. Chosen by policy each
/// frame (`decide_modes`): the selection and the hover, with their ancestors
/// inside the composition and their descendants. Retained subtrees, which is
/// everything else, have no entities.
#[derive(Resource, Default, Debug, PartialEq, Eq)]
pub struct Modes {
    pub live: BTreeSet<NodeId>,
}

impl Modes {
    /// Whether a node is due an entity: 3D nodes always, 2D nodes when Live.
    pub fn wants_entity(&self, store: &Store, id: NodeId) -> bool {
        in_3d(store, id) || self.live.contains(&id)
    }
}

/// The most descendants a picked node brings into the Live set. Past this
/// a hovered 100k-shape group would spawn an entity per shape (3.9 s at 100k
/// on the container); its own node, ancestors and direct children suffice
/// until a producer asks for more (vision 10 §8 Q2).
const LIVE_BUDGET: usize = 2_000;

/// The Live set from the selection and hover: each picked node's ancestors
/// up to its composition and its subtree, the latter within [`LIVE_BUDGET`].
pub(super) fn decide_modes(store: Res<super::DocumentStore>, sel: Res<crate::SelectionState>, mut modes: ResMut<Modes>) {
    let doc = store.0.document();
    let mut live = BTreeSet::new();
    for &id in sel.selected.iter().chain(sel.hovered.iter()) {
        if doc.live(id).is_none() {
            continue;
        }
        for n in super::nest::lineage(doc, id) {
            live.insert(n);
        }
        // Counted only up to the budget: this runs every frame, and a
        // hovered group of a large import has tens of thousands below it.
        match doc.descendants_within(id, LIVE_BUDGET) {
            Some(below) => live.extend(below),
            None => live.extend(doc.children(id).iter().copied()),
        }
    }
    if modes.live != live {
        modes.live = live;
    }
}

/// Spawn and despawn 2D entities as subtrees enter and leave the Live set.
/// Draw data is already in [`Drawables`]; only the entity comes and goes.
pub(super) fn sync_live(
    store: Res<super::DocumentStore>,
    modes: Res<Modes>,
    mut map: ResMut<NodeMap>,
    mut layers: ResMut<CompositionLayers>,
    mut drawables: ResMut<Drawables>,
    mut commands: Commands,
    assets: Option<Res<MeshAssets>>,
) {
    if !modes.is_changed() {
        return;
    }
    let Some(assets) = assets else { return };
    let doc = store.0.document();
    let gone: Vec<NodeId> = map.ids().filter(|&id| !modes.wants_entity(&store.0, id)).collect();
    for id in gone {
        if let Some(e) = map.remove(id)
            && let Ok(mut ec) = commands.get_entity(e)
        {
            ec.try_despawn();
        }
    }
    for &id in &modes.live {
        if map.entity(id).is_some() || doc.live(id).is_none() || !drawable(doc, id) {
            continue;
        }
        if let Some(e) = spawn(&store.0, id, &mut layers, &mut commands, &assets, &mut drawables) {
            map.insert(id, e);
        }
    }
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
            &Shape2d::Rect { w, h, .. } | &Shape2d::Slider { w, h, .. } | &Shape2d::Text { w, h, .. } | &Shape2d::Table { w, h, .. } => p.x >= 0.0 && p.y >= 0.0 && p.x <= w && p.y <= h,
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
pub fn path_outline(store: &Store, id: NodeId) -> std::sync::Arc<kurbo::BezPath> {
    iron_document::contour::geometry(store, id).outline().clone()
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

/// Half the side of an `empty`'s marker box, in document units.
pub const EMPTY_HALF: f64 = 6.0;

/// Marker for materialised 3D nodes.
#[derive(Component, Debug, Clone, Copy)]
pub struct Doc3d;

/// Shared handles for `mesh` nodes. Meshes are looked up by the node's
/// `mesh.asset` string: `torus` (the default), and the unit `cube`,
/// `cylinder` (radius 0.5, height 1 along Y) and `sphere` (radius 0.5),
/// sized through `transform3d.scale`.
#[derive(Resource)]
pub struct MeshAssets {
    pub torus: Handle<Mesh>,
    pub torus_bounds: Cuboid,
    pub cube: Handle<Mesh>,
    pub cylinder: Handle<Mesh>,
    pub sphere: Handle<Mesh>,
    /// The checker material a mesh without a `fill` shows.
    pub material: Handle<StandardMaterial>,
    /// What the gas producer's particles are drawn with (`gas.rs`).
    pub gas_material: Handle<StandardMaterial>,
}

impl MeshAssets {
    /// The mesh for an asset name; `None` for an unknown one.
    pub fn mesh(&self, asset: &str) -> Option<Handle<Mesh>> {
        Some(match asset {
            "torus" => self.torus.clone(),
            "cube" => self.cube.clone(),
            "cylinder" => self.cylinder.clone(),
            "sphere" => self.sphere.clone(),
            _ => return None,
        })
    }

    /// The asset's extent before the node's scale (`scene3d::update_aabbes`
    /// applies it).
    pub fn bounds(&self, asset: &str) -> Cuboid {
        match asset {
            "cube" | "cylinder" | "sphere" => Cuboid::from_size(Vec3::ONE),
            _ => self.torus_bounds,
        }
    }
}

/// A mesh node's `fill.color`, resolved; `assign_mesh_materials` turns it
/// into a material.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct MeshColor(pub [f32; 4]);

/// One `StandardMaterial` per distinct colour seen so far.
#[derive(Resource, Default)]
pub struct MeshMaterials(HashMap<[u32; 4], Handle<StandardMaterial>>);

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
    let gas_material = materials.add(StandardMaterial { base_color: Color::srgb(0.98, 0.6, 0.15), perceptual_roughness: 0.6, ..default() });
    let torus = meshes.add(Torus::default().mesh().major_resolution(8).minor_resolution(6));
    let cube = meshes.add(Cuboid::from_size(Vec3::ONE).mesh());
    let cylinder = meshes.add(Cylinder::new(0.5, 1.0).mesh().resolution(24));
    let sphere = meshes.add(Sphere::new(0.5).mesh().ico(1).expect("icosphere"));
    commands.insert_resource(MeshAssets {
        torus,
        torus_bounds: Cuboid::from_size(Vec3::new(1.75, 0.52, 1.75)),
        cube,
        cylinder,
        sphere,
        material,
        gas_material,
    });
}

/// Give every mesh whose colour changed a material of that colour; a
/// translucent colour blends. Materials are shared by colour.
pub(super) fn assign_mesh_materials(
    mut cache: ResMut<MeshMaterials>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    assets: Option<Res<MeshAssets>>,
    mut removed: RemovedComponents<MeshColor>,
    mut query: Query<(&mut MeshMaterial3d<StandardMaterial>, Option<&MeshColor>), Changed<MeshColor>>,
    mut uncoloured: Query<&mut MeshMaterial3d<StandardMaterial>, Without<MeshColor>>,
) {
    let Some(assets) = assets else { return };
    for e in removed.read() {
        if let Ok(mut material) = uncoloured.get_mut(e)
            && material.0 != assets.material
        {
            material.0 = assets.material.clone();
        }
    }
    for (mut material, color) in &mut query {
        let handle = match color {
            None => assets.material.clone(),
            Some(MeshColor(c)) => {
                let key = c.map(f32::to_bits);
                cache
                    .0
                    .entry(key)
                    .or_insert_with(|| {
                        materials.add(StandardMaterial {
                            base_color: Color::srgba(c[0], c[1], c[2], c[3]),
                            alpha_mode: if c[3] < 1.0 { AlphaMode::Blend } else { AlphaMode::Opaque },
                            perceptual_roughness: 0.7,
                            ..default()
                        })
                    })
                    .clone()
            }
        };
        if material.0 != handle {
            material.0 = handle;
        }
    }
}

pub(super) fn reconcile(
    store: &Store,
    affected: &BTreeSet<NodeId>,
    map: &mut NodeMap,
    layers: &mut CompositionLayers,
    commands: &mut Commands,
    assets: &MeshAssets,
    drawables: &mut Drawables,
    modes: &Modes,
) {
    let doc = store.document();
    let _ = FacetKind::ALL; // keep the import meaningful for readers
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

    // Pass 1: draw data for every touched node, and an entity where one is due.
    for &id in affected {
        match (doc.live(id), map.entity(id)) {
            (Some(_), None) => {
                if !drawable(doc, id) {
                    continue;
                }
                if modes.wants_entity(store, id) {
                    if let Some(e) = spawn(store, id, layers, commands, assets, drawables) {
                        map.insert(id, e);
                    }
                } else {
                    write_values(store, id, layers, None, assets, drawables);
                }
            }
            (None, Some(_)) => {
                let e = map.remove(id).expect("checked");
                drawables.0.remove(&id);
                if let Ok(mut ec) = commands.get_entity(e) {
                    ec.try_despawn();
                }
            }
            (Some(_), Some(e)) => {
                if let Ok(mut ec) = commands.get_entity(e) {
                    write_values(store, id, layers, Some(&mut ec), assets, drawables);
                }
            }
            (None, None) => {
                drawables.0.remove(&id);
            }
        }
    }

    // Pass 2: 3D placement below a moved 3D node. Entities are flat (no
    // `ChildOf`, so Bevy propagates nothing for document nodes); the store
    // computes every world affine, and a 3D ancestor's move is written to
    // each materialised descendant here. 3D nodes are few; 2D entities carry
    // no transform at all.
    for &id in affected {
        if doc.live(id).is_none() || !in_3d(store, id) {
            continue;
        }
        for d in doc.descendants(id) {
            if affected.contains(&d) {
                continue;
            }
            let Some(e) = map.entity(d) else { continue };
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.insert(Reader { store, id: d }.global_3d());
            }
        }
    }
}

/// Nothing in a `library` is materialised but definitions' templates' 2D
/// content: references draw it in place (instance-cache.md phase D), and
/// it shows nowhere else, since no composition holds it.
fn drawable(doc: &iron_document::Document, id: NodeId) -> bool {
    let library = TypeId::lookup("library").expect("registered");
    let template = TypeId::lookup("template").expect("registered");
    let mut in_template = false;
    let mut cur = Some(id);
    while let Some(n) = cur.and_then(|c| doc.live(c)) {
        if n.ty == template && n.id != id {
            in_template = true;
        }
        if n.ty == library {
            return in_template;
        }
        cur = n.parent;
    }
    true
}

/// Spawn the entity for a live node, or `None` for node types that have no
/// runtime form (clip, timeline, entity, the library types).
fn spawn(store: &Store, id: NodeId, layers: &mut CompositionLayers, commands: &mut Commands, assets: &MeshAssets, drawables: &mut Drawables) -> Option<Entity> {
    let node = store.document().live(id)?;
    let mut ec = match node.ty.name() {
        // An embed is placed like a group; what it shows is drawn through it
        // (`nest`), never parented to it.
        // An embed inside a 3D composition is a plane in it (ECM §7.4): 3D
        // content the composition's cameras see; `planes` gives it its mesh
        // and texture.
        "embed" if in_3d(store, id) => commands.spawn((Provenance(id), Doc3d, ActiveState::default(), Name::new(format!("embed plane {id}")), Visibility::default())),
        "group" | "use" | "embed" => commands.spawn((Provenance(id), Name::new(format!("{} {id}", node.ty)), Visibility::default())),
        "rect" | "ellipse" | "slider" | "path" | "text" | "empty" | "table" => {
            commands.spawn((Provenance(id), Name::new(format!("{} {id}", node.ty)), Visibility::default()))
        }
        // A camera node is a pose: embeds that name it look through it.
        "camera" => commands.spawn((Provenance(id), Name::new(format!("camera {id}")), Visibility::default())),
        "mesh" => {
            let asset = Reader { store, id }.str("mesh.asset").unwrap_or_else(|| "torus".to_owned());
            commands.spawn((
                Provenance(id),
                Doc3d,
                Name::new(format!("mesh {id}")),
                Mesh3d(assets.mesh(&asset).unwrap_or_else(|| assets.torus.clone())),
                MeshMaterial3d(assets.material.clone()),
                Shape::Box(assets.bounds(&asset)),
                ActiveState::default(),
            ))
        }
        _ => return None,
    };
    write_values(store, id, layers, Some(&mut ec), assets, drawables);
    Some(ec.id())
}

/// Rewrite every derived component from the node's current *resolved* values:
/// a bound slot contributes what its binding evaluates to, a previewed slot
/// what the gesture says. Authored constants are read the same way.
fn write_values(store: &Store, id: NodeId, layers: &mut CompositionLayers, mut ec: Option<&mut EntityCommands>, assets: &MeshAssets, drawables: &mut Drawables) {
    let Some(node) = store.document().live(id) else { return };
    let r = Reader { store, id };
    // Which panel draws it: 2D panels filter on this; 3D content goes on its
    // composition's render layer, which only that composition's cameras see.
    let composition = iron_document::composition::composition_of(store.document(), id);
    if let Some(ec) = ec.as_deref_mut() {
        ec.insert(InComposition(composition));
    }
    // 3D nodes always have an entity; 2D ones may not.
    let Some(ec3) = ec.as_deref_mut() else {
        if let "rect" | "ellipse" | "slider" | "path" | "text" | "table" = node.ty.name() {
            drawables.0.insert(id, r.doc2d(node.ty.name()));
        }
        return;
    };
    let ec = ec3;
    // A 3D entity's pose is Iron's world affine, written as both the
    // `Transform` and the `GlobalTransform`: flat entities have nothing to
    // propagate, so Bevy's sync only ever copies the same value, and the
    // `Transform` keeps `scene3d::update_aabbes` and the gizmo current.
    match node.ty.name() {
        "embed" if in_3d(store, id) => {
            ec.insert((r.pose_3d(), RenderLayers::layer(layers.of(composition))));
        }
        "group" | "use" | "embed" => {}
        "rect" | "ellipse" | "slider" | "path" | "text" | "empty" | "table" => {
            let d = r.doc2d(node.ty.name());
            drawables.0.insert(id, d.clone());
            ec.insert(d);
        }
        "camera" => {
            ec.insert(r.pose_3d());
        }
        "mesh" => {
            ec.insert(r.pose_3d());
            let asset = r.str("mesh.asset").unwrap_or_else(|| "torus".to_owned());
            let mesh = assets.mesh(&asset).unwrap_or_else(|| {
                warn!("{id}: unknown mesh asset {asset:?}; showing torus");
                assets.torus.clone()
            });
            ec.insert((Mesh3d(mesh), Shape::Box(assets.bounds(&asset)), RenderLayers::layer(layers.of(composition))));
            match r.color("fill.color") {
                Some(c) => {
                    ec.insert(MeshColor(c));
                }
                None => {
                    ec.remove::<MeshColor>();
                }
            }
        }
        _ => {}
    }
}

impl Reader<'_> {
    /// A drawable 2D node's draw data from its resolved values.
    fn doc2d(&self, ty: &str) -> Doc2d {
        let (store, id, r) = (self.store, self.id, self);
        {
            // An empty is a box with a cross for picking, drawn by nothing
            // here: the select tool's overlay shows the marker near the
            // pointer, in scope (component-model.md §4.2).
            if ty == "empty" {
                let h = EMPTY_HALF;
                let mut marker = kurbo::BezPath::new();
                marker.move_to((-h, -h));
                marker.line_to((h, -h));
                marker.line_to((h, h));
                marker.line_to((-h, h));
                marker.close_path();
                marker.move_to((-h, 0.0));
                marker.line_to((h, 0.0));
                marker.move_to((0.0, -h));
                marker.line_to((0.0, h));
                let shape = Shape2d::Path { outline: std::sync::Arc::new(marker), fill: None, even_odd: false };
                return Doc2d { shape, fill: None, stroke: None, shadow: None };
            }
            let shape = match ty {
                "ellipse" => {
                    // Present, or previewed by a gesture that will add it.
                    let arc = ["arc.start", "arc.sweep", "arc.inner"].iter().any(|l| r.get(l).is_some()).then(|| [r.num("arc.start", 0.0), r.num("arc.sweep", std::f32::consts::TAU), r.num("arc.inner", 0.0)]);
                    Shape2d::Ellipse { rx: r.num("radius.rx", 50.0), ry: r.num("radius.ry", 50.0), arc }
                }
                "path" => {
                    use iron_document::contour;
                    // A packed path's outlines are cached with its value.
                    let geometry = contour::geometry(store, id);
                    let fill = geometry.fill_outline();
                    let fill = (!fill.elements().is_empty() && (r.color("fill.color").is_some() || r.color("gradient.start").is_some())).then(|| fill.clone());
                    let even_odd = contour::fill_rule(store, id) == contour::FillRule::EvenOdd;
                    Shape2d::Path { outline: geometry.outline().clone(), fill, even_odd }
                }
                "text" => {
                    let m = iron_document::text::measure(store, id);
                    let glyphs = if m.offset == kurbo::Vec2::ZERO { m.layout.outline.clone() } else { std::sync::Arc::new(kurbo::Affine::translate(m.offset) * &*m.layout.outline) };
                    Shape2d::Text { w: m.frame.width() as f32, h: m.frame.height() as f32, glyphs, paints: m.layout.paints.clone(), layout: m.layout.clone(), offset: [m.offset.x as f32, m.offset.y as f32] }
                }
                "table" => {
                    let decor = store.table_grid(id).map(|g| iron_document::table::decor(store, id, g)).unwrap_or_default();
                    Shape2d::Table { w: r.num("size.w", 0.0), h: r.num("size.h", 0.0), decor: std::sync::Arc::new(decor) }
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
            Doc2d { shape, fill, stroke, shadow }
        }
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

    /// The node's world pose in its composition's 3D space, from the store
    /// (`Store::world_affine3`): every live ancestor's `transform3d` then its
    /// own. Bevy receives it and never propagates it.
    fn global_3d(&self) -> GlobalTransform {
        let c = self.store.world_affine3(self.id).to_cols().map(|v| v as f32);
        GlobalTransform::from(bevy::math::Affine3A::from_cols_array(&c))
    }

    /// The same pose as a `Transform` and a `GlobalTransform` pair.
    fn pose_3d(&self) -> (Transform, GlobalTransform) {
        let global = self.global_3d();
        (Transform::from_matrix(Mat4::from(global.affine())), global)
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
