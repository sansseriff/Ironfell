//! Standard Bevy DefaultPlugins + winit runner. No custom clock or surface host.
use bevy::prelude::*;

#[cfg(target_os = "macos")]
mod input_trace;
mod motion;
#[cfg(target_os = "macos")]
mod pacer;

#[derive(Component)]
struct Follower;
#[derive(Component)]
struct Target;
#[derive(Component)]
struct Guide;
#[derive(Resource, Default)]
struct Drag(Option<Vec2>);

fn main() {
    // `IRON_BEVY_PRESET=low` = sync rendering + frame latency 1. Individual
    // variables below override the preset; with neither, stock Bevy defaults.
    let low = match std::env::var("IRON_BEVY_PRESET").as_deref() {
        Ok("low") => true,
        Ok("default") | Err(_) => false,
        Ok(other) => panic!("unknown IRON_BEVY_PRESET: {other}"),
    };
    // macOS: one display-timed frame per refresh; implies `low`. See pacer.rs.
    #[cfg(target_os = "macos")]
    let pacing = match std::env::var("IRON_BEVY_PACING").as_deref() {
        Ok("late") => Some(pacer::Mode::Immediate),
        Ok("late-fifo") => Some(pacer::Mode::Fifo),
        Ok("metal-link") => Some(pacer::Mode::MetalLink),
        Ok("off") | Err(_) => None,
        Ok(other) => panic!("unknown IRON_BEVY_PACING: {other}"),
    };
    #[cfg(target_os = "macos")]
    let late = pacing.is_some();
    #[cfg(not(target_os = "macos"))]
    let late = std::env::var_os("IRON_BEVY_PACING").is_some() && panic!("IRON_BEVY_PACING is macOS-only");
    let low = low || late;
    let sync = match std::env::var("IRON_BEVY_RENDER_MODE").as_deref() {
        Ok("sync") => true,
        // The pacer sleeps on the render schedule; it must be the input thread.
        Ok("pipelined") if late => panic!("IRON_BEVY_PACING=late needs sync rendering"),
        Ok("pipelined") => false,
        Err(_) => low,
        Ok(other) => panic!("unknown IRON_BEVY_RENDER_MODE: {other}"),
    };
    // wgpu's default is 2 (three CAMetalLayer drawables); 1 gives two.
    let frame_latency = match std::env::var("IRON_BEVY_FRAME_LATENCY") {
        Ok(v) => Some(v.parse().expect("IRON_BEVY_FRAME_LATENCY: positive integer")),
        Err(_) => low.then(|| std::num::NonZeroU32::MIN),
    };
    let present_mode = match std::env::var("IRON_BEVY_PRESENT").as_deref() {
        #[cfg(target_os = "macos")]
        Err(_) if pacing == Some(pacer::Mode::Immediate) => bevy::window::PresentMode::Immediate,
        Ok("fifo") | Err(_) => bevy::window::PresentMode::Fifo,
        // Metal: displaySyncEnabled = false.
        Ok("immediate") => bevy::window::PresentMode::Immediate,
        Ok(other) => panic!("unknown IRON_BEVY_PRESENT: {other}"),
    };
    // Borderless fullscreen gives the layer a chance at direct-to-display.
    let fullscreen = std::env::var_os("IRON_BEVY_FULLSCREEN").is_some();
    let mut plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: Some(Window {
                title: format!(
                    "Iron latency control — standalone Bevy / {} / latency {}",
                    if sync { "sync" } else { "default pipelined" },
                    frame_latency.map_or("default".into(), |v: std::num::NonZeroU32| v.to_string()),
                ),
                // Match the Tauri fixture's 1280×818 logical content area.
                resolution: match std::env::var("IRON_BEVY_SCALE_FACTOR") {
                    // e.g. 1.0: a 6K fullscreen layer renders at half resolution.
                    Ok(v) => bevy::window::WindowResolution::new(1280, 818)
                        .with_scale_factor_override(v.parse().expect("IRON_BEVY_SCALE_FACTOR: number")),
                    Err(_) => (1280, 818).into(),
                },
                desired_maximum_frame_latency: frame_latency,
                present_mode,
                mode: if fullscreen {
                    bevy::window::WindowMode::BorderlessFullscreen(
                        bevy::window::MonitorSelection::Current,
                    )
                } else {
                    bevy::window::WindowMode::Windowed
                },
                ..default()
            }),
            ..default()
        })
        .build();
    if sync {
        plugins = plugins.disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>();
    }
    let mut app = App::new();
    app.insert_resource(ClearColor(Color::WHITE))
        .init_resource::<Drag>()
        .add_plugins(plugins)
        .add_systems(Startup, setup)
        .add_systems(Update, follow_and_drag);
    motion::install(&mut app);
    #[cfg(target_os = "macos")]
    input_trace::install(&mut app);
    #[cfg(target_os = "macos")]
    if let Some(mode) = pacing {
        pacer::install(&mut app, mode);
    }
    app.run();
}

