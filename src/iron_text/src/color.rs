//! Colour glyphs (COLR v0 and v1: emoji, colour fonts) as renderer-neutral
//! drawing operations (plans/specs/text.md §5.1).
//!
//! skrifa walks a glyph's paint graph and calls a painter; this painter
//! records the calls as [`ColorOp`]s in font units, cached per glyph, so a
//! renderer replays them with the glyph's placement: clip to an outline,
//! fill with a solid colour or a gradient, layer with a composite mode
//! (Noto's flags mask and multiply their sheen this way). A glyph whose
//! layers are all source-over has them dropped: they would change nothing
//! and each costs an offscreen layer.

use crate::outline;
use kurbo::{Affine, BezPath, Rect, Shape as _};
use parley::FontData;
use peniko::color::{AlphaColor, Srgb};
use skrifa::color::{Brush, ColorPainter, ColorStop, CompositeMode, Extend, Transform};
use skrifa::instance::{LocationRef, NormalizedCoord};
use skrifa::raw::types::BoundingBox;
use skrifa::{FontRef, GlyphId, MetadataProvider};
use std::sync::Arc;

/// How a colour op paints.
#[derive(Clone, Debug, PartialEq)]
pub enum ColorFill {
    /// A palette colour, linear-ish sRGB RGBA as the font gives it.
    Solid([f32; 4]),
    /// The text's own colour (palette index 0xFFFF), at this alpha.
    Foreground(f32),
    Gradient(peniko::Gradient),
}

/// One recorded painter call. Paths and rects are in the op's `transform`
/// space; `transform` maps it to font units.
#[derive(Clone, Debug, PartialEq)]
pub enum ColorOp {
    PushClip { transform: Affine, path: BezPath },
    PopClip,
    Fill { transform: Affine, path: BezPath, fill: ColorFill },
    /// A group composited onto what is under it with this mode.
    PushLayer(peniko::BlendMode),
    PopLayer,
}

/// A colour glyph's ops in font units (y-up), or `None` for a glyph the
/// font draws in outline only.
pub(crate) fn ops(font: &FontData, glyph: u32, coords: &[i16], outlines: &mut outline::Cache) -> Option<Arc<Vec<ColorOp>>> {
    let f = FontRef::from_index(font.data.as_ref(), font.index).ok()?;
    let colr = f.color_glyphs();
    let g = colr.get(GlyphId::new(glyph))?;
    let palette: Vec<[f32; 4]> = f
        .color_palettes()
        .get(0)
        .map(|p| p.colors().iter().map(|c| [c.red as f32 / 255.0, c.green as f32 / 255.0, c.blue as f32 / 255.0, c.alpha as f32 / 255.0]).collect())
        .unwrap_or_default();
    let nc: Vec<NormalizedCoord> = coords.iter().map(|&c| NormalizedCoord::from_bits(c)).collect();
    let mut p = Painter { font, coords, outlines, palette, transforms: vec![Affine::IDENTITY], clips: Vec::new(), ops: Vec::new() };
    g.paint(LocationRef::new(&nc), &mut p).ok()?;
    let mut ops = p.ops;
    if !ops.iter().any(|o| matches!(o, ColorOp::PushLayer(m) if *m != peniko::BlendMode::default())) {
        ops.retain(|o| !matches!(o, ColorOp::PushLayer(_) | ColorOp::PopLayer));
    }
    Some(Arc::new(ops))
}

struct Painter<'a> {
    font: &'a FontData,
    coords: &'a [i16],
    outlines: &'a mut outline::Cache,
    palette: Vec<[f32; 4]>,
    transforms: Vec<Affine>,
    /// The current clip's bounds in font units, innermost last: what a
    /// bare `fill` covers.
    clips: Vec<Rect>,
    ops: Vec<ColorOp>,
}

fn affine(t: Transform) -> Affine {
    Affine::new([t.xx as f64, t.yx as f64, t.xy as f64, t.yy as f64, t.dx as f64, t.dy as f64])
}

