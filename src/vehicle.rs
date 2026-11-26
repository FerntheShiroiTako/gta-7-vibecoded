//! Cars: procedural models, raycast-suspension physics, damage, fire and
//! explosions, entering/exiting (with carjacking) and run-over hits.

use std::f32::consts::FRAC_PI_2;

use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::camera::CameraShake;
use crate::city::CityInfo;
use crate::fx::{spawn_explosion_fx, Emitter, EmitterKind};
use crate::humanoid::{HumanoidAssets, Look};
use crate::input::GameInput;
use crate::meshgen::{MeshBuilder, Rng, WHITE};
use crate::peds::{spawn_ped, Ped, PedState};
use crate::player::Player;
use crate::world::*;

pub struct VehiclePlugin;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct VehicleControlSet;

impl Plugin for VehiclePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Explosion>()
            .init_resource::<Showcase>()
            .add_systems(Startup, (setup_vehicle_assets, spawn_initial_cars.after(crate::city::setup_city)).chain())
            .add_systems(PreUpdate, (drive_player_vehicle, npc_idle_control).in_set(VehicleControlSet).after(crate::input::GameInputSet))
            .add_systems(PostUpdate, vehicle_physics.before(PhysicsSet::SyncBackend))
            .add_systems(
                PostUpdate,
                update_wheel_visuals
                    .after(PhysicsSet::Writeback)
                    .before(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(Update, (vehicle_damage, handle_explosions, run_over, brake_lights).chain());
    }
}

pub const GROUP_WORLD: Group = Group::GROUP_1;
pub const GROUP_VEHICLE: Group = Group::GROUP_2;
pub const GROUP_PED: Group = Group::GROUP_3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CarKind {
    Sedan,
    Sports,
    Suv,
    Van,
    Taxi,
}

/// Side-profile description (z: negative = front, y relative to axle height).
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub front: f32,
    pub rear: f32,
    pub bottom: f32,
    pub nose_y: f32,
    pub light_y: f32,
    pub hood_front: (f32, f32),
    pub cowl: (f32, f32),
    pub roof_front: (f32, f32),
    pub roof_rear: (f32, f32),
    pub rear_base: (f32, f32),
    pub deck_end: (f32, f32),
    pub tail_y: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct CarSpec {
    pub kind: CarKind,
    pub shape: Shape,
    pub len: f32,
    pub width: f32,
    pub roof: f32,
    pub wheelbase: f32,
    pub track: f32,
    pub wheel_r: f32,
    pub wheel_w: f32,
    pub mass: f32,
    pub engine: f32,
    pub top_speed: f32,
    pub grip: f32,
    pub awd: bool,
    pub susp_rest: f32,
    pub susp_k: f32,
    pub susp_c: f32,
    pub mount_y: f32,
    pub name: &'static str,
}

