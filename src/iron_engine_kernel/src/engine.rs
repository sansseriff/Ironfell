//! The engine: the same values as the reference, computed by doing only
//! what changed and only what is observed.
//!
//! Read with plans/specs/reactive-engine.md §6 (evaluation) and §7
//! (structure). The rules, each marked where it is implemented:
//!
//! R1  A cell is *necessary* when an observer needs it or a necessary cell
//!     reads it. Only necessary cells are ever evaluated (§6.1).
//! R2  A change marks the necessary readers of what changed, nothing else.
//!     An unnecessary cell is not touched; when it becomes necessary it is
//!     queued and compared with its dependencies by stamp (§6.1, "pull").
//! R3  One pass takes queued cells lowest height first. A cell's height is
//!     above every dependency's, so each runs once, after what it reads
//!     (§6.2).
//! R4  A Collection's items are made and disposed by its Structure cell
//!     *inside* the pass. What it makes sits above it (its *anchor*), so
//!     new cells run later in the same pass (§6.2, §7.1).
//! R5  A read found only by running (a tracked read, or a read into an item
//!     that has just come to exist) adds an edge and raises the reader above
//!     the dependency. If anything below the reader's new height has not
//!     run yet, the reader is queued again and runs after it: the
//!     dependency itself may be queued, or it may be current for now and
//!     read something that is (§6.2, §6.3).
//! R6  Raising a reader that reaches the dependency it is being raised above
//!     would close a cycle. The edge is not added and heights are left as
//!     they were. That is not yet a failure: the cycle may run through an
//!     edge that a cell still to run in this pass is about to drop. So the
//!     reader *waits*: it does not finish, it holds what it could not read
//!     necessary (a *want*), and it comes round again when the queue is
//!     empty. Everything that reads a waiting cell, directly or through
//!     other cells, is *unsettled*, and a cell that reads a waiting or
//!     unsettled cell waits too. (An unsettled cell that is queued still
//!     runs: it may be the one that drops the edge.) Only when a whole
//!     round of waiting cells has run and none finished is the cycle real;
//!     then the lowest of them fails, and the rest follow from that. A
//!     failed cell keeps its want and is run again in any pass that removes
//!     an edge, since it has no edge of its own to tell it the cycle ended.
//!     What read its failure may then run a second time in that pass: the
//!     one case of a cell finishing twice (§6.3).
//! R7  Equality cutoff: a cell whose value did not change marks no reader
//!     (§6.5).
//! R8  Disposal removes an item's cells from their dependencies' readers,
//!     deepest first (§7.3).

use crate::model::*;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

pub type CellId = usize;
pub type OwnerId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Input,
    Binding,
    Structure,
    ItemValue,
    ItemIndex,
}

struct Cell {
    owner: OwnerId,
    entry: usize,
    kind: Kind,
    /// Cells this one reads. Static ones are installed when it is made;
    /// tracked ones are found by running and replaced on each evaluation.
    deps: Vec<CellId>,
    readers: Vec<CellId>,
    /// The Structure cell that made this cell's owner. It orders heights and
    /// carries necessity, but is not a value dependency: a kept item is not
    /// re-evaluated because another item came or went.
    anchor: Option<CellId>,
    anchored: Vec<CellId>,
    height: u32,
    observed: bool,
    /// Necessary readers and necessary anchored cells.
    need: u32,
    value: Slot,
    changed_at: u64,
    /// 0 until first evaluated.
    computed_at: u64,
    /// Queued again by R5: evaluate whatever the stamps say.
    forced: bool,
    /// R6: what it could not read without closing a cycle, held necessary
    /// like a dependency, and the cells that hold this one so.
    wants: Vec<CellId>,
    wanted_by: Vec<CellId>,
    /// R6: it has not finished in this pass, and comes round again when
    /// the queue is empty.
    waiting: bool,
    /// R6: it reads a waiting cell, directly or through others, so its
    /// value may yet change in this pass though it is not queued.
    unsettled: bool,
    /// The pass in which it last finished an evaluation.
    finished_in: u64,
    queued: bool,
    alive: bool,
}

