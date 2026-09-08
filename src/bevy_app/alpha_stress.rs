//! Alpha-blending stress fixture, behind `?bevy=alpha`.
//!
//! Exists to verify that the two vector backends composite translucency
//! identically. Semi-transparent overlap is where the backends can differ
//! without it being obvious: premultiplied-vs-straight alpha, and sRGB decode
//! applied to premultiplied colour, are both invisible at α = 1 and both wrong
//! at every other α. A scene made mostly of α = 1 shapes cannot detect either.
//!
//! # Layout, and why it is split
//!
//! - The **left two thirds** are entirely static and deterministic, so the two
//!   backends can be compared pixel-for-pixel. This is the part that proves
//!   correctness.
//! - The **right third** animates, which exercises the same blending under
//!   per-frame rebuilds but cannot be diffed across captures.
//!
//! Keeping them apart is what makes a numeric comparison possible at all; a
//! fixture that animates everywhere can only ever be eyeballed.
//!
//! # What it deliberately covers
//!
//! - every [`Shape`](crate::vector::DisplayList) variant, so the `fill_rect`
//!   fast path and the general path pipeline are both exercised;
//! - fills *and* strokes, since stroke expansion produces its own coverage;
//! - alphas from 0.06 to 0.85, including values low enough that a squared alpha
//!   would be visually obvious;
//! - deep overlap, so per-pixel error compounds rather than cancelling;
//! - translucent content **inside clip layers**, which is where Vello Hybrid
//!   routes through intermediate textures and composites them back — the path
//!   most likely to differ.

use bevy::prelude::*;
use kurbo;
use peniko;

use crate::panels::{Panels, VIEWER_PANEL, overlay_affine};
use crate::vector::{DisplayList, DisplayListRebuild, VectorLayer, order};

#[derive(Component)]
pub struct AlphaStressStatic;

#[derive(Component)]
pub struct AlphaStressAnimated;

/// Deterministic generator: the static half must be byte-identical between runs
/// or the comparison it exists to support is meaningless.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        f64::from(self.0 >> 8) / f64::from(1_u32 << 24)
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.next() * (hi - lo)
    }
}

pub fn setup_alpha_stress(mut commands: Commands) {
    commands.spawn((
        DisplayList::default(),
        VectorLayer::screen(order::ALPHA_STRESS_STATIC),
        AlphaStressStatic,
    ));
    commands.spawn((
        DisplayList::default(),
        VectorLayer::screen(order::ALPHA_STRESS_ANIMATED),
        AlphaStressAnimated,
    ));
}

