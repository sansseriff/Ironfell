//! Text for Iron: layout, glyph outlines, caret geometry, and the font
//! catalog (plans/specs/text.md).
//!
//! Bevy-free and renderer-free, so the document store can measure a text
//! node for bounds and picking, and the renderer can draw the same layout,
//! without either knowing about the other. Shaping and line breaking are
//! parley's; outlines are read with skrifa and handed out as `kurbo`
//! paths, which every backend already draws (fill, stroke, gradient and
//! shadow then work on text with no text-specific renderer code).
//!
//! # Fonts are a process resource
//!
//! There is one [`TextEngine`] per process ([`engine`]), like the system
//! font list of a native app: a document load replaces the store, not the
//! fonts. A host registers font bytes as they arrive ([`TextEngine::register`]);
//! a layout asks for a family it lacks by noting it as *wanted*
//! ([`TextEngine::take_wanted`]), lays out in [`catalog::DEFAULT_FAMILY`]
//! meanwhile, and reports itself [`TextLayout::pending`]. When the family
//! arrives the [`TextEngine::generation`] moves and the store re-measures.

pub mod catalog;
mod color;
mod outline;
pub mod runs;

pub use color::{ColorFill, ColorOp};
pub use runs::{RunStyle, StyleRun};

use kurbo::{Affine, BezPath, Rect};
use parley::{
    Affinity, Alignment, AlignmentOptions, Cursor, FontContext, FontFamily, FontFamilyName, FontFeatures, FontStyle, FontWeight, Layout, LayoutContext, LineHeight, PositionedLayoutItem, Selection, StyleProperty,
};
use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

pub use catalog::DEFAULT_FAMILY;

/// Horizontal alignment of lines within the text's box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

impl Align {
    pub fn parse(s: &str) -> Align {
        match s {
            "center" => Align::Center,
            "right" => Align::Right,
            _ => Align::Left,
        }
    }
}

/// Vertical placement of the laid-out text in a fixed-height box (Figma's
/// top / middle / bottom). Only a `fixed` box has room to place it in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VAlign {
    #[default]
    Top,
    Middle,
    Bottom,
}

impl VAlign {
    pub fn parse(s: &str) -> VAlign {
        match s {
            "middle" => VAlign::Middle,
            "bottom" => VAlign::Bottom,
            _ => VAlign::Top,
        }
    }

    /// How far down a `content_h`-tall layout sits in a `box_h` box.
    pub fn offset(self, content_h: f64, box_h: f64) -> f64 {
        match self {
            VAlign::Top => 0.0,
            VAlign::Middle => (box_h - content_h) * 0.5,
            VAlign::Bottom => box_h - content_h,
        }
    }
}

/// One style for a whole text node: the `font` facet's resolved leaves.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub family: String,
    pub size: f32,
    pub weight: f32,
    pub italic: bool,
    /// Percent of the font size; zero or less is Figma's "Auto" (the
    /// font's own ascent, descent and gap).
    pub line_height: f32,
    /// Percent of the font size, as Figma shows it.
    pub letter_spacing: f32,
    pub align: Align,
    /// Tabular figures (`tnum`), so a live readout's digits do not jitter.
    pub tabular: bool,
    pub underline: bool,
    pub strike: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle { family: DEFAULT_FAMILY.to_owned(), size: 24.0, weight: 400.0, italic: false, line_height: 0.0, letter_spacing: 0.0, align: Align::Left, tabular: false, underline: false, strike: false }
    }
}

/// The cache key for a style: floats by their bits.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct StyleKey {
    family: String,
    size: u32,
    weight: u32,
    italic: bool,
    line_height: u32,
    letter_spacing: u32,
    align: Align,
    tabular: bool,
    underline: bool,
    strike: bool,
}

