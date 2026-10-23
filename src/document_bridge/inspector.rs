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
//!   land.
//!
//! It never changes what the watched panel draws, and it is not picked in.

use super::chunks::{Chunks, chunk_hue, panel_homes};
use super::{PanelViews, nest};
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
}

impl Default for Options {
    fn default() -> Self {
        Options { tint: true, dim: true, outlines: true }
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
                        let bounds = match &chunk.recorded {
                            Some(r) => r.bounds(),
                            None => chunks.per_panel.get(&(o.panel.clone(), first)).and_then(|r| r.bounds()),
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
            let bounds = chunk.recorded.as_ref().and_then(|r| r.bounds());
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
    serde_json::json!({
        "ok": true,
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
