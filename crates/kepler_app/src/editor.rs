//! @file editor.rs
//!
//! @version 1.0.1
//! @author BleckWolf25
//! @license MIT
//!
//! @summary Milestone 2 rocket construction editor with grid/stack/radial attachment and readouts
//!
//! @description
//! Implements the Vehicle Assembly Building (VAB) editor state: spawns procedural part meshes for
//! the active `VesselBlueprint`, renders the center-of-mass reticle, supports palette selection via
//! Bevy 0.20 `ui_widgets::Button` + `On<Activate>` observers, stack/radial attachment, subtree
//! removal, stage index editing, live Tsiolkovsky delta-v / MoI / tip-over torque readouts, and
//! launching directly into flight.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use crate::camera::MainCamera;
use crate::procedural_mesh::build_part_mesh;
use crate::shaders::PartMaterial;
use crate::state::{ActiveBlueprint, GameMode, LivePartCatalog, uniform_border};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::{Activate, Button};
use sim_core::{
    AttachKind, BlueprintAnalysis, DVec2, PartKind, analyze_blueprint,
    starter_two_stage_blueprint,
};

// ---------- CONSTANTS

const GRID_SNAP_M: f64 = 0.10;

// ---------- TYPES

#[derive(Component)]
struct EditorEntity;

#[derive(Component)]
struct EditorPartNode {
    instance_id: u64,
}

#[derive(Component)]
struct EditorComMarker;

#[derive(Component)]
struct EditorReadoutText;

#[derive(Component)]
struct PaletteButton(String);

#[derive(Component)]
struct LaunchButtonMarker;

#[derive(Clone, Copy)]
enum AttachAction {
    AttachBottom,
    AttachTop,
    AttachRadialPair,
    StageDown,
    StageUp,
    DeleteSelected,
    ResetStarter,
    Launch,
}

pub struct EditorPlugin;

// ---------- DECLARATIONS / IMPLEMENTATION

fn setup_editor(
    mut commands: Commands,
    catalog: Res<LivePartCatalog>,
    mut bp: ResMut<ActiveBlueprint>,
) {
    bp.dirty = true;
    spawn_editor_ui(&mut commands, &catalog);
    commands.spawn((
        EditorEntity,
        EditorComMarker,
        Transform::from_xyz(0.0, 0.0, 4.0),
        Visibility::Visible,
    ));
}

fn cleanup_editor(mut commands: Commands, query: Query<Entity, With<EditorEntity>>) {
    for entity in &query {
        commands.entity(entity).despawn();
    }
}