fn setup(
    mut commands: Commands,
    windows: Query<&Window>,
    render_thread: Option<Res<bevy::render::pipelined_rendering::RenderAppChannels>>,
) {
    commands.spawn((Camera2d, Msaa::Off));
    // Same size/colour/offset as the Tauri control, in logical window pixels.
    for (color, size, z) in [
        (Color::BLACK, 22.0, 2.0),
        (Color::srgb_u8(0, 210, 50), 16.0, 3.0),
    ] {
        commands.spawn((
            Sprite::from_color(color, Vec2::splat(size)),
            Transform::from_xyz(-192.0, 0.0, z)
                .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
            Visibility::Hidden,
            Follower,
        ));
    }
    commands.spawn((
        Sprite::from_color(Color::srgb_u8(255, 214, 0), Vec2::splat(48.0)),
        Transform::from_xyz(0.0, 0.0, 1.0),
        Target,
    ));
    commands.spawn((
        Sprite::from_color(Color::srgb_u8(220, 220, 220), Vec2::new(1280.0, 1.0)),
        Transform::default(),
        Guide,
    ));
    println!(
        "IRON_BEVY_PROBE {}",
        serde_json::json!({"runner":"Bevy DefaultPlugins / winit", "renderThreadEnabled":render_thread.is_some(), "window":windows.single().ok().map(|w|serde_json::json!({"logicalSize":[w.width(),w.height()],"physicalSize":[w.physical_width(),w.physical_height()],"dpr":w.scale_factor(),"presentMode":format!("{:?}",w.present_mode),"requestedMaximumFrameLatency":w.desired_maximum_frame_latency.map(|v|v.get())})), "renderMode":if render_thread.is_some() { "pipelined" } else { "sync" },"preset":std::env::var("IRON_BEVY_PRESET").unwrap_or("default".into()),"windowMode":windows.single().ok().map(|w|format!("{:?}",w.mode)),"greenOffsetLogicalPx":-192,"yellowSizeLogicalPx":48,"inputTrace":std::env::var_os("IRON_BEVY_INPUT_TRACE").is_some()})
    );
}

fn follow_and_drag(
    windows: Query<&Window>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut drag: ResMut<Drag>,
    mut followers: Query<(&mut Transform, &mut Visibility), (With<Follower>, Without<Target>)>,
    mut target: Query<&mut Transform, (With<Target>, Without<Follower>)>,
    mut guides: Query<&mut Sprite, With<Guide>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    for mut guide in &mut guides {
        guide.custom_size = Some(Vec2::new(window.width(), 1.0));
    }
    let Ok(mut target) = target.single_mut() else {
        return;
    };
    if keys.just_pressed(KeyCode::KeyR) {
        target.translation = Vec3::new(0.0, 0.0, 1.0);
        drag.0 = None;
    }
    let Some(cursor) = window.cursor_position() else {
        for (_, mut visibility) in &mut followers {
            *visibility = Visibility::Hidden;
        }
        if !buttons.pressed(MouseButton::Left) {
            drag.0 = None;
        }
        return;
    };
    let p = Vec2::new(
        cursor.x - window.width() * 0.5,
        window.height() * 0.5 - cursor.y,
    );
    for (mut transform, mut visibility) in &mut followers {
        transform.translation.x = p.x - 192.0;
        transform.translation.y = p.y;
        *visibility = Visibility::Visible;
    }
    if buttons.just_pressed(MouseButton::Left)
        && (p - target.translation.truncate()).abs().max_element() <= 24.0
    {
        drag.0 = Some(target.translation.truncate() - p);
    }
    if buttons.pressed(MouseButton::Left) {
        if let Some(offset) = drag.0 {
            target.translation = (p + offset).extend(1.0);
        }
    } else {
        drag.0 = None;
    }
}
