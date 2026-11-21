//! @file save.rs
//!
//! @version 1.3.0
//! @author `BleckWolf25`
//! @license MIT
//!
//! @summary Bit-exact binary serialization and deserialization for `SimWorld` and `VesselBlueprint`
//!
//! @description
//! Encodes and decodes complete `SimWorld` and `VesselBlueprint` state (including `Booster` shape
//! and `mirrored` flags) into a versioned little-endian binary stream preserving exact IEEE-754
//! `f64` bit patterns with zero external serialization crates in `core`.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use crate::body::{AtmosphereParams, CelestialBody};
use crate::math::DVec2;
use crate::parts::{
    AttachKind, DecouplerSpec, EngineSpec, FinSpec, HeatShieldSpec, LandingLegSpec, NoseConeSpec,
    ParachuteSpec, PartCategory, PartKind, PartShape, PartSpec, PlacedPart, PodSpec, RcsSpec,
    SrbSpec, TankSpec, VesselBlueprint,
};
use crate::sim::{SimConfig, SimWorld};
use crate::vessel::{FlightControlInput, PartState, SasMode, Vessel};
use core::fmt;

// ---------- CONSTANTS

const MAGIC_WORLD: &[u8; 8] = b"KYWORLD3";
const MAGIC_BLUEPRINT: &[u8; 8] = b"KYCRAFT3";

// ---------- TYPES

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveError {
    InvalidMagic,
    UnexpectedEof,
    InvalidUtf8,
    InvalidTag(&'static str, u8),
    TrailingBytes,
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

// ---------- DECLARATIONS / IMPLEMENTATION

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => write!(f, "invalid or outdated Kepler Yard save header"),
            Self::UnexpectedEof => write!(f, "unexpected end of save data"),
            Self::InvalidUtf8 => write!(f, "invalid UTF-8 string in save data"),
            Self::InvalidTag(field, tag) => write!(f, "invalid {field} tag {tag} in save data"),
            Self::TrailingBytes => write!(f, "unexpected trailing bytes in save data"),
        }
    }
}

impl std::error::Error for SaveError {}

#[must_use]
pub fn save_blueprint_bytes(bp: &VesselBlueprint) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    out.extend_from_slice(MAGIC_BLUEPRINT);
    write_str(&mut out, &bp.name);
    write_u64(&mut out, bp.next_instance_id);
    write_u32(&mut out, bp.parts.len() as u32);
    for p in &bp.parts {
        write_placed_part(&mut out, p);
    }
    out
}

/// Loads a vessel blueprint from its binary representation.
///
/// # Errors
///
/// Returns [`SaveError`] if the data has an invalid signature, malformed payload, invalid UTF-8,
/// or trailing bytes.
pub fn load_blueprint_bytes(bytes: &[u8]) -> Result<VesselBlueprint, SaveError> {
    let mut r = Reader::new(bytes);
    if r.read_bytes(8)? != MAGIC_BLUEPRINT {
        return Err(SaveError::InvalidMagic);
    }
    let name = r.read_string()?;
    let next_instance_id = r.read_u64()?;
    let count = r.read_u32()? as usize;
    let mut parts = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        parts.push(read_placed_part(&mut r)?);
    }
    if r.remaining() != 0 {
        return Err(SaveError::TrailingBytes);
    }
    Ok(VesselBlueprint {
        name,
        parts,
        next_instance_id,
    })
}

#[must_use]
pub fn save_world_bytes(world: &SimWorld) -> Vec<u8> {
    let mut out = Vec::with_capacity(4096);
    out.extend_from_slice(MAGIC_WORLD);
    write_f64(&mut out, world.time_s);
    write_opt_u64(&mut out, world.active_vessel_id);
    write_u64(&mut out, world.next_vessel_id);
    write_config(&mut out, world.config);

    write_u32(&mut out, world.bodies.len() as u32);
    for body in &world.bodies {
        write_body(&mut out, body);
    }

    write_u32(&mut out, world.vessels.len() as u32);
    for vessel in &world.vessels {
        write_vessel(&mut out, vessel);
    }
    out
}

