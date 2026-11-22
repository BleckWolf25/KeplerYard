//! @file flight_hud.rs
//!
//! @version 1.2.0
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Flight HUD, 2D Navball attitude ring, SAS mode selector, map badges, and pause/recover UI
//!
//! @description
//! Renders the ASCII-safe interactive flight telemetry panel, clickable SAS mode buttons, RCS,
//! vessel/focus controls, screen-projected map badges, 2D Navball attitude ring gizmo, and the
//! Escape pause, revert-to-launch, revert-to-VAB, and vessel recovery summary modal.
//!
//! @since 21/11/2026
//! @updated 22/11/2026
// The project header uses bare author metadata rather than doc-markdown code formatting.
#![allow(clippy::doc_markdown)]

// ---------- IMPORTS

use crate::camera::CameraZoomState;
use crate::flight::{FlightEntity, FlightSimState};
use crate::state::{
    CameraFocusState, CameraFocusTarget, FlightClock, FloatingOrigin, GameMode, uniform_border,
    world_to_render_vec2,
};
use crate::ui_kit::{BTN_BORDER, Tooltip};
use bevy::ecs::system::SystemParam;
use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::{
    Activate, Button, Slider, SliderOrientation, SliderRange, SliderThumb, SliderValue, ValueChange,
};
use kepler_core::{ApsisKind, DVec2, SasMode, body::dominant_body, compute_hohmann_transfer};

// ---------- CONSTANTS

const BADGE_SLOTS: usize = 7;

// ---------- TYPES

#[derive(Component)]
pub struct FlightHudText;

#[derive(Component)]
pub struct FlightNavText;

#[derive(Component)]
pub struct FlightControlsText;

#[derive(Component)]
pub struct SasModeButton(pub SasMode);

#[derive(Component)]
pub struct SasToggleButton;

#[derive(Component)]
pub struct RcsToggleButton;

#[derive(Component)]
pub struct FlightPauseModal;

#[derive(Component)]
pub struct PauseSummaryText;

#[derive(Component)]
pub struct RecoverButtonMarker;

#[derive(Component)]
pub struct FlightMapBadge(pub usize);

#[derive(Component)]
pub struct ThrottleThumb;

#[derive(Component)]
pub struct ThrottlePercentText;

#[derive(Clone, Copy)]
pub enum FlightUiAction {
    TriggerStage,
    ToggleSas,
    SetSasMode(SasMode),
    ToggleRcs,
    PlanMoon,
    ClearManeuver,
    CycleVessel,
    CycleFocus,
    ToggleControlsHelp,
    Resume,
    RevertToLaunchpad,
    ReturnToVab,
    RecoverVessel,
}

type PauseSummaryTextQuery<'w, 's> = Query<
    'w,
    's,
    &'static mut Text,
    (
        With<PauseSummaryText>,
        Without<FlightHudText>,
        Without<FlightNavText>,
    ),
>;

type RecoverButtonQuery<'w, 's> = Query<
    'w,
    's,
    &'static mut Node,
    (
        With<RecoverButtonMarker>,
        Without<FlightPauseModal>,
        Without<FlightControlsText>,
    ),
>;

type RcsToggleBorderQuery<'w, 's> = Query<
    'w,
    's,
    &'static mut BorderColor,
    (
        With<RcsToggleButton>,
        Without<SasModeButton>,
        Without<SasToggleButton>,
    ),
>;

#[derive(SystemParam)]
#[allow(clippy::struct_field_names)]
pub struct HudSyncQueries<'w, 's> {
    pub hud_q: Query<'w, 's, &'static mut Text, With<FlightHudText>>,
    pub nav_q: Query<'w, 's, &'static mut Text, (With<FlightNavText>, Without<FlightHudText>)>,
    pub help_node_q: Query<'w, 's, &'static mut Node, With<FlightControlsText>>,
    pub modal_node_q:
        Query<'w, 's, &'static mut Node, (With<FlightPauseModal>, Without<FlightControlsText>)>,
    pub modal_text_q: PauseSummaryTextQuery<'w, 's>,
    pub recover_btn_q: RecoverButtonQuery<'w, 's>,
    pub sas_mode_q: Query<'w, 's, (&'static SasModeButton, &'static mut BorderColor)>,
    pub sas_toggle_q:
        Query<'w, 's, &'static mut BorderColor, (With<SasToggleButton>, Without<SasModeButton>)>,
    pub rcs_toggle_q: RcsToggleBorderQuery<'w, 's>,
}

type FlightBadgeQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static FlightMapBadge,
        &'static mut Node,
        &'static mut Text,
        &'static mut TextColor,
    ),
    (
        Without<FlightHudText>,
        Without<FlightNavText>,
        Without<PauseSummaryText>,
        Without<FlightPauseModal>,
        Without<FlightControlsText>,
        Without<RecoverButtonMarker>,
    ),
>;

type FlightButtonStyleQuery<'w, 's> = Query<
    'w,
    's,
    (&'static Hovered, Has<Pressed>, &'static mut BackgroundColor),
    (Or<(Changed<Hovered>, Added<Pressed>)>, With<Button>),
