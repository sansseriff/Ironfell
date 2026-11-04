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
}

/// A clip on one of its targets, shifted by the relation's stagger offset,
/// with its keys in time order.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub clip: NodeId,
    pub offset: f64,
    pub label: String,
    pub keys: Vec<NodeId>,
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
            let segment = Segment { clip, offset, label: label.clone(), keys: keys.iter().map(|k| k.id).collect() };
            match o.props.iter_mut().find(|p| p.prop == prop) {
                Some(p) => p.segments.push(segment),
                None => o.props.push(Prop { prop: prop.clone(), segments: vec![segment] }),
            }
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
    Plan { clock, objects, end, tick: tick_for(pps), errors }
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
}

/// The rows, top to bottom, in document px from the composition's top.
pub fn rows(plan: &Plan) -> Vec<Row> {
    let mut out = Vec::new();
    let mut y = RULER_H + 4.0;
    for o in &plan.objects {
        let error = o.props.iter().flat_map(|p| &p.segments).any(|s| plan.errors.contains(&s.clip));
        out.push(Row { kind: "object", label: o.label.clone(), y, target: o.target, prop: None, clip: None, error });
        y += ROW_H;
        for p in &o.props {
            let short = p.prop.rsplit('.').next().unwrap_or(&p.prop).to_owned();
            let error = p.segments.iter().any(|s| plan.errors.contains(&s.clip));
            out.push(Row { kind: "property", label: short, y, target: o.target, prop: Some(p.prop.clone()), clip: p.segments.first().map(|s| s.clip), error });
            y += ROW_H;
        }
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

const BG: [f32; 4] = [0.145, 0.145, 0.152, 1.0];
const HEADER: [f32; 4] = [0.17, 0.17, 0.18, 1.0];
const RULER: [f32; 4] = [0.2, 0.2, 0.21, 1.0];
const GRID: [f32; 4] = [1.0, 1.0, 1.0, 0.06];
const TICK: [f32; 4] = [0.55, 0.55, 0.58, 1.0];
const LABEL: [f32; 4] = [0.85, 0.85, 0.88, 1.0];
const DIM: [f32; 4] = [0.6, 0.6, 0.64, 1.0];
const SEGMENT: [f32; 4] = [0.3, 0.55, 0.95, 1.0];
const ERROR: [f32; 4] = [0.9, 0.3, 0.3, 1.0];
const PLAYHEAD: [f32; 4] = [1.0, 0.6, 0.2, 1.0];
const LOOP: [f32; 4] = [1.0, 0.6, 0.2, 0.22];
const SUMMARY: [f32; 4] = [0.45, 0.47, 0.52, 0.55];
const KEY: [f32; 4] = [0.92, 0.93, 0.96, 1.0];
/// A key's diamond: a square this wide, turned 45°.
const KEY_PX: f64 = 10.0;

/// Every node the plan draws: the role groups, bottom to top, each with its
/// pieces in drawing order.
fn wants(plan: &Plan, comp: NodeId) -> Vec<Want> {
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
    let mut by: BTreeMap<&'static str, Vec<Want>> = BTreeMap::new();
    let mut push = |group: &'static str, key: String, ty_name: &str, facets: Vec<Facet>, projects: Option<NodeId>| {
        let mut facets = facets;
        facets.push(named(&key));
        by.entry(group).or_default().push(Want { key, ty: ty(ty_name), facets, projects, parent: Some(group.to_owned()) });
    };
    let width = at(&lit(plan.end));

    // Backdrop and grid.
    push("backdrop", "background".into(), "rect", vec![place("0", "0"), sized(&lit(FAR), &h), paint(BG)], None);
    let ticks = (plan.end / plan.tick).round() as usize;
    for k in 0..=ticks {
        let x = at(&lit(k as f64 * plan.tick));
        push("backdrop", format!("grid {k}"), "rect", vec![place(&x, &lit(RULER_H)), sized("1", &lit(FAR - RULER_H)), paint(GRID)], None);
    }

    // The ruler: a track laying the clock's time along it (std:timeline-
    // ruler), so a press anywhere on it moves the playhead there.
    push("ruler", "ruler bar".into(), "rect", vec![place(&lw, "0"), sized(&format!("({}) - {lw}", width), &lit(RULER_H)), paint(RULER)], None);
    push("ruler", "loop".into(), "rect", vec![place(&at(&format!("{clock}.clock.loop_in")), "0"), sized(&format!("max({clock}.clock.loop_out - {clock}.clock.loop_in, 0) * {pps}"), &lit(RULER_H)), paint(LOOP)], None);
    const MINOR: usize = 5;
    for k in 0..=ticks * MINOR {
        let t = k as f64 * plan.tick / MINOR as f64;
        let x = at(&lit(t));
        let major = k % MINOR == 0;
        let len = if major { 14.0 } else { 6.0 };
        push("ruler", format!("tick {k}"), "rect", vec![place(&x, &lit(RULER_H - len)), sized("1", &lit(len)), paint(if major { TICK } else { DIM })], None);
        if major {
            let text = if plan.tick < 1.0 { format!("{t:.1}s") } else { format!("{t}s") };
            let mut f = vec![place(&format!("{x} + 4"), "8")];
            f.extend(words(&text, LABEL_PX, 500.0));
            f.push(paint(LABEL));
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
            push("backdrop", format!("summary {}", o.target), "rect", vec![place(&at(&lo), &ry(row.y + 5.0)), sized(&format!("max(({hi}) - ({lo}), 0) * {pps}"), &lit(ROW_H - 10.0)), rounded(3.0), paint(SUMMARY)], None);
        }
        for p in &o.props {
            let row = ri.next().expect("a property row");
            for s in &p.segments {
                let colour = if plan.errors.contains(&s.clip) { ERROR } else { SEGMENT };
                let times: Vec<String> = s.keys.iter().map(|&k| format!("{k}.key.t")).collect();
                let (lo, hi) = (fold("min", &times), fold("max", &times));
                // The clip's bar: from its first key to its last. A drag
                // retimes the clip (it projects the clip, so its start is
                // the leaf the drag moves); Delete deletes the clip.
                let x = at(&format!("{}.timing.start + {} + {lo}", s.clip, lit(s.offset)));
                push("tracks", format!("bar {} {}", s.clip, o.target), "rect", vec![place(&x, &ry(row.y + 3.0)), sized(&format!("max(({hi}) - ({lo}), 0) * {pps}"), &lit(SEG_H)), rounded(4.0), paint(colour)], Some(s.clip));
                let mut f = vec![place(&format!("{x} + 8"), &ry(row.y + 4.0))];
                f.extend(words(&s.label, 11.0, 500.0));
                f.push(paint([1.0, 1.0, 1.0, 0.9]));
                push("tracks", format!("bar label {} {}", s.clip, o.target), "text", f, None);
                // A diamond per key: a drag moves the key in time (it
                // projects the key), Delete deletes it.
                let side = KEY_PX / std::f64::consts::SQRT_2;
                for (i, &k) in s.keys.iter().enumerate() {
                    let mut t = Transform2d::default();
                    t.x = bound(&at(&key_x(s, k)));
                    t.y = bound(&ry(row.y + (ROW_H - KEY_PX * std::f64::consts::SQRT_2) / 2.0));
                    t.rot = bound(&lit(std::f64::consts::FRAC_PI_4));
                    let mut st = Stroke::default();
                    st.color = Slot::Const(Value::Color([0.1, 0.12, 0.16, 1.0]));
                    st.width = Slot::Const(Value::Number(1.0));
                    let mut facets = vec![t.wrap(), sized(&lit(side * std::f64::consts::SQRT_2), &lit(side * std::f64::consts::SQRT_2)), paint(KEY), st.wrap()];
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
    push("headers", "header".into(), "rect", vec![place("0", "0"), sized(&lw, &h), paint(HEADER)], None);
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
            facets.push(track.wrap());
        }
        out.push(Want { key: group.into(), ty: ty("group"), facets, projects: None, parent: None });
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
                    ops.push(Op::Link { rel: Relation { id: rel, from: id, to, rel: RelKind::Projects, data: BTreeMap::new() } });
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
    let clock = store.ensure_clock().map_err(|e| e.message)?;
    let comp = find(store);
    let plan = plan(store, clock, comp);
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
    let (t0, pps, lw) = (read("t0", 0.0), read("pps", 100.0), read("label_w", 160.0));
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
    let last = store.clock().map(|c| rows(&plan(store, c, Some(comp))).last().map_or(0.0, |r| r.y - RULER_H - 4.0)).unwrap_or(0.0);
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
    let doc = store.document();
    let end = doc.nodes_of_type(ty("clip")).filter(|c| !c.is_session()).map(|c| crate::time::start(doc, c) + crate::time::length(&crate::time::keys(doc, c))).fold(0.0, f64::max);
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