/// Loads a simulation world from its binary representation.
///
/// # Errors
///
/// Returns [`SaveError`] if the data has an invalid signature, malformed payload, invalid UTF-8,
/// or trailing bytes.
pub fn load_world_bytes(bytes: &[u8]) -> Result<SimWorld, SaveError> {
    let mut r = Reader::new(bytes);
    if r.read_bytes(8)? != MAGIC_WORLD {
        return Err(SaveError::InvalidMagic);
    }
    let time_s = r.read_f64()?;
    let active_vessel_id = r.read_opt_u64()?;
    let next_vessel_id = r.read_u64()?;
    let config = read_config(&mut r)?;

    let body_count = r.read_u32()? as usize;
    let mut bodies = Vec::with_capacity(body_count.min(256));
    for _ in 0..body_count {
        bodies.push(read_body(&mut r)?);
    }

    let vessel_count = r.read_u32()? as usize;
    let mut vessels = Vec::with_capacity(vessel_count.min(4096));
    for _ in 0..vessel_count {
        vessels.push(read_vessel(&mut r)?);
    }

    if r.remaining() != 0 {
        return Err(SaveError::TrailingBytes);
    }

    Ok(SimWorld {
        time_s,
        bodies,
        vessels,
        active_vessel_id,
        next_vessel_id,
        config,
    })
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    const fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], SaveError> {
        let end = self.pos.checked_add(len).ok_or(SaveError::UnexpectedEof)?;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or(SaveError::UnexpectedEof)?;
        self.pos = end;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8, SaveError> {
        let b = self.read_bytes(1)?;
        b.first().copied().ok_or(SaveError::UnexpectedEof)
    }

    fn read_bool(&mut self) -> Result<bool, SaveError> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(SaveError::InvalidTag("bool", other)),
        }
    }

    fn read_u32(&mut self) -> Result<u32, SaveError> {
        let slice = self.read_bytes(4)?;
        let arr: [u8; 4] = slice.try_into().map_err(|_| SaveError::UnexpectedEof)?;
        Ok(u32::from_le_bytes(arr))
    }

    fn read_u64(&mut self) -> Result<u64, SaveError> {
        let slice = self.read_bytes(8)?;
        let arr: [u8; 8] = slice.try_into().map_err(|_| SaveError::UnexpectedEof)?;
        Ok(u64::from_le_bytes(arr))
    }

    fn read_f64(&mut self) -> Result<f64, SaveError> {
        Ok(f64::from_bits(self.read_u64()?))
    }

    fn read_dvec2(&mut self) -> Result<DVec2, SaveError> {
        Ok(DVec2::new(self.read_f64()?, self.read_f64()?))
    }

    fn read_opt_u64(&mut self) -> Result<Option<u64>, SaveError> {
        if self.read_bool()? {
            Ok(Some(self.read_u64()?))
        } else {
            Ok(None)
        }
    }

    fn read_opt_f64(&mut self) -> Result<Option<f64>, SaveError> {
        if self.read_bool()? {
            Ok(Some(self.read_f64()?))
        } else {
            Ok(None)
        }
    }

    fn read_string(&mut self) -> Result<String, SaveError> {
        let len = self.read_u32()? as usize;
        let bytes = self.read_bytes(len)?;
        let s = core::str::from_utf8(bytes).map_err(|_| SaveError::InvalidUtf8)?;
        Ok(s.to_owned())
    }
}

fn write_u8(out: &mut Vec<u8>, v: u8) {
    out.push(v);
}

fn write_bool(out: &mut Vec<u8>, v: bool) {
    out.push(u8::from(v));
}

fn write_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn write_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn write_f64(out: &mut Vec<u8>, v: f64) {
    write_u64(out, v.to_bits());
}

fn write_dvec2(out: &mut Vec<u8>, v: DVec2) {
    write_f64(out, v.x);
    write_f64(out, v.y);
}

