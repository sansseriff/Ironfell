//! The engine against the reference, on written cases and random programs.

use iron_engine_kernel::*;
use std::rc::Rc;

fn n(v: f64) -> Value {
    Value::Num(v)
}
fn c(v: f64) -> Box<Expr> {
    Box::new(Expr::Const(v))
}
fn input(i: usize) -> Box<Expr> {
    Box::new(Expr::Input(i))
}

/// `plots` plots, each a Collection of tick labels over its own range
/// (inputs `2p` and `2p + 1`), each label a position and a text offset.
fn plots(count: usize) -> (Rc<Template>, Vec<Value>) {
    let label = Rc::new(Template {
        entries: vec![
            Entry::Cell(Expr::Mul(Box::new(Expr::ItemValue), c(10.0))),
            Entry::Cell(Expr::Add(Box::new(Expr::Local(0)), c(4.0))),
        ],
    });
    let mut entries = Vec::new();
    let mut inputs = Vec::new();
    for p in 0..count {
        entries.push(Entry::Collection { each: Expr::Ticks(input(2 * p), input(2 * p + 1), c(1.0)), template: label.clone() });
        inputs.push(n(0.0));
        inputs.push(n(5.5));
    }
    (Rc::new(Template { entries }), inputs)
}

fn settled(e: &mut Engine, t: &Template, inputs: &[Value]) {
    e.stabilise();
    e.check();
    assert_eq!(e.values(false), reference(t, inputs));
}

/// The case this engine exists for: one more tick in one plot costs that
/// tick, whatever else the document holds.
#[test]
fn a_tick_change_costs_the_change() {
    for count in [1, 10, 100] {
        let (t, mut inputs) = plots(count);
        let mut e = Engine::new(t.clone(), &inputs);
        e.observe(e.root());
        settled(&mut e, &t, &inputs);

        // A pan that changes no tick: the range moves inside one step.
        let before = e.evaluations;
        inputs[1] = n(5.9);
        e.set_input(1, inputs[1].clone());
        settled(&mut e, &t, &inputs);
        assert_eq!(e.evaluations - before, 1, "only the Collection's `each` runs");

        // One more tick: the Structure cell and the new label's two cells.
        let (before, made) = (e.evaluations, e.made);
        inputs[1] = n(6.5);
        e.set_input(1, inputs[1].clone());
        settled(&mut e, &t, &inputs);
        assert_eq!(e.made - made, 1);
        assert_eq!(e.evaluations - before, 3, "with {count} plots");

        // One fewer: nothing is evaluated but the Structure cell.
        let (before, disposed) = (e.evaluations, e.disposed);
        inputs[1] = n(5.5);
        e.set_input(1, inputs[1].clone());
        settled(&mut e, &t, &inputs);
        assert_eq!(e.disposed - disposed, 1);
        assert_eq!(e.evaluations - before, 1);
    }
}

/// Nothing observed, nothing computed; observing later gives what observing
/// throughout would have (spec §8.8).
#[test]
fn what_nothing_observes_is_not_computed() {
    let (t, mut inputs) = plots(10);
    let mut e = Engine::new(t.clone(), &inputs);
    e.stabilise();
    assert_eq!(e.evaluations, 0);
    assert_eq!(e.made, 0);
    for step in 0..20 {
        inputs[3] = n(5.5 + step as f64);
        e.set_input(3, inputs[3].clone());
        e.stabilise();
    }
    assert_eq!(e.evaluations, 0);
    e.observe(e.root());
    settled(&mut e, &t, &inputs);
    e.unobserve(e.root());
    inputs[3] = n(2.5);
    e.set_input(3, inputs[3].clone());
    e.stabilise();
    e.check();
    assert!(e.values(true).is_empty());
    e.observe(e.root());
    settled(&mut e, &t, &inputs);
}

