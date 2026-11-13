//! The timeline as a composition (plans/specs/timeline.md §4.3, §5): a
//! session 2D composition generated from the document's clips, drawn by an
//! ordinary composition view in the `timeline` notation (`std:timeline`).
//!
//! **Structure is generated; values are bound.** A [`Plan`] says what the
//! timeline shows: its rows (by object, then property), each clip's segments,
//! the ruler's extent and tick spacing, which clips are in error. The view
//! is regenerated only when the plan changes. Positions are bindings on the
//! clips' leaves, the session clock and the composition's `axis`, so
//! retiming a clip moves its segment without regenerating, and dragging a
//! segment edits the clip by binding inversion (the axis is held, §4.2).
//! Static pieces are bound to expressions that read no free leaf, so a drag
//! cannot move them.
//!
//! **Reconciled by key.** Every generated node is named for what it stands
//! for (`seg <clip> <target>`), and regeneration keeps the nodes whose key
//! survives, so session ids and the selection survive too.

use crate::document::{RelKind, Relation};
use crate::expr::{Expr, Leaf};
use crate::facet::{Axis, Composition, ContextOf, Corners, Facet, FacetKind, Fill, Font, LeafPath, LeafStruct, Name, Size, Stroke, Text, Track, Transform2d};
use crate::id::NodeId;
use crate::op::Op;
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::{Slot, Value};
use crate::Store;
use std::collections::{BTreeMap, BTreeSet};

/// The session composition's name, as a view's source names it.
pub const TIMELINE: &str = "timeline";
/// Its notation, whose built-in context is `std:timeline`.
pub const NOTATION: &str = "timeline";

/// The ruler's height and a row's, in document px, which a view at 100%
/// shows as CSS px: the ruler is as tall as the transport the DOM column
/// puts over its left end (§12.3).
pub const RULER_H: f64 = 56.0;
pub const ROW_H: f64 = 22.0;
const SEG_H: f64 = 16.0;
/// How far the background, header and lines reach, in px.
const FAR: f64 = 8000.0;
/// The ruler's labels, px.
const LABEL_PX: f64 = 13.0;
/// Ticks are about this far apart at the current zoom, in px.
const TICK_PX: f64 = 80.0;

/// What the timeline shows. Equal plans draw the same structure.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub clock: NodeId,
    pub objects: Vec<Object>,
    /// The ruler runs from 0 to here, seconds.
    pub end: f64,
    /// Seconds between ticks.
    pub tick: f64,
    pub errors: BTreeSet<NodeId>,
    /// Drawn in the light theme's colours (the shell's appearance).
    pub light: bool,
    /// Inside one transition of a component (machines.md §6, the state
    /// editor), or `None` for the document's clock.
    pub inside: Option<Inside>,
}

/// A transition seen from inside (machines.md §6): the same timeline at a
/// second level. Its rows are the component's parts and their properties,
/// its bars the properties' slices of the transition, its ruler the
/// transition's own time, and its playhead how far into it the main shows
/// (`axis.cursor`).
#[derive(Clone, Debug, PartialEq)]
pub struct Inside {
    pub def: NodeId,
    pub machine: NodeId,
    pub from: String,
    pub to: String,
    /// The transition the pair uses, and whether it is the pair's own (a
    /// pair on the default timing shows it, but its bars do not move).
    pub transition: Option<NodeId>,
    pub own: bool,
    /// Its length, seconds.
    pub secs: f64,
    /// The parts shown open (a row per property); the rest show one row.
    pub open: BTreeSet<NodeId>,
    /// The slices selected (their bars stand out; the others recede).
    pub picked: BTreeSet<NodeId>,
    /// Per part, the slice that starts first: its bar's start is the part
    /// bar's, so dragging the part bar moves all of the part's slices.
    pub earliest: BTreeMap<NodeId, NodeId>,
    /// Each slice's easing as it plays (its own, or the transition's), for
    /// the curve drawn in its bar.
    pub curves: BTreeMap<NodeId, String>,
}

/// Inside a transition the editor's header and navigator sit over the
/// view's top and left: the pieces start below and right of them, document
/// px (the DOM reads these from `timeline_rows`).
pub const INSIDE_TOP: f64 = 32.0;
pub const INSIDE_INSET: f64 = 220.0;
/// Inside, the ruler is a strip: its transport is in the editor's header.
pub const INSIDE_RULER_H: f64 = 26.0;

impl Plan {
    /// Where the ruler starts, from the composition's top.
    pub fn top(&self) -> f64 {
        if self.inside.is_some() { INSIDE_TOP } else { 0.0 }
    }
    /// Where the label column starts, from the composition's left.
    pub fn inset(&self) -> f64 {
        if self.inside.is_some() { INSIDE_INSET } else { 0.0 }
    }
    pub fn ruler_h(&self) -> f64 {
        if self.inside.is_some() { INSIDE_RULER_H } else { RULER_H }
    }
}

/// A row inside a transition: one channel of the component, and its slice
/// (the tween that gives it its own timing, or none: it follows the
/// transition's curve over all of it).
#[derive(Clone, Debug, PartialEq)]
pub struct SliceRow {
    pub channel: String,
    pub tween: Option<NodeId>,
}

/// One animated node: a row naming it, then a row per property.
#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    pub target: NodeId,
    pub label: String,
    pub props: Vec<Prop>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Prop {
    pub prop: String,
    pub segments: Vec<Segment>,
    /// A component's state port (machines.md §6): drawn as a lane of named
    /// stretches. `rest`: the state it holds where nothing keys it, with its
    /// index among the machine's states.
    pub lane: bool,
    pub rest: Option<(String, usize)>,
    /// What the row says, when not the leaf's own name (inside a
    /// transition: the property, `X`, `Opacity`).
    pub label: Option<String>,
    /// Inside a transition: the channel and its slice.
    pub slice: Option<SliceRow>,
}

/// A clip on one of its targets, shifted by the relation's stagger offset,
/// with its keys in time order.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub clip: NodeId,
    pub offset: f64,
    pub label: String,
    pub keys: Vec<NodeId>,
    /// On a component's keyed state (machines.md §4.2): each key that
    /// changes it, with its transition's own length in seconds and name,
    /// drawn as a progress band after the key.
    pub bands: Vec<(NodeId, f64, String)>,
    /// On a keyed state: each key's state and its index among the
    /// machine's states (its lane colour).
    pub states: Vec<(NodeId, String, usize)>,
    /// The state it holds before its first key (its own argument).
    pub before: Option<(String, usize)>,
}

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).expect("registered type")
}

fn name_of(store: &Store, id: NodeId) -> Option<String> {
    store.document().get::<Name>(id).and_then(|n| n.text.constant()).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(str::to_owned)
}

fn label_of(store: &Store, id: NodeId) -> String {
    name_of(store, id).unwrap_or_else(|| format!("{} {id}", store.document().live(id).map_or("node", |n| n.ty.name())))
}

fn number(store: &Store, id: NodeId, facet: FacetKind, field: &str) -> Option<f64> {
    store.resolved(id, LeafPath::new(facet, field).ok()?)?.as_f64()
}

/// The session's timeline composition, if it has been built.
pub fn find(store: &Store) -> Option<NodeId> {
    let doc = store.document();
    doc.roots().iter().copied().find(|&r| r.is_session() && doc.live(r).is_some_and(|n| n.ty.name() == "composition") && name_of(store, r).as_deref() == Some(TIMELINE))
}

/// Tick spacing: the smallest of 1, 2, 5 × 10ⁿ s at least `TICK_PX` apart.
fn tick_for(pps: f64) -> f64 {
    let pps = if pps.is_finite() && pps > 0.0 { pps } else { 100.0 };
    let mut base = 0.01;
    loop {
        for m in [1.0, 2.0, 5.0] {
            let t: f64 = base * m;
            if t * pps >= TICK_PX || t >= 3600.0 {
                return t;
            }
        }
        base *= 10.0;
    }
}

/// What the timeline would show now. `comp` is the composition, whose axis
/// decides the tick spacing.
pub fn plan(store: &Store, clock: NodeId, comp: Option<NodeId>) -> Plan {
    // A transition previewed in the state editor: the timeline is inside it.
    if let Some(pin) = store.pinned_state()
        && let Some((from, to, _)) = &pin.scrub
    {
        return inside_plan(store, clock, comp, pin, from, to);
    }
    let doc = store.document();
    let mut objects: Vec<Object> = Vec::new();
    let mut end: f64 = 0.0;
    let clips: Vec<NodeId> = doc.nodes_of_type(ty("clip")).filter(|c| !c.is_session() && !c.is_expanded()).collect();
    for &clip in &clips {
        let prop = doc.get::<crate::facet::Clip>(clip).and_then(|c| c.prop.constant()).and_then(|v| v.as_str()).unwrap_or("").to_owned();
        let start = crate::time::start(doc, clip);
        let keys = crate::time::keys(doc, clip);
        let label = name_of(store, clip).unwrap_or_else(|| "clip".into());
        for r in doc.relations_from(clip).filter(|r| r.rel == RelKind::Animates && doc.is_live(r.to)) {
            let offset = r.data.get("offset").and_then(|v| v.as_f64()).unwrap_or(0.0);
            end = end.max(start + offset + crate::time::length(&keys));
            let i = match objects.iter().position(|o| o.target == r.to) {
                Some(i) => i,
                None => {
                    objects.push(Object { target: r.to, label: label_of(store, r.to), props: Vec::new() });
                    objects.len() - 1
                }
            };
            let o = &mut objects[i];
            let bands = state_bands(store, r.to, &prop, &keys);
            for (k, len, _) in &bands {
                let t = keys.iter().find(|x| x.id == *k).map_or(0.0, |x| x.t);
                end = end.max(start + offset + t + len);
            }
            let names = machine_states(store, r.to, &prop);
            let states = match &names {
                Some(n) => keys.iter().filter_map(|k| k.value.as_str().map(|v| (k.id, v.to_owned(), n.iter().position(|x| x == v).unwrap_or(0)))).collect(),
                None => Vec::new(),
            };
            let before = names.as_ref().and_then(|n| {
                let port = prop.strip_prefix("args.")?;
                let v = doc.leaf(r.to, LeafPath::new(FacetKind::Args, port).ok()?)?.constant()?.as_str()?.to_owned();
                let i = n.iter().position(|x| *x == v).unwrap_or(0);
                Some((v, i))
            });
            let segment = Segment { clip, offset, label: label.clone(), keys: keys.iter().map(|k| k.id).collect(), bands, states, before };
            match o.props.iter_mut().find(|p| p.prop == prop) {
                Some(p) => p.segments.push(segment),
                None => o.props.push(Prop { prop: prop.clone(), segments: vec![segment], lane: names.is_some(), rest: None, label: None, slice: None }),
            }
        }
    }
    // Every instance of a component with states gets a lane, keyed or not,
    // so what state each one is in reads off the timeline (machines.md §6).
    for e in doc.nodes_of_type(ty("entity")).filter(|&e| !e.is_session() && !e.is_expanded() && doc.is_live(e)) {
        let Some(def) = crate::expand::linked_definition(doc, e) else { continue };
        for (_, port) in crate::machine::machines(doc, def) {
            let prop = format!("args.{port}");
            if objects.iter().any(|o| o.target == e && o.props.iter().any(|p| p.prop == prop)) {
                continue;
            }
            let Ok(path) = LeafPath::new(FacetKind::Args, &port) else { continue };
            let Some(state) = store.resolved(e, path).and_then(|v| v.as_str().map(str::to_owned)) else { continue };
            let names = machine_states(store, e, &prop).unwrap_or_default();
            let index = names.iter().position(|n| *n == state).unwrap_or(0);
            let i = match objects.iter().position(|o| o.target == e) {
                Some(i) => i,
                None => {
                    objects.push(Object { target: e, label: label_of(store, e), props: Vec::new() });
                    objects.len() - 1
                }
            };
            objects[i].props.push(Prop { prop, segments: Vec::new(), lane: true, rest: Some((state, index)), label: None, slice: None });
        }
    }
    let pps = comp.and_then(|c| number(store, c, FacetKind::Axis, "pps")).unwrap_or(100.0);
    let t0 = comp.and_then(|c| number(store, c, FacetKind::Axis, "t0")).unwrap_or(0.0);
    let errors = store.time_errors().keys().copied().filter(|c| clips.contains(c)).collect();
    // The ruler's extent grows in 5 s steps, past the last clip's end and
    // a wide panel's worth past the left edge, so most retimes and pans
    // leave the structure alone.
    let seen = t0 + 2400.0 / pps.max(1.0);
    let end = ((end.max(seen) / 5.0).ceil() * 5.0 + 5.0).max(10.0);
    Plan { clock, objects, end, tick: tick_for(pps), errors, light: false, inside: None }
}

