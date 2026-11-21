//! @file main.rs
//!
//! @version 1.2.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Application entrypoint and Bevy plugin wiring for Kepler Yard
//!
//! @description
//! Configures Bevy 0.20.1 with 2D rendering, HDR bloom, hot-reloading asset file watching,
//! custom WESL shader materials, automatic RON part discovery, procedural part & UI icons,
//! categorized drag-and-drop VAB editor with staging/CoP/craft library, and full cislunar
//! N-body flight with navball, SAS modes, FX, and save/load.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::window::WindowResolution;

// ---------- CONSTANTS

// ---------- TYPES

// ---------- DECLARATIONS / IMPLEMENTATION

mod camera;
mod editor;
mod flight;
mod flight_fx;
mod flight_hud;
mod part_catalog;
mod part_icons;
mod procedural_mesh;
mod save_io;
mod shaders;
mod state;

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.010, 0.015, 0.032)))
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: String::from("Kepler Yard — Orbital Foundry"),
                        resolution: WindowResolution::new(1520, 940),
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    watch_for_changes_override: Some(true),
                    ..default()
                }),
        )
        .init_state::<state::GameMode>()
        .add_plugins((
            state::StatePlugin,
            part_catalog::PartCatalogPlugin,
            part_icons::PartIconsPlugin,
            shaders::KeplerShadersPlugin,
            camera::FloatingCameraPlugin,
            editor::EditorPlugin,
            flight::FlightPlugin,
        ))
        .run();
}

// ---------- EXPORTS
