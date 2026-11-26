//! Wanted level, police dispatch and pursuit, cops on foot, busted.

use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::humanoid::{HumanoidAssets, Look};
use crate::lighting::NoRaytrace;
use crate::meshgen::{MeshBuilder, Rng, WHITE};
use crate::peds::{spawn_ped, Ped, PedState};
use crate::player::Player;
use crate::seats::Seat;
use crate::vehicle::{spawn_car, CarKind, Driver, Vehicle, VehicleAssets, VehicleControl};
use crate::world::*;

pub struct WantedPlugin;

impl Plugin for WantedPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Wanted>()
            .init_resource::<Panic>()
            .init_resource::<CopRng>()
            .add_systems(Startup, setup_police_assets)
            .add_systems(PreUpdate, police_drive.after(crate::input::GameInputSet).after(crate::vehicle::VehicleControlSet))
            .add_systems(Update, (police_dispatch, police_sight, cops_on_foot, flash_lightbars, busted_check));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crime {
    Gunfire,
    Assault,
    Murder,
    ShootCop,
    KillCop,
    Carjack,
}

#[derive(Resource, Default)]
pub struct Wanted {
    pub stars: u32,
    pub last_seen: Vec3,
    pub unseen_t: f32,
    pub seen: bool,
    pub bust_t: f32,
}

impl Wanted {
    pub fn add(&mut self, c: Crime) {
        let (min, add) = match c {
            Crime::Gunfire => (1, 0),
            Crime::Assault | Crime::Carjack => (1, 0),
            Crime::Murder => (2, 0),
            Crime::ShootCop => (3, 0),
            Crime::KillCop => (3, 1),
        };
        let before = self.stars;
        self.stars = (self.stars.max(min) + if before > 0 { add } else { 0 }).min(5);
        if self.stars > before || matches!(c, Crime::KillCop | Crime::Murder) {
            self.unseen_t = 0.0;
        }
    }
    pub fn clear(&mut self) {
        self.stars = 0;
        self.unseen_t = 0.0;
        self.bust_t = 0.0;
    }
    /// seconds out of sight needed to lose the cops
    pub fn escape_time(&self) -> f32 {
        8.0 + self.stars as f32 * 5.0
    }
}

/// Recent scary things (gunfire, explosions) pedestrians react to.
#[derive(Resource, Default)]
pub struct Panic(pub Vec<(Vec3, f32)>);

pub fn report_crime(commands: &mut Commands, c: Crime, pos: Vec3) {
    commands.queue(move |w: &mut World| {
        if let Some(mut wl) = w.get_resource_mut::<Wanted>() {
            wl.add(c);
            wl.last_seen = pos;
        }
        if let Some(mut p) = w.get_resource_mut::<Panic>() {
            p.0.push((pos, if matches!(c, Crime::Gunfire | Crime::Murder | Crime::KillCop) { 60.0 } else { 15.0 }));
        }
    });
}

#[derive(Resource)]
struct CopRng(Rng);
impl Default for CopRng {
    fn default() -> Self {
        CopRng(Rng::new(911))
    }
}

#[derive(Component)]
pub struct PoliceCar {
    pub route: Option<IVec2>,
    pub deployed: bool,
    pub stuck_t: f32,
    pub reverse_t: f32,
    pub lights: [Entity; 2],
}

#[derive(Resource)]
struct PoliceAssets {
    bar: Handle<Mesh>,
    red: Handle<StandardMaterial>,
    blue: Handle<StandardMaterial>,
    off: Handle<StandardMaterial>,
}

fn setup_police_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let mut bar = MeshBuilder::new();
    bar.box_at(Affine3A::IDENTITY, Vec3::new(0.28, 0.06, 0.12), WHITE, 1.0, true, true);
    let glow = |e: LinearRgba| StandardMaterial { base_color: Color::BLACK, emissive: e, ..default() };
    commands.insert_resource(PoliceAssets {
        bar: meshes.add(bar.build()),
        red: materials.add(glow(LinearRgba::rgb(60.0, 2.0, 2.0))),
        blue: materials.add(glow(LinearRgba::rgb(2.0, 8.0, 60.0))),
        off: materials.add(StandardMaterial { base_color: Color::srgb(0.15, 0.15, 0.18), ..default() }),
    });
}

