//! Aircraft: a prop plane and a jet with a lift / drag / thrust flight model
//! (stall included) on raycast landing gear, and a helicopter. Plus the F1
//! vehicle spawn menu.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::input::GameInput;
use crate::meshgen::{loft, MeshBuilder, WHITE};
use crate::player::Player;
use crate::seats::{Seat, SeatKind};
use crate::vehicle::{spawn_car, CarKind, Driver, Explosion, VehicleAssets, GROUP_VEHICLE, GROUP_WORLD};
use crate::world::*;

pub struct AircraftPlugin;

impl Plugin for AircraftPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (setup_aircraft_assets, spawn_airport_fleet.after(crate::city::setup_city)).chain())
            .add_systems(PostUpdate, aircraft_physics.before(PhysicsSet::SyncBackend))
            .add_systems(Update, (aircraft_controls, spin_rotors, aircraft_damage, spawn_menu));
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AirKind {
    Prop,
    Jet,
    Heli,
}

pub struct AirSpec {
    pub name: &'static str,
    pub mass: f32,
    pub wing_area: f32,
    pub thrust: f32,
    pub cd0: f32,
    pub cl0: f32,
    pub cla: f32,
    pub stall: f32,
    /// target roll / pitch / yaw rates at full authority (rad/s)
    pub rates: Vec3,
    pub gear: [Vec3; 3],
    pub gear_len: f32,
    pub half: Vec3,
}

pub fn air_spec(k: AirKind) -> AirSpec {
    match k {
        AirKind::Prop => AirSpec {
            name: "Skylark 172",
            mass: 1100.0,
            wing_area: 16.2,
            thrust: 5600.0,
            cd0: 0.034,
            cl0: 0.5,
            cla: 5.0,
            stall: 0.28,
            rates: Vec3::new(1.9, 1.1, 0.6),
            gear: [Vec3::new(0.0, -0.45, -2.9), Vec3::new(-1.2, -0.45, 0.1), Vec3::new(1.2, -0.45, 0.1)],
            gear_len: 0.75,
            half: Vec3::new(5.6, 1.4, 4.3),
        },
        AirKind::Jet => AirSpec {
            name: "Falcon 9X",
            mass: 8200.0,
            wing_area: 36.0,
            thrust: 68000.0,
            cd0: 0.021,
            cl0: 0.2,
            cla: 4.6,
            stall: 0.3,
            rates: Vec3::new(2.4, 1.0, 0.4),
            gear: [Vec3::new(0.0, -0.8, -5.0), Vec3::new(-1.8, -0.8, 1.2), Vec3::new(1.8, -0.8, 1.2)],
            gear_len: 1.2,
            half: Vec3::new(6.2, 2.0, 7.8),
        },
        AirKind::Heli => AirSpec {
            name: "Hornet H3",
            mass: 1500.0,
            wing_area: 0.0,
            thrust: 0.0,
            cd0: 0.0,
            cl0: 0.0,
            cla: 0.0,
            stall: 0.0,
            rates: Vec3::new(1.6, 1.3, 1.5),
            gear: [Vec3::ZERO; 3],
            gear_len: 0.0,
            half: Vec3::new(1.3, 1.8, 5.5),
        },
    }
}

#[derive(Component)]
pub struct Aircraft {
    pub kind: AirKind,
    pub throttle: f32,
    pub health: f32,
    pub prev_vel: Vec3,
    pub age: f32,
    pub rotor: Option<Entity>,
    pub rotor_speed: f32,
    pub gear_comp: [f32; 3],
    pub dead: bool,
    pub input: AirInput,
}

#[derive(Default, Clone, Copy)]
pub struct AirInput {
    pub pitch: f32,
    pub roll: f32,
    pub yaw: f32,
    pub throttle_axis: f32,
    pub brake: bool,
}

#[derive(Resource)]
pub struct AircraftAssets {
    pub parts: Vec<(AirKind, Vec<(Handle<Mesh>, u8)>, Handle<Mesh>)>,
    pub mats: [Handle<StandardMaterial>; 5], // paint, glass, dark, metal, blade
    pub wheel: Handle<Mesh>,
}

const PAINT: u8 = 0;
const GLASS: u8 = 1;
const DARK: u8 = 2;
const METAL: u8 = 3;