/// A property's name in a row: `X`, `Opacity`, `Fill`.
pub fn property_label(path: LeafPath) -> String {
    let known = match (path.facet.name(), path.field) {
        ("transform2d", "x") => "X",
        ("transform2d", "y") => "Y",
        ("transform2d", "rot") => "Rotation",
        ("transform2d", "sx") => "Scale X",
        ("transform2d", "sy") => "Scale Y",
        ("size", "w") => "Width",
        ("size", "h") => "Height",
        ("ellipse", "rx") => "Radius X",
        ("ellipse", "ry") => "Radius Y",
        ("appearance", "opacity") => "Opacity",
        ("fill", "color") => "Fill",
        ("stroke", "color") => "Stroke",
        ("stroke", "width") => "Stroke width",
        ("text", "content") => "Text",
        ("font", "size") => "Font size",
        ("trim", "start") => "Trim start",
        ("trim", "end") => "Trim end",
        _ => "",
    };
    if known.is_empty() { format!("{} {}", path.facet.name(), path.field.replace('_', " ")) } else { known.to_owned() }
}

/// What the timeline shows inside the transition a pinned state previews
/// (machines.md §6): a row per part of the component that reads a channel,
/// a row per such property under it, each with its slice.
fn inside_plan(store: &Store, clock: NodeId, comp: Option<NodeId>, pin: &crate::machine::Pinned, from: &str, to: &str) -> Plan {
    use crate::machine;
    let doc = store.document();
    let read = |l: Leaf| store.resolved(l.node, l.path);
    let p = machine::plan(doc, &read, pin.def, pin.machine, from, to);
    let own = machine::own_transition(doc, &read, pin.def, pin.machine, from, to).is_some();
    let secs = p.as_ref().map_or(0.0, |p| p.ms / 1000.0);
    let tweens: BTreeMap<String, NodeId> = p
        .as_ref()
        .map(|p| doc.children(p.transition).iter().copied().filter(|&w| doc.live(w).is_some_and(|n| n.ty.name() == "tween")).filter_map(|w| Some((doc.get::<crate::facet::Tween>(w)?.channel.constant()?.as_str()?.to_owned(), w))).collect())
        .unwrap_or_default();
    // The parts as the main shows them, so a row names what the canvas
    // selects and hovers.
    let main = doc.nodes_of_type(ty("use")).find(|&u| !u.is_expanded() && doc.is_live(u) && crate::expand::main_definition(doc, u) == Some(pin.def));
    let readers = machine::readers(doc, pin.def);
    let mut objects: Vec<Object> = Vec::new();
    for (c, name, _) in machine::channels(doc, pin.def) {
        let (part, label) = match readers.get(&c).and_then(|v| v.first()) {
            Some(&(part, path)) => (main.and_then(|m| NodeId::expanded(m, part)).unwrap_or(part), property_label(path)),
            None => (pin.def, name.clone()),
        };
        let i = match objects.iter().position(|o| o.target == part) {
            Some(i) => i,
            None => {
                let label = if part == pin.def { "Not drawn".to_owned() } else { readers.get(&c).and_then(|v| v.first()).map_or_else(|| label_of(store, part), |&(p, _)| label_of(store, p)) };
                objects.push(Object { target: part, label, props: Vec::new() });
                objects.len() - 1
            }
        };
        objects[i].props.push(Prop { prop: name.clone(), segments: Vec::new(), lane: false, rest: None, label: Some(label), slice: Some(SliceRow { channel: name.clone(), tween: tweens.get(&name).copied() }) });
    }
    let pps = comp.and_then(|c| number(store, c, FacetKind::Axis, "pps")).unwrap_or(100.0);
    let leaf_num = |w: NodeId, f: &str| store.resolved(w, LeafPath::new(FacetKind::Tween, f).expect("tween field")).and_then(|v| v.as_f64());
    let mut earliest = BTreeMap::new();
    for o in &objects {
        let ws: Vec<NodeId> = o.props.iter().filter_map(|p| p.slice.as_ref()?.tween).collect();
        if ws.len() == o.props.len()
            && let Some(&w) = ws.iter().min_by(|a, b| leaf_num(**a, "start").unwrap_or(0.0).total_cmp(&leaf_num(**b, "start").unwrap_or(0.0)))
        {
            earliest.insert(o.target, w);
        }
    }
    let whole = p.as_ref().and_then(|p| store.resolved(p.transition, LeafPath::new(FacetKind::Transition, "ease").expect("field"))).and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_else(|| "easy_ease".into());
    let curves = tweens.values().map(|&w| (w, store.resolved(w, LeafPath::new(FacetKind::Tween, "ease").expect("field")).and_then(|v| v.as_str().map(str::to_owned)).filter(|e| !e.trim().is_empty()).unwrap_or_else(|| whole.clone()))).collect();
    let picked = store.selection().into_iter().flat_map(|(n, _)| projected_tweens(doc, n)).filter(|w| tweens.values().any(|x| x == w)).collect();
    let inside = Inside { def: pin.def, machine: pin.machine, from: from.to_owned(), to: to.to_owned(), transition: p.as_ref().map(|p| p.transition), own, secs, open: store.inside_open().clone(), picked, earliest, curves };
    Plan { clock, objects, end: secs, tick: tick_for(pps), errors: BTreeSet::new(), light: false, inside: Some(inside) }
}

/// Where the document's animation ends, seconds: each clip's last key, or
/// a keyed state's transition after it, whichever is later (a state keyed
/// late runs on past its key, machines.md §4.2).
pub fn clips_end(store: &Store) -> f64 {
    let doc = store.document();
    let mut end: f64 = 0.0;
    for clip in doc.nodes_of_type(ty("clip")).filter(|c| !c.is_session() && !c.is_expanded()) {
        let prop = doc.get::<crate::facet::Clip>(clip).and_then(|c| c.prop.constant()).and_then(|v| v.as_str()).unwrap_or("").to_owned();
        let start = crate::time::start(doc, clip);
        let keys = crate::time::keys(doc, clip);
        for r in doc.relations_from(clip).filter(|r| r.rel == RelKind::Animates && doc.is_live(r.to)) {
            let offset = r.data.get("offset").and_then(|v| v.as_f64()).unwrap_or(0.0);
            end = end.max(start + offset + crate::time::length(&keys));
            for (k, len, _) in state_bands(store, r.to, &prop, &keys) {
                let t = keys.iter().find(|x| x.id == k).map_or(0.0, |x| x.t);
                let span = doc.get::<crate::facet::Key>(k).and_then(|f| f.span.constant()).and_then(|v| v.as_f64()).filter(|s| s.is_finite() && *s > 0.0).unwrap_or(len);
                end = end.max(start + offset + t + span);
            }
        }
    }
    end
}

/// The states of the machine an instance's `prop` (`args.<port>`) is the
/// state of, in order; `None` when it is not a state port.
fn machine_states(store: &Store, target: NodeId, prop: &str) -> Option<Vec<String>> {
    let doc = store.document();
    let port = prop.strip_prefix("args.")?;
    let def = crate::expand::linked_definition(doc, target)?;
    let (m, _) = crate::machine::machines(doc, def).into_iter().find(|(_, p)| p == port)?;
    Some(crate::machine::state_names(doc, m))
}