impl From<&TextStyle> for StyleKey {
    fn from(s: &TextStyle) -> Self {
        StyleKey {
            family: s.family.clone(),
            size: s.size.to_bits(),
            weight: s.weight.to_bits(),
            italic: s.italic,
            line_height: s.line_height.to_bits(),
            letter_spacing: s.letter_spacing.to_bits(),
            align: s.align,
            tabular: s.tabular,
            underline: s.underline,
            strike: s.strike,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct LayoutKey {
    text: String,
    style: StyleKey,
    /// The style runs, as their JSON.
    runs: String,
    /// `None`: no wrapping (auto width).
    width: Option<u32>,
}

/// What a text draws, in its local space and painter order.
#[derive(Clone, Debug, PartialEq)]
pub enum TextPaint {
    /// Outline glyphs and their underlines and strikethroughs, filled with
    /// the node's paint (`None`), or with a run's colour.
    Fill { color: Option<[f32; 4]>, path: Arc<BezPath> },
    /// A colour glyph (emoji): its ops in font units, placed by `transform`;
    /// `foreground` is what its "text colour" layers take (`None`: the
    /// node's fill).
    Color { transform: Affine, ops: Arc<Vec<ColorOp>>, foreground: Option<[f32; 4]> },
}

/// Where one shaped run sits: for SVG export's `<tspan>`s.
#[derive(Clone, Debug, PartialEq)]
pub struct RunPlacement {
    /// Byte range of the text.
    pub range: std::ops::Range<usize>,
    pub x: f64,
    pub baseline: f64,
    pub advance: f64,
}

/// One laid-out text: its extent, what it paints, and what the caret
/// needs. Shared (`Arc`) between the store's bounds and the renderer.
pub struct TextLayout {
    /// The box the text occupies, from its origin (top-left): wrap width
    /// or widest line, by the sum of line heights.
    pub width: f64,
    pub height: f64,
    /// Every outline glyph, positioned, in the text's local space, whatever
    /// its colour (previews and bounds use it).
    pub outline: Arc<BezPath>,
    /// What to draw, in order.
    pub paints: Arc<Vec<TextPaint>>,
    /// Laid out in a stand-in because the requested family has not loaded.
    pub pending: bool,
    /// The requested family is in no catalog: it will not load.
    pub missing: bool,
    /// The engine generation it was laid out under.
    pub generation: u64,
    layout: Layout<u32>,
    text: String,
    text_len: usize,
}

impl std::fmt::Debug for TextLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextLayout").field("width", &self.width).field("height", &self.height).field("pending", &self.pending).finish()
    }
}

impl PartialEq for TextLayout {
    /// Layouts are compared by identity: equal inputs share one `Arc`.
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl TextLayout {
    pub fn bounds(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width, self.height)
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text cut into pieces for SVG `<tspan>`s: each line, split at
    /// `breaks` (style-run boundaries), with where the piece starts (its
    /// caret's x) and its line's baseline. Line breaks are left out.
    pub fn segments(&self, breaks: &[usize]) -> Vec<(std::ops::Range<usize>, f64, f64)> {
        let mut out = Vec::new();
        for line in self.layout.lines() {
            let r = line.text_range();
            let baseline = line.metrics().baseline as f64;
            let mut cuts: Vec<usize> = breaks.iter().copied().filter(|&b| b > r.start && b < r.end).collect();
            cuts.push(r.end);
            let mut at = r.start;
            for end in cuts {
                let piece = self.text[at..end].trim_end_matches(['\n', '\r']);
                if !piece.is_empty() {
                    let x = self.caret(at).x0;
                    out.push((at..at + piece.len(), x, baseline));
                }
                at = end;
            }
        }
        out
    }

    /// Every shaped run, in visual order line by line.
    pub fn run_placements(&self) -> Vec<RunPlacement> {
        let mut out = Vec::new();
        for line in self.layout.lines() {
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(run) = item {
                    out.push(RunPlacement { range: run.run().text_range(), x: run.offset() as f64, baseline: run.baseline() as f64, advance: run.advance() as f64 });
                }
            }
        }
        out
    }

    /// The byte offset nearest a point in the text's local space.
    pub fn hit(&self, x: f64, y: f64) -> usize {
        Cursor::from_point(&self.layout, x as f32, y as f32).index().min(self.text_len)
    }

    /// The caret at byte offset `index`: a thin rect the line's height.
    pub fn caret(&self, index: usize) -> Rect {
        let c = Cursor::from_byte_index(&self.layout, index.min(self.text_len), Affinity::Downstream);
        let b = c.geometry(&self.layout, 1.0);
        let r = Rect::new(b.x0, b.y0, b.x1, b.y1);
        if r.height() <= 0.0 { Rect::new(0.0, 0.0, 1.0, self.height.max(1.0)) } else { r }
    }

    /// The highlight rects of the selection from `anchor` to `focus`.
    pub fn selection(&self, anchor: usize, focus: usize) -> Vec<Rect> {
        if anchor == focus {
            return Vec::new();
        }
        self.parley_selection(anchor, focus).geometry(&self.layout).into_iter().map(|(b, _)| Rect::new(b.x0, b.y0, b.x1, b.y1)).collect()
    }

