//! The player character: Rapier kinematic controller, walk / sprint / jump,
//! knockdowns, drowning and the "wasted" respawn.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::camera::CameraRig;
use crate::humanoid::{spawn_humanoid, HumanoidAssets, Look, Rig};
use crate::input::GameInput;
use crate::peds::character_controller;
use crate::vehicle::{Driver, Vehicle, GROUP_PED, GROUP_WORLD};
use crate::world::*;

pub struct PlayerPlugin;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerMoveSet;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_player.after(crate::humanoid::HumanoidAssetsSet))
            .add_systems(Update, (player_movement, player_status).chain().in_set(PlayerMoveSet));
    }
}

#[derive(Component)]
pub struct Player {
    pub health: f32,
    pub armor: f32,
    pub money: i64,
    pub vehicle: Option<Entity>,
    pub vel: Vec3,
    pub knock_t: f32,
    pub visual: Entity,
    pub wasted_t: f32,
    pub water_t: f32,
    pub grounded: bool,
    pub busted_t: f32,
    /// seconds since last on the ground (coyote time, landing)
    pub ground_t: f32,
    /// jump pressed recently (buffered)
    pub jump_buf: f32,
    /// seconds since last hurt (health regen)
    pub calm_t: f32,
    pub last_health: f32,
}

impl Player {
    /// Damage from bullets, blows, crashes and blasts: body armour soaks it up first.
    pub fn hurt(&mut self, dmg: f32) {
        if dmg <= 0.0 {
            return;
        }
        let soak = dmg.min(self.armor);
        self.armor -= soak;
        self.health -= dmg - soak;
    }
}

fn spawn_player(mut commands: Commands, assets: Res<HumanoidAssets>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let vis = spawn_humanoid(&mut commands, &assets, &mut materials, Look::player());
    commands.entity(vis).insert(Transform::from_xyz(0.0, -0.88, 0.0));
    let p = commands
        .spawn((
            Transform::from_translation(spawn_point()),
            Visibility::default(),
            Collider::capsule_y(0.55, 0.3),
            Sensor,
            CollisionGroups::new(GROUP_PED, GROUP_WORLD),
            character_controller(),
            Player {
                health: 100.0,
                armor: 0.0,
                money: 2500,
                vehicle: None,
                vel: Vec3::ZERO,
                knock_t: 0.0,
                visual: vis,
                wasted_t: 0.0,
                water_t: 0.0,
                grounded: false,
                busted_t: 0.0,
                ground_t: 0.0,
                jump_buf: 0.0,
                calm_t: 0.0,
                last_health: 100.0,
            },
        ))
        .id();
    commands.entity(p).add_child(vis);
}