/// The transitions a keyed state's keys select: (key, the transition's own
/// length in seconds, its name), for each key that changes the state.
fn state_bands(store: &Store, target: NodeId, prop: &str, keys: &[crate::time::KeyAt]) -> Vec<(NodeId, f64, String)> {
    let doc = store.document();
    let Some(port) = prop.strip_prefix("args.") else { return Vec::new() };
    let Some(def) = crate::expand::linked_definition(doc, target) else { return Vec::new() };
    let Some((m, _)) = crate::machine::machines(doc, def).into_iter().find(|(_, p)| p == port) else { return Vec::new() };
    let mc = crate::machine::copy_of(doc, Some(target), m);
    let read = |l: Leaf| store.resolved(l.node, l.path);
    let word = |v: &Value| v.as_str().map(str::to_owned);
    let authored = doc.leaf(target, LeafPath::new(FacetKind::Args, port).ok().expect("an argument path")).and_then(|s| s.constant()).and_then(word);
    let mut out = Vec::new();
    for (i, k) in keys.iter().enumerate() {
        let (Some(to), from) = (word(&k.value), if i > 0 { word(&keys[i - 1].value) } else { authored.clone() }) else { continue };
        let Some(from) = from.filter(|f| *f != to) else { continue };
        if let Some(p) = crate::machine::plan(doc, &read, def, mc, &from, &to).filter(|p| p.ms > 0.0) {
            out.push((k.id, p.ms / 1000.0, name_of(store, p.transition).unwrap_or_else(|| format!("{from} → {to}"))));
        }
    }
    out
}

/// One row of the timeline, for its two halves (§12.3): the canvas draws
/// its bars and keys at `y`, the DOM column its name and controls.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// `object` or `property`.
    pub kind: &'static str,
    pub label: String,
    pub y: f64,
    pub target: NodeId,
    /// A property row's leaf path and its first clip.
    pub prop: Option<String>,
    pub clip: Option<NodeId>,
    pub error: bool,
    /// A component's state lane.
    pub lane: bool,
}

/// The rows, top to bottom, in document px from the composition's top.
pub fn rows(plan: &Plan) -> Vec<Row> {
    let mut out = Vec::new();
    let mut y = plan.top() + plan.ruler_h() + 4.0;
    for o in &plan.objects {
        let error = o.props.iter().flat_map(|p| &p.segments).any(|s| plan.errors.contains(&s.clip));
        out.push(Row { kind: "object", label: o.label.clone(), y, target: o.target, prop: None, clip: None, error, lane: false });
        y += ROW_H;
        // Inside a transition a part shows its properties when open.
        if plan.inside.as_ref().is_some_and(|i| !i.open.contains(&o.target)) {
            continue;
        }
        for p in &o.props {
            let short = p.label.clone().unwrap_or_else(|| p.prop.rsplit('.').next().unwrap_or(&p.prop).to_owned());
            let error = p.segments.iter().any(|s| plan.errors.contains(&s.clip));
            out.push(Row { kind: "property", label: short, y, target: o.target, prop: Some(p.prop.clone()), clip: p.segments.first().map(|s| s.clip), error, lane: p.lane });
            y += ROW_H;
        }
    }
    out
}

/// The tweens a piece inside a transition stands for: a slice's bar or
/// diamond its own, a part's bar all of the part's (the leaves it moves).
pub fn projected_tweens(doc: &crate::document::Document, piece: NodeId) -> Vec<NodeId> {
    let is_tween = |n: NodeId| doc.live(n).is_some_and(|x| x.ty.name() == "tween");
    let mut out: Vec<NodeId> = Vec::new();
    for r in doc.relations_from(piece).filter(|r| r.rel == RelKind::Projects) {
        if is_tween(r.to) && !out.contains(&r.to) {
            out.push(r.to);
        }
        if let Some(with) = r.data.get("with").and_then(|v| v.as_str()) {
            for w in with.split_whitespace().filter_map(|l| l.split_once('.')).filter_map(|(id, _)| id.parse::<NodeId>().ok()) {
                if is_tween(w) && !out.contains(&w) {
                    out.push(w);
                }
            }
        }
    }
    out
}

/// The generated pieces by their key (`slice <part> <channel>`, …).
pub fn pieces(store: &Store, comp: NodeId) -> BTreeMap<String, NodeId> {
    let doc = store.document();
    let mut out = BTreeMap::new();
    let mut stack = doc.children(comp).to_vec();
    while let Some(k) = stack.pop() {
        if let Some(key) = name_of(store, k) {
            out.insert(key, k);
        }
        stack.extend(doc.children(k).iter().copied());
    }
    out
}

/// `min` or `max` of several expressions, nested two at a time.
fn fold(f: &str, items: &[String]) -> String {
    match items {
        [] => "0".into(),
        [one] => one.clone(),
        [first, rest @ ..] => format!("{f}({first}, {})", fold(f, rest)),
    }
}

/// One node the timeline should have.
struct Want {
    key: String,
    ty: TypeId,
    facets: Vec<Facet>,
    /// A `projects` relation to what it stands for.
    projects: Option<NodeId>,
    /// The relation's `with`: a second leaf of the subject a drag moves
    /// by as much (`handle::rigid`; a slice's bar).
    with: Option<String>,
    /// The role group it is in (a key), or the composition.
    parent: Option<String>,
}

/// The role groups, bottom to top, and the built-in context each names
/// (std_contexts.xml): what their children afford is that data.
const GROUPS: [(&str, &str); 6] = [
    ("backdrop", "std:timeline-static"),
    ("tracks", "std:timeline-tracks"),
    ("keys", "std:timeline-keys"),
    ("headers", "std:timeline-static"),
    ("ruler", "std:timeline-ruler"),
    ("overlay", "std:timeline-overlay"),
];

fn bound(e: &str) -> Slot {
    Slot::Bound(Expr::parse(e).unwrap_or_else(|err| panic!("timeline binding {e:?}: {err}")))
}

fn lit(v: f64) -> String {
    if v < 0.0 { format!("(0 - {})", -v) } else { format!("{v}") }
}

fn named(key: &str) -> Facet {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(key.into()));
    n.wrap()
}

fn paint(c: [f32; 4]) -> Facet {
    let mut f = Fill::default();
    f.color = Slot::Const(Value::Color(c));
    f.wrap()
}

/// A position bound to `x` and `y` expressions.
fn place(x: &str, y: &str) -> Facet {
    let mut t = Transform2d::default();
    t.x = bound(x);
    t.y = bound(y);
    t.wrap()
}

fn sized(w: &str, h: &str) -> Facet {
    let mut s = Size::default();
    s.w = bound(w);
    s.h = bound(h);
    s.wrap()
}

fn rounded(r: f64) -> Facet {
    let mut c = Corners::default();
    for s in [&mut c.tl, &mut c.tr, &mut c.br, &mut c.bl] {
        *s = Slot::Const(Value::Number(r));
    }
    c.wrap()
}

fn words(content: &str, size: f64, weight: f64) -> [Facet; 2] {
    let mut t = Text::default();
    t.content = Slot::Const(Value::Str(content.into()));
    let mut f = Font::default();
    f.size = Slot::Const(Value::Number(size));
    f.weight = Slot::Const(Value::Number(weight));
    [t.wrap(), f.wrap()]
}

const ERROR: [f32; 4] = [0.9, 0.3, 0.3, 1.0];
/// The playhead's orange (#ff9933), its grip's and its transition bands':
/// not the record colour (`--record`), which marks auto-keyframe.
const PLAYHEAD: [f32; 4] = [1.0, 0.6, 0.2, 1.0];
/// The loop's span on the ruler: the playhead's orange, faint.
const LOOP: [f32; 4] = [PLAYHEAD[0], PLAYHEAD[1], PLAYHEAD[2], 0.22];
/// A state lane's stretches, by the state's index: muted hues that read in
/// both themes under light or dark labels.
const LANE: [[f32; 4]; 6] = [
    [0.36, 0.52, 0.80, 0.75],
    [0.30, 0.66, 0.54, 0.75],
    [0.72, 0.50, 0.82, 0.75],
    [0.84, 0.60, 0.30, 0.75],
    [0.80, 0.42, 0.52, 0.75],
    [0.50, 0.58, 0.40, 0.75],
];

/// The timeline's colours for one theme, in sRGB: each is a shell token
/// (src-ui/app.css), so the canvas is one surface with the app and with
/// the DOM column (TimelinePanel.svelte), which uses the tokens themselves.
/// Change them together.
struct Palette {
    /// The rows: `--bg`.
    bg: [f32; 4],
    /// Under the label column, and the ruler: `--bg-sunken`.
    header: [f32; 4],
    ruler: [f32; 4],
    /// The grid's lines: `--border`.
    grid: [f32; 4],
    /// Major ticks: `--fg-faint`; minor ones: `--border-strong`.
    tick: [f32; 4],
    dim: [f32; 4],
    /// The ruler's labels: `--fg-muted`.
    label: [f32; 4],
    summary: [f32; 4],
    key: [f32; 4],
    key_edge: [f32; 4],
    /// A clip's bar: `--accent`.
    segment: [f32; 4],
}
const fn hex(c: u32) -> [f32; 4] {
    [((c >> 16) & 0xff) as f32 / 255.0, ((c >> 8) & 0xff) as f32 / 255.0, (c & 0xff) as f32 / 255.0, 1.0]
}
const DARK: Palette = Palette {
    bg: hex(0x121314),
    header: hex(0x1c1d1f),
    ruler: hex(0x1c1d1f),
    grid: hex(0x2b2b2b),
    tick: hex(0x6e7681),
    dim: hex(0x3c3c3c),
    label: hex(0x9d9d9d),
    summary: [0.45, 0.47, 0.52, 0.55],
    key: hex(0xcccccc),
    key_edge: hex(0x121314),
    segment: hex(0x0078d4),
};
const LIGHT: Palette = Palette {
    bg: hex(0xffffff),
    header: hex(0xf3f3f3),
    ruler: hex(0xf3f3f3),
    grid: hex(0xe5e5e5),
    tick: hex(0x8b8b8b),
    dim: hex(0xcecece),
    label: hex(0x616161),
    summary: [0.55, 0.57, 0.62, 0.35],
    key: hex(0xffffff),
    key_edge: hex(0x3b3b3b),
    segment: hex(0x005fb8),
};
/// A key's diamond: a square this wide, turned 45°.
const KEY_PX: f64 = 10.0;