    /// Move a selection (`anchor`, `focus`) by a caret motion; `extend`
    /// keeps the anchor. Returns the new pair.
    pub fn motion(&self, anchor: usize, focus: usize, motion: Motion, extend: bool) -> (usize, usize) {
        let s = self.parley_selection(anchor, focus);
        let l = &self.layout;
        let s = match motion {
            Motion::Left => s.previous_visual(l, extend),
            Motion::Right => s.next_visual(l, extend),
            Motion::WordLeft => s.previous_visual_word(l, extend),
            Motion::WordRight => s.next_visual_word(l, extend),
            Motion::Up => s.previous_line(l, extend),
            Motion::Down => s.next_line(l, extend),
            Motion::LineStart => s.line_start(l, extend),
            Motion::LineEnd => s.line_end(l, extend),
        };
        // A collapsed selection moved without `extend` jumps to the side
        // it moved toward only when it was a range; parley does that.
        (s.anchor().index().min(self.text_len), s.focus().index().min(self.text_len))
    }

    /// The word around a point, for a double click.
    pub fn word_at(&self, x: f64, y: f64) -> (usize, usize) {
        let s = Selection::word_from_point(&self.layout, x as f32, y as f32);
        (s.anchor().index().min(self.text_len), s.focus().index().min(self.text_len))
    }

