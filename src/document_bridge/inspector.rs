//! The render inspector (plans/specs/render-inspector.md): a 2D view that
//! watches another panel. Its content is that panel's composition, drawn
//! chunk by chunk in each chunk's colour (`render.rs`, `Chunks::tinted`);
//! this module draws what it sees over that, in an overlay view of its own,
//! so the watched panel's pans and edits redraw only these lines:
//!
//! - the watched panel's viewport, a box, with everything outside it
//!   dimmed: what that panel culls;
//! - every chunk's bounds in its colour;
//! - chunks re-recorded in the last second, filled and fading: where edits
//!   land;
//! - the raster squares the watched view drew (render-cache.md §5), outlined
//!   by how: green from their tile, amber stretched from another scale, red
//!   live because nothing covered them, magenta split around a hot chunk.
//!
//! Its legend also lists, for the selected component instance, how each part
//! of its picture can be drawn (instance-cache.md phase A,
//! `iron_document::analysis`): static, keyed or live, and why.
//!
//! It never changes what the watched panel draws, and it is not picked in.

use super::chunks::{Chunks, chunk_hue, panel_homes};
use super::render::DocumentPanelLayer;
use super::{DocumentStore, PanelViews, nest};
use crate::vector::raster::{self, SquareKind};
use crate::vector::{DisplayList, DisplayListRebuild, Overlay, VectorLayer, VectorView, order};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

/// How long a re-recorded chunk stays lit, in seconds.
const FLASH: f64 = 1.0;

/// What an inspector draws, by its panel id; set from its legend.
#[derive(Resource, Default)]
pub struct InspectorOptions(pub HashMap<String, Options>);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    pub tint: bool,
    pub dim: bool,
    pub outlines: bool,
    pub tiles: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { tint: true, dim: true, outlines: true, tiles: true }
    }
}

/// An inspector panel's overlay view.
#[derive(Component)]
pub struct InspectorOverlay {
    panel: String,
}

