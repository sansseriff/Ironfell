use anyhow::{anyhow, bail};
use bevy::{
    ecs::component::ComponentId,
    prelude::*,
    ptr::OwningPtr,
    reflect::{serde::TypedReflectDeserializer, ReflectFromPtr},
};
use serde::{de::DeserializeSeed, Serialize};
use serde_json::Value;

use crate::InspectorContext;

pub trait Execute {
    type Output: Serialize;

    fn execute(self, ctx: &mut InspectorContext, world: &mut World)
        -> anyhow::Result<Self::Output>;
}

#[derive(Debug)]
pub enum Command {
    UpdateComponent(UpdateComponent),
    ToggleComponent(ToggleComponent),
    RemoveComponent(RemoveComponent),
    InsertComponent(InsertComponent),
    DespawnEntity(DespawnEntity),
    ToggleVisibity(ToggleVisibity),
    ReparentEntity(ReparentEntity),
    SpawnEntity(SpawnEntity),
}

impl Command {
    pub fn execute(self, ctx: &mut InspectorContext, world: &mut World) -> anyhow::Result<Value> {
        fn map_result<T: Serialize>(r: T) -> anyhow::Result<Value> {
            serde_json::to_value(r).map_err(|_| anyhow!("Error while serizalizing result"))
        }

        let result = match self {
            Command::UpdateComponent(command) => command.execute(ctx, world).and_then(map_result),
            Command::ToggleComponent(command) => command.execute(ctx, world).and_then(map_result),
            Command::RemoveComponent(command) => command.execute(ctx, world).and_then(map_result),
            Command::InsertComponent(command) => command.execute(ctx, world).and_then(map_result),
            Command::DespawnEntity(command) => command.execute(ctx, world).and_then(map_result),
            Command::ToggleVisibity(command) => command.execute(ctx, world).and_then(map_result),
            Command::ReparentEntity(command) => command.execute(ctx, world).and_then(map_result),
            Command::SpawnEntity(command) => command.execute(ctx, world).and_then(map_result),
        };
        result
    }
}

#[derive(Debug)]
pub struct UpdateComponent {
    pub entity: Entity,
    pub component: usize,
    pub value: Value,
}

impl Execute for UpdateComponent {
    type Output = ();

    fn execute(
        self,
        _ctx: &mut InspectorContext,
        world: &mut World,
    ) -> anyhow::Result<Self::Output> {
        world.resource_scope(|world, registry: Mut<AppTypeRegistry>| {
            let registry = registry.read();
            let component_id = ComponentId::new(self.component);
            let type_id = world
                .components()
                .get_info(component_id)
                .and_then(|info| info.type_id())
                .ok_or(anyhow!("Component not found"))?;
            let registration = registry
                .get(type_id)
                .ok_or(anyhow!("Component is not registered"))?;

            let deserializer = TypedReflectDeserializer::new(registration, &registry);
            let deserialized = deserializer.deserialize(self.value)?;
            let mut entity = world.get_entity_mut(self.entity)?;
            let mut component_ptr = entity.get_mut_by_id(component_id)?;

            let reflect_from_ptr = registry
                .get_type_data::<ReflectFromPtr>(type_id)
                .ok_or(anyhow!("Component does not implement ReflectFromPtr"))?;

            assert_eq!(
                reflect_from_ptr.type_id(),
                type_id,
                "Mismatch between Ptr's type_id and ReflectFromPtr's type_id",
            );

            let reflect_mut = unsafe { reflect_from_ptr.as_reflect_mut(component_ptr.as_mut()) };

            reflect_mut
                .as_reflect_mut()
                .try_apply(deserialized.as_ref())?;

            Ok(())
        })
    }
}

#[derive(Debug)]
pub struct ToggleComponent {
    pub entity: Entity,
    pub component: usize,
}

impl Execute for ToggleComponent {
    type Output = ();

