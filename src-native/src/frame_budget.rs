//! CPU budget for native just-in-time updates. All times use CACurrentMediaTime.
//! This predicts CPU submission, not GPU completion or display scanout.
use std::collections::VecDeque;

pub struct FrameBudget {
    samples: VecDeque<f64>,
    period: f64,
    last_finish: f64,
    margin: f64,
    timer_guard: f64,
}

impl FrameBudget {
    pub fn new(margin: f64) -> Self {
        Self {
            samples: VecDeque::with_capacity(60),
            period: 0.0,
            last_finish: 0.0,
            margin,
            timer_guard: 0.0,
        }
    }

    /// Reserve time for observed run-loop timer lateness, as framepace does for
    /// oversleep. Decay on immediate frames too, so an OS stall cannot latch us
    /// permanently into early updates.
    pub fn observe_timer(&mut self, overshoot: f64) {
        if overshoot.is_finite() && overshoot >= 0.0 {
            self.timer_guard = (self.timer_guard * 0.9).max(overshoot);
        }
    }

    pub fn margin(&self) -> f64 {
        self.margin
    }

    pub fn set_margin(&mut self, margin: f64) {
        if margin.is_finite() {
            self.margin = margin;
        }
    }

    pub fn observe(&mut self, start: f64, end: f64) {
        let work = end - start;
        if !work.is_finite() || work <= 0.0 {
            return;
        }
        if self.samples.len() == 60 {
            self.samples.pop_front();
        }
        self.samples.push_back(work);
        self.last_finish = end;
    }

    /// Return time before starting work, never time to block the event loop.
    /// First input after idle, startup, refresh transitions and overload run now.
    pub fn delay(&mut self, now: f64, target: f64, period: f64) -> f64 {
        if !target.is_finite() {
            return 0.0;
        }
        self.reserve(now, period)
            .map_or(0.0, |reserve| (target - now - reserve).clamp(0.0, period))
    }

    /// Time to reserve before a deadline: work, margin and timer lateness. `None`
    /// means run now (startup, after idle, refresh transitions, invalid input).
    pub fn reserve(&mut self, now: f64, period: f64) -> Option<f64> {
        if !now.is_finite() || !period.is_finite() || !(0.001..=0.1).contains(&period) {
            return None;
        }
        if self.period > 0.0 && (period / self.period - 1.0).abs() > 0.2 {
            self.samples.clear();
        }
        self.period = period;
        if self.samples.len() < 8 || now - self.last_finish > period * 4.0 {
            return None;
        }
        let mut samples: Vec<_> = self.samples.iter().copied().collect();
        samples.sort_by(f64::total_cmp);
        let p95 = samples[((samples.len() - 1) as f64 * 0.95).ceil() as usize];
        // A sudden workload increase must back off immediately, even before it
        // occupies 5% of the rolling window.
        let work = p95.max(*self.samples.back().unwrap());
        Some(work + self.margin + self.timer_guard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn warm(work: f64) -> FrameBudget {
        let mut budget = FrameBudget::new(0.002);
        for _ in 0..8 {
            budget.observe(1.0 - work, 1.0);
        }
        budget
    }
    #[test]
    fn waits_until_work_plus_margin_remain() {
        let delay = warm(0.003).delay(1.001, 1.016, 1.0 / 60.0);
        assert!((delay - 0.010).abs() < 1e-9);
    }
    #[test]
    fn startup_idle_and_overload_do_not_delay_input() {
        assert_eq!(FrameBudget::new(0.002).delay(1.0, 1.016, 1.0 / 60.0), 0.0);
        assert_eq!(warm(0.003).delay(2.0, 2.016, 1.0 / 60.0), 0.0);
        assert_eq!(warm(0.020).delay(1.001, 1.016, 1.0 / 60.0), 0.0);
        let mut budget = warm(0.003);
        budget.observe(0.98, 1.0);
        assert_eq!(budget.delay(1.001, 1.016, 1.0 / 60.0), 0.0);
    }
    #[test]
    fn refresh_change_relearns_the_budget() {
        let mut budget = warm(0.003);
        assert!(budget.delay(1.001, 1.016, 1.0 / 60.0) > 0.0);
        assert_eq!(budget.delay(1.001, 1.008, 1.0 / 120.0), 0.0);
        assert!(budget.samples.is_empty());
    }
    #[test]
    fn compensates_for_timer_overshoot_and_recovers() {
        let mut budget = warm(0.003);
        budget.observe_timer(0.0015);
        assert!((budget.delay(1.001, 1.016, 1.0 / 60.0) - 0.0085).abs() < 1e-9);
        budget.observe_timer(0.1);
        assert_eq!(budget.delay(1.001, 1.016, 1.0 / 60.0), 0.0);
        for _ in 0..100 {
            budget.observe_timer(0.0);
        }
        assert!(budget.delay(1.001, 1.016, 1.0 / 60.0) > 0.009);
    }
    #[test]
    fn invalid_and_expired_deadlines_never_wait() {
        let mut budget = warm(0.003);
        assert_eq!(budget.delay(1.001, 1.0, 1.0 / 60.0), 0.0);
        assert_eq!(budget.delay(1.001, f64::NAN, 1.0 / 60.0), 0.0);
        assert_eq!(budget.delay(1.001, 1.016, 0.0), 0.0);
        budget.observe(0.0, f64::INFINITY);
        assert_eq!(budget.samples.len(), 8);
    }
}
