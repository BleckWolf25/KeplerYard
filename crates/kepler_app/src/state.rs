//! @file state.rs
//!
//! @version 1.2.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Shared game states, floating-origin resources, camera focus, audio cue hooks, and warp
//!
//! @description
//! Defines the Bevy `GameMode` state machine (`Editor` vs `Flight`), the shared live
//! `ActiveBlueprint`, `FloatingOrigin`, `FlightClock` with physical vs orbital-rails warp caps,
//! `CameraFocusState`, and an `AudioCueQueue` ready for future sound and music synthesis.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use bevy::prelude::*;
use sim_core::{DVec2, PartCatalog, VesselBlueprint, default_part_catalog, lunar_capable_blueprint};

// ---------- CONSTANTS

pub const TIME_WARP_LEVELS: [f64; 8] = [1.0, 2.0, 4.0, 15.0, 50.0, 150.0, 500.0, 1500.0];
pub const PHYSICAL_WARP_MAX_INDEX: usize = 2;

// ---------- TYPES

#[derive(States, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GameMode {
    #[default]
    Editor,
    Flight,
}

#[derive(Resource, Clone, Debug)]
pub struct LivePartCatalog {
    pub sim_catalog: PartCatalog,
    pub visuals: std::collections::HashMap<String, PartVisualStyle>,
}

#[derive(Clone, Copy, Debug)]
pub struct PartVisualStyle {
    pub base_color: [f32; 4],
    pub accent_color: [f32; 4],
    pub emissive_strength: f32,
}

#[derive(Resource, Clone, Debug)]
pub struct ActiveBlueprint {
    pub blueprint: VesselBlueprint,
    pub selected_part_id: Option<u64>,
    pub active_palette_spec: String,
    pub dirty: bool,
}

#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct FloatingOrigin {
    pub origin_m: DVec2,
}

#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct FlightClock {
    pub warp_index: usize,
    pub paused: bool,
    pub physical_capped: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraFocusTarget {
    #[default]
    ActiveVessel,
    Body(u32),
}

#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct CameraFocusState {
    pub target: CameraFocusTarget,
}

/// Simulation audio cue events queued during flight for future sound & music playback.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AudioCue {
    StageSeparation,
    Touchdown { speed_m_s: f64 },
    PartDestroyed { thermal: bool },
    VesselCrashed,
    ChuteDeploy,
}

#[derive(Resource, Default, Debug)]
pub struct AudioCueQueue {
    pub cues: Vec<AudioCue>,
}

pub struct StatePlugin;

// ---------- DECLARATIONS / IMPLEMENTATION

impl Default for PartVisualStyle {
    fn default() -> Self {
        Self {
            base_color: [0.75, 0.78, 0.84, 1.0],
            accent_color: [0.20, 0.85, 1.00, 1.0],
            emissive_strength: 1.5,
        }
    }
}