pub fn spawn_police_car(commands: &mut Commands, va: &VehicleAssets, pa_bar: (&Handle<Mesh>, &Handle<StandardMaterial>), materials: &mut Assets<StandardMaterial>, pos: Vec3, yaw: f32, seed: u32) -> Entity {
    let car = spawn_car(commands, va, materials, CarKind::Sedan, pos, yaw, Color::srgb(0.92, 0.92, 0.94), Driver::Cop(seed));
    let s = crate::vehicle::spec(CarKind::Sedan);
    let roof = (s.shape.roof_front.0 + s.shape.roof_rear.0) * 0.5;
    let l = commands.spawn((Mesh3d(pa_bar.0.clone()), MeshMaterial3d(pa_bar.1.clone()), Transform::from_xyz(-0.3, s.roof + 0.08, roof), NoRaytrace)).id();
    let r = commands.spawn((Mesh3d(pa_bar.0.clone()), MeshMaterial3d(pa_bar.1.clone()), Transform::from_xyz(0.3, s.roof + 0.08, roof), NoRaytrace)).id();
    commands.entity(car).add_children(&[l, r]).insert(PoliceCar { route: None, deployed: false, stuck_t: 0.0, reverse_t: 0.0, lights: [l, r] });
    car
}

fn player_target(p: &Player, pt: &Transform, seats: &Query<&Transform, With<Seat>>) -> Vec3 {
    p.vehicle.and_then(|v| seats.get(v).ok()).map(|t| t.translation).unwrap_or(pt.translation)
}

#[allow(clippy::too_many_arguments)]
fn police_dispatch(
    mut commands: Commands,
    wanted: Res<Wanted>,
    player: Query<(&Player, &Transform)>,
    seats: Query<&Transform, With<Seat>>,
    police: Query<(Entity, &Transform, &Seat), With<PoliceCar>>,
    cops: Query<(Entity, &Transform, &Ped)>,
    va: Res<VehicleAssets>,
    pa: Res<PoliceAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rng: ResMut<CopRng>,
    time: Res<Time>,
    mut timer: Local<f32>,
) {
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 1.0;
    let Ok((p, pt)) = player.single() else { return };
    let target = player_target(p, pt, &seats);
    let desired = [0usize, 1, 2, 4, 6, 8][wanted.stars.min(5) as usize];
    let active: Vec<_> = police.iter().filter(|(_, _, s)| matches!(s.driver, Driver::Cop(_))).collect();
    // send units home once the heat is off (or they are hopelessly far behind)
    for (e, t, s) in police.iter() {
        let d = t.translation.distance(target);
        if (wanted.stars == 0 && d > 90.0) || d > 420.0 {
            if s.driver != Driver::Player {
                commands.entity(e).despawn();
            }
        }
    }
    if wanted.stars == 0 {
        for (e, t, ped) in &cops {
            if ped.cop && t.translation.distance(target) > 90.0 {
                commands.entity(e).despawn();
            }
        }
        return;
    }
    if active.len() >= desired {
        return;
    }
    // spawn on a road intersection 90-200 m away, facing the player
    let mut spots = Vec::new();
    for i in 0..=GRID {
        for j in 0..=GRID {
            let pos = Vec3::new(road_center(i) + LANE * 0.5, 0.6, road_center(j) + LANE * 0.5);
            if (90.0..200.0).contains(&pos.distance(target)) {
                spots.push(pos);
            }
        }
    }
    if !spots.is_empty() {
        let pos = spots[rng.0.int(0, spots.len() as i32 - 1) as usize];
        let dir = (target - pos).with_y(0.0).normalize_or(Vec3::Z);
        let yaw = (-dir.x).atan2(-dir.z);
        let seed = rng.0.next_u32();
        spawn_police_car(&mut commands, &va, (&pa.bar, &pa.off), &mut materials, pos, yaw, seed);
    }
}

