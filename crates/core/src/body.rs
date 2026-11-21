//! @file body.rs
//!
//! @version 1.2.0
//! @author `BleckWolf25`
//! @license MIT
//!
//! @summary Celestial body models, Moon Selene, atmospheres, orbital elements, and Hohmann transfer planning
//!
//! @description
//! Defines massive gravitating bodies (`Kepler Prime` and moon `Selene`) with surface radii,
//! zero-drift barycentric system setup, exponential atmosphere profiles, Sphere of Influence radius
//! calculations, two-body osculating orbital elements, and analytical Hohmann transfer planners.
//!
//! @since 21/11/2026
//! @updated 21/11/2026
// ---------- IMPORTS
use crate::math::{DVec2, GRAV_CONSTANT, TAU_F64, normalize_angle};
use core::f64::consts::PI;

// ---------- CONSTANTS
/// Radius of primary planet Kepler Prime in meters (600 km scaled Earth-surface gravity world).
pub const KEPLER_PRIME_RADIUS_M: f64 = 600_000.0;

/// Surface gravitational acceleration of Kepler Prime (m / s^2).
pub const KEPLER_PRIME_SURFACE_G: f64 = 9.81;

/// Orbital radius of moon Selene around Kepler Prime (3,600 km).
pub const SELENE_ORBIT_RADIUS_M: f64 = 3_600_000.0;

/// Radius of moon Selene in meters (120 km).
pub const SELENE_RADIUS_M: f64 = 120_000.0;

/// Surface gravitational acceleration of moon Selene (m / s^2).
pub const SELENE_SURFACE_G: f64 = 1.63;

// ---------- TYPES

/// Exponential atmosphere density profile (`rho = sea_level_density * exp(-altitude / scale_height)`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtmosphereParams {
    pub sea_level_density_kg_m3: f64,
    pub scale_height_m: f64,
    pub max_altitude_m: f64,
}

/// A massive celestial body participating in full N-body gravitation.
#[derive(Clone, Debug, PartialEq)]
pub struct CelestialBody {
    pub id: u32,
    pub name: String,
    pub mass_kg: f64,
    pub radius_m: f64,
    pub pos: DVec2,
    pub vel: DVec2,
    pub atmosphere: Option<AtmosphereParams>,
}

/// Osculating two-body Keplerian orbital summary relative to a dominant body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitElements {
    pub body_id: u32,
    pub altitude_m: f64,
    pub speed_m_s: f64,
    pub specific_energy_j_kg: f64,
    pub eccentricity: f64,
    pub semi_major_axis_m: f64,
    pub apoapsis_alt_m: f64,
    pub periapsis_alt_m: f64,
    pub period_s: Option<f64>,
    pub bound: bool,
}

/// Analytical Hohmann transfer estimate from current parking orbit around parent to target moon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransferPlanEstimate {
    pub target_body_id: u32,
    pub dv1_m_s: f64,
    pub dv2_capture_m_s: f64,
    pub transfer_time_s: f64,
    pub ideal_phase_rad: f64,
    pub current_phase_rad: f64,
    pub phase_error_rad: f64,
    pub lead_time_until_window_s: f64,
}

// ---------- DECLARATIONS / IMPLEMENTATION

impl AtmosphereParams {
    /// Exponential atmospheric density at `altitude_m` above surface.
    #[must_use]
    pub fn density_at(&self, altitude_m: f64) -> f64 {
        if altitude_m > self.max_altitude_m || self.scale_height_m <= 0.0 {
            return 0.0;
        }
        let h = altitude_m.max(0.0);
        self.sea_level_density_kg_m3 * (-h / self.scale_height_m).exp()
    }
}

impl CelestialBody {
    /// Standard gravitational parameter `mu = G * M` in m^3 / s^2.
    #[inline]
    #[must_use]
    pub fn mu(&self) -> f64 {
        GRAV_CONSTANT * self.mass_kg
    }

    /// Laplace sphere of influence radius around `parent`.
    #[inline]
    #[must_use]
    pub fn soi_radius_around(&self, parent: &Self) -> f64 {
        let r = self.pos.distance(parent.pos).max(self.radius_m);
        let ratio = (self.mass_kg / parent.mass_kg.max(1.0)).max(1.0e-12);
        r * ratio.powf(0.4)
    }

    /// Constructs the default homeworld "Kepler Prime" centered at origin with 9.81 m/s^2 surface g.
    #[must_use]
    pub fn kepler_prime() -> Self {
        let mu = KEPLER_PRIME_SURFACE_G * KEPLER_PRIME_RADIUS_M * KEPLER_PRIME_RADIUS_M;
        Self {
            id: 0,
            name: String::from("Kepler Prime"),
            mass_kg: mu / GRAV_CONSTANT,
            radius_m: KEPLER_PRIME_RADIUS_M,
            pos: DVec2::ZERO,
            vel: DVec2::ZERO,
            atmosphere: Some(AtmosphereParams {
                sea_level_density_kg_m3: 1.225,
                scale_height_m: 5_600.0,
                max_altitude_m: 70_000.0,
            }),
        }
    }

