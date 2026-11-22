//! @file ui_kit.rs
//!
//! @version 1.0.0
//! @author `BleckWolf25`
//! @license MIT
//!
//! @summary Shared UI building blocks: consistent buttons, panels, and mouse-hover tooltips
//!
//! @description
//! Gives the VAB editor and the flight HUD one visual language. Provides a themed button bundle,
//! a text-button helper, and a global tooltip that appears after the pointer rests on any entity
//! carrying a `Tooltip` component, so every control can explain itself (and show its hotkey)
//! without cluttering the screen.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use crate::state::uniform_border;
use bevy::picking::Pickable;
use bevy::picking::hover::{HoverMap, Hovered};
use bevy::prelude::*;
use bevy::ui_widgets::Button;

// ---------- CONSTANTS

pub const UI_TEXT: Color = Color::srgb(0.88, 0.96, 1.0);
pub const UI_ACCENT: Color = Color::srgb(0.32, 0.88, 1.0);
pub const UI_OK: Color = Color::srgb(0.35, 0.95, 0.72);
#[allow(dead_code)]
pub const UI_WARN: Color = Color::srgb(0.95, 0.80, 0.25);
pub const UI_DIM: Color = Color::srgba(0.68, 0.82, 0.92, 0.85);

pub const BTN_BG: Color = Color::srgba(0.08, 0.14, 0.22, 0.92);
pub const BTN_BORDER: Color = Color::srgba(0.24, 0.52, 0.72, 0.5);
pub const BORDER_ON: Color = Color::srgba(0.28, 0.96, 0.62, 0.96);
#[allow(dead_code)]
pub const BORDER_SELECTED: Color = Color::srgba(0.28, 0.92, 1.0, 0.96);

const TOOLTIP_DELAY_S: f32 = 0.35;
const TOOLTIP_WIDTH_PX: f32 = 300.0;

// ---------- TYPES

/// Attach to any hoverable UI entity to give it a hover tooltip.
#[derive(Component, Clone, Debug)]
pub struct Tooltip(pub String);

#[derive(Component)]
struct TooltipRoot;

#[derive(Component)]
struct TooltipLabel;

pub struct UiKitPlugin;

// ---------- DECLARATIONS / IMPLEMENTATION

/// Themed button container. Add children (icon / label) and an `Activate` observer to it.
pub fn button_bundle(width: Option<f32>, tooltip: &str) -> impl Bundle {
    (
        Button,
        Hovered::default(),
        Tooltip(tooltip.to_owned()),
        Node {
            width: width.map_or(Val::Auto, px),
            padding: UiRect::axes(px(7.0), px(4.0)),
            column_gap: px(5.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(CornerRadius::circular(px(4.0))),
            ..default()
        },
        BackgroundColor(BTN_BG),
        uniform_border(BTN_BORDER),
    )
}

/// Spawns a themed button with a single text label and returns its commands so the caller can
/// attach an observer.
pub fn text_button<'a>(
    parent: &'a mut ChildSpawnerCommands<'_>,
    label: &str,
    tooltip: &str,
    width: Option<f32>,
) -> EntityCommands<'a> {
    let mut cmd = parent.spawn(button_bundle(width, tooltip));
    cmd.with_children(|b| {
        b.spawn((
            Text::new(label),
            TextFont::from_font_size(11.0),
            TextColor(UI_TEXT),
            Pickable::IGNORE,
        ));
    });
    cmd
}

/// A translucent themed panel node (no children).
pub fn panel_bundle() -> impl Bundle {
    (
        Node {
            padding: UiRect::all(px(8.0)),
            row_gap: px(6.0),
            column_gap: px(6.0),
            align_items: AlignItems::Center,
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(CornerRadius::circular(px(6.0))),
            ..default()
        },
        BackgroundColor(Color::srgba(0.04, 0.07, 0.12, 0.92)),
        uniform_border(Color::srgba(0.18, 0.65, 0.88, 0.55)),
    )
}

/// Thin vertical divider used between button groups.
pub fn divider_bundle() -> impl Bundle {
    (
        Pickable::IGNORE,
        Node {
            width: px(1.0),
            height: px(20.0),
            margin: UiRect::horizontal(px(3.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.30, 0.55, 0.75, 0.45)),
    )
}

fn spawn_tooltip_root(mut commands: Commands<'_, '_>) {
    commands
        .spawn((
            TooltipRoot,
            Pickable::IGNORE,
            GlobalZIndex(1000),
            Node {
                position_type: PositionType::Absolute,
                display: Display::None,
                max_width: px(TOOLTIP_WIDTH_PX),
                padding: UiRect::axes(px(8.0), px(6.0)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(CornerRadius::circular(px(4.0))),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.04, 0.08, 0.97)),
            uniform_border(Color::srgba(0.32, 0.80, 0.98, 0.75)),
        ))
        .with_children(|root| {
            root.spawn((
                TooltipLabel,
                Pickable::IGNORE,
                Text::new(""),
                TextFont::from_font_size(11.0),
                TextColor(UI_TEXT),
            ));
        });
}

#[allow(clippy::too_many_arguments)]
fn update_tooltip(
    time: Res<'_, Time>,
    hover_map: Res<'_, HoverMap>,
    tips: Query<'_, '_, &Tooltip>,
    windows: Query<'_, '_, &Window>,
    mut root_q: Query<'_, '_, &mut Node, With<TooltipRoot>>,
    mut label_q: Query<'_, '_, &mut Text, With<TooltipLabel>>,
    mut dwell: Local<'_, (Option<Entity>, f32)>,
) {
    let Ok(mut node) = root_q.single_mut() else {
        return;
    };
    let hovered = hover_map
        .values()
        .flat_map(|hits| hits.keys())
        .copied()
        .find(|e| tips.contains(*e));

    if hovered == dwell.0 {
        dwell.1 += time.delta_secs();
    } else {
        *dwell = (hovered, 0.0);
    }

    let (Some(entity), true) = (dwell.0, dwell.1 >= TOOLTIP_DELAY_S) else {
        node.display = Display::None;
        return;
    };
    let (Ok(tip), Ok(window)) = (tips.get(entity), windows.single()) else {
        node.display = Display::None;
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        node.display = Display::None;
        return;
    };

    if let Ok(mut label) = label_q.single_mut()
        && label.0 != tip.0
    {
        label.0.clone_from(&tip.0);
    }

    let x = (cursor.x + 16.0).min(window.width() - TOOLTIP_WIDTH_PX - 8.0).max(4.0);
    let y = if cursor.y > window.height() - 140.0 {
        cursor.y - 70.0
    } else {
        cursor.y + 22.0
    };
    node.left = px(x);
    node.top = px(y);
    node.display = Display::Flex;
}

impl Plugin for UiKitPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_tooltip_root)
            .add_systems(Update, update_tooltip);
    }
}

// ---------- EXPORTS
