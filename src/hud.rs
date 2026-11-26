//! HUD: money, health/armour, speedometer, hints, messages, big banners, FPS
//! and a rotating minimap rendered from the generated city layout.

use bevy::asset::RenderAssetUsages;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::{Font, FontSize, FontSource};
use bevy_rapier3d::prelude::Velocity;

use crate::camera::CameraRig;
use crate::city::{CityInfo, Foot};
use crate::player::Player;
use crate::vehicle::Vehicle;
use crate::world::*;

pub struct HudPlugin;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct HudSetup;

#[derive(Resource)]
pub struct MinimapZoom(pub f32);
impl Default for MinimapZoom {
    fn default() -> Self {
        MinimapZoom(1.0)
    }
}

#[derive(Resource, Clone)]
pub struct MapHandle(pub Handle<Image>);

/// Marker for the minimap frame (blips are drawn inside it).
#[derive(Component)]
pub struct MinimapFrame;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudMessages>()
            .init_resource::<MinimapZoom>()
            .add_systems(Startup, setup_hud.after(crate::city::setup_city).in_set(HudSetup))
            .add_systems(Update, (update_hud, update_minimap, update_messages, update_combat_hud));
    }
}

#[derive(Resource, Default)]
pub struct HudMessages {
    queue: Vec<(String, f32)>,
    big: Option<(String, Color, f32)>,
    pub show_fps: bool,
    /// context hint from other systems (shown when nothing more urgent is)
    pub extra_hint: String,
}

impl HudMessages {
    pub fn push(&mut self, s: &str) {
        self.queue.push((s.to_string(), 3.0));
        if self.queue.len() > 3 {
            self.queue.remove(0);
        }
    }
    pub fn big(&mut self, s: &str, c: Color) {
        self.big = Some((s.to_string(), c, 4.5));
    }
}

#[derive(Component)]
struct MoneyText;
#[derive(Component)]
struct StarsText;
#[derive(Component)]
struct WeaponText;
#[derive(Component)]
struct Crosshair;
#[derive(Component)]
struct HealthFill;
#[derive(Component)]
struct ArmorFill;
#[derive(Component)]
struct SpeedRoot;
#[derive(Component)]
struct SpeedText;
#[derive(Component)]
struct CarName;
#[derive(Component)]
struct CarHealthFill;
#[derive(Component)]
struct HintText;
#[derive(Component)]
struct MessageText;
#[derive(Component)]
struct BigText;
#[derive(Component)]
struct FpsText;
#[derive(Component)]
struct MapRotator;
#[derive(Component)]
struct MapImage;
#[derive(Component)]
struct PlayerArrow;

pub const MAP_MIN: Vec2 = Vec2::new(-1600.0, -1220.0);
pub const MAP_MAX: Vec2 = Vec2::new(760.0, 700.0);
pub const MAP_MPP: f32 = 1.25; // metres per pixel
pub const MINIMAP: f32 = 250.0;

/// Segoe UI read straight from the Windows font folder and registered as normal
/// `Font` assets. (Bevy 0.20's system-font `FontSource::Family` path leaks glyph
/// atlases on every text change - ~190 MiB/s here - so it is deliberately avoided.)
#[derive(Resource, Clone, Default)]
pub struct HudFonts {
    regular: FontSource,
    symbol: FontSource,
    bold: FontSource,
    black: FontSource,
}

fn load_font(fonts: &mut Assets<Font>, file: &str) -> FontSource {
    let dir = std::env::var("WINDIR").unwrap_or_else(|_| "C:/Windows".into());
    match std::fs::read(format!("{dir}/Fonts/{file}")) {
        Ok(bytes) => FontSource::Handle(fonts.add(Font::from_bytes(bytes))),
        Err(_) => FontSource::default(),
    }
}

fn load_hud_fonts(fonts: &mut Assets<Font>) -> HudFonts {
    HudFonts { symbol: load_font(fonts, "seguisym.ttf"), regular: load_font(fonts, "seguisb.ttf"), bold: load_font(fonts, "segoeuib.ttf"), black: load_font(fonts, "seguibl.ttf") }
}

