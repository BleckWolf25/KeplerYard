//! @file state.rs
//!
//! @version 1.0.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Shared game states, floating-origin resources, and craft blueprint state
//!
//! @description
//! Defines the Bevy `GameMode` state machine (`Editor` vs `Flight`), the shared live
//! `ActiveBlueprint` edited in the VAB and launched to the pad, the `FloatingOrigin` resource
//! bridging `f64` simulation coordinates to `f32` Bevy rendering, and time-warp control state.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use bevy::prelude::*;
use sim_core::{DVec2, PartCatalog, VesselBlueprint, default_part_catalog, starter_two_stage_blueprint};

// ---------- CONSTANTS

pub const TIME_WARP_LEVELS: [f64; 6] = [1.0, 2.0, 5.0, 15.0, 50.0, 150.0];

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
                "tank_large" => PartVisualStyle {
                    base_color: [0.84, 0.86, 0.90, 1.0],
                    accent_color: [0.98, 0.56, 0.14, 1.0],
                    emissive_strength: 1.4,
                },
                "engine_booster" => PartVisualStyle {
                    base_color: [0.22, 0.24, 0.29, 1.0],
                    accent_color: [1.00, 0.48, 0.12, 1.0],
                    emissive_strength: 2.2,
                },
                "engine_orbital" => PartVisualStyle {
                    base_color: [0.28, 0.31, 0.37, 1.0],
                    accent_color: [0.30, 0.88, 1.00, 1.0],
                    emissive_strength: 2.0,
                },
                "decoupler_stack" => PartVisualStyle {
                    base_color: [0.36, 0.39, 0.44, 1.0],
                    accent_color: [1.00, 0.80, 0.18, 1.0],
                    emissive_strength: 1.7,
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
        let blueprint = starter_two_stage_blueprint(&cat);
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
            .init_resource::<FlightClock>();
    }
}

// ---------- EXPORTS