>;

// ---------- DECLARATIONS / IMPLEMENTATION

#[allow(clippy::too_many_lines)]
pub fn spawn_flight_hud_ui(commands: &mut Commands<'_, '_>) {
    let panel_bg = BackgroundColor(Color::srgba(0.03, 0.06, 0.11, 0.88));
    let border = uniform_border(Color::srgba(0.20, 0.72, 0.94, 0.55));
    let text_col = TextColor(Color::srgb(0.88, 0.96, 1.0));

    commands
        .spawn((
            FlightEntity,
            Node {
                position_type: PositionType::Absolute,
                left: px(12.0),
                top: px(12.0),
                width: px(396.0),
                padding: UiRect::all(px(11.0)),
                row_gap: px(7.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(CornerRadius::circular(px(6.0))),
                ..default()
            },
            panel_bg,
            border,
        ))
        .with_children(|parent| {
            parent.spawn((
                FlightHudText,
                Text::new("Initializing flight telemetry..."),
                TextFont::from_font_size(11.5),
                text_col,
            ));

            parent
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(4.0),
                    row_gap: px(4.0),
                    ..default()
                })
                .with_children(|bar| {
                    spawn_hud_btn(
                        bar,
                        "STAGE [Space]",
                        "Fire the next stage: ignite its engines, separate, or deploy its chute",
                        FlightUiAction::TriggerStage,
                        None,
                        None,
                        None,
                    );
                    spawn_hud_btn(
                        bar,
                        "SAS [T]",
                        "Stability Assist: holds the ship's current orientation",
                        FlightUiAction::ToggleSas,
                        Some(SasToggleButton),
                        None,
                        None,
                    );
                    for mode in [
                        SasMode::Stability,
                        SasMode::Prograde,
                        SasMode::Retrograde,
                        SasMode::RadialOut,
                        SasMode::Maneuver,
                    ] {
                        spawn_hud_btn(
                            bar,
                            mode.label(),
                            "SAS hold mode - click to switch and enable SAS",
                            FlightUiAction::SetSasMode(mode),
                            None,
                            Some(SasModeButton(mode)),
                            None,
                        );
                    }
                    spawn_hud_btn(
                        bar,
                        "RCS [R]",
                        "Toggle RCS thrusters for fine translation (H/N fore-aft, Q/E lateral)",
                        FlightUiAction::ToggleRcs,
                        None,
                        None,
                        Some(RcsToggleButton),
                    );
                });

            spawn_throttle_slider(parent);

            parent
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(4.0),
                    row_gap: px(4.0),
                    ..default()
                })
                .with_children(|bar| {
                    spawn_hud_btn(
                        bar,
                        "Plan TLI [M]",
                        "Plan a Hohmann transfer burn to the Moon",
                        FlightUiAction::PlanMoon,
                        None,
                        None,
                        None,
                    );
                    spawn_hud_btn(
                        bar,
                        "Clear [V]",
                        "Clear the planned maneuver node",
                        FlightUiAction::ClearManeuver,
                        None,
                        None,
                        None,
                    );
                    spawn_hud_btn(
                        bar,
                        "Vessel [ ] ]",
                        "Cycle control to another vessel in flight",
                        FlightUiAction::CycleVessel,
                        None,
                        None,
                        None,
                    );
                    spawn_hud_btn(
                        bar,
                        "Focus [Tab]",
                        "Cycle the camera's focus between your vessel and nearby bodies",
                        FlightUiAction::CycleFocus,
                        None,
                        None,
                        None,
                    );
                    spawn_hud_btn(
                        bar,
                        "Keys [?]",
                        "Show or hide the full keyboard shortcut list",
                        FlightUiAction::ToggleControlsHelp,
                        None,
                        None,
                        None,
                    );
                });

            parent.spawn((
                FlightControlsText,
                Node {
                    display: Display::None,
                    ..default()
                },
                Text::new(
                    "[Space] Stage  [G] Arm Chute  [Shift/Ctrl/Z/X] Throttle\n\
                     [A/D] Steer  [T] SAS  [B] SAS Mode  [R] RCS  [H/N/Q/E] RCS Trans\n\
                     [M] Plan TLI  [U/J] Pro dV  [O/L] Rad dV  [I/K] Lead  [V] Clear\n\
                     [./,] Warp  [P] Pause  [[]/[]] Vessel  [Tab] Focus  [F5/F9] Save/Load\n\
                     [Esc] Flight Menu",
                ),
                TextFont::from_font_size(10.5),
                TextColor(Color::srgba(0.70, 0.86, 0.96, 0.90)),
            ));
        });

    commands
        .spawn((
            FlightEntity,
            Pickable::IGNORE,
            Node {
                position_type: PositionType::Absolute,
                right: px(16.0),
                bottom: px(158.0),
                padding: UiRect::axes(px(9.0), px(6.0)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(CornerRadius::circular(px(5.0))),
                ..default()
            },
            panel_bg,
            border,
        ))
        .with_children(|p| {
            p.spawn((
                FlightNavText,
                Pickable::IGNORE,
                Text::new("Pitch: +90.0 deg"),
                TextFont::from_font_size(11.0),
                text_col,
            ));
        });

    for i in 0..BADGE_SLOTS {
        commands.spawn((
            FlightEntity,
            FlightMapBadge(i),
            Pickable::IGNORE,
            Node {
                position_type: PositionType::Absolute,
                display: Display::None,
                left: px(0.0),
                top: px(0.0),
                ..default()
            },
            Text::new(""),
            TextFont::from_font_size(11.0),
            TextColor(Color::WHITE),
        ));
    }

    commands
        .spawn((
            FlightEntity,
            FlightPauseModal,
            Node {
                position_type: PositionType::Absolute,
                left: percent(0.0),
                top: percent(0.0),
                width: percent(100.0),
                height: percent(100.0),
                display: Display::None,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.01, 0.02, 0.05, 0.72)),
        ))
        .with_children(|backdrop| {
            backdrop
                .spawn((
                    Node {
                        width: px(410.0),
                        padding: UiRect::all(px(18.0)),
                        row_gap: px(10.0),
                        flex_direction: FlexDirection::Column,
                        border: UiRect::all(px(1.5)),
                        border_radius: BorderRadius::all(CornerRadius::circular(px(8.0))),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.04, 0.08, 0.14, 0.97)),
                    uniform_border(Color::srgba(0.28, 0.88, 1.0, 0.85)),
                ))
                .with_children(|card| {
                    card.spawn((
                        Text::new("MISSION CONTROL"),
                        TextFont::from_font_size(16.0),
                        TextColor(Color::srgb(0.32, 0.90, 1.0)),
                    ));
                    card.spawn((
                        PauseSummaryText,
                        Text::new(""),
                        TextFont::from_font_size(12.0),
                        text_col,
                    ));

                    spawn_modal_btn(card, "Resume Flight [Esc]", FlightUiAction::Resume, false);
                    spawn_modal_btn(
                        card,
                        "Revert to Launchpad",
                        FlightUiAction::RevertToLaunchpad,
                        false,
                    );
                    spawn_modal_btn(
                        card,
                        "Return to VAB Editor",
                        FlightUiAction::ReturnToVab,
                        false,
                    );
                    spawn_modal_btn(
                        card,
                        "Recover Vessel & End Mission",
                        FlightUiAction::RecoverVessel,
                        true,
                    );
                });
        });
}

