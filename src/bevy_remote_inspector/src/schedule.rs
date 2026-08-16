use bevy::{
    app::{FixedMainScheduleOrder, MainScheduleOrder},
    ecs::schedule::{
        InternedScheduleLabel, NodeId, ScheduleGraph, ScheduleLabel,
        graph::Direction as BevyDirection,
    },
    prelude::*,
    reflect::TypeRegistry,
};
use serde::Serialize;

use crate::{InspectorEvent, TrackedData};

pub struct SchedulesPlugin;

impl Plugin for SchedulesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UpdateSchedule>()
            .add_systems(PostUpdate, collect_update_schedule);
    }

    fn finish(&self, app: &mut App) {
        // `MainScheduleOrder` is not present in the world during run, so we have to clone it
        clone_main_schedule_order(&mut app.world_mut());
    }
}

#[derive(Resource, Default)]
struct UpdateSchedule {
    initialized: bool,
    info: ScheduleInfo,
}

#[derive(Resource)]
struct ClonedMainScheduleOrder {
    startup_labels: Vec<InternedScheduleLabel>,
    labels: Vec<InternedScheduleLabel>,
}

fn collect_update_schedule(mut update_schedule: ResMut<UpdateSchedule>, schedules: Res<Schedules>) {
    if update_schedule.initialized {
        return;
    }

    update_schedule.initialized = true;

    let schedule = schedules.get(Update);

    if let Some(sche) = schedule {
        update_schedule.info = ScheduleInfo::from_schedule(sche, ScheduleKind::Main);
    }
}

fn clone_main_schedule_order(world: &mut World) {
    let main_schedule_order = world.resource::<MainScheduleOrder>();
    let my_main_schedule_order = ClonedMainScheduleOrder {
        startup_labels: main_schedule_order.startup_labels.clone(),
        labels: main_schedule_order.labels.clone(),
    };

    world.insert_resource(my_main_schedule_order);
}

#[derive(Serialize, Clone)]
pub struct SystemInfo {
    id: String,
    name: String,
}

#[derive(Serialize, Clone)]
pub struct SetInfo {
    id: String,
    name: String,
}

#[derive(Serialize, Clone, Default)]
pub enum ScheduleKind {
    Startup,
    #[default]
    Main,
    FixedMain,
}

#[derive(Serialize, Clone, Default)]
pub struct ScheduleInfo {
    name: String,
    kind: ScheduleKind,
    systems: Vec<SystemInfo>,
    sets: Vec<SetInfo>,
    hierarchies: Vec<(String, Vec<String>, Vec<String>)>,
    dependencies: Vec<(String, String)>,
}

impl ScheduleInfo {
    pub fn from_schedule(schedule: &Schedule, kind: ScheduleKind) -> Self {
        // bevy 0.18: `Schedule::systems()` yields `SystemKey` rather than `NodeId`.
        let systems = schedule
            .systems()
            .unwrap()
            .map(|(key, sys)| SystemInfo {
                id: get_node_id(&NodeId::System(key)),
                name: sys.name().to_string(),
            })
            .collect();
        let g = schedule.graph();
        // bevy 0.18: system sets moved to a public `system_sets` container field,
        // keyed by `SystemSetKey`.
        let sets = g
            .system_sets
            .iter()
            .filter_map(|(key, set, _)| {
                if set.system_type().is_some() {
                    return None;
                }

                Some(SetInfo {
                    id: get_node_id(&NodeId::Set(key)),
                    name: format!("{:?}", set),
                })
            })
            .collect();

        // bevy 0.18: `Dag::cached_topsort()` became `get_toposort()`, which returns
        // `None` while the graph is dirty (i.e. before the schedule is built).
        let hierarchies = g
            .hierarchy()
            .get_toposort()
            .unwrap_or(&[])
            .iter()
            .filter_map(|n| {
                if is_system_type_set(g, n) {
                    return None;
                }

                let outgoing_neighbors = g
                    .hierarchy()
                    .graph()
                    .neighbors_directed(*n, BevyDirection::Outgoing)
                    .filter_map(|n| {
                        if is_system_type_set(g, &n) {
                            return None;
                        }

                        Some(get_node_id(&n))
                    })
                    .collect::<Vec<_>>();

                let incoming_neighbors = g
                    .hierarchy()
                    .graph()
                    .neighbors_directed(*n, BevyDirection::Incoming)
                    .filter_map(|n| {
                        if is_system_type_set(g, &n) {
                            return None;
                        }

                        Some(get_node_id(&n))
                    })
                    .collect::<Vec<_>>();

                Some((get_node_id(n), outgoing_neighbors, incoming_neighbors))
            })
            .collect();

        let dependencies = g
            .dependency()
            .graph()
            .all_edges()
            .map(|(a, b)| (get_node_id(&a), get_node_id(&b)))
            .collect();

        Self {
            name: format!("{:?}", schedule.label()),
            kind,
            systems,
            sets,
            hierarchies,
            dependencies,
        }
    }
}

impl TrackedData {
    pub fn track_schedules(
        &mut self,
        events: &mut Vec<InspectorEvent>,
        world: &mut World,
        _type_registry: &TypeRegistry,
    ) {
        let update_schedule = world.resource::<UpdateSchedule>();

        if !update_schedule.initialized || self.schedules {
            return;
        }

        self.schedules = true;

        let main_order = world.resource::<ClonedMainScheduleOrder>();
        let fixed_main_order = world.resource::<FixedMainScheduleOrder>();
        let schedules = world.resource::<Schedules>();
        let mut schedule_infos = Vec::new();

        for label in main_order.startup_labels.iter() {
            let Some(schedule) = schedules.get(*label) else {
                continue;
            };
            schedule_infos.push(ScheduleInfo::from_schedule(schedule, ScheduleKind::Startup));
        }

        for label in main_order.labels.iter() {
            // bevy 0.18 changed the `DynEq` signature; interned labels compare directly.
            if *label == RunFixedMainLoop.intern() {
                for schedule in fixed_main_order.labels.iter() {
                    let Some(schedule) = schedules.get(*schedule) else {
                        continue;
                    };
                    schedule_infos.push(ScheduleInfo::from_schedule(
                        schedule,
                        ScheduleKind::FixedMain,
                    ));
                }
            } else {
                let schedule = schedules.get(*label);
                if let Some(schedule) = schedule {
                    schedule_infos.push(ScheduleInfo::from_schedule(schedule, ScheduleKind::Main));
                } else if *label == Update.intern() {
                    schedule_infos.push(update_schedule.info.clone());
                }
            }
        }

        events.push(InspectorEvent::Schedules {
            schedules: schedule_infos,
        });
    }
}

/// bevy 0.18 removed `ScheduleGraph::get_set_at`; set lookup now goes through the
/// `system_sets` container and is keyed by `SystemSetKey`. Systems are never
/// system-type sets, so a non-set node is always `false`.
fn is_system_type_set(g: &ScheduleGraph, node: &NodeId) -> bool {
    node.as_set()
        .and_then(|key| g.system_sets.get(key))
        .is_some_and(|set| set.system_type().is_some())
}

// Dirty hack to get the node id
fn get_node_id(id: &NodeId) -> String {
    let s = format!("{:?}", id);

    // s.split(|c| c == '(' || c == ')')
    //     .nth(1)
    //     .unwrap_or_else(|| s.as_str())
    //     .to_string()

    return s;
}
