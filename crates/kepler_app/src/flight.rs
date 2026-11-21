//! @file flight.rs
//!
//! @version 1.0.1
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Milestone 1 & 2 flight simulator loop, floating-origin visuals, trajectory line, and HUD
//!
//! @description
//! Launches the edited `ActiveBlueprint` onto Kepler Prime's launchpad inside `SimWorld`, steps
//! deterministic symplectic physics with keyboard throttle/steering/staging/time-warp controls,
//! spawns procedural meshes + turbulent engine plumes + analytic atmosphere shader visuals relative
//! to `FloatingOrigin`, draws live predicted orbital trajectories with Ap/Pe/Impact markers, and
//! renders telemetry HUD with stage status and return-to-editor flow.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use crate::camera::CameraZoomState;
use crate::procedural_mesh::{build_part_mesh, build_plume_quad};
use crate::shaders::{AtmosphereMaterial, PartMaterial, PlumeMaterial};
use crate::state::{
    ActiveBlueprint, FlightClock, FloatingOrigin, GameMode, LivePartCatalog, TIME_WARP_LEVELS,
    uniform_border, world_to_render_vec2,
};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use sim_core::{
    ApsisKind, PartKind, PredictionOptions, SimWorld, TrajectoryPrediction, Vessel, VesselId,
};
use std::collections::HashMap;

// ---------- CONSTANTS

// ---------- TYPES

#[derive(Resource)]
struct FlightSimState {
    world: SimWorld,
    throttle: f64,
    sas_enabled: bool,
    last_prediction: TrajectoryPrediction,
}

#[derive(Component)]
struct FlightEntity;

#[derive(Component)]
struct FlightPlanetQuad;

#[derive(Component)]
struct FlightPartVisual {
    vessel_id: VesselId,
    instance_id: u64,
    fuel_cap_kg: f64,
}

#[derive(Component)]
struct FlightPlumeVisual {
    vessel_id: VesselId,
    instance_id: u64,
}

#[derive(Component)]
struct FlightHudText;

#[derive(SystemParam)]
struct FlightAssets<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    atmo_mats: ResMut<'w, Assets<AtmosphereMaterial>>,
    part_mats: ResMut<'w, Assets<PartMaterial>>,
    plume_mats: ResMut<'w, Assets<PlumeMaterial>>,
}

#[derive(SystemParam)]
struct FlightRenderQueries<'w, 's> {
    planet_q: Query<
        'w,
        's,
        (&'static mut Transform, &'static MeshMaterial2d<AtmosphereMaterial>),
        With<FlightPlanetQuad>,
    >,
    part_q: Query<
        'w,
        's,
        (
            Entity,
            &'static FlightPartVisual,
            &'static mut Transform,
            &'static MeshMaterial2d<PartMaterial>,
        ),
        Without<FlightPlanetQuad>,
    >,
    plume_q: Query<
        'w,
        's,
        (
            Entity,
            &'static FlightPlumeVisual,
            &'static mut Transform,
            &'static MeshMaterial2d<PlumeMaterial>,
        ),
        (Without<FlightPlanetQuad>, Without<FlightPartVisual>),
    >,
    hud_q: Query<'w, 's, &'static mut Text, With<FlightHudText>>,
}

pub struct FlightPlugin;

// ---------- DECLARATIONS / IMPLEMENTATION

fn enter_flight(
    mut commands: Commands,
    active_bp: Res<ActiveBlueprint>,
    catalog: Res<LivePartCatalog>,
    mut clock: ResMut<FlightClock>,
    mut origin: ResMut<FloatingOrigin>,
    mut assets: FlightAssets,
) {
    clock.warp_index = 0;
    clock.paused = false;

    let world = match SimWorld::with_launchpad_vessel(&active_bp.blueprint, &catalog.sim_catalog) {
        Ok(w) => w,
        Err(err) => {
            error!("Failed to initialize flight simulation: {err}");
            SimWorld::new()
        }
    };

    if let Some(active) = world.active_vessel() {
        origin.origin_m = active.pos;
    }

    let quad_mesh = assets.meshes.add(Rectangle::new(1.0, 1.0));
    let atmo_mat = assets.atmo_mats.add(AtmosphereMaterial {
        planet_params: Vec4::new(0.0, 600_000.0, 0.0, 600_000.0),
        atmo_params: Vec4::new(5_600.0, 70_000.0, 0.0, 4_000_000.0),
        surface_color: Vec4::new(0.10, 0.16, 0.24, 1.0),
        atmo_color: Vec4::new(0.22, 0.62, 1.00, 1.0),
    });
    commands.spawn((
        FlightEntity,
        FlightPlanetQuad,
        Mesh2d(quad_mesh),
        MeshMaterial2d(atmo_mat),
        Transform::from_xyz(0.0, 0.0, -10.0).with_scale(Vec3::new(4_000_000.0, 4_000_000.0, 1.0)),
    ));

    for vessel in &world.vessels {
        spawn_vessel_visuals(&mut commands, vessel, &catalog, *origin, &mut assets);
    }

    spawn_flight_hud(&mut commands);

    let pred = world
        .active_vessel_id
        .map(|id| world.predict_trajectory(id, PredictionOptions::default()))
        .unwrap_or_default();

    commands.insert_resource(FlightSimState {
        world,
        throttle: 1.0,
        sas_enabled: true,
        last_prediction: pred,
    });
}