struct Owner {
    parent: Option<OwnerId>,
    template: Rc<Template>,
    path: Path,
    /// One cell per template entry: a Binding, or a Collection's Structure.
    entries: Vec<CellId>,
    /// Per Collection entry, its items now, in order.
    items: BTreeMap<usize, Vec<(Key, OwnerId)>>,
    item_value: Option<CellId>,
    item_index: Option<CellId>,
    /// Observers on this owner or an owner above it.
    observers: u32,
    alive: bool,
}

pub struct Engine {
    cells: Vec<Cell>,
    owners: Vec<Owner>,
    inputs: Vec<CellId>,
    queue: BTreeSet<(u32, CellId)>,
    tick: u64,
    /// Bindings and Structure cells evaluated, since the engine was made.
    pub evaluations: u64,
    /// Evaluations given up and queued again by R5 (counted in the above).
    pub retries: u64,
    /// Cells that finished twice in one pass. Always 0: a cell that runs
    /// after everything it reads has nothing to run again for. Without R5's
    /// "queue again" a reader would finish on a value its dependency is
    /// about to replace, and a Structure cell fed by it would make and
    /// dispose items (and their state) for a value that never was.
    pub glitches: u64,
    /// Cells that finished twice in a pass in which a cycle ended. A cell
    /// that failed on a cycle sits below the cell whose edge ends it, so
    /// what read the failure may have finished before it is withdrawn, and
    /// runs again. The failure was the true value of the pass before, not
    /// one that never was; the cost is the extra evaluations (R6).
    pub reruns: u64,
    recovering: bool,
    pass: u64,
    /// R6: cells waiting, and whether each waits on a cycle of its own
    /// (true) or on another waiting cell.
    waiting: Vec<(CellId, bool)>,
    unsettled: Vec<CellId>,
    /// Evaluations finished, and the count when waiting cells were last
    /// let go: equal at the next time means a round with no progress.
    finishes: u64,
    released_at: Option<u64>,
    /// The one waiting cell whose cycle is taken as real this round.
    last_word: Option<CellId>,
    /// Cells that finished failed on a cycle.
    blocked: BTreeSet<CellId>,
    edge_removed: bool,
    /// Item owners made and disposed.
    pub made: u64,
    pub disposed: u64,
}

/// One evaluation's reads (R5).
struct Run<'a> {
    engine: &'a mut Engine,
    cell: CellId,
    owner: OwnerId,
    seen: Vec<CellId>,
    retry: bool,
    /// Something it read is waiting (R6).
    wait: bool,
    /// What it could not read without closing a cycle (R6).
    cycle: Vec<CellId>,
}

impl Run<'_> {
    fn read(&mut self, dep: CellId) -> Slot {
        if !self.engine.cells[self.cell].deps.contains(&dep) && !self.engine.add_edge(dep, self.cell) {
            if !self.cycle.contains(&dep) {
                self.cycle.push(dep);
            }
            return Err("a cycle".into());
        }
        if self.engine.cells[dep].queued {
            self.retry = true;
        }
        if self.engine.cells[dep].waiting || self.engine.cells[dep].unsettled {
            self.wait = true;
        }
        if !self.seen.contains(&dep) {
            self.seen.push(dep);
        }
        self.engine.cells[dep].value.clone()
    }
}