    /// Constructs the second celestial body "Selene" in circular prograde orbit around `planet`.
    #[must_use]
    pub fn selene(planet: &Self) -> Self {
        let mu_selene = SELENE_SURFACE_G * SELENE_RADIUS_M * SELENE_RADIUS_M;
        let v_orb = (planet.mu() / SELENE_ORBIT_RADIUS_M).sqrt();
        let init_angle = 105.0_f64.to_radians();
        let pos_rel = DVec2::new(init_angle.cos(), init_angle.sin()) * SELENE_ORBIT_RADIUS_M;
        let vel_rel = DVec2::new(-init_angle.sin(), init_angle.cos()) * v_orb;

        Self {
            id: 1,
            name: String::from("Selene"),
            mass_kg: mu_selene / GRAV_CONSTANT,
            radius_m: SELENE_RADIUS_M,
            pos: planet.pos + pos_rel,
            vel: planet.vel + vel_rel,
            atmosphere: None,
        }
    }

    /// Constructs the Kepler Prime + Selene two-body pair in a zero-drift barycentric frame.
    #[must_use]
    pub fn kepler_selene_system() -> [Self; 2] {
        let mut planet = Self::kepler_prime();
        let mut moon = Self::selene(&planet);
        let total_mass = (planet.mass_kg + moon.mass_kg).max(1.0);
        let bary_vel = moon.vel * (moon.mass_kg / total_mass);
        planet.vel -= bary_vel;
        moon.vel -= bary_vel;
        [planet, moon]
    }
}

/// Returns the dominant gravitational attractor for a point at `vessel_pos`.
#[must_use]
pub fn dominant_body(bodies: &[CelestialBody], vessel_pos: DVec2) -> Option<&CelestialBody> {
    bodies.iter().max_by(|a, b| {
        let ga = a.mu() / a.pos.distance(vessel_pos).powi(2).max(1.0);
        let gb = b.mu() / b.pos.distance(vessel_pos).powi(2).max(1.0);
        ga.total_cmp(&gb)
    })
}

/// Computes osculating two-body Keplerian orbital elements relative to the strongest attractor.
#[must_use]
pub fn dominant_body_elements(
    bodies: &[CelestialBody],
    vessel_pos: DVec2,
    vessel_vel: DVec2,
) -> Option<OrbitElements> {
    let body = dominant_body(bodies, vessel_pos)?;

    let r_vec = vessel_pos - body.pos;
    let v_vec = vessel_vel - body.vel;
    let r = r_vec.length().max(1.0e-3);
    let v_sq = v_vec.length_squared();
    let v = v_sq.sqrt();
    let mu = body.mu();

    let altitude_m = r - body.radius_m;
    let specific_energy_j_kg = 0.5 * v_sq - mu / r;
    let h_z = r_vec.cross(v_vec);

    let rv_dot = r_vec.dot(v_vec);
    let e_vec = (r_vec * (v_sq - mu / r) - v_vec * rv_dot) / mu.max(1.0);
    let eccentricity = e_vec.length();

    let bound = specific_energy_j_kg < -1.0e-6;
    let semi_major_axis_m = if specific_energy_j_kg.abs() > 1.0e-9 {
        -mu / (2.0 * specific_energy_j_kg)
    } else {
        f64::INFINITY
    };

    let (periapsis_r, apoapsis_r, period_s) = if bound && semi_major_axis_m.is_finite() {
        let pe_r = semi_major_axis_m * (1.0 - eccentricity).max(0.0);
        let ap_r = semi_major_axis_m * (1.0 + eccentricity);
        let period = TAU_F64 * (semi_major_axis_m.powi(3) / mu).max(0.0).sqrt();
        (pe_r, ap_r, Some(period))
    } else {
        let p = (h_z * h_z) / mu.max(1.0);
        let pe_r = p / (1.0 + eccentricity).max(1.0e-9);
        (pe_r, f64::INFINITY, None)
    };

    Some(OrbitElements {
        body_id: body.id,
        altitude_m,
        speed_m_s: v,
        specific_energy_j_kg,
        eccentricity,
        semi_major_axis_m,
        apoapsis_alt_m: apoapsis_r - body.radius_m,
        periapsis_alt_m: periapsis_r - body.radius_m,
        period_s,
        bound,
    })
}

