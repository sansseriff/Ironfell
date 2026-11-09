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
/// On an instance, only the inputs that show as pins (§ `shows`).
pub fn pins_of(geo: &Geo, view: &PanelView, reach: &Reach, k: f64) -> Vec<Pin> {
    pin_layout(geo, view, reach, k, false).0
}

/// Whether an instance's input shows as a pin (types-and-units.md §6): its
/// author's `port.show` (`pin` always, `settings` only when connected), and
/// `auto`: when it is connected, bound, or set away from its default.
fn shows(doc: &Document, spec: NodeId, entity: NodeId, port: NodeId, name: &str) -> bool {
    let Some(p) = doc.get::<iron_document::facet::Port>(spec) else { return true };
    let connected = iron_document::plug::connection_of(doc, port).is_some();
    match p.show.constant().and_then(|v| v.as_str()) {
        Some("pin") => true,
        Some("settings") => connected,
        _ => {
            let arg = iron_document::LeafPath::new(iron_document::FacetKind::Args, name).ok().and_then(|path| doc.leaf(entity, path));
            let changed = match (arg, p.signal.constant()) {
                (Some(slot), _) if slot.expr().is_some() => true,
                (Some(slot), Some(default)) => slot.constant().is_some_and(|v| match (v, default) {
                    (iron_document::Value::Number(a), iron_document::Value::Number(b)) => !(a == b || (a.is_nan() && b.is_nan())),
                    (a, b) => a.as_str().zip(b.as_str()).map_or(a != b, |(x, y)| x != y),
                }),
                _ => false,
            };
            connected || changed
        }
    }
}

/// Pins laid out (`all`: every input, as a line being drawn may land on
/// one), and how many inputs are folded away.
pub fn pin_layout(geo: &Geo, view: &PanelView, reach: &Reach, k: f64, all: bool) -> (Vec<Pin>, usize) {
    let doc = geo.doc();
    let owner = reach.node();
    let (def, entity, main) = match expand::main_definition(doc, owner) {
        Some(d) => (d, None, true),
        None => {
            let Some(e) = expand::use_entity(doc, owner) else { return (Vec::new(), 0) };
            let Some(d) = expand::linked_definition(doc, e) else { return (Vec::new(), 0) };
            (d, Some(e), false)
        }
    };
    let Some(frame) = geo.frame(&reach.chain) else { return (Vec::new(), 0) };
    let r: Rect = (view.affine() * frame).transform_rect_bbox(Rect::new(0.0, 0.0, 1.0, 1.0));
    let ports = iron_document::author::ports_of(doc, def);
    let mut out = Vec::new();
    let (mut ins, mut outs, mut folded) = (0, 0, 0);
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
        if !all
            && dir == "in"
            && let Some(e) = entity
            && !shows(doc, spec, e, port, &name)
        {
            folded += 1;
            continue;
        }
        let (x, i) = if dir == "out" {
            outs += 1;
            (r.x1, outs - 1)
        } else {
            ins += 1;
            (r.x0, ins - 1)
        };
        out.push(Pin { owner, port, name, dir, main, at: Point::new(x, r.y0 + (PIN_TOP + PIN_STEP * i as f64) * k) });
    }
    (out, folded)
}

/// Where the "+N more" pill sits: below the last input pin on the left.
pub fn more_at(geo: &Geo, view: &PanelView, reach: &Reach, k: f64, pins: &[Pin]) -> Option<Point> {
    let frame = geo.frame(&reach.chain)?;
    let r: Rect = (view.affine() * frame).transform_rect_bbox(Rect::new(0.0, 0.0, 1.0, 1.0));
    let ins = pins.iter().filter(|p| p.dir == "in" && !p.main).count();
    Some(Point::new(r.x0, r.y0 + (PIN_TOP + PIN_STEP * ins as f64) * k))
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

/// Whether a line from pin `a` may land on pin `b` by their declared types
/// (out feeds in; a loose end always may).
pub fn fits(doc: &Document, a: &Pin, b: &Pin) -> bool {
    let ty = |p: NodeId| {
        doc.get::<iron_document::facet::Port>(p)
            .and_then(|x| x.datatype.constant())
            .and_then(|v| v.as_str())
            .and_then(|t| iron_document::types::Type::parse(t).ok())
            .unwrap_or(iron_document::types::Type::ANY)
    };
    let (from, to) = if a.dir == "out" { (a, b) } else { (b, a) };
    ty(from.port).feeds(&ty(to.port)).is_ok()
}
