//! The patch algebra as a pure model (plans/handoff-patches.md, PT-3).
//!
//! The design is a note, not a spec:
//! plans/notes/patches-regions-and-time.md. A **patch** is a set of
//! overrides with a **region**, a set of points over **axes**; the value at
//! a point is the base with every patch whose region contains it applied,
//! weakest first. The note's §11 (the PT-2 council) found that its laws as
//! written do not all hold, and proposed repairs. This crate is where they
//! are tried: each property of §11.5 is a test over random scripts, checked
//! against enumerating every point, and each strength rule (`Strength`) is
//! run through the same properties. What passes is what PT-4a writes down.
//!
//! - `axis`: axes (ordered, unordered, paths), values by stable id, points.
//! - `region`: per-axis selections, products (a patch's region) and unions
//!   (a query's answer), with symbolic intersection and difference.
//! - `patch`: patches, the base, and strength.
//! - `world`: the value at a point, winning regions, stale entries, the
//!   inclusion lint.
//! - `edit`: the edit operations, as ops with inverses.
//!
//! What it leaves out: continuous axes (time is an ordered axis of sample
//! points here; intervals with open and closed ends are built, so the step
//! to real numbers changes positions, not the algebra); values that are
//! functions of the coordinates (a clip's); patches that set coordinates
//! (refused by construction: coordinates are not fields here, §11.4);
//! relative regions such as "each instance in its own state" (§11.4, open).
//! It is a model: every operation is written for clarity, and `snapshot`
//! enumerates every point.

pub mod axis;
pub mod edit;
pub mod patch;
pub mod region;
pub mod world;

pub use axis::{Axis, AxisId, Kind, Point, Space, ValueId};
pub use edit::Op;
pub use patch::{Doc, Field, HIDDEN, Key, Node, PRESENT, Patch, PatchId, Strength};
pub use region::{Bound, Product, Region, Sel};
pub use world::{State, World};
