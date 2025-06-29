use bevy::{
    ecs::component::{ComponentId, ComponentInfo},
    prelude::{EntityRef, World},
    reflect::{serde::TypedReflectSerializer, ReflectFromPtr, TypeRegistry},
};
use serde::Serialize;
use serde_json::Value;

use crate::{InspectorEvent, TrackedData};

impl TrackedData {
    pub fn track_components(
        &mut self,
        events: &mut Vec<InspectorEvent>,
        world: &mut World,
        type_registry: &TypeRegistry,
    ) {
        let mut new_components = vec![];
        let components = world.components();
        for info in world.components().iter() {
            let Some(type_id) = info.type_id() else {
                continue;
            };

            let reflected = type_registry.get_type_info(type_id).is_some();

            if components.get_resource_id(type_id).is_some() {
                continue;
            }

            if !self.components.contains(&info.id()) {
                self.components.insert(info.id());
                let required_components = info
                    .required_components()
                    .iter_ids()
                    .map(|id| id.index())
                    .collect::<Vec<_>>();
                new_components.push(InspectorComponentInfo::new(
                    info,
                    reflected,
                    required_components,
                ));
            }
        }

        if !new_components.is_empty() {
            events.push(InspectorEvent::Component {
                components: new_components,
            });
        }
    }
}

#[derive(Serialize)]
pub struct InspectorComponentInfo {
    id: usize,
    name: String,
    reflected: bool,
    required_components: Vec<usize>,
}

impl InspectorComponentInfo {
    pub fn new(
        component_info: &ComponentInfo,
        reflected: bool,
        required_components: Vec<usize>,
    ) -> Self {
        Self {
            id: component_info.id().index(),
            name: component_info.name().into(),
            reflected,
            required_components,
        }
    }
}

pub fn serialize_component(
    component_id: ComponentId,
    entity_ref: &EntityRef,
    type_registry: &TypeRegistry,
    component_info: &ComponentInfo,
) -> Option<Value> {
    let component_ptr = entity_ref.get_by_id(component_id).ok()?;
    let type_id = component_info.type_id()?;

    let reflect_from_ptr = type_registry.get_type_data::<ReflectFromPtr>(type_id)?;

    assert_eq!(
        reflect_from_ptr.type_id(),
        type_id,
        "Mismatch between Ptr's type_id and ReflectFromPtr's type_id",
    );

    let reflect = unsafe { reflect_from_ptr.as_reflect(component_ptr) };

    let serializer = TypedReflectSerializer::new(reflect.as_partial_reflect(), &type_registry);

    let ret = serde_json::to_value(serializer).ok();

    ret
}