fn write_opt_u64(out: &mut Vec<u8>, v: Option<u64>) {
    match v {
        Some(x) => {
            write_bool(out, true);
            write_u64(out, x);
        }
        None => write_bool(out, false),
    }
}

fn write_opt_f64(out: &mut Vec<u8>, v: Option<f64>) {
    match v {
        Some(x) => {
            write_bool(out, true);
            write_f64(out, x);
        }
        None => write_bool(out, false),
    }
}

fn write_str(out: &mut Vec<u8>, s: &str) {
    write_u32(out, s.len() as u32);
    out.extend_from_slice(s.as_bytes());
}

fn write_attach_kind(out: &mut Vec<u8>, kind: AttachKind) {
    let tag = match kind {
        AttachKind::Root => 0,
        AttachKind::StackTop => 1,
        AttachKind::StackBottom => 2,
        AttachKind::RadialLeft => 3,
        AttachKind::RadialRight => 4,
    };
    write_u8(out, tag);
}

fn read_attach_kind(r: &mut Reader<'_>) -> Result<AttachKind, SaveError> {
    match r.read_u8()? {
        0 => Ok(AttachKind::Root),
        1 => Ok(AttachKind::StackTop),
        2 => Ok(AttachKind::StackBottom),
        3 => Ok(AttachKind::RadialLeft),
        4 => Ok(AttachKind::RadialRight),
        other => Err(SaveError::InvalidTag("AttachKind", other)),
    }
}

fn write_config(out: &mut Vec<u8>, c: SimConfig) {
    write_f64(out, c.fixed_dt);
    write_f64(out, c.max_coast_dt);
    write_bool(out, c.atmosphere_drag);
    write_bool(out, c.reentry_heating);
    write_bool(out, c.mutual_n_body);
}

fn read_config(r: &mut Reader<'_>) -> Result<SimConfig, SaveError> {
    Ok(SimConfig {
        fixed_dt: r.read_f64()?,
        max_coast_dt: r.read_f64()?,
        atmosphere_drag: r.read_bool()?,
        reentry_heating: r.read_bool()?,
        mutual_n_body: r.read_bool()?,
    })
}

fn write_body(out: &mut Vec<u8>, b: &CelestialBody) {
    write_u32(out, b.id);
    write_str(out, &b.name);
    write_f64(out, b.mass_kg);
    write_f64(out, b.radius_m);
    write_dvec2(out, b.pos);
    write_dvec2(out, b.vel);
    match b.atmosphere {
        Some(atmo) => {
            write_bool(out, true);
            write_f64(out, atmo.sea_level_density_kg_m3);
            write_f64(out, atmo.scale_height_m);
            write_f64(out, atmo.max_altitude_m);
        }
        None => write_bool(out, false),
    }
}

fn read_body(r: &mut Reader<'_>) -> Result<CelestialBody, SaveError> {
    let id = r.read_u32()?;
    let name = r.read_string()?;
    let mass_kg = r.read_f64()?;
    let radius_m = r.read_f64()?;
    let pos = r.read_dvec2()?;
    let vel = r.read_dvec2()?;
    let atmosphere = if r.read_bool()? {
        Some(AtmosphereParams {
            sea_level_density_kg_m3: r.read_f64()?,
            scale_height_m: r.read_f64()?,
            max_altitude_m: r.read_f64()?,
        })
    } else {
        None
    };
    Ok(CelestialBody {
        id,
        name,
        mass_kg,
        radius_m,
        pos,
        vel,
        atmosphere,
    })
}

fn write_placed_part(out: &mut Vec<u8>, p: &PlacedPart) {
    write_u64(out, p.instance_id);
    write_str(out, &p.spec_id);
    write_opt_u64(out, p.parent_id);
    write_attach_kind(out, p.attach_kind);
    write_dvec2(out, p.local_pos);
    write_f64(out, p.local_rot);
    write_u32(out, p.stage);
    write_bool(out, p.crossfeed);
    write_bool(out, p.mirrored);
}

