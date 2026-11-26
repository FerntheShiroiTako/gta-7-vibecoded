//! Pedestrians: ambient population on the sidewalks, wandering, panic and
//! fleeing, diving out of the way of cars, fighting back (fists or a gun),
//! cops chasing on foot, death into physics ragdolls that drop cash.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::city::CityInfo;
use crate::humanoid::{spawn_humanoid, HumanoidAssets, Look, Rig};
use crate::meshgen::Rng;
use crate::player::Player;
use crate::ragdoll::{RagdollParts, WantsRagdoll};
use crate::vehicle::{GROUP_PED, GROUP_VEHICLE, GROUP_WORLD};
use crate::wanted::Panic;

pub struct PedPlugin;

impl Plugin for PedPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PedRng>().add_systems(Update, (ambient_population, react_to_panic, ped_ai).chain());
    }
}

#[derive(Resource)]
struct PedRng(Rng);
impl Default for PedRng {
    fn default() -> Self {
        PedRng(Rng::new(4711))
    }
}

#[derive(Clone, Copy, Debug)]
pub enum PedState {
    Idle { t: f32 },
    Walk { dir: Vec3, t: f32 },
    Flee { from: Vec3, t: f32 },
    /// going after the player (punches, or shoots if armed)
    Fight { t: f32 },
    Dead { t: f32 },
}

#[derive(Component)]
pub struct Ped {
    pub state: PedState,
    pub health: f32,
    pub vel: Vec3,
    pub knock_t: f32,
    pub seed: u32,
    pub visual: Option<Entity>,
    pub cop: bool,
    pub flee_from_player: bool,
    pub chase: Option<Vec3>,
    pub shoot_cd: f32,
    pub fired: f32,
    pub target: Option<Vec3>,
    pub ambient: bool,
    /// carries a pistol (shoots when fighting)
    pub armed: bool,
    /// mission gang member: guards its spot, fights instead of fleeing
    pub gang: bool,
    pub dodge_cd: f32,
    pub attack_cd: f32,
}

impl Ped {
    pub fn new(seed: u32) -> Self {
        Ped {
            state: PedState::Idle { t: 1.0 },
            health: 100.0,
            vel: Vec3::ZERO,
            knock_t: 0.0,
            seed,
            visual: None,
            cop: false,
            flee_from_player: false,
            chase: None,
            shoot_cd: 1.0,
            fired: 0.0,
            target: None,
            ambient: false,
            armed: seed % 9 == 0,
            gang: false,
            dodge_cd: 0.0,
            attack_cd: 0.0,
        }
    }
}

pub fn character_controller() -> KinematicCharacterController {
    KinematicCharacterController {
        offset: CharacterLength::Absolute(0.03),
        up: Vec3::Y,
        slide: true,
        autostep: Some(CharacterAutostep {
            max_height: CharacterLength::Absolute(0.42),
            min_width: CharacterLength::Absolute(0.15),
            include_dynamic_bodies: false,
        }),
        snap_to_ground: Some(CharacterLength::Absolute(0.35)),
        max_slope_climb_angle: 52f32.to_radians(),
        min_slope_slide_angle: 45f32.to_radians(),
        apply_impulse_to_dynamic_bodies: true,
        custom_mass: Some(80.0),
        filter_flags: QueryFilterFlags::EXCLUDE_SENSORS,
        filter_groups: Some(CollisionGroups::new(GROUP_PED, GROUP_WORLD | GROUP_VEHICLE)),
        ..default()
    }
}

pub fn spawn_ped(commands: &mut Commands, a: &HumanoidAssets, materials: &mut Assets<StandardMaterial>, pos: Vec3, look: Look, template: Ped) -> Entity {
    let vis = spawn_humanoid(commands, a, materials, look);
    commands.entity(vis).insert(Transform::from_xyz(0.0, -0.88, 0.0));
    let e = commands
        .spawn((
            Transform::from_translation(pos),
            Visibility::default(),
            Collider::capsule_y(0.55, 0.3),
            Sensor,
            CollisionGroups::new(GROUP_PED, GROUP_WORLD),
            character_controller(),
            Ped { visual: Some(vis), ..template },
        ))
        .id();
    commands.entity(e).add_child(vis);
    e
}

const POP_TARGET: usize = 28;

