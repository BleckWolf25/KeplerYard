//! @file camera.rs
//!
//! @version 1.3.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Floating-origin 2D HDR bloom camera with UI hover blocking and normalized trackpad zoom
//!
//! @description
//! Spawns the primary `Camera2d` with `Tonemapping::AcesFitted`, `DebandDither::Enabled`, and
//! HDR `Bloom::NATURAL`. Normalizes `MouseScrollUnit::Line` vs `MouseScrollUnit::Pixel`, guards
//! zooming/panning when hovering interactive UI panels, and supports `Tab` focus cycling in flight.
//!
//! @since 21/11/2026
//! @updated 22/11/2026
// The project header uses bare author metadata rather than doc-markdown code formatting.
#![allow(clippy::doc_markdown)]

// ---------- IMPORTS

use crate::state::{CameraFocusState, CameraFocusTarget, GameMode};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::ecs::system::SystemParam;
use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::picking::Pickable;
use bevy::picking::hover::HoverMap;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;

// ---------- CONSTANTS

const EDITOR_MIN_LOG_SCALE: f32 = -5.6;
const EDITOR_MAX_LOG_SCALE: f32 = -1.1;
const FLIGHT_MIN_LOG_SCALE: f32 = -5.4;
const FLIGHT_MAX_LOG_SCALE: f32 = 19.8;

// ---------- TYPES

#[derive(Component)]
pub struct MainCamera;

#[derive(Resource, Clone, Copy, Debug)]
pub struct CameraZoomState {
    pub log_scale: f32,
    pub editor_pan: Vec2,
    pub flight_pan_m: kepler_core::DVec2,
}

#[derive(SystemParam)]
struct CameraInput<'w, 's> {
    windows: Query<'w, 's, &'static Window>,
    mode: Res<'w, State<GameMode>>,
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse_buttons: Res<'w, ButtonInput<MouseButton>>,
    wheel: MessageReader<'w, 's, MouseWheel>,
    motion: MessageReader<'w, 's, MouseMotion>,
    hover_map: Res<'w, HoverMap>,
    ui_nodes: Query<'w, 's, Option<&'static Pickable>, With<Node>>,
}

pub struct FloatingCameraPlugin;

// ---------- DECLARATIONS / IMPLEMENTATION

impl Default for CameraZoomState {
    fn default() -> Self {
        Self {
            log_scale: -3.7,
            editor_pan: Vec2::new(0.0, -3.0),
            flight_pan_m: kepler_core::DVec2::ZERO,
        }
    }
}

fn spawn_main_camera(mut commands: Commands<'_, '_>) {
    commands.spawn((
        Camera2d,
        MainCamera,
        Tonemapping::AcesFitted,
        DebandDither::Enabled,
        Bloom::NATURAL,
    ));
}

fn on_enter_editor(mut zoom: ResMut<'_, CameraZoomState>) {
    zoom.log_scale = -3.7;
    zoom.editor_pan = Vec2::new(0.0, -3.2);
}

fn on_enter_flight(mut zoom: ResMut<'_, CameraZoomState>, mut focus: ResMut<'_, CameraFocusState>) {
    zoom.log_scale = -3.7;
    zoom.flight_pan_m = kepler_core::DVec2::ZERO;
    focus.target = CameraFocusTarget::ActiveVessel;
}

fn read_scroll_delta(input: &mut CameraInput<'_, '_>, over_ui: bool) -> (f32, bool) {
    let mut scroll_delta = 0.0_f32;
    let mut wheel_used = false;
    for ev in input.wheel.read() {
        if !over_ui {
            let normalized = match ev.unit {
                MouseScrollUnit::Line => ev.y,
                MouseScrollUnit::Pixel => ev.y / 28.0,
            };
            scroll_delta += normalized;
            wheel_used = true;
        }
    }
    if input.keys.pressed(KeyCode::Equal) || input.keys.pressed(KeyCode::NumpadAdd) {
        scroll_delta += 0.35;
    }
    if input.keys.pressed(KeyCode::Minus) || input.keys.pressed(KeyCode::NumpadSubtract) {
        scroll_delta -= 0.35;
    }
    (scroll_delta.clamp(-2.5, 2.5), wheel_used)
}

pub fn is_pointer_over_interactive_ui(
    hover_map: &HoverMap,
    ui_nodes: &Query<'_, '_, Option<&Pickable>, With<Node>>,
) -> bool {
    hover_map.values().any(|hits| {
        hits.keys().any(|&entity| {
            ui_nodes
                .get(entity)
                .is_ok_and(|pick| pick.is_none_or(|p| p.is_hoverable))
        })
    })
}