    fn execute(
        self,
        ctx: &mut InspectorContext,
        world: &mut World,
    ) -> anyhow::Result<Self::Output> {
        let component_id = ComponentId::new(self.component);
        let type_id = world
            .components()
            .get_info(component_id)
            .map(|info| info.type_id())
            .ok_or_else(|| anyhow!("Component not found"))?
            .ok_or_else(|| anyhow!("Component is not a Rust type"))?;

        world.resource_scope(|world, registry: Mut<AppTypeRegistry>| {
            let registry = registry.read();
            let type_registration = registry
                .get(type_id)
                .ok_or_else(|| anyhow!("Component is not registered in TypeRegistry"))?;
            let reflect_component = type_registration.data::<ReflectComponent>();
            let reflect_from_ptr = type_registration.data::<ReflectFromPtr>();

            match (reflect_component, reflect_from_ptr) {
                (None, None) => bail!("Cannot get ReflectComponent or ReflectFromPtr"),
                _ => {}
            };

            let entity_disabled_components =
                ctx.disabled_components.0.entry(self.entity).or_default();
            let mut entity_mut = world.get_entity_mut(self.entity)?;

            if let Some(component_val) = entity_disabled_components.remove(&component_id) {
                // enable
                let component_exists = entity_mut.get_by_id(component_id).is_ok();
                if component_exists {
                    bail!("Can not enable component that already exists. Probably bug");
                }

                if let Some(reflect_component) = reflect_component {
                    reflect_component.insert(
                        &mut entity_mut,
                        component_val.as_partial_reflect(),
                        &registry,
                    );
                } else {
                    // It's really weird that it still work here because the component val is dynamic type not the concrete type
                    OwningPtr::make(component_val, |ptr| unsafe {
                        entity_mut.insert_by_id(component_id, ptr);
                    });
                }
            } else {
                // disable
                let component_val = if let Some(reflect_component) = reflect_component {
                    reflect_component
                        .reflect(entity_mut)
                        .ok_or_else(|| anyhow!("Component does not exits. Probaly bug"))?
                        .reflect_clone()
                        .map_err(|_| anyhow!("Failed to clone component"))?
                } else {
                    let component_val = entity_mut
                        .get_by_id(component_id)
                        .map_err(|_| anyhow!("Component does not exits. Probaly bug"))?;

                    unsafe { reflect_from_ptr.unwrap().as_reflect(component_val) }
                        .reflect_clone()
                        .map_err(|_| anyhow!("Failed to clone component"))?
                };

                entity_disabled_components
                    .insert(component_id, component_val.into_partial_reflect());
                let mut entity_mut = world.get_entity_mut(self.entity)?;
                entity_mut.remove_by_id(component_id);
            };

            Ok(())
        })
    }
}

#[derive(Debug)]
pub struct RemoveComponent {
    pub entity: Entity,
    pub component: usize,
}

impl Execute for RemoveComponent {
    type Output = ();

    fn execute(
        self,
        ctx: &mut InspectorContext,
        world: &mut World,
    ) -> anyhow::Result<Self::Output> {
        let component_id = ComponentId::new(self.component);

        let mut entity = world.get_entity_mut(self.entity)?;
        entity.remove_by_id(component_id);

        drop(entity);

        ctx.on_entity_removed(self.entity);

        Ok(())
    }
}

#[derive(Debug)]
pub struct InsertComponent {
    pub entity: Entity,
    pub component: usize,
    pub value: Value,
}

impl Execute for InsertComponent {
    type Output = ();

