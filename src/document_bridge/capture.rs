//! Temporary content-only views and asynchronous GPU readback for agent vision.
use bevy::prelude::*;
use bevy::render::gpu_readback::{ReadbackComplete, ReadbackOnce};
use base64::Engine;
use iron_document::{NodeId, composition::{self, Dims}};
use std::collections::BTreeMap;
use super::{DocumentStore, DocumentOrigin, SpatialScene, planes::PlaneLayer};
use crate::vector::{DisplayList, Offscreen, VectorLayer, VectorView, ViewTarget2d, order};

/// Captures in flight at once.
const MAX_PENDING: usize = 4;
/// A finished result nobody polled (the page reloaded, a script gave up) is
/// dropped after this long, so it never holds a slot or its bytes for good.
const RESULT_TTL_SECS: f64 = 30.;
/// Frames the content's bounds must hold still before the pixels are read:
/// long enough for the renderer to prepare and draw the view at that size.
const SETTLE_FRAMES: u32 = 3;
/// Padding around the content, in document px.
const PADDING: f64 = 16.;
const MAX_EDGE: f64 = 1568.;

#[derive(Resource, Default)]
pub struct Captures {
    next: u64,
    results: BTreeMap<u64, Entry>,
}
struct Entry {
    value: serde_json::Value,
    /// `Time` elapsed when the result was made (or the request, while pending).
    at: f64,
    origin: String,
}
impl Entry {
    fn pending(&self) -> bool { self.value.get("pending").is_some() }
}
impl Captures {
    fn finish(&mut self, ticket: u64, value: serde_json::Value, now: f64) {
        if let Some(entry) = self.results.get_mut(&ticket) { entry.value = value; entry.at = now; }
    }
    /// Drop finished results that were never polled in time, and anything
    /// from a document that is no longer open.
    fn prune(&mut self, now: f64, origin: &str) {
        // A pending capture ends itself within 600 frames; one far older lost its view.
        self.results.retain(|_, e| e.origin == origin && now - e.at <= if e.pending() { 4. * RESULT_TTL_SECS } else { RESULT_TTL_SECS });
    }
}

#[derive(Component)]
pub struct CaptureView {
    ticket: u64,
    source: NodeId,
    origin: String,
    frames: u32,
    /// Frames since `region` last changed.
    still: u32,
    region: kurbo::Rect,
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
    let now = world.resource::<Time>().elapsed_secs_f64();
    let mut captures = world.resource_mut::<Captures>();
    captures.prune(now, &origin);
    if captures.results.values().filter(|e| e.pending()).count() >= MAX_PENDING { return Err("too many pending pictures".into()); }
    captures.next += 1;
    let ticket = captures.next;
    captures.results.insert(ticket, Entry { value: serde_json::json!({"pending": true}), at: now, origin: origin.clone() });
    world.spawn(CaptureView { ticket, source: id, origin, frames: 0, still: 0, region: kurbo::Rect::ZERO, size: UVec2::ONE, stride: 0, reading: false });
    world.resource_mut::<crate::ActivityControl>().remaining_frames = 10;
    Ok(ticket)
}

pub fn poll(world: &mut World, ticket: u64, cancel: bool) -> serde_json::Value {
    world.resource_mut::<crate::ActivityControl>().remaining_frames = 10;
    let result = world.resource::<Captures>().results.get(&ticket).map(|e| e.value.clone())
        .unwrap_or_else(|| serde_json::json!({"ok": false, "error": "picture ticket expired"}));
    if cancel || result.get("pending").is_none() {
        world.resource_mut::<Captures>().results.remove(&ticket);
        let entities: Vec<_> = world.query::<(Entity, &CaptureView)>().iter(world)
            .filter_map(|(e, v)| (v.ticket == ticket).then_some(e)).collect();
        for entity in entities { world.despawn(entity); }
    }
    result
}

/// The capture region for `source` now: its content bounds, padded, and the
/// texture density that fits it in `MAX_EDGE`.
fn framing(spatial: &SpatialScene, source: NodeId) -> (kurbo::Rect, f64, UVec2) {
    let bounds = super::nest::content_bounds(spatial, source).unwrap_or(kurbo::Rect::new(0., 0., 100., 100.));
    let region = bounds.inflate(PADDING, PADDING);
    let density = (MAX_EDGE / region.width().max(region.height())).min(1.);
    let size = UVec2::new((region.width() * density).ceil().max(1.) as u32, (region.height() * density).ceil().max(1.) as u32);
    (region, density, size)
}

