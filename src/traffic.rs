//! Ambient traffic: AI cars follow right-hand lanes along the road grid, pick
//! turns at junctions, stop for red lights and for anything in front of them.

use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::lighting::NoRaytrace;
use crate::meshgen::{rgb, MeshBuilder, Rng, WHITE};
use crate::peds::Ped;
use crate::player::Player;
use crate::seats::Seat;
use crate::vehicle::{spawn_car, CarKind, Driver, VehicleAssets, VehicleControl, PAINTS};
use crate::world::*;

pub struct TrafficPlugin;

impl Plugin for TrafficPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TrafficRng>()
            .add_systems(Startup, setup_signals.after(crate::city::setup_city))
            .add_systems(PreUpdate, traffic_drive.after(crate::vehicle::VehicleControlSet).after(crate::input::GameInputSet))
            .add_systems(Update, (traffic_population, signal_lights));
    }
}

const TRAFFIC_TARGET: usize = 22;
const CYCLE: f32 = 22.0; // full light cycle: EW green, all red, NS green, all red

#[derive(Resource)]
struct TrafficRng(Rng);
impl Default for TrafficRng {
    fn default() -> Self {
        TrafficRng(Rng::new(31337))
    }
}

#[derive(Component)]
pub struct TrafficAi {
    /// grid direction of travel (one of ±X, ±Z)
    pub dir: IVec2,
    /// the intersection we are heading for
    pub node: IVec2,
    pub next_dir: IVec2,
    pub cruise: f32,
    pub blocked_t: f32,
}

/// Which axis has green right now: Some(true) = east-west, Some(false) = north-south, None = all red.
pub fn green_axis(t: f32) -> Option<bool> {
    let p = t.rem_euclid(CYCLE);
    if p < 9.0 {
        Some(true)
    } else if p < 11.0 {
        None
    } else if p < 20.0 {
        Some(false)
    } else {
        None
    }
}

fn node_pos(n: IVec2) -> Vec2 {
    Vec2::new(road_center(n.x), road_center(n.y))
}

/// Lane line offset for a direction (right-hand traffic, inner lane).
fn lane_offset(dir: IVec2) -> Vec2 {
    // right of travel (dx, dz) is forward x up = (-dz, dx)
    Vec2::new(-(dir.y as f32), dir.x as f32) * (LANE * 0.5)
}

fn valid(n: IVec2) -> bool {
    n.x >= 0 && n.y >= 0 && n.x <= GRID && n.y <= GRID
}

fn pick_turn(rng: &mut Rng, node: IVec2, dir: IVec2) -> IVec2 {
    let left = IVec2::new(dir.y, -dir.x);
    let right = -left;
    let mut options = vec![];
    for (d, w) in [(dir, 6), (left, 2), (right, 2)] {
        if valid(node + d) {
            for _ in 0..w {
                options.push(d);
            }
        }
    }
    if options.is_empty() {
        -dir
    } else {
        *rng.pick(&options)
    }
}

#[allow(clippy::too_many_arguments)]
fn traffic_population(
    mut commands: Commands,
    time: Res<Time>,
    player: Query<&Transform, With<Player>>,
    cars: Query<(Entity, &Transform, &Seat), With<TrafficAi>>,
    va: Res<VehicleAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rng: ResMut<TrafficRng>,
    mut timer: Local<f32>,
) {
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 0.4;
    let Ok(pt) = player.single() else { return };
    let mut count = 0;
    for (e, t, seat) in &cars {
        let d = t.translation.distance(pt.translation);
        if d > 270.0 && seat.driver != Driver::Player {
            commands.entity(e).despawn();
        } else if matches!(seat.driver, Driver::Npc(_)) {
            count += 1;
        }
    }
    let occupied: Vec<Vec3> = cars.iter().map(|(_, t, _)| t.translation).collect();
    for _ in 0..24 {
        if count >= TRAFFIC_TARGET {
            break;
        }
        // random lane position on a random road segment
        let horizontal = rng.0.chance(0.5);
        let line = rng.0.int(0, GRID);
        let seg = rng.0.int(0, GRID - 1);
        let forward = rng.0.chance(0.5);
        let dir = match (horizontal, forward) {
            (true, true) => IVec2::new(1, 0),
            (true, false) => IVec2::new(-1, 0),
            (false, true) => IVec2::new(0, 1),
            (false, false) => IVec2::new(0, -1),
        };
        let along = block_min(seg) + rng.0.range(10.0, BLOCK - 10.0);
        let off = lane_offset(dir);
        let p2 = if horizontal { Vec2::new(along, road_center(line)) } else { Vec2::new(road_center(line), along) } + off;
        let pos = Vec3::new(p2.x, 0.55, p2.y);
        let d = pos.distance(pt.translation);
        if !(90.0..220.0).contains(&d) || occupied.iter().any(|o| o.distance(pos) < 12.0) {
            continue;
        }
        // the next intersection ahead in the direction of travel
        let node = if horizontal {
            IVec2::new(if forward { seg + 1 } else { seg }, line)
        } else {
            IVec2::new(line, if forward { seg + 1 } else { seg })
        };
        let kind = *rng.0.pick(&[CarKind::Sedan, CarKind::Sedan, CarKind::Suv, CarKind::Van, CarKind::Taxi, CarKind::Sports, CarKind::Sedan]);
        let color = if kind == CarKind::Taxi {
            Color::srgb(0.95, 0.75, 0.05)
        } else {
            let c = *rng.0.pick(&PAINTS);
            Color::srgb(c.0, c.1, c.2)
        };
        let yaw = (-(dir.x as f32)).atan2(-(dir.y as f32));
        let seed = rng.0.next_u32();
        let e = spawn_car(&mut commands, &va, &mut materials, kind, pos, yaw, color, Driver::Npc(seed));
        let next_dir = pick_turn(&mut rng.0, node, dir);
        commands.entity(e).insert(TrafficAi { dir, node, next_dir, cruise: rng.0.range(11.0, 15.0), blocked_t: 0.0 });
        count += 1;
    }
}