#[allow(clippy::type_complexity)]
fn player_movement(
    time: Res<Time>,
    mut input: ResMut<GameInput>,
    rig: Res<CameraRig>,
    ars: Res<crate::weapons::Arsenal>,
    mut q: Query<(&mut Player, &mut Transform, &mut KinematicCharacterController, Option<&KinematicCharacterControllerOutput>)>,
    mut rigs: Query<&mut Rig>,
) {
    let dt = time.delta_secs().min(0.05);
    let Ok((mut p, mut t, mut kcc, out)) = q.single_mut() else { return };
    if p.vehicle.is_some() {
        input.jump = false;
        kcc.translation = None;
        p.ground_t = 0.0;
        return;
    }
    let grounded = out.map(|o| o.grounded).unwrap_or(false);
    let was_air_t = p.ground_t;
    let fall_speed = -p.vel.y;
    if grounded {
        p.ground_t = 0.0;
    } else {
        p.ground_t += dt;
    }
    p.grounded = grounded;
    let dead = p.health <= 0.0;
    let knocked = p.knock_t > 0.0 || dead;
    p.knock_t = (p.knock_t - dt).max(0.0);

    // landing: dip into the knees, hurt on a big drop
    let mut land = 0.0;
    if grounded && was_air_t > 0.25 && fall_speed > 4.0 {
        land = ((fall_speed - 4.0) / 9.0).clamp(0.15, 1.0);
        if fall_speed > 15.0 && !dead {
            p.hurt((fall_speed - 15.0) * 7.0);
            if fall_speed > 19.0 {
                p.knock_t = 1.2;
            }
        }
    }

    let stats = ars.weapon().stats();
    let aiming = !knocked && (input.aim || ars.cooldown > -0.5) && !stats.melee();
    // swinging a bat / throwing: turn to where the camera looks too
    let face_cam = aiming || (!knocked && ars.cooldown > 0.0);
    let fwd = Vec3::new(-rig.yaw.sin(), 0.0, -rig.yaw.cos());
    let right = Vec3::new(rig.yaw.cos(), 0.0, -rig.yaw.sin());
    let wish = (fwd * input.move_axis.y + right * input.move_axis.x).clamp_length_max(1.0);
    let in_water = t.translation.y < SEA_LEVEL + 0.2;
    // walk (Alt) / jog / sprint; aiming slows you to a strafe; heavy guns slow you down
    let mut speed = if in_water {
        2.2
    } else if aiming {
        if input.walk { 1.4 } else { 2.9 }
    } else if input.sprint {
        7.4
    } else if input.walk {
        1.6
    } else {
        4.6
    };
    speed *= stats.move_mult;
    let target = if knocked { Vec3::ZERO } else { wish * speed };
    let flat = p.vel.with_y(0.0);
    // snappy starts and stops on the ground, a little air control
    let rate = if knocked {
        if grounded { 3.0 } else { 0.0 }
    } else if !grounded {
        2.2
    } else if target.length() + 0.1 < flat.length() {
        13.0
    } else {
        9.0
    };
    let k = 1.0 - (-dt * rate).exp();
    p.vel.x += (target.x - p.vel.x) * k;
    p.vel.z += (target.z - p.vel.z) * k;

    // jump with a short buffer and coyote time
    if input.jump {
        p.jump_buf = 0.15;
    }
    input.jump = false;
    p.jump_buf = (p.jump_buf - dt).max(0.0);
    let can_jump = p.ground_t < 0.12 && !knocked && !in_water && p.vel.y <= 0.5;
    if p.jump_buf > 0.0 && can_jump {
        p.vel.y = 6.4;
        p.jump_buf = 0.0;
        p.ground_t = 0.2;
    } else if grounded && p.vel.y < 0.0 {
        p.vel.y = -1.5;
    }
    if in_water {
        // tread water at the surface
        let depth = SEA_LEVEL - 0.35 - t.translation.y;
        p.vel.y += depth * 12.0 * dt - p.vel.y * 2.0 * dt;
    } else {
        // fall a bit faster than you rise: less floaty
        p.vel.y -= if p.vel.y > 0.0 { 19.0 } else { 26.0 } * dt;
    }
    p.vel.y = p.vel.y.max(-50.0);
    kcc.translation = Some(p.vel * dt);

    // facing: toward the camera while aiming/shooting, else toward where you're going
    let flat = p.vel.with_y(0.0);
    let old_yaw = yaw_of(t.rotation);
    if !knocked {
        let want = if face_cam {
            Some(rig.yaw)
        } else if flat.length() > 0.3 {
            Some((-flat.x).atan2(-flat.z))
        } else {
            None
        };
        if let Some(w) = want {
            let turn = if face_cam { 22.0 } else if flat.length() < 2.0 { 14.0 } else { 9.0 };
            let d = wrap(w - old_yaw);
            let new = old_yaw + d * (1.0 - (-dt * turn).exp());
            t.rotation = Quat::from_rotation_y(new);
        }
    }
    let yaw_rate = wrap(yaw_of(t.rotation) - old_yaw) / dt.max(1e-4);
    // feet: where are we going relative to where we face?
    let local = t.rotation.inverse() * flat;
    let move_dir = if local.length() > 0.2 { Vec2::new(local.x, -local.z).normalize() } else { Vec2::Y };
    let air_t = p.ground_t;
    if let Ok(mut r) = rigs.get_mut(p.visual) {
        r.speed = flat.length();
        r.airborne = !knocked && !in_water && (air_t > 0.15 || p.vel.y > 1.0);
        r.ragdoll = knocked;
        r.vy = p.vel.y;
        r.move_dir = r.move_dir.lerp(move_dir, 1.0 - (-dt * 10.0).exp());
        let lean = (-yaw_rate * flat.length() * 0.012).clamp(-0.25, 0.25);
        r.lean += (lean - r.lean) * (1.0 - (-dt * 8.0).exp());
        r.aim_pitch = rig.pitch;
        if land > 0.0 {
            r.land = r.land.max(land);
        }
    }
    // health creeps back up to half if you stay out of trouble
    if p.health < p.last_health - 0.01 {
        p.calm_t = 0.0;
    }
    p.calm_t += dt;
    if !dead && p.calm_t > 6.0 && p.health < 50.0 {
        p.health = (p.health + 4.0 * dt).min(50.0);
    }
    p.last_health = p.health;
}

