//! @file parts.rs
//!
//! @version 1.0.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Rocket part specifications, blueprints, mass/inertia properties, and Tsiolkovsky staging UI analysis
//!
//! @description
//! Defines part geometry shapes, component specs (Pod, Tank, Engine, Decoupler, Fin, RCS,
//! Parachute), tree-structured vessel blueprints, and engineering telemetry calculations including
//! center of mass, moment of inertia, stage-by-stage Tsiolkovsky delta-v, TWR, and off-axis
//! tip-over torque warnings.
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

/// Procedural vector shape shared by physics footprints, attach nodes, and Bevy mesh generation.
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
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PodSpec {
    pub reaction_torque_nm: f64,
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

/// Functional category and parameters for a rocket part.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PartKind {
    CommandPod(PodSpec),
    Tank(TankSpec),
    Engine(EngineSpec),
    Decoupler(DecouplerSpec),
    Fin(FinSpec),
    Rcs(RcsSpec),
    Parachute(ParachuteSpec),
}

/// Immutable data-driven definition for a part loaded from `.part.ron` or built-in defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct PartSpec {
    pub id: String,
    pub display_name: String,
    pub dry_mass_kg: f64,
    pub shape: PartShape,
    pub kind: PartKind,
    pub stack_top: bool,
    pub stack_bottom: bool,
    pub radial_attach: bool,
    pub drag_coeff: f64,
    pub max_impact_m_s: f64,
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

/// Complete engineering telemetry readout for a `VesselBlueprint`.
#[derive(Clone, Debug, PartialEq)]
pub struct BlueprintAnalysis {
    pub wet_mass_kg: f64,
    pub dry_mass_kg: f64,
    pub fuel_mass_kg: f64,
    pub com_local: DVec2,
    pub moi_kg_m2: f64,
    pub net_thrust_torque_nm: f64,
    pub tip_over_warning: bool,
    pub total_delta_v_m_s: f64,
    pub stages: Vec<StageSummary>,
}

// ---------- DECLARATIONS / IMPLEMENTATION

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
    /// Wet mass of a single part at full tanks.
    pub fn full_mass_kg(&self) -> f64 {
        match self.kind {
            PartKind::Tank(t) => self.dry_mass_kg + t.fuel_capacity_kg,
            _ => self.dry_mass_kg,
        }
    }

    pub fn fuel_capacity_kg(&self) -> f64 {
        match self.kind {
            PartKind::Tank(t) => t.fuel_capacity_kg,
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
        });
        id
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
}

/// Built-in default part catalog mirroring `assets/parts/*.part.ron`.
pub fn default_part_catalog() -> PartCatalog {
    let mut cat = PartCatalog::new();
    cat.insert(PartSpec {
        id: String::from("command_pod"),
        display_name: String::from("Mk-1 Command Pod"),
        dry_mass_kg: 600.0,
        shape: PartShape::Cone {
            bottom_width: 1.25,
            top_width: 0.55,
            height: 1.20,
        },
        kind: PartKind::CommandPod(PodSpec {
            reaction_torque_nm: 4_500.0,
        }),
        stack_top: true,
        stack_bottom: true,
        radial_attach: false,
        drag_coeff: 0.28,
        max_impact_m_s: 16.0,
    });
    cat.insert(PartSpec {
        id: String::from("tank_small"),
        display_name: String::from("FL-S1200 LFO Tank"),
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
    });
    cat.insert(PartSpec {
        id: String::from("tank_large"),
        display_name: String::from("FL-L3600 Core Tank"),
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
    });
    cat.insert(PartSpec {
        id: String::from("engine_orbital"),
        display_name: String::from("LV-28V 'Vesper'"),
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
    });
    cat.insert(PartSpec {
        id: String::from("engine_booster"),
        display_name: String::from("LV-110 'Titan'"),
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
    });
    cat.insert(PartSpec {
        id: String::from("decoupler_stack"),
        display_name: String::from("TR-16 Stack Decoupler"),
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
    });
    cat.insert(PartSpec {
        id: String::from("fin_basic"),
        display_name: String::from("AV-R4 Delta Fin"),
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
    });
    cat
}

/// Builds the default two-stage orbital rocket (~4,818 m/s total delta-v).
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
        core::f64::consts::PI,
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

/// Computes center of mass, moment of inertia, off-axis torque warning, and Tsiolkovsky Δv per stage.
pub fn analyze_blueprint(bp: &VesselBlueprint, catalog: &PartCatalog) -> BlueprintAnalysis {
    let mut wet_mass = 0.0;
    let mut dry_mass = 0.0;
    let mut fuel_mass = 0.0;
    let mut weighted_pos = DVec2::ZERO;

    for part in &bp.parts {
        let Some(spec) = catalog.get(&part.spec_id) else {
            continue;
        };
        let m = spec.full_mass_kg();
        wet_mass += m;
        dry_mass += spec.dry_mass_kg;
        fuel_mass += spec.fuel_capacity_kg();
        weighted_pos += part.local_pos * m;
    }

    let com_local = if wet_mass > 1.0e-9 {
        weighted_pos / wet_mass
    } else {
        DVec2::ZERO
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
            catalog
                .get(&p.spec_id)
                .is_some_and(|s| matches!(s.kind, PartKind::Engine(_)))
        })
        .map(|p| p.stage)
        .min()
        .unwrap_or(0);

    let mut net_torque = 0.0;
    for part in &bp.parts {
        let Some(spec) = catalog.get(&part.spec_id) else {
            continue;
        };
        if let PartKind::Engine(eng) = spec.kind
            && part.stage == active_stage
        {
            let r = part.local_pos - com_local;
            let force = DVec2::Y.rotate(part.local_rot) * eng.max_thrust_n;
            net_torque += r.cross(force);
        }
    }

    let stages = compute_stage_tsiolkovsky(bp, catalog);
    let total_delta_v_m_s = stages.iter().map(|s| s.delta_v_m_s).sum();
    let tip_over_warning = net_torque.abs() > TIP_OVER_TORQUE_WARN_NM;

    BlueprintAnalysis {
        wet_mass_kg: wet_mass,
        dry_mass_kg: dry_mass,
        fuel_mass_kg: fuel_mass,
        com_local,
        moi_kg_m2: moi.max(1.0),
        net_thrust_torque_nm: net_torque,
        tip_over_warning,
        total_delta_v_m_s,
        stages,
    }
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

        let has_decoupler = bp.parts.iter().any(|p| {
            alive.contains(&p.instance_id)
                && p.stage == st
                && catalog
                    .get(&p.spec_id)
                    .is_some_and(|s| matches!(s.kind, PartKind::Decoupler(_)))
        });

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

        if !active_engines.is_empty() || has_decoupler {
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
                && !dec.crossfeed
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
        assert_eq!(analysis.stages.len(), 2);
        assert!(analysis.stages[0].twr_surface > 1.5);
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
}