impl Reads for Run<'_> {
    fn input(&mut self, i: usize) -> Slot {
        let c = self.engine.inputs[i];
        self.read(c)
    }
    fn local(&mut self, k: usize) -> Slot {
        let c = self.engine.owners[self.owner].entries[k];
        self.read(c)
    }
    fn up(&mut self, d: usize, k: usize) -> Slot {
        let o = self.engine.ancestor(self.owner, d);
        let c = self.engine.owners[o].entries[k];
        self.read(c)
    }
    fn item_value(&mut self) -> Slot {
        let c = self.engine.owners[self.owner].item_value.expect("inside an item");
        self.read(c)
    }
    fn item_index(&mut self) -> Slot {
        let c = self.engine.owners[self.owner].item_index.expect("inside an item");
        self.read(c)
    }
    fn collect(&mut self, k: usize, l: usize) -> Vec<Slot> {
        // The Structure cell first: if it has not run, the items are not
        // yet what they will be, and this reader runs again after it.
        let structure = self.engine.owners[self.owner].entries[k];
        let _ = self.read(structure);
        let items: Vec<OwnerId> =
            self.engine.owners[self.owner].items.get(&k).map(|v| v.iter().map(|(_, o)| *o).collect()).unwrap_or_default();
        items
            .into_iter()
            .map(|o| {
                let c = self.engine.owners[o].entries[l];
                self.read(c)
            })
            .collect()
    }
    fn root(&mut self, entry: usize) -> Slot {
        let c = self.engine.owners[0].entries[entry];
        self.read(c)
    }
}

impl Engine {
    pub fn new(template: Rc<Template>, inputs: &[Value]) -> Self {
        let mut e = Engine {
            cells: Vec::new(),
            owners: Vec::new(),
            inputs: Vec::new(),
            queue: BTreeSet::new(),
            tick: 1,
            evaluations: 0,
            retries: 0,
            glitches: 0,
            reruns: 0,
            recovering: false,
            pass: 0,
            waiting: Vec::new(),
            unsettled: Vec::new(),
            finishes: 0,
            released_at: None,
            last_word: None,
            blocked: BTreeSet::new(),
            edge_removed: false,
            made: 0,
            disposed: 0,
        };
        e.owners.push(Owner {
            parent: None,
            template: template.clone(),
            path: Vec::new(),
            entries: Vec::new(),
            items: BTreeMap::new(),
            item_value: None,
            item_index: None,
            observers: 0,
            alive: true,
        });
        for v in inputs {
            let c = e.new_cell(0, usize::MAX, Kind::Input, Vec::new(), None);
            e.cells[c].value = Ok(v.clone());
            e.cells[c].changed_at = e.tick;
            e.cells[c].computed_at = e.tick;
            e.inputs.push(c);
        }
        e.instantiate(0);
        e
    }

    // ---- necessity (R1) ----

    fn necessary(&self, c: CellId) -> bool {
        self.cells[c].observed || self.cells[c].need > 0
    }

    fn below(&self, c: CellId) -> Vec<CellId> {
        let mut out = self.cells[c].deps.clone();
        out.extend(self.cells[c].anchor);
        out.extend(self.cells[c].wants.iter().copied());
        out
    }

    fn became_necessary(&mut self, c: CellId) {
        for d in self.below(c) {
            self.need_more(d);
        }
        // R2: queued to be compared with its dependencies by stamp.
        self.push(c);
    }

    fn became_unnecessary(&mut self, c: CellId) {
        for d in self.below(c) {
            self.need_less(d);
        }
        self.unqueue(c);
    }

    fn need_more(&mut self, c: CellId) {
        let was = self.necessary(c);
        self.cells[c].need += 1;
        if !was {
            self.became_necessary(c);
        }
    }

    fn need_less(&mut self, c: CellId) {
        self.cells[c].need -= 1;
        if !self.necessary(c) {
            self.became_unnecessary(c);
        }
    }

    fn set_observed(&mut self, c: CellId, on: bool) {
        let was = self.necessary(c);
        self.cells[c].observed = on;
        match (was, self.necessary(c)) {
            (false, true) => self.became_necessary(c),
            (true, false) => self.became_unnecessary(c),
            _ => {}
        }
    }

    // ---- the queue ----

    fn push(&mut self, c: CellId) {
        if self.cells[c].alive && !self.cells[c].queued && self.necessary(c) {
            self.cells[c].queued = true;
            self.queue.insert((self.cells[c].height, c));
        }
    }

    fn unqueue(&mut self, c: CellId) {
        if self.cells[c].queued {
            self.cells[c].queued = false;
            self.queue.remove(&(self.cells[c].height, c));
        }
    }

    /// R2 and R7's other half: the readers that must look again.
    fn push_readers(&mut self, c: CellId) {
        for r in self.cells[c].readers.clone() {
            self.push(r);
        }
    }

