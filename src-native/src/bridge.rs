//! Bounded control mailbox. IPC threads never borrow the ECS world.
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Mutex};
use tauri::ipc::Channel;

const MAX_PENDING: usize = 1024;
const MAX_BATCH: usize = 256;
const MAX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Default)]
pub struct Bridge(pub Mutex<Session>);
#[derive(Default)]
pub struct Session {
    pub generation: u32,
    sequence: u32,
    pub ready: bool,
    pub channel: Option<Channel<Value>>,
    pub inbox: VecDeque<Request>,
    bytes: usize,
}

#[derive(Deserialize)]
#[serde(tag = "ty", rename_all = "camelCase")]
pub enum Request {
    Resize {
        dpr: f32,
    },
    SetPanelViewport {
        id: String,
        kind: String,
        composition: String,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    DespawnPanel {
        id: String,
    },
    Mousemove {
        x: f32,
        y: f32,
    },
    LeftBtDown {
        x: Option<f32>,
        y: Option<f32>,
    },
    LeftBtUp,
    MiddleBt {
        down: bool,
    },
    Modifiers {
        m: u8,
    },
    ToolKey {
        key: String,
        m: u8,
    },
    Paste {
        text: String,
    },
    TextInput {
        text: String,
        anchor: u32,
        focus: u32,
    },
    TextMotion {
        motion: String,
        extend: bool,
    },
    TextToggle {
        what: String,
    },
    Theme {
        dark: bool,
    },
    MouseWheel {
        dx: f32,
        dy: f32,
        mode: u32,
    },
    AutoAnimation {
        #[serde(rename = "autoAnimation")]
        auto_animation: u32,
    },
    Keydown {
        key: String,
    },
    Keyup {
        key: String,
    },
    Undo,
    Redo,
    StartRunning,
    StopRunning,
    ReleaseApp,
    DocumentSave {
        #[serde(rename = "requestId")]
        request_id: i64,
    },
    DocumentLoad {
        #[serde(rename = "requestId")]
        request_id: i64,
        text: String,
    },
    DocumentView {
        #[serde(rename = "requestId")]
        request_id: i64,
        query: Value,
    },
    DocumentCall {
        #[serde(rename = "requestId")]
        request_id: i64,
        r#fn: String,
        args: Vec<Value>,
    },
    SolveBench {
        #[serde(rename = "requestId")]
        request_id: i64,
        iterations: u32,
        repeats: u32,
    },
    ProbeStats,
    ProbeReset,
}

impl Bridge {
    pub fn open(&self, channel: Channel<Value>) -> u32 {
        let mut s = self.0.lock().unwrap();
        let generation = s.generation.wrapping_add(1).max(1);
        *s = Session {
            generation,
            channel: Some(channel),
            ..Default::default()
        };
        generation
    }
    pub fn close(&self, generation: u32) {
        let mut s = self.0.lock().unwrap();
        if s.generation == generation {
            s.channel = None;
            s.inbox.clear();
            s.bytes = 0;
        }
    }
    pub fn enqueue(
        &self,
        generation: u32,
        sequence: u32,
        messages: Vec<Value>,
    ) -> Result<(), String> {
        if messages.len() > MAX_BATCH {
            return Err("control batch exceeds 256 messages".into());
        }
        let bytes = serde_json::to_vec(&messages)
            .map_err(|e| e.to_string())?
            .len();
        let decoded = messages
            .into_iter()
            .map(serde_json::from_value)
            .collect::<Result<Vec<Request>, _>>()
            .map_err(|e| e.to_string())?;
        let mut s = self.0.lock().unwrap();
        if s.channel.is_none() || generation != s.generation {
            return Err("stale native session".into());
        }
        if sequence != s.sequence + 1 {
            return Err("out-of-order control batch".into());
        }
        if s.inbox.len() + decoded.len() > MAX_PENDING || s.bytes + bytes > MAX_BYTES {
            return Err("native control mailbox full".into());
        }
        s.sequence = sequence;
        s.bytes += bytes;
        s.inbox.extend(decoded);
        Ok(())
    }
    pub fn pop(&self) -> Option<Request> {
        let mut s = self.0.lock().unwrap();
        let next = s.inbox.pop_front();
        // Conservative byte accounting: release the batch budget only once empty.
        if s.inbox.is_empty() {
            s.bytes = 0;
        }
        next
    }
    pub fn send(&self, value: Value) {
        let s = self.0.lock().unwrap();
        if let Some(channel) = &s.channel {
            let _ = channel.send(value);
        }
    }
    pub fn prepare(&self, direct_pointer: bool) -> bool {
        let mut s = self.0.lock().unwrap();
        if s.channel.is_some() && !s.ready {
            s.ready = true;
            let _ = s
                .channel
                .as_ref()
                .unwrap()
                .send(json!({"ty":"enginePrepared","pointerSource":if direct_pointer{"native"}else{"webview"}}));
            return true;
        }
        false
    }
}

pub fn dispatch(app: &mut ironfell::WorkerApp, request: Request) -> (Option<Value>, bool) {
    use Request::*;
    use ironfell::engine_api as api;
    let mut edge = false;
    let answer = match request {
        Resize { dpr } => {
            api::set_scale_factor(app, dpr);
            None
        }
        SetPanelViewport {
            id,
            kind,
            composition,
            x,
            y,
            w,
            h,
        } => {
            api::set_panel_viewport(app, id, kind, composition, x, y, w, h);
            None
        }
        DespawnPanel { id } => {
            api::despawn_panel(app, id);
            None
        }
        Mousemove { x, y } => {
            api::mouse_move(app, x, y);
            None
        }
        LeftBtDown { x, y } => {
            if let (Some(x), Some(y)) = (x, y) {
                api::left_bt_down_at(app, x, y)
            } else {
                api::left_bt_down(app)
            };
            edge = true;
            None
        }
        LeftBtUp => {
            api::left_bt_up(app);
            edge = true;
            None
        }
        MiddleBt { down } => {
            api::middle_bt(app, down);
            edge = true;
            None
        }
        Modifiers { m } => {
            api::set_modifiers(app, m);
            None
        }
        ToolKey { key, m } => {
            api::tool_key(app, key, m);
            edge = true;
            None
        }
        Paste { text } => {
            api::clipboard_paste(app, text);
            edge = true;
            None
        }
        TextInput { text, anchor, focus } => {
            api::text_input(app, text, anchor, focus);
            edge = true;
            None
        }
        TextMotion { motion, extend } => {
            api::text_motion(app, motion, extend);
            edge = true;
            None
        }
        TextToggle { what } => {
            api::text_toggle(app, what);
            edge = true;
            None
        }
        Theme { dark } => {
            api::set_dark_mode(app, dark);
            None
        }
        MouseWheel { dx, dy, mode } => {
            api::mouse_wheel(app, dx, dy, mode);
            None
        }
        AutoAnimation { auto_animation } => {
            api::set_auto_animation(app, auto_animation);
            None
        }
        Keydown { key } => {
            api::key_down(app, key);
            edge = true;
            None
        }
        Keyup { key } => {
            api::key_up(app, key);
            edge = true;
            None
        }
        Undo => {
            api::undo(app);
            edge = true;
            None
        }
        Redo => {
            api::redo(app);
            edge = true;
            None
        }
        DocumentSave { request_id } => Some(
            json!({"ty":"documentSaved","requestId":request_id,"text":api::document_save(app)}),
        ),
        DocumentLoad { request_id, text } => {
            edge = true;
            Some(reply(
                "documentLoaded",
                request_id,
                api::document_load(app, text).map(|_| String::new()),
            ))
        }
        DocumentView { request_id, query } => Some(reply(
            "documentView",
            request_id,
            api::document_view(app, query.to_string()),
        )),
        DocumentCall {
            request_id,
            r#fn,
            args,
        } => Some(reply(
            "documentCall",
            request_id,
            ironfell::engine_dispatch::call(app, &r#fn, &args),
        )),
        SolveBench {
            request_id,
            iterations,
            repeats,
        } => Some(
            json!({"ty":"solveBench","requestId":request_id,"text":api::solve_bench(iterations.min(10000),repeats.min(100))}),
        ),
        StartRunning | StopRunning | ReleaseApp | ProbeStats | ProbeReset => None,
    };
    (answer, edge)
}
fn reply(ty: &str, request_id: i64, result: Result<String, String>) -> Value {
    match result {
        Ok(text) => json!({"ty":ty,"requestId":request_id,"ok":true,"text":text}),
        Err(error) => json!({"ty":ty,"requestId":request_id,"ok":false,"error":error}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn channel() -> Channel<Value> {
        Channel::new(|_| Ok(()))
    }
    #[test]
    fn replacement_rejects_stale_batches_and_disposal() {
        let bridge = Bridge::default();
        let old = bridge.open(channel());
        let current = bridge.open(channel());
        assert!(bridge.enqueue(old, 1, vec![json!({"ty":"undo"})]).is_err());
        bridge.close(old);
        assert!(
            bridge
                .enqueue(current, 1, vec![json!({"ty":"undo"})])
                .is_ok()
        );
        assert!(matches!(bridge.pop(), Some(Request::Undo)));
    }
    #[test]
    fn batch_validation_is_atomic_and_backpressure_is_explicit() {
        let bridge = Bridge::default();
        let generation = bridge.open(channel());
        assert!(
            bridge
                .enqueue(
                    generation,
                    1,
                    vec![json!({"ty":"undo"}), json!({"ty":"unknown"})]
                )
                .is_err()
        );
        assert!(bridge.pop().is_none());
        assert!(
            bridge
                .enqueue(generation, 1, vec![json!({"ty":"undo"}); 257])
                .is_err()
        );
        for sequence in 1..=4 {
            assert!(
                bridge
                    .enqueue(generation, sequence, vec![json!({"ty":"undo"}); 256])
                    .is_ok()
            );
        }
        assert!(
            bridge
                .enqueue(generation, 5, vec![json!({"ty":"redo"})])
                .is_err()
        );
        for _ in 0..1024 {
            assert!(matches!(bridge.pop(), Some(Request::Undo)));
        }
        assert!(
            bridge
                .enqueue(generation, 5, vec![json!({"ty":"redo"})])
                .is_ok()
        );
        assert!(bridge.enqueue(generation, 5, vec![]).is_err());
    }
}