/// Lofted fuselage from (z, half width, half height, centre y) stations.
fn fuselage(mb: &mut MeshBuilder, stations: &[(f32, f32, f32, f32)]) {
    let rings: Vec<Vec<Vec3>> = stations
        .iter()
        .map(|&(z, rx, ry, cy)| (0..20).map(|k| {
            let a = k as f32 / 20.0 * TAU;
            Vec3::new(a.cos() * rx, cy + a.sin() * ry, z)
        }).collect())
        .collect();
    let mut out = [std::mem::take(mb)];
    loft(&rings, &mut out, &|_, _| 0);
    let [m] = out;
    *mb = m;
    mb.polygon(&rings[0], Vec3::NEG_Z, WHITE);
    mb.polygon(rings.last().unwrap(), Vec3::Z, WHITE);
}

fn wing(mb: &mut MeshBuilder, root: Vec3, span: f32, chord_root: f32, chord_tip: f32, sweep: f32, thick: f32, side: f32) {
    // trapezoid in the plane, extruded vertically
    let tip = root + Vec3::new(side * span, 0.0, sweep);
    let pts = [root + Vec3::new(0.0, 0.0, -chord_root * 0.5), tip + Vec3::new(0.0, 0.0, -chord_tip * 0.5), tip + Vec3::new(0.0, 0.0, chord_tip * 0.5), root + Vec3::new(0.0, 0.0, chord_root * 0.5)];
    let top: Vec<Vec3> = pts.iter().map(|p| *p + Vec3::Y * thick * 0.5).collect();
    let bot: Vec<Vec3> = pts.iter().map(|p| *p - Vec3::Y * thick * 0.5).collect();
    mb.polygon(&top, Vec3::Y, WHITE);
    mb.polygon(&bot, Vec3::NEG_Y, WHITE);
    let c = pts.iter().copied().sum::<Vec3>() / 4.0;
    for i in 0..4 {
        let j = (i + 1) % 4;
        let q = [bot[i], bot[j], top[j], top[i]];
        let mid = (q[0] + q[1]) * 0.5;
        mb.quad(q, (mid - c).with_y(0.0).normalize_or(Vec3::X), [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y], WHITE);
    }
}

fn fin(mb: &mut MeshBuilder, prof: &[Vec2], thick: f32) {
    mb.extrude_x(Affine3A::IDENTITY, prof, -thick * 0.5, thick * 0.5, WHITE, 1.0);
}