    // ---- heights and edges (R3, R5, R6) ----

    fn set_height(&mut self, c: CellId, height: u32) {
        let queued = self.cells[c].queued;
        self.unqueue(c);
        self.cells[c].height = height;
        if queued {
            self.push(c);
        }
    }

    /// Raise `c` to at least `height`, and what sits above it. `Err` when
    /// that reaches `origin`: the edge being added closes a cycle. `raised`
    /// takes each cell moved and the height it had.
    fn raise(&mut self, c: CellId, height: u32, origin: CellId, raised: &mut Vec<(CellId, u32)>) -> Result<(), ()> {
        if c == origin {
            return Err(());
        }
        if self.cells[c].height >= height {
            return Ok(());
        }
        raised.push((c, self.cells[c].height));
        self.set_height(c, height);
        let mut above = self.cells[c].readers.clone();
        above.extend(self.cells[c].anchored.iter().copied());
        for r in above {
            self.raise(r, height + 1, origin, raised)?;
        }
        Ok(())
    }

    /// `reader` now reads `dep`. False when that would close a cycle (R6);
    /// then nothing has changed.
    fn add_edge(&mut self, dep: CellId, reader: CellId) -> bool {
        if dep == reader {
            return false;
        }
        let mut raised = Vec::new();
        if self.raise(reader, self.cells[dep].height + 1, dep, &mut raised).is_err() {
            for (c, height) in raised.into_iter().rev() {
                self.set_height(c, height);
            }
            return false;
        }
        self.cells[reader].deps.push(dep);
        self.cells[dep].readers.push(reader);
        if self.necessary(reader) {
            self.need_more(dep);
        }
        true
    }

    fn remove_edge(&mut self, dep: CellId, reader: CellId) {
        self.cells[reader].deps.retain(|d| *d != dep);
        self.cells[dep].readers.retain(|r| *r != reader);
        self.edge_removed = true;
        if self.necessary(reader) {
            self.need_less(dep);
        }
    }

    /// R6: what `c` could not read is held necessary, so it is brought up
    /// to date (and drops an edge it no longer needs) though nothing reads
    /// it. New wants are taken before old ones are let go.
    fn set_wants(&mut self, c: CellId, wants: Vec<CellId>) {
        let old = std::mem::take(&mut self.cells[c].wants);
        let necessary = self.necessary(c);
        for d in &wants {
            if !old.contains(d) {
                self.cells[*d].wanted_by.push(c);
                if necessary {
                    self.need_more(*d);
                }
            }
        }
        for d in &old {
            if !wants.contains(d) {
                self.cells[*d].wanted_by.retain(|r| *r != c);
                if necessary {
                    self.need_less(*d);
                }
            }
        }
        self.cells[c].wants = wants;
    }

    // ---- making owners and cells (R4) ----

    fn ancestor(&self, mut o: OwnerId, d: usize) -> OwnerId {
        for _ in 0..d {
            o = self.owners[o].parent.expect("an enclosing owner");
        }
        o
    }

    fn new_cell(&mut self, owner: OwnerId, entry: usize, kind: Kind, deps: Vec<CellId>, anchor: Option<CellId>) -> CellId {
        let c = self.cells.len();
        let height = deps.iter().chain(anchor.iter()).map(|d| self.cells[*d].height + 1).max().unwrap_or(0);
        for d in &deps {
            self.cells[*d].readers.push(c);
        }
        if let Some(a) = anchor {
            self.cells[a].anchored.push(c);
        }
        self.cells.push(Cell {
            owner,
            entry,
            kind,
            deps,
            readers: Vec::new(),
            anchor,
            anchored: Vec::new(),
            height,
            observed: false,
            need: 0,
            value: Err("not computed".into()),
            changed_at: 0,
            computed_at: 0,
            forced: false,
            wants: Vec::new(),
            wanted_by: Vec::new(),
            waiting: false,
            unsettled: false,
            finished_in: 0,
            queued: false,
            alive: true,
        });
        if self.owners[owner].observers > 0 {
            self.set_observed(c, true);
        }
        c
    }