/// A reader outside a Collection (a sum over its items) follows items that
/// come and go, in the same pass that makes them (R4, R5).
#[test]
fn a_reader_of_items_runs_after_them_in_one_pass() {
    let item = Rc::new(Template { entries: vec![Entry::Cell(Expr::Mul(Box::new(Expr::ItemValue), c(2.0)))] });
    let t = Rc::new(Template {
        entries: vec![
            Entry::Collection { each: Expr::Ticks(c(0.0), input(0), c(1.0)), template: item },
            Entry::Cell(Expr::Sum(Box::new(Expr::Collect(0, 0)))),
            Entry::Cell(Expr::Add(Box::new(Expr::Local(1)), c(1.0))),
        ],
    });
    let mut inputs = vec![n(3.0)];
    let mut e = Engine::new(t.clone(), &inputs);
    e.observe(e.root());
    settled(&mut e, &t, &inputs);
    assert_eq!(e.values(false)[&(vec![], 1)], Ok(n(12.0)));
    for hi in [6.0, 1.0, 0.0, -1.0, 4.0] {
        inputs[0] = n(hi);
        e.set_input(0, inputs[0].clone());
        settled(&mut e, &t, &inputs);
    }
}

/// Only the sum is observed: the Collection and its items exist because the
/// sum needs them, and stop being kept when it is no longer observed.
#[test]
fn necessity_reaches_through_a_collection() {
    let item = Rc::new(Template {
        entries: vec![
            Entry::Cell(Expr::Mul(Box::new(Expr::ItemValue), c(2.0))),
            // Nothing reads this one, so it is never evaluated.
            Entry::Cell(Expr::Add(Box::new(Expr::ItemIndex), c(100.0))),
        ],
    });
    // Root: the Collection of items, and a one-item Collection holding the
    // reader, so the reader can be observed without the items' owner.
    let t = Rc::new(Template {
        entries: vec![
            Entry::Collection { each: Expr::Ticks(c(1.0), input(0), c(1.0)), template: item },
            Entry::Cell(Expr::Sum(Box::new(Expr::Collect(0, 0)))),
            Entry::Collection { each: Expr::Const(1.0), template: Rc::new(Template { entries: vec![Entry::Cell(Expr::Up(1, 1))] }) },
        ],
    });
    let inputs = vec![n(4.0)];
    let mut e = Engine::new(t.clone(), &inputs);
    e.observe(e.root());
    e.stabilise();
    e.unobserve(e.root());
    let watcher = e.owner_at(&vec![(2, Key::Val(0f64.to_bits()))]).expect("the reader's owner");
    e.observe(watcher);
    let (before, retries) = (e.evaluations, e.retries);
    e.set_input(0, n(5.0));
    e.stabilise();
    e.check();
    // Structure; the sum, which finds the new item's cell not yet run and
    // is queued again above it (R5); that cell; the sum; the watcher's cell.
    assert_eq!(e.evaluations - before, 5);
    assert_eq!(e.retries - retries, 1);
    let necessary = e.values(true);
    assert_eq!(necessary[&(vec![], 1)], Ok(n(30.0)));
    assert!(necessary.keys().all(|(path, entry)| !(path.len() == 1 && path[0].0 == 0 && *entry == 1)), "an item cell nothing reads is not kept");
    e.unobserve(watcher);
    e.set_input(0, n(9.0));
    let before = e.evaluations;
    e.stabilise();
    assert_eq!(e.evaluations, before);
}

