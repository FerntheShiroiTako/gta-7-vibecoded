//! Jobs: walk or drive into a glowing marker to start one, or press J in a cab.
//! - Taxi: pick up fares and drop them off against the clock; streak bonuses.
//! - Courier run: four drop-offs across town, each on a timer.
//! - Contract: take out the Rustline Crew at their corner - they shoot back.
//! - Export: steal a specific car and deliver it to the docks before the timer
//!   runs out; the payout depends on its condition.
//! The active objective gets a yellow corona + beam in the world, a blip on the
//! minimap, an objective line with the distance, and a countdown.

use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::audio::{Sfx, SfxKind};
use crate::city::CityInfo;
use crate::hud::{font, shadow, HudFonts, HudMessages};
use crate::humanoid::{HumanoidAssets, Look};
use crate::input::GameInput;
use crate::lighting::NoRaytrace;
use crate::map::MapBlip;
use crate::meshgen::{MeshBuilder, Rng, WHITE};
use crate::peds::{spawn_ped, Ped, PedState};
use crate::player::Player;
use crate::seats::Seat;
use crate::vehicle::{spawn_car, spec, CarKind, Driver, Vehicle, VehicleAssets};
use crate::world::*;

pub struct MissionPlugin;

impl Plugin for MissionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Jobs>()
            .add_systems(Startup, setup_missions.after(crate::hud::HudSetup).after(crate::city::setup_city))
            .add_systems(Update, (job_givers, run_job, job_visuals).chain());
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Giver {
    Courier,
    Contract,
    Export,
}

impl Giver {
    fn color(self) -> Color {
        match self {
            Giver::Courier => Color::srgb(1.0, 0.85, 0.2),
            Giver::Contract => Color::srgb(1.0, 0.25, 0.2),
            Giver::Export => Color::srgb(0.3, 1.0, 0.45),
        }
    }
    fn glow(self) -> LinearRgba {
        match self {
            Giver::Courier => LinearRgba::rgb(3.0, 2.2, 0.3),
            Giver::Contract => LinearRgba::rgb(3.0, 0.4, 0.3),
            Giver::Export => LinearRgba::rgb(0.5, 3.0, 0.8),
        }
    }
    fn title(self) -> &'static str {
        match self {
            Giver::Courier => "Courier job",
            Giver::Contract => "Contract",
            Giver::Export => "Export job",
        }
    }
}

/// On sidewalks, clear of the street furniture (lamps at 12/40/68 m along a
/// block, trees at 26/54 m).
fn giver_spots() -> [(Giver, Vec3); 3] {
    [
        // right by where you start
        (Giver::Courier, Vec3::new(block_min(4) + 33.0, CURB, block_min(5) + BLOCK - 2.0)),
        // industrial yards by the docks
        (Giver::Contract, Vec3::new(block_min(9) + 31.5, CURB, block_min(6) + 2.0)),
        // midtown, south side
        (Giver::Export, Vec3::new(block_min(6) + 47.0, CURB, block_min(8) + BLOCK - 2.0)),
    ]
}

/// Docks export yard (quay).
const EXPORT_YARD: Vec3 = Vec3::new(505.0, 0.0, -120.0);

#[derive(Component)]
struct GiverMarker(Giver, Vec3);

/// The single objective corona, moved to wherever the job wants you.
#[derive(Component)]
struct Objective;

#[derive(Component)]
struct ObjectiveText;

#[derive(Component)]
struct TimerText;

enum Job {
    Taxi { cab: Entity, fare: Option<Entity>, pickup: Vec3, dest: Option<Vec3>, trip: f32, timer: f32, fares: u32 },
    Courier { drops: Vec<Vec3>, idx: usize, timer: f32 },
    Contract { crew: Vec<Entity>, place: Vec3 },
    Export { car: Entity, stolen: bool, timer: f32 },
}

#[derive(Resource)]
pub struct Jobs {
    job: Option<Job>,
    cooldown: f32,
    rng: Rng,
    /// can't re-trigger a marker until you've stepped out of it
    in_marker: bool,
    pub completed: u32,
}

