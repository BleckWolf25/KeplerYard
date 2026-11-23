//! @file sim.rs
//!
//! @version 1.5.1
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Full N-body symplectic simulator with ablative shielding, rotational contacts, and transfers
//!
//! @description
//! Implements `SimWorld` with fixed 1/120 s symplectic Velocity Verlet substeps under thrust,
//! atmospheric flight, or contact, adaptive coast steps during time warp, barycentric Kepler Prime
//! and Moon gravitation, aerodynamic fin lift and nosecone drag reduction, convective reentry
//! heating with ablator burnoff and lateral wake occlusion, rotational rigid-body surface contacts
//! with part-level impact breakup, and trajectory + maneuver transfer prediction.
//!
//! @since 21/11/2026
//! @updated 22/11/2026
// The project header uses bare author metadata rather than doc-markdown code formatting.
#![allow(clippy::doc_markdown)]

// ---------- IMPORTS
use crate::body::{
    CelestialBody, CelestialCatalog, OrbitElements, dominant_body, dominant_body_elements,
};
use crate::math::{DVec2, GRAV_CONSTANT, TAU_F64};
use crate::parts::{PartKind, VesselBlueprint};
use crate::vessel::{AMBIENT_TEMP_K, SasMode, SimError, StagingOutcome, Vessel, VesselId};
use std::collections::HashMap;

// ---------- CONSTANTS
/// Nominal fixed physics substep duration (1/120 s).
pub const FIXED_SUBSTEP_DT: f64 = 1.0 / 120.0;

/// Maximum coast substep duration during high time warp with zero thrust (seconds).
pub const MAX_COAST_SUBSTEP_DT: f64 = 1.0;

/// Scaled Sutton-Graves convective reentry heating constant.
const REENTRY_HEAT_COEFF: f64 = 6.2e-4;

/// Effective radiative cooling constant for thin hull / thermal tile dissipation.
const RADIATIVE_COOL_COEFF: f64 = 5.5e-8;

/// Effective latent heat of ablation for heat shield material (J / kg).
const ABLATION_HEAT_J_PER_KG: f64 = 450_000.0;

// ---------- TYPES

/// Simulation configuration toggles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SimConfig {
    pub fixed_dt: f64,
    pub max_coast_dt: f64,
    pub atmosphere_drag: bool,
    pub reentry_heating: bool,
    pub mutual_n_body: bool,
}

/// Visual/audio telemetry event emitted during a simulation step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SimStepEvent {
    PartDestroyed { world_pos: DVec2, thermal: bool },
    VesselCrashed { world_pos: DVec2 },
    Touchdown { world_pos: DVec2, speed_m_s: f64 },
    ChuteDeployed { world_pos: DVec2 },
}

/// Complete deterministic world state containing celestial bodies and all vessels/debris.
#[derive(Clone, Debug, PartialEq)]
pub struct SimWorld {
    pub time_s: f64,
    pub bodies: Vec<CelestialBody>,
    pub vessels: Vec<Vessel>,
    pub active_vessel_id: Option<VesselId>,
    pub next_vessel_id: VesselId,
    pub config: SimConfig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApsisKind {
    Apoapsis,
    Periapsis,
    Impact,
    MoonEncounter,
    ClosestApproach,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ApsisMarker {
    pub kind: ApsisKind,
    pub world_pos: DVec2,
    pub altitude_m: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PredictionOptions {
    pub max_steps: usize,
    pub max_horizon_s: f64,
}

/// Optional impulsive maneuver plan evaluated in transfer prediction.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ManeuverPlan {
    pub enabled: bool,
    pub lead_time_s: f64,
    pub prograde_dv_m_s: f64,
    pub radial_dv_m_s: f64,
}

/// Forward-propagated trajectory samples, planned transfer arc, and encounter markers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrajectoryPrediction {
    pub points_world: Vec<DVec2>,
    pub markers: Vec<ApsisMarker>,
    pub closest_moon_dist_m: Option<f64>,
    pub closest_moon_rel_speed_m_s: Option<f64>,
    pub moon_ghost_pos: Option<DVec2>,
    pub planned_points_world: Vec<DVec2>,
    pub planned_markers: Vec<ApsisMarker>,
    pub planned_closest_moon_dist_m: Option<f64>,
    pub planned_moon_ghost_pos: Option<DVec2>,
    pub maneuver_world_pos: Option<DVec2>,
}

/// Lightweight point-mass state for trajectory propagation.
#[derive(Clone, Copy, Debug)]
struct BodyPoint {
    mass_kg: f64,
    radius_m: f64,
    pos: DVec2,
    vel: DVec2,
}

// ---------- DECLARATIONS / IMPLEMENTATION
impl Default for SimConfig {
    fn default() -> Self {
        Self {
            fixed_dt: FIXED_SUBSTEP_DT,
            max_coast_dt: MAX_COAST_SUBSTEP_DT,
            atmosphere_drag: true,
            reentry_heating: true,
            mutual_n_body: true,
        }
    }
}

impl Default for PredictionOptions {
    fn default() -> Self {
        Self {
            max_steps: 1_500,
            max_horizon_s: 28_000.0,
        }
    }
}

impl Default for SimWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl SimWorld {
    #[must_use]
    pub fn new() -> Self {
        Self {
            time_s: 0.0,
            bodies: Vec::new(),
            vessels: Vec::new(),
            active_vessel_id: None,
            next_vessel_id: 1,
            config: SimConfig::default(),
        }
    }

    /// Creates a single-body launchpad sandbox around Kepler Prime (without the Moon).
    ///
    /// # Errors
    ///
    /// Returns [`SimError`] if the blueprint cannot be assembled into a valid vessel.
    pub fn with_launchpad_vessel(
        bp: &VesselBlueprint,
        catalog: &crate::parts::PartCatalog,
    ) -> Result<Self, SimError> {
        let mut world = Self::new();
        let planet = CelestialBody::kepler_prime();
        let mut vessel = Vessel::from_blueprint(world.next_vessel_id, bp, catalog)?;
        world.next_vessel_id += 1;
        vessel.place_on_north_pole_with_vel(planet.pos, planet.vel, planet.radius_m);
        let active_id = vessel.id;
        world.bodies.push(planet);
        world.vessels.push(vessel);
        world.active_vessel_id = Some(active_id);
        Ok(world)
    }

    /// Creates the barycentric two-body Kepler system (Kepler Prime + Moon) with
    /// the vessel placed upright on Kepler Prime's launchpad.
    ///
    /// # Errors
    ///
    /// Returns [`SimError`] if the blueprint cannot be assembled into a valid vessel.
    pub fn with_kepler_system_vessel(
        bp: &VesselBlueprint,
        catalog: &crate::parts::PartCatalog,
    ) -> Result<Self, SimError> {
        let mut world = Self::new();
        let catalog_bodies = CelestialCatalog::kepler_moon().build_system();
        let [planet, moon]: [CelestialBody; 2] = catalog_bodies
            .try_into()
            .map_err(|_| SimError::EmptyBlueprint)?;
        let mut vessel = Vessel::from_blueprint(world.next_vessel_id, bp, catalog)?;
        world.next_vessel_id += 1;
        vessel.place_on_north_pole_with_vel(planet.pos, planet.vel, planet.radius_m);
        let active_id = vessel.id;
        world.bodies.push(planet);
        world.bodies.push(moon);
        world.vessels.push(vessel);
        world.active_vessel_id = Some(active_id);
        Ok(world)
    }

    /// Creates the Earth-Moon system with the vessel placed upright on Earth's north pole.
    ///
    /// # Errors
    ///
    /// Returns [`SimError`] if the blueprint cannot be assembled into a valid vessel.
    pub fn with_earth_moon_vessel(
        bp: &VesselBlueprint,
        catalog: &crate::parts::PartCatalog,
    ) -> Result<Self, SimError> {
        let mut world = Self::new();
        let catalog_bodies = CelestialCatalog::earth_moon().build_system();
        let [earth, moon]: [CelestialBody; 2] = catalog_bodies
            .try_into()
            .map_err(|_| SimError::EmptyBlueprint)?;
        let mut vessel = Vessel::from_blueprint(world.next_vessel_id, bp, catalog)?;
        world.next_vessel_id += 1;
        vessel.place_on_north_pole_with_vel(earth.pos, earth.vel, earth.radius_m);
        let active_id = vessel.id;
        world.bodies.push(earth);
        world.bodies.push(moon);
        world.vessels.push(vessel);
        world.active_vessel_id = Some(active_id);
        Ok(world)
    }

    #[must_use]
    pub fn active_vessel(&self) -> Option<&Vessel> {
        let id = self.active_vessel_id?;
        self.vessels.iter().find(|v| v.id == id)
    }

    pub fn active_vessel_mut(&mut self) -> Option<&mut Vessel> {
        let id = self.active_vessel_id?;
        self.vessels.iter_mut().find(|v| v.id == id)
    }

