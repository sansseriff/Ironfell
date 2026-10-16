//! Bounded macOS composition probe. No product/document semantics live here.
mod display_link;

use bevy::{app::PluginsState, prelude::*, render::RenderApp, window::*};
use serde::Serialize;
use std::{
    cell::RefCell,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{Manager, RunEvent};

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}

#[derive(Default, Clone, Serialize)]
struct Stats {
    frames: u64,
    running: bool,
    render_thread_enabled: bool,
    ecs_threads: Vec<String>,
    gpu: String,
    physical_size: [u32; 2],
}

#[derive(Default)]
struct Bridge {
    running: AtomicBool,
    alternate: AtomicBool,
    stats: Mutex<Stats>,
}

#[derive(Resource)]
struct ProbeBridge(Arc<Bridge>);
#[derive(Component)]
struct Spinner;
#[derive(Resource)]
struct LaneA(Vec<f32>);
#[derive(Resource)]
struct LaneB(Vec<f32>);

struct Host {
    app: App,
    window: Entity,
    native: tauri::WebviewWindow,
    bridge: Arc<Bridge>,
    driver: display_link::DisplayDriver,
    settle: u8,
}

fn remember_thread(bridge: &Bridge) {
    let name = format!("{:?}", std::thread::current().id());
    let mut stats = bridge.stats.lock().unwrap();
    if !stats.ecs_threads.contains(&name) {
        stats.ecs_threads.push(name);
    }
}

fn lane_a(mut lane: ResMut<LaneA>, bridge: Res<ProbeBridge>) {
    if bridge.0.running.load(Ordering::Relaxed) {
        for x in &mut lane.0 {
            *x = (*x + 0.0001).sin();
        }
        remember_thread(&bridge.0);
    }
}

fn lane_b(mut lane: ResMut<LaneB>, bridge: Res<ProbeBridge>) {
    if bridge.0.running.load(Ordering::Relaxed) {
        for x in &mut lane.0 {
            *x = (*x + 0.0002).cos();
        }
        remember_thread(&bridge.0);
    }
}

fn animate(
    time: Res<Time>,
    bridge: Res<ProbeBridge>,
    mut object: Query<&mut Transform, With<Spinner>>,
) {
    if bridge.0.running.load(Ordering::Relaxed) {
        for mut transform in &mut object {
            transform.rotate_y(time.delta_secs() * 0.7);
        }
    }
}

fn scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.8, 1.8, 1.8))),
        MeshMaterial3d(materials.add(Color::srgb(0.15, 0.72, 0.88))),
        Transform::from_xyz(1.0, 0.2, 0.0),
        Spinner,
    ));
    commands.spawn((
        PointLight {
            intensity: 1_800_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 6.0, 4.0),
    ));
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(4.0, 3.0, 7.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

impl Host {
    fn new(
        native: tauri::WebviewWindow,
        bridge: Arc<Bridge>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let size = native.inner_size()?;
        let wrapper = WindowWrapper::new(native.clone());
        let raw = RawHandleWrapper::new(&wrapper)?;
        let mut app = App::new();
        let mut window = Window {
            present_mode: PresentMode::AutoVsync,
            ..default()
        };
        window.resolution.set_scale_factor(1.0);
        window.resolution.set(size.width as f32, size.height as f32);
        let window = app
            .world_mut()
            .spawn((
                window,
                PrimaryWindow,
                raw.clone(),
                RawHandleWrapperHolder(Arc::new(Mutex::new(Some(raw)))),
            ))
            .id();
        app.insert_resource(ClearColor(Color::srgb(0.055, 0.075, 0.12)));
        app.insert_resource(ProbeBridge(bridge.clone()));
        app.insert_resource(LaneA(vec![0.0; 100_000]));
        app.insert_resource(LaneB(vec![1.0; 100_000]));
        app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: None,
            exit_condition: ExitCondition::DontExit,
            close_when_requested: false,
            ..default()
        }));
        app.add_systems(Startup, scene);
        app.add_systems(Update, (animate, lane_a, lane_b));
        if app.plugins_state() != PluginsState::Ready {
            return Err("native plugins not ready".into());
        }
        app.finish();
        app.cleanup();
        app.world_mut().write_message(WindowCreated { window });
        let gpu = app
            .world()
            .get_resource::<bevy::render::renderer::RenderAdapterInfo>()
            .map(|a| format!("{} / {:?}", a.name, a.backend))
            .unwrap_or_default();
        *bridge.stats.lock().unwrap() = Stats {
            running: true,
            render_thread_enabled: app.get_sub_app(RenderApp).is_none()
                && app
                    .world()
                    .contains_resource::<bevy::render::pipelined_rendering::RenderAppChannels>(),
            gpu,
            physical_size: [size.width, size.height],
            ..default()
        };
        bridge.running.store(true, Ordering::Relaxed);
        let driver = display_link::DisplayDriver::new(&native)?;
        Ok(Self {
            app,
            window,
            native,
            bridge,
            driver,
            settle: 6,
        })
    }

    fn wake(&mut self) {
        self.settle = 6;
        self.driver.wake();
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            self.driver.pause();
            return;
        }
        if let Some(mut window) = self.app.world_mut().get_mut::<Window>(self.window) {
            window.resolution.set(width as f32, height as f32);
        }
        self.app.world_mut().write_message(WindowResized {
            window: self.window,
            width: width as f32,
            height: height as f32,
        });
        self.bridge.stats.lock().unwrap().physical_size = [width, height];
        self.wake();
    }

    fn tick(&mut self) {
        if self.native.is_minimized().unwrap_or(false) {
            self.driver.pause();
            return;
        }
        let running = self.bridge.running.load(Ordering::Relaxed);
        if !running && self.settle == 0 {
            self.driver.pause();
            return;
        }
        self.settle = self.settle.saturating_sub(1);
        let alt = self.bridge.alternate.load(Ordering::Relaxed);
        let color = if alt {
            Color::srgb(0.18, 0.07, 0.09)
        } else {
            Color::srgb(0.055, 0.075, 0.12)
        };
        if self.app.world().resource::<ClearColor>().0 != color {
            self.app.insert_resource(ClearColor(color));
        }
        self.app.update();
        let mut stats = self.bridge.stats.lock().unwrap();
        stats.frames += 1;
        stats.running = running;
    }

    fn shutdown(mut self) {
        self.driver.pause();
        // Service render-world main-thread jobs while waiting for the final frame.
        // Drop render resources here, before the Tauri window can be destroyed.
        use bevy::{
            ecs::schedule::MainThreadExecutor, render::pipelined_rendering::RenderAppChannels,
            tasks::ComputeTaskPool,
        };
        if let Some(mut channels) = self.app.world_mut().remove_resource::<RenderAppChannels>() {
            let executor = self.app.world().resource::<MainThreadExecutor>();
            let render =
                ComputeTaskPool::get().scope_with_executor(true, Some(&*executor.0), |scope| {
                    scope.spawn(async { channels.recv().await });
                });
            drop(render);
        }
    }
}

