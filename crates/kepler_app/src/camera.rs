//! @file camera.rs
//!
//! @version 1.2.0
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
//! @updated 21/11/2026

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
const FLIGHT_MAX_LOG_SCALE: f32 = 10.6;

// ---------- TYPES

#[derive(Component)]
pub struct MainCamera;

#[derive(Resource, Clone, Copy, Debug)]
pub struct CameraZoomState {
    pub log_scale: f32,
    pub editor_pan: Vec2,
    pub flight_pan_m: sim_core::DVec2,
}

#[derive(SystemParam)]
struct CameraInput<'w, 's> {
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
    zoom.log_scale = -3.7;
    zoom.editor_pan = Vec2::new(0.0, -3.2);
}

fn on_enter_flight(
    mut zoom: ResMut<CameraZoomState>,
    mut focus: ResMut<CameraFocusState>,
) {
    zoom.log_scale = -3.7;
    zoom.flight_pan_m = sim_core::DVec2::ZERO;
    focus.target = CameraFocusTarget::ActiveVessel;
}

fn is_pointer_over_interactive_ui(
    hover_map: &HoverMap,
    ui_nodes: &Query<Option<&Pickable>, With<Node>>,
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
    mut input: CameraInput,
    mut zoom: ResMut<CameraZoomState>,
    mut focus: ResMut<CameraFocusState>,
    mut cam_q: Query<(&mut Transform, &mut Projection), With<MainCamera>>,
) {
    let over_ui = is_pointer_over_interactive_ui(&input.hover_map, &input.ui_nodes);

    let mut scroll_delta = 0.0_f32;
    for ev in input.wheel.read() {
        if !over_ui {
            let normalized = match ev.unit {
                MouseScrollUnit::Line => ev.y,
                MouseScrollUnit::Pixel => ev.y / 28.0,
            };
            scroll_delta += normalized;
        }
    }
    if input.keys.pressed(KeyCode::Equal) || input.keys.pressed(KeyCode::NumpadAdd) {
        scroll_delta += 0.35;
    }
    if input.keys.pressed(KeyCode::Minus) || input.keys.pressed(KeyCode::NumpadSubtract) {
        scroll_delta -= 0.35;
    }

    scroll_delta = scroll_delta.clamp(-2.5, 2.5);
    if scroll_delta.abs() > 1.0e-4 {
        zoom.log_scale -= scroll_delta * 0.14;
    }

    let (min_log, max_log) = match input.mode.get() {
        GameMode::Editor => (EDITOR_MIN_LOG_SCALE, EDITOR_MAX_LOG_SCALE),
        GameMode::Flight => (FLIGHT_MIN_LOG_SCALE, FLIGHT_MAX_LOG_SCALE),
    };
    zoom.log_scale = zoom.log_scale.clamp(min_log, max_log);

    let ortho_scale = zoom.log_scale.exp();

    let mut drag = Vec2::ZERO;
    for ev in input.motion.read() {
        if !over_ui
            && (input.mouse_buttons.pressed(MouseButton::Right)
                || input.mouse_buttons.pressed(MouseButton::Middle))
        {
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
                zoom.flight_pan_m = sim_core::DVec2::ZERO;
                focus.target = CameraFocusTarget::ActiveVessel;
            }
            if input.keys.just_pressed(KeyCode::Tab) {
                zoom.flight_pan_m = sim_core::DVec2::ZERO;
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