fn build_model(k: AirKind) -> (Vec<(MeshBuilder, u8)>, MeshBuilder) {
    let mut paint = MeshBuilder::new();
    let mut glass = MeshBuilder::new();
    let mut dark = MeshBuilder::new();
    let mut metal = MeshBuilder::new();
    let mut rotor = MeshBuilder::new();
    match k {
        AirKind::Prop => {
            fuselage(&mut paint, &[(-4.2, 0.22, 0.22, 0.05), (-3.9, 0.5, 0.5, 0.05), (-2.6, 0.6, 0.66, 0.12), (-1.0, 0.62, 0.72, 0.15), (0.6, 0.52, 0.6, 0.18), (2.2, 0.32, 0.38, 0.3), (3.7, 0.16, 0.22, 0.42), (4.3, 0.06, 0.1, 0.46)]);
            glass.ellipsoid(Vec3::new(0.0, 0.55, -1.4), Vec3::new(0.58, 0.42, 1.05), 16, 8, WHITE);
            for s in [-1.0, 1.0] {
                wing(&mut paint, Vec3::new(s * 0.4, 0.95, -1.0), 5.2, 1.5, 1.1, 0.1, 0.14, s);
                wing(&mut paint, Vec3::new(s * 0.1, 0.42, 3.9), 1.8, 0.9, 0.6, 0.1, 0.08, s);
                // wing strut
                let a = Vec3::new(s * 0.55, -0.3, -0.9);
                let b = Vec3::new(s * 2.4, 0.9, -1.0);
                let d = b - a;
                metal.box_at(Affine3A::from_rotation_translation(Quat::from_rotation_arc(Vec3::Y, d.normalize()), (a + b) * 0.5), Vec3::new(0.04, d.length() * 0.5, 0.08), WHITE, 1.0, true, true);
            }
            fin(&mut paint, &[Vec2::new(3.0, 0.45), Vec2::new(4.35, 0.45), Vec2::new(4.35, 1.85), Vec2::new(3.95, 1.85)], 0.08);
            dark.cylinder_at(Affine3A::from_rotation_translation(Quat::from_rotation_x(-FRAC_PI_2), Vec3::new(0.0, 0.05, -4.2)), 0.2, 0.02, 0.35, 12, WHITE, false);
            for b in [0.0, PI] {
                let r = Quat::from_rotation_z(b);
                rotor.box_at(Affine3A::from_rotation_translation(r, r * Vec3::new(0.0, 0.85, 0.0)), Vec3::new(0.08, 0.85, 0.02), WHITE, 1.0, true, true);
            }
            for (i, g) in air_spec(k).gear.iter().enumerate() {
                let top = *g + Vec3::Y * 0.25;
                metal.box_at(Affine3A::from_translation(top - Vec3::Y * 0.25), Vec3::new(0.04, 0.28, 0.04), WHITE, 1.0, true, true);
                let _ = i;
            }
        }
        AirKind::Jet => {
            fuselage(&mut paint, &[(-7.8, 0.15, 0.15, 0.2), (-7.3, 0.6, 0.6, 0.25), (-6.0, 0.95, 1.0, 0.3), (-3.0, 1.05, 1.12, 0.35), (2.0, 1.0, 1.08, 0.35), (5.0, 0.7, 0.8, 0.5), (7.3, 0.25, 0.4, 0.8), (7.8, 0.08, 0.15, 0.9)]);
            glass.ellipsoid(Vec3::new(0.0, 0.85, -6.1), Vec3::new(0.7, 0.4, 0.9), 14, 7, WHITE);
            for s in [-1.0, 1.0] {
                wing(&mut paint, Vec3::new(s * 0.9, -0.1, 0.8), 5.3, 3.4, 1.2, 2.2, 0.2, s);
                wing(&mut paint, Vec3::new(s * 0.2, 2.6, 7.2), 2.2, 1.3, 0.7, 0.9, 0.1, s);
                // rear engines
                let ec = Vec3::new(s * 1.55, 0.9, 4.0);
                metal.cylinder_at(Affine3A::from_rotation_translation(Quat::from_rotation_x(FRAC_PI_2), ec - Vec3::Z * 1.4), 0.55, 0.45, 2.8, 16, WHITE, true);
                dark.cylinder_at(Affine3A::from_rotation_translation(Quat::from_rotation_x(FRAC_PI_2), ec - Vec3::Z * 1.45), 0.47, 0.47, 0.06, 16, WHITE, true);
                metal.box_at(Affine3A::from_translation(Vec3::new(s * 1.1, 0.9, 4.0)), Vec3::new(0.5, 0.1, 0.6), WHITE, 1.0, true, true);
            }
            fin(&mut paint, &[Vec2::new(5.5, 1.0), Vec2::new(7.9, 1.0), Vec2::new(8.2, 2.7), Vec2::new(7.2, 2.7)], 0.16);
            for g in air_spec(k).gear.iter() {
                metal.box_at(Affine3A::from_translation(*g + Vec3::Y * 0.1), Vec3::new(0.06, 0.4, 0.06), WHITE, 1.0, true, true);
            }
        }
        AirKind::Heli => {
            fuselage(&mut paint, &[(-2.4, 0.2, 0.25, 0.9), (-2.1, 0.75, 0.75, 0.85), (-1.0, 0.95, 0.95, 0.85), (0.6, 0.9, 0.95, 0.9), (1.6, 0.55, 0.6, 1.1), (2.2, 0.22, 0.25, 1.3)]);
            glass.ellipsoid(Vec3::new(0.0, 1.05, -1.4), Vec3::new(0.85, 0.7, 0.95), 16, 8, WHITE);
            paint.cylinder_at(Affine3A::from_rotation_translation(Quat::from_rotation_x(FRAC_PI_2), Vec3::new(0.0, 1.3, 2.0)), 0.22, 0.1, 3.6, 10, WHITE, true);
            fin(&mut paint, &[Vec2::new(5.0, 1.2), Vec2::new(5.7, 1.2), Vec2::new(5.8, 2.4), Vec2::new(5.4, 2.4)], 0.08);
            for s in [-1.0, 1.0] {
                metal.box_at(Affine3A::from_translation(Vec3::new(s * 0.85, 0.06, 0.0)), Vec3::new(0.05, 0.05, 1.7), WHITE, 1.0, true, true);
                for z in [-0.8, 0.8] {
                    metal.box_at(Affine3A::from_rotation_translation(Quat::from_rotation_z(s * 0.35), Vec3::new(s * 0.7, 0.35, z)), Vec3::new(0.035, 0.32, 0.035), WHITE, 1.0, true, true);
                }
                // tail rotor
                metal.box_at(Affine3A::from_translation(Vec3::new(0.12, 1.6, 5.6)), Vec3::new(0.02, 0.5, 0.06), WHITE, 1.0, true, true);
            }
            dark.cylinder(Vec3::new(0.0, 1.75, -0.3), 0.18, 0.4, 10, WHITE, true);
            for b in 0..4 {
                let r = Quat::from_rotation_y(b as f32 * FRAC_PI_2);
                rotor.box_at(Affine3A::from_rotation_translation(r, r * Vec3::new(0.0, 0.0, 3.6)), Vec3::new(0.14, 0.025, 3.6), WHITE, 1.0, true, true);
            }
        }
    }
    (vec![(paint, PAINT), (glass, GLASS), (dark, DARK), (metal, METAL)], rotor)
}

