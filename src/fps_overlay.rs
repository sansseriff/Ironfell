use crate::vector::BackendKind;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin}; // removed LogDiagnosticsPlugin

use bevy::{
    color::palettes::basic::{AQUA, LIME, RED, WHITE},
    prelude::*,
};

pub(crate) struct FPSOverlayPlugin;

impl Plugin for FPSOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_ui).add_systems(
            Update,
            (
                update_fps_display,
                update_backend_display,
                update_canvas_display,
            ),
        );
    }
}

/// Component for FPS text
#[derive(Component)]
struct FpsText;

/// Root node of the overlay, in the window's bottom-right corner.
#[derive(Component)]
struct FpsRoot;

fn setup_ui(mut commands: Commands) {
    let font = TextFont {
        // bevy 0.19 made text size a unit-carrying enum rather than a bare f32.
        font_size: bevy::text::FontSize::Px(12.0),

        ..Default::default()
    };

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::all(Val::Px(6.0)),
                // The window's bottom-right corner, over the timeline: out
                // of every composition view's way.
                right: Val::Px(8.0),
                bottom: Val::Px(8.0),
                ..default()
            },
            BackgroundColor(Color::BLACK.with_alpha(0.75)),
            GlobalZIndex(i32::MAX),
            FpsRoot,
        ))
        .with_children(|p| {
            p.spawn((Text::default(), FpsText, Name::new("FPS Text")))
                .with_children(|p| {
                    p.spawn((
                        TextSpan::new("\nFPS (raw): "),
                        font.clone(),
                        TextColor(WHITE.into()),
                    ));
                    p.spawn((TextSpan::new(""), font.clone(), TextColor(AQUA.into())));
                    p.spawn((
                        TextSpan::new("\nFPS (SMA): "),
                        font.clone(),
                        TextColor(WHITE.into()),
                    ));
                    p.spawn((TextSpan::new(""), font.clone(), TextColor(AQUA.into())));
                    p.spawn((
                        TextSpan::new("\nFPS (EMA): "),
                        font.clone(),
                        TextColor(WHITE.into()),
                    ));
                    p.spawn((TextSpan::new(""), font.clone(), TextColor(AQUA.into())));
                    // Which renderer is realizing the 2D layers.
                    p.spawn((
                        TextSpan::new("\n2D backend: "),
                        font.clone(),
                        TextColor(WHITE.into()),
                    ));
                    // Filled in by `update_backend_display` from the actual
                    // installed backend, rather than hardcoded here.
                    p.spawn((TextSpan::new(""), font.clone(), TextColor(LIME.into())));
                    // Which build is running: timings from the slow
                    // development build mislead (`build_label`).
                    let (label, colour) = build_label();
                    p.spawn((TextSpan::new("\nBuild: "), font.clone(), TextColor(WHITE.into())));
                    p.spawn((TextSpan::new(label), font.clone(), TextColor(colour.into())));
                    // The canvas's backing size, which the render scale
                    // setting changes (`update_canvas_display`).
                    p.spawn((TextSpan::new("\nCanvas: "), font.clone(), TextColor(WHITE.into())));
                    p.spawn((TextSpan::new(""), font.clone(), TextColor(AQUA.into())));
                });
        });
}

/// The build's optimization level and wasm SIMD, as a label and a colour:
/// green when optimized with SIMD (what ships and what benchmarks measure),
/// red otherwise. `.claude/skills/run-iron/build.sh` builds `dev-opt` (opt 1)
/// by default and `wasm-bench` (opt 3, SIMD) with `PROFILE=wasm-bench`.
fn build_label() -> (String, bevy::color::Srgba) {
    let (name, opt, simd, fast) = build_profile();
    let label = format!("{name} (opt {opt}{}){}", if simd { ", SIMD" } else { ", no SIMD" }, if fast { "" } else { " - SLOW, not for timing" });
    (label, if fast { LIME } else { RED })
}

/// The build's profile name, optimization level, wasm SIMD, and whether it
/// is fast enough to judge performance by. Also reported to the shell's
/// Settings view (`web_ffi::document_build_info`).
pub(crate) fn build_profile() -> (&'static str, &'static str, bool, bool) {
    let opt = env!("IRON_OPT_LEVEL");
    let simd = crate::host_services::cpu_simd_available();
    let fast = matches!(opt, "2" | "3" | "s" | "z") && simd;
    let name = match opt {
        "1" => "dev-opt",
        "3" => "wasm-bench",
        "z" => "release",
        _ => "custom",
    };
    (name, opt, simd, fast)
}

/// The canvas's size in physical pixels, when it changes.
fn update_canvas_display(
    window: Single<Ref<Window>, With<bevy::window::PrimaryWindow>>,
    query: Single<Entity, With<FpsText>>,
    mut writer: TextUiWriter,
    mut shown: Local<UVec2>,
) {
    let size = window.physical_size();
    if *shown == size {
        return;
    }
    *shown = size;
    *writer.text(*query, 12) = format!("{}x{} px", size.x, size.y);
}

/// Show the active 2D vector backend, and colour it so a glance is enough.
///
/// Reads the `BackendKind` the plugin actually installed rather than a build-time
/// constant, so the label cannot drift from what is really rendering.
fn update_backend_display(
    backend: Res<BackendKind>,
    query: Single<Entity, With<FpsText>>,
    mut writer: TextUiWriter,
    mut initialised: Local<bool>,
) {
    if *initialised {
        return;
    }
    *initialised = true;

    let (label, colour) = match *backend {
        #[cfg(feature = "classic")]
        BackendKind::ClassicVello => ("vello classic", AQUA),
        BackendKind::Hybrid => ("vello_hybrid (sparse strips)", LIME),
    };
    *writer.text(*query, 8) = label.to_owned();
    *writer.color(*query, 8) = TextColor(colour.into());
}

fn update_fps_display(
    diagnostics: Res<DiagnosticsStore>,
    query: Single<Entity, With<FpsText>>,
    mut writer: TextUiWriter,
    time: Res<Time<Real>>,
    mut last: Local<f64>,
    redraw: Option<Res<crate::RedrawNeeded>>,
) {
    // Diagnostics must not wake an otherwise idle renderer just to count it.
    // Content/input wakes rendering; the overlay catches up on its next frame.
    if redraw.is_some_and(|r| !r.now) {
        return;
    }
    // Four times a second: rewriting the text every frame would mark the UI
    // changed every frame, and the window would never be idle
    // (plans/specs/view-textures.md, render on demand).
    let now = time.elapsed_secs_f64();
    if now - *last < 0.25 {
        return;
    }
    *last = now;
    let text_entity = *query;

    if let Some(fps) = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS) {
        if let Some(raw) = fps.value() {
            *writer.text(text_entity, 2) = format!("{raw:.2}");
        }
        if let Some(sma) = fps.average() {
            *writer.text(text_entity, 4) = format!("{sma:.2}");
        }

        if let Some(ema) = fps.smoothed() {
            *writer.text(text_entity, 6) = format!("{ema:.2}");
        }
    }
}