/// Every node the plan draws: the role groups, bottom to top, each with its
/// pieces in drawing order.
fn wants(plan: &Plan, comp: NodeId) -> Vec<Want> {
    if let Some(inside) = &plan.inside {
        return inside_wants(plan, comp, inside);
    }
    let ax = |f: &str| format!("{comp}.axis.{f}");
    let (lw, t0, pps, y0) = (ax("label_w"), ax("t0"), ax("pps"), ax("y0"));
    // A row's y, `v` px from the top, scrolled by the axis's `y0`: the rows
    // scroll under the ruler, which stays.
    let ry = |v: f64| format!("{} - {y0}", lit(v));
    // Where time `e` (an expression) is, in px.
    let at = |e: &str| format!("{lw} + ({e} - {t0}) * {pps}");
    // The static pieces reach well past any panel, so the view reads as
    // one surface however it is sized or scrolled.
    let h = lit(FAR);
    let clock = plan.clock;
    let pal = if plan.light { &LIGHT } else { &DARK };
    let mut by: BTreeMap<&'static str, Vec<Want>> = BTreeMap::new();
    let mut push = |group: &'static str, key: String, ty_name: &str, facets: Vec<Facet>, projects: Option<NodeId>| {
        let mut facets = facets;
        facets.push(named(&key));
        by.entry(group).or_default().push(Want { key, ty: ty(ty_name), facets, projects, with: None, parent: Some(group.to_owned()) });
    };
    let width = at(&lit(plan.end));

    // Backdrop and grid.
    push("backdrop", "background".into(), "rect", vec![place("0", "0"), sized(&lit(FAR), &h), paint(pal.bg)], None);
    let ticks = (plan.end / plan.tick).round() as usize;
    for k in 0..=ticks {
        let x = at(&lit(k as f64 * plan.tick));
        push("backdrop", format!("grid {k}"), "rect", vec![place(&x, &lit(RULER_H)), sized("1", &lit(FAR - RULER_H)), paint(pal.grid)], None);
    }

    // The ruler: a track laying the clock's time along it (std:timeline-
    // ruler), so a press anywhere on it moves the playhead there.
    push("ruler", "ruler bar".into(), "rect", vec![place(&lw, "0"), sized(&format!("({}) - {lw}", width), &lit(RULER_H)), paint(pal.ruler)], None);
    push("ruler", "loop".into(), "rect", vec![place(&at(&format!("{clock}.clock.loop_in")), "0"), sized(&format!("max({clock}.clock.loop_out - {clock}.clock.loop_in, 0) * {pps}"), &lit(RULER_H)), paint(LOOP)], None);
    const MINOR: usize = 5;
    for k in 0..=ticks * MINOR {
        let t = k as f64 * plan.tick / MINOR as f64;
        let x = at(&lit(t));
        let major = k % MINOR == 0;
        let len = if major { 14.0 } else { 6.0 };
        push("ruler", format!("tick {k}"), "rect", vec![place(&x, &lit(RULER_H - len)), sized("1", &lit(len)), paint(if major { pal.tick } else { pal.dim })], None);
        if major {
            let text = if plan.tick < 1.0 { format!("{t:.1}s") } else { format!("{t}s") };
            let mut f = vec![place(&format!("{x} + 4"), "8")];
            f.extend(words(&text, LABEL_PX, 500.0));
            f.push(paint(pal.label));
            push("ruler", format!("tick label {}", k / MINOR), "text", f, None);
        }
    }
    let head = at(&format!("{clock}.clock.t"));
    // The playhead's head, low on the ruler under the labels.
    push("ruler", "playhead grip".into(), "rect", vec![place(&format!("{head} - 6"), &lit(RULER_H - 20.0)), sized("12", "18"), rounded(3.0), paint(PLAYHEAD)], None);

    // Rows (their names and controls are the DOM column's, §12.3): per
    // object a summary of its keys, per property a bar per clip and a
    // diamond per key.
    let rows = rows(plan);
    let mut ri = rows.iter();
    let key_x = |s: &Segment, k: NodeId| format!("{}.timing.start + {} + {k}.key.t", s.clip, lit(s.offset));
    for o in &plan.objects {
        let row = ri.next().expect("an object row");
        let all: Vec<String> = o.props.iter().flat_map(|p| &p.segments).flat_map(|s| s.keys.iter().map(move |&k| key_x(s, k))).collect();
        if !all.is_empty() {
            let (lo, hi) = (fold("min", &all), fold("max", &all));
            push("backdrop", format!("summary {}", o.target), "rect", vec![place(&at(&lo), &ry(row.y + 5.0)), sized(&format!("max(({hi}) - ({lo}), 0) * {pps}"), &lit(ROW_H - 10.0)), rounded(3.0), paint(pal.summary)], None);
        }
        for p in &o.props {
            let row = ri.next().expect("a property row");
            // A state lane (machines.md §6): a stretch per state, named,
            // from its key to the next, under the bar (which stays, clear,
            // for dragging the clip). An unkeyed one holds all along.
            // A keyed stretch stands for its key (in the tracks: a click
            // selects that occurrence, a drag moves it, a double-click opens
            // the state); one before the first key, or an unkeyed lane's, is
            // static.
            let stretch = |push: &mut dyn FnMut(&'static str, String, &str, Vec<Facet>, Option<NodeId>), key: String, from: String, to: String, name: &str, index: usize, faint: bool, of: Option<NodeId>| {
                let mut c = LANE[index % LANE.len()];
                if faint {
                    c[3] *= 0.55;
                }
                let group = if of.is_some() { "tracks" } else { "backdrop" };
                push(group, format!("lane {key}"), "rect", vec![place(&at(&from), &ry(row.y + 3.0)), sized(&format!("max(({to}) - ({from}), 0) * {pps}"), &lit(SEG_H)), rounded(4.0), paint(c)], of);
                let mut f = vec![place(&format!("{} + 7", at(&from)), &ry(row.y + 4.0))];
                f.extend(words(name, 11.0, 600.0));
                f.push(paint(if plan.light { [0.12, 0.12, 0.14, 0.92] } else { [1.0, 1.0, 1.0, 0.92] }));
                push(group, format!("lane label {key}"), "text", f, None);
            };
            if let Some((state, index)) = &p.rest {
                stretch(&mut push, format!("{} {}", o.target, p.prop), "0".into(), lit(plan.end), state, *index, true, None);
            }
            for s in &p.segments {
                if let (Some((name, index)), Some((first, ..))) = (&s.before, s.states.first()) {
                    stretch(&mut push, format!("before {} {}", s.clip, o.target), "0".into(), key_x(s, *first), name, *index, true, None);
                }
                for (i, (k, name, index)) in s.states.iter().enumerate() {
                    let to = s.states.get(i + 1).map_or_else(|| lit(plan.end), |(n, ..)| key_x(s, *n));
                    // A key that changes the state says through which
                    // transition, and how long it takes (its band).
                    let said = match s.bands.iter().find(|(b, ..)| b == k) {
                        Some((_, len, t)) => format!("→ {name} · {t} {}s", (len * 100.0).round() / 100.0),
                        None => name.clone(),
                    };
                    stretch(&mut push, format!("{k} {}", o.target), key_x(s, *k), to, &said, *index, false, Some(*k));
                }
            }
            for s in &p.segments {
                let mut colour = if plan.errors.contains(&s.clip) { ERROR } else { pal.segment };
                if !s.states.is_empty() && !plan.errors.contains(&s.clip) {
                    colour = [colour[0], colour[1], colour[2], 0.0];
                }
                let times: Vec<String> = s.keys.iter().map(|&k| format!("{k}.key.t")).collect();
                let (lo, hi) = (fold("min", &times), fold("max", &times));
                // The clip's bar: from its first key to its last. A drag
                // retimes the clip (it projects the clip, so its start is
                // the leaf the drag moves); Delete deletes the clip.
                let x = at(&format!("{}.timing.start + {} + {lo}", s.clip, lit(s.offset)));
                // A state lane's stretches stand in for its bar.
                if s.states.is_empty() || plan.errors.contains(&s.clip) {
                push("tracks", format!("bar {} {}", s.clip, o.target), "rect", vec![place(&x, &ry(row.y + 3.0)), sized(&format!("max(({hi}) - ({lo}), 0) * {pps}"), &lit(SEG_H)), rounded(4.0), paint(colour)], Some(s.clip));
                }
                if s.states.is_empty() {
                    let mut f = vec![place(&format!("{x} + 8"), &ry(row.y + 4.0))];
                    f.extend(words(&s.label, 11.0, 500.0));
                    f.push(paint([1.0, 1.0, 1.0, 0.9]));
                    push("tracks", format!("bar label {} {}", s.clip, o.target), "text", f, None);
                }
                // A keyed state's transitions (machines.md §4.2): a band rising
                // from each key that changes the state over the transition's
                // length, or the key's own span (one occurrence retimed).
                for (k, len, name) in &s.bands {
                    let x = at(&key_x(s, *k));
                    let w = format!("(isnan({k}.key.span) || {k}.key.span <= 0 ? {} : {k}.key.span) * {pps}", lit(*len));
                    let h = SEG_H;
                    let mut shape = crate::facet::PathShape::default();
                    shape.d = Slot::Bound(Expr::parse(&format!("'M 0,{h} L ' + str({w}) + ',0 L ' + str({w}) + ',{h} Z'")).expect("a band's path"));
                    let mut fill = Fill::default();
                    fill.color = Slot::Const(Value::Color([PLAYHEAD[0], PLAYHEAD[1], PLAYHEAD[2], 0.35]));
                    let _ = name;
                    push("tracks", format!("band {k} {}", o.target), "path", vec![place(&x, &ry(row.y + 3.0)), shape.wrap(), fill.wrap()], Some(*k));
                }
                // A diamond per key: a drag moves the key in time (it
                // projects the key), Delete deletes it.
                let side = KEY_PX / std::f64::consts::SQRT_2;
                for (i, &k) in s.keys.iter().enumerate() {
                    let mut t = Transform2d::default();
                    t.x = bound(&at(&key_x(s, k)));
                    t.y = bound(&ry(row.y + (ROW_H - KEY_PX * std::f64::consts::SQRT_2) / 2.0));
                    t.rot = bound(&lit(std::f64::consts::FRAC_PI_4));
                    let mut st = Stroke::default();
                    st.color = Slot::Const(Value::Color(pal.key_edge));
                    st.width = Slot::Const(Value::Number(1.0));
                    let mut facets = vec![t.wrap(), sized(&lit(side * std::f64::consts::SQRT_2), &lit(side * std::f64::consts::SQRT_2)), paint(pal.key), st.wrap()];
                    // The first and last keys are where the clip starts and
                    // ends: moving them trims it, and the cursor says so
                    // (Figma's trim cursors, §12.5).
                    let end = if s.keys.len() < 2 { None } else if i == 0 { Some("trim-start") } else if i + 1 == s.keys.len() { Some("trim-end") } else { None };
                    if let Some(cursor) = end {
                        let mut a = crate::facet::Affords::default();
                        a.style = Slot::Const(Value::Str(format!("cursor: {cursor}")));
                        facets.push(a.wrap());
                    }
                    push("keys", format!("key {k} {}", o.target), "rect", facets, Some(k));
                }
            }
        }
    }
    push("headers", "header".into(), "rect", vec![place("0", "0"), sized(&lw, &h), paint(pal.header)], None);
    // The playhead's line, over everything and transparent to the pointer.
    push("overlay", "playhead".into(), "rect", vec![place(&format!("{head} - 1"), "0"), sized("2", &h), paint(PLAYHEAD)], None);

    let mut out = Vec::new();
    for (group, context) in GROUPS {
        let mut c = ContextOf::default();
        c.name = Slot::Const(Value::Str(context.into()));
        let mut facets = vec![named(group), c.wrap()];
        if group == "ruler" {
            // The clock's time along the ruler: local x means
            // `t0 + (x - label_w) / pps`.
            let mut track = Track::default();
            track.value = bound(&format!("{clock}.clock.t"));
            track.origin = bound(&format!("{t0} - {lw} / {pps}"));
            track.scale = bound(&format!("1 / {pps}"));
            // Time starts at 0 and runs on: the playhead never goes before it.
            track.min = Slot::Const(Value::Number(0.0));
            track.max = Slot::Const(Value::Number(0.0));
            facets.push(track.wrap());
        }
        out.push(Want { key: group.into(), ty: ty("group"), facets, projects: None, with: None, parent: None });
        out.extend(by.remove(group).unwrap_or_default());
    }
    out
}

