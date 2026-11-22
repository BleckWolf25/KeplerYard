//! @file flight.rs
//!
//! @version 1.4.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Flight simulation coordinator with SFS part shaders, throat glow, FX, HUD, and save/load
//!
//! @description
//! Orchestrates live flight over `SimWorld`: spawns mirrored part meshes with archetype style IDs,
//! updates live engine nozzle incandescence and pressure-responsive plumes, drives physical vs
//! orbital warp limits, SAS hold modes, RCS translation, maneuver nodes, `[F5]`/`[F9]` quickloads,
//! parallax starfield/launchpad FX, and the 2D Navball HUD.
//!
//! @since 21/11/2026
//! @updated 22/11/2026
// The project header uses bare author metadata rather than doc-markdown code formatting.
#![allow(clippy::doc_markdown)]

// ---------- IMPORTS

use crate::camera::{CameraZoomState, MainCamera};
use crate::flight_fx::{
    BurstSpec, FlightFxMarker, FxMeshCache, spawn_burst_ring_and_sparks, spawn_environment_fx,
    update_environment_and_particles,
};
use crate::flight_hud::{
    FlightMapBadge, HudSyncQueries, draw_navball_gizmo, spawn_flight_hud_ui,
    style_flight_buttons_on_change, style_flight_buttons_on_release, update_flight_hud_and_badges,
};
use crate::procedural_mesh::{
    build_deployed_chute_mesh, build_part_mesh, build_plume_quad, build_reentry_quad,
};
use crate::save_io::{load_quicksave, save_quicksave};
use crate::shaders::{AtmosphereMaterial, PartMaterial, PlumeMaterial, ReentryMaterial};
use crate::state::{
    ActiveBlueprint, AudioCue, AudioCueQueue, CameraFocusState, CameraFocusTarget, FlightClock,
    FloatingOrigin, GameMode, LivePartCatalog, PHYSICAL_WARP_MAX_INDEX, TIME_WARP_LEVELS,
    part_style_id, world_to_render_vec2,
};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use kepler_core::{
    ApsisKind, DVec2, ManeuverPlan, PartKind, PredictionOptions, SasMode, SimWorld,
    TrajectoryPrediction, TransferPlanEstimate, Vessel, VesselId, body::dominant_body,
    compute_hohmann_transfer, sim::SimStepEvent,
};
use std::collections::{HashMap, HashSet};

// ---------- CONSTANTS

const PREDICTION_REFRESH_FRAMES: u32 = 4;

// ---------- TYPES

#[derive(Resource)]
#[allow(clippy::struct_excessive_bools)]
pub struct FlightSimState {
    pub world: SimWorld,
    pub throttle: f64,
    pub sas_enabled: bool,
    pub sas_mode: SasMode,
    pub rcs_enabled: bool,
    pub maneuver: ManeuverPlan,
    pub transfer_est: Option<TransferPlanEstimate>,
    pub last_prediction: TrajectoryPrediction,
    pub status_banner: String,
    pub pause_menu_open: bool,
    pub recovered_summary: bool,
    pub show_controls_help: bool,
    pub request_revert_pad: bool,
    pub needs_visual_rebuild: bool,
    pub max_altitude_m: f64,
    pub max_speed_m_s: f64,
    pub max_temp_k: f64,
    pub staging_count: u32,
    pub stage_requested: bool,
    pub throttle_override: Option<f64>,
    pred_tick: u32,
}

#[derive(Component)]
pub struct FlightEntity;

#[derive(Component)]
struct FlightBodyQuad {
    body_id: u32,
}

#[derive(Component)]
struct FlightPartVisual {
    vessel_id: VesselId,
    instance_id: u64,
    fuel_cap_kg: f64,
    ablator_cap_kg: f64,
    max_temp_k: f64,
}

#[derive(Component)]
struct FlightPlumeVisual {
    vessel_id: VesselId,
    instance_id: u64,
    nozzle_offset_y: f32,
}

#[derive(Component)]
struct FlightChuteVisual {
    vessel_id: VesselId,
    instance_id: u64,
}

#[derive(Component)]
struct FlightReentryVisual {
    vessel_id: VesselId,
}

#[derive(Clone, Copy)]
struct PartRenderSnapshot {
    pos: Vec2,
    rot: f32,
    fuel_kg: f32,
    ablator_kg: f32,
    throttle: f32,
    gimbal_rad: f32,
    temp_k: f32,
    chute_deployed: bool,
}

type BodyVisualQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static FlightBodyQuad,
        &'static mut Transform,
        &'static MeshMaterial2d<AtmosphereMaterial>,
    ),
>;

type PartVisualQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static FlightPartVisual,
        &'static mut Transform,
        &'static MeshMaterial2d<PartMaterial>,
    ),
    Without<FlightBodyQuad>,
>;

type PlumeVisualQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static FlightPlumeVisual,
        &'static mut Transform,
        &'static MeshMaterial2d<PlumeMaterial>,
    ),
    (Without<FlightBodyQuad>, Without<FlightPartVisual>),
>;

type ChuteVisualQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static FlightChuteVisual,
        &'static mut Transform,
        &'static mut Visibility,
    ),
    (
        Without<FlightBodyQuad>,
        Without<FlightPartVisual>,
        Without<FlightPlumeVisual>,
    ),
>;

type ReentryVisualQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static FlightReentryVisual,
        &'static mut Transform,
        &'static MeshMaterial2d<ReentryMaterial>,
    ),
    (
        Without<FlightBodyQuad>,
        Without<FlightPartVisual>,
        Without<FlightPlumeVisual>,
        Without<FlightChuteVisual>,
    ),
>;

type VesselDynamicEntityQuery<'w, 's> = Query<
    'w,
    's,
    Entity,
    Or<(
        With<FlightPartVisual>,
        With<FlightPlumeVisual>,
        With<FlightChuteVisual>,
        With<FlightReentryVisual>,
    )>,
