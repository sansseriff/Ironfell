//! The pointer tolerances every tool shares, in one place
//! (plans/specs/contexts.md §4, domain 4: they become context data later).

/// Window px the pointer may travel and still click, rather than drag (the
/// pen reads it as CSS px).
pub const SLOP: f32 = 3.0;
/// Seconds and window px between two presses that make a double-click.
pub const DOUBLE_CLICK: (f64, f32) = (0.4, 5.0);