fn spawn_hud_btn(
    bar: &mut ChildSpawnerCommands<'_>,
    caption: &str,
    tooltip: &str,
    action: FlightUiAction,
    sas_toggle: Option<SasToggleButton>,
    sas_mode: Option<SasModeButton>,
    rcs_toggle: Option<RcsToggleButton>,
) {
    let mut cmd = bar.spawn((
        Button,
        Hovered::default(),
        Tooltip(tooltip.to_owned()),
        Node {
            padding: UiRect::axes(px(6.0), px(3.0)),
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(CornerRadius::circular(px(3.0))),
            ..default()
        },
        BackgroundColor(Color::srgba(0.08, 0.14, 0.22, 0.92)),
        uniform_border(Color::srgba(0.24, 0.52, 0.72, 0.5)),
    ));
    if let Some(m) = sas_toggle {
        cmd.insert(m);
    }
    if let Some(m) = sas_mode {
        cmd.insert(m);
    }
    if let Some(m) = rcs_toggle {
        cmd.insert(m);
    }
    cmd.observe(
        move |_act: On<'_, '_, Activate>,
              mut flight: ResMut<'_, FlightSimState>,
              mut clock: ResMut<'_, FlightClock>,
              mut focus: ResMut<'_, CameraFocusState>,
              mut zoom: ResMut<'_, CameraZoomState>,
              mut next_mode: ResMut<'_, NextState<GameMode>>| {
            apply_flight_ui_action(
                action,
                &mut flight,
                &mut clock,
                &mut focus,
                &mut zoom,
                &mut next_mode,
            );
        },
    )
    .with_children(|b| {
        b.spawn((
            Text::new(caption),
            TextFont::from_font_size(10.5),
            TextColor(Color::srgb(0.88, 0.96, 1.0)),
            Pickable::IGNORE,
        ));
    });
}

const THROTTLE_TRACK_W: f32 = 150.0;
const THROTTLE_THUMB_W: f32 = 16.0;

