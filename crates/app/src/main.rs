//! @file main.rs
//!
//! @version 1.4.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Application entrypoint and Bevy plugin wiring for Kepler Yard
//!
//! @description
//! Configures Bevy 0.20.1 with 2D rendering, HDR bloom, hot-reloading asset file watching,
//! custom WESL shader materials, automatic RON part discovery, procedural part & UI icons,
//! categorized freeform VAB editor, and full cislunar N-body flight.
//!
//! @since 21/11/2026
//! @updated 22/11/2026
// The project header uses bare author metadata rather than doc-markdown code formatting.
#![allow(clippy::doc_markdown)]
// ---------- IMPORTS
use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::window::WindowResolution;
use bevy::window::{MonitorSelection, WindowMode};

// ---------- DECLARATIONS / IMPLEMENTATION

mod camera;
mod editor;
mod flight;
mod flight_fx;
mod flight_hud;
mod main_menu;
mod part_catalog;
mod part_icons;
mod procedural_mesh;
mod save_io;
mod shaders;
mod state;
mod ui_kit;

fn toggle_fullscreen(
    keys: Res<'_, ButtonInput<KeyCode>>,
    mut settings: ResMut<'_, state::AppSettings>,
    mut windows: Query<'_, '_, &mut Window>,
) {
    if keys.just_pressed(KeyCode::F11)
        && let Ok(mut window) = windows.single_mut()
    {
        window.mode = if matches!(window.mode, WindowMode::Windowed) {
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        } else {
            WindowMode::Windowed
        };
        settings.borderless_fullscreen = matches!(
            window.mode,
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        );
        let _ = save_io::save_settings(settings.borderless_fullscreen, settings.show_tutorial);
    }
}

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.010, 0.015, 0.032)))
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: String::from("Kepler Yard"),
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
            ui_kit::UiKitPlugin,
            main_menu::MainMenuPlugin,
            part_catalog::PartCatalogPlugin,
            part_icons::PartIconsPlugin,
            shaders::KeplerShadersPlugin,
            camera::FloatingCameraPlugin,
            editor::EditorPlugin,
            flight::FlightPlugin,
        ))
        .add_systems(Update, toggle_fullscreen)
        .run();
}

// ---------- EXPORTS
