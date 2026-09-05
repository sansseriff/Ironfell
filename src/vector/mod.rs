//! The 2D vector rendering seam.
//!
//! ```text
//!   app draw systems  ──emit──▶  DisplayList  ──realize──▶  backend  ──▶  pixels
//!   (know no renderer)           (the seam)                 (Vello Hybrid)
//! ```
//!
//! Two properties matter more than the layering itself:
//!
//! 1. **Renderer types stay isolated.** They do not leak into app drawing code.
//! 2. **Unchanged layers cost nothing.** [`DisplayListRebuild::rebuild`] compares
//!    content before marking the component changed, so a layer that draws the
//!    same thing twice re-encodes once. Idle frames do no encoding at all.

mod backend;
mod composite;
mod composite_material;
mod display_list;
mod hybrid;

// Re-exported: what app drawing code needs. Backends reach into the submodules
// directly (`display_list::DrawCmd`, ...) rather than
// going through here, so this list stays scoped to the authoring side.
pub use backend::{VectorLayer, order};
pub use composite::VectorCamera;
pub use display_list::{DisplayList, DisplayListRebuild};

use bevy::prelude::*;

/// Installs the display-list seam and Vello Hybrid renderer.
pub struct VectorPlugin;

impl Plugin for VectorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            composite::CompositePlugin,
            composite_material::CompositeMaterialPlugin,
            hybrid::HybridBackend,
        ));
    }
}