fn spawn_editor_ui(commands: &mut Commands, catalog: &LivePartCatalog) {
    let panel_bg = BackgroundColor(Color::srgba(0.04, 0.07, 0.12, 0.90));
    let border = uniform_border(Color::srgba(0.18, 0.65, 0.88, 0.55));
    let text_col = TextColor(Color::srgb(0.86, 0.95, 1.0));

    commands
        .spawn((
            EditorEntity,
            Node {
                position_type: PositionType::Absolute,
                left: px(14.0),
                top: px(14.0),
                width: px(255.0),
                padding: UiRect::all(px(12.0)),
                row_gap: px(6.0),
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
                Text::new("PART PALETTE (RON)"),
                TextFont::from_font_size(14.0),
                TextColor(Color::srgb(0.32, 0.88, 1.0)),
            ));

            for spec in catalog.sim_catalog.iter() {
                let label = format!("{}  ({:.0} kg)", spec.display_name, spec.full_mass_kg());
                let spec_id = spec.id.clone();
                parent
                    .spawn((
                        Button,
                        Hovered::default(),
                        PaletteButton(spec.id.clone()),
                        Node {
                            padding: UiRect::axes(px(10.0), px(7.0)),
                            border: UiRect::all(px(1.0)),
                            border_radius: BorderRadius::all(CornerRadius::circular(px(4.0))),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.08, 0.14, 0.22, 0.92)),
                        uniform_border(Color::srgba(0.24, 0.52, 0.72, 0.5)),
                    ))
                    .observe(
                        move |_act: On<Activate>, mut active_bp: ResMut<ActiveBlueprint>| {
                            active_bp.active_palette_spec = spec_id.clone();
                        },
                    )
                    .with_children(|btn| {
                        btn.spawn((Text::new(label), TextFont::from_font_size(12.0), text_col));
                    });
            }

            parent.spawn((
                Text::new(
                    "Click part to select.\n\
                     L-Click select / R-Drag pan / Wheel zoom.\n\
                     Keys: [B] Bottom [T] Top [R] Radial\n\
                     [ [ / ] ] Stage -/+  [Del] Remove  [Enter] Launch",
                ),
                TextFont::from_font_size(11.0),
                TextColor(Color::srgba(0.68, 0.82, 0.92, 0.85)),
            ));
        });

    commands
        .spawn((
            EditorEntity,
            Node {
                position_type: PositionType::Absolute,
                right: px(14.0),
                top: px(14.0),
                width: px(325.0),
                padding: UiRect::all(px(12.0)),
                row_gap: px(8.0),
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
                Text::new("KEPLER YARD — ASSEMBLY"),
                TextFont::from_font_size(14.0),
                TextColor(Color::srgb(0.32, 0.88, 1.0)),
            ));

            parent.spawn((
                EditorReadoutText,
                Text::new("Analyzing blueprint..."),
                TextFont::from_font_size(12.0),
                text_col,
            ));

            let buttons = [
                ("Attach Below Selected [B]", AttachAction::AttachBottom, false),
                ("Attach Above Selected [T]", AttachAction::AttachTop, false),
                ("Attach Radial Pair [R]", AttachAction::AttachRadialPair, false),
                ("Stage -1 [ [ ]", AttachAction::StageDown, false),
                ("Stage +1 [ ] ]", AttachAction::StageUp, false),
                ("Remove Subtree [Del]", AttachAction::DeleteSelected, false),
                ("Reset Starter Rocket", AttachAction::ResetStarter, false),
                ("LAUNCH TO PAD [Enter]", AttachAction::Launch, true),
            ];

            for (caption, action, is_launch) in buttons {
                let bg = if is_launch {
                    BackgroundColor(Color::srgba(0.10, 0.45, 0.30, 0.96))
                } else {
                    BackgroundColor(Color::srgba(0.08, 0.14, 0.22, 0.92))
                };
                let b_col = if is_launch {
                    uniform_border(Color::srgba(0.28, 0.96, 0.62, 0.9))
                } else {
                    uniform_border(Color::srgba(0.24, 0.52, 0.72, 0.5))
                };

                let mut entity_cmds = parent.spawn((
                    Button,
                    Hovered::default(),
                    Node {
                        padding: UiRect::axes(px(10.0), px(7.0)),
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(CornerRadius::circular(px(4.0))),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    bg,
                    b_col,
                ));
                if is_launch {
                    entity_cmds.insert(LaunchButtonMarker);
                }
                entity_cmds
                    .observe(
                        move |_act: On<Activate>,
                              catalog: Res<LivePartCatalog>,
                              mut active_bp: ResMut<ActiveBlueprint>,
                              mut next_mode: ResMut<NextState<GameMode>>| {
                            apply_editor_action(action, &catalog, &mut active_bp, &mut next_mode);
                        },
                    )
                    .with_children(|btn| {
                        btn.spawn((Text::new(caption), TextFont::from_font_size(12.0), text_col));
                    });
            }
        });
}

fn button_bg_for_state(pressed: bool, hovered: bool, is_launch: bool) -> BackgroundColor {
    if is_launch {
        match (pressed, hovered) {
            (true, _) => BackgroundColor(Color::srgba(0.16, 0.64, 0.42, 0.98)),
            (false, true) => BackgroundColor(Color::srgba(0.13, 0.55, 0.36, 0.98)),
            (false, false) => BackgroundColor(Color::srgba(0.10, 0.45, 0.30, 0.96)),
        }
    } else {
        match (pressed, hovered) {
            (true, _) => BackgroundColor(Color::srgba(0.16, 0.38, 0.56, 0.98)),
            (false, true) => BackgroundColor(Color::srgba(0.12, 0.24, 0.36, 0.95)),
            (false, false) => BackgroundColor(Color::srgba(0.08, 0.14, 0.22, 0.92)),
        }
    }
}

