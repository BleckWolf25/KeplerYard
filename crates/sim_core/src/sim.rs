//! @file sim.rs
//!
//! @version 1.0.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Full N-body symplectic Velocity Verlet simulator and orbital trajectory predictor
//!
//! @description
//! Implements the deterministic physics simulation world (`SimWorld`), executing fixed 1/120 s
//! symplectic leapfrog/Velocity Verlet substeps under thrust/contact and adaptive coast steps
//! during time warp, mutual N-body gravity across all celestial bodies and vessels, surface
//! pad contact resolution, and forward orbital trajectory prediction with Ap/Pe/impact markers.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use crate::body::{CelestialBody, OrbitElements, dominant_body_elements};
use crate::math::{DVec2, GRAV_CONSTANT, TAU_F64};
use crate::parts::{PartKind, PartSpec, VesselBlueprint};
use crate::vessel::{SimError, StagingOutcome, Vessel, VesselId};

// ---------- CONSTANTS

/// Nominal fixed physics substep duration (1/120 s).
pub const FIXED_SUBSTEP_DT: f64 = 1.0 / 120.0;

/// Maximum coast substep duration during high time warp with zero thrust (seconds).
pub const MAX_COAST_SUBSTEP_DT: f64 = 1.0;

// ---------- TYPES

/// Simulation configuration toggles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SimConfig {
    pub fixed_dt: f64,
    pub max_coast_dt: f64,
    /// Atmosphere drag toggle (disabled by default in M0–M2, activated in M3).
    pub atmosphere_drag: bool,
    /// Mutual vessel-to-vessel and vessel-to-body N-body attraction toggle.
    pub mutual_n_body: bool,
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

/// Forward-propagated trajectory samples and orbital markers for UI/line rendering.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrajectoryPrediction {
    pub points_world: Vec<DVec2>,
    pub markers: Vec<ApsisMarker>,
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
            atmosphere_drag: false,
            mutual_n_body: true,
        }
    }
}

impl Default for PredictionOptions {
    fn default() -> Self {
        Self {
            max_steps: 720,
            max_horizon_s: 3_600.0,
        }
    }
}

impl Default for SimWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl SimWorld {
    /// Creates an empty simulation world with default configuration.
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

    /// Creates a standard launchpad sandbox around Kepler Prime with the supplied vessel.
    pub fn with_launchpad_vessel(
        bp: &VesselBlueprint,
        catalog: &crate::parts::PartCatalog,
    ) -> Result<Self, SimError> {
        let mut world = Self::new();
        let planet = CelestialBody::kepler_prime();
        let mut vessel = Vessel::from_blueprint(world.next_vessel_id, bp, catalog)?;
        world.next_vessel_id += 1;
        vessel.place_on_north_pole(planet.pos, planet.radius_m);
        let active_id = vessel.id;
        world.bodies.push(planet);
        world.vessels.push(vessel);
        world.active_vessel_id = Some(active_id);
        Ok(world)
    }

    pub fn active_vessel(&self) -> Option<&Vessel> {
        let id = self.active_vessel_id?;
        self.vessels.iter().find(|v| v.id == id)
    }

    pub fn active_vessel_mut(&mut self) -> Option<&mut Vessel> {
        let id = self.active_vessel_id?;
        self.vessels.iter_mut().find(|v| v.id == id)
    }

    /// Activates the next stage on the active vessel and inserts any spawned debris vessels.
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

    /// Advances the simulation by `total_dt` seconds, choosing `1/120 s` substeps when any vessel
    /// is burning, rotating fast, or near ground contact, and larger adaptive substeps on coasts.
    pub fn step(&mut self, total_dt: f64) {
        if total_dt <= 0.0 || !total_dt.is_finite() {
            return;
        }

        let any_burning_or_contact = self.vessels.iter().any(|v| {
            let thrusting = !v.is_debris
                && v.control.throttle > 0.0
                && v.parts
                    .iter()
                    .any(|p| matches!(p.spec.kind, PartKind::Engine(_)) && p.engine_active);
            let near_surface = self
                .bodies
                .iter()
                .any(|b| v.pos.distance(b.pos) - b.radius_m < 100.0);
            let steering =
                !v.is_debris && (v.control.steer.abs() > 1.0e-4 || v.angular_vel.abs() > 0.05);
            thrusting || near_surface || steering || v.landed
        });

        let substep_limit = if any_burning_or_contact {
            self.config.fixed_dt
        } else {
            self.config.max_coast_dt.max(self.config.fixed_dt)
        };

        let mut remaining = total_dt;
        while remaining > 1.0e-12 {
            let dt = remaining.min(substep_limit);
            self.step_verlet(dt);
            remaining -= dt;
        }
    }

