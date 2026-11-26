//! Low-poly humanoid built from meshes, with a procedural walk/run/jump rig.
//! Shared by the player and pedestrians.

use bevy::math::Affine3A;
use bevy::prelude::*;

use crate::meshgen::{MeshBuilder, WHITE};

pub struct HumanoidPlugin;

impl Plugin for HumanoidPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_humanoid_assets.in_set(HumanoidAssetsSet)).add_systems(Update, (animate_rigs, strip_shadows));
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct HumanoidAssetsSet;

#[derive(Clone, Copy)]
pub struct Look {
    pub skin: Color,
    pub top: Color,
    pub legs: Color,
    pub shoes: Color,
    pub hair: Color,
}

impl Look {
    pub fn player() -> Self {
        Look {
            skin: Color::srgb(0.72, 0.52, 0.4),
            top: Color::srgb(0.85, 0.42, 0.12),
            legs: Color::srgb(0.16, 0.2, 0.3),
            shoes: Color::srgb(0.9, 0.9, 0.88),
            hair: Color::srgb(0.08, 0.06, 0.05),
        }
    }
    pub fn cop() -> Self {
        Look {
            skin: Color::srgb(0.75, 0.56, 0.44),
            top: Color::srgb(0.08, 0.12, 0.28),
            legs: Color::srgb(0.06, 0.07, 0.12),
            shoes: Color::srgb(0.03, 0.03, 0.03),
            hair: Color::srgb(0.12, 0.08, 0.05),
        }
    }
    pub fn random(seed: u32) -> Self {
        let mut r = crate::meshgen::Rng::new(seed as u64 * 7919 + 13);
        let skins = [
            Color::srgb(0.95, 0.78, 0.65),
            Color::srgb(0.82, 0.62, 0.48),
            Color::srgb(0.6, 0.42, 0.3),
            Color::srgb(0.4, 0.27, 0.18),
            Color::srgb(0.28, 0.18, 0.12),
        ];
        let tops = [
            Color::srgb(0.8, 0.1, 0.1),
            Color::srgb(0.1, 0.3, 0.7),
            Color::srgb(0.9, 0.9, 0.9),
            Color::srgb(0.15, 0.15, 0.15),
            Color::srgb(0.2, 0.55, 0.3),
            Color::srgb(0.85, 0.75, 0.3),
            Color::srgb(0.55, 0.3, 0.6),
            Color::srgb(0.95, 0.5, 0.6),
        ];
        let legs = [Color::srgb(0.15, 0.2, 0.35), Color::srgb(0.12, 0.12, 0.12), Color::srgb(0.55, 0.5, 0.4), Color::srgb(0.35, 0.3, 0.25)];
        let hair = [Color::srgb(0.05, 0.04, 0.03), Color::srgb(0.35, 0.22, 0.1), Color::srgb(0.75, 0.6, 0.3), Color::srgb(0.5, 0.5, 0.5)];
        Look {
            skin: *r.pick(&skins),
            top: *r.pick(&tops),
            legs: *r.pick(&legs),
            shoes: *r.pick(&[Color::srgb(0.1, 0.1, 0.1), Color::srgb(0.9, 0.9, 0.9), Color::srgb(0.4, 0.25, 0.15)]),
            hair: *r.pick(&hair),
        }
    }
}

#[derive(Resource)]
pub struct HumanoidAssets {
    pub torso: Handle<Mesh>,
    pub hips: Handle<Mesh>,
    pub head: Handle<Mesh>,
    pub hair: Handle<Mesh>,
    pub upper_arm: Handle<Mesh>,
    pub forearm: Handle<Mesh>,
    pub hand: Handle<Mesh>,
    pub thigh: Handle<Mesh>,
    pub shin: Handle<Mesh>,
    pub shoe: Handle<Mesh>,
}

#[derive(Component, Default)]
pub struct Rig {
    pub aim: bool,
    pub hand_r: Option<Entity>,
    pub phase: f32,
    /// horizontal speed in m/s
    pub speed: f32,
    pub airborne: bool,
    pub ragdoll: bool,
    pub seated: bool,
    pub punch: f32,
    /// local direction of travel (x right, y forward); zero = forward
    pub move_dir: Vec2,
    /// camera pitch while aiming (radians, up positive)
    pub aim_pitch: f32,
    /// rifle-style grip: both hands on the gun / bat swing
    pub two_handed: bool,
    /// landing impact 0..1, decays
    pub land: f32,
    /// sideways lean into turns (radians)
    pub lean: f32,
    /// vertical velocity (for the jump pose)
    pub vy: f32,
    /// flinch from being hit 0..1, decays
    pub hit: f32,
    pub idle_t: f32,
}