pub(crate) fn advance(
    mut commands: Commands, spatial: Res<SpatialScene>, origin: Res<DocumentOrigin>, store: Res<DocumentStore>, time: Res<Time>,
    mut captures: ResMut<Captures>, mut active: ResMut<crate::ActivityControl>,
    mut views: Query<(Entity, &mut CaptureView, Option<&ViewTarget2d>, Option<&mut PlaneLayer>, Option<&mut VectorView>)>,
) {
    let now = time.elapsed_secs_f64();
    for (entity, mut capture, target, layer, vview) in &mut views {
        active.remaining_frames = active.remaining_frames.max(10);
        capture.frames += 1;
        // The same scope can be reloaded with the origin unchanged: a deleted
        // composition would otherwise render as a blank white success.
        let gone = !store.0.document().is_live(capture.source);
        if capture.frames > 600 || gone || capture.origin != origin.0 || !captures.results.contains_key(&capture.ticket) {
            let error = if gone { "the composition no longer exists" } else { "picture capture expired or document changed" };
            captures.finish(capture.ticket, serde_json::json!({"ok": false, "error": error}), now);
            commands.entity(entity).despawn();
            continue;
        }
        if capture.reading { continue; }
        // Frame the content every frame until the pixels are read: text and
        // embeds that settle after the request change the bounds, and the
        // read waits until they have held still.
        let (region, density, size) = framing(&spatial, capture.source);
        if capture.frames == 1 {
            commands.entity(entity).insert((DisplayList::default(), Offscreen,
                VectorView { pos: UVec2::ZERO, size },
                VectorLayer::screen(order::DOCUMENT_2D).in_view(entity),
                PlaneLayer { embed: capture.source, source: capture.source, region, density }));
        } else if region != capture.region {
            if let Some(mut layer) = layer { layer.region = region; layer.density = density; }
            if let Some(mut vview) = vview { vview.size = size; }
        }
        if capture.frames == 1 || region != capture.region {
            capture.region = region;
            capture.size = size;
            capture.still = 0;
            continue;
        }
        capture.still += 1;
        if capture.still >= SETTLE_FRAMES && let Some(target) = target && target.bucket.x >= size.x && target.bucket.y >= size.y {
            capture.stride = (target.bucket.x as usize * 4).div_ceil(256) * 256;
            capture.reading = true;
            commands.entity(entity).insert(ReadbackOnce::texture(target.image.clone())).observe(captured);
        }
    }
}

fn captured(event: On<ReadbackComplete>, mut commands: Commands, views: Query<&CaptureView>, mut captures: ResMut<Captures>, time: Res<Time>) {
    let Ok(view) = views.get(event.entity) else { return };
    if !captures.results.contains_key(&view.ticket) { return; }
    let result = encode(&event.data, view.size, view.stride)
        .map(|data| serde_json::json!({"ok": true, "node": view.source.to_string(), "image": {"name": format!("{} composition", view.source), "mediaType": "image/png", "data": data}}))
        .unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error}));
    captures.finish(view.ticket, result, time.elapsed_secs_f64());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(b64: &str) -> (u32, u32, Vec<u8>) {
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        buf.truncate(info.buffer_size());
        (info.width, info.height, buf)
    }

    #[test]
    fn encode_skips_row_padding_and_flattens_premultiplied_pixels_onto_white() {
        // 2×2 in a texture whose rows are padded to 256 bytes (wgpu's copy
        // alignment) and wider than the picture (a bucketed target).
        let stride = 256;
        let mut data = vec![0xAB; stride * 3];
        let px = |data: &mut Vec<u8>, x: usize, y: usize, p: [u8; 4]| data[y * stride + x * 4..][..4].copy_from_slice(&p);
        px(&mut data, 0, 0, [0, 0, 0, 0]);         // transparent → white
        px(&mut data, 1, 0, [128, 0, 0, 128]);     // half red, premultiplied
        px(&mut data, 0, 1, [10, 20, 30, 255]);    // opaque, unchanged
        px(&mut data, 1, 1, [0, 0, 0, 255]);       // opaque black
        let (w, h, pixels) = decode(&encode(&data, UVec2::new(2, 2), stride).unwrap());
        assert_eq!((w, h), (2, 2));
        assert_eq!(pixels, [255, 255, 255, 255, 255, 127, 127, 255, 10, 20, 30, 255, 0, 0, 0, 255]);
    }

    #[test]
    fn encode_rejects_short_or_misaligned_readback() {
        assert!(encode(&[0; 8], UVec2::new(2, 2), 8).is_err());
        assert!(encode(&[0; 64], UVec2::new(4, 1), 8).is_err());
    }

    #[test]
    fn unpolled_results_expire_and_never_hold_a_slot() {
        let mut captures = Captures::default();
        for ticket in 1..=4 {
            captures.results.insert(ticket, Entry { value: serde_json::json!({"ok": true}), at: 0., origin: "a".into() });
        }
        captures.results.insert(5, Entry { value: serde_json::json!({"pending": true}), at: 0., origin: "a".into() });
        captures.results.insert(6, Entry { value: serde_json::json!({"ok": true}), at: 0., origin: "b".into() });
        captures.prune(RESULT_TTL_SECS + 1., "a");
        // Finished and stale, or from another document: gone. Pending: kept.
        assert_eq!(captures.results.keys().copied().collect::<Vec<_>>(), [5]);
    }
}