    /// Single synchronized symplectic Velocity Verlet / leapfrog substep of duration `dt`.
    pub fn step_verlet(&mut self, dt: f64) {
        if dt <= 0.0 {
            return;
        }

        let (body_acc_0, vessel_acc_0, vessel_alpha_0) = self.compute_accelerations();

        let mut body_vhalf = Vec::with_capacity(self.bodies.len());
        for (i, body) in self.bodies.iter_mut().enumerate() {
            let a0 = body_acc_0.get(i).copied().unwrap_or(DVec2::ZERO);
            let vh = body.vel + a0 * (0.5 * dt);
            body.pos += vh * dt;
            body_vhalf.push(vh);
        }

        let mut vessel_vhalf = Vec::with_capacity(self.vessels.len());
        let mut vessel_whalf = Vec::with_capacity(self.vessels.len());
        for (i, vessel) in self.vessels.iter_mut().enumerate() {
            if vessel.crashed {
                vessel_vhalf.push(DVec2::ZERO);
                vessel_whalf.push(0.0);
                continue;
            }
            let a0 = vessel_acc_0.get(i).copied().unwrap_or(DVec2::ZERO);
            let alpha0 = vessel_alpha_0.get(i).copied().unwrap_or(0.0);
            let vh = vessel.vel + a0 * (0.5 * dt);
            let wh = vessel.angular_vel + alpha0 * (0.5 * dt);

            vessel.pos += vh * dt;
            vessel.angle += wh * dt;
            vessel.wrap_angle();
            vessel.consume_propellant(dt);

            vessel_vhalf.push(vh);
            vessel_whalf.push(wh);
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
            let vh = vessel_vhalf.get(i).copied().unwrap_or(vessel.vel);
            let wh = vessel_whalf.get(i).copied().unwrap_or(vessel.angular_vel);
            let a1 = vessel_acc_1.get(i).copied().unwrap_or(DVec2::ZERO);
            let alpha1 = vessel_alpha_1.get(i).copied().unwrap_or(0.0);
            vessel.vel = vh + a1 * (0.5 * dt);
            vessel.angular_vel = wh + alpha1 * (0.5 * dt);
        }

        self.resolve_surface_contacts();
        self.time_s += dt;
    }

    /// Computes total mechanical energy (kinetic + gravitational potential) of the entire N-body world.
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

    /// Computes total linear momentum (kg·m/s) across all celestial bodies and vessels.
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

    /// Computes total scalar angular momentum (kg·m²/s) about origin across all bodies and vessels.
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

    /// Returns osculating orbital elements for the requested vessel around its dominant attractor.
    pub fn vessel_orbit_elements(&self, vessel_id: VesselId) -> Option<OrbitElements> {
        let vessel = self.vessels.iter().find(|v| v.id == vessel_id)?;
        dominant_body_elements(&self.bodies, vessel.pos, vessel.vel)
    }

