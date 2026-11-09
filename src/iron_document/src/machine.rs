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
use crate::facet::{Args, Channel, LeafPath, Name, Transition, Tween};
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

fn text(slot: &crate::value::Slot) -> Option<String> {
    slot.constant().and_then(|v| v.as_str()).map(str::to_owned)
}

fn number(slot: &crate::value::Slot) -> Option<f64> {
    slot.constant().and_then(Value::as_f64)
}

fn children(doc: &Document, n: NodeId, ty: &str) -> Vec<NodeId> {
    doc.children(n).iter().copied().filter(|&c| !c.is_expanded() && ty_of(doc, c) == Some(ty)).collect()
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

/// The pose a machine gives `state`: a value per channel, the channel's
/// rest value where the state says nothing.
pub fn pose(doc: &Document, def: NodeId, machine: NodeId, state: &str) -> Option<BTreeMap<NodeId, f64>> {
    let s = children(doc, machine, "state").into_iter().find(|&s| name_of(doc, s).as_deref() == Some(state))?;
    let args = doc.get::<Args>(s);
    Some(
        channels(doc, def)
            .into_iter()
            .map(|(c, name, rest)| (c, args.and_then(|a| a.slots.get(name.as_str())).and_then(number).unwrap_or(rest)))
            .collect(),
    )
}

/// A transition's plan: its length, curve, and each tweened channel's
/// (start, end) as fractions of the transition and curve.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub transition: NodeId,
    pub ms: f64,
    pub curve: Curve,
    pub tweens: BTreeMap<NodeId, (f64, f64, Curve)>,
}

/// The transition `machine` runs from `from` to `to`: an exact match first,
/// then `*` on either side. `None` snaps.
pub fn plan(doc: &Document, def: NodeId, machine: NodeId, from: &str, to: &str) -> Option<Plan> {
    let ts: Vec<(NodeId, Transition)> = children(doc, machine, "transition").into_iter().filter_map(|t| Some((t, doc.get::<Transition>(t)?.clone()))).collect();
    let fits = |t: &Transition, exact_from: bool, exact_to: bool| {
        let (f, g) = (text(&t.from).unwrap_or_else(|| "*".into()), text(&t.to).unwrap_or_else(|| "*".into()));
        (if exact_from { f == from } else { f == "*" || f == from }) && (if exact_to { g == to } else { g == "*" || g == to })
    };
    let (id, t) = [(true, true), (true, false), (false, true), (false, false)].iter().find_map(|&(a, b)| ts.iter().find(|(_, t)| fits(t, a, b)))?;
    let ms = number(&t.ms).unwrap_or(300.0).max(0.0);
    // One default with the facets' (Easy Ease), for an unknown name too.
    let easy = curve("easy_ease").expect("a named curve");
    let curve = text(&t.ease).and_then(|e| curve(&e).ok()).unwrap_or(easy);
    let by_name: BTreeMap<String, NodeId> = channels(doc, def).into_iter().map(|(c, n, _)| (n, c)).collect();
    let mut tweens = BTreeMap::new();
    for w in children(doc, *id, "tween") {
        let Some(tw) = doc.get::<Tween>(w) else { continue };
        let Some(&c) = text(&tw.channel).and_then(|n| by_name.get(&n)) else { continue };
        let (at, len) = (number(&tw.at).unwrap_or(0.0), number(&tw.ms).unwrap_or(ms));
        let e = text(&tw.ease).and_then(|e| crate::time::curve(&e).ok()).unwrap_or(easy);
        // A tween ends with its transition at the latest (one running past
        // it would jump to its end).
        if ms > 0.0 {
            tweens.insert(c, ((at / ms).clamp(0.0, 1.0), ((at + len) / ms).clamp(0.0, 1.0), e));
        }
    }
    Some(Plan { transition: *id, ms, curve, tweens })
}

impl Plan {
    /// How far channel `c` has gone, 0 → 1, at transition progress `p`.
    pub fn shaped(&self, c: NodeId, p: f64) -> f64 {
        match self.tweens.get(&c) {
            Some(&(a, b, e)) => e.at(if b > a { (p - a) / (b - a) } else if p >= b { 1.0 } else { 0.0 }),
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
                let Some(to) = pose(doc, def, m, &state) else { continue };
                seen.insert((e, m));
                match self.known.get_mut(&(e, m)) {
                    None => {
                        self.known.insert((e, m), Known { state, values: to, run: None });
                    }
                    Some(k) if k.state != state => {
                        let from_state = std::mem::replace(&mut k.state, state.clone());
                        match plan(doc, def, m, &from_state, &state).filter(|p| p.ms > 0.0 && !snap.contains(&e)) {
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