pub fn font(f: &HudFonts, size: f32, weight: u16) -> TextFont {
    let font = if weight >= 850 { f.black.clone() } else if weight >= 700 { f.bold.clone() } else { f.regular.clone() };
    TextFont { font, font_size: FontSize::Px(size), ..default() }
}

pub fn shadow() -> TextShadow {
    TextShadow { offset: Vec2::new(2.0, 2.0), color: Color::srgba(0.0, 0.0, 0.0, 0.85) }
}

/// Rasterise the world layout into a minimap image.
pub fn build_map_image(info: &CityInfo) -> Image {
    let w = ((MAP_MAX.x - MAP_MIN.x) / MAP_MPP) as u32;
    let h = ((MAP_MAX.y - MAP_MIN.y) / MAP_MPP) as u32;
    let mut px = vec![[46u8, 82, 115, 255]; (w * h) as usize];
    let to_px = |p: Vec2| ((p - MAP_MIN) / MAP_MPP).as_ivec2();
    let fill = |px: &mut Vec<[u8; 4]>, a: Vec2, b: Vec2, c: [u8; 4]| {
        let p0 = to_px(a.min(b)).max(IVec2::ZERO);
        let p1 = to_px(a.max(b)).min(IVec2::new(w as i32 - 1, h as i32 - 1));
        for y in p0.y..=p1.y {
            for x in p0.x..=p1.x {
                px[(y as u32 * w + x as u32) as usize] = c;
            }
        }
    };
    let road = [128, 131, 136, 255];
    let block = [58, 62, 66, 255];
    // terrain areas
    for y in 0..h {
        for x in 0..w {
            let p = MAP_MIN + Vec2::new(x as f32 + 0.5, y as f32 + 0.5) * MAP_MPP;
            let i = (y * w + x) as usize;
            if HILL.contains(p) && p.y < -CITY_HALF {
                let hh = hill_height(p.x, p.y);
                if hh > SEA_LEVEL {
                    let g = (hh / 120.0).clamp(0.0, 1.0);
                    let contour = (hh / 20.0).fract() < 0.08 && hh > 5.0;
                    let base = [(70.0 - 20.0 * g) as u8, (108.0 - 25.0 * g) as u8, (62.0 - 15.0 * g) as u8, 255];
                    px[i] = if contour { [base[0] - 12, base[1] - 14, base[2] - 10, 255] } else { base };
                }
            } else if BEACH.contains(p) && p.y > CITY_HALF {
                let hh = beach_height(p.x, p.y);
                if hh > SEA_LEVEL {
                    px[i] = [196, 178, 128, 255];
                }
            }
        }
    }
    fill(&mut px, CITY.min, CITY.max, road);
    fill(&mut px, AIRPORT.min, AIRPORT.max, [76, 104, 64, 255]);
    fill(&mut px, QUAY.min, QUAY.max, [104, 106, 110, 255]);
    for &z in &PIERS_Z {
        fill(&mut px, Vec2::new(PIER_X.0, z - PIER_HALF_WIDTH), Vec2::new(PIER_X.1, z + PIER_HALF_WIDTH), [104, 106, 110, 255]);
    }
    fill(&mut px, Vec2::new(-CITY_HALF, CITY_HALF), Vec2::new(CITY_HALF, CITY_HALF + PROMENADE_DEPTH), [170, 165, 150, 255]);
    for a in [RUNWAY, TAXIWAY, AIRPORT_ROAD] {
        fill(&mut px, a.min, a.max, road);
    }
    fill(&mut px, APRON.min, APRON.max, [140, 142, 145, 255]);
    for bx in 0..GRID {
        for bz in 0..GRID {
            let a = Vec2::new(block_min(bx), block_min(bz));
            fill(&mut px, a, a + Vec2::splat(BLOCK), block);
        }
    }
    for (a, b, kind) in &info.footprints {
        let c = match kind {
            Foot::Building => [88, 92, 98, 255],
            Foot::Park => [52, 100, 54, 255],
            Foot::Water => [46, 82, 115, 255],
        };
        fill(&mut px, *a, *b, c);
    }
    let data: Vec<u8> = px.into_iter().flatten().collect();
    Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD)
}

