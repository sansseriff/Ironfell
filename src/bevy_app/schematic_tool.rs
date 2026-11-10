//! The schematic's tools (plans/specs/schematic.md §4): placing a part
//! (`place:<key>`) and wiring pins (`wire`). Both round to the view's grid
//! (`snap.grid`). What they make is the store's (`parts::place_part`,
//! `parts::wire_ops`); here is only the pointer.
//!
//! **Placement**: a ghost of the part's symbol follows the pointer; Space
//! turns it a quarter; a click places it, and the tool stays armed for the
//! next. Esc puts it down.
//!
//! **Wire**: a click on a pin starts a wire; each click on the canvas adds
//! a corner, the leg drawn orthogonally (horizontal first; Space flips it);
//! a click on another pin finishes it, one transaction. A wire may start or
//! end on another wire, which is split there at a junction
//! (`parts::wire_between`). Esc drops the wire in progress, then puts the
//! tool down.

use bevy::prelude::*;
use iron_document::composition::Dims;
use iron_document::parts::{self, Terminal};
use iron_document::{Actor, TransactionInput};
use kurbo::{Affine, Point, Shape as _};

use crate::bevy_app::select::{ActiveTool, ToolKeys};
use crate::bevy_app::view_focus::ViewFocus;
use crate::document_bridge::{DocumentStore, PanelViews};

/// CSS px from a pin within which the wire tool takes it.
const PIN_HIT: f64 = 9.0;

#[derive(Resource, Default)]
pub struct SchematicTool {
    /// Quarter turns the next placed part takes.
    pub turns: u8,
    /// The pointer over a 2D view: its panel and sheet point, on the grid.
    pub pointer: Option<(String, Point)>,
    /// What a click would wire to, while wiring: a pin, or a point on a
    /// wire; and where it is.
    pub hot: Option<(String, Terminal, Point)>,
    pub wire: Option<Wire>,
}

/// A wire in progress.
pub struct Wire {
    pub panel: String,
    pub from: (Terminal, Point),
    /// Corners placed so far, sheet px.
    pub corners: Vec<Point>,
    pub vertical_first: bool,
}

impl Wire {
    /// Its points to `to`: the start, the corners, the elbow, `to`.
    fn points(&self, to: Point) -> Vec<Point> {
        let mut pts = vec![self.from.1];
        pts.extend(self.corners.iter().copied());
        let last = *pts.last().expect("starts with its pin");
        if let Some(c) = parts::elbow([last.x, last.y], [to.x, to.y], self.vertical_first) {
            pts.push(Point::new(c[0], c[1]));
        }
        pts.push(to);
        pts
    }
}

/// Drop repeated points and corners that do not turn.
fn tidy(mut pts: Vec<Point>) -> Vec<Point> {
    pts.dedup_by(|a, b| a.distance(*b) < 1e-6);
    let mut i = 1;
    while i + 1 < pts.len() {
        let (a, b, c) = (pts[i - 1], pts[i], pts[i + 1]);
        let straight = ((a.x - b.x).abs() < 1e-6 && (b.x - c.x).abs() < 1e-6) || ((a.y - b.y).abs() < 1e-6 && (b.y - c.y).abs() < 1e-6);
        if straight {
            pts.remove(i);
        } else {
            i += 1;
        }
    }
    pts
}

fn snap(p: Point, grid: f64) -> Point {
    if grid <= 0.0 {
        return p;
    }
    Point::new((p.x / grid).round() * grid, (p.y / grid).round() * grid)
}

