//! @file vessel.rs
//!
//! @version 1.0.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Runtime vessel rigid body, crossfeed propellant draining, thrust/torque, and staging splits
//!
//! @description
//! Converts blueprints into live rigid-body vessels, resolves fuel crossfeed across decouplers,
//! computes thrust, reaction-wheel, and gimbal torque, handles decoupler tree splitting into
//! tracked debris vessels with exact linear momentum conservation, and computes collision extents.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use crate::math::{DVec2, G0, normalize_angle};
use crate::parts::{PartCatalog, PartKind, PartSpec, PlacedPart, VesselBlueprint};
use core::fmt;
use std::collections::{HashMap, HashSet, VecDeque};

// ---------- CONSTANTS

// ---------- TYPES

pub type VesselId = u64;

/// Errors returned when constructing or stepping vessels from invalid data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimError {
    EmptyBlueprint,
    MissingPartSpec(String),
    ZeroMassVessel,
    NoCoreRoot,
}

/// Live per-part runtime state inside a `Vessel`.
#[derive(Clone, Debug, PartialEq)]
pub struct PartState {
    pub instance_id: u64,
    pub parent_id: Option<u64>,
    pub spec: PartSpec,
    pub local_pos: DVec2,
    pub local_rot: f64,
    pub stage: u32,
    pub fuel_kg: f64,
    pub engine_active: bool,
    pub decoupled: bool,
    pub active_throttle: f64,
}

/// Player or autopilot control inputs applied during simulation substeps.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FlightControlInput {
    pub throttle: f64,
    /// Steering torque command in `[-1.0, 1.0]` (`+1.0` rotates CCW / nose left).
    pub steer: f64,
    pub sas_enabled: bool,
}

/// Complete rigid-body and part-tree state of a spacecraft or debris stage.
#[derive(Clone, Debug, PartialEq)]
pub struct Vessel {
    pub id: VesselId,
    pub name: String,
    pub is_debris: bool,
    pub pos: DVec2,
    pub vel: DVec2,
    pub angle: f64,
    pub angular_vel: f64,
    pub parts: Vec<PartState>,
    pub com_local: DVec2,
    pub total_mass_kg: f64,
    pub moi_kg_m2: f64,
    pub current_stage: u32,
    pub max_stage: u32,
    pub landed: bool,
    pub crashed: bool,
    pub control: FlightControlInput,
}

/// Outcome returned when activating the next stage on a vessel.
#[derive(Clone, Debug, PartialEq)]
pub struct StagingOutcome {
    pub fired_stage: u32,
    pub ignited_engines: usize,
    pub fired_decouplers: usize,
    pub spawned_debris: Vec<Vessel>,
}

// ---------- DECLARATIONS / IMPLEMENTATION

impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyBlueprint => write!(f, "vessel blueprint contains no parts"),
            Self::MissingPartSpec(id) => {
                write!(f, "part specification '{id}' not found in catalog")
            }
            Self::ZeroMassVessel => write!(f, "vessel total mass must be strictly positive"),
            Self::NoCoreRoot => write!(f, "vessel contains no root or surviving core part"),
        }
    }
}

impl std::error::Error for SimError {}

impl PartState {
    pub fn from_placed(placed: &PlacedPart, spec: PartSpec) -> Self {
        let fuel_kg = spec.fuel_capacity_kg();
        Self {
            instance_id: placed.instance_id,
            parent_id: placed.parent_id,
            spec,
            local_pos: placed.local_pos,
            local_rot: placed.local_rot,
            stage: placed.stage,
            fuel_kg,
            engine_active: false,
            decoupled: false,
            active_throttle: 0.0,
        }
    }

    #[inline]
    pub fn mass_kg(&self) -> f64 {
        self.spec.dry_mass_kg + self.fuel_kg
    }
}