fn setup_aircraft_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let mut parts = Vec::new();
    for k in [AirKind::Prop, AirKind::Jet, AirKind::Heli] {
        let (ps, rotor) = build_model(k);
        let ps = ps.into_iter().filter(|(m, _)| !m.is_empty()).map(|(m, i)| (meshes.add(m.build()), i)).collect();
        parts.push((k, ps, meshes.add(rotor.build())));
    }
    let mut wheel = MeshBuilder::new();
    wheel.cylinder_at(Affine3A::from_rotation_translation(Quat::from_rotation_z(-FRAC_PI_2), Vec3::new(-0.08, 0.0, 0.0)), 0.25, 0.25, 0.16, 14, WHITE, true);
    commands.insert_resource(AircraftAssets {
        parts,
        mats: [
            materials.add(StandardMaterial { base_color: Color::srgb(0.92, 0.93, 0.95), metallic: 0.3, perceptual_roughness: 0.35, clearcoat: 0.6, ..default() }),
            materials.add(StandardMaterial { base_color: Color::srgb(0.04, 0.06, 0.09), perceptual_roughness: 0.04, metallic: 0.2, ..default() }),
            materials.add(StandardMaterial { base_color: Color::srgb(0.06, 0.06, 0.07), perceptual_roughness: 0.6, ..default() }),
            materials.add(StandardMaterial { base_color: Color::srgb(0.6, 0.62, 0.65), metallic: 0.9, perceptual_roughness: 0.3, ..default() }),
            materials.add(StandardMaterial { base_color: Color::srgb(0.08, 0.08, 0.08), perceptual_roughness: 0.5, ..default() }),
        ],
        wheel: meshes.add(wheel.build()),
    });
}

