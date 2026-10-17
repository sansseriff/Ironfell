//! A Bevy-side producer of runtime signals (vision 10 §4, phase 4): gas in
//! the piston demo's chamber.
//!
//! The particles are Bevy entities with no document nodes. Each frame the
//! chamber's and the piston's poses are read from the store (Iron owns
//! every world transform), the particles bounce inside, and the momentum
//! the piston face receives becomes a pressure, published onto the
//! document's `pressure` parameter as a runtime signal. Bindings read it
//! like any leaf (the gauge bars); nothing enters the history until a
//! freeze (`engine_api::document_producer`) commits the value.
//!
//! The producer runs only while something shows the chamber's composition,
//! so a document with the demo in a hidden tab costs nothing per frame.

use super::{CompositionLayers, DocumentStore, EmbedViews, MeshAssets, PanelViews};
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use iron_document::component::Name as DocName;
use iron_document::composition::{self, Dims};
use iron_document::{Leaf, NodeId, ProducerId, Store, TypeId, Value, Version};
use std::collections::BTreeMap;

/// The producer's registered name.
pub const PRODUCER: &str = "gas";
const PARTICLES: usize = 160;
/// Particle speed, world units per second: the gas's temperature. Elastic
/// walls keep it, so the gas is isothermal and `P·V` holds.
const SPEED: f32 = 1.8;
/// The gauge's time constant, seconds.
const SMOOTHING: f32 = 0.4;
/// Particle radius, world units; also the wall margin.
const RADIUS: f32 = 0.035;
/// The largest step one frame integrates: a slow frame runs the gas slower
/// rather than coarser.
const MAX_STEP: f32 = 0.1;

/// The nodes the producer reads and drives, found by name.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Targets {
    composition: NodeId,
    chamber: NodeId,
    piston: NodeId,
    /// The `param_spec` whose default the pressure is published onto.
    gauge: NodeId,
}

/// The chamber's gas region in the chamber frame: `a` along the axis from
/// the closed end, `(b, c)` radial.
#[derive(Clone, Copy, Debug)]
struct Chamber {
    centre: Vec3,
    u: Vec3,
    v: Vec3,
    w: Vec3,
    /// Reachable radius and axial extent for a particle centre.
    radius: f32,
    a_min: f32,
    a_max: f32,
    /// The whole chamber's length, for normalising the pressure.
    length: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Particle {
    p: Vec3,
    v: Vec3,
}

#[derive(Resource)]
pub struct GasSim {
    /// Whether the producer steps and publishes; a freeze stops it.
    pub running: bool,
    /// Frames published so far.
    pub frames: u64,
    /// The smoothed relative pressure last published.
    pub pressure: f32,
    found: Option<(Version, Option<Targets>)>,
    particles: Vec<Particle>,
    entities: Vec<Entity>,
    impulse: f32,
    seed: u32,
}

impl Default for GasSim {
    fn default() -> Self {
        GasSim { running: true, frames: 0, pressure: 0.0, found: None, particles: Vec::new(), entities: Vec::new(), impulse: 0.0, seed: 0x9E37_79B9 }
    }
}

impl GasSim {
    fn rnd(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed as f32) / (u32::MAX as f32)
    }
}

#[derive(Component)]
pub struct GasParticle;

fn name_of(store: &Store, id: NodeId) -> Option<String> {
    store.document().get::<DocName>(id).and_then(|n| n.text.constant()).and_then(|v| v.as_str()).map(str::to_owned)
}

/// The piston demo's nodes: a 3D composition named `cylinder` holding
/// meshes `chamber` and `piston`, and a `param_spec` named `pressure`.
fn find(store: &Store) -> Option<Targets> {
    let doc = store.document();
    let composition = doc.nodes_of_type(TypeId::lookup("composition")?).find(|&c| name_of(store, c).as_deref() == Some("cylinder") && composition::dims(doc, c) == Some(Dims::Three))?;
    let child = |name: &str| doc.children(composition).iter().copied().find(|&k| name_of(store, k).as_deref() == Some(name));
    let gauge = doc.nodes_of_type(TypeId::lookup("param_spec")?).find(|&p| name_of(store, p).as_deref() == Some("pressure"))?;
    Some(Targets { composition, chamber: child("chamber")?, piston: child("piston")?, gauge })
}

/// The gas region from the chamber's and the piston's world poses. The
/// chamber is a unit cylinder along its local Y, so its scale gives the
/// radius and length; the piston is another, whose near face closes the
/// region.
fn chamber(store: &Store, t: &Targets) -> Option<Chamber> {
    let ch = store.world_affine3(t.chamber);
    let pi = store.world_affine3(t.piston);
    let col = |m: &iron_document::Affine3, i: usize| Vec3::new(m.m[i][0] as f32, m.m[i][1] as f32, m.m[i][2] as f32);
    let axis = col(&ch, 1);
    let length = axis.length();
    let radius = 0.5 * col(&ch, 0).length();
    if length < 1e-3 || radius < 1e-3 {
        return None;
    }
    let u = axis / length;
    let v = col(&ch, 0).normalize_or_zero();
    let w = u.cross(v).normalize_or_zero();
    let centre = Vec3::new(ch.t[0] as f32, ch.t[1] as f32, ch.t[2] as f32);
    let piston_centre = Vec3::new(pi.t[0] as f32, pi.t[1] as f32, pi.t[2] as f32);
    let face = (piston_centre - centre).dot(u) - 0.5 * col(&pi, 1).length();
    let a_min = -0.5 * length + RADIUS;
    let a_max = (face - RADIUS).max(a_min + RADIUS);
    Some(Chamber { centre, u, v, w, radius: (radius - RADIUS).max(RADIUS), a_min, a_max, length })
}