fn style_buttons_on_change(
    mut buttons: Query<
        (&Hovered, Has<Pressed>, Has<LaunchButtonMarker>, &mut BackgroundColor),
        (
            Or<(Changed<Hovered>, Added<Pressed>)>,
            With<Button>,
        ),
    >,
) {
    for (hovered, pressed, is_launch, mut bg) in &mut buttons {
        *bg = button_bg_for_state(pressed, hovered.get(), is_launch);
    }
}

fn style_buttons_on_release(
    mut removed_pressed: RemovedComponents<Pressed>,
    mut buttons: Query<(&Hovered, Has<LaunchButtonMarker>, &mut BackgroundColor), With<Button>>,
) {
    for entity in removed_pressed.read() {
        if let Ok((hovered, is_launch, mut bg)) = buttons.get_mut(entity) {
            *bg = button_bg_for_state(false, hovered.get(), is_launch);
        }
    }
}

fn rebuild_editor_meshes_if_dirty(
    mut commands: Commands,
    mut active_bp: ResMut<ActiveBlueprint>,
    catalog: Res<LivePartCatalog>,
    existing: Query<Entity, With<EditorPartNode>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PartMaterial>>,
) {
    if !active_bp.dirty {
        return;
    }
    active_bp.dirty = false;

    for e in &existing {
        commands.entity(e).despawn();
    }

    for part in &active_bp.blueprint.parts {
        let Some(spec) = catalog.sim_catalog.get(&part.spec_id) else {
            continue;
        };
        let style = catalog
            .visuals
            .get(&part.spec_id)
            .copied()
            .unwrap_or_default();
        let selected = active_bp.selected_part_id == Some(part.instance_id);
        let mesh = meshes.add(build_part_mesh(&spec.shape));
        let material = materials.add(PartMaterial {
            base_color: Vec4::from_array(style.base_color),
            accent_color: Vec4::from_array(style.accent_color),
            flags: Vec4::new(
                style.emissive_strength,
                if selected { 1.0 } else { 0.0 },
                1.0,
                0.0,
            ),
        });

        commands.spawn((
            EditorEntity,
            EditorPartNode {
                instance_id: part.instance_id,
            },
            Mesh2d(mesh),
            MeshMaterial2d(material),
            Transform::from_xyz(part.local_pos.x as f32, part.local_pos.y as f32, 0.0)
                .with_rotation(Quat::from_rotation_z(part.local_rot as f32)),
        ));
    }
}

fn handle_editor_part_picking(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cam_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    catalog: Res<LivePartCatalog>,
    mut active_bp: ResMut<ActiveBlueprint>,
) {
    if !mouse_buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok((camera, cam_xform)) = cam_q.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if cursor.x < 275.0 || cursor.x > window.width() - 345.0 {
        return;
    }
    let Ok(world_pos) = camera.viewport_to_world_2d(cam_xform, cursor) else {
        return;
    };

    let click_d = DVec2::new(f64::from(world_pos.x), f64::from(world_pos.y));
    let mut best: Option<(u64, f64)> = None;
    for part in &active_bp.blueprint.parts {
        let Some(spec) = catalog.sim_catalog.get(&part.spec_id) else {
            continue;
        };
        let half = spec.shape.contact_half_extent();
        let rel = (click_d - part.local_pos).rotate(-part.local_rot);
        if rel.x.abs() <= half.x + 0.15 && rel.y.abs() <= half.y + 0.15 {
            let d2 = rel.length_squared();
            if best.is_none_or(|(_, bd2)| d2 < bd2) {
                best = Some((part.instance_id, d2));
            }
        }
    }

    if let Some((hit_id, _)) = best {
        active_bp.selected_part_id = Some(hit_id);
        active_bp.dirty = true;
    }
}

