//! @file part_icons.rs
//!
//! @version 1.2.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Pre-frame-0 procedural RGBA thumbnails and UI category/action icons via FromWorld
//!
//! @description
//! Rasterizes `96x96` shaded SFS-style part thumbnails and `32x32` UI category/action icons inside
//! `impl FromWorld for PartIconRegistry`, ensuring every `ImageNode` handle is already valid before
//! `OnEnter(GameMode::Editor)` spawns UI panels.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use crate::state::{LivePartCatalog, PartVisualStyle, part_style_id};
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use sim_core::{PartCategory, PartShape, PartSpec};
use std::collections::HashMap;

// ---------- CONSTANTS

const PART_ICON_PX: u32 = 96;
const UI_ICON_PX: u32 = 32;

// ---------- TYPES

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UiIconKind {
    All,
    Command,
    Propulsion,
    FuelTanks,
    AeroAndThermal,
    Structural,
    Save,
    Load,
    Launch,
    RocketPreset,
}

#[derive(Resource)]
pub struct PartIconRegistry {
    pub part_icons: HashMap<String, Handle<Image>>,
    pub ui_icons: HashMap<UiIconKind, Handle<Image>>,
    last_catalog_len: usize,
}

pub struct PartIconsPlugin;

// ---------- DECLARATIONS / IMPLEMENTATION

impl UiIconKind {
    pub fn from_category(cat: PartCategory) -> Self {
        match cat {
            PartCategory::Command => Self::Command,
            PartCategory::Propulsion => Self::Propulsion,
            PartCategory::FuelTanks => Self::FuelTanks,
            PartCategory::AeroAndThermal => Self::AeroAndThermal,
            PartCategory::Structural => Self::Structural,
        }
    }
}

impl FromWorld for PartIconRegistry {
    fn from_world(world: &mut World) -> Self {
        let mut registry = Self {
            part_icons: HashMap::new(),
            ui_icons: HashMap::new(),
            last_catalog_len: 0,
        };
        let catalog = world.resource::<LivePartCatalog>().clone();
        let mut images = world.resource_mut::<Assets<Image>>();
        populate_ui_icons(&mut images, &mut registry);
        rebuild_part_icons(&catalog, &mut images, &mut registry);
        registry
    }
}

fn populate_ui_icons(images: &mut Assets<Image>, registry: &mut PartIconRegistry) {
    let kinds = [
        UiIconKind::All,
        UiIconKind::Command,
        UiIconKind::Propulsion,
        UiIconKind::FuelTanks,
        UiIconKind::AeroAndThermal,
        UiIconKind::Structural,
        UiIconKind::Save,
        UiIconKind::Load,
        UiIconKind::Launch,
        UiIconKind::RocketPreset,
    ];
    for kind in kinds {
        let img = rasterize_ui_icon(kind);
        registry.ui_icons.insert(kind, images.add(img));
    }
}

fn refresh_part_icons_on_catalog_change(
    catalog: Res<LivePartCatalog>,
    mut images: ResMut<Assets<Image>>,
    mut registry: ResMut<PartIconRegistry>,
) {
    if catalog.is_changed() || registry.last_catalog_len != catalog.sim_catalog.len() {
        rebuild_part_icons(&catalog, &mut images, &mut registry);
    }
}

fn rebuild_part_icons(
    catalog: &LivePartCatalog,
    images: &mut Assets<Image>,
    registry: &mut PartIconRegistry,
) {
    registry.last_catalog_len = catalog.sim_catalog.len();
    for spec in catalog.sim_catalog.iter() {
        let style = catalog.visuals.get(&spec.id).copied().unwrap_or_default();
        let img = rasterize_part_thumbnail(spec, style);
        registry.part_icons.insert(spec.id.clone(), images.add(img));
    }
}