pub fn spawn_aircraft(commands: &mut Commands, a: &AircraftAssets, materials: &mut Assets<StandardMaterial>, kind: AirKind, pos: Vec3, yaw: f32, color: Color) -> Entity {
    let s = air_spec(kind);
    let paint = materials.add(StandardMaterial { base_color: color, metallic: 0.3, perceptual_roughness: 0.35, clearcoat: 0.6, ..default() });
    let (collider, inertia) = match kind {
        AirKind::Prop => (
            Collider::compound(vec![
                (Vec3::new(0.0, 0.15, 0.0), Quat::IDENTITY, Collider::cuboid(0.6, 0.65, 4.0)),
                (Vec3::new(0.0, 0.95, -1.0), Quat::IDENTITY, Collider::cuboid(5.5, 0.08, 0.7)),
            ]),
            Vec3::new(1800.0, 3000.0, 1400.0),
        ),
        AirKind::Jet => (
            Collider::compound(vec![
                (Vec3::new(0.0, 0.35, 0.0), Quat::IDENTITY, Collider::cuboid(1.0, 1.05, 7.6)),
                (Vec3::new(0.0, -0.1, 1.6), Quat::IDENTITY, Collider::cuboid(6.0, 0.12, 1.6)),
            ]),
            Vec3::new(30000.0, 45000.0, 20000.0),
        ),
        AirKind::Heli => (
            Collider::compound(vec![
                (Vec3::new(0.0, 0.95, -0.4), Quat::IDENTITY, Collider::cuboid(0.9, 0.75, 1.9)),
                (Vec3::new(0.0, 1.3, 3.5), Quat::IDENTITY, Collider::cuboid(0.2, 0.25, 2.0)),
                (Vec3::new(-0.85, 0.06, 0.0), Quat::IDENTITY, Collider::cuboid(0.06, 0.06, 1.7)),
                (Vec3::new(0.85, 0.06, 0.0), Quat::IDENTITY, Collider::cuboid(0.06, 0.06, 1.7)),
            ]),
            Vec3::new(2200.0, 2600.0, 1200.0),
        ),
    };
    let e = commands
        .spawn((
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
            RigidBody::Dynamic,
            collider,
            ColliderMassProperties::MassProperties(MassProperties { local_center_of_mass: Vec3::new(0.0, 0.3, -0.3), mass: s.mass, principal_inertia_local_frame: Quat::IDENTITY, principal_inertia: inertia }),
            Velocity::default(),
            ExternalForce::default(),
            ExternalImpulse::default(),
            ReadMassProperties::default(),
            Damping { linear_damping: 0.0, angular_damping: 0.3 },
            Ccd::enabled(),
            CollisionGroups::new(GROUP_VEHICLE, GROUP_WORLD | GROUP_VEHICLE),
            Friction::coefficient(0.6),
        ))
        .id();
    let (_, parts, rotor_mesh) = a.parts.iter().find(|(k, _, _)| *k == kind).expect("aircraft model");
    for (mesh, slot) in parts {
        let mat = if *slot == PAINT { paint.clone() } else { a.mats[*slot as usize].clone() };
        let c = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat), Transform::IDENTITY)).id();
        commands.entity(e).add_child(c);
    }
    let rotor_pos = match kind {
        AirKind::Prop => Vec3::new(0.0, 0.05, -4.45),
        AirKind::Jet => Vec3::new(0.0, -50.0, 0.0),
        AirKind::Heli => Vec3::new(0.0, 1.95, -0.3),
    };
    let rotor = commands.spawn((Mesh3d(rotor_mesh.clone()), MeshMaterial3d(a.mats[4].clone()), Transform::from_translation(rotor_pos))).id();
    commands.entity(e).add_child(rotor);
    if kind != AirKind::Heli {
        for g in s.gear {
            let w = commands.spawn((Mesh3d(a.wheel.clone()), MeshMaterial3d(a.mats[2].clone()), Transform::from_translation(g - Vec3::Y * (s.gear_len - 0.3)))).id();
            commands.entity(e).add_child(w);
        }
    }
    let (cam_dist, cam_height) = match kind {
        AirKind::Prop => (13.0, 2.6),
        AirKind::Jet => (22.0, 4.0),
        AirKind::Heli => (13.0, 3.2),
    };
    commands.entity(e).insert((
        Seat { name: s.name, kind: if kind == AirKind::Heli { SeatKind::Heli } else { SeatKind::Plane }, half: s.half, driver: Driver::None, cam_dist, cam_height, broken: false },
        Aircraft { kind, throttle: 0.0, health: 1000.0, prev_vel: Vec3::ZERO, age: 0.0, rotor: Some(rotor), rotor_speed: 0.0, gear_comp: [0.0; 3], dead: false, input: AirInput::default() },
    ));
    e
}

fn spawn_airport_fleet(mut commands: Commands, a: Res<AircraftAssets>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let east = -FRAC_PI_2;
    spawn_aircraft(&mut commands, &a, &mut materials, AirKind::Prop, Vec3::new(RUNWAY.min.x + 40.0, 1.0, RUNWAY.center().y), east, Color::srgb(0.9, 0.9, 0.92));
    spawn_aircraft(&mut commands, &a, &mut materials, AirKind::Jet, Vec3::new(-1000.0, 1.6, -120.0), east, Color::srgb(0.95, 0.95, 0.97));
    spawn_aircraft(&mut commands, &a, &mut materials, AirKind::Prop, Vec3::new(-1080.0, 1.0, -60.0), east, Color::srgb(0.75, 0.12, 0.1));
    spawn_aircraft(&mut commands, &a, &mut materials, AirKind::Heli, Vec3::new(-1180.0, 0.3, -120.0), 0.0, Color::srgb(0.1, 0.25, 0.55));
    // helicopter on the hospital roof pad
    spawn_aircraft(&mut commands, &a, &mut materials, AirKind::Heli, Vec3::new(block_min(1) + BLOCK * 0.5 + 10.0, CURB + 24.35, block_min(6) + 34.0), 0.0, Color::srgb(0.85, 0.1, 0.1));
}

