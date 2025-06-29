pub mod command;
mod component;
mod entity;
mod schedule;
pub mod type_registry;

use bevy::{
    ecs::{component::ComponentId, entity::EntityHashMap},
    prelude::*,
};
use component::InspectorComponentInfo;
use entity::EntityMutation;
use schedule::ScheduleInfo;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use type_registry::ZeroSizedTypes;

pub struct RemoteInspectorPlugin;

impl Plugin for RemoteInspectorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DisabledComponents>()
            .init_resource::<EntityVisibilities>()
            .init_resource::<DeepCompareComponents>()
            .init_resource::<TrackedDatas>();
    }
}

#[derive(Default)]
pub struct TrackedData {
    pub type_registry: bool,
    pub components: HashSet<ComponentId>,
    pub entities: EntityHashMap<HashSet<ComponentId>>,
    pub schedules: bool,
}

#[derive(Resource, Default, Deref, DerefMut)]
pub struct TrackedDatas(HashMap<u32, TrackedData>); // Using u32 as a simple client ID

#[derive(Serialize)]
#[serde(rename_all(serialize = "snake_case"))]
#[serde(tag = "kind")]
pub enum InspectorEvent {
    TypeRegistry {
        types: Vec<Value>,
    },
    Component {
        components: Vec<InspectorComponentInfo>,
    },
    Entity {
        #[serde(serialize_with = "serialize_entity")]
        entity: Entity,
        mutation: EntityMutation,
    },
    Schedules {
        schedules: Vec<ScheduleInfo>,
    },
}

fn serialize_entity<S>(entity: &Entity, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_u64(entity.to_bits())
}

#[derive(Resource, Default)]
struct DisabledComponents(EntityHashMap<HashMap<ComponentId, Box<dyn PartialReflect>>>);

#[derive(Resource, Default)]
struct EntityVisibilities(EntityHashMap<Visibility>);

#[derive(Resource, Default)]
struct DeepCompareComponents {
    ids: HashSet<ComponentId>,
    values: HashMap<Entity, HashMap<ComponentId, Value>>,
}

impl DeepCompareComponents {
    /// Compare the component with the previous value and return None if the component should not be deep compared
    pub fn is_eq(
        &mut self,
        entity: Entity,
        component_id: ComponentId,
        new_value: &Value,
    ) -> Option<bool> {
        if !self.ids.contains(&component_id) {
            return None;
        }
        let entry = self.values.entry(entity).or_default();

        let old_value = entry.get(&component_id);
        if let Some(old_value) = old_value {
            if old_value == new_value {
                return Some(true);
            }
        }

        entry.insert(component_id, new_value.clone());

        Some(false)
    }
}

pub struct InspectorContext<'a> {
    disabled_components: &'a mut DisabledComponents,
    entity_visibilities: &'a mut EntityVisibilities,
    deep_compare_components: &'a mut DeepCompareComponents,
}

impl<'a> InspectorContext<'a> {
    pub fn run<T>(world: &mut World, f: impl FnOnce(&mut InspectorContext, &mut World) -> T) -> T {
        world.resource_scope(|world, mut disabled_components: Mut<DisabledComponents>| {
            world.resource_scope(|world, mut entity_visibilities: Mut<EntityVisibilities>| {
                world.resource_scope(
                    |world, mut deep_compare_components: Mut<DeepCompareComponents>| {
                        let mut ctx = InspectorContext {
                            disabled_components: &mut disabled_components,
                            entity_visibilities: &mut entity_visibilities,
                            deep_compare_components: &mut deep_compare_components,
                        };
                        f(&mut ctx, world)
                    },
                )
            })
        })
    }

    pub fn on_entity_removed(&mut self, entity: Entity) {
        self.disabled_components.0.remove(&entity);
        self.entity_visibilities.0.remove(&entity);
        self.deep_compare_components.values.remove(&entity);
    }
}

/// Get inspector events for streaming updates
pub fn get_inspector_events(world: &mut World, client_id: u32) -> Vec<InspectorEvent> {
    let mut events = Vec::new();
    let mut zsts = ZeroSizedTypes::default();

    world.resource_scope(|world, mut tracked_datas: Mut<TrackedDatas>| {
        InspectorContext::run(world, |ctx, world| {
            world.resource_scope(|world, type_registry: Mut<AppTypeRegistry>| {
                let type_registry = type_registry.read();
                let tracked = tracked_datas.entry(client_id).or_default();

                tracked.track_type_registry(&mut events, &mut zsts, &type_registry);
                tracked.track_components(&mut events, world, &type_registry);
                tracked.track_entities(&mut events, world, &type_registry, ctx, &zsts);
                tracked.track_schedules(&mut events, world, &type_registry);
            });
        });
    });

    events
}
