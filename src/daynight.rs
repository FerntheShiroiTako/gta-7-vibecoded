//! Day / night cycle: the sun crosses the sky (the atmosphere colours dawn and
//! dusk), exposure adapts, and at night the city lights up - lit office and
//! flat windows, glowing street lamps with real light near the camera, and car
//! headlights.

use bevy::camera::Exposure;
use bevy::light::{EnvironmentMapLight, GeneratedEnvironmentMapLight};
use bevy::pbr::DistanceFog;
use bevy::prelude::*;

use crate::camera::{MainCamera, Sun};
use crate::city::{CityInfo, CityMats, M};
use crate::player::Player;
use crate::seats::{Seat, SeatKind};

pub struct DayNightPlugin;

impl Plugin for DayNightPlugin {
    fn build(&self, app: &mut App) {
        let start = std::env::var("HH_HOUR").ok().and_then(|h| h.parse().ok()).unwrap_or(17.0);
        app.insert_resource(TimeOfDay { hours: start, minutes_per_second: 1.0 })
            .add_systems(Startup, setup_night_lights)
            .add_systems(Update, (advance_time, apply_time_of_day, street_lights, headlights, clock_text).chain());
    }
}

#[derive(Resource)]
pub struct TimeOfDay {
    pub hours: f32,
    pub minutes_per_second: f32,
}

impl TimeOfDay {
    /// Sun elevation in radians (negative at night).
    pub fn sun_elevation(&self) -> f32 {
        let day = ((self.hours - 6.0) / 12.0 * std::f32::consts::PI).sin();
        day * 1.15 - 0.05
    }
    /// 0 = full day, 1 = full night.
    pub fn night(&self) -> f32 {
        let e = self.sun_elevation();
        ((0.12 - e) / 0.3).clamp(0.0, 1.0)
    }
}

#[derive(Component)]
struct LampLight;
#[derive(Component)]
struct HeadLight;
#[derive(Component)]
struct ClockText;

const LAMP_POOL: usize = 28;

fn setup_night_lights(mut commands: Commands) {
    for _ in 0..LAMP_POOL {
        commands.spawn((
            PointLight { intensity: 0.0, range: 26.0, radius: 0.3, color: Color::srgb(1.0, 0.82, 0.55), shadow_maps_enabled: false, ..default() },
            Transform::default(),
            Visibility::Hidden,
            LampLight,
        ));
    }
    for _ in 0..2 {
        commands.spawn((
            SpotLight { intensity: 0.0, range: 70.0, color: Color::srgb(1.0, 0.95, 0.85), outer_angle: 0.55, inner_angle: 0.3, shadow_maps_enabled: true, ..default() },
            Transform::default(),
            Visibility::Hidden,
            HeadLight,
        ));
    }
    commands.spawn((
        Text::new(""),
        TextColor(Color::srgb(0.92, 0.92, 0.92)),
        TextShadow::default(),
        TextFont { font_size: bevy::text::FontSize::Px(22.0), ..default() },
        Node { position_type: PositionType::Absolute, top: px(158), right: px(34), ..default() },
        ClockText,
    ));
}