    fn parley_selection(&self, anchor: usize, focus: usize) -> Selection {
        let at = |i: usize| Cursor::from_byte_index(&self.layout, i.min(self.text_len), Affinity::Downstream);
        Selection::new(at(anchor), at(focus))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    WordLeft,
    WordRight,
    Up,
    Down,
    LineStart,
    LineEnd,
}

/// The process's fonts and layout caches. See the crate docs.
pub struct TextEngine {
    inner: Mutex<Inner>,
}

struct Inner {
    fonts: FontContext,
    layout_cx: LayoutContext<u32>,
    /// Lower-cased family names registered so far.
    loaded: BTreeSet<String>,
    /// Catalog families a layout asked for and the host has not yet sent.
    wanted: BTreeSet<String>,
    /// Requested families already handed to the host.
    requested: BTreeSet<String>,
    layouts: HashMap<LayoutKey, Arc<TextLayout>>,
    outlines: outline::Cache,
    /// Colour glyphs' ops by (font, glyph, coordinates); `None` for an
    /// outline glyph. And which fonts have colour glyphs at all.
    colors: HashMap<(u64, u32, u32, Vec<i16>), Option<Arc<Vec<ColorOp>>>>,
    has_colr: HashMap<(u64, u32), bool>,
    generation: u64,
    /// Installed fonts are enumerated (the `system` feature).
    system: bool,
}

/// The most layouts kept. A live readout makes a new string most frames;
/// past this the cache starts over rather than tracking recency.
const LAYOUT_CACHE: usize = 4096;

pub fn engine() -> &'static TextEngine {
    static ENGINE: OnceLock<TextEngine> = OnceLock::new();
    ENGINE.get_or_init(TextEngine::new)
}

impl TextEngine {
    fn new() -> TextEngine {
        let system = cfg!(feature = "system");
        let fonts = FontContext { collection: parley::fontique::Collection::new(parley::fontique::CollectionOptions { shared: false, system_fonts: system }), source_cache: Default::default() };
        TextEngine {
            inner: Mutex::new(Inner {
                fonts,
                layout_cx: LayoutContext::new(),
                loaded: BTreeSet::new(),
                wanted: BTreeSet::new(),
                requested: BTreeSet::new(),
                layouts: HashMap::new(),
                outlines: outline::Cache::default(),
                colors: HashMap::new(),
                has_colr: HashMap::new(),
                generation: 0,
                system,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Register a font file's bytes; returns the family names it holds.
    /// Every cached layout is dropped and the generation moves, so text
    /// waiting on this family re-measures.
    pub fn register(&self, bytes: Vec<u8>) -> Vec<String> {
        let mut inner = self.lock();
        let added = inner.fonts.collection.register_fonts(parley::fontique::Blob::from(bytes), None);
        let mut names = Vec::new();
        for (family, _) in added {
            if let Some(name) = inner.fonts.collection.family_name(family).map(str::to_owned) {
                inner.loaded.insert(name.to_lowercase());
                inner.wanted.remove(&name.to_lowercase());
                names.push(name);
            }
        }
        inner.layouts.clear();
        inner.generation += 1;
        names
    }

    /// Whether `name` can be drawn now: registered, or installed.
    pub fn has_family(&self, name: &str) -> bool {
        self.lock().available(name)
    }

    /// Installed families, sorted; empty without the `system` feature.
    pub fn system_families(&self) -> Vec<String> {
        let mut inner = self.lock();
        if !inner.system {
            return Vec::new();
        }
        let mut names: Vec<String> = inner.fonts.collection.family_names().filter(|n| !n.starts_with('.')).map(str::to_owned).collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup();
        names
    }

    /// Moves whenever the set of fonts changes.
    pub fn generation(&self) -> u64 {
        self.lock().generation
    }

    /// Ask for a family before any layout needs it (the picker's hover, or
    /// the default font at startup).
    pub fn want(&self, name: &str) {
        let mut inner = self.lock();
        inner.want(name);
    }

    /// Catalog families wanted and not yet handed out, in request order of
    /// priority: the caller fetches these. Each is returned once.
    pub fn take_wanted(&self) -> Vec<&'static catalog::Family> {
        let mut inner = self.lock();
        let wanted: Vec<String> = std::mem::take(&mut inner.wanted).into_iter().collect();
        let mut out = Vec::new();
        for w in wanted {
            if inner.requested.insert(w.clone()) {
                if let Some(f) = catalog::family(&w) {
                    out.push(f);
                }
            }
        }
        out
    }

    /// Lay out `text` in `style` with `runs` over it, wrapping at `width`
    /// when given.
    pub fn layout(&self, text: &str, style: &TextStyle, runs: &[StyleRun], width: Option<f64>) -> Arc<TextLayout> {
        let key = LayoutKey { text: text.to_owned(), style: style.into(), runs: runs::to_json(runs), width: width.map(|w| (w.max(0.0) as f32).to_bits()) };
        let mut inner = self.lock();
        if let Some(l) = inner.layouts.get(&key) {
            return l.clone();
        }
        let built = Arc::new(inner.build(text, style, runs, width));
        if inner.layouts.len() >= LAYOUT_CACHE {
            inner.layouts.clear();
        }
        inner.layouts.insert(key, built.clone());
        built
    }
}

impl Inner {
    fn available(&mut self, name: &str) -> bool {
        self.loaded.contains(&name.to_lowercase()) || (self.system && self.fonts.collection.family_id(name).is_some())
    }

    fn want(&mut self, name: &str) {
        let lower = name.to_lowercase();
        if self.available(name) {
            return;
        }
        if !self.loaded.contains(&lower) && !self.requested.contains(&lower) && catalog::family(name).is_some() {
            self.wanted.insert(lower);
        }
    }

    fn build(&mut self, text: &str, style: &TextStyle, runs: &[StyleRun], width: Option<f64>) -> TextLayout {
        let have = self.available(&style.family);
        let missing = catalog::family(&style.family).is_none() && !have;
        let mut pending = !have && !missing;
        self.want(&style.family);
        self.want(DEFAULT_FAMILY);
        // A run's family, and the emoji face when there are emoji, load the
        // same way: on demand, the text redrawn when they arrive.
        let mut extra: Vec<&str> = runs.iter().filter_map(|r| r.style.family.as_deref()).collect();
        extra.extend(catalog::emoji::needed(text).into_iter().map(|f| f.name));
        for f in extra {
            self.want(f);
            pending |= !self.available(f) && catalog::family(f).is_some();
        }
        let generation = self.generation;
        let size = style.size.max(0.5);
        // Nothing loaded at all (the first frames): an estimate, so bounds
        // and picking work, and no glyphs.
        if self.loaded.is_empty() && !self.system {
            let lines: Vec<&str> = text.split('\n').collect();
            let widest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f64 * size as f64 * 0.55;
            let w = width.unwrap_or(widest).max(1.0);
            let h = lines.len() as f64 * size as f64 * 1.2;
            return TextLayout { width: w, height: h, outline: Arc::new(BezPath::new()), paints: Arc::new(Vec::new()), pending: true, missing, generation, layout: Layout::new(), text: text.to_owned(), text_len: text.len() };
        }
        // Each family falls back to the default face, then to the emoji
        // chunks (unloaded ones are simply absent).
        let list = |family: &str| -> FontFamily<'static> {
            let mut names = vec![FontFamilyName::Named(Cow::Owned(family.to_owned())), FontFamilyName::Named(Cow::Borrowed(DEFAULT_FAMILY))];
            names.extend(catalog::emoji::families().map(|f| FontFamilyName::Named(Cow::Borrowed(f))));
            FontFamily::List(Cow::Owned(names))
        };
        // Brush 0 is the node's own paint; a run's colour gets an index.
        let mut colors: Vec<Option<[f32; 4]>> = vec![None];
        let Inner { fonts, layout_cx, outlines, colors: color_cache, has_colr, .. } = self;
        let mut b = layout_cx.ranged_builder(fonts, text, 1.0, false);
        b.push_default(StyleProperty::FontFamily(list(&style.family)));
        b.push_default(StyleProperty::FontSize(size));
        b.push_default(StyleProperty::FontWeight(FontWeight::new(style.weight)));
        if style.italic {
            b.push_default(StyleProperty::FontStyle(FontStyle::Italic));
        }
        b.push_default(StyleProperty::LineHeight(if style.line_height > 0.0 { LineHeight::FontSizeRelative(style.line_height / 100.0) } else { LineHeight::MetricsRelative(1.0) }));
        if style.letter_spacing != 0.0 {
            b.push_default(StyleProperty::LetterSpacing(style.letter_spacing / 100.0 * size));
        }
        if style.tabular {
            b.push_default(StyleProperty::FontFeatures(FontFeatures::Source(Cow::Borrowed("\"tnum\" 1"))));
        }
        b.push_default(StyleProperty::Underline(style.underline));
        b.push_default(StyleProperty::Strikethrough(style.strike));
        for r in runs {
            let range = r.start.min(text.len())..r.end.min(text.len());
            if range.is_empty() {
                continue;
            }
            let st = &r.style;
            if let Some(f) = &st.family {
                b.push(StyleProperty::FontFamily(list(f)), range.clone());
            }
            if let Some(px) = st.size {
                let px = px.max(0.5);
                b.push(StyleProperty::FontSize(px), range.clone());
                if style.letter_spacing != 0.0 {
                    b.push(StyleProperty::LetterSpacing(style.letter_spacing / 100.0 * px), range.clone());
                }
            }
            if let Some(w) = st.weight {
                b.push(StyleProperty::FontWeight(FontWeight::new(w)), range.clone());
            }
            if let Some(i) = st.italic {
                b.push(StyleProperty::FontStyle(if i { FontStyle::Italic } else { FontStyle::Normal }), range.clone());
            }
            if let Some(c) = st.color {
                colors.push(Some(c));
                b.push(StyleProperty::Brush((colors.len() - 1) as u32), range.clone());
            }
            if let Some(u) = st.underline {
                b.push(StyleProperty::Underline(u), range.clone());
            }
            if let Some(k) = st.strike {
                b.push(StyleProperty::Strikethrough(k), range.clone());
            }
        }
        let mut layout: Layout<u32> = b.build(text);
        layout.break_all_lines(width.map(|w| w as f32));
        let alignment = match style.align {
            Align::Left => Alignment::Left,
            Align::Center => Alignment::Center,
            Align::Right => Alignment::Right,
        };
        layout.align(alignment, AlignmentOptions { align_when_overflowing: true });

        // Outline glyphs and decorations gather by brush; colour glyphs are
        // painted where they fall.
        let mut fills: Vec<BezPath> = vec![BezPath::new(); colors.len()];
        let mut all = BezPath::new();
        let mut color_paints = Vec::new();
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(run) = item else { continue };
                let r = run.run();
                let font = r.font();
                let px = r.font_size() as f64;
                let skew = r.synthesis().skew().map(|deg| (deg as f64).to_radians().tan());
                let coords = r.normalized_coords();
                let brush = run.style().brush as usize;
                let colr = *has_colr.entry((font.data.id(), font.index)).or_insert_with(|| skrifa::FontRef::from_index(font.data.as_ref(), font.index).is_ok_and(|f| skrifa::raw::TableProvider::colr(&f).is_ok()));
                for g in run.positioned_glyphs() {
                    let place = |upem: f64| {
                        let s = px / upem;
                        // Font units are y-up; the canvas is y-down.
                        let mut t = Affine::new([s, 0.0, 0.0, -s, g.x as f64, g.y as f64]);
                        if let Some(k) = skew {
                            t = Affine::translate((g.x as f64, g.y as f64)) * Affine::new([1.0, 0.0, -k, 1.0, 0.0, 0.0]) * Affine::translate((-(g.x as f64), -(g.y as f64))) * t;
                        }
                        t
                    };
                    if colr {
                        let key = (font.data.id(), font.index, g.id, coords.to_vec());
                        let ops = match color_cache.get(&key) {
                            Some(o) => o.clone(),
                            None => {
                                let o = color::ops(font, g.id, coords, outlines);
                                color_cache.insert(key, o.clone());
                                o
                            }
                        };
                        if let Some(ops) = ops {
                            let upem = skrifa::FontRef::from_index(font.data.as_ref(), font.index).map(|f| skrifa::MetadataProvider::metrics(&f, skrifa::instance::Size::unscaled(), skrifa::instance::LocationRef::default()).units_per_em as f64).unwrap_or(1000.0);
                            color_paints.push(TextPaint::Color { transform: place(upem.max(1.0)), ops, foreground: colors.get(brush).copied().flatten() });
                            continue;
                        }
                    }
                    let Some((glyph, upem)) = outlines.get(font, g.id, coords) else { continue };
                    let placed = place(upem) * &*glyph;
                    all.extend(placed.elements().iter().copied());
                    if let Some(f) = fills.get_mut(brush) {
                        f.extend(placed.elements().iter().copied());
                    }
                }
                // Underline and strikethrough: rects along the run, in the
                // decoration's brush, at the run's metrics unless set.
                let m = r.metrics();
                let (x0, x1, base) = (run.offset() as f64, (run.offset() + run.advance()) as f64, run.baseline() as f64);
                let st = run.style();
                for (d, off, thick) in [(&st.underline, m.underline_offset, m.underline_size), (&st.strikethrough, m.strikethrough_offset, m.strikethrough_size)] {
                    let Some(d) = d else { continue };
                    let y = base - d.offset.unwrap_or(off) as f64;
                    let h = (d.size.unwrap_or(thick) as f64).max(0.5);
                    let rect = kurbo::Shape::to_path(&Rect::new(x0, y, x1, y + h), 0.1);
                    all.extend(rect.elements().iter().copied());
                    if let Some(f) = fills.get_mut(d.brush as usize) {
                        f.extend(rect.elements().iter().copied());
                    }
                }
            }
        }
        let mut paints: Vec<TextPaint> = fills.into_iter().zip(colors).filter(|(p, _)| !p.elements().is_empty()).map(|(p, c)| TextPaint::Fill { color: c, path: Arc::new(p) }).collect();
        paints.extend(color_paints);
        // The furthest any run reaches: parley's own width under-counts a
        // ZWJ ligature (its joined clusters measure partly as trailing
        // space), which would leave an emoji family outside the box.
        let reach = layout.lines().flat_map(|l| l.items().filter_map(|i| match i { PositionedLayoutItem::GlyphRun(r) => Some((r.offset() + r.advance()) as f64), _ => None }).collect::<Vec<_>>()).fold(0.0, f64::max);
        let content_w = (layout.full_width() as f64).max(reach);
        TextLayout {
            width: width.unwrap_or(content_w).max(1.0),
            height: (layout.height() as f64).max(size as f64 * 1.2),
            outline: Arc::new(all),
            paints: Arc::new(paints),
            pending,
            missing,
            generation,
            layout,
            text: text.to_owned(),
            text_len: text.len(),
        }
    }
}

/// Whether a character asks for the emoji face: pictographs, symbols that
/// emoji fonts draw, and the joiners and selectors of emoji sequences.
pub fn is_emoji(c: char) -> bool {
    matches!(c as u32,
        0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0x2300..=0x23FF | 0xFE0F | 0x200D | 0xE0020..=0xE007F)
}

/// One family's name drawn in its own face: the picker's preview. The path
/// is in px, baseline-aligned in a box `size` high.
pub fn preview(name: &str, size: f32) -> Option<(BezPath, f64)> {
    if !engine().has_family(name) {
        return None;
    }
    let style = TextStyle { family: name.to_owned(), size, ..TextStyle::default() };
    let l = engine().layout(name, &style, &[], None);
    Some(((*l.outline).clone(), l.width))
}

/// A path as SVG path data, to a tenth of a px: compact enough to ship
/// one per family in the picker.
pub fn svg_path(path: &BezPath) -> String {
    use kurbo::PathEl;
    use std::fmt::Write;
    let n = |v: f64| (v * 10.0).round() / 10.0;
    let mut d = String::new();
    for el in path.elements() {
        let _ = match *el {
            PathEl::MoveTo(p) => write!(d, "M{} {}", n(p.x), n(p.y)),
            PathEl::LineTo(p) => write!(d, "L{} {}", n(p.x), n(p.y)),
            PathEl::QuadTo(a, p) => write!(d, "Q{} {} {} {}", n(a.x), n(a.y), n(p.x), n(p.y)),
            PathEl::CurveTo(a, b, p) => write!(d, "C{} {} {} {} {} {}", n(a.x), n(a.y), n(b.x), n(b.y), n(p.x), n(p.y)),
            PathEl::ClosePath => write!(d, "Z"),
        };
    }
    d
}

/// Previews for the named families that can be drawn now, as the JSON
/// object `previews.json` holds: `{name: {d, w, h}}`. The native host uses
/// it for installed fonts, which no build step could know.
pub fn previews_json(names: &[String], size: f32) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for name in names {
        if let Some((path, w)) = preview(name, size) {
            let style = TextStyle { family: name.clone(), size, ..TextStyle::default() };
            let h = engine().layout(name, &style, &[], None).height;
            out.insert(name.clone(), serde_json::json!({ "d": svg_path(&path), "w": (w * 10.0).ceil() / 10.0, "h": (h * 10.0).ceil() / 10.0 }));
        }
    }
    serde_json::Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inter() {
        static ONCE: OnceLock<()> = OnceLock::new();
        ONCE.get_or_init(|| {
            let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../public/fonts/Inter.ttf")).expect("public/fonts/Inter.ttf");
            assert_eq!(engine().register(bytes), vec!["Inter".to_owned()]);
        });
    }