impl Vessel {
    /// Builds a `Vessel` from a `VesselBlueprint`, computing exact CoM and moment of inertia.
    pub fn from_blueprint(
        id: VesselId,
        bp: &VesselBlueprint,
        catalog: &PartCatalog,
    ) -> Result<Self, SimError> {
        if bp.parts.is_empty() {
            return Err(SimError::EmptyBlueprint);
        }

        let mut parts = Vec::with_capacity(bp.parts.len());
        let mut max_stage = 0_u32;
        for placed in &bp.parts {
            let Some(spec) = catalog.get(&placed.spec_id).cloned() else {
                return Err(SimError::MissingPartSpec(placed.spec_id.clone()));
            };
            max_stage = max_stage.max(placed.stage);
            parts.push(PartState::from_placed(placed, spec));
        }

        let mut vessel = Self {
            id,
            name: bp.name.clone(),
            is_debris: false,
            pos: DVec2::ZERO,
            vel: DVec2::ZERO,
            angle: 0.0,
            angular_vel: 0.0,
            parts,
            com_local: DVec2::ZERO,
            total_mass_kg: 1.0,
            moi_kg_m2: 1.0,
            current_stage: 0,
            max_stage,
            landed: false,
            crashed: false,
            control: FlightControlInput {
                throttle: 0.0,
                steer: 0.0,
                sas_enabled: true,
            },
        };
        vessel.recompute_mass_properties(false)?;
        Ok(vessel)
    }

    /// Places the vessel upright on a planet surface centered at `body_pos` with radius `body_radius`.
    pub fn place_on_north_pole(&mut self, body_pos: DVec2, body_radius: f64) {
        let min_local_y = self
            .parts
            .iter()
            .map(|p| p.local_pos.y - p.spec.shape.contact_half_extent().y)
            .fold(f64::INFINITY, f64::min);

        let lowest_rel_com_y = min_local_y - self.com_local.y;
        self.pos = body_pos + DVec2::new(0.0, body_radius - lowest_rel_com_y);
        self.vel = DVec2::ZERO;
        self.angle = 0.0;
        self.angular_vel = 0.0;
        self.landed = true;
    }

    /// Recomputes `total_mass_kg`, `com_local`, and `moi_kg_m2`.
    /// When `compensate_world_pos` is true, shifts `self.pos` by the world-space CoM delta so
    /// physical part locations never jump as propellant burns.
    pub fn recompute_mass_properties(&mut self, compensate_world_pos: bool) -> Result<(), SimError> {
        let old_com = self.com_local;
        let mut total_mass = 0.0;
        let mut weighted = DVec2::ZERO;
        for part in &self.parts {
            let m = part.mass_kg();
            total_mass += m;
            weighted += part.local_pos * m;
        }
        if total_mass <= 1.0e-9 {
            return Err(SimError::ZeroMassVessel);
        }

        let new_com = weighted / total_mass;
        let mut moi = 0.0;
        for part in &self.parts {
            let m = part.mass_kg();
            let d2 = (part.local_pos - new_com).length_squared();
            moi += m * (part.spec.shape.unit_moi() + d2);
        }

        if compensate_world_pos {
            let delta_com = new_com - old_com;
            self.pos += delta_com.rotate(self.angle);
        }

        self.com_local = new_com;
        self.total_mass_kg = total_mass;
        self.moi_kg_m2 = moi.max(0.5);
        Ok(())
    }

    /// World position of a given part's center.
    #[inline]
    pub fn part_world_pos(&self, part: &PartState) -> DVec2 {
        self.pos + (part.local_pos - self.com_local).rotate(self.angle)
    }

    /// Computes total remaining propellant in kg across all tanks in this vessel.
    pub fn total_fuel_kg(&self) -> f64 {
        self.parts.iter().map(|p| p.fuel_kg).sum()
    }

    /// Computes total fuel capacity in kg across all tanks in this vessel.
    pub fn total_fuel_capacity_kg(&self) -> f64 {
        self.parts.iter().map(|p| p.spec.fuel_capacity_kg()).sum()
    }

    /// Computes instantaneous max reaction-wheel torque authority.
    pub fn available_reaction_torque(&self) -> f64 {
        if self.is_debris {
            return 0.0;
        }
        self.parts
            .iter()
            .filter_map(|p| match p.spec.kind {
                PartKind::CommandPod(pod) => Some(pod.reaction_torque_nm),
                _ => None,
            })
            .sum()
    }

