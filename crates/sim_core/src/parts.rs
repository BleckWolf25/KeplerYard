//! @file parts.rs
//!
//! @version 1.2.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Categorized rocket part specifications, SRBs, landing legs, CoP, and pre-flight analysis
//!
//! @description
//! Defines part categories (`Command`, `Propulsion`, `FuelTanks`, `AeroAndThermal`, `Structural`),
//! vector shapes (`RoundedRect`, `Cone`, `Nozzle`, `Fin`, `Dome`, `LandingStrut`), thermal
//! tolerances, SRBs, RCS, landing struts, nosecones, probe cores, tree-structured blueprints,
//! symmetry/cloning helpers, Center of Pressure, and pre-flight engineering validation checks.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use crate::body::KEPLER_PRIME_SURFACE_G;
use crate::math::{DVec2, G0};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

// ---------- CONSTANTS

/// Torque threshold (N·m) above which the editor emits an asymmetric tip-over warning.
pub const TIP_OVER_TORQUE_WARN_NM: f64 = 120.0;

// ---------- TYPES

/// Functional category used to organize the editor part palette cleanly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PartCategory {
    Command,
    Propulsion,
    FuelTanks,
    AeroAndThermal,
    Structural,
}

/// Procedural vector shape shared by physics footprints, attach nodes, icons, and Bevy meshes.
#[derive(Clone, Debug, PartialEq)]
pub enum PartShape {
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
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PodSpec {
    pub reaction_torque_nm: f64,
    pub crewed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TankSpec {
    pub fuel_capacity_kg: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineSpec {
    pub max_thrust_n: f64,
    pub isp_s: f64,
    pub gimbal_range_rad: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SrbSpec {
    pub max_thrust_n: f64,
    pub isp_s: f64,
    pub fuel_capacity_kg: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecouplerSpec {
    pub ejection_impulse_ns: f64,
    pub crossfeed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FinSpec {
    pub lift_area_m2: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RcsSpec {
    pub thrust_n: f64,
    pub isp_s: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParachuteSpec {
    pub deployed_drag_area_m2: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeatShieldSpec {
    pub ablator_kg: f64,
    pub wake_shield_factor: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LandingLegSpec {
    pub impact_tolerance_m_s: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoseConeSpec {
    pub drag_reduction: f64,
}

/// Functional category and parameters for a rocket part.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PartKind {
    CommandPod(PodSpec),
    Tank(TankSpec),
    Engine(EngineSpec),
    SolidBooster(SrbSpec),
    Decoupler(DecouplerSpec),
    Fin(FinSpec),
    Rcs(RcsSpec),
    Parachute(ParachuteSpec),
    HeatShield(HeatShieldSpec),
    LandingLeg(LandingLegSpec),
    NoseCone(NoseConeSpec),
}

/// Immutable data-driven definition for a part loaded from `.part.ron` or built-in defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct PartSpec {
    pub id: String,
    pub display_name: String,
    pub category: PartCategory,
    pub dry_mass_kg: f64,
    pub shape: PartShape,
    pub kind: PartKind,
    pub stack_top: bool,
    pub stack_bottom: bool,
    pub radial_attach: bool,
    pub drag_coeff: f64,
    pub max_impact_m_s: f64,
    pub max_temp_k: f64,
}

/// Connection relationship between a child part and its parent in the craft tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachKind {
    Root,
    StackTop,
    StackBottom,
    RadialLeft,
    RadialRight,
}

/// Placed part entry inside a `VesselBlueprint` before launch.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedPart {
    pub instance_id: u64,
    pub spec_id: String,
    pub parent_id: Option<u64>,
    pub attach_kind: AttachKind,
    pub local_pos: DVec2,
    pub local_rot: f64,
    pub stage: u32,
    pub crossfeed: bool,
}

/// Editable vessel assembly graph produced by the VAB editor and launched into `SimWorld`.
#[derive(Clone, Debug, PartialEq)]
pub struct VesselBlueprint {
    pub name: String,
    pub parts: Vec<PlacedPart>,
    pub next_instance_id: u64,
}

/// Catalog of available `PartSpec` definitions keyed by unique part ID.
#[derive(Clone, Debug, Default)]
pub struct PartCatalog {
    specs: BTreeMap<String, PartSpec>,
}

/// Tsiolkovsky performance metrics for a single stage in the editor UI.
#[derive(Clone, Debug, PartialEq)]
pub struct StageSummary {
    pub stage: u32,
    pub wet_mass_kg: f64,
    pub dry_mass_kg: f64,
    pub fuel_kg: f64,
    pub thrust_n: f64,
    pub twr_surface: f64,
    pub delta_v_m_s: f64,
    pub has_decoupler: bool,
}

/// Complete engineering telemetry readout and pre-flight checklist for a `VesselBlueprint`.
#[derive(Clone, Debug, PartialEq)]
pub struct BlueprintAnalysis {
    pub wet_mass_kg: f64,
    pub dry_mass_kg: f64,
    pub fuel_mass_kg: f64,
    pub com_local: DVec2,
    pub cop_local: DVec2,
    pub aero_unstable: bool,
    pub moi_kg_m2: f64,
    pub net_thrust_torque_nm: f64,
    pub tip_over_warning: bool,
    pub total_delta_v_m_s: f64,
    pub stages: Vec<StageSummary>,
    pub warnings: Vec<String>,
}

// ---------- DECLARATIONS / IMPLEMENTATION

impl PartCategory {
    pub const ALL: [Self; 5] = [
        Self::Command,
        Self::Propulsion,
        Self::FuelTanks,
        Self::AeroAndThermal,
        Self::Structural,
    ];

    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Command => "CMD",
            Self::Propulsion => "ENG",
            Self::FuelTanks => "FUEL",
            Self::AeroAndThermal => "AERO",
            Self::Structural => "STR",
        }
    }
}

impl PartShape {
    /// Width and height bounding box in local meters (at `local_rot = 0`).
    pub fn bounding_size(&self) -> DVec2 {
        match *self {
            Self::RoundedRect { width, height, .. } => DVec2::new(width, height),
            Self::Cone {
                bottom_width,
                top_width,
                height,
            } => DVec2::new(bottom_width.max(top_width), height),
            Self::Nozzle {
                mount_width,
                bell_width,
                height,
                ..
            } => DVec2::new(mount_width.max(bell_width), height),
            Self::Fin {
                root_chord,
                span,
                ..
            } => DVec2::new(span, root_chord),
            Self::Dome { width, height } => DVec2::new(width, height),
            Self::LandingStrut {
                width,
                height,
                foot_span,
            } => DVec2::new(width.max(foot_span), height),
        }
    }

    /// Top connection offset in local part coordinates.
    pub fn top_offset(&self) -> DVec2 {
        DVec2::new(0.0, 0.5 * self.bounding_size().y)
    }

    /// Bottom connection offset in local part coordinates.
    pub fn bottom_offset(&self) -> DVec2 {
        DVec2::new(0.0, -0.5 * self.bounding_size().y)
    }

    /// Radial left surface offset in local part coordinates.
    pub fn radial_left_offset(&self) -> DVec2 {
        DVec2::new(-0.5 * self.bounding_size().x, 0.0)
    }

    /// Radial right surface offset in local part coordinates.
    pub fn radial_right_offset(&self) -> DVec2 {
        DVec2::new(0.5 * self.bounding_size().x, 0.0)
    }

    /// Approximate rectangular moment of inertia factor `(w^2 + h^2) / 12` per kg.
    pub fn unit_moi(&self) -> f64 {
        let size = self.bounding_size();
        (size.x * size.x + size.y * size.y) / 12.0
    }

    /// Effective half-extents for collision and editor selection.
    pub fn contact_half_extent(&self) -> DVec2 {
        self.bounding_size() * 0.5
    }
}

impl PartSpec {
    /// Wet mass of a single part at full propellant load.
    pub fn full_mass_kg(&self) -> f64 {
        match self.kind {
            PartKind::Tank(t) => self.dry_mass_kg + t.fuel_capacity_kg,
            PartKind::SolidBooster(s) => self.dry_mass_kg + s.fuel_capacity_kg,
            _ => self.dry_mass_kg,
        }
    }

    pub fn fuel_capacity_kg(&self) -> f64 {
        match self.kind {
            PartKind::Tank(t) => t.fuel_capacity_kg,
            PartKind::SolidBooster(s) => s.fuel_capacity_kg,
            _ => 0.0,
        }
    }
}

impl PartCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, spec: PartSpec) {
        self.specs.insert(spec.id.clone(), spec);
    }

    pub fn get(&self, id: &str) -> Option<&PartSpec> {
        self.specs.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &PartSpec> {
        self.specs.values()
    }

    pub fn len(&self) -> usize {
        self.specs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }
}

impl VesselBlueprint {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            parts: Vec::new(),
            next_instance_id: 1,
        }
    }

    pub fn add_root(&mut self, spec_id: impl Into<String>, stage: u32) -> u64 {
        let id = self.next_instance_id;
        self.next_instance_id += 1;
        self.parts.push(PlacedPart {
            instance_id: id,
            spec_id: spec_id.into(),
            parent_id: None,
            attach_kind: AttachKind::Root,
            local_pos: DVec2::ZERO,
            local_rot: 0.0,
            stage,
            crossfeed: false,
        });
        id
    }

    pub fn attach_child(
        &mut self,
        parent_id: u64,
        spec_id: impl Into<String>,
        attach_kind: AttachKind,
        local_pos: DVec2,
        local_rot: f64,
        stage: u32,
    ) -> u64 {
        let id = self.next_instance_id;
        self.next_instance_id += 1;
        self.parts.push(PlacedPart {
            instance_id: id,
            spec_id: spec_id.into(),
            parent_id: Some(parent_id),
            attach_kind,
            local_pos,
            local_rot,
            stage,
            crossfeed: false,
        });
        id
    }

    pub fn set_part_crossfeed(&mut self, part_id: u64, crossfeed: bool) {
        if let Some(p) = self.parts.iter_mut().find(|p| p.instance_id == part_id) {
            p.crossfeed = crossfeed;
        }
    }

    /// Removes a non-root part and all of its descendant children.
    pub fn remove_subtree(&mut self, target_id: u64) -> bool {
        let Some(target) = self.parts.iter().find(|p| p.instance_id == target_id) else {
            return false;
        };
        if target.parent_id.is_none() {
            return false;
        }
        let mut to_remove = HashSet::new();
        let mut queue = VecDeque::new();
        to_remove.insert(target_id);
        queue.push_back(target_id);
        while let Some(curr) = queue.pop_front() {
            for part in &self.parts {
                if part.parent_id == Some(curr) && to_remove.insert(part.instance_id) {
                    queue.push_back(part.instance_id);
                }
            }
        }
        self.parts.retain(|p| !to_remove.contains(&p.instance_id));
        true
    }

    /// Clones a non-root part subtree translated by `offset`, returning the cloned root ID.
    pub fn duplicate_subtree(&mut self, target_id: u64, offset: DVec2) -> Option<u64> {
        let root = self
            .parts
            .iter()
            .find(|p| p.instance_id == target_id)?
            .clone();
        root.parent_id?;

        let mut order = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        visited.insert(target_id);
        queue.push_back(target_id);

        while let Some(curr) = queue.pop_front() {
            if let Some(p) = self.parts.iter().find(|p| p.instance_id == curr) {
                order.push(p.clone());
                for c in &self.parts {
                    if c.parent_id == Some(curr) && visited.insert(c.instance_id) {
                        queue.push_back(c.instance_id);
                    }
                }
            }
        }

        let mut remap = HashMap::new();
        for orig in &order {
            let new_id = self.next_instance_id;
            self.next_instance_id += 1;
            remap.insert(orig.instance_id, new_id);
        }

        let new_root_id = remap.get(&target_id).copied()?;
        for orig in order {
            let new_id = remap.get(&orig.instance_id).copied()?;
            let new_parent = if orig.instance_id == target_id {
                orig.parent_id
            } else {
                orig.parent_id.and_then(|pid| remap.get(&pid).copied())
            };
            self.parts.push(PlacedPart {
                instance_id: new_id,
                spec_id: orig.spec_id,
                parent_id: new_parent,
                attach_kind: orig.attach_kind,
                local_pos: orig.local_pos + offset,
                local_rot: orig.local_rot,
                stage: orig.stage,
                crossfeed: orig.crossfeed,
            });
        }
        Some(new_root_id)
    }

    /// Finds a symmetric opposite-side sibling for `part_id` if radially attached in a pair.
    pub fn find_mirror_twin(&self, part_id: u64) -> Option<u64> {
        let target = self.parts.iter().find(|p| p.instance_id == part_id)?;
        let opposite = match target.attach_kind {
            AttachKind::RadialLeft => AttachKind::RadialRight,
            AttachKind::RadialRight => AttachKind::RadialLeft,
            _ => return None,
        };
        self.parts
            .iter()
            .find(|p| {
                p.instance_id != part_id
                    && p.parent_id == target.parent_id
                    && p.spec_id == target.spec_id
                    && p.attach_kind == opposite
                    && (p.local_pos.y - target.local_pos.y).abs() < 0.08
            })
            .map(|p| p.instance_id)
    }

    /// Checks whether `candidate_id` is `target_id` or inside `target_id`'s subtree.
    pub fn is_in_subtree(&self, target_id: u64, candidate_id: u64) -> bool {
        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();
        queue.push_back(target_id);
        visited.insert(target_id);
        while let Some(curr) = queue.pop_front() {
            if curr == candidate_id {
                return true;
            }
            for p in &self.parts {
                if p.parent_id == Some(curr) && visited.insert(p.instance_id) {
                    queue.push_back(p.instance_id);
                }
            }
        }
        false
    }

    /// Translates `target_id` and all its descendants by `delta` meters.
    pub fn translate_subtree(&mut self, target_id: u64, delta: DVec2) {
        let mut queue = VecDeque::new();
        let mut ids = HashSet::new();
        queue.push_back(target_id);
        ids.insert(target_id);
        while let Some(curr) = queue.pop_front() {
            for p in &self.parts {
                if p.parent_id == Some(curr) && ids.insert(p.instance_id) {
                    queue.push_back(p.instance_id);
                }
            }
        }
        for p in &mut self.parts {
            if ids.contains(&p.instance_id) {
                p.local_pos += delta;
            }
        }
    }
}

/// Built-in default part catalog mirroring `assets/parts/*.part.ron`.
pub fn default_part_catalog() -> PartCatalog {
    let mut cat = PartCatalog::new();
    cat.insert(PartSpec {
        id: String::from("command_pod"),
        display_name: String::from("Mk-1 Command Pod"),
        category: PartCategory::Command,
        dry_mass_kg: 600.0,
        shape: PartShape::Cone {
            bottom_width: 1.25,
            top_width: 0.55,
            height: 1.20,
        },
        kind: PartKind::CommandPod(PodSpec {
            reaction_torque_nm: 4_500.0,
            crewed: true,
        }),
        stack_top: true,
        stack_bottom: true,
        radial_attach: false,
        drag_coeff: 0.28,
        max_impact_m_s: 16.0,
        max_temp_k: 1_850.0,
    });
    cat.insert(PartSpec {
        id: String::from("probe_core"),
        display_name: String::from("RC-001 Probe Core"),
        category: PartCategory::Command,
        dry_mass_kg: 110.0,
        shape: PartShape::RoundedRect {
            width: 1.10,
            height: 0.42,
            corner_radius: 0.05,
        },
        kind: PartKind::CommandPod(PodSpec {
            reaction_torque_nm: 1_400.0,
            crewed: false,
        }),
        stack_top: true,
        stack_bottom: true,
        radial_attach: false,
        drag_coeff: 0.25,
        max_impact_m_s: 12.0,
        max_temp_k: 1_500.0,
    });
    cat.insert(PartSpec {
        id: String::from("parachute_mk1"),
        display_name: String::from("Mk-16 Chute"),
        category: PartCategory::AeroAndThermal,
        dry_mass_kg: 45.0,
        shape: PartShape::Dome {
            width: 0.60,
            height: 0.34,
        },
        kind: PartKind::Parachute(ParachuteSpec {
            deployed_drag_area_m2: 95.0,
        }),
        stack_top: false,
        stack_bottom: true,
        radial_attach: false,
        drag_coeff: 0.22,
        max_impact_m_s: 16.0,
        max_temp_k: 1_650.0,
    });
    cat.insert(PartSpec {
        id: String::from("heat_shield"),
        display_name: String::from("HS-125 Ablative Shield"),
        category: PartCategory::AeroAndThermal,
        dry_mass_kg: 75.0,
        shape: PartShape::RoundedRect {
            width: 1.38,
            height: 0.24,
            corner_radius: 0.06,
        },
        kind: PartKind::HeatShield(HeatShieldSpec {
            ablator_kg: 120.0,
            wake_shield_factor: 0.04,
        }),
        stack_top: true,
        stack_bottom: true,
        radial_attach: false,
        drag_coeff: 0.85,
        max_impact_m_s: 20.0,
        max_temp_k: 3_400.0,
    });
    cat.insert(PartSpec {
        id: String::from("nosecone"),
        display_name: String::from("AC-125 Aero Nosecone"),
        category: PartCategory::AeroAndThermal,
        dry_mass_kg: 38.0,
        shape: PartShape::Cone {
            bottom_width: 1.25,
            top_width: 0.14,
            height: 0.95,
        },
        kind: PartKind::NoseCone(NoseConeSpec {
            drag_reduction: 0.45,
        }),
        stack_top: false,
        stack_bottom: true,
        radial_attach: false,
        drag_coeff: 0.09,
        max_impact_m_s: 14.0,
        max_temp_k: 1_750.0,
    });
    cat.insert(PartSpec {
        id: String::from("tank_small"),
        display_name: String::from("FL-S1200 LFO Tank"),
        category: PartCategory::FuelTanks,
        dry_mass_kg: 150.0,
        shape: PartShape::RoundedRect {
            width: 1.25,
            height: 1.80,
            corner_radius: 0.08,
        },
        kind: PartKind::Tank(TankSpec {
            fuel_capacity_kg: 1_200.0,
        }),
        stack_top: true,
        stack_bottom: true,
        radial_attach: false,
        drag_coeff: 0.32,
        max_impact_m_s: 12.0,
        max_temp_k: 1_250.0,
    });
    cat.insert(PartSpec {
        id: String::from("tank_large"),
        display_name: String::from("FL-L3600 Core Tank"),
        category: PartCategory::FuelTanks,
        dry_mass_kg: 400.0,
        shape: PartShape::RoundedRect {
            width: 1.65,
            height: 3.40,
            corner_radius: 0.10,
        },
        kind: PartKind::Tank(TankSpec {
            fuel_capacity_kg: 3_600.0,
        }),
        stack_top: true,
        stack_bottom: true,
        radial_attach: false,
        drag_coeff: 0.34,
        max_impact_m_s: 12.0,
        max_temp_k: 1_250.0,
    });
    cat.insert(PartSpec {
        id: String::from("tank_heavy"),
        display_name: String::from("FL-H8200 Heavy Tank"),
        category: PartCategory::FuelTanks,
        dry_mass_kg: 820.0,
        shape: PartShape::RoundedRect {
            width: 2.20,
            height: 4.40,
            corner_radius: 0.12,
        },
        kind: PartKind::Tank(TankSpec {
            fuel_capacity_kg: 8_200.0,
        }),
        stack_top: true,
        stack_bottom: true,
        radial_attach: false,
        drag_coeff: 0.36,
        max_impact_m_s: 12.0,
        max_temp_k: 1_250.0,
    });
    cat.insert(PartSpec {
        id: String::from("engine_orbital"),
        display_name: String::from("LV-28V 'Vesper'"),
        category: PartCategory::Propulsion,
        dry_mass_kg: 220.0,
        shape: PartShape::Nozzle {
            mount_width: 1.15,
            throat_width: 0.48,
            bell_width: 0.98,
            height: 1.05,
        },
        kind: PartKind::Engine(EngineSpec {
            max_thrust_n: 28_000.0,
            isp_s: 340.0,
            gimbal_range_rad: 0.08,
        }),
        stack_top: true,
        stack_bottom: true,
        radial_attach: false,
        drag_coeff: 0.42,
        max_impact_m_s: 10.0,
        max_temp_k: 1_550.0,
    });
    cat.insert(PartSpec {
        id: String::from("engine_booster"),
        display_name: String::from("LV-110 'Titan'"),
        category: PartCategory::Propulsion,
        dry_mass_kg: 650.0,
        shape: PartShape::Nozzle {
            mount_width: 1.55,
            throat_width: 0.68,
            bell_width: 1.38,
            height: 1.45,
        },
        kind: PartKind::Engine(EngineSpec {
            max_thrust_n: 110_000.0,
            isp_s: 295.0,
            gimbal_range_rad: 0.06,
        }),
        stack_top: true,
        stack_bottom: false,
        radial_attach: false,
        drag_coeff: 0.45,
        max_impact_m_s: 10.0,
        max_temp_k: 1_650.0,
    });
    cat.insert(PartSpec {
        id: String::from("engine_heavy"),
        display_name: String::from("LV-240 'Kronos'"),
        category: PartCategory::Propulsion,
        dry_mass_kg: 1_180.0,
        shape: PartShape::Nozzle {
            mount_width: 2.05,
            throat_width: 0.92,
            bell_width: 1.88,
            height: 1.75,
        },
        kind: PartKind::Engine(EngineSpec {
            max_thrust_n: 240_000.0,
            isp_s: 310.0,
            gimbal_range_rad: 0.07,
        }),
        stack_top: true,
        stack_bottom: false,
        radial_attach: false,
        drag_coeff: 0.48,
        max_impact_m_s: 10.0,
        max_temp_k: 1_750.0,
    });
    cat.insert(PartSpec {
        id: String::from("srb_radial"),
        display_name: String::from("SRB-90 'Hammer' Booster"),
        category: PartCategory::Propulsion,
        dry_mass_kg: 420.0,
        shape: PartShape::RoundedRect {
            width: 0.95,
            height: 3.60,
            corner_radius: 0.10,
        },
        kind: PartKind::SolidBooster(SrbSpec {
            max_thrust_n: 135_000.0,
            isp_s: 240.0,
            fuel_capacity_kg: 1_950.0,
        }),
        stack_top: true,
        stack_bottom: false,
        radial_attach: true,
        drag_coeff: 0.34,
        max_impact_m_s: 11.0,
        max_temp_k: 1_550.0,
    });
    cat.insert(PartSpec {
        id: String::from("rcs_block"),
        display_name: String::from("RV-105 RCS Thruster"),
        category: PartCategory::Propulsion,
        dry_mass_kg: 35.0,
        shape: PartShape::RoundedRect {
            width: 0.36,
            height: 0.46,
            corner_radius: 0.06,
        },
        kind: PartKind::Rcs(RcsSpec {
            thrust_n: 1_400.0,
            isp_s: 260.0,
        }),
        stack_top: false,
        stack_bottom: false,
        radial_attach: true,
        drag_coeff: 0.18,
        max_impact_m_s: 12.0,
        max_temp_k: 1_500.0,
    });
    cat.insert(PartSpec {
        id: String::from("decoupler_stack"),
        display_name: String::from("TR-16 Stack Decoupler"),
        category: PartCategory::Structural,
        dry_mass_kg: 80.0,
        shape: PartShape::RoundedRect {
            width: 1.35,
            height: 0.36,
            corner_radius: 0.04,
        },
        kind: PartKind::Decoupler(DecouplerSpec {
            ejection_impulse_ns: 280.0,
            crossfeed: false,
        }),
        stack_top: true,
        stack_bottom: true,
        radial_attach: false,
        drag_coeff: 0.30,
        max_impact_m_s: 14.0,
        max_temp_k: 1_350.0,
    });
    cat.insert(PartSpec {
        id: String::from("decoupler_radial"),
        display_name: String::from("TT-38 Radial Pylon"),
        category: PartCategory::Structural,
        dry_mass_kg: 45.0,
        shape: PartShape::RoundedRect {
            width: 0.32,
            height: 0.90,
            corner_radius: 0.04,
        },
        kind: PartKind::Decoupler(DecouplerSpec {
            ejection_impulse_ns: 650.0,
            crossfeed: false,
        }),
        stack_top: false,
        stack_bottom: false,
        radial_attach: true,
        drag_coeff: 0.25,
        max_impact_m_s: 14.0,
        max_temp_k: 1_350.0,
    });
    cat.insert(PartSpec {
        id: String::from("landing_leg"),
        display_name: String::from("LT-2 Landing Strut"),
        category: PartCategory::Structural,
        dry_mass_kg: 55.0,
        shape: PartShape::LandingStrut {
            width: 0.65,
            height: 1.45,
            foot_span: 0.95,
        },
        kind: PartKind::LandingLeg(LandingLegSpec {
            impact_tolerance_m_s: 22.0,
        }),
        stack_top: false,
        stack_bottom: false,
        radial_attach: true,
        drag_coeff: 0.20,
        max_impact_m_s: 22.0,
        max_temp_k: 1_650.0,
    });
    cat.insert(PartSpec {
        id: String::from("fin_basic"),
        display_name: String::from("AV-R4 Delta Fin"),
        category: PartCategory::AeroAndThermal,
        dry_mass_kg: 25.0,
        shape: PartShape::Fin {
            root_chord: 1.15,
            tip_chord: 0.42,
            span: 0.78,
            sweep: 0.55,
        },
        kind: PartKind::Fin(FinSpec { lift_area_m2: 0.62 }),
        stack_top: false,
        stack_bottom: false,
        radial_attach: true,
        drag_coeff: 0.14,
        max_impact_m_s: 12.0,
        max_temp_k: 1_450.0,
    });
    cat
}

/// Builds the default two-stage orbital rocket (~4,650 m/s total delta-v).
pub fn starter_two_stage_blueprint(catalog: &PartCatalog) -> VesselBlueprint {
    let mut bp = VesselBlueprint::new("Kepler-I Orbital");
    let pod_id = bp.add_root("command_pod", 1);

    let pod_bottom = catalog
        .get("command_pod")
        .map_or(-0.6, |s| s.shape.bottom_offset().y);
    let ts_h = catalog
        .get("tank_small")
        .map_or(1.8, |s| s.shape.bounding_size().y);
    let ts_y = pod_bottom - 0.5 * ts_h;
    let ts_id = bp.attach_child(
        pod_id,
        "tank_small",
        AttachKind::StackBottom,
        DVec2::new(0.0, ts_y),
        0.0,
        1,
    );

    let eo_h = catalog
        .get("engine_orbital")
        .map_or(1.05, |s| s.shape.bounding_size().y);
    let eo_y = ts_y - 0.5 * ts_h - 0.5 * eo_h;
    let eo_id = bp.attach_child(
        ts_id,
        "engine_orbital",
        AttachKind::StackBottom,
        DVec2::new(0.0, eo_y),
        0.0,
        1,
    );

    let dec_h = catalog
        .get("decoupler_stack")
        .map_or(0.36, |s| s.shape.bounding_size().y);
    let dec_y = eo_y - 0.5 * eo_h - 0.5 * dec_h;
    let dec_id = bp.attach_child(
        eo_id,
        "decoupler_stack",
        AttachKind::StackBottom,
        DVec2::new(0.0, dec_y),
        0.0,
        1,
    );

    let tl_h = catalog
        .get("tank_large")
        .map_or(3.4, |s| s.shape.bounding_size().y);
    let tl_w = catalog
        .get("tank_large")
        .map_or(1.65, |s| s.shape.bounding_size().x);
    let tl_y = dec_y - 0.5 * dec_h - 0.5 * tl_h;
    let tl_id = bp.attach_child(
        dec_id,
        "tank_large",
        AttachKind::StackBottom,
        DVec2::new(0.0, tl_y),
        0.0,
        0,
    );

    let fin_span = catalog
        .get("fin_basic")
        .map_or(0.78, |s| s.shape.bounding_size().x);
    let fin_y = tl_y - 0.30 * tl_h;
    bp.attach_child(
        tl_id,
        "fin_basic",
        AttachKind::RadialLeft,
        DVec2::new(-0.5 * tl_w - 0.5 * fin_span, fin_y),
        0.0,
        0,
    );
    bp.attach_child(
        tl_id,
        "fin_basic",
        AttachKind::RadialRight,
        DVec2::new(0.5 * tl_w + 0.5 * fin_span, fin_y),
        0.0,
        0,
    );

    let eb_h = catalog
        .get("engine_booster")
        .map_or(1.45, |s| s.shape.bounding_size().y);
    let eb_y = tl_y - 0.5 * tl_h - 0.5 * eb_h;
    bp.attach_child(
        tl_id,
        "engine_booster",
        AttachKind::StackBottom,
        DVec2::new(0.0, eb_y),
        0.0,
        0,
    );

    bp
}

/// Builds a heavy 3-stage Selene-capable rocket with nose parachute and ablative heat shield.
pub fn lunar_capable_blueprint(catalog: &PartCatalog) -> VesselBlueprint {
    let mut bp = VesselBlueprint::new("Selene Explorer III");
    let pod_id = bp.add_root("command_pod", 3);

    let size_of = |id: &str, fallback: DVec2| -> DVec2 {
        catalog
            .get(id)
            .map_or(fallback, |s| s.shape.bounding_size())
    };

    let pod_sz = size_of("command_pod", DVec2::new(1.25, 1.20));
    let chute_sz = size_of("parachute_mk1", DVec2::new(0.60, 0.34));
    bp.attach_child(
        pod_id,
        "parachute_mk1",
        AttachKind::StackTop,
        DVec2::new(0.0, 0.5 * pod_sz.y + 0.5 * chute_sz.y),
        0.0,
        3,
    );

    let shield_sz = size_of("heat_shield", DVec2::new(1.38, 0.24));
    let shield_y = -0.5 * pod_sz.y - 0.5 * shield_sz.y;
    let shield_id = bp.attach_child(
        pod_id,
        "heat_shield",
        AttachKind::StackBottom,
        DVec2::new(0.0, shield_y),
        0.0,
        2,
    );

    let dec_sz = size_of("decoupler_stack", DVec2::new(1.35, 0.36));
    let dec_top_y = shield_y - 0.5 * shield_sz.y - 0.5 * dec_sz.y;
    let dec_top_id = bp.attach_child(
        shield_id,
        "decoupler_stack",
        AttachKind::StackBottom,
        DVec2::new(0.0, dec_top_y),
        0.0,
        2,
    );

    let ts_sz = size_of("tank_small", DVec2::new(1.25, 1.80));
    let ts_y = dec_top_y - 0.5 * dec_sz.y - 0.5 * ts_sz.y;
    let ts_id = bp.attach_child(
        dec_top_id,
        "tank_small",
        AttachKind::StackBottom,
        DVec2::new(0.0, ts_y),
        0.0,
        1,
    );

    let eo_sz = size_of("engine_orbital", DVec2::new(1.15, 1.05));
    let eo_y = ts_y - 0.5 * ts_sz.y - 0.5 * eo_sz.y;
    let eo_id = bp.attach_child(
        ts_id,
        "engine_orbital",
        AttachKind::StackBottom,
        DVec2::new(0.0, eo_y),
        0.0,
        1,
    );

    let dec_mid_y = eo_y - 0.5 * eo_sz.y - 0.5 * dec_sz.y;
    let dec_mid_id = bp.attach_child(
        eo_id,
        "decoupler_stack",
        AttachKind::StackBottom,
        DVec2::new(0.0, dec_mid_y),
        0.0,
        1,
    );

    let th_sz = size_of("tank_heavy", DVec2::new(2.20, 4.40));
    let th_y = dec_mid_y - 0.5 * dec_sz.y - 0.5 * th_sz.y;
    let th_id = bp.attach_child(
        dec_mid_id,
        "tank_heavy",
        AttachKind::StackBottom,
        DVec2::new(0.0, th_y),
        0.0,
        0,
    );

    let eh_sz = size_of("engine_heavy", DVec2::new(2.05, 1.75));
    let eh_y = th_y - 0.5 * th_sz.y - 0.5 * eh_sz.y;
    bp.attach_child(
        th_id,
        "engine_heavy",
        AttachKind::StackBottom,
        DVec2::new(0.0, eh_y),
        0.0,
        0,
    );

    let fin_sz = size_of("fin_basic", DVec2::new(0.78, 1.15));
    let fin_y = th_y - 0.34 * th_sz.y;
    bp.attach_child(
        th_id,
        "fin_basic",
        AttachKind::RadialLeft,
        DVec2::new(-0.5 * th_sz.x - 0.5 * fin_sz.x, fin_y),
        0.0,
        0,
    );
    bp.attach_child(
        th_id,
        "fin_basic",
        AttachKind::RadialRight,
        DVec2::new(0.5 * th_sz.x + 0.5 * fin_sz.x, fin_y),
        0.0,
        0,
    );

    bp
}

/// Computes center of mass, center of pressure, MoI, tip-over torque, Δv, and pre-flight warnings.
pub fn analyze_blueprint(bp: &VesselBlueprint, catalog: &PartCatalog) -> BlueprintAnalysis {
    let mut wet_mass = 0.0;
    let mut dry_mass = 0.0;
    let mut fuel_mass = 0.0;
    let mut weighted_pos = DVec2::ZERO;
    let mut cop_weight_sum = 0.0;
    let mut cop_weighted_pos = DVec2::ZERO;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for part in &bp.parts {
        let Some(spec) = catalog.get(&part.spec_id) else {
            continue;
        };
        let m = spec.full_mass_kg();
        wet_mass += m;
        dry_mass += spec.dry_mass_kg;
        fuel_mass += spec.fuel_capacity_kg();
        weighted_pos += part.local_pos * m;

        let sz = spec.shape.bounding_size();
        min_y = min_y.min(part.local_pos.y - 0.5 * sz.y);
        max_y = max_y.max(part.local_pos.y + 0.5 * sz.y);

        let aero_w = match spec.kind {
            PartKind::Fin(f) => f.lift_area_m2 * 3.8,
            _ => spec.drag_coeff * sz.x.max(0.2),
        };
        cop_weight_sum += aero_w;
        cop_weighted_pos += part.local_pos * aero_w;
    }

    let com_local = if wet_mass > 1.0e-9 {
        weighted_pos / wet_mass
    } else {
        DVec2::ZERO
    };
    let cop_local = if cop_weight_sum > 1.0e-9 {
        cop_weighted_pos / cop_weight_sum
    } else {
        com_local
    };

    let mut moi = 0.0;
    for part in &bp.parts {
        let Some(spec) = catalog.get(&part.spec_id) else {
            continue;
        };
        let m = spec.full_mass_kg();
        let d2 = (part.local_pos - com_local).length_squared();
        moi += m * (spec.shape.unit_moi() + d2);
    }

    let active_stage = bp
        .parts
        .iter()
        .filter(|p| {
            catalog.get(&p.spec_id).is_some_and(|s| {
                matches!(s.kind, PartKind::Engine(_) | PartKind::SolidBooster(_))
            })
        })
        .map(|p| p.stage)
        .min()
        .unwrap_or(0);

    let mut net_torque = 0.0;
    for part in &bp.parts {
        let Some(spec) = catalog.get(&part.spec_id) else {
            continue;
        };
        if part.stage != active_stage {
            continue;
        }
        let thrust = match spec.kind {
            PartKind::Engine(eng) => eng.max_thrust_n,
            PartKind::SolidBooster(srb) => srb.max_thrust_n,
            _ => 0.0,
        };
        if thrust > 0.0 {
            let r = part.local_pos - com_local;
            let force = DVec2::Y.rotate(part.local_rot) * thrust;
            net_torque += r.cross(force);
        }
    }

    let stages = compute_stage_tsiolkovsky(bp, catalog);
    let total_delta_v_m_s = stages.iter().map(|s| s.delta_v_m_s).sum();
    let tip_over_warning = net_torque.abs() > TIP_OVER_TORQUE_WARN_NM;
    let stack_len = (max_y - min_y).max(0.0);
    let aero_unstable = stack_len > 2.2 && cop_local.y > com_local.y + 0.15;

    let warnings = build_preflight_warnings(
        bp,
        catalog,
        &stages,
        tip_over_warning,
        net_torque,
        aero_unstable,
    );

    BlueprintAnalysis {
        wet_mass_kg: wet_mass,
        dry_mass_kg: dry_mass,
        fuel_mass_kg: fuel_mass,
        com_local,
        cop_local,
        aero_unstable,
        moi_kg_m2: moi.max(1.0),
        net_thrust_torque_nm: net_torque,
        tip_over_warning,
        total_delta_v_m_s,
        stages,
        warnings,
    }
}

fn build_preflight_warnings(
    bp: &VesselBlueprint,
    catalog: &PartCatalog,
    stages: &[StageSummary],
    tip_over_warning: bool,
    net_torque: f64,
    aero_unstable: bool,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if tip_over_warning {
        warnings.push(format!("Tip-over thrust torque ({net_torque:+.0} N·m)"));
    }
    if aero_unstable {
        warnings.push(String::from("CoP is above CoM — add tail fins"));
    }

    if let Some(s0) = stages.first() {
        if s0.thrust_n <= 1.0 {
            warnings.push(String::from("First active stage has no thrust"));
        } else if s0.twr_surface < 1.05 {
            warnings.push(format!("Low liftoff TWR ({:.2} < 1.05)", s0.twr_surface));
        }
    } else {
        warnings.push(String::from("No propulsion stages configured"));
    }

    let alive_all: HashSet<u64> = bp.parts.iter().map(|p| p.instance_id).collect();
    for p in &bp.parts {
        if let Some(spec) = catalog.get(&p.spec_id)
            && matches!(spec.kind, PartKind::Engine(_))
            && reachable_fuel_tanks_blueprint(&bp.parts, &alive_all, p.instance_id, catalog)
                .is_empty()
        {
            warnings.push(format!("{} has no reachable fuel", spec.display_name));
            break;
        }
    }

    let has_crewed_pod = bp.parts.iter().any(|p| {
        catalog
            .get(&p.spec_id)
            .is_some_and(|s| matches!(s.kind, PartKind::CommandPod(pod) if pod.crewed))
    });
    if has_crewed_pod {
        let has_chute = bp.parts.iter().any(|p| {
            catalog
                .get(&p.spec_id)
                .is_some_and(|s| matches!(s.kind, PartKind::Parachute(_)))
        });
        let has_shield = bp.parts.iter().any(|p| {
            catalog
                .get(&p.spec_id)
                .is_some_and(|s| matches!(s.kind, PartKind::HeatShield(_)))
        });
        if !has_chute {
            warnings.push(String::from("Crewed pod lacks a parachute"));
        }
        if !has_shield {
            warnings.push(String::from("Crewed pod lacks a heat shield"));
        }
    }

    warnings
}

/// Computes stage-by-stage Tsiolkovsky delta-v in firing order (`stage 0, 1, 2...`) for UI readouts.
fn compute_stage_tsiolkovsky(bp: &VesselBlueprint, catalog: &PartCatalog) -> Vec<StageSummary> {
    if bp.parts.is_empty() {
        return Vec::new();
    }

    let max_stage = bp.parts.iter().map(|p| p.stage).max().unwrap_or(0);
    let mut alive: HashSet<u64> = bp.parts.iter().map(|p| p.instance_id).collect();
    let mut fuel_left: HashMap<u64, f64> = bp
        .parts
        .iter()
        .map(|p| {
            let f = catalog
                .get(&p.spec_id)
                .map_or(0.0, PartSpec::fuel_capacity_kg);
            (p.instance_id, f)
        })
        .collect();

    let mut summaries = Vec::new();

    for st in 0..=max_stage {
        let fired_decouplers: Vec<u64> = bp
            .parts
            .iter()
            .filter(|p| {
                alive.contains(&p.instance_id)
                    && p.stage == st
                    && catalog
                        .get(&p.spec_id)
                        .is_some_and(|s| matches!(s.kind, PartKind::Decoupler(_)))
            })
            .map(|p| p.instance_id)
            .collect();

        let has_decoupler = !fired_decouplers.is_empty();

        for dec_id in fired_decouplers {
            let mut queue = VecDeque::new();
            for p in &bp.parts {
                if alive.contains(&p.instance_id) && p.parent_id == Some(dec_id) {
                    queue.push_back(p.instance_id);
                }
            }
            while let Some(drop_id) = queue.pop_front() {
                if alive.remove(&drop_id) {
                    for child in &bp.parts {
                        if alive.contains(&child.instance_id) && child.parent_id == Some(drop_id) {
                            queue.push_back(child.instance_id);
                        }
                    }
                }
            }
        }

        let active_engines: Vec<(&PlacedPart, EngineSpec)> = bp
            .parts
            .iter()
            .filter(|p| alive.contains(&p.instance_id) && p.stage == st)
            .filter_map(|p| {
                let spec = catalog.get(&p.spec_id)?;
                if let PartKind::Engine(eng) = spec.kind {
                    Some((p, eng))
                } else {
                    None
                }
            })
            .collect();

        let active_srbs: Vec<(&PlacedPart, SrbSpec)> = bp
            .parts
            .iter()
            .filter(|p| alive.contains(&p.instance_id) && p.stage == st)
            .filter_map(|p| {
                let spec = catalog.get(&p.spec_id)?;
                if let PartKind::SolidBooster(srb) = spec.kind {
                    Some((p, srb))
                } else {
                    None
                }
            })
            .collect();

        let wet_mass: f64 = bp
            .parts
            .iter()
            .filter(|p| alive.contains(&p.instance_id))
            .map(|p| {
                let dry = catalog.get(&p.spec_id).map_or(0.0, |s| s.dry_mass_kg);
                let fuel = fuel_left.get(&p.instance_id).copied().unwrap_or(0.0);
                dry + fuel
            })
            .sum();

        let mut stage_fuel = 0.0;
        let mut total_thrust = 0.0;
        let mut total_mdot = 0.0;

        if !active_engines.is_empty() {
            let mut reachable_tanks = HashSet::new();
            for (eng_part, eng) in &active_engines {
                total_thrust += eng.max_thrust_n;
                if eng.isp_s > 0.0 {
                    total_mdot += eng.max_thrust_n / (eng.isp_s * G0);
                }
                for tank_id in reachable_fuel_tanks_blueprint(
                    &bp.parts,
                    &alive,
                    eng_part.instance_id,
                    catalog,
                ) {
                    reachable_tanks.insert(tank_id);
                }
            }
            for tank_id in reachable_tanks {
                if let Some(f) = fuel_left.get_mut(&tank_id) {
                    stage_fuel += *f;
                    *f = 0.0;
                }
            }
        }

        for (srb_part, srb) in &active_srbs {
            total_thrust += srb.max_thrust_n;
            if srb.isp_s > 0.0 {
                total_mdot += srb.max_thrust_n / (srb.isp_s * G0);
            }
            if let Some(f) = fuel_left.get_mut(&srb_part.instance_id) {
                stage_fuel += *f;
                *f = 0.0;
            }
        }

        let dry_after_burn = (wet_mass - stage_fuel).max(1.0e-6);
        let isp_eff = if total_mdot > 1.0e-9 {
            total_thrust / (total_mdot * G0)
        } else {
            0.0
        };
        let delta_v = if wet_mass > dry_after_burn && isp_eff > 0.0 {
            isp_eff * G0 * (wet_mass / dry_after_burn).ln()
        } else {
            0.0
        };

        let twr = if wet_mass > 1.0e-6 {
            total_thrust / (wet_mass * KEPLER_PRIME_SURFACE_G)
        } else {
            0.0
        };

        if !active_engines.is_empty() || !active_srbs.is_empty() || has_decoupler {
            summaries.push(StageSummary {
                stage: st,
                wet_mass_kg: wet_mass,
                dry_mass_kg: dry_after_burn,
                fuel_kg: stage_fuel,
                thrust_n: total_thrust,
                twr_surface: twr,
                delta_v_m_s: delta_v,
                has_decoupler,
            });
        }
    }

    summaries
}

/// Traverses the alive part graph across crossfeed-enabled edges to locate fuel tanks for an engine.
fn reachable_fuel_tanks_blueprint(
    parts: &[PlacedPart],
    alive: &HashSet<u64>,
    start_id: u64,
    catalog: &PartCatalog,
) -> Vec<u64> {
    let by_id: HashMap<u64, &PlacedPart> = parts
        .iter()
        .filter(|p| alive.contains(&p.instance_id))
        .map(|p| (p.instance_id, p))
        .collect();

    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    let mut tanks = Vec::new();
    visited.insert(start_id);
    queue.push_back(start_id);

    while let Some(curr) = queue.pop_front() {
        let Some(&curr_part) = by_id.get(&curr) else {
            continue;
        };
        if let Some(spec) = catalog.get(&curr_part.spec_id) {
            if matches!(spec.kind, PartKind::Tank(_)) {
                tanks.push(curr);
            }
            if let PartKind::Decoupler(dec) = spec.kind
                && !(dec.crossfeed || curr_part.crossfeed)
                && curr != start_id
            {
                continue;
            }
        }

        if let Some(pid) = curr_part.parent_id
            && by_id.contains_key(&pid)
            && visited.insert(pid)
        {
            queue.push_back(pid);
        }

        for (&cid, &cpart) in &by_id {
            if cpart.parent_id == Some(curr) && visited.insert(cid) {
                queue.push_back(cid);
            }
        }
    }

    tanks
}

// ---------- TESTS

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_rocket_analysis_matches_two_stage_expectations() {
        let cat = default_part_catalog();
        let bp = starter_two_stage_blueprint(&cat);
        let analysis = analyze_blueprint(&bp, &cat);
        assert!(analysis.total_delta_v_m_s > 4_600.0);
        assert!(!analysis.tip_over_warning);
        assert!(!analysis.aero_unstable);
        assert_eq!(analysis.stages.len(), 2);
        assert!(analysis.stages[0].twr_surface > 1.5);
    }

    #[test]
    fn lunar_rocket_exceeds_moon_transfer_delta_v() {
        let cat = default_part_catalog();
        let bp = lunar_capable_blueprint(&cat);
        let analysis = analyze_blueprint(&bp, &cat);
        assert!(analysis.total_delta_v_m_s > 5_200.0);
        assert!(!analysis.tip_over_warning);
    }

    #[test]
    fn off_axis_engine_triggers_tip_over_torque_warning() {
        let cat = default_part_catalog();
        let mut bp = VesselBlueprint::new("Asymmetric");
        let pod = bp.add_root("command_pod", 0);
        bp.attach_child(
            pod,
            "engine_orbital",
            AttachKind::RadialRight,
            DVec2::new(1.2, -0.5),
            0.0,
            0,
        );
        let analysis = analyze_blueprint(&bp, &cat);
        assert!(analysis.tip_over_warning);
        assert!(analysis.net_thrust_torque_nm.abs() > TIP_OVER_TORQUE_WARN_NM);
    }

    #[test]
    fn crossfeed_enabled_radial_decoupler_shares_fuel() {
        let cat = default_part_catalog();
        let mut bp = VesselBlueprint::new("Asparagus");
        let pod = bp.add_root("probe_core", 0);
        let core_tank = bp.attach_child(
            pod,
            "tank_small",
            AttachKind::StackBottom,
            DVec2::new(0.0, -1.11),
            0.0,
            0,
        );
        bp.attach_child(
            core_tank,
            "engine_orbital",
            AttachKind::StackBottom,
            DVec2::new(0.0, -2.53),
            0.0,
            0,
        );
        let pylon = bp.attach_child(
            core_tank,
            "decoupler_radial",
            AttachKind::RadialLeft,
            DVec2::new(-0.785, -1.11),
            0.0,
            1,
        );
        bp.set_part_crossfeed(pylon, true);
        bp.attach_child(
            pylon,
            "tank_small",
            AttachKind::RadialLeft,
            DVec2::new(-1.57, -1.11),
            0.0,
            1,
        );
        let analysis = analyze_blueprint(&bp, &cat);
        assert!((analysis.stages[0].fuel_kg - 2_400.0).abs() < 1.0e-6);
    }
}
