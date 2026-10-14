//! Diagnostic only: one complete Vello Hybrid scene per frame, without Bevy.
#![cfg(target_arch = "wasm32")]
use kurbo::{Affine, BezPath, Circle, PathEl, Point, Rect, Shape as _, Stroke};
use peniko::Color;
use serde::Deserialize;
use vello_hybrid::{RenderSize, RenderTargetConfig, Renderer, Resources, Scene, TextureBindings};
use wasm_bindgen::prelude::*;

#[derive(Debug)]
struct WebDisplay;
impl raw_window_handle::HasDisplayHandle for WebDisplay {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Ok(raw_window_handle::DisplayHandle::web())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    field: [f64; 2],
    sample_index: usize,
    physical_size: [u32; 2],
    shapes: Vec<Primitive>,
}
#[derive(Deserialize)]
struct Primitive {
    kind: String,
    matrix: [f64; 6],
    fill: Option<[f32; 4]>,
    stroke: Option<Outline>,
    #[serde(default)]
    w: f64,
    #[serde(default)]
    h: f64,
    #[serde(default)]
    r: f64,
    #[serde(default)]
    path: Vec<Vec<serde_json::Value>>,
}
#[derive(Deserialize)]
struct Outline {
    color: [f32; 4],
    width: f64,
}
fn color(c: [f32; 4]) -> Color {
    // Same byte quantization as ThorVG, keeping opacity/color comparable.
    Color::from_rgba8(
        (c[0] * 255.0).round() as u8,
        (c[1] * 255.0).round() as u8,
        (c[2] * 255.0).round() as u8,
        (c[3] * 255.0).round() as u8,
    )
}
fn path(s: &Primitive) -> BezPath {
    match s.kind.as_str() {
        "rect" => Rect::new(0., 0., s.w, s.h).into_path(0.1),
        "circle" => Circle::new((0., 0.), s.r).into_path(0.1),
        "path" => {
            let mut p = BezPath::new();
            for cmd in &s.path {
                let n = |i: usize| cmd[i].as_f64().unwrap();
                match cmd[0].as_str().unwrap() {
                    "moveTo" => p.move_to((n(1), n(2))),
                    "lineTo" => p.line_to((n(1), n(2))),
                    "bezierCurveTo" => p.curve_to((n(1), n(2)), (n(3), n(4)), (n(5), n(6))),
                    "closePath" => p.close_path(),
                    other => panic!("unsupported path operation {other}"),
                }
            }
            p
        }
        other => panic!("unsupported primitive {other}"),
    }
}