/// A tracked read that finds a cell above the reader raises the reader and
/// runs it again after that cell (R5); one that reaches itself fails (R6).
#[test]
fn tracked_reads_raise_and_cycles_fail() {
    let candidates = Rc::new(vec![1usize, 2]);
    let t = Rc::new(Template {
        entries: vec![
            Entry::Cell(Expr::Deref(input(0), candidates)),
            Entry::Cell(Expr::Add(input(1), c(1.0))),
            Entry::Cell(Expr::Add(Box::new(Expr::Local(1)), c(1.0))),
        ],
    });
    // Entry 0 reads entries declared after it, which the reference (it
    // goes in declaration order) cannot; the values are checked by hand.
    let mut e = Engine::new(t, &[n(1.0), n(10.0)]);
    e.observe(e.root());
    e.stabilise();
    e.check();
    assert_eq!(e.values(false)[&(vec![], 0)], Ok(n(12.0)));
    assert!(e.retries > 0, "the reader ran before what it reads and came round again");
    for (i, v, want) in [(1, 20.0, 22.0), (0, 0.0, 21.0), (1, 30.0, 31.0), (0, 1.0, 32.0)] {
        e.set_input(i, n(v));
        e.stabilise();
        e.check();
        assert_eq!(e.values(false)[&(vec![], 0)], Ok(n(want)));
    }

    let cyclic = Rc::new(Template {
        entries: vec![Entry::Cell(Expr::Deref(input(0), Rc::new(vec![1usize]))), Entry::Cell(Expr::Add(Box::new(Expr::Local(0)), c(1.0)))],
    });
    let mut e = Engine::new(cyclic, &[n(0.0)]);
    e.observe(e.root());
    e.stabilise();
    let values = e.values(false);
    assert!(values[&(vec![], 0)].is_err());
    assert!(values[&(vec![], 1)].is_err());
}

// ---- random programs ----

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn chance(&mut self, of: usize) -> bool {
        self.below(of) == 0
    }
}

/// What an expression being generated may read.
struct Scope {
    inputs: usize,
    /// Cell entries declared so far in this owner.
    cells: Vec<usize>,
    /// Collections declared so far in this owner, with their templates'
    /// cell entries.
    collections: Vec<(usize, Vec<usize>)>,
    /// For each enclosing owner, nearest first, its readable cell entries.
    ups: Vec<Vec<usize>>,
    is_item: bool,
    /// Root cell entries a tracked read may reach.
    root_cells: Vec<usize>,
}

fn number(r: &mut Rng, s: &Scope, depth: usize) -> Expr {
    let leaf = depth == 0 || r.chance(3);
    if leaf {
        loop {
            match r.below(6) {
                0 => return Expr::Const(r.below(5) as f64 - 1.0),
                1 => return Expr::Input(r.below(s.inputs)),
                2 if !s.cells.is_empty() => return Expr::Local(s.cells[r.below(s.cells.len())]),
                3 if !s.ups.is_empty() => {
                    let d = r.below(s.ups.len());
                    if !s.ups[d].is_empty() {
                        return Expr::Up(d + 1, s.ups[d][r.below(s.ups[d].len())]);
                    }
                }
                4 if s.is_item => return Expr::ItemValue,
                5 if s.is_item => return Expr::ItemIndex,
                _ => {}
            }
        }
    }
    match r.below(5) {
        0 | 1 => Expr::Add(Box::new(number(r, s, depth - 1)), Box::new(number(r, s, depth - 1))),
        2 => Expr::Mul(Box::new(number(r, s, depth - 1)), Box::new(Expr::Const(r.below(3) as f64))),
        3 => Expr::Sum(Box::new(list(r, s, depth - 1))),
        _ if !s.root_cells.is_empty() => Expr::Deref(Box::new(number(r, s, depth - 1)), Rc::new(s.root_cells.clone())),
        _ => Expr::Add(Box::new(number(r, s, depth - 1)), Box::new(Expr::Const(1.0))),
    }
}

fn list(r: &mut Rng, s: &Scope, depth: usize) -> Expr {
    if !s.collections.is_empty() && r.chance(2) {
        let (k, cells) = &s.collections[r.below(s.collections.len())];
        if !cells.is_empty() {
            return Expr::Collect(*k, cells[r.below(cells.len())]);
        }
    }
    let step = Expr::Const([1.0, 1.0, 2.0][r.below(3)]);
    Expr::Ticks(Box::new(number(r, s, depth)), Box::new(number(r, s, depth)), Box::new(step))
}