fn yaw_of(q: Quat) -> f32 {
    let f = q * Vec3::NEG_Z;
    (-f.x).atan2(-f.z)
}

fn wrap(a: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    (a + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI
}

#[allow(clippy::too_many_arguments)]
fn player_status(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Player, &mut Transform, &mut Visibility), Without<crate::seats::Seat>>,
    mut cars: Query<(&mut crate::seats::Seat, &Transform)>,
    mut hud: ResMut<crate::hud::HudMessages>,
    mut wanted: ResMut<crate::wanted::Wanted>,
    ragdolls: Query<&crate::ragdoll::RagdollParts>,
    assets: Res<HumanoidAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();
    let Ok((player_e, mut p, mut t, mut vis)) = q.single_mut() else { return };
    if p.busted_t > 0.0 {
        return;
    }
    // drowning: water is survivable for a while
    if p.vehicle.is_none() && t.translation.y < SEA_LEVEL - 0.2 {
        p.water_t += dt;
        if p.water_t > 25.0 {
            p.health -= 10.0 * dt;
        }
    } else {
        p.water_t = 0.0;
    }
    if p.health <= 0.0 {
        if p.wasted_t == 0.0 {
            hud.big("WASTED", Color::srgb(0.85, 0.1, 0.1));
            wanted.clear();
            if p.vehicle.is_none() {
                commands.entity(p.visual).insert(crate::ragdoll::WantsRagdoll { vel: p.vel });
            }
            // a dead driver is thrown clear of the car
            if let Some(ve) = p.vehicle.take() {
                if let Ok((mut seat, ct)) = cars.get_mut(ve) {
                    seat.driver = Driver::None;
                    t.translation = ct.translation + ct.rotation * Vec3::new(-(seat.half.x + 1.0), 0.6, 0.0);
                }
                *vis = Visibility::Inherited;
            }
        }
        p.wasted_t += dt;
        if p.wasted_t > 4.5 {
            p.health = 100.0;
            p.wasted_t = 0.0;
            p.vel = Vec3::ZERO;
            p.knock_t = 0.0;
            t.translation = hospital_point();
            // fresh body (the old one may be a ragdoll lying in the street)
            if let Ok(parts) = ragdolls.get(p.visual) {
                for e in &parts.0 {
                    commands.entity(*e).despawn();
                }
            }
            commands.entity(p.visual).despawn();
            let vis = spawn_humanoid(&mut commands, &assets, &mut materials, Look::player());
            commands.entity(vis).insert(Transform::from_xyz(0.0, -0.88, 0.0));
            commands.entity(player_e).add_child(vis);
            p.visual = vis;
            let bill = (p.money / 20).max(100).min(p.money.max(0));
            p.money -= bill;
            hud.push(&format!("Port Solace General Hospital - bill ${bill}"));
        }
    }
}
