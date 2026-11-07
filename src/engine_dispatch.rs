//! Transport-neutral document dispatch. Behavior stays in engine_api; this
//! allowlist only decodes the existing document-call protocol.
use crate::WorkerApp;
use serde::de::DeserializeOwned;
use serde_json::Value;
fn arity(args:&[Value],expected:usize)->Result<(),String>{
    if args.len()==expected {Ok(())} else {Err(format!("expected {expected} arguments, got {}",args.len()))}
}
fn argument<T:DeserializeOwned>(args:&[Value],index:usize)->Result<T,String>{
    serde_json::from_value(args.get(index).cloned().unwrap_or(Value::Null)).map_err(|e|format!("argument {index}: {e}"))
}
pub fn call(app:&mut WorkerApp,name:&str,args:&[Value])->Result<String,String>{
    Ok(match name {
        "load" => { arity(args,1)?; crate::engine_api::document_load(app,argument::<String>(args,0)?)?; String::new() },
        "save" => { arity(args,0)?; crate::engine_api::document_save(app) },
        "scope_open" => { arity(args,3)?; crate::engine_api::scope_open(app,argument::<String>(args,0)?,argument::<String>(args,1)?,argument::<Vec<String>>(args,2)?)? },
        "scope_assign" => { arity(args,1)?; crate::engine_api::scope_assign(app,argument::<String>(args,0)?)? },
        "scope_template" => { arity(args,1)?; crate::engine_api::scope_template(app,argument::<String>(args,0)?)? },
        "narrative_list" => { arity(args,0)?; crate::engine_api::narrative_list(app) },
        "narrative_read" => { arity(args,1)?; crate::engine_api::narrative_read(app,argument::<String>(args,0)?) },
        "narrative_edit" => { arity(args,4)?; crate::engine_api::narrative_edit(app,argument::<String>(args,0)?,argument::<String>(args,1)?,argument::<String>(args,2)?,argument::<String>(args,3)?) },
        "narrative_create" => { arity(args,3)?; crate::engine_api::narrative_create(app,argument::<String>(args,0)?,argument::<String>(args,1)?,argument::<String>(args,2)?) },
        "narrative_markdown" => { arity(args,1)?; crate::engine_api::narrative_markdown(app,argument::<String>(args,0)?) },
        "narrative_write" => { arity(args,4)?; crate::engine_api::narrative_write(app,argument::<String>(args,0)?,argument::<String>(args,1)?,argument::<String>(args,2)?,argument::<String>(args,3)?) },
        "narrative_rewrite" => { arity(args,3)?; crate::engine_api::narrative_rewrite(app,argument::<String>(args,0)?,argument::<String>(args,1)?,argument::<String>(args,2)?) },
        "timeline_axis" => { arity(args,1)?; crate::engine_api::timeline_axis(app,argument::<String>(args,0)?) },
        "timeline_rows" => { arity(args,1)?; crate::engine_api::timeline_rows(app,argument::<String>(args,0)?) },
        "timeline_key" => { arity(args,2)?; crate::engine_api::timeline_key(app,argument::<String>(args,0)?,argument::<String>(args,1)?) },
        "clock_read" => { arity(args,0)?; crate::engine_api::clock_read(app) },
        "clock_set" => { arity(args,1)?; crate::engine_api::clock_set(app,argument::<String>(args,0)?) },
        "narrative_probe" => { arity(args,2)?; crate::engine_api::narrative_probe(app,argument::<String>(args,0)?,argument::<String>(args,1)?) },
        "narrative_targets" => { arity(args,1)?; crate::engine_api::narrative_targets(app,argument::<String>(args,0)?) },
        "narrative_values" => { arity(args,1)?; crate::engine_api::narrative_values(app,argument::<String>(args,0)?) },
        "scope_log" => { arity(args,1)?; crate::engine_api::scope_log(app,argument::<u64>(args,0)?)? },
        "view" => { arity(args,1)?; crate::engine_api::document_view(app,argument::<String>(args,0)?)? },
        "apply" => { arity(args,2)?; crate::engine_api::document_apply(app,argument::<String>(args,0)?,argument::<u32>(args,1)?) },
        "build_info" => { arity(args,0)?; crate::engine_api::document_build_info(app) },
        "frame_stats" => { arity(args,0)?; crate::engine_api::document_frame_stats(app) },
        "stress" => { arity(args,1)?; crate::engine_api::document_stress(app,argument::<u32>(args,0)?) },
        "meters" => { arity(args,1)?; crate::engine_api::document_meters(app,argument::<u32>(args,0)?) },
        "snapping" => { arity(args,1)?; crate::engine_api::document_snapping(app,argument::<bool>(args,0)?) },
        "raster" => { arity(args,2)?; crate::engine_api::document_raster(app,argument::<bool>(args,0)?,argument::<u32>(args,1)?) },
        "spatial" => { arity(args,1)?; crate::engine_api::document_spatial(app,argument::<bool>(args,0)?) },
        "signals" => { arity(args,0)?; crate::engine_api::document_signals(app) },
        "producer" => { arity(args,2)?; crate::engine_api::document_producer(app,argument::<String>(args,0)?,argument::<String>(args,1)?) },
        "timings" => { arity(args,1)?; crate::engine_api::document_timings(app,argument::<bool>(args,0)?) },
        "make_component" => { arity(args,2)?; crate::engine_api::document_make_component(app,argument::<String>(args,0)?,argument::<String>(args,1)?) },
        "author" => { arity(args,2)?; crate::engine_api::document_author(app,argument::<String>(args,0)?,argument::<String>(args,1)?) },
        "detach" => { arity(args,1)?; crate::engine_api::document_detach(app,argument::<String>(args,0)?) },
        "table" => { arity(args,1)?; crate::engine_api::document_table(app,argument::<String>(args,0)?) },
        "context" => { arity(args,1)?; crate::engine_api::document_context(app,argument::<String>(args,0)?) },
        "computed" => { arity(args,1)?; crate::engine_api::document_computed(app,argument::<String>(args,0)?) },
        "view_context" => { arity(args,1)?; crate::engine_api::document_view_context(app,argument::<String>(args,0)?) },
        "demo" => { arity(args,0)?; crate::engine_api::document_demo(app) },
        "showcase" => { arity(args,0)?; crate::engine_api::document_showcase(app) },
        "contexts" => { arity(args,0)?; crate::engine_api::document_contexts(app) },
        "circuit" => { arity(args,1)?; crate::engine_api::document_circuit(app,argument::<String>(args,0)?) },
        "compose" => { arity(args,1)?; crate::engine_api::document_compose(app,argument::<String>(args,0)?) },
        "netlist" => { arity(args,1)?; crate::engine_api::document_netlist(app,argument::<String>(args,0)?) },
        "table_edit" => { arity(args,3)?; crate::engine_api::document_table_edit(app,argument::<String>(args,0)?,argument::<String>(args,1)?,argument::<String>(args,2)?) },
        "link" => { arity(args,4)?; crate::engine_api::document_link(app,argument::<String>(args,0)?,argument::<String>(args,1)?,argument::<String>(args,2)?,argument::<String>(args,3)?) },
        "layers" => { arity(args,2)?; crate::engine_api::document_layers(app,argument::<String>(args,0)?,argument::<String>(args,1)?) },
        "select" => { arity(args,2)?; crate::engine_api::document_select(app,argument::<String>(args,0)?,argument::<String>(args,1)?) },
        "counterparts" => { arity(args,1)?; crate::engine_api::document_counterparts(app,argument::<String>(args,0)?) },
        "to_path" => { arity(args,2)?; crate::engine_api::document_to_path(app,argument::<String>(args,0)?,argument::<bool>(args,1)?) },
        "export_svg" => { arity(args,1)?; crate::engine_api::document_export_svg(app,argument::<String>(args,0)?) },
        "import_svg" => { arity(args,1)?; crate::engine_api::document_import_svg(app,argument::<String>(args,0)?) },
        "import_svg_flat" => { arity(args,1)?; crate::engine_api::document_import_svg_flat(app,argument::<String>(args,0)?) },
        "import_svg_at" => { arity(args,5)?; crate::engine_api::document_import_svg_at(app,argument::<String>(args,0)?,argument::<String>(args,1)?,argument::<f64>(args,2)?,argument::<f64>(args,3)?,argument::<String>(args,4)?) },
        "focus" => { arity(args,1)?; crate::engine_api::document_focus(app,argument::<String>(args,0)?) },
        "selection" => { arity(args,0)?; crate::engine_api::document_selection(app) },
        "inspect" => { arity(args,1)?; crate::engine_api::document_inspect(app,argument::<String>(args,0)?) },
        "component" => { arity(args,3)?; crate::engine_api::document_component(app,argument::<String>(args,0)?,argument::<String>(args,1)?,argument::<bool>(args,2)?) },
        "inspect_patch" => { arity(args,2)?; crate::engine_api::document_inspect_patch(app,argument::<String>(args,0)?,argument::<String>(args,1)?) },
        "inspect_scrub" => { arity(args,6)?; crate::engine_api::document_inspect_scrub(app,argument::<String>(args,0)?,argument::<String>(args,1)?,argument::<String>(args,2)?,argument::<u32>(args,3)?,argument::<f64>(args,4)?,argument::<Option<String>>(args,5)?) },
        "inspect_set" => { arity(args,3)?; crate::engine_api::document_inspect_set(app,argument::<String>(args,0)?,argument::<String>(args,1)?,argument::<String>(args,2)?) },
        "compositions" => { arity(args,0)?; crate::engine_api::document_compositions(app) },
        "composition_info" => { arity(args,1)?; crate::engine_api::document_composition_info(app,argument::<String>(args,0)?) },
        "latency" => { arity(args,2)?; crate::engine_api::document_latency(app,argument::<String>(args,0)?,argument::<String>(args,1)?) },
        "render_inspector" => { arity(args,1)?; crate::engine_api::document_render_inspector(app,argument::<String>(args,0)?) },
        "render_inspector_options" => { arity(args,2)?; crate::engine_api::document_render_inspector_options(app,argument::<String>(args,0)?,argument::<String>(args,1)?) },
        "set_run_mode" => { arity(args,2)?; crate::engine_api::document_set_run_mode(app,argument::<String>(args,0)?,argument::<String>(args,1)?) },
        "next_ids" => { arity(args,1)?; crate::engine_api::document_next_ids(app,argument::<u32>(args,0)?) },
        "next_relation_ids" => { arity(args,1)?; crate::engine_api::document_next_relation_ids(app,argument::<u32>(args,0)?) },
        "place" => { arity(args,3)?; crate::engine_api::document_place(app,argument::<String>(args,0)?,argument::<String>(args,1)?,argument::<u32>(args,2)?)? },
        "schema" => { arity(args,0)?; crate::engine_api::document_schema() },
        "inspect_bind" => { arity(args,3)?; crate::engine_api::document_inspect_bind(app,argument::<String>(args,0)?,argument::<String>(args,1)?,argument::<String>(args,2)?) },
        "text_style" => { arity(args,2)?; crate::engine_api::document_text_style(app,argument::<String>(args,0)?,argument::<String>(args,1)?) },
        "fonts" => { arity(args,0)?; crate::engine_api::fonts() },
        "font_previews" => { arity(args,1)?; crate::engine_api::font_previews(argument::<Vec<String>>(args,0)?) },
        _=>return Err(format!("unknown document operation: {name}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::*;
    use serde_json::json;

    #[test]
    fn invalid_dispatch_is_explicit_and_does_not_touch_the_world() {
        let mut engine = App::new();
        let window = engine.world_mut().spawn_empty().id();
        let mut app = WorkerApp::new(engine, window);
        assert_eq!(call(&mut app, "missing", &[]).unwrap_err(), "unknown document operation: missing");
        assert_eq!(call(&mut app, "next_ids", &[]).unwrap_err(), "expected 1 arguments, got 0");
        assert!(call(&mut app, "next_ids", &[json!("wrong type")]).unwrap_err().starts_with("argument 0:"));
        assert!(call(&mut app, "apply", &[json!("{}"), json!(-1)]).unwrap_err().starts_with("argument 1:"));
        assert!(call(&mut app, "load", &[json!("not json")]).is_err());
        // No document store is installed: bad arguments must be rejected before
        // an operation can access it. A valid process-level read still works.
        let schema: Value = serde_json::from_str(&call(&mut app, "schema", &[]).unwrap()).unwrap();
        assert!(schema.is_object());
    }
}