    /// Cycles focus to the next controllable vessel, returning the newly focused vessel ID.
    pub fn cycle_active_vessel(&mut self) -> Option<VesselId> {
        let controllable: Vec<VesselId> = self
            .vessels
            .iter()
            .filter(|v| v.is_controllable())
            .map(|v| v.id)
            .collect();
        if controllable.len() <= 1 {
            return self.active_vessel_id;
        }
        let curr_pos = self
            .active_vessel_id
            .and_then(|id| controllable.iter().position(|&cid| cid == id))
            .unwrap_or(0);
        let next_id = controllable[(curr_pos + 1) % controllable.len()];
        self.active_vessel_id = Some(next_id);
        Some(next_id)
    }

    /// Returns `true` if any active vessel requires fine physical substeps (`1/120 s`).
    #[must_use]
    pub fn requires_physical_warp(&self) -> bool {
        self.vessels.iter().any(|v| {
            let thrusting = !v.is_debris
                && !v.crashed
                && v.parts.iter().any(|p| {
                    p.engine_active
                        && match p.spec.kind {
                            PartKind::Engine(_) => v.control.throttle > 0.0,
                            PartKind::SolidBooster(_) => p.fuel_kg > 0.0,
                            _ => false,
                        }
                });
            // Debris that has already come to rest cannot affect the active craft and must not pin
            // the whole simulation to physical warp.
            let settled_debris = v.is_debris && v.landed;
            let in_atmo_or_contact = !v.crashed
                && !settled_debris
                && self.bodies.iter().any(|b| {
                    let alt = v.pos.distance(b.pos) - b.radius_m;
                    let atmo_top = b.atmosphere.map_or(200.0, |a| a.max_altitude_m + 500.0);
                    alt < atmo_top
                });
            let steering =
                !v.is_debris && (v.control.steer.abs() > 1.0e-4 || v.angular_vel.abs() > 0.05);
            thrusting || in_atmo_or_contact || steering || (v.landed && !settled_debris)
        })
    }

    pub fn trigger_active_stage(&mut self) -> Option<StagingOutcome> {
        let active_id = self.active_vessel_id?;
        let idx = self.vessels.iter().position(|v| v.id == active_id)?;
        let mut next_id = self.next_vessel_id;
        let outcome = self.vessels.get_mut(idx)?.activate_next_stage(&mut next_id);
        self.next_vessel_id = next_id;
        for debris in &outcome.spawned_debris {
            self.vessels.push(debris.clone());
        }
        Some(outcome)
    }

    /// Computes desired SAS target angle for the given vessel given its orbital velocity and plan.
    #[must_use]
    pub fn compute_sas_target_angle(
        &self,
        vessel: &Vessel,
        plan: Option<ManeuverPlan>,
    ) -> Option<f64> {
        if !vessel.control.sas_enabled {
            return None;
        }
        let body = dominant_body(&self.bodies, vessel.pos)?;
        let rel_vel = vessel.vel - body.vel;
        let rel_pos = vessel.pos - body.pos;
        match vessel.control.sas_mode {
            SasMode::Prograde if rel_vel.length_squared() > 1.0 => {
                Some((-rel_vel.x).atan2(rel_vel.y))
            }
            SasMode::Retrograde if rel_vel.length_squared() > 1.0 => {
                Some(rel_vel.x.atan2(-rel_vel.y))
            }
            SasMode::RadialOut if rel_pos.length_squared() > 1.0 => {
                Some((-rel_pos.x).atan2(rel_pos.y))
            }
            SasMode::Maneuver => {
                let m = plan?;
                if !m.enabled || self.bodies.is_empty() {
                    return None;
                }
                let p_vel = self.bodies.first().map_or(DVec2::ZERO, |b| b.vel);
                let p_pos = self.bodies.first().map_or(DVec2::ZERO, |b| b.pos);
                let prograde = (vessel.vel - p_vel).normalize_or_zero();
                let radial_out = (vessel.pos - p_pos).normalize_or_zero();
                let dv_vec = prograde * m.prograde_dv_m_s + radial_out * m.radial_dv_m_s;
                (dv_vec.length_squared() > 1.0e-4).then(|| (-dv_vec.x).atan2(dv_vec.y))
            }
            _ => None,
        }
    }

    /// Advances the simulation by `total_dt` seconds and collects significant physics events.
    #[allow(clippy::while_float)]
    pub fn step_with_events(
        &mut self,
        total_dt: f64,
        maneuver: Option<ManeuverPlan>,
    ) -> Vec<SimStepEvent> {
        let mut events = Vec::new();
        if total_dt <= 0.0 || !total_dt.is_finite() {
            return events;
        }

        let substep_limit = if self.requires_physical_warp() {
            self.config.fixed_dt
        } else {
            self.config.max_coast_dt.max(self.config.fixed_dt)
        };

        let mut remaining = total_dt;
        while remaining > 1.0e-12 {
            let dt = remaining.min(substep_limit);
            self.step_verlet_with_events(dt, maneuver, &mut events);
            remaining -= dt;
        }
        events
    }

    /// Advances the simulation by an exact whole number of fixed ticks.
    ///
    /// While [`Self::requires_physical_warp`] is true every substep is exactly `fixed_dt`, so the
    /// result is bit-exact no matter how the ticks are split across calls (frame timing cannot
    /// change the trajectory). Otherwise ticks are grouped into coarse coast substeps of at most
    /// `max_coast_dt`, each an exact multiple of `fixed_dt`.
    pub fn step_ticks(&mut self, ticks: u32, maneuver: Option<ManeuverPlan>) -> Vec<SimStepEvent> {
        let mut events = Vec::new();
        let fixed_dt = self.config.fixed_dt;
        if ticks == 0 || fixed_dt <= 0.0 || !fixed_dt.is_finite() {
            return events;
        }
        let ticks_per_coarse = if self.requires_physical_warp() {
            1
        } else {
            ((self.config.max_coast_dt / fixed_dt).round() as u32).max(1)
        };
        let mut remaining = ticks;
        while remaining > 0 {
            let chunk = remaining.min(ticks_per_coarse);
            self.step_verlet_with_events(f64::from(chunk) * fixed_dt, maneuver, &mut events);
            remaining -= chunk;
        }
        events
    }

    /// Advances the simulation by `total_dt` seconds.
    pub fn step(&mut self, total_dt: f64) {
        let _ = self.step_with_events(total_dt, None);
    }

    /// Single synchronized symplectic Velocity Verlet / leapfrog substep of duration `dt`.
    pub fn step_verlet(&mut self, dt: f64) {
        let mut ignored = Vec::new();
        self.step_verlet_with_events(dt, None, &mut ignored);
    }

    fn step_verlet_with_events(
        &mut self,
        dt: f64,
        maneuver: Option<ManeuverPlan>,
        events: &mut Vec<SimStepEvent>,
    ) {
        if dt <= 0.0 {
            return;
        }

        let target_angles: Vec<Option<f64>> = self
            .vessels
            .iter()
            .map(|v| self.compute_sas_target_angle(v, maneuver))
            .collect();

        for (vessel, target) in self.vessels.iter_mut().zip(target_angles) {
            if !vessel.crashed {
                vessel.control.sas_target_angle = target;
                let pressure_pa = ambient_pressure_at(vessel.pos, &self.bodies);
                vessel.prepare_step_throttles_at_pressure(dt, pressure_pa);
            }
        }

        let (body_acc_0, vessel_acc_0, vessel_alpha_0) = self.compute_accelerations();

        let mut body_vhalf = Vec::with_capacity(self.bodies.len());
        for (i, body) in self.bodies.iter_mut().enumerate() {
            let a0 = body_acc_0.get(i).copied().unwrap_or(DVec2::ZERO);
            let vh = body.vel + a0 * (0.5 * dt);
            body.pos += vh * dt;
            body_vhalf.push(vh);
        }

        let mut vessel_velocity_halfstep = Vec::with_capacity(self.vessels.len());
        let mut vessel_angular_velocity_halfstep = Vec::with_capacity(self.vessels.len());
        for (i, vessel) in self.vessels.iter_mut().enumerate() {
            if vessel.crashed {
                vessel_velocity_halfstep.push(DVec2::ZERO);
                vessel_angular_velocity_halfstep.push(0.0);
                continue;
            }
            let a0 = vessel_acc_0.get(i).copied().unwrap_or(DVec2::ZERO);
            let alpha0 = vessel_alpha_0.get(i).copied().unwrap_or(0.0);
            let vh = vessel.vel + a0 * (0.5 * dt);
            let wh = vessel.angular_vel + alpha0 * (0.5 * dt);

            vessel.pos += vh * dt;
            vessel.angle += wh * dt;
            vessel.wrap_angle();
            let pressure_pa = ambient_pressure_at(vessel.pos, &self.bodies);
            vessel.consume_propellant_at_pressure(dt, pressure_pa);

            vessel_velocity_halfstep.push(vh);
            vessel_angular_velocity_halfstep.push(wh);
        }

        let (body_acc_1, vessel_acc_1, vessel_alpha_1) = self.compute_accelerations();

        for (i, body) in self.bodies.iter_mut().enumerate() {
            let vh = body_vhalf.get(i).copied().unwrap_or(body.vel);
            let a1 = body_acc_1.get(i).copied().unwrap_or(DVec2::ZERO);
            body.vel = vh + a1 * (0.5 * dt);
        }

        for (i, vessel) in self.vessels.iter_mut().enumerate() {
            if vessel.crashed {
                continue;
            }
            let vh = vessel_velocity_halfstep
                .get(i)
                .copied()
                .unwrap_or(vessel.vel);
            let wh = vessel_angular_velocity_halfstep
                .get(i)
                .copied()
                .unwrap_or(vessel.angular_vel);
            let a1 = vessel_acc_1.get(i).copied().unwrap_or(DVec2::ZERO);
            let alpha1 = vessel_alpha_1.get(i).copied().unwrap_or(0.0);
            vessel.vel = vh + a1 * (0.5 * dt);
            vessel.angular_vel = wh + alpha1 * (0.5 * dt);
        }

        self.update_parachutes_and_heating(dt, events);
        self.resolve_surface_contacts(events);
        self.time_s += dt;
    }

