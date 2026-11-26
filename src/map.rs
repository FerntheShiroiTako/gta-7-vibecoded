//! Full-screen map with waypoints, minimap blips, and the pause menu.

use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

use crate::camera::CameraRig;
use crate::hud::{MapHandle, MinimapFrame, MAP_MAX, MAP_MIN, MINIMAP};
use crate::input::{GameInput, UiCursor};
use crate::player::Player;
use crate::seats::{Seat, SeatKind};
use crate::vehicle::Driver;
use crate::wanted::PoliceCar;

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Menus>()
            .init_resource::<Waypoint>()
            .add_systems(Startup, setup_map_ui.after(crate::hud::HudSetup))
            .add_systems(Update, (menu_toggles, map_clicks, update_full_map, menu_buttons, update_blips).chain());
    }
}

#[derive(Resource, Default)]
pub struct Menus {
    pub map: bool,
    pub pause: bool,
    pub spawn: bool,
}

impl Menus {
    pub fn any(&self) -> bool {
        self.map || self.pause || self.spawn
    }
}

#[derive(Resource, Default)]
pub struct Waypoint(pub Option<Vec2>);

#[derive(Component)]
struct FullMap;
#[derive(Component)]
struct FullMapImage;
#[derive(Component)]
struct MapMarker(u8); // 0 player, 1 waypoint
#[derive(Component)]
struct PauseMenu;
#[derive(Component, Clone, Copy)]
enum MenuButton {
    Resume,
    Map,
    Spawn,
    Quit,
}
#[derive(Component)]
struct Blip(usize);
#[derive(Component)]
struct WaypointText;

const BLIPS: usize = 24;

fn map_uv(p: Vec2) -> Vec2 {
    (p - MAP_MIN) / (MAP_MAX - MAP_MIN)
}

fn setup_map_ui(mut commands: Commands, map: Res<MapHandle>, frames: Query<Entity, With<MinimapFrame>>) {
    let aspect = (MAP_MAX.x - MAP_MIN.x) / (MAP_MAX.y - MAP_MIN.y);
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.05, 0.92)),
            Visibility::Hidden,
            GlobalZIndex(10),
            FullMap,
        ))
        .with_children(|c| {
            c.spawn((
                ImageNode::new(map.0.clone()),
                Node { height: percent(86), aspect_ratio: Some(aspect), border: UiRect::all(px(2)), ..default() },
                BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.3)),
                RelativeCursorPosition::default(),
                Interaction::default(),
                FullMapImage,
            ))
            .with_children(|m| {
                for (k, color, size) in [(0u8, Color::srgb(1.0, 1.0, 1.0), 14.0), (1, Color::srgb(0.85, 0.3, 0.95), 16.0)] {
                    m.spawn((
                        Node { position_type: PositionType::Absolute, width: px(size), height: px(size), margin: UiRect { left: px(-size * 0.5), top: px(-size * 0.5), ..default() }, border_radius: BorderRadius::all(px(size)), border: UiRect::all(px(2)), ..default() },
                        BackgroundColor(color),
                        BorderColor::all(Color::BLACK),
                        Visibility::Hidden,
                        MapMarker(k),
                    ));
                }
            });
            c.spawn((
                Text::new("Left click: set waypoint    Right click: clear    M / Esc: close"),
                TextColor(Color::srgb(0.85, 0.85, 0.85)),
                Node { position_type: PositionType::Absolute, bottom: px(14), ..default() },
            ));
        });
    // pause menu
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), flex_direction: FlexDirection::Column, justify_content: JustifyContent::Center, align_items: AlignItems::Center, row_gap: px(14), ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
            Visibility::Hidden,
            GlobalZIndex(9),
            PauseMenu,
        ))
        .with_children(|c| {
            c.spawn((Text::new("PAUSED"), TextFont { font_size: bevy::text::FontSize::Px(64.0), ..default() }, TextColor(Color::WHITE)));
            for (label, b) in [("Resume", MenuButton::Resume), ("Map", MenuButton::Map), ("Spawn vehicle (F1)", MenuButton::Spawn), ("Quit", MenuButton::Quit)] {
                c.spawn((
                    bevy::ui_widgets::Button,
                    Interaction::default(),
                    b,
                    Node { width: px(320), height: px(52), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border_radius: BorderRadius::all(px(8)), ..default() },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.12)),
                ))
                .with_children(|b| {
                    b.spawn((Text::new(label), TextColor(Color::WHITE), TextFont { font_size: bevy::text::FontSize::Px(26.0), ..default() }));
                });
            }
            c.spawn((Text::new("Key bindings: edit keybinds.cfg next to the game"), TextColor(Color::srgb(0.7, 0.7, 0.7))));
        });
    // minimap blips live inside the minimap frame (clipped by it)
    if let Ok(frame) = frames.single() {
        commands.entity(frame).with_children(|f| {
            for i in 0..BLIPS {
                f.spawn((
                    Node { position_type: PositionType::Absolute, width: px(9), height: px(9), margin: UiRect { left: px(-4.5), top: px(-4.5), ..default() }, border_radius: BorderRadius::all(px(5)), border: UiRect::all(px(1)), ..default() },
                    BackgroundColor(Color::WHITE),
                    BorderColor::all(Color::BLACK),
                    Visibility::Hidden,
                    Blip(i),
                ));
            }
        });
    }
    commands.spawn((
        Text::new(""),
        TextColor(Color::srgb(0.9, 0.6, 1.0)),
        TextShadow::default(),
        Node { position_type: PositionType::Absolute, left: px(30), bottom: px(300), ..default() },
        WaypointText,
    ));
}

