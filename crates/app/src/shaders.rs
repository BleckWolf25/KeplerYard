//! @file shaders.rs
//!
//! @version 1.2.0
//! @author `BleckWolf25`
//! @license MIT
//!
//! @summary Bevy 0.20 custom WESL 2D materials for atmospheres, plumes, reentry plasma, and parts
//!
//! @description
//! Registers `Material2dPlugin` pipelines backed by hot-reloadable `.wesl` fragment shaders:
//! `AtmosphereMaterial` (with sun terminator & surface detail), `PlumeMaterial` (pressure-aware),
//! `ReentryMaterial`, and `PartMaterial` (with separated highlight vs thermal incandescent glow).
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};

// ---------- CONSTANTS

// ---------- TYPES

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct AtmosphereMaterial {
    #[uniform(0)]
    pub planet_params: Vec4,
    #[uniform(0)]
    pub atmo_params: Vec4,
    #[uniform(0)]
    pub surface_color: Vec4,
    #[uniform(0)]
    pub atmo_color: Vec4,
    #[uniform(0)]
    pub sun_and_detail: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct PlumeMaterial {
    #[uniform(0)]
    pub params: Vec4,
    #[uniform(0)]
    pub core_color: Vec4,
    #[uniform(0)]
    pub outer_color: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct ReentryMaterial {
    #[uniform(0)]
    pub params: Vec4,
    #[uniform(0)]
    pub plasma_color: Vec4,
    #[uniform(0)]
    pub wake_color: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct PartMaterial {
    #[uniform(0)]
    pub base_color: Vec4,
    #[uniform(0)]
    pub accent_color: Vec4,
    #[uniform(0)]
    pub flags: Vec4,
    #[uniform(0)]
    pub thermal: Vec4,
}

pub struct KeplerShadersPlugin;

// ---------- DECLARATIONS / IMPLEMENTATION

impl Material2d for AtmosphereMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/atmosphere.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

impl Material2d for PlumeMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/plume.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

impl Material2d for ReentryMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/reentry.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

impl Material2d for PartMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/part_glow.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

impl Plugin for KeplerShadersPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            Material2dPlugin::<AtmosphereMaterial>::default(),
            Material2dPlugin::<PlumeMaterial>::default(),
            Material2dPlugin::<ReentryMaterial>::default(),
            Material2dPlugin::<PartMaterial>::default(),
        ));
    }
}

// ---------- EXPORTS
