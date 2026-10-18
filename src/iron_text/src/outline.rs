//! Glyph outlines in font units, read once per glyph and variation
//! location and reused by every layout (plans/specs/text.md §5).

use kurbo::BezPath;
use parley::FontData;
use skrifa::instance::{LocationRef, NormalizedCoord, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    blob: u64,
    index: u32,
    glyph: u32,
    coords: Vec<i16>,
}

#[derive(Default)]
pub(crate) struct Cache {
    glyphs: HashMap<Key, Option<Arc<BezPath>>>,
    upem: HashMap<(u64, u32), f64>,
}

/// Past this many glyphs the cache starts over.
const MAX: usize = 1 << 16;

impl Cache {
    /// The glyph's outline in font units (y-up) and the font's units per
    /// em, or `None` for a glyph with no outline (a space).
    pub(crate) fn get(&mut self, font: &FontData, glyph: u32, coords: &[i16]) -> Option<(Arc<BezPath>, f64)> {
        let blob = font.data.id();
        let upem = match self.upem.get(&(blob, font.index)) {
            Some(u) => *u,
            None => {
                let f = FontRef::from_index(font.data.as_ref(), font.index).ok()?;
                let u = f.metrics(Size::unscaled(), LocationRef::default()).units_per_em as f64;
                let u = u.max(1.0);
                self.upem.insert((blob, font.index), u);
                u
            }
        };
        let key = Key { blob, index: font.index, glyph, coords: coords.to_vec() };
        if let Some(hit) = self.glyphs.get(&key) {
            return hit.clone().map(|p| (p, upem));
        }
        if self.glyphs.len() >= MAX {
            self.glyphs.clear();
        }
        let path = draw(font, glyph, coords).filter(|p| !p.elements().is_empty()).map(Arc::new);
        self.glyphs.insert(key, path.clone());
        path.map(|p| (p, upem))
    }
}

fn draw(font: &FontData, glyph: u32, coords: &[i16]) -> Option<BezPath> {
    let f = FontRef::from_index(font.data.as_ref(), font.index).ok()?;
    let outline = f.outline_glyphs().get(GlyphId::new(glyph))?;
    let coords: Vec<NormalizedCoord> = coords.iter().map(|&c| NormalizedCoord::from_bits(c)).collect();
    let mut pen = Pen(BezPath::new());
    outline.draw(DrawSettings::unhinted(Size::unscaled(), LocationRef::new(&coords)), &mut pen).ok()?;
    Some(pen.0)
}

struct Pen(BezPath);

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to((x as f64, y as f64));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to((x as f64, y as f64));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to((cx0 as f64, cy0 as f64), (x as f64, y as f64));
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.curve_to((cx0 as f64, cy0 as f64), (cx1 as f64, cy1 as f64), (x as f64, y as f64));
    }
    fn close(&mut self) {
        self.0.close_path();
    }
}
