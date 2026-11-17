//! Differences between the engines that are intended, and the old engine's
//! known failures of atomicity (reactive-engine handoff §4.1, task 4; spec
//! §16.1). The harness reads both tables.
//!
//! **Intended differences.** Each entry names the difference, the predicate
//! that narrows where the comparison is relaxed, and the test asserting the
//! new behaviour. Only the spec's §16.1 list may appear here, each added in
//! the phase that first needs it; any other is a stop-and-report. Empty
//! while both stores run the old engine.
//!
//! **Known not atomic.** Calls after which the old engine, having returned
//! an error, is not as it was before the call. Measured in phase 0, each
//! with the test that shows it; an entry is deleted when the new engine
//! takes that call over and meets atomicity for it (spec §11).

/// An intended difference between subject and reference.
pub struct Intended {
    pub name: &'static str,
    /// Where the comparison is relaxed, and how narrowly.
    pub predicate: &'static str,
    /// The test that asserts the new behaviour.
    pub test: &'static str,
}

pub const INTENDED: &[Intended] = &[];

/// A call the old engine does not leave as it was when it fails.
pub struct NotAtomic {
    /// The public method.
    pub method: &'static str,
    /// The situation, and what is left behind.
    pub situation: &'static str,
    /// The test that shows it.
    pub test: &'static str,
}

/// Found by `IRON_ATOMICITY=1` over the suite and `tests/atomicity.rs`,
/// 2026-11-16. The suite itself reaches only the solver's entry (in
/// `tests/constraints.rs`, `a_binding_that_reads_and_feeds_a_cluster_is_a_cycle`).
/// Phase 2a fixed a refused transaction's expansion problems, in both
/// strategies, and listed the store state outside the document that a
/// failure leaves (reactive-engine handoff §4.3).
pub const NOT_ATOMIC: &[NotAtomic] = &[
    NotAtomic {
        method: "apply_session",
        situation: "an op list that fails part-way is rolled back by the inverses of the ops before it: a created session node is left as a tombstone and the session id counter has moved; and the failing op's own writes stay, since `reparent`, `set` and `add_facet` write before their checks and return no inverse. Phase 2b takes `apply_session` over (spec §11.3)",
        test: "tests/atomicity.rs a_failed_session_op_list_leaves_a_tombstone",
    },
    NotAtomic {
        method: "apply",
        situation: "a transaction refused by `post_apply` (a cycle through a cluster, an unsolvable cluster, an exact request unmet) gets its document back, but `after_transaction` has already run: the overlay cleared, geometry noted, repeats settled (`repeats`, `presence`, `repeat_inputs`, `truncated`, the published items, and the `expansion_problems` from `run` and from `settle_repeats`), signals stamped; and rebuilding the graph leaves the leaves it touched as pending changes",
        test: "tests/atomicity.rs a_transaction_refused_by_the_solver_leaves_a_change_pending",
    },
    NotAtomic {
        method: "apply (and the calls that apply a transaction)",
        situation: "with a machine state pinned, `run` counts a shared edit (`machines.shared`) before any check, so a refused transaction still counts",
        test: "none: listed in phase 2a from reading `lib.rs` `run`",
    },
    NotAtomic {
        method: "undo, redo",
        situation: "the machines the transaction touches are marked to snap (`machines.snap`) before `run`, and stay marked if it fails",
        test: "none: listed in phase 2a from reading `lib.rs` `undo` and `redo`",
    },
    NotAtomic {
        method: "fire, fire_at",
        situation: "an action that emits and then fails on a later write has already queued its event",
        test: "tests/atomicity.rs a_failed_action_has_already_queued_its_emit",
    },
];