/// The nodes inside a transition (machines.md §6): the same grammar as the
/// document's timeline, in the transition's time. A slice is a bar between
/// two diamonds: the bar moves the slice (its start and end together), a
/// diamond trims that end. A pair on the default timing shows its curve
/// but does not move until it has timing of its own.
fn inside_wants(plan: &Plan, comp: NodeId, inside: &Inside) -> Vec<Want> {
    let ax = |f: &str| format!("{comp}.axis.{f}");
    let (lw, t0, pps, y0, cursor) = (ax("label_w"), ax("t0"), ax("pps"), ax("y0"), ax("cursor"));
    let (top, inset, rh) = (plan.top(), plan.inset(), plan.ruler_h());
    let left = format!("{} + {lw}", lit(inset));
    let at = |e: &str| format!("{left} + ({e} - {t0}) * {pps}");
    let ry = |v: f64| format!("{} - {y0}", lit(v));
    // The transition's length, live: retiming it rescales the bars.
    let secs = match inside.transition {
        Some(t) => format!("({t}.transition.ms / 1000)"),
        None => lit(inside.secs),
    };
    let h = lit(FAR);
    let pal = if plan.light { &LIGHT } else { &DARK };
    let mut by: BTreeMap<&'static str, Vec<Want>> = BTreeMap::new();
    let mut push = |group: &'static str, key: String, ty_name: &str, facets: Vec<Facet>, projects: Option<NodeId>, with: Option<String>| {
        let mut facets = facets;
        facets.push(named(&key));
        by.entry(group).or_default().push(Want { key, ty: ty(ty_name), facets, projects, with, parent: Some(group.to_owned()) });
    };
    push("backdrop", "background".into(), "rect", vec![place("0", "0"), sized(&lit(FAR), &h), paint(pal.bg)], None, None);
    let ticks = (plan.end / plan.tick).floor() as usize;
    for k in 0..=ticks {
        let x = at(&lit(k as f64 * plan.tick));
        push("backdrop", format!("grid {k}"), "rect", vec![place(&x, &lit(top + rh)), sized("1", &lit(FAR)), paint(pal.grid)], None, None);
    }
    // The end of the transition: past it there is no time.
    push("backdrop", "after".into(), "rect", vec![place(&at(&secs), &lit(top)), sized(&lit(FAR), &h), paint(pal.header)], None, None);

    push("ruler", "ruler bar".into(), "rect", vec![place(&left, &lit(top)), sized(&format!("max(({}) - ({left}), 0)", at(&secs)), &lit(rh)), paint(pal.ruler)], None, None);
    const MINOR: usize = 5;
    let digits = if plan.tick >= 1.0 { 0 } else if plan.tick >= 0.1 { 1 } else { 2 };
    for k in 0..=ticks * MINOR {
        let t = k as f64 * plan.tick / MINOR as f64;
        if t > plan.end + 1e-9 {
            break;
        }
        let x = at(&lit(t));
        let major = k % MINOR == 0;
        let len = if major { 10.0 } else { 5.0 };
        push("ruler", format!("tick {k}"), "rect", vec![place(&x, &lit(top + rh - len)), sized("1", &lit(len)), paint(if major { pal.tick } else { pal.dim })], None, None);
        if major {
            let mut f = vec![place(&format!("{x} + 4"), &lit(top + 3.0))];
            f.extend(words(&format!("{t:.digits$}s"), 11.0, 500.0));
            f.push(paint(pal.label));
            push("ruler", format!("tick label {}", k / MINOR), "text", f, None, None);
        }
    }
    let head = at(&cursor);
    push("ruler", "playhead grip".into(), "rect", vec![place(&format!("{head} - 5"), &lit(top + rh - 12.0)), sized("10", "12"), rounded(3.0), paint(PLAYHEAD)], None, None);

    let rows = rows(plan);
    let mut ri = rows.iter();
    let side = KEY_PX / std::f64::consts::SQRT_2;
    for o in &plan.objects {
        let row = ri.next().expect("an object row");
        let (los, his): (Vec<String>, Vec<String>) = o.props.iter().filter_map(|p| p.slice.as_ref()).map(|s| match s.tween {
            Some(w) => (format!("{w}.tween.start"), format!("{w}.tween.end")),
            None => ("0".into(), "1".into()),
        }).unzip();
        let open = inside.open.contains(&o.target);
        match inside.earliest.get(&o.target).filter(|_| inside.own) {
            // The part's bar: from its first slice's start to its last's
            // end. A drag moves all of the part's slices together (its x
            // reads the first one's start; the rest move with it).
            Some(&first) => {
                let ws: Vec<NodeId> = o.props.iter().filter_map(|p| p.slice.as_ref()?.tween).collect();
                let with = ws.iter().flat_map(|w| [format!("{w}.tween.start"), format!("{w}.tween.end")]).collect::<Vec<_>>().join(" ");
                let all_picked = ws.iter().all(|w| inside.picked.contains(w));
                let mut c = pal.segment;
                c[3] = if open { 0.3 } else if inside.picked.is_empty() || all_picked { 0.85 } else { 0.35 };
                let mut facets = vec![place(&at(&format!("{first}.tween.start * {secs}")), &ry(row.y + 4.0)), sized(&format!("max(({hi}) - {first}.tween.start, 0) * {secs} * {pps}", hi = fold("max", &his)), &lit(ROW_H - 8.0)), rounded(4.0), paint(c)];
                if all_picked && !inside.picked.is_empty() {
                    let mut st = Stroke::default();
                    st.color = Slot::Const(Value::Color(pal.key));
                    st.width = Slot::Const(Value::Number(1.5));
                    facets.push(st.wrap());
                }
                push("tracks", format!("part {}", o.target), "rect", facets, Some(first), Some(with));
            }
            None if !los.is_empty() => {
                let (lo, hi) = (fold("min", &los), fold("max", &his));
                push("backdrop", format!("summary {}", o.target), "rect", vec![place(&at(&format!("({lo}) * {secs}")), &ry(row.y + 5.0)), sized(&format!("max(({hi}) - ({lo}), 0) * {secs} * {pps}"), &lit(ROW_H - 10.0)), rounded(3.0), paint(pal.summary)], None, None);
            }
            None => {}
        }
        if !open {
            continue;
        }
        for p in &o.props {
            let row = ri.next().expect("a property row");
            let Some(slice) = &p.slice else { continue };
            let key = format!("{} {}", o.target, slice.channel);
            match slice.tween.filter(|_| inside.own) {
                Some(w) => {
                    // The bar moves the slice: its x reads the start, and the
                    // drag carries the end with it (`with`).
                    // Keyed by the slice itself: a kept piece keeps what it
                    // projects, so another transition's bars are new ones.
                    // Selected slices stand out, outlined; while some are,
                    // the rest recede.
                    let picked = inside.picked.contains(&w);
                    let mut c = pal.segment;
                    if !inside.picked.is_empty() && !picked {
                        c[3] = 0.4;
                    }
                    let width = format!("max({w}.tween.end - {w}.tween.start, 0) * {secs} * {pps}");
                    let mut facets = vec![place(&at(&format!("{w}.tween.start * {secs}")), &ry(row.y + 3.0)), sized(&width, &lit(SEG_H)), rounded(4.0), paint(c)];
                    if picked {
                        let mut st = Stroke::default();
                        st.color = Slot::Const(Value::Color(pal.key));
                        st.width = Slot::Const(Value::Number(1.5));
                        facets.push(st.wrap());
                    }
                    push("tracks", format!("slice {w}"), "rect", facets, Some(w), Some("tween.end".to_owned()));
                    // Its easing, drawn in the bar (over everything, inert).
                    if let Some(curve) = inside.curves.get(&w).and_then(|e| crate::time::curve(e).ok()) {
                        const N: usize = 16;
                        let (top_y, h) = (2.5, SEG_H - 5.0);
                        let mut d = format!("'M 0,{} ", lit(top_y + h));
                        for i in 1..=N {
                            let u = i as f64 / N as f64;
                            let y = top_y + h * (1.0 - curve.at(u));
                            d.push_str(&format!("L ' + str(({width}) * {u}) + ',{y:.2} "));
                        }
                        d.push('\'');
                        let mut shape = crate::facet::PathShape::default();
                        shape.d = Slot::Bound(Expr::parse(&d).expect("a curve's path"));
                        let mut st = Stroke::default();
                        st.color = Slot::Const(Value::Color([1.0, 1.0, 1.0, if c[3] < 1.0 { 0.4 } else { 0.85 }]));
                        st.width = Slot::Const(Value::Number(1.25));
                        push("overlay", format!("curve {w}"), "path", vec![place(&at(&format!("{w}.tween.start * {secs}")), &ry(row.y + 3.0)), shape.wrap(), st.wrap()], None, None);
                    }
                    for (end, field, cursor) in [("start", "start", "trim-start"), ("end", "end", "trim-end")] {
                        let mut t = Transform2d::default();
                        t.x = bound(&at(&format!("{w}.tween.{field} * {secs}")));
                        t.y = bound(&ry(row.y + (ROW_H - KEY_PX * std::f64::consts::SQRT_2) / 2.0));
                        t.rot = bound(&lit(std::f64::consts::FRAC_PI_4));
                        let mut st = Stroke::default();
                        st.color = Slot::Const(Value::Color(pal.key_edge));
                        st.width = Slot::Const(Value::Number(1.0));
                        let mut a = crate::facet::Affords::default();
                        a.style = Slot::Const(Value::Str(format!("cursor: {cursor}")));
                        push("keys", format!("{end} {w}"), "rect", vec![t.wrap(), sized(&lit(side * std::f64::consts::SQRT_2), &lit(side * std::f64::consts::SQRT_2)), paint(pal.key), st.wrap(), a.wrap()], Some(w), None);
                    }
                }
                None => {
                    // Following the transition's curve over all of it (or a
                    // pair on the default timing): drawn, not a target.
                    let (a, b) = match slice.tween {
                        Some(w) => (format!("{w}.tween.start"), format!("{w}.tween.end")),
                        None => ("0".into(), "1".into()),
                    };
                    let mut c = pal.segment;
                    c[3] = 0.28;
                    push("backdrop", format!("follow {key}"), "rect", vec![place(&at(&format!("({a}) * {secs}")), &ry(row.y + 3.0)), sized(&format!("max(({b}) - ({a}), 0) * {secs} * {pps}"), &lit(SEG_H)), rounded(4.0), paint(c)], None, None);
                }
            }
        }
    }
    // The column and the editor's header are DOM over these.
    push("headers", "header".into(), "rect", vec![place("0", "0"), sized(&left, &h), paint(pal.header)], None, None);
    push("headers", "top".into(), "rect", vec![place("0", "0"), sized(&lit(FAR), &lit(top)), paint(pal.header)], None, None);
    push("overlay", "playhead".into(), "rect", vec![place(&format!("{head} - 1"), &lit(top)), sized("2", &h), paint(PLAYHEAD)], None, None);

    let mut out = Vec::new();
    for (group, context) in GROUPS {
        let mut c = ContextOf::default();
        c.name = Slot::Const(Value::Str(context.into()));
        let mut facets = vec![named(group), c.wrap()];
        if group == "ruler" {
            // How far into the transition, along the ruler.
            let mut track = Track::default();
            track.value = bound(&cursor);
            track.origin = bound(&format!("{t0} - ({left}) / {pps}"));
            track.scale = bound(&format!("1 / {pps}"));
            track.min = Slot::Const(Value::Number(0.0));
            track.max = Slot::Const(Value::Number(inside.secs.max(1e-3)));
            facets.push(track.wrap());
        }
        out.push(Want { key: group.into(), ty: ty("group"), facets, projects: None, with: None, parent: None });
        out.extend(by.remove(group).unwrap_or_default());
    }
    out
}