pub fn spec(kind: CarKind) -> CarSpec {
    let sedan_shape = Shape {
        front: -2.3,
        rear: 2.3,
        bottom: -0.14,
        nose_y: 0.12,
        light_y: 0.36,
        hood_front: (-1.85, 0.52),
        cowl: (-0.92, 0.62),
        roof_front: (-0.22, 1.06),
        roof_rear: (0.72, 1.05),
        rear_base: (1.38, 0.66),
        deck_end: (2.15, 0.62),
        tail_y: 0.32,
    };
    let (shape, len, width, wheelbase, track, wheel_r, mass, engine, top, grip, awd, name) = match kind {
        CarKind::Sedan | CarKind::Taxi => (sedan_shape, 4.6, 1.86, 2.75, 1.6, 0.34, 1450.0, 8200.0, 52.0, 1.0, false, if kind == CarKind::Taxi { "Metro Cab" } else { "Solace Avera" }),
        CarKind::Sports => (
            Shape {
                front: -2.2,
                rear: 2.2,
                bottom: -0.12,
                nose_y: 0.04,
                light_y: 0.24,
                hood_front: (-1.65, 0.43),
                cowl: (-0.72, 0.53),
                roof_front: (-0.02, 0.8),
                roof_rear: (0.55, 0.8),
                rear_base: (1.55, 0.5),
                deck_end: (2.1, 0.48),
                tail_y: 0.28,
            },
            4.4,
            1.96,
            2.6,
            1.68,
            0.34,
            1280.0,
            12500.0,
            76.0,
            1.18,
            false,
            "Velocita GT",
        ),
        CarKind::Suv => (
            Shape {
                front: -2.4,
                rear: 2.4,
                bottom: -0.1,
                nose_y: 0.22,
                light_y: 0.62,
                hood_front: (-2.0, 0.8),
                cowl: (-1.1, 0.9),
                roof_front: (-0.42, 1.45),
                roof_rear: (2.22, 1.45),
                rear_base: (2.36, 0.94),
                deck_end: (2.38, 0.92),
                tail_y: 0.4,
            },
            4.8,
            1.98,
            2.85,
            1.68,
            0.4,
            2050.0,
            10500.0,
            50.0,
            0.97,
            true,
            "Ridgeback",
        ),
        CarKind::Van => (
            Shape {
                front: -2.6,
                rear: 2.6,
                bottom: -0.08,
                nose_y: 0.22,
                light_y: 0.72,
                hood_front: (-2.45, 0.86),
                cowl: (-2.05, 1.02),
                roof_front: (-1.45, 1.78),
                roof_rear: (2.55, 1.78),
                rear_base: (2.58, 1.02),
                deck_end: (2.59, 1.0),
                tail_y: 0.4,
            },
            5.2,
            2.02,
            3.2,
            1.72,
            0.36,
            2350.0,
            8000.0,
            42.0,
            0.92,
            false,
            "Hauler",
        ),
    };
    let susp_rest = 0.34;
    // spring so static compression is ~0.1 m
    let susp_k: f32 = mass * 9.81 / 4.0 / 0.1;
    let susp_c = 2.0 * 0.55 * (susp_k * mass / 4.0).sqrt();
    CarSpec {
        kind,
        shape,
        len,
        width,
        roof: shape.roof_front.1.max(shape.roof_rear.1),
        wheelbase,
        track,
        wheel_r,
        wheel_w: 0.25,
        mass,
        engine,
        top_speed: top,
        grip,
        awd,
        susp_rest,
        susp_k,
        susp_c,
        mount_y: 0.24,
        name,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Driver {
    None,
    Player,
    Npc(u32),
    Cop(u32),
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Wheel {
    pub mount: Vec3,
    pub front: bool,
    pub left: bool,
    pub driven: bool,
    pub comp: f32,
    pub ext: f32,
    pub contact: bool,
    pub spin: f32,
    pub slip: f32,
    pub entity: Option<Entity>,
}

#[derive(Component)]
pub struct Vehicle {
    pub spec: CarSpec,
    pub health: f32,
    pub burn_t: Option<f32>,
    pub wrecked: bool,
    pub driver: Driver,
    pub wheels: [Wheel; 4],
    pub steer: f32,
    pub prev_vel: Vec3,
    pub age: f32,
    pub paint: Handle<StandardMaterial>,
    pub tail: Handle<StandardMaterial>,
    pub braking: bool,
    pub upside_t: f32,
    pub rest_t: f32,
    pub smoke: Option<Entity>,
    pub last_hit_by_player: bool,
}

#[derive(Component, Default, Clone, Copy)]
pub struct VehicleControl {
    pub throttle: f32,
    pub brake: f32,
    pub steer: f32,
    pub handbrake: bool,
}

#[derive(Component)]
pub struct WheelVisual {
    pub car: Entity,
    pub idx: usize,
}

#[derive(Message, Clone, Copy)]
pub struct Explosion {
    pub pos: Vec3,
    pub radius: f32,
    pub force: f32,
    pub damage: f32,
    pub by_player: bool,
}

#[derive(Resource, Default)]
pub struct Showcase {
    pub cars: Vec<Entity>,
    pub npc_car: Option<Entity>,
}

#[derive(Resource)]
pub struct VehicleAssets {
    pub glass: Handle<StandardMaterial>,
    pub trim: Handle<StandardMaterial>,
    pub chrome: Handle<StandardMaterial>,
    pub rubber: Handle<StandardMaterial>,
    pub head: Handle<StandardMaterial>,
    pub charred: Handle<StandardMaterial>,
    pub plate: Handle<StandardMaterial>,
    pub meshes: Vec<(CarKind, CarMeshes)>,
}

#[derive(Clone)]
pub struct CarMeshes {
    pub paint: Handle<Mesh>,
    pub glass: Handle<Mesh>,
    pub trim: Handle<Mesh>,
    pub chrome: Handle<Mesh>,
    pub head: Handle<Mesh>,
    pub tail: Handle<Mesh>,
    pub plate: Handle<Mesh>,
    pub tire: Handle<Mesh>,
    pub rim: Handle<Mesh>,
    pub interior: Handle<Mesh>,
}

pub use crate::car_model::build_car;

fn setup_vehicle_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let mut list = Vec::new();
    for kind in [CarKind::Sedan, CarKind::Sports, CarKind::Suv, CarKind::Van, CarKind::Taxi] {
        let s = spec(kind);
        let [paint, glass, trim, chrome, head, tail, plate, tire, rim, interior] = build_car(&s);
        list.push((
            kind,
            CarMeshes {
                paint: meshes.add(paint.build()),
                glass: meshes.add(glass.build()),
                trim: meshes.add(trim.build()),
                chrome: meshes.add(chrome.build()),
                head: meshes.add(head.build()),
                tail: meshes.add(tail.build()),
                plate: meshes.add(plate.build()),
                tire: meshes.add(tire.build()),
                rim: meshes.add(rim.build()),
                interior: meshes.add(interior.build()),
            },
        ));
    }
    commands.insert_resource(VehicleAssets {
        glass: materials.add(StandardMaterial { base_color: Color::srgba(0.03, 0.05, 0.06, 0.55), alpha_mode: AlphaMode::Blend, perceptual_roughness: 0.03, metallic: 0.1, reflectance: 0.7, ..default() }),
        trim: materials.add(StandardMaterial { base_color: Color::srgb(0.035, 0.035, 0.04), perceptual_roughness: 0.6, ..default() }),
        chrome: materials.add(StandardMaterial { base_color: Color::srgb(0.85, 0.86, 0.88), perceptual_roughness: 0.12, metallic: 1.0, ..default() }),
        rubber: materials.add(StandardMaterial { base_color: Color::srgb(0.03, 0.03, 0.03), perceptual_roughness: 0.92, ..default() }),
        head: materials.add(StandardMaterial { base_color: Color::srgb(0.95, 0.95, 0.9), emissive: LinearRgba::rgb(3.0, 2.9, 2.6), ..default() }),
        charred: materials.add(StandardMaterial { base_color: Color::srgb(0.05, 0.045, 0.04), perceptual_roughness: 0.95, ..default() }),
        plate: materials.add(StandardMaterial { base_color: Color::srgb(0.92, 0.92, 0.88), perceptual_roughness: 0.5, ..default() }),
        meshes: list,
    });
}

pub const PAINTS: [(f32, f32, f32); 12] = [
    (0.6, 0.02, 0.02),
    (0.02, 0.1, 0.45),
    (0.85, 0.85, 0.85),
    (0.02, 0.02, 0.02),
    (0.35, 0.36, 0.38),
    (0.75, 0.55, 0.05),
    (0.05, 0.3, 0.12),
    (0.55, 0.25, 0.05),
    (0.25, 0.05, 0.3),
    (0.6, 0.62, 0.65),
    (0.08, 0.35, 0.5),
    (0.9, 0.35, 0.02),
];

pub fn spawn_car(
    commands: &mut Commands,
    assets: &VehicleAssets,
    materials: &mut Assets<StandardMaterial>,
    kind: CarKind,
    pos: Vec3,
    yaw: f32,
    color: Color,
    driver: Driver,
) -> Entity {
    let s = spec(kind);
    let cm = assets.meshes.iter().find(|(k, _)| *k == kind).map(|(_, m)| m.clone()).expect("car meshes");
    let paint = materials.add(StandardMaterial {
        base_color: color,
        metallic: 0.55,
        perceptual_roughness: 0.32,
        clearcoat: 1.0,
        clearcoat_perceptual_roughness: 0.06,
        ..default()
    });
    let tail = materials.add(StandardMaterial { base_color: Color::srgb(0.5, 0.02, 0.02), emissive: LinearRgba::rgb(0.6, 0.02, 0.02), ..default() });
    let sh = s.shape;
    let hw = s.width * 0.5;
    let belt = (sh.cowl.1 + sh.rear_base.1) * 0.5;
    let body = Collider::compound(vec![
        (Vec3::new(0.0, (belt + sh.bottom) * 0.5, (sh.front + sh.rear) * 0.5), Quat::IDENTITY, Collider::cuboid(hw, (belt - sh.bottom) * 0.5, (sh.rear - sh.front) * 0.5)),
        (
            Vec3::new(0.0, (belt + s.roof) * 0.5, (sh.cowl.0 + sh.rear_base.0) * 0.5),
            Quat::IDENTITY,
            Collider::cuboid(hw - 0.1, (s.roof - belt) * 0.5, (sh.rear_base.0 - sh.cowl.0) * 0.5),
        ),
    ]);
    let h = s.roof - sh.bottom;
    let l = s.len;
    let w = s.width;
    let m = s.mass;
    let mass_props = MassProperties {
        local_center_of_mass: Vec3::new(0.0, 0.05, 0.0),
        mass: m,
        principal_inertia_local_frame: Quat::IDENTITY,
        principal_inertia: Vec3::new(m / 12.0 * (h * h + l * l), m / 12.0 * (w * w + l * l), m / 12.0 * (w * w + h * h)),
    };
    let fa = -s.wheelbase * 0.5;
    let ra = s.wheelbase * 0.5;
    let tx = s.track * 0.5;
    let mut wheels = [Wheel::default(); 4];
    for (i, (x, z, front)) in [(-tx, fa, true), (tx, fa, true), (-tx, ra, false), (tx, ra, false)].iter().enumerate() {
        wheels[i] = Wheel {
            mount: Vec3::new(*x, s.mount_y, *z),
            front: *front,
            left: *x < 0.0,
            driven: s.awd || !*front,
            ext: s.susp_rest,
            ..default()
        };
    }
    let car = commands
        .spawn((
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
            RigidBody::Dynamic,
            body,
            ColliderMassProperties::MassProperties(mass_props),
            Velocity::default(),
            ExternalForce::default(),
            ExternalImpulse::default(),
            ReadMassProperties::default(),
            Damping { linear_damping: 0.02, angular_damping: 0.5 },
            Ccd::enabled(),
            CollisionGroups::new(GROUP_VEHICLE, GROUP_WORLD | GROUP_VEHICLE),
            Friction::coefficient(0.4),
            VehicleControl::default(),
        ))
        .id();
    let parts = [
        (cm.paint.clone(), paint.clone()),
        (cm.glass.clone(), assets.glass.clone()),
        (cm.trim.clone(), assets.trim.clone()),
        (cm.chrome.clone(), assets.chrome.clone()),
        (cm.head.clone(), assets.head.clone()),
        (cm.tail.clone(), tail.clone()),
        (cm.plate.clone(), assets.plate.clone()),
    ];
    for (mesh, mat) in parts {
        let see_through = mat == assets.glass;
        let e = commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), Transform::IDENTITY, CarPart)).id();
        if see_through {
            commands.entity(e).insert(crate::lighting::NoRaytrace);
        }
        commands.entity(car).add_child(e);
    }
    // seats, dash and the cabin tub: only ever seen through the glass, no shadow needed
    let inner = commands.spawn((Mesh3d(cm.interior.clone()), MeshMaterial3d(assets.trim.clone()), Transform::IDENTITY, CarPart, bevy::light::NotShadowCaster)).id();
    commands.entity(car).add_child(inner);
    for (i, wh) in wheels.iter_mut().enumerate() {
        let we = commands
            .spawn((
                Transform::from_translation(wh.mount - Vec3::Y * s.susp_rest),
                Visibility::default(),
                WheelVisual { car, idx: i },
            ))
            .id();
        let tire = commands.spawn((Mesh3d(cm.tire.clone()), MeshMaterial3d(assets.rubber.clone()), Transform::IDENTITY, CarPart)).id();
        let rim = commands.spawn((Mesh3d(cm.rim.clone()), MeshMaterial3d(assets.chrome.clone()), Transform::IDENTITY, CarPart)).id();
        commands.entity(we).add_children(&[tire, rim]);
        commands.entity(car).add_child(we);
        wh.entity = Some(we);
    }
    commands.entity(car).insert(crate::seats::Seat {
        name: s.name,
        kind: crate::seats::SeatKind::Car,
        half: Vec3::new(s.width * 0.5, s.roof, s.len * 0.5),
        driver,
        cam_dist: s.len * 1.25 + 2.2,
        cam_height: s.roof + 0.9,
        broken: false,
    });
    commands.entity(car).insert(Vehicle {
        spec: s,
        health: 1000.0,
        burn_t: None,
        wrecked: false,
        driver,
        wheels,
        steer: 0.0,
        prev_vel: Vec3::ZERO,
        age: 0.0,
        paint,
        tail,
        braking: false,
        upside_t: 0.0,
        rest_t: 0.0,
        smoke: None,
        last_hit_by_player: false,
    });
    car
}

