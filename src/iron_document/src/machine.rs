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

/// The definition's node an instance's copy stands for (itself when
/// authored).
pub fn copy_source(n: NodeId) -> NodeId {
    n.expanded_parts().map_or(n, |(_, local)| local)
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
    /// Where it was last driven to (`machine.at`), and whether it was
    /// going down the states then.
    at: Option<f64>,
    down: bool,
}

#[derive(Clone, Debug)]
struct Run {
    plan: Plan,
    start: f64,
    from: BTreeMap<NodeId, f64>,
    to: BTreeMap<NodeId, f64>,
    /// The states it goes between.
    states: (String, String),
}

/// The player: what each instance's machines were last seen in, and what
/// is running.
#[derive(Default)]
pub struct Player {
    known: BTreeMap<(NodeId, NodeId), Known>,
    /// Instances whose next state change snaps instead of playing: those
    /// an undo or redo set the state of.
    pub snap: std::collections::BTreeSet<NodeId>,
    /// A state pinned for editing on its main (session only).
    pub pinned: Option<Pinned>,
    /// What was running at the last advance, for views.
    pub running: Vec<Running>,
}

/// What a running transition reports, for a probe or a view.
#[derive(Clone, Debug, PartialEq)]
pub struct Running {
    pub entity: NodeId,
    pub transition: NodeId,
    pub progress: f64,
    /// The states it goes between.
    pub from: String,
    pub to: String,
}