/// The session ops that make the store's timeline composition show `plan`:
/// the composition created when missing (with an axis), nodes whose key
/// survives kept and updated in place, the rest created or deleted.
pub fn reconcile(store: &Store, plan: &Plan) -> Vec<Op> {
    let doc = store.document();
    let mut ops = Vec::new();
    let existing = find(store);
    let comp = match existing {
        Some(c) => c,
        None => {
            let id = doc.next_session_ids(1)[0];
            let mut c = Composition::default();
            c.dims = Slot::Const(Value::Enum("2d".into()));
            c.notation = Slot::Const(Value::Str(NOTATION.into()));
            let order = doc.roots().last().and_then(|&r| doc.live(r)).map_or_else(OrderKey::first, |n| OrderKey::after(&n.order));
            ops.push(Op::Create { id, ty: ty("composition"), parent: None, order, facets: vec![named(TIMELINE), c.wrap(), Axis::default().wrap()] });
            id
        }
    };
    let wants = wants(plan, comp);
    // Every generated node, at any depth, by its key.
    let mut have: BTreeMap<String, NodeId> = BTreeMap::new();
    if existing.is_some() {
        let mut stack: Vec<NodeId> = doc.children(comp).to_vec();
        while let Some(k) = stack.pop() {
            if let Some(key) = name_of(store, k) {
                have.insert(key, k);
            }
            stack.extend(doc.children(k).iter().copied());
        }
    }
    let fresh = wants.iter().filter(|w| !have.get(&w.key).is_some_and(|&id| doc.live(id).is_some_and(|n| n.ty == w.ty))).count();
    let mut ids = doc.next_session_ids(fresh + ops.len()).into_iter().skip(ops.len());
    let links: Vec<_> = wants.iter().filter(|w| w.projects.is_some()).collect();
    let mut rel_ids = doc.next_session_relation_ids(links.len()).into_iter();
    let wanted: BTreeSet<&str> = wants.iter().map(|w| w.key.as_str()).collect();
    let doomed: Vec<NodeId> = have
        .iter()
        .filter(|(key, id)| !(wanted.contains(key.as_str()) && wants.iter().any(|w| &w.key == *key && doc.live(**id).is_some_and(|n| n.ty == w.ty))))
        .map(|(_, &id)| id)
        .collect();
    // Deleting a group deletes its pieces: delete only the outermost.
    for &id in &doomed {
        if !doomed.iter().any(|&o| o != id && doc.is_ancestor(o, id)) {
            ops.push(Op::Delete { id });
        }
    }
    // A node's id once known, by key: groups come before their pieces.
    let mut placed: BTreeMap<String, NodeId> = BTreeMap::new();
    let mut orders: BTreeMap<Option<String>, OrderKey> = BTreeMap::new();
    for w in wants {
        let parent = match &w.parent {
            Some(g) => *placed.get(g).expect("a group before its pieces"),
            None => comp,
        };
        let order = orders.get(&w.parent).map_or_else(OrderKey::first, OrderKey::after);
        orders.insert(w.parent.clone(), order.clone());
        let kept = have.get(&w.key).copied().filter(|&id| doc.live(id).is_some_and(|n| n.ty == w.ty));
        match kept {
            Some(id) => {
                placed.insert(w.key.clone(), id);
                if doc.live(id).is_some_and(|n| n.parent != Some(parent)) {
                    ops.push(Op::Reparent { id, parent: Some(parent), order: order.clone() });
                } else if doc.live(id).is_some_and(|n| n.order != order) {
                    ops.push(Op::Reorder { id, order: order.clone() });
                }
                for f in w.facets {
                    match doc.facet(id, f.kind()) {
                        None => ops.push(Op::AddFacet { id, facet: f }),
                        Some(e) if *e != f => {
                            for field in f.field_names() {
                                if e.get(field) != f.get(field) {
                                    let path = LeafPath::new(f.kind(), field).expect("declared field");
                                    ops.push(Op::Set { id, path, slot: f.get(field).cloned().expect("declared field") });
                                }
                            }
                        }
                        Some(_) => {}
                    }
                }
            }
            None => {
                let id = ids.next().expect("an id per new node");
                placed.insert(w.key.clone(), id);
                ops.push(Op::Create { id, ty: w.ty, parent: Some(parent), order: order.clone(), facets: w.facets });
                if let Some(to) = w.projects {
                    let rel = rel_ids.next().expect("a relation id per link");
                    let data = w.with.clone().map(|p| BTreeMap::from([("with".to_owned(), Value::Str(p))])).unwrap_or_default();
                    ops.push(Op::Link { rel: Relation { id: rel, from: id, to, rel: RelKind::Projects, data } });
                }
            }
        }
    }
    ops
}

/// Bring the timeline composition up to date: build it when missing,
/// regenerate it when `last` (the plan it was built from) differs. The
/// clock is created when missing. Returns the ids the ops touched, `None`
/// when nothing changed.
pub fn sync(store: &mut Store, last: &mut Option<(NodeId, Plan)>) -> Result<Option<Vec<NodeId>>, String> {
    sync_themed(store, last, false)
}

/// `sync`, drawn in the light theme's colours when `light`.
pub fn sync_themed(store: &mut Store, last: &mut Option<(NodeId, Plan)>, light: bool) -> Result<Option<Vec<NodeId>>, String> {
    let clock = store.ensure_clock().map_err(|e| e.message)?;
    let comp = find(store);
    let mut plan = plan(store, clock, comp);
    plan.light = light;
    // Inside a transition the playhead is the preview: the main shows the
    // moment it is at. Entering one puts the playhead where the preview is.
    if let (Some(c), Some(inside)) = (comp, &plan.inside)
        && let Some(pin) = store.pinned_state().cloned()
        && let Some((from, to, progress)) = pin.scrub.clone()
    {
        let entering = last.as_ref().and_then(|(_, lp)| lp.inside.as_ref()).is_none_or(|was| (was.def, was.machine, &was.from, &was.to) != (inside.def, inside.machine, &inside.from, &inside.to));
        let secs = inside.secs.max(1e-6);
        let cursor_path = LeafPath::new(FacetKind::Axis, "cursor").expect("axis field");
        if entering {
            store.apply_session(vec![Op::Set { id: c, path: cursor_path, slot: Slot::Const(Value::Number(progress * secs)) }]).map_err(|e| e.message)?;
        } else {
            let p = (number(store, c, FacetKind::Axis, "cursor").unwrap_or(0.0) / secs).clamp(0.0, 1.0);
            if (p - progress).abs() > 1e-9 {
                store.pin_state(Some(crate::machine::Pinned { scrub: Some((from, to, p)), ..pin }));
            }
        }
    }
    if let (Some(c), Some((lc, lp))) = (comp, last.as_ref())
        && *lc == c
        && *lp == plan
    {
        return Ok(None);
    }
    let ops = reconcile(store, &plan);
    let mut touched = Vec::new();
    for op in &ops {
        match op {
            Op::Create { id, .. } | Op::Delete { id } | Op::Set { id, .. } | Op::AddFacet { id, .. } | Op::Reorder { id, .. } | Op::Reparent { id, .. } => touched.push(*id),
            _ => {}
        }
    }
    if !ops.is_empty() {
        store.apply_session(ops).map_err(|e| e.message)?;
    }
    let comp = find(store).ok_or("the timeline composition was not built")?;
    *last = Some((comp, plan));
    Ok(Some(touched))
}

/// Zoom limits for the axis, px per second.
pub const PPS: (f64, f64) = (4.0, 4000.0);

