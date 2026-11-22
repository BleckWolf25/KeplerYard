//! @file lib.rs
//!
//! @version 1.5.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Root crate module for the headless f64 orbital physics, reentry, and engineering core
//!
//! @description
//! Exposes pure-Rust SI-unit N-body orbital mechanics, celestial bodies (Kepler Prime and the Moon),
//! transfer planning estimates, part and vessel assembly models, staging and crossfeed draining,
//! atmospheric aerodynamics and ablative reentry heating, symplectic leapfrog integration, live
//! trajectory prediction, and bit-exact binary save/load codecs with zero external dependencies.
//!
//! @since 21/11/2026
//! @updated 22/11/2026
// The project header uses bare author metadata rather than doc-markdown code formatting.
#![allow(clippy::doc_markdown)]
// ---------- DECLARATIONS / IMPLEMENTATION
pub mod body;
pub mod math;
pub mod parts;
pub mod save;
pub mod sim;
pub mod vessel;

// ---------- EXPORTS
pub use body::{
    AtmosphereParams, AtmosphericState, CelestialBody, CelestialBodySpec, CelestialCatalog,
    DRY_AIR_GAS_CONSTANT_J_KGK, DRY_AIR_HEAT_CAPACITY_RATIO, EARTH_MOON_DISTANCE_M,
    EARTH_MOON_RADIUS_M, EARTH_MOON_SURFACE_G, EARTH_RADIUS_M, EARTH_SURFACE_G,
    KEPLER_PRIME_RADIUS_M, KEPLER_PRIME_SURFACE_G, MOON_ORBIT_RADIUS_M, MOON_RADIUS_M,
    MOON_SURFACE_G, OrbitElements, SEA_LEVEL_STANDARD_TEMP_K, TransferPlanEstimate,
    compute_hohmann_transfer, dominant_body_elements,
};
pub use math::{DVec2, G0, GRAV_CONSTANT, SplitMix64, normalize_angle, rotate_vec2};
pub use parts::{
    AttachKind, BlueprintAnalysis, DecouplerSpec, EngineSpec, FinSpec, HeatShieldSpec,
    LandingLegSpec, NoseConeSpec, ParachuteSpec, PartCatalog, PartCategory, PartKind, PartShape,
    PartSpec, PlacedPart, PodSpec, RcsSpec, SrbSpec, StageSummary, TankSpec, VesselBlueprint,
    analyze_blueprint, default_part_catalog, lunar_capable_blueprint, starter_two_stage_blueprint,
};
pub use save::{
    SaveError, load_blueprint_bytes, load_world_bytes, save_blueprint_bytes, save_world_bytes,
};
pub use sim::{
    ApsisKind, ApsisMarker, ManeuverPlan, PredictionOptions, SimConfig, SimWorld,
    TrajectoryPrediction,
};
pub use vessel::{
    FlightControlInput, PartState, SasMode, SimError, StagingOutcome, Vessel, VesselId,
};