/// Step the gas and publish the pressure. Runs before `sync_document`, so
/// the published value reaches this frame's reconcile like any change.
pub(super) fn step_and_publish(
    mut sim: ResMut<GasSim>,
    mut store: ResMut<DocumentStore>,
    views: Res<PanelViews>,
    embeds: Res<EmbedViews>,
    time: Res<Time>,
    assets: Option<Res<MeshAssets>>,
    mut layers: ResMut<CompositionLayers>,
    mut commands: Commands,
    mut transforms: Query<&mut Transform, With<GasParticle>>,
) {
    let version = store.0.document().version();
    if sim.found.as_ref().map(|(v, _)| *v) != Some(version) {
        sim.found = Some((version, find(&store.0)));
    }
    let Some((_, Some(targets))) = sim.found else {
        if !sim.entities.is_empty() {
            for e in sim.entities.drain(..) {
                commands.entity(e).try_despawn();
            }
            sim.particles.clear();
        }
        return;
    };
    let shown = views.0.iter().any(|v| v.composition == Some(targets.composition)) || embeds.0.iter().any(|e| e.source == targets.composition);
    if !shown || !sim.running {
        return;
    }
    let Some(ch) = chamber(&store.0, &targets) else { return };
    let Some(assets) = assets else { return };
    let _timing = crate::perf::span(crate::perf::Phase::Produce);

    // Spawn on first use: uniformly in the region, isotropic velocities.
    if sim.particles.is_empty() {
        let layer = layers.of(Some(targets.composition));
        for _ in 0..PARTICLES {
            let (r, th) = (ch.radius * sim.rnd().sqrt(), sim.rnd() * std::f32::consts::TAU);
            let a = ch.a_min + sim.rnd() * (ch.a_max - ch.a_min);
            let (z, phi) = (sim.rnd() * 2.0 - 1.0, sim.rnd() * std::f32::consts::TAU);
            let s = (1.0 - z * z).sqrt();
            let p = Particle { p: Vec3::new(a, r * th.cos(), r * th.sin()), v: Vec3::new(z, s * phi.cos(), s * phi.sin()) * SPEED };
            sim.particles.push(p);
            let e = commands
                .spawn((
                    GasParticle,
                    Name::new("gas particle"),
                    Mesh3d(assets.sphere.clone()),
                    MeshMaterial3d(assets.gas_material.clone()),
                    Transform::from_translation(ch.centre + ch.u * p.p.x + ch.v * p.p.y + ch.w * p.p.z).with_scale(Vec3::splat(2.0 * RADIUS)),
                    RenderLayers::layer(layer),
                ))
                .id();
            sim.entities.push(e);
        }
    }

    let dt = time.delta_secs().min(MAX_STEP);
    let mut impulse = 0.0;
    for i in 0..sim.particles.len() {
        let mut q = sim.particles[i];
        q.p += q.v * dt;
        // The wall: reflect the outward radial velocity.
        let r = q.p.yz().length();
        if r > ch.radius {
            let n = q.p.yz() / r;
            let vn = q.v.yz().dot(n);
            if vn > 0.0 {
                q.v.y -= 2.0 * vn * n.x;
                q.v.z -= 2.0 * vn * n.y;
            }
            q.p.y = n.x * ch.radius;
            q.p.z = n.y * ch.radius;
        }
        // The closed end and the piston face. A face that moved into the
        // gas pushes the particle ahead of it.
        if q.p.x < ch.a_min {
            q.p.x = ch.a_min;
            q.v.x = q.v.x.abs();
        }
        if q.p.x > ch.a_max {
            q.p.x = ch.a_max;
            if q.v.x > 0.0 {
                impulse += 2.0 * q.v.x;
            }
            q.v.x = -q.v.x.abs();
        }
        sim.particles[i] = q;
        if let Ok(mut t) = transforms.get_mut(sim.entities[i]) {
            t.translation = ch.centre + ch.u * q.p.x + ch.v * q.p.y + ch.w * q.p.z;
        }
    }
    sim.impulse += impulse;

    // Pressure: momentum per time per area, smoothed, relative to the ideal
    // gas filling the whole chamber (`N·v²/3V`), so a reading of 1 means
    // "as if the piston were fully withdrawn".
    if dt > 0.0 {
        let area = std::f32::consts::PI * ch.radius * ch.radius;
        let raw = sim.impulse / (dt * area);
        sim.impulse = 0.0;
        let reference = PARTICLES as f32 * SPEED * SPEED / (3.0 * area * ch.length);
        let k = 1.0 - (-dt / SMOOTHING).exp();
        sim.pressure += (raw / reference - sim.pressure) * k;
    }
    let producer = store.0.producer(PRODUCER).unwrap_or_else(|| store.0.register_producer(PRODUCER));
    let leaf = Leaf { node: targets.gauge, path: "param_spec.default".parse().expect("declared leaf") };
    // Publish at gauge precision, so a settled gas does not re-evaluate its
    // readers every frame.
    let value = (sim.pressure as f64 * 1000.0).round() / 1000.0;
    store.0.publish(producer, BTreeMap::from([(leaf, Value::Number(value))]));
    sim.frames += 1;
}

/// The producer's id in the store, registering it if needed.
pub fn producer_id(store: &mut Store) -> ProducerId {
    store.producer(PRODUCER).unwrap_or_else(|| store.register_producer(PRODUCER))
}
