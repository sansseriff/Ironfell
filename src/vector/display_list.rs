//! Renderer-neutral 2D drawing intent.
//!
//! This is the seam described in `plans/vision/01_layered-architecture-overview.md`
//! (Layer 4, "presentation projections"). App code describes *what* to draw; a
//! backend decides *how*. Nothing here mentions Vello, and nothing here knows
//! about Bevy rendering.
//!
//! `kurbo` and `peniko` types are used directly rather than being re-wrapped.
//! They are the Linebender ecosystem's shared geometry and style vocabulary, not
//! renderer types, so
//! wrapping would add conversion cost and no isolation.
//!
//! # Why shapes stay shapes
//!
//! [`Shape`] keeps rectangles, rounded rectangles, circles and lines distinct
//! instead of flattening everything to a [`BezPath`]. Backends have materially
//! faster paths for these: Vello Hybrid has `fill_rect` and
//! `fill_blurred_rounded_rect`, and upstream's "blit rect" work
//! ([Zulip, Feb 2026](https://xi.zulipchat.com/#narrow/channel/197075-vello))
//! turns unclipped rects into a quad blit that skips strip generation entirely.
//! Throwing that information away at the seam cannot be recovered downstream.

use bevy::prelude::*;
use kurbo;
use peniko;

use kurbo::{Affine, BezPath, Circle, Line, Rect, RoundedRect, Shape as _, Stroke};
use peniko::{Color, Fill};

/// A drawable outline, keeping its specific type so backends can take fast paths.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Rect(Rect),
    RoundedRect(RoundedRect),
    Circle(Circle),
    Line(Line),
    Path(BezPath),
}

impl From<Rect> for Shape {
    fn from(v: Rect) -> Self {
        Self::Rect(v)
    }
}
impl From<RoundedRect> for Shape {
    fn from(v: RoundedRect) -> Self {
        Self::RoundedRect(v)
    }
}
impl From<Circle> for Shape {
    fn from(v: Circle) -> Self {
        Self::Circle(v)
    }
}
impl From<Line> for Shape {
    fn from(v: Line) -> Self {
        Self::Line(v)
    }
}
impl From<BezPath> for Shape {
    fn from(v: BezPath) -> Self {
        Self::Path(v)
    }
}

impl Shape {
    /// The outline as a path.
    pub fn to_path(&self) -> BezPath {
        match self {
            Self::Rect(s) => s.to_path(0.1),
            Self::RoundedRect(s) => s.to_path(0.1),
            Self::Circle(s) => s.to_path(0.1),
            Self::Line(s) => s.to_path(0.1),
            Self::Path(s) => s.clone(),
        }
    }

    /// Untransformed bounds of the outline.
    pub fn bounding_box(&self) -> Rect {
        match self {
            Self::Rect(s) => s.bounding_box(),
            Self::RoundedRect(s) => s.bounding_box(),
            Self::Circle(s) => s.bounding_box(),
            Self::Line(s) => s.bounding_box(),
            Self::Path(s) => s.bounding_box(),
        }
    }
}

/// How a shape is coloured.
///
/// Solid colours and gradients share draw operations. Images can become variants here,
/// which is why call sites take `impl Into<Brush>` rather than a `Color`.
#[derive(Clone, Debug, PartialEq)]
pub enum Brush {
    Solid(Color),
    Gradient(peniko::Gradient),
}

impl From<peniko::Gradient> for Brush {
    fn from(v: peniko::Gradient) -> Self { Self::Gradient(v) }
}

impl From<Color> for Brush {
    fn from(v: Color) -> Self {
        Self::Solid(v)
    }
}

