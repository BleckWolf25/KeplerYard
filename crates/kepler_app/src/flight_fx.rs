//! @file flight_fx.rs
//!
//! @version 1.1.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Parallax starfield, launchpad gantry tower, and staging/impact particle bursts
//!
//! @description
//! Spawns a deterministic parallax starfield across all zoom scales, renders a procedural
//! launchpad apron and gantry tower at Kepler Prime's north pole, and drives transient glowing
//! shockwave rings and spark/dust particles using flat emissive material pass (style ID 99).
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use crate::camera::CameraZoomState;
use crate::flight::FlightSimState;
use crate::procedural_mesh::{build_quad_mesh, build_ring_mesh};
use crate::shaders::PartMaterial;
use crate::state::{FloatingOrigin, world_to_render_vec2};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use sim_core::{DVec2, KEPLER_PRIME_RADIUS_M, SplitMix64};

// ---------- CONSTANTS

const STAR_COUNT: usize = 135;
const STAR_WRAP_W: f32 = 2_400.0;
const STAR_WRAP_H: f32 = 1_500.0;

// ---------- TYPES

#[derive(Component)]
pub struct FlightFxMarker;

#[derive(Component)]
pub struct ParallaxStar {
    base_pos: Vec2,
    parallax: f32,
    size_px: f32,
}

#[derive(Component)]
pub struct LaunchpadStructure {
    local_offset_m: DVec2,
}

#[derive(Component)]
pub struct BurstParticle {
    world_pos: DVec2,
    vel_world: DVec2,
    age_s: f32,
    max_age_s: f32,
    start_size: f32,
    end_size: f32,
    base_color: Vec4,
}

#[derive(Resource, Default)]
pub struct FxMeshCache {
    pub quad: Option<Handle<Mesh>>,
    pub ring: Option<Handle<Mesh>>,
}

pub struct BurstSpec {
    pub world_pos: DVec2,
    pub base_vel: DVec2,
    pub color: Vec4,
    pub count: usize,
    pub seed: u64,
}

type BurstParticleQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut BurstParticle,
        &'static mut Transform,
        &'static MeshMaterial2d<PartMaterial>,
    ),
    (Without<ParallaxStar>, Without<LaunchpadStructure>),
>;

#[derive(SystemParam)]
pub struct FxQueries<'w, 's> {
    star_q: Query<'w, 's, (&'static ParallaxStar, &'static mut Transform)>,
    pad_q: Query<
        'w,
        's,
        (
            &'static LaunchpadStructure,
            &'static mut Transform,
            &'static mut Visibility,
        ),
        Without<ParallaxStar>,
    >,
    burst_q: BurstParticleQuery<'w, 's>,
}

// ---------- DECLARATIONS / IMPLEMENTATION

pub fn spawn_environment_fx(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<PartMaterial>,
) {
    let quad = meshes.add(build_quad_mesh(1.0, 1.0));
    let ring = meshes.add(build_ring_mesh(0.75, 1.0, 24));
    commands.insert_resource(FxMeshCache {
        quad: Some(quad.clone()),
        ring: Some(ring),
    });

    let mut rng = SplitMix64::new(0xA572_0F1E_1D00_2026);
    for _ in 0..STAR_COUNT {
        let bx = rng.range_f64(-0.5 * f64::from(STAR_WRAP_W), 0.5 * f64::from(STAR_WRAP_W)) as f32;
        let by = rng.range_f64(-0.5 * f64::from(STAR_WRAP_H), 0.5 * f64::from(STAR_WRAP_H)) as f32;
        let layer = rng.next_f64() as f32;
        let parallax = 0.000_005 + layer * 0.000_022;
        let size_px = 1.2 + layer * 1.9;
        let tint = 0.65 + 0.35 * (rng.next_f64() as f32);
        let warm = rng.next_f64() as f32;
        let col = Vec4::new(
            tint * (0.88 + 0.12 * warm),
            tint * 0.94,
            tint * (1.0 - 0.12 * warm),
            0.55 + 0.40 * layer,
        );

        let mat = materials.add(PartMaterial {
            base_color: col,
            accent_color: col,
            flags: Vec4::new(2.2, 0.0, 1.0, 0.0),
            thermal: Vec4::new(0.0, 1.0, 99.0, 0.0),
        });

        commands.spawn((
            FlightFxMarker,
            ParallaxStar {
                base_pos: Vec2::new(bx, by),
                parallax,
                size_px,
            },
            Mesh2d(quad.clone()),
            MeshMaterial2d(mat),
            Transform::from_xyz(bx, by, -12.0),
        ));
    }

    let pad_pieces = [
        (DVec2::new(0.0, -0.35), 14.0_f32, 0.7_f32, Vec4::new(0.28, 0.31, 0.36, 1.0)),
        (DVec2::new(-4.8, 7.5), 1.4_f32, 15.0_f32, Vec4::new(0.36, 0.24, 0.20, 1.0)),
        (DVec2::new(-2.4, 12.2), 4.2_f32, 0.45_f32, Vec4::new(0.88, 0.52, 0.18, 1.0)),
    ];
    for (offset, w, h, color) in pad_pieces {
        let mat = materials.add(PartMaterial {
            base_color: color,
            accent_color: Vec4::new(1.0, 0.65, 0.2, 1.0),
            flags: Vec4::new(1.4, 0.0, 1.0, 0.0),
            thermal: Vec4::new(0.0, 1.0, 99.0, 0.0),
        });
        commands.spawn((
            FlightFxMarker,
            LaunchpadStructure {
                local_offset_m: offset,
            },
            Mesh2d(quad.clone()),
            MeshMaterial2d(mat),
            Transform::from_xyz(0.0, 0.0, -2.5).with_scale(Vec3::new(w, h, 1.0)),
        ));
    }
}