#[allow(clippy::too_many_arguments)]
fn menu_toggles(
    input: Res<GameInput>,
    mut menus: ResMut<Menus>,
    mut ui: ResMut<UiCursor>,
    mut vtime: ResMut<Time<Virtual>>,
    mut full: Query<&mut Visibility, (With<FullMap>, Without<PauseMenu>)>,
    mut pause: Query<&mut Visibility, (With<PauseMenu>, Without<FullMap>)>,
) {
    if input.pause {
        if menus.map || menus.spawn {
            menus.map = false;
            menus.spawn = false;
        } else {
            menus.pause = !menus.pause;
        }
    }
    if input.map {
        menus.map = !menus.map;
        if menus.map {
            menus.pause = false;
            menus.spawn = false;
        }
    }
    if input.spawn_menu {
        menus.spawn = !menus.spawn;
        if menus.spawn {
            menus.pause = false;
            menus.map = false;
        }
    }
    let any = menus.any();
    if ui.0 != any {
        ui.0 = any;
    }
    if any && !vtime.is_paused() {
        vtime.pause();
    } else if !any && vtime.is_paused() {
        vtime.unpause();
    }
    if let Ok(mut v) = full.single_mut() {
        *v = if menus.map { Visibility::Inherited } else { Visibility::Hidden };
    }
    if let Ok(mut v) = pause.single_mut() {
        *v = if menus.pause { Visibility::Inherited } else { Visibility::Hidden };
    }
}

fn map_clicks(menus: Res<Menus>, mouse: Res<ButtonInput<MouseButton>>, map: Query<&RelativeCursorPosition, With<FullMapImage>>, mut wp: ResMut<Waypoint>, mut hud: ResMut<crate::hud::HudMessages>) {
    if !menus.map {
        return;
    }
    let Ok(rc) = map.single() else { return };
    if mouse.just_pressed(MouseButton::Right) {
        wp.0 = None;
    }
    if mouse.just_pressed(MouseButton::Left) && rc.cursor_over() {
        if let Some(n) = rc.normalized {
            let uv = n + Vec2::splat(0.5);
            wp.0 = Some(MAP_MIN + uv * (MAP_MAX - MAP_MIN));
            hud.push("Waypoint set");
        }
    }
}

fn update_full_map(menus: Res<Menus>, wp: Res<Waypoint>, player: Query<&Transform, With<Player>>, mut markers: Query<(&MapMarker, &mut Node, &mut Visibility)>) {
    if !menus.map {
        return;
    }
    let ppos = player.single().map(|t| Vec2::new(t.translation.x, t.translation.z)).ok();
    for (m, mut n, mut v) in &mut markers {
        let p = if m.0 == 0 { ppos } else { wp.0 };
        match p {
            Some(p) => {
                let uv = map_uv(p);
                n.left = percent(uv.x * 100.0);
                n.top = percent(uv.y * 100.0);
                *v = Visibility::Inherited;
            }
            None => *v = Visibility::Hidden,
        }
    }
}