    #[test]
    fn lays_out_and_outlines() {
        inter();
        let l = engine().layout("Hello", &TextStyle::default(), &[], None);
        assert!(!l.pending && !l.missing);
        assert!(l.width > 40.0 && l.width < 90.0, "width {}", l.width);
        assert!(l.height > 24.0 && l.height < 40.0, "height {}", l.height);
        let b = kurbo::Shape::bounding_box(&*l.outline);
        assert!(b.x0 >= 0.0 && b.x1 <= l.width + 1.0 && b.y0 >= 0.0 && b.y1 <= l.height, "{b:?}");
    }

    #[test]
    fn wraps_at_width() {
        inter();
        let one = engine().layout("one two three four", &TextStyle::default(), &[], None);
        let wrapped = engine().layout("one two three four", &TextStyle::default(), &[], Some(80.0));
        assert_eq!(wrapped.width, 80.0);
        assert!(wrapped.height > one.height * 2.0);
    }

    #[test]
    fn cached_by_input() {
        inter();
        let a = engine().layout("same", &TextStyle::default(), &[], None);
        let b = engine().layout("same", &TextStyle::default(), &[], None);
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn unknown_family_is_missing_and_falls_back() {
        inter();
        let style = TextStyle { family: "No Such Face".into(), ..TextStyle::default() };
        let l = engine().layout("abc", &style, &[], None);
        assert!(l.missing && !l.pending);
        assert!(!l.outline.elements().is_empty());
    }

    #[test]
    fn catalog_family_not_loaded_is_pending_and_wanted() {
        inter();
        let style = TextStyle { family: "Lora".into(), ..TextStyle::default() };
        let l = engine().layout("abc", &style, &[], None);
        assert!(l.pending);
        assert!(engine().take_wanted().iter().any(|f| f.name == "Lora"));
    }

    #[test]
    fn caret_and_hit_agree() {
        inter();
        let l = engine().layout("abcdef", &TextStyle::default(), &[], None);
        let c = l.caret(3);
        assert_eq!(l.hit(c.x0 + 0.5, c.center().y), 3);
        assert_eq!(l.motion(3, 3, Motion::Right, false), (4, 4));
        assert_eq!(l.motion(3, 3, Motion::Right, true), (3, 4));
        assert_eq!(l.selection(1, 4).len(), 1);
    }

    #[cfg(feature = "system")]
    #[test]
    fn installed_fonts_lay_out_without_registering() {
        let names = engine().system_families();
        assert!(names.len() > 20, "{} installed families", names.len());
        let family = ["Helvetica", "Arial", "DejaVu Sans"].into_iter().find(|f| engine().has_family(f)).expect("a common installed face");
        let style = TextStyle { family: family.into(), ..TextStyle::default() };
        let l = engine().layout("installed", &style, &[], None);
        assert!(!l.pending && !l.missing && !l.outline.elements().is_empty());
    }

    #[test]
    fn runs_colour_and_decorate() {
        inter();
        let red = [1.0, 0.0, 0.0, 1.0];
        let runs = vec![StyleRun { start: 0, end: 5, style: RunStyle { color: Some(red), underline: Some(true), ..Default::default() } }];
        let l = engine().layout("Hello world", &TextStyle::default(), &runs, None);
        let fills: Vec<_> = l.paints.iter().filter_map(|p| match p { TextPaint::Fill { color, path } => Some((*color, path.clone())), _ => None }).collect();
        assert_eq!(fills.len(), 2, "node paint and red");
        let (_, redpath) = fills.iter().find(|(c, _)| *c == Some(red)).unwrap();
        // The underline runs under "Hello" only: a rect below the baseline.
        let b = kurbo::Shape::bounding_box(&**redpath);
        let plain = kurbo::Shape::bounding_box(&*engine().layout("Hello", &TextStyle::default(), &[], None).outline);
        assert!(b.y1 > plain.y1, "underline below the glyphs: {b:?} vs {plain:?}");
        // Strikethrough over the whole node.
        let style = TextStyle { strike: true, ..TextStyle::default() };
        let struck = engine().layout("Hello", &style, &[], None);
        assert!(struck.outline.elements().len() > engine().layout("Hello", &TextStyle::default(), &[], None).outline.elements().len());
    }

    #[test]
    fn emoji_are_colour_glyphs() {
        inter();
        static EMOJI: OnceLock<()> = OnceLock::new();
        EMOJI.get_or_init(|| {
            // Only the chunk the text needs.
            let chunk = catalog::emoji::single('🎉').expect("🎉 has a chunk");
            let bytes = std::fs::read(format!("{}/../../public/fonts/{}", env!("CARGO_MANIFEST_DIR"), chunk.file)).unwrap();
            assert_eq!(engine().register(bytes), vec![chunk.name.to_owned()]);
        });
        let l = engine().layout("hi 🎉", &TextStyle::default(), &[], None);
        assert!(!l.pending);
        let colour: Vec<_> = l.paints.iter().filter_map(|p| match p { TextPaint::Color { ops, .. } => Some(ops.clone()), _ => None }).collect();
        assert_eq!(colour.len(), 1, "one colour glyph");
        let fills = colour[0].iter().filter(|o| matches!(o, ColorOp::Fill { fill: ColorFill::Solid(_), .. })).count();
        assert!(catalog::emoji::needed("👋🏽").len() == 2, "a toned emoji needs its sequence chunk too");
        assert!(catalog::emoji::needed("🎉").len() == 1);
        assert!(catalog::emoji::needed("🏳️‍🌈").iter().any(|f| f.name.contains("Seq")), "a selector between flag and joiner");
        // COLR v1: gradients and clips, not just flat layers.
        let gradients = colour[0].iter().filter(|o| matches!(o, ColorOp::Fill { fill: ColorFill::Gradient(_), .. })).count();
        assert!(gradients > 0 || fills > 1, "a painted glyph: {fills} solid, {gradients} gradient fills");
    }

    /// Colour glyphs, sequences included, paint inside the measured box.
    #[test]
    fn emoji_sequences_fit_their_box() {
        inter();
        for c in ['😀', '🏳', '🌈', '🇯', '🇧', '👨', '👩', '👧', '🥰', '🔥', '👋'] {
            for f in [catalog::emoji::single(c), catalog::emoji::sequence(c)].into_iter().flatten() {
                if !engine().has_family(f.name) {
                    engine().register(std::fs::read(format!("{}/../../public/fonts/{}", env!("CARGO_MANIFEST_DIR"), f.file)).unwrap());
                }
            }
        }
        for text in ["😀🏳️‍🌈", "🏳️‍🌈", "🇯🇵", "👨‍👩‍👧", "😀🥰🔥👋🏽🇯🇵🇧🇷🏳️‍🌈👨‍👩‍👧"] {
            let l = engine().layout(text, &TextStyle { size: 100.0, ..TextStyle::default() }, &[], None);
            assert!(!l.pending, "{text} pending");
            let mut right = 0.0f64;
            let mut count = 0;
            for p in l.paints.iter() {
                if let TextPaint::Color { transform, ops, .. } = p {
                    count += 1;
                    for o in ops.iter() {
                        if let ColorOp::Fill { transform: t, path, .. } | ColorOp::PushClip { transform: t, path } = o {
                            right = right.max((*transform * *t).transform_rect_bbox(kurbo::Shape::bounding_box(path)).x1);
                        }
                    }
                }
            }
            println!("{text}: width {} paints {count} right {right}", l.width);
            let expected = match text { "😀🏳️‍🌈" => 2, "😀🥰🔥👋🏽🇯🇵🇧🇷🏳️‍🌈👨‍👩‍👧" => 8, _ => 1 };
            assert_eq!(count, expected, "one colour glyph per emoji or sequence: {text}");
            assert!(right <= l.width + 1.0, "{text} paints to {right}, past its width {}", l.width);
        }
    }
}
