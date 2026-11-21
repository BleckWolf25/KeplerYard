//! @file camera.rs
//!
//! @version 1.0.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Floating-origin 2D HDR bloom camera with logarithmic zoom across pad and orbital scales
//!
//! @description
//! Spawns the primary `Camera2d` with `Tonemapping::AcesFitted`, `DebandDither::Enabled`, and
//! HDR `Bloom::NATURAL`, handles smooth mouse-wheel/keyboard logarithmic zooming from 1m launchpad
//! framing to 10,000 km orbital map scale, and tracks floating-origin pan offsets cleanly.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use crate::state::GameMode;
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;

// ---------- CONSTANTS

const EDITOR_MIN_LOG_SCALE: f32 = -5.2;
const EDITOR_MAX_LOG_SCALE: f32 = -2.2;
const FLIGHT_MIN_LOG_SCALE: f32 = -5.4;
const FLIGHT_MAX_LOG_SCALE: f32 = 8.6;

// ---------- TYPES

#[derive(Component)]
pub struct MainCamera;

#[derive(Resource, Clone, Copy, Debug)]
pub struct CameraZoomState {
    pub log_scale: f32,
    pub editor_pan: Vec2,
    pub flight_pan_m: sim_core::DVec2,
}

pub struct FloatingCameraPlugin;

// ---------- DECLARATIONS / IMPLEMENTATION

impl Default for CameraZoomState {
    fn default() -> Self {
        Self {
            log_scale: -4.1,
            editor_pan: Vec2::ZERO,
            flight_pan_m: sim_core::DVec2::ZERO,
        }
    }
}

fn spawn_main_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        MainCamera,
        Tonemapping::AcesFitted,
        DebandDither::Enabled,
        Bloom::NATURAL,
    ));
}

fn on_enter_editor(mut zoom: ResMut<CameraZoomState>) {
    zoom.log_scale = -4.1;
    zoom.editor_pan = Vec2::ZERO;
}

fn on_enter_flight(mut zoom: ResMut<CameraZoomState>) {
    zoom.log_scale = -3.8;
    zoom.flight_pan_m = sim_core::DVec2::ZERO;
}

fn update_camera_zoom_and_pan(
    mode: Res<State<GameMode>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    mut motion: MessageReader<MouseMotion>,
    mut zoom: ResMut<CameraZoomState>,
    mut cam_q: Query<(&mut Transform, &mut Projection), With<MainCamera>>,
) {
    let mut scroll_delta = 0.0_f32;
    for ev in wheel.read() {
        scroll_delta += ev.y;
    }
    if keys.pressed(KeyCode::Equal) || keys.pressed(KeyCode::NumpadAdd) {
        scroll_delta += 0.35;
    }
    if keys.pressed(KeyCode::Minus) || keys.pressed(KeyCode::NumpadSubtract) {
        scroll_delta -= 0.35;
    }

    if scroll_delta.abs() > 1.0e-4 {
        zoom.log_scale -= scroll_delta * 0.14;
    }

    let (min_log, max_log) = match mode.get() {
        GameMode::Editor => (EDITOR_MIN_LOG_SCALE, EDITOR_MAX_LOG_SCALE),
        GameMode::Flight => (FLIGHT_MIN_LOG_SCALE, FLIGHT_MAX_LOG_SCALE),
    };
    zoom.log_scale = zoom.log_scale.clamp(min_log, max_log);

    let ortho_scale = zoom.log_scale.exp();

    let mut drag = Vec2::ZERO;
    for ev in motion.read() {
        if mouse_buttons.pressed(MouseButton::Right) || mouse_buttons.pressed(MouseButton::Middle) {
            drag += ev.delta;
        }
    }

    match mode.get() {
        GameMode::Editor => {
            zoom.editor_pan.x -= drag.x * ortho_scale;
            zoom.editor_pan.y += drag.y * ortho_scale;
            zoom.editor_pan = zoom.editor_pan.clamp(Vec2::splat(-30.0), Vec2::splat(30.0));
        }
        GameMode::Flight => {
            zoom.flight_pan_m.x -= f64::from(drag.x * ortho_scale);
            zoom.flight_pan_m.y += f64::from(drag.y * ortho_scale);
            if keys.just_pressed(KeyCode::Home) || keys.just_pressed(KeyCode::KeyC) {
                zoom.flight_pan_m = sim_core::DVec2::ZERO;
            }
        }
    }

    for (mut transform, mut projection) in &mut cam_q {
        if let Projection::Orthographic(ortho) = &mut *projection {
            ortho.scale = ortho_scale;
        }
        match mode.get() {
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
