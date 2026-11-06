//! Tauri owns the window/event loop; the shared Iron engine owns all product state.
mod agent;
mod bridge;
mod display_link;
mod frame_budget;
mod gpu_timing;
mod latency_probe;
mod motion_3d;
mod native_input;
mod native_surface;
mod pacing;
mod present_feedback;
mod simulation_probe;
mod vsync_clock;
use bevy::{app::PluginsState, prelude::*, window::*};
use bridge::{Bridge, Request};
use serde_json::{Value, json};
use std::{cell::RefCell, sync::Arc};
use tauri::{Manager, RunEvent, ipc::Channel};
thread_local! {static HOST:RefCell<Option<Host>>=const{RefCell::new(None)};}
struct Host {
    app: ironfell::WorkerApp,
    native: tauri::WebviewWindow,
    bridge: Arc<Bridge>,
    driver: display_link::DisplayDriver,
    frames: u64,
    rendered_frames: u64,
    session_opened: bool,
    trace: Option<simulation_probe::Trace>,
    surface: native_surface::NativeSurface,
    input: Option<native_input::NativeInput>,
    last_native_sample: Option<std::time::Instant>,
    native_samples: u64,
    native_input_age_ms: f64,
    deferred_release: Option<Request>,
    pacing: pacing::Pacer,
    present: Option<present_feedback::PresentFeedback>,
    layer_probe: Option<latency_probe::Probe>,
}
impl Host {
    fn new(
        native: tauri::WebviewWindow,
        bridge: Arc<Bridge>,
        webview: *mut std::ffi::c_void,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let (surface, raw) = unsafe { native_surface::NativeSurface::new(&native, webview) }?;
        let (width, height) = surface.sync();
        let mut variant = if std::env::var_os("IRON_NATIVE_SIMULATION").is_some()
            || std::env::var_os("IRON_NATIVE_ALWAYS_RENDER").is_some()
        {
            ironfell::VARIANT_ALWAYS_RENDER
        } else {
            0
        };
        match std::env::var("IRON_NATIVE_RENDER_MODE").as_deref() {
            Ok("sync") => variant |= ironfell::VARIANT_SYNC_RENDER,
            Ok("pipelined") | Err(_) => {}
            Ok(mode) => return Err(format!("unknown IRON_NATIVE_RENDER_MODE: {mode}").into()),
        }
        let present = match std::env::var("IRON_NATIVE_PRESENT_MODE").as_deref() {
            Ok("immediate") => PresentMode::AutoNoVsync,
            _ => PresentMode::AutoVsync,
        };
        let mut app = ironfell::init_app(variant, |app| {
            let entity = ironfell::surface::spawn_external_window(app, raw, width, height, present);
            // Use Bevy's usual allowance: a one-frame cap starves Metal's drawable
            // pool and can stall extraction. Keep an explicit comparison control.
            let latency = std::env::var("IRON_NATIVE_FRAME_LATENCY")
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(2)
                .clamp(1, 3);
            app.world_mut()
                .get_mut::<Window>(entity)
                .unwrap()
                .desired_maximum_frame_latency = std::num::NonZeroU32::new(latency);
            entity
        });
        if app.plugins_state() != PluginsState::Ready {
            return Err("native engine plugins not ready".into());
        }
        let layer_probe = latency_probe::Probe::install(&mut app);
        let trace = simulation_probe::install(&mut app);
        let pacing = pacing::install(&mut app);
        motion_3d::install(&mut app);
        app.finish();
        app.cleanup();
        if let Some(info) = app
            .world()
            .get_resource::<bevy::render::renderer::RenderAdapterInfo>()
        {
            let device = app
                .world()
                .resource::<bevy::render::renderer::RenderDevice>();
            println!(
                "IRON_NATIVE_GPU {}",
                json!({"adapter":info.name,"backend":format!("{:?}",info.backend),"features":format!("{:?}",device.features()),"limits":format!("{:?}",device.limits())})
            );
        }
        let driver = display_link::DisplayDriver::for_view(surface.view(), pacing.clone())?;
        let input = if std::env::var_os("IRON_NATIVE_JS_POINTER").is_some() {
            None
        } else {
            Some(native_input::NativeInput::new(surface.view())?)
        };
        let present = present_feedback::PresentFeedback::install(pacing.is_late_fifo());
        Ok(Self {
            app,
            native,
            bridge,
            driver,
            frames: 0,
            rendered_frames: 0,
            session_opened: false,
            trace,
            surface,
            input,
            last_native_sample: None,
            native_samples: 0,
            native_input_age_ms: 0.0,
            deferred_release: None,
            pacing,
            present,
            layer_probe,
        })
    }
    fn prepare_session(&mut self) {
        if self.bridge.prepare(self.input.is_some()) {
            ironfell::engine_api::reset_host_session(&mut self.app);
            self.deferred_release = None;
            self.session_opened = true;
        }
    }
    fn wake(&mut self) {
        self.prepare_session();
        self.driver.wake();
    }
    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            self.driver.pause();
        } else {
            self.wake();
        }
    }
    fn sync_surface(&mut self) -> bool {
        let (width, height) = self.surface.sync();
        if width == 0 || height == 0 {
            return false;
        }
        let entity = self.app.window;
        let window = self.app.world().get::<Window>(entity).unwrap();
        if window.physical_size() != UVec2::new(width, height) {
            println!(
                "IRON_SURFACE_RESIZE {:?} -> {:?}",
                window.physical_size(),
                (width, height)
            );
            self.app
                .world_mut()
                .get_mut::<Window>(entity)
                .unwrap()
                .resolution
                .set(width as f32, height as f32);
            self.app.world_mut().write_message(WindowResized {
                window: entity,
                width: width as f32,
                height: height as f32,
            });
        }
        true
    }
    fn tick(&mut self, timing: pacing::DisplayTiming) {
        let start = objc2_quartz_core::CACurrentMediaTime();
        if self.native.is_minimized().unwrap_or(false) {
            self.driver.pause();
            return;
        }
        if !self.sync_surface() {
            return;
        }
        let mut native_moved = false;
        let mut input_age = None;
        let mut input_received = None;
        if self.input.is_some() {
            if let Some(sample) = native_input::latest() {
                if self.last_native_sample != Some(sample.received) {
                    self.last_native_sample = Some(sample.received);
                    self.native_samples += 1;
                    self.native_input_age_ms = sample.received.elapsed().as_secs_f64() * 1000.0;
                    input_age = Some(self.native_input_age_ms);
                    input_received = Some(
                        objc2_quartz_core::CACurrentMediaTime()
                            - sample.received.elapsed().as_secs_f64(),
                    );
                }
                let (x, y) = self.surface.pointer(sample.point);
                let physical = self.app.to_physical_size(x, y);
                if self.app.world().resource::<ironfell::PointerState>().screen != physical {
                    native_moved = true;
                    ironfell::engine_api::mouse_move(&mut self.app, x, y);
                    ironfell::engine_api::set_modifiers(&mut self.app, sample.modifiers);
                }
            }
        }
        self.prepare_session();
        let opened = std::mem::take(&mut self.session_opened);
        for _ in 0..256 {
            let deferred = self.deferred_release.is_some();
            let Some(request) = self.deferred_release.take().or_else(|| self.bridge.pop()) else {
                break;
            };
            // Native motion and HTML button ownership arrive independently. Let
            // tools preview the final held pose before committing a release. A
            // short gesture can otherwise collapse to a click between ticks.
            if native_moved
                && !deferred
                && matches!(
                    request,
                    Request::LeftBtUp | Request::MiddleBt { down: false }
                )
            {
                self.deferred_release = Some(request);
                break;
            }
            if matches!(request, Request::ProbeStats) {
                let render_thread = self
                    .app
                    .world()
                    .contains_resource::<bevy::render::pipelined_rendering::RenderAppChannels>(
                );
                let size = self
                    .app
                    .world()
                    .get::<Window>(self.app.window)
                    .unwrap()
                    .physical_size();
                self.bridge.send(json!({"ty":"probeStats","stats":{
                    "frames":self.frames,
                    "renderThreadEnabled":render_thread,
                    "requestedRenderMode":std::env::var("IRON_NATIVE_RENDER_MODE").unwrap_or("pipelined".into()),
                    "pacing":self.pacing.mode(),
                    "backend":"native",
                    "pointerSource":if self.input.is_some(){"native"}else{"webview"},
                    "nativePointerSamples":self.native_samples,
                    "nativeInputAgeMs":self.native_input_age_ms,
                    "physicalSize":[size.x,size.y],
                    "renderedFrames":self.rendered_frames,
                    "simulation":self.trace.as_ref().map(|t|t.snapshot())
                }}));
                continue;
            }
            let (answer, edge) = bridge::dispatch(&mut self.app, request);
            if let Some(answer) = answer {
                self.bridge.send(answer);
            }
            // Preserve button/key transitions across engine frames, including when
            // the webview delivers several events in a single IPC batch.
            if edge {
                break;
            }
        }
        if let Some(probe) = &mut self.layer_probe {
            probe.before_frame(&mut self.app, &self.surface);
        }
        self.pacing.begin(
            &mut self.app,
            self.frames + 1,
            timing,
            start,
            input_received,
        );
        self.pacing.set_frame_clock(&mut self.app, timing);
        self.app.frame();
        if let Some(present) = &mut self.present {
            let step = self.app.world().resource::<Time>().delta_secs_f64();
            let results =
                present.after_frame(start, self.pacing.planned_vsync(timing), step, self.pacing.engaged());
            self.pacing.observe_frames(&results);
            present.note_margin(self.pacing.margin_ms());
        }
        // adaptive pacing: low latency while dragging. Left button held over the
        // canvas (before any movement, so draining the queue doesn't show) or any
        // document gesture, whatever tool or context runs it.
        let dragging = self.app.world().resource::<ironfell::PointerState>().buttons.left
            || self
                .app
                .world()
                .resource::<ironfell::document_bridge::DocumentStore>()
                .0
                .active_gesture()
                .is_some();
        self.pacing.set_engaged(dragging);
        self.frames += 1;
        if self.frames == 30 {
            println!("IRON_NATIVE_PRESENTATION {}", self.surface.presentation());
        }
        if self.app.rendered_last_frame() {
            self.rendered_frames += 1;
        }
        if opened {
            let version = self
                .app
                .world()
                .resource::<ironfell::document_bridge::DocumentStore>()
                .0
                .document()
                .version()
                .0 as u32;
            self.bridge
                .send(json!({"ty":"documentChanged","version":version}));
        }
        for event in self
            .app
            .world()
            .resource::<ironfell::host_events::HostEvents>()
            .drain()
        {
            // Font files ship inside the bundled frontend (`public/fonts`),
            // so the native host reads them in place of the web's fetch
            // (plans/specs/text.md §4). Installed fonts need no file at all.
            if let ironfell::host_events::EngineEvent::Fonts { fonts } = &event {
                use tauri::Manager;
                let resolver = self.native.app_handle().asset_resolver();
                for f in fonts {
                    match resolver.get(format!("fonts/{}", f.file)) {
                        Some(asset) => {
                            iron_text::engine().register(asset.bytes);
                        }
                        None => eprintln!("font {} ({}) is not in the bundle", f.family, f.file),
                    }
                }
                continue;
            }
            let value = serde_json::to_value(event).unwrap();
            self.bridge.send(value);
        }
        self.pacing.tick(
            timing,
            self.frames,
            start,
            input_received,
            input_age,
            self.app.rendered_last_frame(),
        );
        if self.pacing.should_stop() || self.layer_probe.as_ref().is_some_and(|p|p.finished()) {
            let _ = self.native.close();
        }
    }
    fn shutdown(mut self) {
        self.driver.pause();
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
        self.pacing.save();
    }
}
pub(crate) fn tick(timing: pacing::DisplayTiming) {
    HOST.with(|host| {
        if let Some(host) = host.borrow_mut().as_mut() {
            host.tick(timing);
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
fn open_session(
    app: tauri::AppHandle,
    bridge: tauri::State<'_, Arc<Bridge>>,
    channel: Channel<Value>,
) -> u32 {
    let generation = bridge.open(channel);
    wake(&app);
    generation
}
#[tauri::command]
fn close_session(bridge: tauri::State<'_, Arc<Bridge>>, generation: u32) {
    bridge.close(generation);
}
#[tauri::command]
fn send_batch(
    app: tauri::AppHandle,
    bridge: tauri::State<'_, Arc<Bridge>>,
    generation: u32,
    sequence: u32,
    messages: Vec<Value>,
) -> Result<(), String> {
    bridge.enqueue(generation, sequence, messages)?;
    wake(&app);
    Ok(())
}
#[tauri::command]
fn native_test(
    window: tauri::WebviewWindow,
    action: String,
    report: Option<Value>,
) -> Result<(), String> {
    if std::env::var_os("IRON_NATIVE_SELF_TEST").is_none() {
        return Err("self-test controls disabled".into());
    }
    match action.as_str() {
        "trace" => std::fs::write(
            ".agents/native-concurrency.json",
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .map_err(|e| e.to_string())?,
        "report" => println!("IRON_NATIVE_TEST {}", report.unwrap_or_default()),
        "resize" => window
            .set_size(tauri::PhysicalSize::new(1800, 1200))
            .map_err(|e| e.to_string())?,
        "minimize" => window.minimize().map_err(|e| e.to_string())?,
        "restore" => {
            window.unminimize().map_err(|e| e.to_string())?;
            window.set_focus().map_err(|e| e.to_string())?;
        }
        "close" => window.close().map_err(|e| e.to_string())?,
        _ => return Err("unknown self-test action".into()),
    }
    Ok(())
}
fn main() {
    let mut expose_diagnostic = std::env::var_os("IRON_NATIVE_LAYER_PROBE").is_some()
        || std::env::var_os("IRON_NATIVE_SELF_TEST").is_some()
        || std::env::var_os("IRON_NATIVE_PACING_TRACE").is_some();
    let bridge = Arc::new(Bridge::default());
    let setup_bridge = bridge.clone();
    let app = tauri::Builder::default()
        .manage(bridge)
        .invoke_handler(tauri::generate_handler![
            open_session,
            close_session,
            send_batch,
            agent::agent_connection,
            native_test
        ])
        .on_page_load(|webview, payload| {
            if std::env::var_os("IRON_NATIVE_SELF_TEST").is_some()
                && matches!(payload.event(), tauri::webview::PageLoadEvent::Finished)
            {
                let film = std::env::var_os("IRON_NATIVE_FILM").is_some();
                if film && !payload.url().query_pairs().any(|(k,v)| k == "latencyFilm" && v == "1") {
                    return;
                }
                let script = if std::env::var_os("IRON_NATIVE_AGENT_TEST").is_some() {
                    println!("IRON_NATIVE_AGENT_PHASE page-loaded");
                    include_str!("../test/agent.js")
                } else if film {
                    include_str!("../test/film.js")
                } else if std::env::var_os("IRON_NATIVE_CURSOR_TEST").is_some() {
                    include_str!("../test/cursors.js")
                } else {
                    include_str!("../test/workspace.js")
                };
                if let Err(error) = webview.eval(script) {
                    eprintln!("IRON_NATIVE_SELF_TEST_EVAL_ERROR {error}");
                }
            }
        })
        .setup(move |app| {
            let native = app
                .get_webview_window("main")
                .ok_or("missing native window")?;
            if std::env::var_os("IRON_NATIVE_AGENT_TOKEN").is_some()
                && std::env::var_os("IRON_NATIVE_FILM").is_none()
            {
                let mut url = native.url()?;
                if std::env::var_os("IRON_NATIVE_AGENT_TEST").is_some() {
                    url.query_pairs_mut().append_pair("demo", "");
                } else {
                    url.query_pairs_mut().append_pair("scope", "last");
                }
                native.navigate(url)?;
                app.handle().show()?;
                native.set_focus()?;
            }
            if std::env::var_os("IRON_NATIVE_FILM").is_some() {
                let mut url = native.url()?;
                let render = std::env::var("IRON_NATIVE_RENDER_MODE").unwrap_or("pipelined".into());
                let pacing = std::env::var("IRON_NATIVE_PACING").unwrap_or("display".into());
                url.query_pairs_mut().append_pair("latencyFilm", "1")
                    .append_pair("filmLabel", &format!("Native · {render} / {pacing}"));
                if std::env::var_os("IRON_NATIVE_LAYER_PROBE").is_some() {
                    let view=std::env::var("IRON_NATIVE_PROBE_WEBVIEW").unwrap_or("overlay".into());
                    url.query_pairs_mut().append_pair("layerProbe","1");
                    native.set_title(&format!("Iron — layers · {view} · {render} / {pacing}"))?;
                }
                native.navigate(url)?;
                if std::env::var_os("IRON_NATIVE_LAYER_PROBE").is_none() {
                    native.set_title(&format!("Iron — filming · {render} / {pacing}"))?;
                }
            }
            let owner = native.clone();
            let bridge = setup_bridge.clone();
            native.with_webview(move |webview| {
                let host = Host::new(owner, bridge, webview.inner())
                    .expect("native engine initialization failed");
                HOST.with(|slot| *slot.borrow_mut() = Some(host));
            })?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("native host construction failed");
    app.run_return(move |_, event| match event {
        RunEvent::MainEventsCleared if expose_diagnostic => {
            let window = HOST.with(|slot| slot.borrow().as_ref().map(|h| h.native.clone()));
            if let Some(window) = window {
                expose_diagnostic = false;
                // Run after window creation/launch, without borrowing HOST
                // across AppKit calls. Off-screen/hidden views do not tick.
                let _ = window.center();
                let _ = window.show();
                // Bounded GPU-readback controls need an unoccluded drawable.
                // Keep only these automated test windows above other apps.
                if std::env::var_os("IRON_NATIVE_PROBE_CAPTURE_DIR").is_some()
                    || std::env::var_os("IRON_NATIVE_AGENT_TEST").is_some()
                {
                    let _ = window.set_always_on_top(true);
                }
                let _ = window.unminimize();
                let _ = window.set_focus();
                println!(
                    "IRON_NATIVE_DIAGNOSTIC_WINDOW {}",
                    json!({
                        "visible":window.is_visible().ok(),
                        "minimized":window.is_minimized().ok(),
                        "position":window.outer_position().ok(),
                    })
                );
                HOST.with(|slot| {
                    if let Some(host) = slot.borrow_mut().as_mut() {
                        host.wake();
                    }
                });
            }
        }
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
