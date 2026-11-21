//! @file lib.rs
//!
//! @version 1.0.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Root crate module for the headless f64 orbital physics and rocket engineering core
//!
//! @description
//! Exposes pure-Rust SI-unit orbital mechanics, celestial body definitions, part and vessel
//! assembly models, staging and decoupler splitting, crossfeed fuel draining, symplectic leapfrog
//! integration, and live trajectory prediction with zero external dependencies.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

// ---------- CONSTANTS

// ---------- TYPES

// ---------- DECLARATIONS / IMPLEMENTATION

pub mod body;
pub mod math;
pub mod parts;
pub mod sim;
pub mod vessel;

// ---------- EXPORTS

pub use body::{AtmosphereParams, CelestialBody, OrbitElements, dominant_body_elements};
pub use math::{DVec2, G0, GRAV_CONSTANT, SplitMix64, normalize_angle, rotate_vec2};
pub use parts::{
    AttachKind, BlueprintAnalysis, DecouplerSpec, EngineSpec, FinSpec, ParachuteSpec, PartCatalog,
    PartKind, PartShape, PartSpec, PlacedPart, PodSpec, RcsSpec, StageSummary, TankSpec,
    VesselBlueprint, analyze_blueprint, default_part_catalog, starter_two_stage_blueprint,
};
pub use sim::{
    ApsisKind, ApsisMarker, PredictionOptions, SimConfig, SimWorld, TrajectoryPrediction,
};
pub use vessel::{FlightControlInput, PartState, SimError, StagingOutcome, Vessel, VesselId};
