//! Component state machines, first slice: driven by events
//! (plans/specs/machines.md phase 1).
//!
//! A definition may hold `channel`s (numbers its template binds to, copied
//! onto each instance like ports) and `machine`s, each named for the `in`
//! port whose value is its state. A machine's `state`s give a pose (a value
//! per channel, as args); its `transition`s say how to go from one state to
//! another: a length in ms and an easing, and `tween`s that give one channel
//! its own slice and curve.
//!
//! The player watches every linked instance's state arguments. When one
//! changes it runs the transition: progress goes 0 → 1 over `ms` on the
//! host's clock, and each channel is `from + (to − from) · ease(curve,
//! window(progress, at/ms, (at+ms)/ms))`. An interrupted transition starts
//! from where the channels are. A change with no transition, an instance
//! seen for the first time, and an undo all snap to the pose. Channel values
//! are published (`ProducerId::MACHINES`) only where they differ from the
//! channel's rest value, so a resting instance is static to the renderer.
//! Nothing here is saved or undone: it is presentation state.

use crate::document::Document;
use crate::expr::Leaf;
use crate::facet::{Args, Channel, LeafPath, Name};
use crate::id::NodeId;
use crate::time::{curve, Curve};
use crate::value::Value;
use std::collections::BTreeMap;

fn ty_of(doc: &Document, n: NodeId) -> Option<&'static str> {
    doc.live(n).map(|x| x.ty.name())
}

fn name_of(doc: &Document, n: NodeId) -> Option<String> {
    doc.get::<Name>(n).and_then(|x| x.text.constant()).and_then(|v| v.as_str()).map(str::to_owned)
}

fn number(slot: &crate::value::Slot) -> Option<f64> {
    slot.constant().and_then(Value::as_f64)
}

/// A node's children of a type: authored ones under an authored node, the
/// instance's copies under a copy.
fn children(doc: &Document, n: NodeId, ty: &str) -> Vec<NodeId> {
    doc.children(n).iter().copied().filter(|&c| c.is_expanded() == n.is_expanded() && ty_of(doc, c) == Some(ty)).collect()
}

/// What the player reads: resolved leaves (constants, bindings and an
/// instance's overrides alike).
type Read<'a> = &'a dyn Fn(Leaf) -> Option<Value>;

fn leaf(node: NodeId, path: &str) -> Leaf {
    Leaf { node, path: path.parse().expect("declared leaf") }
}

fn read_text(read: Read, node: NodeId, path: &str) -> Option<String> {
    read(leaf(node, path)).and_then(|v| v.as_str().map(str::to_owned))
}

fn read_num(read: Read, node: NodeId, path: &str) -> Option<f64> {
    read(leaf(node, path)).and_then(|v| v.as_f64())
}

/// A definition's channels: (node, name, rest value).
pub fn channels(doc: &Document, def: NodeId) -> Vec<(NodeId, String, f64)> {
    children(doc, def, "channel")
        .into_iter()
        .filter_map(|c| Some((c, name_of(doc, c)?, doc.get::<Channel>(c).and_then(|x| number(&x.value)).unwrap_or(0.0))))
        .collect()
}

/// A definition's machines: (node, the port name they follow).
pub fn machines(doc: &Document, def: NodeId) -> Vec<(NodeId, String)> {
    children(doc, def, "machine").into_iter().filter_map(|m| Some((m, name_of(doc, m)?))).collect()
}

/// The machine an instance runs: its copy of `m` (machines.md §9 Q12), or
/// `m` itself (a main, or before expansion).
pub fn copy_of(doc: &Document, entity: Option<NodeId>, m: NodeId) -> NodeId {
    entity.and_then(|e| NodeId::expanded(e, m)).filter(|&c| doc.is_live(c)).unwrap_or(m)
}

