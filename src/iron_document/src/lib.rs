//! `iron_document`: the authored store.
//!
//! One identity-stable, renderer-neutral document holds all authored intent.
//! Everything downstream is derived and rebuildable. This crate is that
//! document: nodes with stable ids, typed components, first-class relations,
//! an operation log with computed inverses, undo/redo, canonical JSON, and
//! elided views. It depends on nothing renderer-shaped and tests natively.
//!
//! The design record is `plans/document-spine.md`.
//!
//! ```text
//!   TransactionInput ──apply──▶ Store ──▶ Transaction (with inverse) ──▶ log
//!                                  │
//!                                  ├── document(): reads, indexes, leaves
//!                                  ├── view(): elided tree for a reader
//!                                  └── save()/load(): canonical JSON
//! ```

pub mod component;
pub mod document;
pub mod expr;
pub mod graph;
pub mod id;
pub mod op;
pub mod order;
pub mod registry;
pub mod serial;
pub mod value;
pub mod view;

pub use component::{Component, ComponentKind, LeafError, LeafPath, LeafStruct};
pub use document::{Document, Node, RelKind, Relation};
pub use expr::{Expr, Leaf};
pub use graph::ResolvedChange;
pub use id::{NodeId, RelationId, Version};
pub use op::{Actor, ApplyError, ErrorKind, Op, Transaction, TransactionInput};
pub use order::OrderKey;
pub use registry::{ChildPolicy, Host, TypeId, TypeSpec};
pub use serial::LoadError;
pub use value::{Slot, Value, ValueKind};
pub use view::{Fidelity, ViewError, ViewQuery};

use std::collections::VecDeque;

/// Result of a successful apply.
#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
    pub version: Version,
    /// True when an idempotency key matched an earlier transaction and nothing
    /// was applied.
    pub replayed: bool,
}

/// Undo/redo stacks plus the log of everything applied, in order.
#[derive(Clone, Debug, Default)]
pub struct History {
    past: Vec<Transaction>,
    future: Vec<Transaction>,
    log: VecDeque<Transaction>,
    horizon: usize,
}

impl History {
    pub fn past(&self) -> &[Transaction] {
        &self.past
    }

    pub fn future(&self) -> &[Transaction] {
        &self.future
    }

    /// Every applied transaction, including undo and redo ones, oldest first.
    /// Bounded by the horizon; the append-only file is the durable log.
    pub fn log(&self) -> impl Iterator<Item = &Transaction> {
        self.log.iter()
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    fn record(&mut self, tx: Transaction) {
        self.log.push_back(tx);
        while self.log.len() > self.horizon {
            self.log.pop_front();
        }
    }
}

pub struct Store {
    doc: Document,
    history: History,
    recent_keys: VecDeque<(String, Version)>,
    reactive: graph::Reactive,
}

impl Default for Store {
    fn default() -> Self {
        Store::new()
    }
}

const DEFAULT_HORIZON: usize = 200;
const RECENT_KEYS: usize = 64;

impl Store {
    pub fn new() -> Store {
        Store::with_document(Document::new())
    }

    pub fn with_document(doc: Document) -> Store {
        let mut store = Store {
            doc,
            history: History {
                horizon: DEFAULT_HORIZON,
                ..Default::default()
            },
            recent_keys: VecDeque::new(),
            reactive: graph::Reactive::default(),
        };
        store.reactive.rebuild(&store.doc);
        store
    }

    pub fn reactive(&self) -> &graph::Reactive {
        &self.reactive
    }

    /// The value a reader sees for a leaf: the gesture preview if one is in
    /// progress, else the binding's resolved value, else the constant.
    pub fn resolved(&self, id: NodeId, path: LeafPath) -> Option<Value> {
        self.reactive.read(&self.doc, Leaf { node: id, path })
    }

    pub fn has_changes(&self) -> bool {
        self.reactive.has_changes()
    }

    /// Leaves whose resolved value changed since the last drain.
    pub fn drain_changes(&mut self) -> Vec<ResolvedChange> {
        self.reactive.drain_changes()
    }

    /// Preview a leaf's value for the duration of a gesture. Bindings that
    /// read it re-evaluate now; nothing is written to the document until
    /// [`Store::commit_gesture`].
    pub fn preview(&mut self, id: NodeId, path: LeafPath, value: Value) {
        self.reactive.preview(&self.doc, Leaf { node: id, path }, value);
    }

    pub fn gesture_in_progress(&self) -> bool {
        !self.reactive.overlay().is_empty()
    }

    /// Turn the previewed values into one transaction. Previews on bound
    /// leaves are dropped: a derived value has no handle without a
    /// disambiguation policy (doc 02 invariant 2). `None` when nothing was
    /// previewed or nothing differs from the document.
    pub fn commit_gesture(
        &mut self,
        label: impl Into<String>,
        actor: Actor,
        ts: u64,
    ) -> Option<Result<Applied, Vec<ApplyError>>> {
        let mut ops = Vec::new();
        for (leaf, value) in self.reactive.overlay() {
            let Some(slot) = self.doc.leaf(leaf.node, leaf.path) else {
                continue;
            };
            if slot.expr().is_some() {
                continue;
            }
            if slot.constant() != Some(value) {
                ops.push(Op::Set {
                    id: leaf.node,
                    path: leaf.path,
                    slot: Slot::Const(value.clone()),
                });
            }
        }
        if ops.is_empty() {
            self.cancel_gesture();
            return None;
        }
        let result = self.apply(TransactionInput {
            label: label.into(),
            actor,
            ts,
            idempotency_key: None,
            ops,
        });
        self.reactive.clear_overlay(&self.doc);
        Some(result)
    }

