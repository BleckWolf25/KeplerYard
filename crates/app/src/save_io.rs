//! @file `save_io.rs`
//!
//! @version 1.1.0
//! @author `BleckWolf25`
//! @license MIT
//!
//! @summary Multi-slot craft library and bit-exact `SimWorld` quicksave persistence
//!
//! @description
//! Resolves platform-conventional save directories via `std::env` and manages multi-slot craft
//! blueprints under `saves/crafts/*.kyc` alongside bit-exact flight quicksaves (`quicksave.kyw`)
//! and maneuver plan sidecar state (`quicksave.kym`).
//!
//! @since 21/11/2026
//! @updated 22/11/2026

// ---------- IMPORTS

use kepler_core::{
    ManeuverPlan, SimWorld, VesselBlueprint, load_blueprint_bytes, load_world_bytes,
    save_blueprint_bytes, save_world_bytes,
};
use std::fs;
use std::path::{Path, PathBuf};

// ---------- CONSTANTS

const QUICKSAVE_FILE: &str = "quicksave.kyw";
const QUICKSAVE_MANEUVER_FILE: &str = "quicksave.kym";
const LEGACY_CRAFT_FILE: &str = "custom_craft.kyc";
const SETTINGS_FILE: &str = "settings.ini";

// ---------- TYPES

#[derive(Clone, Debug)]
pub struct SavedCraftEntry {
    pub name: String,
    pub part_count: usize,
    pub path: PathBuf,
}

// ---------- DECLARATIONS / IMPLEMENTATION

/// Returns and ensures the local save folder using standard environment variables.
pub fn save_directory() -> Result<PathBuf, String> {
    let base = std::env::var_os("KEPLER_YARD_SAVES")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .map(|local_app| PathBuf::from(local_app).join("KeplerYard").join("saves"))
        })
        .or_else(|| {
            std::env::var_os("XDG_DATA_HOME")
                .map(|xdg| PathBuf::from(xdg).join("kepler_yard").join("saves"))
        })
        .or_else(|| {
            std::env::var_os("HOME").map(|home| {
                PathBuf::from(home)
                    .join(".local")
                    .join("share")
                    .join("kepler_yard")
                    .join("saves")
            })
        })
        .unwrap_or_else(|| PathBuf::from("saves"));

    fs::create_dir_all(&base).map_err(|e| format!("failed to create save dir: {e}"))?;
    Ok(base)
}

pub fn crafts_directory() -> Result<PathBuf, String> {
    let dir = save_directory()?.join("crafts");
    fs::create_dir_all(&dir).map_err(|e| format!("failed to create crafts dir: {e}"))?;
    Ok(dir)
}

pub fn slugify_craft_name(name: &str) -> String {
    let mut slug = String::new();
    let mut prev_sep = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_sep = false;
        } else if !prev_sep && !slug.is_empty() {
            slug.push('_');
            prev_sep = true;
        }
    }
    while slug.ends_with('_') {
        slug.pop();
    }
    if slug.is_empty() {
        String::from("unnamed_craft")
    } else {
        slug
    }
}

pub fn save_quicksave(world: &SimWorld, plan: ManeuverPlan) -> Result<PathBuf, String> {
    let dir = save_directory()?;
    let path = dir.join(QUICKSAVE_FILE);
    let bytes = save_world_bytes(world);
    fs::write(&path, bytes).map_err(|e| format!("failed to write quicksave: {e}"))?;

    let mut plan_bytes = Vec::with_capacity(25);
    plan_bytes.push(u8::from(plan.enabled));
    plan_bytes.extend_from_slice(&plan.lead_time_s.to_bits().to_le_bytes());
    plan_bytes.extend_from_slice(&plan.prograde_dv_m_s.to_bits().to_le_bytes());
    plan_bytes.extend_from_slice(&plan.radial_dv_m_s.to_bits().to_le_bytes());
    let _ = fs::write(dir.join(QUICKSAVE_MANEUVER_FILE), plan_bytes);
    Ok(path)
}

pub fn load_quicksave() -> Result<(SimWorld, ManeuverPlan), String> {
    let dir = save_directory()?;
    let path = dir.join(QUICKSAVE_FILE);
    let bytes = fs::read(&path).map_err(|e| format!("no quicksave at {}: {e}", path.display()))?;
    let world = load_world_bytes(&bytes).map_err(|e| format!("invalid quicksave: {e}"))?;

    let plan = if let Ok(pb) = fs::read(dir.join(QUICKSAVE_MANEUVER_FILE))
        && pb.len() == 25
        && let (Ok(a), Ok(b), Ok(c)) = (
            pb[1..9].try_into(),
            pb[9..17].try_into(),
            pb[17..25].try_into(),
        ) {
        ManeuverPlan {
            enabled: pb[0] != 0,
            lead_time_s: f64::from_le_bytes(a),
            prograde_dv_m_s: f64::from_le_bytes(b),
            radial_dv_m_s: f64::from_le_bytes(c),
        }
    } else {
        ManeuverPlan::default()
    };

    Ok((world, plan))
}