#[allow(clippy::too_many_arguments)]
fn ambient_population(
    mut commands: Commands,
    time: Res<Time>,
    info: Res<CityInfo>,
    hum: Option<Res<HumanoidAssets>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    player: Query<&Transform, With<Player>>,
    peds: Query<(Entity, &Transform, &Ped)>,
    parts: Query<&RagdollParts>,
    mut rng: ResMut<PedRng>,
    mut timer: Local<f32>,
) {
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 0.5;
    let (Ok(pt), Some(hum)) = (player.single(), hum) else { return };
    let mut alive = 0;
    for (e, t, p) in &peds {
        let d = t.translation.distance(pt.translation);
        if p.ambient && d > 160.0 {
            despawn_ped(&mut commands, e, p, &parts);
        } else if p.ambient && p.health > 0.0 {
            alive += 1;
        }
    }
    // candidate sidewalk spots in a ring just beyond close view
    let ring: Vec<Vec3> = info
        .sidewalk_points
        .iter()
        .copied()
        .filter(|p| (45.0..110.0).contains(&p.distance(pt.translation)))
        .collect();
    for _ in 0..4 {
        if alive >= POP_TARGET || ring.is_empty() {
            break;
        }
        let sp = ring[rng.0.int(0, ring.len() as i32 - 1) as usize];
        let seed = rng.0.next_u32();
        let idle = rng.0.range(0.0, 2.0);
        spawn_ped(&mut commands, &hum, &mut materials, sp + Vec3::Y * 0.95, Look::random(seed), Ped { ambient: true, state: PedState::Idle { t: idle }, ..Ped::new(seed) });
        alive += 1;
    }
}

pub fn despawn_ped(commands: &mut Commands, e: Entity, p: &Ped, parts: &Query<&RagdollParts>) {
    if let Some(v) = p.visual {
        if let Ok(rp) = parts.get(v) {
            for part in &rp.0 {
                commands.entity(*part).despawn();
            }
        }
    }
    commands.entity(e).despawn();
}