    pub fn cancel_gesture(&mut self) {
        self.reactive.clear_overlay(&self.doc);
    }

    /// Keep the graph in step with a transaction that just applied.
    fn after_transaction(&mut self, tx: &Transaction) {
        let structural = tx.ops.iter().chain(tx.inverse.iter()).any(|op| match op {
            Op::Create { .. } | Op::Delete { .. } | Op::AddComp { .. } | Op::RemoveComp { .. } => true,
            Op::Set { slot, .. } => slot.expr().is_some(),
            _ => false,
        });
        if structural {
            self.reactive.rebuild(&self.doc);
        } else {
            let changed: Vec<Leaf> = tx
                .ops
                .iter()
                .filter_map(|op| match op {
                    Op::Set { id, path, .. } => Some(Leaf {
                        node: *id,
                        path: *path,
                    }),
                    _ => None,
                })
                .collect();
            self.reactive.invalidate(&self.doc, changed);
        }
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn set_horizon(&mut self, horizon: usize) {
        self.history.horizon = horizon.max(1);
        while self.history.past.len() > self.history.horizon {
            self.history.past.remove(0);
        }
    }

    /// Apply a transaction atomically. Either every op applies and the
    /// version advances, or nothing applies and every error is reported.
    pub fn apply(&mut self, input: TransactionInput) -> Result<Applied, Vec<ApplyError>> {
        if let Some(k) = &input.idempotency_key
            && let Some((_, v)) = self.recent_keys.iter().find(|(key, _)| key == k)
        {
            return Ok(Applied {
                version: *v,
                replayed: true,
            });
        }
        let tx = self.run(input)?;
        self.after_transaction(&tx);
        let version = tx.version;
        if let Some(k) = &tx.idempotency_key {
            self.recent_keys.push_back((k.clone(), version));
            while self.recent_keys.len() > RECENT_KEYS {
                self.recent_keys.pop_front();
            }
        }
        self.history.future.clear();
        self.history.past.push(tx.clone());
        while self.history.past.len() > self.history.horizon {
            self.history.past.remove(0);
        }
        self.history.record(tx);
        Ok(Applied {
            version,
            replayed: false,
        })
    }

    /// Undo the most recent transaction. `ts` is the caller's clock.
    /// `None` when there is nothing to undo.
    pub fn undo(&mut self, ts: u64) -> Option<Result<Applied, Vec<ApplyError>>> {
        let tx = self.history.past.pop()?;
        let input = TransactionInput {
            label: format!("undo: {}", tx.label),
            actor: Actor::System,
            ts,
            idempotency_key: None,
            ops: tx.undo_ops(),
        };
        Some(match self.run(input) {
            Ok(applied) => {
                self.after_transaction(&applied);
                let version = applied.version;
                self.history.record(applied);
                self.history.future.push(tx);
                Ok(Applied {
                    version,
                    replayed: false,
                })
            }
            Err(e) => {
                self.history.past.push(tx);
                Err(e)
            }
        })
    }

    pub fn redo(&mut self, ts: u64) -> Option<Result<Applied, Vec<ApplyError>>> {
        let tx = self.history.future.pop()?;
        let input = TransactionInput {
            label: format!("redo: {}", tx.label),
            actor: Actor::System,
            ts,
            idempotency_key: None,
            ops: tx.ops.clone(),
        };
        Some(match self.run(input) {
            Ok(applied) => {
                self.after_transaction(&applied);
                let version = applied.version;
                self.history.past.push(applied.clone());
                self.history.record(applied);
                Ok(Applied {
                    version,
                    replayed: false,
                })
            }
            Err(e) => {
                self.history.future.push(tx);
                Err(e)
            }
        })
    }

    /// Run the ops against a working copy; commit only if all succeed.
    ///
    /// Cloning per transaction is the simplest way to report every error in a
    /// batch and leave the store untouched on failure. Transactions are
    /// coalesced per gesture, so this is one clone per commit, not per frame.
    /// If it ever shows up in a profile, the alternative is applying with
    /// rollback through the inverses.
    fn run(&mut self, input: TransactionInput) -> Result<Transaction, Vec<ApplyError>> {
        let mut work = self.doc.clone();
        let stamp = work.version().next();
        let mut inverse = Vec::new();
        let mut errors = Vec::new();
        for (i, op) in input.ops.iter().enumerate() {
            match op::apply_op(&mut work, i, op, stamp) {
                Ok(inv) => inverse.extend(inv),
                Err(e) => errors.push(e),
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        work.set_version(stamp);
        self.doc = work;
        Ok(Transaction {
            version: stamp,
            label: input.label,
            actor: input.actor,
            ts: input.ts,
            idempotency_key: input.idempotency_key,
            ops: input.ops,
            inverse,
        })
    }

    pub fn view(&self, q: &ViewQuery) -> Result<String, ViewError> {
        view::render_tree(&self.doc, &self.reactive, q)
    }

    pub fn save(&self) -> String {
        serial::save(&self.doc)
    }

    /// A store over a loaded document, with empty history.
    pub fn load(text: &str) -> Result<Store, LoadError> {
        serial::load(text).map(Store::with_document)
    }
}