/// One drawing operation, in painter order.
#[derive(Clone, Debug, PartialEq)]
pub enum DrawCmd {
    Fill {
        transform: Affine,
        brush: Brush,
        fill_rule: Fill,
        shape: Shape,
    },
    Stroke {
        transform: Affine,
        style: Stroke,
        brush: Brush,
        shape: Shape,
    },
    /// Another view's texture (a 3D composition seen from 2D, ECM §7.4):
    /// its top-left `size` texels drawn into the rectangle `(0, 0)–size`
    /// under `transform`. `version` changes whenever the texture's content
    /// does, so the list compares unequal and the view re-renders.
    Texture {
        transform: Affine,
        image: Handle<Image>,
        size: UVec2,
        version: u32,
    },
    /// A rect's drop shadow, drawn analytically: a rounded rect blurred by a
    /// Gaussian of `std_dev`, with no offscreen pass (Vello's
    /// `fill_blurred_rounded_rect`, the reason [`Shape`] keeps rects).
    BlurredRect {
        transform: Affine,
        color: Color,
        rect: Rect,
        radius: f64,
        std_dev: f64,
    },
    /// Begin a layer whose content casts a drop shadow (a filter layer, for
    /// shapes the analytic shadow cannot draw). Offset and blur are in
    /// `transform`'s space; `bounds`, in the same space, covers the content
    /// and its shadow. Always balanced by [`DrawCmd::PopLayer`].
    PushShadow {
        transform: Affine,
        color: Color,
        offset: kurbo::Vec2,
        std_dev: f64,
        bounds: Rect,
    },
    /// Begin a clip layer. Always balanced by [`DrawCmd::PopLayer`].
    PushClip {
        transform: Affine,
        shape: Shape,
    },
    PushOpacity { opacity: f32, bounds: Rect },
    PopLayer,
}

/// An ordered list of drawing commands for one layer.
///
/// Held as a component and rewritten by app systems through
/// [`DisplayListRebuild::rebuild`], which only marks the component changed when
/// the content actually differs. That single behaviour is what keeps an idle
/// frame from re-encoding anything — the cheapest large win available, and the
/// reason this type owns a scratch buffer.
#[derive(Component, Default, Debug)]
pub struct DisplayList {
    cmds: Vec<DrawCmd>,
    /// Retained so a rebuild allocates nothing in the steady state.
    scratch: Vec<DrawCmd>,
}

impl DisplayList {
    pub fn cmds(&self) -> &[DrawCmd] {
        &self.cmds
    }

    /// Conservative bounds of everything this layer draws, in layer space.
    ///
    /// `None` for an empty list. Clips are treated as content rather than as
    /// intersections, so the result can be larger than what is actually painted
    /// — never smaller, which is the direction that keeps callers correct.
    ///
    /// Three things want this, in increasing order of ambition:
    ///
    /// 1. **Culling** (used today): skip a layer entirely when its bounds miss
    ///    the viewport, so an off-screen layer costs no encoding at all.
    /// 2. **Damage**: union of a layer's previous and current bounds is the
    ///    region a change actually dirtied.
    /// 3. **Rasterization**: a cached raster needs a size and a placement, and
    ///    this is where both come from.
    ///
    /// Recomputed on demand rather than cached, because it is O(commands) over
    /// data that is already hot and is only asked for once per rebuild. If that
    /// stops being true, cache it next to `cmds` and invalidate in `rebuild`.
    pub fn bounds(&self) -> Option<Rect> {
        let mut acc: Option<Rect> = None;
        for cmd in &self.cmds {
            let b = match cmd {
                DrawCmd::Fill {
                    transform, shape, ..
                }
                | DrawCmd::PushClip { transform, shape } => {
                    transform.transform_rect_bbox(shape.bounding_box())
                }
                DrawCmd::Stroke {
                    transform,
                    style,
                    shape,
                    ..
                } => {
                    // Inflate by half the stroke width before mapping, so the
                    // outset is measured in the space the width is defined in.
                    let half = style.width * 0.5;
                    transform.transform_rect_bbox(shape.bounding_box().inflate(half, half))
                }
                DrawCmd::Texture { transform, size, .. } => {
                    transform.transform_rect_bbox(Rect::new(0.0, 0.0, size.x as f64, size.y as f64))
                }
                DrawCmd::BlurredRect { transform, rect, std_dev, .. } => {
                    let pad = 3.0 * std_dev;
                    transform.transform_rect_bbox(rect.inflate(pad, pad))
                }
                DrawCmd::PushShadow { transform, bounds, .. } => transform.transform_rect_bbox(*bounds),
                DrawCmd::PushOpacity { .. } | DrawCmd::PopLayer => continue,
            };
            acc = Some(match acc {
                Some(a) => a.union(b),
                None => b,
            });
        }
        acc
    }
}

/// Records commands into a [`DisplayList`].
pub struct DisplayListBuilder<'a> {
    out: &'a mut Vec<DrawCmd>,
    depth: u32,
}