fn read_placed_part(r: &mut Reader<'_>) -> Result<PlacedPart, SaveError> {
    Ok(PlacedPart {
        instance_id: r.read_u64()?,
        spec_id: r.read_string()?,
        parent_id: r.read_opt_u64()?,
        attach_kind: read_attach_kind(r)?,
        local_pos: r.read_dvec2()?,
        local_rot: r.read_f64()?,
        stage: r.read_u32()?,
        crossfeed: r.read_bool()?,
        mirrored: r.read_bool()?,
    })
}

#[allow(clippy::too_many_lines)]
fn write_part_spec(out: &mut Vec<u8>, s: &PartSpec) {
    write_str(out, &s.id);
    write_str(out, &s.display_name);
    let cat_tag = match s.category {
        PartCategory::Command => 0,
        PartCategory::Propulsion => 1,
        PartCategory::FuelTanks => 2,
        PartCategory::AeroAndThermal => 3,
        PartCategory::Structural => 4,
    };
    write_u8(out, cat_tag);
    write_f64(out, s.dry_mass_kg);

    match s.shape {
        PartShape::RoundedRect {
            width,
            height,
            corner_radius,
        } => {
            write_u8(out, 0);
            write_f64(out, width);
            write_f64(out, height);
            write_f64(out, corner_radius);
        }
        PartShape::Cone {
            bottom_width,
            top_width,
            height,
        } => {
            write_u8(out, 1);
            write_f64(out, bottom_width);
            write_f64(out, top_width);
            write_f64(out, height);
        }
        PartShape::Nozzle {
            mount_width,
            throat_width,
            bell_width,
            height,
        } => {
            write_u8(out, 2);
            write_f64(out, mount_width);
            write_f64(out, throat_width);
            write_f64(out, bell_width);
            write_f64(out, height);
        }
        PartShape::Fin {
            root_chord,
            tip_chord,
            span,
            sweep,
        } => {
            write_u8(out, 3);
            write_f64(out, root_chord);
            write_f64(out, tip_chord);
            write_f64(out, span);
            write_f64(out, sweep);
        }
        PartShape::Dome { width, height } => {
            write_u8(out, 4);
            write_f64(out, width);
            write_f64(out, height);
        }
        PartShape::LandingStrut {
            width,
            height,
            foot_span,
        } => {
            write_u8(out, 5);
            write_f64(out, width);
            write_f64(out, height);
            write_f64(out, foot_span);
        }
        PartShape::Booster {
            width,
            height,
            nozzle_width,
        } => {
            write_u8(out, 6);
            write_f64(out, width);
            write_f64(out, height);
            write_f64(out, nozzle_width);
        }
    }

    match s.kind {
        PartKind::CommandPod(p) => {
            write_u8(out, 0);
            write_f64(out, p.reaction_torque_nm);
            write_bool(out, p.crewed);
        }
        PartKind::Tank(t) => {
            write_u8(out, 1);
            write_f64(out, t.fuel_capacity_kg);
        }
        PartKind::Engine(e) => {
            write_u8(out, 2);
            write_f64(out, e.max_thrust_n);
            write_f64(out, e.isp_s);
            write_f64(out, e.gimbal_range_rad);
        }
        PartKind::Decoupler(d) => {
            write_u8(out, 3);
            write_f64(out, d.ejection_impulse_ns);
            write_bool(out, d.crossfeed);
        }
        PartKind::Fin(f) => {
            write_u8(out, 4);
            write_f64(out, f.lift_area_m2);
        }
        PartKind::Rcs(rc) => {
            write_u8(out, 5);
            write_f64(out, rc.thrust_n);
            write_f64(out, rc.isp_s);
        }
        PartKind::Parachute(ch) => {
            write_u8(out, 6);
            write_f64(out, ch.deployed_drag_area_m2);
        }
        PartKind::HeatShield(hs) => {
            write_u8(out, 7);
            write_f64(out, hs.ablator_kg);
            write_f64(out, hs.wake_shield_factor);
        }
        PartKind::SolidBooster(srb) => {
            write_u8(out, 8);
            write_f64(out, srb.max_thrust_n);
            write_f64(out, srb.isp_s);
            write_f64(out, srb.fuel_capacity_kg);
        }
        PartKind::LandingLeg(leg) => {
            write_u8(out, 9);
            write_f64(out, leg.impact_tolerance_m_s);
        }
        PartKind::NoseCone(nc) => {
            write_u8(out, 10);
            write_f64(out, nc.drag_reduction);
        }
    }

    write_bool(out, s.stack_top);
    write_bool(out, s.stack_bottom);
    write_bool(out, s.radial_attach);
    write_f64(out, s.drag_coeff);
    write_f64(out, s.max_impact_m_s);
    write_f64(out, s.max_temp_k);
}