fn rasterize_part_thumbnail(spec: &PartSpec, style: PartVisualStyle) -> Image {
    let sz = PART_ICON_PX;
    let mut data = vec![0_u8; (sz * sz * 4) as usize];
    let dims = spec.shape.bounding_size();
    let max_dim = dims.x.max(dims.y).max(0.25) as f32;
    let scale = 74.0 / max_dim;
    let style_num = part_style_id(spec) as i32;

    for py in 0..sz {
        for px in 0..sz {
            let x = (px as f32 + 0.5 - sz as f32 * 0.5) / scale;
            let y = -(py as f32 + 0.5 - sz as f32 * 0.5) / scale;
            let (sdf, u_local, v_local) = part_shape_sdf(&spec.shape, x, y);
            let idx = ((py * sz + px) * 4) as usize;

            if sdf <= 0.03 {
                let coverage = ((-sdf + 0.03) / 0.06).clamp(0.0, 1.0);
                let nx = ((u_local - 0.5) * 2.0).clamp(-0.99, 0.99);
                let nz = (1.0 - nx * nx).max(0.04).sqrt();
                let cyl = 0.44 + 0.65 * (nx * -0.55 + nz * 0.78).max(0.0);

                let mut r = style.base_color[0] * cyl;
                let mut g = style.base_color[1] * cyl;
                let mut b = style.base_color[2] * cyl;

                if style_num == 0 {
                    // Tank collars & accent stripe
                    if !(0.06..=0.94).contains(&v_local) {
                        r *= 0.55;
                        g *= 0.55;
                        b *= 0.55;
                    } else if (0.83..=0.91).contains(&v_local) || (u_local - 0.5).abs() < 0.07 {
                        r = style.accent_color[0] * cyl;
                        g = style.accent_color[1] * cyl;
                        b = style.accent_color[2] * cyl;
                    }
                } else if style_num == 1 {
                    // Command pod window & collar
                    let win_d = ((u_local - 0.5).powi(2) + ((v_local - 0.58) * 1.3).powi(2)).sqrt();
                    if win_d < 0.16 {
                        r = style.accent_color[0] * 0.85;
                        g = style.accent_color[1] * 0.85;
                        b = style.accent_color[2] * 0.95;
                    } else if v_local > 0.85 || v_local < 0.12 {
                        r *= 0.6;
                        g *= 0.6;
                        b *= 0.6;
                    }
                } else if style_num == 2 {
                    // Probe gold foil
                    if (0.20..=0.80).contains(&v_local) {
                        r = 0.88 * cyl;
                        g = 0.66 * cyl;
                        b = 0.18 * cyl;
                    }
                } else if style_num == 3 || style_num == 4 {
                    // Engine / Booster accent & nozzle shadow
                    if v_local > 0.70 || v_local < 0.12 {
                        r *= 0.65;
                        g *= 0.65;
                        b *= 0.65;
                    }
                    if style_num == 4 && (0.74..=0.82).contains(&v_local) {
                        r = style.accent_color[0] * cyl;
                        g = style.accent_color[1] * cyl;
                        b = style.accent_color[2] * cyl;
                    }
                } else if (style_num == 5 || style_num == 6) && (0.36..=0.64).contains(&v_local) {
                    r = style.accent_color[0] * cyl;
                    g = style.accent_color[1] * cyl;
                    b = style.accent_color[2] * cyl;
                }

                let edge_hi = (1.0 - (-sdf * 18.0).clamp(0.0, 1.0)) * 0.22;
                put_pixel(&mut data, idx, r + edge_hi, g + edge_hi, b + edge_hi, coverage);
            } else {
                let glow = (-sdf * 7.5).exp() * 0.24;
                if glow > 0.02 {
                    put_pixel(
                        &mut data,
                        idx,
                        style.accent_color[0] * glow,
                        style.accent_color[1] * glow,
                        style.accent_color[2] * glow,
                        glow,
                    );
                }
            }
        }
    }

    Image::new(
        Extent3d {
            width: sz,
            height: sz,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

fn part_shape_sdf(shape: &PartShape, x: f32, y: f32) -> (f32, f32, f32) {
    let dims = shape.bounding_size();
    let v_norm = (y / (dims.y as f32).max(0.05) + 0.5).clamp(0.0, 1.0);
    let mut u_norm = (x / (dims.x as f32).max(0.05) + 0.5).clamp(0.0, 1.0);

    let sdf = match *shape {
        PartShape::RoundedRect {
            width,
            height,
            corner_radius,
        } => {
            let hw = width as f32 * 0.5;
            let hh = height as f32 * 0.5;
            let r = (corner_radius as f32).min(hw * 0.45).min(hh * 0.45);
            let qx = x.abs() - (hw - r);
            let qy = y.abs() - (hh - r);
            Vec2::new(qx.max(0.0), qy.max(0.0)).length() + qx.max(qy).min(0.0) - r
        }
        PartShape::Cone {
            bottom_width,
            top_width,
            height,
        } => {
            let h = height as f32;
            let t = ((y / h) + 0.5).clamp(0.0, 1.0);
            let half_w = 0.5 * (bottom_width as f32 * (1.0 - t) + top_width as f32 * t);
            u_norm = (x / (2.0 * half_w).max(0.05) + 0.5).clamp(0.0, 1.0);
            (x.abs() - half_w).max(y.abs() - 0.5 * h)
        }
        PartShape::Nozzle {
            mount_width,
            throat_width,
            bell_width,
            height,
        } => {
            let h = height as f32;
            let t = ((y / h) + 0.5).clamp(0.0, 1.0);
            let half_w = if t > 0.64 {
                let u = (t - 0.64) / 0.36;
                0.5 * (throat_width as f32 * (1.0 - u) + mount_width as f32 * u)
            } else {
                let u = t / 0.64;
                0.5 * (bell_width as f32 * (1.0 - u) + throat_width as f32 * u)
            };
            u_norm = (x / (2.0 * half_w).max(0.05) + 0.5).clamp(0.0, 1.0);
            (x.abs() - half_w).max(y.abs() - 0.5 * h)
        }
        PartShape::Fin {
            root_chord,
            tip_chord,
            span,
            sweep,
        } => {
            let hx = span as f32 * 0.5;
            let hy = root_chord as f32 * 0.5;
            let u = ((hx - x) / span as f32).clamp(0.0, 1.0);
            let y_top = hy - (sweep as f32) * u;
            let y_bot = -hy + (root_chord as f32 - tip_chord as f32 - sweep as f32) * u;
            let dy = (y - y_top).max(y_bot - y);
            (x.abs() - hx).max(dy)
        }
        PartShape::Dome { width, height } => {
            let hw = (width as f32 * 0.5).max(0.05);
            let hh = (height as f32 * 0.5).max(0.05);
            let u = x / hw;
            let v = (y + hh) / (2.0 * hh);
            let ell = (u * u + v * v).sqrt() - 1.0;
            ell.max(-y - hh) * hw.min(hh)
        }
        PartShape::LandingStrut {
            width,
            height,
            foot_span,
        } => {
            let hx = (width.max(foot_span) as f32 * 0.5).max(0.1);
            let hh = height as f32 * 0.5;
            let t = ((hh - y) / (height as f32)).clamp(0.0, 1.0);
            let leg_center = hx * (0.65 - 1.15 * t);
            let leg_d = (x - leg_center).abs().max(y.abs() - hh) - 0.12 * hx;
            let foot_d = (x + 0.5 * hx).abs().max((y + 0.9 * hh).abs()) - 0.15 * hh;
            leg_d.min(foot_d)
        }
        PartShape::Booster {
            width,
            height,
            nozzle_width,
        } => {
            let h = height as f32;
            let t = ((y / h) + 0.5).clamp(0.0, 1.0);
            let half_w = if t > 0.86 {
                let u = (t - 0.86) / 0.14;
                0.5 * (width as f32 * (1.0 - u) + (width as f32 * 0.18) * u)
            } else if t < 0.12 {
                let u = t / 0.12;
                0.5 * (nozzle_width as f32 * (1.0 - u) + (width as f32 * 0.55) * u)
            } else {
                0.5 * width as f32
            };
            u_norm = (x / (2.0 * half_w).max(0.05) + 0.5).clamp(0.0, 1.0);
            (x.abs() - half_w).max(y.abs() - 0.5 * h)
        }
    };

    (sdf, u_norm, v_norm)
}

fn rasterize_ui_icon(kind: UiIconKind) -> Image {
    let sz = UI_ICON_PX;
    let mut data = vec![0_u8; (sz * sz * 4) as usize];

    let (cr, cg, cb) = match kind {
        UiIconKind::All => (0.55, 0.92, 1.0),
        UiIconKind::Command => (0.28, 0.88, 1.0),
        UiIconKind::Propulsion => (1.0, 0.56, 0.18),
        UiIconKind::FuelTanks => (0.98, 0.82, 0.28),
        UiIconKind::AeroAndThermal => (1.0, 0.45, 0.24),
        UiIconKind::Structural => (0.76, 0.86, 0.98),
        UiIconKind::Save => (0.35, 0.90, 1.0),
        UiIconKind::Load => (0.98, 0.80, 0.30),
        UiIconKind::Launch => (0.32, 0.98, 0.64),
        UiIconKind::RocketPreset => (0.62, 0.88, 1.0),
    };

    for py in 0..sz {
        for px in 0..sz {
            let u = (px as f32 + 0.5) / sz as f32 * 2.0 - 1.0;
            let v = -((py as f32 + 0.5) / sz as f32 * 2.0 - 1.0);
            let sdf = match kind {
                UiIconKind::All => {
                    let qx = (u.abs() - 0.42).abs() - 0.22;
                    let qy = (v.abs() - 0.42).abs() - 0.22;
                    qx.max(qy)
                }
                UiIconKind::Command => {
                    let w = 0.55 * (1.0 - (v + 0.6) * 0.42);
                    (u.abs() - w).max(v.abs() - 0.68)
                }
                UiIconKind::Propulsion => {
                    let bell = (u.abs() - (0.32 - 0.26 * v)).max(v.abs() - 0.52);
                    let flame = (u.abs() - 0.18).max((v + 0.64).abs() - 0.20);
                    bell.min(flame)
                }
                UiIconKind::FuelTanks => {
                    let tank = (u.abs() - 0.40).max(v.abs() - 0.68);
                    let gap = 0.08 - v.abs();
                    tank.max(gap)
                }
                UiIconKind::AeroAndThermal => {
                    let dome = (u * u + (v + 0.15).powi(2)).sqrt() - 0.72;
                    dome.max(-v - 0.45)
                }
                UiIconKind::Structural => (u.abs() - 0.68).max(v.abs() - 0.28),
                UiIconKind::Save => {
                    let stem = (u.abs() - 0.16).max((v - 0.25).abs() - 0.45);
                    let head = (u.abs() - (0.55 + v * 0.9)).max(-v - 0.52).max(v + 0.05);
                    let tray = (u.abs() - 0.68).max((v + 0.74).abs() - 0.10);
                    stem.min(head).min(tray)
                }
                UiIconKind::Load => {
                    let stem = (u.abs() - 0.16).max((v + 0.15).abs() - 0.35);
                    let head = (u.abs() - (0.62 - v * 0.9)).max(v - 0.68).max(-v + 0.10);
                    let tray = (u.abs() - 0.68).max((v + 0.74).abs() - 0.10);
                    stem.min(head).min(tray)
                }
                UiIconKind::Launch | UiIconKind::RocketPreset => {
                    let body = (u.abs() - 0.24 * (1.0 - v * 0.35)).max(v.abs() - 0.72);
                    let fins = (u.abs() - 0.58)
                        .max(0.22 - u.abs())
                        .max((v + 0.45).abs() - 0.24);
                    body.min(fins)
                }
            };
            if sdf <= 0.08 {
                let a = ((-sdf + 0.08) / 0.16).clamp(0.0, 1.0);
                let idx = ((py * sz + px) * 4) as usize;
                put_pixel(&mut data, idx, cr, cg, cb, a);
            }
        }
    }

    Image::new(
        Extent3d {
            width: sz,
            height: sz,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

fn put_pixel(buf: &mut [u8], idx: usize, r: f32, g: f32, b: f32, a: f32) {
    if let Some(slot) = buf.get_mut(idx..idx + 4) {
        slot[0] = (r.clamp(0.0, 1.0) * 255.0) as u8;
        slot[1] = (g.clamp(0.0, 1.0) * 255.0) as u8;
        slot[2] = (b.clamp(0.0, 1.0) * 255.0) as u8;
        slot[3] = (a.clamp(0.0, 1.0) * 255.0) as u8;
    }
}

impl Plugin for PartIconsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PartIconRegistry>()
            .add_systems(Update, refresh_part_icons_on_catalog_change);
    }
}

// ---------- EXPORTS
