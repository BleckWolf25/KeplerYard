//! @file part_catalog.rs
//!
//! @version 1.0.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Hot-reloadable RON part asset loader and live catalog synchronization
//!
//! @description
//! Implements Bevy 0.20 `AssetLoader` for `*.part.ron` files via `serde` and `ron`, loads all
//! part specifications at startup, and reacts to `AssetEvent<PartSpecAsset>` modifications so
//! tweaking mass, thrust, Isp, or dimensions on disk updates the editor and visuals immediately.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use crate::state::{ActiveBlueprint, LivePartCatalog, PartVisualStyle};
use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::prelude::*;
use serde::Deserialize;
use sim_core::{
    DecouplerSpec, EngineSpec, FinSpec, ParachuteSpec, PartKind, PartShape, PartSpec, PodSpec,
    RcsSpec, TankSpec,
};
use std::fmt;

// ---------- CONSTANTS

const PART_ASSET_PATHS: [&str; 7] = [
    "parts/command_pod.part.ron",
    "parts/tank_small.part.ron",
    "parts/tank_large.part.ron",
    "parts/engine_orbital.part.ron",
    "parts/engine_booster.part.ron",
    "parts/decoupler_stack.part.ron",
    "parts/fin_basic.part.ron",
];

// ---------- TYPES

#[derive(Deserialize, Clone, Debug)]
#[serde(rename = "PartSpecRon")]
pub struct PartSpecRon {
    pub id: String,
    pub display_name: String,
    pub dry_mass_kg: f64,
    pub shape: PartShapeRon,
    pub kind: PartKindRon,
    pub stack_top: bool,
    pub stack_bottom: bool,
    pub radial_attach: bool,
    pub drag_coeff: f64,
    pub max_impact_m_s: f64,
    pub base_color: [f32; 4],
    pub accent_color: [f32; 4],
    pub emissive_strength: f32,
}

#[derive(Deserialize, Clone, Debug)]
pub enum PartShapeRon {
    RoundedRect {
        width: f64,
        height: f64,
        corner_radius: f64,
    },
    Cone {
        bottom_width: f64,
        top_width: f64,
        height: f64,
    },
    Nozzle {
        mount_width: f64,
        throat_width: f64,
        bell_width: f64,
        height: f64,
    },
    Fin {
        root_chord: f64,
        tip_chord: f64,
        span: f64,
        sweep: f64,
    },
}

#[derive(Deserialize, Clone, Debug)]
pub enum PartKindRon {
    CommandPod {
        reaction_torque_nm: f64,
    },
    Tank {
        fuel_capacity_kg: f64,
    },
    Engine {
        max_thrust_n: f64,
        isp_s: f64,
        gimbal_range_rad: f64,
    },
    Decoupler {
        ejection_impulse_ns: f64,
        crossfeed: bool,
    },
    Fin {
        lift_area_m2: f64,
    },
    Rcs {
        thrust_n: f64,
        isp_s: f64,
    },
    Parachute {
        deployed_drag_area_m2: f64,
    },
}

#[derive(Asset, TypePath, Clone, Debug)]
pub struct PartSpecAsset {
    pub spec: PartSpec,
    pub style: PartVisualStyle,
}

#[derive(Default, TypePath)]
pub struct PartRonLoader;

#[derive(Debug)]
pub enum PartLoadError {
    Io(std::io::Error),
    Utf8(std::str::Utf8Error),
    Ron(ron::error::SpannedError),
}

#[derive(Resource, Default)]
struct LoadedPartHandles {
    handles: Vec<Handle<PartSpecAsset>>,
}

pub struct PartCatalogPlugin;

// ---------- DECLARATIONS / IMPLEMENTATION

impl fmt::Display for PartLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "failed to read part RON file: {e}"),
            Self::Utf8(e) => write!(f, "part RON file is not valid UTF-8: {e}"),
            Self::Ron(e) => write!(f, "failed to parse part RON: {e}"),
        }
    }
}

impl std::error::Error for PartLoadError {}

impl From<std::io::Error> for PartLoadError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<std::str::Utf8Error> for PartLoadError {
    fn from(value: std::str::Utf8Error) -> Self {
        Self::Utf8(value)
    }
}

impl From<ron::error::SpannedError> for PartLoadError {
    fn from(value: ron::error::SpannedError) -> Self {
        Self::Ron(value)
    }
}

