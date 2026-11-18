//! The tests that register real fonts with `iron_text`'s process-wide engine.
//! A binary of their own: in `suite`, registering would change every other
//! test's text measure partway through the run.

mod layout;
mod table;
mod text;
