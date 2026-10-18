//! Fonts for text nodes, as a host service (plans/specs/text.md §4).
//!
//! The engine never fetches: it asks. Each frame the text engine's wanted
//! families (a layout that needed one, or the default asked for at
//! startup) go to the host as an [`EngineEvent::Fonts`]; the host fetches
//! the files in that order and hands back the bytes (`web_ffi::register_font`).
//! Arrival moves the engine's generation, and [`refresh_text`] reports
//! every text node changed so it re-measures and redraws. Nothing waits:
//! text lays out in the default face, or as an estimate, until its font
//! arrives.

use super::DocumentStore;
use crate::host_events::{EngineEvent, FontRequest, HostEvents};
use bevy::prelude::*;

/// Ask for the default face once the app runs, so new text has it.
pub(super) fn want_default_font() {
    iron_text::engine().want(iron_text::DEFAULT_FAMILY);
}

pub(super) fn refresh_text(mut store: ResMut<DocumentStore>, events: Res<HostEvents>) {
    if store.bypass_change_detection().0.refresh_text() {
        store.set_changed();
    }
    let wanted = iron_text::engine().take_wanted();
    if !wanted.is_empty() {
        events.push(EngineEvent::Fonts { fonts: wanted.into_iter().map(|f| FontRequest { family: f.name.to_owned(), file: f.file.to_owned() }).collect() });
    }
}
