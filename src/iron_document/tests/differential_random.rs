//! Random differential scripts (reactive-engine handoff §4.1, task 5; spec
//! §16.1). A seeded generator builds a document from the pieces the other
//! tests build (repeats keyed by value and by index, a conditional,
//! `std:plot`, a laser-fiber-meter chain, a component of its own with a
//! repeat, state and a context read inside, placed in a repeat's items, a
//! clip on the playhead), then runs a script of random steps over it. With
//! the `differential` feature every step runs through the facade, which
//! compares the two stores after each call.
//!
//! The script must reach the situations the spec lists, and counts that it
//! did; a run that never reaches the hard paths proves nothing. 200 seeds
//! run in the normal suite; the 10,000-seed run is ignored:
//!   cargo test -p iron_document --release --features differential --target aarch64-apple-darwin --test differential_random -- --ignored --nocapture

use iron_document::facet::Name;
use iron_document::*;
use std::collections::{BTreeMap, BTreeSet};

/// SplitMix64: small, seedable, and the same everywhere.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
    fn pick<T: Copy>(&mut self, xs: &[T]) -> Option<T> {
        (!xs.is_empty()).then(|| xs[self.below(xs.len())])
    }
}

/// The situations a script must reach (spec §16.1, "Scripts").
const SITUATIONS: &[&str] = &[
    "repeat in an instance",
    "instance in a repeat",
    "conditional toggled",
    "keys changed kind",
    "override on an item that left and returned",
    "enter or exit in flight",
    "provider moved",
    "connection to an instance port",
    "definition edited",
    "failing transaction",
    "undo",
    "redo",
    "preview",
    "publish",
    "clock moved",
    "nested repeats changed together",
    "tracked read",
    "port_spec datatype changed",
    "providers reordered",
];

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn num(v: f64) -> Slot {
    Slot::Const(Value::Number(v))
}

/// What the script knows of its document: named nodes, and what it has done.
struct World {
    store: Store,
    names: BTreeMap<String, NodeId>,
    rng: Rng,
    now: f64,
    producer: Option<ProducerId>,
    gesture: bool,
    /// Overrides made on items of `$rows`, by item key, with whether that
    /// key has left since.
    overrides: BTreeMap<String, bool>,
    /// The kind of `$names`' elements last set: words or numbers.
    names_kind: &'static str,
    reached: BTreeMap<&'static str, u32>,
    template_extra: Vec<NodeId>,
}

impl World {
    fn reach(&mut self, what: &'static str) {
        *self.reached.entry(what).or_insert(0) += 1;
    }

    fn id(&self, name: &str) -> Option<NodeId> {
        self.names.get(name).copied().filter(|&n| self.store.tree().is_live(n))
    }

    /// Plan and apply one `<edit>`; its `$names` join the world's.
    fn edit(&mut self, xml: &str) -> Result<(), String> {
        // Names from earlier edits become the ids they named; an edit's
        // own names stay for the planner.
        let mut text = String::new();
        let mut rest = xml;
        while let Some(at) = rest.find('$') {
            text.push_str(&rest[..at]);
            let tail = &rest[at + 1..];
            let len = tail.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(tail.len());
            let token = &rest[at..at + 1 + len];
            match self.names.get(token) {
                Some(id) if len > 0 => text.push_str(&id.to_string()),
                _ => text.push_str(token),
            }
            rest = &rest[at + 1 + len..];
        }
        text.push_str(rest);
        let plan = edit::plan(&self.store, &format!("<edit>{text}</edit>"), None).map_err(|f| f.message())?;
        self.store.apply(tx("edit", plan.ops)).map_err(|e| format!("{e:?}"))?;
        for (k, v) in plan.aliases {
            self.names.insert(k, v);
        }
        Ok(())
    }

    fn set(&mut self, id: NodeId, leaf: &str, slot: Slot) -> Result<Applied, Vec<ApplyError>> {
        self.store.apply(tx("set", vec![Op::Set { id, path: path(leaf), slot }]))
    }

