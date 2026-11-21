//! @file vessel.rs
//!
//! @version 1.3.0
//! @author `BleckWolf25`
//! @license MIT
//!
//! @summary Runtime vessel rigid body, SAS hold modes, RCS, SRBs, ablator, and mirrored parts
//!
//! @description
//! Converts blueprints into live rigid-body vessels, tracks part attach kind and mirrored visual
//! flag, resolves fuel crossfeed, simulates liquid engines, SRBs, and RCS thrusters, tracks
//! reentry temperatures, ablator mass, and parachutes, and handles decoupler splits.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use crate::math::{DVec2, G0, normalize_angle};
use crate::parts::{AttachKind, PartCatalog, PartKind, PartSpec, PlacedPart, VesselBlueprint};
use core::fmt;
use std::collections::{HashMap, HashSet, VecDeque};

// ---------- CONSTANTS

pub const AMBIENT_TEMP_K: f64 = 293.15;

// ---------- TYPES

pub type VesselId = u64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimError {
    EmptyBlueprint,
    MissingPartSpec(String),
    ZeroMassVessel,
    NoCoreRoot,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SasMode {
    #[default]
    Stability,
    Prograde,
    Retrograde,
    RadialOut,
    Maneuver,
}

#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct PartState {
    pub instance_id: u64,
    pub parent_id: Option<u64>,
    pub attach_kind: AttachKind,
    pub mirrored: bool,
    pub spec: PartSpec,
    pub local_pos: DVec2,
    pub local_rot: f64,
    pub stage: u32,
    pub crossfeed: bool,
    pub fuel_kg: f64,
    pub ablator_kg: f64,
    pub engine_active: bool,
    pub decoupled: bool,
    pub active_throttle: f64,
    pub active_gimbal_rad: f64,
    pub chute_armed: bool,
    pub chute_deployed: bool,
    pub temperature_k: f64,
    pub heat_flux_w_m2: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FlightControlInput {
    pub throttle: f64,
    pub steer: f64,
    pub sas_enabled: bool,
    pub sas_mode: SasMode,
    pub sas_target_angle: Option<f64>,
    pub rcs_enabled: bool,
    pub rcs_translate: DVec2,
}

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

#[derive(Clone, Debug, PartialEq)]
pub struct StagingOutcome {
    pub fired_stage: u32,
    pub ignited_engines: usize,
    pub fired_decouplers: usize,
    pub armed_chutes: usize,
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

impl SasMode {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Stability => "STAB",
            Self::Prograde => "PRO",
            Self::Retrograde => "RETRO",
            Self::RadialOut => "RAD+",
            Self::Maneuver => "MAN",
        }
    }

    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Stability => Self::Prograde,
            Self::Prograde => Self::Retrograde,
            Self::Retrograde => Self::RadialOut,
            Self::RadialOut => Self::Maneuver,
            Self::Maneuver => Self::Stability,
        }
    }
}

impl PartState {
    #[must_use]
    pub const fn from_placed(placed: &PlacedPart, spec: PartSpec) -> Self {
        let fuel_kg = spec.fuel_capacity_kg();
        let ablator_kg = match spec.kind {
            PartKind::HeatShield(hs) => hs.ablator_kg,
            _ => 0.0,
        };
        let crossfeed = match spec.kind {
            PartKind::Decoupler(d) => d.crossfeed || placed.crossfeed,
            _ => placed.crossfeed,
        };
        Self {
            instance_id: placed.instance_id,
            parent_id: placed.parent_id,
            attach_kind: placed.attach_kind,
            mirrored: placed.mirrored,
            spec,
            local_pos: placed.local_pos,
            local_rot: placed.local_rot,
            stage: placed.stage,
            crossfeed,
            fuel_kg,
            ablator_kg,
            engine_active: false,
            decoupled: false,
            active_throttle: 0.0,
            active_gimbal_rad: 0.0,
            chute_armed: false,
            chute_deployed: false,
            temperature_k: AMBIENT_TEMP_K,
            heat_flux_w_m2: 0.0,
        }
    }