fn menu_buttons(
    mut menus: ResMut<Menus>,
    mut buttons: Query<(&Interaction, &MenuButton, &mut BackgroundColor), Changed<Interaction>>,
    mut quit: ResMut<crate::lighting::QuitRequest>,
) {
    for (i, b, mut bg) in &mut buttons {
        bg.0 = match i {
            Interaction::Hovered => Color::srgba(1.0, 1.0, 1.0, 0.25),
            Interaction::Pressed => Color::srgba(1.0, 0.8, 0.3, 0.4),
            Interaction::None => Color::srgba(1.0, 1.0, 1.0, 0.12),
        };
        if *i == Interaction::Pressed {
            match b {
                MenuButton::Resume => menus.pause = false,
                MenuButton::Map => {
                    menus.pause = false;
                    menus.map = true;
                }
                MenuButton::Spawn => {
                    menus.pause = false;
                    menus.spawn = true;
                }
                MenuButton::Quit => crate::lighting::request_quit(&mut quit),
            }
        }
    }
}

/// Anything that should show on the minimap: shops, job markers, targets.
/// Hidden entities (InheritedVisibility off) don't get a blip.
#[derive(Component, Clone, Copy)]
pub struct MapBlip {
    pub color: Color,
    /// only shown within this distance of the player
    pub range: f32,
}

/// Minimap blips: waypoint (purple), job markers/targets, shops, police
/// (flashing red/blue), aircraft (yellow).
#[allow(clippy::type_complexity)]
fn update_blips(
    time: Res<Time<Real>>,
    rig: Res<CameraRig>,
    wp: Res<Waypoint>,
    player: Query<(&Player, &Transform)>,
    seats: Query<(Entity, &Transform, &Seat, Has<PoliceCar>)>,
    mut blips: Query<(&Blip, &mut Node, &mut BackgroundColor, &mut Visibility)>,
    mut wtext: Query<&mut Text, With<WaypointText>>,
    zoom: Res<crate::hud::MinimapZoom>,
    marks: Query<(&GlobalTransform, &MapBlip, &InheritedVisibility)>,
) {
    let Ok((p, pt)) = player.single() else { return };
    let me = p.vehicle.and_then(|v| seats.get(v).ok()).map(|s| s.1.translation).unwrap_or(pt.translation);
    let me2 = Vec2::new(me.x, me.z);
    let mut list: Vec<(Vec2, Color)> = Vec::new();
    if let Some(w) = wp.0 {
        list.push((w, Color::srgb(0.85, 0.3, 0.95)));
        if let Ok(mut t) = wtext.single_mut() {
            t.0 = format!("Waypoint {:.0} m", w.distance(me2));
        }
        if w.distance(me2) < 12.0 {
            if let Ok(mut t) = wtext.single_mut() {
                t.0 = "Waypoint reached".into();
            }
        }
    } else if let Ok(mut t) = wtext.single_mut() {
        t.0.clear();
    }
    let mut marked: Vec<(f32, Vec2, Color)> = marks
        .iter()
        .filter(|(_, _, v)| v.get())
        .map(|(gt, m, _)| {
            let p = Vec2::new(gt.translation().x, gt.translation().z);
            (p.distance(me2), p, m.color, m.range)
        })
        .filter(|(d, _, _, r)| d < r)
        .map(|(d, p, c, _)| (d, p, c))
        .collect();
    marked.sort_by(|a, b| a.0.total_cmp(&b.0));
    list.extend(marked.into_iter().map(|(_, p, c)| (p, c)));
    let flash = (time.elapsed_secs() * 4.0) as i32 % 2 == 0;
    for (e, t, seat, police) in &seats {
        if Some(e) == p.vehicle {
            continue;
        }
        let pos = Vec2::new(t.translation.x, t.translation.z);
        if police && matches!(seat.driver, Driver::Cop(_)) {
            list.push((pos, if flash { Color::srgb(1.0, 0.15, 0.15) } else { Color::srgb(0.2, 0.4, 1.0) }));
        } else if matches!(seat.kind, SeatKind::Plane | SeatKind::Heli) {
            list.push((pos, Color::srgb(1.0, 0.85, 0.2)));
        }
    }
    let scale = 1.0 / crate::hud::MAP_MPP * zoom.0;
    let rot = Rot2::radians(rig.yaw);
    let half = MINIMAP * 0.5 - 3.0;
    for (b, mut n, mut bg, mut v) in &mut blips {
        let Some((pos, c)) = list.get(b.0) else {
            *v = Visibility::Hidden;
            continue;
        };
        let off = rot * ((*pos - me2) * scale);
        // clamp to the frame edge so far-away targets still show a direction
        let k = (half / off.x.abs().max(off.y.abs()).max(1e-3)).min(1.0);
        let o = off * k;
        n.left = px(MINIMAP * 0.5 + o.x - 3.0);
        n.top = px(MINIMAP * 0.5 + o.y - 3.0);
        bg.0 = *c;
        *v = Visibility::Inherited;
    }
}