fn axis_ops(store: &Store, t0: f64, pps: f64) -> Result<(NodeId, Vec<Op>), String> {
    let comp = find(store).ok_or("no timeline composition")?;
    let set = |f: &str, v: f64| Op::Set { id: comp, path: LeafPath::new(FacetKind::Axis, f).expect("axis field"), slot: Slot::Const(Value::Number(v)) };
    Ok((comp, vec![set("t0", t0.max(0.0)), set("pps", pps.clamp(PPS.0, PPS.1))]))
}

/// Navigate the timeline's time axis (timeline.md §6): pan by `dx` document
/// px (positive moves later), and zoom by `zoom` (log scale, positive zooms
/// out) keeping the time under document x `at` where it is. Session ops:
/// how a view is looked at is never an edit.
pub fn navigate(store: &mut Store, dx: f64, zoom: f64, at: f64) -> Result<(), String> {
    let Some(comp) = find(store) else { return Ok(()) };
    let read = |f: &str, d: f64| number(store, comp, FacetKind::Axis, f).unwrap_or(d);
    let inset = store.clock().map(|c| plan(store, c, Some(comp)).inset()).unwrap_or(0.0);
    let (t0, pps, lw) = (read("t0", 0.0), read("pps", 100.0), read("label_w", 160.0) + inset);
    let under = t0 + (at - lw) / pps;
    let next = (pps * (-zoom).exp()).clamp(PPS.0, PPS.1);
    let t0 = under - (at - lw) / next + dx / next;
    let (_, ops) = axis_ops(store, t0, next)?;
    store.apply_session(ops).map_err(|e| e.message)
}

/// Scroll the rows by `dy` document px (positive shows later rows), between
/// the top and the last row just under the ruler. A session op, as
/// [`navigate`] is.
pub fn scroll(store: &mut Store, dy: f64) -> Result<(), String> {
    let Some(comp) = find(store) else { return Ok(()) };
    let y0 = number(store, comp, FacetKind::Axis, "y0").unwrap_or(0.0);
    let last = store.clock().map(|c| {
        let p = plan(store, c, Some(comp));
        rows(&p).last().map_or(0.0, |r| r.y - p.top() - p.ruler_h() - 4.0)
    }).unwrap_or(0.0);
    let next = (y0 + dy).clamp(0.0, last.max(0.0));
    if next == y0 {
        return Ok(());
    }
    let op = Op::Set { id: comp, path: LeafPath::new(FacetKind::Axis, "y0").expect("axis field"), slot: Slot::Const(Value::Number(next)) };
    store.apply_session(vec![op]).map_err(|e| e.message)
}

/// Fit every clip into `width` document px of the view (`F`): the axis
/// starts at 0, and the last clip's end sits a little short of the edge.
pub fn fit(store: &mut Store, width: f64) -> Result<(), String> {
    let Some(comp) = find(store) else { return Ok(()) };
    let lw = number(store, comp, FacetKind::Axis, "label_w").unwrap_or(160.0);
    // Inside a transition: all of it, from its start.
    if let Some(p) = store.clock().map(|c| plan(store, c, Some(comp))).filter(|p| p.inside.is_some()) {
        // Short of the edge by the view's settings button, so the last
        // diamonds stay clear of it.
        let pps = (width - lw - p.inset() - 56.0).max(40.0) / p.end.max(0.05);
        let (_, ops) = axis_ops(store, 0.0, pps)?;
        return store.apply_session(ops).map_err(|e| e.message);
    }
    let end = clips_end(store);
    let pps = (width - lw - 24.0).max(40.0) / end.max(1.0);
    let (_, ops) = axis_ops(store, 0.0, pps)?;
    store.apply_session(ops).map_err(|e| e.message)
}

// ---- Keys (§12.2) -------------------------------------------------------------

/// How near the playhead a key counts as at it: half a frame (1/30 s).
pub const AT_KEY: f64 = 1.0 / 60.0;

/// The first clip animating `leaf`, with the target's stagger offset.
pub fn clip_of(doc: &crate::document::Document, leaf: Leaf) -> Option<(NodeId, f64)> {
    let path = leaf.path.to_string();
    doc.relations_to(leaf.node).filter(|r| r.rel == RelKind::Animates && doc.is_live(r.from)).find_map(|r| {
        let prop = doc.get::<crate::facet::Clip>(r.from)?.prop.constant()?.as_str()?.to_owned();
        (prop == path).then(|| (r.from, r.data.get("offset").and_then(|v| v.as_f64()).unwrap_or(0.0)))
    })
}

/// Whether auto-keyframe is on (the clock's `autokey`).
pub fn autokey(store: &Store) -> bool {
    store.clock().and_then(|c| store.resolved(c, LeafPath::new(FacetKind::Clock, "autokey").ok()?)) == Some(Value::Bool(true))
}

/// `v` moved by `to − from`, for the kinds that move (numbers, quantities,
/// vectors, colours).
fn shifted(v: &Value, from: &Value, to: &Value) -> Option<Value> {
    use Value::*;
    let d = |v: f64, a: f64, b: f64| v + (b - a);
    Some(match (v, from, to) {
        (Number(v), Number(a), Number(b)) => Number(d(*v, *a, *b)),
        (Quantity { si: v, dim }, Quantity { si: a, .. }, Quantity { si: b, .. }) => Quantity { si: d(*v, *a, *b), dim: *dim },
        (Vec2(v), Vec2(a), Vec2(b)) => Vec2([d(v[0], a[0], b[0]), d(v[1], a[1], b[1])]),
        (Vec3(v), Vec3(a), Vec3(b)) => Vec3([d(v[0], a[0], b[0]), d(v[1], a[1], b[1]), d(v[2], a[2], b[2])]),
        (Color(v), Color(a), Color(b)) => Color(std::array::from_fn(|i| (v[i] + (b[i] - a[i])).clamp(0.0, 1.0))),
        _ => return None,
    })
}

fn key_facet(t: f64, value: Value, easing: &str) -> Facet {
    let mut k = crate::facet::Key::default();
    k.t = Slot::Const(Value::Number(t));
    k.value = Slot::Const(value);
    k.easing = Slot::Const(Value::Enum(easing.into()));
    k.wrap()
}

fn key_path(f: &str) -> LeafPath {
    LeafPath::new(FacetKind::Key, f).expect("a key field")
}

/// Ids for the ops a commit makes, handed out in order.
struct Ids {
    nodes: std::vec::IntoIter<NodeId>,
    rels: std::vec::IntoIter<crate::id::RelationId>,
}

/// A key at clip-local time `u` in `clip`: when `u` is before the clip's
/// start, the start moves to it and every key shifts to keep its time.
fn insert_key(doc: &crate::document::Document, clip: NodeId, keys: &[crate::time::KeyAt], u: f64, value: Value, ids: &mut Ids, ops: &mut Vec<Op>) {
    let mut t = u;
    if u < 0.0 {
        let start = crate::time::start(doc, clip);
        ops.push(Op::Set { id: clip, path: LeafPath::new(FacetKind::Timing, "start").expect("timing.start"), slot: Slot::Const(Value::Number(start + u)) });
        for k in keys {
            ops.push(Op::Set { id: k.id, path: key_path("t"), slot: Slot::Const(Value::Number(k.t - u)) });
        }
        t = 0.0;
    }
    let order = doc.children(clip).iter().filter_map(|&c| doc.live(c)).map(|n| n.order.clone()).max().map_or_else(OrderKey::first, |o| OrderKey::after(&o));
    let easing = keys.iter().rev().find(|k| k.t <= t).map_or("linear", |k| k.easing.as_str()).to_owned();
    let id = ids.nodes.next().expect("an id per new node");
    ops.push(Op::Create { id, ty: ty("key"), parent: Some(clip), order, facets: vec![key_facet(t, value, &easing)] });
}

/// What committing these previewed leaves writes when they are animated, or
/// when auto-keyframe is on (§12.2): key ops for each leaf it applies to,
/// and those leaves. Every other leaf is an ordinary `Set` (the caller's).
pub fn key_ops(store: &Store, leaves: &[(Leaf, Value)]) -> (Vec<Op>, Vec<Leaf>) {
    let doc = store.document();
    let now = store.time();
    let auto = autokey(store);
    let mut ids = Ids { nodes: doc.next_ids(leaves.len() * 3 + 1).into_iter(), rels: doc.next_relation_ids(leaves.len()).into_iter() };
    let mut timeline: Option<NodeId> = doc.nodes_of_type(ty("timeline")).find(|t| !t.is_session());
    let mut ops = Vec::new();
    let mut handled = Vec::new();
    for (leaf, value) in leaves {
        if leaf.node.is_session() {
            continue;
        }
        // A leaf the gesture carried but did not change (a horizontal drag's
        // y) writes nothing: no key, no animation.
        let same = |a: &Value, b: &Value| match (a, b) {
            (Value::Number(x), Value::Number(y)) => (x - y).abs() < 1e-9,
            (a, b) => a == b,
        };
        let shown = clip_of(doc, *leaf)
            .and_then(|(clip, offset)| crate::time::value_at(&crate::time::keys(doc, clip), now - crate::time::start(doc, clip) - offset))
            .or_else(|| doc.leaf(leaf.node, leaf.path).and_then(|s| s.constant()).cloned());
        if shown.as_ref().is_some_and(|v| same(v, value)) {
            handled.push(*leaf);
            continue;
        }
        match clip_of(doc, *leaf) {
            Some((clip, offset)) => {
                let keys = crate::time::keys(doc, clip);
                let u = now - crate::time::start(doc, clip) - offset;
                if let Some(k) = keys.iter().find(|k| (k.t - u).abs() <= AT_KEY) {
                    // On a key: that key's value.
                    ops.push(Op::Set { id: k.id, path: key_path("value"), slot: Slot::Const(value.clone()) });
                } else if auto {
                    insert_key(doc, clip, &keys, u, value.clone(), &mut ids, &mut ops);
                } else if keys.first().is_none_or(|k| u < k.t) {
                    // Before the clip drives it: the authored value.
                    continue;
                } else {
                    // Between keys or after them, auto-keyframe off: the
                    // whole animation moves by the drag.
                    let Some(was) = crate::time::value_at(&keys, u) else { continue };
                    let moved: Vec<Op> = keys.iter().filter_map(|k| Some(Op::Set { id: k.id, path: key_path("value"), slot: Slot::Const(shifted(&k.value, &was, value)?) })).collect();
                    if moved.len() != keys.len() {
                        continue;
                    }
                    ops.extend(moved);
                }
                handled.push(*leaf);
            }
            None if auto && now > AT_KEY => {
                // Auto-keyframe on an unanimated leaf: an animation from its
                // value now, at 0, to the dragged one at the playhead.
                let Some(before) = doc.leaf(leaf.node, leaf.path).and_then(|s| s.constant()).cloned() else { continue };
                if crate::time::lerp(&before, value, 0.5).is_none() {
                    continue;
                }
                let tl = match timeline {
                    Some(t) => t,
                    None => {
                        let id = ids.nodes.next().expect("an id");
                        let order = doc.roots().iter().filter(|r| !r.is_session()).last().and_then(|&r| doc.live(r)).map_or_else(OrderKey::first, |n| OrderKey::after(&n.order));
                        ops.push(Op::Create { id, ty: ty("timeline"), parent: None, order, facets: vec![named("main")] });
                        timeline = Some(id);
                        id
                    }
                };
                let clip = ids.nodes.next().expect("an id");
                let mut timing = crate::facet::Timing::default();
                timing.start = Slot::Const(Value::Number(0.0));
                let mut c = crate::facet::Clip::default();
                c.prop = Slot::Const(Value::Str(leaf.path.to_string()));
                let label = format!("{} {}", name_of(store, leaf.node).unwrap_or_else(|| doc.live(leaf.node).map_or("node", |n| n.ty.name()).to_owned()), leaf.path.field);
                let order = doc.children(tl).iter().filter_map(|&c| doc.live(c)).map(|n| n.order.clone()).max().map_or_else(OrderKey::first, |o| OrderKey::after(&o));
                ops.push(Op::Create { id: clip, ty: ty("clip"), parent: Some(tl), order, facets: vec![timing.wrap(), c.wrap(), named(&label)] });
                let k0 = OrderKey::first();
                ops.push(Op::Create { id: ids.nodes.next().expect("an id"), ty: ty("key"), parent: Some(clip), order: k0.clone(), facets: vec![key_facet(0.0, before, "ease_in_out")] });
                ops.push(Op::Create { id: ids.nodes.next().expect("an id"), ty: ty("key"), parent: Some(clip), order: OrderKey::after(&k0), facets: vec![key_facet(now, value.clone(), "ease_in_out")] });
                ops.push(Op::Link { rel: Relation { id: ids.rels.next().expect("a relation id"), from: clip, to: leaf.node, rel: RelKind::Animates, data: BTreeMap::new() } });
                handled.push(*leaf);
            }
            None => {}
        }
    }
    (ops, handled)
}