/// A vertical click-and-drag throttle control, so the mouse can set thrust directly instead of
/// only nudging it with Shift/Ctrl. Keyboard throttle control keeps working side by side.
fn spawn_throttle_slider(parent: &mut ChildSpawnerCommands<'_>) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8.0),
            margin: UiRect::top(px(2.0)),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new("Throttle"),
                TextFont::from_font_size(10.5),
                TextColor(Color::srgb(0.88, 0.96, 1.0)),
                Pickable::IGNORE,
            ));
            row.spawn((
                Slider {
                    orientation: SliderOrientation::Horizontal,
                    ..default()
                },
                SliderRange::new(0.0, 100.0),
                SliderValue(100.0),
                Tooltip(String::from(
                    "Drag to set throttle directly\n[Shift/Up] More  [Ctrl/Down] Less  [Z] Full  [X] Cut",
                )),
                Node {
                    position_type: PositionType::Relative,
                    width: px(THROTTLE_TRACK_W),
                    height: px(18.0),
                    padding: UiRect::all(px(2.0)),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(CornerRadius::circular(px(9.0))),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.08, 0.14, 0.22, 0.92)),
                uniform_border(BTN_BORDER),
            ))
            .with_children(|track| {
                track.spawn((
                    ThrottleThumb,
                    SliderThumb,
                    Pickable::IGNORE,
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0.0),
                        top: px(1.0),
                        width: px(THROTTLE_THUMB_W),
                        height: px(14.0),
                        border_radius: BorderRadius::all(CornerRadius::circular(px(7.0))),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.30, 0.95, 0.55)),
                ));
            })
            .observe(
                move |ev: On<'_, '_, ValueChange<f32>>, mut flight: ResMut<'_, FlightSimState>| {
                    flight.throttle_override = Some((f64::from(ev.value) / 100.0).clamp(0.0, 1.0));
                },
            );
            row.spawn((
                ThrottlePercentText,
                Text::new("100%"),
                TextFont::from_font_size(10.5),
                TextColor(Color::srgb(0.88, 0.96, 1.0)),
                Pickable::IGNORE,
            ));
        });
}

/// Keeps the throttle slider's thumb position, value, and readout in sync with the sim, in
/// either direction: dragging the slider sets throttle (via `ValueChange`'s observer), and
/// keyboard throttle changes move the slider back to match.
pub fn sync_throttle_slider(
    flight: Res<'_, FlightSimState>,
    mut commands: Commands<'_, '_>,
    slider_q: Query<'_, '_, (Entity, &SliderValue), With<Slider>>,
    mut thumb_q: Query<'_, '_, &mut Node, With<ThrottleThumb>>,
    mut pct_q: Query<'_, '_, &mut Text, With<ThrottlePercentText>>,
) {
    let target = (flight.throttle * 100.0).clamp(0.0, 100.0) as f32;
    let Ok((entity, value)) = slider_q.single() else {
        return;
    };
    if (value.0 - target).abs() > 0.05 {
        commands.entity(entity).insert(SliderValue(target));
    }
    if let Ok(mut node) = thumb_q.single_mut() {
        let travel = THROTTLE_TRACK_W - THROTTLE_THUMB_W - 4.0;
        node.left = px((value.0 / 100.0).clamp(0.0, 1.0) * travel);
    }
    if let Ok(mut text) = pct_q.single_mut() {
        let new_text = format!("{:.0}%", flight.throttle * 100.0);
        if text.0 != new_text {
            text.0 = new_text;
        }
    }
}

fn spawn_modal_btn(
    card: &mut ChildSpawnerCommands<'_>,
    caption: &str,
    action: FlightUiAction,
    is_recover: bool,
) {
    let mut cmd = card.spawn((
        Button,
        Hovered::default(),
        Tooltip(caption.to_owned()),
        Node {
            padding: UiRect::axes(px(10.0), px(7.0)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(CornerRadius::circular(px(5.0))),
            ..default()
        },
        BackgroundColor(Color::srgba(0.08, 0.16, 0.26, 0.95)),
        uniform_border(Color::srgba(0.28, 0.72, 0.92, 0.7)),
    ));
    if is_recover {
        cmd.insert(RecoverButtonMarker);
    }
    cmd.observe(
        move |_act: On<'_, '_, Activate>,
              mut flight: ResMut<'_, FlightSimState>,
              mut clock: ResMut<'_, FlightClock>,
              mut focus: ResMut<'_, CameraFocusState>,
              mut zoom: ResMut<'_, CameraZoomState>,
              mut next_mode: ResMut<'_, NextState<GameMode>>| {
            apply_flight_ui_action(
                action,
                &mut flight,
                &mut clock,
                &mut focus,
                &mut zoom,
                &mut next_mode,
            );
        },
    )
    .with_children(|b| {
        b.spawn((
            Text::new(caption),
            TextFont::from_font_size(12.0),
            TextColor(Color::srgb(0.90, 0.97, 1.0)),
            Pickable::IGNORE,
        ));
    });
}