    #[must_use]
    pub fn total_mechanical_energy_j(&self) -> f64 {
        let mut ke = 0.0;
        for b in &self.bodies {
            ke += 0.5 * b.mass_kg * b.vel.length_squared();
        }
        for v in &self.vessels {
            ke += 0.5 * v.total_mass_kg * v.vel.length_squared();
            ke += 0.5 * v.moi_kg_m2 * v.angular_vel * v.angular_vel;
        }

        let mut pe = 0.0;
        for i in 0..self.bodies.len() {
            let Some(bi) = self.bodies.get(i) else {
                continue;
            };
            for j in (i + 1)..self.bodies.len() {
                let Some(bj) = self.bodies.get(j) else {
                    continue;
                };
                let r = bi.pos.distance(bj.pos).max(1.0);
                pe -= GRAV_CONSTANT * bi.mass_kg * bj.mass_kg / r;
            }
            for v in &self.vessels {
                let r = bi.pos.distance(v.pos).max(1.0);
                pe -= GRAV_CONSTANT * bi.mass_kg * v.total_mass_kg / r;
            }
        }
        if self.config.mutual_n_body {
            for i in 0..self.vessels.len() {
                let Some(vi) = self.vessels.get(i) else {
                    continue;
                };
                for j in (i + 1)..self.vessels.len() {
                    let Some(vj) = self.vessels.get(j) else {
                        continue;
                    };
                    let r = vi.pos.distance(vj.pos).max(1.0);
                    pe -= GRAV_CONSTANT * vi.total_mass_kg * vj.total_mass_kg / r;
                }
            }
        }

        ke + pe
    }

    #[must_use]
    pub fn total_linear_momentum(&self) -> DVec2 {
        let mut p = DVec2::ZERO;
        for b in &self.bodies {
            p += b.vel * b.mass_kg;
        }
        for v in &self.vessels {
            p += v.vel * v.total_mass_kg;
        }
        p
    }

    #[must_use]
    pub fn total_angular_momentum(&self) -> f64 {
        let mut l = 0.0;
        for b in &self.bodies {
            l += b.pos.cross(b.vel) * b.mass_kg;
        }
        for v in &self.vessels {
            l += v.pos.cross(v.vel) * v.total_mass_kg + v.moi_kg_m2 * v.angular_vel;
        }
        l
    }

    #[must_use]
    pub fn vessel_orbit_elements(&self, vessel_id: VesselId) -> Option<OrbitElements> {
        let vessel = self.vessels.iter().find(|v| v.id == vessel_id)?;
        dominant_body_elements(&self.bodies, vessel.pos, vessel.vel)
    }

    /// Predicts current coast trajectory.
    #[must_use]
    pub fn predict_trajectory(
        &self,
        vessel_id: VesselId,
        opts: PredictionOptions,
    ) -> TrajectoryPrediction {
        self.predict_trajectory_with_plan(vessel_id, opts, None)
    }

    /// Predicts both the current coast trajectory and an optional impulsive maneuver transfer trajectory,
    /// including Moon encounter / closest-approach diagnostics, maneuver node location, and ghost moon positions.
    #[must_use]
    pub fn predict_trajectory_with_plan(
        &self,
        vessel_id: VesselId,
        opts: PredictionOptions,
        plan: Option<ManeuverPlan>,
    ) -> TrajectoryPrediction {
        let mut result = self.propagate_coast_arc(vessel_id, opts, None);

        if let Some(m) = plan
            && m.enabled
        {
            let planned = self.propagate_coast_arc(vessel_id, opts, Some(m));
            result.planned_points_world = planned.points_world;
            result.planned_markers = planned.markers;
            result.planned_closest_moon_dist_m = planned.closest_moon_dist_m;
            result.planned_moon_ghost_pos = planned.moon_ghost_pos;
            result.maneuver_world_pos = planned.maneuver_world_pos;
        }

        result
    }

    #[allow(clippy::too_many_lines)]
    fn propagate_coast_arc(
        &self,
        vessel_id: VesselId,
        opts: PredictionOptions,
        plan: Option<ManeuverPlan>,
    ) -> TrajectoryPrediction {
        let Some(vessel) = self.vessels.iter().find(|v| v.id == vessel_id) else {
            return TrajectoryPrediction::default();
        };
        if self.bodies.is_empty() || vessel.crashed {
            return TrajectoryPrediction::default();
        }

        let mut bodies: Vec<BodyPoint> = self
            .bodies
            .iter()
            .map(|b| BodyPoint {
                mass_kg: b.mass_kg,
                radius_m: b.radius_m,
                pos: b.pos,
                vel: b.vel,
            })
            .collect();

        let moon_soi = if bodies.len() > 1 {
            self.bodies
                .get(1)
                .and_then(|m| self.bodies.first().map(|p| m.soi_radius_around(p)))
                .unwrap_or(400_000.0)
        } else {
            0.0
        };

        let mut v_pos = vessel.pos;
        let mut v_vel = vessel.vel;
        let mut burn_applied = plan.is_none_or(|p| !p.enabled);
        let mut maneuver_world_pos = None;

        let mut points = Vec::with_capacity(opts.max_steps.min(1024));
        points.push(v_pos);
        let mut markers = Vec::new();

        let primary_pos = bodies.first().map_or(DVec2::ZERO, |b| b.pos);
        let mut prev_dir = (v_pos - primary_pos).normalize_or_zero();
        let mut swept_angle = 0.0;

        let mut prev_alt_2: Option<(f64, DVec2)> = None;
        let mut prev_alt_1: Option<(f64, DVec2)> = None;
        let mut elapsed = 0.0;

        let mut best_moon_dist = f64::INFINITY;
        let mut best_moon_rel_speed = None;
        let mut best_moon_pos = None;
        let mut best_vessel_pos = None;
        let mut encountered_moon = false;

        for step_idx in 0..opts.max_steps {
            if elapsed >= opts.max_horizon_s {
                break;
            }

            if !burn_applied
                && let Some(m) = plan
                && elapsed >= m.lead_time_s
            {
                let p_pos = bodies.first().map_or(DVec2::ZERO, |b| b.pos);
                let p_vel = bodies.first().map_or(DVec2::ZERO, |b| b.vel);
                let rel_v = v_vel - p_vel;
                let prograde = rel_v.normalize_or_zero();
                let radial_out = (v_pos - p_pos).normalize_or_zero();
                v_vel += prograde * m.prograde_dv_m_s + radial_out * m.radial_dv_m_s;
                maneuver_world_pos = Some(v_pos);
                burn_applied = true;
                swept_angle = 0.0;
                prev_alt_2 = None;
                prev_alt_1 = None;
            }

            let dominant_r = bodies
                .iter()
                .map(|b| v_pos.distance(b.pos))
                .fold(f64::INFINITY, f64::min);
            let speed = v_vel.length().max(25.0);
            let mut dt = (dominant_r / speed * 0.018).clamp(0.4, 22.0);
            if !burn_applied
                && let Some(m) = plan
                && elapsed + dt > m.lead_time_s
                && m.lead_time_s > elapsed
            {
                dt = (m.lead_time_s - elapsed).max(0.05);
            }

            let (b_acc_0, v_acc_0) = compute_coast_accelerations(&bodies, v_pos);
            let mut b_vhalf = Vec::with_capacity(bodies.len());
            for (i, b) in bodies.iter_mut().enumerate() {
                let a0 = b_acc_0.get(i).copied().unwrap_or(DVec2::ZERO);
                let vh = b.vel + a0 * (0.5 * dt);
                b.pos += vh * dt;
                b_vhalf.push(vh);
            }
            let v_vhalf = v_vel + v_acc_0 * (0.5 * dt);
            v_pos += v_vhalf * dt;

            let (b_acc_1, v_acc_1) = compute_coast_accelerations(&bodies, v_pos);
            for (i, b) in bodies.iter_mut().enumerate() {
                let vh = b_vhalf.get(i).copied().unwrap_or(b.vel);
                let a1 = b_acc_1.get(i).copied().unwrap_or(DVec2::ZERO);
                b.vel = vh + a1 * (0.5 * dt);
            }
            v_vel = v_vhalf + v_acc_1 * (0.5 * dt);
            elapsed += dt;

            let mut impacted = false;
            for b in &bodies {
                let dist = v_pos.distance(b.pos);
                if dist <= b.radius_m {
                    let hit_pos = b.pos + (v_pos - b.pos).normalize_or_zero() * b.radius_m;
                    points.push(hit_pos);
                    markers.push(ApsisMarker {
                        kind: ApsisKind::Impact,
                        world_pos: hit_pos,
                        altitude_m: 0.0,
                    });
                    impacted = true;
                    break;
                }
            }
            if impacted {
                break;
            }

            points.push(v_pos);

            if let Some(moon) = bodies.get(1) {
                let d_moon = v_pos.distance(moon.pos);
                if d_moon < best_moon_dist {
                    best_moon_dist = d_moon;
                    best_moon_rel_speed = Some((v_vel - moon.vel).length());
                    best_moon_pos = Some(moon.pos);
                    best_vessel_pos = Some(v_pos);
                }
                if !encountered_moon && d_moon <= moon_soi && markers.len() < 8 {
                    encountered_moon = true;
                    markers.push(ApsisMarker {
                        kind: ApsisKind::MoonEncounter,
                        world_pos: v_pos,
                        altitude_m: d_moon - moon.radius_m,
                    });
                }
            }

            if let Some(primary) = bodies.first() {
                let rel = v_pos - primary.pos;
                let alt = rel.length() - primary.radius_m;
                if let (Some((a2, _)), Some((a1, p1))) = (prev_alt_2, prev_alt_1) {
                    if a1 > a2 && a1 > alt && markers.len() < 8 {
                        markers.push(ApsisMarker {
                            kind: ApsisKind::Apoapsis,
                            world_pos: p1,
                            altitude_m: a1,
                        });
                    } else if a1 < a2 && a1 < alt && markers.len() < 8 {
                        markers.push(ApsisMarker {
                            kind: ApsisKind::Periapsis,
                            world_pos: p1,
                            altitude_m: a1,
                        });
                    }
                }
                prev_alt_2 = prev_alt_1;
                prev_alt_1 = Some((alt, v_pos));

                let dir = rel.normalize_or_zero();
                let dtheta = prev_dir.cross(dir).atan2(prev_dir.dot(dir));
                swept_angle += dtheta;
                prev_dir = dir;
                if burn_applied
                    && !encountered_moon
                    && step_idx > 16
                    && swept_angle.abs() >= TAU_F64
                {
                    break;
                }
            }
        }

        if let (Some(moon), Some(vp)) = (bodies.get(1), best_vessel_pos)
            && best_moon_dist < 1_500_000.0
            && markers.len() < 10
        {
            markers.push(ApsisMarker {
                kind: ApsisKind::ClosestApproach,
                world_pos: vp,
                altitude_m: best_moon_dist - moon.radius_m,
            });
        }

        TrajectoryPrediction {
            points_world: points,
            markers,
            closest_moon_dist_m: if best_moon_dist.is_finite() {
                Some(best_moon_dist)
            } else {
                None
            },
            closest_moon_rel_speed_m_s: best_moon_rel_speed,
            moon_ghost_pos: best_moon_pos,
            planned_points_world: Vec::new(),
            planned_markers: Vec::new(),
            planned_closest_moon_dist_m: None,
            planned_moon_ghost_pos: None,
            maneuver_world_pos,
        }
    }