pub fn save_craft_blueprint(bp: &VesselBlueprint) -> Result<PathBuf, String> {
    let slug = slugify_craft_name(&bp.name);
    let path = crafts_directory()?.join(format!("{slug}.kyc"));
    let bytes = save_blueprint_bytes(bp);
    fs::write(&path, bytes).map_err(|e| format!("failed to save craft: {e}"))?;
    Ok(path)
}

pub fn list_saved_crafts() -> Result<Vec<SavedCraftEntry>, String> {
    let mut entries = Vec::new();
    let crafts_dir = crafts_directory()?;
    if let Ok(read_dir) = fs::read_dir(&crafts_dir) {
        for item in read_dir.flatten() {
            let path = item.path();
            if path.extension().and_then(|e| e.to_str()) == Some("kyc")
                && let Ok(bp) = load_craft_from_path(&path)
            {
                entries.push(SavedCraftEntry {
                    name: bp.name,
                    part_count: bp.parts.len(),
                    path,
                });
            }
        }
    }

    let legacy = save_directory()?.join(LEGACY_CRAFT_FILE);
    if legacy.exists()
        && let Ok(bp) = load_craft_from_path(&legacy)
    {
        entries.push(SavedCraftEntry {
            name: format!("{} (legacy)", bp.name),
            part_count: bp.parts.len(),
            path: legacy,
        });
    }

    entries.sort_by_key(|a| a.name.to_lowercase());
    Ok(entries)
}

pub fn load_craft_from_path(path: &Path) -> Result<VesselBlueprint, String> {
    let bytes = fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    load_blueprint_bytes(&bytes).map_err(|e| format!("invalid craft file: {e}"))
}

pub fn delete_saved_craft(path: &Path) -> Result<(), String> {
    fs::remove_file(path).map_err(|e| format!("failed to delete {}: {e}", path.display()))
}

pub fn save_quicksave_slot(
    slot: u8,
    world: &SimWorld,
    plan: ManeuverPlan,
) -> Result<PathBuf, String> {
    if !(1..=3).contains(&slot) {
        return Err(String::from("save slot must be between 1 and 3"));
    }
    let dir = save_directory()?;
    let path = dir.join(format!("slot_{slot}.kyw"));
    fs::write(&path, save_world_bytes(world))
        .map_err(|e| format!("failed to write save slot {slot}: {e}"))?;
    let mut plan_bytes = Vec::with_capacity(25);
    plan_bytes.push(u8::from(plan.enabled));
    plan_bytes.extend_from_slice(&plan.lead_time_s.to_bits().to_le_bytes());
    plan_bytes.extend_from_slice(&plan.prograde_dv_m_s.to_bits().to_le_bytes());
    plan_bytes.extend_from_slice(&plan.radial_dv_m_s.to_bits().to_le_bytes());
    let _ = fs::write(dir.join(format!("slot_{slot}.kym")), plan_bytes);
    Ok(path)
}

pub fn load_quicksave_slot(slot: u8) -> Result<(SimWorld, ManeuverPlan), String> {
    if !(1..=3).contains(&slot) {
        return Err(String::from("save slot must be between 1 and 3"));
    }
    let dir = save_directory()?;
    let path = dir.join(format!("slot_{slot}.kyw"));
    let world =
        load_world_bytes(&fs::read(&path).map_err(|e| format!("no save in slot {slot}: {e}"))?)
            .map_err(|e| format!("invalid save in slot {slot}: {e}"))?;
    let plan = if let Ok(pb) = fs::read(dir.join(format!("slot_{slot}.kym")))
        && pb.len() == 25
        && let (Ok(a), Ok(b), Ok(c)) = (
            pb[1..9].try_into(),
            pb[9..17].try_into(),
            pb[17..25].try_into(),
        ) {
        ManeuverPlan {
            enabled: pb[0] != 0,
            lead_time_s: f64::from_le_bytes(a),
            prograde_dv_m_s: f64::from_le_bytes(b),
            radial_dv_m_s: f64::from_le_bytes(c),
        }
    } else {
        ManeuverPlan::default()
    };
    Ok((world, plan))
}

pub fn save_settings(borderless_fullscreen: bool, show_tutorial: bool) -> Result<(), String> {
    let dir = save_directory()?;
    let text =
        format!("borderless_fullscreen={borderless_fullscreen}\nshow_tutorial={show_tutorial}\n");
    fs::write(dir.join(SETTINGS_FILE), text).map_err(|e| format!("failed to write settings: {e}"))
}

pub fn load_settings() -> Result<(bool, bool), String> {
    let dir = save_directory()?;
    let text =
        fs::read_to_string(dir.join(SETTINGS_FILE)).map_err(|e| format!("no settings: {e}"))?;
    let mut fullscreen = false;
    let mut tutorial = true;
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=')
            && let Ok(enabled) = value.parse::<bool>()
        {
            match key {
                "borderless_fullscreen" => fullscreen = enabled,
                "show_tutorial" => tutorial = enabled,
                _ => {}
            }
        }
    }
    Ok((fullscreen, tutorial))
}

// ---------- EXPORTS
