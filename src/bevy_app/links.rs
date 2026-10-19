//! Connection lines (component-authoring.md §3): a selected component's
//! settings and unplaced outputs as **pins** on its box's edges (inputs on
//! the left, outputs on the right), the lines of its connections and links,
//! and where a line dragged from a pin may land.
//!
//! A pin of an instance connects to another instance's pin (`author::connect`).
//! A pin of a main links to one of the main's parts: the shell asks which
//! property and how the port's range maps onto it, and writes one binding in
//! the definition.

use iron_document::{Document, NodeId, expand};
use kurbo::{Point, Rect};

use crate::bevy_app::select::{Geo, Reach};
use crate::document_bridge::PanelView;

/// CSS px between pins, and from the box's top to the first.
pub const PIN_STEP: f64 = 20.0;
pub const PIN_TOP: f64 = 14.0;
/// CSS px from a pin within which a press or a drop finds it.
pub const PIN_HIT: f64 = 9.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    /// The component's picture: a main or an instance's `use`.
    pub owner: NodeId,
    /// What a line from it names: the definition's port_spec on a main,
    /// the instance's port (`#entity/spec`) otherwise.
    pub port: NodeId,
    pub name: String,
    pub dir: String,
    pub main: bool,
    /// Window px.
    pub at: Point,
}

/// The settings and outputs with no place on the picture of the component
/// `reach` shows, laid out on its box in `view`; `k` is CSS px → window px.
pub fn pins_of(geo: &Geo, view: &PanelView, reach: &Reach, k: f64) -> Vec<Pin> {
    let doc = geo.doc();
    let owner = reach.node();
    let (def, entity, main) = match expand::main_definition(doc, owner) {
        Some(d) => (d, None, true),
        None => {
            let Some(e) = expand::use_entity(doc, owner) else { return Vec::new() };
            let Some(d) = expand::linked_definition(doc, e) else { return Vec::new() };
            (d, Some(e), false)
        }
    };
    let Some(frame) = geo.frame(&reach.chain) else { return Vec::new() };
    let r: Rect = (view.affine() * frame).transform_rect_bbox(Rect::new(0.0, 0.0, 1.0, 1.0));
    let ports = iron_document::author::ports_of(doc, def);
    let mut out = Vec::new();
    let (mut ins, mut outs) = (0, 0);
    for (spec, name, dir, placed) in ports {
        if placed {
            // On a main, a placed port links from where it sits (the dial
            // meter's `in` drives its needle); on an instance it plugs.
            if main
                && let Some(a) = doc.descendants(owner).into_iter().find(|&n| doc.get::<iron_document::facet::Anchor>(n).and_then(|x| x.port.constant()).and_then(|v| v.as_ref()) == Some(spec))
            {
                out.push(Pin { owner, port: spec, name, dir, main, at: view.affine() * geo.world(a) * Point::ORIGIN });
            }
            continue;
        }
        let port = match entity {
            Some(e) => match NodeId::expanded(e, spec) {
                Some(p) => p,
                None => continue,
            },
            None => spec,
        };
        let (x, i) = if dir == "out" {
            outs += 1;
            (r.x1, outs - 1)
        } else {
            ins += 1;
            (r.x0, ins - 1)
        };
        out.push(Pin { owner, port, name, dir, main, at: Point::new(x, r.y0 + (PIN_TOP + PIN_STEP * i as f64) * k) });
    }
    out
}

/// The pin under `screen`, if any.
pub fn pin_at(pins: &[Pin], screen: Point, k: f64) -> Option<&Pin> {
    pins.iter().filter(|p| p.at.distance(screen) <= PIN_HIT * k).min_by(|a, b| a.at.distance(screen).total_cmp(&b.at.distance(screen)))
}

/// The component pictures in `view`'s composition, as reaches (top-level
/// ones: what a line can land on).
pub fn components_in(doc: &Document, view: &PanelView) -> Vec<Reach> {
    let Some(comp) = view.composition else { return Vec::new() };
    doc.children(comp)
        .iter()
        .copied()
        .filter(|&n| doc.live(n).is_some_and(|x| x.ty.name() == "use"))
        .filter(|&n| expand::main_definition(doc, n).is_some() || expand::use_entity(doc, n).is_some_and(|e| expand::linked_definition(doc, e).is_some()))
        .map(|n| Reach { panel: view.panel.clone(), chain: vec![n] })
        .collect()
}

/// Lines to draw for the selected component `reach`: each of its pins'
/// connections to another component's pin, and each link from a main's pin
/// to a part that reads it. `(from, to, link)`; links are drawn dashed.
pub fn lines_of(geo: &Geo, view: &PanelView, reach: &Reach, k: f64) -> Vec<(Point, Point, bool)> {
    let doc = geo.doc();
    let mine = pins_of(geo, view, reach, k);
    let mut out = Vec::new();
    let others: Vec<Pin> = components_in(doc, view).iter().filter(|r| r.node() != reach.node()).flat_map(|r| pins_of(geo, view, r, k)).collect();
    for pin in &mine {
        if pin.main {
            // Parts of the main whose bindings read the pin's spec.
            for n in doc.descendants(pin.owner) {
                let reads = doc.facets_of(n).any(|f| f.field_names().into_iter().any(|field| f.get(field).and_then(|s| s.expr()).is_some_and(|e| e.deps().iter().any(|d| d.node == pin.port))));
                if reads {
                    let c = view.affine() * geo.world(n) * Point::ORIGIN;
                    out.push((pin.at, c, true));
                }
            }
        } else if let Some((_, partner)) = iron_document::plug::connection_of(doc, pin.port)
            && let Some(other) = others.iter().find(|o| o.port == partner)
        {
            out.push((pin.at, other.at, false));
        }
    }
    out
}

/// A line's curve, leaving and arriving level, as Figma's prototype noodles.
pub fn noodle(a: Point, b: Point) -> kurbo::BezPath {
    let dx = ((b.x - a.x).abs() * 0.5).max(30.0);
    let mut p = kurbo::BezPath::new();
    p.move_to(a);
    let s = if b.x >= a.x { 1.0 } else { -1.0 };
    p.curve_to(a + kurbo::Vec2::new(dx * s, 0.0), b - kurbo::Vec2::new(dx * s, 0.0), b);
    p
}
