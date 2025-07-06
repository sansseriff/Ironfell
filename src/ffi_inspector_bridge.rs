use crate::WorkerApp;
use bevy::prelude::*;
use bevy_remote_inspector::{
    InspectorContext, TrackedDatas,
    command::{
        DespawnEntity, Execute, InsertComponent, RemoveComponent, ReparentEntity, SpawnEntity,
        ToggleComponent, ToggleVisibity, UpdateComponent,
    },
    get_inspector_events,
};
use serde_json::Value;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// Send inspector updates from worker to main thread
    #[wasm_bindgen(js_namespace = rustBridge)]
    pub(crate) fn send_inspector_update_from_worker(update_json: &str);
}

/// Update a component on an entity
#[wasm_bindgen]
pub fn inspector_update_component(
    ptr: u64,
    entity_id: u64,
    component_id: usize,
    value_json: &str,
) -> bool {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };

    let entity = Entity::from_bits(entity_id);
    let value: Value = match serde_json::from_str(value_json) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let command = UpdateComponent {
        entity,
        component: component_id,
        value,
    };

    execute_inspector_command(app, |ctx, world| command.execute(ctx, world))
}

/// Toggle a component on an entity (add if missing, remove if present)
#[wasm_bindgen]
pub fn inspector_toggle_component(ptr: u64, entity_id: u64, component_id: usize) -> bool {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };

    let entity = Entity::from_bits(entity_id);
    let command = ToggleComponent {
        entity,
        component: component_id,
    };

    execute_inspector_command(app, |ctx, world| command.execute(ctx, world))
}

/// Remove a component from an entity
#[wasm_bindgen]
pub fn inspector_remove_component(ptr: u64, entity_id: u64, component_id: usize) -> bool {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };

    let entity = Entity::from_bits(entity_id);
    let command = RemoveComponent {
        entity,
        component: component_id,
    };

    execute_inspector_command(app, |ctx, world| command.execute(ctx, world))
}

/// Insert a component on an entity
#[wasm_bindgen]
pub fn inspector_insert_component(
    ptr: u64,
    entity_id: u64,
    component_id: usize,
    value_json: &str,
) -> bool {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };

    let entity = Entity::from_bits(entity_id);
    let value: Value = match serde_json::from_str(value_json) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let command = InsertComponent {
        entity,
        component: component_id,
        value,
    };

    execute_inspector_command(app, |ctx, world| command.execute(ctx, world))
}

/// Despawn an entity
#[wasm_bindgen]
pub fn inspector_despawn_entity(ptr: u64, entity_id: u64, kind: &str) -> bool {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };

    let entity = Entity::from_bits(entity_id);
    let despawn_kind = match kind {
        "recursive" => bevy_remote_inspector::command::DespawnEntityKind::Recursive,
        "descendant" => bevy_remote_inspector::command::DespawnEntityKind::Descendant,
        _ => return false,
    };

    let command = DespawnEntity {
        entity,
        kind: despawn_kind,
    };

    execute_inspector_command(app, |ctx, world| command.execute(ctx, world))
}

/// Toggle visibility of an entity
#[wasm_bindgen]
pub fn inspector_toggle_visibility(ptr: u64, entity_id: u64) -> bool {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };

    info!("type of entity_id: {}", entity_id);

    let entity = Entity::from_bits(entity_id);
    let command = ToggleVisibity { entity };

    execute_inspector_command(app, |ctx, world| command.execute(ctx, world))
}

/// Reparent an entity
#[wasm_bindgen]
pub fn inspector_reparent_entity(ptr: u64, entity_id: u64, parent_id: Option<u64>) -> bool {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };

    let entity = Entity::from_bits(entity_id);
    let parent = parent_id.map(Entity::from_bits);

    let command = ReparentEntity { entity, parent };

    execute_inspector_command(app, |ctx, world| command.execute(ctx, world))
}

/// Spawn a new entity
#[wasm_bindgen]
pub fn inspector_spawn_entity(ptr: u64, parent_id: Option<u64>) -> u64 {
    info!("Spawning entity with parent: {:?}", parent_id);
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };

    let parent = parent_id.map(Entity::from_bits);
    let command = SpawnEntity { parent };

    match execute_inspector_command_with_result(app, |ctx, world| command.execute(ctx, world)) {
        Some(entity_bits) => entity_bits,
        None => 0, // Return 0 for error/invalid entity
    }
}

/// Resource to track streaming state
#[derive(Resource)]
pub struct InspectorStreamingState {
    pub continuous_streaming_enabled: bool, // For animations/automatic updates
    pub last_update_tick: u32,
    pub update_every_n_ticks: u32, // Update frequency control for continuous streaming
}

impl Default for InspectorStreamingState {
    fn default() -> Self {
        Self {
            continuous_streaming_enabled: true, // Disabled by default for efficiency
            last_update_tick: 0,
            update_every_n_ticks: 3, // Update every 3 ticks when continuous streaming is enabled
        }
    }
}