#[allow(clippy::type_complexity)]
fn traffic_drive(
    time: Res<Time>,
    mut rng: ResMut<TrafficRng>,
    mut cars: Query<(Entity, &Transform, &Velocity, &Seat, &mut VehicleControl, &mut TrafficAi)>,
    others: Query<(Entity, &Transform), (With<Seat>, Without<TrafficAi>)>,
    peds: Query<&Transform, (With<Ped>, Without<Seat>)>,
    player: Query<(&Transform, &Player), (Without<Seat>, Without<Ped>)>,
) {
    let dt = time.delta_secs();
    let lights = green_axis(time.elapsed_secs());
    // everything a car might have to stop for
    let mut obstacles: Vec<(Entity, Vec3)> = others.iter().map(|(e, t)| (e, t.translation)).collect();
    obstacles.extend(cars.iter().map(|(e, t, ..)| (e, t.translation)));
    obstacles.extend(peds.iter().map(|t| (Entity::PLACEHOLDER, t.translation)));
    if let Ok((t, p)) = player.single() {
        if p.vehicle.is_none() {
            obstacles.push((Entity::PLACEHOLDER, t.translation));
        }
    }
    for (e, t, vel, seat, mut c, mut ai) in &mut cars {
        if !matches!(seat.driver, Driver::Npc(_)) {
            continue;
        }
        let pos = Vec2::new(t.translation.x, t.translation.z);
        let dirf = ai.dir.as_vec2();
        let node = node_pos(ai.node);
        let to_node = (node - pos).dot(dirf);
        // inside the junction box: commit to the chosen exit
        if to_node < LANE * 0.5 && ai.next_dir != ai.dir || to_node < -ROAD * 0.5 {
            ai.dir = ai.next_dir;
            let d = ai.dir;
            ai.node += d;
            if !valid(ai.node) {
                ai.dir = -ai.dir;
                let d = ai.dir;
                ai.node += d * 2;
            }
            let (n, d) = (ai.node, ai.dir);
            ai.next_dir = pick_turn(&mut rng.0, n, d);
            continue;
        }
        // steer toward a point a few metres ahead on our lane line
        let lane_point = node + lane_offset(ai.dir) - dirf * (to_node - 9.0).max(-4.0);
        let target = Vec3::new(lane_point.x, t.translation.y, lane_point.y);
        let local = t.rotation.inverse() * (target - t.translation);
        let angle = local.x.atan2(-local.z);
        let speed = vel.linear.length();
        let mut want = ai.cruise;
        if ai.next_dir != ai.dir && to_node < 22.0 {
            want = want.min(6.5);
        }
        // red light: stop before the stop line
        let our_axis = ai.dir.x != 0;
        let stop_line = ROAD * 0.5 + 4.5;
        if lights != Some(our_axis) && to_node > stop_line - 1.0 && to_node < stop_line + 26.0 {
            want = want.min(((to_node - stop_line) * 0.6).max(0.0));
        }
        // keep distance from whatever is ahead in our path
        let fwd = t.rotation * Vec3::NEG_Z;
        let look = 8.0 + speed * 1.2;
        for (oe, op) in &obstacles {
            if *oe == e {
                continue;
            }
            let d = *op - t.translation;
            let ahead = d.dot(fwd);
            let side = (d - fwd * ahead).with_y(0.0).length();
            if ahead > 0.0 && ahead < look && side < 2.4 {
                want = want.min(((ahead - 6.5) * 0.8).max(0.0));
            }
        }
        if want < 0.5 && speed < 1.0 {
            ai.blocked_t += dt;
        } else {
            ai.blocked_t = 0.0;
        }
        let (throttle, brake) = if speed < want - 0.5 {
            (((want - speed) * 0.4).clamp(0.25, 1.0), 0.0)
        } else if speed > want + 1.0 {
            (0.0, ((speed - want) * 0.3).clamp(0.2, 1.0))
        } else {
            (0.12, 0.0)
        };
        // stopped and wanting to stay stopped: hold on the parking brake
        // (the brake pedal at a standstill selects reverse)
        let hold = want < 0.3 && speed < 1.5;
        let (throttle, brake) = if hold { (0.0, 0.0) } else { (throttle, brake) };
        *c = VehicleControl { throttle, brake, steer: (angle * 2.2).clamp(-1.0, 1.0), handbrake: hold };
    }
}