fn advance_time(time: Res<Time>, keys: Res<ButtonInput<KeyCode>>, mut tod: ResMut<TimeOfDay>, mut hud: ResMut<crate::hud::HudMessages>) {
    tod.hours = (tod.hours + time.delta_secs() * tod.minutes_per_second / 60.0).rem_euclid(24.0);
    if keys.just_pressed(KeyCode::F2) {
        tod.hours = (tod.hours + 3.0).rem_euclid(24.0);
        hud.push("Time +3 h");
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_time_of_day(
    tod: Res<TimeOfDay>,
    mats: Option<Res<CityMats>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut sun: Query<(&mut Transform, &mut DirectionalLight), With<Sun>>,
    mut cam: Query<(&mut Exposure, Option<&mut GeneratedEnvironmentMapLight>, Option<&mut EnvironmentMapLight>, Option<&mut DistanceFog>), With<MainCamera>>,
    mut last: Local<f32>,
) {
    let night = tod.night();
    let elev = tod.sun_elevation();
    // the sun travels east -> south -> west
    let az = (tod.hours - 12.0) / 12.0 * std::f32::consts::PI;
    if let Ok((mut t, mut l)) = sun.single_mut() {
        // rises east, due south at noon, sets west; below the horizon the same
        // light becomes a dim moon high in the opposite sky
        let (dir_to_light, illum) = if elev > -0.02 {
            (Vec3::new(-az.sin() * elev.cos(), elev.sin(), az.cos() * elev.cos()), bevy::light::light_consts::lux::RAW_SUNLIGHT * (elev / 0.15).clamp(0.0, 1.0))
        } else {
            (Vec3::new(az.sin() * 0.6, 0.75, -az.cos() * 0.4), 18.0 * night)
        };
        *t = Transform::from_translation(Vec3::ZERO).looking_to(-dir_to_light.normalize(), Vec3::Y);
        l.illuminance = illum;
    }
    if let Ok((mut ex, gen_env, env, fog)) = cam.single_mut() {
        let ev = 13.2 - night * 7.0;
        if ex.ev100 != ev {
            ex.ev100 = ev;
        }
        // keep sky light relative to exposure: daylight ratio ~2.1, a dark
        // night is ~4% of the day's apparent ambient
        let ambient = 1.0 - night * 0.96;
        let intensity = 2.13 * 2f32.powf(ev) * ambient;
        if let Some(mut g) = gen_env {
            if (g.intensity - intensity).abs() > intensity * 1e-3 {
                g.intensity = intensity;
            }
        }
        if let Some(mut e) = env {
            if (e.intensity - intensity).abs() > intensity * 1e-3 {
                e.intensity = intensity;
            }
        }
        if let Some(mut fog) = fog {
            let k = 1.0 - night * 0.9;
            fog.color = Color::srgba(0.62 * k, 0.7 * k, 0.8 * k, 1.0);
        }
    }
    // windows and lamp heads only need updating when the light level changes
    if (night - *last).abs() < 0.01 {
        return;
    }
    *last = night;
    let Some(mats) = mats else { return };
    let window = LinearRgba::rgb(3.0, 2.4, 1.6) * (night * night * 16.0);
    let lamp = LinearRgba::rgb(1.0, 0.8, 0.55) * (0.6 + night * 300.0);
    let facades = [M::Office, M::Res, M::Ind];
    let mut handles: Vec<Handle<StandardMaterial>> = facades.iter().map(|m| mats.get(*m)).collect();
    handles.extend(mats.tinted.iter().filter(|((m, _), _)| facades.contains(m)).map(|(_, h)| h.clone()));
    for h in &handles {
        if let Some(mut m) = materials.get_mut(h) {
            m.emissive = window;
        }
    }
    let mut lamps: Vec<Handle<StandardMaterial>> = vec![mats.get(M::Lamp)];
    lamps.extend(mats.tinted.iter().filter(|((m, _), _)| *m == M::Lamp).map(|(_, h)| h.clone()));
    for h in &lamps {
        if let Some(mut m) = materials.get_mut(h) {
            m.emissive = lamp;
        }
    }
}

/// Real point lights on the street lamps nearest the camera.
fn street_lights(tod: Res<TimeOfDay>, info: Res<CityInfo>, cam: Query<&GlobalTransform, With<MainCamera>>, mut lights: Query<(&mut PointLight, &mut Transform, &mut Visibility), With<LampLight>>, mut timer: Local<u32>) {
    let night = tod.night();
    *timer += 1;
    if night < 0.05 {
        for (mut l, _, mut v) in &mut lights {
            if l.intensity != 0.0 {
                l.intensity = 0.0;
            }
            if *v != Visibility::Hidden {
                *v = Visibility::Hidden;
            }
        }
        return;
    }
    if *timer % 10 != 0 {
        return;
    }
    let Ok(c) = cam.single() else { return };
    let eye = c.translation();
    let mut near: Vec<(f32, Vec3)> = info.lamps.iter().map(|p| (p.distance_squared(eye), *p)).collect();
    let k = LAMP_POOL.min(near.len().saturating_sub(1));
    near.select_nth_unstable_by(k, |a, b| a.0.partial_cmp(&b.0).unwrap());
    for (i, (mut l, mut t, mut v)) in lights.iter_mut().enumerate() {
        if let Some((_, p)) = near.get(i) {
            t.translation = *p - Vec3::Y * 0.3;
            l.intensity = 260_000.0 * night;
            *v = Visibility::Inherited;
        } else {
            l.intensity = 0.0;
            *v = Visibility::Hidden;
        }
    }
}

/// Headlight spot lamps on the player's car after dark.
fn headlights(
    tod: Res<TimeOfDay>,
    player: Query<&Player>,
    cars: Query<(&Transform, &Seat), Without<HeadLight>>,
    mut lights: Query<(&mut SpotLight, &mut Transform, &mut Visibility), With<HeadLight>>,
) {
    let night = tod.night();
    let car = player.single().ok().and_then(|p| p.vehicle).and_then(|v| cars.get(v).ok()).filter(|(_, s)| s.kind == SeatKind::Car);
    for (i, (mut l, mut t, mut v)) in lights.iter_mut().enumerate() {
        let on = car.is_some() && night > 0.1;
        let want = if on { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
        match car {
            Some((ct, seat)) if night > 0.1 => {
                let side = if i == 0 { -0.6 } else { 0.6 };
                let pos = ct.transform_point(Vec3::new(side, 0.35, -seat.half.z - 0.1));
                let fwd = ct.rotation * Vec3::new(0.0, -0.08, -1.0);
                *t = Transform::from_translation(pos).looking_to(fwd, Vec3::Y);
                l.intensity = 2_500_000.0 * night;
            }
            _ => {
                if l.intensity != 0.0 {
                    l.intensity = 0.0;
                }
            }
        }
    }
}

fn clock_text(tod: Res<TimeOfDay>, mut q: Query<&mut Text, With<ClockText>>) {
    if let Ok(mut t) = q.single_mut() {
        let h = tod.hours.floor() as u32;
        let m = ((tod.hours.fract()) * 60.0) as u32;
        t.0 = format!("{h:02}:{m:02}");
    }
}
