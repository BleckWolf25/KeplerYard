# KeplerYard Accuracy Contract

This document defines the physical formulas used by the simulation and the
approximations that remain under development.

## Exact or direct formulas

All two-body formulas below assume a spherically symmetric primary, a
negligible-mass orbiting body, and no atmospheric or third-body perturbations.

- Standard gravitational parameter: `mu = G * mass`
- Newtonian N-body acceleration (vector form, summed over all attracting
  bodies `i`): `a = sum_i( -G * m_i * r_vec_i / |r_vec_i|^3 )`, where `r_vec_i`
  is the vector from body `i` to the simulated body. The scalar magnitude from
  a single body is `a = G * m / r^2`.
- Circular orbital velocity: `v = sqrt(mu / r)`
- Escape velocity: `v = sqrt(2 * mu / r)`
- Orbital period: `T = 2*pi*sqrt(a^3/mu)` for any bound Keplerian orbit with
  semi-major axis `a`. For circular motion `r = a`, giving
  `T = 2*pi*sqrt(r^3/mu)`.
- Specific orbital energy: `epsilon = v^2/2 - mu/r` (negative for bound
  orbits; equals `-mu/(2*a)` for any conic).
- Ideal rocket equation: `delta_v = Isp * g0 * ln(m0/m1)`, where
  `g0 = 9.80665 m/s^2` is the defined standard gravity, not local gravity.

## Numerical methods

- Flight integration uses symplectic velocity-Verlet substeps at a fixed
  substep rate of 120 Hz (`dt = 1/120 s`). Velocity Verlet is symplectic only
  for conservative forces: coasting phases in pure gravity preserve energy,
  while drag, thrust, and heating are non-conservative and must not be assumed
  to satisfy the symplectic property or the conservation tests below.
- High time-warp coasting uses bounded adaptive substeps.
- Finite-difference conservation tests compare energy, momentum, and angular
  momentum. These tests apply only to conservative (gravity-only) segments.
- Orbital elements are osculating two-body elements relative to the dominant
  instantaneous attractor. The attractor-selection rule must be documented
  (e.g., largest `mu/r^2` versus sphere-of-influence switching) because the
  choice determines where element discontinuities appear.

## Atmospheric approximation

The current density model is exponential:

`rho(h) = rho0 * exp(-h/H)`

The scale height `H` and reference surface density `rho0` are named
body-specific parameters (for Earth with sea-level `rho0`, `H ~ 8.5 km`).
This is not a full standard-atmosphere table. Pressure and speed of sound use
a dry-air ideal-gas approximation with a constant temperature:

- Dry-air gas constant: `R = 287.05287 J/(kg*K)`
- Heat-capacity ratio: `gamma = 1.4`
- Reference temperature: `T = 288.15 K` (sea-level standard, held constant)
- Pressure: `p = rho * R * T`
- Speed of sound: `a = sqrt(gamma * R * T)`

Consequences of the constant-temperature assumption, which implementers and
test authors must be aware of:

- Pressure decays with the same scale height as density (`p/p0 = rho/rho0`).
- The speed of sound is a constant `~ 340.3 m/s` at all altitudes; Mach-based
  drag regimes do not vary with altitude in this model.

This isothermal model deviates from the lapse-rate troposphere of the 1976 US
Standard Atmosphere (6.5 K/km). It is suitable for the current prototype, but
it must be replaced by body-specific temperature profiles before real-world
claims are made.

## Tuned or approximation-only models

The following remain engineering approximations until replaced and validated:

- Exponential atmospheric density (per-body `H` and `rho0` require sources)
- Sutton-Graves convective heating: `q = k * sqrt(rho / R_n) * v^3`. The
  coefficient `k` depends on atmospheric composition (Earth:
  `k ~ 1.74e-4` in SI with `q` in `W/m^2`), so every non-Earth atmosphere
  needs its own referenced `k` value.
- Effective radiative cooling coefficient
- Ablation latent heat
- Simplified drag-area and lift calculations
- Kepler Prime and Moon development body parameters
- Maneuver and Hohmann estimates in a planar two-body model

## Required validation

Every formula or model should have:

1. A named formula or standard reference (e.g., 1976 US Standard Atmosphere
   for the ideal-gas constants; Sutton-Graves 1961 for the heating model).
2. A unit test with an independent analytical result.
3. A documented range of validity (altitude band, speed range, time span).
4. A clearly labeled approximation when empirical coefficients are used.
5. For conservation tests, an explicit statement of which force model the
   invariant is expected to hold under.