    fn execute(
        self,
        _ctx: &mut InspectorContext,
        world: &mut World,
    ) -> anyhow::Result<Self::Output> {
        let component_id = ComponentId::new(self.component);

        world.resource_scope(|world, registry: Mut<AppTypeRegistry>| {
            let registry = registry.read();
            let type_id = world
                .components()
                .get_info(component_id)
                .and_then(|info| info.type_id())
                .ok_or(anyhow!("Component not found"))?;
            let registration = registry
                .get(type_id)
                .ok_or(anyhow!("Component is not registered"))?;
            let reflect_component = registration.data::<ReflectComponent>();

            let deserializer = TypedReflectDeserializer::new(registration, &registry);
            let partial_reflect = deserializer.deserialize(&self.value)?;

            let mut entity = world.get_entity_mut(self.entity)?;
            if entity.get_by_id(component_id).is_ok() {
                bail!("Component already exists")
            }

            if let Some(reflect_component) = reflect_component {
                reflect_component.insert(&mut entity, partial_reflect.as_ref(), &registry);
                return Ok(());
            }

            let reflect = partial_reflect.try_into_reflect().map_err(|_| {
                anyhow!(
                    "Can not convert {:?} to Reflect for type {}",
                    self.value,
                    registration.type_info().type_path()
                )
            })?;

            OwningPtr::make(reflect, |ptr| unsafe {
                entity.insert_by_id(component_id, ptr);
            });

            Ok(())
        })
    }
}

#[derive(Debug)]
pub enum DespawnEntityKind {
    Recursive,
    Descendant,
}

#[derive(Debug)]
pub struct DespawnEntity {
    pub entity: Entity,
    pub kind: DespawnEntityKind,
}

impl Execute for DespawnEntity {
    type Output = ();

