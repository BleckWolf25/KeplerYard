//! @file math.rs
//!
//! @version 1.0.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Double-precision 2D vector algebra, physical constants, and deterministic PRNG utilities
//!
//! @description
//! Provides SI physical constants, a self-contained f64 2D vector type, planar rotation and
//! cross-product operations, angle wrapping, and a seeded SplitMix64 generator for deterministic
//! simulation testing with zero external crate dependencies.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use core::f64::consts::PI;
use core::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

// ---------- CONSTANTS

/// Universal gravitational constant in m^3 / (kg * s^2).
pub const GRAV_CONSTANT: f64 = 6.674_30e-11;

/// Standard gravity used for specific impulse mass-flow conversion (m / s^2).
pub const G0: f64 = 9.806_65;

/// Full circle in radians (2 * pi).
pub const TAU_F64: f64 = core::f64::consts::TAU;

// ---------- TYPES

/// Double-precision 2D vector in SI meters, m/s, N, or dimensionless direction.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DVec2 {
    pub x: f64,
    pub y: f64,
}

/// Deterministic 64-bit SplitMix PRNG for reproducible tests and headless simulation seeding.
#[derive(Clone, Debug)]
pub struct SplitMix64 {
    state: u64,
}

// ---------- DECLARATIONS / IMPLEMENTATION

impl DVec2 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
    pub const X: Self = Self { x: 1.0, y: 0.0 };
    pub const Y: Self = Self { x: 0.0, y: 1.0 };

    #[inline]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    #[inline]
    pub fn dot(self, rhs: Self) -> f64 {
        self.x * rhs.x + self.y * rhs.y
    }

    /// Planar 2D scalar cross product (z-component of 3D cross product): x1*y2 - y1*x2.
    #[inline]
    pub fn cross(self, rhs: Self) -> f64 {
        self.x * rhs.y - self.y * rhs.x
    }

    #[inline]
    pub fn length_squared(self) -> f64 {
        self.dot(self)
    }

    #[inline]
    pub fn length(self) -> f64 {
        self.length_squared().sqrt()
    }

    #[inline]
    pub fn distance(self, rhs: Self) -> f64 {
        (self - rhs).length()
    }

    #[inline]
    pub fn normalize_or_zero(self) -> Self {
        let len = self.length();
        if len > 1.0e-15 && len.is_finite() {
            self / len
        } else {
            Self::ZERO
        }
    }

    /// Rotates counter-clockwise by `angle_rad` radians.
    #[inline]
    pub fn rotate(self, angle_rad: f64) -> Self {
        rotate_vec2(self, angle_rad)
    }

    /// Perpendicular vector rotated +90 degrees CCW: (-y, x).
    #[inline]
    pub fn perp(self) -> Self {
        Self::new(-self.y, self.x)
    }

    #[inline]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl Add for DVec2 {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl AddAssign for DVec2 {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl Sub for DVec2 {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl SubAssign for DVec2 {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

impl Neg for DVec2 {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

impl Mul<f64> for DVec2 {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f64) -> Self {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

impl Mul<DVec2> for f64 {
    type Output = DVec2;

    #[inline]
    fn mul(self, rhs: DVec2) -> DVec2 {
        DVec2::new(self * rhs.x, self * rhs.y)
    }
}

impl MulAssign<f64> for DVec2 {
    #[inline]
    fn mul_assign(&mut self, rhs: f64) {
        self.x *= rhs;
        self.y *= rhs;
    }
}

impl Div<f64> for DVec2 {
    type Output = Self;

    #[inline]
    fn div(self, rhs: f64) -> Self {
        Self::new(self.x / rhs, self.y / rhs)
    }
}

impl DivAssign<f64> for DVec2 {
    #[inline]
    fn div_assign(&mut self, rhs: f64) {
        self.x /= rhs;
        self.y /= rhs;
    }
}

/// Rotates a 2D vector counter-clockwise by `angle_rad` radians.
#[inline]
pub fn rotate_vec2(v: DVec2, angle_rad: f64) -> DVec2 {
    let (s, c) = angle_rad.sin_cos();
    DVec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// Wraps an angle into `[-PI, PI]`.
#[inline]
pub fn normalize_angle(mut angle: f64) -> f64 {
    angle = (angle + PI).rem_euclid(TAU_F64) - PI;
    if angle == -PI { PI } else { angle }
}

impl SplitMix64 {
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Generates a uniform f64 in `[0.0, 1.0)`.
    pub fn next_f64(&mut self) -> f64 {
        let bits = (self.next_u64() >> 11) as f64;
        bits * (1.0 / ((1_u64 << 53) as f64))
    }

    /// Generates a uniform f64 in `[min, max]`.
    pub fn range_f64(&mut self, min: f64, max: f64) -> f64 {
        min + (max - min) * self.next_f64()
    }
}

// ---------- TESTS

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_matches_rocket_up_convention() {
        let fwd = rotate_vec2(DVec2::Y, core::f64::consts::FRAC_PI_2);
        assert!((fwd.x + 1.0).abs() < 1.0e-14);
        assert!(fwd.y.abs() < 1.0e-14);
    }

    #[test]
    fn normalize_angle_wraps_into_pi_range() {
        let wrapped = normalize_angle(3.5 * PI);
        assert!((wrapped + 0.5 * PI).abs() < 1.0e-14);
    }
}
