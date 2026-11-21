//! @file `part_catalog.rs`
//!
//! @version 1.3.0
//! @author `BleckWolf25`
//! @license MIT
//!
//! @summary Hot-reloadable RON part asset discovery, loader, and live catalog synchronization
//!
//! @description
//! Discovers all `*.part.ron` files in `assets/parts/` automatically (supporting `Booster` and
//! `LandingStrut` silhouettes) and implements Bevy 0.20 `AssetLoader` for `PartSpecAsset`.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use crate::state::{ActiveBlueprint, LivePartCatalog, PartVisualStyle};
use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::prelude::*;
use kepler_core::{
    DecouplerSpec, EngineSpec, FinSpec, HeatShieldSpec, LandingLegSpec, NoseConeSpec,
    ParachuteSpec, PartCategory, PartKind, PartShape, PartSpec, PodSpec, RcsSpec, SrbSpec,
    TankSpec,
};
use serde::Deserialize;
use std::fmt;
use std::path::{Path, PathBuf};

// ---------- CONSTANTS

const FALLBACK_PART_ASSET_PATHS: [&str; 17] = [
    "parts/command_pod.part.ron",
    "parts/probe_core.part.ron",
    "parts/parachute_mk1.part.ron",
    "parts/heat_shield.part.ron",
    "parts/nosecone.part.ron",
    "parts/tank_small.part.ron",
    "parts/tank_large.part.ron",
    "parts/tank_heavy.part.ron",
    "parts/engine_orbital.part.ron",
    "parts/engine_booster.part.ron",
    "parts/engine_heavy.part.ron",
    "parts/srb_radial.part.ron",
    "parts/rcs_block.part.ron",
    "parts/decoupler_stack.part.ron",
    "parts/decoupler_radial.part.ron",
    "parts/landing_leg.part.ron",
    "parts/fin_basic.part.ron",
];

// ---------- TYPES

#[derive(Deserialize, Clone, Debug)]
#[serde(rename = "PartSpecRon")]
pub struct PartSpecRon {
    pub id: String,
    pub display_name: String,
    pub category: PartCategoryRon,
    pub dry_mass_kg: f64,
    pub shape: PartShapeRon,
    pub kind: PartKindRon,
    pub stack_top: bool,
    pub stack_bottom: bool,
    pub radial_attach: bool,
    pub drag_coeff: f64,
    pub max_impact_m_s: f64,
    pub max_temp_k: f64,
    pub base_color: [f32; 4],
    pub accent_color: [f32; 4],
    pub emissive_strength: f32,
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub enum PartCategoryRon {
    Command,
    Propulsion,
    FuelTanks,
    AeroAndThermal,
    Structural,
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
    Dome {
        width: f64,
        height: f64,
    },
    LandingStrut {
        width: f64,
        height: f64,
        foot_span: f64,
    },
    Booster {
        width: f64,
        height: f64,
        nozzle_width: f64,
    },
}

#[derive(Deserialize, Clone, Debug)]
pub enum PartKindRon {
    CommandPod {
        reaction_torque_nm: f64,
        #[serde(default = "default_true")]
        crewed: bool,
    },
    Tank {
        fuel_capacity_kg: f64,
    },
    Engine {
        max_thrust_n: f64,
        isp_s: f64,
        gimbal_range_rad: f64,
    },
    SolidBooster {
        max_thrust_n: f64,
        isp_s: f64,
        fuel_capacity_kg: f64,
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
    HeatShield {
        ablator_kg: f64,
        wake_shield_factor: f64,
    },
    LandingLeg {
        impact_tolerance_m_s: f64,
    },
    NoseCone {
        drag_reduction: f64,
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

const fn default_true() -> bool {
    true
}

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

impl From<PartCategoryRon> for PartCategory {
    fn from(value: PartCategoryRon) -> Self {
        match value {
            PartCategoryRon::Command => Self::Command,
            PartCategoryRon::Propulsion => Self::Propulsion,
            PartCategoryRon::FuelTanks => Self::FuelTanks,
            PartCategoryRon::AeroAndThermal => Self::AeroAndThermal,
            PartCategoryRon::Structural => Self::Structural,
        }
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
            PartShapeRon::Dome { width, height } => Self::Dome { width, height },
            PartShapeRon::LandingStrut {
                width,
                height,
                foot_span,
            } => Self::LandingStrut {
                width,
                height,
                foot_span,
            },
            PartShapeRon::Booster {
                width,
                height,
                nozzle_width,
            } => Self::Booster {
                width,
                height,
                nozzle_width,
            },
        }
    }
}

impl From<PartKindRon> for PartKind {
    fn from(value: PartKindRon) -> Self {
        match value {
            PartKindRon::CommandPod {
                reaction_torque_nm,
                crewed,
            } => Self::CommandPod(PodSpec {
                reaction_torque_nm,
                crewed,
            }),
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
            PartKindRon::SolidBooster {
                max_thrust_n,
                isp_s,
                fuel_capacity_kg,
            } => Self::SolidBooster(SrbSpec {
                max_thrust_n,
                isp_s,
                fuel_capacity_kg,
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
            PartKindRon::HeatShield {
                ablator_kg,
                wake_shield_factor,
            } => Self::HeatShield(HeatShieldSpec {
                ablator_kg,
                wake_shield_factor,
            }),
            PartKindRon::LandingLeg {
                impact_tolerance_m_s,
            } => Self::LandingLeg(LandingLegSpec {
                impact_tolerance_m_s,
            }),
            PartKindRon::NoseCone { drag_reduction } => {
                Self::NoseCone(NoseConeSpec { drag_reduction })
            }
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
            category: parsed.category.into(),
            dry_mass_kg: parsed.dry_mass_kg,
            shape: parsed.shape.into(),
            kind: parsed.kind.into(),
            stack_top: parsed.stack_top,
            stack_bottom: parsed.stack_bottom,
            radial_attach: parsed.radial_attach,
            drag_coeff: parsed.drag_coeff,
            max_impact_m_s: parsed.max_impact_m_s,
            max_temp_k: parsed.max_temp_k,
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

fn discover_part_asset_paths() -> Vec<String> {
    let mut dirs: Vec<PathBuf> = vec![
        PathBuf::from("assets/parts"),
        PathBuf::from("crates/app/assets/parts"),
    ];
    if let Some(manifest_dir) = option_env!("CARGO_MANIFEST_DIR") {
        dirs.push(Path::new(manifest_dir).join("assets").join("parts"));
    }

    for dir in dirs {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            let mut found = Vec::new();
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|s| s.to_str())
                    && name.ends_with(".part.ron")
                {
                    found.push(format!("parts/{name}"));
                }
            }
            if !found.is_empty() {
                found.sort();
                return found;
            }
        }
    }

    FALLBACK_PART_ASSET_PATHS
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
}

fn load_part_ron_assets(mut commands: Commands<'_, '_>, asset_server: Res<'_, AssetServer>) {
    let paths = discover_part_asset_paths();
    let handles = paths
        .into_iter()
        .map(|path| asset_server.load::<PartSpecAsset>(path))
        .collect();
    commands.insert_resource(LoadedPartHandles { handles });
}

fn sync_hot_reloaded_parts(
    mut events: MessageReader<'_, '_, AssetEvent<PartSpecAsset>>,
    assets: Res<'_, Assets<PartSpecAsset>>,
    handles: Res<'_, LoadedPartHandles>,
    mut catalog: ResMut<'_, LivePartCatalog>,
    mut active_bp: ResMut<'_, ActiveBlueprint>,
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
