//! Opt-in native controls: ordinary Bevy sprites on the final window camera,
//! and GPU-output readback against the Vello reference. No authored state changes.
use bevy::{
    camera::visibility::{NoFrustumCulling, RenderLayers},
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    transform::TransformSystems,
};
use ironfell::document_bridge::PanelViews;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Component)]
struct DirectMarker;
#[derive(Component)]
struct MarkerBorder;

pub struct Probe {
    mode: String,
    frame: u64,
    prepared: bool,
    motion: bool,
    capture_dir: Option<String>,
    requested: usize,
    completed: Arc<AtomicUsize>,
}
impl Probe {
    pub fn install(app: &mut ironfell::WorkerApp) -> Option<Self> {
        if std::env::var_os("IRON_NATIVE_LAYER_PROBE").is_none() {
            return None;
        }
        assert!(
            std::env::var_os("IRON_NATIVE_FILM").is_some(),
            "layer probe requires filming mode"
        );
        let mode = std::env::var("IRON_NATIVE_PROBE_WEBVIEW").unwrap_or("overlay".into());
        assert!(matches!(mode.as_str(), "overlay" | "hidden" | "detached"));
        app.add_systems(Startup, setup);
        app.add_systems(PostUpdate, follow.before(TransformSystems::Propagate));
        Some(Self {
            mode,
            frame: 0,
            prepared: false,
            motion: std::env::var_os("IRON_NATIVE_PROBE_MOTION").is_some(),
            capture_dir: std::env::var("IRON_NATIVE_PROBE_CAPTURE_DIR").ok(),
            requested: 0,
            completed: Arc::new(AtomicUsize::new(0)),
        })
    }
    pub fn before_frame(
        &mut self,
        app: &mut ironfell::WorkerApp,
        surface: &crate::native_surface::NativeSurface,
    ) {
        let Some(panel) = app
            .world()
            .resource::<PanelViews>()
            .get("latency-film")
            .cloned()
        else {
            return;
        };
        self.frame += 1;
        if self.frame < 30 {
            return;
        }
        if !self.prepared {
            let webview_state = surface.probe_webview(&self.mode);
            println!(
                "IRON_NATIVE_LAYER_PROBE {}",
                serde_json::json!({"webview":self.mode,"webviewState":webview_state,"motion":self.motion,"spriteOffsetCss":-192,"vectorOffsetCss":96,"panel":[panel.rect.x,panel.rect.y,panel.rect.w,panel.rect.h]})
            );
            self.prepared = true;
        }
        if !self.motion {
            return;
        }
        let k = app.world().resource::<ironfell::Dpr>().0 as f32;
        // Unique, non-predictable positions within each 401-frame cycle reveal
        // stale imagery. This is a renderer control, not physical mouse input.
        let x =
            panel.rect.x + panel.rect.w * 0.5 + ((self.frame * 37 % 401) as f32 - 200.0) * k * 0.5;
        let y = panel.rect.y + panel.rect.h * 0.5;
        let scale = app.scale_factor;
        ironfell::engine_api::mouse_move(app, x / scale, y / scale);
        if self.frame % 10 != 0 || self.requested >= 120 {
            return;
        }
        let Some(dir) = self.capture_dir.clone() else {
            return;
        };
        std::fs::create_dir_all(&dir).expect("probe output directory");
        self.requested += 1;
        let frame = self.frame;
        let completed = self.completed.clone();
        app.world_mut().spawn(Screenshot::primary_window()).observe(move |event: On<ScreenshotCaptured>| {
            let image=event.image.clone().try_into_dynamic().expect("GPU screenshot format");
            let rgba=image.to_rgba8();
            // Empty surface readbacks provide no marker-age evidence.
            let available=rgba.get_pixel(rgba.width()/2,rgba.height()/2).0[3]!=0;
            let mut green=(u32::MAX,0u32,0usize); let mut magenta=green;
            for yy in ((y-30.0*k).max(0.0) as u32)..((y+30.0*k).min(rgba.height() as f32) as u32) {
                for xx in 0..rgba.width() {
                    let p=rgba.get_pixel(xx,yy).0;
                    let target=if p[0]<90 && p[1]>140 && p[2]<100 {Some(&mut green)}
                    else if p[0]>180 && p[1]<90 && p[2]>100 {Some(&mut magenta)} else {None};
                    if let Some(t)=target {t.0=t.0.min(xx);t.1=t.1.max(xx);t.2+=1;}
                }
            }
            let center=|b:(u32,u32,usize)| if b.2>8 {Some((b.0 as f32+b.1 as f32)*0.5)} else {None};
            let sprite=center(green).map(|v|v+192.0*k);let vector=center(magenta).map(|v|v-96.0*k);
            let pass=sprite.zip(vector).is_some_and(|(s,v)|(s-x).abs()<=1.5 && (v-x).abs()<=1.5);
            println!("IRON_NATIVE_LAYER_CAPTURE {}",serde_json::json!({"frame":frame,"expectedX":x,"spriteInputX":sprite,"vectorInputX":vector,"pass":pass,"available":available,"kind":"GPU output, before WindowServer"}));
            image.save(format!("{dir}/frame-{frame:04}.png")).expect("save probe image");
            if available {completed.fetch_add(1,Ordering::Relaxed);}
        });
    }
    pub fn finished(&self) -> bool {
        self.motion && self.capture_dir.is_some() && self.completed.load(Ordering::Relaxed) >= 12
    }
}
fn setup(mut commands: Commands) {
    // Layer 1 is the shared engine's final vector-composite camera. These
    // sprites bypass Vello and its intermediate view textures entirely.
    for (color, border, z) in [
        (Color::BLACK, true, 500.0),
        (Color::srgb_u8(0, 210, 50), false, 501.0),
    ] {
        let mut e = commands.spawn((
            Sprite::from_color(color, Vec2::splat(1.0)),
            Transform::from_xyz(0.0, 0.0, z),
            RenderLayers::layer(1),
            NoFrustumCulling,
            Visibility::Hidden,
            DirectMarker,
        ));
        if border {
            e.insert(MarkerBorder);
        }
    }
}
fn follow(
    pointer: Res<ironfell::PointerState>,
    dpr: Res<ironfell::Dpr>,
    views: Res<PanelViews>,
    windows: Query<&Window>,
    mut markers: Query<
        (
            &mut Transform,
            &mut Sprite,
            &mut Visibility,
            Has<MarkerBorder>,
        ),
        With<DirectMarker>,
    >,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let show = views
        .get("latency-film")
        .is_some_and(|p| p.rect.contains(pointer.screen));
    for (mut transform, mut sprite, mut visibility, border) in &mut markers {
        let next_visibility = if show {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility != next_visibility {
            *visibility = next_visibility;
        }
        let next_transform = Transform {
            translation: Vec3::new(
                pointer.screen.x - 192.0 * dpr.0 as f32 - window.physical_width() as f32 * 0.5,
                window.physical_height() as f32 * 0.5 - pointer.screen.y,
                if border { 500.0 } else { 501.0 },
            ),
            rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_4),
            ..default()
        };
        if *transform != next_transform {
            *transform = next_transform;
        }
        let size = Some(Vec2::splat(if border { 22.0 } else { 16.0 }) * dpr.0 as f32);
        if sprite.custom_size != size {
            sprite.custom_size = size;
        }
    }
}