/// Trigger inspector streaming immediately (called after commands)
fn trigger_inspector_streaming(world: &mut World) {
    let events = get_inspector_events(world, 0);
    if !events.is_empty() {
        match serde_json::to_string(&events) {
            Ok(json) => {
                send_inspector_update_from_worker(&json);
            }
            Err(e) => {
                error!("Failed to serialize inspector events: {}", e);
            }
        }
    }
}

/// System for continuous streaming (only when enabled, for animations)
/// this is added in bevy_app.rs
pub fn inspector_continuous_streaming_system(world: &mut World) {
    // Check if continuous streaming is enabled
    let streaming_enabled = {
        let state = world.get_resource::<InspectorStreamingState>();
        match state {
            Some(state) => state.continuous_streaming_enabled,
            None => {
                // Initialize the resource if it doesn't exist
                world.insert_resource(InspectorStreamingState::default());
                false
            }
        }
    };

    if !streaming_enabled {
        return;
    }

    // Frame limiting using an internal counter
    let should_update = {
        let mut state = world.get_resource_mut::<InspectorStreamingState>().unwrap();
        state.last_update_tick += 1;

        if state.last_update_tick >= state.update_every_n_ticks {
            state.last_update_tick = 0;
            true
        } else {
            false
        }
    };

    if !should_update {
        return;
    }
    // Use the same trigger function for consistency
    trigger_inspector_streaming(world);
}

/// Helper function to execute inspector commands
fn execute_inspector_command<F, T>(app: &mut WorkerApp, f: F) -> bool
where
    F: FnOnce(&mut InspectorContext, &mut World) -> anyhow::Result<T>,
{
    let result = InspectorContext::run(app.world_mut(), f);
    let success = result.is_ok();

    // Trigger immediate streaming update after successful command execution
    if success {
        trigger_inspector_streaming(app.world_mut());
    }

    success
}

/// Helper function to execute inspector commands that return a value
fn execute_inspector_command_with_result<F, T>(app: &mut WorkerApp, f: F) -> Option<T>
where
    F: FnOnce(&mut InspectorContext, &mut World) -> anyhow::Result<T>,
{
    let result = InspectorContext::run(app.world_mut(), f);

    // Trigger immediate streaming update after successful command execution
    if result.is_ok() {
        trigger_inspector_streaming(app.world_mut());
    }

    result.ok()
}

/// Enable continuous inspector streaming (for animations/automatic updates)
#[wasm_bindgen]
pub fn enable_inspector_streaming(ptr: u64) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    if let Some(mut state) = app
        .world_mut()
        .get_resource_mut::<InspectorStreamingState>()
    {
        state.continuous_streaming_enabled = true;
    }
}

/// Disable continuous inspector streaming
#[wasm_bindgen]
pub fn disable_inspector_streaming(ptr: u64) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    if let Some(mut state) = app
        .world_mut()
        .get_resource_mut::<InspectorStreamingState>()
    {
        state.continuous_streaming_enabled = false;
    }
}

/// Set continuous streaming frequency (ticks between updates for animations)
#[wasm_bindgen]
pub fn set_inspector_streaming_frequency(ptr: u64, ticks: u32) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    if let Some(mut state) = app
        .world_mut()
        .get_resource_mut::<InspectorStreamingState>()
    {
        state.update_every_n_ticks = ticks.max(1); // Ensure at least 1 tick
    }
}

/// Force an immediate inspector update (same as what happens after commands)
#[wasm_bindgen]
pub fn force_inspector_update(ptr: u64) {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };
    trigger_inspector_streaming(app.world_mut());
}

/// Get inspector streaming events for a specific client (deprecated - use callback streaming)
#[wasm_bindgen]
pub fn inspector_get_streaming_events(_ptr: u64, _client_id: u32) -> String {
    // This is now deprecated in favor of callback-based streaming
    // Return empty array to maintain compatibility
    "[]".to_string()
}

/// Reset streaming state for a client (useful when reconnecting)
#[wasm_bindgen]
pub fn inspector_reset_streaming_state(ptr: u64, client_id: u32) -> bool {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };

    // Remove the client's tracked data to reset state
    if let Some(mut tracked_datas) = app.world_mut().get_resource_mut::<TrackedDatas>() {
        tracked_datas.remove(&client_id);
        true
    } else {
        false
    }
}

/// Export the type registry schema for dynamic UI generation
#[wasm_bindgen]
pub fn get_type_registry_schema(ptr: u64) -> String {
    let app = unsafe { &mut *(ptr as *mut WorkerApp) };

    InspectorContext::run(app.world_mut(), |_ctx, world| {
        world.resource_scope(|_world, type_registry: Mut<AppTypeRegistry>| {
            let type_registry = type_registry.read();

            match bevy_remote_inspector::type_registry::export_type_registry(&type_registry) {
                Ok(schema) => match serde_json::to_string(&schema) {
                    Ok(json) => json,
                    Err(e) => {
                        error!("Failed to serialize type registry schema: {}", e);
                        "{}".to_string()
                    }
                },
                Err(e) => {
                    error!("Failed to export type registry: {}", e);
                    "{}".to_string()
                }
            }
        })
    })
}
