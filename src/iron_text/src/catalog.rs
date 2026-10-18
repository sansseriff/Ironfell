//! The curated font catalog (plans/specs/text.md §4): which families the
//! picker offers and which file holds each. Files are variable TTFs from
//! the Google Fonts repository, self-hosted under `public/fonts/` and
//! fetched by the host only when a document or the picker needs them.
//!
//! The document stores a family *name*; this table is how a host turns the
//! name into bytes. A family not listed here is a missing font: its text
//! draws in [`DEFAULT_FAMILY`] and the inspector says so.

use serde::Serialize;

/// The family new text uses, and the fallback for every other.
pub const DEFAULT_FAMILY: &str = "Inter";

/// Colour emoji: Noto Color Emoji (COLR v1), split into chunks fetched on
/// demand (`src/iron_text/tools/split_emoji.py`, plans/specs/text.md §5.1).
/// Each chunk is its own family; every family falls back to all of them.
pub mod emoji {
    use super::{Category, Family};
    use std::sync::OnceLock;

    struct Chunk {
        family: Family,
        ranges: Vec<(u32, u32)>,
    }

    struct Chunks {
        singles: Vec<Chunk>,
        sequences: Vec<Chunk>,
    }

    fn chunks() -> &'static Chunks {
        static CHUNKS: OnceLock<Chunks> = OnceLock::new();
        CHUNKS.get_or_init(|| {
            let v: serde_json::Value = serde_json::from_str(include_str!("../../../public/fonts/emoji/chunks.json")).expect("chunks.json");
            let read = |key: &str| -> Vec<Chunk> {
                v[key]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| Chunk {
                        // Leaked once: the catalog's names are 'static.
                        family: Family {
                            name: Box::leak(c["family"].as_str().unwrap_or_default().to_owned().into_boxed_str()),
                            file: Box::leak(c["file"].as_str().unwrap_or_default().to_owned().into_boxed_str()),
                            category: Category::Emoji,
                            weights: (400, 400),
                        },
                        ranges: c["ranges"].as_array().into_iter().flatten().filter_map(|r| Some((r[0].as_u64()? as u32, r[1].as_u64()? as u32))).collect(),
                    })
                    .collect()
            };
            Chunks { singles: read("chunks"), sequences: read("sequences") }
        })
    }

    fn find(list: &'static [Chunk], c: char) -> Option<&'static Family> {
        let c = c as u32;
        list.iter().find(|k| k.ranges.iter().any(|&(a, b)| a <= c && c <= b)).map(|k| &k.family)
    }

    /// The chunk drawing `c` on its own.
    pub fn single(c: char) -> Option<&'static Family> {
        find(&chunks().singles, c)
    }

    /// The chunk holding the sequences that start with `c` (skin tones,
    /// ZWJ families, flags).
    pub fn sequence(c: char) -> Option<&'static Family> {
        find(&chunks().sequences, c)
    }

    /// Every chunk's family, sequences first: once loaded, a sequence chunk
    /// shapes a whole sequence where a single chunk would draw its parts.
    pub fn families() -> impl Iterator<Item = &'static str> {
        chunks().sequences.iter().chain(&chunks().singles).map(|k| k.family.name)
    }

    pub(super) fn by_name(name: &str) -> Option<&'static Family> {
        chunks().sequences.iter().chain(&chunks().singles).map(|k| &k.family).find(|f| f.name.eq_ignore_ascii_case(name))
    }

    /// The chunks a text needs: each emoji's own, and its sequence chunk
    /// when something joins it (ZWJ, a skin tone, a tag) or it starts a flag.
    pub fn needed(text: &str) -> Vec<&'static Family> {
        let mut out: Vec<&'static Family> = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        for (i, &c) in chars.iter().enumerate() {
            if !crate::is_emoji(c) {
                continue;
            }
            // What follows, past presentation selectors (🏳️‍🌈 is flag, VS16, ZWJ, rainbow).
            let next = chars[i + 1..].iter().map(|&n| n as u32).find(|&n| n != 0xFE0F && n != 0xFE0E).unwrap_or(0);
            let joined = next == 0x200D || (0x1F3FB..=0x1F3FF).contains(&next) || (0xE0020..=0xE007F).contains(&next) || (0x1F1E6..=0x1F1FF).contains(&(c as u32));
            for f in [single(c), joined.then(|| sequence(c)).flatten()].into_iter().flatten() {
                if !out.iter().any(|o| std::ptr::eq(*o, f)) {
                    out.push(f);
                }
            }
        }
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Sans,
    Serif,
    Mono,
    Handwriting,
    /// Fallback faces (emoji): loaded for text that needs them, not picked.
    Emoji,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Family {
    pub name: &'static str,
    /// Relative to the host's font root (`public/fonts/` on the web).
    pub file: &'static str,
    pub category: Category,
    /// The weight axis's range; equal ends for a static face.
    pub weights: (u16, u16),
}

const fn f(name: &'static str, file: &'static str, category: Category, weights: (u16, u16)) -> Family {
    Family { name, file, category, weights }
}

pub static FAMILIES: &[Family] = &[
    f("Caveat", "Caveat.ttf", Category::Handwriting, (400, 700)),
    f("DM Sans", "DMSans.ttf", Category::Sans, (100, 1000)),
    f("Fira Code", "FiraCode.ttf", Category::Mono, (300, 700)),
    f("IBM Plex Sans", "IBMPlexSans.ttf", Category::Sans, (100, 700)),
    f("Instrument Sans", "InstrumentSans.ttf", Category::Sans, (400, 700)),
    f("Instrument Serif", "InstrumentSerif.ttf", Category::Serif, (400, 400)),
    f("Inter", "Inter.ttf", Category::Sans, (100, 900)),
    f("JetBrains Mono", "JetBrainsMono.ttf", Category::Mono, (100, 800)),
    f("Lora", "Lora.ttf", Category::Serif, (400, 700)),
    f("Montserrat", "Montserrat.ttf", Category::Sans, (100, 900)),
    f("Open Sans", "OpenSans.ttf", Category::Sans, (300, 800)),
    f("Playfair Display", "PlayfairDisplay.ttf", Category::Serif, (400, 900)),
    f("Roboto", "Roboto.ttf", Category::Sans, (100, 900)),
    f("Source Serif 4", "SourceSerif4.ttf", Category::Serif, (200, 900)),
    f("Space Grotesk", "SpaceGrotesk.ttf", Category::Sans, (300, 700)),
];

pub fn family(name: &str) -> Option<&'static Family> {
    FAMILIES.iter().find(|f| f.name.eq_ignore_ascii_case(name)).or_else(|| emoji::by_name(name))
}

/// Figma's weight names, by the usual CSS weights.
pub const WEIGHT_NAMES: &[(u16, &str)] = &[
    (100, "Thin"),
    (200, "Extra Light"),
    (300, "Light"),
    (400, "Regular"),
    (500, "Medium"),
    (600, "Semi Bold"),
    (700, "Bold"),
    (800, "Extra Bold"),
    (900, "Black"),
];

/// The catalog as JSON, for a host's picker.
pub fn to_json() -> String {
    serde_json::to_string(&serde_json::json!({
        "default": DEFAULT_FAMILY,
        "families": FAMILIES,
        "weights": WEIGHT_NAMES.iter().map(|(w, n)| serde_json::json!({"weight": w, "name": n})).collect::<Vec<_>>(),
    }))
    .expect("static data serialises")
}