fn arrow_image() -> Image {
    let n = 32u32;
    let mut data = vec![0u8; (n * n * 4) as usize];
    for y in 0..n {
        for x in 0..n {
            let u = (x as f32 + 0.5) / n as f32 - 0.5;
            let v = (y as f32 + 0.5) / n as f32 - 0.5;
            // arrow pointing up: inside triangle with a notch
            let inside = v > -0.45 && v < 0.4 && u.abs() < (v + 0.45) * 0.55 && !(v > 0.15 && u.abs() < (v - 0.15) * 1.3);
            let edge = v > -0.48 && v < 0.43 && u.abs() < (v + 0.48) * 0.6 && !inside;
            let i = ((y * n + x) * 4) as usize;
            let c = if inside { [255, 255, 255, 255] } else if edge { [10, 10, 10, 255] } else { [0, 0, 0, 0] };
            data[i..i + 4].copy_from_slice(&c);
        }
    }
    Image::new(Extent3d { width: n, height: n, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD)
}

fn setup_hud(mut commands: Commands, info: Res<CityInfo>, mut images: ResMut<Assets<Image>>, mut fonts: ResMut<Assets<Font>>) {
    let hf = load_hud_fonts(&mut fonts);
    commands.insert_resource(hf.clone());
    let map = images.add(build_map_image(&info));
    commands.insert_resource(MapHandle(map.clone()));
    let arrow = images.add(arrow_image());
    let map_size = Vec2::new((MAP_MAX.x - MAP_MIN.x) / MAP_MPP, (MAP_MAX.y - MAP_MIN.y) / MAP_MPP);

    // money (top right)
    commands.spawn((
        Text::new("$0"),
        font(&hf, 44.0, 800),
        TextColor(Color::srgb(0.47, 0.86, 0.42)),
        shadow(),
        Node { position_type: PositionType::Absolute, top: px(22), right: px(34), ..default() },
        MoneyText,
    ));
    commands.spawn((
        Text::new(""),
        TextFont { font: hf.symbol.clone(), font_size: FontSize::Px(34.0), ..default() },
        TextColor(Color::srgb(1.0, 1.0, 1.0)),
        shadow(),
        Node { position_type: PositionType::Absolute, top: px(78), right: px(34), ..default() },
        StarsText,
    ));
    commands.spawn((
        Text::new(""),
        font(&hf, 22.0, 700),
        TextColor(Color::srgb(0.95, 0.95, 0.95)),
        shadow(),
        Node { position_type: PositionType::Absolute, top: px(124), right: px(34), ..default() },
        WeaponText,
    ));
    commands.spawn((
        Node { position_type: PositionType::Absolute, left: percent(50), top: percent(50), width: px(6), height: px(6), margin: UiRect { left: px(-3), top: px(-3), ..default() }, border_radius: BorderRadius::all(px(3)), ..default() },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
        Visibility::Hidden,
        Crosshair,
    ));
    // fps (top left)
    commands.spawn((
        Text::new(""),
        font(&hf, 16.0, 600),
        TextColor(Color::srgb(0.9, 0.9, 0.6)),
        shadow(),
        Node { position_type: PositionType::Absolute, top: px(8), left: px(10), ..default() },
        FpsText,
    ));

    // minimap (bottom left) with health + armour bars underneath
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: px(28),
            bottom: px(26),
            flex_direction: FlexDirection::Column,
            row_gap: px(5),
            ..default()
        })
        .with_children(|col| {
            col.spawn((
                Node {
                    width: px(MINIMAP),
                    height: px(MINIMAP),
                    overflow: Overflow::clip(),
                    border: UiRect::all(px(3)),
                    border_radius: BorderRadius::all(px(10)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.18, 0.32, 0.45)),
                BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.85)),
                MinimapFrame,
            ))
            .with_children(|mm| {
                mm.spawn((
                    Node { position_type: PositionType::Absolute, left: px(0), top: px(0), width: px(MINIMAP), height: px(MINIMAP), ..default() },
                    UiTransform::default(),
                    MapRotator,
                ))
                .with_children(|rot| {
                    rot.spawn((
                        ImageNode::new(map),
                        Node { position_type: PositionType::Absolute, width: px(map_size.x), height: px(map_size.y), ..default() },
                        MapImage,
                    ));
                    rot.spawn((
                        Text::new("N"),
                        font(&hf, 18.0, 800),
                        TextColor(Color::WHITE),
                        shadow(),
                        Node { position_type: PositionType::Absolute, left: px(MINIMAP * 0.5 - 6.0), top: px(4), ..default() },
                    ));
                });
                mm.spawn((
                    ImageNode::new(arrow),
                    Node { position_type: PositionType::Absolute, left: px(MINIMAP * 0.5 - 12.0), top: px(MINIMAP * 0.5 - 12.0), width: px(24), height: px(24), ..default() },
                    UiTransform::default(),
                    PlayerArrow,
                ));
            });
            col.spawn((Node { width: px(MINIMAP), height: px(9), column_gap: px(4), ..default() },)).with_children(|bars| {
                bars.spawn((Node { width: percent(66), height: percent(100), ..default() }, BackgroundColor(Color::srgba(0.1, 0.25, 0.1, 0.85)))).with_children(|b| {
                    b.spawn((Node { width: percent(100), height: percent(100), ..default() }, BackgroundColor(Color::srgb(0.32, 0.78, 0.3)), HealthFill));
                });
                bars.spawn((Node { width: percent(33), height: percent(100), ..default() }, BackgroundColor(Color::srgba(0.08, 0.15, 0.3, 0.85)))).with_children(|b| {
                    b.spawn((Node { width: percent(0), height: percent(100), ..default() }, BackgroundColor(Color::srgb(0.3, 0.55, 0.95)), ArmorFill));
                });
            });
        });

    // speedometer (bottom right)
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(40),
                bottom: px(30),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                ..default()
            },
            Visibility::Hidden,
            SpeedRoot,
        ))
        .with_children(|c| {
            c.spawn((Text::new(""), font(&hf, 20.0, 700), TextColor(Color::srgb(0.95, 0.95, 0.95)), shadow(), CarName));
            c.spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::FlexEnd, column_gap: px(6), ..default() }).with_children(|r| {
                r.spawn((Text::new("0"), font(&hf, 64.0, 800), TextColor(Color::WHITE), shadow(), SpeedText));
                r.spawn((Text::new("KM/H"), font(&hf, 20.0, 700), TextColor(Color::srgb(0.85, 0.85, 0.85)), shadow(), Node { margin: UiRect::bottom(px(12)), ..default() }));
            });
            c.spawn((Node { width: px(180), height: px(7), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)))).with_children(|b| {
                b.spawn((Node { width: percent(100), height: percent(100), ..default() }, BackgroundColor(Color::srgb(0.95, 0.65, 0.15)), CarHealthFill));
            });
        });

    // hint + messages (bottom centre), big banner (centre)
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            bottom: px(120),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(8),
            ..default()
        })
        .with_children(|c| {
            c.spawn((Text::new(""), font(&hf, 24.0, 700), TextColor(Color::srgb(1.0, 0.95, 0.75)), shadow(), MessageText));
            c.spawn((Text::new(""), font(&hf, 20.0, 600), TextColor(Color::WHITE), shadow(), HintText));
        });
    commands
        .spawn(Node { position_type: PositionType::Absolute, width: percent(100), top: percent(38), justify_content: JustifyContent::Center, ..default() })
        .with_children(|c| {
            c.spawn((Text::new(""), font(&hf, 110.0, 900), TextColor(Color::srgb(0.85, 0.1, 0.1)), TextShadow { offset: Vec2::new(4.0, 4.0), color: Color::BLACK }, BigText));
        });
}