/// The static field. Rebuilt only when the panel moves, thanks to content
/// comparison in `rebuild`.
pub fn render_alpha_stress_static(
    mut layers: Query<&mut DisplayList, With<AlphaStressStatic>>,
    panels: Res<Panels>,
) {
    let Ok(mut list) = layers.single_mut() else {
        return;
    };
    let Some(rect) = panels.rect(VIEWER_PANEL) else {
        list.rebuild(|_| {});
        return;
    };

    // Confined to the left two thirds; the animated layer owns the rest.
    let w = f64::from(rect.w) * 0.66;
    let h = f64::from(rect.h);
    let x0 = f64::from(rect.x);
    let y0 = f64::from(rect.y);
    let clip = rect.to_kurbo();

    list.rebuild(|b| {
        b.clipped(kurbo::Affine::IDENTITY, clip, |b| {
            // Opaque ground first. Without it the app's own animated layers show
            // through the translucent fixture, and a pixel diff between backends
            // measures the animation phase rather than the blending.
            b.fill(
                kurbo::Affine::IDENTITY,
                peniko::Color::new([0.10, 0.11, 0.13, 1.0]),
                clip,
            );

            let mut rng = Rng(0x1234_5678);

            // Broad wash of overlapping translucent fills.
            for i in 0..90 {
                let cx = x0 + rng.range(0.05, 0.95) * w;
                let cy = y0 + rng.range(0.05, 0.95) * h;
                let size = rng.range(40.0, 190.0);
                let alpha = rng.range(0.06, 0.55) as f32;
                let color = peniko::Color::new([
                    rng.range(0.1, 1.0) as f32,
                    rng.range(0.1, 1.0) as f32,
                    rng.range(0.1, 1.0) as f32,
                    alpha,
                ]);
                let t = kurbo::Affine::IDENTITY;
                match i % 4 {
                    // Axis-aligned rect: takes Vello Hybrid's `fill_rect` fast
                    // path, which is a different code path from the rest.
                    0 => b.fill(
                        t,
                        color,
                        kurbo::Rect::new(cx, cy, cx + size, cy + size * 0.6),
                    ),
                    1 => b.fill(
                        t,
                        color,
                        kurbo::RoundedRect::new(cx, cy, cx + size, cy + size * 0.8, size * 0.25),
                    ),
                    2 => b.fill(t, color, kurbo::Circle::new((cx, cy), size * 0.45)),
                    _ => {
                        // A closed cubic blob, so curve flattening is covered too.
                        let mut p = kurbo::BezPath::new();
                        let r = size * 0.5;
                        p.move_to((cx - r, cy));
                        p.curve_to((cx - r, cy - r), (cx + r, cy - r), (cx + r, cy));
                        p.curve_to((cx + r, cy + r), (cx - r, cy + r), (cx - r, cy));
                        p.close_path();
                        b.fill(t, color, p);
                    }
                }
            }

            // Translucent strokes: coverage from stroke expansion rather than fills.
            for _ in 0..22 {
                let cx = x0 + rng.range(0.05, 0.95) * w;
                let cy = y0 + rng.range(0.05, 0.95) * h;
                let size = rng.range(50.0, 200.0);
                let color = peniko::Color::new([
                    rng.range(0.2, 1.0) as f32,
                    rng.range(0.2, 1.0) as f32,
                    rng.range(0.2, 1.0) as f32,
                    rng.range(0.10, 0.65) as f32,
                ]);
                b.stroke(
                    kurbo::Affine::IDENTITY,
                    kurbo::Stroke::new(rng.range(1.0, 9.0)),
                    color,
                    kurbo::Circle::new((cx, cy), size * 0.5),
                );
            }

            // Translucent content inside a nested clip. Hybrid composites layers
            // through intermediate textures, so this is the path where an alpha
            // convention error is most likely to survive everything above.
            let inner = kurbo::Rect::new(x0 + w * 0.10, y0 + h * 0.55, x0 + w * 0.62, y0 + h * 0.95);
            b.clipped(kurbo::Affine::IDENTITY, inner, |b| {
                for k in 0..26 {
                    let f = f64::from(k);
                    let cx = inner.x0 + rng.range(0.0, 1.0) * inner.width();
                    let cy = inner.y0 + rng.range(0.0, 1.0) * inner.height();
                    b.fill(
                        kurbo::Affine::IDENTITY,
                        peniko::Color::new([
                            (0.2 + 0.8 * (f / 26.0)) as f32,
                            0.35,
                            (1.0 - f / 26.0) as f32,
                            rng.range(0.08, 0.5) as f32,
                        ]),
                        kurbo::Circle::new((cx, cy), rng.range(25.0, 85.0)),
                    );
                }
            });

            // A deliberate low-alpha stack: twelve layers of 0.12 over the same
            // spot. Correct compositing approaches opacity; a squared alpha stays
            // conspicuously faint, and the difference is easy to see by eye.
            for k in 0..12 {
                let off = f64::from(k) * 4.0;
                b.fill(
                    kurbo::Affine::IDENTITY,
                    peniko::Color::new([0.05, 0.85, 0.55, 0.12]),
                    kurbo::Rect::new(
                        x0 + w * 0.66 + off,
                        y0 + h * 0.08 + off,
                        x0 + w * 0.66 + 150.0 + off,
                        y0 + h * 0.08 + 110.0 + off,
                    ),
                );
            }
        });
    });
}

/// The animated third: same blending, but rebuilt every frame.
pub fn render_alpha_stress_animated(
    mut layers: Query<&mut DisplayList, With<AlphaStressAnimated>>,
    panels: Res<Panels>,
    time: Res<Time>,
) {
    let Ok(mut list) = layers.single_mut() else {
        return;
    };
    let Some(rect) = panels.rect(VIEWER_PANEL) else {
        list.rebuild(|_| {});
        return;
    };
    let base = overlay_affine(rect);
    let clip = rect.to_kurbo();
    let t = f64::from(time.elapsed_secs());

    // Anchored in the right third, in overlay-world coordinates (panel centre
    // origin, y-up), so the static half stays diffable.
    let cx = f64::from(rect.w) * 0.30;

    list.rebuild(|b| {
        b.clipped(kurbo::Affine::IDENTITY, clip, |b| {
            for k in 0..7 {
                let f = f64::from(k);
                let phase = t * (0.35 + f * 0.08) + f * 0.9;
                let orbit = 60.0 + f * 34.0;
                let x = cx + phase.cos() * orbit;
                let y = phase.sin() * orbit;
                let spin = kurbo::Affine::translate((x, y))
                    * kurbo::Affine::rotate(phase * 1.3)
                    * kurbo::Affine::scale(0.75 + 0.35 * (phase * 0.7).sin());
                let color = peniko::Color::new([
                    (0.5 + 0.5 * (phase).sin()) as f32,
                    (0.5 + 0.5 * (phase * 1.7).cos()) as f32,
                    (0.5 + 0.5 * (phase * 0.6).sin()) as f32,
                    (0.18 + 0.30 * (0.5 + 0.5 * (phase * 0.9).sin())) as f32,
                ]);
                b.fill(
                    base * spin,
                    color,
                    kurbo::RoundedRect::new(-95.0, -95.0, 95.0, 95.0, 30.0),
                );
                b.stroke(
                    base * spin,
                    kurbo::Stroke::new(5.0),
                    peniko::Color::new([1.0, 1.0, 1.0, 0.35]),
                    kurbo::Circle::new((0.0, 0.0), 110.0),
                );
            }
        });
    });
}