#[derive(Component)]
pub struct CarPart;

fn spawn_initial_cars(
    mut commands: Commands,
    assets: Res<VehicleAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    info: Res<CityInfo>,
    mut showcase: ResMut<Showcase>,
) {
    let mut rng = Rng::new(77);
    // showcase cars on the street next to the spawn point (eastbound outer lane)
    let z = road_center(6) + LANE * 1.5;
    let x0 = block_min(4) + 8.0;
    // spaced out, nose-to-tail along an empty lane: sports car in front with a clear road ahead
    for (kind, dx) in [(CarKind::Sports, 30.0), (CarKind::Sedan, 15.0), (CarKind::Suv, 0.0)] {
        let color = match kind {
            CarKind::Sports => Color::srgb(0.75, 0.05, 0.02),
            CarKind::Suv => Color::srgb(0.02, 0.02, 0.025),
            _ => Color::srgb(0.08, 0.25, 0.55),
        };
        let e = spawn_car(&mut commands, &assets, &mut materials, kind, Vec3::new(x0 + dx, spec(kind).wheel_r + 0.12, z), -FRAC_PI_2, color, Driver::None);
        showcase.cars.push(e);
    }
    // an idling cab with a driver behind them (for carjacking)
    let e = spawn_car(&mut commands, &assets, &mut materials, CarKind::Taxi, Vec3::new(x0 - 16.0, 0.46, z), -FRAC_PI_2, Color::srgb(0.95, 0.75, 0.05), Driver::Npc(5));
    showcase.npc_car = Some(e);

    let mut count = 0;
    for &(p, yaw) in info.parking.iter() {
        if count >= 85 {
            break;
        }
        if (p.x - x0).abs() < 120.0 && (p.z - z).abs() < 10.0 {
            continue;
        }
        let kind = *rng.pick(&[CarKind::Sedan, CarKind::Sedan, CarKind::Sedan, CarKind::Suv, CarKind::Suv, CarKind::Van, CarKind::Sports, CarKind::Taxi]);
        let color = if kind == CarKind::Taxi {
            Color::srgb(0.95, 0.75, 0.05)
        } else {
            let c = *rng.pick(&PAINTS);
            Color::srgb(c.0, c.1, c.2)
        };
        let driver = if rng.chance(0.15) { Driver::Npc(rng.next_u32()) } else { Driver::None };
        spawn_car(&mut commands, &assets, &mut materials, kind, Vec3::new(p.x, p.y + spec(kind).wheel_r + 0.12, p.z), yaw, color, driver);
        count += 1;
    }
    // airport + docks parking
    for k in 0..6 {
        let kind = if k % 2 == 0 { CarKind::Van } else { CarKind::Suv };
        spawn_car(&mut commands, &assets, &mut materials, kind, Vec3::new(-1000.0 + k as f32 * 8.0, 0.6, -190.0), 0.0, Color::srgb(0.9, 0.9, 0.9), Driver::None);
        spawn_car(&mut commands, &assets, &mut materials, kind, Vec3::new(505.0, 0.6, -400.0 + k as f32 * 9.0), FRAC_PI_2, Color::srgb(0.2, 0.3, 0.5), Driver::None);
    }
}