pub fn apply_flight_ui_action(
    action: FlightUiAction,
    flight: &mut FlightSimState,
    clock: &mut FlightClock,
    focus: &mut CameraFocusState,
    zoom: &mut CameraZoomState,
    next_mode: &mut NextState<GameMode>,
) {
    match action {
        FlightUiAction::TriggerStage => {
            flight.stage_requested = true;
        }
        FlightUiAction::ToggleSas => {
            flight.sas_enabled = !flight.sas_enabled;
        }
        FlightUiAction::SetSasMode(mode) => {
            flight.sas_mode = mode;
            flight.sas_enabled = true;
            flight.status_banner = format!("SAS mode: {}", mode.label());
        }
        FlightUiAction::ToggleRcs => {
            flight.rcs_enabled = !flight.rcs_enabled;
            flight.status_banner = if flight.rcs_enabled {
                String::from("RCS thrusters ARMED.")
            } else {
                String::from("RCS thrusters OFF.")
            };
        }
        FlightUiAction::PlanMoon => {
            if let Some(v) = flight.world.active_vessel()
                && let Some(est) = compute_hohmann_transfer(&flight.world.bodies, v.pos, v.vel, 1)
            {
                flight.maneuver = kepler_core::ManeuverPlan {
                    enabled: true,
                    lead_time_s: est.lead_time_until_window_s,
                    prograde_dv_m_s: est.dv1_m_s,
                    radial_dv_m_s: 0.0,
                };
                flight.status_banner = String::from("Planned Hohmann TLI maneuver to the Moon.");
            }
        }
        FlightUiAction::ClearManeuver => {
            flight.maneuver.enabled = false;
            if flight.sas_mode == SasMode::Maneuver {
                flight.sas_mode = SasMode::Stability;
            }
            flight.status_banner = String::from("Cleared maneuver plan.");
        }
        FlightUiAction::CycleVessel => {
            if let Some(new_id) = flight.world.cycle_active_vessel() {
                if let Some((ctrl, name)) = flight
                    .world
                    .active_vessel()
                    .map(|v| (v.control, v.name.clone()))
                {
                    flight.throttle = ctrl.throttle;
                    flight.sas_enabled = ctrl.sas_enabled;
                    flight.sas_mode = ctrl.sas_mode;
                    flight.rcs_enabled = ctrl.rcs_enabled;
                    flight.status_banner = format!("Switched to {name} (#{new_id})");
                }
                focus.target = CameraFocusTarget::ActiveVessel;
                zoom.flight_pan_m = DVec2::ZERO;
            }
        }
        FlightUiAction::CycleFocus => {
            zoom.flight_pan_m = DVec2::ZERO;
            focus.target = match focus.target {
                CameraFocusTarget::ActiveVessel => CameraFocusTarget::Body(0),
                CameraFocusTarget::Body(0) => CameraFocusTarget::Body(1),
                CameraFocusTarget::Body(_) => CameraFocusTarget::ActiveVessel,
            };
        }
        FlightUiAction::ToggleControlsHelp => {
            flight.show_controls_help = !flight.show_controls_help;
        }
        FlightUiAction::Resume => {
            flight.pause_menu_open = false;
            flight.recovered_summary = false;
        }
        FlightUiAction::RevertToLaunchpad => {
            flight.request_revert_pad = true;
            flight.pause_menu_open = false;
            clock.warp_index = 0;
            clock.paused = false;
            focus.target = CameraFocusTarget::ActiveVessel;
            zoom.flight_pan_m = DVec2::ZERO;
        }
        FlightUiAction::ReturnToVab => {
            next_mode.set(GameMode::Editor);
        }
        FlightUiAction::RecoverVessel => {
            flight.recovered_summary = true;
            flight.status_banner = String::from("Vessel safely recovered!");
        }
    }
}

pub fn style_flight_buttons_on_change(mut buttons: FlightButtonStyleQuery<'_, '_>) {
    for (hovered, pressed, mut bg) in &mut buttons {
        *bg = match (pressed, hovered.get()) {
            (true, _) => BackgroundColor(Color::srgba(0.16, 0.38, 0.56, 0.98)),
            (false, true) => BackgroundColor(Color::srgba(0.12, 0.24, 0.36, 0.95)),
            (false, false) => BackgroundColor(Color::srgba(0.08, 0.14, 0.22, 0.92)),
        };
    }
}

pub fn style_flight_buttons_on_release(
    mut removed_pressed: RemovedComponents<'_, '_, Pressed>,
    mut buttons: Query<'_, '_, (&Hovered, &mut BackgroundColor), With<Button>>,
) {
    for entity in removed_pressed.read() {
        if let Ok((hovered, mut bg)) = buttons.get_mut(entity) {
            *bg = if hovered.get() {
                BackgroundColor(Color::srgba(0.12, 0.24, 0.36, 0.95))
            } else {
                BackgroundColor(Color::srgba(0.08, 0.14, 0.22, 0.92))
            };
        }
    }
}

