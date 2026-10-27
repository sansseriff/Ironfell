//! The pointer tolerances every tool shares, in one place
//! (plans/specs/contexts.md §4, domain 4): the defaults, which a view's
//! context can change (`tolerance.*`, resolved in `view_focus`).

/// Window px the pointer may travel and still click, rather than drag (the
/// pen reads it as CSS px).
pub const SLOP: f32 = 3.0;
/// Seconds and window px between two presses that make a double-click.
pub const DOUBLE_CLICK: (f64, f32) = (0.4, 5.0);

/// A view's tolerances, as its focus context says (`tolerance.*`;
/// `view_focus`): the defaults above unless a context changes them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tolerances {
    pub slop: f32,
    /// Seconds.
    pub double_click: f64,
    /// Grips' and handles' reach, times their usual size.
    pub hit: f64,
}

impl Default for Tolerances {
    fn default() -> Self {
        Tolerances { slop: SLOP, double_click: DOUBLE_CLICK.0, hit: 1.0 }
    }
}