fn drive_player_vehicle(input: Res<GameInput>, player: Query<&Player>, mut cars: Query<(&Vehicle, &mut VehicleControl, &Velocity, &Transform)>) {
    let Ok(p) = player.single() else { return };
    let Some(ve) = p.vehicle else { return };
    let Ok((v, mut c, vel, t)) = cars.get_mut(ve) else { return };
    if v.wrecked {
        *c = VehicleControl::default();
        return;
    }
    let fwd = t.rotation * Vec3::NEG_Z;
    let speed_f = vel.linear.dot(fwd);
    c.steer = input.steer;
    c.handbrake = input.handbrake;
    // S brakes while moving forward, then reverses; W does the mirror when reversing.
    if speed_f < -1.0 && input.throttle > 0.0 {
        c.throttle = 0.0;
        c.brake = 1.0;
    } else {
        // ramp the pedal instead of snapping it
        c.throttle += (input.throttle - c.throttle) * 0.25;
        c.brake = input.brake;
    }
}

/// NPC drivers in parked cars just sit with the brake on (traffic AI comes in phase 2).
fn npc_idle_control(mut cars: Query<(&Vehicle, &mut VehicleControl)>) {
    for (v, mut c) in &mut cars {
        if matches!(v.driver, Driver::Npc(_) | Driver::Cop(_)) {
            // parked with someone inside: handbrake (brake at a standstill means reverse)
            *c = VehicleControl { throttle: 0.0, brake: 0.0, steer: 0.0, handbrake: true };
        } else if v.driver == Driver::None {
            *c = VehicleControl { throttle: 0.0, brake: 0.0, steer: 0.0, handbrake: true };
        }
    }
}