#[allow(clippy::too_many_lines)]
fn read_part_spec(r: &mut Reader<'_>) -> Result<PartSpec, SaveError> {
    let id = r.read_string()?;
    let display_name = r.read_string()?;
    let category = match r.read_u8()? {
        0 => PartCategory::Command,
        1 => PartCategory::Propulsion,
        2 => PartCategory::FuelTanks,
        3 => PartCategory::AeroAndThermal,
        4 => PartCategory::Structural,
        other => return Err(SaveError::InvalidTag("PartCategory", other)),
    };
    let dry_mass_kg = r.read_f64()?;

    let shape = match r.read_u8()? {
        0 => PartShape::RoundedRect {
            width: r.read_f64()?,
            height: r.read_f64()?,
            corner_radius: r.read_f64()?,
        },
        1 => PartShape::Cone {
            bottom_width: r.read_f64()?,
            top_width: r.read_f64()?,
            height: r.read_f64()?,
        },
        2 => PartShape::Nozzle {
            mount_width: r.read_f64()?,
            throat_width: r.read_f64()?,
            bell_width: r.read_f64()?,
            height: r.read_f64()?,
        },
        3 => PartShape::Fin {
            root_chord: r.read_f64()?,
            tip_chord: r.read_f64()?,
            span: r.read_f64()?,
            sweep: r.read_f64()?,
        },
        4 => PartShape::Dome {
            width: r.read_f64()?,
            height: r.read_f64()?,
        },
        5 => PartShape::LandingStrut {
            width: r.read_f64()?,
            height: r.read_f64()?,
            foot_span: r.read_f64()?,
        },
        6 => PartShape::Booster {
            width: r.read_f64()?,
            height: r.read_f64()?,
            nozzle_width: r.read_f64()?,
        },
        other => return Err(SaveError::InvalidTag("PartShape", other)),
    };

    let kind = match r.read_u8()? {
        0 => PartKind::CommandPod(PodSpec {
            reaction_torque_nm: r.read_f64()?,
            crewed: r.read_bool()?,
        }),
        1 => PartKind::Tank(TankSpec {
            fuel_capacity_kg: r.read_f64()?,
        }),
        2 => PartKind::Engine(EngineSpec {
            max_thrust_n: r.read_f64()?,
            isp_s: r.read_f64()?,
            gimbal_range_rad: r.read_f64()?,
        }),
        3 => PartKind::Decoupler(DecouplerSpec {
            ejection_impulse_ns: r.read_f64()?,
            crossfeed: r.read_bool()?,
        }),
        4 => PartKind::Fin(FinSpec {
            lift_area_m2: r.read_f64()?,
        }),
        5 => PartKind::Rcs(RcsSpec {
            thrust_n: r.read_f64()?,
            isp_s: r.read_f64()?,
        }),
        6 => PartKind::Parachute(ParachuteSpec {
            deployed_drag_area_m2: r.read_f64()?,
        }),
        7 => PartKind::HeatShield(HeatShieldSpec {
            ablator_kg: r.read_f64()?,
            wake_shield_factor: r.read_f64()?,
        }),
        8 => PartKind::SolidBooster(SrbSpec {
            max_thrust_n: r.read_f64()?,
            isp_s: r.read_f64()?,
            fuel_capacity_kg: r.read_f64()?,
        }),
        9 => PartKind::LandingLeg(LandingLegSpec {
            impact_tolerance_m_s: r.read_f64()?,
        }),
        10 => PartKind::NoseCone(NoseConeSpec {
            drag_reduction: r.read_f64()?,
        }),
        other => return Err(SaveError::InvalidTag("PartKind", other)),
    };

    Ok(PartSpec {
        id,
        display_name,
        category,
        dry_mass_kg,
        shape,
        kind,
        stack_top: r.read_bool()?,
        stack_bottom: r.read_bool()?,
        radial_attach: r.read_bool()?,
        drag_coeff: r.read_f64()?,
        max_impact_m_s: r.read_f64()?,
        max_temp_k: r.read_f64()?,
    })
}