impl Default for Jobs {
    fn default() -> Self {
        Jobs { job: None, cooldown: 0.0, rng: Rng::new(8086), in_marker: false, completed: 0 }
    }
}

impl Jobs {
    pub fn active(&self) -> bool {
        self.job.is_some()
    }
    /// Where the current job wants you next (None while hunting a moving target).
    pub fn target(&self) -> Option<Vec3> {
        match self.job.as_ref()? {
            Job::Taxi { pickup, dest, .. } => Some(dest.unwrap_or(*pickup)),
            Job::Courier { drops, idx, .. } => drops.get(*idx).copied(),
            Job::Contract { place, .. } => Some(*place),
            Job::Export { stolen, .. } => stolen.then_some(EXPORT_YARD),
        }
    }
    /// The car an export job wants stolen.
    pub fn export_car(&self) -> Option<Entity> {
        match self.job.as_ref()? {
            Job::Export { car, .. } => Some(*car),
            _ => None,
        }
    }
    pub fn name(&self) -> &'static str {
        match self.job.as_ref() {
            None => "none",
            Some(Job::Taxi { .. }) => "taxi",
            Some(Job::Courier { .. }) => "courier",
            Some(Job::Contract { .. }) => "contract",
            Some(Job::Export { .. }) => "export",
        }
    }
}

/// Marker positions (for tests and the map).
pub fn giver_positions() -> Vec<(&'static str, Vec3)> {
    giver_spots().iter().map(|(g, p)| (g.title(), *p)).collect()
}

fn setup_missions(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    fonts: Res<HudFonts>,
    mut glow: ResMut<crate::activities::GlowMats>,
) {
    let mut glow_mat = |materials: &mut Assets<StandardMaterial>, c: LinearRgba| crate::activities::glow_material(materials, &mut glow, c);
    let mut ring = MeshBuilder::new();
    ring.cylinder(Vec3::ZERO, 2.4, 1.4, 32, WHITE, false);
    let ring = meshes.add(ring.build());
    let mut beam = MeshBuilder::new();
    beam.cylinder(Vec3::ZERO, 0.45, 70.0, 12, WHITE, false);
    let beam = meshes.add(beam.build());
    let mut icon = MeshBuilder::new();
    icon.cylinder_at(Affine3A::from_rotation_translation(Quat::from_rotation_x(std::f32::consts::PI), Vec3::new(0.0, 0.5, 0.0)), 0.45, 0.0, 0.7, 4, WHITE, true);
    icon.cylinder_at(Affine3A::from_translation(Vec3::new(0.0, 0.5, 0.0)), 0.45, 0.0, 0.3, 4, WHITE, false);
    let icon = meshes.add(icon.build());

    for (g, p) in giver_spots() {
        let m = glow_mat(&mut materials, g.glow());
        let faint = glow_mat(&mut materials, g.glow() * 0.08);
        let solid = materials.add(StandardMaterial { base_color: g.color(), emissive: g.glow() * 0.6, ..default() });
        let root = commands.spawn((Transform::from_translation(p), Visibility::default(), GiverMarker(g, p), MapBlip { color: g.color(), range: 900.0 })).id();
        let parts = [
            commands.spawn((Mesh3d(ring.clone()), MeshMaterial3d(m.clone()), Transform::default(), NoRaytrace)).id(),
            commands.spawn((Mesh3d(beam.clone()), MeshMaterial3d(faint), Transform::default(), NoRaytrace)).id(),
            commands.spawn((Mesh3d(icon.clone()), MeshMaterial3d(solid), Transform::from_xyz(0.0, 2.2, 0.0), Spinner)).id(),
        ];
        commands.entity(root).add_children(&parts);
    }
    // the objective corona
    let m = glow_mat(&mut materials, LinearRgba::rgb(3.0, 2.4, 0.4));
    let faint = glow_mat(&mut materials, LinearRgba::rgb(0.4, 0.3, 0.05));
    let root = commands.spawn((Transform::default(), Visibility::Hidden, Objective, MapBlip { color: Color::srgb(1.0, 0.9, 0.2), range: f32::INFINITY })).id();
    let parts = [
        commands.spawn((Mesh3d(ring.clone()), MeshMaterial3d(m), Transform::from_scale(Vec3::new(1.3, 1.0, 1.3)), NoRaytrace)).id(),
        commands.spawn((Mesh3d(beam.clone()), MeshMaterial3d(faint), Transform::default(), NoRaytrace)).id(),
    ];
    commands.entity(root).add_children(&parts);

    commands.spawn((
        Text::new(""),
        font(&fonts, 26.0, 700),
        TextColor(Color::srgb(1.0, 0.95, 0.85)),
        shadow(),
        TextLayout::justify(Justify::Center),
        Node { position_type: PositionType::Absolute, width: percent(100), bottom: px(62), ..default() },
        ObjectiveText,
    ));
    commands.spawn((
        Text::new(""),
        font(&fonts, 40.0, 850),
        TextColor(Color::WHITE),
        shadow(),
        TextLayout::justify(Justify::Center),
        Node { position_type: PositionType::Absolute, width: percent(100), top: px(24), ..default() },
        TimerText,
    ));
}