impl Painter<'_> {
    fn current(&self) -> Affine {
        *self.transforms.last().expect("never empty")
    }

    fn glyph_path(&mut self, glyph: GlyphId) -> BezPath {
        self.outlines.get(self.font, glyph.to_u32(), self.coords).map(|(p, _)| (*p).clone()).unwrap_or_default()
    }

    fn color(&self, palette_index: u16, alpha: f32) -> ColorFill {
        if palette_index == 0xFFFF {
            return ColorFill::Foreground(alpha);
        }
        let c = self.palette.get(palette_index as usize).copied().unwrap_or([0.0, 0.0, 0.0, 1.0]);
        ColorFill::Solid([c[0], c[1], c[2], c[3] * alpha])
    }

    fn stops(&self, stops: &[ColorStop]) -> Vec<(f32, peniko::Color)> {
        stops
            .iter()
            .map(|s| {
                let c = match self.color(s.palette_index, s.alpha) {
                    ColorFill::Solid(c) => c,
                    _ => [0.0, 0.0, 0.0, s.alpha],
                };
                (s.offset, AlphaColor::<Srgb>::new(c))
            })
            .collect()
    }

    fn fill_value(&self, brush: Brush<'_>) -> ColorFill {
        let extend = |e: Extend| match e {
            Extend::Repeat => peniko::Extend::Repeat,
            Extend::Reflect => peniko::Extend::Reflect,
            _ => peniko::Extend::Pad,
        };
        let pt = |p: skrifa::raw::types::Point<f32>| kurbo::Point::new(p.x as f64, p.y as f64);
        match brush {
            Brush::Solid { palette_index, alpha } => self.color(palette_index, alpha),
            Brush::LinearGradient { p0, p1, color_stops, extend: e } => {
                let mut g = peniko::Gradient::new_linear(pt(p0), pt(p1)).with_stops(self.stops(color_stops).as_slice());
                g.extend = extend(e);
                ColorFill::Gradient(g)
            }
            Brush::RadialGradient { c0, r0, c1, r1, color_stops, extend: e } => {
                let mut g = peniko::Gradient::new_two_point_radial(pt(c0), r0, pt(c1), r1).with_stops(self.stops(color_stops).as_slice());
                g.extend = extend(e);
                ColorFill::Gradient(g)
            }
            Brush::SweepGradient { c0, start_angle, end_angle, color_stops, extend: e } => {
                let mut g = peniko::Gradient::new_sweep(pt(c0), start_angle.to_radians(), end_angle.to_radians()).with_stops(self.stops(color_stops).as_slice());
                g.extend = extend(e);
                ColorFill::Gradient(g)
            }
        }
    }
}

