//! Opt-in `IRON_NATIVE_3D_MOTION=1`: strafe every 3D camera side to side at the
//! fly controller's walk speed, driven by `Time` exactly as holding A or D is, to
//! look for hiccups in the 3D views without a human at the keyboard.
//!
//! `IRON_NATIVE_2D_MOTION=1` does the same for the 2D views: each view's pan slides
//! side to side at a constant speed from wherever it starts, so all its content
//! moves continuously. Uses the same `ViewCameras` pan the pan tools write. A
//! number instead of `1` sets the speed in window px per second.
//!
//! `IRON_NATIVE_3D_LOAD=N` adds N high-polygon lit spheres (≈20k triangles each)
//! on the ground plane: a repeatable heavy GPU scene. It works with or without the
//! motion, and in any pacing mode, for comparisons against stock scheduling.
use bevy::{platform::collections::HashMap, prelude::*};

/// The camera controller's default `walk_speed`, in world units per second.
const SPEED: f64 = 5.0;
/// Seconds per sweep in one direction.
const LEG: f64 = 2.0;
/// Default 2D pan speed, window px per second: 10 px per 60 Hz refresh.
const PAN_SPEED: f64 = 600.0;

pub fn install(app: &mut ironfell::WorkerApp) {
    if let Some(n) = std::env::var("IRON_NATIVE_3D_LOAD").ok() {
        let n: usize = n.parse().expect("IRON_NATIVE_3D_LOAD: sphere count");
        app.add_systems(Startup, move |commands: Commands, meshes: ResMut<Assets<Mesh>>, materials: ResMut<Assets<StandardMaterial>>| {
            load(n, commands, meshes, materials)
        });
    }
    if std::env::var_os("IRON_NATIVE_3D_MOTION").is_some() {
        app.add_systems(Update, strafe);
    }
    if let Ok(v) = std::env::var("IRON_NATIVE_2D_MOTION") {
        let speed = match v.parse::<f64>() {
            Ok(s) if s > 1.0 => s,
            _ => PAN_SPEED,
        };
        app.add_systems(Update, move |time: Res<Time>, views: Res<ironfell::document_bridge::PanelViews>, cameras: ResMut<ironfell::document_bridge::ViewCameras>, base: Local<HashMap<String, Vec2>>, count: Local<usize>| {
            pan_2d(speed, time, views, cameras, base, count)
        });
    }
}

/// Triangle wave: distance along a back-and-forth sweep at constant `speed`.
fn sweep(time: &Time, speed: f64) -> f64 {
    let phase = (time.elapsed_secs_f64() / LEG).rem_euclid(2.0);
    speed * LEG * if phase < 1.0 { phase } else { 2.0 - phase }
}

fn pan_2d(
    speed: f64,
    time: Res<Time>,
    views: Res<ironfell::document_bridge::PanelViews>,
    mut cameras: ResMut<ironfell::document_bridge::ViewCameras>,
    mut base: Local<HashMap<String, Vec2>>,
    mut count: Local<usize>,
) {
    let offset = Vec2::new(sweep(&time, speed) as f32, 0.0);
    let mut n = 0;
    // 2D composition views: no 3D layer, not render inspectors.
    for view in views.0.iter().filter(|v| v.layer == 0 && v.observes.is_none()) {
        let camera = cameras.0.entry(view.panel.clone()).or_default();
        let start = *base.entry(view.panel.clone()).or_insert(camera.pan);
        camera.pan = start + offset;
        n += 1;
    }
    if n != *count {
        *count = n;
        println!("IRON_NATIVE_2D_MOTION {}", serde_json::json!({"views": n}));
    }
}

fn load(
    n: usize,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Sphere::new(0.4).mesh().ico(5).expect("ico sphere"));
    // Few shared materials so Bevy batches: GPU-heavy without per-draw CPU cost.
    let palette: Vec<_> = (0..8)
        .map(|k| {
            materials.add(StandardMaterial {
                base_color: Color::hsl(k as f32 * 45.0, 0.6, 0.55),
                perceptual_roughness: 0.4,
                ..default()
            })
        })
        .collect();
    let side = (n as f32).sqrt().ceil() as usize;
    let spacing = 48.0 / side.max(1) as f32;
    for i in 0..n {
        let (x, z) = ((i % side) as f32, (i / side) as f32);
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(palette[i % palette.len()].clone()),
            Transform::from_xyz((x + 0.5) * spacing - 24.0, 0.4, (z + 0.5) * spacing - 24.0),
        ));
    }
    println!("IRON_NATIVE_3D_LOAD {}", serde_json::json!({"spheres": n, "trianglesApprox": n * 20480}));
}

fn strafe(
    time: Res<Time>,
    mut cameras: Query<(Entity, &mut Transform), With<Camera3d>>,
    mut base: Local<HashMap<Entity, Vec3>>,
    mut count: Local<usize>,
) {
    let offset = sweep(&time, SPEED);
    let mut n = 0;
    for (entity, mut transform) in &mut cameras {
        let start = *base.entry(entity).or_insert(transform.translation);
        let right = transform.right();
        transform.translation = start + right * offset as f32;
        n += 1;
    }
    if n != *count {
        *count = n;
        println!("IRON_NATIVE_3D_MOTION {}", serde_json::json!({"cameras": n}));
    }
}
