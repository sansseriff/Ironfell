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
/// Only solid colour is used today. Gradients and images become variants here,
/// which is why call sites take `impl Into<Brush>` rather than a `Color`.
#[derive(Clone, Debug, PartialEq)]
pub enum Brush {
    Solid(Color),
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
    /// Begin a clip layer. Always balanced by [`DrawCmd::PopLayer`].
    PushClip {
        transform: Affine,
        shape: Shape,
    },
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
                DrawCmd::PopLayer => continue,
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