>;

#[derive(SystemParam)]
struct FlightAssets<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    atmo_mats: ResMut<'w, Assets<AtmosphereMaterial>>,
    part_mats: ResMut<'w, Assets<PartMaterial>>,
    plume_mats: ResMut<'w, Assets<PlumeMaterial>>,
    reentry_mats: ResMut<'w, Assets<ReentryMaterial>>,
}

#[derive(SystemParam)]
struct FlightReadState<'w, 's> {
    time: Res<'w, Time>,
    flight: Res<'w, FlightSimState>,
    clock: Res<'w, FlightClock>,
    zoom: Res<'w, CameraZoomState>,
    focus: Res<'w, CameraFocusState>,
    windows: Query<'w, 's, &'static Window>,
    cam_q: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<MainCamera>>,
}

#[derive(SystemParam)]
#[allow(clippy::struct_field_names)]
struct FlightRenderQueries<'w, 's> {
    body_q: BodyVisualQuery<'w, 's>,
    part_q: PartVisualQuery<'w, 's>,
    plume_q: PlumeVisualQuery<'w, 's>,
    chute_q: ChuteVisualQuery<'w, 's>,
    reentry_q: ReentryVisualQuery<'w, 's>,
}

#[derive(SystemParam)]
struct FlightStepContext<'w> {
    time: Res<'w, Time>,
    keys: Res<'w, ButtonInput<KeyCode>>,
    active_bp: Res<'w, ActiveBlueprint>,
    catalog: Res<'w, LivePartCatalog>,
    clock: ResMut<'w, FlightClock>,
    origin: ResMut<'w, FloatingOrigin>,
    focus: ResMut<'w, CameraFocusState>,
    fx_cache: Res<'w, FxMeshCache>,
    audio_cues: ResMut<'w, AudioCueQueue>,
}

type BadgeQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static FlightMapBadge,
        &'static mut Node,
        &'static mut Text,
        &'static mut TextColor,
    ),
    (
        Without<crate::flight_hud::FlightHudText>,
        Without<crate::flight_hud::FlightNavText>,
        Without<crate::flight_hud::PauseSummaryText>,
        Without<crate::flight_hud::FlightPauseModal>,
        Without<crate::flight_hud::FlightControlsText>,
        Without<crate::flight_hud::RecoverButtonMarker>,
    ),
>;

pub struct FlightPlugin;

// ---------- DECLARATIONS / IMPLEMENTATION

fn enter_flight(
    mut commands: Commands<'_, '_>,
    active_bp: Res<'_, ActiveBlueprint>,
    catalog: Res<'_, LivePartCatalog>,
    mut clock: ResMut<'_, FlightClock>,
    mut origin: ResMut<'_, FloatingOrigin>,
    mut assets: FlightAssets<'_>,
) {
    clock.warp_index = 0;
    clock.paused = false;
    clock.physical_capped = true;

    let world =
        match SimWorld::with_kepler_system_vessel(&active_bp.blueprint, &catalog.sim_catalog) {
            Ok(w) => w,
            Err(err) => {
                error!("Failed to initialize flight simulation: {err}");
                SimWorld::new()
            }
        };

    if let Some(active) = world.active_vessel() {
        origin.origin_m = active.pos;
    }

    spawn_environment_fx(&mut commands, &mut assets.meshes, &mut assets.part_mats);
    spawn_celestial_quads(&mut commands, &world, &mut assets);
    for vessel in &world.vessels {
        spawn_vessel_visuals(&mut commands, vessel, &catalog, *origin, &mut assets);
    }
    spawn_flight_hud_ui(&mut commands);

    let pred = world.active_vessel_id.map_or_default(|id| {
        world.predict_trajectory_with_plan(id, PredictionOptions::default(), None)
    });

    commands.insert_resource(FlightSimState {
        world,
        throttle: 1.0,
        sas_enabled: true,
        sas_mode: SasMode::Stability,
        rcs_enabled: false,
        maneuver: ManeuverPlan::default(),
        transfer_est: None,
        last_prediction: pred,
        status_banner: String::from("Ready on Kepler Prime launchpad."),
        pause_menu_open: false,
        recovered_summary: false,
        show_controls_help: false,
        request_revert_pad: false,
        needs_visual_rebuild: false,
        max_altitude_m: 0.0,
        max_speed_m_s: 0.0,
        max_temp_k: 293.15,
        staging_count: 0,
        stage_requested: false,
        throttle_override: None,
        pred_tick: 0,
    });
}

fn exit_flight(mut commands: Commands<'_, '_>, query: Query<'_, '_, Entity, With<FlightEntity>>) {
    for e in &query {
        commands.entity(e).despawn();
    }
    commands.remove_resource::<FlightSimState>();
    commands.remove_resource::<FxMeshCache>();
}

fn cleanup_fx_entities(
    mut commands: Commands<'_, '_>,
    q: Query<'_, '_, Entity, With<FlightFxMarker>>,
) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

