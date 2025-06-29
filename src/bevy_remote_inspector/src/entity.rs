use bevy::{
    ecs::component::ComponentId,
    prelude::*,
    reflect::{serde::TypedReflectSerializer, TypeRegistry},
};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;

use crate::{
    component::serialize_component, type_registry::ZeroSizedTypes, InspectorContext,
    InspectorEvent, TrackedData,
};

#[derive(Serialize)]
#[serde(rename_all(serialize = "snake_case"))]
#[serde(tag = "kind")]
pub enum EntityMutation {
    Remove,
    Change {
        // Both onAdd and onChange
        changes: Vec<EntityMutationChange>,
        removes: Vec<(usize, bool)>,
    },
}

#[derive(Serialize)]
pub struct EntityMutationChange(
    usize,
    bool,
    #[serde(skip_serializing_if = "Option::is_none")] Option<Value>,
);

impl TrackedData {
    pub fn track_entities(
        &mut self,
        events: &mut Vec<InspectorEvent>,
        world: &mut World,
        type_registry: &TypeRegistry,
        ctx: &mut InspectorContext,
        zsts: &ZeroSizedTypes,
    ) {
        // Clean up disabled components for removed entities
        let invalid_entities: Vec<Entity> = ctx
            .disabled_components
            .0
            .keys()
            .copied()
            .filter(|k| world.get_entity(*k).is_err())
            .collect();
        
        for entity in invalid_entities {
            ctx.disabled_components.0.remove(&entity);
        }

        // Clean up tracked entities that were removed
        let removed_entities: Vec<(Entity, HashSet<ComponentId>)> = self
            .entities
            .iter()
            .filter(|(k, _)| world.get_entity(**k).is_err())
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        
        for (entity, _) in &removed_entities {
            self.entities.remove(entity);
        }

        events.reserve(removed_entities.len());

        for (removed_entity, _) in removed_entities {
            ctx.on_entity_removed(removed_entity);
            events.push(InspectorEvent::Entity {
                entity: removed_entity,
                mutation: EntityMutation::Remove,
            });
        }

        let this_run = world.change_tick();
        for entity_ref in world.iter_entities() {
            let id = entity_ref.id();
            let entity_disbled_components = ctx.disabled_components.0.get_mut(&entity_ref.id());
            if let Some(component_ids) = self.entities.get_mut(&id) {
                let mut changes: Vec<EntityMutationChange> = vec![];
                let archetype = entity_ref.archetype();
                
                // Find removed components and collect them
                let removed_component_ids: Vec<_> = component_ids
                    .iter()
                    .filter(|&id| {
                        archetype
                            .components()
                            .find(|component_id| component_id == id)
                            .is_none()
                    })
                    .map(|id| {
                        let is_disabled = entity_disbled_components
                            .as_ref()
                            .map(|disabled| disabled.contains_key(id))
                            .unwrap_or_default();

                        (id.index(), is_disabled)
                    })
                    .collect();
                
                // Remove the components from tracking
                for (component_index, _) in &removed_component_ids {
                    let component_id = ComponentId::new(*component_index);
                    component_ids.remove(&component_id);
                }

                for component_id in entity_ref.archetype().components() {
                    let Some(ticks) = entity_ref.get_change_ticks_by_id(component_id) else {
                        continue;
                    };

                    let Some(component_info) = world.components().get_info(component_id) else {
                        continue;
                    };

                    if !ticks.is_changed(world.last_change_tick(), this_run) {
                        continue;
                    }

                    let is_disabled = entity_disbled_components
                        .as_ref()
                        .map(|disabled| disabled.contains_key(&component_id))
                        .unwrap_or_default();

                    let is_tracked = component_ids.contains(&component_id);
                    if zsts.contains_key(&component_info.type_id().unwrap()) {
                        // ZST are only serialized when they are added to the entity
                        if !is_tracked {
                            component_ids.insert(component_id);
                            changes.push(EntityMutationChange(
                                component_id.index(),
                                is_disabled,
                                None,
                            ));
                        }
                    } else {
                        let serialized = serialize_component(
                            component_id,
                            &entity_ref,
                            &type_registry,
                            component_info,
                        );

                        if !is_tracked {
                            component_ids.insert(component_id);
                        }

                        // Only if the component is untracked or serializable
                        if !is_tracked || serialized.is_some() {
                            match serialized.as_ref() {
                                Some(serialized) => {
                                    if let Some(true) = ctx.deep_compare_components.is_eq(
                                        entity_ref.id(),
                                        component_id,
                                        serialized,
                                    ) {
                                        continue;
                                    }
                                }
                                _ => {}
                            }

                            changes.push(EntityMutationChange(
                                component_id.index(),
                                is_disabled,
                                serialized,
                            ));
                        }
                    }
                }
                if !changes.is_empty() || !removed_component_ids.is_empty() {
                    events.push(InspectorEvent::Entity {
                        entity: id,
                        mutation: EntityMutation::Change {
                            changes,
                            removes: removed_component_ids,
                        },
                    });
                }
            } else {
                // Untracked entity, serialize all component
                self.entities
                    .insert(id, entity_ref.archetype().components().collect());
                let disabled_componentsi = entity_disbled_components.map(|components| {
                    let iter = components.iter().map(|(component_id, value)| {
                        let serialized = {
                            let reflect: &dyn PartialReflect = value.as_partial_reflect();
                            let serializer = TypedReflectSerializer::new(reflect, &type_registry);

                            let ret = serde_json::to_value(serializer).ok();

                            ret
                        };
                        EntityMutationChange(component_id.index(), true, serialized)
                    });

                    return Box::new(iter) as Box<dyn Iterator<Item = EntityMutationChange>>;
                });

                let changes = entity_ref.archetype().components().map(|component_id| {
                    let component_info = world.components().get_info(component_id).unwrap();
                    let serialized = serialize_component(
                        component_id,
                        &entity_ref,
                        &type_registry,
                        component_info,
                    );

                    if let Some(serialized) = serialized.as_ref() {
                        ctx.deep_compare_components
                            .values
                            .entry(entity_ref.id())
                            .or_default()
                            .insert(component_id, serialized.clone());
                    }

                    EntityMutationChange(component_id.index(), false, serialized)
                });

                let changes = if let Some(disabled_components) = disabled_componentsi {
                    changes.chain(disabled_components).collect::<Vec<_>>()
                } else {
                    changes.collect()
                };

                events.push(InspectorEvent::Entity {
                    entity: id,
                    mutation: EntityMutation::Change {
                        changes,
                        removes: vec![],
                    },
                });
            }
        }
    }
}
