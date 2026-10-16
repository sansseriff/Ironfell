//! Opt-in feasibility instrumentation, not authored simulation semantics.
//! A single document rectangle named "native test" drives 100k runtime particles.
use bevy::{
    prelude::*,
    render::{Render, RenderApp, RenderSystems},
};
use iron_document::{ComponentKind, LeafPath, Value};
use ironfell::document_bridge::DocumentStore;
use serde::Serialize;
use serde_json::json;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Instant,
};
#[derive(Clone, Resource)]
pub struct Trace(Arc<Mutex<Data>>);
struct Data {
    epoch: Instant,
    spans: VecDeque<Span>,
    entities: usize,
    speeds: Vec<f32>,
}
#[derive(Clone, Serialize)]
struct Span {
    name: &'static str,
    start: u64,
    end: u64,
    thread: String,
}
#[derive(Component)]
struct Particle(f32);
#[derive(Component)]
struct LaneA;
#[derive(Component)]
struct LaneB;
#[derive(Resource, Default)]
struct Speed(f32);
#[derive(Resource)]
struct RenderStart(Instant);
impl Trace {
    fn record(&self, name: &'static str, start: Instant) {
        let mut d = self.0.lock().unwrap();
        let end = Instant::now();
        let span = Span {
            name,
            start: start.duration_since(d.epoch).as_micros() as u64,
            end: end.duration_since(d.epoch).as_micros() as u64,
            thread: format!("{:?}", std::thread::current().id()),
        };
        if d.spans.len() == 2048 {
            d.spans.pop_front();
        }
        d.spans.push_back(span);
    }
    pub fn snapshot(&self) -> serde_json::Value {
        let d = self.0.lock().unwrap();
        let overlaps =
            |a: &Span, b: &Span| a.start < b.end && b.start < a.end && a.thread != b.thread;
        let parallel = d
            .spans
            .iter()
            .filter(|a| a.name == "simulation_a")
            .filter(|a| {
                d.spans
                    .iter()
                    .any(|b| b.name == "simulation_b" && overlaps(a, b))
            })
            .count();
        let render_overlap = d
            .spans
            .iter()
            .filter(|a| a.name == "render")
            .filter(|a| {
                d.spans
                    .iter()
                    .any(|b| b.name.starts_with("simulation_") && overlaps(a, b))
            })
            .count();
        json!({"entities":d.entities,"ecsParallelIntervals":parallel,"renderSimulationOverlapIntervals":render_overlap,"documentSpeeds":d.speeds,"spans":d.spans})
    }
}
fn speed(store: Res<DocumentStore>, mut speed: ResMut<Speed>, trace: Res<Trace>) {
    let name = LeafPath::new(ComponentKind::Name, "text").unwrap();
    let x = LeafPath::new(ComponentKind::Transform2d, "x").unwrap();
    let source =
        store.0.document().live_nodes().find(
            |n| matches!(store.0.resolved(n.id,name),Some(Value::Str(s)) if s=="native test"),
        );
    let value = source
        .and_then(|n| store.0.resolved(n.id, x))
        .and_then(|v| {
            if let Value::Number(n) = v {
                Some(n as f32)
            } else {
                None
            }
        })
        .unwrap_or(0.0);
    speed.0 = value;
    let mut d = trace.0.lock().unwrap();
    if !d.speeds.contains(&value) {
        d.speeds.push(value);
    }
}
fn lane_a(
    mut q: Query<&mut Particle, (With<LaneA>, Without<LaneB>)>,
    speed: Res<Speed>,
    trace: Res<Trace>,
) {
    let start = Instant::now();
    for mut p in &mut q {
        for _ in 0..16 {
            p.0 = (p.0 + speed.0 * 0.00001).sin();
        }
    }
    trace.record("simulation_a", start);
}
fn lane_b(
    mut q: Query<&mut Particle, (With<LaneB>, Without<LaneA>)>,
    speed: Res<Speed>,
    trace: Res<Trace>,
) {
    let start = Instant::now();
    for mut p in &mut q {
        for _ in 0..16 {
            p.0 = (p.0 + speed.0 * 0.00002).cos();
        }
    }
    trace.record("simulation_b", start);
}
fn render_start(mut start: ResMut<RenderStart>) {
    start.0 = Instant::now();
}
fn render_end(start: Res<RenderStart>, trace: Res<Trace>) {
    trace.record("render", start.0);
}
pub fn install(app: &mut ironfell::WorkerApp) -> Option<Trace> {
    let count = std::env::var("IRON_NATIVE_SIMULATION")
        .ok()?
        .parse::<usize>()
        .ok()?
        .clamp(2, 1_000_000);
    let trace = Trace(Arc::new(Mutex::new(Data {
        epoch: Instant::now(),
        spans: VecDeque::new(),
        entities: count,
        speeds: Vec::new(),
    })));
    app.insert_resource(trace.clone());
    app.init_resource::<Speed>();
    for n in 0..count {
        if n % 2 == 0 {
            app.world_mut().spawn((Particle(n as f32 * 0.001), LaneA));
        } else {
            app.world_mut().spawn((Particle(n as f32 * 0.001), LaneB));
        }
    }
    app.add_systems(PreUpdate, speed);
    app.add_systems(Update, (lane_a, lane_b));
    let render = app
        .get_sub_app_mut(RenderApp)
        .expect("install before plugin finalization");
    render.insert_resource(trace.clone());
    render.insert_resource(RenderStart(Instant::now()));
    render.add_systems(
        Render,
        (
            render_start.before(RenderSystems::Render),
            render_end.after(RenderSystems::Render),
        ),
    );
    Some(trace)
}
