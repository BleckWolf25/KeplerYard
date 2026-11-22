//! @file editor.rs
//!
//! @version 2.0.1
//! @author `BleckWolf25`
//! @license MIT
//!
//! @summary Mouse-first, SFS-style VAB assembly editor with symmetry, snapping, and craft library
//!
//! @description
//! Easy to learn, hard to master. Click a catalog part to pick it up and click the bay to place it
//! (hold Shift to keep placing), or drag it straight in. Parts snap flush onto glowing attach nodes
//! and otherwise drop on a 0.10 m grid. A floating toolbar follows the selected part (rotate,
//! flip, clone, crossfeed, stage, delete), right-click deletes, dragging empty space pans, and the
//! wheel zooms toward the cursor. Every control also keeps its keyboard shortcut and a hover tooltip.
//! "Auto-attach" builds a rocket in a few clicks; stages, crossfeed, symmetry and the craft library
//! remain for mastery.
//!
//! @since 21/11/2026
//! @updated 21/11/2026

// ---------- IMPORTS

use crate::camera::{CameraZoomState, MainCamera, is_pointer_over_interactive_ui};
use crate::part_icons::{PartIconRegistry, UiIconKind};
use crate::procedural_mesh::build_part_mesh;
use crate::save_io::{
    delete_saved_craft, list_saved_crafts, load_craft_from_path, save_craft_blueprint,
};
use crate::shaders::PartMaterial;
use crate::state::{ActiveBlueprint, GameMode, LivePartCatalog, part_style_id, uniform_border};
use crate::ui_kit::{
    BORDER_ON, BTN_BORDER, UI_ACCENT, UI_DIM, UI_OK, UI_TEXT, button_bundle, divider_bundle,
    panel_bundle, text_button, Tooltip,
};
use bevy::ecs::system::SystemParam;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::picking::Pickable;
use bevy::picking::hover::{HoverMap, Hovered};
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::{Activate, Button, ScrollArea};
use kepler_core::{
    AttachKind, BlueprintAnalysis, DVec2, PartCategory, PartKind, PartSpec, PlacedPart,
    VesselBlueprint, analyze_blueprint, lunar_capable_blueprint, starter_two_stage_blueprint,
};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt::Write as _;

// ---------- CONSTANTS

const GRID_SNAP_M: f64 = 0.10;
const PALETTE_PANEL_WIDTH: f32 = 300.0;
const INSPECTOR_PANEL_WIDTH: f32 = 340.0;
const DRAG_THRESHOLD_PX: f32 = 7.0;
const CLICK_SLOP_PX: f32 = 6.0;
const MAX_UNDO_HISTORY: usize = 60;
const TOOLBAR_W: f32 = 352.0;
const TOOLBAR_H: f32 = 38.0;
const TOP_BAR_CLEARANCE: f32 = 64.0;
const SNAP_RADIUS_PX: f64 = 52.0;

// ---------- TYPES

#[derive(Component)]
struct EditorEntity;

#[derive(Component)]
struct EditorPartNode {
    instance_id: u64,
}

#[derive(Component)]
struct EditorDragGhost;

#[derive(Component)]
struct EditorPaletteGrid;

#[derive(Component)]
struct EditorStageList;

#[derive(Component)]
struct EditorCraftLibraryBox;

#[derive(Component)]
struct EditorCraftList;

#[derive(Component)]
struct PaletteButton(String);

#[derive(Component)]
struct CategoryTabButton(Option<PartCategory>);

#[derive(Component)]
struct LaunchButtonMarker;

#[derive(Component)]
struct SelectionToolbar;

#[derive(Component)]
struct CrossfeedToolButton;

/// Which dynamic text a `Text` entity displays; one query updates them all.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum EditorText {
    Readout,
    SymStatus,
    CraftName,
    SymLabel,
    Brush,
    Hint,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ToggleKind {
    Symmetry,
    Snap,
}

#[derive(Component)]
struct ToggleButton(ToggleKind);

#[derive(Clone, Copy)]
enum AttachAction {
    AttachBottom,
    AttachTop,
    AttachRadialPair,
    AutoAttach,
    ToggleSymmetry,
    ToggleSnap,
    RotateLeft,
    RotateRight,
    FlipMirror,
    DuplicateSelected,
    ToggleCrossfeed,
    Undo,
    Redo,
    StageDown,
    StageUp,
    DeleteSelected,
    LoadLunarPreset,
    ResetStarter,
    ToggleRename,
    SaveCraft,
    ToggleCraftsDrawer,
    Launch,
}

#[derive(Clone, Copy)]
struct DragGhostPose {
    pos: DVec2,
    rot: f64,
    mirrored: bool,
    snapped: bool,
    style_id: f32,
}

#[derive(Clone, Debug)]
struct SnapTarget {
    parent_id: u64,
    attach_kind: AttachKind,
    local_pos: DVec2,
    local_rot: f64,
    stage: u32,
}

#[derive(Clone, Debug)]
enum DragPhase {
    Idle,
    /// Left button went down on a catalog tile; becomes a drag or a click-to-carry.
    PendingPalette {
        spec_id: String,
        start_cursor: Vec2,
    },
    /// Dragging a new part out of the catalog (release to drop).
    DraggingNew {
        spec_id: String,
    },
    /// Part picked up by a click; follows the cursor until the next click places it.
    Carrying {
        spec_id: String,
    },
    PendingExisting {
        part_id: u64,
        start_cursor: Vec2,
        start_world: DVec2,
        alt_clone: bool,
        ctrl_single: bool,
    },
    DraggingExisting {
        part_id: u64,
        last_world: DVec2,
        is_root: bool,
    },
    /// Left-drag on empty space pans the view; a plain click deselects.
    PanningCanvas {
        start_cursor: Vec2,
        start_pan: Vec2,
        moved: bool,
    },
}

#[derive(Resource, Default)]
struct EditorMeshCache {
    meshes: HashMap<(String, bool), Handle<Mesh>>,
}

#[derive(Resource)]
#[allow(clippy::struct_excessive_bools)]
struct EditorState {
    selected_category: Option<PartCategory>,
    palette_dirty: bool,
    lists_dirty: bool,
    crafts_drawer_open: bool,
    symmetry_2x: bool,
    snap_nodes: bool,
    renaming_craft: bool,
    drag: DragPhase,
    snap_preview: Option<DVec2>,
    hover_part: Option<u64>,
    right_press: Option<Vec2>,
    undo_stack: VecDeque<VesselBlueprint>,
    redo_stack: VecDeque<VesselBlueprint>,
    status_note: String,
}

#[derive(SystemParam)]
struct EditorContext<'w, 's> {
    windows: Query<'w, 's, &'static Window>,
    cam_q: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<MainCamera>>,
    catalog: Res<'w, LivePartCatalog>,
    mouse_buttons: Res<'w, ButtonInput<MouseButton>>,
    keys: Res<'w, ButtonInput<KeyCode>>,
    hover_map: Res<'w, HoverMap>,
    ui_nodes: Query<'w, 's, Option<&'static Pickable>, With<Node>>,
}

#[derive(SystemParam)]
struct EditorAssets<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<PartMaterial>>,
    mesh_cache: ResMut<'w, EditorMeshCache>,
}

#[derive(SystemParam)]
#[allow(clippy::struct_field_names)]
struct EditorUiLists<'w, 's> {
    stage_list_q: Query<'w, 's, Entity, With<EditorStageList>>,
    craft_box_q: Query<'w, 's, &'static mut Node, With<EditorCraftLibraryBox>>,
    craft_list_q: Query<'w, 's, Entity, With<EditorCraftList>>,
    children_q: Query<'w, 's, &'static Children>,
}

#[derive(SystemParam)]
#[allow(clippy::struct_field_names)]
struct EditorReadoutQueries<'w, 's> {
    text_q: Query<'w, 's, (&'static EditorText, &'static mut Text)>,
    mat_q: Query<
        'w,
        's,
        (
            &'static EditorPartNode,
            &'static MeshMaterial2d<PartMaterial>,
        ),
    >,
    palette_q: Query<'w, 's, (&'static PaletteButton, &'static mut BorderColor)>,
    tab_q: Query<
        'w,
        's,
        (&'static CategoryTabButton, &'static mut BorderColor),
        Without<PaletteButton>,
    >,
    toggle_q: Query<
        'w,
        's,
        (&'static ToggleButton, &'static mut BorderColor),
        (Without<PaletteButton>, Without<CategoryTabButton>),
    >,
}

type ButtonStyleQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Hovered,
        Has<Pressed>,
        Has<LaunchButtonMarker>,
        &'static mut BackgroundColor,
    ),
    (Or<(Changed<Hovered>, Added<Pressed>)>, With<Button>),
>;

type PalettePressQuery<'w, 's> = Query<'w, 's, &'static PaletteButton, Added<Pressed>>;

pub struct EditorPlugin;

// ---------- DECLARATIONS / IMPLEMENTATION

impl Default for EditorState {
    fn default() -> Self {
        Self {
            selected_category: None,
            palette_dirty: true,
            lists_dirty: true,
            crafts_drawer_open: false,
            symmetry_2x: true,
            snap_nodes: true,
            renaming_craft: false,
            drag: DragPhase::Idle,
            snap_preview: None,
            hover_part: None,
            right_press: None,
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            status_note: String::from("Pick a part on the left, then click the bay to place it."),
        }
    }
}

impl EditorState {
    fn record_undo(&mut self, bp: &VesselBlueprint) {
        self.undo_stack.push_back(bp.clone());
        if self.undo_stack.len() > MAX_UNDO_HISTORY {
            self.undo_stack.pop_front();
        }
        self.redo_stack.clear();
    }
}

fn get_or_build_mesh(
    assets: &mut EditorAssets<'_>,
    spec_id: &str,
    shape: &kepler_core::PartShape,
    mirrored: bool,
) -> Handle<Mesh> {
    let key = (spec_id.to_owned(), mirrored);
    if let Some(h) = assets.mesh_cache.meshes.get(&key) {
        return h.clone();
    }
    let handle = assets.meshes.add(build_part_mesh(shape, mirrored));
    assets.mesh_cache.meshes.insert(key, handle.clone());
    handle
}

fn setup_editor(
    mut commands: Commands<'_, '_>,
    icons: Res<'_, PartIconRegistry>,
    mut bp: ResMut<'_, ActiveBlueprint>,
) {
    bp.dirty = true;
    commands.insert_resource(EditorState::default());
    commands.insert_resource(EditorMeshCache::default());
    spawn_editor_ui(&mut commands, &icons);
}

fn cleanup_editor(
    mut commands: Commands<'_, '_>,
    query: Query<'_, '_, Entity, With<EditorEntity>>,
) {
    for entity in &query {
        commands.entity(entity).despawn();
    }
    commands.remove_resource::<EditorState>();
    commands.remove_resource::<EditorMeshCache>();
}

fn bind_action(cmd: &mut EntityCommands<'_>, action: AttachAction) {
    cmd.observe(
        move |_act: On<'_, '_, Activate>,
              catalog: Res<'_, LivePartCatalog>,
              mut active_bp: ResMut<'_, ActiveBlueprint>,
              mut ed_state: ResMut<'_, EditorState>,
              mut next_mode: ResMut<'_, NextState<GameMode>>| {
            apply_editor_action(
                action,
                &catalog,
                &mut active_bp,
                &mut ed_state,
                &mut next_mode,
            );
        },
    );
}

fn spawn_icon(parent: &mut ChildSpawnerCommands<'_>, handle: Handle<Image>, size: f32) {
    parent.spawn((
        ImageNode::new(handle),
        Pickable::IGNORE,
        Node {
            width: px(size),
            height: px(size),
            min_width: px(size),
            min_height: px(size),
            flex_shrink: 0.0,
            ..default()
        },
    ));
}

fn label_text(text: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text.into()),
        TextFont::from_font_size(size),
        TextColor(color),
        Pickable::IGNORE,
    )
}

