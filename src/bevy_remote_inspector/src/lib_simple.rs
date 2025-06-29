pub mod command;
mod component;
mod entity;
mod schedule;
mod type_registry;

use bevy::{
    ecs::{component::ComponentId, entity::EntityHashMap},
    prelude::*,
    utils::HashMap,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;

pub struct RemoteInspectorPlugin;

impl Plugin for RemoteInspectorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DisabledComponents>()
            .init_resource::<EntityVisibilities>()
            .init_resource::<DeepCompareComponents>();
    }
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