    /// The reads an expression makes whatever its values: installed when the
    /// cell is made, so the first pass needs no retry for them.
    fn static_deps(&self, e: &Expr, owner: OwnerId, out: &mut Vec<CellId>) {
        let mut add = |c: CellId| {
            if !out.contains(&c) {
                out.push(c);
            }
        };
        match e {
            Expr::Const(_) => {}
            Expr::Input(i) => add(self.inputs[*i]),
            Expr::Local(k) | Expr::Collect(k, _) => add(self.owners[owner].entries[*k]),
            Expr::Up(d, k) => add(self.owners[self.ancestor(owner, *d)].entries[*k]),
            Expr::ItemValue => add(self.owners[owner].item_value.expect("inside an item")),
            Expr::ItemIndex => add(self.owners[owner].item_index.expect("inside an item")),
            Expr::Add(a, b) | Expr::Mul(a, b) => {
                self.static_deps(a, owner, out);
                self.static_deps(b, owner, out);
            }
            Expr::Ticks(a, b, c) => {
                self.static_deps(a, owner, out);
                self.static_deps(b, owner, out);
                self.static_deps(c, owner, out);
            }
            Expr::Sum(a) | Expr::Deref(a, _) => self.static_deps(a, owner, out),
        }
    }

    /// Make an owner's cells from its template, in entry order.
    fn instantiate(&mut self, owner: OwnerId) {
        let template = self.owners[owner].template.clone();
        let anchor = self.owners[owner].item_value.and_then(|c| self.cells[c].anchor);
        for (i, entry) in template.entries.iter().enumerate() {
            let (kind, expr) = match entry {
                Entry::Cell(e) => (Kind::Binding, e),
                Entry::Collection { each, .. } => (Kind::Structure, each),
            };
            let mut deps = Vec::new();
            self.static_deps(expr, owner, &mut deps);
            let c = self.new_cell(owner, i, kind, deps, anchor);
            self.owners[owner].entries.push(c);
        }
    }

    fn make_item(&mut self, parent: OwnerId, entry: usize, structure: CellId, key: Key, template: Rc<Template>) -> OwnerId {
        let o = self.owners.len();
        let mut path = self.owners[parent].path.clone();
        path.push((entry, key));
        self.owners.push(Owner {
            parent: Some(parent),
            template,
            path,
            entries: Vec::new(),
            items: BTreeMap::new(),
            item_value: None,
            item_index: None,
            observers: self.owners[parent].observers,
            alive: true,
        });
        let value = self.new_cell(o, usize::MAX, Kind::ItemValue, Vec::new(), Some(structure));
        let index = self.new_cell(o, usize::MAX, Kind::ItemIndex, Vec::new(), Some(structure));
        self.owners[o].item_value = Some(value);
        self.owners[o].item_index = Some(index);
        self.instantiate(o);
        self.made += 1;
        o
    }

    // ---- disposal (R8) ----

    fn dispose(&mut self, o: OwnerId) {
        let children: Vec<OwnerId> = self.owners[o].items.values().flatten().map(|(_, c)| *c).collect();
        for c in children {
            self.dispose(c);
        }
        // Readers before what they read: entries in reverse, then the item cells.
        let mut cells: Vec<CellId> = self.owners[o].entries.iter().rev().copied().collect();
        cells.extend(self.owners[o].item_index);
        cells.extend(self.owners[o].item_value);
        for c in cells {
            self.unqueue(c);
            if self.necessary(c) {
                for d in self.below(c) {
                    self.need_less(d);
                }
            }
            for d in self.cells[c].deps.clone() {
                self.cells[d].readers.retain(|r| *r != c);
            }
            if let Some(a) = self.cells[c].anchor {
                self.cells[a].anchored.retain(|r| *r != c);
            }
            // A reader outside the item (a `Collect`) loses this dependency;
            // the Structure cell's change runs it again.
            for r in self.cells[c].readers.clone() {
                self.cells[r].deps.retain(|d| *d != c);
            }
            for d in self.cells[c].wants.clone() {
                self.cells[d].wanted_by.retain(|r| *r != c);
            }
            for r in self.cells[c].wanted_by.clone() {
                self.cells[r].wants.retain(|d| *d != c);
            }
            self.blocked.remove(&c);
            self.edge_removed = true;
            let cell = &mut self.cells[c];
            cell.alive = false;
            cell.observed = false;
            cell.need = 0;
            cell.deps.clear();
            cell.readers.clear();
            cell.wants.clear();
            cell.wanted_by.clear();
            cell.waiting = false;
        }
        self.owners[o].alive = false;
        self.owners[o].items.clear();
        self.disposed += 1;
    }