/// Player input -> aircraft controls. W/S pitch (W = nose down), A/D roll,
/// Q/E yaw, Shift/Ctrl throttle (planes) or collective (helicopter), Space brakes.
fn aircraft_controls(time: Res<Time>, input: Res<GameInput>, player: Query<&Player>, mut air: Query<(&Seat, &mut Aircraft)>) {
    let dt = time.delta_secs();
    let pv = player.single().ok().and_then(|p| p.vehicle);
    for (seat, mut a) in &mut air {
        if seat.driver == Driver::Player && pv.is_some() && !a.dead {
            a.input = AirInput { pitch: -input.move_axis.y, roll: input.move_axis.x, yaw: input.yaw, throttle_axis: input.throttle_axis, brake: input.handbrake };
            if a.kind != AirKind::Heli {
                a.throttle = (a.throttle + input.throttle_axis * dt * 0.6).clamp(0.0, 1.0);
            } else {
                a.throttle = 1.0;
            }
        } else {
            a.input = AirInput { brake: true, ..default() };
            if seat.driver != Driver::Player {
                a.throttle = (a.throttle - dt * 0.5).max(0.0);
            }
        }
    }
}

#[allow(clippy::type_complexity)]
fn aircraft_physics(
    time: Res<Time>,
    rapier: ReadRapierContext,
    mut air: Query<(Entity, &Transform, &Velocity, &ReadMassProperties, &mut ExternalForce, &mut Aircraft)>,
) {
    let dt = time.delta_secs().clamp(1e-4, 1.0 / 30.0);
    let Ok(ctx) = rapier.single() else { return };
    for (e, t, vel, mp, mut ef, mut a) in &mut air {
        a.age += dt;
        let s = air_spec(a.kind);
        let m = mp.get();
        let mass = if m.mass > 1.0 { m.mass } else { s.mass };
        let com = t.transform_point(m.local_center_of_mass);
        let f = t.rotation * Vec3::NEG_Z;
        let up = t.rotation * Vec3::Y;
        let right = t.rotation * Vec3::X;
        let v = vel.linear;
        let speed = v.length();
        let q = 0.5 * 1.225 * speed * speed;
        let w_local = t.rotation.inverse() * vel.angular;
        let inp = if a.dead { AirInput::default() } else { a.input };
        let mut force = Vec3::ZERO;
        let mut torque_local = Vec3::ZERO;
        let inertia = m.principal_inertia.max(Vec3::splat(100.0));
        if a.kind == AirKind::Heli {
            // collective around hover; tilt the rotor disc with the cyclic
            let running = a.throttle > 0.5 && !a.dead;
            let collective = if running { 1.0 + 0.65 * inp.throttle_axis } else { 0.0 };
            let lift = mass * 9.81 * collective * up.y.max(0.2).powf(0.3);
            force += up * lift;
            force -= v * mass * 0.25 + Vec3::Y * v.y * mass * 0.4;
            let want = Vec3::new(inp.pitch * s.rates.y, -inp.yaw * s.rates.z, -inp.roll * s.rates.x);
            torque_local += (want - w_local) * inertia * 3.0;
            if running && inp.pitch.abs() < 0.05 && inp.roll.abs() < 0.05 {
                // auto-level when the stick is centred
                let lvl = up.cross(Vec3::Y);
                torque_local += t.rotation.inverse() * lvl * inertia.x * 4.0;
            }
            a.rotor_speed = a.rotor_speed + ((if running { 38.0 } else { 0.0 }) - a.rotor_speed) * (dt * 0.8).min(1.0);
        } else {
            // thrust, lift with stall, induced + parasitic drag
            force += f * a.throttle * s.thrust;
            if speed > 1.0 {
                let vd = v / speed;
                let aoa = (-v.dot(up)).atan2(v.dot(f).max(0.1));
                let cl = if aoa.abs() < s.stall {
                    s.cl0 + s.cla * aoa
                } else {
                    // stalled: lift collapses
                    (s.cl0 + s.cla * s.stall * aoa.signum()) * (1.0 - ((aoa.abs() - s.stall) * 4.0).min(0.75))
                };
                let lift_dir = (up - vd * up.dot(vd)).normalize_or(up);
                force += lift_dir * q * s.wing_area * cl;
                let cd = s.cd0 + 0.045 * cl * cl;
                force -= vd * q * s.wing_area * cd;
                // weathervane: nose follows the airflow
                let stab = f.cross(vd) * q * s.wing_area * 0.35;
                torque_local += t.rotation.inverse() * stab;
                if aoa.abs() > s.stall {
                    torque_local.x -= q * s.wing_area * 0.08; // nose drops in a stall
                }
            }
            let authority = (q / (0.5 * 1.225 * 28.0 * 28.0)).min(1.2) + 0.08;
            // +X rotation lifts a -Z-facing nose: pitch > 0 (S) = nose up, W = nose down
            let want = Vec3::new(inp.pitch * s.rates.y, -inp.yaw * s.rates.z, -inp.roll * s.rates.x) * authority.min(1.0);
            torque_local += (want - w_local) * inertia * 2.5 * authority.min(1.0);
            // landing gear: spring-damper rays with brakes and rolling friction
            let filter = QueryFilter::default().exclude_rigid_body(e).exclude_sensors();
            for (i, g) in s.gear.iter().enumerate() {
                let mount = t.transform_point(*g + Vec3::Y * 0.3);
                let len = s.gear_len + 0.3;
                if let Some((_, toi)) = ctx.cast_ray(mount, -up, len, true, filter) {
                    let comp = len - toi;
                    let k = mass * 9.81 / 3.0 / 0.12;
                    let cvel = (comp - a.gear_comp[i]) / dt;
                    a.gear_comp[i] = comp;
                    let n = (k * comp + 2.0 * 0.4 * (k * mass / 3.0).sqrt() * cvel).max(0.0);
                    let p = mount - up * toi;
                    let vp = v + vel.angular.cross(p - com);
                    let steer = if i == 0 { -(inp.yaw + inp.roll) * 0.4 } else { 0.0 };
                    let wf = Quat::from_axis_angle(up, steer) * f;
                    let wr = wf.cross(up);
                    let mut fx = -vp.dot(wf) * if inp.brake && a.throttle < 0.1 { mass * 0.35 } else { mass * 0.01 };
                    fx = fx.clamp(-n * 0.8, n * 0.8);
                    let fy = (-vp.dot(wr) * mass * 0.9).clamp(-n * 0.9, n * 0.9);
                    let fw = up * n + wf * fx + wr * fy;
                    force += fw;
                    torque_local += t.rotation.inverse() * (p - com).cross(fw);
                } else {
                    a.gear_comp[i] = 0.0;
                }
            }
            a.rotor_speed = a.rotor_speed + (a.throttle * 60.0 - a.rotor_speed) * (dt * 2.0).min(1.0);
        }
        let _ = right;
        ef.force = force;
        ef.torque = t.rotation * torque_local;
    }
}