impl ColorPainter for Painter<'_> {
    fn push_transform(&mut self, transform: Transform) {
        let t = self.current() * affine(transform);
        self.transforms.push(t);
    }

    fn pop_transform(&mut self) {
        if self.transforms.len() > 1 {
            self.transforms.pop();
        }
    }

    fn push_clip_glyph(&mut self, glyph_id: GlyphId) {
        let path = self.glyph_path(glyph_id);
        let t = self.current();
        self.clips.push(t.transform_rect_bbox(path.bounding_box()));
        self.ops.push(ColorOp::PushClip { transform: t, path });
    }

    fn push_clip_box(&mut self, clip_box: BoundingBox<f32>) {
        let r = Rect::new(clip_box.x_min as f64, clip_box.y_min as f64, clip_box.x_max as f64, clip_box.y_max as f64);
        let t = self.current();
        self.clips.push(t.transform_rect_bbox(r));
        self.ops.push(ColorOp::PushClip { transform: t, path: r.to_path(0.1) });
    }

    fn pop_clip(&mut self) {
        self.clips.pop();
        self.ops.push(ColorOp::PopClip);
    }

    fn fill(&mut self, brush: Brush<'_>) {
        // The clip region, in the brush's space.
        let t = self.current();
        let area = self.clips.last().copied().unwrap_or(Rect::new(-4096.0, -4096.0, 4096.0, 4096.0));
        let local = t.inverse().transform_rect_bbox(area);
        let fill = self.fill_value(brush);
        self.ops.push(ColorOp::Fill { transform: t, path: local.to_path(0.1), fill });
    }

    /// Clip and fill in one: an outline filled, with no clip layer, which is
    /// every layer of a COLR v0 glyph.
    fn fill_glyph(&mut self, glyph_id: GlyphId, brush_transform: Option<Transform>, brush: Brush<'_>) {
        let path = self.glyph_path(glyph_id);
        let t = self.current();
        let fill = self.fill_value(brush);
        match brush_transform.map(affine) {
            // The path into the brush's space, so one transform serves both.
            Some(b) => self.ops.push(ColorOp::Fill { transform: t * b, path: b.inverse() * path, fill }),
            None => self.ops.push(ColorOp::Fill { transform: t, path, fill }),
        }
    }

    fn push_layer(&mut self, composite_mode: CompositeMode) {
        self.ops.push(ColorOp::PushLayer(blend(composite_mode)));
    }

    fn pop_layer(&mut self) {
        self.ops.push(ColorOp::PopLayer);
    }
}

/// COLR's composite modes as peniko blend modes (glifo's table).
fn blend(mode: CompositeMode) -> peniko::BlendMode {
    use peniko::{BlendMode, Compose, Mix};
    let (mix, compose) = match mode {
        CompositeMode::Clear => (Mix::Normal, Compose::Clear),
        CompositeMode::Src => (Mix::Normal, Compose::Copy),
        CompositeMode::Dest => (Mix::Normal, Compose::Dest),
        CompositeMode::DestOver => (Mix::Normal, Compose::DestOver),
        CompositeMode::SrcIn => (Mix::Normal, Compose::SrcIn),
        CompositeMode::DestIn => (Mix::Normal, Compose::DestIn),
        CompositeMode::SrcOut => (Mix::Normal, Compose::SrcOut),
        CompositeMode::DestOut => (Mix::Normal, Compose::DestOut),
        CompositeMode::SrcAtop => (Mix::Normal, Compose::SrcAtop),
        CompositeMode::DestAtop => (Mix::Normal, Compose::DestAtop),
        CompositeMode::Xor => (Mix::Normal, Compose::Xor),
        CompositeMode::Plus => (Mix::Normal, Compose::Plus),
        CompositeMode::Screen => (Mix::Screen, Compose::SrcOver),
        CompositeMode::Overlay => (Mix::Overlay, Compose::SrcOver),
        CompositeMode::Darken => (Mix::Darken, Compose::SrcOver),
        CompositeMode::Lighten => (Mix::Lighten, Compose::SrcOver),
        CompositeMode::ColorDodge => (Mix::ColorDodge, Compose::SrcOver),
        CompositeMode::ColorBurn => (Mix::ColorBurn, Compose::SrcOver),
        CompositeMode::HardLight => (Mix::HardLight, Compose::SrcOver),
        CompositeMode::SoftLight => (Mix::SoftLight, Compose::SrcOver),
        CompositeMode::Difference => (Mix::Difference, Compose::SrcOver),
        CompositeMode::Exclusion => (Mix::Exclusion, Compose::SrcOver),
        CompositeMode::Multiply => (Mix::Multiply, Compose::SrcOver),
        CompositeMode::HslHue => (Mix::Hue, Compose::SrcOver),
        CompositeMode::HslSaturation => (Mix::Saturation, Compose::SrcOver),
        CompositeMode::HslColor => (Mix::Color, Compose::SrcOver),
        CompositeMode::HslLuminosity => (Mix::Luminosity, Compose::SrcOver),
        _ => (Mix::Normal, Compose::SrcOver),
    };
    BlendMode::new(mix, compose)
}