    /// Activates engines and triggers decouplers assigned to `self.current_stage`, splitting any
    /// detached subtrees into new tracked debris vessels while conserving linear momentum.
    pub fn activate_next_stage(&mut self, next_vessel_id: &mut u64) -> StagingOutcome {
        let stage = self.current_stage;
        let mut ignited_engines = 0_usize;
        let mut fired_decouplers = Vec::new();

        for part in &mut self.parts {
            if part.stage != stage {
                continue;
            }
            match part.spec.kind {
                PartKind::Engine(_) if !part.engine_active => {
                    part.engine_active = true;
                    ignited_engines += 1;
                }
                PartKind::Decoupler(_) if !part.decoupled => {
                    part.decoupled = true;
                    fired_decouplers.push(part.instance_id);
                }
                _ => {}
            }
        }

        let fired_dec_count = fired_decouplers.len();
        let mut spawned_debris = Vec::new();

        for dec_id in fired_decouplers {
            let child_roots: Vec<u64> = self
                .parts
                .iter()
                .filter(|p| p.parent_id == Some(dec_id))
                .map(|p| p.instance_id)
                .collect();

            if child_roots.is_empty() {
                continue;
            }

            let ejection_impulse = self
                .parts
                .iter()
                .find(|p| p.instance_id == dec_id)
                .and_then(|p| match p.spec.kind {
                    PartKind::Decoupler(d) => Some(d.ejection_impulse_ns),
                    _ => None,
                })
                .unwrap_or(0.0);

            let dec_rot = self
                .parts
                .iter()
                .find(|p| p.instance_id == dec_id)
                .map_or(0.0, |p| p.local_rot);

            let mut detached_ids = HashSet::new();
            let mut queue = VecDeque::new();
            for root_id in child_roots {
                if detached_ids.insert(root_id) {
                    queue.push_back(root_id);
                }
            }
            while let Some(curr) = queue.pop_front() {
                for p in &self.parts {
                    if p.parent_id == Some(curr) && detached_ids.insert(p.instance_id) {
                        queue.push_back(p.instance_id);
                    }
                }
            }

            if detached_ids.is_empty() || detached_ids.len() >= self.parts.len() {
                continue;
            }

            let mut debris_parts = Vec::new();
            let mut parent_parts = Vec::with_capacity(self.parts.len() - detached_ids.len());
            for mut p in self.parts.drain(..) {
                if detached_ids.contains(&p.instance_id) {
                    if p.parent_id == Some(dec_id) {
                        p.parent_id = None;
                    }
                    p.active_throttle = 0.0;
                    debris_parts.push(p);
                } else {
                    parent_parts.push(p);
                }
            }
            self.parts = parent_parts;

            let old_com = self.com_local;
            let old_pos = self.pos;
            let old_vel = self.vel;
            let old_angle = self.angle;
            let old_omega = self.angular_vel;

            let mut debris = Vessel {
                id: *next_vessel_id,
                name: format!("{} Stage {}", self.name, stage + 1),
                is_debris: true,
                pos: old_pos,
                vel: old_vel,
                angle: old_angle,
                angular_vel: old_omega,
                parts: debris_parts,
                com_local: DVec2::ZERO,
                total_mass_kg: 1.0,
                moi_kg_m2: 1.0,
                current_stage: stage + 1,
                max_stage: self.max_stage,
                landed: false,
                crashed: false,
                control: FlightControlInput::default(),
            };
            *next_vessel_id += 1;

            if self.recompute_mass_properties(false).is_ok()
                && debris.recompute_mass_properties(false).is_ok()
            {
                let parent_offset_world = (self.com_local - old_com).rotate(old_angle);
                let debris_offset_world = (debris.com_local - old_com).rotate(old_angle);

                self.pos = old_pos + parent_offset_world;
                debris.pos = old_pos + debris_offset_world;

                self.vel = old_vel + parent_offset_world.perp() * old_omega;
                debris.vel = old_vel + debris_offset_world.perp() * old_omega;

                let eject_dir = DVec2::new(0.0, -1.0).rotate(old_angle + dec_rot);
                let impulse_vec = eject_dir * ejection_impulse;
                debris.vel += impulse_vec / debris.total_mass_kg;
                self.vel -= impulse_vec / self.total_mass_kg;

                spawned_debris.push(debris);
            }
        }

        self.current_stage = self.current_stage.saturating_add(1);

        StagingOutcome {
            fired_stage: stage,
            ignited_engines,
            fired_decouplers: fired_dec_count,
            spawned_debris,
        }
    }

