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
use std::io::Write;
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
                .filter(|xdg| !xdg.is_empty())
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

/// Writes `bytes` to `path` via a temporary sibling file and an atomic rename, so a crash or full
/// disk mid-write can never leave a truncated save behind.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut tmp_name = path.file_name().map(std::ffi::OsString::from).unwrap_or_default();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// Stable FNV-1a hash used to give names with no ASCII characters a distinct file name.
fn fnv1a_32(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// Picks the file for `name`, never overwriting a different craft that slugs to the same name.
fn unique_craft_path(dir: &Path, name: &str) -> PathBuf {
    let base = if slugify_craft_name(name) == "unnamed_craft" {
        format!("craft_{:08x}", fnv1a_32(name))
    } else {
        slugify_craft_name(name)
    };
    for n in 1_u32..1000 {
        let file = if n == 1 {
            format!("{base}.kyc")
        } else {
            format!("{base}_{n}.kyc")
        };
        let path = dir.join(file);
        if !path.exists() {
            return path;
        }
        // Same craft name: overwrite in place. Different name: keep looking.
        if load_craft_from_path(&path).is_ok_and(|existing| existing.name == name) {
            return path;
        }
    }
    dir.join(format!("{base}_{:08x}.kyc", fnv1a_32(name)))
}

pub fn save_quicksave(world: &SimWorld, plan: ManeuverPlan) -> Result<PathBuf, String> {
    let dir = save_directory()?;
    let path = dir.join(QUICKSAVE_FILE);
    let bytes = save_world_bytes(world);
    write_atomic(&path, &bytes).map_err(|e| format!("failed to write quicksave: {e}"))?;

    let mut plan_bytes = Vec::with_capacity(25);
    plan_bytes.push(u8::from(plan.enabled));
    plan_bytes.extend_from_slice(&plan.lead_time_s.to_bits().to_le_bytes());
    plan_bytes.extend_from_slice(&plan.prograde_dv_m_s.to_bits().to_le_bytes());
    plan_bytes.extend_from_slice(&plan.radial_dv_m_s.to_bits().to_le_bytes());
    write_atomic(&dir.join(QUICKSAVE_MANEUVER_FILE), &plan_bytes)
        .map_err(|e| format!("failed to write maneuver plan: {e}"))?;
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
        let (lead, pro, rad) = (
            f64::from_le_bytes(a),
            f64::from_le_bytes(b),
            f64::from_le_bytes(c),
        );
        if lead.is_finite() && lead >= 0.0 && pro.is_finite() && rad.is_finite() {
            ManeuverPlan {
                enabled: pb[0] != 0,
                lead_time_s: lead,
                prograde_dv_m_s: pro,
                radial_dv_m_s: rad,
            }
        } else {
            ManeuverPlan::default()
        }
    } else {
        ManeuverPlan::default()
    };

    Ok((world, plan))
}

pub fn save_craft_blueprint(bp: &VesselBlueprint) -> Result<PathBuf, String> {
    let path = unique_craft_path(&crafts_directory()?, &bp.name);
    let bytes = save_blueprint_bytes(bp);
    write_atomic(&path, &bytes).map_err(|e| format!("failed to save craft: {e}"))?;
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
    write_atomic(&path, &save_world_bytes(world))
        .map_err(|e| format!("failed to write save slot {slot}: {e}"))?;
    let mut plan_bytes = Vec::with_capacity(25);
    plan_bytes.push(u8::from(plan.enabled));
    plan_bytes.extend_from_slice(&plan.lead_time_s.to_bits().to_le_bytes());
    plan_bytes.extend_from_slice(&plan.prograde_dv_m_s.to_bits().to_le_bytes());
    plan_bytes.extend_from_slice(&plan.radial_dv_m_s.to_bits().to_le_bytes());
    write_atomic(
        &dir.join(format!("slot_{slot}.kym")),
        &plan_bytes,
    )
    .map_err(|e| format!("failed to write maneuver plan for slot {slot}: {e}"))?;
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
        let (lead, pro, rad) = (
            f64::from_le_bytes(a),
            f64::from_le_bytes(b),
            f64::from_le_bytes(c),
        );
        if lead.is_finite() && lead >= 0.0 && pro.is_finite() && rad.is_finite() {
            ManeuverPlan {
                enabled: pb[0] != 0,
                lead_time_s: lead,
                prograde_dv_m_s: pro,
                radial_dv_m_s: rad,
            }
        } else {
            ManeuverPlan::default()
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

// ---------- TESTS

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kepler_yard_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn blueprint(name: &str) -> VesselBlueprint {
        VesselBlueprint {
            name: name.to_owned(),
            parts: Vec::new(),
            next_instance_id: 1,
        }
    }

    fn save_to(dir: &Path, bp: &VesselBlueprint) -> PathBuf {
        let path = unique_craft_path(dir, &bp.name);
        write_atomic(&path, &save_blueprint_bytes(bp)).expect("write craft");
        path
    }

    #[test]
    fn crafts_with_colliding_slugs_do_not_overwrite_each_other() {
        let dir = temp_dir("slug");
        let a = save_to(&dir, &blueprint("My Rocket!"));
        let b = save_to(&dir, &blueprint("my rocket"));
        assert_ne!(a, b);
        // Saving the same craft again reuses its own file.
        assert_eq!(save_to(&dir, &blueprint("My Rocket!")), a);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_ascii_names_get_distinct_files() {
        let dir = temp_dir("unicode");
        let a = save_to(&dir, &blueprint("火箭"));
        let b = save_to(&dir, &blueprint("ロケット"));
        assert_ne!(a, b);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_replaces_content_and_leaves_no_temp_file() {
        let dir = temp_dir("atomic");
        let path = dir.join("quicksave.kyw");
        write_atomic(&path, b"first").expect("first write");
        write_atomic(&path, b"second").expect("second write");
        assert_eq!(fs::read(&path).expect("read back"), b"second");
        assert!(!dir.join("quicksave.kyw.tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}

// ---------- EXPORTS