#[derive(Component)]
struct Spinner;

fn random_point(rng: &mut Rng, info: &CityInfo, from: Vec3, min: f32, max: f32) -> Vec3 {
    for _ in 0..400 {
        let p = *rng.pick(&info.sidewalk_points);
        let d = p.with_y(0.0).distance(from.with_y(0.0));
        if d >= min && d <= max {
            return p;
        }
    }
    *rng.pick(&info.sidewalk_points)
}

/// What the player is in / where they are this frame.
struct Me {
    pos: Vec3,
    speed: f32,
    vehicle: Option<Entity>,
    alive: bool,
}

#[derive(bevy::ecs::system::SystemParam)]
struct Feedback<'w> {
    hud: ResMut<'w, HudMessages>,
    sfx: MessageWriter<'w, Sfx>,
}

impl Feedback<'_> {
    fn pass(&mut self, p: &mut Player, title: &str, pay: i64, at: Vec3) {
        p.money += pay;
        self.hud.big("JOB COMPLETE", Color::srgb(1.0, 0.8, 0.15));
        self.hud.push(&format!("{title}: +${pay}"));
        self.sfx.write(Sfx { kind: SfxKind::Reward, pos: at, volume: 1.0 });
    }
    fn fail(&mut self, why: &str) {
        self.hud.big("JOB FAILED", Color::srgb(0.9, 0.15, 0.1));
        self.hud.push(why);
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn job_givers(
    mut commands: Commands,
    mut jobs: ResMut<Jobs>,
    time: Res<Time>,
    mut input: ResMut<GameInput>,
    player: Query<(&Player, &Transform)>,
    cars: Query<(&Transform, &Vehicle), Without<Player>>,
    info: Res<CityInfo>,
    hum: Option<Res<HumanoidAssets>>,
    va: Option<Res<VehicleAssets>>,
    seats: Query<&Transform, (With<Seat>, Without<Player>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut fb: Feedback,
) {
    jobs.cooldown = (jobs.cooldown - time.delta_secs()).max(0.0);
    let Ok((p, pt)) = player.single() else { return };
    // while a job runs, J belongs to it (quit the taxi job)
    if jobs.job.is_some() || p.health <= 0.0 {
        return;
    }
    let job_key = std::mem::take(&mut input.job);
    let car = p.vehicle.and_then(|v| cars.get(v).ok().map(|c| (v, c)));
    let pos = car.map(|(_, (t, _))| t.translation).unwrap_or(pt.translation);
    // taxi: press J in a cab
    if let Some((cab, (_, v))) = car {
        if v.spec.kind == CarKind::Taxi {
            if job_key {
                let pickup = random_point(&mut jobs.rng, &info, pos, 80.0, 260.0);
                let fare = hum.as_ref().map(|h| spawn_fare(&mut commands, h, &mut materials, pickup, jobs.rng.next_u32()));
                jobs.job = Some(Job::Taxi { cab, fare, pickup, dest: None, trip: 0.0, timer: 0.0, fares: 0 });
                fb.hud.push("Taxi job: pick up the fare (blue blip). Press J to quit.");
                return;
            }
            fb.hud.extra_hint = "Press J to start a taxi job".into();
        }
    }
    // markers
    let mut inside = None;
    for (g, mp) in giver_spots() {
        if (mp - pos).with_y(0.0).length() < 3.2 && (mp.y - pos.y).abs() < 3.0 {
            inside = Some(g);
        }
    }
    let Some(g) = inside else {
        jobs.in_marker = false;
        return;
    };
    if jobs.in_marker || jobs.cooldown > 0.0 {
        return;
    }
    jobs.in_marker = true;
    match g {
        Giver::Courier => {
            let mut drops = Vec::new();
            let mut from = pos;
            for k in 0..4 {
                let d = random_point(&mut jobs.rng, &info, from, if k == 0 { 200.0 } else { 250.0 }, 650.0);
                drops.push(d);
                from = d;
            }
            let timer = leg_time(pos, drops[0]);
            jobs.job = Some(Job::Courier { drops, idx: 0, timer });
            fb.hud.big("COURIER RUN", Color::srgb(1.0, 0.85, 0.2));
            fb.hud.push("Deliver 4 packages - each drop is on the clock");
        }
        Giver::Contract => {
            let Some(h) = hum.as_ref() else { return };
            let place = random_point(&mut jobs.rng, &info, pos, 300.0, 650.0);
            let mut crew = Vec::new();
            for k in 0..4 {
                let seed = jobs.rng.next_u32();
                let a = k as f32 * 1.7;
                let at = place + Vec3::new(a.cos() * 3.5, 0.95, a.sin() * 3.5);
                let base = Look::random(seed);
                let look = Look { top: Color::srgb(0.42, 0.08, 0.5), legs: Color::srgb(0.05, 0.05, 0.06), shoes: Color::srgb(0.9, 0.9, 0.9), ..base };
                let e = spawn_ped(
                    &mut commands,
                    h,
                    &mut materials,
                    at,
                    look,
                    Ped { gang: true, armed: k != 3, health: if k == 0 { 180.0 } else { 120.0 }, state: PedState::Idle { t: 1.0e9 }, ..Ped::new(seed) },
                );
                commands.entity(e).insert(MapBlip { color: Color::srgb(1.0, 0.15, 0.1), range: f32::INFINITY });
                crew.push(e);
            }
            jobs.job = Some(Job::Contract { crew, place });
            fb.hud.big("CONTRACT", Color::srgb(1.0, 0.25, 0.2));
            fb.hud.push("Take out the Rustline Crew on their corner. They're armed.");
        }
        Giver::Export => {
            let Some(va) = va.as_ref() else { return };
            // a free curbside spot across town
            let mut spot = None;
            for _ in 0..200 {
                let (sp, yaw) = *jobs.rng.pick(&info.parking);
                let d = sp.with_y(0.0).distance(pos.with_y(0.0));
                if (300.0..800.0).contains(&d) && seats.iter().all(|t| t.translation.distance(sp) > 7.0) {
                    spot = Some((sp, yaw));
                    break;
                }
            }
            let Some((sp, yaw)) = spot else { return };
            let s = spec(CarKind::Sports);
            let car = spawn_car(&mut commands, va, &mut materials, CarKind::Sports, Vec3::new(sp.x, sp.y + s.wheel_r + 0.12, sp.z), yaw, Color::srgb(0.85, 0.65, 0.1), Driver::None);
            commands.entity(car).insert(MapBlip { color: Color::srgb(0.3, 1.0, 0.45), range: f32::INFINITY });
            jobs.job = Some(Job::Export { car, stolen: false, timer: 0.0 });
            fb.hud.big("EXPORT", Color::srgb(0.3, 1.0, 0.45));
            fb.hud.push(&format!("A buyer wants a gold {}. Steal it (green blip) and bring it to the docks.", s.name));
        }
    }
}

fn spawn_fare(commands: &mut Commands, h: &HumanoidAssets, materials: &mut Assets<StandardMaterial>, at: Vec3, seed: u32) -> Entity {
    let e = spawn_ped(commands, h, materials, at + Vec3::Y * 0.95, Look::random(seed), Ped { state: PedState::Idle { t: 1.0e9 }, ..Ped::new(seed) });
    commands.entity(e).insert(MapBlip { color: Color::srgb(0.3, 0.7, 1.0), range: f32::INFINITY });
    e
}

/// Generous time for a leg driven across the grid.
fn leg_time(a: Vec3, b: Vec3) -> f32 {
    let d = (a - b).with_y(0.0).length();
    // city blocks: Manhattan distance is closer to what you actually drive
    let m = (a.x - b.x).abs() + (a.z - b.z).abs();
    20.0 + d.max(m * 0.85) / 12.0
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn run_job(
    mut commands: Commands,
    time: Res<Time>,
    mut jobs: ResMut<Jobs>,
    mut input: ResMut<GameInput>,
    mut player: Query<(&mut Player, &Transform)>,
    cars: Query<(&Transform, &Velocity, Option<&Vehicle>, &Seat), Without<Player>>,
    peds: Query<&Ped>,
    info: Res<CityInfo>,
    hum: Option<Res<HumanoidAssets>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut wanted: ResMut<crate::wanted::Wanted>,
    mut fb: Feedback,
) {
    let dt = time.delta_secs();
    let Ok((mut p, pt)) = player.single_mut() else { return };
    let me = match p.vehicle.and_then(|v| cars.get(v).ok().map(|c| (v, c))) {
        Some((v, (t, vel, _, _))) => Me { pos: t.translation, speed: vel.linear.length(), vehicle: Some(v), alive: p.health > 0.0 },
        None => Me { pos: pt.translation, speed: p.vel.length(), vehicle: None, alive: p.health > 0.0 },
    };
    let Jobs { job, rng, cooldown, completed, .. } = &mut *jobs;
    let Some(j) = job.as_mut() else { return };
    let busted = p.busted_t > 0.0;
    // outcome: None = keep going, Some(true) = finished (already rewarded), Some(false) = failed
    let mut end: Option<bool> = None;
    let mut cleanup: Vec<Entity> = Vec::new();
    if !me.alive || busted {
        fb.fail(if busted { "Busted - the job's off" } else { "Wasted - the job's off" });
        end = Some(false);
    } else {
        match j {
            Job::Taxi { cab, fare, pickup, dest, trip, timer, fares } => {
                let quit = std::mem::take(&mut input.job);
                if me.vehicle != Some(*cab) || quit {
                    fb.hud.push(&format!("Taxi job over - {fares} fares"));
                    end = Some(true);
                    if let Some(f) = fare.take() {
                        cleanup.push(f);
                    }
                } else if dest.is_none() {
                    // pick-up stage
                    let fare_ok = fare.is_none_or(|f| peds.get(f).is_ok_and(|pd| pd.health > 0.0));
                    if !fare_ok {
                        fb.fail("Your fare got hurt");
                        end = Some(false);
                    } else if (me.pos - *pickup).with_y(0.0).length() < 8.0 && me.speed < 2.0 {
                        if let Some(f) = fare.take() {
                            cleanup.push(f);
                        }
                        let d = random_point(rng, &info, *pickup, 250.0, 850.0);
                        *trip = (d - *pickup).with_y(0.0).length();
                        *timer = leg_time(*pickup, d);
                        *dest = Some(d);
                        fb.hud.push("Fare on board - get them there before the meter runs out");
                        fb.sfx.write(Sfx { kind: SfxKind::Pickup, pos: me.pos, volume: 0.6 });
                    }
                } else if let Some(d) = *dest {
                    *timer -= dt;
                    if *timer <= 0.0 {
                        fb.fail("Too slow - your passenger jumped out");
                        end = Some(false);
                    } else if (me.pos - d).with_y(0.0).length() < 8.0 && me.speed < 2.5 {
                        *fares += 1;
                        let pay = (15.0 + *trip * 0.12 + *timer * 1.5) as i64;
                        p.money += pay;
                        fb.hud.push(&format!("Fare paid: +${pay}  ({fares} in a row)"));
                        if *fares % 5 == 0 {
                            let bonus = 100 * *fares as i64;
                            p.money += bonus;
                            fb.hud.big("TAXI STREAK", Color::srgb(1.0, 0.85, 0.15));
                            fb.hud.push(&format!("{fares} fares in a row: +${bonus} bonus"));
                        }
                        fb.sfx.write(Sfx { kind: SfxKind::Reward, pos: me.pos, volume: 0.8 });
                        // next fare
                        *pickup = random_point(rng, &info, me.pos, 80.0, 260.0);
                        *dest = None;
                        *fare = hum.as_ref().map(|h| spawn_fare(&mut commands, h, &mut materials, *pickup, rng.next_u32()));
                    }
                }
            }
            Job::Courier { drops, idx, timer } => {
                *timer -= dt;
                let target = drops[*idx];
                if *timer <= 0.0 {
                    fb.fail("Out of time - the package is late");
                    end = Some(false);
                } else if (me.pos - target).with_y(0.0).length() < 6.0 && me.speed < 8.0 && (me.pos.y - target.y).abs() < 4.0 {
                    p.money += 150;
                    *idx += 1;
                    fb.sfx.write(Sfx { kind: SfxKind::Pickup, pos: me.pos, volume: 0.8 });
                    if *idx >= drops.len() {
                        fb.pass(&mut p, "Courier run", 600, me.pos);
                        end = Some(true);
                    } else {
                        *timer = leg_time(target, drops[*idx]);
                        fb.hud.push(&format!("Delivered +$150 - {} to go", drops.len() - *idx));
                    }
                }
            }
            Job::Contract { crew, place } => {
                let alive = crew.iter().filter(|e| peds.get(**e).is_ok_and(|pd| pd.health > 0.0)).count();
                if alive == 0 {
                    fb.pass(&mut p, "Contract on the Rustline Crew", 2500, *place);
                    end = Some(true);
                }
            }
            Job::Export { car, stolen, timer } => match cars.get(*car) {
                Err(_) => {
                    fb.fail("The car is gone");
                    end = Some(false);
                }
                Ok((ct, vel, v, _)) => {
                    let wrecked = v.is_some_and(|v| v.wrecked || v.burn_t.is_some());
                    if wrecked {
                        fb.fail("You wrecked the merchandise");
                        end = Some(false);
                    } else if !*stolen {
                        if me.vehicle == Some(*car) {
                            *stolen = true;
                            *timer = leg_time(ct.translation, EXPORT_YARD) + 30.0;
                            wanted.add(crate::wanted::Crime::Carjack);
                            fb.hud.push("The alarm went off! Get it to the docks (yellow blip)");
                        }
                    } else {
                        *timer -= dt;
                        if *timer <= 0.0 {
                            fb.fail("The buyer left - too late");
                            end = Some(false);
                        } else if (ct.translation - EXPORT_YARD).with_y(0.0).length() < 8.0 && vel.linear.length() < 3.0 {
                            let health = v.map(|v| v.health).unwrap_or(1000.0);
                            let pay = (3000.0 * (health / 1000.0).clamp(0.25, 1.0)) as i64;
                            fb.pass(&mut p, &format!("Export ({:.0}% condition)", (health / 10.0).clamp(0.0, 100.0)), pay, ct.translation);
                            commands.entity(*car).remove::<MapBlip>();
                            end = Some(true);
                        }
                    }
                }
            },
        }
    }
    if let Some(ok) = end {
        // failed/aborted jobs clean up anyone still waiting around
        if let Some(j) = job.take() {
            match j {
                Job::Taxi { fare: Some(f), .. } => cleanup.push(f),
                Job::Contract { crew, .. } if !ok => {
                    for e in crew {
                        if peds.get(e).is_ok_and(|pd| pd.health > 0.0) {
                            cleanup.push(e);
                        }
                    }
                }
                Job::Export { car, .. } if !ok => {
                    if let Ok(mut ec) = commands.get_entity(car) {
                        ec.remove::<MapBlip>();
                    }
                }
                _ => {}
            }
        }
        if ok {
            *completed += 1;
        }
        *cooldown = 4.0;
    }
    for e in cleanup {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.despawn();
        }
    }
}

#[allow(clippy::type_complexity)]
fn job_visuals(
    time: Res<Time>,
    jobs: Res<Jobs>,
    player: Query<(&Player, &Transform)>,
    cars: Query<&Transform, (With<Seat>, Without<Player>, Without<Objective>, Without<GiverMarker>)>,
    mut obj: Query<(&mut Transform, &mut Visibility), (With<Objective>, Without<Player>, Without<GiverMarker>, Without<Spinner>)>,
    mut givers: Query<&mut Visibility, (With<GiverMarker>, Without<Objective>)>,
    mut spinners: Query<&mut Transform, (With<Spinner>, Without<Objective>, Without<Player>, Without<Seat>)>,
    mut texts: ParamSet<(Query<&mut Text, With<ObjectiveText>>, Query<(&mut Text, &mut TextColor), With<TimerText>>)>,
) {
    let t = time.elapsed_secs();
    for mut s in &mut spinners {
        s.rotation = Quat::from_rotation_y(t * 2.0);
        s.translation.y = 2.2 + (t * 2.5).sin() * 0.15;
    }
    let Ok((p, pt)) = player.single() else { return };
    let me = p.vehicle.and_then(|v| cars.get(v).ok()).map(|t| t.translation).unwrap_or(pt.translation);
    let (target, line, timer): (Option<Vec3>, String, Option<f32>) = match &jobs.job {
        None => (None, String::new(), None),
        Some(Job::Taxi { pickup, dest, timer, fares, .. }) => match dest {
            None => (Some(*pickup), format!("Pick up the fare  ({fares} done)"), None),
            Some(d) => (Some(*d), "Drop the fare off".into(), Some(*timer)),
        },
        Some(Job::Courier { drops, idx, timer }) => (drops.get(*idx).copied(), format!("Deliver package {} of {}", idx + 1, drops.len()), Some(*timer)),
        Some(Job::Contract { place, .. }) => {
            let near = (me - *place).with_y(0.0).length() < 45.0;
            (if near { None } else { Some(*place) }, "Take out the Rustline Crew (red blips)".into(), None)
        }
        Some(Job::Export { stolen, timer, .. }) => {
            if *stolen {
                (Some(EXPORT_YARD), "Deliver the car to the docks export yard".into(), Some(*timer))
            } else {
                (None, "Steal the gold sports car (green blip)".into(), None)
            }
        }
    };
    if let Ok((mut tr, mut vis)) = obj.single_mut() {
        match target {
            Some(p) => {
                tr.translation = Vec3::new(p.x, p.y.max(ground_height(p.x, p.z)), p.z);
                if *vis != Visibility::Inherited {
                    *vis = Visibility::Inherited;
                }
            }
            None => {
                if *vis != Visibility::Hidden {
                    *vis = Visibility::Hidden;
                }
            }
        }
    }
    let show_givers = jobs.job.is_none();
    for mut v in &mut givers {
        let want = if show_givers { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    if let Ok(mut tx) = texts.p0().single_mut() {
        tx.0 = match target {
            Some(p) if !line.is_empty() => format!("{line}  -  {:.0} m", (p - me).with_y(0.0).length()),
            _ => line,
        };
    }
    if let Ok((mut tx, mut col)) = texts.p1().single_mut() {
        match timer {
            Some(s) => {
                let s = s.max(0.0);
                tx.0 = format!("{}:{:02}", (s / 60.0) as u32, (s % 60.0) as u32);
                col.0 = if s < 10.0 && (t * 4.0) as i32 % 2 == 0 { Color::srgb(1.0, 0.25, 0.2) } else { Color::WHITE };
            }
            None => tx.0.clear(),
        }
    }
}
