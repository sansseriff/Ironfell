use crate::{
    ActiveInfo,
    // bevy_app::{ActiveState, CurrentVolume},
    bevy_app::{ActiveState, CurrentVolume, MainCamera},
    send_pick_from_worker,
};
use bevy::math::bounding::RayCast3d;
use bevy::platform_support::collections::HashMap;
use bevy::{input::mouse::MouseWheel, prelude::*};
use wasm_bindgen::JsValue;

/// 基于 ray pick 的 hover / 选中 / 拖动
pub(crate) struct RayPickPlugin;

impl Plugin for RayPickPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (mouse_events_system, update_active));
    }
}

fn mouse_events_system(
    mut cursor_moved_events: EventReader<CursorMoved>,
    mut mouse_wheel_events: EventReader<MouseWheel>,
    mut app_info: ResMut<ActiveInfo>,
    cameras: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    // cameras: Query<(&Camera, &GlobalTransform)>,
    mut query: Query<(Entity, &CurrentVolume, &mut Transform), With<ActiveState>>,
) {
    // For dragging, using only the last move event is sufficient to obtain the final movement offset.

    if app_info.drag != Entity::PLACEHOLDER && !cursor_moved_events.is_empty() {
        let last_cursor_event: Option<&CursorMoved> = cursor_moved_events.read().last();
        if let Some(last_move) = last_cursor_event {
            let (camera, global_transform) = cameras.single().unwrap();

            for (entity, _, mut transform) in query.iter_mut() {
                if app_info.drag == entity {
                    let cur =
                        screen_to_world(last_move.position, camera, global_transform).unwrap();
                    let last =
                        screen_to_world(app_info.last_drag_pos, camera, global_transform).unwrap();
                    let offset = cur - last;
                    transform.translation += Vec3::new(offset.x, offset.y, 0.0);

                    app_info.last_drag_pos = last_move.position;
                }
            }
        }
        return;
    }

    // hover 列表
    // The frequency of mouse events is usually higher than that of
    // rendering, so a HashMap is used to avoid duplicate pick results.
    let mut list: HashMap<Entity, u64> = HashMap::default();

    for event in cursor_moved_events.read() {
        let (camera, transform) = cameras.single().unwrap();
        let ray = ray_from_screenspace(event.position, camera, transform).unwrap();
        let ray_cast = RayCast3d::from_ray(ray, 30.);
        // info!("ray_cast");

        // perform ray picking
        for (entity, volume, _) in query.iter_mut() {
            // Perform ray intersection with the AABB volume
            let toi = ray_cast.aabb_intersection_at(volume);

            // Intentionally do not set the hover state here.
            // Instead, collect all entities hit by the ray and send them to the main thread.
            // The main thread will decide which entity should be hovered,
            // and then send that info back to the appropriate entity.
            // status.hover = toi.is_some();
            if toi.is_some() {
                list.insert(entity, entity.to_bits());
            }
        }
    }

    if !list.is_empty() {
        // 通知 js pick 结果
        let js_array = js_sys::Array::new();
        for (_, &item) in list.iter() {
            js_array.push(&JsValue::from(item));
        }
        // if app_info.is_in_worker {
        send_pick_from_worker(js_array);
        // } else {
        //     send_pick_from_rust(js_array);
        // }
    }

    // TODO: mouse wheel
    for _event in mouse_wheel_events.read() {}
}

/// 更新 选中/高亮
fn update_active(active_info: ResMut<ActiveInfo>, mut query: Query<(Entity, &mut ActiveState)>) {
    for (entity, mut status) in query.iter_mut() {
        status.hover = active_info.hover.contains_key(&entity);
        status.selected = active_info.selection.contains_key(&entity)
    }
}

// Construct a camera ray
fn ray_from_screenspace(
    cursor_pos_screen: Vec2,
    camera: &Camera,
    camera_transform: &GlobalTransform,
) -> Option<Ray3d> {
    let mut viewport_pos = cursor_pos_screen;
    if let Some(viewport) = &camera.viewport {
        // this does not run most (all?) of the time. Viewport is None
        viewport_pos -= viewport.physical_position.as_vec2();
    }
    camera
        .viewport_to_world(camera_transform, viewport_pos)
        .map(Ray3d::from)
        .ok()
}

fn screen_to_world(
    pixel_pos: Vec2,
    camera: &Camera,
    camera_transform: &GlobalTransform,
) -> Option<Vec3> {
    // info!("pixel pos in raypick: {:?}", pixel_pos);
    let ray = ray_from_screenspace(pixel_pos, camera, camera_transform);
    if let Some(ray) = ray {
        // Intersections between the ray and all planes of the object
        let d = ray.intersect_plane(Vec3::new(0., 0., 2.), InfinitePlane3d::new(Vec3::Z));
        if let Some(d) = d {
            return Some(ray.origin + ray.direction * d);
        }
    }
    None
}
