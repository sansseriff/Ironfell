//! The 2D vector rendering seam.
//!
//! ```text
//!   app draw systems  ──emit──▶  DisplayList  ──realize──▶  backend  ──▶  pixels
//!   (know no renderer)           (the seam)                 (hybrid | classic)
//! ```
//!
//! Two properties matter more than the layering itself:
//!
//! 1. **A backend swap is additive.** Renderer-specific types stay in sibling
//!    backend modules rather than leaking into app drawing code.
//! 2. **Unchanged layers cost nothing.** [`DisplayListRebuild::rebuild`] compares
//!    content before marking the component changed, so a layer that draws the
//!    same thing twice re-encodes once. Idle frames do no encoding at all.
//!
//! The app installs only its selected backend. This is load-bearing for startup:
//! `VelloPlugin` builds classic's compute pipelines in `Plugin::finish`, so a
//! classic backend that is merely *present* still costs cold-start time. That is
//! why selection happens at app construction rather than by toggling a resource,
//! and why switching renderers means rebuilding the `App` (the wasm module and
//! the canvas are reused — nothing is re-downloaded).

mod backend;
#[cfg(feature = "classic")]
mod classic;
mod composite;
mod composite_material;
mod display_list;
mod hybrid;

// Re-exported: what app drawing code needs. Backends reach into the submodules
// directly (`display_list::DrawCmd`, ...) rather than
// going through here, so this list stays scoped to the authoring side.
pub use backend::{BackendKind, VectorLayer, order};
pub use composite::VectorCamera;
pub use display_list::{DisplayList, DisplayListRebuild};

use bevy::prelude::*;

/// Installs the display-list seam and exactly one backend.
pub struct VectorPlugin {
    backend: BackendKind,
}

impl Default for VectorPlugin {
    fn default() -> Self {
        Self::hybrid_only()
    }
}

impl VectorPlugin {
    /// Normal fast-start path: Hybrid is the only renderer initialized.
    pub fn hybrid_only() -> Self {
        Self {
            backend: BackendKind::Hybrid,
        }
    }

    /// Explicit fallback path, entered after the user requests classic.
    #[cfg(feature = "classic")]
    pub fn classic_only() -> Self {
        Self {
            backend: BackendKind::ClassicVello,
        }
    }
}

impl Plugin for VectorPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.backend)
            .add_plugins(composite::CompositePlugin);

        // `VelloPlugin` creates the classic renderer and all of its compute
        // pipelines in `Plugin::finish`, so selecting Hybrid must omit it
        // entirely rather than leave it installed and idle.
        match self.backend {
            #[cfg(feature = "classic")]
            BackendKind::ClassicVello => {
                app.add_plugins(classic::ClassicVelloBackend);
            }
            BackendKind::Hybrid => {
                app.add_plugins((
                    composite_material::CompositeMaterialPlugin,
                    hybrid::HybridBackend,
                ));
            }
        }
    }
}
