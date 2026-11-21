//! @file body.rs
//!
//! @version 1.0.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Celestial body models, exponential atmospheres, and osculating orbital elements
//!
//! @description
//! Defines massive gravitating bodies with surface radii and optional exponential atmosphere
//! profiles, along with analytical two-body osculating orbital element calculation (apoapsis,
//! periapsis, eccentricity, specific energy, and orbital period) relative to a dominant body.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use crate::math::{DVec2, GRAV_CONSTANT, TAU_F64};

// ---------- CONSTANTS

/// Radius of primary planet Kepler Prime in meters (600 km scaled Earth-surface gravity world).
pub const KEPLER_PRIME_RADIUS_M: f64 = 600_000.0;

/// Surface gravitational acceleration of Kepler Prime (m / s^2).
pub const KEPLER_PRIME_SURFACE_G: f64 = 9.81;

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

// ---------- DECLARATIONS / IMPLEMENTATION

impl AtmosphereParams {
    /// Exponential atmospheric density at `altitude_m` above surface.
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
    pub fn mu(&self) -> f64 {
        GRAV_CONSTANT * self.mass_kg
    }

    /// Constructs the default homeworld "Kepler Prime" centered at origin with 9.81 m/s^2 surface g.
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
}

/// Computes osculating two-body Keplerian orbital elements relative to the strongest attractor.
pub fn dominant_body_elements(
    bodies: &[CelestialBody],
    vessel_pos: DVec2,
    vessel_vel: DVec2,
) -> Option<OrbitElements> {
    let body = bodies.iter().max_by(|a, b| {
        let ga = a.mu() / a.pos.distance(vessel_pos).powi(2).max(1.0);
        let gb = b.mu() / b.pos.distance(vessel_pos).powi(2).max(1.0);
        ga.total_cmp(&gb)
    })?;

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

// ---------- TESTS

#[cfg(test)]
mod tests {
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
}