impl Player {
    /// Advance to `now` (seconds on the host's clock). Answers the channel
    /// values to publish (only those away from rest), and what is running.
    pub fn advance(&mut self, doc: &Document, read: &dyn Fn(Leaf) -> Option<Value>, now: f64, clock: f64) -> (BTreeMap<Leaf, Value>, Vec<Running>) {
        let snap = std::mem::take(&mut self.snap);
        let mut seen = std::collections::BTreeSet::new();
        let mut running = Vec::new();
        // The machines (an instance's copies) with a transition under way.
        let mut busy = std::collections::BTreeSet::new();
        let entity_ty = crate::registry::TypeId::lookup("entity").expect("registered");
        for e in doc.nodes_of_type(entity_ty).filter(|&e| doc.is_live(e)).collect::<Vec<_>>() {
            let Some(def) = crate::expand::linked_definition(doc, e) else { continue };
            for (m, port) in machines(doc, def) {
                let Ok(path) = LeafPath::new(crate::facet::FacetKind::Args, &port) else { continue };
                let Some(state) = read(Leaf { node: e, path }).and_then(|v| v.as_str().map(str::to_owned)) else { continue };
                let mc = copy_of(doc, Some(e), m);
                // State keyed on the timeline (machines.md §4.2): the clock
                // drives the transition, so scrubbing shows it mid-way,
                // forwards and backwards.
                if let Some((values, run)) = clocked(doc, read, def, mc, Leaf { node: e, path }, clock) {
                    seen.insert((e, m));
                    if let Some(r) = run {
                        running.push(Running { entity: e, transition: r.0, progress: r.1, from: r.2.clone(), to: r.3.clone() });
                        busy.insert(mc);
                    }
                    self.known.insert((e, m), Known { state, values, run: None, at: None, down: false });
                    continue;
                }
                // Driven from outside (`machine.at`): a parent's transition
                // moves this machine through its own.
                if let Some(at) = read_num(read, mc, "machine.at").filter(|a| *a >= 0.0 && doc.facet(mc, crate::facet::FacetKind::Machine).is_some()) {
                    let before = self.known.get(&(e, m));
                    let down = match before.and_then(|k| k.at) {
                        Some(was) if at < was => true,
                        Some(was) if at > was => false,
                        _ => before.is_some_and(|k| k.down),
                    };
                    if let Some((values, nearest, moving)) = driven(doc, read, def, mc, at, down) {
                        seen.insert((e, m));
                        if let Some((t, p, from, to)) = moving {
                            running.push(Running { entity: e, transition: t, progress: p, from, to });
                        }
                        self.known.insert((e, m), Known { state: nearest, values, run: None, at: Some(at), down });
                        continue;
                    }
                }
                let Some(to) = pose(doc, read, def, mc, &state) else { continue };
                seen.insert((e, m));
                match self.known.get_mut(&(e, m)) {
                    None => {
                        self.known.insert((e, m), Known { state, values: to, run: None, at: None, down: false });
                    }
                    Some(k) if k.state != state => {
                        let from_state = std::mem::replace(&mut k.state, state.clone());
                        match plan(doc, read, def, mc, &from_state, &state).filter(|p| p.ms > 0.0 && !snap.contains(&e)) {
                            Some(plan) => k.run = Some(Run { plan, start: now, from: k.values.clone(), to, states: (from_state.clone(), state.clone()) }),
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
        // A pinned state shows on its main, which reads the definition's own
        // channels.
        let mut pinned_out = BTreeMap::new();
        if let Some(pin) = &self.pinned
            && let Some(p) = pinned_pose(doc, read, pin)
        {
            let value_path: LeafPath = "channel.value".parse().expect("declared leaf");
            for (c, v) in p {
                let rest = doc.get::<Channel>(c).and_then(|x| number(&x.value)).unwrap_or(0.0);
                if (v - rest).abs() > 1e-12 {
                    pinned_out.insert(Leaf { node: c, path: value_path }, Value::Number(v));
                }
            }
        }
        let mut out = BTreeMap::new();
        let value_path: LeafPath = "channel.value".parse().expect("declared leaf");
        for (&(e, m), k) in self.known.iter_mut() {
            if k.run.is_some() {
                busy.insert(copy_of(doc, Some(e), m));
            }
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
                    running.push(Running { entity: e, transition: run.plan.transition, progress: p, from: run.states.0.clone(), to: run.states.1.clone() });
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
        out.extend(pinned_out);
        // Only while moving, so a resting instance publishes nothing.
        let running_path: LeafPath = "machine.running".parse().expect("declared leaf");
        for mc in busy {
            if doc.facet(mc, crate::facet::FacetKind::Machine).is_some() {
                out.insert(Leaf { node: mc, path: running_path }, Value::Bool(true));
            }
        }
        self.running = running.clone();
        (out, running)
    }
}

/// A machine driven to position `at` along its states in order: the
/// channels' values, the nearest state, and the transition it is part way
/// through. Between states `i` and `i + 1` it shows their transition at the
/// fraction, the one going down when `down` (so a child disappears by its
/// own disappear), and eases plainly where there is none. A pure function
/// of `at`: a parent keyed on the clock scrubs its children with it.
#[allow(clippy::type_complexity)]
fn driven(doc: &Document, read: Read, def: NodeId, machine: NodeId, at: f64, down: bool) -> Option<(BTreeMap<NodeId, f64>, String, Option<(NodeId, f64, String, String)>)> {
    let states = state_names(doc, machine);
    let last = states.len().checked_sub(1)?;
    let at = at.min(last as f64);
    let nearest = states[at.round() as usize].clone();
    let i = (at.floor() as usize).min(last.saturating_sub(1));
    let f = at - i as f64;
    if last == 0 || f <= 0.0 {
        return Some((pose(doc, read, def, machine, &states[i])?, nearest, None));
    }
    let (lo, hi) = (pose(doc, read, def, machine, &states[i])?, pose(doc, read, def, machine, &states[i + 1])?);
    if f >= 1.0 {
        return Some((hi, nearest, None));
    }
    // Going down, from the upper state to the lower at 1 - f.
    let (from, to, from_name, to_name, p) = if down { (&hi, &lo, &states[i + 1], &states[i], 1.0 - f) } else { (&lo, &hi, &states[i], &states[i + 1], f) };
    let plan = plan(doc, read, def, machine, from_name, to_name);
    let values = to
        .iter()
        .map(|(&c, &b)| {
            let a = from.get(&c).copied().unwrap_or(b);
            (c, a + (b - a) * plan.as_ref().map_or(p, |pl| pl.shaped(c, p)))
        })
        .collect();
    Some((values, nearest, plan.map(|pl| (pl.transition, p, from_name.clone(), to_name.clone()))))
}

/// A machine whose state is keyed on the timeline, at clock time `clock`:
/// the channels' values, and the transition running and its progress. The
/// transition a key selects runs from the key's time for its length (or
/// the key's `span`, one occurrence retimed), from the previous state's
/// pose, so any time gives the same picture. `None` when no clip drives
/// the state, or the playhead is before its first key.
fn clocked(doc: &Document, read: Read, def: NodeId, machine: NodeId, state: Leaf, clock: f64) -> Option<(BTreeMap<NodeId, f64>, Option<(NodeId, f64, String, String)>)> {
    let (clip, offset) = crate::timeline_view::clip_of(doc, state)?;
    let keys = crate::time::keys(doc, clip);
    let u = clock - crate::time::start(doc, clip) - offset;
    let i = keys.iter().rposition(|k| k.t <= u + 1e-9)?;
    let word = |v: &Value| v.as_str().map(str::to_owned);
    let to_state = word(&keys[i].value)?;
    let from_state = if i > 0 { word(&keys[i - 1].value) } else { doc.leaf(state.node, state.path).and_then(|s| s.constant()).and_then(word) };
    let to = pose(doc, read, def, machine, &to_state)?;
    let Some(from_state) = from_state.filter(|f| *f != to_state) else { return Some((to, None)) };
    let Some(plan) = plan(doc, read, def, machine, &from_state, &to_state).filter(|p| p.ms > 0.0) else { return Some((to, None)) };
    let span = doc.get::<crate::facet::Key>(keys[i].id).and_then(|k| number(&k.span)).filter(|s| s.is_finite() && *s > 0.0).unwrap_or(plan.ms / 1000.0);
    let p = ((u - keys[i].t) / span).clamp(0.0, 1.0);
    if p >= 1.0 {
        return Some((to, None));
    }
    let from = pose(doc, read, def, machine, &from_state)?;
    let values = to.iter().map(|(&c, &b)| {
        let a = from.get(&c).copied().unwrap_or(b);
        (c, a + (b - a) * plan.shaped(c, p))
    }).collect();
    Some((values, Some((plan.transition, p, from_state, to_state))))
}

// ── Authoring by hand (machines.md §9 Q8) ──────────────────────────────

fn order_after_last(doc: &Document, parent: NodeId) -> crate::order::OrderKey {
    doc.children(parent).iter().filter(|c| !c.is_expanded()).filter_map(|&c| doc.live(c)).map(|n| n.order.clone()).max().map_or_else(crate::order::OrderKey::first, |o| crate::order::OrderKey::after(&o))
}

fn named(t: &str) -> crate::facet::Facet {
    use crate::facet::LeafStruct;
    let mut n = Name::default();
    n.text = crate::value::Slot::Const(Value::Str(t.into()));
    n.wrap()
}

fn ty(n: &str) -> crate::registry::TypeId {
    crate::registry::TypeId::lookup(n).expect("registered")
}

/// Ids after the document's and any the ops create.
fn fresh(doc: &Document, ops: &[crate::op::Op]) -> NodeId {
    let mut next = doc.next_id();
    for op in ops {
        if let crate::op::Op::Create { id, .. } = op
            && !id.is_session()
            && !id.is_expanded()
            && *id >= next
        {
            next = id.offset(1);
        }
    }
    next
}

/// A machine's states' names, in order.
pub fn state_names(doc: &Document, machine: NodeId) -> Vec<String> {
    children(doc, machine, "state").into_iter().filter_map(|s| name_of(doc, s)).collect()
}

/// The `in` port a machine follows, on its definition.
fn state_port(doc: &Document, def: NodeId, machine: NodeId) -> Option<NodeId> {
    let port = name_of(doc, machine)?;
    children(doc, def, "port_spec").into_iter().find(|&p| name_of(doc, p).as_deref() == Some(port.as_str()))
}

/// The state port's type, the machine's states as a choice.
fn choice_of(names: &[String]) -> String {
    format!("choice({})", names.join("|"))
}

/// "Add state" on a main: the first makes the machine, following a new
/// input `state` whose type is the choice of its states, with "default"
/// and "state_2" and one transition between any two (300 ms, Easy Ease);
/// each later one adds a state. Answers the ops and the new state's name.
pub fn add_state_ops(doc: &Document, def: NodeId) -> Result<(Vec<crate::op::Op>, String), String> {
    use crate::facet::{Args, LeafStruct, Port, Transition};
    use crate::op::Op;
    use crate::value::Slot;
    if doc.live(def).map(|n| n.ty.name()) != Some("definition") {
        return Err(format!("{def} is not a definition"));
    }
    let mut ops = Vec::new();
    let mut next = fresh(doc, &[]);
    let mut take = || {
        let id = next;
        next = next.offset(1);
        id
    };
    let text = |t: &str| Slot::Const(Value::Str(t.into()));
    match machines(doc, def).first() {
        None => {
            let names = vec!["default".to_owned(), "state_2".to_owned()];
            let (spec, machine) = (take(), take());
            let mut p = Port::default();
            p.kind = text("value");
            p.dir = Slot::Const(Value::Enum("in".into()));
            p.signal = Slot::Const(Value::Enum(names[0].clone()));
            p.datatype = text(&choice_of(&names));
            p.show = Slot::Const(Value::Enum("pin".into()));
            p.group = text("States");
            let port_name = if children(doc, def, "port_spec").iter().any(|&s| name_of(doc, s).as_deref() == Some("state")) { "state_".to_owned() + &machines(doc, def).len().to_string() } else { "state".to_owned() };
            let mut order = order_after_last(doc, def);
            ops.push(Op::Create { id: spec, ty: ty("port_spec"), parent: Some(def), order: order.clone(), facets: vec![named(&port_name), p.wrap()] });
            order = crate::order::OrderKey::after(&order);
            ops.push(Op::Create { id: machine, ty: ty("machine"), parent: Some(def), order, facets: vec![named(&port_name)] });
            let mut k = crate::order::OrderKey::first();
            for n in &names {
                ops.push(Op::Create { id: take(), ty: ty("state"), parent: Some(machine), order: k.clone(), facets: vec![named(n), Args::default().wrap()] });
                k = crate::order::OrderKey::after(&k);
            }
            let mut t = Transition::default();
            t.ms = Slot::Const(Value::Number(300.0));
            ops.push(Op::Create { id: take(), ty: ty("transition"), parent: Some(machine), order: k, facets: vec![named("change"), t.wrap()] });
            // Existing instances get the argument (op.rs's catch-up).
            Ok((ops, names[1].clone()))
        }
        Some(&(machine, _)) => {
            let mut names = state_names(doc, machine);
            let mut i = names.len() + 1;
            while names.contains(&format!("state_{i}")) {
                i += 1;
            }
            let name = format!("state_{i}");
            let order = children(doc, machine, "state").into_iter().filter_map(|s| doc.live(s)).map(|n| n.order.clone()).max().map_or_else(crate::order::OrderKey::first, |o| crate::order::OrderKey::after(&o));
            ops.push(Op::Create { id: take(), ty: ty("state"), parent: Some(machine), order, facets: vec![named(&name), Args::default().wrap()] });
            names.push(name.clone());
            if let Some(spec) = state_port(doc, def, machine) {
                ops.push(Op::Set { id: spec, path: "port.datatype".parse().expect("declared"), slot: text(&choice_of(&names)) });
            }
            Ok((ops, name))
        }
    }
}

/// Rename a state, and what names it: the port's choice, its default,
/// transitions' `from`/`to`.
pub fn rename_state_ops(doc: &Document, state: NodeId, name: &str) -> Result<Vec<crate::op::Op>, String> {
    use crate::op::Op;
    use crate::value::Slot;
    let name = name.trim().replace(' ', "_");
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        return Err("a state's name is letters, digits, - and _".into());
    }
    let old = name_of(doc, state).ok_or("not a state")?;
    let machine = doc.live(state).and_then(|n| n.parent).ok_or("a state belongs to a machine")?;
    let def = doc.live(machine).and_then(|n| n.parent).ok_or("a machine belongs to a definition")?;
    let mut names = state_names(doc, machine);
    if names.contains(&name) {
        return Err(format!("there is already a state {name:?}"));
    }
    for n in names.iter_mut().filter(|n| **n == old) {
        *n = name.clone();
    }
    let text = |t: &str| Slot::Const(Value::Str(t.into()));
    let mut ops = vec![Op::Set { id: state, path: "name.text".parse().expect("declared"), slot: text(&name) }];
    if let Some(spec) = state_port(doc, def, machine) {
        ops.push(Op::Set { id: spec, path: "port.datatype".parse().expect("declared"), slot: text(&choice_of(&names)) });
        if doc.get::<crate::facet::Port>(spec).and_then(|p| p.signal.constant()).and_then(|v| v.as_str()) == Some(old.as_str()) {
            ops.push(Op::Set { id: spec, path: "port.signal".parse().expect("declared"), slot: Slot::Const(Value::Enum(name.clone())) });
        }
    }
    for t in children(doc, machine, "transition") {
        for end in ["from", "to"] {
            let p: LeafPath = format!("transition.{end}").parse().expect("declared");
            if doc.leaf(t, p).and_then(|s| s.constant()).and_then(|v| v.as_str()) == Some(old.as_str()) {
                ops.push(Op::Set { id: t, path: p, slot: text(&name) });
            }
        }
    }
    Ok(ops)
}

/// Delete a state, and what names it: transitions from or to it go, the
/// port's choice loses it, and a default that was it becomes the first
/// state left. A machine keeps at least one state.
pub fn delete_state_ops(doc: &Document, state: NodeId) -> Result<Vec<crate::op::Op>, String> {
    use crate::op::Op;
    use crate::value::Slot;
    let name = name_of(doc, state).ok_or("not a state")?;
    let machine = doc.live(state).and_then(|n| n.parent).ok_or("a state belongs to a machine")?;
    let def = doc.live(machine).and_then(|n| n.parent).ok_or("a machine belongs to a definition")?;
    let names: Vec<String> = state_names(doc, machine).into_iter().filter(|n| *n != name).collect();
    let Some(first) = names.first().cloned() else { return Err("a machine keeps at least one state".into()) };
    let mut ops = vec![Op::Delete { id: state }];
    for t in children(doc, machine, "transition") {
        let ends = ["from", "to"].map(|e| doc.leaf(t, format!("transition.{e}").parse().expect("declared")).and_then(|s| s.constant()).and_then(|v| v.as_str()).map(str::to_owned));
        if ends.iter().any(|e| e.as_deref() == Some(name.as_str())) {
            ops.push(Op::Delete { id: t });
        }
    }
    if let Some(spec) = state_port(doc, def, machine) {
        ops.push(Op::Set { id: spec, path: "port.datatype".parse().expect("declared"), slot: Slot::Const(Value::Str(choice_of(&names))) });
        if doc.get::<crate::facet::Port>(spec).and_then(|p| p.signal.constant()).and_then(|v| v.as_str()) == Some(name.as_str()) {
            ops.push(Op::Set { id: spec, path: "port.signal".parse().expect("declared"), slot: Slot::Const(Value::Enum(first)) });
        }
    }
    Ok(ops)
}

/// Make a state the machine's default: the state port's value where
/// nothing sets it (new instances start in it).
pub fn set_default_state_ops(doc: &Document, state: NodeId) -> Result<Vec<crate::op::Op>, String> {
    let name = name_of(doc, state).ok_or("not a state")?;
    let machine = doc.live(state).and_then(|n| n.parent).ok_or("a state belongs to a machine")?;
    let def = doc.live(machine).and_then(|n| n.parent).ok_or("a machine belongs to a definition")?;
    let spec = state_port(doc, def, machine).ok_or("the machine follows no port")?;
    Ok(vec![crate::op::Op::Set { id: spec, path: "port.signal".parse().expect("declared"), slot: crate::value::Slot::Const(Value::Enum(name)) }])
}

/// A state pinned for editing on a main (session only): the main shows its
/// pose, and number edits to the component's parts record into it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pinned {
    pub def: NodeId,
    pub machine: NodeId,
    pub state: String,
    /// A transition scrubbed by hand in the machine panel (machines.md §6,
    /// "Clicking a transition"): from, to, and its progress 0..1. The main
    /// shows that moment instead of the state's pose.
    pub scrub: Option<(String, String, f64)>,
}

impl Pinned {
    pub fn state(def: NodeId, machine: NodeId, state: impl Into<String>) -> Pinned {
        Pinned { def, machine, state: state.into(), scrub: None }
    }
}

/// What a pin shows on its main: the state's pose, or a scrubbed
/// transition's moment.
fn pinned_pose(doc: &Document, read: Read, pin: &Pinned) -> Option<BTreeMap<NodeId, f64>> {
    let Some((from, to, p)) = &pin.scrub else { return pose(doc, read, pin.def, pin.machine, &pin.state) };
    let (a, b) = (pose(doc, read, pin.def, pin.machine, from)?, pose(doc, read, pin.def, pin.machine, to)?);
    let plan = plan(doc, read, pin.def, pin.machine, from, to);
    let p = p.clamp(0.0, 1.0);
    Some(b.iter().map(|(&c, &y)| {
        let x = a.get(&c).copied().unwrap_or(y);
        let k = plan.as_ref().map_or(if p >= 1.0 { 1.0 } else { 0.0 }, |pl| pl.shaped(c, p));
        (c, x + (y - x) * k)
    }).collect())
}

/// The transition a keyed state is in at clock time `clock`, and its
/// progress (for the timeline's state lanes); `None` at rest.
pub fn progress_at(doc: &Document, read: Read, def: NodeId, machine: NodeId, state: Leaf, clock: f64) -> Option<(NodeId, f64, String, String)> {
    clocked(doc, read, def, machine, state, clock)?.1
}

/// Is the machine a chain or a loop (each state has at most one explicit
/// way in and one way out)? Then the panel draws it as a strip; otherwise
/// as a graph (machines.md §6).
pub fn is_strip(doc: &Document, read: Read, machine: NodeId) -> bool {
    let mut ins: BTreeMap<String, usize> = BTreeMap::new();
    let mut outs: BTreeMap<String, usize> = BTreeMap::new();
    for t in children(doc, machine, "transition") {
        let (f, g) = (read_text(read, t, "transition.from").unwrap_or_else(|| "*".into()), read_text(read, t, "transition.to").unwrap_or_else(|| "*".into()));
        if f != "*" && g != "*" {
            *outs.entry(f).or_default() += 1;
            *ins.entry(g).or_default() += 1;
        }
    }
    ins.values().chain(outs.values()).all(|&n| n <= 1)
}

/// The states in strip order (machines.md §6): along the explicit
/// transitions when they make a chain or a loop (from the chain's start, or
/// the first state for a loop), the rest after in the machine's order; and
/// whether the last runs back to the first (a loop).
pub fn strip_order(doc: &Document, read: Read, machine: NodeId) -> (Vec<String>, bool) {
    let names = state_names(doc, machine);
    let mut next: BTreeMap<String, String> = BTreeMap::new();
    for t in children(doc, machine, "transition") {
        let (f, g) = (read_text(read, t, "transition.from").unwrap_or_else(|| "*".into()), read_text(read, t, "transition.to").unwrap_or_else(|| "*".into()));
        if f != "*" && g != "*" && names.contains(&f) && names.contains(&g) {
            next.entry(f).or_insert(g);
        }
    }
    let mut out: Vec<String> = Vec::new();
    let mut looped = false;
    // Chains first, each from a state nothing explicit leads into.
    let starts: Vec<String> = names.iter().filter(|n| next.contains_key(*n) && !next.values().any(|v| v == *n)).cloned().collect();
    let firsts = if starts.is_empty() { names.iter().filter(|n| next.contains_key(*n)).take(1).cloned().collect() } else { starts };
    for s in firsts {
        let mut at = s;
        while !out.contains(&at) {
            out.push(at.clone());
            match next.get(&at) {
                Some(n) => {
                    if out.first() == Some(n) {
                        looped = true;
                    }
                    at = n.clone();
                }
                None => break,
            }
        }
    }
    for n in names {
        if !out.contains(&n) {
            out.push(n);
        }
    }
    (out, looped)
}

/// Give one pair of states its own transition (machines.md §6): a
/// `from → to` transition after the machine's others, with the length and
/// easing of the one the pair used, and copies of its tweens. Answers the
/// ops and the new transition.
pub fn add_transition_ops(doc: &Document, read: Read, def: NodeId, machine: NodeId, from: &str, to: &str) -> Result<(Vec<crate::op::Op>, NodeId), String> {
    use crate::facet::{LeafStruct, Transition, Tween};
    use crate::op::Op;
    use crate::value::Slot;
    let names = state_names(doc, machine);
    for s in [from, to] {
        if !names.iter().any(|n| n == s) {
            return Err(format!("{s:?} is not a state of this machine"));
        }
    }
    if from == to {
        return Err("a transition goes between two different states".into());
    }
    let was = plan(doc, read, def, machine, from, to);
    if let Some(p) = &was
        && read_text(read, p.transition, "transition.from").as_deref() == Some(from)
        && read_text(read, p.transition, "transition.to").as_deref() == Some(to)
    {
        return Err(format!("{from} → {to} has its own transition already"));
    }
    let mut next = fresh(doc, &[]);
    let id = next;
    next = next.offset(1);
    let text = |t: &str| Slot::Const(Value::Str(t.into()));
    let mut t = Transition::default();
    t.from = text(from);
    t.to = text(to);
    if let Some(p) = &was {
        t.ms = Slot::Const(Value::Number(p.ms));
        if let Some(e) = doc.leaf(p.transition, "transition.ease".parse().expect("declared")) {
            t.ease = e.clone();
        }
    }
    let mut ops = vec![Op::Create { id, ty: ty("transition"), parent: Some(machine), order: order_after_last(doc, machine), facets: vec![named(&format!("{from} to {to}")), t.wrap()] }];
    if let Some(p) = &was {
        let mut order = crate::order::OrderKey::first();
        for w in children(doc, p.transition, "tween") {
            let Some(f) = doc.get::<Tween>(w) else { continue };
            ops.push(Op::Create { id: next, ty: ty("tween"), parent: Some(id), order: order.clone(), facets: vec![f.clone().wrap()] });
            next = next.offset(1);
            order = crate::order::OrderKey::after(&order);
        }
    }
    Ok((ops, id))
}

/// Give a channel its own slice of a transition: a tween over all of it,
/// with the transition's easing, which the panel's bar then narrows.
pub fn add_tween_ops(doc: &Document, transition: NodeId, channel: &str) -> Result<(Vec<crate::op::Op>, NodeId), String> {
    use crate::facet::{LeafStruct, Tween};
    use crate::op::Op;
    use crate::value::Slot;
    if ty_of(doc, transition) != Some("transition") {
        return Err(format!("{transition} is not a transition"));
    }
    if children(doc, transition, "tween").iter().any(|&w| doc.get::<Tween>(w).and_then(|t| t.channel.constant()).and_then(|v| v.as_str()) == Some(channel)) {
        return Err(format!("{channel} has a tween here already"));
    }
    let id = fresh(doc, &[]);
    let mut w = Tween::default();
    w.channel = Slot::Const(Value::Str(channel.into()));
    if let Some(e) = doc.leaf(transition, "transition.ease".parse().expect("declared")) {
        w.ease = e.clone();
    }
    Ok((vec![Op::Create { id, ty: ty("tween"), parent: Some(transition), order: order_after_last(doc, transition), facets: vec![w.wrap()] }], id))
}

/// Record edits while a state is pinned (machines.md §6, "Editing a part
/// while pinned"): a number set on a part of the component's picture
/// becomes that state's pose. A leaf already bound to a channel sets the
/// channel's value in the state; any other gets a new channel, resting at
/// the leaf's value now (what every other state keeps), the leaf bound to
/// it. Other ops pass through.
pub fn record(doc: &Document, pin: &Pinned, ops: Vec<crate::op::Op>) -> Vec<crate::op::Op> {
    // A scrubbed transition shows a moment between two states, not one:
    // edits then are plain edits (the panel says so), never a pose.
    if pin.scrub.is_some() {
        return ops;
    }
    use crate::expr::Expr;
    use crate::facet::{Args, Channel, FacetKind, LeafStruct};
    use crate::op::Op;
    use crate::value::Slot;
    let Some(template) = children(doc, pin.def, "template").first().copied() else { return ops };
    let Some(state) = children(doc, pin.machine, "state").into_iter().find(|&s| name_of(doc, s).as_deref() == Some(pin.state.as_str())) else { return ops };
    let channels: Vec<(NodeId, String, f64)> = channels(doc, pin.def);
    let mut pose: Args = doc.get::<Args>(state).cloned().unwrap_or_default();
    let mut posed = false;
    let mut next = fresh(doc, &ops);
    let mut order = order_after_last(doc, pin.def);
    let mut made: Vec<String> = Vec::new();
    let mut out = Vec::new();
    let part_of = |id: NodeId| !id.is_expanded() && doc.is_ancestor(template, id);
    // A leaf's channel, if its binding is one: `#ch.channel.value`.
    let bound_channel = |slot: Option<&Slot>| match slot.and_then(|s| s.expr()) {
        Some(Expr::Ref(l)) => channels.iter().find(|(c, ..)| *c == l.node).map(|(_, n, _)| n.clone()),
        _ => None,
    };
    let mut record_one = |id: NodeId, path: LeafPath, v: f64, current: Option<f64>, out: &mut Vec<Op>, pose: &mut Args| -> Slot {
        if let Some(name) = bound_channel(doc.leaf(id, path)) {
            pose.insert(&name, Slot::Const(Value::Number(v)));
            return doc.leaf(id, path).cloned().expect("bound");
        }
        let base = format!("{}_{}", name_of(doc, id).unwrap_or_else(|| doc.live(id).map_or("part", |n| n.ty.name()).to_owned()), path.field).replace(' ', "_");
        let mut name = base.clone();
        let mut k = 2;
        while channels.iter().any(|(_, n, _)| *n == name) || made.contains(&name) {
            name = format!("{base}_{k}");
            k += 1;
        }
        made.push(name.clone());
        let ch = next;
        next = next.offset(1);
        let mut c = Channel::default();
        c.value = Slot::Const(Value::Number(current.unwrap_or(v)));
        out.push(Op::Create { id: ch, ty: ty("channel"), parent: Some(pin.def), order: order.clone(), facets: vec![named(&name), c.wrap()] });
        order = crate::order::OrderKey::after(&order);
        pose.insert(&name, Slot::Const(Value::Number(v)));
        Slot::Bound(Expr::Ref(Leaf { node: ch, path: "channel.value".parse().expect("declared") }))
    };
    let rest: LeafPath = "channel.value".parse().expect("declared");
    for op in ops {
        match op {
            // A part already bound to a channel: an edit to it inverts the
            // binding and lands on the channel's rest value, which every state
            // that does not set the channel shows. It is the pinned state's.
            Op::Set { id, path, slot: Slot::Const(Value::Number(v)) } if path == rest && channels.iter().any(|(c, ..)| *c == id) => {
                let name = channels.iter().find(|(c, ..)| *c == id).map(|(_, n, _)| n.clone()).expect("found");
                pose.insert(&name, Slot::Const(Value::Number(v)));
                posed = true;
            }
            Op::Set { id, path, slot: Slot::Const(Value::Number(v)) } if part_of(id) && path.facet != FacetKind::Name => {
                let current = doc.leaf(id, path).and_then(|s| s.constant()).and_then(Value::as_f64).or_else(|| path.facet.default_facet().get(path.field).and_then(|s| s.constant()).and_then(Value::as_f64));
                let slot = record_one(id, path, v, current, &mut out, &mut pose);
                posed = true;
                out.push(Op::Set { id, path, slot });
            }
            Op::AddFacet { id, mut facet } if part_of(id) => {
                let default = facet.kind().default_facet();
                for field in facet.field_names() {
                    let (Some(Value::Number(v)), d) = (facet.get(field).and_then(|s| s.constant()).cloned(), default.get(field).and_then(|s| s.constant()).and_then(Value::as_f64)) else { continue };
                    if Some(v) == d {
                        continue;
                    }
                    let path = LeafPath::new(facet.kind(), field).expect("declared");
                    let slot = record_one(id, path, v, d, &mut out, &mut pose);
                    posed = true;
                    let _ = facet.set(field, slot);
                }
                out.push(Op::AddFacet { id, facet });
            }
            op => out.push(op),
        }
    }
    if posed {
        out.push(Op::AddFacet { id: state, facet: pose.wrap() });
    }
    out
}