fn exit_flight(mut commands: Commands, query: Query<Entity, With<FlightEntity>>) {
    for e in &query {
        commands.entity(e).despawn();
    }
    commands.remove_resource::<FlightSimState>();
}

fn spawn_flight_hud(commands: &mut Commands) {
    commands
        .spawn((
            FlightEntity,
            Node {
                position_type: PositionType::Absolute,
                left: px(14.0),
                top: px(14.0),
                width: px(360.0),
                padding: UiRect::all(px(12.0)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(CornerRadius::circular(px(6.0))),
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.06, 0.11, 0.86)),
            uniform_border(Color::srgba(0.20, 0.72, 0.94, 0.55)),
        ))
        .with_children(|parent| {
            parent.spawn((
                FlightHudText,
                Text::new("Initializing flight telemetry..."),
                TextFont::from_font_size(13.0),
                TextColor(Color::srgb(0.88, 0.96, 1.0)),
            ));
        });
}

fn spawn_vessel_visuals(
    commands: &mut Commands,
    vessel: &Vessel,
    catalog: &LivePartCatalog,
    origin: FloatingOrigin,
    assets: &mut FlightAssets,
) {
    for part in &vessel.parts {
        let style = catalog
            .visuals
            .get(&part.spec.id)
            .copied()
            .unwrap_or_default();
        let mesh = assets.meshes.add(build_part_mesh(&part.spec.shape));
        let fuel_cap = part.spec.fuel_capacity_kg();
        let fuel_frac = if fuel_cap > 0.0 {
            (part.fuel_kg / fuel_cap) as f32
        } else {
            1.0
        };
        let mat = assets.part_mats.add(PartMaterial {
            base_color: Vec4::from_array(style.base_color),
            accent_color: Vec4::from_array(style.accent_color),
            flags: Vec4::new(style.emissive_strength, 0.0, fuel_frac, 0.0),
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
            },
            Mesh2d(mesh),
            MeshMaterial2d(mat),
            Transform::from_xyz(pos_2d.x, pos_2d.y, if vessel.is_debris { -1.0 } else { 0.0 })
                .with_rotation(Quat::from_rotation_z(angle)),
        ));

        if matches!(part.spec.kind, PartKind::Engine(_)) {
            let bell_w = part.spec.shape.bounding_size().x as f32 * 0.9;
            let plume_len = bell_w * 4.5;
            let plume_mesh = assets.meshes.add(build_plume_quad(bell_w, plume_len));
            let plume_mat = assets.plume_mats.add(PlumeMaterial {
                params: Vec4::new(0.0, 0.0, 0.5, part.instance_id as f32),
                core_color: Vec4::new(0.95, 0.98, 1.0, 1.0),
                outer_color: Vec4::new(1.0, 0.45, 0.10, 1.0),
            });
            commands.spawn((
                FlightEntity,
                FlightPlumeVisual {
                    vessel_id: vessel.id,
                    instance_id: part.instance_id,
                },
                Mesh2d(plume_mesh),
                MeshMaterial2d(plume_mat),
                Transform::from_xyz(pos_2d.x, pos_2d.y, 1.0)
                    .with_rotation(Quat::from_rotation_z(angle)),
            ));
        }
    }
}

