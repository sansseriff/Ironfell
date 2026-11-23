//! Every integration test of the store in one test binary: one link after a
//! change, not one per file (the files linked separately took ~50 s). Each
//! file is a module, so a filter names it: `cargo test -p iron_document
//! --test suite contexts::`. Two kinds of test keep a binary of their own,
//! because they change state the whole process shares: `fonts` (the tests that
//! register real fonts, which would change every other test's text measure
//! partway through a run) and `instance_cost` (a counting allocator).

mod analysis;
mod anchors;
mod appearance;
mod atomicity;
mod authoring;
mod cells_demo;
mod clients;
mod clock;
mod compose;
mod compositions;
mod constraints;
mod contexts;
mod copies;
mod copies_apart;
mod definitions;
mod differential_harness;
mod differential_random;
mod easing;
mod edit;
mod embeds;
mod frame_constraints;
mod free_running;
mod gallery;
mod gestures;
mod handles;
mod id_growth;
mod id_order;
mod latency;
mod layers;
mod machine_authoring;
mod machines;
mod narrative;
mod owners;
mod packed_paths;
mod patches;
mod plot;
mod plot_perf;
mod ports;
mod reactive;
mod reactivity;
mod rederive;
mod registry_options;
mod repeat;
mod reply;
mod restructure;
mod rollback;
mod schematic;
mod scopes;
mod series;
mod session;
mod shapes;
mod signals;
mod sized;
mod slider;
mod solve_cost;
mod spine;
mod standard;
mod time;
mod timeline_view;
mod templates;
mod tokens;
mod types;
mod values;
mod virtual_views;