fn handle_editor_keyboard_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    catalog: Res<LivePartCatalog>,
    mut active_bp: ResMut<ActiveBlueprint>,
    mut next_mode: ResMut<NextState<GameMode>>,
) {
    let action = if keys.just_pressed(KeyCode::KeyB) {
        Some(AttachAction::AttachBottom)
    } else if keys.just_pressed(KeyCode::KeyT) {
        Some(AttachAction::AttachTop)
    } else if keys.just_pressed(KeyCode::KeyR) {
        Some(AttachAction::AttachRadialPair)
    } else if keys.just_pressed(KeyCode::BracketLeft) {
        Some(AttachAction::StageDown)
    } else if keys.just_pressed(KeyCode::BracketRight) {
        Some(AttachAction::StageUp)
    } else if keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace) {
        Some(AttachAction::DeleteSelected)
    } else if keys.just_pressed(KeyCode::Enter) {
        Some(AttachAction::Launch)
    } else {
        None
    };

    if let Some(act) = action {
        apply_editor_action(act, &catalog, &mut active_bp, &mut next_mode);
    }
}

fn apply_editor_action(
    action: AttachAction,
    catalog: &LivePartCatalog,
    active_bp: &mut ActiveBlueprint,
    next_mode: &mut NextState<GameMode>,
) {
    match action {
        AttachAction::AttachBottom => {
            attach_to_selected(active_bp, catalog, AttachKind::StackBottom);
        }
        AttachAction::AttachTop => {
            attach_to_selected(active_bp, catalog, AttachKind::StackTop);
        }
        AttachAction::AttachRadialPair => {
            attach_radial_pair(active_bp, catalog);
        }
        AttachAction::StageDown => {
            if let Some(sel) = active_bp.selected_part_id
                && let Some(part) = active_bp
                    .blueprint
                    .parts
                    .iter_mut()
                    .find(|p| p.instance_id == sel)
            {
                part.stage = part.stage.saturating_sub(1);
                active_bp.dirty = true;
            }
        }
        AttachAction::StageUp => {
            if let Some(sel) = active_bp.selected_part_id
                && let Some(part) = active_bp
                    .blueprint
                    .parts
                    .iter_mut()
                    .find(|p| p.instance_id == sel)
            {
                part.stage = part.stage.saturating_add(1).min(9);
                active_bp.dirty = true;
            }
        }
        AttachAction::DeleteSelected => {
            if let Some(sel) = active_bp.selected_part_id
                && active_bp.blueprint.remove_subtree(sel)
            {
                active_bp.selected_part_id =
                    active_bp.blueprint.parts.first().map(|p| p.instance_id);
                active_bp.dirty = true;
            }
        }
        AttachAction::ResetStarter => {
            active_bp.blueprint = starter_two_stage_blueprint(&catalog.sim_catalog);
            active_bp.selected_part_id = active_bp.blueprint.parts.first().map(|p| p.instance_id);
            active_bp.dirty = true;
        }
        AttachAction::Launch => {
            next_mode.set(GameMode::Flight);
        }
    }
}

fn attach_to_selected(
    active_bp: &mut ActiveBlueprint,
    catalog: &LivePartCatalog,
    kind: AttachKind,
) {
    let Some(parent_id) = active_bp.selected_part_id else {
        return;
    };
    let Some(parent_part) = active_bp
        .blueprint
        .parts
        .iter()
        .find(|p| p.instance_id == parent_id)
        .cloned()
    else {
        return;
    };
    let Some(parent_spec) = catalog.sim_catalog.get(&parent_part.spec_id) else {
        return;
    };
    let Some(new_spec) = catalog.sim_catalog.get(&active_bp.active_palette_spec) else {
        return;
    };

    let offset = match kind {
        AttachKind::StackBottom if parent_spec.stack_bottom && new_spec.stack_top => DVec2::new(
            0.0,
            parent_spec.shape.bottom_offset().y - new_spec.shape.top_offset().y,
        ),
        AttachKind::StackTop if parent_spec.stack_top && new_spec.stack_bottom => DVec2::new(
            0.0,
            parent_spec.shape.top_offset().y - new_spec.shape.bottom_offset().y,
        ),
        _ => return,
    };

    let pos = snap_to_grid(parent_part.local_pos + offset);
    let new_id = active_bp.blueprint.attach_child(
        parent_id,
        new_spec.id.clone(),
        kind,
        pos,
        0.0,
        parent_part.stage,
    );
    active_bp.selected_part_id = Some(new_id);
    active_bp.dirty = true;
}