fn spin_rotors(time: Res<Time>, air: Query<&Aircraft>, mut rotors: Query<&mut Transform>) {
    for a in &air {
        let Some(r) = a.rotor else { continue };
        if let Ok(mut t) = rotors.get_mut(r) {
            let axis = if a.kind == AirKind::Heli { Vec3::Y } else { Vec3::Z };
            t.rotate_local_axis(Dir3::new(axis).unwrap(), a.rotor_speed * time.delta_secs());
        }
    }
}

fn aircraft_damage(
    time: Res<Time>,
    mut air: Query<(&Transform, &Velocity, &mut Aircraft, &mut Seat)>,
    mut ex: MessageWriter<Explosion>,
    mut player: Query<&mut Player>,
) {
    let dt = time.delta_secs().max(1e-4);
    for (t, vel, mut a, mut seat) in &mut air {
        let dv = (vel.linear - a.prev_vel).length();
        a.prev_vel = vel.linear;
        if a.dead || a.age < 2.0 {
            continue;
        }
        let threshold = 9.0 + 9.81 * dt * 2.0;
        if dv > threshold {
            a.health -= (dv - threshold) * 70.0;
        }
        if t.translation.y < SEA_LEVEL - 1.5 {
            a.health = 0.0;
        }
        if a.health <= 0.0 {
            a.dead = true;
            seat.broken = true;
            ex.write(Explosion { pos: t.translation, radius: 14.0, force: 2.0, damage: 900.0, by_player: seat.driver == Driver::Player });
            if seat.driver == Driver::Player {
                if let Ok(mut p) = player.single_mut() {
                    p.health = 0.0;
                }
            }
        }
    }
}

#[derive(Component)]
struct SpawnMenuUi;

const SPAWNABLES: [&str; 9] = ["Solace Avera (sedan)", "Velocita GT (sports)", "Ridgeback (SUV)", "Hauler (van)", "Metro Cab (taxi)", "Police cruiser", "Skylark 172 (prop plane)", "Falcon 9X (jet)", "Hornet H3 (helicopter)"];