fn money_str(m: i64) -> String {
    let s = m.abs().to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if m < 0 { format!("-${out}") } else { format!("${out}") }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_hud(
    player: Query<(&Player, &Transform)>,
    cars: Query<(&crate::seats::Seat, &Velocity, &Transform, Option<&Vehicle>, Option<&crate::aircraft::Aircraft>)>,
    diag: Res<DiagnosticsStore>,
    mut hud_msgs: ResMut<HudMessages>,
    input: Res<crate::input::GameInput>,
    mut texts: ParamSet<(
        Query<&mut Text, With<MoneyText>>,
        Query<&mut Text, With<SpeedText>>,
        Query<&mut Text, With<CarName>>,
        Query<&mut Text, With<HintText>>,
        Query<&mut Text, With<FpsText>>,
    )>,
    mut nodes: ParamSet<(Query<&mut Node, With<HealthFill>>, Query<&mut Node, With<ArmorFill>>, Query<&mut Node, With<CarHealthFill>>)>,
    mut speed_root: Query<&mut Visibility, With<SpeedRoot>>,
    mut shown_money: Local<f64>,
    time: Res<Time>,
) {
    let Ok((p, pt)) = player.single() else { return };
    // money counts up/down like a till
    let target = p.money as f64;
    let d = target - *shown_money;
    *shown_money += d * (time.delta_secs_f64() * 6.0).min(1.0);
    if d.abs() < 1.0 {
        *shown_money = target;
    }
    if let Ok(mut t) = texts.p0().single_mut() {
        t.0 = money_str(shown_money.round() as i64);
    }
    if let Ok(mut n) = nodes.p0().single_mut() {
        n.width = percent(p.health.clamp(0.0, 100.0));
    }
    if let Ok(mut n) = nodes.p1().single_mut() {
        n.width = percent(p.armor.clamp(0.0, 100.0));
    }
    let in_car = p.vehicle.and_then(|v| cars.get(v).ok());
    if let Ok(mut vis) = speed_root.single_mut() {
        *vis = if in_car.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    }
    if let Some((seat, vel, t, veh, air)) = in_car {
        let kmh = if air.is_some() { vel.linear.length() * 3.6 } else { (vel.linear.dot(t.rotation * Vec3::NEG_Z)).abs() * 3.6 };
        if let Ok(mut tx) = texts.p1().single_mut() {
            tx.0 = format!("{:.0}", kmh);
        }
        if let Ok(mut tx) = texts.p2().single_mut() {
            tx.0 = match air {
                Some(a) => format!("{}   ALT {:.0} m   THR {:.0}%", seat.name.to_uppercase(), (t.translation.y - SEA_LEVEL).max(0.0), a.throttle * 100.0),
                None => seat.name.to_uppercase(),
            };
        }
        let health = veh.map(|v| v.health).or(air.map(|a| a.health)).unwrap_or(1000.0);
        if let Ok(mut n) = nodes.p2().single_mut() {
            n.width = percent((health / 10.0).clamp(0.0, 100.0));
        }
    }
    // context hint
    let mut hint = String::new();
    if p.vehicle.is_none() && p.health > 0.0 {
        for (seat, _, t, _, _) in &cars {
            if !seat.broken && crate::seats::dist_to_seat(pt.translation, t, seat) < 2.2 {
                hint = format!("Press F to enter the {}", seat.name);
                break;
            }
        }
    } else if let Some((_, _, _, Some(v), _)) = in_car {
        if v.burn_t.is_some() && !v.wrecked {
            hint = "It's on fire - press F to bail out!".into();
        }
    }
    if hint.is_empty() {
        hint = std::mem::take(&mut hud_msgs.extra_hint);
    } else {
        hud_msgs.extra_hint.clear();
    }
    if let Ok(mut tx) = texts.p3().single_mut() {
        tx.0 = hint;
    }
    if input.toggle_fps {
        hud_msgs.show_fps = !hud_msgs.show_fps;
    }
    if let Ok(mut tx) = texts.p4().single_mut() {
        tx.0 = if hud_msgs.show_fps {
            let fps = diag.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()).unwrap_or(0.0);
            format!("{fps:.0} FPS")
        } else {
            String::new()
        };
    }
}