pub fn draw_navball_gizmo(
    flight: &FlightSimState,
    window: &Window,
    ortho_scale: f32,
    gizmos: &mut Gizmos<'_, '_>,
) {
    let Some(v) = flight.world.active_vessel() else {
        return;
    };
    let Some(body) = dominant_body(&flight.world.bodies, v.pos) else {
        return;
    };

    let center = Vec2::new(
        (0.5 * window.width() - 88.0).max(60.0) * ortho_scale,
        (-0.5 * window.height() + 86.0).min(-60.0) * ortho_scale,
    );
    let r = 56.0 * ortho_scale;

    gizmos.circle_2d(center, r, Color::srgba(0.28, 0.78, 0.98, 0.85));
    gizmos.circle_2d(center, r * 0.15, Color::srgba(0.5, 0.65, 0.8, 0.35));

    let up_d = (v.pos - body.pos).normalize_or_zero();
    let up_v = Vec2::new(up_d.x as f32, up_d.y as f32);
    let horizon_v = Vec2::new(-up_v.y, up_v.x);
    gizmos.line_2d(
        center - horizon_v * (r * 0.92),
        center + horizon_v * (r * 0.92),
        Color::srgba(0.45, 0.65, 0.85, 0.45),
    );
    gizmos.line_2d(
        center,
        center + up_v * (r * 0.35),
        Color::srgba(0.45, 0.65, 0.85, 0.35),
    );

    let nose_d = DVec2::Y.rotate(v.angle);
    let nose_v = Vec2::new(nose_d.x as f32, nose_d.y as f32);
    gizmos.line_2d(
        center,
        center + nose_v * (r * 0.95),
        Color::srgb(1.0, 1.0, 1.0),
    );
    gizmos.circle_2d(center + nose_v * (r * 0.95), r * 0.08, Color::WHITE);

    let rel_v = v.vel - body.vel;
    if rel_v.length_squared() > 1.0 {
        let pro_d = rel_v.normalize_or_zero();
        let pro_v = Vec2::new(pro_d.x as f32, pro_d.y as f32);
        gizmos.circle_2d(
            center + pro_v * (r * 0.82),
            r * 0.10,
            Color::srgb(0.28, 0.98, 0.56),
        );
        gizmos.circle_2d(
            center - pro_v * (r * 0.82),
            r * 0.10,
            Color::srgb(1.0, 0.62, 0.20),
        );
    }

    if let Some(moon) = flight.world.bodies.get(1) {
        let moon_dir_d = (moon.pos - v.pos).normalize_or_zero();
        let moon_v = Vec2::new(moon_dir_d.x as f32, moon_dir_d.y as f32);
        gizmos.circle_2d(
            center + moon_v * (r * 1.06),
            r * 0.07,
            Color::srgb(1.0, 0.86, 0.28),
        );
    }

    if flight.maneuver.enabled
        && let Some(primary) = flight.world.bodies.first()
    {
        let pro = (v.vel - primary.vel).normalize_or_zero();
        let rad = (v.pos - primary.pos).normalize_or_zero();
        let dv = (pro * flight.maneuver.prograde_dv_m_s + rad * flight.maneuver.radial_dv_m_s)
            .normalize_or_zero();
        if dv.length_squared() > 0.1 {
            let mv = Vec2::new(dv.x as f32, dv.y as f32);
            gizmos.line_2d(
                center + mv * (r * 0.55),
                center + mv * (r * 1.02),
                Color::srgb(0.98, 0.35, 0.92),
            );
        }
    }
}

pub fn update_flight_hud_and_badges(
    flight: &FlightSimState,
    clock: &FlightClock,
    origin: FloatingOrigin,
    cam: (&Camera, &GlobalTransform),
    window: &Window,
    mut hud: HudSyncQueries<'_, '_>,
    mut badge_q: FlightBadgeQuery<'_, '_>,
) {
    if let Ok(mut text) = hud.hud_q.single_mut() {
        *text = Text::new(format_flight_hud(flight, clock));
    }

    if let Ok(mut nav_txt) = hud.nav_q.single_mut() {
        *nav_txt = Text::new(format_navball_readout(flight));
    }

    if let Ok(mut help_node) = hud.help_node_q.single_mut() {
        help_node.display = if flight.show_controls_help {
            Display::Flex
        } else {
            Display::None
        };
    }

    let show_modal = flight.pause_menu_open
        || flight.recovered_summary
        || flight.world.active_vessel().is_some_and(|v| v.crashed);

    if let Ok(mut m_node) = hud.modal_node_q.single_mut() {
        m_node.display = if show_modal {
            Display::Flex
        } else {
            Display::None
        };
    }

    if show_modal && let Ok(mut m_txt) = hud.modal_text_q.single_mut() {
        *m_txt = Text::new(format_mission_summary(flight));
    }

    if let Ok(mut r_node) = hud.recover_btn_q.single_mut() {
        let can_recover = !flight.recovered_summary
            && flight.world.active_vessel().is_some_and(|v| {
                v.landed
                    && !v.crashed
                    && flight
                        .world
                        .bodies
                        .first()
                        .is_some_and(|p| v.pos.distance(p.pos) - p.radius_m < 1_000.0)
            });
        r_node.display = if can_recover {
            Display::Flex
        } else {
            Display::None
        };
    }

    for (btn, mut border) in &mut hud.sas_mode_q {
        let active = flight.sas_enabled && flight.sas_mode == btn.0;
        *border = if active {
            uniform_border(Color::srgba(0.28, 0.96, 0.62, 0.96))
        } else {
            uniform_border(Color::srgba(0.24, 0.52, 0.72, 0.45))
        };
    }
    if let Ok(mut b) = hud.sas_toggle_q.single_mut() {
        *b = if flight.sas_enabled {
            uniform_border(Color::srgba(0.28, 0.96, 0.62, 0.96))
        } else {
            uniform_border(Color::srgba(0.55, 0.35, 0.35, 0.60))
        };
    }
    if let Ok(mut b) = hud.rcs_toggle_q.single_mut() {
        *b = if flight.rcs_enabled {
            uniform_border(Color::srgba(0.32, 0.92, 1.0, 0.96))
        } else {
            uniform_border(Color::srgba(0.24, 0.52, 0.72, 0.45))
        };
    }

    update_floating_map_badges(flight, origin, cam, window, &mut badge_q);
}