/// Computes an analytical Hohmann transfer estimate from current orbit around primary `bodies[0]`
/// to orbiting `target_body_id`.
#[must_use]
pub fn compute_hohmann_transfer(
    bodies: &[CelestialBody],
    vessel_pos: DVec2,
    vessel_vel: DVec2,
    target_body_id: u32,
) -> Option<TransferPlanEstimate> {
    let primary = bodies.first()?;
    let target = bodies.iter().find(|b| b.id == target_body_id)?;
    if target.id == primary.id {
        return None;
    }

    let r_vec = vessel_pos - primary.pos;
    let v_vec = vessel_vel - primary.vel;
    let r1 = r_vec.length().max(primary.radius_m + 10_000.0);
    let t_rel = target.pos - primary.pos;
    let r2 = t_rel.length().max(r1 + 100_000.0);

    let mu_p = primary.mu();
    let v1 = v_vec.length().max((mu_p / r1).sqrt());
    let a_tx = f64::midpoint(r1, r2);
    let v_tx_peri = (mu_p * (2.0 / r1 - 1.0 / a_tx)).max(0.0).sqrt();
    let v_tx_apo = (mu_p * (2.0 / r2 - 1.0 / a_tx)).max(0.0).sqrt();
    let v2_target = (mu_p / r2).sqrt();

    let dv1_m_s = (v_tx_peri - v1).max(0.0);
    let v_inf = (v2_target - v_tx_apo).abs();
    let r_cap = target.radius_m + 30_000.0;
    let v_hyp = (v_inf * v_inf + 2.0 * target.mu() / r_cap).sqrt();
    let v_circ_cap = (target.mu() / r_cap).sqrt();
    let dv2_capture_m_s = (v_hyp - v_circ_cap).max(0.0);

    let transfer_time_s = PI * (a_tx.powi(3) / mu_p).max(0.0).sqrt();
    let ideal_phase_rad = normalize_angle(PI * (1.0 - (a_tx / r2).powf(1.5)));

    let vessel_ang = r_vec.y.atan2(r_vec.x);
    let target_ang = t_rel.y.atan2(t_rel.x);
    let current_phase_rad = normalize_angle(target_ang - vessel_ang);

    let prograde_sign = if r_vec.cross(v_vec) >= 0.0 { 1.0 } else { -1.0 };
    let omega1 = prograde_sign * (mu_p / r1.powi(3)).sqrt();
    let omega2 = (mu_p / r2.powi(3)).sqrt();
    let rel_omega = omega1 - omega2;

    let phase_error_rad = normalize_angle(current_phase_rad - ideal_phase_rad);
    let lead_time_until_window_s = if rel_omega.abs() > 1.0e-9 {
        let delta = if rel_omega > 0.0 {
            (current_phase_rad - ideal_phase_rad).rem_euclid(TAU_F64)
        } else {
            (ideal_phase_rad - current_phase_rad).rem_euclid(TAU_F64)
        };
        delta / rel_omega.abs()
    } else {
        0.0
    };

    Some(TransferPlanEstimate {
        target_body_id,
        dv1_m_s,
        dv2_capture_m_s,
        transfer_time_s,
        ideal_phase_rad,
        current_phase_rad,
        phase_error_rad,
        lead_time_until_window_s,
    })
}

// ---------- TESTS
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn circular_orbit_elements_match_analytical_values() {
        let planet = CelestialBody::kepler_prime();
        let r = planet.radius_m + 100_000.0;
        let v_circ = (planet.mu() / r).sqrt();
        let elems = dominant_body_elements(&[planet], DVec2::new(r, 0.0), DVec2::new(0.0, v_circ));
        assert!(elems.is_some());
        if let Some(el) = elems {
            assert!(el.bound);
            assert!(el.eccentricity < 1.0e-12);
            assert!((el.apoapsis_alt_m - 100_000.0).abs() < 1.0e-4);
            assert!((el.periapsis_alt_m - 100_000.0).abs() < 1.0e-4);
            assert!((el.altitude_m - 100_000.0).abs() < 1.0e-4);
        }
    }

    #[test]
    fn selene_hohmann_transfer_estimates_match_orbital_mechanics() {
        let planet = CelestialBody::kepler_prime();
        let moon = CelestialBody::selene(&planet);
        let r1 = planet.radius_m + 100_000.0;
        let v1 = (planet.mu() / r1).sqrt();
        let plan =
            compute_hohmann_transfer(&[planet, moon], DVec2::new(r1, 0.0), DVec2::new(0.0, v1), 1)
                .expect("transfer plan computed");
        assert!((plan.dv1_m_s - 660.3).abs() < 5.0);
        assert!((plan.dv2_capture_m_s - 307.2).abs() < 5.0);
        assert!(plan.transfer_time_s > 5_000.0);
    }

    #[test]
    fn barycentric_system_has_zero_net_momentum() {
        let [planet, moon] = CelestialBody::kepler_selene_system();
        let p_tot = planet.vel * planet.mass_kg + moon.vel * moon.mass_kg;
        assert!(p_tot.length() < 1.0e-6);
    }
}