fn update_messages(
    time: Res<Time>,
    mut msgs: ResMut<HudMessages>,
    mut texts: ParamSet<(Query<&mut Text, With<MessageText>>, Query<(&mut Text, &mut TextColor), With<BigText>>)>,
) {
    let dt = time.delta_secs();
    for m in msgs.queue.iter_mut() {
        m.1 -= dt;
    }
    msgs.queue.retain(|m| m.1 > 0.0);
    let line = msgs.queue.last().map(|m| m.0.clone()).unwrap_or_default();
    if let Ok(mut t) = texts.p0().single_mut() {
        t.0 = line;
    }
    let mut clear = false;
    if let Some(b) = msgs.big.as_mut() {
        b.2 -= dt;
        clear = b.2 <= 0.0;
    }
    if clear {
        msgs.big = None;
    }
    if let Ok((mut t, mut c)) = texts.p1().single_mut() {
        match &msgs.big {
            Some((s, col, _)) => {
                t.0 = s.clone();
                c.0 = *col;
            }
            None => t.0.clear(),
        }
    }
}

#[allow(clippy::type_complexity)]
fn update_minimap(
    rig: Res<CameraRig>,
    player: Query<(&Player, &Transform)>,
    cars: Query<(&Transform, &Velocity), With<Vehicle>>,
    mut q: ParamSet<(Query<&mut Node, With<MapImage>>, Query<&mut UiTransform, With<MapRotator>>, Query<&mut UiTransform, With<PlayerArrow>>, Query<&mut Node, With<MapImage>>)>,
    mut zoom: ResMut<MinimapZoom>,
) {
    let Ok((p, pt)) = player.single() else { return };
    let (pos, heading, speed) = match p.vehicle.and_then(|v| cars.get(v).ok()) {
        Some((t, v)) => (t.translation, crate::camera::yaw_of(t.rotation), v.linear.length()),
        None => (pt.translation, crate::camera::yaw_of(pt.rotation), 0.0),
    };
    let want = if speed > 15.0 { 0.65 } else { 1.0 };
    zoom.0 += (want - zoom.0) * 0.03;
    let z = zoom.0;
    let map_px = (Vec2::new(pos.x, pos.z) - MAP_MIN) / MAP_MPP;
    let size = (MAP_MAX - MAP_MIN) / MAP_MPP;
    if let Ok(mut n) = q.p0().single_mut() {
        n.left = px(MINIMAP * 0.5 - map_px.x * z);
        n.top = px(MINIMAP * 0.5 - map_px.y * z);
        n.width = px(size.x * z);
        n.height = px(size.y * z);
    }
    // the map turns with the camera so "up" is always where the camera looks
    if let Ok(mut t) = q.p1().single_mut() {
        t.rotation = Rot2::radians(rig.yaw);
    }
    if let Ok(mut t) = q.p2().single_mut() {
        t.rotation = Rot2::radians(rig.yaw - heading);
    }
}