#[derive(Resource)]
struct SignalMats {
    ew_red: Handle<StandardMaterial>,
    ew_green: Handle<StandardMaterial>,
    ns_red: Handle<StandardMaterial>,
    ns_green: Handle<StandardMaterial>,
}

/// Signal poles at every junction corner, lamps merged into four meshes whose
/// shared materials are switched with the light phase.
fn setup_signals(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let mut poles = MeshBuilder::new();
    let mut lamps: [MeshBuilder; 4] = Default::default(); // ew_red, ew_green, ns_red, ns_green
    let dark = rgb(0.12, 0.12, 0.13);
    for i in 0..=GRID {
        for j in 0..=GRID {
            let c = Vec2::new(road_center(i), road_center(j));
            for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let p = c + Vec2::new(sx, sz) * (ROAD * 0.5 + 0.8);
                if !(CITY.contains(p)) {
                    continue;
                }
                let base = Vec3::new(p.x, CURB, p.y);
                poles.cylinder(base, 0.09, 3.4, 8, dark, false);
                // housing facing traffic along x and along z
                for (face, ew) in [(Vec3::new(-sx, 0.0, 0.0), true), (Vec3::new(0.0, 0.0, -sz), false)] {
                    let hc = base + Vec3::Y * 3.0 + face * 0.18;
                    poles.cuboid(hc, Vec3::new(0.14, 0.32, 0.14), dark, 1.0);
                    let red = hc + Vec3::Y * 0.14 + face * 0.15;
                    let green = hc - Vec3::Y * 0.14 + face * 0.15;
                    let (ri, gi) = if ew { (0, 1) } else { (2, 3) };
                    lamps[ri].ellipsoid(red, Vec3::splat(0.075), 8, 5, WHITE);
                    lamps[gi].ellipsoid(green, Vec3::splat(0.075), 8, 5, WHITE);
                }
            }
        }
    }
    let pole_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.12, 0.12, 0.13), metallic: 0.6, perceptual_roughness: 0.5, ..default() });
    commands.spawn((Mesh3d(meshes.add(poles.build())), MeshMaterial3d(pole_mat), Transform::IDENTITY));
    let mut lamp_mat = || materials.add(StandardMaterial { base_color: Color::srgb(0.1, 0.1, 0.1), emissive: LinearRgba::BLACK, ..default() });
    let mats = SignalMats { ew_red: lamp_mat(), ew_green: lamp_mat(), ns_red: lamp_mat(), ns_green: lamp_mat() };
    for (k, mb) in lamps.into_iter().enumerate() {
        let m = [&mats.ew_red, &mats.ew_green, &mats.ns_red, &mats.ns_green][k].clone();
        commands.spawn((Mesh3d(meshes.add(mb.build())), MeshMaterial3d(m), Transform::IDENTITY, NoRaytrace));
    }
    let _ = Affine3A::IDENTITY;
    commands.insert_resource(mats);
}

fn signal_lights(time: Res<Time>, mats: Option<Res<SignalMats>>, mut materials: ResMut<Assets<StandardMaterial>>, mut last: Local<Option<Option<bool>>>) {
    let Some(mats) = mats else { return };
    let g = green_axis(time.elapsed_secs());
    if *last == Some(g) {
        return;
    }
    *last = Some(g);
    let red = LinearRgba::rgb(30.0, 0.5, 0.3);
    let green = LinearRgba::rgb(0.4, 25.0, 4.0);
    let off = LinearRgba::BLACK;
    let set = |materials: &mut Assets<StandardMaterial>, h: &Handle<StandardMaterial>, e: LinearRgba| {
        if let Some(mut m) = materials.get_mut(h) {
            m.emissive = e;
        }
    };
    set(&mut materials, &mats.ew_red, if g == Some(true) { off } else { red });
    set(&mut materials, &mats.ew_green, if g == Some(true) { green } else { off });
    set(&mut materials, &mats.ns_red, if g == Some(false) { off } else { red });
    set(&mut materials, &mats.ns_green, if g == Some(false) { green } else { off });
}