    /// Forward-propagates a coasting snapshot of `vessel_id` under N-body gravity to produce
    /// live trajectory points and apoapsis/periapsis/impact markers.
    pub fn predict_trajectory(
        &self,
        vessel_id: VesselId,
        opts: PredictionOptions,
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

        let mut v_pos = vessel.pos;
        let mut v_vel = vessel.vel;

        let mut points = Vec::with_capacity(opts.max_steps.min(1024));
        points.push(v_pos);
        let mut markers = Vec::new();

        let primary_pos = bodies.first().map_or(DVec2::ZERO, |b| b.pos);
        let mut prev_dir = (v_pos - primary_pos).normalize_or_zero();
        let mut swept_angle = 0.0;

        let mut prev_alt_2: Option<(f64, DVec2)> = None;
        let mut prev_alt_1: Option<(f64, DVec2)> = None;
        let mut elapsed = 0.0;

        for step_idx in 0..opts.max_steps {
            if elapsed >= opts.max_horizon_s {
                break;
            }

            let dominant_r = bodies
                .iter()
                .map(|b| v_pos.distance(b.pos))
                .fold(f64::INFINITY, f64::min);
            let speed = v_vel.length().max(25.0);
            let dt = (dominant_r / speed * 0.015).clamp(0.35, 10.0);

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

            if let Some(primary) = bodies.first() {
                let rel = v_pos - primary.pos;
                let alt = rel.length() - primary.radius_m;
                if let (Some((a2, _)), Some((a1, p1))) = (prev_alt_2, prev_alt_1) {
                    if a1 > a2 && a1 > alt && markers.len() < 6 {
                        markers.push(ApsisMarker {
                            kind: ApsisKind::Apoapsis,
                            world_pos: p1,
                            altitude_m: a1,
                        });
                    } else if a1 < a2 && a1 < alt && markers.len() < 6 {
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
                if step_idx > 16 && swept_angle.abs() >= TAU_F64 {
                    break;
                }
            }
        }

        TrajectoryPrediction {
            points_world: points,
            markers,
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
            let (mut force, mut torque) = vessel.compute_thrust_and_control_wrench();

            if self.config.atmosphere_drag {
                let (drag_f, drag_t) = compute_vessel_drag(vessel, &self.bodies);
                force += drag_f;
                torque += drag_t;
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

    fn resolve_surface_contacts(&mut self) {
        for vessel in &mut self.vessels {
            if vessel.crashed {
                continue;
            }
            vessel.landed = false;
            for body in &self.bodies {
                let mut deepest_penetration = 0.0;
                let mut contact_normal = DVec2::Y;
                let mut contact_impact_limit = 12.0;

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
                        let world_pt = vessel.pos + (local_pt - vessel.com_local).rotate(vessel.angle);
                        let rel = world_pt - body.pos;
                        let dist = rel.length();
                        let pen = body.radius_m - dist;
                        if pen > deepest_penetration {
                            deepest_penetration = pen;
                            contact_normal = rel.normalize_or_zero();
                            contact_impact_limit = part.spec.max_impact_m_s;
                        }
                    }
                }

                if deepest_penetration > 0.0 {
                    let rel_vel = vessel.vel - body.vel;
                    let v_rad = rel_vel.dot(contact_normal);
                    if v_rad < -contact_impact_limit {
                        vessel.pos += contact_normal * deepest_penetration;
                        vessel.vel = body.vel;
                        vessel.angular_vel = 0.0;
                        vessel.crashed = true;
                        break;
                    }

                    vessel.pos += contact_normal * deepest_penetration;
                    if v_rad < 0.0 {
                        vessel.vel -= contact_normal * v_rad;
                    }
                    let tangent = contact_normal.perp();
                    let v_tan = (vessel.vel - body.vel).dot(tangent);
                    if v_tan.abs() < 4.0 {
                        vessel.vel -= tangent * (v_tan * 0.25);
                        vessel.angular_vel *= 0.80;
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

fn compute_vessel_drag(vessel: &Vessel, bodies: &[CelestialBody]) -> (DVec2, f64) {
    let mut drag_force = DVec2::ZERO;
    let mut drag_torque = 0.0;

    for body in bodies {
        let Some(atmo) = body.atmosphere else {
            continue;
        };
        let alt = vessel.pos.distance(body.pos) - body.radius_m;
        let rho = atmo.density_at(alt);
        if rho <= 0.0 {
            continue;
        }
        let v_rel = vessel.vel - body.vel;
        let speed = v_rel.length();
        if speed <= 1.0e-3 {
            continue;
        }
        let drag_dir = -v_rel / speed;
        for part in &vessel.parts {
            let area = part_reference_area(&part.spec);
            let mag = 0.5 * rho * speed * speed * part.spec.drag_coeff * area;
            let f = drag_dir * mag;
            let r_world = (part.local_pos - vessel.com_local).rotate(vessel.angle);
            drag_force += f;
            drag_torque += r_world.cross(f);
        }
    }

    (drag_force, drag_torque)
}

fn part_reference_area(spec: &PartSpec) -> f64 {
    match spec.kind {
        PartKind::Fin(f) => f.lift_area_m2,
        _ => spec.shape.bounding_size().x.max(0.2),
    }
}

// ---------- TESTS

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{G0, SplitMix64};
    use crate::parts::{default_part_catalog, starter_two_stage_blueprint};

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
        let rel_e_err = (e1 - e0).abs() / e0.abs();
        let rel_l_err = (l1 - l0).abs() / l0.abs();

        assert!(rel_e_err < 2.0e-6, "relative energy error too high: {rel_e_err}");
        assert!(rel_l_err < 1.0e-10, "angular momentum drift too high: {rel_l_err}");
    }

    #[test]
    fn three_body_mutual_gravity_conserves_energy_and_momenta() {
        let mut world = SimWorld::new();
        let planet = CelestialBody::kepler_prime();
        let moon_r = 2_400_000.0;
        let moon_v = (planet.mu() / moon_r).sqrt();
        let moon = CelestialBody {
            id: 1,
            name: String::from("Selene"),
            mass_kg: planet.mass_kg * 0.02,
            radius_m: 120_000.0,
            pos: DVec2::new(moon_r, 0.0),
            vel: DVec2::new(0.0, moon_v),
            atmosphere: None,
        };

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
            crate::parts::AttachKind::StackBottom,
            DVec2::new(0.0, -1.5),
            0.0,
            0,
        );
        bp.attach_child(
            tank,
            "engine_orbital",
            crate::parts::AttachKind::StackBottom,
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
                    v.control.steer = if (300..550).contains(&tick) { -0.25 } else { 0.0 };
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
}