#[derive(Component, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Limb {
    ArmL,
    ArmR,
    ForearmL,
    ForearmR,
    LegL,
    LegR,
    ShinL,
    ShinR,
    Body,
    /// upper body pivot at hip height (lean, twist, aim pitch); not a ragdoll part
    Spine,
}

fn mesh(mb: MeshBuilder, meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
    meshes.add(mb.build())
}

fn ellipse_ring(y: f32, rx: f32, rz: f32, zoff: f32, n: usize) -> Vec<Vec3> {
    (0..n)
        .map(|k| {
            let a = k as f32 / n as f32 * std::f32::consts::TAU;
            Vec3::new(a.cos() * rx, y, a.sin() * rz + zoff)
        })
        .collect()
}

/// Vertical loft through elliptical sections (y, rx, rz, z offset), capped at both ends.
fn body_loft(sections: &[(f32, f32, f32, f32)]) -> MeshBuilder {
    let rings: Vec<Vec<Vec3>> = sections.iter().map(|&(y, rx, rz, zo)| ellipse_ring(y, rx, rz, zo, 18)).collect();
    let mut out = [MeshBuilder::new()];
    crate::meshgen::loft(&rings, &mut out, &|_, _| 0);
    let [mut mb] = out;
    mb.polygon(&rings[0], Vec3::NEG_Y, WHITE);
    mb.polygon(rings.last().unwrap(), Vec3::Y, WHITE);
    mb
}

/// Tapered limb hanging down from a joint at the origin, with rounded joint ends.
fn limb(len: f32, r_top: f32, r_bot: f32) -> MeshBuilder {
    let mut mb = MeshBuilder::new();
    mb.cylinder_at(Affine3A::from_translation(Vec3::new(0.0, -len, 0.0)), r_bot, r_top, len, 12, WHITE, false);
    mb.ellipsoid(Vec3::ZERO, Vec3::splat(r_top * 1.02), 12, 6, WHITE);
    mb.ellipsoid(Vec3::new(0.0, -len, 0.0), Vec3::splat(r_bot * 1.02), 12, 6, WHITE);
    mb
}