fn attach_radial_pair(active_bp: &mut ActiveBlueprint, catalog: &LivePartCatalog) {
    let Some(parent_id) = active_bp.selected_part_id else {
        return;
    };
    let Some(parent_part) = active_bp
        .blueprint
        .parts
        .iter()
        .find(|p| p.instance_id == parent_id)
        .cloned()
    else {
        return;
    };
    let Some(parent_spec) = catalog.sim_catalog.get(&parent_part.spec_id) else {
        return;
    };
    let Some(new_spec) = catalog.sim_catalog.get(&active_bp.active_palette_spec) else {
        return;
    };

    let dx = 0.5 * parent_spec.shape.bounding_size().x + 0.5 * new_spec.shape.bounding_size().x;
    let left_pos = snap_to_grid(parent_part.local_pos + DVec2::new(-dx, 0.0));
    let right_pos = snap_to_grid(parent_part.local_pos + DVec2::new(dx, 0.0));
    let right_rot = if matches!(new_spec.kind, PartKind::Fin(_)) {
        core::f64::consts::PI
    } else {
        0.0
    };

    let new_left = active_bp.blueprint.attach_child(
        parent_id,
        new_spec.id.clone(),
        AttachKind::RadialLeft,
        left_pos,
        0.0,
        parent_part.stage,
    );
    active_bp.blueprint.attach_child(
        parent_id,
        new_spec.id.clone(),
        AttachKind::RadialRight,
        right_pos,
        right_rot,
        parent_part.stage,
    );
    active_bp.selected_part_id = Some(new_left);
    active_bp.dirty = true;
}

#[inline]
fn snap_to_grid(pos: DVec2) -> DVec2 {
    DVec2::new(
        (pos.x / GRID_SNAP_M).round() * GRID_SNAP_M,
        (pos.y / GRID_SNAP_M).round() * GRID_SNAP_M,
    )
}

fn draw_editor_gizmos(
    catalog: Res<LivePartCatalog>,
    active_bp: Res<ActiveBlueprint>,
    mut gizmos: Gizmos,
    mut com_q: Query<&mut Transform, With<EditorComMarker>>,
) {
    let analysis = analyze_blueprint(&active_bp.blueprint, &catalog.sim_catalog);

    let grid_col = Color::srgba(0.14, 0.30, 0.44, 0.25);
    for i in -8..=8 {
        let p = i as f32;
        gizmos.line_2d(Vec2::new(p, -10.0), Vec2::new(p, 10.0), grid_col);
        gizmos.line_2d(Vec2::new(-10.0, p), Vec2::new(10.0, p), grid_col);
    }

    let com_v = Vec2::new(analysis.com_local.x as f32, analysis.com_local.y as f32);
    let reticle_col = Color::srgb(1.0, 0.88, 0.22);
    gizmos.circle_2d(com_v, 0.22, reticle_col);
    gizmos.line_2d(com_v - Vec2::X * 0.35, com_v + Vec2::X * 0.35, reticle_col);
    gizmos.line_2d(com_v - Vec2::Y * 0.35, com_v + Vec2::Y * 0.35, reticle_col);

    if let Ok(mut com_tf) = com_q.single_mut() {
        com_tf.translation = Vec3::new(com_v.x, com_v.y, 4.0);
    }
}

