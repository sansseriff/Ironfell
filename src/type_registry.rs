// from: https://github.com/notmd/bevy_remote_inspector

use bevy::{
    prelude::*,
    reflect::{
        ArrayInfo, EnumInfo, ListInfo, MapInfo, OpaqueInfo, SetInfo, StructInfo, TupleInfo,
        TupleStructInfo, TypeInfo, TypeRegistry, VariantInfo, serde::TypedReflectSerializer,
    },
    utils::TypeIdMap,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{InspectorEvent, TrackedData};

/// Any type that is ZST or if has no reflected fields
#[derive(Default, Deref, DerefMut)]
pub struct ZeroSizedTypes(TypeIdMap<()>);

impl TrackedData {
    pub fn track_type_registry(
        &mut self,
        events: &mut Vec<InspectorEvent>,
        zsts: &mut ZeroSizedTypes,
        type_registry: &TypeRegistry,
    ) {
        if self.type_registry {
            return;
        }

        self.type_registry = true;
        let types = serialize_type_registry(&type_registry, zsts);
        events.push(InspectorEvent::TypeRegistry { types });
    }
}

fn serialize_type_registry(registry: &TypeRegistry, zsts: &mut ZeroSizedTypes) -> Vec<Value> {
    let types = registry
        .iter()
        .map(|registration| {
            let default_value: Option<Value> =
                registration.data::<ReflectDefault>().and_then(|d| {
                    let reflect = d.default();
                    let serializer =
                        TypedReflectSerializer::new(reflect.as_partial_reflect(), &registry);

                    serde_json::to_value(serializer).ok()
                });

            if registration.type_info().is::<Vec3>() {}

            let type_name = registration.type_info().type_path();
            let type_info = match registration.type_info() {
                TypeInfo::Struct(info) => {
                    if info.field_len() == 0 {
                        zsts.insert(info.type_id(), ());
                    }
                    RegistryItem::Struct(StructValue::new(info, default_value))
                }
                TypeInfo::TupleStruct(info) => {
                    if info.field_len() == 0 {
                        zsts.insert(info.type_id(), ());
                    }
                    RegistryItem::TupleStruct(TupleStructValue::new(info, default_value))
                }
                TypeInfo::Tuple(info) => RegistryItem::Tuple(TupleValue::new(info)),
                TypeInfo::List(info) => {
                    RegistryItem::Array(ArrayValue::from_list(info, default_value))
                }
                TypeInfo::Array(info) => {
                    RegistryItem::Array(ArrayValue::from_array(info, default_value))
                }
                TypeInfo::Map(info) => RegistryItem::Map(MapValue::new(info, default_value)),
                TypeInfo::Set(infi) => RegistryItem::Set(SetValue::new(infi, default_value)),
                TypeInfo::Enum(info) => RegistryItem::Enum(EnumValue::new(info, default_value)),
                TypeInfo::Opaque(info) => {
                    RegistryItem::Opaque(OpaqueValue::new(info, default_value))
                }
            };

            json!([type_name, type_info])
        })
        .collect::<Vec<_>>();

    types
}

#[derive(Serialize)]
#[serde(rename_all(serialize = "snake_case"))]
#[serde(tag = "kind")]
enum RegistryItem {
    Struct(StructValue),
    TupleStruct(TupleStructValue),
    Tuple(TupleValue),
    Array(ArrayValue),
    Map(MapValue),
    Set(SetValue),
    Enum(EnumValue),
    Opaque(OpaqueValue),
}

#[derive(Serialize)]
struct StructValue {
    fields: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default: Option<Value>,
    short_name: &'static str,
}

impl StructValue {
    fn new(info: &StructInfo, default_value: Option<Value>) -> Self {
        let fields = info
            .iter()
            .map(|field| {
                let field_name = field.name();
                let field_type = field.type_info().map(|info| info.type_path());
                json!({
                    "name": field_name,
                    "type": field_type,
                })
            })
            .collect::<Vec<_>>();

        Self {
            fields,
            default: default_value,
            short_name: info.ty().short_path(),
        }
    }
}

#[derive(Serialize)]
struct TupleStructValue {
    fields: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default: Option<Value>,
    short_name: &'static str,
}

impl TupleStructValue {
    fn new(info: &TupleStructInfo, default_value: Option<Value>) -> Self {
        let fields = info
            .iter()
            .map(|field| field.type_path().to_string())
            .collect::<Vec<_>>();

        Self {
            fields,
            default: default_value,
            short_name: info.ty().short_path(),
        }
    }
}

#[derive(Serialize)]
struct TupleValue {
    fields: Vec<String>,
    short_name: &'static str,
}

impl TupleValue {
    fn new(info: &TupleInfo) -> Self {
        let fields = info
            .iter()
            .map(|field| field.type_path().to_string())
            .collect::<Vec<_>>();

        Self {
            fields,
            short_name: info.ty().short_path(),
        }
    }
}

#[derive(Serialize)]
struct ArrayValue {
    item: &'static str,
    capacity: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default: Option<Value>,
    short_name: &'static str,
}

impl ArrayValue {
    fn from_array(info: &ArrayInfo, default_value: Option<Value>) -> Self {
        Self {
            item: info.item_ty().path(),
            capacity: Some(info.capacity()),
            default: default_value,
            short_name: info.ty().short_path(),
        }
    }

    fn from_list(info: &ListInfo, default_value: Option<Value>) -> Self {
        Self {
            item: info.item_ty().path(),
            capacity: None,
            default: default_value,
            short_name: info.ty().short_path(),
        }
    }
}

#[derive(Serialize)]
struct MapValue {
    key: &'static str,
    value: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    default: Option<Value>,
    short_name: &'static str,
}

impl MapValue {
    fn new(info: &MapInfo, default_value: Option<Value>) -> Self {
        Self {
            key: info.key_ty().path(),
            value: info.value_ty().path(),
            default: default_value,
            short_name: info.ty().short_path(),
        }
    }
}

#[derive(Serialize)]
struct SetValue {
    item: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    default: Option<Value>,
    short_name: &'static str,
}

impl SetValue {
    fn new(info: &SetInfo, default_value: Option<Value>) -> Self {
        Self {
            item: info.value_ty().path(),
            default: default_value,
            short_name: info.ty().short_path(),
        }
    }
}

#[derive(Serialize)]
struct EnumValue {
    variants: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default: Option<Value>,
    short_name: &'static str,
}

impl EnumValue {
    fn new(info: &EnumInfo, default_value: Option<Value>) -> Self {
        let variants = info
            .iter()
            .map(|variant| {
                let name = variant.name();
                let mut value = match variant {
                    VariantInfo::Struct(info) => {
                        let fields = info
                            .iter()
                            .map(|field| {
                                let field_name = field.name();
                                let field_type = field.type_info().map(|info| info.type_path());
                                json!({
                                    "name": field_name,
                                    "type": field_type,
                                })
                            })
                            .collect::<Vec<_>>();

                        json!({
                            "kind": "struct",
                            "fields": fields,
                        })
                    }
                    VariantInfo::Tuple(info) => {
                        let fields = info
                            .iter()
                            .map(|field| field.type_path())
                            .collect::<Vec<_>>();

                        json!({
                            "kind": "tuple",
                            "fields": fields
                        })
                    }
                    VariantInfo::Unit(_) => {
                        json!({
                            "kind": "unit",
                        })
                    }
                };

                value
                    .as_object_mut()
                    .unwrap()
                    .insert("name".to_string(), json!(name));

                value
            })
            .collect::<Vec<_>>();

        Self {
            variants,
            default: default_value,
            short_name: info.ty().short_path(),
        }
    }
}

#[derive(Serialize)]
struct OpaqueValue {
    #[serde(skip_serializing_if = "Option::is_none")]
    default: Option<Value>,
    short_name: &'static str,
}

impl OpaqueValue {
    fn new(info: &OpaqueInfo, default_value: Option<Value>) -> Self {
        Self {
            default: default_value,
            short_name: info.ty().short_path(),
        }
    }
}
