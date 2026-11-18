//! Differences between subject and reference that are intended, and the
//! calls the old engine once left changed after an error (reactive-engine
//! handoff §4.1, task 4; spec §16.1). The harness reads both tables. Both
//! are empty since phase 2b, whose engine now runs on both sides; 2c's
//! differences from it are entered here.
//!
//! **Intended differences.** The new engine need not copy the old one's
//! quirks (the user, 2026-11-16). A quirk the old engine can drop cheaply is
//! fixed in both, so the comparison stays exact. Any other is an entry
//! here, with the narrow case it relaxes and the test of the new
//! behaviour. Adding one is a stop-and-report only when it is unclear which
//! behaviour is right, or when it alters something a user can see and may
//! rely on.
//!
//! **Not atomic.** Calls after which the old engine, having returned an
//! error, was not as it was before the call. Phase 0 measured four; phase
//! 2a and 2b's stage 1 fixed them all in both engines, so the table is
//! empty. `IRON_ATOMICITY=1` reports any new one.

use super::Snapshot;

/// An intended difference between subject and reference.
pub struct Intended {
    pub name: &'static str,
    /// How the comparison is relaxed for it, and how narrowly.
    pub relax: Relax,
    /// The test that asserts the new behaviour.
    pub test: &'static str,
}

/// The two ways a difference is allowed for.
pub enum Relax {
    /// Applied to both stores' snapshots before every comparison: a
    /// difference that is global and harmless. It may only make the two
    /// alike in the narrow respect it names.
    Normalise(fn(&mut Snapshot)),
    /// Recognises one call after which the stores legitimately differ, from
    /// the method and the two return values (subject, reference) as text.
    /// The facade stops comparing that script from then on.
    Diverge(fn(&str, &str, &str) -> bool),
}

pub const INTENDED: &[Intended] = &[];

thread_local! {
    /// Entries a test adds for its own duration (`with_intended`).
    static EXTRA: std::cell::RefCell<Vec<&'static Intended>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Run `f` with `extra` entries in force too: for tests of the harness.
pub fn with_intended<R>(extra: &'static [Intended], f: impl FnOnce() -> R) -> R {
    EXTRA.with(|e| e.borrow_mut().extend(extra.iter()));
    struct Reset(usize);
    impl Drop for Reset {
        fn drop(&mut self) {
            EXTRA.with(|e| e.borrow_mut().truncate(self.0));
        }
    }
    let _reset = Reset(EXTRA.with(|e| e.borrow().len()) - extra.len());
    f()
}

/// Every entry in force: `INTENDED` and a test's own.
pub(crate) fn entries() -> Vec<&'static Intended> {
    INTENDED.iter().chain(EXTRA.with(|e| e.borrow().clone())).collect()
}

/// How often each entry mattered, by name: (comparisons a normaliser turned
/// from different to equal, scripts a divergence ended). The 10,000-seed
/// run reports them.
static COUNTS: std::sync::Mutex<std::collections::BTreeMap<&'static str, (usize, usize)>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

pub(crate) fn count(name: &'static str, normalised: bool) {
    let mut c = COUNTS.lock().unwrap_or_else(|e| e.into_inner());
    let e = c.entry(name).or_default();
    if normalised {
        e.0 += 1;
    } else {
        e.1 += 1;
    }
}

/// `COUNTS`, for a report.
pub fn counts() -> Vec<(&'static str, usize, usize)> {
    COUNTS.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|(n, (a, b))| (*n, *a, *b)).collect()
}

/// A call the old engine does not leave as it was when it fails.
pub struct NotAtomic {
    /// The public method.
    pub method: &'static str,
    /// The situation, and what is left behind.
    pub situation: &'static str,
    /// The test that shows it.
    pub test: &'static str,
}

pub const NOT_ATOMIC: &[NotAtomic] = &[];