pub(crate) fn tick() {
    HOST.with(|host| {
        if let Some(host) = host.borrow_mut().as_mut() {
            host.tick();
        }
    });
}

fn wake(app: &tauri::AppHandle) {
    let _ = app.run_on_main_thread(|| {
        HOST.with(|host| {
            if let Some(host) = host.borrow_mut().as_mut() {
                host.wake();
            }
        })
    });
}

#[tauri::command]
fn toggle_animation(app: tauri::AppHandle, bridge: tauri::State<'_, Arc<Bridge>>) -> bool {
    let running = !bridge.running.fetch_xor(true, Ordering::Relaxed);
    wake(&app);
    running
}

#[tauri::command]
fn toggle_background(app: tauri::AppHandle, bridge: tauri::State<'_, Arc<Bridge>>) {
    bridge.alternate.fetch_xor(true, Ordering::Relaxed);
    wake(&app);
}

#[tauri::command]
fn probe_stats(bridge: tauri::State<'_, Arc<Bridge>>) -> Stats {
    bridge.stats.lock().unwrap().clone()
}

/// Test-only host controls: enabled only in an explicitly requested self-test run.
#[tauri::command]
fn probe_test(
    window: tauri::WebviewWindow,
    action: String,
    report: Option<serde_json::Value>,
) -> Result<(), String> {
    if std::env::var_os("IRON_NATIVE_SELF_TEST").is_none() {
        return Err("self-test controls disabled".into());
    }
    match action.as_str() {
        "resize" => window
            .set_size(tauri::PhysicalSize::new(1800, 1200))
            .map_err(|e| e.to_string())?,
        "minimize" => window.minimize().map_err(|e| e.to_string())?,
        "restore" => {
            window.unminimize().map_err(|e| e.to_string())?;
            window.set_focus().map_err(|e| e.to_string())?;
        }
        "report" => println!("IRON_NATIVE_TEST {}", report.unwrap_or_default()),
        "close" => window.close().map_err(|e| e.to_string())?,
        _ => return Err("unknown self-test action".into()),
    }
    Ok(())
}

fn main() {
    let bridge = Arc::new(Bridge::default());
    let setup_bridge = bridge.clone();
    let app = tauri::Builder::default()
        .manage(bridge)
        .invoke_handler(tauri::generate_handler![
            toggle_animation,
            toggle_background,
            probe_stats,
            probe_test
        ])
        .on_page_load(|webview, payload| {
            if std::env::var_os("IRON_NATIVE_SELF_TEST").is_some()
                && matches!(payload.event(), tauri::webview::PageLoadEvent::Finished)
            {
                let _ = webview.eval(include_str!("../probe-ui/self-test.js"));
            }
        })
        .setup(move |app| {
            let native = app
                .get_webview_window("main")
                .ok_or("missing native window")?;
            let host = Host::new(native, setup_bridge.clone())?;
            HOST.with(|slot| *slot.borrow_mut() = Some(host));
            Ok(())
        })
        .build(tauri::generate_context!("tauri.probe.conf.json"))
        .expect("native host construction failed");
    app.run_return(|_, event| match event {
        RunEvent::WindowEvent {
            event: tauri::WindowEvent::Resized(size),
            ..
        } => HOST.with(|slot| {
            if let Some(host) = slot.borrow_mut().as_mut() {
                host.resize(size.width, size.height);
            }
        }),
        RunEvent::WindowEvent {
            event: tauri::WindowEvent::Focused(true) | tauri::WindowEvent::Moved(_),
            ..
        } => HOST.with(|slot| {
            if let Some(host) = slot.borrow_mut().as_mut() {
                host.wake();
            }
        }),
        RunEvent::WindowEvent {
            event: tauri::WindowEvent::CloseRequested { .. },
            ..
        }
        | RunEvent::ExitRequested { .. } => HOST.with(|slot| {
            if let Some(host) = slot.borrow_mut().take() {
                host.shutdown();
            }
        }),
        _ => {}
    });
}