    /// Live item scopes of a repeat, with their keys.
    fn items(&self, repeat: NodeId) -> Vec<(String, NodeId)> {
        let doc = self.store.tree();
        doc.children(repeat)
            .iter()
            .copied()
            .filter(|c| c.is_item_scope() && doc.is_live(*c))
            .map(|c| (c.to_string().rsplit("/~").next().unwrap_or("").to_owned(), c))
            .collect()
    }

    fn live_nodes(&self) -> Vec<NodeId> {
        self.store.tree().live_nodes().map(|n| n.id).collect()
    }
}

/// The document: a composition and a model, and a random set of pieces.
fn build(seed: u64) -> World {
    let mut w = World {
        store: Store::new(),
        names: BTreeMap::new(),
        rng: Rng(seed),
        now: 0.0,
        producer: None,
        gesture: false,
        overrides: BTreeMap::new(),
        names_kind: "words",
        reached: BTreeMap::new(),
        template_extra: Vec::new(),
    };
    w.edit(r#"<model name="model"/><library id="$lib" name="library"/><composition id="$c" name="sheet" dims="2d"/>"#).unwrap();
    let r = &mut w.rng;
    // The plot is the large piece: placing `std:plot` brings its
    // definition, body and picture, 281 nodes. One seed in five has it
    // (about 360 nodes at most); the rest stay under about 200.
    let (ticks, rows, when, chip, clip) = (r.chance(80), r.chance(85), r.chance(70), r.chance(60), r.chance(40));
    let plot = r.chance(20);
    let lab = !plot && r.chance(50);
    let fades = r.chance(50);
    let presence = if fades { r#" repeat.enter_ms="100" repeat.exit_ms="200""# } else { "" };
    let keyed = if w.rng.chance(50) { r#" repeat.key="{= t => t}""# } else { "" };
    if ticks {
        w.edit(&format!(
            r#"<let in="$c" id="$lo" name="lo" value="0"/>
            <group in="$c" id="$ticks" name="ticks" y="10" repeat.each="{{= range($lo, $lo + 20, 4)}}"{keyed}{presence}>
              <text name="tick" x="{{= $ticks.repeat.item * 3}}" y="0" content="{{= fmt($ticks.repeat.item)}}"/>
            </group>"#
        ))
        .unwrap();
    }
    if rows {
        w.edit(&format!(
            r#"<let in="$c" id="$names" name="names" value="{{= list('a', 'b', 'c')}}"/>
            <group in="$c" id="$rows" name="rows" y="40" repeat.each="{{= $names}}"{presence}>
              <rect name="row" x="0" y="{{= $rows.repeat.index * 12}}" w="10" h="10" appearance.opacity="{{= $rows.repeat.presence}}"/>
            </group>"#
        ))
        .unwrap();
    }
    if when {
        let exit = if fades { r#" when.exit_ms="100""# } else { "" };
        w.edit(&format!(
            r#"<let in="$c" id="$open" name="open" value="false"/>
            <group in="$c" id="$body" name="body" when="{{= $open}}"{exit}><rect name="panel" w="10" h="10"/></group>"#
        ))
        .unwrap();
    }
    if plot {
        w.edit(r#"<let in="$c" id="$span" name="span" value="2"/><place def="std:plot" in="$c" id="$plot" x="300" y="0" w="300" h="200" xs="0 1 2" ys="{= $self.xs * 2}" x_min="0" x_max="{= $span}"/>"#).unwrap();
    }
    if lab {
        w.edit(r#"<place def="std:laser" in="$c" id="$laser" x="40" y="300"/><place def="std:fiber" in="$c" id="$fiber" x="230" y="300"/><place def="std:meter" in="$c" id="$meter" x="420" y="300"/>"#).unwrap();
    }
    if chip {
        // A component of its own: a repeat inside, a session state toggled
        // by a click, and a context read with a default.
        w.edit(
            r#"<definition in="$lib" id="$chip" name="chip">
              <port_spec id="$chip_n" name="n" kind="value" dir="in" signal="2" datatype="number"/>
              <template id="$chip_t" notation="free">
                <let name="gap" provide="default" value="8"/>
                <let id="$hot" name="hot" keep="session" value="false"/>
                <rect id="$chip_box" name="box" x="{= @gap}" y="0" w="20" h="10" fill.color="{= $hot ? rgb(1, 0, 0) : rgb(0, 0, 1)}"/>
                <on event="click" target="$hot" to="{= !$hot}"/>
                <group id="$dots" name="dots" repeat.count="2"><ellipse name="dot" x="{= $dots.repeat.index * 5}" y="12" rx="2" ry="2"/></group>
              </template>
            </definition>
            <group in="$c" id="$plain" name="plain" y="400"/>
            <group in="$c" id="$wide" name="wide" y="450"><let id="$gap" name="gap" provide="yes" value="40"/><let id="$gap2" name="gap" provide="yes" value="60"/></group>
            <let in="$c" id="$n" name="n" value="2"/>"#,
        )
        .unwrap();
        let chip = w.names["$chip"];
        w.edit(&format!(r#"<group in="$plain" id="$cells" name="cells" repeat.count="{{= $n}}"><place def="{chip}" x="0" y="0"/></group><place def="{chip}" in="$wide" id="$chip_wide" x="0" y="20" n="{{= $n + 1}}"/>"#)).unwrap();
    }
    if w.rng.chance(50) {
        // A repeat in each item of another, both counted by one value.
        w.edit(r#"<let in="$c" id="$k" name="k" value="2"/><group in="$c" id="$outer" name="outer" y="600" repeat.count="{= $k}"><group id="$inner" name="inner" repeat.count="{= $k}"><rect name="cell" x="{= $inner.repeat.index * 4}" w="3" h="3"/></group></group>"#).unwrap();
    }
    if w.rng.chance(50) {
        // Two rects a tracked read can join into a cycle and part again.
        w.edit(r#"<rect in="$c" id="$da" name="da" x="0" y="700" w="1" h="1"/><rect in="$c" id="$db" name="db" x="10" y="700" w="1" h="1"/>"#).unwrap();
    }
    if clip {
        w.edit(
            r#"<rect in="$c" id="$anim" name="anim" x="50" y="500" w="10" h="10"/>
            <timeline id="$tl" name="timeline"><clip id="$clip" timing.start="0" clip.prop="transform2d.x"><key key.t="0" key.value="0"/><key key.t="2" key.value="300"/></clip></timeline>
            <link from="$clip" to="$anim" rel="animates"/>"#,
        )
        .unwrap();
        w.store.ensure_clock().unwrap();
    }
    if lab {
        // A connection to an instance's port.
        connect(&mut w);
    }
    w
}

/// The port `name` of the entity a placed `use` figures.
fn port(w: &World, placed: &str, name: &str) -> Option<NodeId> {
    let doc = w.store.tree();
    let u = w.id(placed)?;
    let e = composition::figure_of(doc, u).map(|(_, e)| e)?;
    doc.children(e).iter().copied().find(|&p| doc.is_live(p) && doc.get::<Name>(p).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name))
}

fn connect(w: &mut World) {
    let (Some(out), Some(inp)) = (port(w, "$laser", "out"), port(w, "$fiber", "in")) else { return };
    let model = w.store.tree().roots().iter().copied().find(|&r| w.store.tree().live(r).is_some_and(|n| n.ty.name() == "model")).unwrap();
    let xml = format!(r#"<connection in="{model}" id="$wire"><link from="$wire" to="{out}" rel="connects" end="a"/><link from="$wire" to="{inp}" rel="connects" end="b"/></connection>"#);
    if w.edit(&xml).is_ok() {
        w.reach("connection to an instance port");
    }
}

/// One random step. Each arm does one kind of thing the host or the model
/// does, and notes the situations it reached.
fn step(w: &mut World) {
    let doc_has = |w: &World, n: &str| w.id(n).is_some();
    match w.rng.below(29) {
        // Value-driven structure.
        0 if doc_has(w, "$lo") => {
            let v = (w.rng.below(40) as f64) * if w.rng.chance(30) { 0.5 } else { 1.0 };
            let _ = w.set(w.names["$lo"], "let.value", num(v));
        }
        1 if doc_has(w, "$names") => {
            let words = ["a", "b", "c", "d", "e"];
            let len = w.rng.below(5);
            let (kind, list) = if w.rng.chance(25) {
                ("numbers", (0..len).map(|_| (w.rng.below(6) as f64).to_string()).collect::<Vec<_>>())
            } else {
                ("words", (0..len).map(|_| format!("'{}'", words[w.rng.below(words.len())])).collect())
            };
            let before: BTreeSet<String> = w.items(w.names["$rows"]).into_iter().map(|(k, _)| k).collect();
            if w.set(w.names["$names"], "let.value", Slot::Bound(Expr::parse(&format!("list({})", list.join(", "))).unwrap())).is_ok() {
                if kind != w.names_kind {
                    w.reach("keys changed kind");
                }
                w.names_kind = kind;
                let after: BTreeSet<String> = w.items(w.names["$rows"]).into_iter().map(|(k, _)| k).collect();
                let returned: Vec<String> = w.overrides.iter().filter(|(k, left)| **left && after.contains(*k)).map(|(k, _)| k.clone()).collect();
                for k in returned {
                    w.reach("override on an item that left and returned");
                    w.overrides.insert(k, false);
                }
                for (k, left) in w.overrides.iter_mut() {
                    if before.contains(k) && !after.contains(k) {
                        *left = true;
                    }
                }
            }
        }
        2 if doc_has(w, "$open") => {
            let open = w.store.resolved(w.names["$open"], path("let.value")) == Some(Value::Bool(true));
            if w.set(w.names["$open"], "let.value", Slot::Const(Value::Bool(!open))).is_ok() {
                w.reach("conditional toggled");
            }
        }
        3 if doc_has(w, "$span") => {
            let v = 1.0 + w.rng.below(30) as f64 * 0.37;
            if w.set(w.names["$span"], "let.value", num(v)).is_ok() {
                w.reach("repeat in an instance");
            }
        }
        // The chip's own repeat, in each of its instances.
        3 if doc_has(w, "$dots") => {
            let v = w.rng.below(4) as f64;
            if w.set(w.names["$dots"], "repeat.count", num(v)).is_ok() {
                w.reach("repeat in an instance");
                w.reach("definition edited");
            }
        }
        4 if doc_has(w, "$n") => {
            let v = w.rng.below(4) as f64;
            if w.set(w.names["$n"], "let.value", num(v)).is_ok() && v > 0.0 {
                w.reach("instance in a repeat");
            }
        }
        // An override on a copy inside an item.
        5 | 22 | 23 | 24 if doc_has(w, "$rows") => {
            let items = w.items(w.names["$rows"]);
            if let Some((key, item)) = w.rng.pick(&items.iter().map(|(k, i)| (k.as_str(), *i)).collect::<Vec<_>>()).map(|(k, i)| (k.to_owned(), i)) {
                let copy = w.store.tree().children(item).first().copied();
                let x = w.rng.below(50) as f64;
                if let Some(copy) = copy
                    && w.set(copy, "transform2d.x", num(x)).is_ok()
                {
                    w.overrides.entry(key).or_insert(false);
                }
            }
        }
        // A definition edit: a constant in the template, or a node added
        // to it or taken away.
        6 if doc_has(w, "$chip_box") => {
            let ok = match w.rng.below(3) {
                0 => {
                    let v = 10.0 + w.rng.below(30) as f64;
                    w.set(w.names["$chip_box"], "size.w", num(v)).is_ok()
                }
                1 => {
                    let t = w.names["$chip_t"];
                    w.edit(&format!(r#"<rect in="{t}" id="$extra" name="extra" x="30" y="0" w="4" h="4"/>"#)).map(|_| w.template_extra.push(w.names["$extra"])).is_ok()
                }
                _ => match w.template_extra.pop() {
                    Some(n) => w.store.apply(tx("del", vec![Op::Delete { id: n }])).is_ok(),
                    None => false,
                },
            };
            if ok {
                w.reach("definition edited");
            }
        }
        // Transactions that fail.
        7 => {
            let nodes = w.live_nodes();
            let derived: Vec<NodeId> = nodes.iter().copied().filter(|n| n.is_expanded()).collect();
            let ops = match (w.rng.below(3), w.rng.pick(&derived), w.id("$lo").or(w.id("$open"))) {
                (0, Some(d), _) => vec![Op::Delete { id: d }],
                (1, _, Some(l)) => vec![Op::Set { id: l, path: path("let.value"), slot: Slot::Bound(Expr::parse(&format!("{l}.let.value + 1")).unwrap()) }],
                _ => {
                    let a = w.store.authored().next_id();
                    vec![Op::Set { id: a, path: path("transform2d.x"), slot: num(1.0) }]
                }
            };
            if w.store.apply(tx("bad", ops)).is_err() {
                w.reach("failing transaction");
            }
        }
        8 => {
            if let Some(r) = w.store.undo(0)
                && r.is_ok()
            {
                w.reach("undo");
                if w.rng.chance(40)
                    && let Some(r) = w.store.redo(0)
                    && r.is_ok()
                {
                    w.reach("redo");
                }
            }
        }
        9 => {
            if let Some(r) = w.store.redo(0)
                && r.is_ok()
            {
                w.reach("redo");
            }
        }
        // A gesture's previews, committed or cancelled at a later step.
        10 => {
            if w.gesture {
                if w.rng.chance(50) {
                    let _ = w.store.commit_gesture("drag", Actor::Human, 0);
                } else {
                    w.store.cancel_gesture();
                }
                w.gesture = false;
            } else {
                let movable: Vec<NodeId> = w.live_nodes().into_iter().filter(|&n| w.store.tree().get::<facet::Transform2d>(n).is_some()).collect();
                if let Some(n) = w.rng.pick(&movable) {
                    w.store.begin_gesture(&[n]);
                    w.gesture = true;
                    if w.store.preview_target(n, path("transform2d.x"), Value::Number(w.rng.below(100) as f64)).is_ok() {
                        w.reach("preview");
                    }
                }
            }
        }
        // A drag on the plot: actions previewed through the overlay.
        11 if doc_has(w, "$plot") => {
            let doc = w.store.tree();
            let area = doc.descendants(w.names["$plot"]).into_iter().find(|&d| doc.is_live(d) && !expand::in_repeat_template(doc, d) && doc.get::<Name>(d).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some("plot area"));
            if let Some(area) = area {
                let dx = w.rng.below(40) as f64 - 20.0;
                if w.store.pointer_action(area, "drag", [dx, 0.0, 100.0 + dx, 50.0, 0.0]).is_ok() {
                    w.reach("repeat in an instance");
                }
                if w.rng.chance(50) {
                    let _ = w.store.commit_gesture("pan", Actor::Human, 0);
                } else {
                    w.store.cancel_gesture();
                }
            }
        }
        // A host producer.
        12 => {
            let p = *w.producer.get_or_insert_with(|| w.store.register_producer("random"));
            let targets: Vec<NodeId> = w.live_nodes().into_iter().filter(|&n| !n.is_expanded() && w.store.tree().get::<facet::Transform2d>(n).is_some()).collect();
            if w.rng.chance(25) {
                w.store.unpublish(p);
            } else if let Some(n) = w.rng.pick(&targets) {
                let values = BTreeMap::from([(Leaf { node: n, path: path("transform2d.y") }, Value::Number(w.rng.below(200) as f64))]);
                w.store.publish(p, values);
                w.reach("publish");
            }
        }
        // The playhead.
        13 if doc_has(w, "$clip") => {
            w.store.set_time(w.rng.below(30) as f64 * 0.1);
            w.reach("clock moved");
        }
        // Presentation time: the steppers the host calls each frame.
        14 | 15 => {
            w.now += 0.03 + w.rng.below(10) as f64 * 0.02;
            let moving = w.store.advance_presence(w.now);
            if !moving.is_empty() {
                w.reach("enter or exit in flight");
            }
            w.store.advance_machines(w.now);
            w.store.advance_steps(w.now, &BTreeSet::new());
            w.store.advance_queries(w.now);
            w.store.advance_motion(w.now);
        }
        16 => {
            let nodes = w.live_nodes();
            let picked: Vec<(NodeId, BTreeMap<String, Value>)> = (0..w.rng.below(3)).filter_map(|_| w.rng.pick(&nodes)).map(|n| (n, BTreeMap::new())).collect();
            let _ = w.store.set_selection(&picked);
        }
        // Move the context provider, or stop and start providing.
        17 if doc_has(w, "$gap") => {
            let (gap, plain, wide) = (w.names["$gap"], w.names["$plain"], w.names["$wide"]);
            let ok = if w.rng.chance(50) {
                let to = if w.store.tree().live(gap).and_then(|n| n.parent) == Some(wide) { plain } else { wide };
                w.store.apply(tx("move", vec![Op::Reparent { id: gap, parent: Some(to), order: OrderKey::first() }])).is_ok()
            } else {
                let on = w.store.resolved(gap, path("let.provide")) == Some(Value::Enum("yes".into()));
                w.set(gap, "let.provide", Slot::Const(Value::Enum(if on { "no" } else { "yes" }.into()))).is_ok()
            };
            if ok {
                w.reach("provider moved");
            }
        }
        // Connect or disconnect the laser's fiber.
        18 if doc_has(w, "$laser") => match w.id("$wire") {
            Some(wire) => {
                let _ = w.store.apply(tx("cut", vec![Op::Delete { id: wire }]));
            }
            None => connect(w),
        },
        // A click on what listens for one.
        19 => {
            let nodes = w.live_nodes();
            if let Some(n) = w.rng.pick(&nodes)
                && let Some(at) = w.store.listens_for(n, "click")
            {
                let _ = w.store.fire_at(at, "click", Actor::Human, 0);
            }
        }
        // What the host drains each frame.
        20 => {
            w.store.take_changes();
            w.store.take_emitted();
        }
        // Nested repeats whose counts change in one round.
        25 if doc_has(w, "$k") => {
            let v = 1.0 + w.rng.below(3) as f64;
            let was = w.store.resolved(w.names["$k"], path("let.value"));
            if w.set(w.names["$k"], "let.value", num(v)).is_ok() && was != Some(Value::Number(v)) {
                w.reach("nested repeats changed together");
            }
        }
        // A tracked read, which can close a cycle with the other rect and
        // open it again.
        26 if doc_has(w, "$da") => {
            let (a, b) = (w.names["$da"], w.names["$db"]);
            let ops = match w.rng.below(4) {
                0 => vec![Op::Set { id: a, path: path("size.w"), slot: Slot::Bound(Expr::parse(&format!("deref({b}, 'size.w')")).unwrap()) }],
                1 => vec![Op::Set { id: b, path: path("size.w"), slot: Slot::Bound(Expr::parse(&format!("{a}.size.w + 1")).unwrap()) }],
                2 => vec![Op::Set { id: b, path: path("size.w"), slot: num(w.rng.below(9) as f64) }],
                _ => vec![Op::Set { id: a, path: path("size.w"), slot: num(1.0) }],
            };
            if w.store.apply(tx("deref", ops)).is_ok() {
                w.reach("tracked read");
            }
        }
        // A definition's input changes type: instances' arguments are
        // checked again.
        27 if doc_has(w, "$chip_n") => {
            let to = ["number", "bool", ""][w.rng.below(3)];
            if w.set(w.names["$chip_n"], "port.datatype", Slot::Const(Value::Str(to.into()))).is_ok() {
                w.reach("port_spec datatype changed");
            }
        }
        // The other provider first: `@gap` answers from it.
        28 if doc_has(w, "$gap2") && doc_has(w, "$gap") => {
            let (gap, gap2) = (w.names["$gap"], w.names["$gap2"]);
            let doc = w.store.tree();
            let (Some(a), Some(b)) = (doc.live(gap), doc.live(gap2)) else { return };
            if a.parent != b.parent {
                return;
            }
            let (moved, before) = if a.order < b.order { (gap2, a.order.clone()) } else { (gap, b.order.clone()) };
            if w.store.apply(tx("reorder", vec![Op::Reorder { id: moved, order: OrderKey::before(&before) }])).is_ok() {
                w.reach("providers reordered");
            }
        }
        // A constant on an authored node, the most common edit.
        _ => {
            let targets: Vec<NodeId> = w.live_nodes().into_iter().filter(|&n| !n.is_expanded() && !n.is_session() && w.store.tree().get::<facet::Transform2d>(n).is_some()).collect();
            if let Some(n) = w.rng.pick(&targets) {
                let x = w.rng.below(500) as f64;
                let _ = w.set(n, "transform2d.x", num(x));
            }
        }
    }
}

fn script(seed: u64, steps: usize) -> (BTreeMap<&'static str, u32>, usize) {
    let mut w = build(seed);
    let mut most = 0;
    for _ in 0..steps {
        step(&mut w);
        most = most.max(w.store.tree().live_nodes().count());
    }
    (w.reached, most)
}

/// Run the seeds on every core: each seed's two stores stay on one thread.
fn run(seeds: std::ops::Range<u64>) {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};
    let next = AtomicU64::new(seeds.start);
    let total: Mutex<BTreeMap<&'static str, u32>> = Mutex::new(BTreeMap::new());
    let largest = AtomicU64::new(0);
    let failed: Mutex<Option<(u64, String)>> = Mutex::new(None);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let seed = next.fetch_add(1, Ordering::Relaxed);
                    if seed >= seeds.end || failed.lock().unwrap().is_some() {
                        break;
                    }
                    match std::panic::catch_unwind(|| script(seed, 30)) {
                        Ok((reached, most)) => {
                            largest.fetch_max(most as u64, Ordering::Relaxed);
                            let mut t = total.lock().unwrap();
                            for (k, v) in reached {
                                *t.entry(k).or_insert(0) += v;
                            }
                        }
                        Err(e) => {
                            let why = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                            let mut f = failed.lock().unwrap();
                            if f.as_ref().is_none_or(|(s, _)| seed < *s) {
                                *f = Some((seed, why));
                            }
                        }
                    }
                }
            });
        }
    });
    if let Some((seed, why)) = failed.into_inner().unwrap() {
        panic!("seed {seed}: {why}");
    }
    let total = total.into_inner().unwrap();
    println!("{} seeds on {threads} threads, largest document {} live nodes; situations reached:", seeds.end - seeds.start, largest.into_inner());
    for s in SITUATIONS {
        println!("  {s:45} {}", total.get(s).copied().unwrap_or(0));
    }
    let missing: Vec<&&str> = SITUATIONS.iter().filter(|s| !total.contains_key(**s)).collect();
    assert!(missing.is_empty(), "never reached: {missing:?}");
}

#[test]
fn two_hundred_random_scripts() {
    run(0..200);
}

#[test]
#[ignore]
fn ten_thousand_random_scripts() {
    run(0..10_000);
}

/// One seed, from `IRON_SEED`, with its failure in full: for reproducing
/// what the runs above report.
#[test]
#[ignore]
fn one_seed() {
    let seed: u64 = std::env::var("IRON_SEED").expect("set IRON_SEED").parse().expect("a number");
    script(seed, 30);
}