/// Pursuit driving: chase, ram, stop next to a player on foot.
#[allow(clippy::type_complexity)]
fn police_drive(
    time: Res<Time>,
    wanted: Res<Wanted>,
    player: Query<(&Player, &Transform), Without<PoliceCar>>,
    seats: Query<&Transform, (With<Seat>, Without<PoliceCar>)>,
    mut cars: Query<(&Transform, &Velocity, &Seat, &mut VehicleControl, &mut PoliceCar)>,
) {
    let dt = time.delta_secs();
    let Ok((p, pt)) = player.single() else { return };
    let target = p.vehicle.and_then(|v| seats.get(v).ok()).map(|t| t.translation).unwrap_or(pt.translation);
    for (t, vel, seat, mut c, mut pc) in &mut cars {
        if !matches!(seat.driver, Driver::Cop(_)) {
            continue;
        }
        if wanted.stars == 0 {
            *c = VehicleControl { throttle: 0.4, ..default() };
            continue;
        }
        // follow the road grid junction to junction until close, then go direct
        let direct = t.translation.distance(target) < 45.0;
        let goal = if direct {
            pc.route = None;
            target
        } else {
            let here = IVec2::new(((t.translation.x - road_center(0)) / PITCH).round() as i32, ((t.translation.z - road_center(0)) / PITCH).round() as i32).clamp(IVec2::ZERO, IVec2::splat(GRID));
            let node_pos = |n: IVec2| Vec3::new(road_center(n.x), t.translation.y, road_center(n.y));
            let node = pc.route.unwrap_or(here);
            let reached = node_pos(node).with_y(0.0).distance(t.translation.with_y(0.0)) < 8.0;
            let next = if reached || pc.route.is_none() {
                let start = if reached { node } else { here };
                let mut best = start;
                let mut bd = node_pos(start).distance(target);
                for d in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                    let n = start + d;
                    if n.min_element() < 0 || n.max_element() > GRID {
                        continue;
                    }
                    let dd = node_pos(n).distance(target);
                    if dd < bd {
                        bd = dd;
                        best = n;
                    }
                }
                if pc.route.is_none() && !reached { start } else { best }
            } else {
                node
            };
            pc.route = Some(next);
            node_pos(next)
        };
        let local = t.rotation.inverse() * (goal - t.translation);
        let dist = t.translation.distance(target);
        let angle = local.x.atan2(-local.z);
        let speed = vel.linear.length();
        let on_foot = p.vehicle.is_none();
        if pc.reverse_t > 0.0 {
            pc.reverse_t -= dt;
            *c = VehicleControl { throttle: 0.0, brake: 1.0, steer: -angle.signum(), handbrake: false };
            continue;
        }
        let mut throttle = 1.0;
        let mut brake = 0.0;
        if on_foot && dist < 22.0 {
            throttle = 0.0;
            brake = 1.0;
        } else if angle.abs() > 1.2 && speed > 12.0 {
            throttle = 0.3;
        }
        if throttle > 0.5 && speed < 1.0 {
            pc.stuck_t += dt;
            if pc.stuck_t > 2.0 {
                pc.stuck_t = 0.0;
                pc.reverse_t = 1.4;
            }
        } else {
            pc.stuck_t = 0.0;
        }
        *c = VehicleControl { throttle, brake, steer: (angle * 2.0).clamp(-1.0, 1.0), handbrake: angle.abs() > 1.4 && speed > 15.0 };
    }
}