    /// Consumes propellant for duration `dt` across crossfeed-reachable tanks and computes
    /// effective per-engine throttle fraction for this substep.
    pub fn consume_propellant(&mut self, dt: f64) {
        let cmd_throttle = if self.is_debris || self.crashed {
            0.0
        } else {
            self.control.throttle.clamp(0.0, 1.0)
        };

        for p in &mut self.parts {
            p.active_throttle = 0.0;
        }
        if cmd_throttle <= 0.0 || dt <= 0.0 {
            return;
        }

        let engine_indices: Vec<usize> = self
            .parts
            .iter()
            .enumerate()
            .filter_map(|(idx, p)| match p.spec.kind {
                PartKind::Engine(_) if p.engine_active => Some(idx),
                _ => None,
            })
            .collect();

        if engine_indices.is_empty() {
            return;
        }

        let mut fuel_changed = false;
        for eng_idx in engine_indices {
            let (eng_id, eng_spec) = match self.parts.get(eng_idx) {
                Some(p) => match p.spec.kind {
                    PartKind::Engine(e) => (p.instance_id, e),
                    _ => continue,
                },
                None => continue,
            };

            if eng_spec.isp_s <= 0.0 || eng_spec.max_thrust_n <= 0.0 {
                continue;
            }

            let mdot_requested = (eng_spec.max_thrust_n * cmd_throttle) / (eng_spec.isp_s * G0);
            let dm_requested = mdot_requested * dt;
            if dm_requested <= 0.0 {
                continue;
            }

            let mut tank_indices = self.reachable_tank_indices(eng_id);
            tank_indices.sort_by(|&a, &b| {
                let ya = self.parts.get(a).map_or(0.0, |p| p.local_pos.y);
                let yb = self.parts.get(b).map_or(0.0, |p| p.local_pos.y);
                ya.total_cmp(&yb)
            });

            let mut remaining = dm_requested;
            for t_idx in tank_indices {
                if remaining <= 1.0e-12 {
                    break;
                }
                if let Some(tank) = self.parts.get_mut(t_idx)
                    && tank.fuel_kg > 0.0
                {
                    let take = tank.fuel_kg.min(remaining);
                    tank.fuel_kg -= take;
                    remaining -= take;
                    fuel_changed = true;
                }
            }

            let drained = dm_requested - remaining;
            let actual_fraction = (drained / dm_requested).clamp(0.0, 1.0);
            if let Some(eng_part) = self.parts.get_mut(eng_idx) {
                eng_part.active_throttle = cmd_throttle * actual_fraction;
            }
        }

        if fuel_changed {
            let _ = self.recompute_mass_properties(true);
        }
    }

    /// Computes instantaneous non-gravitational net force (world N) and torque (N·m) from
    /// active engines, gimbal vectoring, reaction wheels, and optional SAS rate damping.
    pub fn compute_thrust_and_control_wrench(&self) -> (DVec2, f64) {
        let mut net_force = DVec2::ZERO;
        let mut net_torque = 0.0;

        let steer_cmd = if self.is_debris || self.crashed {
            0.0
        } else {
            self.control.steer.clamp(-1.0, 1.0)
        };

        let sas_torque_norm = if !self.is_debris && !self.crashed && self.control.sas_enabled {
            if steer_cmd.abs() > 0.05 {
                (-self.angular_vel * 0.45).clamp(-0.5, 0.5)
            } else {
                (-self.angular_vel * 3.2).clamp(-1.0, 1.0)
            }
        } else {
            0.0
        };

        let total_steer = (steer_cmd + sas_torque_norm).clamp(-1.0, 1.0);
        net_torque += total_steer * self.available_reaction_torque();

        for part in &self.parts {
            if let PartKind::Engine(eng) = part.spec.kind
                && part.engine_active
                && part.active_throttle > 0.0
            {
                let r_local = part.local_pos - self.com_local;
                let lever_sign = if r_local.y <= 0.0 { -1.0 } else { 1.0 };
                let gimbal_angle = total_steer * eng.gimbal_range_rad * lever_sign;
                let local_thrust_dir = DVec2::Y.rotate(part.local_rot + gimbal_angle);
                let force_local = local_thrust_dir * (eng.max_thrust_n * part.active_throttle);
                net_torque += r_local.cross(force_local);
                net_force += force_local.rotate(self.angle);
            }
        }

        (net_force, net_torque)
    }

