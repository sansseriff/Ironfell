//! Engine-owned outbound payloads. Each app has its own outbox, including when
//! systems execute on worker threads. Hosts drain it on the engine owner thread.
use bevy::prelude::Resource;
use serde::Serialize;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "ty", rename_all = "camelCase")]
pub enum EngineEvent {
    Hover { list: Vec<String> },
    Selection { list: Vec<String> },
    DocumentChanged { version: u32 },
    /// UI read invalidation, coalesced at the end of a frame.
    #[serde(rename_all = "camelCase")]
    StateChanged { version: u32, session_version: u64, resolved_version: u64, nodes: Vec<String>, authored: bool, structure: bool, compositions: bool, reset: bool, can_undo: bool, can_redo: bool },
    Cursor { cursor: String },
    Tool { tool: String },
    ConvertQuestion { question: serde_json::Value },
    Clipboard { text: String },
    /// Font files the engine wants, most needed first (plans/specs/text.md §4).
    Fonts { fonts: Vec<FontRequest> },
    /// Questions for Models (iron_document `query.rs`): the host carries
    /// each out and answers with the `query_result` command.
    Queries { requests: Vec<QueryRequest> },
    /// Text editing on the canvas (`bevy_app::text_tool`): the draft and
    /// its selection (UTF-16 offsets) for the host's hidden textarea, and
    /// the caret's top-left and height in window px, where the textarea
    /// goes so an IME's candidates appear by the text.
    TextEdit { active: bool, node: String, text: String, anchor: u32, focus: u32, caret: [f32; 3] },
    /// A link dropped from a main's port onto one of its parts
    /// (component-authoring.md §3): the host asks which property, and how
    /// the port's range maps onto it. `x`, `y` are CSS px in the window.
    LinkDrop { spec: String, node: String, x: f32, y: f32 },
    /// What a view offers, resolved from its focus context
    /// (plans/specs/contexts.md §2.7): its palette (`select rect|ellipse
    /// pen`, `|` sharing a slot) and keymap (`v:select r:rect`).
    Focus { panel: String, palette: String, keys: String },
    /// The renderer stopped: the GPU device was lost or wgpu reported an
    /// error (`render_loss`). `kind` is wgpu's error type (`DeviceLost`,
    /// `Validation`, `OutOfMemory`, `Internal`). The main world still runs.
    RenderLost { kind: String, error: String },
    /// A double-click on a node whose context says entering opens it
    /// (`affords.enter="open"`): the shell opens what it stands for. `node`
    /// is the clicked node, `name` its name (what kind of piece it is, as a
    /// timeline names its pieces: `band <key> …`), `stands_for` what it
    /// projects, if anything.
    Open { panel: String, node: String, name: String, stands_for: Option<String> },
}

#[derive(Debug, Clone, Serialize)]
pub struct FontRequest {
    pub family: String,
    /// Relative to the host's font root.
    pub file: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueryRequest {
    pub node: String,
    /// Names the request among the node's: the answer repeats it.
    pub seq: u64,
    /// Which Model: to the web host, a URL to POST `input` to.
    pub model: String,
    pub input: serde_json::Value,
}

#[derive(Resource, Default)]
pub struct HostEvents(Mutex<Vec<EngineEvent>>);

impl HostEvents {
    pub fn clipboard(&self,text:String) { self.push(EngineEvent::Clipboard { text }); }
    pub fn cursor(&self,cursor:String) { self.push(EngineEvent::Cursor { cursor }); }
    pub fn tool(&self,tool:String) { self.push(EngineEvent::Tool { tool }); }
    pub fn convert_question(&self,question:String) { self.push(EngineEvent::ConvertQuestion { question: serde_json::from_str(&question).expect("engine conversion question is JSON") }); }
    pub fn document_changed(&self,version:u32) { self.push(EngineEvent::DocumentChanged { version }); }
    pub fn push(&self, event: EngineEvent) { self.0.lock().expect("engine outbox").push(event); }
    pub fn drain(&self) -> Vec<EngineEvent> { std::mem::take(&mut *self.0.lock().expect("engine outbox")) }
}