fn update_floating_map_badges(
    flight: &FlightSimState,
    origin: FloatingOrigin,
    (camera, cam_xform): (&Camera, &GlobalTransform),
    window: &Window,
    badge_q: &mut FlightBadgeQuery<'_, '_>,
) {
    let mut entries: Vec<(DVec2, String, Color)> = Vec::with_capacity(BADGE_SLOTS);

    if let Some(v) = flight.world.active_vessel() {
        entries.push((v.pos, v.name.clone(), Color::srgb(0.38, 0.96, 1.0)));
    }
    if let Some(moon) = flight.world.bodies.get(1) {
        entries.push((
            moon.pos,
            String::from("Moon"),
            Color::srgb(0.95, 0.88, 0.42),
        ));
    }
    if let Some(mpos) = flight.last_prediction.maneuver_world_pos {
        entries.push((
            mpos,
            format!("NODE T-{:.0}s", flight.maneuver.lead_time_s),
            Color::srgb(1.0, 0.52, 0.95),
        ));
    }

    let markers = if flight.maneuver.enabled && !flight.last_prediction.planned_markers.is_empty() {
        &flight.last_prediction.planned_markers
    } else {
        &flight.last_prediction.markers
    };

    for m in markers {
        if entries.len() >= BADGE_SLOTS {
            break;
        }
        let (lbl, col) = match m.kind {
            ApsisKind::Apoapsis => (
                format!("Ap {:.1} km", m.altitude_m / 1000.0),
                Color::srgb(0.25, 0.98, 0.60),
            ),
            ApsisKind::Periapsis => (
                format!("Pe {:.1} km", m.altitude_m / 1000.0),
                Color::srgb(0.40, 0.76, 1.0),
            ),
            ApsisKind::Impact => (String::from("IMPACT"), Color::srgb(1.0, 0.34, 0.28)),
            ApsisKind::MoonEncounter => (
                format!("Moon Encounter ({:.0} km)", m.altitude_m / 1000.0),
                Color::srgb(1.0, 0.86, 0.24),
            ),
            ApsisKind::ClosestApproach => (
                format!("Closest ({:.0} km)", m.altitude_m / 1000.0),
                Color::srgb(0.95, 0.50, 0.98),
            ),
        };
        entries.push((m.world_pos, lbl, col));
    }

    for (slot, mut node, mut text, mut tcol) in badge_q {
        if let Some((wpos, label, col)) = entries.get(slot.0) {
            let rpos = world_to_render_vec2(*wpos, origin);
            if let Ok(vp) = camera.world_to_viewport(cam_xform, rpos.extend(0.0))
                && vp.x > 16.0
                && vp.y > 16.0
                && vp.x < window.width() - 80.0
                && vp.y < window.height() - 24.0
            {
                node.display = Display::Flex;
                node.left = px(vp.x + 8.0);
                node.top = px(vp.y - 10.0);
                *text = Text::new(label);
                tcol.0 = *col;
                continue;
            }
        }
        node.display = Display::None;
    }
}

fn format_navball_readout(flight: &FlightSimState) -> String {
    let Some(v) = flight.world.active_vessel() else {
        return String::from("NO VESSEL");
    };
    let Some(body) = dominant_body(&flight.world.bodies, v.pos) else {
        return String::from("DEEP SPACE");
    };
    let up = (v.pos - body.pos).normalize_or_zero();
    let east = DVec2::new(up.y, -up.x);
    let nose = DVec2::Y.rotate(v.angle);
    let pitch_deg = nose.dot(up).atan2(nose.dot(east)).to_degrees();
    let rel_v = v.vel - body.vel;
    let v_vert = rel_v.dot(up);
    let v_horiz = rel_v.dot(east);

    format!(
        "Pitch: {:+.1} deg   SAS: {}\n\
         V/S: {:+.1} m/s   H/S: {:+.1} m/s\n\
         RCS: {}   Vessel #{}",
        pitch_deg,
        if flight.sas_enabled {
            flight.sas_mode.label()
        } else {
            "OFF"
        },
        v_vert,
        v_horiz,
        if flight.rcs_enabled {
            if v.has_rcs() { "ON" } else { "NO BLOCK" }
        } else {
            "OFF"
        },
        v.id
    )
}