    /// Normalizes vessel orientation angle into `[-PI, PI]`.
    pub fn wrap_angle(&mut self) {
        self.angle = normalize_angle(self.angle);
    }

    /// Locates indices of crossfeed-connected fuel tanks reachable from `start_instance_id`.
    fn reachable_tank_indices(&self, start_instance_id: u64) -> Vec<usize> {
        let id_to_idx: HashMap<u64, usize> = self
            .parts
            .iter()
            .enumerate()
            .map(|(i, p)| (p.instance_id, i))
            .collect();

        let Some(&start_idx) = id_to_idx.get(&start_instance_id) else {
            return Vec::new();
        };

        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut tanks = Vec::new();
        visited.insert(start_idx);
        queue.push_back(start_idx);

        while let Some(curr_idx) = queue.pop_front() {
            let Some(curr) = self.parts.get(curr_idx) else {
                continue;
            };
            if matches!(curr.spec.kind, PartKind::Tank(_)) {
                tanks.push(curr_idx);
            }
            if let PartKind::Decoupler(dec) = curr.spec.kind
                && !dec.crossfeed
                && curr_idx != start_idx
            {
                continue;
            }

            if let Some(pid) = curr.parent_id
                && let Some(&p_idx) = id_to_idx.get(&pid)
                && visited.insert(p_idx)
            {
                queue.push_back(p_idx);
            }

            for (c_idx, child) in self.parts.iter().enumerate() {
                if child.parent_id == Some(curr.instance_id) && visited.insert(c_idx) {
                    queue.push_back(c_idx);
                }
            }
        }

        tanks
    }
}

// ---------- TESTS

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parts::{default_part_catalog, starter_two_stage_blueprint};

    #[test]
    fn decoupler_staging_conserves_momentum_and_blocks_crossfeed() {
        let cat = default_part_catalog();
        let bp = starter_two_stage_blueprint(&cat);
        let mut vessel = Vessel::from_blueprint(1, &bp, &cat).expect("valid starter rocket");
        vessel.vel = DVec2::new(120.0, 840.0);
        vessel.angle = 0.35;

        let mut next_id = 2;
        let out0 = vessel.activate_next_stage(&mut next_id);
        assert_eq!(out0.ignited_engines, 1);
        assert!(out0.spawned_debris.is_empty());

        vessel.control.throttle = 1.0;
        let upper_fuel_before = vessel
            .parts
            .iter()
            .find(|p| p.spec.id == "tank_small")
            .map_or(0.0, |p| p.fuel_kg);
        vessel.consume_propellant(1.0);
        let upper_fuel_after = vessel
            .parts
            .iter()
            .find(|p| p.spec.id == "tank_small")
            .map_or(0.0, |p| p.fuel_kg);
        assert!((upper_fuel_before - upper_fuel_after).abs() < 1.0e-12);

        let m_before_split = vessel.total_mass_kg;
        let p_before_split = vessel.vel * m_before_split;
        let out1 = vessel.activate_next_stage(&mut next_id);
        assert_eq!(out1.fired_decouplers, 1);
        assert_eq!(out1.ignited_engines, 1);
        assert_eq!(out1.spawned_debris.len(), 1);

        let debris = &out1.spawned_debris[0];
        assert!(debris.is_debris);
        let m_after_split = vessel.total_mass_kg + debris.total_mass_kg;
        let p_after_split = vessel.vel * vessel.total_mass_kg + debris.vel * debris.total_mass_kg;

        assert!((m_before_split - m_after_split).abs() < 1.0e-9);
        assert!(p_before_split.distance(p_after_split) < 1.0e-8);
    }
}