fn template(r: &mut Rng, inputs: usize, ups: &[Vec<usize>], root_cells: &[usize], levels: usize) -> (Template, Vec<usize>) {
    let is_root = ups.is_empty() && levels == 2;
    let mut s = Scope { inputs, cells: Vec::new(), collections: Vec::new(), ups: ups.to_vec(), is_item: !is_root, root_cells: root_cells.to_vec() };
    let mut entries = Vec::new();
    for i in 0..2 + r.below(4) {
        if levels > 0 && r.chance(3) {
            let each = if r.chance(4) { number(r, &s, 1) } else { list(r, &s, 1) };
            let mut inner_ups = vec![s.cells.clone()];
            inner_ups.extend(ups.iter().cloned());
            let (inner, cells) = template(r, inputs, &inner_ups, &s.root_cells, levels - 1);
            entries.push(Entry::Collection { each, template: Rc::new(inner) });
            s.collections.push((i, cells));
        } else {
            entries.push(Entry::Cell(number(r, &s, 2)));
            s.cells.push(i);
            if is_root {
                s.root_cells.push(i);
            }
        }
    }
    (Template { entries }, s.cells)
}

fn program(seed: u64) -> (Rc<Template>, Vec<Value>, Rng) {
    let mut r = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
    let inputs = 3 + r.below(3);
    let (t, _) = template(&mut r, inputs, &[], &[], 2);
    let values = (0..inputs).map(|_| n(r.below(7) as f64 - 1.0)).collect();
    (Rc::new(t), values, r)
}

/// Everything observed: after every change the engine equals the reference.
#[test]
fn random_programs_agree_with_the_reference() {
    let (mut retries, mut made, mut disposed) = (0, 0, 0);
    for seed in 1..=400 {
        let (t, mut inputs, mut r) = program(seed);
        let mut e = Engine::new(t.clone(), &inputs);
        e.observe(e.root());
        e.stabilise();
        e.check();
        assert_eq!(e.values(false), reference(&t, &inputs), "seed {seed}, at the start");
        for step in 0..40 {
            for _ in 0..1 + r.below(2) {
                let i = r.below(inputs.len());
                inputs[i] = n(r.below(9) as f64 - 2.0 + if r.chance(4) { 0.5 } else { 0.0 });
                e.set_input(i, inputs[i].clone());
            }
            e.stabilise();
            e.check();
            assert_eq!(e.values(false), reference(&t, &inputs), "seed {seed}, step {step}");
        }
        retries += e.retries;
        made += e.made;
        disposed += e.disposed;
    }
    // The programs must reach the hard paths, or agreement proves little.
    assert!(retries > 100 && made > 10_000 && disposed > 5_000, "retries {retries}, made {made}, disposed {disposed}");
}

/// The consistency property (spec §8.8): observe any changing subset while
/// the inputs move; what is necessary is always right, and observing
/// everything afterwards gives what observing everything throughout would.
#[test]
fn random_observation_is_consistent() {
    for seed in 1..=400 {
        let (t, mut inputs, mut r) = program(seed + 10_000);
        let mut e = Engine::new(t.clone(), &inputs);
        let mut watching: Vec<usize> = Vec::new();
        for step in 0..40 {
            match r.below(4) {
                0 => {
                    let owners = e.live_owners();
                    let o = owners[r.below(owners.len())];
                    e.observe(o);
                    watching.push(o);
                }
                1 if !watching.is_empty() => {
                    let o = watching.swap_remove(r.below(watching.len()));
                    e.unobserve(o);
                }
                _ => {
                    let i = r.below(inputs.len());
                    inputs[i] = n(r.below(9) as f64 - 2.0);
                    e.set_input(i, inputs[i].clone());
                }
            }
            // An owner disposed while watched took its observers with it.
            watching.retain(|o| e.is_alive(*o));
            e.stabilise();
            e.check();
            let want = reference(&t, &inputs);
            for (key, value) in e.values(true) {
                assert_eq!(Some(&value), want.get(&key), "seed {seed}, step {step}, {key:?}");
            }
        }
        e.observe(e.root());
        e.stabilise();
        e.check();
        assert_eq!(e.values(false), reference(&t, &inputs), "seed {seed}, observing everything at the end");
    }
}
