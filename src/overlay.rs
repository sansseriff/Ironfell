use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};

use bevy::{
    color::palettes::basic::{AQUA, LIME, SILVER},
    prelude::*,
};

pub(crate) struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_ui)
            .add_systems(Update, update_fps_display);
    }
}

/// Component for FPS text
#[derive(Component)]
struct FpsText;

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
        ))
        .with_children(|p| {
            p.spawn((Text::default(), FpsText, Name::new("FPS Text")))
                .with_children(|p| {
                    p.spawn((TextSpan::new("FPS: "), font.clone(), TextColor(LIME.into())));
                    p.spawn((TextSpan::new("0.00"), font.clone(), TextColor(AQUA.into())));
                    p.spawn((
                        TextSpan::new("\nFPS (avg): "),
                        font.clone(),
                        TextColor(LIME.into()),
                    ));
                    p.spawn((TextSpan::new("0.00"), font.clone(), TextColor(AQUA.into())));
                });
        });
}

fn update_fps_display(
    diagnostics: Res<DiagnosticsStore>,
    query: Single<Entity, With<FpsText>>,
    mut writer: TextUiWriter,
) {
    let text_entity = *query;

    if let Some(fps) = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS) {
        if let Some(current_fps) = fps.value() {
            *writer.text(text_entity, 2) = format!("{current_fps:.2}");
        }

        if let Some(avg_fps) = fps.smoothed() {
            *writer.text(text_entity, 4) = format!("{avg_fps:.2}");
        }
    }
}