#[allow(clippy::too_many_arguments)]
pub fn schematic_tool_system(
    pointer: Res<crate::PointerState>,
    hits: Res<crate::PointerHits>,
    views: Res<PanelViews>,
    focus: Res<ViewFocus>,
    mut active: ResMut<ActiveTool>,
    mut keys: ResMut<ToolKeys>,
    mut store: ResMut<DocumentStore>,
    mut tool: ResMut<SchematicTool>,
    dpr: Res<crate::Dpr>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    // Tool picks and Esc; Space turns the part or flips the wire's elbow.
    let mut rest = Vec::new();
    for (key, m) in std::mem::take(&mut keys.0) {
        let placing = matches!(*active, ActiveTool::Place(_));
        match key.as_str() {
            "tool:wire" => {
                *active = ActiveTool::Wire;
                tool.wire = None;
            }
            k if k.starts_with("tool:place:") => match parts::find(&k["tool:place:".len()..]) {
                Some(part) => {
                    // Another part starts unturned.
                    if *active != ActiveTool::Place(part.key.as_str()) {
                        tool.turns = 0;
                    }
                    *active = ActiveTool::Place(part.key.as_str());
                }
                None => warn!("no part {k}"),
            },
            "escape" if *active == ActiveTool::Wire && tool.wire.is_some() => tool.wire = None,
            "escape" if placing || *active == ActiveTool::Wire => *active = ActiveTool::Select,
            _ => rest.push((key, m)),
        }
    }
    keys.0 = rest;
    if keyboard.just_pressed(KeyCode::Space) {
        match *active {
            ActiveTool::Place(_) => tool.turns = (tool.turns + 1) % 4,
            ActiveTool::Wire => {
                if let Some(w) = tool.wire.as_mut() {
                    w.vertical_first = !w.vertical_first;
                }
            }
            _ => {}
        }
    }
    if !matches!(*active, ActiveTool::Wire | ActiveTool::Place(_)) {
        tool.wire = None;
        tool.hot = None;
        tool.pointer = None;
        return;
    }

    let Some(view) = hits.panel.as_deref().and_then(|p| views.get(p)).filter(|v| v.dims == Dims::Two).cloned() else {
        tool.pointer = None;
        tool.hot = None;
        return;
    };
    let Some(sheet) = view.composition else { return };
    let to_screen = view.affine();
    let screen = Point::new(pointer.screen.x as f64, pointer.screen.y as f64);
    let grid = focus.grid(&view.panel);
    let at = snap(to_screen.inverse() * screen, grid);
    if tool.pointer.as_ref() != Some(&(view.panel.clone(), at)) {
        tool.pointer = Some((view.panel.clone(), at));
    }

    if let ActiveTool::Place(key) = *active {
        if pointer.just_pressed_left {
            let rot = f64::from(tool.turns) * std::f64::consts::FRAC_PI_2;
            if let Err(e) = store.0.place_part(key, sheet, [at.x, at.y], rot, Actor::Human) {
                warn!("place {key}: {e}");
            }
        }
        return;
    }

    // Wiring: the nearest pin within reach, else a wire there (on the grid
    // where the grid lies on it).
    let reach = PIN_HIT * dpr.0 * focus.tolerances(Some(&view.panel)).hit;
    let pin = parts::pins_on(&store.0, sheet)
        .into_iter()
        .map(|(end, p)| (end, Point::new(p[0], p[1])))
        .map(|(end, p)| ((to_screen * p).distance(screen), end, p))
        .filter(|(d, ..)| *d <= reach)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, end, p)| (view.panel.clone(), Terminal::Pin(end), p));
    let zoom = (to_screen * Point::new(1.0, 0.0)).distance(to_screen * Point::ORIGIN);
    let raw = to_screen.inverse() * screen;
    let hot = pin.or_else(|| {
        let (path, on) = parts::wire_at(&store.0, sheet, [raw.x, raw.y], reach / zoom)?;
        let on = Point::new(on[0], on[1]);
        let gridded = snap(on, grid);
        let on = match parts::wire_at(&store.0, sheet, [gridded.x, gridded.y], 1e-6) {
            Some((p, _)) if p == path => gridded,
            _ => on,
        };
        Some((view.panel.clone(), Terminal::Wire { path, at: [on.x, on.y] }, on))
    });
    if tool.hot.as_ref().map(|h| (h.1, h.2)) != hot.as_ref().map(|h| (h.1, h.2)) {
        tool.hot = hot.clone();
    }
    if !pointer.just_pressed_left {
        return;
    }
    let wire = tool.wire.take();
    match (wire, hot) {
        (None, Some((panel, t, p))) => tool.wire = Some(Wire { panel, from: (t, p), corners: Vec::new(), vertical_first: false }),
        (None, None) => {}
        (Some(w), Some((_, t, p))) if w.panel == view.panel && t != w.from.0 => {
            let pts: Vec<[f64; 2]> = tidy(w.points(p)).into_iter().map(|q| [q.x, q.y]).collect();
            match parts::wire_between(&store.0, sheet, w.from.0, t, &pts) {
                Ok(ops) => {
                    if let Err(errors) = store.0.apply(TransactionInput { label: "wire".into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }) {
                        for e in errors {
                            warn!("wire rejected: {e}");
                        }
                    }
                }
                Err(e) => warn!("wire: {e}"),
            }
        }
        (Some(mut w), _) if w.panel == view.panel => {
            // A corner where the pointer is, by way of the elbow.
            let pts = w.points(at);
            w.corners = tidy(pts)[1..].to_vec();
            tool.wire = Some(w);
        }
        (Some(w), _) => tool.wire = Some(w),
    }
}