pub fn spawn_burst_ring_and_sparks(
    commands: &mut Commands,
    cache: &FxMeshCache,
    materials: &mut Assets<PartMaterial>,
    spec: BurstSpec,
) {
    if let Some(ring_mesh) = &cache.ring {
        let mat = materials.add(PartMaterial {
            base_color: spec.color,
            accent_color: spec.color,
            flags: Vec4::new(2.8, 0.0, 1.0, 0.0),
            thermal: Vec4::new(0.0, 1.0, 99.0, 0.0),
        });
        commands.spawn((
            FlightFxMarker,
            BurstParticle {
                world_pos: spec.world_pos,
                vel_world: spec.base_vel,
                age_s: 0.0,
                max_age_s: 0.55,
                start_size: 1.0,
                end_size: 11.0,
                base_color: spec.color,
            },
            Mesh2d(ring_mesh.clone()),
            MeshMaterial2d(mat),
            Transform::from_xyz(0.0, 0.0, 2.5),
        ));
    }

    if let Some(quad_mesh) = &cache.quad {
        let mut rng = SplitMix64::new(spec.seed ^ 0x9E37_79B9);
        for _ in 0..spec.count {
            let ang = rng.range_f64(0.0, core::f64::consts::TAU);
            let spd = rng.range_f64(6.0, 26.0);
            let vel = spec.base_vel + DVec2::new(ang.cos(), ang.sin()) * spd;
            let mat = materials.add(PartMaterial {
                base_color: spec.color,
                accent_color: spec.color,
                flags: Vec4::new(2.5, 0.0, 1.0, 0.0),
                thermal: Vec4::new(0.0, 1.0, 99.0, 0.0),
            });
            commands.spawn((
                FlightFxMarker,
                BurstParticle {
                    world_pos: spec.world_pos,
                    vel_world: vel,
                    age_s: 0.0,
                    max_age_s: rng.range_f64(0.35, 0.85) as f32,
                    start_size: 0.55,
                    end_size: 0.08,
                    base_color: spec.color,
                },
                Mesh2d(quad_mesh.clone()),
                MeshMaterial2d(mat),
                Transform::from_xyz(0.0, 0.0, 2.6),
            ));
        }
    }
}

pub fn update_environment_and_particles(
    mut commands: Commands,
    time: Res<Time>,
    origin: Res<FloatingOrigin>,
    zoom: Res<CameraZoomState>,
    flight: Res<FlightSimState>,
    mut fx: FxQueries,
    mut materials: ResMut<Assets<PartMaterial>>,
) {
    let ortho_scale = zoom.log_scale.exp();

    for (star, mut tf) in &mut fx.star_q {
        let sx = (star.base_pos.x - (origin.origin_m.x as f32) * star.parallax + 0.5 * STAR_WRAP_W)
            .rem_euclid(STAR_WRAP_W)
            - 0.5 * STAR_WRAP_W;
        let sy = (star.base_pos.y - (origin.origin_m.y as f32) * star.parallax + 0.5 * STAR_WRAP_H)
            .rem_euclid(STAR_WRAP_H)
            - 0.5 * STAR_WRAP_H;
        tf.translation.x = sx * ortho_scale;
        tf.translation.y = sy * ortho_scale;
        tf.scale = Vec3::splat(star.size_px * ortho_scale);
    }

    let p_pos = flight.world.bodies.first().map_or(DVec2::ZERO, |b| b.pos);
    let pole_world = p_pos + DVec2::new(0.0, KEPLER_PRIME_RADIUS_M);
    let near_pad = pole_world.distance(origin.origin_m) < 40_000.0;
    for (pad, mut tf, mut vis) in &mut fx.pad_q {
        if near_pad {
            *vis = Visibility::Visible;
            let rp = world_to_render_vec2(pole_world + pad.local_offset_m, *origin);
            tf.translation.x = rp.x;
            tf.translation.y = rp.y;
        } else {
            *vis = Visibility::Hidden;
        }
    }

    let dt = time.delta_secs().min(0.05);
    for (entity, mut p, mut tf, mat_h) in &mut fx.burst_q {
        p.age_s += dt;
        if p.age_s >= p.max_age_s {
            commands.entity(entity).despawn();
            continue;
        }
        let step_dp = p.vel_world * f64::from(dt);
        p.world_pos += step_dp;
        let t = (p.age_s / p.max_age_s).clamp(0.0, 1.0);
        let sz = p.start_size + (p.end_size - p.start_size) * t;
        let rp = world_to_render_vec2(p.world_pos, *origin);
        tf.translation.x = rp.x;
        tf.translation.y = rp.y;
        tf.scale = Vec3::splat(sz);
        if let Some(mut mat) = materials.get_mut(&mat_h.0) {
            mat.base_color.w = p.base_color.w * (1.0 - t);
        }
    }
}

// ---------- EXPORTS