fn setup_humanoid_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    // chest -> waist, slightly barrel-chested, shoulders rounded off
    let mut torso = body_loft(&[
        (-0.15, 0.152, 0.098, 0.0),
        (-0.06, 0.148, 0.097, 0.0),
        (0.04, 0.165, 0.106, -0.005),
        (0.14, 0.188, 0.118, -0.012),
        (0.22, 0.198, 0.112, -0.008),
        (0.28, 0.17, 0.095, 0.0),
        (0.31, 0.1, 0.07, 0.0),
        (0.33, 0.055, 0.05, 0.0),
    ]);
    for sx in [-1.0f32, 1.0] {
        torso.ellipsoid(Vec3::new(sx * 0.185, 0.235, 0.0), Vec3::new(0.06, 0.055, 0.062), 12, 6, WHITE);
    }
    // collar
    torso.ellipsoid(Vec3::new(0.0, 0.31, 0.01), Vec3::new(0.085, 0.03, 0.07), 12, 4, WHITE);
    let hips = body_loft(&[(-0.11, 0.15, 0.095, 0.0), (-0.04, 0.172, 0.105, 0.0), (0.04, 0.168, 0.102, 0.0), (0.1, 0.153, 0.098, 0.0)]);
    let mut head = MeshBuilder::new();
    head.ellipsoid(Vec3::new(0.0, 0.012, 0.006), Vec3::new(0.096, 0.114, 0.106), 16, 10, WHITE); // cranium
    head.ellipsoid(Vec3::new(0.0, -0.06, -0.022), Vec3::new(0.074, 0.062, 0.074), 14, 8, WHITE); // jaw
    head.ellipsoid(Vec3::new(0.0, -0.018, -0.104), Vec3::new(0.016, 0.03, 0.022), 8, 5, WHITE); // nose
    head.ellipsoid(Vec3::new(0.0, -0.085, -0.075), Vec3::new(0.035, 0.022, 0.02), 8, 4, WHITE); // chin
    for sx in [-1.0f32, 1.0] {
        head.ellipsoid(Vec3::new(sx * 0.097, -0.008, 0.012), Vec3::new(0.016, 0.03, 0.022), 8, 5, WHITE); // ears
        head.ellipsoid(Vec3::new(sx * 0.035, 0.03, -0.088), Vec3::new(0.022, 0.009, 0.012), 8, 4, WHITE); // brow
    }
    head.cylinder_at(Affine3A::from_translation(Vec3::new(0.0, -0.2, 0.005)), 0.052, 0.046, 0.15, 12, WHITE, false); // neck
    let mut hair = MeshBuilder::new();
    hair.ellipsoid(Vec3::new(0.0, 0.05, 0.014), Vec3::new(0.103, 0.083, 0.112), 16, 8, WHITE);
    hair.ellipsoid(Vec3::new(0.0, 0.0, 0.058), Vec3::new(0.098, 0.085, 0.06), 14, 7, WHITE);
    let mut hand = MeshBuilder::new();
    hand.ellipsoid(Vec3::new(0.0, -0.01, 0.0), Vec3::new(0.038, 0.058, 0.024), 10, 6, WHITE);
    hand.ellipsoid(Vec3::new(0.0, -0.075, 0.0), Vec3::new(0.034, 0.03, 0.02), 10, 5, WHITE); // fingers
    hand.ellipsoid(Vec3::new(0.0, -0.02, -0.03), Vec3::new(0.014, 0.034, 0.014), 8, 4, WHITE); // thumb
    let mut shoe = MeshBuilder::new();
    shoe.ellipsoid(Vec3::new(0.0, 0.005, -0.045), Vec3::new(0.055, 0.045, 0.13), 14, 7, WHITE);
    shoe.box_at(Affine3A::from_translation(Vec3::new(0.0, -0.035, -0.045)), Vec3::new(0.057, 0.012, 0.132), WHITE, 1.0, true, true);
    let mut shin = limb(0.42, 0.058, 0.044);
    shin.ellipsoid(Vec3::new(0.0, -0.15, 0.016), Vec3::new(0.056, 0.12, 0.06), 12, 6, WHITE); // calf
    let mut thigh = limb(0.43, 0.084, 0.06);
    thigh.ellipsoid(Vec3::new(0.0, -0.16, -0.008), Vec3::new(0.08, 0.15, 0.084), 12, 6, WHITE);
    commands.insert_resource(HumanoidAssets {
        torso: mesh(torso, &mut meshes),
        hips: mesh(hips, &mut meshes),
        head: mesh(head, &mut meshes),
        hair: mesh(hair, &mut meshes),
        upper_arm: mesh(limb(0.3, 0.062, 0.05), &mut meshes),
        forearm: mesh(limb(0.27, 0.05, 0.039), &mut meshes),
        hand: mesh(hand, &mut meshes),
        thigh: mesh(thigh, &mut meshes),
        shin: mesh(shin, &mut meshes),
        shoe: mesh(shoe, &mut meshes),
    });
}

/// Clothing/skin materials are shared between everyone wearing the same colour
/// (people are built from small palettes), which keeps draws batched.
fn mat(materials: &mut Assets<StandardMaterial>, c: Color, rough: f32) -> Handle<StandardMaterial> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<[u32; 5], Handle<StandardMaterial>>>> = OnceLock::new();
    let l = c.to_linear();
    let key = [l.red.to_bits(), l.green.to_bits(), l.blue.to_bits(), l.alpha.to_bits(), rough.to_bits()];
    let mut cache = CACHE.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap();
    if let Some(h) = cache.get(&key) {
        if materials.contains(h.id()) {
            return h.clone();
        }
    }
    let h = materials.add(StandardMaterial { base_color: c, perceptual_roughness: rough, ..default() });
    cache.insert(key, h.clone());
    h
}

/// Marks a body whose parts shouldn't cast shadows (people sitting inside cars).
#[derive(Component)]
pub struct NoShadowsPending;

pub fn no_shadows(commands: &mut Commands, root: Entity) {
    commands.entity(root).insert(NoShadowsPending);
}