#[allow(clippy::type_complexity)]
fn update_combat_hud(
    time: Res<Time>,
    wanted: Res<crate::wanted::Wanted>,
    ars: Res<crate::weapons::Arsenal>,
    player: Query<&Player>,
    mut texts: ParamSet<(Query<(&mut Text, &mut TextColor), With<StarsText>>, Query<&mut Text, With<WeaponText>>)>,
    mut cross: Query<&mut Visibility, With<Crosshair>>,
    rig: Res<crate::camera::CameraRig>,
) {
    let Ok(p) = player.single() else { return };
    if let Ok((mut t, mut c)) = texts.p0().single_mut() {
        let n = wanted.stars as usize;
        t.0 = if n == 0 { String::new() } else { format!("{}{}", "★".repeat(n), "☆".repeat(5 - n)) };
        // stars blink while the cops have lost sight of you
        let blink = !wanted.seen && (time.elapsed_secs() * 3.0) as i32 % 2 == 0;
        c.0 = if blink { Color::srgba(1.0, 1.0, 1.0, 0.25) } else { Color::WHITE };
    }
    let w = ars.weapon();
    let st = w.stats();
    if let Ok(mut t) = texts.p1().single_mut() {
        t.0 = if p.vehicle.is_some() {
            String::new()
        } else if st.clip == 0 {
            st.name.to_uppercase()
        } else if ars.reloading > 0.0 {
            format!("{}  RELOADING", st.name.to_uppercase())
        } else {
            let (clip, res) = ars.ammo[ars.current];
            format!("{}  {} / {}", st.name.to_uppercase(), clip, res)
        };
    }
    if let Ok(mut v) = cross.single_mut() {
        // only while aiming (or right after a shot), and not through a scope
        let show = p.vehicle.is_none() && p.health > 0.0 && !st.melee() && (rig.aim || ars.cooldown > -0.6) && !(rig.aim && st.aim_fov < 20.0);
        *v = if show { Visibility::Inherited } else { Visibility::Hidden };
    }
}

