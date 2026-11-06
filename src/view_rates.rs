//! How often each view renders (the composition settings' FPS readout,
//! `frame_stats`). Views render on demand into their own textures
//! (plans/specs/view-textures.md), so a frozen view renders no frames while
//! an animating one renders every frame; the counts here are per view.
//!
//! The 2D renderer runs in the render world and knows a view by its entity;
//! 3D panel cameras are counted in the main world by panel id. Counts only
//! grow; the shell turns two readings into a rate.

use bevy::prelude::Entity;
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

static BY_ENTITY: Mutex<Option<HashMap<Entity, u64>>> = Mutex::new(None);
static BY_PANEL: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());

/// A 2D view's scene was rebuilt and rendered this frame.
pub(crate) fn rendered_2d(view: Entity) {
    if let Ok(mut m) = BY_ENTITY.lock() {
        *m.get_or_insert_with(HashMap::new).entry(view).or_default() += 1;
    }
}

/// A 3D panel camera renders this frame.
pub(crate) fn rendered_3d(panel: &str) {
    if let Ok(mut m) = BY_PANEL.lock() {
        *m.entry(panel.to_owned()).or_default() += 1;
    }
}

/// Every panel's render count: its 2D view's (found through `panel_of`, the
/// main world's view entities by panel) plus its 3D camera's.
pub(crate) fn counts(panel_of: &[(Entity, String)]) -> BTreeMap<String, u64> {
    let mut out = BY_PANEL.lock().map(|m| m.clone()).unwrap_or_default();
    if let Ok(m) = BY_ENTITY.lock()
        && let Some(m) = m.as_ref()
    {
        for (e, panel) in panel_of {
            *out.entry(panel.clone()).or_default() += m.get(e).copied().unwrap_or(0);
        }
    }
    out
}