/// F1: list of vehicles; number keys 1-9 spawn one in front of the player.
#[allow(clippy::too_many_arguments)]
fn spawn_menu(
    mut commands: Commands,
    mut menus: ResMut<crate::map::Menus>,
    keys: Res<ButtonInput<KeyCode>>,
    player: Query<(&Player, &Transform)>,
    va: Res<VehicleAssets>,
    aa: Res<AircraftAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut ui: Query<(Entity, &mut Visibility), With<SpawnMenuUi>>,
    mut hud: ResMut<crate::hud::HudMessages>,
    mut meshes: ResMut<Assets<Mesh>>,
    spawn_req: Option<ResMut<SpawnRequest>>,
) {
    if ui.is_empty() {
        let mut text = String::from("SPAWN VEHICLE\n\n");
        for (i, n) in SPAWNABLES.iter().enumerate() {
            text.push_str(&format!("  {}   {}\n", i + 1, n));
        }
        text.push_str("\nPress a number   -   F1 / Esc to close");
        commands.spawn((
            Text::new(text),
            TextColor(Color::WHITE),
            Node { position_type: PositionType::Absolute, left: px(60), top: px(120), padding: UiRect::all(px(20)), border_radius: BorderRadius::all(px(10)), ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.75)),
            GlobalZIndex(11),
            Visibility::Hidden,
            SpawnMenuUi,
        ));
        return;
    }
    if let Ok((_, mut v)) = ui.single_mut() {
        *v = if menus.spawn { Visibility::Inherited } else { Visibility::Hidden };
    }
    let mut choice = if menus.spawn {
        [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9].iter().position(|k| keys.just_pressed(*k))
    } else {
        None
    };
    if let Some(mut r) = spawn_req {
        if let Some(c) = r.0.take() {
            choice = Some(c);
        }
    }
    let Some(c) = choice else { return };
    let Ok((p, pt)) = player.single() else { return };
    if p.vehicle.is_some() {
        hud.push("Get out first");
        return;
    }
    let fwd = (pt.rotation * Vec3::NEG_Z).with_y(0.0).normalize_or(Vec3::NEG_Z);
    let dist = if c >= 6 { 16.0 } else { 6.0 };
    let pos = pt.translation + fwd * dist + Vec3::Y * 1.0;
    let yaw = (-fwd.x).atan2(-fwd.z) + FRAC_PI_2;
    let color = Color::srgb(0.12, 0.3, 0.7);
    match c {
        0 => drop(spawn_car(&mut commands, &va, &mut materials, CarKind::Sedan, pos, yaw, color, Driver::None)),
        1 => drop(spawn_car(&mut commands, &va, &mut materials, CarKind::Sports, pos, yaw, Color::srgb(0.85, 0.08, 0.05), Driver::None)),
        2 => drop(spawn_car(&mut commands, &va, &mut materials, CarKind::Suv, pos, yaw, Color::srgb(0.05, 0.05, 0.06), Driver::None)),
        3 => drop(spawn_car(&mut commands, &va, &mut materials, CarKind::Van, pos, yaw, Color::srgb(0.9, 0.9, 0.9), Driver::None)),
        4 => drop(spawn_car(&mut commands, &va, &mut materials, CarKind::Taxi, pos, yaw, Color::srgb(0.95, 0.75, 0.05), Driver::None)),
        5 => {
            let mut bar = MeshBuilder::new();
            bar.cuboid(Vec3::ZERO, Vec3::new(0.28, 0.06, 0.12), WHITE, 1.0);
            let m = meshes.add(bar.build());
            let mat = materials.add(StandardMaterial { base_color: Color::srgb(0.15, 0.15, 0.18), ..default() });
            let car = crate::wanted::spawn_police_car(&mut commands, &va, (&m, &mat), &mut materials, pos, yaw, 1);
            commands.entity(car).insert(Seat { name: "Police cruiser", kind: SeatKind::Car, half: Vec3::new(0.93, 1.06, 2.3), driver: Driver::None, cam_dist: 8.0, cam_height: 2.0, broken: false });
        }
        6 => drop(spawn_aircraft(&mut commands, &aa, &mut materials, AirKind::Prop, pos, yaw, Color::srgb(0.9, 0.9, 0.92))),
        7 => drop(spawn_aircraft(&mut commands, &aa, &mut materials, AirKind::Jet, pos + Vec3::Y, yaw, Color::srgb(0.95, 0.95, 0.97))),
        _ => drop(spawn_aircraft(&mut commands, &aa, &mut materials, AirKind::Heli, pos - Vec3::Y * 0.6, yaw, Color::srgb(0.1, 0.25, 0.55))),
    }
    hud.push(&format!("Spawned {}", SPAWNABLES[c]));
    menus.spawn = false;
}

/// Lets tests request a spawn without the menu.
#[derive(Resource, Default)]
pub struct SpawnRequest(pub Option<usize>);
