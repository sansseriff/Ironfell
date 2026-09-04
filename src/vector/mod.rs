//! The 2D vector rendering seam.
//!
//! ```text
//!   app draw systems  ──emit──▶  DisplayList  ──realize──▶  backend  ──▶  pixels
//!   (know no renderer)           (the seam)                 (classic | hybrid)
//! ```
//!
//! Two properties matter more than the layering itself:
//!
//! 1. **A backend swap is additive.** `classic.rs` is the only module naming a
//!    Vello type. A sparse-strips backend is a sibling module plus a match arm,
//!    not a rewrite of drawing code.
//! 2. **Unchanged layers cost nothing.** [`DisplayListRebuild::rebuild`] compares
//!    content before marking the component changed, so a layer that draws the
//!    same thing twice re-encodes once. Idle frames do no encoding at all.
//!
//! Both backends can run at once (see [`ActiveBackends`]), which is how two
//! renderers get compared on identical input rather than on two builds that have
//! quietly diverged.

mod backend;
mod classic;
mod display_list;

// Re-exported: what app drawing code needs. Backends reach into the submodules
// directly (`backend::BackendKind`, `display_list::DrawCmd`, ...) rather than
// going through here, so this list stays scoped to the authoring side.
pub use backend::{ActiveBackends, VectorLayer, order};
pub use classic::ClassicBackendCamera;
pub use display_list::{DisplayList, DisplayListRebuild};

use bevy::prelude::*;

/// Installs the display-list seam and every backend that can realize it.
pub struct VectorPlugin {
    pub backends: ActiveBackends,
}

impl Default for VectorPlugin {
    fn default() -> Self {
        Self {
            backends: ActiveBackends::default(),
        }
    }
}

impl Plugin for VectorPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.backends.clone())
            .add_plugins(classic::ClassicVelloBackend);
        // The sparse-strips backend plugs in here. Selecting `BackendKind::Hybrid`
        // before then leaves every layer unrealized, which is deliberate: a
        // missing backend should be obvious, not silently fall back to the other.
    }
}