fn spawn_celestial_quads(
    commands: &mut Commands<'_, '_>,
    world: &SimWorld,
    assets: &mut FlightAssets<'_>,
) {
    let sun_dir = Vec2::new(0.72, 0.69).normalize();
    for body in &world.bodies {
        let quad_mesh = assets.meshes.add(Rectangle::new(1.0, 1.0));
        let (surface_color, atmo_color, scale_h, max_atmo, z) = if body.id == 0 {
            let sh = body.atmosphere.map_or(5_600.0, |a| a.scale_height_m) as f32;
            let ma = body.atmosphere.map_or(70_000.0, |a| a.max_altitude_m) as f32;
            (
                Vec4::new(0.08, 0.16, 0.28, 1.0),
                Vec4::new(0.24, 0.64, 1.00, 1.0),
                sh,
                ma,
                -10.0,
            )
        } else {
            (
                Vec4::new(0.38, 0.40, 0.44, 1.0),
                Vec4::new(0.76, 0.80, 0.88, 1.0),
                1.0,
                0.0,
                -9.5,
            )
        };

        let mat = assets.atmo_mats.add(AtmosphereMaterial {
            planet_params: Vec4::new(0.0, body.radius_m as f32, 0.0, body.radius_m as f32),
            atmo_params: Vec4::new(scale_h, max_atmo, 0.0, 4_000_000.0),
            surface_color,
            atmo_color,
            sun_and_detail: Vec4::new(sun_dir.x, sun_dir.y, body.id as f32, 0.0),
        });
        commands.spawn((
            FlightEntity,
            FlightBodyQuad { body_id: body.id },
            Mesh2d(quad_mesh),
            MeshMaterial2d(mat),
            Transform::from_xyz(0.0, 0.0, z).with_scale(Vec3::new(4_000_000.0, 4_000_000.0, 1.0)),
        ));
    }
}

#[allow(clippy::too_many_lines)]
fn spawn_vessel_visuals(
    commands: &mut Commands<'_, '_>,
    vessel: &Vessel,
    catalog: &LivePartCatalog,
    origin: FloatingOrigin,
    assets: &mut FlightAssets<'_>,
) {
    let mut max_w = 1.6_f32;
    let mut max_h = 2.0_f32;

    for part in &vessel.parts {
        let style = catalog
            .visuals
            .get(&part.spec.id)
            .copied()
            .unwrap_or_default();
        let mesh = assets
            .meshes
            .add(build_part_mesh(&part.spec.shape, part.mirrored));
        let sz = part.spec.shape.bounding_size();
        max_w = max_w.max((part.local_pos.x.abs() * 2.0 + sz.x) as f32);
        max_h = max_h.max((part.local_pos.y.abs() * 2.0 + sz.y) as f32);

        let fuel_cap = part.spec.fuel_capacity_kg();
        let fuel_frac = if fuel_cap > 0.0 {
            (part.fuel_kg / fuel_cap) as f32
        } else {
            1.0
        };
        let abl_cap = match part.spec.kind {
            PartKind::HeatShield(hs) => hs.ablator_kg,
            _ => 0.0,
        };
        let abl_frac = if abl_cap > 0.0 {
            (part.ablator_kg / abl_cap) as f32
        } else {
            1.0
        };
        let mat = assets.part_mats.add(PartMaterial {
            base_color: Vec4::from_array(style.base_color),
            accent_color: Vec4::from_array(style.accent_color),
            flags: Vec4::new(style.emissive_strength, 0.0, fuel_frac, 0.0),
            thermal: Vec4::new(0.0, abl_frac, part_style_id(&part.spec), 0.0),
        });
        let world_p = vessel.part_world_pos(part);
        let pos_2d = world_to_render_vec2(world_p, origin);
        let angle = (vessel.angle + part.local_rot) as f32;

        commands.spawn((
            FlightEntity,
            FlightPartVisual {
                vessel_id: vessel.id,
                instance_id: part.instance_id,
                fuel_cap_kg: fuel_cap,
                ablator_cap_kg: abl_cap,
                max_temp_k: part.spec.max_temp_k,
            },
            Mesh2d(mesh),
            MeshMaterial2d(mat),
            Transform::from_xyz(
                pos_2d.x,
                pos_2d.y,
                if vessel.is_debris { -1.0 } else { 0.0 },
            )
            .with_rotation(Quat::from_rotation_z(angle)),
        ));

        if matches!(
            part.spec.kind,
            PartKind::Engine(_) | PartKind::SolidBooster(_)
        ) {
            let bell_w = sz.x as f32 * 0.92;
            let plume_len = bell_w * 4.8;
            let nozzle_offset_y = -(sz.y as f32) * 0.48;
            let plume_mesh = assets.meshes.add(build_plume_quad(bell_w, plume_len));
            let outer_col = if matches!(part.spec.kind, PartKind::SolidBooster(_)) {
                Vec4::new(1.0, 0.65, 0.20, 1.0)
            } else {
                Vec4::new(1.0, 0.45, 0.10, 1.0)
            };
            let plume_mat = assets.plume_mats.add(PlumeMaterial {
                params: Vec4::new(0.0, 0.0, 1.0, part.instance_id as f32),
                core_color: Vec4::new(0.96, 0.98, 1.0, 1.0),
                outer_color: outer_col,
            });
            commands.spawn((
                FlightEntity,
                FlightPlumeVisual {
                    vessel_id: vessel.id,
                    instance_id: part.instance_id,
                    nozzle_offset_y,
                },
                Mesh2d(plume_mesh),
                MeshMaterial2d(plume_mat),
                Transform::from_xyz(pos_2d.x, pos_2d.y, 1.0)
                    .with_rotation(Quat::from_rotation_z(angle)),
            ));
        }

        if matches!(part.spec.kind, PartKind::Parachute(_)) {
            let canopy_mesh = assets.meshes.add(build_deployed_chute_mesh(5.2, 4.6));
            let canopy_mat = assets.part_mats.add(PartMaterial {
                base_color: Vec4::new(0.96, 0.42, 0.18, 0.94),
                accent_color: Vec4::new(1.00, 0.94, 0.78, 1.0),
                flags: Vec4::new(1.5, 0.0, 1.0, 0.0),
                thermal: Vec4::new(0.0, 1.0, 13.0, 0.0),
            });
            commands.spawn((
                FlightEntity,
                FlightChuteVisual {
                    vessel_id: vessel.id,
                    instance_id: part.instance_id,
                },
                Mesh2d(canopy_mesh),
                MeshMaterial2d(canopy_mat),
                Transform::from_xyz(pos_2d.x, pos_2d.y, 1.5)
                    .with_rotation(Quat::from_rotation_z(angle)),
                Visibility::Hidden,
            ));
        }
    }

    let reentry_w = (max_w * 2.2).clamp(3.4, 9.0);
    let reentry_l = (max_h * 1.35).clamp(4.8, 12.0);
    let reentry_mesh = assets.meshes.add(build_reentry_quad(reentry_w, reentry_l));
    let reentry_mat = assets.reentry_mats.add(ReentryMaterial {
        params: Vec4::ZERO,
        plasma_color: Vec4::new(1.0, 0.92, 0.72, 1.0),
        wake_color: Vec4::new(1.0, 0.34, 0.08, 1.0),
    });
    let v_pos = world_to_render_vec2(vessel.pos, origin);
    commands.spawn((
        FlightEntity,
        FlightReentryVisual {
            vessel_id: vessel.id,
        },
        Mesh2d(reentry_mesh),
        MeshMaterial2d(reentry_mat),
        Transform::from_xyz(v_pos.x, v_pos.y, 2.0),
    ));
}