#[allow(clippy::type_complexity)]
fn vehicle_physics(
    time: Res<Time>,
    rapier: ReadRapierContext,
    mut cars: Query<(Entity, &Transform, &Velocity, &ReadMassProperties, &mut ExternalForce, &mut Vehicle, &VehicleControl)>,
) {
    let dt = time.delta_secs().clamp(1e-4, 1.0 / 30.0);
    let Ok(ctx) = rapier.single() else { return };
    for (e, t, vel, mp, mut ef, mut v, ctl) in cars.iter_mut() {
        v.age += dt;
        let s = v.spec;
        let speed = vel.linear.length();
        // let parked cars sleep: once settled and unoccupied, stop rewriting forces
        if v.driver == Driver::None && speed < 0.05 && vel.angular.length() < 0.05 && v.age > 3.0 && !v.wrecked {
            v.rest_t += dt;
            if v.rest_t > 1.0 {
                continue;
            }
        } else {
            v.rest_t = 0.0;
        }
        let mprops = mp.get();
        let mass = if mprops.mass > 1.0 { mprops.mass } else { s.mass };
        let com = t.transform_point(mprops.local_center_of_mass);
        let up = t.rotation * Vec3::Y;
        let fwd = t.rotation * Vec3::NEG_Z;
        let speed_f = vel.linear.dot(fwd);

        let control = if v.wrecked { VehicleControl { handbrake: true, ..default() } } else { *ctl };
        let max_steer = 0.62 + (0.15 - 0.62) * (speed / 42.0).clamp(0.0, 1.0);
        let target = control.steer * max_steer;
        let rate = if target.abs() < v.steer.abs() { 7.0 } else { 4.5 };
        v.steer += (target - v.steer) * (dt * rate).min(1.0);

        let filter = QueryFilter::default().exclude_rigid_body(e).exclude_sensors().groups(CollisionGroups::new(GROUP_VEHICLE, GROUP_WORLD | GROUP_VEHICLE));
        let mut force = Vec3::ZERO;
        let mut torque = Vec3::ZERO;
        let len = s.susp_rest + s.wheel_r;
        let n_driven = if s.awd { 4.0 } else { 2.0 };
        let power = (1.0 - (speed_f.max(0.0) / s.top_speed).powi(2)).max(0.0);
        let mut contacts = 0;
        let mut comps = [0.0f32; 4];
        for i in 0..4 {
            let w = v.wheels[i];
            let mount = t.transform_point(w.mount);
            let hit = ctx.cast_ray_and_get_normal(mount, -up, len, true, filter);
            let mut nw = w;
            if let Some((_, hit)) = hit {
                let toi = hit.time_of_impact;
                contacts += 1;
                let comp = (len - toi).max(0.0);
                let comp_vel = (comp - w.comp) / dt;
                nw.comp = comp;
                nw.contact = true;
                nw.ext = toi - s.wheel_r;
                comps[i] = comp;
                let n_force = (s.susp_k * comp + s.susp_c * comp_vel).clamp(0.0, s.mass * 9.81 * 3.0);
                let p = mount - up * toi;
                let vp = vel.linear + vel.angular.cross(p - com);
                let steer = if w.front { v.steer } else { 0.0 };
                let wf = Quat::from_axis_angle(up, -steer) * fwd;
                let wr = wf.cross(up);
                let v_long = vp.dot(wf);
                let v_lat = vp.dot(wr);
                let hand = control.handbrake && !w.front;
                let lat_mu = if hand { 0.42 } else { 1.0 };
                let mu = 1.28 * s.grip;
                let max_f = mu * n_force;
                // longitudinal
                let mut fx = 0.0;
                if w.driven && control.throttle > 0.0 {
                    fx += control.throttle * s.engine / n_driven * power;
                }
                if control.brake > 0.0 {
                    if speed_f > 1.0 {
                        let bf = 1.15 * s.mass * 9.81 * if w.front { 0.32 } else { 0.18 };
                        fx -= v_long.signum() * bf.min(v_long.abs() * mass * 0.25 / dt) * control.brake;
                    } else if w.driven && speed_f > -14.0 {
                        fx -= control.brake * s.engine * 0.45 / n_driven;
                    }
                }
                if hand {
                    let lock = 0.85 * max_f;
                    fx -= v_long.signum() * lock.min(v_long.abs() * mass * 0.25 / dt);
                }
                if control.throttle == 0.0 && control.brake == 0.0 {
                    // engine braking + rolling resistance, and a hold when nearly stopped
                    if v_long.abs() < 1.0 {
                        fx -= (v_long * mass * 0.25 / dt * 0.5).clamp(-max_f, max_f);
                    } else {
                        fx -= v_long * 45.0;
                    }
                } else {
                    fx -= v_long * 12.0;
                }
                // lateral (slip-angle curve, saturating -> drift)
                let mut fy;
                if vp.length() < 2.5 {
                    fy = (-v_lat * mass * 0.25 / dt * 0.5).clamp(-max_f, max_f);
                } else {
                    let slip = v_lat.atan2(v_long.abs().max(0.5));
                    let peak = 0.14;
                    let x = slip / peak;
                    let curve = if x.abs() < 1.0 { x } else { x.signum() * (1.0 - 0.12 * ((x.abs() - 1.0).min(3.0) / 3.0)) };
                    fy = -curve * max_f * lat_mu;
                }
                let total = (fx * fx + fy * fy).sqrt();
                let mut slipping = false;
                if total > max_f && total > 1e-3 {
                    let k = max_f / total;
                    fx *= k;
                    fy *= k;
                    slipping = true;
                }
                nw.slip = if slipping || v_lat.abs() > 3.5 { v_lat.abs().max(4.0) } else { v_lat.abs() * 0.3 };
                let f = up * n_force + wf * fx + wr * fy;
                force += f;
                torque += (p - com).cross(f);
                // wheel spin for visuals (burnouts spin faster than the ground speed)
                let spin_v = if hand { 0.0 } else if w.driven && control.throttle > 0.0 && slipping { v_long + 18.0 } else { v_long };
                nw.spin -= spin_v / s.wheel_r * dt;
            } else {
                nw.comp = 0.0;
                nw.contact = false;
                nw.ext = s.susp_rest;
                nw.slip = 0.0;
                if w.driven && control.throttle > 0.0 {
                    nw.spin -= 30.0 * dt;
                }
            }
            v.wheels[i] = nw;
        }
        // anti-roll bars
        for (l, r) in [(0usize, 1usize), (2, 3)] {
            let d = (comps[l] - comps[r]) * s.susp_k * 0.45;
            for (idx, sign) in [(l, -1.0f32), (r, 1.0)] {
                if v.wheels[idx].contact {
                    let p = t.transform_point(v.wheels[idx].mount);
                    let f = up * d * sign * -1.0;
                    force += f;
                    torque += (p - com).cross(f);
                }
            }
        }
        // aerodynamics
        let drag = 0.42;
        force -= vel.linear * speed * drag;
        if contacts > 0 {
            force -= up * speed * speed * 0.9;
        } else if v.driver == Driver::Player && !v.wrecked {
            // gentle air control for stunts
            let right = t.rotation * Vec3::X;
            torque += right * (control.throttle - control.brake) * s.mass * 0.6 + up * -control.steer * s.mass * 0.4;
        }
        // self-righting when flipped and stopped (player only)
        if up.y < 0.25 && speed < 3.0 {
            v.upside_t += dt;
            if v.driver == Driver::Player && v.upside_t > 2.0 && !v.wrecked {
                let axis = up.cross(Vec3::Y).normalize_or(fwd);
                torque += axis * s.mass * 9.0;
                force += Vec3::Y * s.mass * 6.0;
            }
        } else {
            v.upside_t = 0.0;
        }
        ef.force = force;
        ef.torque = torque;
    }
}