fn update_editor_readouts_and_highlights(
    time: Res<Time>,
    catalog: Res<LivePartCatalog>,
    active_bp: Res<ActiveBlueprint>,
    mut readout_q: Query<&mut Text, With<EditorReadoutText>>,
    mat_q: Query<(&EditorPartNode, &MeshMaterial2d<PartMaterial>)>,
    mut palette_q: Query<(&PaletteButton, &mut BorderColor)>,
    mut materials: ResMut<Assets<PartMaterial>>,
) {
    let t = time.elapsed_secs();
    for (node, mat_handle) in &mat_q {
        if let Some(mut mat) = materials.get_mut(&mat_handle.0) {
            mat.flags.y = if active_bp.selected_part_id == Some(node.instance_id) {
                1.0
            } else {
                0.0
            };
            mat.flags.w = t;
        }
    }

    for (pbtn, mut border) in &mut palette_q {
        *border = if pbtn.0 == active_bp.active_palette_spec {
            uniform_border(Color::srgba(0.28, 0.92, 1.0, 0.95))
        } else {
            uniform_border(Color::srgba(0.24, 0.52, 0.72, 0.50))
        };
    }

    if let Ok(mut text) = readout_q.single_mut() {
        let analysis = analyze_blueprint(&active_bp.blueprint, &catalog.sim_catalog);
        *text = Text::new(format_analysis_text(&active_bp, &catalog, &analysis));
    }
}

fn format_analysis_text(
    active_bp: &ActiveBlueprint,
    catalog: &LivePartCatalog,
    a: &BlueprintAnalysis,
) -> String {
    let sel_info = active_bp
        .selected_part_id
        .and_then(|id| active_bp.blueprint.parts.iter().find(|p| p.instance_id == id))
        .map(|p| {
            let name = catalog
                .sim_catalog
                .get(&p.spec_id)
                .map_or(p.spec_id.as_str(), |s| s.display_name.as_str());
            format!("{name} (Stage S{})", p.stage + 1)
        })
        .unwrap_or_else(|| String::from("None"));

    let mut out = format!(
        "Craft: {}\n\
         Palette: {}\n\
         Selected: {}\n\n\
         Wet Mass: {:.0} kg  (Fuel: {:.0} kg)\n\
         Dry Mass: {:.0} kg\n\
         CoM: ({:+.2}, {:+.2}) m\n\
         MoI: {:.0} kg·m²\n\
         Total Δv: {:.0} m/s\n",
        active_bp.blueprint.name,
        active_bp.active_palette_spec,
        sel_info,
        a.wet_mass_kg,
        a.fuel_mass_kg,
        a.dry_mass_kg,
        a.com_local.x,
        a.com_local.y,
        a.moi_kg_m2,
        a.total_delta_v_m_s
    );

    if a.tip_over_warning {
        out.push_str(&format!(
            "\nWARNING: Off-axis thrust torque {:.0} N·m!\n",
            a.net_thrust_torque_nm
        ));
    }

    out.push_str("\nSTAGES (firing order):\n");
    for s in &a.stages {
        out.push_str(&format!(
            "  S{}: Δv {:.0} m/s | TWR {:.2} | {:.0} kN{}\n",
            s.stage + 1,
            s.delta_v_m_s,
            s.twr_surface,
            s.thrust_n / 1000.0,
            if s.has_decoupler { " [SEP]" } else { "" }
        ));
    }
    out
}

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameMode::Editor), setup_editor)
            .add_systems(OnExit(GameMode::Editor), cleanup_editor)
            .add_systems(
                Update,
                (
                    style_buttons_on_change,
                    style_buttons_on_release,
                    handle_editor_keyboard_shortcuts,
                    handle_editor_part_picking,
                    rebuild_editor_meshes_if_dirty,
                    draw_editor_gizmos,
                    update_editor_readouts_and_highlights,
                )
                    .run_if(in_state(GameMode::Editor)),
            );
    }
}

// ---------- EXPORTS
