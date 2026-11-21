//! @file part_icons.rs
//!
//! @version 1.1.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Procedural RGBA image generator for part palette thumbnails and UI category/action icons
//!
//! @description
//! Software-rasterizes anti-aliased `64x64` RGBA `Image` previews for every `PartSpec` directly
//! from its `PartShape` and RON colors so the editor palette displays actual images of every
//! rocket part (including landing struts, SRBs, and RCS blocks), plus `24x24` UI icons.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use crate::state::{LivePartCatalog, PartVisualStyle};
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use sim_core::{PartCategory, PartShape, PartSpec};
use std::collections::HashMap;

// ---------- CONSTANTS

const PART_ICON_PX: u32 = 64;
const UI_ICON_PX: u32 = 24;

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

#[derive(Resource, Default)]
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

fn init_ui_icons(
    mut images: ResMut<Assets<Image>>,
    mut registry: ResMut<PartIconRegistry>,
    catalog: Res<LivePartCatalog>,
) {
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
    rebuild_part_icons(&catalog, &mut images, &mut registry);
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
    let scale = 46.0 / max_dim;

    for py in 0..sz {
        for px in 0..sz {
            let x = (px as f32 + 0.5 - sz as f32 * 0.5) / scale;
            let y = -(py as f32 + 0.5 - sz as f32 * 0.5) / scale;
            let sdf = part_shape_sdf(&spec.shape, x, y);
            let idx = ((py * sz + px) * 4) as usize;

            if sdf <= 0.04 {
                let coverage = ((-sdf + 0.04) / 0.08).clamp(0.0, 1.0);
                let edge_band = (1.0 - (-sdf * 14.0).clamp(0.0, 1.0)) * 0.65;
                let center_stripe =
                    (1.0 - (x.abs() / (dims.x as f32 * 0.14).max(0.04))).clamp(0.0, 1.0);
                let mut r = style.base_color[0] * 0.90
                    + style.accent_color[0] * (center_stripe * 0.45 + edge_band * 0.55);
                let mut g = style.base_color[1] * 0.90
                    + style.accent_color[1] * (center_stripe * 0.45 + edge_band * 0.55);
                let mut b = style.base_color[2] * 0.90
                    + style.accent_color[2] * (center_stripe * 0.45 + edge_band * 0.55);
                let shade =
                    0.85 + 0.25 * (1.0 - (x / (dims.x as f32 * 0.5).max(0.1)).abs()).clamp(0.0, 1.0);
                r *= shade;
                g *= shade;
                b *= shade;

                put_pixel(&mut data, idx, r, g, b, coverage);
            } else {
                let glow = (-sdf * 6.5).exp() * 0.28;
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

fn part_shape_sdf(shape: &PartShape, x: f32, y: f32) -> f32 {
    match *shape {
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
            let leg_d = (x - leg_center).abs().max(y.abs() - hh) - 0.10 * hx;
            let foot_d = (x + 0.5 * hx).abs().max((y + 0.9 * hh).abs()) - 0.14 * hh;
            leg_d.min(foot_d)
        }
    }
}

fn rasterize_ui_icon(kind: UiIconKind) -> Image {
    let sz = UI_ICON_PX;
    let mut data = vec![0_u8; (sz * sz * 4) as usize];

    let (cr, cg, cb) = match kind {
        UiIconKind::All => (0.45, 0.90, 1.0),
        UiIconKind::Command => (0.25, 0.88, 1.0),
        UiIconKind::Propulsion => (1.0, 0.52, 0.16),
        UiIconKind::FuelTanks => (0.95, 0.80, 0.25),
        UiIconKind::AeroAndThermal => (0.98, 0.42, 0.24),
        UiIconKind::Structural => (0.72, 0.82, 0.95),
        UiIconKind::Save => (0.35, 0.88, 1.0),
        UiIconKind::Load => (0.95, 0.78, 0.28),
        UiIconKind::Launch => (0.30, 0.98, 0.62),
        UiIconKind::RocketPreset => (0.58, 0.85, 1.0),
    };

    for py in 0..sz {
        for px in 0..sz {
            let u = (px as f32 + 0.5) / sz as f32 * 2.0 - 1.0;
            let v = -((py as f32 + 0.5) / sz as f32 * 2.0 - 1.0);
            let inside = match kind {
                UiIconKind::All => {
                    u.abs() > 0.12 && u.abs() < 0.72 && v.abs() > 0.12 && v.abs() < 0.72
                }
                UiIconKind::Command => (u.abs() * 1.4 + v).abs() < 0.65 && v > -0.6 && v < 0.75,
                UiIconKind::Propulsion => {
                    (u.abs() < (0.55 - 0.3 * v) && v > -0.35 && v < 0.7)
                        || (u.abs() < 0.22 && v <= -0.35 && v > -0.82)
                }
                UiIconKind::FuelTanks => u.abs() < 0.42 && v.abs() < 0.72 && (v.abs() > 0.08),
                UiIconKind::AeroAndThermal => {
                    u.abs() < 0.75 && v < 0.7 && v > -0.55 && (u * u + (v + 0.2) * (v + 0.2) < 0.68)
                }
                UiIconKind::Structural => u.abs() < 0.72 && v.abs() < 0.32,
                UiIconKind::Save => {
                    (u.abs() < 0.18 && v > -0.15 && v < 0.75)
                        || (v <= -0.10 && v > -0.65 && u.abs() < (-v - 0.05) * 1.1)
                        || (v < -0.68 && v > -0.85 && u.abs() < 0.7)
                }
                UiIconKind::Load => {
                    (u.abs() < 0.18 && v < 0.25 && v > -0.55)
                        || ((0.10..0.70).contains(&v) && u.abs() < (0.75 - v) * 1.1)
                        || (v < -0.68 && v > -0.85 && u.abs() < 0.7)
                }
                UiIconKind::Launch | UiIconKind::RocketPreset => {
                    (u.abs() < 0.28 * (1.0 - v * 0.45) && v > -0.45 && v < 0.82)
                        || (v <= -0.15 && v > -0.65 && u.abs() < 0.68 && u.abs() > 0.26)
                }
            };
            if inside {
                let idx = ((py * sz + px) * 4) as usize;
                put_pixel(&mut data, idx, cr, cg, cb, 1.0);
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
            .add_systems(Startup, init_ui_icons)
            .add_systems(Update, refresh_part_icons_on_catalog_change);
    }
}

// ---------- EXPORTS