fn update_wheel_visuals(cars: Query<&Vehicle>, mut wheels: Query<(&WheelVisual, &mut Transform)>) {
    for (wv, mut t) in &mut wheels {
        let Ok(v) = cars.get(wv.car) else { continue };
        let w = v.wheels[wv.idx];
        let steer = if w.front { v.steer } else { 0.0 };
        t.translation = w.mount - Vec3::Y * w.ext.clamp(0.0, v.spec.susp_rest);
        t.rotation = Quat::from_rotation_y(-steer) * Quat::from_rotation_x(w.spin);
    }
}

#[allow(clippy::too_many_arguments)]
fn vehicle_damage(
    mut commands: Commands,
    time: Res<Time>,
    mut cars: Query<(Entity, &Transform, &Velocity, &mut Vehicle)>,
    mut player: Query<&mut Player>,
    mut explosions: MessageWriter<Explosion>,
    assets: Res<VehicleAssets>,
    mut parts: Query<(&ChildOf, &mut MeshMaterial3d<StandardMaterial>), With<CarPart>>,
    wheel_parents: Query<&ChildOf, With<WheelVisual>>,
    mut sfx: MessageWriter<crate::audio::Sfx>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs().max(1e-4);
    let mut pl = player.single_mut().ok();
    let mut wrecked_now = Vec::new();
    for (e, t, vel, mut v) in &mut cars {
        let dv = (vel.linear - v.prev_vel).length();
        v.prev_vel = vel.linear;
        if v.age > 1.5 && !v.wrecked {
            // impacts: sudden velocity change beyond what gravity/suspension produce
            let threshold = 4.5 + 9.81 * dt * 2.0;
            if dv > threshold {
                let dmg = (dv - threshold) * 24.0;
                if dv > threshold + 2.0 {
                    sfx.write(crate::audio::Sfx { kind: crate::audio::SfxKind::Crash, pos: t.translation, volume: ((dv - threshold) / 12.0).clamp(0.2, 1.0) });
                }
                v.health -= dmg;
                if v.driver == Driver::Player {
                    if let Some(p) = pl.as_mut() {
                        if dv > 10.0 {
                            p.hurt((dv - 10.0) * 2.2);
                        }
                    }
                    v.last_hit_by_player = true;
                }
            }
            // drowned
            if t.translation.y < SEA_LEVEL - 0.6 {
                v.health = v.health.min(0.0);
                v.burn_t = Some(0.0);
                v.wrecked = true;
                wrecked_now.push(e);
                continue;
            }
            // long upside-down catches fire
            if v.upside_t > 4.0 && v.driver != Driver::Player {
                v.health = v.health.min(0.0);
            }
            if v.health <= 0.0 && v.burn_t.is_none() {
                v.burn_t = Some(5.0);
                let em = commands
                    .spawn((Transform::from_xyz(0.0, v.spec.shape.cowl.1, v.spec.shape.front + 0.6), Emitter { kind: EmitterKind::Fire, rate: 40.0, acc: 0.0 }))
                    .id();
                commands.entity(e).add_child(em);
                if let Some(old) = v.smoke.take() {
                    commands.entity(old).despawn();
                }
                v.smoke = Some(em);
            } else if v.health < 450.0 && v.smoke.is_none() && v.burn_t.is_none() {
                let kind = if v.health < 250.0 { EmitterKind::BlackSmoke } else { EmitterKind::Smoke };
                let em = commands
                    .spawn((Transform::from_xyz(0.0, v.spec.shape.cowl.1, v.spec.shape.front + 0.6), Emitter { kind, rate: 12.0, acc: 0.0 }))
                    .id();
                commands.entity(e).add_child(em);
                v.smoke = Some(em);
            }
            if let Some(bt) = v.burn_t.as_mut() {
                *bt -= dt;
                if *bt <= 0.0 {
                    v.wrecked = true;
                    explosions.write(Explosion { pos: t.translation + Vec3::Y * 0.5, radius: 11.0, force: 2.2, damage: 900.0, by_player: v.last_hit_by_player });
                    wrecked_now.push(e);
                    if let Some(old) = v.smoke.take() {
                        commands.entity(old).despawn();
                    }
                    let em = commands.spawn((Transform::from_xyz(0.0, 0.8, 0.0), Emitter { kind: EmitterKind::BlackSmoke, rate: 10.0, acc: 0.0 })).id();
                    commands.entity(e).add_child(em);
                    v.smoke = Some(em);
                    v.driver = if v.driver == Driver::Player { Driver::Player } else { Driver::None };
                }
            }
        }
    }
    // char the wrecks: swap paint/lights/glass to the burnt material
    if !wrecked_now.is_empty() {
        for (child_of, mut mat) in &mut parts {
            let mut owner = child_of.parent();
            if let Ok(wp) = wheel_parents.get(owner) {
                owner = wp.parent();
            }
            if wrecked_now.contains(&owner) {
                mat.0 = assets.charred.clone();
            }
        }
        for e in &wrecked_now {
            if let Ok((_, _, _, v)) = cars.get(*e) {
                if let Some(mut m) = materials.get_mut(&v.tail) {
                    m.emissive = LinearRgba::BLACK;
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_explosions(
    mut commands: Commands,
    mut msgs: MessageReader<Explosion>,
    mut bodies: Query<(Entity, &Transform, &ReadMassProperties, &mut ExternalImpulse, Option<&mut Vehicle>)>,
    mut player: Query<(&mut Player, &Transform), Without<ReadMassProperties>>,
    mut peds: Query<(&mut Ped, &Transform), (Without<Player>, Without<ReadMassProperties>)>,
    mut shake: ResMut<CameraShake>,
    cam: Query<&Transform, (With<crate::camera::MainCamera>, Without<ReadMassProperties>, Without<Player>, Without<Ped>)>,
    fx: Res<crate::fx::FxAssets>,
) {
    for ex in msgs.read() {
        spawn_explosion_fx(&mut commands, &fx, ex.pos);
        if let Ok(ct) = cam.single() {
            let d = ct.translation.distance(ex.pos);
            shake.trauma = (shake.trauma + (1.0 - d / 120.0).max(0.0)).min(1.0);
        }
        for (_, t, mp, mut imp, veh) in &mut bodies {
            let d = t.translation.distance(ex.pos);
            if d > ex.radius * 1.6 {
                continue;
            }
            let k = (1.0 - d / (ex.radius * 1.6)).max(0.0);
            let mass = mp.get().mass.max(50.0);
            let dir = (t.translation - ex.pos + Vec3::Y * 1.2).normalize_or(Vec3::Y);
            let off = Vec3::new(0.3, 0.0, 0.5) * (1.0 - k);
            imp.impulse += dir * mass * 9.0 * ex.force * k;
            imp.torque_impulse += off.cross(dir) * mass * 2.0 * k;
            if let Some(mut v) = veh {
                if d < ex.radius {
                    v.health -= ex.damage * (1.0 - d / ex.radius);
                    if ex.by_player {
                        v.last_hit_by_player = true;
                    }
                }
            }
        }
        if let Ok((mut p, pt)) = player.single_mut() {
            let d = pt.translation.distance(ex.pos);
            if d < ex.radius && p.vehicle.is_none() {
                let k = 1.0 - d / ex.radius;
                // lethal point-blank, survivable at the edge of the blast
                p.hurt(130.0 * k.powf(1.6));
                p.vel += ((pt.translation - ex.pos).normalize_or(Vec3::Y) + Vec3::Y) * 9.0 * k;
                p.knock_t = 1.2 * k + 0.3;
            }
        }
        for (mut ped, pt) in &mut peds {
            let d = pt.translation.distance(ex.pos);
            if d < ex.radius * 1.2 {
                let k = 1.0 - d / (ex.radius * 1.2);
                ped.health -= 200.0 * k;
                ped.vel += ((pt.translation - ex.pos).normalize_or(Vec3::Y) + Vec3::Y) * 10.0 * k;
                ped.knock_t = 2.0;
            } else if d < 40.0 {
                ped.state = PedState::Flee { from: ex.pos, t: 8.0 };
            }
        }
    }
}

/// Cars hitting people on foot (player and pedestrians are sensors, so the solver ignores them).
fn run_over(
    cars: Query<(&Transform, &Velocity, &Vehicle)>,
    mut player: Query<(&mut Player, &Transform), Without<Vehicle>>,
    mut peds: Query<(&mut Ped, &Transform), (Without<Player>, Without<Vehicle>)>,
    mut hud: ResMut<crate::hud::HudMessages>,
) {
    let mut pl = player.single_mut().ok();
    for (ct, vel, v) in &cars {
        let speed = vel.linear.length();
        if speed < 2.5 {
            continue;
        }
        let s = &v.spec;
        let inv = ct.rotation.inverse();
        let half = Vec3::new(s.width * 0.5 + 0.35, 1.3, s.len * 0.5 + 0.35);
        let hit = |p: Vec3| -> bool {
            let l = inv * (p - ct.translation);
            l.x.abs() < half.x && l.z.abs() < half.z && l.y > -1.6 && l.y < s.roof + 0.6
        };
        if let Some((p, pt)) = pl.as_mut() {
            if p.vehicle.is_none() && p.knock_t <= 0.0 && hit(pt.translation) {
                let rel = (vel.linear - p.vel).length();
                if rel > 3.0 {
                    p.hurt((rel - 3.0) * 4.5);
                    p.vel = vel.linear * 1.05 + Vec3::Y * (2.0 + rel * 0.25);
                    p.knock_t = 1.2;
                    if rel > 12.0 {
                        hud.push("Ouch!");
                    }
                }
            }
        }
        for (mut ped, pt) in &mut peds {
            if ped.knock_t <= 0.0 && hit(pt.translation) {
                let rel = (vel.linear - ped.vel).length();
                if rel > 3.0 {
                    ped.health -= (rel - 3.0) * 9.0;
                    ped.vel = vel.linear * 1.05 + Vec3::Y * (2.0 + rel * 0.25);
                    ped.knock_t = 2.0;
                    ped.state = PedState::Flee { from: ct.translation, t: 10.0 };
                }
            }
        }
    }
}

fn brake_lights(cars: Query<(&Vehicle, &VehicleControl, &Velocity, &Transform)>, mut materials: ResMut<Assets<StandardMaterial>>, mut last: Local<Vec<(AssetId<StandardMaterial>, bool)>>) {
    let mut next = Vec::new();
    for (v, c, vel, t) in &cars {
        if v.wrecked {
            continue;
        }
        let fwd = t.rotation * Vec3::NEG_Z;
        let on = v.driver != Driver::None && (c.brake > 0.0 && vel.linear.dot(fwd) > 0.5 || c.handbrake);
        let id = v.tail.id();
        let changed = last.iter().find(|(i, _)| *i == id).map(|(_, b)| *b != on).unwrap_or(true);
        if changed {
            if let Some(mut m) = materials.get_mut(&v.tail) {
                m.emissive = if on { LinearRgba::rgb(14.0, 0.3, 0.2) } else if v.driver != Driver::None { LinearRgba::rgb(1.6, 0.04, 0.03) } else { LinearRgba::rgb(0.05, 0.0, 0.0) };
            }
        }
        next.push((id, on));
    }
    *last = next;
}

pub fn car_yaw(t: &Transform) -> f32 {
    crate::camera::yaw_of(t.rotation)
}

