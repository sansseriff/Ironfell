//! Opt-in CPU scheduling trace. These times do not measure scanout/GPU completion.
use bevy::{
    prelude::*,
    render::{Render, RenderApp, RenderSystems, view::window::prepare_windows},
};
use objc2_quartz_core::CACurrentMediaTime;
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy)]
pub struct DisplayTiming {
    pub callback: f64,
    pub timestamp: f64,
    pub target: f64,
}
#[derive(Clone, Resource)]
pub struct Trace(Arc<Mutex<Data>>);
struct Data {
    path: String,
    epoch: f64,
    seconds: Option<f64>,
    rows: VecDeque<Row>,
}
#[derive(Serialize)]
struct Row {
    name: &'static str,
    start: f64,
    end: f64,
    frame: u64,
    target: f64,
    timestamp: f64,
    input_age_ms: Option<f64>,
    rendered: bool,
}
#[derive(Resource, Default)]
struct Stages {
    frame: u64,
    acquire: f64,
    render: f64,
}
impl Trace {
    fn record(&self, row: Row) {
        let mut data = self.0.lock().unwrap();
        if data.rows.len() == 24000 {
            data.rows.pop_front();
        }
        data.rows.push_back(row);
    }
    pub fn tick(
        &self,
        timing: DisplayTiming,
        frame: u64,
        input_age_ms: Option<f64>,
        rendered: bool,
    ) {
        self.record(Row {
            name: "tick",
            start: timing.callback,
            end: CACurrentMediaTime(),
            frame,
            target: timing.target,
            timestamp: timing.timestamp,
            input_age_ms,
            rendered,
        });
    }
    fn stage(&self, name: &'static str, frame: u64, start: f64) {
        self.record(Row {
            name,
            start,
            end: CACurrentMediaTime(),
            frame,
            target: 0.0,
            timestamp: 0.0,
            input_age_ms: None,
            rendered: true,
        });
    }
    pub fn should_stop(&self) -> bool {
        let d = self.0.lock().unwrap();
        d.seconds
            .is_some_and(|s| CACurrentMediaTime() - d.epoch >= s)
    }
    pub fn save(&self) {
        let d = self.0.lock().unwrap();
        let value = serde_json::json!({"epoch":d.epoch,"rows":d.rows,
            "presentMode":std::env::var("IRON_NATIVE_PRESENT_MODE").unwrap_or("fifo".into()),
            "frameLatency":std::env::var("IRON_NATIVE_FRAME_LATENCY").unwrap_or("2".into())});
        match std::fs::write(&d.path, serde_json::to_vec(&value).unwrap()) {
            Ok(()) => println!("IRON_NATIVE_PACING {} {} rows", d.path, d.rows.len()),
            Err(e) => eprintln!("could not save pacing trace: {e}"),
        }
    }
}
fn acquire_start(mut stages: ResMut<Stages>) {
    stages.frame += 1;
    stages.acquire = CACurrentMediaTime();
}
fn acquire_end(stages: Res<Stages>, trace: Res<Trace>) {
    trace.stage("acquire", stages.frame, stages.acquire);
}
fn render_start(mut stages: ResMut<Stages>) {
    stages.render = CACurrentMediaTime();
}
fn render_end(stages: Res<Stages>, trace: Res<Trace>) {
    trace.stage("render_submit", stages.frame, stages.render);
}

pub fn install(app: &mut ironfell::WorkerApp) -> Option<Trace> {
    let path = std::env::var("IRON_NATIVE_PACING_TRACE").ok()?;
    let trace = Trace(Arc::new(Mutex::new(Data {
        path,
        epoch: CACurrentMediaTime(),
        seconds: std::env::var("IRON_NATIVE_PACING_SECONDS")
            .ok()
            .and_then(|s| s.parse().ok()),
        rows: VecDeque::new(),
    })));
    let render = app
        .get_sub_app_mut(RenderApp)
        .expect("install before finalization");
    render.insert_resource(trace.clone());
    render.init_resource::<Stages>();
    render.add_systems(
        Render,
        (
            acquire_start
                .before(prepare_windows)
                .in_set(RenderSystems::PrepareViews),
            acquire_end
                .after(prepare_windows)
                .in_set(RenderSystems::PrepareViews),
            render_start
                .before(RenderSystems::Render)
                .after(RenderSystems::Prepare),
            render_end
                .after(RenderSystems::Render)
                .before(RenderSystems::Cleanup),
        ),
    );
    Some(trace)
}