impl Default for LivePartCatalog {
    fn default() -> Self {
        let sim_catalog = default_part_catalog();
        let mut visuals = std::collections::HashMap::new();
        for spec in sim_catalog.iter() {
            let style = match spec.id.as_str() {
                "command_pod" => PartVisualStyle {
                    base_color: [0.20, 0.24, 0.30, 1.0],
                    accent_color: [0.16, 0.85, 1.00, 1.0],
                    emissive_strength: 1.8,
                },
                "probe_core" => PartVisualStyle {
                    base_color: [0.24, 0.27, 0.34, 1.0],
                    accent_color: [0.98, 0.78, 0.24, 1.0],
                    emissive_strength: 2.1,
                },
                "parachute_mk1" => PartVisualStyle {
                    base_color: [0.78, 0.34, 0.22, 1.0],
                    accent_color: [1.00, 0.86, 0.52, 1.0],
                    emissive_strength: 1.5,
                },
                "heat_shield" => PartVisualStyle {
                    base_color: [0.26, 0.19, 0.16, 1.0],
                    accent_color: [1.00, 0.52, 0.18, 1.0],
                    emissive_strength: 1.6,
                },
                "nosecone" => PartVisualStyle {
                    base_color: [0.82, 0.84, 0.89, 1.0],
                    accent_color: [0.96, 0.44, 0.16, 1.0],
                    emissive_strength: 1.4,
                },
                "tank_large" => PartVisualStyle {
                    base_color: [0.84, 0.86, 0.90, 1.0],
                    accent_color: [0.98, 0.56, 0.14, 1.0],
                    emissive_strength: 1.4,
                },
                "tank_heavy" => PartVisualStyle {
                    base_color: [0.88, 0.89, 0.93, 1.0],
                    accent_color: [1.00, 0.42, 0.12, 1.0],
                    emissive_strength: 1.4,
                },
                "engine_booster" => PartVisualStyle {
                    base_color: [0.22, 0.24, 0.29, 1.0],
                    accent_color: [1.00, 0.48, 0.12, 1.0],
                    emissive_strength: 2.2,
                },
                "engine_heavy" => PartVisualStyle {
                    base_color: [0.19, 0.22, 0.27, 1.0],
                    accent_color: [1.00, 0.34, 0.10, 1.0],
                    emissive_strength: 2.4,
                },
                "engine_orbital" => PartVisualStyle {
                    base_color: [0.28, 0.31, 0.37, 1.0],
                    accent_color: [0.30, 0.88, 1.00, 1.0],
                    emissive_strength: 2.0,
                },
                "srb_radial" => PartVisualStyle {
                    base_color: [0.86, 0.87, 0.90, 1.0],
                    accent_color: [1.00, 0.36, 0.12, 1.0],
                    emissive_strength: 1.7,
                },
                "rcs_block" => PartVisualStyle {
                    base_color: [0.72, 0.76, 0.82, 1.0],
                    accent_color: [0.36, 0.94, 1.00, 1.0],
                    emissive_strength: 1.8,
                },
                "decoupler_stack" | "decoupler_radial" => PartVisualStyle {
                    base_color: [0.36, 0.39, 0.44, 1.0],
                    accent_color: [1.00, 0.80, 0.18, 1.0],
                    emissive_strength: 1.7,
                },
                "landing_leg" => PartVisualStyle {
                    base_color: [0.34, 0.37, 0.42, 1.0],
                    accent_color: [0.95, 0.68, 0.20, 1.0],
                    emissive_strength: 1.5,
                },
                _ => PartVisualStyle::default(),
            };
            visuals.insert(spec.id.clone(), style);
        }
        Self {
            sim_catalog,
            visuals,
        }
    }
}

impl Default for ActiveBlueprint {
    fn default() -> Self {
        let cat = default_part_catalog();
        let blueprint = lunar_capable_blueprint(&cat);
        let selected_part_id = blueprint.parts.first().map(|p| p.instance_id);
        Self {
            blueprint,
            selected_part_id,
            active_palette_spec: String::from("tank_small"),
            dirty: true,
        }
    }
}

impl FlightClock {
    pub fn warp_factor(&self) -> f64 {
        if self.paused {
            0.0
        } else {
            TIME_WARP_LEVELS
                .get(self.warp_index)
                .copied()
                .unwrap_or(1.0)
        }
    }
}

/// Converts `f64` world coordinates into floating-origin-relative `f32` coordinates.
#[inline]
pub fn world_to_render_vec2(world_m: DVec2, origin: FloatingOrigin) -> Vec2 {
    let rel = world_m - origin.origin_m;
    Vec2::new(rel.x as f32, rel.y as f32)
}

/// Constructs a uniform four-sided `BorderColor`.
#[inline]
pub const fn uniform_border(color: Color) -> BorderColor {
    BorderColor {
        top: color,
        right: color,
        bottom: color,
        left: color,
    }
}

impl Plugin for StatePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LivePartCatalog>()
            .init_resource::<ActiveBlueprint>()
            .init_resource::<FloatingOrigin>()
            .init_resource::<FlightClock>()
            .init_resource::<CameraFocusState>()
            .init_resource::<AudioCueQueue>();
    }
}

// ---------- EXPORTS