    // ---- the pass (R3) ----

    fn stale(&self, c: CellId) -> bool {
        let cell = &self.cells[c];
        cell.forced || cell.computed_at == 0 || cell.deps.iter().any(|d| self.cells[*d].changed_at > cell.computed_at)
    }

    /// Bring every necessary cell up to date.
    pub fn stabilise(&mut self) {
        self.pass += 1;
        self.released_at = None;
        self.recovering = false;
        let mut budget = 200_000 + 100 * self.cells.len();
        loop {
            while let Some((_, c)) = self.queue.pop_first() {
                self.cells[c].queued = false;
                if !self.cells[c].alive || !self.necessary(c) || !self.stale(c) {
                    continue;
                }
                budget -= 1;
                if budget == 0 {
                    panic!("stabilise did not settle");
                }
                match self.cells[c].kind {
                    Kind::Binding | Kind::Structure => self.evaluate(c),
                    Kind::Input | Kind::ItemValue | Kind::ItemIndex => {}
                }
            }
            if !self.waiting.is_empty() {
                self.release();
            } else if self.edge_removed && !self.blocked.is_empty() {
                // R6: an edge went, so a cycle may have ended. The cells
                // that failed on one look again.
                self.edge_removed = false;
                self.recovering = true;
                for c in self.blocked.clone() {
                    self.cells[c].forced = true;
                    self.push(c);
                }
            } else {
                break;
            }
        }
        self.edge_removed = false;
    }

    /// R6: the queue is empty, so every cell that could have dropped an
    /// edge has run. The waiting cells come round again. If the last round
    /// of them finished nothing, their cycles are real, and the lowest cell
    /// waiting on one is the one to fail.
    fn release(&mut self) {
        let waiting = std::mem::take(&mut self.waiting);
        self.last_word = None;
        if self.released_at == Some(self.finishes) {
            self.last_word = waiting
                .iter()
                .filter(|(c, on_cycle)| *on_cycle && self.cells[*c].alive && self.necessary(*c))
                .map(|(c, _)| (self.cells[*c].height, *c))
                .min()
                .map(|(_, c)| c);
        }
        self.released_at = Some(self.finishes);
        for c in std::mem::take(&mut self.unsettled) {
            self.cells[c].unsettled = false;
        }
        for (c, _) in waiting {
            self.cells[c].waiting = false;
            self.push(c);
        }
    }

    fn wait(&mut self, c: CellId, on_cycle: bool) {
        self.cells[c].forced = true;
        self.cells[c].waiting = true;
        self.waiting.push((c, on_cycle));
        let mut above = vec![c];
        while let Some(c) = above.pop() {
            for r in self.cells[c].readers.iter().chain(self.cells[c].anchored.iter()).copied().collect::<Vec<_>>() {
                if !self.cells[r].unsettled {
                    self.cells[r].unsettled = true;
                    self.unsettled.push(r);
                    above.push(r);
                }
            }
        }
    }

