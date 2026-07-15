use js_sys::Function;
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{window, CanvasRenderingContext2d, HtmlCanvasElement, OffscreenCanvas};

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestOptions {
    width: f64,
    height: f64,
    square_size: f64,
    dpr: f64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
pub struct Point {
    x: f64,
    y: f64,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FrameSample {
    rendered: Point,
    pointer: Point,
    dragging: bool,
    phase: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    worker_raf_now: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    worker_abs_raf_now: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    worker_render_start_now: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    worker_render_end_now: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    worker_abs_render_start_now: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    worker_abs_render_end_now: Option<f64>,
}

impl FrameSample {
    fn new(rendered: Point, pointer: Point, dragging: bool, phase: &'static str) -> Self {
        Self {
            rendered,
            pointer,
            dragging,
            phase,
            worker_raf_now: None,
            worker_abs_raf_now: None,
            worker_render_start_now: None,
            worker_render_end_now: None,
            worker_abs_render_start_now: None,
            worker_abs_render_end_now: None,
        }
    }
}

thread_local! {
    static BEVY_FRAME_CALLBACK: RefCell<Option<Function>> = const { RefCell::new(None) };
    static BEVY_PENDING_RESET: RefCell<Option<(Point, Point)>> = const { RefCell::new(None) };
}

#[wasm_bindgen]
pub fn start_winit_main(
    canvas: HtmlCanvasElement,
    options: JsValue,
    on_frame: Function,
) -> Result<WinitMainHandle, JsValue> {
    console_error_panic_hook::set_once();
    let options: TestOptions = serde_wasm_bindgen::from_value(options)?;
    winit_main::start(canvas, options, on_frame)
}

#[wasm_bindgen]
pub struct WinitMainHandle {
    state: Rc<RefCell<winit_main::WinitState>>,
}

#[wasm_bindgen]
impl WinitMainHandle {
    pub fn start(&self, pointer: JsValue, rendered: JsValue) -> Result<(), JsValue> {
        let pointer: Point = serde_wasm_bindgen::from_value(pointer)?;
        let rendered: Point = serde_wasm_bindgen::from_value(rendered)?;
        self.state.borrow_mut().reset(pointer, rendered);
        Ok(())
    }
}

#[wasm_bindgen]
pub fn start_bevy_main(
    canvas_id: String,
    options: JsValue,
    on_frame: Function,
) -> Result<BevyMainHandle, JsValue> {
    console_error_panic_hook::set_once();
    let options: TestOptions = serde_wasm_bindgen::from_value(options)?;
    BEVY_FRAME_CALLBACK.with(|callback| {
        *callback.borrow_mut() = Some(on_frame);
    });
    bevy_main::start(canvas_id, options);
    Ok(BevyMainHandle)
}

#[wasm_bindgen]
pub struct BevyMainHandle;

#[wasm_bindgen]
impl BevyMainHandle {
    pub fn start(&self, pointer: JsValue, rendered: JsValue) -> Result<(), JsValue> {
        let pointer: Point = serde_wasm_bindgen::from_value(pointer)?;
        let rendered: Point = serde_wasm_bindgen::from_value(rendered)?;
        BEVY_PENDING_RESET.with(|pending| {
            *pending.borrow_mut() = Some((pointer, rendered));
        });
        Ok(())
    }
}

#[wasm_bindgen]
pub async fn start_wgpu_worker(
    canvas: OffscreenCanvas,
    options: JsValue,
    on_frame: Function,
) -> Result<WgpuWorkerHandle, JsValue> {
    console_error_panic_hook::set_once();
    let options: TestOptions = serde_wasm_bindgen::from_value(options)?;
    rust_wgpu_worker::start(canvas, options, on_frame).await
}

#[wasm_bindgen]
pub struct WgpuWorkerHandle {
    inner: Rc<RefCell<rust_wgpu_worker::WgpuWorkerInner>>,
}

#[wasm_bindgen]
impl WgpuWorkerHandle {
    pub fn start(&self, pointer: JsValue, rendered: JsValue) -> Result<(), JsValue> {
        let pointer: Point = serde_wasm_bindgen::from_value(pointer)?;
        let rendered: Point = serde_wasm_bindgen::from_value(rendered)?;
        self.inner.borrow_mut().start(pointer, rendered);
        Ok(())
    }

    pub fn pointer_down(&self, point: JsValue) -> Result<(), JsValue> {
        let point: Point = serde_wasm_bindgen::from_value(point)?;
        self.inner.borrow_mut().pointer_down(point);
        Ok(())
    }

    pub fn pointer_move(&self, point: JsValue) -> Result<(), JsValue> {
        let point: Point = serde_wasm_bindgen::from_value(point)?;
        self.inner.borrow_mut().pointer_move(point);
        Ok(())
    }

    pub fn pointer_up(&self, point: JsValue) -> Result<(), JsValue> {
        let point: Point = serde_wasm_bindgen::from_value(point)?;
        self.inner.borrow_mut().pointer_up(point);
        Ok(())
    }

    pub fn stop(&self) {
        self.inner.borrow_mut().active = false;
    }

    pub fn dispose(&self) {
        self.inner.borrow_mut().disposed = true;
    }
}

#[wasm_bindgen]
pub async fn start_bevy_worker(
    _canvas: OffscreenCanvas,
    _options: JsValue,
    _on_frame: Function,
) -> Result<JsValue, JsValue> {
    Err(JsValue::from_str(
        "rust-bevy-worker needs a stripped version of the repo's OffscreenCanvas Bevy window plumbing; the entry point is reserved but not implemented yet.",
    ))
}

#[wasm_bindgen]
pub async fn start_bevy_vello_worker(
    _canvas: OffscreenCanvas,
    _options: JsValue,
    _on_frame: Function,
) -> Result<JsValue, JsValue> {
    Err(JsValue::from_str(
        "rust-bevy-vello-worker needs the stripped Bevy worker app plus bevy_vello wiring; the entry point is reserved but not implemented yet.",
    ))
}

fn send_frame(callback: &Function, frame: FrameSample) {
    if let Ok(value) = serde_wasm_bindgen::to_value(&frame) {
        let _ = callback.call1(&JsValue::NULL, &value);
    }
}

fn send_bevy_frame(frame: FrameSample) {
    BEVY_FRAME_CALLBACK.with(|callback| {
        if let Some(callback) = callback.borrow().as_ref() {
            send_frame(callback, frame);
        }
    });
}

mod winit_main {
    use super::*;
    use wasm_bindgen::JsValue;
    use winit::application::ApplicationHandler;
    use winit::dpi::{LogicalPosition, LogicalSize};
    use winit::event::{ElementState, MouseButton, WindowEvent};
    use winit::event_loop::{ActiveEventLoop, EventLoop};
    use winit::platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys, WindowExtWebSys};
    use winit::window::{Window, WindowAttributes, WindowId};

    pub fn start(
        canvas: HtmlCanvasElement,
        options: TestOptions,
        on_frame: Function,
    ) -> Result<WinitMainHandle, JsValue> {
        let event_loop = EventLoop::new().map_err(|error| JsValue::from_str(&error.to_string()))?;
        let state = Rc::new(RefCell::new(WinitState::new(Point {
            x: options.width / 2.0,
            y: options.height / 2.0,
        })));
        let app = WinitLatencyApp::new(canvas, options, on_frame, Rc::clone(&state));
        event_loop.spawn_app(app);
        Ok(WinitMainHandle { state })
    }

    pub struct WinitState {
        pointer: Point,
        rendered: Point,
        drag_offset: Point,
        dragging: bool,
        just_released: bool,
    }

    impl WinitState {
        fn new(initial: Point) -> Self {
            Self {
                pointer: initial,
                rendered: initial,
                drag_offset: Point::default(),
                dragging: false,
                just_released: false,
            }
        }

        pub fn reset(&mut self, pointer: Point, rendered: Point) {
            self.pointer = pointer;
            self.rendered = rendered;
            self.drag_offset = Point::default();
            self.dragging = false;
            self.just_released = false;
        }
    }

    struct WinitLatencyApp {
        canvas: Option<HtmlCanvasElement>,
        options: TestOptions,
        on_frame: Function,
        window: Option<Window>,
        ctx: Option<CanvasRenderingContext2d>,
        state: Rc<RefCell<WinitState>>,
    }

    impl WinitLatencyApp {
        fn new(
            canvas: HtmlCanvasElement,
            options: TestOptions,
            on_frame: Function,
            state: Rc<RefCell<WinitState>>,
        ) -> Self {
            Self {
                canvas: Some(canvas),
                options,
                on_frame,
                window: None,
                ctx: None,
                state,
            }
        }

        fn create_window(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let attrs = WindowAttributes::default()
                .with_title("latency rust winit")
                .with_inner_size(LogicalSize::new(self.options.width, self.options.height))
                .with_canvas(self.canvas.take());
            let Ok(window) = event_loop.create_window(attrs) else {
                return;
            };
            let Some(canvas) = window.canvas() else {
                return;
            };
            canvas.set_width((self.options.width * self.options.dpr).round() as u32);
            canvas.set_height((self.options.height * self.options.dpr).round() as u32);
            let style = canvas.style();
            let _ = style.set_property("width", &format!("{}px", self.options.width));
            let _ = style.set_property("height", &format!("{}px", self.options.height));
            self.ctx = canvas
                .get_context("2d")
                .ok()
                .flatten()
                .and_then(|value| value.dyn_into::<CanvasRenderingContext2d>().ok());
            if let Some(ctx) = &self.ctx {
                let _ = ctx.set_transform(
                    self.options.dpr,
                    0.0,
                    0.0,
                    self.options.dpr,
                    0.0,
                    0.0,
                );
            }
            window.request_redraw();
            self.window = Some(window);
        }

        fn cursor_moved(&mut self, position: LogicalPosition<f64>) {
            let mut state = self.state.borrow_mut();
            state.pointer = Point {
                x: position.x,
                y: position.y,
            };
            if state.dragging {
                state.rendered = Point {
                    x: state.pointer.x - state.drag_offset.x,
                    y: state.pointer.y - state.drag_offset.y,
                };
            }
        }

        fn mouse_input(&mut self, button_state: ElementState, button: MouseButton) {
            if button != MouseButton::Left {
                return;
            }
            let mut state = self.state.borrow_mut();
            match button_state {
                ElementState::Pressed => {
                    state.dragging = self.is_inside(state.pointer, state.rendered);
                    state.just_released = false;
                    state.drag_offset = Point {
                        x: state.pointer.x - state.rendered.x,
                        y: state.pointer.y - state.rendered.y,
                    };
                }
                ElementState::Released => {
                    state.just_released = state.dragging;
                    state.dragging = false;
                }
            }
        }

        fn draw(&mut self) {
            let Some(ctx) = &self.ctx else {
                return;
            };
            ctx.clear_rect(0.0, 0.0, self.options.width, self.options.height);
            ctx.set_fill_style_str("#f6f7f8");
            ctx.fill_rect(0.0, 0.0, self.options.width, self.options.height);
            self.draw_grid(ctx);
            self.draw_background_animation(ctx);
            let mut state = self.state.borrow_mut();
            self.draw_reference(ctx, &state);
            self.draw_square(ctx, &state);
            let phase = if state.dragging {
                "dragging"
            } else if state.just_released {
                "released"
            } else {
                "idle"
            };
            send_frame(
                &self.on_frame,
                FrameSample::new(state.rendered, state.pointer, state.dragging, phase),
            );
            state.just_released = false;
        }

        fn draw_grid(&self, ctx: &CanvasRenderingContext2d) {
            ctx.set_stroke_style_str("#dfe3e8");
            ctx.set_line_width(1.0);
            let mut x = 0.0;
            while x <= self.options.width {
                ctx.begin_path();
                ctx.move_to(x, 0.0);
                ctx.line_to(x, self.options.height);
                ctx.stroke();
                x += 80.0;
            }
            let mut y = 0.0;
            while y <= self.options.height {
                ctx.begin_path();
                ctx.move_to(0.0, y);
                ctx.line_to(self.options.width, y);
                ctx.stroke();
                y += 80.0;
            }
        }

        fn draw_background_animation(&self, ctx: &CanvasRenderingContext2d) {
            let t = window()
                .and_then(|window| window.performance())
                .map(|performance| performance.now() / 1000.0)
                .unwrap_or(0.0);
            let shapes = [
                (0.18, 0.50, 0.70, "#2f80ed"),
                (0.52, 0.35, 0.95, "#00a86b"),
                (0.78, 0.62, 1.25, "#7c3aed"),
                (0.35, 0.72, 1.55, "#eab308"),
            ];

            let _ = ctx.save();
            ctx.set_global_alpha(0.16);
            for (base_x, base_y, speed, color) in shapes {
                let x = self.options.width * base_x + (t * speed).sin() * 90.0;
                let y = self.options.height * base_y + (t * speed * 0.83).cos() * 46.0;
                ctx.set_fill_style_str(color);
                ctx.begin_path();
                let _ = ctx.arc(x, y, 42.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
            }
            let _ = ctx.restore();
        }

        fn draw_reference(&self, ctx: &CanvasRenderingContext2d, state: &WinitState) {
            ctx.set_stroke_style_str("#0877ff");
            ctx.set_line_width(2.0);
            ctx.begin_path();
            ctx.move_to(state.pointer.x - 12.0, state.pointer.y);
            ctx.line_to(state.pointer.x + 12.0, state.pointer.y);
            ctx.move_to(state.pointer.x, state.pointer.y - 12.0);
            ctx.line_to(state.pointer.x, state.pointer.y + 12.0);
            ctx.stroke();
        }

        fn draw_square(&self, ctx: &CanvasRenderingContext2d, state: &WinitState) {
            let half = self.options.square_size / 2.0;
            ctx.set_fill_style_str(if state.dragging { "#e53935" } else { "#30343b" });
            ctx.fill_rect(
                state.rendered.x - half,
                state.rendered.y - half,
                self.options.square_size,
                self.options.square_size,
            );
            ctx.set_stroke_style_str("#111827");
            ctx.set_line_width(2.0);
            ctx.stroke_rect(
                state.rendered.x - half,
                state.rendered.y - half,
                self.options.square_size,
                self.options.square_size,
            );
        }

        fn is_inside(&self, point: Point, rendered: Point) -> bool {
            let half = self.options.square_size / 2.0;
            (point.x - rendered.x).abs() <= half && (point.y - rendered.y).abs() <= half
        }
    }

    impl ApplicationHandler for WinitLatencyApp {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            self.create_window(event_loop);
        }

        fn window_event(
            &mut self,
            _event_loop: &ActiveEventLoop,
            _window_id: WindowId,
            event: WindowEvent,
        ) {
            match event {
                WindowEvent::CursorMoved { position, .. } => {
                    self.cursor_moved(position.to_logical(self.options.dpr));
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
                WindowEvent::MouseInput { state, button, .. } => {
                    self.mouse_input(state, button);
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
                WindowEvent::RedrawRequested => self.draw(),
                _ => {}
            }
        }

        fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }
}

mod rust_wgpu_worker {
    use super::*;
    use bytemuck::{Pod, Zeroable};
    use wasm_bindgen::closure::Closure;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_name = requestAnimationFrame)]
        fn request_animation_frame(callback: &Closure<dyn FnMut(f64)>) -> i32;

        #[wasm_bindgen(js_namespace = performance, js_name = now)]
        fn performance_now() -> f64;
    }

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    struct Vertex {
        position: [f32; 2],
        color: [f32; 4],
    }

    pub struct WgpuWorkerInner {
        options: TestOptions,
        on_frame: Function,
        renderer: WgpuRenderer,
        pointer: Point,
        rendered: Point,
        drag_offset: Point,
        dragging: bool,
        just_released: bool,
        pub active: bool,
        pub disposed: bool,
        raf_closure: Option<Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>>>,
    }

    struct WgpuRenderer {
        surface: wgpu::Surface<'static>,
        device: wgpu::Device,
        queue: wgpu::Queue,
        config: wgpu::SurfaceConfiguration,
        pipeline: wgpu::RenderPipeline,
        vertex_buffer: wgpu::Buffer,
    }

    pub async fn start(
        canvas: OffscreenCanvas,
        options: TestOptions,
        on_frame: Function,
    ) -> Result<WgpuWorkerHandle, JsValue> {
        canvas.set_width((options.width * options.dpr).round() as u32);
        canvas.set_height((options.height * options.dpr).round() as u32);
        let renderer = WgpuRenderer::new(canvas, options).await?;
        let initial = Point {
            x: options.width / 2.0,
            y: options.height / 2.0,
        };
        let inner = Rc::new(RefCell::new(WgpuWorkerInner {
            options,
            on_frame,
            renderer,
            pointer: initial,
            rendered: initial,
            drag_offset: Point::default(),
            dragging: false,
            just_released: false,
            active: false,
            disposed: false,
            raf_closure: None,
        }));
        install_raf_loop(&inner);
        Ok(WgpuWorkerHandle { inner })
    }

    fn install_raf_loop(inner: &Rc<RefCell<WgpuWorkerInner>>) {
        let slot: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
        let slot_for_closure = Rc::clone(&slot);
        let inner_for_closure = Rc::clone(inner);
        *slot.borrow_mut() = Some(Closure::wrap(Box::new(move |now: f64| {
            {
                let mut inner = inner_for_closure.borrow_mut();
                inner.frame(now);
                if inner.disposed {
                    return;
                }
            }
            if let Some(callback) = slot_for_closure.borrow().as_ref() {
                request_animation_frame(callback);
            }
        }) as Box<dyn FnMut(f64)>));

        if let Some(callback) = slot.borrow().as_ref() {
            request_animation_frame(callback);
        }
        inner.borrow_mut().raf_closure = Some(slot);
    }

    impl WgpuWorkerInner {
        pub fn start(&mut self, pointer: Point, rendered: Point) {
            self.pointer = pointer;
            self.rendered = rendered;
            self.drag_offset = Point::default();
            self.dragging = false;
            self.just_released = false;
            self.active = true;
        }

        pub fn pointer_down(&mut self, point: Point) {
            self.pointer = point;
            self.dragging = self.is_inside(point);
            self.just_released = false;
            self.drag_offset = Point {
                x: point.x - self.rendered.x,
                y: point.y - self.rendered.y,
            };
        }

        pub fn pointer_move(&mut self, point: Point) {
            self.pointer = point;
            self.just_released = false;
        }

        pub fn pointer_up(&mut self, point: Point) {
            self.pointer = point;
            self.just_released = self.dragging;
            self.dragging = false;
        }

        fn frame(&mut self, now: f64) {
            if self.dragging {
                self.rendered = Point {
                    x: self.pointer.x - self.drag_offset.x,
                    y: self.pointer.y - self.drag_offset.y,
                };
            }
            let render_start = performance_now();
            self.renderer.draw(
                self.options,
                self.pointer,
                self.rendered,
                self.dragging,
                now / 1000.0,
            );
            let render_end = performance_now();
            if self.active {
                let phase = if self.dragging {
                    "dragging"
                } else if self.just_released {
                    "released"
                } else {
                    "idle"
                };
                send_frame(
                    &self.on_frame,
                    FrameSample {
                        worker_raf_now: Some(now),
                        worker_render_start_now: Some(render_start),
                        worker_render_end_now: Some(render_end),
                        ..FrameSample::new(self.rendered, self.pointer, self.dragging, phase)
                    },
                );
            }
            self.just_released = false;
        }

        fn is_inside(&self, point: Point) -> bool {
            let half = self.options.square_size / 2.0;
            (point.x - self.rendered.x).abs() <= half && (point.y - self.rendered.y).abs() <= half
        }
    }

    impl WgpuRenderer {
        async fn new(canvas: OffscreenCanvas, options: TestOptions) -> Result<Self, JsValue> {
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
                backends: wgpu::Backends::BROWSER_WEBGPU,
                ..Default::default()
            });
            let surface = instance
                .create_surface(wgpu::SurfaceTarget::OffscreenCanvas(canvas))
                .map_err(|error| JsValue::from_str(&format!("failed to create wgpu surface: {error}")))?;
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                })
                .await
                .ok_or_else(|| JsValue::from_str("wgpu adapter unavailable in worker"))?;
            let (device, queue) = adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: Some("latency rust worker device"),
                        required_features: wgpu::Features::empty(),
                        required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                            .using_resolution(adapter.limits()),
                        memory_hints: wgpu::MemoryHints::Performance,
                    },
                    None,
                )
                .await
                .map_err(|error| JsValue::from_str(&format!("failed to request wgpu device: {error}")))?;

            let width = (options.width * options.dpr).round().max(1.0) as u32;
            let height = (options.height * options.dpr).round().max(1.0) as u32;
            let mut config = surface
                .get_default_config(&adapter, width, height)
                .ok_or_else(|| JsValue::from_str("surface has no default config"))?;
            config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
            surface.configure(&device, &config);

            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("latency rust worker shader"),
                source: wgpu::ShaderSource::Wgsl(
                    r#"
                    struct VertexOut {
                        @builtin(position) position: vec4f,
                        @location(0) color: vec4f,
                    };

                    @vertex
                    fn vs_main(@location(0) position: vec2f, @location(1) color: vec4f) -> VertexOut {
                        var out: VertexOut;
                        out.position = vec4f(position, 0.0, 1.0);
                        out.color = color;
                        return out;
                    }

                    @fragment
                    fn fs_main(@location(0) color: vec4f) -> @location(0) vec4f {
                        return color;
                    }
                    "#.into(),
                ),
            });

            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("latency rust worker pipeline"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[
                            wgpu::VertexAttribute {
                                offset: 0,
                                shader_location: 0,
                                format: wgpu::VertexFormat::Float32x2,
                            },
                            wgpu::VertexAttribute {
                                offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                                shader_location: 1,
                                format: wgpu::VertexFormat::Float32x4,
                            },
                        ],
                    }],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

            let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("latency rust worker vertices"),
                size: 16_384,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            Ok(Self {
                surface,
                device,
                queue,
                config,
                pipeline,
                vertex_buffer,
            })
        }

        fn draw(
            &mut self,
            options: TestOptions,
            pointer: Point,
            rendered: Point,
            dragging: bool,
            t: f64,
        ) {
            let Ok(frame) = self.surface.get_current_texture() else {
                self.surface.configure(&self.device, &self.config);
                return;
            };
            let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
            let vertices = build_vertices(options, pointer, rendered, dragging, t);
            self.queue
                .write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&vertices));

            let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("latency rust worker encoder"),
            });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("latency rust worker pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.965,
                                g: 0.969,
                                b: 0.973,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                pass.draw(0..vertices.len() as u32, 0..1);
            }
            self.queue.submit(Some(encoder.finish()));
            frame.present();
        }
    }

    fn build_vertices(
        options: TestOptions,
        pointer: Point,
        rendered: Point,
        dragging: bool,
        t: f64,
    ) -> Vec<Vertex> {
        let mut vertices = Vec::with_capacity(96);
        add_background_shapes(&mut vertices, options, t);
        add_rect(&mut vertices, options, pointer.x - 1.0, pointer.y - 14.0, 2.0, 28.0, [0.031, 0.467, 1.0, 1.0]);
        add_rect(&mut vertices, options, pointer.x - 14.0, pointer.y - 1.0, 28.0, 2.0, [0.031, 0.467, 1.0, 1.0]);
        let half = options.square_size / 2.0;
        add_rect(
            &mut vertices,
            options,
            rendered.x - half,
            rendered.y - half,
            options.square_size,
            options.square_size,
            if dragging {
                [0.898, 0.224, 0.208, 1.0]
            } else {
                [0.188, 0.204, 0.231, 1.0]
            },
        );
        vertices
    }

    fn add_background_shapes(vertices: &mut Vec<Vertex>, options: TestOptions, t: f64) {
        let shapes = [
            (0.18, 0.50, 0.70, [0.184, 0.502, 0.929, 0.16]),
            (0.52, 0.35, 0.95, [0.000, 0.659, 0.420, 0.16]),
            (0.78, 0.62, 1.25, [0.486, 0.227, 0.929, 0.16]),
            (0.35, 0.72, 1.55, [0.918, 0.702, 0.031, 0.16]),
        ];
        for (base_x, base_y, speed, color) in shapes {
            let x = options.width * base_x + (t * speed).sin() * 90.0;
            let y = options.height * base_y + (t * speed * 0.83).cos() * 46.0;
            add_rect(vertices, options, x - 42.0, y - 42.0, 84.0, 84.0, color);
        }
    }

    fn add_rect(
        vertices: &mut Vec<Vertex>,
        options: TestOptions,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        color: [f32; 4],
    ) {
        let x0 = (x / options.width) as f32 * 2.0 - 1.0;
        let x1 = ((x + w) / options.width) as f32 * 2.0 - 1.0;
        let y0 = 1.0 - (y / options.height) as f32 * 2.0;
        let y1 = 1.0 - ((y + h) / options.height) as f32 * 2.0;
        vertices.extend_from_slice(&[
            Vertex { position: [x0, y0], color },
            Vertex { position: [x1, y0], color },
            Vertex { position: [x0, y1], color },
            Vertex { position: [x0, y1], color },
            Vertex { position: [x1, y0], color },
            Vertex { position: [x1, y1], color },
        ]);
    }
}

