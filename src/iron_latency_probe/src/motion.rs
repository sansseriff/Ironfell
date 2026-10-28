//! Opt-in `IRON_BEVY_MOTION=1`: full-height bars sweeping across the window, for
//! judging tearing and smoothness by eye. A tear shows as a horizontal break in
//! the bars; a dropped or repeated frame as a hitch in the sweep.
//!
//! Motion is a function of the time the frame is meant to be seen: the pacer's
//! planned vsync when pacing, otherwise the frame's start. Both are host time, in
//! f64: host time is ~10⁵ s, where f32 resolves only ~16 ms.
use bevy::prelude::*;

const BAR_SPACING: f32 = 96.0;
const BAR_WIDTH: f32 = 32.0;
/// Logical px per second: 8 px per 60 Hz refresh.
const BAR_SPEED: f32 = 480.0;

#[derive(Component)]
struct Bar(usize);

pub fn install(app: &mut App) {
    if std::env::var_os("IRON_BEVY_MOTION").is_none() {
        return;
    }
    app.add_systems(Startup, spawn).add_systems(Update, animate);
}

fn spawn(mut commands: Commands, windows: Query<&Window>) {
    let width = windows.single().map_or(1280.0, |w| w.width());
    let bars = (width / BAR_SPACING).ceil() as usize + 2;
    for i in 0..bars {
        commands.spawn((
            Sprite::from_color(Color::srgb(0.12, 0.12, 0.16), Vec2::new(BAR_WIDTH, 4000.0)),
            Transform::from_xyz(0.0, 0.0, -1.0),
            Bar(i),
        ));
    }
}

#[cfg(target_os = "macos")]
fn shown_at(_time: &Time) -> f64 {
    crate::pacer::planned_vsync().unwrap_or_else(|| objc2_quartz_core::CACurrentMediaTime())
}

#[cfg(not(target_os = "macos"))]
fn shown_at(time: &Time) -> f64 {
    time.elapsed_secs_f64()
}

fn animate(
    time: Res<Time>,
    windows: Query<&Window>,
    mut start: Local<Option<f64>>,
    mut bars: Query<(&Bar, &mut Transform)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let now = shown_at(&time);
    let t = now - *start.get_or_insert(now);
    let w = window.width();
    let span = bars.iter().count() as f64 * BAR_SPACING as f64;
    for (Bar(i), mut transform) in &mut bars {
        let x = (*i as f64 * BAR_SPACING as f64 + t * BAR_SPEED as f64).rem_euclid(span);
        transform.translation.x = x as f32 - w * 0.5 - BAR_SPACING;
    }
}
