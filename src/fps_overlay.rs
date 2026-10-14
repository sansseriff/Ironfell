use crate::vector::BackendKind;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin}; // removed LogDiagnosticsPlugin

use bevy::{
    color::palettes::basic::{AQUA, LIME, WHITE},
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
                });
        });
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
) {
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