/// The previous (`dir < 0`) or next key of `clip` from the playhead, as a
/// clock time (‹ and ›).
pub fn step_key(store: &Store, clip: NodeId, dir: i32) -> Option<f64> {
    let doc = store.document();
    let offset = doc.relations_from(clip).find(|r| r.rel == RelKind::Animates).and_then(|r| r.data.get("offset").and_then(|v| v.as_f64())).unwrap_or(0.0);
    let base = crate::time::start(doc, clip) + offset;
    let now = store.time();
    let times = crate::time::keys(doc, clip).into_iter().map(|k| base + k.t);
    if dir < 0 { times.filter(|&t| t < now - AT_KEY).fold(None, |a: Option<f64>, t| Some(a.map_or(t, |a| a.max(t)))) } else { times.filter(|&t| t > now + AT_KEY).fold(None, |a: Option<f64>, t| Some(a.map_or(t, |a| a.min(t)))) }
}

/// ◇: remove the key at the playhead, or add one there holding the value
/// shown now (the first target's). Removing the last key removes the clip.
pub fn toggle_key_ops(store: &Store, clip: NodeId) -> Result<Vec<Op>, String> {
    let doc = store.document();
    let rel = doc.relations_from(clip).find(|r| r.rel == RelKind::Animates && doc.is_live(r.to)).ok_or("the clip animates nothing")?;
    let offset = rel.data.get("offset").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let path: LeafPath = doc.get::<crate::facet::Clip>(clip).and_then(|c| c.prop.constant()).and_then(|v| v.as_str()).ok_or("the clip has no prop")?.parse().map_err(|e| format!("{e}"))?;
    let keys = crate::time::keys(doc, clip);
    let u = store.time() - crate::time::start(doc, clip) - offset;
    if let Some(k) = keys.iter().find(|k| (k.t - u).abs() <= AT_KEY) {
        return Ok(vec![Op::Delete { id: if keys.len() <= 1 { clip } else { k.id } }]);
    }
    let value = store.resolved(rel.to, path).ok_or("no value to key")?;
    let mut ids = Ids { nodes: doc.next_ids(1).into_iter(), rels: Vec::new().into_iter() };
    let mut ops = Vec::new();
    insert_key(doc, clip, &keys, u, value, &mut ids, &mut ops);
    Ok(ops)
}

/// A new clip animating `leaf` with `keys` (clock times, values, easing):
/// on the document's timeline, made if there is none.
fn new_clip_ops(store: &Store, leaf: Leaf, keys: &[(f64, Value, &str)]) -> Vec<Op> {
    let doc = store.document();
    let mut ids = doc.next_ids(keys.len() + 2).into_iter();
    let mut ops = Vec::new();
    let tl = match doc.nodes_of_type(ty("timeline")).find(|t| !t.is_session()) {
        Some(t) => t,
        None => {
            let id = ids.next().expect("an id");
            let order = doc.roots().iter().filter(|r| !r.is_session()).last().and_then(|&r| doc.live(r)).map_or_else(OrderKey::first, |n| OrderKey::after(&n.order));
            ops.push(Op::Create { id, ty: ty("timeline"), parent: None, order, facets: vec![named("main")] });
            id
        }
    };
    let clip = ids.next().expect("an id");
    let mut timing = crate::facet::Timing::default();
    timing.start = Slot::Const(Value::Number(0.0));
    let mut c = crate::facet::Clip::default();
    c.prop = Slot::Const(Value::Str(leaf.path.to_string()));
    let label = format!("{} {}", name_of(store, leaf.node).unwrap_or_else(|| doc.live(leaf.node).map_or("node", |n| n.ty.name()).to_owned()), leaf.path.field);
    let order = doc.children(tl).iter().filter_map(|&c| doc.live(c)).map(|n| n.order.clone()).max().map_or_else(OrderKey::first, |o| OrderKey::after(&o));
    ops.push(Op::Create { id: clip, ty: ty("clip"), parent: Some(tl), order, facets: vec![timing.wrap(), c.wrap(), named(&label)] });
    let mut k = OrderKey::first();
    for (t, v, easing) in keys {
        ops.push(Op::Create { id: ids.next().expect("an id"), ty: ty("key"), parent: Some(clip), order: k.clone(), facets: vec![key_facet(*t, v.clone(), easing)] });
        k = OrderKey::after(&k);
    }
    let rel = doc.next_relation_ids(1)[0];
    ops.push(Op::Link { rel: Relation { id: rel, from: clip, to: leaf.node, rel: RelKind::Animates, data: BTreeMap::new() } });
    ops
}

/// The inspector's ◇ on a property (After Effects' stopwatch, timeline.md
/// §12.3): an animated leaf toggles its key at the playhead; an unanimated
/// one starts an animation with a key there holding its value now.
pub fn key_leaf_ops(store: &Store, leaf: Leaf) -> Result<Vec<Op>, String> {
    if let Some((clip, _)) = clip_of(store.document(), leaf) {
        return toggle_key_ops(store, clip);
    }
    let value = store.resolved(leaf.node, leaf.path).ok_or("no value to key")?;
    // Words (a component's state) step: hold keys (machines.md §4.2).
    let steps = matches!(value, Value::Str(_) | Value::Enum(_));
    if !steps && crate::time::lerp(&value, &value, 0.5).is_none() {
        return Err(format!("{} does not animate", leaf.path));
    }
    if store.document().leaf(leaf.node, leaf.path).is_some_and(|s| s.expr().is_some()) {
        return Err(format!("{} is set by a formula; remove the formula to key it", leaf.path));
    }
    Ok(new_clip_ops(store, leaf, &[(store.time(), value, if steps { "hold" } else { "easy_ease" })]))
}

/// "Draw on" (After Effects' Trim Paths recipe): `trim.end` keyed from 0 at
/// the playhead to 1 `secs` later with Easy Ease, adding the trim if the
/// shape has none. One transaction.
pub fn draw_on_ops(store: &Store, node: NodeId, secs: f64) -> Result<Vec<Op>, String> {
    let doc = store.document();
    let spec = doc.live(node).ok_or("no such node")?.ty.spec();
    if !spec.allows(FacetKind::Trim) {
        return Err("only rectangles, ellipses and paths draw on".into());
    }
    let leaf = Leaf { node, path: LeafPath::new(FacetKind::Trim, "end").expect("trim.end") };
    if clip_of(doc, leaf).is_some() {
        return Err("its trim end is already animated; edit those keys".into());
    }
    let mut ops = Vec::new();
    if doc.facet(node, FacetKind::Trim).is_none() {
        ops.push(Op::AddFacet { id: node, facet: FacetKind::Trim.default_facet() });
    }
    let now = store.time();
    ops.extend(new_clip_ops(store, leaf, &[(now, Value::Number(0.0), "easy_ease"), (now + secs.max(0.05), Value::Number(1.0), "easy_ease")]));
    Ok(ops)
}

/// A value set from the inspector on an animated leaf (After Effects: with
/// the stopwatch on, a change keys the playhead): the key there takes it,
/// or a new key is added there. `None` for a leaf no clip animates.
pub fn set_animated_ops(store: &Store, leaf: Leaf, value: Value) -> Option<Vec<Op>> {
    let doc = store.document();
    let (clip, offset) = clip_of(doc, leaf)?;
    let keys = crate::time::keys(doc, clip);
    let u = store.time() - crate::time::start(doc, clip) - offset;
    if let Some(k) = keys.iter().find(|k| (k.t - u).abs() <= AT_KEY) {
        return Some(vec![Op::Set { id: k.id, path: key_path("value"), slot: Slot::Const(value) }]);
    }
    let mut ids = Ids { nodes: doc.next_ids(1).into_iter(), rels: Vec::new().into_iter() };
    let mut ops = Vec::new();
    insert_key(doc, clip, &keys, u, value, &mut ids, &mut ops);
    Some(ops)
}
