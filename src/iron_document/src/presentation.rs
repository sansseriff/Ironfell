//! Ephemeral numeric presentation transitions. Semantic values, bindings,
//! history and exports always observe the target; only the drawn control eases.
use crate::{contexts, Leaf, NodeId, StoreCore, Value};

pub(crate) struct Tween {
    from: f64,
    to: f64,
    start: f64,
    seconds: f64,
    ease: &'static str,
}

impl Tween {
    fn at(&self, now: f64) -> f64 {
        let u = ((now - self.start) / self.seconds).clamp(0.0, 1.0);
        let ease = match self.ease { "in" => "ease_in", "out" => "ease_out", "in_out" => "ease_in_out", _ => "linear" };
        self.from + (self.to - self.from) * crate::time::ease(ease, u)
    }
}

impl StoreCore {
    /// Value used to draw a control, without delaying its semantic output.
    pub fn presented_number(&self, leaf: Leaf) -> Option<f64> {
        let target = self.resolved(leaf.node, leaf.path)?.as_f64()?;
        Some(self.motion.values.get(&leaf).filter(|t| t.to == target).map_or(target, |t| t.at(self.motion.clock)))
    }

    pub fn animate_presentation(&mut self, leaf: Leaf, from: f64) {
        self.cancel_presentation(leaf);
        if !self.motion.enabled || !from.is_finite() { return; }
        let Some(to) = self.resolved(leaf.node, leaf.path).and_then(|v| v.as_f64()).filter(|v| v.is_finite()) else { return };
        let chain = contexts::chain_of(self.tree(), leaf.node);
        if chain.is_empty() { return; }
        let declared = self.resolver().get(&chain, chain.len() - 1, contexts::Field::VALUE_MOTION);
        let Some(m) = contexts::motion(&declared).filter(|m| m.ms > 0.0) else { return };
        if from == to { return; }
        self.motion.values.insert(leaf, Tween { from, to, start: self.motion.clock, seconds: m.ms / 1000.0, ease: m.ease });
        self.reactive.mark(leaf, Value::Number(to));
    }

    pub fn cancel_presentation(&mut self, leaf: Leaf) {
        if let Some(t) = self.motion.values.remove(&leaf) {
            self.reactive.mark(leaf, Value::Number(t.to));
        }
    }

    pub(crate) fn clear_presentation(&mut self) {
        for (leaf, t) in std::mem::take(&mut self.motion.values) {
            self.reactive.mark(leaf, Value::Number(t.to));
        }
    }

    pub(crate) fn advance_presentation(&mut self) -> Vec<NodeId> {
        let mut moving = Vec::new();
        let tweens = std::mem::take(&mut self.motion.values);
        for (leaf, t) in tweens {
            let target = self.resolved(leaf.node, leaf.path).and_then(|v| v.as_f64());
            if self.tree().is_live(leaf.node) {
                moving.push(leaf.node);
                self.reactive.mark(leaf, Value::Number(t.to));
                if target == Some(t.to) && self.motion.clock < t.start + t.seconds {
                    self.motion.values.insert(leaf, t);
                }
            }
        }
        moving
    }
}