    fn evaluate(&mut self, c: CellId) {
        let owner = self.cells[c].owner;
        let entry = self.cells[c].entry;
        let template = self.owners[owner].template.clone();
        let expr = match &template.entries[entry] {
            Entry::Cell(e) => e,
            Entry::Collection { each, .. } => each,
        };
        self.evaluations += 1;
        let last_word = self.last_word == Some(c);
        if last_word {
            self.last_word = None;
        }
        let started_at = self.cells[c].height;
        let mut run = Run { engine: self, cell: c, owner, seen: Vec::new(), retry: false, wait: false, cycle: Vec::new() };
        let mut value = eval(expr, &mut run);
        let (seen, mut retry, wait, cycle) = (run.seen, run.retry, run.wait, run.cycle);
        let height = self.cells[c].height;
        if height > started_at && self.queue.first().is_some_and(|(lowest, _)| *lowest < height) {
            retry = true;
        }
        let failed = !cycle.is_empty();
        self.set_wants(c, cycle);
        if failed && !last_word {
            // R6: perhaps a cycle, perhaps an edge about to go.
            self.wait(c, true);
            return;
        }
        if failed {
            value = Err("a cycle".into());
        } else if retry {
            // R5: something it read has not run yet. It is above that now,
            // so it comes round again after it.
            self.retries += 1;
            self.cells[c].forced = true;
            self.push(c);
            return;
        } else if wait {
            self.wait(c, false);
            return;
        }
        self.cells[c].forced = false;
        self.cells[c].unsettled = false;
        self.blocked.remove(&c);
        if failed {
            self.blocked.insert(c);
        }
        if self.cells[c].finished_in == self.pass {
            if self.recovering {
                self.reruns += 1;
            } else {
                self.glitches += 1;
            }
        }
        self.cells[c].finished_in = self.pass;
        self.finishes += 1;
        for d in self.cells[c].deps.clone() {
            if !seen.contains(&d) {
                self.remove_edge(d, c);
            }
        }
        self.tick += 1;
        self.cells[c].computed_at = self.tick;
        if self.cells[c].kind == Kind::Structure {
            self.reconcile(c, owner, entry, &value);
            return;
        }
        // R7
        if self.cells[c].value != value {
            self.cells[c].value = value;
            self.cells[c].changed_at = self.tick;
            self.push_readers(c);
        }
    }

    fn write(&mut self, c: CellId, value: Value) {
        if self.cells[c].value.as_ref() != Ok(&value) {
            self.tick += 1;
            self.cells[c].value = Ok(value);
            self.cells[c].changed_at = self.tick;
            self.cells[c].computed_at = self.tick;
            self.push_readers(c);
        }
    }

    /// R4: one Collection's items brought to what `each` now says. Nothing
    /// outside the Collection is visited (§7.1).
    fn reconcile(&mut self, structure: CellId, owner: OwnerId, entry: usize, each: &Slot) {
        let template = match &self.owners[owner].template.entries[entry] {
            Entry::Collection { template, .. } => template.clone(),
            Entry::Cell(_) => unreachable!("a Structure cell belongs to a Collection"),
        };
        let wanted = items_of(each);
        let old = self.owners[owner].items.remove(&entry).unwrap_or_default();
        let same_keys = old.len() == wanted.len() && old.iter().zip(&wanted).all(|((a, _), (b, _))| a == b);
        let mut kept: BTreeMap<Key, OwnerId> = old.into_iter().collect();
        let mut now = Vec::new();
        for (index, (key, value)) in wanted.into_iter().enumerate() {
            let item = match kept.remove(&key) {
                Some(o) => o,
                None => self.make_item(owner, entry, structure, key, template.clone()),
            };
            let (v, i) = (self.owners[item].item_value.expect("an item"), self.owners[item].item_index.expect("an item"));
            self.write(v, value);
            self.write(i, Value::Num(index as f64));
            now.push((key, item));
        }
        for (_, gone) in kept {
            self.dispose(gone);
        }
        self.owners[owner].items.insert(entry, now);
        self.cells[structure].value = each.clone();
        if !same_keys {
            self.cells[structure].changed_at = self.tick;
            self.push_readers(structure);
        }
    }

    // ---- the outside ----

    pub fn set_input(&mut self, i: usize, value: Value) {
        let c = self.inputs[i];
        self.write(c, value);
    }

    fn subtree(&self, o: OwnerId, out: &mut Vec<OwnerId>) {
        out.push(o);
        for (_, c) in self.owners[o].items.values().flatten() {
            self.subtree(*c, out);
        }
    }