mod bevy_main {
    use super::*;
    use bevy::prelude::*;
    use bevy::window::{PrimaryWindow, PresentMode, WindowPlugin, WindowResolution};

    #[derive(Resource, Clone, Copy)]
    struct LatencyOptions {
        width: f32,
        height: f32,
        square_size: f32,
    }

    #[derive(Resource, Clone, Copy)]
    struct DragModel {
        pointer: Vec2,
        rendered: Vec2,
        drag_offset: Vec2,
        dragging: bool,
        just_released: bool,
    }

    #[derive(Component)]
    struct DraggableSquare;

    #[derive(Component)]
    struct PointerLine;

    #[derive(Component)]
    struct BackgroundShape {
        base: Vec2,
        radius: Vec2,
        speed: f32,
        phase: f32,
    }

    pub fn start(canvas_id: String, options: TestOptions) {
        let selector = if canvas_id.starts_with('#') {
            canvas_id
        } else {
            format!("#{canvas_id}")
        };
        let opts = LatencyOptions {
            width: options.width as f32,
            height: options.height as f32,
            square_size: options.square_size as f32,
        };
        let initial = Vec2::new(options.width as f32 / 2.0, options.height as f32 / 2.0);

        App::new()
            .insert_resource(ClearColor(Color::srgb(0.965, 0.969, 0.973)))
            .insert_resource(opts)
            .insert_resource(DragModel {
                pointer: initial,
                rendered: initial,
                drag_offset: Vec2::ZERO,
                dragging: false,
                just_released: false,
            })
            .add_plugins(DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "latency rust bevy main".to_string(),
                    canvas: Some(selector),
                    fit_canvas_to_parent: false,
                    resolution: WindowResolution::new(opts.width, opts.height),
                    present_mode: PresentMode::AutoNoVsync,
                    prevent_default_event_handling: true,
                    ..default()
                }),
                ..default()
            }))
            .add_systems(Startup, setup)
            .add_systems(Update, (animate_background_shapes, apply_pending_reset, track_cursor, track_buttons, apply_drag, emit_frame).chain())
            .run();
    }

    fn setup(mut commands: Commands, opts: Res<LatencyOptions>) {
        commands.spawn(Camera2d);
        spawn_grid(&mut commands, *opts);
        spawn_background_shapes(&mut commands, *opts);
        commands.spawn((
            Sprite::from_color(Color::srgb(0.031, 0.467, 1.0), Vec2::new(2.0, 28.0)),
            Transform::from_translation(Vec3::new(0.0, 0.0, 2.0)),
            PointerLine,
        ));
        commands.spawn((
            Sprite::from_color(Color::srgb(0.031, 0.467, 1.0), Vec2::new(28.0, 2.0)),
            Transform::from_translation(Vec3::new(0.0, 0.0, 2.0)),
            PointerLine,
        ));
        commands.spawn((
            Sprite::from_color(
                Color::srgb(0.188, 0.204, 0.231),
                Vec2::splat(opts.square_size),
            ),
            Transform::from_translation(Vec3::new(0.0, 0.0, 3.0)),
            DraggableSquare,
        ));
    }

    fn spawn_grid(commands: &mut Commands, opts: LatencyOptions) {
        let color = Color::srgb(0.875, 0.890, 0.910);
        let mut x = 0.0;
        while x <= opts.width {
            let world = screen_to_world(Vec2::new(x, opts.height / 2.0), opts);
            commands.spawn((
                Sprite::from_color(color, Vec2::new(1.0, opts.height)),
                Transform::from_translation(Vec3::new(world.x, world.y, 0.0)),
            ));
            x += 80.0;
        }
        let mut y = 0.0;
        while y <= opts.height {
            let world = screen_to_world(Vec2::new(opts.width / 2.0, y), opts);
            commands.spawn((
                Sprite::from_color(color, Vec2::new(opts.width, 1.0)),
                Transform::from_translation(Vec3::new(world.x, world.y, 0.0)),
            ));
            y += 80.0;
        }
    }

    fn spawn_background_shapes(commands: &mut Commands, opts: LatencyOptions) {
        let shapes = [
            (Vec2::new(0.18, 0.50), Vec2::new(90.0, 46.0), 0.70, 0.0, Color::srgba(0.184, 0.502, 0.929, 0.16)),
            (Vec2::new(0.52, 0.35), Vec2::new(82.0, 52.0), 0.95, 1.4, Color::srgba(0.000, 0.659, 0.420, 0.16)),
            (Vec2::new(0.78, 0.62), Vec2::new(76.0, 42.0), 1.25, 2.2, Color::srgba(0.486, 0.227, 0.929, 0.16)),
            (Vec2::new(0.35, 0.72), Vec2::new(86.0, 38.0), 1.55, 0.8, Color::srgba(0.918, 0.702, 0.031, 0.16)),
        ];
        for (base_fraction, radius, speed, phase, color) in shapes {
            let screen = Vec2::new(opts.width * base_fraction.x, opts.height * base_fraction.y);
            let world = screen_to_world(screen, opts);
            commands.spawn((
                Sprite::from_color(color, Vec2::splat(84.0)),
                Transform::from_translation(Vec3::new(world.x, world.y, 1.0)),
                BackgroundShape {
                    base: screen,
                    radius,
                    speed,
                    phase,
                },
            ));
        }
    }

    fn animate_background_shapes(
        time: Res<Time>,
        opts: Res<LatencyOptions>,
        mut shapes: Query<(&BackgroundShape, &mut Transform)>,
    ) {
        let t = time.elapsed_secs();
        for (shape, mut transform) in &mut shapes {
            let screen = Vec2::new(
                shape.base.x + (t * shape.speed + shape.phase).sin() * shape.radius.x,
                shape.base.y + (t * shape.speed * 0.83 + shape.phase).cos() * shape.radius.y,
            );
            let world = screen_to_world(screen, *opts);
            transform.translation.x = world.x;
            transform.translation.y = world.y;
        }
    }

    fn track_cursor(
        mut cursor_events: EventReader<CursorMoved>,
        opts: Res<LatencyOptions>,
        mut model: ResMut<DragModel>,
    ) {
        for event in cursor_events.read() {
            let x = event.position.x.clamp(0.0, opts.width);
            let y = event.position.y.clamp(0.0, opts.height);
            model.pointer = Vec2::new(x, y);
        }
    }

    fn apply_pending_reset(mut model: ResMut<DragModel>) {
        BEVY_PENDING_RESET.with(|pending| {
            let Some((pointer, rendered)) = pending.borrow_mut().take() else {
                return;
            };
            model.pointer = Vec2::new(pointer.x as f32, pointer.y as f32);
            model.rendered = Vec2::new(rendered.x as f32, rendered.y as f32);
            model.drag_offset = Vec2::ZERO;
            model.dragging = false;
            model.just_released = false;
        });
    }

    fn track_buttons(buttons: Res<ButtonInput<MouseButton>>, opts: Res<LatencyOptions>, mut model: ResMut<DragModel>) {
        if buttons.just_pressed(MouseButton::Left) {
            model.dragging = is_inside(model.pointer, model.rendered, opts.square_size);
            model.just_released = false;
            model.drag_offset = model.pointer - model.rendered;
        }
        if buttons.just_released(MouseButton::Left) {
            model.just_released = model.dragging;
            model.dragging = false;
        }
    }

    fn apply_drag(
        opts: Res<LatencyOptions>,
        mut model: ResMut<DragModel>,
        square: Single<(&mut Transform, &mut Sprite), With<DraggableSquare>>,
        mut lines: Query<&mut Transform, (With<PointerLine>, Without<DraggableSquare>)>,
        window: Single<&Window, With<PrimaryWindow>>,
    ) {
        if let Some(cursor) = window.cursor_position() {
            model.pointer = Vec2::new(cursor.x.clamp(0.0, opts.width), cursor.y.clamp(0.0, opts.height));
        }
        if model.dragging {
            model.rendered = model.pointer - model.drag_offset;
        }

        let (mut transform, mut sprite) = square.into_inner();
        let world = screen_to_world(model.rendered, *opts);
        transform.translation = Vec3::new(world.x, world.y, 3.0);
        sprite.color = if model.dragging {
            Color::srgb(0.898, 0.224, 0.208)
        } else {
            Color::srgb(0.188, 0.204, 0.231)
        };

        let pointer_world = screen_to_world(model.pointer, *opts);
        for mut line in &mut lines {
            line.translation.x = pointer_world.x;
            line.translation.y = pointer_world.y;
        }
    }

    fn emit_frame(model: Res<DragModel>) {
        let phase = if model.dragging {
            "dragging"
        } else if model.just_released {
            "released"
        } else {
            "idle"
        };
        send_bevy_frame(FrameSample::new(
            Point {
                x: model.rendered.x as f64,
                y: model.rendered.y as f64,
            },
            Point {
                x: model.pointer.x as f64,
                y: model.pointer.y as f64,
            },
            model.dragging,
            phase,
        ));
    }

    fn is_inside(point: Vec2, rendered: Vec2, square_size: f32) -> bool {
        let half = square_size / 2.0;
        (point.x - rendered.x).abs() <= half && (point.y - rendered.y).abs() <= half
    }

    fn screen_to_world(point: Vec2, opts: LatencyOptions) -> Vec2 {
        Vec2::new(point.x - opts.width / 2.0, opts.height / 2.0 - point.y)
    }
}