fn step_flight_controls_and_physics(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut next_mode: ResMut<NextState<GameMode>>,
    mut clock: ResMut<FlightClock>,
    mut flight: ResMut<FlightSimState>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        next_mode.set(GameMode::Editor);
        return;
    }

    let dt_real = f64::from(time.delta_secs()).min(0.05);

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

    if keys.just_pressed(KeyCode::Period) {
        clock.warp_index = (clock.warp_index + 1).min(TIME_WARP_LEVELS.len() - 1);
    }
    if keys.just_pressed(KeyCode::Comma) {
        clock.warp_index = clock.warp_index.saturating_sub(1);
    }
    if keys.just_pressed(KeyCode::KeyP) {
        clock.paused = !clock.paused;
    }

    let mut steer = 0.0;
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        steer += 1.0;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        steer -= 1.0;
    }

    if keys.just_pressed(KeyCode::Space) {
        let _ = flight.world.trigger_active_stage();
    }

    let throttle = flight.throttle;
    let sas_enabled = flight.sas_enabled;
    if let Some(active) = flight.world.active_vessel_mut() {
        active.control.throttle = throttle;
        active.control.steer = steer;
        active.control.sas_enabled = sas_enabled;
    }

    let sim_dt = dt_real * clock.warp_factor();
    if sim_dt > 0.0 {
        flight.world.step(sim_dt);
    }

    flight.last_prediction = flight
        .world
        .active_vessel_id
        .map(|id| flight.world.predict_trajectory(id, PredictionOptions::default()))
        .unwrap_or_default();
}

fn spawn_new_debris_visuals(
    mut commands: Commands,
    catalog: Res<LivePartCatalog>,
    origin: Res<FloatingOrigin>,
    flight: Res<FlightSimState>,
    existing_parts: Query<&FlightPartVisual>,
    mut assets: FlightAssets,
) {
    let spawned_vessels: std::collections::HashSet<VesselId> =
        existing_parts.iter().map(|v| v.vessel_id).collect();
    for vessel in &flight.world.vessels {
        if !spawned_vessels.contains(&vessel.id) {
            spawn_vessel_visuals(&mut commands, vessel, &catalog, *origin, &mut assets);
        }
    }
}

fn sync_flight_visuals_and_hud(
    time: Res<Time>,
    flight: Res<FlightSimState>,
    clock: Res<FlightClock>,
    zoom: Res<CameraZoomState>,
    mut origin: ResMut<FloatingOrigin>,
    mut commands: Commands,
    mut gizmos: Gizmos,
    mut assets: FlightAssets,
    mut queries: FlightRenderQueries,
) {
    if let Some(active) = flight.world.active_vessel() {
        origin.origin_m = active.pos + zoom.flight_pan_m;
    }

    let elapsed = time.elapsed_secs();

    if let Some(planet) = flight.world.bodies.first()
        && let Ok((mut tf, mat_handle)) = queries.planet_q.single_mut()
    {
        let quad_span = (zoom.log_scale.exp() * 3000.0).clamp(600.0, 20_000_000.0);
        tf.translation = Vec3::new(0.0, 0.0, -10.0);
        tf.scale = Vec3::new(quad_span, quad_span, 1.0);

        let u_rel = origin.origin_m - planet.pos;
        let r_cam = u_rel.length();
        let h_origin = r_cam - planet.radius_m;
        if let Some(mut mat) = assets.atmo_mats.get_mut(&mat_handle.0) {
            mat.planet_params = Vec4::new(
                u_rel.x as f32,
                u_rel.y as f32,
                h_origin as f32,
                planet.radius_m as f32,
            );
            if let Some(atmo) = planet.atmosphere {
                mat.atmo_params = Vec4::new(
                    atmo.scale_height_m as f32,
                    atmo.max_altitude_m as f32,
                    elapsed,
                    quad_span * 0.5,
                );
            }
        }
    }

    let mut part_lookup: HashMap<(VesselId, u64), (Vec2, f32, f32, f32)> = HashMap::new();
    for vessel in &flight.world.vessels {
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
                (render_p, rot, part.fuel_kg as f32, throttle),
            );
        }
    }

    for (entity, vis, mut tf, mat_h) in &mut queries.part_q {
        let Some(&(pos, rot, fuel_kg, _)) = part_lookup.get(&(vis.vessel_id, vis.instance_id))
        else {
            commands.entity(entity).despawn();
            continue;
        };
        tf.translation.x = pos.x;
        tf.translation.y = pos.y;
        tf.rotation = Quat::from_rotation_z(rot);
        if let Some(mut mat) = assets.part_mats.get_mut(&mat_h.0)
            && vis.fuel_cap_kg > 0.0
        {
            mat.flags.z = (fuel_kg / vis.fuel_cap_kg as f32).clamp(0.0, 1.0);
        }
    }

    for (entity, vis, mut tf, mat_h) in &mut queries.plume_q {
        let Some(&(pos, rot, _, throttle)) = part_lookup.get(&(vis.vessel_id, vis.instance_id))
        else {
            commands.entity(entity).despawn();
            continue;
        };
        let nozzle_exit = pos + Vec2::from_angle(rot).rotate(Vec2::new(0.0, -0.45));
        tf.translation.x = nozzle_exit.x;
        tf.translation.y = nozzle_exit.y;
        tf.rotation = Quat::from_rotation_z(rot);
        if let Some(mut mat) = assets.plume_mats.get_mut(&mat_h.0) {
            mat.params.x = throttle;
            mat.params.y = elapsed;
        }
    }

    draw_trajectory_gizmos(
        &flight.last_prediction,
        *origin,
        zoom.log_scale.exp(),
        &mut gizmos,
    );

    if let Ok(mut text) = queries.hud_q.single_mut() {
        *text = Text::new(format_flight_hud(&flight, &clock));
    }
}