    #[inline]
    #[must_use]
    pub fn mass_kg(&self) -> f64 {
        self.spec.dry_mass_kg + self.fuel_kg
    }
}

impl Vessel {
    /// Creates a vessel from the provided blueprint and part catalog.
    ///
    /// # Errors
    ///
    /// Returns [`SimError`] if the blueprint is empty, a part spec is missing, no command pod is
    /// present, or the resulting vessel has zero mass.
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
                sas_mode: SasMode::Stability,
                sas_target_angle: None,
                rcs_enabled: false,
                rcs_translate: DVec2::ZERO,
            },
        };
        vessel.recompute_mass_properties(false)?;
        Ok(vessel)
    }

    pub fn place_on_north_pole(&mut self, body_pos: DVec2, body_radius: f64) {
        self.place_on_north_pole_with_vel(body_pos, DVec2::ZERO, body_radius);
    }

    pub fn place_on_north_pole_with_vel(
        &mut self,
        body_pos: DVec2,
        body_vel: DVec2,
        body_radius: f64,
    ) {
        let min_local_y = self
            .parts
            .iter()
            .map(|p| p.local_pos.y - p.spec.shape.contact_half_extent().y)
            .fold(f64::INFINITY, f64::min);

        let lowest_rel_com_y = min_local_y - self.com_local.y;
        self.pos = body_pos + DVec2::new(0.0, body_radius - lowest_rel_com_y);
        self.vel = body_vel;
        self.angle = 0.0;
        self.angular_vel = 0.0;
        self.landed = true;
    }

    /// Recomputes the vessel's center of mass and moment of inertia.
    ///
    /// # Errors
    ///
    /// Returns [`SimError::ZeroMassVessel`] if the vessel has no positive-mass parts.
    pub fn recompute_mass_properties(
        &mut self,
        compensate_world_pos: bool,
    ) -> Result<(), SimError> {
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

    #[inline]
    #[must_use]
    pub fn part_world_pos(&self, part: &PartState) -> DVec2 {
        self.pos + (part.local_pos - self.com_local).rotate(self.angle)
    }

    #[must_use]
    pub fn is_controllable(&self) -> bool {
        !self.crashed
            && self
                .parts
                .iter()
                .any(|p| matches!(p.spec.kind, PartKind::CommandPod(_)))
    }

    #[must_use]
    pub fn total_fuel_kg(&self) -> f64 {
        self.parts.iter().map(|p| p.fuel_kg).sum()
    }

    #[must_use]
    pub fn total_fuel_capacity_kg(&self) -> f64 {
        self.parts.iter().map(|p| p.spec.fuel_capacity_kg()).sum()
    }

    #[must_use]
    pub fn total_ablator_kg(&self) -> f64 {
        self.parts.iter().map(|p| p.ablator_kg).sum()
    }

    #[must_use]
    pub fn total_ablator_capacity_kg(&self) -> f64 {
        self.parts
            .iter()
            .filter_map(|p| match p.spec.kind {
                PartKind::HeatShield(hs) => Some(hs.ablator_kg),
                _ => None,
            })
            .sum()
    }

    pub fn peak_temperature_k(&self) -> f64 {
        self.parts
            .iter()
            .map(|p| p.temperature_k)
            .fold(AMBIENT_TEMP_K, f64::max)
    }

    pub fn peak_heat_flux(&self) -> f64 {
        self.parts
            .iter()
            .map(|p| p.heat_flux_w_m2)
            .fold(0.0, f64::max)
    }

    #[must_use]
    pub fn any_chute_armed(&self) -> bool {
        self.parts.iter().any(|p| p.chute_armed)
    }

    #[must_use]
    pub fn any_chute_deployed(&self) -> bool {
        self.parts.iter().any(|p| p.chute_deployed)
    }

    #[must_use]
    pub fn has_rcs(&self) -> bool {
        self.parts
            .iter()
            .any(|p| matches!(p.spec.kind, PartKind::Rcs(_)))
    }

    #[must_use]
    pub fn active_max_thrust_n(&self) -> f64 {
        self.parts
            .iter()
            .filter(|p| p.engine_active)
            .filter_map(|p| match p.spec.kind {
                PartKind::Engine(e) => Some(e.max_thrust_n),
                PartKind::SolidBooster(s) if p.fuel_kg > 0.0 => Some(s.max_thrust_n),
                _ => None,
            })
            .sum()
    }

    pub fn arm_all_parachutes(&mut self) -> usize {
        let mut armed = 0_usize;
        for p in &mut self.parts {
            if matches!(p.spec.kind, PartKind::Parachute(_)) && !p.chute_armed {
                p.chute_armed = true;
                armed += 1;
            }
        }
        armed
    }

    pub fn destroy_part_subtree(&mut self, root_id: u64) -> usize {
        let root_hit = self.parts.iter().any(|p| {
            p.instance_id == root_id
                && (p.parent_id.is_none() || matches!(p.spec.kind, PartKind::CommandPod(_)))
        });
        if root_hit {
            self.crashed = true;
            return 1;
        }

        let mut to_remove = HashSet::new();
        let mut queue = VecDeque::new();
        if to_remove.insert(root_id) {
            queue.push_back(root_id);
        }
        while let Some(curr) = queue.pop_front() {
            for p in &self.parts {
                if p.parent_id == Some(curr) && to_remove.insert(p.instance_id) {
                    queue.push_back(p.instance_id);
                }
            }
        }

        let removed = to_remove.len();
        self.parts.retain(|p| !to_remove.contains(&p.instance_id));
        if self.parts.is_empty() || self.recompute_mass_properties(true).is_err() {
            self.crashed = true;
        }
        removed
    }

    pub fn apply_thermal_damage(&mut self) -> usize {
        let overheated_ids: Vec<u64> = self
            .parts
            .iter()
            .filter(|p| p.temperature_k > p.spec.max_temp_k)
            .map(|p| p.instance_id)
            .collect();

        if overheated_ids.is_empty() {
            return 0;
        }

        let root_burned = self.parts.iter().any(|p| {
            overheated_ids.contains(&p.instance_id)
                && (p.parent_id.is_none() || matches!(p.spec.kind, PartKind::CommandPod(_)))
        });
        if root_burned {
            self.crashed = true;
            return overheated_ids.len();
        }

        let mut to_remove = HashSet::new();
        let mut queue = VecDeque::new();
        for id in overheated_ids {
            if to_remove.insert(id) {
                queue.push_back(id);
            }
        }
        while let Some(curr) = queue.pop_front() {
            for p in &self.parts {
                if p.parent_id == Some(curr) && to_remove.insert(p.instance_id) {
                    queue.push_back(p.instance_id);
                }
            }
        }

        let removed = to_remove.len();
        self.parts.retain(|p| !to_remove.contains(&p.instance_id));
        if self.parts.is_empty() || self.recompute_mass_properties(true).is_err() {
            self.crashed = true;
        }
        removed
    }

    #[must_use]
    pub fn available_reaction_torque(&self) -> f64 {
        if self.is_debris {
            return 0.0;
        }
        let pod_torque: f64 = self
            .parts
            .iter()
            .filter_map(|p| match p.spec.kind {
                PartKind::CommandPod(pod) => Some(pod.reaction_torque_nm),
                _ => None,
            })
            .sum();

        let rcs_torque = if self.control.rcs_enabled && self.total_fuel_kg() > 0.0 {
            self.parts
                .iter()
                .filter_map(|p| match p.spec.kind {
                    PartKind::Rcs(r) => {
                        let lever = (p.local_pos - self.com_local).length().max(0.6);
                        Some(r.thrust_n * lever)
                    }
                    _ => None,
                })
                .sum()
        } else {
            0.0
        };

        pod_torque + rcs_torque
    }

    #[allow(clippy::too_many_lines)]
    pub fn activate_next_stage(&mut self, next_vessel_id: &mut u64) -> StagingOutcome {
        let stage = self.current_stage;
        let mut ignited_engines = 0_usize;
        let mut armed_chutes = 0_usize;
        let mut fired_decouplers = Vec::new();

        for part in &mut self.parts {
            if part.stage != stage {
                continue;
            }
            match part.spec.kind {
                PartKind::Engine(_) | PartKind::SolidBooster(_) if !part.engine_active => {
                    part.engine_active = true;
                    ignited_engines += 1;
                }
                PartKind::Decoupler(_) if !part.decoupled => {
                    part.decoupled = true;
                    fired_decouplers.push(part.instance_id);
                }
                PartKind::Parachute(_) if !part.chute_armed => {
                    part.chute_armed = true;
                    armed_chutes += 1;
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

            let dec_snapshot = self
                .parts
                .iter()
                .find(|p| p.instance_id == dec_id)
                .map(|p| {
                    let imp = match p.spec.kind {
                        PartKind::Decoupler(d) => d.ejection_impulse_ns,
                        _ => 0.0,
                    };
                    (imp, p.local_rot, p.attach_kind)
                });
            let (ejection_impulse, dec_rot, dec_attach) =
                dec_snapshot.unwrap_or((0.0, 0.0, AttachKind::StackBottom));

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
                    p.active_gimbal_rad = 0.0;
                    debris_parts.push(p);
                } else {
                    parent_parts.push(p);
                }
            }
            self.parts = parent_parts;

            let debris_has_pod = debris_parts
                .iter()
                .any(|p| matches!(p.spec.kind, PartKind::CommandPod(_)));

            let old_com = self.com_local;
            let old_pos = self.pos;
            let old_vel = self.vel;
            let old_angle = self.angle;
            let old_omega = self.angular_vel;

            let mut debris = Self {
                id: *next_vessel_id,
                name: format!("{} Stage {}", self.name, stage + 1),
                is_debris: !debris_has_pod,
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

                let local_eject = match dec_attach {
                    AttachKind::RadialLeft => DVec2::new(-1.0, 0.0),
                    AttachKind::RadialRight => DVec2::new(1.0, 0.0),
                    AttachKind::StackTop => DVec2::new(0.0, 1.0),
                    AttachKind::StackBottom | AttachKind::Root => DVec2::new(0.0, -1.0),
                };
                let eject_dir = local_eject.rotate(old_angle + dec_rot);
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
            armed_chutes,
            spawned_debris,
        }
    }

    pub fn prepare_step_throttles(&mut self, dt: f64) {
        let mut scratch_fuel: Vec<f64> = self.parts.iter().map(|p| p.fuel_kg).collect();
        let _ = self.apply_propellant_step(dt, &mut scratch_fuel);
        let steer_cmd = self.effective_steer_command();
        for p in &mut self.parts {
            p.active_gimbal_rad = if let PartKind::Engine(eng) = p.spec.kind
                && p.engine_active
                && p.active_throttle > 0.0
            {
                let r_local = p.local_pos - self.com_local;
                let lever_sign = if r_local.y <= 0.0 { -1.0 } else { 1.0 };
                steer_cmd * eng.gimbal_range_rad * lever_sign
            } else {
                0.0
            };
        }
    }

    pub fn consume_propellant(&mut self, dt: f64) {
        let mut live_fuel: Vec<f64> = self.parts.iter().map(|p| p.fuel_kg).collect();
        let changed = self.apply_propellant_step(dt, &mut live_fuel);
        if changed {
            for (part, f) in self.parts.iter_mut().zip(live_fuel) {
                part.fuel_kg = f;
            }
            let _ = self.recompute_mass_properties(true);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn apply_propellant_step(&mut self, dt: f64, fuel_pool: &mut [f64]) -> bool {
        let cmd_throttle = if self.is_debris || self.crashed {
            0.0
        } else {
            self.control.throttle.clamp(0.0, 1.0)
        };

        for p in &mut self.parts {
            p.active_throttle = 0.0;
        }
        if dt <= 0.0 {
            return false;
        }

        let mut fuel_changed = false;

        let srb_indices: Vec<usize> = self
            .parts
            .iter()
            .enumerate()
            .filter_map(|(idx, p)| match p.spec.kind {
                PartKind::SolidBooster(_) if p.engine_active => Some(idx),
                _ => None,
            })
            .collect();

        for srb_idx in srb_indices {
            let srb = match self.parts.get(srb_idx) {
                Some(p) => match p.spec.kind {
                    PartKind::SolidBooster(s) => s,
                    _ => continue,
                },
                None => continue,
            };
            if srb.isp_s <= 0.0 || srb.max_thrust_n <= 0.0 {
                continue;
            }
            let dm_req = (srb.max_thrust_n / (srb.isp_s * G0)) * dt;
            if let Some(pool) = fuel_pool.get_mut(srb_idx)
                && *pool > 0.0
            {
                let take = pool.min(dm_req);
                *pool -= take;
                fuel_changed = true;
                let frac = (take / dm_req.max(1.0e-12)).clamp(0.0, 1.0);
                if let Some(p) = self.parts.get_mut(srb_idx) {
                    p.active_throttle = frac;
                }
            }
        }

        if cmd_throttle > 0.0 {
            let engine_indices: Vec<usize> = self
                .parts
                .iter()
                .enumerate()
                .filter_map(|(idx, p)| match p.spec.kind {
                    PartKind::Engine(_) if p.engine_active => Some(idx),
                    _ => None,
                })
                .collect();

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
                    if let Some(tank_fuel) = fuel_pool.get_mut(t_idx)
                        && *tank_fuel > 0.0
                    {
                        let take = tank_fuel.min(remaining);
                        *tank_fuel -= take;
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
        }

        if !self.is_debris
            && !self.crashed
            && self.control.rcs_enabled
            && self.control.rcs_translate.length_squared() > 1.0e-4
        {
            let trans_mag = self.control.rcs_translate.x.abs().clamp(0.0, 1.0)
                + self.control.rcs_translate.y.abs().clamp(0.0, 1.0);
            let rcs_indices: Vec<usize> = self
                .parts
                .iter()
                .enumerate()
                .filter_map(|(idx, p)| match p.spec.kind {
                    PartKind::Rcs(_) => Some(idx),
                    _ => None,
                })
                .collect();

            for r_idx in rcs_indices {
                let (r_id, r_spec) = match self.parts.get(r_idx) {
                    Some(p) => match p.spec.kind {
                        PartKind::Rcs(r) => (p.instance_id, r),
                        _ => continue,
                    },
                    None => continue,
                };
                if r_spec.isp_s <= 0.0 || r_spec.thrust_n <= 0.0 {
                    continue;
                }
                let dm_req = (r_spec.thrust_n * trans_mag / (r_spec.isp_s * G0)) * dt;
                let mut remaining = dm_req;
                let mut tanks = self.reachable_tank_indices(r_id);
                if tanks.is_empty() {
                    tanks = self
                        .parts
                        .iter()
                        .enumerate()
                        .filter_map(|(idx, p)| {
                            matches!(p.spec.kind, PartKind::Tank(_)).then_some(idx)
                        })
                        .collect();
                }
                for t_idx in tanks {
                    if remaining <= 1.0e-12 {
                        break;
                    }
                    if let Some(tank_fuel) = fuel_pool.get_mut(t_idx)
                        && *tank_fuel > 0.0
                    {
                        let take = tank_fuel.min(remaining);
                        *tank_fuel -= take;
                        remaining -= take;
                        fuel_changed = true;
                    }
                }
                let frac = ((dm_req - remaining) / dm_req.max(1.0e-12)).clamp(0.0, 1.0);
                if let Some(p) = self.parts.get_mut(r_idx) {
                    p.active_throttle = frac;
                }
            }
        }

        fuel_changed
    }

    #[must_use]
    pub fn effective_steer_command(&self) -> f64 {
        let steer_cmd = if self.is_debris || self.crashed {
            0.0
        } else {
            self.control.steer.clamp(-1.0, 1.0)
        };

        let sas_torque_norm = if !self.is_debris && !self.crashed && self.control.sas_enabled {
            if steer_cmd.abs() > 0.05 {
                (-self.angular_vel * 0.45).clamp(-0.5, 0.5)
            } else if self.control.sas_mode != SasMode::Stability
                && let Some(target_ang) = self.control.sas_target_angle
            {
                let err = normalize_angle(target_ang - self.angle);
                (err * 3.8 - self.angular_vel * 3.6).clamp(-1.0, 1.0)
            } else {
                (-self.angular_vel * 3.2).clamp(-1.0, 1.0)
            }
        } else {
            0.0
        };

        (steer_cmd + sas_torque_norm).clamp(-1.0, 1.0)
    }

    #[must_use]
    pub fn compute_thrust_and_control_wrench(&self) -> (DVec2, f64) {
        let mut net_force = DVec2::ZERO;
        let mut net_torque = 0.0;

        let total_steer = self.effective_steer_command();
        net_torque += total_steer * self.available_reaction_torque();

        for part in &self.parts {
            match part.spec.kind {
                PartKind::Engine(eng) if part.engine_active && part.active_throttle > 0.0 => {
                    let r_local = part.local_pos - self.com_local;
                    let lever_sign = if r_local.y <= 0.0 { -1.0 } else { 1.0 };
                    let gimbal_angle = total_steer * eng.gimbal_range_rad * lever_sign;
                    let local_thrust_dir = DVec2::Y.rotate(part.local_rot + gimbal_angle);
                    let force_local = local_thrust_dir * (eng.max_thrust_n * part.active_throttle);
                    net_torque += r_local.cross(force_local);
                    net_force += force_local.rotate(self.angle);
                }
                PartKind::SolidBooster(srb) if part.engine_active && part.active_throttle > 0.0 => {
                    let r_local = part.local_pos - self.com_local;
                    let local_thrust_dir = DVec2::Y.rotate(part.local_rot);
                    let force_local = local_thrust_dir * (srb.max_thrust_n * part.active_throttle);
                    net_torque += r_local.cross(force_local);
                    net_force += force_local.rotate(self.angle);
                }
                PartKind::Rcs(rcs)
                    if !self.is_debris
                        && !self.crashed
                        && self.control.rcs_enabled
                        && part.active_throttle > 0.0 =>
                {
                    let cmd = DVec2::new(
                        self.control.rcs_translate.x.clamp(-1.0, 1.0),
                        self.control.rcs_translate.y.clamp(-1.0, 1.0),
                    );
                    let f_local = cmd * (rcs.thrust_n * part.active_throttle);
                    net_force += f_local.rotate(self.angle);
                }
                _ => {}
            }
        }

        (net_force, net_torque)
    }

    pub fn wrap_angle(&mut self) {
        self.angle = normalize_angle(self.angle);
    }

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
            if matches!(curr.spec.kind, PartKind::Decoupler(_))
                && !curr.crossfeed
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
    #![allow(clippy::expect_used)]

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
        assert_eq!(out0.spawned_debris.len(), 0);

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

    #[test]
    fn radial_decoupler_ejects_laterally_not_downward() {
        let cat = default_part_catalog();
        let mut bp = VesselBlueprint::new("RadialEject");
        let pod = bp.add_root("command_pod", 1);
        let tank = bp.attach_child(
            pod,
            "tank_small",
            AttachKind::StackBottom,
            DVec2::new(0.0, -1.5),
            0.0,
            1,
        );
        let pylon = bp.attach_child(
            tank,
            "decoupler_radial",
            AttachKind::RadialLeft,
            DVec2::new(-0.785, -1.5),
            0.0,
            0,
        );
        bp.attach_child(
            pylon,
            "srb_radial",
            AttachKind::RadialLeft,
            DVec2::new(-1.42, -1.5),
            0.0,
            0,
        );

        let mut vessel = Vessel::from_blueprint(1, &bp, &cat).expect("valid craft");
        let mut next_id = 2;
        let outcome = vessel.activate_next_stage(&mut next_id);
        assert_eq!(outcome.spawned_debris.len(), 1);
        let booster = &outcome.spawned_debris[0];
        assert!(
            booster.vel.x < -0.1 && booster.vel.y.abs() < 1.0e-6,
            "expected leftward lateral ejection, got {:?}",
            booster.vel
        );
    }
}
