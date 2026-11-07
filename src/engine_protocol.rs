//! Shared UI input/control protocol. Hosts own acquisition, surfaces and scheduling;
//! all ordinary engine messages are decoded and dispatched here.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "ty", rename_all = "camelCase")]
pub enum Request {
    SetUiInterest {
        all: bool,
        compositions: bool,
        nodes: Vec<String>,
    },
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
    /// Dev: destroy the GPU device, as a browser's GPU process dropping it
    /// would, to exercise the renderer's recovery (`render_loss`).
    LoseDevice,
}

pub fn dispatch(app: &mut crate::WorkerApp, request: Request) -> (Option<Value>, bool) {
    use Request::*;
    use crate::engine_api as api;
    let queued = matches!(&request, DocumentLoad { .. })
        || matches!(&request, DocumentCall { r#fn, .. } if r#fn == "load");
    let document_call = matches!(&request, DocumentCall { .. });
    let mut edge = false;
    let answer = match request {
        SetUiInterest { all, compositions, nodes } => {
            if let Some(mut interests) = app.world_mut().get_resource_mut::<crate::document_bridge::ui::UiInterests>() {
                interests.all = all;
                interests.compositions = compositions;
                interests.nodes = nodes.into_iter().filter_map(|n| n.parse().ok()).collect();
            }
            None
        }
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
            json!({"ty":"documentSaved","requestId":request_id,"text":crate::engine_dispatch::call(app, "save", &[]).expect("save has no arguments")}),
        ),
        DocumentLoad { request_id, text } => {
            edge = true;
            Some(reply(
                "documentLoaded",
                request_id,
                crate::engine_dispatch::call(app, "load", &[Value::String(text)]),
            ))
        }
        DocumentView { request_id, query } => Some(reply(
            "documentView",
            request_id,
            crate::engine_dispatch::call(app, "view", &[Value::String(query.to_string())]),
        )),
        DocumentCall {
            request_id,
            r#fn,
            args,
        } => Some(reply(
            "documentCall",
            request_id,
            crate::engine_dispatch::call(app, &r#fn, &args),
        )),
        SolveBench {
            request_id,
            iterations,
            repeats,
        } => Some(
            json!({"ty":"solveBench","requestId":request_id,"text":api::solve_bench(iterations.min(10000),repeats.min(100))}),
        ),
        StartRunning | StopRunning | ReleaseApp | ProbeStats | ProbeReset => None,
        LoseDevice => {
            if let Some(device) = app.world().get_resource::<bevy::render::renderer::RenderDevice>() {
                device.wgpu_device().destroy();
            }
            None
        }
    };
    let answer = answer.map(|mut value| {
        let applied = document_call && value.get("text").and_then(Value::as_str)
            .filter(|text| text.len() < 65536)
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .is_some_and(|result| result.get("ok") == Some(&Value::Bool(true)) && result.get("version").is_some());
        value["ack"] = json!(if queued { "queued" } else if applied { "applied" } else { "completed" });
        if let Some(store) = app.world().get_resource::<crate::document_bridge::DocumentStore>() {
            value["version"] = json!(store.0.document().version().0);
            value["sessionVersion"] = json!(store.0.session_version());
        }
        value
    });
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
    #[test]
    fn shared_input_messages_round_trip_and_reject_incomplete_text() {
        for message in [
            json!({"ty":"paste","text":"<svg/>"}),
            json!({"ty":"textInput","text":"hello","anchor":1,"focus":5}),
            json!({"ty":"leftBtDown","x":50.25,"y":60.5}),
            json!({"ty":"toolKey","key":"v","m":0}),
        ] {
            let request: Request = serde_json::from_value(message.clone()).unwrap();
            assert_eq!(serde_json::to_value(request).unwrap(), message);
        }
        assert!(serde_json::from_value::<Request>(json!({"ty":"textInput","text":"hello"})).is_err());
        assert!(serde_json::from_value::<Request>(json!({"ty":"textInput","text":"hello","anchor":-1,"focus":0})).is_err());
    }
}