fn write_part_state(out: &mut Vec<u8>, p: &PartState) {
    write_u64(out, p.instance_id);
    write_opt_u64(out, p.parent_id);
    write_attach_kind(out, p.attach_kind);
    write_bool(out, p.mirrored);
    write_part_spec(out, &p.spec);
    write_dvec2(out, p.local_pos);
    write_f64(out, p.local_rot);
    write_u32(out, p.stage);
    write_bool(out, p.crossfeed);
    write_f64(out, p.fuel_kg);
    write_f64(out, p.ablator_kg);
    write_bool(out, p.engine_active);
    write_bool(out, p.decoupled);
    write_f64(out, p.active_throttle);
    write_f64(out, p.active_gimbal_rad);
    write_bool(out, p.chute_armed);
    write_bool(out, p.chute_deployed);
    write_f64(out, p.temperature_k);
    write_f64(out, p.heat_flux_w_m2);
}

fn read_part_state(r: &mut Reader<'_>) -> Result<PartState, SaveError> {
    Ok(PartState {
        instance_id: r.read_u64()?,
        parent_id: r.read_opt_u64()?,
        attach_kind: read_attach_kind(r)?,
        mirrored: r.read_bool()?,
        spec: read_part_spec(r)?,
        local_pos: r.read_dvec2()?,
        local_rot: r.read_f64()?,
        stage: r.read_u32()?,
        crossfeed: r.read_bool()?,
        fuel_kg: r.read_f64()?,
        ablator_kg: r.read_f64()?,
        engine_active: r.read_bool()?,
        decoupled: r.read_bool()?,
        active_throttle: r.read_f64()?,
        active_gimbal_rad: r.read_f64()?,
        chute_armed: r.read_bool()?,
        chute_deployed: r.read_bool()?,
        temperature_k: r.read_f64()?,
        heat_flux_w_m2: r.read_f64()?,
    })
}

fn write_sas_mode(out: &mut Vec<u8>, mode: SasMode) {
    let tag = match mode {
        SasMode::Stability => 0,
        SasMode::Prograde => 1,
        SasMode::Retrograde => 2,
        SasMode::RadialOut => 3,
        SasMode::Maneuver => 4,
    };
    write_u8(out, tag);
}

fn read_sas_mode(r: &mut Reader<'_>) -> Result<SasMode, SaveError> {
    match r.read_u8()? {
        0 => Ok(SasMode::Stability),
        1 => Ok(SasMode::Prograde),
        2 => Ok(SasMode::Retrograde),
        3 => Ok(SasMode::RadialOut),
        4 => Ok(SasMode::Maneuver),
        other => Err(SaveError::InvalidTag("SasMode", other)),
    }
}

fn write_vessel(out: &mut Vec<u8>, v: &Vessel) {
    write_u64(out, v.id);
    write_str(out, &v.name);
    write_bool(out, v.is_debris);
    write_dvec2(out, v.pos);
    write_dvec2(out, v.vel);
    write_f64(out, v.angle);
    write_f64(out, v.angular_vel);
    write_u32(out, v.parts.len() as u32);
    for p in &v.parts {
        write_part_state(out, p);
    }
    write_dvec2(out, v.com_local);
    write_f64(out, v.total_mass_kg);
    write_f64(out, v.moi_kg_m2);
    write_u32(out, v.current_stage);
    write_u32(out, v.max_stage);
    write_bool(out, v.landed);
    write_bool(out, v.crashed);
    write_f64(out, v.control.throttle);
    write_f64(out, v.control.steer);
    write_bool(out, v.control.sas_enabled);
    write_sas_mode(out, v.control.sas_mode);
    write_opt_f64(out, v.control.sas_target_angle);
    write_bool(out, v.control.rcs_enabled);
    write_dvec2(out, v.control.rcs_translate);
}

