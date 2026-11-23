//! @file main_menu.rs
//!
//! @version 1.0.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Main menu, persistent settings, save slots, and launch entry points
//!
//! @description
//! Provides the initial game menu and a settings panel for fullscreen and tutorial feedback.
//! Save-slot actions use the app save directory and load bit-exact worlds through
//! `PendingFlightLoad` before entering the flight state.
//!
//! @since 22/11/2026
//! @updated 22/11/2026

// ---------- IMPORTS

use crate::save_io::{load_quicksave, load_quicksave_slot, load_settings, save_settings};
use crate::state::{AppSettings, GameMode, PendingFlightLoad};
use crate::ui_kit::{UI_ACCENT, UI_DIM, UI_TEXT, text_button};
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use bevy::window::{MonitorSelection, WindowMode};

// ---------- TYPES

#[derive(Component)]
struct MainMenuEntity;

#[derive(Component)]
struct MenuStatusText;

pub struct MainMenuPlugin;

// ---------- DECLARATIONS / IMPLEMENTATION

// The complete menu layout is intentionally kept in one setup function so all primary and
// settings controls remain visually and behaviorally adjacent.
#[allow(clippy::too_many_lines)]
fn setup_main_menu(
    mut commands: Commands<'_, '_>,
    mut settings: ResMut<'_, AppSettings>,
    mut windows: Query<'_, '_, &mut Window>,
) {
    if let Ok((fullscreen, tutorial)) = load_settings() {
        settings.borderless_fullscreen = fullscreen;
        settings.show_tutorial = tutorial;
    }
    if let Ok(mut window) = windows.single_mut()
        && settings.borderless_fullscreen
    {
        window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
    }

    commands
        .spawn((
            MainMenuEntity,
            Node {
                width: percent(100.0),
                height: percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|root| {
            root.spawn((
                MainMenuEntity,
                Node {
                    width: px(520.0),
                    padding: UiRect::all(px(24.0)),
                    row_gap: px(14.0),
                    flex_direction: FlexDirection::Column,
                    border: UiRect::all(px(1.5)),
                    border_radius: BorderRadius::all(CornerRadius::circular(px(10.0))),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.02, 0.05, 0.10, 0.97)),
            ))
            .with_children(|card| {
                card.spawn((
                    Text::new("KEPLER YARD"),
                    TextFont::from_font_size(28.0),
                    TextColor(UI_ACCENT),
                ));
                card.spawn((
                    Text::new("Earth-Moon flight laboratory"),
                    TextFont::from_font_size(13.0),
                    TextColor(UI_DIM),
                ));

                card.spawn((
                    Text::new("FLIGHT"),
                    TextFont::from_font_size(11.0),
                    TextColor(UI_ACCENT),
                ));
                let mut launch = text_button(
                    card,
                    "Launch Vehicle Assembly",
                    "Start a new craft and mission",
                    Some(230.0),
                );
                launch.observe(
                    move |_activate: On<'_, '_, Activate>,
                          mut next: ResMut<'_, NextState<GameMode>>| {
                        next.set(GameMode::Editor);
                    },
                );
                let mut continue_btn = text_button(
                    card,
                    "Continue Quicksave",
                    "Load the current quicksave",
                    Some(230.0),
                );
                continue_btn.observe(
                    move |_activate: On<'_, '_, Activate>,
                          mut commands: Commands<'_, '_>,
                          mut next: ResMut<'_, NextState<GameMode>>,
                          mut status: Query<'_, '_, &mut Text, With<MenuStatusText>>| {
                        match load_quicksave() {
                            Ok((world, maneuver)) => {
                                commands.insert_resource(PendingFlightLoad { world, maneuver });
                                next.set(GameMode::Flight);
                            }
                            Err(err) => {
                                if let Ok(mut text) = status.single_mut() {
                                    *text = Text::new(err);
                                }
                            }
                        }
                    },
                );

                card.spawn((
                    Text::new("SAVE SLOTS"),
                    TextFont::from_font_size(11.0),
                    TextColor(UI_ACCENT),
                ));
                for slot in 1..=3 {
                    let mut button = text_button(
                        card,
                        &format!("Load Slot {slot}"),
                        &format!("Load save slot {slot}"),
                        Some(230.0),
                    );
                    button.observe(
                        move |_activate: On<'_, '_, Activate>,
                              mut commands: Commands<'_, '_>,
                              mut next: ResMut<'_, NextState<GameMode>>,
                              mut status: Query<'_, '_, &mut Text, With<MenuStatusText>>| {
                            match load_quicksave_slot(slot) {
                                Ok((world, maneuver)) => {
                                    commands.insert_resource(PendingFlightLoad { world, maneuver });
                                    next.set(GameMode::Flight);
                                }
                                Err(err) => {
                                    if let Ok(mut text) = status.single_mut() {
                                        *text = Text::new(err);
                                    }
                                }
                            }
                        },
                    );
                }

                card.spawn((
                    Text::new("SETTINGS"),
                    TextFont::from_font_size(11.0),
                    TextColor(UI_ACCENT),
                ));
                let mut fullscreen = text_button(
                    card,
                    "Toggle Borderless Fullscreen",
                    "F11 is also available in flight",
                    Some(230.0),
                );
                fullscreen.observe(
                    move |_activate: On<'_, '_, Activate>,
                          mut settings: ResMut<'_, AppSettings>,
                          mut windows: Query<'_, '_, &mut Window>| {
                        settings.borderless_fullscreen = !settings.borderless_fullscreen;
                        if let Ok(mut window) = windows.single_mut() {
                            window.mode = if settings.borderless_fullscreen {
                                WindowMode::BorderlessFullscreen(MonitorSelection::Current)
                            } else {
                                WindowMode::Windowed
                            };
                        }
                        let _ =
                            save_settings(settings.borderless_fullscreen, settings.show_tutorial);
                    },
                );
                let mut tutorial = text_button(
                    card,
                    "Toggle Mission Tutorials",
                    "Show launch and mission guidance",
                    Some(230.0),
                );
                tutorial.observe(
                    move |_activate: On<'_, '_, Activate>,
                          mut settings: ResMut<'_, AppSettings>| {
                        settings.show_tutorial = !settings.show_tutorial;
                        let _ =
                            save_settings(settings.borderless_fullscreen, settings.show_tutorial);
                    },
                );

                card.spawn((
                    MenuStatusText,
                    Text::new(""),
                    TextFont::from_font_size(11.0),
                    TextColor(UI_TEXT),
                ));
            });
        });
}

fn cleanup_main_menu(
    mut commands: Commands<'_, '_>,
    query: Query<'_, '_, Entity, With<MainMenuEntity>>,
) {
    for entity in &query {
        commands.entity(entity).despawn();
    }
}

impl Plugin for MainMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameMode::Menu), setup_main_menu)
            .add_systems(OnExit(GameMode::Menu), cleanup_main_menu);
    }
}
