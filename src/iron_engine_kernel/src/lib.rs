//! A reference model of the new reactive engine's core pass.
//!
//! plans/specs/reactive-engine.md §6 and §7 describe one pass that keeps
//! values, and the structure those values decide, up to date by doing only
//! what changed and only what something observes. This crate is that pass
//! and nothing else, small enough to read in one sitting, with a brute-force
//! reference to test it against. It is not used by the app: the real engine
//! (`iron_document::engine`, phase 2b) is written from the spec, with this
//! as the worked example of the algorithm. `engine.rs` lists the rules.
//!
//! What it leaves out, each the spec's and not this model's: templates
//! shared between owners with overrides (§5.4), owner states other than
//! observed and not (§8.1), exits and presence (§7.1 step 3), layout and
//! solve cells (§9), time (§10), transactions (§11), ids and the Tree (§12).
//! Its expression language is a toy; only the reads matter.

pub mod engine;
pub mod model;
pub mod reference;

pub use engine::Engine;
pub use model::{Entry, Expr, Key, Path, Slot, Template, Value};
pub use reference::reference;