/// Human-readable stat sheet for a part (used by tooltips and the brush card).
fn part_summary(spec: &PartSpec) -> String {
    let mut s = format!(
        "{}\nMass {:.0} kg (dry {:.0} kg)",
        spec.display_name,
        spec.full_mass_kg(),
        spec.dry_mass_kg
    );
    let _ = match spec.kind {
        PartKind::CommandPod(p) => write!(
            s,
            "\n{} command pod, torque {:.0} N*m",
            if p.crewed { "Crewed" } else { "Probe" },
            p.reaction_torque_nm
        ),
        PartKind::Tank(t) => write!(s, "\nPropellant {:.0} kg", t.fuel_capacity_kg),
        PartKind::Engine(e) => write!(
            s,
            "\nThrust {:.0} kN | Isp {:.0} s | gimbal {:.1} deg",
            e.max_thrust_n / 1000.0,
            e.isp_s,
            e.gimbal_range_rad.to_degrees()
        ),
        PartKind::SolidBooster(b) => write!(
            s,
            "\nSolid motor {:.0} kN | Isp {:.0} s | grain {:.0} kg",
            b.max_thrust_n / 1000.0,
            b.isp_s,
            b.fuel_capacity_kg
        ),
        PartKind::Decoupler(d) => write!(
            s,
            "\nSeparates a stage ({:.0} N*s){}",
            d.ejection_impulse_ns,
            if d.crossfeed { " | crossfeed" } else { "" }
        ),
        PartKind::Fin(f) => write!(s, "\nStabilising fin, lift area {:.2} m2", f.lift_area_m2),
        PartKind::Rcs(r) => write!(s, "\nRCS {:.0} N | Isp {:.0} s", r.thrust_n, r.isp_s),
        PartKind::Parachute(p) => write!(s, "\nDeployed drag area {:.0} m2", p.deployed_drag_area_m2),
        PartKind::HeatShield(h) => write!(s, "\nAblator {:.0} kg", h.ablator_kg),
        PartKind::LandingLeg(l) => write!(s, "\nSurvives {:.1} m/s touchdown", l.impact_tolerance_m_s),
        PartKind::NoseCone(n) => write!(s, "\nDrag reduction {:.0}%", n.drag_reduction * 100.0),
    };
    let mut nodes = Vec::new();
    if spec.stack_top {
        nodes.push("top");
    }
    if spec.stack_bottom {
        nodes.push("bottom");
    }
    if spec.radial_attach {
        nodes.push("side");
    }
    let _ = write!(s, "\nAttaches: {}", nodes.join(", "));
    s
}