#[allow(clippy::too_many_lines)]
fn format_flight_hud(flight: &FlightSimState, clock: &FlightClock) -> String {
    let Some(v) = flight.world.active_vessel() else {
        return String::from("No active vessel");
    };
    let elem = flight.world.vessel_orbit_elements(v.id);
    let (body_name, alt_km, speed, ap_km, pe_km, ecc, status) = elem.map_or(
        ("Deep Space", 0.0, 0.0, 0.0, 0.0, 0.0, "DEEP SPACE"),
        |el| {
            let b_name = flight
                .world
                .bodies
                .iter()
                .find(|b| b.id == el.body_id)
                .map_or("Deep Space", |b| b.name.as_str());
            let st = if v.crashed {
                "CRASHED"
            } else if v.landed {
                "LANDED"
            } else if el.bound && el.periapsis_alt_m > 70_000.0 {
                "STABLE ORBIT"
            } else if el.bound {
                "SUBORBITAL"
            } else {
                "ESCAPE"
            };
            (
                b_name,
                el.altitude_m / 1000.0,
                el.speed_m_s,
                el.apoapsis_alt_m / 1000.0,
                el.periapsis_alt_m / 1000.0,
                el.eccentricity,
                st,
            )
        },
    );

    let fuel_kg = v.total_fuel_kg();
    let cap_kg = v.total_fuel_capacity_kg().max(1.0);
    let fuel_pct = (fuel_kg / cap_kg) * 100.0;
    let abl_cap = v.total_ablator_capacity_kg();
    let abl_str = if abl_cap > 0.0 {
        format!("{:.0}/{:.0} kg", v.total_ablator_kg(), abl_cap)
    } else {
        String::from("--")
    };
    let chute_str = if v.any_chute_deployed() {
        "DEPLOYED"
    } else if v.any_chute_armed() {
        "ARMED"
    } else {
        "STOWED"
    };

    let transfer_str = match (flight.transfer_est, flight.maneuver.enabled) {
        (Some(est), true) => {
            let closest = flight
                .last_prediction
                .planned_closest_moon_dist_m
                .map_or_else(
                    || String::from("--"),
                    |d| format!("{:.0} km", (d - 120_000.0) / 1000.0),
                );
            let dv_tot = flight
                .maneuver
                .prograde_dv_m_s
                .hypot(flight.maneuver.radial_dv_m_s);
            let max_acc = (v.active_max_thrust_n() / v.total_mass_kg.max(1.0)).max(0.1);
            let burn_s = dv_tot / max_acc;
            format!(
                "TLI NODE: T-{:.0}s | Burn ~{:.1}s\n\
                 Pro: {:+.1} m/s  Rad: {:+.1} m/s  (Cap ~{:.0})\n\
                 Planned Moon Miss/Peri: {}",
                flight.maneuver.lead_time_s,
                burn_s,
                flight.maneuver.prograde_dv_m_s,
                flight.maneuver.radial_dv_m_s,
                est.dv2_capture_m_s,
                closest
            )
        }
        (Some(est), false) => format!(
            "Moon Window: in {:.0}s | Est TLI dV {:.0} m/s",
            est.lead_time_until_window_s, est.dv1_m_s
        ),
        (None, _) => String::new(),
    };

    let warp_mode = if clock.paused {
        " (PAUSED)"
    } else if clock.physical_capped {
        " (PHYS)"
    } else {
        " (RAILS)"
    };

    format!(
        "TELEMETRY - {} ({})\n\
         Status: {}   MET: {:.1} s   Warp: {}x{}\n\
         Alt: {:.2} km   Speed: {:.1} m/s\n\
         Ap: {:.2} km   Pe: {:.2} km   Ecc: {:.4}\n\
         Temp: {:.0} K   Ablator: {}   Chute: {}\n\
         Throttle: {:.0}%   Stage: {}/{}   Mass: {:.0} kg\n\
         Fuel: {:.0} kg ({:.0}%)\n\
         {}\n\
         {}",
        v.name,
        body_name,
        status,
        flight.world.time_s,
        clock.warp_factor(),
        warp_mode,
        alt_km,
        speed,
        ap_km,
        pe_km,
        ecc,
        v.peak_temperature_k(),
        abl_str,
        chute_str,
        flight.throttle * 100.0,
        v.current_stage,
        v.max_stage + 1,
        v.total_mass_kg,
        fuel_kg,
        fuel_pct,
        transfer_str,
        flight.status_banner,
    )
}

fn format_mission_summary(flight: &FlightSimState) -> String {
    let outcome = if flight.recovered_summary {
        "MISSION RECOVERED SAFELY"
    } else if flight.world.active_vessel().is_some_and(|v| v.crashed) {
        "VESSEL LOST / CRASHED"
    } else {
        "FLIGHT PAUSED"
    };

    format!(
        "{}\n\n\
         Mission Elapsed Time: {:.1} s\n\
         Peak Altitude: {:.2} km\n\
         Peak Speed: {:.1} m/s\n\
         Peak Hull Temp: {:.0} K\n\
         Stages Fired: {}",
        outcome,
        flight.world.time_s,
        flight.max_altitude_m / 1000.0,
        flight.max_speed_m_s,
        flight.max_temp_k,
        flight.staging_count,
    )
}

// ---------- EXPORTS