impl From<PartShapeRon> for PartShape {
    fn from(value: PartShapeRon) -> Self {
        match value {
            PartShapeRon::RoundedRect {
                width,
                height,
                corner_radius,
            } => Self::RoundedRect {
                width,
                height,
                corner_radius,
            },
            PartShapeRon::Cone {
                bottom_width,
                top_width,
                height,
            } => Self::Cone {
                bottom_width,
                top_width,
                height,
            },
            PartShapeRon::Nozzle {
                mount_width,
                throat_width,
                bell_width,
                height,
            } => Self::Nozzle {
                mount_width,
                throat_width,
                bell_width,
                height,
            },
            PartShapeRon::Fin {
                root_chord,
                tip_chord,
                span,
                sweep,
            } => Self::Fin {
                root_chord,
                tip_chord,
                span,
                sweep,
            },
        }
    }
}

impl From<PartKindRon> for PartKind {
    fn from(value: PartKindRon) -> Self {
        match value {
            PartKindRon::CommandPod { reaction_torque_nm } => {
                Self::CommandPod(PodSpec { reaction_torque_nm })
            }
            PartKindRon::Tank { fuel_capacity_kg } => Self::Tank(TankSpec { fuel_capacity_kg }),
            PartKindRon::Engine {
                max_thrust_n,
                isp_s,
                gimbal_range_rad,
            } => Self::Engine(EngineSpec {
                max_thrust_n,
                isp_s,
                gimbal_range_rad,
            }),
            PartKindRon::Decoupler {
                ejection_impulse_ns,
                crossfeed,
            } => Self::Decoupler(DecouplerSpec {
                ejection_impulse_ns,
                crossfeed,
            }),
            PartKindRon::Fin { lift_area_m2 } => Self::Fin(FinSpec { lift_area_m2 }),
            PartKindRon::Rcs { thrust_n, isp_s } => Self::Rcs(RcsSpec { thrust_n, isp_s }),
            PartKindRon::Parachute {
                deployed_drag_area_m2,
            } => Self::Parachute(ParachuteSpec {
                deployed_drag_area_m2,
            }),
        }
    }
}

impl AssetLoader for PartRonLoader {
    type Asset = PartSpecAsset;
    type Settings = ();
    type Error = PartLoadError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let text = core::str::from_utf8(&bytes)?;
        let parsed: PartSpecRon = ron::from_str(text)?;
        let spec = PartSpec {
            id: parsed.id,
            display_name: parsed.display_name,
            dry_mass_kg: parsed.dry_mass_kg,
            shape: parsed.shape.into(),
            kind: parsed.kind.into(),
            stack_top: parsed.stack_top,
            stack_bottom: parsed.stack_bottom,
            radial_attach: parsed.radial_attach,
            drag_coeff: parsed.drag_coeff,
            max_impact_m_s: parsed.max_impact_m_s,
        };
        let style = PartVisualStyle {
            base_color: parsed.base_color,
            accent_color: parsed.accent_color,
            emissive_strength: parsed.emissive_strength,
        };
        Ok(PartSpecAsset { spec, style })
    }

    fn extensions(&self) -> &[&str] {
        &["part.ron", "ron"]
    }
}

fn load_part_ron_assets(mut commands: Commands, asset_server: Res<AssetServer>) {
    let handles = PART_ASSET_PATHS
        .iter()
        .map(|path| asset_server.load::<PartSpecAsset>(*path))
        .collect();
    commands.insert_resource(LoadedPartHandles { handles });
}

fn sync_hot_reloaded_parts(
    mut events: MessageReader<AssetEvent<PartSpecAsset>>,
    assets: Res<Assets<PartSpecAsset>>,
    handles: Res<LoadedPartHandles>,
    mut catalog: ResMut<LivePartCatalog>,
    mut active_bp: ResMut<ActiveBlueprint>,
) {
    let mut updated = false;
    for ev in events.read() {
        match ev {
            AssetEvent::Added { id }
            | AssetEvent::Modified { id }
            | AssetEvent::LoadedWithDependencies { id } => {
                if let Some(asset) = assets.get(*id) {
                    catalog.visuals.insert(asset.spec.id.clone(), asset.style);
                    catalog.sim_catalog.insert(asset.spec.clone());
                    updated = true;
                }
            }
            AssetEvent::Removed { .. } | AssetEvent::Unused { .. } => {}
        }
    }

    if !updated {
        for handle in &handles.handles {
            if let Some(asset) = assets.get(handle)
                && catalog.sim_catalog.get(&asset.spec.id) != Some(&asset.spec)
            {
                catalog.visuals.insert(asset.spec.id.clone(), asset.style);
                catalog.sim_catalog.insert(asset.spec.clone());
                updated = true;
            }
        }
    }

    if updated {
        active_bp.dirty = true;
    }
}

impl Plugin for PartCatalogPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<PartSpecAsset>()
            .init_asset_loader::<PartRonLoader>()
            .init_resource::<LoadedPartHandles>()
            .add_systems(Startup, load_part_ron_assets)
            .add_systems(Update, sync_hot_reloaded_parts);
    }
}

// ---------- EXPORTS