#[allow(clippy::too_many_lines)]
fn spawn_editor_ui(commands: &mut Commands<'_, '_>, icons: &PartIconRegistry) {
    let panel_bg = BackgroundColor(Color::srgba(0.04, 0.07, 0.12, 0.92));
    let border = uniform_border(Color::srgba(0.18, 0.65, 0.88, 0.55));

    // ---- Left panel: parts catalog, brush card, quick attach
    commands
        .spawn((
            EditorEntity,
            Node {
                position_type: PositionType::Absolute,
                left: px(12.0),
                top: px(12.0),
                bottom: px(12.0),
                width: px(PALETTE_PANEL_WIDTH),
                padding: UiRect::all(px(10.0)),
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
            parent.spawn(label_text("PARTS CATALOG", 14.0, UI_ACCENT));

            parent
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(4.0),
                    row_gap: px(4.0),
                    ..default()
                })
                .with_children(|tabs| {
                    spawn_category_tab(tabs, icons, None, "ALL", UiIconKind::All);
                    for cat in PartCategory::ALL {
                        spawn_category_tab(
                            tabs,
                            icons,
                            Some(cat),
                            cat.short_label(),
                            UiIconKind::from_category(cat),
                        );
                    }
                });

            parent.spawn((
                EditorPaletteGrid,
                ScrollArea,
                Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(6.0),
                    row_gap: px(6.0),
                    align_content: AlignContent::FlexStart,
                    overflow: Overflow::scroll_y(),
                    flex_grow: 1.0,
                    min_height: px(0.0),
                    ..default()
                },
            ));

            // Brush card: stats of the part currently in hand
            parent
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(5.0),
                        padding: UiRect::all(px(7.0)),
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(CornerRadius::circular(px(4.0))),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.06, 0.10, 0.17, 0.96)),
                    uniform_border(Color::srgba(0.32, 0.72, 0.92, 0.45)),
                ))
                .with_children(|card| {
                    card.spawn((
                        EditorText::Brush,
                        label_text("", 10.5, UI_TEXT),
                    ));

                    let mut auto = text_button(
                        card,
                        "AUTO-ATTACH to selected [A]",
                        "Attach the part in hand to the selected part's next free node.\nRepeat to chain parts downward in one click each.",
                        None,
                    );
                    bind_action(&mut auto, AttachAction::AutoAttach);

                    card.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(4.0),
                        ..default()
                    })
                    .with_children(|row| {
                        for (label, tip, action) in [
                            ("Below [B]", "Attach below the selected part", AttachAction::AttachBottom),
                            ("Above [T]", "Attach above the selected part", AttachAction::AttachTop),
                            ("Side [R]", "Attach a radial pair to the selected part", AttachAction::AttachRadialPair),
                        ] {
                            let mut b = text_button(row, label, tip, Some(76.0));
                            bind_action(&mut b, action);
                        }
                    });
                });

            parent.spawn((EditorText::SymStatus, label_text("", 11.0, UI_OK)));
            parent.spawn(label_text(
                "Click a part, then click the bay (Shift: keep placing)\n\
                 Drag parts in or around the rocket | Right-click: delete\n\
                 Drag empty space: pan | Scroll: zoom | Q/E rotate | F flip\n\
                 Ctrl+Drag: move one part | Alt+Drag: clone",
                10.0,
                UI_DIM,
            ));
        });

    // ---- Top bar: history, modes, craft name, file
    commands
        .spawn((
            EditorEntity,
            Pickable::IGNORE,
            Node {
                position_type: PositionType::Absolute,
                left: px(PALETTE_PANEL_WIDTH + 24.0),
                right: px(INSPECTOR_PANEL_WIDTH + 24.0),
                top: px(12.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|row| {
            row.spawn(panel_bundle()).with_children(|bar| {
                let mut c = text_button(bar, "Undo", "Undo the last edit [Ctrl+Z]", Some(54.0));
                bind_action(&mut c, AttachAction::Undo);
                let mut c = text_button(bar, "Redo", "Redo [Ctrl+Y]", Some(54.0));
                bind_action(&mut c, AttachAction::Redo);

                bar.spawn(divider_bundle());

                let mut sym = bar.spawn(button_bundle(
                    Some(86.0),
                    "Mirror symmetry [X]\nParts placed, moved, rotated or staged on one side are mirrored on the other.",
                ));
                sym.insert(ToggleButton(ToggleKind::Symmetry));
                bind_action(&mut sym, AttachAction::ToggleSymmetry);
                sym.with_children(|b| {
                    b.spawn((EditorText::SymLabel, label_text("Sym 2x", 11.0, UI_TEXT)));
                });

                let mut snap = bar.spawn(button_bundle(
                    Some(70.0),
                    "Node snapping [S]\nWhen on, parts click onto glowing attach nodes. Hold Shift to place freely.",
                ));
                snap.insert(ToggleButton(ToggleKind::Snap));
                bind_action(&mut snap, AttachAction::ToggleSnap);
                snap.with_children(|b| {
                    b.spawn(label_text("Snap", 11.0, UI_TEXT));
                });

                bar.spawn(divider_bundle());

                let mut name = bar.spawn(button_bundle(
                    Some(210.0),
                    "Craft name - click to rename, then type and press Enter",
                ));
                bind_action(&mut name, AttachAction::ToggleRename);
                name.with_children(|b| {
                    b.spawn((EditorText::CraftName, label_text("Craft", 12.0, UI_TEXT)));
                });

                bar.spawn(divider_bundle());

                let mut save = bar.spawn(button_bundle(Some(72.0), "Save this craft to disk [Ctrl+S]"));
                bind_action(&mut save, AttachAction::SaveCraft);
                save.with_children(|b| {
                    if let Some(h) = icons.ui_icons.get(&UiIconKind::Save) {
                        spawn_icon(b, h.clone(), 14.0);
                    }
                    b.spawn(label_text("Save", 11.0, UI_TEXT));
                });

                let mut lib = bar.spawn(button_bundle(
                    Some(92.0),
                    "Craft library: load or delete saved crafts, or start from a preset rocket",
                ));
                bind_action(&mut lib, AttachAction::ToggleCraftsDrawer);
                lib.with_children(|b| {
                    if let Some(h) = icons.ui_icons.get(&UiIconKind::Load) {
                        spawn_icon(b, h.clone(), 14.0);
                    }
                    b.spawn(label_text("Library", 11.0, UI_TEXT));
                });
            });
        });

    // ---- Context hint bar (bottom centre)
    commands
        .spawn((
            EditorEntity,
            Pickable::IGNORE,
            Node {
                position_type: PositionType::Absolute,
                left: px(PALETTE_PANEL_WIDTH + 24.0),
                right: px(INSPECTOR_PANEL_WIDTH + 24.0),
                bottom: px(12.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|row| {
            row.spawn((
                Pickable::IGNORE,
                Node {
                    padding: UiRect::axes(px(12.0), px(6.0)),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(CornerRadius::circular(px(5.0))),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.03, 0.06, 0.11, 0.86)),
                uniform_border(Color::srgba(0.20, 0.72, 0.94, 0.40)),
            ))
            .with_children(|pill| {
                pill.spawn((EditorText::Hint, label_text("", 11.5, UI_TEXT)));
            });
        });

    // ---- Floating toolbar that follows the selected part
    commands
        .spawn((
            EditorEntity,
            SelectionToolbar,
            GlobalZIndex(5),
            Node {
                position_type: PositionType::Absolute,
                display: Display::None,
                left: px(0.0),
                top: px(0.0),
                min_width: px(TOOLBAR_W),
                padding: UiRect::all(px(5.0)),
                column_gap: px(3.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(CornerRadius::circular(px(6.0))),
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.06, 0.11, 0.95)),
            uniform_border(Color::srgba(0.28, 0.92, 1.0, 0.80)),
        ))
        .with_children(|bar| {
            let tools = [
                ("Rot+", "Rotate +15 deg [Q]  (Shift+Q: 90 deg)", AttachAction::RotateLeft),
                ("Rot-", "Rotate -15 deg [E]  (Shift+E: 90 deg)", AttachAction::RotateRight),
                ("Flip", "Mirror the part left/right [F]", AttachAction::FlipMirror),
                ("Copy", "Clone this part and everything attached below it [Ctrl+D]", AttachAction::DuplicateSelected),
                ("CF", "Toggle fuel crossfeed through this decoupler", AttachAction::ToggleCrossfeed),
                ("S-", "Move to an earlier stage [ [ ]", AttachAction::StageDown),
                ("S+", "Move to a later stage [ ] ]", AttachAction::StageUp),
                ("Del", "Delete part and its attachments [Del] (right-click also deletes)", AttachAction::DeleteSelected),
            ];
            for (label, tip, action) in tools {
                let mut b = text_button(bar, label, tip, Some(38.0));
                bind_action(&mut b, action);
                if label == "CF" {
                    b.insert(CrossfeedToolButton);
                }
                if label == "Del" {
                    b.insert(uniform_border(Color::srgba(0.92, 0.34, 0.34, 0.7)));
                }
            }
        });

    // ---- Right panel: assembly readout, stage stack, craft library, launch
    commands
        .spawn((
            EditorEntity,
            Node {
                position_type: PositionType::Absolute,
                right: px(12.0),
                top: px(12.0),
                bottom: px(12.0),
                width: px(INSPECTOR_PANEL_WIDTH),
                padding: UiRect::all(px(11.0)),
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
            parent.spawn(label_text("KEPLER YARD - ASSEMBLY", 14.0, UI_ACCENT));

            parent
                .spawn((
                    ScrollArea,
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(6.0),
                        overflow: Overflow::scroll_y(),
                        flex_grow: 1.0,
                        min_height: px(0.0),
                        ..default()
                    },
                ))
                .with_children(|body| {
                    body.spawn((
                        EditorText::Readout,
                        Text::new("Analyzing blueprint..."),
                        TextFont::from_font_size(11.5),
                        TextColor(UI_TEXT),
                    ));

                    // Craft Library Drawer (presets + saved crafts)
                    body.spawn((
                        EditorCraftLibraryBox,
                        Node {
                            display: Display::None,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(4.0),
                            padding: UiRect::all(px(6.0)),
                            border: UiRect::all(px(1.0)),
                            border_radius: BorderRadius::all(CornerRadius::circular(px(4.0))),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.06, 0.10, 0.17, 0.96)),
                        uniform_border(Color::srgba(0.32, 0.72, 0.92, 0.5)),
                    ))
                    .with_children(|drawer| {
                        drawer.spawn(label_text("CRAFT LIBRARY", 11.0, UI_ACCENT));
                        drawer
                            .spawn(Node {
                                flex_direction: FlexDirection::Row,
                                column_gap: px(4.0),
                                ..default()
                            })
                            .with_children(|row| {
                                for (label, tip, action) in [
                                    ("Preset: Selene", "Load the lunar-capable Selene Explorer III", AttachAction::LoadLunarPreset),
                                    ("Preset: Kepler-I", "Load the two-stage Kepler-I orbital rocket", AttachAction::ResetStarter),
                                ] {
                                    let mut b = text_button(row, label, tip, Some(150.0));
                                    bind_action(&mut b, action);
                                    if let Some(h) = icons.ui_icons.get(&UiIconKind::RocketPreset) {
                                        let handle = h.clone();
                                        b.with_children(|c| spawn_icon(c, handle, 14.0));
                                    }
                                }
                            });
                        drawer.spawn(label_text("SAVED CRAFTS", 11.0, UI_ACCENT));
                        drawer.spawn((
                            EditorCraftList,
                            ScrollArea,
                            Node {
                                max_height: px(140.0),
                                flex_direction: FlexDirection::Column,
                                row_gap: px(4.0),
                                overflow: Overflow::scroll_y(),
                                ..default()
                            },
                        ));
                    });

                    body.spawn(label_text(
                        "STAGE STACK (click to select, ^/v to shift stage)",
                        11.0,
                        UI_ACCENT,
                    ));

                    body.spawn((
                        EditorStageList,
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(3.0),
                            ..default()
                        },
                    ));
                });

            // Pinned Launch Button at bottom
            let mut launch = parent.spawn((
                Button,
                Hovered::default(),
                LaunchButtonMarker,
                Tooltip(String::from("Roll out to the launch pad [Enter]")),
                Node {
                    padding: UiRect::axes(px(10.0), px(10.0)),
                    column_gap: px(6.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(CornerRadius::circular(px(5.0))),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.10, 0.45, 0.30, 0.96)),
                uniform_border(Color::srgba(0.28, 0.96, 0.62, 0.9)),
            ));
            bind_action(&mut launch, AttachAction::Launch);
            launch.with_children(|btn| {
                if let Some(handle) = icons.ui_icons.get(&UiIconKind::Launch) {
                    spawn_icon(btn, handle.clone(), 16.0);
                }
                btn.spawn(label_text("LAUNCH TO PAD [Enter]", 13.0, UI_TEXT));
            });
        });
}

fn spawn_category_tab(
    tabs: &mut ChildSpawnerCommands<'_>,
    icons: &PartIconRegistry,
    category: Option<PartCategory>,
    label: &str,
    icon_kind: UiIconKind,
) {
    tabs.spawn((
        Button,
        Hovered::default(),
        CategoryTabButton(category),
        Node {
            padding: UiRect::axes(px(6.0), px(4.0)),
            column_gap: px(4.0),
            align_items: AlignItems::Center,
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(CornerRadius::circular(px(4.0))),
            ..default()
        },
        BackgroundColor(Color::srgba(0.08, 0.14, 0.22, 0.92)),
        uniform_border(Color::srgba(0.24, 0.52, 0.72, 0.5)),
    ))
    .observe(
        move |_act: On<'_, '_, Activate>, mut ed_state: ResMut<'_, EditorState>| {
            ed_state.selected_category = category;
            ed_state.palette_dirty = true;
        },
    )
    .with_children(|btn| {
        if let Some(handle) = icons.ui_icons.get(&icon_kind) {
            btn.spawn((
                ImageNode::new(handle.clone()),
                Pickable::IGNORE,
                Node {
                    width: px(14.0),
                    height: px(14.0),
                    min_width: px(14.0),
                    min_height: px(14.0),
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
        }
        btn.spawn((
            Text::new(label),
            TextFont::from_font_size(10.0),
            TextColor(Color::srgb(0.88, 0.96, 1.0)),
            Pickable::IGNORE,
        ));
    });
}

fn short_name(name: &str) -> String {
    const MAX: usize = 13;
    if name.chars().count() <= MAX {
        name.to_owned()
    } else {
        let cut: String = name.chars().take(MAX - 1).collect();
        format!("{cut}.")
    }
}

fn rebuild_palette_grid_if_dirty(
    mut commands: Commands<'_, '_>,
    mut ed_state: ResMut<'_, EditorState>,
    catalog: Res<'_, LivePartCatalog>,
    icons: Res<'_, PartIconRegistry>,
    grid_q: Query<'_, '_, Entity, With<EditorPaletteGrid>>,
    children_q: Query<'_, '_, &Children>,
) {
    if !ed_state.palette_dirty && !catalog.is_changed() && !icons.is_changed() {
        return;
    }
    let Ok(grid_entity) = grid_q.single() else {
        return;
    };
    ed_state.palette_dirty = false;

    if let Ok(children) = children_q.get(grid_entity) {
        for &child in children {
            commands.entity(child).despawn();
        }
    }

    commands.entity(grid_entity).with_children(|grid| {
        for spec in catalog.sim_catalog.iter() {
            if let Some(filter) = ed_state.selected_category
                && spec.category != filter
            {
                continue;
            }
            let spec_id = spec.id.clone();
            let badge = format!("{:.0} kg", spec.full_mass_kg());

            grid.spawn((
                Button,
                Hovered::default(),
                PaletteButton(spec.id.clone()),
                Tooltip(part_summary(spec)),
                Node {
                    width: px(84.0),
                    height: px(100.0),
                    padding: UiRect::all(px(4.0)),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(CornerRadius::circular(px(5.0))),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.08, 0.14, 0.22, 0.92)),
                uniform_border(Color::srgba(0.24, 0.52, 0.72, 0.5)),
            ))
            .observe(
                move |_act: On<'_, '_, Activate>, mut active_bp: ResMut<'_, ActiveBlueprint>| {
                    active_bp.active_palette_spec.clone_from(&spec_id);
                },
            )
            .with_children(|tile| {
                if let Some(icon_h) = icons.part_icons.get(&spec.id) {
                    spawn_icon(tile, icon_h.clone(), 52.0);
                }
                tile.spawn(label_text(
                    short_name(&spec.display_name),
                    9.5,
                    Color::srgb(0.88, 0.96, 1.0),
                ));
                tile.spawn(label_text(badge, 9.5, Color::srgba(0.76, 0.92, 1.0, 0.75)));
            });
        }
    });
}

#[allow(clippy::too_many_lines)]
fn rebuild_stage_and_craft_lists_if_dirty(
    mut commands: Commands<'_, '_>,
    mut ed_state: ResMut<'_, EditorState>,
    active_bp: Res<'_, ActiveBlueprint>,
    catalog: Res<'_, LivePartCatalog>,
    mut lists: EditorUiLists<'_, '_>,
) {
    if let Ok(mut cbox) = lists.craft_box_q.single_mut() {
        cbox.display = if ed_state.crafts_drawer_open {
            Display::Flex
        } else {
            Display::None
        };
    }

    if !ed_state.lists_dirty && !active_bp.is_changed() {
        return;
    }
    ed_state.lists_dirty = false;

    if let Ok(stage_root) = lists.stage_list_q.single() {
        if let Ok(children) = lists.children_q.get(stage_root) {
            for &c in children {
                commands.entity(c).despawn();
            }
        }
        let mut by_stage: BTreeMap<u32, Vec<(u64, String, bool)>> = BTreeMap::new();
        for p in &active_bp.blueprint.parts {
            let name = catalog
                .sim_catalog
                .get(&p.spec_id)
                .map_or(p.spec_id.as_str(), |s| s.display_name.as_str());
            let is_dec = catalog
                .sim_catalog
                .get(&p.spec_id)
                .is_some_and(|s| matches!(s.kind, PartKind::Decoupler(_)));
            let label = if is_dec && p.crossfeed {
                format!("{name} [CF]")
            } else {
                name.to_owned()
            };
            by_stage.entry(p.stage).or_default().push((
                p.instance_id,
                label,
                active_bp.selected_part_id == Some(p.instance_id),
            ));
        }

        commands.entity(stage_root).with_children(|col| {
            for (st, items) in by_stage {
                col.spawn((
                    Text::new(format!("Stage S{}", st + 1)),
                    TextFont::from_font_size(11.0),
                    TextColor(Color::srgb(0.95, 0.80, 0.25)),
                ));
                for (pid, label, selected) in items {
                    let row_border = if selected {
                        Color::srgba(0.28, 0.92, 1.0, 0.95)
                    } else {
                        Color::srgba(0.20, 0.38, 0.54, 0.45)
                    };
                    col.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                        column_gap: px(4.0),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Button,
                            Hovered::default(),
                            Node {
                                flex_grow: 1.0,
                                padding: UiRect::axes(px(6.0), px(3.0)),
                                border: UiRect::all(px(1.0)),
                                border_radius: BorderRadius::all(CornerRadius::circular(px(3.0))),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.08, 0.14, 0.22, 0.90)),
                            uniform_border(row_border),
                        ))
                        .observe(
                            move |_act: On<'_, '_, Activate>,
                                  mut active_bp: ResMut<'_, ActiveBlueprint>,
                                  mut ed_state: ResMut<'_, EditorState>| {
                                active_bp.selected_part_id = Some(pid);
                                active_bp.dirty = true;
                                ed_state.lists_dirty = true;
                            },
                        )
                        .with_children(|btn| {
                            btn.spawn((
                                Text::new(label),
                                TextFont::from_font_size(10.5),
                                TextColor(Color::srgb(0.86, 0.95, 1.0)),
                                Pickable::IGNORE,
                            ));
                        });

                        for (btn_lbl, delta) in [("v", -1_i32), ("^", 1_i32)] {
                            row.spawn((
                                Button,
                                Hovered::default(),
                                Node {
                                    width: px(22.0),
                                    padding: UiRect::axes(px(4.0), px(2.0)),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    border: UiRect::all(px(1.0)),
                                    border_radius: BorderRadius::all(CornerRadius::circular(px(
                                        3.0,
                                    ))),
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(0.08, 0.14, 0.22, 0.90)),
                                uniform_border(Color::srgba(0.24, 0.52, 0.72, 0.45)),
                            ))
                            .observe(
                                move |_act: On<'_, '_, Activate>,
                                      mut active_bp: ResMut<'_, ActiveBlueprint>,
                                      mut ed_state: ResMut<'_, EditorState>| {
                                    ed_state.record_undo(&active_bp.blueprint);
                                    shift_part_stage(
                                        &mut active_bp.blueprint,
                                        pid,
                                        delta,
                                        ed_state.symmetry_2x,
                                    );
                                    active_bp.dirty = true;
                                    ed_state.lists_dirty = true;
                                },
                            )
                            .with_children(|b| {
                                b.spawn((
                                    Text::new(btn_lbl),
                                    TextFont::from_font_size(10.0),
                                    TextColor(Color::srgb(0.86, 0.95, 1.0)),
                                    Pickable::IGNORE,
                                ));
                            });
                        }
                    });
                }
            }
        });
    }

    if ed_state.crafts_drawer_open
        && let Ok(craft_root) = lists.craft_list_q.single()
    {
        if let Ok(children) = lists.children_q.get(craft_root) {
            for &c in children {
                commands.entity(c).despawn();
            }
        }
        let entries = list_saved_crafts().unwrap_or_default();
        commands.entity(craft_root).with_children(|col| {
            if entries.is_empty() {
                col.spawn((
                    Text::new("No saved crafts yet."),
                    TextFont::from_font_size(10.5),
                    TextColor(Color::srgba(0.7, 0.8, 0.9, 0.8)),
                ));
            }
            for entry in entries {
                let load_path = entry.path.clone();
                let del_path = entry.path.clone();
                col.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(4.0),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Button,
                        Hovered::default(),
                        Node {
                            flex_grow: 1.0,
                            padding: UiRect::axes(px(6.0), px(3.0)),
                            border: UiRect::all(px(1.0)),
                            border_radius: BorderRadius::all(CornerRadius::circular(px(3.0))),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.08, 0.14, 0.22, 0.92)),
                        uniform_border(Color::srgba(0.24, 0.52, 0.72, 0.5)),
                    ))
                    .observe(
                        move |_act: On<'_, '_, Activate>,
                              mut active_bp: ResMut<'_, ActiveBlueprint>,
                              mut ed_state: ResMut<'_, EditorState>| {
                            match load_craft_from_path(&load_path) {
                                Ok(loaded) => {
                                    ed_state.record_undo(&active_bp.blueprint);
                                    active_bp.blueprint = loaded;
                                    active_bp.selected_part_id =
                                        active_bp.blueprint.parts.first().map(|p| p.instance_id);
                                    active_bp.dirty = true;
                                    ed_state.lists_dirty = true;
                                    ed_state.status_note =
                                        format!("Loaded '{}'.", active_bp.blueprint.name);
                                }
                                Err(err) => ed_state.status_note = err,
                            }
                        },
                    )
                    .with_children(|b| {
                        b.spawn((
                            Text::new(format!("{} ({}p)", entry.name, entry.part_count)),
                            TextFont::from_font_size(10.5),
                            TextColor(Color::srgb(0.88, 0.96, 1.0)),
                            Pickable::IGNORE,
                        ));
                    });

                    row.spawn((
                        Button,
                        Hovered::default(),
                        Node {
                            width: px(24.0),
                            padding: UiRect::axes(px(4.0), px(3.0)),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border: UiRect::all(px(1.0)),
                            border_radius: BorderRadius::all(CornerRadius::circular(px(3.0))),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.32, 0.10, 0.12, 0.92)),
                        uniform_border(Color::srgba(0.92, 0.34, 0.34, 0.6)),
                    ))
                    .observe(
                        move |_act: On<'_, '_, Activate>, mut ed_state: ResMut<'_, EditorState>| {
                            if let Err(err) = delete_saved_craft(&del_path) {
                                ed_state.status_note = err;
                            } else {
                                ed_state.lists_dirty = true;
                                ed_state.status_note = String::from("Deleted saved craft.");
                            }
                        },
                    )
                    .with_children(|b| {
                        b.spawn((
                            Text::new("X"),
                            TextFont::from_font_size(10.0),
                            TextColor(Color::srgb(1.0, 0.78, 0.78)),
                            Pickable::IGNORE,
                        ));
                    });
                });
            }
        });
    }
}