#[allow(clippy::too_many_lines)]
fn step_flight_controls_and_physics(
    mut commands: Commands<'_, '_>,
    mut ctx: FlightStepContext<'_>,
    mut flight: ResMut<'_, FlightSimState>,
    mut part_mats: ResMut<'_, Assets<PartMaterial>>,
) {
    let time = &ctx.time;
    let keys = &ctx.keys;
    let active_bp = &ctx.active_bp;
    let catalog = &ctx.catalog;
    let clock = &mut ctx.clock;
    let origin = &mut ctx.origin;
    let focus = &mut ctx.focus;

    if flight.request_revert_pad {
        flight.request_revert_pad = false;
        if let Ok(new_world) =
            SimWorld::with_kepler_system_vessel(&active_bp.blueprint, &catalog.sim_catalog)
        {
            flight.world = new_world;
            flight.throttle = 1.0;
            flight.sas_enabled = true;
            flight.sas_mode = SasMode::Stability;
            flight.rcs_enabled = false;
            flight.maneuver = ManeuverPlan::default();
            flight.recovered_summary = false;
            flight.max_altitude_m = 0.0;
            flight.max_speed_m_s = 0.0;
            flight.max_temp_k = 293.15;
            flight.staging_count = 0;
            flight.needs_visual_rebuild = true;
            flight.status_banner = String::from("Reverted to launchpad.");
            if let Some(v) = flight.world.active_vessel() {
                origin.origin_m = v.pos;
            }
        }
        return;
    }

    if keys.just_pressed(KeyCode::Escape) {
        flight.pause_menu_open = !flight.pause_menu_open;
        return;
    }
    if keys.just_pressed(KeyCode::Slash) {
        flight.show_controls_help = !flight.show_controls_help;
    }

    if flight.pause_menu_open || flight.recovered_summary {
        return;
    }

    let dt_real = f64::from(time.delta_secs()).min(0.05);

    if let Some(t) = flight.throttle_override.take() {
        flight.throttle = t;
    }
    if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ArrowUp) {
        flight.throttle = (flight.throttle + 0.65 * dt_real).min(1.0);
    }
    if keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ArrowDown) {
        flight.throttle = (flight.throttle - 0.65 * dt_real).max(0.0);
    }
    if keys.just_pressed(KeyCode::KeyZ) {
        flight.throttle = 1.0;
    }
    if keys.just_pressed(KeyCode::KeyX) {
        flight.throttle = 0.0;
    }
    if keys.just_pressed(KeyCode::KeyT) {
        flight.sas_enabled = !flight.sas_enabled;
    }
    if keys.just_pressed(KeyCode::KeyB) {
        flight.sas_mode = flight.sas_mode.next();
        flight.sas_enabled = true;
        flight.status_banner = format!("SAS mode: {}", flight.sas_mode.label());
    }
    if keys.just_pressed(KeyCode::KeyR) {
        flight.rcs_enabled = !flight.rcs_enabled;
        flight.status_banner = if flight.rcs_enabled {
            String::from("RCS ARMED ([H/N] Fore/Aft, [Q/E] Lateral)")
        } else {
            String::from("RCS OFF")
        };
    }

    if (keys.just_pressed(KeyCode::BracketRight) || keys.just_pressed(KeyCode::BracketLeft))
        && let Some(id) = flight.world.cycle_active_vessel()
        && let Some((ctrl, name)) = flight
            .world
            .active_vessel()
            .map(|v| (v.control, v.name.clone()))
    {
        flight.throttle = ctrl.throttle;
        flight.sas_enabled = ctrl.sas_enabled;
        flight.sas_mode = ctrl.sas_mode;
        flight.rcs_enabled = ctrl.rcs_enabled;
        flight.status_banner = format!("Focused {name} (#{id})");
        focus.target = CameraFocusTarget::ActiveVessel;
    }

    let physical_only = flight.world.requires_physical_warp();
    clock.physical_capped = physical_only;
    if keys.just_pressed(KeyCode::Period) {
        let cap = if physical_only {
            PHYSICAL_WARP_MAX_INDEX
        } else {
            TIME_WARP_LEVELS.len() - 1
        };
        clock.warp_index = (clock.warp_index + 1).min(cap);
    }
    if keys.just_pressed(KeyCode::Comma) {
        clock.warp_index = clock.warp_index.saturating_sub(1);
    }
    if physical_only && clock.warp_index > PHYSICAL_WARP_MAX_INDEX {
        clock.warp_index = PHYSICAL_WARP_MAX_INDEX;
    }
    if keys.just_pressed(KeyCode::KeyP) {
        clock.paused = !clock.paused;
    }

    if keys.just_pressed(KeyCode::KeyG)
        && let Some(active) = flight.world.active_vessel_mut()
    {
        let armed = active.arm_all_parachutes();
        if armed > 0 {
            flight.status_banner = format!("Armed {armed} parachute(s) (< 5.5 km & 320 m/s).");
        }
    }

    if keys.just_pressed(KeyCode::KeyM)
        && let Some(v) = flight.world.active_vessel()
        && let Some(est) = compute_hohmann_transfer(&flight.world.bodies, v.pos, v.vel, 1)
    {
        flight.maneuver = ManeuverPlan {
            enabled: true,
            lead_time_s: est.lead_time_until_window_s,
            prograde_dv_m_s: est.dv1_m_s,
            radial_dv_m_s: 0.0,
        };
        flight.status_banner = String::from("Planned Hohmann TLI maneuver to Moon.");
    }
    if keys.just_pressed(KeyCode::KeyV) {
        flight.maneuver.enabled = false;
        if flight.sas_mode == SasMode::Maneuver {
            flight.sas_mode = SasMode::Stability;
        }
        flight.status_banner = String::from("Cleared maneuver plan.");
    }
    if flight.maneuver.enabled {
        if keys.pressed(KeyCode::KeyU) {
            flight.maneuver.prograde_dv_m_s += 30.0 * dt_real;
        }
        if keys.pressed(KeyCode::KeyJ) {
            flight.maneuver.prograde_dv_m_s -= 30.0 * dt_real;
        }
        if keys.pressed(KeyCode::KeyO) {
            flight.maneuver.radial_dv_m_s += 25.0 * dt_real;
        }
        if keys.pressed(KeyCode::KeyL) {
            flight.maneuver.radial_dv_m_s -= 25.0 * dt_real;
        }
        if keys.pressed(KeyCode::KeyI) {
            flight.maneuver.lead_time_s += 120.0 * dt_real;
        }
        if keys.pressed(KeyCode::KeyK) {
            flight.maneuver.lead_time_s = (flight.maneuver.lead_time_s - 120.0 * dt_real).max(0.0);
        }
    }

    if keys.just_pressed(KeyCode::F5) {
        match save_quicksave(&flight.world, flight.maneuver) {
            Ok(path) => {
                flight.status_banner = format!("Quicksaved to {}", path.display());
            }
            Err(err) => flight.status_banner = err,
        }
    }
    if keys.just_pressed(KeyCode::F9) {
        match load_quicksave() {
            Ok((loaded, plan)) => {
                flight.world = loaded;
                flight.maneuver = plan;
                if let Some((ctrl, pos)) = flight.world.active_vessel().map(|v| (v.control, v.pos))
                {
                    flight.throttle = ctrl.throttle;
                    flight.sas_enabled = ctrl.sas_enabled;
                    flight.sas_mode = ctrl.sas_mode;
                    flight.rcs_enabled = ctrl.rcs_enabled;
                    origin.origin_m = pos;
                }
                flight.needs_visual_rebuild = true;
                flight.pred_tick = PREDICTION_REFRESH_FRAMES;
                flight.status_banner = String::from("Loaded bit-exact quicksave.");
            }
            Err(err) => flight.status_banner = err,
        }
        return;
    }

    let mut steer = 0.0;
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        steer += 1.0;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        steer -= 1.0;
    }

    let mut rcs_trans = DVec2::ZERO;
    if keys.pressed(KeyCode::KeyH) {
        rcs_trans.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyN) {
        rcs_trans.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyQ) {
        rcs_trans.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyE) {
        rcs_trans.x += 1.0;
    }

    let stage_clicked = flight.stage_requested;
    flight.stage_requested = false;
    if (keys.just_pressed(KeyCode::Space) || stage_clicked)
        && let Some(outcome) = flight.world.trigger_active_stage()
    {
        flight.staging_count += 1;
        flight.status_banner = format!(
            "Stage {} fired ({} eng, {} sep, {} chute).",
            outcome.fired_stage + 1,
            outcome.ignited_engines,
            outcome.fired_decouplers,
            outcome.armed_chutes
        );
        if outcome.fired_decouplers > 0
            && let Some(v) = flight.world.active_vessel()
        {
            ctx.audio_cues.cues.push(AudioCue::StageSeparation);
            spawn_burst_ring_and_sparks(
                &mut commands,
                &ctx.fx_cache,
                &mut part_mats,
                BurstSpec {
                    world_pos: v.pos,
                    base_vel: v.vel,
                    color: Vec4::new(1.0, 0.82, 0.35, 0.92),
                    count: 10,
                    seed: flight.world.next_vessel_id,
                },
            );
        }
    }

    let throttle = flight.throttle;
    let sas_enabled = flight.sas_enabled;
    let sas_mode = flight.sas_mode;
    let rcs_enabled = flight.rcs_enabled;
    if let Some(active) = flight.world.active_vessel_mut() {
        active.control.throttle = throttle;
        active.control.steer = steer;
        active.control.sas_enabled = sas_enabled;
        active.control.sas_mode = sas_mode;
        active.control.rcs_enabled = rcs_enabled;
        active.control.rcs_translate = rcs_trans;
    }

    if flight.maneuver.enabled && flight.maneuver.lead_time_s <= 15.0 && clock.warp_index > 0 {
        clock.warp_index = 0;
        flight.status_banner = String::from("Auto-dropped warp for maneuver node!");
    }

    let sim_dt = dt_real * clock.warp_factor();
    if sim_dt > 0.0 {
        let plan_opt = flight.maneuver.enabled.then_some(flight.maneuver);
        let events = flight.world.step_with_events(sim_dt, plan_opt);
        for ev in events {
            match ev {
                SimStepEvent::PartDestroyed { world_pos, thermal } => {
                    ctx.audio_cues
                        .cues
                        .push(AudioCue::PartDestroyed { thermal });
                    spawn_burst_ring_and_sparks(
                        &mut commands,
                        &ctx.fx_cache,
                        &mut part_mats,
                        BurstSpec {
                            world_pos,
                            base_vel: DVec2::ZERO,
                            color: Vec4::new(1.0, 0.42, 0.14, 0.95),
                            count: 12,
                            seed: (flight.world.time_s * 100.0) as u64,
                        },
                    );
                }
                SimStepEvent::VesselCrashed { world_pos } => {
                    ctx.audio_cues.cues.push(AudioCue::VesselCrashed);
                    spawn_burst_ring_and_sparks(
                        &mut commands,
                        &ctx.fx_cache,
                        &mut part_mats,
                        BurstSpec {
                            world_pos,
                            base_vel: DVec2::ZERO,
                            color: Vec4::new(1.0, 0.25, 0.12, 1.0),
                            count: 24,
                            seed: (flight.world.time_s * 1000.0) as u64,
                        },
                    );
                }
                SimStepEvent::Touchdown {
                    world_pos,
                    speed_m_s,
                } => {
                    ctx.audio_cues.cues.push(AudioCue::Touchdown { speed_m_s });
                    spawn_burst_ring_and_sparks(
                        &mut commands,
                        &ctx.fx_cache,
                        &mut part_mats,
                        BurstSpec {
                            world_pos,
                            base_vel: DVec2::ZERO,
                            color: Vec4::new(0.80, 0.78, 0.72, 0.65),
                            count: 10,
                            seed: (flight.world.time_s * 50.0) as u64,
                        },
                    );
                }
                SimStepEvent::ChuteDeployed { .. } => {
                    ctx.audio_cues.cues.push(AudioCue::ChuteDeploy);
                }
            }
        }
        if flight.maneuver.enabled {
            flight.maneuver.lead_time_s = (flight.maneuver.lead_time_s - sim_dt).max(0.0);
        }
    }

    if let Some((v_id, peak_t)) = flight
        .world
        .active_vessel()
        .map(|v| (v.id, v.peak_temperature_k()))
    {
        flight.max_temp_k = flight.max_temp_k.max(peak_t);
        if let Some(el) = flight.world.vessel_orbit_elements(v_id) {
            flight.max_altitude_m = flight.max_altitude_m.max(el.altitude_m);
            flight.max_speed_m_s = flight.max_speed_m_s.max(el.speed_m_s);
        }
    }

    flight.transfer_est = flight
        .world
        .active_vessel()
        .and_then(|v| compute_hohmann_transfer(&flight.world.bodies, v.pos, v.vel, 1));

    flight.pred_tick += 1;
    if flight.pred_tick >= PREDICTION_REFRESH_FRAMES || flight.maneuver.enabled {
        flight.pred_tick = 0;
        let plan_opt = flight.maneuver.enabled.then_some(flight.maneuver);
        flight.last_prediction = flight.world.active_vessel_id.map_or_default(|id| {
            flight
                .world
                .predict_trajectory_with_plan(id, PredictionOptions::default(), plan_opt)
        });
    }
}