    fn observe_by(&mut self, o: OwnerId, on: bool) {
        if !self.owners[o].alive {
            return;
        }
        let mut owners = Vec::new();
        self.subtree(o, &mut owners);
        for o in owners {
            let before = self.owners[o].observers;
            self.owners[o].observers = if on { before + 1 } else { before - 1 };
            if (before == 0) != (self.owners[o].observers == 0) {
                let mut cells = self.owners[o].entries.clone();
                cells.extend(self.owners[o].item_value);
                cells.extend(self.owners[o].item_index);
                for c in cells {
                    self.set_observed(c, on);
                }
            }
        }
    }

    /// Observe an owner and everything it comes to hold: a view's region.
    pub fn observe(&mut self, o: OwnerId) {
        self.observe_by(o, true);
    }

    /// Undo one `observe`. An owner disposed meanwhile took its observers
    /// with it.
    pub fn unobserve(&mut self, o: OwnerId) {
        if self.owners[o].alive && self.owners[o].observers > 0 {
            self.observe_by(o, false);
        }
    }

    pub fn root(&self) -> OwnerId {
        0
    }

    pub fn live_owners(&self) -> Vec<OwnerId> {
        (0..self.owners.len()).filter(|o| self.owners[*o].alive).collect()
    }

    pub fn owner_at(&self, path: &Path) -> Option<OwnerId> {
        (0..self.owners.len()).find(|o| self.owners[*o].alive && &self.owners[*o].path == path)
    }

    pub fn observers(&self, o: OwnerId) -> u32 {
        self.owners[o].observers
    }

    pub fn is_alive(&self, o: OwnerId) -> bool {
        self.owners[o].alive
    }

    /// Every binding's value, keyed as the reference keys them; only the
    /// necessary ones when `necessary_only`.
    pub fn values(&self, necessary_only: bool) -> BTreeMap<(Path, usize), Slot> {
        let mut out = BTreeMap::new();
        for (c, cell) in self.cells.iter().enumerate() {
            if cell.alive && cell.kind == Kind::Binding && (!necessary_only || self.necessary(c)) {
                out.insert((self.owners[cell.owner].path.clone(), cell.entry), cell.value.clone());
            }
        }
        out
    }

    /// What must hold after `stabilise`. Recomputes necessity from scratch
    /// and checks heights, the queue and currency. Panics with the reason.
    pub fn check(&self) {
        assert!(self.queue.is_empty(), "the queue is not empty after stabilise");
        assert!(self.waiting.is_empty(), "a cell is still waiting after stabilise");
        assert_eq!(self.glitches, 0, "a cell finished twice in one pass");
        for (c, cell) in self.cells.iter().enumerate() {
            if !cell.alive {
                continue;
            }
            assert!(self.owners[cell.owner].alive, "cell {c} outlived its owner");
            for d in cell.deps.iter().chain(cell.anchor.iter()) {
                assert!(self.cells[*d].alive, "cell {c} reads a disposed cell {d}");
                assert!(self.cells[*d].height < cell.height, "cell {c} is not above {d}");
            }
            for d in &cell.deps {
                assert!(self.cells[*d].readers.contains(&c), "cell {d} does not list its reader {c}");
            }
            let above = cell.readers.iter().chain(cell.anchored.iter()).chain(cell.wanted_by.iter());
            let need = above.filter(|r| self.cells[**r].alive && self.necessary(**r)).count();
            assert!(!cell.waiting && !cell.unsettled, "cell {c} is still waiting");
            assert_eq!(cell.wants.is_empty(), !self.blocked.contains(&c), "cell {c}: wants and failure disagree");
            assert_eq!(cell.need as usize, need, "cell {c}: necessary readers miscounted");
            if cell.kind != Kind::Input {
                assert_eq!(cell.observed, self.owners[cell.owner].observers > 0, "cell {c}: observed disagrees with its owner");
            }
            if self.necessary(c) && matches!(cell.kind, Kind::Binding | Kind::Structure) {
                assert!(!self.stale(c), "necessary cell {c} is not current");
            }
        }
    }
}