/// Draw the tool's feedback over `panel`: the ghost of the part being
/// placed, the hot pin, and the wire in progress.
pub fn draw(b: &mut crate::vector::DisplayListBuilder<'_>, tool: &SchematicTool, active: ActiveTool, panel: &str, to_screen: Affine, k: f64, accent: peniko::Color) {
    let Some((p, at)) = tool.pointer.as_ref() else { return };
    if p != panel {
        return;
    }
    let thin = kurbo::Stroke::new(1.5 * k);
    if let ActiveTool::Place(key) = active
        && let Some(part) = parts::find(key)
    {
        let place = to_screen * Affine::translate(at.to_vec2()) * Affine::rotate(f64::from(tool.turns) * std::f64::consts::FRAC_PI_2);
        let ghost = accent.with_alpha(0.7);
        for m in &part.symbol {
            match m {
                parts::Mark::Path(contours) => {
                    let g = iron_document::geometry::Geometry::new(contours.clone());
                    b.stroke(Affine::IDENTITY, thin.clone(), ghost, place * (**g.outline()).clone());
                }
                parts::Mark::Circle { at: c, r } => b.stroke(Affine::IDENTITY, thin.clone(), ghost, place * kurbo::Circle::new((c[0], c[1]), *r).to_path(0.1)),
                parts::Mark::Dot { at: c, r } => b.fill(Affine::IDENTITY, ghost, place * kurbo::Circle::new((c[0], c[1]), *r).to_path(0.1)),
                parts::Mark::Label { .. } => {}
            }
        }
        for pin in &part.pins {
            b.fill(Affine::IDENTITY, ghost, kurbo::Circle::new(place * Point::new(pin.at[0], pin.at[1]), 2.5 * k));
        }
        return;
    }
    if active != ActiveTool::Wire {
        return;
    }
    if let Some(w) = tool.wire.as_ref().filter(|w| w.panel == panel) {
        let to = tool.hot.as_ref().filter(|h| h.0 == panel).map_or(*at, |h| h.2);
        let pts = w.points(to);
        let mut line = kurbo::BezPath::new();
        line.move_to(to_screen * pts[0]);
        for q in &pts[1..] {
            line.line_to(to_screen * *q);
        }
        b.stroke(Affine::IDENTITY, thin.clone(), accent, line);
    }
    if let Some((_, _, p)) = tool.hot.as_ref().filter(|h| h.0 == panel) {
        let ring = kurbo::Circle::new(to_screen * *p, 6.0 * k);
        b.stroke(Affine::IDENTITY, kurbo::Stroke::new(2.0 * k), peniko::Color::new([0.05, 0.72, 0.58, 1.0]), ring);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iron_document::NodeId;

    #[test]
    fn a_wire_turns_once_per_corner_and_tidies_straight_runs() {
        let a = Point::new(0.0, 0.0);
        let w = Wire { panel: "p".into(), from: (Terminal::Pin(parts::End { pin: "#1".parse::<NodeId>().unwrap(), anchor: "#2".parse::<NodeId>().unwrap() }), a), corners: vec![], vertical_first: false };
        assert_eq!(w.points(Point::new(30.0, 20.0)), [a, Point::new(30.0, 0.0), Point::new(30.0, 20.0)]);
        assert_eq!(tidy(vec![a, Point::new(10.0, 0.0), Point::new(20.0, 0.0), Point::new(20.0, 0.0), Point::new(20.0, 5.0)]), [a, Point::new(20.0, 0.0), Point::new(20.0, 5.0)]);
        assert_eq!(snap(Point::new(14.0, 26.0), 10.0), Point::new(10.0, 30.0));
    }
}