/// Line of sight from any cop to the player decides whether the stars stay lit.
#[allow(clippy::too_many_arguments)]
fn police_sight(
    time: Res<Time>,
    mut wanted: ResMut<Wanted>,
    player: Query<(Entity, &Player, &Transform)>,
    seats: Query<&Transform, With<Seat>>,
    police: Query<(&Transform, &Seat), With<PoliceCar>>,
    cops: Query<(&Transform, &Ped)>,
    rapier: ReadRapierContext,
    mut hud: ResMut<crate::hud::HudMessages>,
    mut timer: Local<f32>,
) {
    if wanted.stars == 0 {
        return;
    }
    let Ok((pe, p, pt)) = player.single() else { return };
    let target = player_target(p, pt, &seats) + Vec3::Y * 1.0;
    *timer -= time.delta_secs();
    if *timer <= 0.0 {
        *timer = 0.25;
        let Ok(ctx) = rapier.single() else { return };
        let mut eyes: Vec<Vec3> = police.iter().filter(|(_, s)| matches!(s.driver, Driver::Cop(_))).map(|(t, _)| t.translation + Vec3::Y * 1.3).collect();
        eyes.extend(cops.iter().filter(|(_, c)| c.cop && c.health > 0.0).map(|(t, _)| t.translation + Vec3::Y * 0.7));
        let mut filter = QueryFilter::default().exclude_sensors().exclude_collider(pe);
        if let Some(v) = p.vehicle {
            filter = filter.exclude_rigid_body(v);
        }
        wanted.seen = eyes.iter().any(|e| {
            let d = target - *e;
            let len = d.length();
            len < 110.0 && ctx.cast_ray(*e, d / len.max(0.01), len - 2.5, true, filter).is_none_or(|(hit, _)| police.get(hit).is_ok())
        });
    }
    // the escape clock only runs once units are actually searching the area
    let searching = police.iter().any(|(t, _)| t.translation.distance(target) < 260.0) || cops.iter().any(|(t, c)| c.cop && t.translation.distance(target) < 260.0);
    if wanted.seen {
        wanted.unseen_t = 0.0;
        wanted.last_seen = target;
    } else if searching {
        wanted.unseen_t += time.delta_secs();
        if wanted.unseen_t > wanted.escape_time() {
            wanted.clear();
            hud.push("You lost the cops");
        }
    }
}

/// Cops leave their car near a player on foot, chase on foot and shoot.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn cops_on_foot(
    mut commands: Commands,
    time: Res<Time>,
    wanted: Res<Wanted>,
    mut player: Query<(Entity, &mut Player, &Transform), (Without<PoliceCar>, Without<Ped>)>,
    mut cars: Query<(&Transform, &Velocity, &mut Seat, &mut PoliceCar)>,
    mut vehicles: Query<&mut Vehicle>,
    mut cops: Query<(&mut Ped, &mut Transform), (Without<Player>, Without<PoliceCar>)>,
    hum: Option<Res<HumanoidAssets>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    rapier: ReadRapierContext,
    mut sfx: MessageWriter<crate::audio::Sfx>,
    mut rng: ResMut<CopRng>,
) {
    let dt = time.delta_secs();
    let Ok((pe, mut p, pt)) = player.single_mut() else { return };
    let Some(hum) = hum else { return };
    if wanted.stars > 0 && p.vehicle.is_none() {
        for (t, vel, mut seat, mut pc) in &mut cars {
            if pc.deployed || !matches!(seat.driver, Driver::Cop(_)) || vel.linear.length() > 2.0 || t.translation.distance(pt.translation) > 30.0 {
                continue;
            }
            pc.deployed = true;
            let Driver::Cop(seed) = seat.driver else { continue };
            seat.driver = Driver::None;
            for side in [-1.0f32, 1.0] {
                let door = t.translation + t.rotation * Vec3::new(side * 1.9, 0.6, -0.3);
                spawn_ped(&mut commands, &hum, &mut materials, door, Look::cop(), Ped { cop: true, state: PedState::Idle { t: 0.0 }, ..Ped::new(seed.wrapping_add(side as u32)) });
            }
        }
    }
    let Ok(ctx) = rapier.single() else { return };
    let target_veh = p.vehicle;
    for (mut cop, mut ct) in &mut cops {
        if !cop.cop || cop.health <= 0.0 {
            continue;
        }
        cop.shoot_cd -= dt;
        if wanted.stars == 0 {
            continue;
        }
        let to = pt.translation - ct.translation;
        let d = to.length();
        let yaw = (-to.x).atan2(-to.z);
        ct.rotation = Quat::from_rotation_y(yaw);
        // 1 star: walk up and make the arrest; 2+: keep a firing distance
        let keep = if wanted.stars <= 1 { 1.4 } else { 9.0 };
        cop.chase = if d > keep { Some(pt.translation) } else { None };
        // arrests happen up close for low wanted levels; otherwise they open fire
        if wanted.stars >= 2 && d < 45.0 && cop.shoot_cd <= 0.0 {
            cop.shoot_cd = rng.0.range(0.8, 1.6);
            let from = ct.translation + Vec3::Y * 0.6;
            let aim = pt.translation + Vec3::Y * 0.4 - from;
            let mut filter = QueryFilter::default().exclude_sensors().exclude_collider(pe);
            if let Some(v) = target_veh {
                filter = filter.exclude_rigid_body(v);
            }
            let clear = ctx.cast_ray(from, aim.normalize_or(Vec3::Z), aim.length() - 0.6, true, filter).is_none();
            let hit_chance = (0.75 - d / 70.0).clamp(0.12, 0.7);
            if clear && rng.0.chance(hit_chance) {
                if let Some(v) = target_veh {
                    if let Ok(mut veh) = vehicles.get_mut(v) {
                        veh.health -= 25.0;
                    }
                } else {
                    p.hurt(7.0);
                }
            }
            cop.fired = 0.06;
            sfx.write(crate::audio::Sfx { kind: crate::audio::SfxKind::Pistol, pos: ct.translation, volume: 0.8 });
        }
    }
}