fn strip_shadows(mut commands: Commands, q: Query<Entity, With<NoShadowsPending>>, children: Query<&Children>, meshes: Query<(), With<Mesh3d>>) {
    for root in &q {
        for e in children.iter_descendants(root) {
            if meshes.contains(e) {
                commands.entity(e).insert(bevy::light::NotShadowCaster);
            }
        }
        commands.entity(root).remove::<NoShadowsPending>();
    }
}

/// Spawns the visual body (feet at local y = 0, facing -Z) and returns its root.
pub fn spawn_humanoid(commands: &mut Commands, a: &HumanoidAssets, materials: &mut Assets<StandardMaterial>, look: Look) -> Entity {
    let skin = mat(materials, look.skin, 0.6);
    let top = mat(materials, look.top, 0.8);
    let legs = mat(materials, look.legs, 0.85);
    let shoes = mat(materials, look.shoes, 0.5);
    let hair = mat(materials, look.hair, 0.7);

    let root = commands.spawn((Transform::default(), Visibility::default(), Rig::default())).id();
    let body = commands
        .spawn((Transform::from_xyz(0.0, 0.0, 0.0), Visibility::default(), Limb::Body))
        .id();
    commands.entity(root).add_child(body);

    let part = |commands: &mut Commands, parent: Entity, m: &Handle<Mesh>, mt: &Handle<StandardMaterial>, t: Transform| {
        let e = commands.spawn((Mesh3d(m.clone()), MeshMaterial3d(mt.clone()), t)).id();
        commands.entity(parent).add_child(e);
        e
    };
    part(commands, body, &a.hips, &legs, Transform::from_xyz(0.0, 0.97, 0.0));
    // upper body hangs off a spine pivot at hip height
    const HIP: f32 = 0.97;
    let spine = commands.spawn((Transform::from_xyz(0.0, HIP, 0.0), Visibility::default(), Limb::Spine)).id();
    commands.entity(body).add_child(spine);
    part(commands, spine, &a.torso, &top, Transform::from_xyz(0.0, 1.2 - HIP, 0.0));
    part(commands, spine, &a.head, &skin, Transform::from_xyz(0.0, 1.68 - HIP, 0.0));
    part(commands, spine, &a.hair, &hair, Transform::from_xyz(0.0, 1.68 - HIP, 0.0));

    for (side, arm, fore) in [(-1.0f32, Limb::ArmL, Limb::ForearmL), (1.0, Limb::ArmR, Limb::ForearmR)] {
        let shoulder = commands.spawn((Transform::from_xyz(0.212 * side, 1.455 - HIP, 0.0), Visibility::default(), arm)).id();
        commands.entity(spine).add_child(shoulder);
        part(commands, shoulder, &a.upper_arm, &top, Transform::IDENTITY);
        let elbow = commands.spawn((Transform::from_xyz(0.0, -0.3, 0.0), Visibility::default(), fore)).id();
        commands.entity(shoulder).add_child(elbow);
        part(commands, elbow, &a.forearm, &skin, Transform::IDENTITY);
        let hand = part(commands, elbow, &a.hand, &skin, Transform::from_xyz(0.0, -0.31, 0.0));
        if side > 0.0 {
            commands.entity(root).insert(Rig { hand_r: Some(hand), ..default() });
        }
    }
    for (side, leg, shin) in [(-1.0f32, Limb::LegL, Limb::ShinL), (1.0, Limb::LegR, Limb::ShinR)] {
        let hip = commands.spawn((Transform::from_xyz(0.1 * side, 0.93, 0.0), Visibility::default(), leg)).id();
        commands.entity(body).add_child(hip);
        part(commands, hip, &a.thigh, &legs, Transform::IDENTITY);
        let knee = commands.spawn((Transform::from_xyz(0.0, -0.43, 0.0), Visibility::default(), shin)).id();
        commands.entity(hip).add_child(knee);
        part(commands, knee, &a.shin, &legs, Transform::IDENTITY);
        part(commands, knee, &a.shoe, &shoes, Transform::from_xyz(0.0, -0.45, 0.0));
    }
    root
}