/// The pose machine `machine` (a definition's or an instance's copy) gives
/// `state`: a value per channel of `def`, the channel's rest value where
/// the state says nothing.
pub fn pose(doc: &Document, read: Read, def: NodeId, machine: NodeId, state: &str) -> Option<BTreeMap<NodeId, f64>> {
    let s = children(doc, machine, "state").into_iter().find(|&s| name_of(doc, s).as_deref() == Some(state))?;
    let has = doc.get::<Args>(s).map(|a| a.slots.keys().map(|k| k.to_string()).collect::<Vec<_>>()).unwrap_or_default();
    Some(
        channels(doc, def)
            .into_iter()
            .map(|(c, name, rest)| (c, if has.contains(&name) { read_num(read, s, &format!("args.{name}")).unwrap_or(rest) } else { rest }))
            .collect(),
    )
}

/// How one channel moves through a transition: a window of its progress
/// with an easing, or a function of progress (`tween.shape`).
#[derive(Clone, Debug)]
pub enum Shape {
    Window(f64, f64, Curve),
    Fn(std::sync::Arc<crate::expr::Closure>),
}

impl PartialEq for Shape {
    fn eq(&self, o: &Shape) -> bool {
        match (self, o) {
            (Shape::Window(a, b, c), Shape::Window(x, y, z)) => a == x && b == y && c == z,
            (Shape::Fn(a), Shape::Fn(b)) => std::sync::Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// A transition's plan: its length, curve, and each tweened channel's
/// shape.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub transition: NodeId,
    pub ms: f64,
    pub curve: Curve,
    pub tweens: BTreeMap<NodeId, Shape>,
}

/// The transition `machine` runs from `from` to `to`: an exact match first,
/// then `*` on either side. `None` snaps.
pub fn plan(doc: &Document, read: Read, def: NodeId, machine: NodeId, from: &str, to: &str) -> Option<Plan> {
    let ts: Vec<(NodeId, String, String)> = children(doc, machine, "transition")
        .into_iter()
        .map(|t| (t, read_text(read, t, "transition.from").unwrap_or_else(|| "*".into()), read_text(read, t, "transition.to").unwrap_or_else(|| "*".into())))
        .collect();
    let fits = |f: &str, g: &str, exact_from: bool, exact_to: bool| (if exact_from { f == from } else { f == "*" || f == from }) && (if exact_to { g == to } else { g == "*" || g == to });
    let &(id, _, _) = [(true, true), (true, false), (false, true), (false, false)].iter().find_map(|&(a, b)| ts.iter().find(|(_, f, g)| fits(f, g, a, b)))?;
    let ms = read_num(read, id, "transition.ms").unwrap_or(300.0).max(0.0);
    // One default with the facets' (Easy Ease), for an unknown name too.
    let easy = curve("easy_ease").expect("a named curve");
    let ease = |n: NodeId, p: &str| read_text(read, n, p).and_then(|e| curve(&e).ok()).unwrap_or(easy);
    let by_name: BTreeMap<String, NodeId> = channels(doc, def).into_iter().map(|(c, n, _)| (n, c)).collect();
    let mut tweens = BTreeMap::new();
    for w in children(doc, id, "tween") {
        let Some(&c) = read_text(read, w, "tween.channel").and_then(|n| by_name.get(&n)) else { continue };
        let shape = match read(leaf(w, "tween.shape")) {
            Some(Value::Fn(f)) => Shape::Fn(f),
            _ => {
                let (a, b) = (read_num(read, w, "tween.start").unwrap_or(0.0).clamp(0.0, 1.0), read_num(read, w, "tween.end").unwrap_or(1.0).clamp(0.0, 1.0));
                Shape::Window(a, b, ease(w, "tween.ease"))
            }
        };
        tweens.insert(c, shape);
    }
    Some(Plan { transition: id, ms, curve: ease(id, "transition.ease"), tweens })
}

impl Plan {
    /// How far channel `c` has gone, 0 → 1, at transition progress `p`.
    pub fn shaped(&self, c: NodeId, p: f64) -> f64 {
        match self.tweens.get(&c) {
            Some(Shape::Window(a, b, e)) => e.at(if b > a { (p - a) / (b - a) } else if p >= *b { 1.0 } else { 0.0 }),
            Some(Shape::Fn(f)) => f.apply(&[Value::Number(p)]).ok().and_then(|v| v.as_f64()).unwrap_or(if p >= 1.0 { 1.0 } else { 0.0 }),
            None => self.curve.at(p),
        }
    }
}

/// One machine on one instance.
#[derive(Clone, Debug)]
struct Known {
    state: String,
    values: BTreeMap<NodeId, f64>,
    run: Option<Run>,
}

#[derive(Clone, Debug)]
struct Run {
    plan: Plan,
    start: f64,
    from: BTreeMap<NodeId, f64>,
    to: BTreeMap<NodeId, f64>,
}

/// The player: what each instance's machines were last seen in, and what
/// is running.
#[derive(Default)]
pub struct Player {
    known: BTreeMap<(NodeId, NodeId), Known>,
    /// Instances whose next state change snaps instead of playing: those
    /// an undo or redo set the state of.
    pub snap: std::collections::BTreeSet<NodeId>,
}

/// What a running transition reports, for a probe or a view.
#[derive(Clone, Debug, PartialEq)]
pub struct Running {
    pub entity: NodeId,
    pub transition: NodeId,
    pub progress: f64,
}

impl Player {
    /// Advance to `now` (seconds on the host's clock). Answers the channel
    /// values to publish (only those away from rest), and what is running.
    pub fn advance(&mut self, doc: &Document, read: &dyn Fn(Leaf) -> Option<Value>, now: f64) -> (BTreeMap<Leaf, Value>, Vec<Running>) {
        let snap = std::mem::take(&mut self.snap);
        let mut seen = std::collections::BTreeSet::new();
        let entity_ty = crate::registry::TypeId::lookup("entity").expect("registered");
        for e in doc.nodes_of_type(entity_ty).filter(|&e| doc.is_live(e)).collect::<Vec<_>>() {
            let Some(def) = crate::expand::linked_definition(doc, e) else { continue };
            for (m, port) in machines(doc, def) {
                let Ok(path) = LeafPath::new(crate::facet::FacetKind::Args, &port) else { continue };
                let Some(state) = read(Leaf { node: e, path }).and_then(|v| v.as_str().map(str::to_owned)) else { continue };
                let mc = copy_of(doc, Some(e), m);
                let Some(to) = pose(doc, read, def, mc, &state) else { continue };
                seen.insert((e, m));
                match self.known.get_mut(&(e, m)) {
                    None => {
                        self.known.insert((e, m), Known { state, values: to, run: None });
                    }
                    Some(k) if k.state != state => {
                        let from_state = std::mem::replace(&mut k.state, state.clone());
                        match plan(doc, read, def, mc, &from_state, &state).filter(|p| p.ms > 0.0 && !snap.contains(&e)) {
                            Some(plan) => k.run = Some(Run { plan, start: now, from: k.values.clone(), to }),
                            None => {
                                k.values = to;
                                k.run = None;
                            }
                        }
                    }
                    // The same state: a pose edited in the definition shows at
                    // once when nothing is running.
                    Some(k) if k.run.is_none() => k.values = to,
                    Some(_) => {}
                }
            }
        }
        self.known.retain(|key, _| seen.contains(key));
        let mut running = Vec::new();
        let mut out = BTreeMap::new();
        let value_path: LeafPath = "channel.value".parse().expect("declared leaf");
        for (&(e, _), k) in self.known.iter_mut() {
            if let Some(run) = &k.run {
                let p = ((now - run.start) * 1000.0 / run.plan.ms).clamp(0.0, 1.0);
                for (&c, &to) in &run.to {
                    let from = run.from.get(&c).copied().unwrap_or(to);
                    k.values.insert(c, from + (to - from) * run.plan.shaped(c, p));
                }
                if p >= 1.0 {
                    k.values = run.to.clone();
                    k.run = None;
                } else {
                    running.push(Running { entity: e, transition: run.plan.transition, progress: p });
                }
            }
            for (&c, &v) in &k.values {
                let rest = doc.get::<Channel>(c).and_then(|x| number(&x.value)).unwrap_or(0.0);
                if (v - rest).abs() > 1e-12
                    && let Some(copy) = NodeId::expanded(e, c)
                {
                    out.insert(Leaf { node: copy, path: value_path }, Value::Number(v));
                }
            }
        }
        (out, running)
    }
}