fn draw_trajectory_gizmos(
    pred: &TrajectoryPrediction,
    origin: FloatingOrigin,
    ortho_scale: f32,
    gizmos: &mut Gizmos,
) {
    if pred.points_world.len() >= 2 {
        let path_color = Color::srgba(0.24, 0.88, 1.0, 0.85);
        for pair in pred.points_world.windows(2) {
            if let [a, b] = pair {
                let pa = world_to_render_vec2(*a, origin);
                let pb = world_to_render_vec2(*b, origin);
                gizmos.line_2d(pa, pb, path_color);
            }
        }
    }

    let marker_radius = (8.0 * ortho_scale).max(0.4);
    for m in &pred.markers {
        let p = world_to_render_vec2(m.world_pos, origin);
        let color = match m.kind {
            ApsisKind::Apoapsis => Color::srgb(0.25, 0.98, 0.60),
            ApsisKind::Periapsis => Color::srgb(0.35, 0.72, 1.00),
            ApsisKind::Impact => Color::srgb(1.00, 0.28, 0.24),
        };
        gizmos.circle_2d(p, marker_radius, color);
    }
}

fn format_flight_hud(flight: &FlightSimState, clock: &FlightClock) -> String {
    let Some(v) = flight.world.active_vessel() else {
        return String::from("No active vessel");
    };
    let elem = flight.world.vessel_orbit_elements(v.id);
    let (alt_km, speed, ap_km, pe_km, ecc, status) = match elem {
        Some(el) => {
            let st = if v.crashed {
                "CRASHED"
            } else if v.landed {
                "LANDED"
            } else if el.bound && el.periapsis_alt_m > 70_000.0 {
                "STABLE ORBIT"
            } else if el.bound {
                "SUBORBITAL"
            } else {
                "ESCAPE TRAJECTORY"
            };
            (
                el.altitude_m / 1000.0,
                el.speed_m_s,
                el.apoapsis_alt_m / 1000.0,
                el.periapsis_alt_m / 1000.0,
                el.eccentricity,
                st,
            )
        }
        None => (0.0, 0.0, 0.0, 0.0, 0.0, "DEEP SPACE"),
    };

    let fuel_kg = v.total_fuel_kg();
    let cap_kg = v.total_fuel_capacity_kg().max(1.0);
    let fuel_pct = (fuel_kg / cap_kg) * 100.0;

    format!(
        "KEPLER FLIGHT TELEMETRY\n\
         Status: {}   MET: {:.1} s\n\
         Altitude: {:.2} km   Speed: {:.1} m/s\n\
         Apoapsis: {:.2} km   Periapsis: {:.2} km\n\
         Eccentricity: {:.4}\n\
         Throttle: {:.0}%   SAS: {}   Stage: {}/{}\n\
         Propellant: {:.0} kg ({:.0}%)\n\
         Mass: {:.0} kg   Vessels/Debris: {}\n\
         Warp: {}x{}\n\n\
         [Space] Stage   [Shift/Ctrl/Z/X] Throttle\n\
         [A/D] Steer   [T] SAS   [./,] Warp   [P] Pause\n\
         [Wheel] Zoom   [R-Drag] Pan   [C] Recenter\n\
         [Esc] Return to VAB Editor",
        status,
        flight.world.time_s,
        alt_km,
        speed,
        ap_km,
        pe_km,
        ecc,
        flight.throttle * 100.0,
        if flight.sas_enabled { "ON" } else { "OFF" },
        v.current_stage,
        v.max_stage + 1,
        fuel_kg,
        fuel_pct,
        v.total_mass_kg,
        flight.world.vessels.len(),
        clock.warp_factor(),
        if clock.paused { " (PAUSED)" } else { "" },
    )
}

impl Plugin for FlightPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameMode::Flight), enter_flight)
            .add_systems(OnExit(GameMode::Flight), exit_flight)
            .add_systems(
                Update,
                (
                    step_flight_controls_and_physics,
                    spawn_new_debris_visuals,
                    sync_flight_visuals_and_hud,
                )
                    .chain()
                    .run_if(in_state(GameMode::Flight)),
            );
    }
}

// ---------- EXPORTS