fn update_camera_zoom_and_pan(
    mut input: CameraInput<'_, '_>,
    mut zoom: ResMut<'_, CameraZoomState>,
    mut focus: ResMut<'_, CameraFocusState>,
    mut cam_q: Query<'_, '_, (&mut Transform, &mut Projection), With<MainCamera>>,
    mut left_pan_armed: Local<'_, bool>,
) {
    let over_ui = is_pointer_over_interactive_ui(&input.hover_map, &input.ui_nodes);

    let (scroll_delta, wheel_used) = read_scroll_delta(&mut input, over_ui);
    let old_scale = zoom.log_scale.exp();
    if scroll_delta.abs() > 1.0e-4 {
        zoom.log_scale -= scroll_delta * 0.14;
    }

    let (min_log, max_log) = match input.mode.get() {
        GameMode::Editor => (EDITOR_MIN_LOG_SCALE, EDITOR_MAX_LOG_SCALE),
        GameMode::Flight => (FLIGHT_MIN_LOG_SCALE, FLIGHT_MAX_LOG_SCALE),
    };
    zoom.log_scale = zoom.log_scale.clamp(min_log, max_log);

    let ortho_scale = zoom.log_scale.exp();

    // Zoom toward the mouse cursor: keep the world point under the pointer fixed on screen.
    if wheel_used
        && !over_ui
        && let Ok(window) = input.windows.single()
        && let Some(cursor) = window.cursor_position()
    {
        let offset_px = Vec2::new(
            cursor.x - window.width() * 0.5,
            window.height() * 0.5 - cursor.y,
        );
        let shift = offset_px * (old_scale - ortho_scale);
        match input.mode.get() {
            GameMode::Editor => {
                zoom.editor_pan += shift;
                zoom.editor_pan = zoom.editor_pan.clamp(Vec2::splat(-60.0), Vec2::splat(60.0));
            }
            GameMode::Flight => {
                zoom.flight_pan_m.x += f64::from(shift.x);
                zoom.flight_pan_m.y += f64::from(shift.y);
            }
        }
    }

    // Right/middle drag pans everywhere; in flight the left button also drags the view
    // (the editor reserves left-drag for parts and uses its own empty-space panning).
    if input.mouse_buttons.just_pressed(MouseButton::Left) {
        *left_pan_armed = !over_ui;
    }
    let flight_left_pan = matches!(input.mode.get(), GameMode::Flight)
        && *left_pan_armed
        && input.mouse_buttons.pressed(MouseButton::Left);
    let right_or_middle = !over_ui
        && (input.mouse_buttons.pressed(MouseButton::Right)
            || input.mouse_buttons.pressed(MouseButton::Middle));
    let mut drag = Vec2::ZERO;
    for ev in input.motion.read() {
        if right_or_middle || flight_left_pan {
            drag += ev.delta;
        }
    }

    match input.mode.get() {
        GameMode::Editor => {
            zoom.editor_pan.x -= drag.x * ortho_scale;
            zoom.editor_pan.y += drag.y * ortho_scale;
            zoom.editor_pan = zoom.editor_pan.clamp(Vec2::splat(-60.0), Vec2::splat(60.0));
        }
        GameMode::Flight => {
            zoom.flight_pan_m.x -= f64::from(drag.x * ortho_scale);
            zoom.flight_pan_m.y += f64::from(drag.y * ortho_scale);
            if input.keys.just_pressed(KeyCode::Home) || input.keys.just_pressed(KeyCode::KeyC) {
                zoom.flight_pan_m = kepler_core::DVec2::ZERO;
                focus.target = CameraFocusTarget::ActiveVessel;
            }
            if input.keys.just_pressed(KeyCode::Tab) {
                zoom.flight_pan_m = kepler_core::DVec2::ZERO;
                focus.target = match focus.target {
                    CameraFocusTarget::ActiveVessel => CameraFocusTarget::Body(0),
                    CameraFocusTarget::Body(0) => CameraFocusTarget::Body(1),
                    CameraFocusTarget::Body(_) => CameraFocusTarget::ActiveVessel,
                };
            }
        }
    }

    for (mut transform, mut projection) in &mut cam_q {
        if let Projection::Orthographic(ortho) = &mut *projection {
            ortho.scale = ortho_scale;
        }
        match input.mode.get() {
            GameMode::Editor => {
                transform.translation.x = zoom.editor_pan.x;
                transform.translation.y = zoom.editor_pan.y;
            }
            GameMode::Flight => {
                transform.translation.x = 0.0;
                transform.translation.y = 0.0;
            }
        }
    }
}

impl Plugin for FloatingCameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraZoomState>()
            .add_systems(Startup, spawn_main_camera)
            .add_systems(OnEnter(GameMode::Editor), on_enter_editor)
            .add_systems(OnEnter(GameMode::Flight), on_enter_flight)
            .add_systems(Update, update_camera_zoom_and_pan);
    }
}

// ---------- EXPORTS
