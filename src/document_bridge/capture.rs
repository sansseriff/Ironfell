//! Temporary content-only views and asynchronous GPU readback for agent vision.
use bevy::prelude::*;
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use base64::Engine;
use iron_document::{NodeId, composition::{self, Dims}};
use std::collections::BTreeMap;
use super::{DocumentStore, DocumentOrigin, SpatialScene, planes::PlaneLayer};
use crate::vector::{DisplayList, Offscreen, VectorLayer, VectorView, ViewTarget2d, order};

#[derive(Resource, Default)]
pub struct Captures {
    next: u64,
    pub results: BTreeMap<u64, serde_json::Value>,
}
#[derive(Component)]
pub struct CaptureView {
    ticket: u64,
    source: NodeId,
    origin: String,
    frames: u32,
    size: UVec2,
    stride: usize,
    reading: bool,
}

pub fn start(world: &mut World, source: &str) -> Result<u64, String> {
    let store = world.resource::<DocumentStore>();
    let doc = store.0.document();
    let id = if source.starts_with('#') { source.parse::<NodeId>().map_err(|e| e.to_string())? } else {
        doc.nodes_of_type(iron_document::TypeId::lookup("composition").unwrap())
            .find(|&id| doc.is_live(id) && doc.get::<iron_document::facet::Name>(id)
                .and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(source))
            .ok_or_else(|| format!("no composition named {source}"))?
    };
    if !doc.is_live(id) || composition::dims(doc, id) != Some(Dims::Two) {
        return Err("Picture capture currently supports 2D compositions only".into());
    }
    let origin = world.resource::<DocumentOrigin>().0.clone();
    let mut captures = world.resource_mut::<Captures>();
    if captures.results.len() >= 4 { return Err("too many pending pictures".into()); }
    captures.next += 1;
    let ticket = captures.next;
    captures.results.insert(ticket, serde_json::json!({"pending": true}));
    world.spawn(CaptureView { ticket, source: id, origin, frames: 0, size: UVec2::ONE, stride: 0, reading: false });
    world.resource_mut::<crate::ActivityControl>().remaining_frames = 10;
    Ok(ticket)
}

pub fn poll(world: &mut World, ticket: u64, cancel: bool) -> serde_json::Value {
    world.resource_mut::<crate::ActivityControl>().remaining_frames = 10;
    let result = world.resource::<Captures>().results.get(&ticket).cloned()
        .unwrap_or_else(|| serde_json::json!({"ok": false, "error": "picture ticket expired"}));
    if cancel || result.get("pending").is_none() {
        world.resource_mut::<Captures>().results.remove(&ticket);
        let entities: Vec<_> = world.query::<(Entity, &CaptureView)>().iter(world)
            .filter_map(|(e, v)| (v.ticket == ticket).then_some(e)).collect();
        for entity in entities { world.despawn(entity); }
    }
    result
}

pub fn advance(
    mut commands: Commands, spatial: Res<SpatialScene>, origin: Res<DocumentOrigin>,
    mut captures: ResMut<Captures>, mut active: ResMut<crate::ActivityControl>,
    mut views: Query<(Entity, &mut CaptureView, Option<&ViewTarget2d>)>,
) {
    for (entity, mut capture, target) in &mut views {
        active.remaining_frames = active.remaining_frames.max(10);
        capture.frames += 1;
        if capture.frames > 600 || capture.origin != origin.0 || !captures.results.contains_key(&capture.ticket) {
            if captures.results.contains_key(&capture.ticket) {
                captures.results.insert(capture.ticket, serde_json::json!({"ok": false, "error": "picture capture expired or document changed"}));
            }
            commands.entity(entity).despawn();
            continue;
        }
        if capture.frames == 1 {
            let bounds = super::nest::content_bounds(&spatial, capture.source)
                .unwrap_or(kurbo::Rect::new(0., 0., 100., 100.));
            let region = bounds.inflate(16., 16.);
            let density = (1568. / region.width().max(region.height())).min(1.);
            capture.size = UVec2::new((region.width() * density).ceil().max(1.) as u32, (region.height() * density).ceil().max(1.) as u32);
            commands.entity(entity).insert((DisplayList::default(), Offscreen,
                VectorView { pos: UVec2::ZERO, size: capture.size },
                VectorLayer::screen(order::DOCUMENT_2D).in_view(entity),
                PlaneLayer { embed: capture.source, source: capture.source, region, density }));
        }
        // Let the normal renderer prepare and draw this view before requesting its pixels.
        if capture.frames >= 4 && !capture.reading && let Some(target) = target {
            capture.stride = (target.bucket.x as usize * 4).div_ceil(256) * 256;
            capture.reading = true;
            commands.entity(entity).insert(Readback::texture(target.image.clone())).observe(captured);
        }
    }
}

fn captured(event: On<ReadbackComplete>, mut commands: Commands, views: Query<&CaptureView>, mut captures: ResMut<Captures>) {
    let Ok(view) = views.get(event.entity) else { return };
    if !captures.results.contains_key(&view.ticket) { return; }
    let result = encode(&event.data, view.size, view.stride)
        .map(|data| serde_json::json!({"ok": true, "image": {"name": format!("{} composition", view.source), "mediaType": "image/png", "data": data}}))
        .unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error}));
    captures.results.insert(view.ticket, result);
    commands.entity(event.entity).despawn();
}

fn encode(data: &[u8], size: UVec2, stride: usize) -> Result<String, String> {
    let width = size.x as usize * 4;
    if stride < width || data.len() < stride * size.y as usize { return Err("incomplete GPU picture".into()); }
    let mut pixels = Vec::with_capacity(width * size.y as usize);
    for row in data.chunks(stride).take(size.y as usize) {
        for p in row[..width].chunks_exact(4) {
            // Vello's texture is premultiplied; flatten onto neutral white.
            let white = 255 - p[3];
            pixels.extend_from_slice(&[p[0].saturating_add(white), p[1].saturating_add(white), p[2].saturating_add(white), 255]);
        }
    }
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, size.x, size.y);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&pixels).map_err(|e| e.to_string())?;
    }
    if bytes.len() > 2_100_000 { return Err("picture too large; reduce composition detail".into()); }
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}
