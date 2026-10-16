//! Resolved clip frames, shared by picking and presentation. No renderer types.
use crate::{NodeId, Store, Value};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipFrame {
    pub w: f64,
    pub h: f64,
    pub radius: f64,
    pub ellipse: bool,
}

impl ClipFrame {
    pub fn read(store: &Store, id: NodeId) -> Option<Self> {
        let read = |leaf: &str| store.resolved(id, leaf.parse().expect("declared leaf"));
        if read("group_clip.enabled") != Some(Value::Bool(true)) { return None; }
        let number = |leaf, default| read(leaf).and_then(|v| v.as_f64()).filter(|v| v.is_finite()).unwrap_or(default);
        let w = number("group_clip.w", 200.0).max(0.0);
        let h = number("group_clip.h", 200.0).max(0.0);
        Some(Self { w, h, radius: number("group_clip.radius", 0.0).clamp(0.0, w.min(h) / 2.0), ellipse: read("group_clip.shape").and_then(|v| v.as_str().map(str::to_owned)).as_deref() == Some("ellipse") })
    }

    pub fn contains(self, [x, y]: [f64; 2]) -> bool {
        if self.w <= 0.0 || self.h <= 0.0 || x < 0.0 || y < 0.0 || x > self.w || y > self.h { return false; }
        if self.ellipse {
            ((x - self.w / 2.0) / (self.w / 2.0)).powi(2) + ((y - self.h / 2.0) / (self.h / 2.0)).powi(2) <= 1.0
        } else {
            let r = self.radius;
            let dx = x - x.clamp(r, self.w - r);
            let dy = y - y.clamp(r, self.h - r);
            dx * dx + dy * dy <= r * r
        }
    }
}

/// The point is in this node's composition space. Embedded compositions call
/// this again in their own space; ancestors never cross embed references.
pub fn inside_clips(store: &Store, id: NodeId, point: [f64; 2]) -> bool {
    let mut at = Some(id);
    while let Some(id) = at {
        if let Some(frame) = ClipFrame::read(store, id) {
            let Some(inv) = store.world_affine(id).inverse() else { return false; };
            if !frame.contains(inv.apply(point)) { return false; }
        }
        at = store.document().live(id).and_then(|n| n.parent);
    }
    true
}
