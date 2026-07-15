use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin}; // removed LogDiagnosticsPlugin

use bevy::{
    color::palettes::basic::{AQUA, LIME, WHITE},
    prelude::*,
};

pub(crate) struct FPSOverlayPlugin;

impl Plugin for FPSOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_ui)
            .add_systems(Update, (update_fps_display, position_fps_overlay));
    }
}

/// Component for FPS text
#[derive(Component)]
struct FpsText;

/// Root node of the overlay, repositioned to track the viewer panel.
#[derive(Component)]
struct FpsRoot;

fn setup_ui(mut commands: Commands) {
    let font = TextFont {
        font_size: 30.0,

        ..Default::default()
    };

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::all(Val::Px(20.0)),
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
                });
        });
}

/// Keep the overlay in the upper-left corner of the 3D viewer panel (the UI camera is
/// full-window, so node coordinates are window coordinates; scale factor is 1.0 so
/// logical px == physical px).
fn position_fps_overlay(
    panels: Res<crate::panels::Panels>,
    mut roots: Query<&mut Node, With<FpsRoot>>,
) {
    if !panels.is_changed() {
        return;
    }
    let Some(rect) = panels.rect(crate::panels::VIEWER_PANEL) else {
        return;
    };
    for mut node in roots.iter_mut() {
        node.left = Val::Px(rect.x + 8.0);
        node.top = Val::Px(rect.y + 8.0);
    }
}

fn update_fps_display(
    diagnostics: Res<DiagnosticsStore>,
    query: Single<Entity, With<FpsText>>,
    mut writer: TextUiWriter,
) {
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