pub(super) fn draw_render_inspectors(
    mut commands: Commands,
    views: Res<PanelViews>,
    chunks: Res<Chunks>,
    options: Res<InspectorOptions>,
    time: Res<Time>,
    mut overlays: Query<(Entity, &InspectorOverlay, &mut DisplayList, &mut VectorView)>,
    panel_layers: Query<(Entity, &DocumentPanelLayer)>,
) {
    let now = time.elapsed_secs_f64();
    let mut seen = Vec::new();
    for (e, o, mut list, mut vv) in &mut overlays {
        let Some(view) = views.get(&o.panel).filter(|v| v.observes.is_some()) else {
            commands.entity(e).despawn();
            continue;
        };
        seen.push(o.panel.clone());
        let r = view.rect;
        let next = VectorView { pos: UVec2::new(r.x.max(0.0) as u32, r.y.max(0.0) as u32), size: UVec2::new(r.w.max(1.0) as u32, r.h.max(1.0) as u32) };
        if *vv != next {
            *vv = next;
        }
        let opts = options.0.get(&o.panel).copied().unwrap_or_default();
        let watched = view.observes.as_deref().and_then(|w| views.get(w));
        let to_screen = view.affine();
        let panel_rect = view.rect.to_kurbo();
        // The watched panel's raster squares, as its last build drew them.
        let squares = view
            .observes
            .as_deref()
            .filter(|_| opts.tiles)
            .and_then(|w| panel_layers.iter().find(|(_, l)| l.panel == w))
            .and_then(|(e, _)| raster::picture(e))
            .map(|(_, sq)| sq)
            .unwrap_or_default();
        list.rebuild(|b| {
            // What the watched panel sees, in this one's window px.
            let seen_rect = watched.map(|w| to_screen.transform_rect_bbox(w.affine().inverse().transform_rect_bbox(w.rect.to_kurbo())));
            if opts.dim
                && let Some(s) = seen_rect
            {
                let mut mask = kurbo::BezPath::new();
                use kurbo::Shape as _;
                mask.extend(panel_rect.path_elements(0.1));
                mask.extend(s.path_elements(0.1));
                b.fill_with_rule(kurbo::Affine::IDENTITY, peniko::Color::new([1.0, 1.0, 1.0, 0.7]), peniko::Fill::EvenOdd, mask);
            }
            if opts.outlines {
                for home in panel_homes(view) {
                    let Some(part) = chunks.homes.get(&home) else { continue };
                    for chunk in &part.chunks {
                        let first = chunk.roots[0];
                        let bounds = if chunk.embeds {
                            chunks.per_panel.get(&(o.panel.clone(), first)).and_then(|r| r.bounds()).map(|b| chunk.frame.transform_rect_bbox(b))
                        } else {
                            chunk.bounds()
                        };
                        let Some(bounds) = bounds else { continue };
                        let on_screen = to_screen.transform_rect_bbox(bounds);
                        let [cr, cg, cb] = chunk_hue(first);
                        let age = now - chunk.recorded_at;
                        if (0.0..FLASH).contains(&age) {
                            let a = 0.35 * (1.0 - age / FLASH) as f32;
                            b.fill(kurbo::Affine::IDENTITY, peniko::Color::new([cr, cg, cb, a]), on_screen);
                        }
                        b.stroke(kurbo::Affine::IDENTITY, kurbo::Stroke::new(1.0), peniko::Color::new([cr * 0.8, cg * 0.8, cb * 0.8, 1.0]), on_screen);
                    }
                }
            }
            // Over the chunk bounds, under the viewport box. Squares come in
            // the watched view's scene px: back to its composition's space.
            let from_scene = watched.map_or(kurbo::Affine::IDENTITY, |w| w.affine().inverse() * kurbo::Affine::translate((w.rect.x.max(0.0).floor() as f64, w.rect.y.max(0.0).floor() as f64)));
            for sq in &squares {
                let [r, g, bl] = match sq.kind {
                    SquareKind::Tile => [0.1, 0.65, 0.2],
                    SquareKind::Stretched => [1.0, 0.6, 0.0],
                    SquareKind::Live => [0.9, 0.1, 0.1],
                    SquareKind::Split => [0.85, 0.1, 0.85],
                };
                let on_screen = to_screen.transform_rect_bbox(from_scene.transform_rect_bbox(sq.rect)).inset(-0.5);
                b.stroke(kurbo::Affine::IDENTITY, kurbo::Stroke::new(1.5), peniko::Color::new([r, g, bl, 0.9]), on_screen);
            }
            if let Some(s) = seen_rect {
                b.stroke(kurbo::Affine::IDENTITY, kurbo::Stroke::new(2.0), peniko::Color::new([0.1, 0.45, 1.0, 1.0]), s);
            }
        });
    }
    for view in views.0.iter().filter(|v| v.observes.is_some() && !seen.contains(&v.panel)) {
        let r = view.rect;
        let vv = VectorView { pos: UVec2::new(r.x.max(0.0) as u32, r.y.max(0.0) as u32), size: UVec2::new(r.w.max(1.0) as u32, r.h.max(1.0) as u32) };
        let e = commands.spawn((DisplayList::default(), InspectorOverlay { panel: view.panel.clone() }, vv, Overlay)).id();
        commands.entity(e).insert(VectorLayer::screen(order::EDITOR_OVERLAY).in_view(e));
    }
}