/// Procedural animation. The gait is driven by distance travelled (one cycle =
/// two steps, step length grows with speed) so feet don't skate; legs swing
/// along the actual direction of travel (strafe / backpedal while aiming); the
/// upper body leans, counter-twists and pitches with the aim.
fn animate_rigs(time: Res<Time>, mut rigs: Query<(&mut Rig, &Children)>, children_q: Query<&Children>, mut limbs: Query<(&Limb, &mut Transform)>) {
    use std::f32::consts::{FRAC_PI_2, PI};
    let dt = time.delta_secs().min(0.05);
    for (mut rig, children) in &mut rigs {
        let speed = rig.speed;
        let step_len = 0.55 + 0.19 * speed.min(8.0);
        rig.phase = (rig.phase + dt * PI * speed / step_len) % (PI * 64.0);
        rig.idle_t += dt;
        rig.punch = (rig.punch - dt * 3.0).max(0.0);
        rig.land = (rig.land - dt * 3.5).max(0.0);
        rig.hit = (rig.hit - dt * 4.0).max(0.0);
        let ph = rig.phase;
        let (s, c) = ph.sin_cos();
        let run = ((speed - 2.0) / 5.0).clamp(0.0, 1.0);
        let moving = (speed / 0.6).clamp(0.0, 1.0);
        // direction of travel in body space (-Z is forward)
        let md = if rig.move_dir.length_squared() > 0.01 { rig.move_dir.normalize() } else { Vec2::Y };
        let d = Vec3::new(md.x, 0.0, -md.y);
        let swing_axis = Vec3::NEG_Y.cross(d).normalize_or(Vec3::X);
        let hip_amp = (0.3 + 0.4 * run) * moving;
        let knee_stance = 0.06 + 0.16 * run * moving;
        let knee_swing = (0.45 + 1.05 * run) * moving;
        let arm_amp = (0.22 + 0.6 * run) * moving * md.y.abs().max(0.4);
        let elbow = 0.18 + 1.25 * run * moving;
        let (air, rag, seated, aim, two, punch) = (rig.airborne, rig.ragdoll, rig.seated, rig.aim, rig.two_handed, rig.punch);
        let (land, hit, lean, pitch, vy) = (rig.land, rig.hit, rig.lean, rig.aim_pitch.clamp(-1.0, 1.0), rig.vy);
        let fwd_sign = if md.y < -0.2 { -1.0 } else { 1.0 };
        let breath = (rig.idle_t * 1.7).sin();
        let k = 1.0 - (-dt * 26.0).exp();
        let mut stack: Vec<Entity> = children.iter().collect();
        while let Some(e) = stack.pop() {
            if let Ok(ch) = children_q.get(e) {
                stack.extend(ch.iter());
            }
            let Ok((limb, mut t)) = limbs.get_mut(e) else { continue };
            let leg = |sgn: f32| Quat::from_axis_angle(swing_axis, sgn * hip_amp * s);
            let knee = |cc: f32| Quat::from_rotation_x(-(knee_stance + knee_swing * cc.max(0.0).powf(1.3) + land * 0.9));
            let rot = if seated {
                match limb {
                    Limb::LegL | Limb::LegR => Quat::from_rotation_x(1.45),
                    Limb::ShinL | Limb::ShinR => Quat::from_rotation_x(-1.35),
                    Limb::ArmL => Quat::from_rotation_x(0.95) * Quat::from_rotation_z(0.25),
                    Limb::ArmR => Quat::from_rotation_x(0.95) * Quat::from_rotation_z(-0.25),
                    Limb::ForearmL | Limb::ForearmR => Quat::from_rotation_x(0.35),
                    _ => Quat::IDENTITY,
                }
            } else if rag {
                match limb {
                    Limb::Body => Quat::from_rotation_x(-FRAC_PI_2),
                    Limb::ArmL => Quat::from_rotation_z(-1.2),
                    Limb::ArmR => Quat::from_rotation_z(1.2),
                    _ => Quat::IDENTITY,
                }
            } else if air {
                // tuck going up, reach for the ground coming down
                let up = (vy / 5.0).clamp(-1.0, 1.0);
                let tuck = 0.5 + 0.5 * up;
                match limb {
                    Limb::LegL => Quat::from_rotation_x(0.25 + 0.55 * tuck),
                    Limb::LegR => Quat::from_rotation_x(-0.15 + 0.25 * tuck),
                    Limb::ShinL => Quat::from_rotation_x(-(0.3 + 1.0 * tuck)),
                    Limb::ShinR => Quat::from_rotation_x(-(0.5 + 0.6 * tuck)),
                    Limb::ArmL if !aim => Quat::from_rotation_x(0.3) * Quat::from_rotation_z(-0.5 - 0.5 * (1.0 - tuck)),
                    Limb::ArmR if !aim => Quat::from_rotation_x(0.3) * Quat::from_rotation_z(0.5 + 0.5 * (1.0 - tuck)),
                    Limb::ArmL => Quat::from_rotation_x(FRAC_PI_2 + pitch * 0.65) * Quat::from_rotation_z(if two { 0.45 } else { 0.1 }),
                    Limb::ArmR => Quat::from_rotation_x(FRAC_PI_2 + pitch * 0.65),
                    Limb::ForearmL | Limb::ForearmR => Quat::from_rotation_x(0.4),
                    Limb::Spine => Quat::from_rotation_x(-0.15 * tuck),
                    Limb::Body => Quat::IDENTITY,
                }
            } else {
                match limb {
                    Limb::LegL => leg(1.0),
                    Limb::LegR => leg(-1.0),
                    Limb::ShinL => knee(c),
                    Limb::ShinR => knee(-c),
                    // hips turn with the stride
                    Limb::Body => Quat::from_rotation_y(s * 0.07 * moving * fwd_sign),
                    Limb::Spine => {
                        let lean_fwd = if aim { 0.03 } else { 0.03 + 0.17 * run * moving };
                        let twist = if aim || punch > 0.0 { 0.0 } else { -s * 0.1 * moving * fwd_sign };
                        let swing_twist = if punch > 0.0 && two { 0.7 - 1.6 * (1.0 - punch) } else { 0.0 };
                        let aim_pitch = if aim { pitch * 0.35 } else { 0.0 };
                        Quat::from_rotation_y(twist + swing_twist) * Quat::from_rotation_z(lean) * Quat::from_rotation_x(-lean_fwd + aim_pitch + breath * 0.012 + hit * 0.3 - land * 0.25)
                    }
                    Limb::ArmR if punch > 0.0 && two => Quat::from_rotation_x(1.35) * Quat::from_rotation_z(-0.9 + 1.6 * (1.0 - punch)),
                    Limb::ArmL if punch > 0.0 && two => Quat::from_rotation_x(1.25) * Quat::from_rotation_z(0.2 + 1.2 * (1.0 - punch)),
                    Limb::ArmR if punch > 0.0 => Quat::from_rotation_x(1.55 * punch.min(1.0)) * Quat::from_rotation_z(-0.15),
                    Limb::ArmR if aim => Quat::from_rotation_x(FRAC_PI_2 + pitch * 0.65) * Quat::from_rotation_z(if two { -0.05 } else { 0.0 }),
                    Limb::ArmL if aim && two => Quat::from_rotation_x(FRAC_PI_2 + pitch * 0.65 - 0.08) * Quat::from_rotation_z(0.5),
                    Limb::ArmL if aim => Quat::from_rotation_x(0.25 + 0.4 * moving) * Quat::from_rotation_z(-0.12),
                    Limb::ForearmR if aim => Quat::from_rotation_x(if two { 0.15 } else { 0.0 }),
                    Limb::ForearmL if aim && two => Quat::from_rotation_x(0.45),
                    Limb::ArmL => Quat::from_rotation_x(-s * arm_amp + breath * 0.02) * Quat::from_rotation_z(-0.1 - 0.06 * run - hit * 0.4),
                    Limb::ArmR => Quat::from_rotation_x(s * arm_amp + breath * 0.02) * Quat::from_rotation_z(0.1 + 0.06 * run + hit * 0.4),
                    Limb::ForearmL | Limb::ForearmR => Quat::from_rotation_x(elbow),
                }
            };
            t.rotation = t.rotation.slerp(rot, k);
            if *limb == Limb::Body {
                // two bounces per cycle: high at mid-stance, low at heel strike
                let bob = if rag {
                    0.15
                } else if air || seated {
                    0.0
                } else {
                    moving * (0.018 + 0.035 * run) * (2.0 * ph).cos() - 0.035 * run * moving - land * 0.16
                };
                t.translation.y += (bob - t.translation.y) * k;
            }
        }
    }
}