#[wasm_bindgen]
pub struct Probe {
    fixture: Fixture,
    paths: Vec<BezPath>,
    scratch: BezPath,
    zoom: f64,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    resources: Resources,
    scene: Scene,
}
#[wasm_bindgen]
impl Probe {
    /// Factors are generated once in JS for all renderers. Geometry is actually
    /// changed in local coordinates, not simulated with a scale transform.
    pub fn draw(
        &mut self,
        dx: f64,
        dy: f64,
        angle: f64,
        leaf_dx: f64,
        leaf_dy: f64,
        highlight: bool,
        geometry: bool,
        factors: &[f64],
        offsets: &[f64],
    ) -> Result<(), JsValue> {
        let n = self.fixture.shapes.len();
        if factors.len() != n || offsets.len() != n * 2 {
            return Err(JsValue::from_str("bad motion buffer"));
        }
        self.scene.reset();
        self.scene.set_paint(Color::from_rgba8(246, 247, 248, 255));
        self.scene.fill_rect(&Rect::new(
            0.,
            0.,
            self.fixture.physical_size[0] as f64,
            self.fixture.physical_size[1] as f64,
        ));
        let root = Affine::scale(self.zoom)
            * Affine::translate((self.fixture.field[0] + dx, self.fixture.field[1] + dy))
            * Affine::rotate(angle);
        for (i, s) in self.fixture.shapes.iter().enumerate() {
            let mut m = s.matrix;
            m[4] += offsets[i * 2];
            m[5] += offsets[i * 2 + 1];
            if i == self.fixture.sample_index {
                m[4] += leaf_dx;
                m[5] += leaf_dy;
            }
            self.scene.set_transform(root * Affine::new(m));
            let f = if geometry { factors[i] } else { 1. };
            // Reuse allocation, never retain a raster or screen-space scene.
            let p = if geometry {
                self.scratch.truncate(0);
                self.scratch
                    .extend(self.paths[i].elements().iter().copied());
                if s.kind == "path" {
                    let point = |p: Point| Point::new(p.x * f, p.y * (2. - f));
                    for el in self.scratch.elements_mut() {
                        *el = match *el {
                            PathEl::MoveTo(p) => PathEl::MoveTo(point(p)),
                            PathEl::LineTo(p) => PathEl::LineTo(point(p)),
                            PathEl::CurveTo(a, b, c) => {
                                let mut a = point(a);
                                let mut b = point(b);
                                a.y += 10. * (f - 1.);
                                b.y -= 10. * (f - 1.);
                                PathEl::CurveTo(a, b, point(c))
                            }
                            other => other,
                        };
                    }
                } else {
                    self.scratch.apply_affine(if s.kind == "circle" {
                        Affine::scale(f)
                    } else {
                        Affine::scale_non_uniform(f, 2. - f)
                    });
                }
                &self.scratch
            } else {
                &self.paths[i]
            };
            let rect = Rect::new(0., 0., s.w * f, s.h * (2. - f));
            if let Some(c) = s.fill {
                self.scene.set_paint(color(c));
                if s.kind == "rect" {
                    self.scene.fill_rect(&rect);
                } else {
                    self.scene.fill_path(p);
                }
            }
            if let Some(stroke) = &s.stroke {
                self.scene.set_paint(color(stroke.color));
                self.scene
                    .set_stroke(Stroke::new(stroke.width).with_miter_limit(4.));
                if s.kind == "rect" {
                    self.scene.stroke_rect(&rect);
                } else {
                    self.scene.stroke_path(p);
                }
            }
            if highlight {
                self.scene.set_paint(Color::from_rgba8(26, 102, 255, 128));
                self.scene.set_stroke(Stroke::new(2.).with_miter_limit(4.));
                if s.kind == "rect" {
                    self.scene.stroke_rect(&rect);
                } else {
                    self.scene.stroke_path(p);
                }
            }
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            e => {
                return Err(JsValue::from_str(&format!(
                    "surface acquisition failed: {e:?}"
                )));
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.renderer
            .render(
                &self.scene,
                &mut self.resources,
                &self.device,
                &self.queue,
                &mut encoder,
                &RenderSize {
                    width: self.fixture.physical_size[0],
                    height: self.fixture.physical_size[1],
                },
                &view,
                &TextureBindings::new(),
            )
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        self.queue.submit([encoder.finish()]);
        frame.present();
        Ok(())
    }
}

#[wasm_bindgen]
pub async fn create_probe(
    canvas: web_sys::HtmlCanvasElement,
    json: String,
    zoom: f64,
    webgpu: bool,
) -> Result<Probe, JsValue> {
    console_error_panic_hook::set_once();
    let fixture: Fixture =
        serde_json::from_str(&json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let [width, height] = fixture.physical_size;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: if webgpu {
            wgpu::Backends::BROWSER_WEBGPU
        } else {
            wgpu::Backends::GL
        },
        ..wgpu::InstanceDescriptor::new_with_display_handle(Box::new(WebDisplay))
    });
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        })
        .await
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let caps = surface.get_capabilities(&adapter);
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .unwrap_or(caps.formats[0]);
    surface.configure(
        &device,
        &wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        },
    );
    let (renderer, resources) = Renderer::new(
        &device,
        &RenderTargetConfig {
            format,
            width,
            height,
        },
    );
    let paths = fixture.shapes.iter().map(path).collect();
    Ok(Probe {
        scene: Scene::new(width as u16, height as u16),
        fixture,
        paths,
        scratch: BezPath::new(),
        zoom,
        surface,
        device,
        queue,
        renderer,
        resources,
    })
}
