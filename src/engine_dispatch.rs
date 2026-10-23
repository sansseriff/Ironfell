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
        "save" => { arity(args,0)?; crate::engine_api::document_save(app) },
        "view" => { arity(args,1)?; crate::engine_api::document_view(app,argument::<String>(args,0)?)? },
        "apply" => { arity(args,2)?; crate::engine_api::document_apply(app,argument::<String>(args,0)?,argument::<u32>(args,1)?) },
        "build_info" => { arity(args,0)?; crate::engine_api::document_build_info(app) },
        "stress" => { arity(args,1)?; crate::engine_api::document_stress(app,argument::<u32>(args,0)?) },
        "snapping" => { arity(args,1)?; crate::engine_api::document_snapping(app,argument::<bool>(args,0)?) },
        "spatial" => { arity(args,1)?; crate::engine_api::document_spatial(app,argument::<bool>(args,0)?) },
        "signals" => { arity(args,0)?; crate::engine_api::document_signals(app) },
        "producer" => { arity(args,2)?; crate::engine_api::document_producer(app,argument::<String>(args,0)?,argument::<String>(args,1)?) },
        "timings" => { arity(args,1)?; crate::engine_api::document_timings(app,argument::<bool>(args,0)?) },
        "make_component" => { arity(args,2)?; crate::engine_api::document_make_component(app,argument::<String>(args,0)?,argument::<String>(args,1)?) },
        "author" => { arity(args,2)?; crate::engine_api::document_author(app,argument::<String>(args,0)?,argument::<String>(args,1)?) },
        "detach" => { arity(args,1)?; crate::engine_api::document_detach(app,argument::<String>(args,0)?) },
        "link" => { arity(args,4)?; crate::engine_api::document_link(app,argument::<String>(args,0)?,argument::<String>(args,1)?,argument::<String>(args,2)?,argument::<String>(args,3)?) },
        "counterparts" => { arity(args,1)?; crate::engine_api::document_counterparts(app,argument::<String>(args,0)?) },
        "to_path" => { arity(args,2)?; crate::engine_api::document_to_path(app,argument::<String>(args,0)?,argument::<bool>(args,1)?) },
        "export_svg" => { arity(args,1)?; crate::engine_api::document_export_svg(app,argument::<String>(args,0)?) },
        "import_svg" => { arity(args,1)?; crate::engine_api::document_import_svg(app,argument::<String>(args,0)?) },
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