fn flash_lightbars(time: Res<Time>, wanted: Res<Wanted>, pa: Res<PoliceAssets>, police: Query<(&PoliceCar, &Seat)>, mut mats: Query<&mut MeshMaterial3d<StandardMaterial>>) {
    let phase = (time.elapsed_secs() * 6.0) as i32 % 2 == 0;
    for (pc, seat) in &police {
        let on = wanted.stars > 0 && matches!(seat.driver, Driver::Cop(_) | Driver::None);
        let (a, b) = if !on { (&pa.off, &pa.off) } else if phase { (&pa.red, &pa.off) } else { (&pa.off, &pa.blue) };
        for (e, m) in pc.lights.iter().zip([a, b]) {
            if let Ok(mut mm) = mats.get_mut(*e) {
                if mm.0 != *m {
                    mm.0 = m.clone();
                }
            }
        }
    }
}

/// A cop standing next to a slow player on foot at 1-2 stars makes the arrest.
#[allow(clippy::type_complexity)]
fn busted_check(
    time: Res<Time>,
    mut wanted: ResMut<Wanted>,
    mut player: Query<(&mut Player, &mut Transform), Without<Ped>>,
    cops: Query<(&Ped, &Transform), Without<Player>>,
    mut hud: ResMut<crate::hud::HudMessages>,
    mut ars: ResMut<crate::weapons::Arsenal>,
) {
    let Ok((mut p, mut pt)) = player.single_mut() else { return };
    if p.busted_t > 0.0 {
        p.busted_t += time.delta_secs();
        if p.busted_t > 4.0 {
            p.busted_t = 0.0;
            pt.translation = police_point();
            p.vel = Vec3::ZERO;
            let fine = (p.money / 10).max(100).min(p.money.max(0));
            p.money -= fine;
            *ars = crate::weapons::Arsenal::default();
            hud.push(&format!("Released from Port Solace PD - fined ${fine}"));
        }
        return;
    }
    if wanted.stars == 0 || wanted.stars > 2 || p.vehicle.is_some() || p.health <= 0.0 {
        wanted.bust_t = 0.0;
        return;
    }
    let near = cops.iter().any(|(c, t)| c.cop && c.health > 0.0 && t.translation.distance(pt.translation) < 2.2);
    if near && p.vel.with_y(0.0).length() < 3.0 {
        wanted.bust_t += time.delta_secs();
        if wanted.bust_t > 1.5 {
            wanted.clear();
            p.busted_t = 0.01;
            hud.big("BUSTED", Color::srgb(0.2, 0.45, 0.95));
        }
    } else {
        wanted.bust_t = (wanted.bust_t - time.delta_secs()).max(0.0);
    }
}