    fn compute_accelerations(&self) -> (Vec<DVec2>, Vec<DVec2>, Vec<f64>) {
        let mut body_acc = vec![DVec2::ZERO; self.bodies.len()];
        let mut vessel_acc = vec![DVec2::ZERO; self.vessels.len()];
        let mut vessel_alpha = vec![0.0; self.vessels.len()];

        for i in 0..self.bodies.len() {
            let Some(bi) = self.bodies.get(i) else {
                continue;
            };
            for j in (i + 1)..self.bodies.len() {
                let Some(bj) = self.bodies.get(j) else {
                    continue;
                };
                let d = bj.pos - bi.pos;
                let r2 = d.length_squared().max(1.0);
                let inv_r3 = 1.0 / (r2 * r2.sqrt());
                let f_over_g = d * inv_r3;
                if let Some(ai) = body_acc.get_mut(i) {
                    *ai += f_over_g * (GRAV_CONSTANT * bj.mass_kg);
                }
                if let Some(aj) = body_acc.get_mut(j) {
                    *aj -= f_over_g * (GRAV_CONSTANT * bi.mass_kg);
                }
            }
        }

        for (bi_idx, body) in self.bodies.iter().enumerate() {
            for (vi_idx, vessel) in self.vessels.iter().enumerate() {
                let d = body.pos - vessel.pos;
                let r2 = d.length_squared().max(1.0);
                let inv_r3 = 1.0 / (r2 * r2.sqrt());
                let g_vec = d * inv_r3;
                if let Some(va) = vessel_acc.get_mut(vi_idx) {
                    *va += g_vec * (GRAV_CONSTANT * body.mass_kg);
                }
                if self.config.mutual_n_body
                    && let Some(ba) = body_acc.get_mut(bi_idx)
                {
                    *ba -= g_vec * (GRAV_CONSTANT * vessel.total_mass_kg);
                }
            }
        }

        if self.config.mutual_n_body {
            for i in 0..self.vessels.len() {
                let Some(vi) = self.vessels.get(i) else {
                    continue;
                };
                for j in (i + 1)..self.vessels.len() {
                    let Some(vj) = self.vessels.get(j) else {
                        continue;
                    };
                    let d = vj.pos - vi.pos;
                    let r2 = d.length_squared().max(1.0);
                    let inv_r3 = 1.0 / (r2 * r2.sqrt());
                    let g_vec = d * inv_r3;
                    if let Some(ai) = vessel_acc.get_mut(i) {
                        *ai += g_vec * (GRAV_CONSTANT * vj.total_mass_kg);
                    }
                    if let Some(aj) = vessel_acc.get_mut(j) {
                        *aj -= g_vec * (GRAV_CONSTANT * vi.total_mass_kg);
                    }
                }
            }
        }

        for (vi_idx, vessel) in self.vessels.iter().enumerate() {
            if vessel.crashed {
                continue;
            }
            let pressure_pa = ambient_pressure_at(vessel.pos, &self.bodies);
            let (mut force, mut torque) =
                vessel.compute_thrust_and_control_wrench_at_pressure(pressure_pa);

            if self.config.atmosphere_drag {
                let (aero_f, aero_t) = compute_vessel_aerodynamics(vessel, &self.bodies);
                force += aero_f;
                torque += aero_t;
            }

            if let Some(va) = vessel_acc.get_mut(vi_idx) {
                *va += force / vessel.total_mass_kg.max(1.0e-6);
            }
            if let Some(valpha) = vessel_alpha.get_mut(vi_idx) {
                *valpha += torque / vessel.moi_kg_m2.max(1.0e-3);
            }
        }

        (body_acc, vessel_acc, vessel_alpha)
    }