fn sync_or_rebuild_vessel_visuals(
    mut commands: Commands<'_, '_>,
    catalog: Res<'_, LivePartCatalog>,
    origin: Res<'_, FloatingOrigin>,
    mut flight: ResMut<'_, FlightSimState>,
    existing_parts: Query<'_, '_, &FlightPartVisual>,
    all_vessel_vis: VesselDynamicEntityQuery<'_, '_>,
    mut assets: FlightAssets<'_>,
) {
    if flight.needs_visual_rebuild {
        flight.needs_visual_rebuild = false;
        for e in &all_vessel_vis {
            commands.entity(e).despawn();
        }
        for vessel in &flight.world.vessels {
            spawn_vessel_visuals(&mut commands, vessel, &catalog, *origin, &mut assets);
        }
        return;
    }

    let spawned_vessels: HashSet<VesselId> = existing_parts.iter().map(|v| v.vessel_id).collect();
    for vessel in &flight.world.vessels {
        if !spawned_vessels.contains(&vessel.id) {
            spawn_vessel_visuals(&mut commands, vessel, &catalog, *origin, &mut assets);
        }
    }
}

#[allow(clippy::too_many_lines)]
fn sync_flight_visuals_and_hud(
    state: FlightReadState<'_, '_>,
    mut origin: ResMut<'_, FloatingOrigin>,
    mut commands: Commands<'_, '_>,
    mut gizmos: Gizmos<'_, '_>,
    mut assets: FlightAssets<'_>,
    mut queries: FlightRenderQueries<'_, '_>,
    hud_ctx: (HudSyncQueries<'_, '_>, BadgeQuery<'_, '_>),
) {
    let (hud_queries, badge_q) = hud_ctx;

    match state.focus.target {
        CameraFocusTarget::ActiveVessel => {
            if let Some(active) = state.flight.world.active_vessel() {
                origin.origin_m = active.pos + state.zoom.flight_pan_m;
            }
        }
        CameraFocusTarget::Body(id) => {
            if let Some(body) = state.flight.world.bodies.iter().find(|b| b.id == id) {
                origin.origin_m = body.pos + state.zoom.flight_pan_m;
            }
        }
    }

    let elapsed = state.time.elapsed_secs();
    let ortho_scale = state.zoom.log_scale.exp();
    let quad_span = (ortho_scale * 3400.0).clamp(600.0, 65_000_000.0);

    for (b_quad, mut tf, mat_handle) in &mut queries.body_q {
        let Some(body) = state
            .flight
            .world
            .bodies
            .iter()
            .find(|b| b.id == b_quad.body_id)
        else {
            continue;
        };
        tf.translation.x = 0.0;
        tf.translation.y = 0.0;
        tf.scale = Vec3::new(quad_span, quad_span, 1.0);

        let u_rel = origin.origin_m - body.pos;
        let r_cam = u_rel.length();
        let h_origin = r_cam - body.radius_m;
        if let Some(mut mat) = assets.atmo_mats.get_mut(&mat_handle.0) {
            mat.planet_params = Vec4::new(
                u_rel.x as f32,
                u_rel.y as f32,
                h_origin as f32,
                body.radius_m as f32,
            );
            let (sh, ma) = body.atmosphere.map_or((1.0_f32, 0.0_f32), |a| {
                (a.scale_height_m as f32, a.max_altitude_m as f32)
            });
            mat.atmo_params = Vec4::new(sh, ma, elapsed, quad_span * 0.5);
        }
    }

    let mut part_lookup: HashMap<(VesselId, u64), PartRenderSnapshot> = HashMap::new();
    let mut vessel_pressure_ratio: HashMap<VesselId, f32> = HashMap::new();
    let mut vessel_rel_vel: HashMap<VesselId, Vec2> = HashMap::new();

    for vessel in &state.flight.world.vessels {
        let p_ratio = if let Some(primary) = state.flight.world.bodies.first()
            && let Some(atmo) = primary.atmosphere
        {
            let alt = vessel.pos.distance(primary.pos) - primary.radius_m;
            let rho = atmo.density_at(alt);
            (rho / atmo.sea_level_density_kg_m3.max(0.01)).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        vessel_pressure_ratio.insert(vessel.id, p_ratio);

        let dom_vel =
            dominant_body(&state.flight.world.bodies, vessel.pos).map_or(DVec2::ZERO, |b| b.vel);
        let rv = vessel.vel - dom_vel;
        vessel_rel_vel.insert(vessel.id, Vec2::new(rv.x as f32, rv.y as f32));

        for part in &vessel.parts {
            let world_p = vessel.part_world_pos(part);
            let render_p = world_to_render_vec2(world_p, *origin);
            let rot = (vessel.angle + part.local_rot) as f32;
            let throttle = if part.engine_active {
                part.active_throttle as f32
            } else {
                0.0
            };
            part_lookup.insert(
                (vessel.id, part.instance_id),
                PartRenderSnapshot {
                    pos: render_p,
                    rot,
                    fuel_kg: part.fuel_kg as f32,
                    ablator_kg: part.ablator_kg as f32,
                    throttle,
                    gimbal_rad: part.active_gimbal_rad as f32,
                    temp_k: part.temperature_k as f32,
                    chute_deployed: part.chute_deployed,
                },
            );
        }
    }

    for (entity, vis, mut tf, mat_h) in &mut queries.part_q {
        let Some(&snap) = part_lookup.get(&(vis.vessel_id, vis.instance_id)) else {
            commands.entity(entity).despawn();
            continue;
        };
        tf.translation.x = snap.pos.x;
        tf.translation.y = snap.pos.y;
        tf.rotation = Quat::from_rotation_z(snap.rot + snap.gimbal_rad * 0.5);
        if let Some(mut mat) = assets.part_mats.get_mut(&mat_h.0) {
            if vis.fuel_cap_kg > 0.0 {
                mat.flags.z = (snap.fuel_kg / vis.fuel_cap_kg as f32).clamp(0.0, 1.0);
            }
            let heat_ratio =
                ((snap.temp_k - 360.0) / (vis.max_temp_k as f32 - 360.0)).clamp(0.0, 1.0);
            mat.thermal.x = heat_ratio;
            if vis.ablator_cap_kg > 0.0 {
                mat.thermal.y = (snap.ablator_kg / vis.ablator_cap_kg as f32).clamp(0.0, 1.0);
            }
            mat.thermal.w = snap.throttle;
            mat.flags.w = elapsed;
        }
    }

    for (entity, vis, mut tf, mat_h) in &mut queries.plume_q {
        let Some(&snap) = part_lookup.get(&(vis.vessel_id, vis.instance_id)) else {
            commands.entity(entity).despawn();
            continue;
        };
        let nozzle_exit =
            snap.pos + Vec2::from_angle(snap.rot).rotate(Vec2::new(0.0, vis.nozzle_offset_y));
        tf.translation.x = nozzle_exit.x;
        tf.translation.y = nozzle_exit.y;
        tf.rotation = Quat::from_rotation_z(snap.rot + snap.gimbal_rad);
        if let Some(mut mat) = assets.plume_mats.get_mut(&mat_h.0) {
            mat.params.x = snap.throttle;
            mat.params.y = elapsed;
            mat.params.z = vessel_pressure_ratio
                .get(&vis.vessel_id)
                .copied()
                .unwrap_or(0.5);
        }
    }

    for (entity, vis, mut tf, mut visibility) in &mut queries.chute_q {
        let Some(&snap) = part_lookup.get(&(vis.vessel_id, vis.instance_id)) else {
            commands.entity(entity).despawn();
            continue;
        };
        if snap.chute_deployed {
            *visibility = Visibility::Visible;
            let vel = vessel_rel_vel
                .get(&vis.vessel_id)
                .copied()
                .unwrap_or(Vec2::Y);
            let up_dir = (-vel).normalize_or_zero();
            let canopy_rot = if up_dir.length_squared() > 0.01 {
                (-up_dir.x).atan2(up_dir.y)
            } else {
                0.0
            };
            tf.translation.x = snap.pos.x;
            tf.translation.y = snap.pos.y;
            tf.rotation = Quat::from_rotation_z(canopy_rot);
        } else {
            *visibility = Visibility::Hidden;
        }
    }

    for (entity, vis, mut tf, mat_h) in &mut queries.reentry_q {
        let Some(vessel) = state
            .flight
            .world
            .vessels
            .iter()
            .find(|v| v.id == vis.vessel_id)
        else {
            commands.entity(entity).despawn();
            continue;
        };
        let pos = world_to_render_vec2(vessel.pos, *origin);
        let flux = vessel.peak_heat_flux() as f32;
        let intensity = (flux / 250_000.0).clamp(0.0, 1.8);
        let vel = vessel_rel_vel
            .get(&vis.vessel_id)
            .copied()
            .unwrap_or(Vec2::ZERO);
        let vel_ang = if vel.length_squared() > 1.0 {
            (-vel.x).atan2(vel.y)
        } else {
            vessel.angle as f32
        };
        tf.translation.x = pos.x;
        tf.translation.y = pos.y;
        tf.rotation = Quat::from_rotation_z(vel_ang);
        if let Some(mut mat) = assets.reentry_mats.get_mut(&mat_h.0) {
            mat.params.x = if flux > 8_000.0 { intensity } else { 0.0 };
            mat.params.y = elapsed;
        }
    }

    draw_orbital_and_transfer_gizmos(&state.flight, *origin, ortho_scale, &mut gizmos);

    if let Ok(window) = state.windows.single() {
        draw_navball_gizmo(&state.flight, window, ortho_scale, &mut gizmos);
        if let Ok(cam) = state.cam_q.single() {
            update_flight_hud_and_badges(
                &state.flight,
                &state.clock,
                *origin,
                cam,
                window,
                hud_queries,
                badge_q,
            );
        }
    }
}

fn draw_orbital_and_transfer_gizmos(
    flight: &FlightSimState,
    origin: FloatingOrigin,
    ortho_scale: f32,
    gizmos: &mut Gizmos<'_, '_>,
) {
    if let (Some(planet), Some(moon)) = (flight.world.bodies.first(), flight.world.bodies.get(1)) {
        let p_pos = world_to_render_vec2(planet.pos, origin);
        let orbit_r = planet.pos.distance(moon.pos) as f32;
        gizmos.circle_2d(p_pos, orbit_r, Color::srgba(0.55, 0.62, 0.78, 0.25));
    }

    let pred = &flight.last_prediction;
    draw_polyline_and_markers(
        &pred.points_world,
        &pred.markers,
        origin,
        ortho_scale,
        Color::srgba(0.24, 0.88, 1.0, 0.85),
        gizmos,
    );

    if !pred.planned_points_world.is_empty() {
        draw_polyline_and_markers(
            &pred.planned_points_world,
            &pred.planned_markers,
            origin,
            ortho_scale,
            Color::srgba(1.0, 0.72, 0.18, 0.90),
            gizmos,
        );
    }

    if let Some(mpos) = pred.maneuver_world_pos {
        let mp = world_to_render_vec2(mpos, origin);
        let r = (10.0 * ortho_scale).max(0.5);
        gizmos.circle_2d(mp, r, Color::srgb(1.0, 0.45, 0.92));
        gizmos.line_2d(
            mp - Vec2::X * r * 1.4,
            mp + Vec2::X * r * 1.4,
            Color::srgb(1.0, 0.45, 0.92),
        );
        gizmos.line_2d(
            mp - Vec2::Y * r * 1.4,
            mp + Vec2::Y * r * 1.4,
            Color::srgb(1.0, 0.45, 0.92),
        );
    }

    if let (Some(ghost_m), Some(moon)) = (
        pred.planned_moon_ghost_pos.or(pred.moon_ghost_pos),
        flight.world.bodies.get(1),
    ) {
        let gp = world_to_render_vec2(ghost_m, origin);
        gizmos.circle_2d(
            gp,
            moon.radius_m as f32,
            Color::srgba(0.95, 0.85, 0.35, 0.45),
        );
    }

    if ortho_scale > 8.0
        && let Some(v) = flight.world.active_vessel()
    {
        let vp = world_to_render_vec2(v.pos, origin);
        let mr = 7.0 * ortho_scale;
        gizmos.circle_2d(vp, mr, Color::srgb(0.38, 0.96, 1.0));
    }
}

fn draw_polyline_and_markers(
    points: &[DVec2],
    markers: &[kepler_core::ApsisMarker],
    origin: FloatingOrigin,
    ortho_scale: f32,
    path_color: Color,
    gizmos: &mut Gizmos<'_, '_>,
) {
    if points.len() >= 2 {
        for pair in points.windows(2) {
            if let [a, b] = pair {
                let pa = world_to_render_vec2(*a, origin);
                let pb = world_to_render_vec2(*b, origin);
                gizmos.line_2d(pa, pb, path_color);
            }
        }
    }

    let marker_radius = (8.0 * ortho_scale).max(0.4);
    for m in markers {
        let p = world_to_render_vec2(m.world_pos, origin);
        let color = match m.kind {
            ApsisKind::Apoapsis => Color::srgb(0.25, 0.98, 0.60),
            ApsisKind::Periapsis => Color::srgb(0.35, 0.72, 1.00),
            ApsisKind::Impact => Color::srgb(1.00, 0.28, 0.24),
            ApsisKind::MoonEncounter => Color::srgb(1.00, 0.85, 0.22),
            ApsisKind::ClosestApproach => Color::srgb(0.95, 0.45, 0.98),
        };
        gizmos.circle_2d(p, marker_radius, color);
    }
}

impl Plugin for FlightPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameMode::Flight), enter_flight)
            .add_systems(OnExit(GameMode::Flight), (exit_flight, cleanup_fx_entities))
            .add_systems(
                Update,
                (
                    style_flight_buttons_on_change,
                    style_flight_buttons_on_release,
                    step_flight_controls_and_physics,
                    sync_or_rebuild_vessel_visuals,
                    sync_flight_visuals_and_hud,
                    crate::flight_hud::sync_throttle_slider,
                    update_environment_and_particles,
                )
                    .chain()
                    .run_if(in_state(GameMode::Flight)),
            );
    }
}

// ---------- EXPORTS