fn read_vessel(r: &mut Reader<'_>) -> Result<Vessel, SaveError> {
    let id = r.read_u64()?;
    let name = r.read_string()?;
    let is_debris = r.read_bool()?;
    let pos = r.read_dvec2()?;
    let vel = r.read_dvec2()?;
    let angle = r.read_f64()?;
    let angular_vel = r.read_f64()?;
    let count = r.read_u32()? as usize;
    let mut parts = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        parts.push(read_part_state(r)?);
    }
    Ok(Vessel {
        id,
        name,
        is_debris,
        pos,
        vel,
        angle,
        angular_vel,
        parts,
        com_local: r.read_dvec2()?,
        total_mass_kg: r.read_f64()?,
        moi_kg_m2: r.read_f64()?,
        current_stage: r.read_u32()?,
        max_stage: r.read_u32()?,
        landed: r.read_bool()?,
        crashed: r.read_bool()?,
        control: FlightControlInput {
            throttle: r.read_f64()?,
            steer: r.read_f64()?,
            sas_enabled: r.read_bool()?,
            sas_mode: read_sas_mode(r)?,
            sas_target_angle: r.read_opt_f64()?,
            rcs_enabled: r.read_bool()?,
            rcs_translate: r.read_dvec2()?,
        },
    })
}

// ---------- TESTS

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::parts::{default_part_catalog, lunar_capable_blueprint};

    #[test]
    fn blueprint_save_load_roundtrips_exactly() {
        let cat = default_part_catalog();
        let mut bp = lunar_capable_blueprint(&cat);
        bp.attach_child(
            1,
            "landing_leg",
            AttachKind::RadialLeft,
            DVec2::new(-0.9, -0.5),
            0.0,
            0,
        );
        let bytes = save_blueprint_bytes(&bp);
        let loaded = load_blueprint_bytes(&bytes).expect("valid blueprint bytes");
        assert_eq!(bp, loaded);
    }

    #[test]
    fn bit_exact_save_load_preserves_future_trajectory() {
        let cat = default_part_catalog();
        let bp = lunar_capable_blueprint(&cat);
        let mut world_a =
            SimWorld::with_kepler_system_vessel(&bp, &cat).expect("valid system world");
        world_a.trigger_active_stage();
        for tick in 0..900 {
            if let Some(v) = world_a.active_vessel_mut() {
                v.control.throttle = 1.0;
                v.control.steer = if (200..420).contains(&tick) {
                    -0.2
                } else {
                    0.0
                };
            }
            world_a.step(1.0 / 60.0);
        }

        let bytes = save_world_bytes(&world_a);
        let mut world_b = load_world_bytes(&bytes).expect("valid world save");

        for _ in 0..3_600 {
            world_a.step(1.0 / 60.0);
            world_b.step(1.0 / 60.0);
        }

        let va = world_a.active_vessel().expect("vessel A");
        let vb = world_b.active_vessel().expect("vessel B");
        assert_eq!(va.pos.x.to_bits(), vb.pos.x.to_bits());
        assert_eq!(va.pos.y.to_bits(), vb.pos.y.to_bits());
        assert_eq!(va.vel.x.to_bits(), vb.vel.x.to_bits());
        assert_eq!(va.vel.y.to_bits(), vb.vel.y.to_bits());
        assert_eq!(va.angle.to_bits(), vb.angle.to_bits());
        assert_eq!(va.total_fuel_kg().to_bits(), vb.total_fuel_kg().to_bits());
        assert_eq!(
            va.peak_temperature_k().to_bits(),
            vb.peak_temperature_k().to_bits()
        );
    }
}