    #[allow(clippy::too_many_lines)]
    fn update_parachutes_and_heating(&mut self, dt: f64, events: &mut Vec<SimStepEvent>) {
        for vessel in &mut self.vessels {
            if vessel.crashed {
                continue;
            }

            let mut max_rho = 0.0;
            let mut rel_vel = DVec2::ZERO;
            let mut min_alt = f64::INFINITY;
            let mut descending = false;
            let mut ambient_temperature_k = AMBIENT_TEMP_K;

            for body in &self.bodies {
                let alt = vessel.pos.distance(body.pos) - body.radius_m;
                min_alt = min_alt.min(alt);
                if let Some(atmo) = body.atmosphere {
                    let rho = atmo.density_at(alt);
                    if rho > max_rho {
                        max_rho = rho;
                        rel_vel = vessel.vel - body.vel;
                        let surface_normal = (vessel.pos - body.pos).normalize_or_zero();
                        descending = rel_vel.dot(surface_normal) < 0.0;
                        let state = atmo.state_at(alt);
                        if state.temperature_k > 0.0 {
                            ambient_temperature_k = state.temperature_k;
                        }
                    }
                }
            }

            let speed = rel_vel.length();
            for part in &mut vessel.parts {
                if matches!(part.spec.kind, PartKind::Parachute(_))
                    && part.chute_armed
                    && !part.chute_deployed
                    && !vessel.landed
                    && min_alt <= 5_500.0
                    && speed <= 320.0
                    && max_rho > 0.01
                    && descending
                {
                    part.chute_deployed = true;
                    events.push(SimStepEvent::ChuteDeployed {
                        world_pos: vessel.pos,
                    });
                } else if vessel.landed && part.chute_deployed {
                    part.chute_deployed = false;
                    part.chute_armed = false;
                }
            }

            if !self.config.reentry_heating {
                continue;
            }

            let flow_dir = if speed > 1.0e-3 {
                rel_vel / speed
            } else {
                DVec2::Y
            };
            let flow_perp = flow_dir.perp();

            let shields: Vec<(f64, f64, f64, f64)> = vessel
                .parts
                .iter()
                .filter_map(|p| match p.spec.kind {
                    PartKind::HeatShield(hs) => {
                        let world_p = (p.local_pos - vessel.com_local).rotate(vessel.angle);
                        let half_w = 0.5 * p.spec.shape.bounding_size().x;
                        let factor = if p.ablator_kg > 0.0 {
                            hs.wake_shield_factor
                        } else {
                            (hs.wake_shield_factor * 8.0).clamp(0.25, 0.55)
                        };
                        Some((
                            world_p.dot(flow_dir),
                            world_p.dot(flow_perp),
                            half_w,
                            factor,
                        ))
                    }
                    _ => None,
                })
                .collect();

            let base_flux = if descending && max_rho > 0.0 && speed > 450.0 {
                REENTRY_HEAT_COEFF * max_rho.sqrt() * speed.powi(3)
            } else {
                0.0
            };

            for part in &mut vessel.parts {
                let world_off = (part.local_pos - vessel.com_local).rotate(vessel.angle);
                let proj = world_off.dot(flow_dir);
                let lat = world_off.dot(flow_perp);

                let wake_factor = match part.spec.kind {
                    PartKind::HeatShield(_) => 1.0,
                    _ => shields
                        .iter()
                        .filter(|&&(sp, sl, shw, _)| {
                            proj < sp - 0.05 && (lat - sl).abs() <= shw + 0.20
                        })
                        .map(|&(_, _, _, f)| f)
                        .fold(1.0_f64, f64::min),
                };

                let size = part.spec.shape.bounding_size();
                let mut q_in = base_flux * wake_factor * size.x.max(0.5);
                part.heat_flux_w_m2 = q_in;

                if matches!(part.spec.kind, PartKind::HeatShield(_))
                    && part.ablator_kg > 0.0
                    && part.temperature_k > 700.0
                    && q_in > 0.0
                {
                    let burn_heat = q_in * 0.65;
                    let req_kg = (burn_heat / ABLATION_HEAT_J_PER_KG) * dt;
                    let burned_kg = req_kg.min(part.ablator_kg);
                    part.ablator_kg -= burned_kg;
                    let absorbed_w = (burned_kg * ABLATION_HEAT_J_PER_KG) / dt.max(1.0e-9);
                    q_in = (q_in - absorbed_w).max(0.0);
                }

                let thermal_mass = (part.spec.dry_mass_kg * 28.0).max(1_200.0);
                let q_cool = RADIATIVE_COOL_COEFF
                    * (part.temperature_k.powi(4) - ambient_temperature_k.powi(4))
                    * (size.x + size.y);
                let net_q = q_in - q_cool;
                part.temperature_k =
                    (part.temperature_k + (net_q / thermal_mass) * dt).max(ambient_temperature_k);
            }

            let was_crashed = vessel.crashed;
            let destroyed = vessel.apply_thermal_damage();
            if vessel.crashed && !was_crashed {
                events.push(SimStepEvent::VesselCrashed {
                    world_pos: vessel.pos,
                });
            } else if destroyed > 0 {
                events.push(SimStepEvent::PartDestroyed {
                    world_pos: vessel.pos,
                    thermal: true,
                });
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn resolve_surface_contacts(&mut self, events: &mut Vec<SimStepEvent>) {
        for vessel in &mut self.vessels {
            if vessel.crashed {
                continue;
            }
            let was_landed = vessel.landed;
            vessel.landed = false;

            for body in &self.bodies {
                let mut broken_part: Option<(u64, DVec2)> = None;
                for part in &vessel.parts {
                    let half = part.spec.shape.contact_half_extent();
                    let offsets = [
                        DVec2::new(-half.x, -half.y),
                        DVec2::new(half.x, -half.y),
                        DVec2::new(-half.x, half.y),
                        DVec2::new(half.x, half.y),
                    ];
                    for off in offsets {
                        let local_pt = part.local_pos + off.rotate(part.local_rot);
                        let r_world = (local_pt - vessel.com_local).rotate(vessel.angle);
                        let world_pt = vessel.pos + r_world;
                        let rel = world_pt - body.pos;
                        let dist = rel.length();
                        if dist < body.radius_m {
                            let normal = rel.normalize_or_zero();
                            let v_pt =
                                (vessel.vel - body.vel) + r_world.perp() * vessel.angular_vel;
                            let v_rad = v_pt.dot(normal);
                            if v_rad < -part.spec.max_impact_m_s {
                                broken_part = Some((part.instance_id, world_pt));
                                break;
                            }
                        }
                    }
                    if broken_part.is_some() {
                        break;
                    }
                }

                if let Some((part_id, impact_pos)) = broken_part {
                    vessel.destroy_part_subtree(part_id);
                    if vessel.crashed {
                        vessel.vel = body.vel;
                        vessel.angular_vel = 0.0;
                        events.push(SimStepEvent::VesselCrashed {
                            world_pos: impact_pos,
                        });
                        break;
                    }
                    events.push(SimStepEvent::PartDestroyed {
                        world_pos: impact_pos,
                        thermal: false,
                    });
                }

                let mut contacts: Vec<(DVec2, DVec2)> = Vec::new();
                let mut max_pen = 0.0;
                let mut main_normal = DVec2::Y;

                for part in &vessel.parts {
                    let half = part.spec.shape.contact_half_extent();
                    let offsets = [
                        DVec2::new(-half.x, -half.y),
                        DVec2::new(half.x, -half.y),
                        DVec2::new(-half.x, half.y),
                        DVec2::new(half.x, half.y),
                    ];
                    for off in offsets {
                        let local_pt = part.local_pos + off.rotate(part.local_rot);
                        let r_world = (local_pt - vessel.com_local).rotate(vessel.angle);
                        let world_pt = vessel.pos + r_world;
                        let rel = world_pt - body.pos;
                        let dist = rel.length();
                        let pen = body.radius_m - dist;
                        if pen > 0.0 {
                            let normal = rel.normalize_or_zero();
                            if pen > max_pen {
                                max_pen = pen;
                                main_normal = normal;
                            }
                            contacts.push((r_world, normal));
                        }
                    }
                }

                if max_pen > 0.0 {
                    let mass_kg = vessel.total_mass_kg.max(1.0e-6);
                    let moi = vessel.moi_kg_m2.max(1.0e-3);
                    let approach_speed = (vessel.vel - body.vel).length();
                    vessel.pos += main_normal * max_pen;

                    for _ in 0..2 {
                        for &(r_world, normal) in &contacts {
                            let v_pt =
                                (vessel.vel - body.vel) + r_world.perp() * vessel.angular_vel;
                            let v_n = v_pt.dot(normal);
                            if v_n < 0.0 {
                                let r_cn = r_world.cross(normal);
                                let inv_mass_n =
                                    1.0 / mass_kg + (r_cn * r_cn) / moi;
                                let j_n = -v_n / inv_mass_n.max(1.0e-9);
                                vessel.vel += normal * (j_n / mass_kg);
                                vessel.angular_vel += (r_cn * j_n) / moi;

                                let tangent = normal.perp();
                                let v_pt_after =
                                    (vessel.vel - body.vel) + r_world.perp() * vessel.angular_vel;
                                let tangential_speed = v_pt_after.dot(tangent);
                                let contact_radius_tangent_cross = r_world.cross(tangent);
                                let inv_mass_t = 1.0 / mass_kg
                                    + (contact_radius_tangent_cross * contact_radius_tangent_cross)
                                        / moi;
                                let max_fric = 0.65 * j_n;
                                let j_t = (-tangential_speed / inv_mass_t.max(1.0e-9))
                                    .clamp(-max_fric, max_fric);
                                vessel.vel += tangent * (j_t / mass_kg);
                                vessel.angular_vel +=
                                    (contact_radius_tangent_cross * j_t) / moi;
                            }
                        }
                    }

                    vessel.angular_vel *= 0.96;
                    if !was_landed && approach_speed > 1.8 {
                        events.push(SimStepEvent::Touchdown {
                            world_pos: vessel.pos,
                            speed_m_s: approach_speed,
                        });
                    }
                    vessel.landed = true;
                }
            }
        }
    }
}

fn compute_coast_accelerations(bodies: &[BodyPoint], v_pos: DVec2) -> (Vec<DVec2>, DVec2) {
    let mut b_acc = vec![DVec2::ZERO; bodies.len()];
    let mut v_acc = DVec2::ZERO;

    for i in 0..bodies.len() {
        let Some(bi) = bodies.get(i) else {
            continue;
        };
        for j in (i + 1)..bodies.len() {
            let Some(bj) = bodies.get(j) else {
                continue;
            };
            let d = bj.pos - bi.pos;
            let r2 = d.length_squared().max(1.0);
            let inv_r3 = 1.0 / (r2 * r2.sqrt());
            let g_vec = d * inv_r3;
            if let Some(ai) = b_acc.get_mut(i) {
                *ai += g_vec * (GRAV_CONSTANT * bj.mass_kg);
            }
            if let Some(aj) = b_acc.get_mut(j) {
                *aj -= g_vec * (GRAV_CONSTANT * bi.mass_kg);
            }
        }

        let dv = bi.pos - v_pos;
        let r2 = dv.length_squared().max(1.0);
        let inv_r3 = 1.0 / (r2 * r2.sqrt());
        v_acc += dv * (GRAV_CONSTANT * bi.mass_kg * inv_r3);
    }

    (b_acc, v_acc)
}

/// Returns the strongest nearby body's ideal-gas surface pressure at the vessel position.
fn ambient_pressure_at(position: DVec2, bodies: &[CelestialBody]) -> f64 {
    bodies
        .iter()
        .filter_map(|body| {
            let atmosphere = body.atmosphere?;
            let altitude_m = position.distance(body.pos) - body.radius_m;
            let state = atmosphere.state_at(altitude_m);
            (state.density_kg_m3 > 0.0).then_some(state.pressure_pa)
        })
        .fold(0.0, f64::max)
}

/// Computes exponential atmospheric drag, nosecone drag relief, fin lift, and parachute drag.
fn compute_vessel_aerodynamics(vessel: &Vessel, bodies: &[CelestialBody]) -> (DVec2, f64) {
    let mut aero_force = DVec2::ZERO;
    let mut aero_torque = 0.0;

    let fwd_world = DVec2::Y.rotate(vessel.angle);
    let right_world = DVec2::X.rotate(vessel.angle);

    let mut capped_drag_mult: HashMap<u64, f64> = HashMap::new();
    for p in &vessel.parts {
        if let PartKind::NoseCone(nc) = p.spec.kind
            && let Some(pid) = p.parent_id
        {
            capped_drag_mult.insert(pid, (1.0 - nc.drag_reduction).clamp(0.35, 1.0));
        }
    }

    for body in bodies {
        let Some(atmo) = body.atmosphere else {
            continue;
        };
        let alt = vessel.pos.distance(body.pos) - body.radius_m;
        let rho = atmo.density_at(alt);
        if rho <= 0.0 {
            continue;
        }
        let v_vessel_rel = vessel.vel - body.vel;
        if v_vessel_rel.length_squared() <= 1.0e-4 {
            continue;
        }

        for part in &vessel.parts {
            let r_world = (part.local_pos - vessel.com_local).rotate(vessel.angle);
            let v_part = v_vessel_rel + r_world.perp() * vessel.angular_vel;
            let speed = v_part.length();
            if speed <= 1.0e-3 {
                continue;
            }
            let v_hat = v_part / speed;
            let q_dyn = 0.5 * rho * speed * speed;

            let size = part.spec.shape.bounding_size();
            let sin_aoa = fwd_world.cross(v_hat);
            let cos_aoa = fwd_world.dot(v_hat);
            let cap_mult = capped_drag_mult
                .get(&part.instance_id)
                .copied()
                .unwrap_or(1.0);

            let eff_area = match part.spec.kind {
                PartKind::Parachute(chute) if part.chute_deployed => chute.deployed_drag_area_m2,
                PartKind::Fin(fin) => fin.lift_area_m2 * (0.15 + 0.85 * sin_aoa.abs()),
                _ => size.x * cos_aoa.abs() * cap_mult + size.y * sin_aoa.abs(),
            };

            let eff_cd = match part.spec.kind {
                PartKind::Parachute(_) if part.chute_deployed => 1.45,
                _ => part.spec.drag_coeff,
            };

            let mut part_force = -v_hat * (q_dyn * eff_cd * eff_area);

            if let PartKind::Fin(fin) = part.spec.kind {
                let lateral_speed = v_part.dot(right_world);
                let cl_lat = -2.8 * (lateral_speed / speed);
                part_force += right_world * (q_dyn * fin.lift_area_m2 * cl_lat);
            }

            aero_force += part_force;
            aero_torque += r_world.cross(part_force);
        }
    }

    (aero_force, aero_torque)
}

// ---------- TESTS

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::math::{G0, SplitMix64};
    use crate::parts::{
        AttachKind, default_part_catalog, lunar_capable_blueprint, starter_two_stage_blueprint,
    };

    fn propagate_circular_orbit(mu: f64, radius_m: f64, dt_s: f64, steps: usize) -> DVec2 {
        let mut pos = DVec2::new(radius_m, 0.0);
        let mut vel = DVec2::new(0.0, (mu / radius_m).sqrt());
        for _ in 0..steps {
            let r_sq = pos.length_squared().max(1.0);
            let acceleration = pos * (-mu / (r_sq * r_sq.sqrt()));
            pos += vel * dt_s + acceleration * (0.5 * dt_s * dt_s);
            let next_r_sq = pos.length_squared().max(1.0);
            let next_acceleration = pos * (-mu / (next_r_sq * next_r_sq.sqrt()));
            vel += (acceleration + next_acceleration) * (0.5 * dt_s);
        }
        pos
    }

    #[test]
    fn golden_elliptical_orbit_conserves_energy_and_angular_momentum() {
        let planet = CelestialBody::kepler_prime();
        let r_p = planet.radius_m + 100_000.0;
        let v_circ = (planet.mu() / r_p).sqrt();
        let v_p = v_circ * 1.15;

        let cat = default_part_catalog();
        let mut bp = VesselBlueprint::new("Probe");
        bp.add_root("command_pod", 0);
        let mut vessel = Vessel::from_blueprint(1, &bp, &cat).expect("valid probe");
        vessel.pos = DVec2::new(r_p, 0.0);
        vessel.vel = DVec2::new(0.0, v_p);

        let mut world = SimWorld::new();
        world.config.mutual_n_body = false;
        world.config.atmosphere_drag = false;
        world.config.reentry_heating = false;
        world.bodies.push(planet);
        world.vessels.push(vessel);
        world.active_vessel_id = Some(1);

        let elems0 = world
            .vessel_orbit_elements(1)
            .expect("orbit elements available");
        let period = elems0.period_s.expect("bound orbit has period");
        let e0 = world.total_mechanical_energy_j();
        let l0 = world.total_angular_momentum();

        let total_time = 5.0 * period;
        let dt = 0.5;
        let steps = (total_time / dt) as usize;
        for _ in 0..steps {
            world.step_verlet(dt);
        }

        let e1 = world.total_mechanical_energy_j();
        let l1 = world.total_angular_momentum();
        let relative_energy_error = (e1 - e0).abs() / e0.abs();
        let relative_angular_momentum_error = (l1 - l0).abs() / l0.abs();

        assert!(
            relative_energy_error < 2.0e-6,
            "relative energy error too high: {relative_energy_error}"
        );
        assert!(
            relative_angular_momentum_error < 1.0e-10,
            "angular momentum drift too high: {relative_angular_momentum_error}"
        );
    }

    #[test]
    fn three_body_mutual_gravity_conserves_energy_and_momenta() {
        let mut world = SimWorld::new();
        let planet = CelestialBody::kepler_prime();
        let moon = CelestialBody::moon(&planet);

        let cat = default_part_catalog();
        let bp = starter_two_stage_blueprint(&cat);
        let mut probe = Vessel::from_blueprint(1, &bp, &cat).expect("valid vessel");
        probe.pos = DVec2::new(0.0, 950_000.0);
        probe.vel = DVec2::new(-(planet.mu() / 950_000.0).sqrt(), 0.0);

        world.bodies.push(planet);
        world.bodies.push(moon);
        world.vessels.push(probe);

        let e0 = world.total_mechanical_energy_j();
        let p0 = world.total_linear_momentum();
        let l0 = world.total_angular_momentum();

        for _ in 0..2_000 {
            world.step_verlet(0.25);
        }

        let e1 = world.total_mechanical_energy_j();
        let p1 = world.total_linear_momentum();
        let l1 = world.total_angular_momentum();

        assert!((e1 - e0).abs() / e0.abs() < 1.0e-6);
        assert!(p0.distance(p1) / p0.length().max(1.0) < 1.0e-12);
        assert!((l1 - l0).abs() / l0.abs() < 1.0e-11);
    }

    #[test]
    fn zero_g_burn_matches_tsiolkovsky_rocket_equation() {
        let cat = default_part_catalog();
        let mut bp = VesselBlueprint::new("BurnTest");
        let pod = bp.add_root("command_pod", 0);
        let tank = bp.attach_child(
            pod,
            "tank_small",
            AttachKind::StackBottom,
            DVec2::new(0.0, -1.5),
            0.0,
            0,
        );
        bp.attach_child(
            tank,
            "engine_orbital",
            AttachKind::StackBottom,
            DVec2::new(0.0, -2.925),
            0.0,
            0,
        );

        let mut world = SimWorld::new();
        let mut vessel = Vessel::from_blueprint(1, &bp, &cat).expect("valid burn rocket");
        vessel.control.throttle = 1.0;
        let m0 = vessel.total_mass_kg;
        world.vessels.push(vessel);
        world.active_vessel_id = Some(1);
        world.trigger_active_stage();

        for _ in 0..(30 * 120) {
            world.step(1.0 / 120.0);
        }

        let v = world.active_vessel().expect("vessel exists");
        let m1 = v.total_mass_kg;
        let expected_dv = 340.0 * G0 * (m0 / m1).ln();
        let actual_dv = v.vel.length();
        assert!(
            (actual_dv - expected_dv).abs() < 1.0e-4,
            "Tsiolkovsky mismatch: expected {expected_dv}, got {actual_dv}"
        );
    }

    #[test]
    fn randomized_orbital_conservation_property_suite() {
        let mut rng = SplitMix64::new(0x5EED_CAFE_BABE_1234);
        let cat = default_part_catalog();
        let mut bp = VesselBlueprint::new("PropProbe");
        bp.add_root("command_pod", 0);

        for case in 0..32 {
            let planet = CelestialBody::kepler_prime();
            let r = planet.radius_m + rng.range_f64(80_000.0, 500_000.0);
            let angle = rng.range_f64(0.0, TAU_F64);
            let v_circ = (planet.mu() / r).sqrt();
            let speed_factor = rng.range_f64(0.85, 1.25);

            let pos = DVec2::new(r * angle.cos(), r * angle.sin());
            let tangent = DVec2::new(-angle.sin(), angle.cos());
            let vel = tangent * (v_circ * speed_factor);

            let mut vessel = Vessel::from_blueprint(1, &bp, &cat).expect("valid probe");
            vessel.pos = pos;
            vessel.vel = vel;

            let mut world = SimWorld::new();
            world.config.mutual_n_body = false;
            world.config.atmosphere_drag = false;
            world.config.reentry_heating = false;
            world.bodies.push(planet);
            world.vessels.push(vessel);

            let e0 = world.total_mechanical_energy_j();
            let l0 = world.total_angular_momentum();

            for _ in 0..800 {
                world.step_verlet(0.25);
            }

            let e1 = world.total_mechanical_energy_j();
            let l1 = world.total_angular_momentum();
            let rel_e = (e1 - e0).abs() / e0.abs();
            let rel_l = (l1 - l0).abs() / l0.abs().max(1.0);
            assert!(rel_e < 1.0e-5, "case {case} energy err {rel_e}");
            assert!(rel_l < 1.0e-10, "case {case} ang mom err {rel_l}");
        }
    }

    #[test]
    fn replay_determinism_is_bit_exact() {
        let cat = default_part_catalog();
        let bp = starter_two_stage_blueprint(&cat);

        let run_once = || -> (DVec2, DVec2, f64, f64) {
            let mut world =
                SimWorld::with_launchpad_vessel(&bp, &cat).expect("valid launchpad setup");
            world.trigger_active_stage();
            for tick in 0..1_200 {
                if let Some(v) = world.active_vessel_mut() {
                    v.control.throttle = 1.0;
                    v.control.steer = if (300..550).contains(&tick) {
                        -0.25
                    } else {
                        0.0
                    };
                }
                world.step(1.0 / 60.0);
            }
            let v = world.active_vessel().expect("active vessel");
            (v.pos, v.vel, v.angle, v.total_fuel_kg())
        };

        let run_a = run_once();
        let run_b = run_once();
        assert_eq!(run_a.0.x.to_bits(), run_b.0.x.to_bits());
        assert_eq!(run_a.0.y.to_bits(), run_b.0.y.to_bits());
        assert_eq!(run_a.1.x.to_bits(), run_b.1.x.to_bits());
        assert_eq!(run_a.1.y.to_bits(), run_b.1.y.to_bits());
        assert_eq!(run_a.2.to_bits(), run_b.2.to_bits());
        assert_eq!(run_a.3.to_bits(), run_b.3.to_bits());
    }

    #[test]
    fn trajectory_prediction_continues_after_long_lead_maneuver() {
        let [planet, moon] = CelestialBody::kepler_moon_system();
        let r_p = planet.radius_m + 100_000.0;
        let v_p = (planet.mu() / r_p).sqrt();

        let cat = default_part_catalog();
        let mut bp = VesselBlueprint::new("OrbitCheck");
        bp.add_root("command_pod", 0);
        let mut vessel = Vessel::from_blueprint(1, &bp, &cat).expect("valid probe");
        vessel.pos = planet.pos + DVec2::new(r_p, 0.0);
        vessel.vel = planet.vel + DVec2::new(0.0, v_p);

        let mut world = SimWorld::new();
        world.bodies.push(planet);
        world.bodies.push(moon);
        world.vessels.push(vessel);

        let plan = ManeuverPlan {
            enabled: true,
            lead_time_s: 2_300.0,
            prograde_dv_m_s: 660.0,
            radial_dv_m_s: 0.0,
        };
        let pred = world.predict_trajectory_with_plan(1, PredictionOptions::default(), Some(plan));
        assert!(pred.maneuver_world_pos.is_some());
        assert!(
            pred.planned_points_world.len() > 260,
            "expected transfer arc after burn, got {} pts",
            pred.planned_points_world.len()
        );
    }

    #[test]
    fn trajectory_prediction_detects_apoapsis_and_periapsis_on_orbit() {
        let planet = CelestialBody::kepler_prime();
        let r_p = planet.radius_m + 100_000.0;
        let v_p = (planet.mu() / r_p).sqrt() * 1.12;

        let cat = default_part_catalog();
        let mut bp = VesselBlueprint::new("OrbitCheck");
        bp.add_root("command_pod", 0);
        let mut vessel = Vessel::from_blueprint(1, &bp, &cat).expect("valid probe");
        vessel.pos = DVec2::new(r_p, 0.0);
        vessel.vel = DVec2::new(0.0, v_p);

        let mut world = SimWorld::new();
        world.bodies.push(planet);
        world.vessels.push(vessel);

        let pred = world.predict_trajectory(1, PredictionOptions::default());
        assert!(pred.points_world.len() > 50);
        assert!(
            pred.markers.iter().any(|m| m.kind == ApsisKind::Apoapsis),
            "expected apoapsis marker on elliptical orbit"
        );
    }

    #[test]
    fn fins_stabilize_rocket_during_atmospheric_ascent() {
        let cat = default_part_catalog();
        let mut bp = VesselBlueprint::new("FinDart");
        let pod = bp.add_root("command_pod", 0);
        let tank = bp.attach_child(
            pod,
            "tank_large",
            AttachKind::StackBottom,
            DVec2::new(0.0, -2.3),
            0.0,
            0,
        );
        bp.attach_child(
            tank,
            "fin_basic",
            AttachKind::RadialLeft,
            DVec2::new(-1.21, -3.5),
            0.0,
            0,
        );
        bp.attach_child(
            tank,
            "fin_basic",
            AttachKind::RadialRight,
            DVec2::new(1.21, -3.5),
            0.0,
            0,
        );

        let planet = CelestialBody::kepler_prime();
        let mut vessel = Vessel::from_blueprint(1, &bp, &cat).expect("valid finned dart");
        vessel.pos = DVec2::new(0.0, planet.radius_m + 1_200.0);
        vessel.vel = DVec2::new(0.0, 260.0);
        vessel.angle = 0.25;
        vessel.control.sas_enabled = false;

        let mut world = SimWorld::new();
        world.bodies.push(planet);
        world.vessels.push(vessel);
        world.active_vessel_id = Some(1);

        for _ in 0..(4 * 120) {
            world.step(1.0 / 120.0);
        }

        let v = world.active_vessel().expect("vessel alive");
        assert!(
            v.angle.abs() < 0.14,
            "expected aerodynamic fins to damp tilt toward prograde, got angle={}",
            v.angle
        );
    }

    #[test]
    fn heat_shield_ablates_and_protects_reentry_capsule() {
        let cat = default_part_catalog();
        let mut bp = VesselBlueprint::new("ReentryCapsule");
        let pod = bp.add_root("command_pod", 0);
        bp.attach_child(
            pod,
            "parachute_mk1",
            AttachKind::StackTop,
            DVec2::new(0.0, 0.77),
            0.0,
            0,
        );
        bp.attach_child(
            pod,
            "heat_shield",
            AttachKind::StackBottom,
            DVec2::new(0.0, -0.72),
            0.0,
            0,
        );

        let planet = CelestialBody::kepler_prime();
        let r0 = planet.radius_m + 72_000.0;
        let mut vessel = Vessel::from_blueprint(1, &bp, &cat).expect("valid capsule");
        vessel.pos = DVec2::new(0.0, r0);
        vessel.vel = DVec2::new(2_180.0, -240.0);
        vessel.angle = vessel.vel.x.atan2(-vessel.vel.y);
        vessel.control.sas_mode = SasMode::Retrograde;
        vessel.arm_all_parachutes();

        let mut world = SimWorld::new();
        world.bodies.push(planet);
        world.vessels.push(vessel);
        world.active_vessel_id = Some(1);

        let init_ablator = world.active_vessel().expect("cap").total_ablator_kg();

        for _ in 0..2_000 {
            world.step(0.5);
            if world.active_vessel().is_some_and(|v| v.landed || v.crashed) {
                break;
            }
        }

        let v = world.active_vessel().expect("capsule exists");
        assert!(!v.crashed, "capsule should survive shielded reentry");
        assert!(v.landed, "capsule should safely touch down under chute");
        assert!(
            v.total_ablator_kg() < init_ablator,
            "expected heat shield ablator to burn off during reentry"
        );
    }

    #[test]
    fn unshielded_tank_overheats_on_steep_reentry() {
        let cat = default_part_catalog();
        let mut bp = VesselBlueprint::new("Unshielded");
        let pod = bp.add_root("command_pod", 0);
        bp.attach_child(
            pod,
            "tank_small",
            AttachKind::StackBottom,
            DVec2::new(0.0, -1.5),
            0.0,
            0,
        );

        let planet = CelestialBody::kepler_prime();
        let mut vessel = Vessel::from_blueprint(1, &bp, &cat).expect("valid craft");
        vessel.pos = DVec2::new(0.0, planet.radius_m + 18_000.0);
        vessel.vel = DVec2::new(2_600.0, -900.0);

        let mut world = SimWorld::new();
        world.bodies.push(planet);
        world.vessels.push(vessel);
        world.active_vessel_id = Some(1);

        for _ in 0..(12 * 120) {
            world.step(1.0 / 120.0);
            if world.active_vessel().is_some_and(|v| v.crashed) {
                break;
            }
        }

        let v = world.active_vessel().expect("vessel exists");
        assert!(
            v.crashed || v.parts.len() < 2,
            "steep unshielded reentry should destroy parts or crash"
        );
    }

    #[test]
    fn rcs_translation_accelerates_probe_in_vacuum() {
        let cat = default_part_catalog();
        let mut bp = VesselBlueprint::new("RcsTest");
        let pod = bp.add_root("probe_core", 0);
        let tank = bp.attach_child(
            pod,
            "tank_small",
            AttachKind::StackBottom,
            DVec2::new(0.0, -1.11),
            0.0,
            0,
        );
        bp.attach_child(
            tank,
            "rcs_block",
            AttachKind::RadialLeft,
            DVec2::new(-0.805, -1.11),
            0.0,
            0,
        );

        let mut world = SimWorld::new();
        let mut vessel = Vessel::from_blueprint(1, &bp, &cat).expect("valid probe");
        vessel.control.rcs_enabled = true;
        vessel.control.rcs_translate = DVec2::new(0.0, 1.0);
        world.vessels.push(vessel);
        world.active_vessel_id = Some(1);

        let fuel0 = world.active_vessel().expect("v").total_fuel_kg();
        for _ in 0..120 {
            world.step(1.0 / 120.0);
        }
        let v = world.active_vessel().expect("v");
        assert!(v.vel.y > 0.5, "expected positive RCS translation velocity");
        assert!(
            v.total_fuel_kg() < fuel0,
            "expected RCS propellant consumption"
        );
    }

    #[test]
    fn sacrificial_landing_leg_absorbs_impact_without_destroying_pod() {
        let cat = default_part_catalog();
        let mut bp = VesselBlueprint::new("Lander");
        let pod = bp.add_root("command_pod", 0);
        let tank = bp.attach_child(
            pod,
            "tank_small",
            AttachKind::StackBottom,
            DVec2::new(0.0, -1.5),
            0.0,
            0,
        );
        bp.attach_child(
            tank,
            "engine_orbital",
            AttachKind::StackBottom,
            DVec2::new(0.0, -2.925),
            0.0,
            0,
        );

        let planet = CelestialBody::kepler_prime();
        let mut vessel = Vessel::from_blueprint(1, &bp, &cat).expect("valid lander");
        vessel.pos = DVec2::new(0.0, planet.radius_m + 3.4);
        vessel.vel = DVec2::new(0.0, -11.5); // Exceeds engine (10 m/s) but below pod (16 m/s)

        let mut world = SimWorld::new();
        world.bodies.push(planet);
        world.vessels.push(vessel);
        world.active_vessel_id = Some(1);

        for _ in 0..120 {
            world.step(1.0 / 120.0);
        }

        let v = world.active_vessel().expect("v");
        assert!(!v.crashed, "command pod should survive engine crumple");
        assert!(
            v.parts.len() < 3,
            "expected lower engine part to break on impact"
        );
    }

    #[test]
    fn heat_does_not_increase_without_atmospheric_density() {
        let cat = default_part_catalog();
        let mut bp = VesselBlueprint::new("VacuumProbe");
        let pod = bp.add_root("command_pod", 0);
        bp.attach_child(
            pod,
            "heat_shield",
            AttachKind::StackBottom,
            DVec2::new(0.0, -0.72),
            0.0,
            0,
        );

        let planet = CelestialBody::kepler_prime();
        let mut vessel = Vessel::from_blueprint(1, &bp, &cat).expect("valid vacuum probe");
        vessel.pos = DVec2::new(0.0, planet.radius_m + 500_000.0);
        vessel.vel = DVec2::new(4_000.0, 0.0);

        let init_ablator = vessel.total_ablator_kg();
        let mut world = SimWorld::new();
        world.bodies.push(planet);
        world.vessels.push(vessel);
        world.active_vessel_id = Some(1);

        for _ in 0..240 {
            world.step(1.0 / 120.0);
        }

        let vessel = world.active_vessel().expect("vacuum probe");
        assert_eq!(vessel.total_ablator_kg(), init_ablator);
        assert!(vessel.peak_temperature_k() <= AMBIENT_TEMP_K + 1.0);
        assert!(vessel.peak_heat_flux() <= 1.0e-6);
    }

    #[test]
    fn heat_does_not_increase_during_atmospheric_ascent() {
        let cat = default_part_catalog();
        let mut bp = VesselBlueprint::new("AscentProbe");
        bp.add_root("probe_core", 0);

        let planet = CelestialBody::kepler_prime();
        let mut vessel = Vessel::from_blueprint(1, &bp, &cat).expect("valid ascent probe");
        vessel.pos = DVec2::new(0.0, planet.radius_m + 50_000.0);
        vessel.vel = DVec2::new(495.0, 700.0);
        vessel.landed = false;

        let mut world = SimWorld::new();
        world.bodies.push(planet);
        world.vessels.push(vessel);
        world.active_vessel_id = Some(1);

        for _ in 0..240 {
            world.step(1.0 / 120.0);
        }

        let vessel = world.active_vessel().expect("ascent probe");
        assert!(vessel.peak_heat_flux() <= 1.0e-6);
        assert!(vessel.peak_temperature_k() <= AMBIENT_TEMP_K + 1.0);
    }

    #[test]
    fn fixed_120_hz_substep_matches_finer_reference_circular_orbit() {
        let earth = CelestialBody::earth();
        let radius_m = earth.radius_m + 300_000.0;
        let mu = earth.mu();
        let period_s = TAU_F64 * (radius_m.powi(3) / mu).sqrt();

        let steps_120 = ((period_s / FIXED_SUBSTEP_DT).round()) as usize;
        let steps_240 = ((period_s / (0.5 * FIXED_SUBSTEP_DT)).round()) as usize;
        let at_120 = propagate_circular_orbit(mu, radius_m, FIXED_SUBSTEP_DT, steps_120);
        let at_240 = propagate_circular_orbit(mu, radius_m, 0.5 * FIXED_SUBSTEP_DT, steps_240);
        let reference_error_m = at_240.distance(DVec2::new(radius_m, 0.0));
        let coarse_error_m = at_120.distance(DVec2::new(radius_m, 0.0));
        assert!(
            reference_error_m < 100.0,
            "reference error {reference_error_m:.3} m"
        );
        assert!(
            coarse_error_m < 1_000.0,
            "120 Hz error {coarse_error_m:.3} m"
        );
        assert!(
            (coarse_error_m - reference_error_m).abs() < 1.0,
            "120 Hz={coarse_error_m:.3} m, 240 Hz={reference_error_m:.3} m"
        );
    }

    #[test]
    fn step_ticks_is_bit_exact_regardless_of_call_partitioning() {
        let cat = default_part_catalog();
        let bp = lunar_capable_blueprint(&cat);
        let mut a = SimWorld::with_kepler_system_vessel(&bp, &cat).expect("world a");
        let mut b = SimWorld::with_kepler_system_vessel(&bp, &cat).expect("world b");
        a.trigger_active_stage();
        b.trigger_active_stage();
        assert!(a.requires_physical_warp());

        let _ = a.step_ticks(600, None);
        for chunk in [1_u32, 7, 120, 3, 469] {
            let _ = b.step_ticks(chunk, None);
        }
        let (va, vb) = (a.active_vessel().expect("a"), b.active_vessel().expect("b"));
        assert_eq!(va.pos.x.to_bits(), vb.pos.x.to_bits());
        assert_eq!(va.pos.y.to_bits(), vb.pos.y.to_bits());
        assert_eq!(va.vel.x.to_bits(), vb.vel.x.to_bits());
        assert_eq!(va.vel.y.to_bits(), vb.vel.y.to_bits());
        assert_eq!(a.time_s.to_bits(), b.time_s.to_bits());
    }

    #[test]
    fn settled_debris_does_not_force_physical_warp() {
        let cat = default_part_catalog();
        let bp = lunar_capable_blueprint(&cat);
        let mut world = SimWorld::with_kepler_system_vessel(&bp, &cat).expect("world");
        let mut debris = world.active_vessel().expect("vessel").clone();
        debris.id = 999;
        debris.is_debris = true;
        debris.landed = true;
        debris.crashed = false;
        // Park the active craft far from every atmosphere and make it quiescent.
        let far = world.bodies[0].pos + DVec2::new(world.bodies[0].radius_m * 40.0, 0.0);
        let active = world.active_vessel_mut().expect("vessel");
        active.pos = far;
        active.landed = false;
        active.angular_vel = 0.0;
        active.control.steer = 0.0;
        for p in &mut active.parts {
            p.engine_active = false;
        }
        world.vessels.push(debris);
        assert!(!world.requires_physical_warp());
    }
}