impl DisplayListBuilder<'_> {
    /// Fill `shape`, using the non-zero rule.
    pub fn fill(&mut self, transform: Affine, brush: impl Into<Brush>, shape: impl Into<Shape>) {
        self.fill_with_rule(transform, brush, Fill::NonZero, shape);
    }

    pub fn fill_with_rule(
        &mut self,
        transform: Affine,
        brush: impl Into<Brush>,
        fill_rule: Fill,
        shape: impl Into<Shape>,
    ) {
        self.out.push(DrawCmd::Fill {
            transform,
            brush: brush.into(),
            fill_rule,
            shape: shape.into(),
        });
    }

    pub fn stroke(
        &mut self,
        transform: Affine,
        style: Stroke,
        brush: impl Into<Brush>,
        shape: impl Into<Shape>,
    ) {
        self.out.push(DrawCmd::Stroke {
            transform,
            style,
            brush: brush.into(),
            shape: shape.into(),
        });
    }

    /// Draw another view's texture; see [`DrawCmd::Texture`].
    pub fn texture(&mut self, transform: Affine, image: Handle<Image>, size: UVec2, version: u32) {
        self.out.push(DrawCmd::Texture { transform, image, size, version });
    }

    /// A rect's analytic drop shadow; see [`DrawCmd::BlurredRect`].
    pub fn blurred_rect(&mut self, transform: Affine, color: Color, rect: Rect, radius: f64, std_dev: f64) {
        self.out.push(DrawCmd::BlurredRect { transform, color, rect, radius, std_dev });
    }

    /// Run `f` with what it draws casting a drop shadow; see
    /// [`DrawCmd::PushShadow`]. Scoped, as [`Self::clipped`] is.
    pub fn shadowed(&mut self, transform: Affine, color: Color, offset: kurbo::Vec2, std_dev: f64, bounds: Rect, f: impl FnOnce(&mut Self)) {
        self.out.push(DrawCmd::PushShadow { transform, color, offset, std_dev, bounds });
        self.depth += 1;
        f(self);
        self.depth -= 1;
        self.out.push(DrawCmd::PopLayer);
    }

    /// Composite the scope once, so overlapping children share one opacity.
    pub fn opacity(&mut self, opacity: f32, bounds: Rect, f: impl FnOnce(&mut Self)) {
        if opacity >= 1.0 { f(self); return; }
        if opacity <= 0.0 { return; }
        self.out.push(DrawCmd::PushOpacity { opacity, bounds });
        self.depth += 1;
        f(self);
        self.depth -= 1;
        self.out.push(DrawCmd::PopLayer);
    }

    /// Run `f` with everything it draws clipped to `shape`.
    ///
    /// Scoped rather than exposing raw push/pop, because an unbalanced pair is
    /// silent in one backend and a panic in another.
    pub fn clipped(
        &mut self,
        transform: Affine,
        shape: impl Into<Shape>,
        f: impl FnOnce(&mut Self),
    ) {
        self.out.push(DrawCmd::PushClip {
            transform,
            shape: shape.into(),
        });
        self.depth += 1;
        f(self);
        self.depth -= 1;
        self.out.push(DrawCmd::PopLayer);
    }
}

/// Rewrites a [`DisplayList`] without triggering change detection unless the
/// content actually changed.
pub trait DisplayListRebuild {
    /// Rebuild the list, returning whether it differs from the previous frame.
    ///
    /// Call sites can rebuild unconditionally every frame and still get an
    /// unchanged component when the output is identical, so backends downstream
    /// can skip the work with an ordinary `Changed<DisplayList>` filter.
    fn rebuild(&mut self, f: impl FnOnce(&mut DisplayListBuilder<'_>)) -> bool;
}

impl DisplayListRebuild for Mut<'_, DisplayList> {
    fn rebuild(&mut self, f: impl FnOnce(&mut DisplayListBuilder<'_>)) -> bool {
        let changed = {
            // Deliberately bypassed: taking `&mut` the normal way would mark the
            // component changed before we know whether anything differs.
            let list = self.bypass_change_detection();
            list.scratch.clear();
            let mut builder = DisplayListBuilder {
                out: &mut list.scratch,
                depth: 0,
            };
            f(&mut builder);
            debug_assert_eq!(
                builder.depth, 0,
                "unbalanced clip scope in display list build"
            );

            if list.scratch == list.cmds {
                false
            } else {
                core::mem::swap(&mut list.cmds, &mut list.scratch);
                true
            }
        };
        if changed {
            self.set_changed();
        }
        changed
    }
}