const fn button_bg_for_state(pressed: bool, hovered: bool, is_launch: bool) -> BackgroundColor {
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

fn style_buttons_on_change(mut buttons: ButtonStyleQuery<'_, '_>) {
    for (hovered, pressed, is_launch, mut bg) in &mut buttons {
        *bg = button_bg_for_state(pressed, hovered.get(), is_launch);
    }
}

fn style_buttons_on_release(
    mut removed_pressed: RemovedComponents<'_, '_, Pressed>,
    mut buttons: Query<
        '_,
        '_,
        (&Hovered, Has<LaunchButtonMarker>, &mut BackgroundColor),
        With<Button>,
    >,
) {
    for entity in removed_pressed.read() {
        if let Ok((hovered, is_launch, mut bg)) = buttons.get_mut(entity) {
            *bg = button_bg_for_state(false, hovered.get(), is_launch);
        }
    }
}

fn rebuild_editor_meshes_if_dirty(
    mut commands: Commands<'_, '_>,
    mut active_bp: ResMut<'_, ActiveBlueprint>,
    mut ed_state: ResMut<'_, EditorState>,
    catalog: Res<'_, LivePartCatalog>,
    existing: Query<'_, '_, Entity, With<EditorPartNode>>,
    mut assets: EditorAssets<'_>,
) {
    if !active_bp.dirty {
        return;
    }
    active_bp.dirty = false;
    ed_state.lists_dirty = true;

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
        let mesh = get_or_build_mesh(&mut assets, &part.spec_id, &spec.shape, part.mirrored);
        let material = assets.materials.add(PartMaterial {
            base_color: Vec4::from_array(style.base_color),
            accent_color: Vec4::from_array(style.accent_color),
            flags: Vec4::new(
                style.emissive_strength,
                if selected { 1.0 } else { 0.0 },
                1.0,
                0.0,
            ),
            thermal: Vec4::new(0.0, 1.0, part_style_id(spec), 0.0),
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

fn handle_palette_press_for_drag(
    pressed_q: PalettePressQuery<'_, '_>,
    windows: Query<'_, '_, &Window>,
    mut active_bp: ResMut<'_, ActiveBlueprint>,
    mut ed_state: ResMut<'_, EditorState>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    for pbtn in &pressed_q {
        active_bp.active_palette_spec.clone_from(&pbtn.0);
        ed_state.drag = DragPhase::PendingPalette {
            spec_id: pbtn.0.clone(),
            start_cursor: cursor,
        };
    }
}

#[allow(clippy::too_many_lines)]
fn handle_editor_canvas_drag_and_drop(
    mut commands: Commands<'_, '_>,
    ctx: EditorContext<'_, '_>,
    mut zoom: ResMut<'_, CameraZoomState>,
    mut active_bp: ResMut<'_, ActiveBlueprint>,
    mut ed_state: ResMut<'_, EditorState>,
    mut node_q: Query<'_, '_, (&EditorPartNode, &mut Transform)>,
    ghost_q: Query<'_, '_, Entity, With<EditorDragGhost>>,
    mut assets: EditorAssets<'_>,
) {
    let mouse_buttons = &ctx.mouse_buttons;
    let keys = &ctx.keys;
    let Ok(window) = ctx.windows.single() else {
        return;
    };
    let Ok((camera, cam_xform)) = ctx.cam_q.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        ed_state.hover_part = None;
        return;
    };
    let over_ui = is_pointer_over_interactive_ui(&ctx.hover_map, &ctx.ui_nodes);
    let in_canvas = !over_ui
        && cursor.x > PALETTE_PANEL_WIDTH + 18.0
        && cursor.x < window.width() - INSPECTOR_PANEL_WIDTH - 18.0;
    let Ok(world_pos) = camera.viewport_to_world_2d(cam_xform, cursor) else {
        return;
    };
    let world_d = DVec2::new(f64::from(world_pos.x), f64::from(world_pos.y));
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let bypass_snap = shift || !ed_state.snap_nodes;
    let carrying = matches!(ed_state.drag, DragPhase::Carrying { .. });
    // The snap capture radius is constant on screen, so it feels the same at every zoom level.
    let snap_radius = (SNAP_RADIUS_PX * f64::from(zoom.log_scale.exp())).clamp(0.6, 2.4);

    ed_state.hover_part = if in_canvas {
        pick_part_at(&active_bp, &ctx.catalog, world_d)
    } else {
        None
    };

    // Escape drops whatever is in hand.
    if carrying && keys.just_pressed(KeyCode::Escape) {
        ed_state.drag = DragPhase::Idle;
        ed_state.snap_preview = None;
        ed_state.status_note = String::from("Placement cancelled.");
        despawn_ghosts(&mut commands, &ghost_q);
        return;
    }

    // Right button: cancel carrying, or (as a click without dragging) delete the part under it.
    if mouse_buttons.just_pressed(MouseButton::Right) {
        if carrying {
            ed_state.drag = DragPhase::Idle;
            ed_state.snap_preview = None;
            ed_state.right_press = None;
            ed_state.status_note = String::from("Placement cancelled.");
            despawn_ghosts(&mut commands, &ghost_q);
            return;
        }
        ed_state.right_press = in_canvas.then_some(cursor);
    }
    if mouse_buttons.just_released(MouseButton::Right)
        && let Some(start) = ed_state.right_press.take()
        && cursor.distance(start) < CLICK_SLOP_PX
        && matches!(ed_state.drag, DragPhase::Idle)
        && let Some(hit) = ed_state.hover_part
    {
        active_bp.selected_part_id = Some(hit);
        delete_selected(&mut active_bp, &mut ed_state);
    }

    if mouse_buttons.just_pressed(MouseButton::Left) && in_canvas && !carrying {
        if let Some(hit_id) = ed_state.hover_part {
            active_bp.selected_part_id = Some(hit_id);
            ed_state.lists_dirty = true;
            let alt_clone = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
            let ctrl_single =
                keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
            ed_state.drag = DragPhase::PendingExisting {
                part_id: hit_id,
                start_cursor: cursor,
                start_world: world_d,
                alt_clone,
                ctrl_single,
            };
        } else if matches!(ed_state.drag, DragPhase::Idle) {
            ed_state.drag = DragPhase::PanningCanvas {
                start_cursor: cursor,
                start_pan: zoom.editor_pan,
                moved: false,
            };
        }
    }

    let phase = ed_state.drag.clone();
    match phase {
        DragPhase::Idle => {
            ed_state.snap_preview = None;
            despawn_ghosts(&mut commands, &ghost_q);
        }
        DragPhase::PanningCanvas {
            start_cursor,
            start_pan,
            moved,
        } => {
            if mouse_buttons.pressed(MouseButton::Left) {
                let delta = cursor - start_cursor;
                let moved_now = moved || delta.length() > DRAG_THRESHOLD_PX;
                if moved_now {
                    let scale = zoom.log_scale.exp();
                    zoom.editor_pan = (start_pan + Vec2::new(-delta.x * scale, delta.y * scale))
                        .clamp(Vec2::splat(-60.0), Vec2::splat(60.0));
                }
                ed_state.drag = DragPhase::PanningCanvas {
                    start_cursor,
                    start_pan,
                    moved: moved_now,
                };
            } else {
                if !moved {
                    active_bp.selected_part_id = None;
                    ed_state.lists_dirty = true;
                }
                ed_state.drag = DragPhase::Idle;
            }
        }
        DragPhase::PendingPalette {
            spec_id,
            start_cursor,
        } => {
            if !mouse_buttons.pressed(MouseButton::Left) {
                // A plain click on a catalog tile picks the part up; click the bay to place it.
                let name = ctx
                    .catalog
                    .sim_catalog
                    .get(&spec_id)
                    .map_or_else(|| spec_id.clone(), |s| s.display_name.clone());
                ed_state.status_note =
                    format!("Carrying {name}. Click the bay to place it (Shift: keep placing).");
                ed_state.drag = DragPhase::Carrying { spec_id };
                ed_state.snap_preview = None;
            } else if cursor.distance(start_cursor) > DRAG_THRESHOLD_PX {
                ed_state.drag = DragPhase::DraggingNew { spec_id };
            }
        }
        DragPhase::DraggingNew { spec_id } | DragPhase::Carrying { spec_id } => {
            let show_ghost = !carrying || in_canvas;
            let snap = if bypass_snap || !show_ghost {
                None
            } else {
                find_best_snap_target(
                    &active_bp,
                    &ctx.catalog,
                    &spec_id,
                    world_d,
                    None,
                    snap_radius,
                )
            };
            let free_target = find_free_target(&active_bp, world_d);
            let preview_pos = snap
                .as_ref()
                .map_or_else(|| snap_to_grid(world_d), |s| s.local_pos);
            let preview_rot = snap.as_ref().map_or(0.0, |s| s.local_rot);
            let mirrored = snap
                .as_ref()
                .is_some_and(|s| s.attach_kind == AttachKind::RadialRight);
            let style_id = ctx
                .catalog
                .sim_catalog
                .get(&spec_id)
                .map_or(0.0, part_style_id);
            ed_state.snap_preview = snap.as_ref().map(|s| s.local_pos);

            let place_now = if carrying {
                mouse_buttons.just_pressed(MouseButton::Left) && in_canvas
            } else {
                mouse_buttons.just_released(MouseButton::Left)
            };

            if place_now {
                if in_canvas && let Some(target) = snap.or(free_target) {
                    ed_state.record_undo(&active_bp.blueprint);
                    let new_id = attach_with_optional_symmetry(
                        &mut active_bp.blueprint,
                        &ctx.catalog,
                        &target,
                        &spec_id,
                        ed_state.symmetry_2x,
                    );
                    active_bp.selected_part_id = Some(new_id);
                    active_bp.dirty = true;
                    ed_state.status_note = format!("Placed {spec_id}.");
                }
                ed_state.drag = if carrying && shift {
                    DragPhase::Carrying { spec_id }
                } else {
                    DragPhase::Idle
                };
                ed_state.snap_preview = None;
                despawn_ghosts(&mut commands, &ghost_q);
            } else {
                despawn_ghosts(&mut commands, &ghost_q);
                if show_ghost {
                    spawn_drag_ghost(
                        &mut commands,
                        &ctx.catalog,
                        &spec_id,
                        DragGhostPose {
                            pos: preview_pos,
                            rot: preview_rot,
                            mirrored,
                            snapped: snap.is_some(),
                            style_id,
                        },
                        &mut assets,
                    );
                }
            }
        }
        DragPhase::PendingExisting {
            part_id,
            start_cursor,
            start_world,
            alt_clone,
            ctrl_single,
        } => {
            if !mouse_buttons.pressed(MouseButton::Left) {
                ed_state.drag = DragPhase::Idle;
                ed_state.snap_preview = None;
            } else if cursor.distance(start_cursor) > DRAG_THRESHOLD_PX {
                let is_root = active_bp
                    .blueprint
                    .parts
                    .iter()
                    .find(|p| p.instance_id == part_id)
                    .is_some_and(|p| p.parent_id.is_none());
                ed_state.record_undo(&active_bp.blueprint);
                let active_id = if alt_clone && !is_root {
                    if let Some(cloned_id) = active_bp
                        .blueprint
                        .duplicate_subtree(part_id, DVec2::new(0.2, 0.2))
                    {
                        active_bp.selected_part_id = Some(cloned_id);
                        active_bp.dirty = true;
                        cloned_id
                    } else {
                        part_id
                    }
                } else {
                    if ctrl_single && !is_root {
                        active_bp.blueprint.reparent_children_to_parent(part_id);
                    }
                    part_id
                };
                ed_state.drag = DragPhase::DraggingExisting {
                    part_id: active_id,
                    last_world: start_world,
                    is_root,
                };
            }
        }
        DragPhase::DraggingExisting {
            part_id,
            last_world,
            is_root,
        } => {
            let delta = world_d - last_world;
            if delta.length_squared() > 1.0e-8 {
                active_bp.blueprint.translate_subtree(part_id, delta);
                for (node, mut tf) in &mut node_q {
                    if (is_root || active_bp.blueprint.is_in_subtree(part_id, node.instance_id))
                        && let Some(p) = active_bp
                            .blueprint
                            .parts
                            .iter()
                            .find(|p| p.instance_id == node.instance_id)
                    {
                        tf.translation.x = p.local_pos.x as f32;
                        tf.translation.y = p.local_pos.y as f32;
                    }
                }
            }

            let spec_id = active_bp
                .blueprint
                .parts
                .iter()
                .find(|p| p.instance_id == part_id)
                .map_or_else(String::new, |p| p.spec_id.clone());

            let curr_pos = active_bp
                .blueprint
                .parts
                .iter()
                .find(|p| p.instance_id == part_id)
                .map_or(world_d, |p| p.local_pos);
            let snap = if is_root || bypass_snap {
                None
            } else {
                find_best_snap_target(
                    &active_bp,
                    &ctx.catalog,
                    &spec_id,
                    curr_pos,
                    Some(part_id),
                    snap_radius,
                )
            };
            ed_state.snap_preview = snap.as_ref().map(|s| s.local_pos);

            if mouse_buttons.just_released(MouseButton::Left) {
                if let Some(target) = snap {
                    let shift_vec = target.local_pos - curr_pos;
                    active_bp.blueprint.translate_subtree(part_id, shift_vec);
                    if let Some(p) = active_bp
                        .blueprint
                        .parts
                        .iter_mut()
                        .find(|p| p.instance_id == part_id)
                    {
                        p.parent_id = Some(target.parent_id);
                        p.attach_kind = target.attach_kind;
                        p.local_rot = target.local_rot;
                        p.mirrored = target.attach_kind == AttachKind::RadialRight;
                    }
                    active_bp.dirty = true;
                    ed_state.status_note = String::from("Snapped moved part cleanly.");
                } else {
                    let snapped = snap_to_grid(curr_pos);
                    active_bp
                        .blueprint
                        .translate_subtree(part_id, snapped - curr_pos);
                    active_bp.dirty = true;
                    ed_state.status_note = String::from("Placed moved part on grid.");
                }
                ed_state.drag = DragPhase::Idle;
                ed_state.snap_preview = None;
            } else {
                ed_state.drag = DragPhase::DraggingExisting {
                    part_id,
                    last_world: world_d,
                    is_root,
                };
            }
        }
    }
}

fn find_free_target(bp: &ActiveBlueprint, cursor_m: DVec2) -> Option<SnapTarget> {
    let pos = snap_to_grid(cursor_m);
    let nearest = bp.blueprint.parts.iter().min_by(|a, b| {
        a.local_pos
            .distance(pos)
            .total_cmp(&b.local_pos.distance(pos))
    })?;
    let diff = pos - nearest.local_pos;
    let attach_kind = if diff.x.abs() < 0.35 {
        if diff.y >= 0.0 {
            AttachKind::StackTop
        } else {
            AttachKind::StackBottom
        }
    } else if diff.x < 0.0 {
        AttachKind::RadialLeft
    } else {
        AttachKind::RadialRight
    };
    Some(SnapTarget {
        parent_id: nearest.instance_id,
        attach_kind,
        local_pos: pos,
        local_rot: 0.0,
        stage: nearest.stage,
    })
}

fn attach_with_optional_symmetry(
    bp: &mut VesselBlueprint,
    catalog: &LivePartCatalog,
    target: &SnapTarget,
    spec_id: &str,
    symmetry_2x: bool,
) -> u64 {
    let default_cf = catalog
        .sim_catalog
        .get(spec_id)
        .is_some_and(|s| matches!(s.kind, PartKind::Decoupler(d) if d.crossfeed));
    let new_id = bp.attach_child(
        target.parent_id,
        spec_id.to_owned(),
        target.attach_kind,
        target.local_pos,
        target.local_rot,
        target.stage,
    );
    bp.set_part_crossfeed(new_id, default_cf);

    if !symmetry_2x {
        return new_id;
    }

    let is_radial = matches!(
        target.attach_kind,
        AttachKind::RadialLeft | AttachKind::RadialRight
    );
    if is_radial {
        if let Some(parent) = bp
            .parts
            .iter()
            .find(|p| p.instance_id == target.parent_id)
            .cloned()
            && parent.local_pos.x.abs() < 0.08
        {
            let opp_kind = if target.attach_kind == AttachKind::RadialLeft {
                AttachKind::RadialRight
            } else {
                AttachKind::RadialLeft
            };
            let opp_pos = DVec2::new(
                2.0 * parent.local_pos.x - target.local_pos.x,
                target.local_pos.y,
            );
            let twin_id = bp.attach_child(
                target.parent_id,
                spec_id.to_owned(),
                opp_kind,
                snap_to_grid(opp_pos),
                -target.local_rot,
                target.stage,
            );
            bp.set_part_crossfeed(twin_id, default_cf);
        } else if let Some(twin_parent_id) = bp.find_mirror_twin(target.parent_id) {
            let opp_kind = if target.attach_kind == AttachKind::RadialLeft {
                AttachKind::RadialRight
            } else {
                AttachKind::RadialLeft
            };
            let opp_pos = DVec2::new(-target.local_pos.x, target.local_pos.y);
            let twin_id = bp.attach_child(
                twin_parent_id,
                spec_id.to_owned(),
                opp_kind,
                snap_to_grid(opp_pos),
                -target.local_rot,
                target.stage,
            );
            bp.set_part_crossfeed(twin_id, default_cf);
        }
    } else if let Some(twin_parent_id) = bp.find_mirror_twin(target.parent_id) {
        let opp_pos = DVec2::new(-target.local_pos.x, target.local_pos.y);
        let twin_id = bp.attach_child(
            twin_parent_id,
            spec_id.to_owned(),
            target.attach_kind,
            snap_to_grid(opp_pos),
            -target.local_rot,
            target.stage,
        );
        bp.set_part_crossfeed(twin_id, default_cf);
    }

    new_id
}

fn pick_part_at(bp: &ActiveBlueprint, catalog: &LivePartCatalog, world_d: DVec2) -> Option<u64> {
    let mut best: Option<(u64, f64)> = None;
    for part in &bp.blueprint.parts {
        let Some(spec) = catalog.sim_catalog.get(&part.spec_id) else {
            continue;
        };
        let half = spec.shape.contact_half_extent();
        let rel = (world_d - part.local_pos).rotate(-part.local_rot);
        if rel.x.abs() <= half.x + 0.15 && rel.y.abs() <= half.y + 0.15 {
            let d2 = rel.length_squared();
            if best.is_none_or(|(_, bd2)| d2 < bd2) {
                best = Some((part.instance_id, d2));
            }
        }
    }
    best.map(|(id, _)| id)
}

fn find_best_snap_target(
    bp: &ActiveBlueprint,
    catalog: &LivePartCatalog,
    new_spec_id: &str,
    cursor_m: DVec2,
    moving_subtree_root: Option<u64>,
    snap_radius: f64,
) -> Option<SnapTarget> {
    let new_spec = catalog.sim_catalog.get(new_spec_id)?;
    let mut best: Option<(f64, SnapTarget)> = None;

    for parent in &bp.blueprint.parts {
        if let Some(root_id) = moving_subtree_root
            && bp.blueprint.is_in_subtree(root_id, parent.instance_id)
        {
            continue;
        }
        let Some(pspec) = catalog.sim_catalog.get(&parent.spec_id) else {
            continue;
        };

        let mut candidates: Vec<SnapTarget> = Vec::with_capacity(4);
        if pspec.stack_bottom && new_spec.stack_top {
            let y =
                parent.local_pos.y + pspec.shape.bottom_offset().y - new_spec.shape.top_offset().y;
            candidates.push(SnapTarget {
                parent_id: parent.instance_id,
                attach_kind: AttachKind::StackBottom,
                local_pos: snap_to_grid(DVec2::new(parent.local_pos.x, y)),
                local_rot: 0.0,
                stage: parent.stage,
            });
        }
        if pspec.stack_top && new_spec.stack_bottom {
            let y =
                parent.local_pos.y + pspec.shape.top_offset().y - new_spec.shape.bottom_offset().y;
            candidates.push(SnapTarget {
                parent_id: parent.instance_id,
                attach_kind: AttachKind::StackTop,
                local_pos: snap_to_grid(DVec2::new(parent.local_pos.x, y)),
                local_rot: 0.0,
                stage: parent.stage,
            });
        }

        let is_radial_pylon = matches!(pspec.kind, PartKind::Decoupler(_)) && pspec.radial_attach;
        if is_radial_pylon
            && (new_spec.stack_top || new_spec.stack_bottom || new_spec.radial_attach)
        {
            let dx = 0.5 * pspec.shape.bounding_size().x + 0.5 * new_spec.shape.bounding_size().x;
            let (kind, sign) =
                if parent.attach_kind == AttachKind::RadialRight || parent.local_pos.x > 0.0 {
                    (AttachKind::RadialRight, 1.0)
                } else {
                    (AttachKind::RadialLeft, -1.0)
                };
            candidates.push(SnapTarget {
                parent_id: parent.instance_id,
                attach_kind: kind,
                local_pos: snap_to_grid(DVec2::new(
                    parent.local_pos.x + sign * dx,
                    parent.local_pos.y,
                )),
                local_rot: 0.0,
                stage: parent.stage,
            });
        } else if new_spec.radial_attach && (pspec.stack_top || pspec.stack_bottom) {
            let dx = 0.5 * pspec.shape.bounding_size().x + 0.5 * new_spec.shape.bounding_size().x;
            let y = cursor_m.y.clamp(
                parent.local_pos.y - 0.45 * pspec.shape.bounding_size().y,
                parent.local_pos.y + 0.45 * pspec.shape.bounding_size().y,
            );
            candidates.push(SnapTarget {
                parent_id: parent.instance_id,
                attach_kind: AttachKind::RadialLeft,
                local_pos: snap_to_grid(DVec2::new(parent.local_pos.x - dx, y)),
                local_rot: 0.0,
                stage: parent.stage,
            });
            candidates.push(SnapTarget {
                parent_id: parent.instance_id,
                attach_kind: AttachKind::RadialRight,
                local_pos: snap_to_grid(DVec2::new(parent.local_pos.x + dx, y)),
                local_rot: 0.0,
                stage: parent.stage,
            });
        }

        for cand in candidates {
            let dist = cand.local_pos.distance(cursor_m);
            if dist <= snap_radius && best.as_ref().is_none_or(|(bd, _)| dist < *bd) {
                best = Some((dist, cand));
            }
        }
    }

    best.map(|(_, t)| t)
}

fn despawn_ghosts(
    commands: &mut Commands<'_, '_>,
    ghost_q: &Query<'_, '_, Entity, With<EditorDragGhost>>,
) {
    for e in ghost_q {
        commands.entity(e).despawn();
    }
}

fn spawn_drag_ghost(
    commands: &mut Commands<'_, '_>,
    catalog: &LivePartCatalog,
    spec_id: &str,
    pose: DragGhostPose,
    assets: &mut EditorAssets<'_>,
) {
    let Some(spec) = catalog.sim_catalog.get(spec_id) else {
        return;
    };
    let style = catalog.visuals.get(spec_id).copied().unwrap_or_default();
    let mesh = get_or_build_mesh(assets, spec_id, &spec.shape, pose.mirrored);
    let mut base = Vec4::from_array(style.base_color);
    base.w = if pose.snapped { 0.80 } else { 0.52 };
    let accent = if pose.snapped {
        Vec4::new(0.25, 0.98, 0.62, 1.0)
    } else {
        Vec4::from_array(style.accent_color)
    };
    let mat = assets.materials.add(PartMaterial {
        base_color: base,
        accent_color: accent,
        flags: Vec4::new(2.0, 1.0, 1.0, 0.0),
        thermal: Vec4::new(0.0, 1.0, pose.style_id, 0.0),
    });

    commands.spawn((
        EditorEntity,
        EditorDragGhost,
        Mesh2d(mesh),
        MeshMaterial2d(mat),
        Transform::from_xyz(pose.pos.x as f32, pose.pos.y as f32, 3.0)
            .with_rotation(Quat::from_rotation_z(pose.rot as f32)),
    ));
}

fn handle_craft_rename_typing(
    mut key_msgs: MessageReader<'_, '_, KeyboardInput>,
    mut active_bp: ResMut<'_, ActiveBlueprint>,
    mut ed_state: ResMut<'_, EditorState>,
) {
    if !ed_state.renaming_craft {
        key_msgs.clear();
        return;
    }

    for ev in key_msgs.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        match ev.key_code {
            KeyCode::Enter | KeyCode::Escape => {
                ed_state.renaming_craft = false;
                if active_bp.blueprint.name.trim().is_empty() {
                    active_bp.blueprint.name = String::from("Unnamed Craft");
                }
                ed_state.status_note = format!("Craft renamed to '{}'.", active_bp.blueprint.name);
                ed_state.lists_dirty = true;
            }
            KeyCode::Backspace => {
                active_bp.blueprint.name.pop();
            }
            _ => {
                if let Some(txt) = &ev.text {
                    for ch in txt.chars() {
                        if !ch.is_control() && active_bp.blueprint.name.len() < 30 {
                            active_bp.blueprint.name.push(ch);
                        }
                    }
                }
            }
        }
    }
}

fn handle_editor_keyboard_shortcuts(
    keys: Res<'_, ButtonInput<KeyCode>>,
    catalog: Res<'_, LivePartCatalog>,
    mut active_bp: ResMut<'_, ActiveBlueprint>,
    mut ed_state: ResMut<'_, EditorState>,
    mut next_mode: ResMut<'_, NextState<GameMode>>,
) {
    if ed_state.renaming_craft {
        return;
    }

    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

    // Arrow key nudging of selected part
    let nudge_step = if shift { 0.025 } else { 0.05 };
    let mut nudge = DVec2::ZERO;
    if keys.just_pressed(KeyCode::ArrowLeft) {
        nudge.x -= nudge_step;
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        nudge.x += nudge_step;
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        nudge.y += nudge_step;
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        nudge.y -= nudge_step;
    }
    if nudge.length_squared() > 0.0
        && let Some(sel) = active_bp.selected_part_id
    {
        ed_state.record_undo(&active_bp.blueprint);
        active_bp.blueprint.translate_subtree(sel, nudge);
        if ed_state.symmetry_2x
            && let Some(twin) = active_bp.blueprint.find_mirror_twin(sel)
        {
            active_bp
                .blueprint
                .translate_subtree(twin, DVec2::new(-nudge.x, nudge.y));
        }
        active_bp.dirty = true;
        return;
    }

    if (keys.just_pressed(KeyCode::KeyQ) || keys.just_pressed(KeyCode::KeyE))
        && let Some(sel) = active_bp.selected_part_id
    {
        let deg = if shift { 90.0_f64 } else { 15.0_f64 };
        let rad = if keys.just_pressed(KeyCode::KeyQ) {
            deg.to_radians()
        } else {
            -deg.to_radians()
        };
        ed_state.record_undo(&active_bp.blueprint);
        active_bp.blueprint.rotate_part(sel, rad);
        if ed_state.symmetry_2x
            && let Some(twin) = active_bp.blueprint.find_mirror_twin(sel)
        {
            active_bp.blueprint.rotate_part(twin, -rad);
        }
        active_bp.dirty = true;
        return;
    }

    let action = if ctrl && keys.just_pressed(KeyCode::KeyZ) {
        Some(AttachAction::Undo)
    } else if ctrl && keys.just_pressed(KeyCode::KeyY) {
        Some(AttachAction::Redo)
    } else if ctrl && keys.just_pressed(KeyCode::KeyS) {
        Some(AttachAction::SaveCraft)
    } else if ctrl && keys.just_pressed(KeyCode::KeyD) {
        Some(AttachAction::DuplicateSelected)
    } else if ctrl {
        None
    } else if keys.just_pressed(KeyCode::KeyF) {
        Some(AttachAction::FlipMirror)
    } else if keys.just_pressed(KeyCode::KeyA) {
        Some(AttachAction::AutoAttach)
    } else if keys.just_pressed(KeyCode::KeyS) {
        Some(AttachAction::ToggleSnap)
    } else if keys.just_pressed(KeyCode::KeyB) {
        Some(AttachAction::AttachBottom)
    } else if keys.just_pressed(KeyCode::KeyT) {
        Some(AttachAction::AttachTop)
    } else if keys.just_pressed(KeyCode::KeyR) {
        Some(AttachAction::AttachRadialPair)
    } else if keys.just_pressed(KeyCode::KeyX) {
        Some(AttachAction::ToggleSymmetry)
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
        apply_editor_action(act, &catalog, &mut active_bp, &mut ed_state, &mut next_mode);
    }
}

fn shift_part_stage(bp: &mut VesselBlueprint, part_id: u64, delta: i32, symmetry_2x: bool) {
    let twin = if symmetry_2x {
        bp.find_mirror_twin(part_id)
    } else {
        None
    };
    for id in [Some(part_id), twin].into_iter().flatten() {
        if let Some(part) = bp.parts.iter_mut().find(|p| p.instance_id == id) {
            if delta < 0 {
                part.stage = part.stage.saturating_sub(delta.unsigned_abs());
            } else {
                part.stage = part.stage.saturating_add(delta as u32).min(9);
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
fn apply_editor_action(
    action: AttachAction,
    catalog: &LivePartCatalog,
    active_bp: &mut ActiveBlueprint,
    ed_state: &mut EditorState,
    next_mode: &mut NextState<GameMode>,
) {
    match action {
        AttachAction::AttachBottom => {
            ed_state.record_undo(&active_bp.blueprint);
            attach_to_selected(active_bp, catalog, AttachKind::StackBottom);
        }
        AttachAction::AttachTop => {
            ed_state.record_undo(&active_bp.blueprint);
            attach_to_selected(active_bp, catalog, AttachKind::StackTop);
        }
        AttachAction::AttachRadialPair => {
            ed_state.record_undo(&active_bp.blueprint);
            attach_radial_pair(active_bp, catalog, true);
        }
        AttachAction::AutoAttach => auto_attach(active_bp, catalog, ed_state),
        AttachAction::ToggleSnap => {
            ed_state.snap_nodes = !ed_state.snap_nodes;
            ed_state.status_note = if ed_state.snap_nodes {
                String::from("Node snapping ON.")
            } else {
                String::from("Node snapping OFF - parts place freely on the grid.")
            };
        }
        AttachAction::ToggleSymmetry => {
            ed_state.symmetry_2x = !ed_state.symmetry_2x;
            ed_state.status_note = if ed_state.symmetry_2x {
                String::from("2x radial symmetry enabled.")
            } else {
                String::from("1x single placement mode.")
            };
        }
        AttachAction::RotateLeft => {
            if let Some(sel) = active_bp.selected_part_id {
                let rad = 15.0_f64.to_radians();
                ed_state.record_undo(&active_bp.blueprint);
                active_bp.blueprint.rotate_part(sel, rad);
                if ed_state.symmetry_2x
                    && let Some(twin) = active_bp.blueprint.find_mirror_twin(sel)
                {
                    active_bp.blueprint.rotate_part(twin, -rad);
                }
                active_bp.dirty = true;
            }
        }
        AttachAction::RotateRight => {
            if let Some(sel) = active_bp.selected_part_id {
                let rad = -15.0_f64.to_radians();
                ed_state.record_undo(&active_bp.blueprint);
                active_bp.blueprint.rotate_part(sel, rad);
                if ed_state.symmetry_2x
                    && let Some(twin) = active_bp.blueprint.find_mirror_twin(sel)
                {
                    active_bp.blueprint.rotate_part(twin, -rad);
                }
                active_bp.dirty = true;
            }
        }
        AttachAction::FlipMirror => {
            if let Some(sel) = active_bp.selected_part_id {
                ed_state.record_undo(&active_bp.blueprint);
                active_bp.blueprint.toggle_part_mirrored(sel);
                if ed_state.symmetry_2x
                    && let Some(twin) = active_bp.blueprint.find_mirror_twin(sel)
                {
                    active_bp.blueprint.toggle_part_mirrored(twin);
                }
                active_bp.dirty = true;
            }
        }
        AttachAction::DuplicateSelected => {
            if let Some(sel) = active_bp.selected_part_id {
                ed_state.record_undo(&active_bp.blueprint);
                if let Some(cloned) = active_bp
                    .blueprint
                    .duplicate_subtree(sel, DVec2::new(0.4, -0.4))
                {
                    active_bp.selected_part_id = Some(cloned);
                    active_bp.dirty = true;
                    ed_state.status_note = String::from("Cloned selected subtree.");
                }
            }
        }
        AttachAction::ToggleCrossfeed => {
            if let Some(sel) = active_bp.selected_part_id {
                let twin = if ed_state.symmetry_2x {
                    active_bp.blueprint.find_mirror_twin(sel)
                } else {
                    None
                };
                if let Some(part) = active_bp
                    .blueprint
                    .parts
                    .iter()
                    .find(|p| p.instance_id == sel)
                    && catalog
                        .sim_catalog
                        .get(&part.spec_id)
                        .is_some_and(|s| matches!(s.kind, PartKind::Decoupler(_)))
                {
                    ed_state.record_undo(&active_bp.blueprint);
                    let new_cf = !part.crossfeed;
                    for id in [Some(sel), twin].into_iter().flatten() {
                        active_bp.blueprint.set_part_crossfeed(id, new_cf);
                    }
                    active_bp.dirty = true;
                    ed_state.status_note = format!(
                        "Decoupler crossfeed {}.",
                        if new_cf { "ENABLED" } else { "DISABLED" }
                    );
                } else {
                    ed_state.status_note = String::from("Select a decoupler to toggle crossfeed.");
                }
            }
        }
        AttachAction::Undo => {
            if let Some(prev) = ed_state.undo_stack.pop_back() {
                ed_state.redo_stack.push_back(active_bp.blueprint.clone());
                active_bp.blueprint = prev;
                active_bp.selected_part_id =
                    active_bp.blueprint.parts.first().map(|p| p.instance_id);
                active_bp.dirty = true;
                ed_state.status_note = String::from("Undid last VAB edit.");
            }
        }
        AttachAction::Redo => {
            if let Some(next_bp) = ed_state.redo_stack.pop_back() {
                ed_state.undo_stack.push_back(active_bp.blueprint.clone());
                active_bp.blueprint = next_bp;
                active_bp.selected_part_id =
                    active_bp.blueprint.parts.first().map(|p| p.instance_id);
                active_bp.dirty = true;
                ed_state.status_note = String::from("Redid VAB edit.");
            }
        }
        AttachAction::StageDown => {
            if let Some(sel) = active_bp.selected_part_id {
                ed_state.record_undo(&active_bp.blueprint);
                shift_part_stage(&mut active_bp.blueprint, sel, -1, ed_state.symmetry_2x);
                active_bp.dirty = true;
            }
        }
        AttachAction::StageUp => {
            if let Some(sel) = active_bp.selected_part_id {
                ed_state.record_undo(&active_bp.blueprint);
                shift_part_stage(&mut active_bp.blueprint, sel, 1, ed_state.symmetry_2x);
                active_bp.dirty = true;
            }
        }
        AttachAction::DeleteSelected => delete_selected(active_bp, ed_state),
        AttachAction::LoadLunarPreset => {
            ed_state.record_undo(&active_bp.blueprint);
            active_bp.blueprint = lunar_capable_blueprint(&catalog.sim_catalog);
            active_bp.selected_part_id = active_bp.blueprint.parts.first().map(|p| p.instance_id);
            active_bp.dirty = true;
            ed_state.status_note = String::from("Loaded Selene Explorer III preset.");
        }
        AttachAction::ResetStarter => {
            ed_state.record_undo(&active_bp.blueprint);
            active_bp.blueprint = starter_two_stage_blueprint(&catalog.sim_catalog);
            active_bp.selected_part_id = active_bp.blueprint.parts.first().map(|p| p.instance_id);
            active_bp.dirty = true;
            ed_state.status_note = String::from("Loaded Kepler-I Orbital preset.");
        }
        AttachAction::ToggleRename => {
            ed_state.renaming_craft = !ed_state.renaming_craft;
            if ed_state.renaming_craft {
                ed_state.status_note = String::from("Type new craft name and press Enter.");
            }
        }
        AttachAction::SaveCraft => match save_craft_blueprint(&active_bp.blueprint) {
            Ok(path) => {
                ed_state.lists_dirty = true;
                ed_state.status_note =
                    format!("Saved '{}' ({})", active_bp.blueprint.name, path.display());
            }
            Err(err) => ed_state.status_note = err,
        },
        AttachAction::ToggleCraftsDrawer => {
            ed_state.crafts_drawer_open = !ed_state.crafts_drawer_open;
            ed_state.lists_dirty = true;
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
        AttachKind::StackBottom => DVec2::new(
            0.0,
            parent_spec.shape.bottom_offset().y - new_spec.shape.top_offset().y,
        ),
        AttachKind::StackTop => DVec2::new(
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

fn attach_radial_pair(active_bp: &mut ActiveBlueprint, catalog: &LivePartCatalog, pair: bool) {
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

    let new_left = active_bp.blueprint.attach_child(
        parent_id,
        new_spec.id.clone(),
        AttachKind::RadialLeft,
        left_pos,
        0.0,
        parent_part.stage,
    );
    if pair {
        active_bp.blueprint.attach_child(
            parent_id,
            new_spec.id.clone(),
            AttachKind::RadialRight,
            right_pos,
            0.0,
            parent_part.stage,
        );
    }
    active_bp.selected_part_id = Some(new_left);
    active_bp.dirty = true;
}

/// Deletes the selected part (and everything attached to it). The root cannot be deleted.
fn delete_selected(active_bp: &mut ActiveBlueprint, ed_state: &mut EditorState) {
    let Some(sel) = active_bp.selected_part_id else {
        return;
    };
    let is_root = active_bp
        .blueprint
        .parts
        .iter()
        .find(|p| p.instance_id == sel)
        .is_some_and(|p| p.parent_id.is_none());
    if is_root {
        ed_state.status_note =
            String::from("The root part can't be deleted - drag it to move the whole rocket.");
        return;
    }
    ed_state.record_undo(&active_bp.blueprint);
    let twin = if ed_state.symmetry_2x {
        active_bp.blueprint.find_mirror_twin(sel)
    } else {
        None
    };
    let before = active_bp.blueprint.parts.len();
    active_bp.blueprint.remove_subtree(sel);
    if let Some(tid) = twin {
        active_bp.blueprint.remove_subtree(tid);
    }
    let removed = before - active_bp.blueprint.parts.len();
    active_bp.selected_part_id = active_bp.blueprint.parts.first().map(|p| p.instance_id);
    active_bp.dirty = true;
    ed_state.status_note = format!("Deleted {removed} part(s). Ctrl+Z to undo.");
}

/// One-click building: attach the brush to the selected part's next free node.
fn auto_attach(
    active_bp: &mut ActiveBlueprint,
    catalog: &LivePartCatalog,
    ed_state: &mut EditorState,
) {
    let Some(sel) = active_bp.selected_part_id else {
        ed_state.status_note = String::from("Select a part first, then Auto-attach.");
        return;
    };
    let Some(parent) = active_bp
        .blueprint
        .parts
        .iter()
        .find(|p| p.instance_id == sel)
        .cloned()
    else {
        return;
    };
    let (Some(pspec), Some(new_spec)) = (
        catalog.sim_catalog.get(&parent.spec_id),
        catalog.sim_catalog.get(&active_bp.active_palette_spec),
    ) else {
        return;
    };
    let has_child = |bp: &VesselBlueprint, kind: AttachKind| {
        bp.parts
            .iter()
            .any(|p| p.parent_id == Some(sel) && p.attach_kind == kind)
    };
    let sym = ed_state.symmetry_2x;
    let radial_only = new_spec.radial_attach && !new_spec.stack_top && !new_spec.stack_bottom;
    let name = new_spec.display_name.clone();

    if radial_only {
        ed_state.record_undo(&active_bp.blueprint);
        attach_radial_pair(active_bp, catalog, sym);
    } else if pspec.stack_bottom
        && new_spec.stack_top
        && !has_child(&active_bp.blueprint, AttachKind::StackBottom)
    {
        ed_state.record_undo(&active_bp.blueprint);
        attach_to_selected(active_bp, catalog, AttachKind::StackBottom);
    } else if pspec.stack_top
        && new_spec.stack_bottom
        && !has_child(&active_bp.blueprint, AttachKind::StackTop)
    {
        ed_state.record_undo(&active_bp.blueprint);
        attach_to_selected(active_bp, catalog, AttachKind::StackTop);
    } else if new_spec.radial_attach {
        ed_state.record_undo(&active_bp.blueprint);
        attach_radial_pair(active_bp, catalog, sym);
    } else {
        ed_state.status_note = format!("No free node on the selected part for {name}.");
        return;
    }
    ed_state.status_note = format!("Attached {name}.");
}

/// Keeps the floating toolbar glued above the selected part (hidden while dragging).
fn update_selection_toolbar(
    ed_state: Res<'_, EditorState>,
    active_bp: Res<'_, ActiveBlueprint>,
    catalog: Res<'_, LivePartCatalog>,
    windows: Query<'_, '_, &Window>,
    cam_q: Query<'_, '_, (&Camera, &GlobalTransform), With<MainCamera>>,
    mut bar_q: Query<'_, '_, &mut Node, With<SelectionToolbar>>,
    mut cf_q: Query<'_, '_, &mut Node, (With<CrossfeedToolButton>, Without<SelectionToolbar>)>,
) {
    let Ok(mut bar) = bar_q.single_mut() else {
        return;
    };
    let hide = |bar: &mut Node| bar.display = Display::None;

    if !matches!(ed_state.drag, DragPhase::Idle) {
        hide(&mut bar);
        return;
    }
    let (Ok(window), Ok((camera, cam_xform))) = (windows.single(), cam_q.single()) else {
        hide(&mut bar);
        return;
    };
    let Some(part) = active_bp.selected_part_id.and_then(|id| {
        active_bp
            .blueprint
            .parts
            .iter()
            .find(|p| p.instance_id == id)
    }) else {
        hide(&mut bar);
        return;
    };
    let Some(spec) = catalog.sim_catalog.get(&part.spec_id) else {
        hide(&mut bar);
        return;
    };

    let half = spec.shape.contact_half_extent();
    let top_world = Vec3::new(
        part.local_pos.x as f32,
        (part.local_pos.y + half.y + 0.2) as f32,
        0.0,
    );
    let bottom_world = Vec3::new(
        part.local_pos.x as f32,
        (part.local_pos.y - half.y - 0.2) as f32,
        0.0,
    );
    let (Ok(top_vp), Ok(bottom_vp)) = (
        camera.world_to_viewport(cam_xform, top_world),
        camera.world_to_viewport(cam_xform, bottom_world),
    ) else {
        hide(&mut bar);
        return;
    };

    let min_x = PALETTE_PANEL_WIDTH + 24.0;
    let max_x = (window.width() - INSPECTOR_PANEL_WIDTH - 24.0 - TOOLBAR_W).max(min_x);
    let x = (top_vp.x - TOOLBAR_W * 0.5).clamp(min_x, max_x);
    let above = top_vp.y - TOOLBAR_H - 8.0;
    let y = if above >= TOP_BAR_CLEARANCE {
        above
    } else {
        (bottom_vp.y + 8.0).min(window.height() - TOOLBAR_H - 56.0)
    };
    bar.left = px(x);
    bar.top = px(y);
    bar.display = Display::Flex;

    let is_decoupler = matches!(spec.kind, PartKind::Decoupler(_));
    for mut cf in &mut cf_q {
        cf.display = if is_decoupler {
            Display::Flex
        } else {
            Display::None
        };
    }
}

fn draw_part_outline(gizmos: &mut Gizmos<'_, '_>, part: &PlacedPart, half: DVec2, color: Color) {
    let corners = [
        DVec2::new(-half.x, -half.y),
        DVec2::new(half.x, -half.y),
        DVec2::new(half.x, half.y),
        DVec2::new(-half.x, half.y),
    ];
    let to_world = |c: DVec2| {
        let w = part.local_pos + c.rotate(part.local_rot);
        Vec2::new(w.x as f32, w.y as f32)
    };
    for i in 0..4 {
        gizmos.line_2d(to_world(corners[i]), to_world(corners[(i + 1) % 4]), color);
    }
}

#[inline]
fn snap_to_grid(pos: DVec2) -> DVec2 {
    DVec2::new(
        (pos.x / GRID_SNAP_M).round() * GRID_SNAP_M,
        (pos.y / GRID_SNAP_M).round() * GRID_SNAP_M,
    )
}

fn draw_editor_gizmos(
    catalog: Res<'_, LivePartCatalog>,
    active_bp: Res<'_, ActiveBlueprint>,
    ed_state: Res<'_, EditorState>,
    mut gizmos: Gizmos<'_, '_>,
) {
    let analysis = analyze_blueprint(&active_bp.blueprint, &catalog.sim_catalog);

    // Outline the part under the cursor so hovering gives instant feedback before clicking.
    if matches!(ed_state.drag, DragPhase::Idle)
        && let Some(hid) = ed_state.hover_part
        && let Some(part) = active_bp
            .blueprint
            .parts
            .iter()
            .find(|p| p.instance_id == hid)
        && let Some(spec) = catalog.sim_catalog.get(&part.spec_id)
    {
        let color = if active_bp.selected_part_id == Some(hid) {
            BORDER_ON
        } else {
            Color::srgba(0.70, 0.90, 1.0, 0.9)
        };
        draw_part_outline(&mut gizmos, part, spec.shape.contact_half_extent(), color);
    }

    let grid_col = Color::srgba(0.14, 0.30, 0.44, 0.24);
    for ix in -16..=16 {
        let p = ix as f32;
        gizmos.line_2d(Vec2::new(p, -24.0), Vec2::new(p, 24.0), grid_col);
    }
    for iy in -24..=24 {
        let p = iy as f32;
        gizmos.line_2d(Vec2::new(-16.0, p), Vec2::new(16.0, p), grid_col);
    }

    // Show available attach-node anchors while dragging a part
    if !matches!(ed_state.drag, DragPhase::Idle) {
        let node_col = Color::srgba(0.28, 0.90, 1.0, 0.55);
        for part in &active_bp.blueprint.parts {
            if let Some(spec) = catalog.sim_catalog.get(&part.spec_id) {
                let p = Vec2::new(part.local_pos.x as f32, part.local_pos.y as f32);
                if spec.stack_top {
                    let top = p + Vec2::new(0.0, spec.shape.top_offset().y as f32);
                    gizmos.circle_2d(top, 0.08, node_col);
                }
                if spec.stack_bottom {
                    let bot = p + Vec2::new(0.0, spec.shape.bottom_offset().y as f32);
                    gizmos.circle_2d(bot, 0.08, node_col);
                }
            }
        }
    }

    // Yellow Center of Mass (CoM) reticle
    let center_of_mass_position =
        Vec2::new(analysis.com_local.x as f32, analysis.com_local.y as f32);
    let center_of_mass_color = Color::srgb(1.0, 0.88, 0.22);
    gizmos.circle_2d(center_of_mass_position, 0.22, center_of_mass_color);
    gizmos.line_2d(
        center_of_mass_position - Vec2::X * 0.38,
        center_of_mass_position + Vec2::X * 0.38,
        center_of_mass_color,
    );
    gizmos.line_2d(
        center_of_mass_position - Vec2::Y * 0.38,
        center_of_mass_position + Vec2::Y * 0.38,
        center_of_mass_color,
    );

    // Blue Center of Pressure (CoP) reticle
    let center_of_pressure_position =
        Vec2::new(analysis.cop_local.x as f32, analysis.cop_local.y as f32);
    let center_of_pressure_color = if analysis.aero_unstable {
        Color::srgb(1.0, 0.32, 0.26)
    } else {
        Color::srgb(0.26, 0.78, 1.0)
    };
    gizmos.circle_2d(center_of_pressure_position, 0.19, center_of_pressure_color);
    gizmos.line_2d(
        center_of_pressure_position + Vec2::new(-0.24, -0.24),
        center_of_pressure_position + Vec2::new(0.24, 0.24),
        center_of_pressure_color,
    );
    gizmos.line_2d(
        center_of_pressure_position + Vec2::new(-0.24, 0.24),
        center_of_pressure_position + Vec2::new(0.24, -0.24),
        center_of_pressure_color,
    );

    if let Some(snap_m) = ed_state.snap_preview {
        let sp = Vec2::new(snap_m.x as f32, snap_m.y as f32);
        gizmos.circle_2d(sp, 0.30, Color::srgb(0.28, 1.0, 0.62));
    }
}

fn editor_hint(active_bp: &ActiveBlueprint, catalog: &LivePartCatalog, ed_state: &EditorState) -> String {
    let name_of = |id: &str| {
        catalog
            .sim_catalog
            .get(id)
            .map_or_else(|| id.to_owned(), |s| s.display_name.clone())
    };
    match &ed_state.drag {
        DragPhase::Carrying { spec_id } => format!(
            "Placing {} | Click: place (Shift: keep placing) | Right-click / Esc: cancel",
            name_of(spec_id)
        ),
        DragPhase::DraggingNew { spec_id } => format!(
            "Dropping {} | Release on a glowing node to snap, or anywhere to place freely",
            name_of(spec_id)
        ),
        DragPhase::DraggingExisting { .. } => {
            String::from("Moving | Release on a node to snap | Hold Shift to bypass snapping")
        }
        DragPhase::PendingPalette { .. }
        | DragPhase::PendingExisting { .. }
        | DragPhase::PanningCanvas { .. } => ed_state.status_note.clone(),
        DragPhase::Idle => {
            if let Some(id) = ed_state.hover_part
                && let Some(part) = active_bp
                    .blueprint
                    .parts
                    .iter()
                    .find(|p| p.instance_id == id)
            {
                format!(
                    "{} | Click: select | Drag: move | Ctrl+Drag: one part | Alt+Drag: clone | Right-click: delete",
                    name_of(&part.spec_id)
                )
            } else if active_bp.selected_part_id.is_some() {
                String::from(
                    "Use the toolbar on the part (rotate, flip, copy, stage, delete) | Click empty space to deselect | Scroll: zoom",
                )
            } else {
                String::from(
                    "Pick a part on the left, then click the bay | Drag empty space: pan | Scroll: zoom to cursor",
                )
            }
        }
    }
}

fn update_editor_readouts_and_highlights(
    time: Res<'_, Time>,
    catalog: Res<'_, LivePartCatalog>,
    active_bp: Res<'_, ActiveBlueprint>,
    ed_state: Res<'_, EditorState>,
    mut queries: EditorReadoutQueries<'_, '_>,
    mut materials: ResMut<'_, Assets<PartMaterial>>,
) {
    let t = time.elapsed_secs();
    for (node, mat_handle) in &queries.mat_q {
        if let Some(mut mat) = materials.get_mut(&mat_handle.0) {
            mat.flags.y = if active_bp.selected_part_id == Some(node.instance_id) {
                1.0
            } else {
                0.0
            };
            mat.flags.w = t;
        }
    }

    for (pbtn, mut border) in &mut queries.palette_q {
        let in_hand = match &ed_state.drag {
            DragPhase::Carrying { spec_id } | DragPhase::DraggingNew { spec_id } => {
                *spec_id == pbtn.0
            }
            _ => false,
        };
        *border = if in_hand {
            uniform_border(BORDER_ON)
        } else if pbtn.0 == active_bp.active_palette_spec {
            uniform_border(Color::srgba(0.28, 0.92, 1.0, 0.96))
        } else {
            uniform_border(Color::srgba(0.24, 0.52, 0.72, 0.45))
        };
    }

    for (tbtn, mut border) in &mut queries.tab_q {
        *border = if tbtn.0 == ed_state.selected_category {
            uniform_border(Color::srgba(0.28, 0.96, 0.62, 0.95))
        } else {
            uniform_border(Color::srgba(0.24, 0.52, 0.72, 0.45))
        };
    }

    for (toggle, mut border) in &mut queries.toggle_q {
        let on = match toggle.0 {
            ToggleKind::Symmetry => ed_state.symmetry_2x,
            ToggleKind::Snap => ed_state.snap_nodes,
        };
        *border = uniform_border(if on { BORDER_ON } else { BTN_BORDER });
    }

    let analysis = analyze_blueprint(&active_bp.blueprint, &catalog.sim_catalog);
    for (tag, mut text) in &mut queries.text_q {
        let new_text = match tag {
            EditorText::Readout => format_analysis_text(&active_bp, &catalog, &ed_state, &analysis),
            EditorText::SymStatus => format!(
                "Symmetry {} [X] | Node snap {} [S]",
                if ed_state.symmetry_2x { "2x" } else { "1x" },
                if ed_state.snap_nodes { "ON" } else { "OFF" }
            ),
            EditorText::CraftName => format!(
                "{}{}",
                active_bp.blueprint.name,
                if ed_state.renaming_craft {
                    "_  (Enter: done)"
                } else {
                    ""
                }
            ),
            EditorText::SymLabel => String::from(if ed_state.symmetry_2x {
                "Sym 2x"
            } else {
                "Sym 1x"
            }),
            EditorText::Brush => catalog
                .sim_catalog
                .get(&active_bp.active_palette_spec)
                .map_or_else(
                    || String::from("No part selected"),
                    |s| format!("IN HAND: {}", part_summary(s)),
                ),
            EditorText::Hint => editor_hint(&active_bp, &catalog, &ed_state),
        };
        if text.0 != new_text {
            text.0 = new_text;
        }
    }
}

fn format_analysis_text(
    active_bp: &ActiveBlueprint,
    catalog: &LivePartCatalog,
    ed_state: &EditorState,
    a: &BlueprintAnalysis,
) -> String {
    let sel_info = active_bp
        .selected_part_id
        .and_then(|id| {
            active_bp
                .blueprint
                .parts
                .iter()
                .find(|p| p.instance_id == id)
        })
        .map_or_else(
            || String::from("None"),
            |p| {
                let name = catalog
                    .sim_catalog
                    .get(&p.spec_id)
                    .map_or(p.spec_id.as_str(), |s| s.display_name.as_str());
                format!(
                    "{name} (S{}, {:.0} deg)",
                    p.stage + 1,
                    p.local_rot.to_degrees()
                )
            },
        );

    let pal_name = catalog
        .sim_catalog
        .get(&active_bp.active_palette_spec)
        .map_or(active_bp.active_palette_spec.as_str(), |s| {
            s.display_name.as_str()
        });

    let name_suffix = if ed_state.renaming_craft { "_" } else { "" };
    let mut out = format!(
        "Craft: {}{} ({} parts)\n\
         Active Brush: {}\n\
         Selected: {}\n\
         Wet: {:.0} kg  |  Dry: {:.0} kg  |  Fuel: {:.0} kg\n\
         CoM: ({:+.2}, {:+.2})  |  CoP: ({:+.2}, {:+.2}) m\n\
         MoI: {:.0} kg*m^2  |  Total dV: {:.0} m/s\n",
        active_bp.blueprint.name,
        name_suffix,
        active_bp.blueprint.parts.len(),
        pal_name,
        sel_info,
        a.wet_mass_kg,
        a.dry_mass_kg,
        a.fuel_mass_kg,
        a.com_local.x,
        a.com_local.y,
        a.cop_local.x,
        a.cop_local.y,
        a.moi_kg_m2,
        a.total_delta_v_m_s
    );

    if !a.warnings.is_empty() {
        out.push_str("\nCHECKLIST WARNINGS:\n");
        for w in &a.warnings {
            let _ = writeln!(out, "  ! {w}");
        }
    }

    out.push_str("\nSTAGES:\n");
    for s in &a.stages {
        let _ = writeln!(
            out,
            "  S{}: dV {:.0} m/s | TWR {:.2} | {:.0} kN{}",
            s.stage + 1,
            s.delta_v_m_s,
            s.twr_surface,
            s.thrust_n / 1000.0,
            if s.has_decoupler { " [SEP]" } else { "" }
        );
    }
    out.push('\n');
    out.push_str(&ed_state.status_note);
    out
}

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameMode::Editor), setup_editor)
            .add_systems(OnExit(GameMode::Editor), cleanup_editor)
            .add_systems(
                Update,
                (
                    rebuild_palette_grid_if_dirty,
                    rebuild_stage_and_craft_lists_if_dirty,
                    style_buttons_on_change,
                    style_buttons_on_release,
                    handle_craft_rename_typing,
                    handle_palette_press_for_drag,
                    handle_editor_canvas_drag_and_drop,
                    handle_editor_keyboard_shortcuts,
                    rebuild_editor_meshes_if_dirty,
                    draw_editor_gizmos,
                    update_selection_toolbar,
                    update_editor_readouts_and_highlights,
                )
                    .chain()
                    .run_if(in_state(GameMode::Editor)),
            );
    }
}

// ---------- EXPORTS
