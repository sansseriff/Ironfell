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
use crate::expr::Expr;
use crate::facet::{Axis, Composition, Corners, Facet, FacetKind, Fill, Font, LeafPath, LeafStruct, Name, Size, Text, Transform2d};
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

/// The ruler's height and a row's, in document px.
pub const RULER_H: f64 = 24.0;
pub const ROW_H: f64 = 22.0;
const SEG_H: f64 = 16.0;
/// How far the background, header and lines reach, in px.
const FAR: f64 = 8000.0;
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

/// A clip on one of its targets, shifted by the relation's stagger offset.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub clip: NodeId,
    pub offset: f64,
    pub label: String,
    /// Whether the clip's ends are numbers, shown (and typed) as readouts
    /// at the segment's ends.
    pub numeric: bool,
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
        let start = number(store, clip, FacetKind::Timing, "start").unwrap_or(0.0);
        let dur = number(store, clip, FacetKind::Timing, "dur").unwrap_or(0.0);
        let label = name_of(store, clip).unwrap_or_else(|| "clip".into());
        for r in doc.relations_from(clip).filter(|r| r.rel == RelKind::Animates && doc.is_live(r.to)) {
            let offset = r.data.get("offset").and_then(|v| v.as_f64()).unwrap_or(0.0);
            end = end.max(start + offset + dur);
            let i = match objects.iter().position(|o| o.target == r.to) {
                Some(i) => i,
                None => {
                    objects.push(Object { target: r.to, label: label_of(store, r.to), props: Vec::new() });
                    objects.len() - 1
                }
            };
            let o = &mut objects[i];
            let numeric = doc.get::<crate::facet::Clip>(clip).is_some_and(|c| [&c.from, &c.to].iter().all(|s| s.constant().is_some_and(|v| matches!(v.kind(), crate::value::ValueKind::Number | crate::value::ValueKind::Quantity))));
            let segment = Segment { clip, offset, label: label.clone(), numeric };
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

/// One node the timeline should have.
struct Want {
    key: String,
    ty: TypeId,
    facets: Vec<Facet>,
    /// A `projects` relation to what it stands for.
    projects: Option<NodeId>,
}

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
const VALUE: [f32; 4] = [0.75, 0.82, 1.0, 1.0];

/// Every node the plan draws, bottom to top.
fn wants(plan: &Plan, comp: NodeId) -> Vec<Want> {
    let ax = |f: &str| format!("{comp}.axis.{f}");
    let (lw, t0, pps) = (ax("label_w"), ax("t0"), ax("pps"));
    // Where time `e` (an expression) is, in px.
    let at = |e: &str| format!("{lw} + ({e} - {t0}) * {pps}");
    // The static pieces reach well past any panel, so the view reads as
    // one surface however it is sized or scrolled.
    let height = FAR;
    let h = lit(height);
    let mut out = Vec::new();
    let mut push = |key: String, ty_name: &str, facets: Vec<Facet>, projects: Option<NodeId>| {
        let mut facets = facets;
        facets.push(named(&key));
        out.push(Want { key, ty: ty(ty_name), facets, projects });
    };
    let width = at(&lit(plan.end));
    push("background".into(), "rect", vec![place("0", "0"), sized(&lit(FAR), &h), paint(BG)], None);
    push("ruler".into(), "rect", vec![place(&lw, "0"), sized(&format!("({}) - {lw}", width), &lit(RULER_H)), paint(RULER)], None);
    // The loop range, a band on the ruler: no width when there is no loop.
    let clock = plan.clock;
    push("loop".into(), "rect", vec![place(&at(&format!("{clock}.clock.loop_in")), "0"), sized(&format!("max({clock}.clock.loop_out - {clock}.clock.loop_in, 0) * {pps}"), &lit(RULER_H)), paint(LOOP)], None);
    let ticks = (plan.end / plan.tick).round() as usize;
    for k in 0..=ticks {
        let t = k as f64 * plan.tick;
        let x = at(&lit(t));
        push(format!("grid {k}"), "rect", vec![place(&x, &lit(RULER_H)), sized("1", &lit(height - RULER_H)), paint(GRID)], None);
        push(format!("tick {k}"), "rect", vec![place(&x, &lit(RULER_H - 6.0)), sized("1", "6"), paint(TICK)], None);
        let text = if plan.tick < 1.0 { format!("{t:.1}s") } else { format!("{t}s") };
        let mut f = vec![place(&format!("{x} + 3"), "4")];
        f.extend(words(&text, 10.0, 400.0));
        f.push(paint(DIM));
        push(format!("tick label {k}"), "text", f, None);
    }
    let mut y = RULER_H + 4.0;
    let mut labels = Vec::new();
    for o in &plan.objects {
        // A clip in error marks its object's row (the reason is in the
        // clip's probe).
        let bad = o.props.iter().flat_map(|p| &p.segments).any(|s| plan.errors.contains(&s.clip));
        let (text, colour) = if bad { (format!("⚠ {}", o.label), ERROR) } else { (o.label.clone(), LABEL) };
        labels.push((format!("object {}", o.target), 8.0, y, text, 12.0, 600.0, colour));
        y += ROW_H;
        for p in &o.props {
            let short = p.prop.rsplit('.').next().unwrap_or(&p.prop).to_owned();
            labels.push((format!("property {} {}", o.target, p.prop), 22.0, y, short, 11.0, 400.0, DIM));
            for s in &p.segments {
                let start = if s.offset == 0.0 { format!("{}.timing.start", s.clip) } else { format!("{}.timing.start + {}", s.clip, lit(s.offset)) };
                let x = at(&start);
                let colour = if plan.errors.contains(&s.clip) { ERROR } else { SEGMENT };
                let key = format!("segment {} {}", s.clip, o.target);
                push(key.clone(), "rect", vec![place(&x, &lit(y + 3.0)), sized(&format!("{}.timing.dur * {pps}", s.clip), &lit(SEG_H)), rounded(4.0), paint(colour)], Some(s.clip));
                let mut f = vec![place(&format!("{x} + 6"), &lit(y + 4.0))];
                f.extend(words(&s.label, 11.0, 500.0));
                f.push(paint([1.0, 1.0, 1.0, 0.95]));
                push(format!("segment label {} {}", s.clip, o.target), "text", f, Some(s.clip));
                // The ends' values, as readouts that invert: typing in one
                // edits the clip's `from` or `to` (timeline.md §5).
                if s.numeric {
                    let end_x = at(&format!("{start} + {}.timing.dur", s.clip));
                    for (end, x, align) in [("from", format!("{x} - 64"), "right"), ("to", format!("{end_x} + 4"), "left")] {
                        let mut t = Text::default();
                        t.content = bound(&format!("fmt({}.clip.{end}, 0)", s.clip));
                        t.sizing = Slot::Const(Value::Enum("fixed".into()));
                        let mut font = Font::default();
                        font.size = Slot::Const(Value::Number(10.0));
                        font.align = Slot::Const(Value::Enum(align.into()));
                        font.tabular = Slot::Const(Value::Bool(true));
                        let mut affords = crate::facet::Affords::default();
                        affords.style = Slot::Const(Value::Str("edit: text".into()));
                        let f = vec![place(&x, &lit(y + 5.0)), t.wrap(), font.wrap(), sized("60", "14"), paint(VALUE), affords.wrap()];
                        push(format!("{end} {} {}", s.clip, o.target), "text", f, Some(s.clip));
                    }
                }
            }
            y += ROW_H;
        }
    }
    push("header".into(), "rect", vec![place("0", "0"), sized(&lw, &h), paint(HEADER)], None);
    for (key, x, y, text, size, weight, colour) in labels {
        let mut f = vec![place(&lit(x), &lit(y + 3.0))];
        f.extend(words(&text, size, weight));
        f.push(paint(colour));
        push(key, "text", f, None);
    }
    let head = at(&format!("{}.clock.t", plan.clock));
    push("playhead".into(), "rect", vec![place(&format!("{head} - 1"), "0"), sized("2", &h), paint(PLAYHEAD)], None);
    push("playhead grip".into(), "rect", vec![place(&format!("{head} - 6"), "2"), sized("12", &lit(RULER_H - 6.0)), rounded(3.0), paint(PLAYHEAD)], None);
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
    let mut have: BTreeMap<String, NodeId> = BTreeMap::new();
    if existing.is_some() {
        for &k in doc.children(comp) {
            if let Some(key) = name_of(store, k) {
                have.insert(key, k);
            }
        }
    }
    let fresh = wants.iter().filter(|w| !have.get(&w.key).is_some_and(|&id| doc.live(id).is_some_and(|n| n.ty == w.ty))).count();
    let mut ids = doc.next_session_ids(fresh + ops.len()).into_iter().skip(ops.len());
    let links: Vec<_> = wants.iter().filter(|w| w.projects.is_some()).collect();
    let mut rel_ids = doc.next_session_relation_ids(links.len()).into_iter();
    let wanted: BTreeSet<&str> = wants.iter().map(|w| w.key.as_str()).collect();
    for (key, &id) in &have {
        let keep = wanted.contains(key.as_str()) && wants.iter().any(|w| &w.key == key && doc.live(id).is_some_and(|n| n.ty == w.ty));
        if !keep {
            ops.push(Op::Delete { id });
        }
    }
    let mut order = OrderKey::first();
    for w in wants {
        let kept = have.get(&w.key).copied().filter(|&id| doc.live(id).is_some_and(|n| n.ty == w.ty));
        match kept {
            Some(id) => {
                if doc.live(id).is_some_and(|n| n.order != order) {
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
                ops.push(Op::Create { id, ty: w.ty, parent: Some(comp), order: order.clone(), facets: w.facets });
                if let Some(to) = w.projects {
                    let rel = rel_ids.next().expect("a relation id per link");
                    ops.push(Op::Link { rel: Relation { id: rel, from: id, to, rel: RelKind::Projects, data: BTreeMap::new() } });
                }
            }
        }
        order = OrderKey::after(&order);
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
            Op::Create { id, .. } | Op::Delete { id } | Op::Set { id, .. } | Op::AddFacet { id, .. } | Op::Reorder { id, .. } => touched.push(*id),
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

/// Fit every clip into `width` document px of the view (`F`): the axis
/// starts at 0, and the last clip's end sits a little short of the edge.
pub fn fit(store: &mut Store, width: f64) -> Result<(), String> {
    let Some(comp) = find(store) else { return Ok(()) };
    let lw = number(store, comp, FacetKind::Axis, "label_w").unwrap_or(160.0);
    let doc = store.document();
    let field = |id: NodeId, f: &str| number(store, id, FacetKind::Timing, f).unwrap_or(0.0);
    let end = doc.nodes_of_type(ty("clip")).filter(|c| !c.is_session()).map(|c| field(c, "start") + field(c, "dur")).fold(0.0, f64::max);
    let pps = (width - lw - 24.0).max(40.0) / end.max(1.0);
    let (_, ops) = axis_ops(store, 0.0, pps)?;
    store.apply_session(ops).map_err(|e| e.message)
}
