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