fn react_to_panic(time: Res<Time>, mut panic: ResMut<Panic>, mut peds: Query<(&mut Ped, &Transform)>) {
    for (pos, radius) in panic.0.drain(..) {
        for (mut p, t) in &mut peds {
            if p.cop || p.gang || p.health <= 0.0 || matches!(p.state, PedState::Fight { .. }) || t.translation.distance(pos) > radius {
                continue;
            }
            p.state = PedState::Flee { from: pos, t: 10.0 + (p.seed % 5) as f32 };
        }
    }
    let _ = time;
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn ped_ai(
    mut commands: Commands,
    time: Res<Time>,
    info: Res<CityInfo>,
    mut player: Query<(&Transform, &mut Player), Without<Ped>>,
    mut peds: Query<(Entity, &mut Ped, &mut Transform, Option<&mut KinematicCharacterController>, Option<&KinematicCharacterControllerOutput>)>,
    cars: Query<(&Transform, &Velocity), (With<crate::seats::Seat>, Without<Ped>, Without<Player>)>,
    mut rigs: Query<&mut Rig>,
    parts: Query<&RagdollParts>,
    mut drops: ResMut<crate::activities::Drops>,
    mut sfx: MessageWriter<crate::audio::Sfx>,
    rapier: ReadRapierContext,
) {
    let dt = time.delta_secs().min(0.05);
    let (ppos, p_in_car, p_alive) = player.single().map(|(t, p)| (t.translation, p.vehicle.is_some(), p.health > 0.0)).unwrap_or((Vec3::ZERO, false, false));
    // fast-moving vehicles pedestrians should get out of the way of
    let movers: Vec<(Vec3, Vec3)> = cars.iter().filter(|(_, v)| v.linear.length() > 6.0).map(|(t, v)| (t.translation, v.linear)).collect();
    let mut player_dmg = 0.0;
    for (e, mut ped, mut t, kcc, out) in &mut peds {
        // death: hand the body over to physics, maybe drop some cash
        if ped.health <= 0.0 && !matches!(ped.state, PedState::Dead { .. }) {
            ped.state = PedState::Dead { t: 0.0 };
            if let Some(v) = ped.visual {
                commands.entity(v).insert(WantsRagdoll { vel: ped.vel });
            }
            commands.entity(e).remove::<KinematicCharacterController>();
            use crate::activities::PickupKind as K;
            let here = t.translation.with_y(t.translation.y - 0.9);
            if ped.cop || ped.armed || ped.gang {
                drops.0.push((here, K::Weapon(if ped.gang && ped.seed % 2 == 0 { 2 } else { 1 })));
            }
            if ped.gang || ped.seed % 3 != 0 {
                let cash = if ped.gang { 150 + (ped.seed % 200) as i64 } else { 10 + (ped.seed % 90) as i64 };
                drops.0.push((here + Vec3::new(0.5, 0.0, 0.3), K::Cash(cash)));
            }
            continue;
        }
        if let PedState::Dead { t: tt } = &mut ped.state {
            *tt += dt;
            if *tt > 45.0 {
                let p = &*ped;
                despawn_ped(&mut commands, e, p, &parts);
            }
            continue;
        }
        let Some(mut kcc) = kcc else { continue };
        let grounded = out.map(|o| o.grounded).unwrap_or(false);
        let mut wish = Vec3::ZERO;
        let mut speed = 0.0;
        let seed = ped.seed;
        let flee_player = ped.flee_from_player;
        ped.dodge_cd = (ped.dodge_cd - dt).max(0.0);
        ped.attack_cd = (ped.attack_cd - dt).max(0.0);
        // gang members guard their spot until the player shows up
        if ped.gang && !matches!(ped.state, PedState::Fight { .. }) && p_alive && t.translation.distance(ppos) < 38.0 {
            ped.state = PedState::Fight { t: 60.0 };
        }
        // dive out of the way of a car about to run us over
        let mut dodge = None;
        if ped.dodge_cd <= 0.0 && grounded && ped.knock_t <= 0.0 {
            for (cp, cv) in &movers {
                let rel = t.translation - *cp;
                if rel.length() > 22.0 {
                    continue;
                }
                let speed = cv.length();
                let dir = *cv / speed;
                let ahead = rel.dot(dir);
                let tti = ahead / speed;
                let side = (rel - dir * ahead).with_y(0.0);
                if ahead > 0.0 && tti < 1.2 && side.length() < 2.6 {
                    let away = if side.length() > 0.05 { side.normalize() } else { dir.cross(Vec3::Y).normalize() };
                    dodge = Some(away);
                    break;
                }
            }
        }
        if let Some(away) = dodge {
            // reflexes vary: most people make it
            if ped.seed % 5 != 0 {
                ped.vel = away * 7.5 + Vec3::Y * 3.2;
                ped.dodge_cd = 1.5;
                if !ped.cop && !ped.gang && !matches!(ped.state, PedState::Fight { .. }) {
                    ped.state = PedState::Flee { from: t.translation - away, t: 4.0 };
                }
            } else {
                ped.dodge_cd = 1.0;
            }
        }
        if let Some(c) = ped.chase {
            wish = (c - t.translation).with_y(0.0).normalize_or_zero();
            speed = 6.0;
        } else {
            match &mut ped.state {
                PedState::Idle { t: tt } => {
                    *tt -= dt;
                    if *tt <= 0.0 && !ped.cop {
                        ped.state = PedState::Walk { dir: Vec3::ZERO, t: 30.0 };
                        ped.target = None;
                    }
                }
                PedState::Walk { t: tt, .. } => {
                    *tt -= dt;
                    // walk between neighbouring sidewalk points (they lie along the curbs)
                    let need = ped.target.is_none_or(|g| g.with_y(0.0).distance(t.translation.with_y(0.0)) < 1.0);
                    if need {
                        let here = t.translation;
                        let mut r = Rng::new(seed as u64 ^ (time.elapsed_secs() * 7.0) as u64);
                        let near: Vec<Vec3> = info.sidewalk_points.iter().copied().filter(|p| {
                            let d = p.with_y(0.0).distance(here.with_y(0.0));
                            d > 4.0 && d < 13.0
                        }).collect();
                        ped.target = if near.is_empty() { None } else { Some(*r.pick(&near)) };
                    }
                    if let Some(g) = ped.target {
                        wish = (g - t.translation).with_y(0.0).normalize_or_zero();
                        speed = 1.3 + (seed % 4) as f32 * 0.12;
                    }
                }
                PedState::Flee { from, t: tt } => {
                    *tt -= dt;
                    let src = if flee_player { ppos } else { *from };
                    wish = (t.translation - src).with_y(0.0).normalize_or(Vec3::X);
                    speed = 6.0;
                    if *tt <= 0.0 {
                        ped.state = PedState::Walk { dir: Vec3::ZERO, t: 20.0 };
                        ped.flee_from_player = false;
                        ped.target = None;
                    }
                }
                PedState::Fight { t: tt } => {
                    *tt -= dt;
                    let to = (ppos - t.translation).with_y(0.0);
                    let d = to.length();
                    // give up when the player is dead, far away or drove off
                    if *tt <= 0.0 || !p_alive || d > 90.0 || (p_in_car && !ped.armed) {
                        ped.state = PedState::Walk { dir: Vec3::ZERO, t: 20.0 };
                        ped.target = None;
                    } else {
                        let face = to.normalize_or(Vec3::Z);
                        let yaw = (-face.x).atan2(-face.z);
                        if ped.armed {
                            // keep shooting distance
                            if d > 13.0 {
                                wish = face;
                                speed = 4.5;
                            } else if d < 5.0 {
                                wish = -face;
                                speed = 2.0;
                            }
                            if d < 40.0 && ped.attack_cd <= 0.0 {
                                ped.attack_cd = 0.9 + (seed % 7) as f32 * 0.12;
                                ped.fired = 0.06;
                                let from = t.translation + Vec3::Y * 0.6;
                                let aim = ppos + Vec3::Y * 0.4 - from;
                                let clear = rapier
                                    .single()
                                    .map(|ctx| ctx.cast_ray(from, aim.normalize_or(Vec3::Z), aim.length() - 0.8, true, QueryFilter::default().exclude_sensors().exclude_collider(e)).is_none())
                                    .unwrap_or(false);
                                let chance = (0.6 - d / 80.0).clamp(0.12, 0.5);
                                if clear && (seed as f32 * 0.37 + time.elapsed_secs() * 3.1).sin().abs() < chance {
                                    player_dmg += if p_in_car { 0.0 } else { 6.0 };
                                }
                                sfx.write(crate::audio::Sfx { kind: crate::audio::SfxKind::Pistol, pos: t.translation, volume: 0.8 });
                            }
                        } else if d > 1.3 {
                            wish = face;
                            speed = if d > 5.0 { 5.2 } else { 2.5 };
                        } else if ped.attack_cd <= 0.0 && !p_in_car {
                            ped.attack_cd = 0.8;
                            player_dmg += 6.0;
                            if let Some(v) = ped.visual {
                                if let Ok(mut rig) = rigs.get_mut(v) {
                                    rig.punch = 1.0;
                                }
                            }
                            sfx.write(crate::audio::Sfx { kind: crate::audio::SfxKind::Punch, pos: t.translation, volume: 0.6 });
                        }
                        t.rotation = t.rotation.slerp(Quat::from_rotation_y(yaw), (dt * 12.0).min(1.0));
                    }
                }
                PedState::Dead { .. } => {}
            }
        }
        let knocked = ped.knock_t > 0.0;
        ped.knock_t = (ped.knock_t - dt).max(0.0);
        ped.fired = (ped.fired - dt).max(0.0);
        let target = wish * speed;
        // a dive keeps its momentum for a moment instead of being steered
        let diving = ped.dodge_cd > 1.1;
        if knocked || diving {
            if grounded && knocked {
                let f = (1.0 - dt * 4.0).max(0.0);
                ped.vel.x *= f;
                ped.vel.z *= f;
            }
        } else {
            let k = (dt * 10.0).min(1.0);
            ped.vel.x += (target.x - ped.vel.x) * k;
            ped.vel.z += (target.z - ped.vel.z) * k;
        }
        if grounded && ped.vel.y < 0.0 {
            ped.vel.y = -1.0;
        }
        ped.vel.y = (ped.vel.y - 18.0 * dt).max(-40.0);
        kcc.translation = Some(ped.vel * dt);
        let flat = ped.vel.with_y(0.0);
        let fighting = matches!(ped.state, PedState::Fight { .. });
        if flat.length() > 0.4 && !knocked && !ped.cop && !fighting {
            let yaw = (-flat.x).atan2(-flat.z);
            t.rotation = t.rotation.slerp(Quat::from_rotation_y(yaw), (dt * 10.0).min(1.0));
        }
        if let Some(v) = ped.visual {
            if let Ok(mut rig) = rigs.get_mut(v) {
                rig.speed = flat.length();
                rig.airborne = !grounded && !knocked;
                rig.ragdoll = knocked;
                rig.aim = (ped.cop && ped.chase.is_none()) || (fighting && ped.armed);
            }
        }
    }
    if player_dmg > 0.0 {
        if let Ok((_, mut p)) = player.single_mut() {
            p.hurt(player_dmg);
        }
    }
}