    fn execute(
        self,
        _ctx: &mut InspectorContext,
        world: &mut World,
    ) -> anyhow::Result<Self::Output> {
        let entity = world.get_entity_mut(self.entity)?;

        match self.kind {
            DespawnEntityKind::Recursive => {
                entity.despawn();
            }
            DespawnEntityKind::Descendant => {
                // For descendant, just despawn children (not available in current Bevy)
                // entity.despawn_descendants();
                entity.despawn();
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct ToggleVisibity {
    pub entity: Entity,
}

impl Execute for ToggleVisibity {
    type Output = ();

    fn execute(
        self,
        ctx: &mut InspectorContext,
        world: &mut World,
    ) -> anyhow::Result<Self::Output> {
        let mut entity = world.get_entity_mut(self.entity)?;
        if let Some(visibility) = ctx.entity_visibilities.0.remove(&self.entity) {
            if let Some(mut component) = entity.get_mut::<Visibility>() {
                *component = visibility
            }

            return Ok(());
        }

        let view_visibility = entity
            .get::<ViewVisibility>()
            .ok_or(anyhow!("Entity does not have ViewVisibility component"))?
            .clone();

        let mut visibility = entity
            .get_mut::<Visibility>()
            .ok_or(anyhow!("Entity does not have Visibility component"))?;

        ctx.entity_visibilities
            .0
            .insert(self.entity, visibility.clone());

        if view_visibility.get() {
            *visibility = Visibility::Hidden;
        } else {
            *visibility = Visibility::Visible;
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct ReparentEntity {
    pub entity: Entity,
    pub parent: Option<Entity>,
}

impl Execute for ReparentEntity {
    type Output = ();

    fn execute(
        self,
        _ctx: &mut InspectorContext,
        world: &mut World,
    ) -> anyhow::Result<Self::Output> {
        if let Some(parent) = self.parent {
            let parent_exists = world.get_entity(parent).is_ok();
            if !parent_exists {
                bail!("Parent entity {parent} does not exist");
            }

            if self.entity == parent {
                bail!("Can not set entity as parent of itself");
            }

            // TODO: Implement modern parent-child relationship
            // entity.insert(ChildOf { parent });
            let _entity = world.get_entity_mut(self.entity)?;
        } else {
            let _entity = world.get_entity_mut(self.entity)?;
            // TODO: Implement removing parent
            // entity.remove::<ChildOf>();
            let _entity = world.get_entity_mut(self.entity)?;
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct SpawnEntity {
    pub parent: Option<Entity>,
}

impl Execute for SpawnEntity {
    type Output = u64;

    fn execute(
        self,
        _ctx: &mut InspectorContext,
        world: &mut World,
    ) -> anyhow::Result<Self::Output> {
        let child = if let Some(parent) = self.parent {
            if world.get_entity(parent).is_err() {
                bail!("Parent entity does not exist");
            };

            let child = world.spawn_empty().id();
            let mut parent = world.entity_mut(parent);
            parent.add_child(child);

            child
        } else {
            world.spawn_empty().id()
        };

        Ok(child.to_bits())
    }
}

#[cfg(test)]
mod test {
    use std::sync::{Arc, RwLock};

    use crate::{DeepCompareComponents, DisabledComponents, EntityVisibilities};

    use super::*;
    use bevy::reflect::{TypeRegistry, TypeRegistryArc};

    #[derive(Component, Reflect, Default)]
    #[reflect(Component)]
    struct ComponentReflectComponent(usize);

    #[derive(Component, Reflect, Default, serde::Deserialize, serde::Serialize)]
    #[reflect(Deserialize)]
    struct ComponentReflectDeserialize(usize);

    #[derive(Component, Reflect, Default, serde::Deserialize, serde::Serialize)]
    #[reflect(Deserialize)]
    struct ComponentReflectBoth(usize);

    #[derive(Component, Reflect, Default)]
    struct ComponentReflectNothing(usize);

    fn create_world() -> World {
        let mut world = World::default();
        let mut type_registry = TypeRegistry::default();

        type_registry.register::<ComponentReflectComponent>();
        type_registry.register::<ComponentReflectDeserialize>();
        type_registry.register::<ComponentReflectBoth>();
        type_registry.register::<ComponentReflectNothing>();

        world.insert_resource(DisabledComponents::default());
        world.insert_resource(DeepCompareComponents::default());
        world.insert_resource(EntityVisibilities::default());
        world.insert_resource(AppTypeRegistry(TypeRegistryArc {
            internal: Arc::new(RwLock::new(type_registry)),
        }));

        world
    }

    #[test]
    fn test_toggle_component() {
        fn toggle_component<T: Component + Default>() {
            let mut world = create_world();
            let entity = world.spawn(T::default()).id();

            // disable
            InspectorContext::run(&mut world, |ctx, world| {
                let command = ToggleComponent {
                    entity,
                    component: world.register_component::<T>().index(),
                };
                let result = command.execute(ctx, world);

                assert!(result.is_ok());

                let entity = world.entity(entity);
                assert!(!entity.contains::<T>());
            });

            // enable
            InspectorContext::run(&mut world, |ctx, world| {
                let command = ToggleComponent {
                    entity,
                    component: world.register_component::<T>().index(),
                };
                let result = command.execute(ctx, world);
                assert!(result.is_ok());

                let entity = world.entity(entity);
                assert!(entity.contains::<T>());
            });
        }

        toggle_component::<ComponentReflectComponent>();
        toggle_component::<ComponentReflectDeserialize>();
        toggle_component::<ComponentReflectBoth>();
        toggle_component::<ComponentReflectNothing>();
    }

    #[test]
    fn test_insert_component() {
        fn insert_component<T: Component>() {
            let mut world = create_world();
            let entity = world.spawn_empty().id();

            InspectorContext::run(&mut world, |ctx, world| {
                let command = InsertComponent {
                    entity,
                    component: world.register_component::<T>().index(),
                    value: serde_json::json!(0),
                };
                let result = command.execute(ctx, world);
                assert!(result.is_ok());

                let entity = world.entity(entity);
                assert!(entity.contains::<T>());
            });
        }

        insert_component::<ComponentReflectComponent>();
        insert_component::<ComponentReflectDeserialize>();
        insert_component::<ComponentReflectBoth>();
        // insert_component::<ComponentReflectNothing>();
    }
}