/// What an inspector's legend shows (`document_render_inspector`): the
/// watched panel's chunks and how its view culls them.
pub fn info(world: &World, panel: &str) -> serde_json::Value {
    let (Some(views), Some(chunks)) = (world.get_resource::<PanelViews>(), world.get_resource::<Chunks>()) else {
        return serde_json::json!({"ok": false, "error": "no views"});
    };
    let Some(view) = views.get(panel) else { return serde_json::json!({"ok": false, "error": format!("no panel {panel:?}")}) };
    let watched = view.observes.as_deref().and_then(|w| views.get(w));
    let now = world.get_resource::<Time>().map_or(0.0, |t| t.elapsed_secs_f64());
    let seen = watched.map(|w| w.affine().inverse().transform_rect_bbox(w.rect.to_kurbo()));
    let (mut count, mut nodes, mut visible, mut recent, mut largest) = (0, 0, 0, 0, 0);
    for home in panel_homes(view) {
        let Some(part) = chunks.homes.get(&home) else { continue };
        for chunk in &part.chunks {
            count += 1;
            nodes += chunk.nodes;
            largest = largest.max(chunk.cost);
            let bounds = chunk.bounds();
            if let (Some(b), Some(s)) = (bounds, seen)
                && !b.intersect(s).is_zero_area()
            {
                visible += 1;
            }
            if now - chunk.recorded_at < FLASH {
                recent += 1;
            }
        }
    }
    let perf = crate::perf::report(false);
    // The watched panel's raster squares by kind, and the cache's last second.
    let mut squares = [0_usize; 4];
    let mut rastering = false;
    let mut scale = None;
    if let Some(w) = view.observes.as_deref()
        && let Some(mut q) = world.try_query::<(Entity, &DocumentPanelLayer)>()
        && let Some((e, _)) = q.iter(world).find(|(_, l)| l.panel == w)
        && let Some((on, sq)) = raster::picture(e)
    {
        rastering = on;
        for s in &sq {
            squares[s.kind as usize] += 1;
            scale.get_or_insert(s.scale);
        }
    }
    let r = raster::summary();
    serde_json::json!({
        "raster": {
            "on": rastering,
            "scale": scale,
            "tile": squares[SquareKind::Tile as usize],
            "stretched": squares[SquareKind::Stretched as usize],
            "live": squares[SquareKind::Live as usize],
            "split": squares[SquareKind::Split as usize],
            "tiles": r.tiles,
            "mb": r.pages * 16,
            "budget_mb": r.budget_mb,
            "baked": r.baked,
            "bake_ms": (r.bake_ms as f64 * 10.0).round() / 10.0,
            "live_squares": r.live_squares,
        },
        "ok": true,
        "instance": world.get_resource::<DocumentStore>().and_then(|s| instance_info(&s.0)),
        "watching": view.observes,
        "zoom": watched.map(|w| w.zoom),
        "chunks": count,
        "visible": visible,
        "nodes": nodes,
        "largest": largest,
        "recent": recent,
        "commands": perf.get("commands"),
        "culled": perf.get("culled"),
        "homes": panel_homes(view).iter().map(|h| match h { nest::Home::Composition(c) => c.to_string(), nest::Home::Loose => "loose".into(), nest::Home::Unknown => "?".into() }).collect::<Vec<_>>(),
    })
}

/// The render analysis of the selected instance: the nearest `use` at or
/// above the first selected node that is a linked instance or a main.
fn instance_info(store: &iron_document::Store) -> Option<serde_json::Value> {
    use iron_document::analysis::{self, Input};
    use iron_document::facet::Name;
    let doc = store.document();
    let name = |n: iron_document::NodeId| doc.get::<Name>(n).and_then(|x| x.text.constant()).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map_or_else(|| n.to_string(), str::to_owned);
    let (first, _) = store.selection().into_iter().next()?;
    let mut at = Some(first);
    let a = loop {
        let n = at?;
        if let Some(a) = analysis::classify(doc, store.reactive(), n) {
            break a;
        }
        at = doc.node(n).and_then(|x| x.parent);
    };
    let parts: Vec<serde_json::Value> = a
        .parts
        .iter()
        .filter(|p| p.draws)
        .map(|p| {
            let mut channels = serde_json::Map::new();
            for (c, k) in &p.classes {
                channels.insert(c.name().into(), k.name().into());
            }
            serde_json::json!({
                "name": name(p.node),
                "class": p.class().name(),
                "movable": p.movable(),
                "channels": channels,
                "reasons": p.reasons,
            })
        })
        .collect();
    let inputs: serde_json::Map<String, serde_json::Value> = a
        .inputs
        .iter()
        .map(|(k, v)| (k.clone(), match v {
            Input::Constant(v) => v.to_string().into(),
            Input::Wired(why) => why.clone().into(),
        }))
        .collect();
    let runs: Vec<serde_json::Value> = analysis::runs(&a).iter().map(|r| serde_json::json!({"class": r.class.name(), "parts": r.parts.len()})).collect();
    Some(serde_json::json!({
        "name": name(a.instance),
        "definition": name(a.definition),
        "inputs": inputs,
        "parts": parts,
        "runs": runs,
    }))
}
