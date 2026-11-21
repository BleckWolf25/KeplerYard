//! @file main.rs
//!
//! @version 1.0.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Application entrypoint and Bevy plugin wiring for Kepler Yard Milestones 1 and 2
//!
//! @description
//! Configures Bevy 0.20.1 with 2D rendering, HDR bloom, hot-reloading asset file watching,
//! custom WESL shader materials, RON part loading, the VAB construction editor, orbital flight
//! synchronization, floating-origin camera control, and simulation HUD.
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
mod part_catalog;
mod procedural_mesh;
mod shaders;
mod state;

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.015, 0.022, 0.042)))
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: String::from("Kepler Yard — Orbital Foundry"),
                        resolution: WindowResolution::new(1440, 900),
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
            shaders::KeplerShadersPlugin,
            camera::FloatingCameraPlugin,
            editor::EditorPlugin,
            flight::FlightPlugin,
        ))
        .run();
}

// ---------- EXPORTS
