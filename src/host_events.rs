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
    Cursor { cursor: String },
    Tool { tool: String },
    ConvertQuestion { question: String },
    Clipboard { text: String },
    /// Font files the engine wants, most needed first (plans/specs/text.md §4).
    Fonts { fonts: Vec<FontRequest> },
    /// Text editing on the canvas (`bevy_app::text_tool`): the draft and
    /// its selection (UTF-16 offsets) for the host's hidden textarea, and
    /// the caret's top-left and height in window px, where the textarea
    /// goes so an IME's candidates appear by the text.
    TextEdit { active: bool, node: String, text: String, anchor: u32, focus: u32, caret: [f32; 3] },
    /// A link dropped from a main's port onto one of its parts
    /// (component-authoring.md §3): the host asks which property, and how
    /// the port's range maps onto it. `x`, `y` are CSS px in the window.
    LinkDrop { spec: String, node: String, x: f32, y: f32 },
}

#[derive(Debug, Clone, Serialize)]
pub struct FontRequest {
    pub family: String,
    /// Relative to the host's font root.
    pub file: String,
}

#[derive(Resource, Default)]
pub struct HostEvents(Mutex<Vec<EngineEvent>>);

impl HostEvents {
    pub fn clipboard(&self,text:String) { self.push(EngineEvent::Clipboard { text }); }
    pub fn cursor(&self,cursor:String) { self.push(EngineEvent::Cursor { cursor }); }
    pub fn tool(&self,tool:String) { self.push(EngineEvent::Tool { tool }); }
    pub fn convert_question(&self,question:String) { self.push(EngineEvent::ConvertQuestion { question }); }
    pub fn document_changed(&self,version:u32) { self.push(EngineEvent::DocumentChanged { version }); }
    pub fn push(&self, event: EngineEvent) { self.0.lock().expect("engine outbox").push(event); }
    pub fn drain(&self) -> Vec<EngineEvent> { std::mem::take(&mut *self.0.lock().expect("engine outbox")) }
}
