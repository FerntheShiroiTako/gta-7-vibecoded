//! Particles: smoke, fire, explosions, sparks, debris, tire smoke and skid marks.

use std::collections::{HashMap, VecDeque};

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::lighting::NoRaytrace;
use crate::meshgen::Rng;
use crate::vehicle::{Vehicle, GROUP_WORLD, GROUP_VEHICLE};

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SkidPool>()
            .init_resource::<FxRng>()
            .add_systems(Startup, (setup_fx, warmup_fx).chain())
            .add_systems(Update, (emitters, update_particles, update_flashes, tire_fx, debris_cleanup, warmup_cleanup));
    }
}

#[derive(Resource)]
pub struct FxAssets {
    pub sphere: Handle<Mesh>,
    pub smoke: Handle<StandardMaterial>,
    pub black: Handle<StandardMaterial>,
    pub fire: Handle<StandardMaterial>,
    pub fireball: Handle<StandardMaterial>,
    pub spark: Handle<StandardMaterial>,
    pub tire_smoke: Handle<StandardMaterial>,
    pub debris_mesh: Handle<Mesh>,
    pub debris_mat: Handle<StandardMaterial>,
    pub skid_mesh: Handle<Mesh>,
    pub skid_mat: Handle<StandardMaterial>,
}

#[derive(Resource)]
pub struct FxRng(pub Rng);
impl Default for FxRng {
    fn default() -> Self {
        FxRng(Rng::new(4242))
    }
}

#[derive(Component)]
pub struct Particle {
    pub vel: Vec3,
    pub life: f32,
    pub max: f32,
    pub grow: f32,
    pub drag: f32,
    pub buoy: f32,
    pub size: f32,
    pub shrink: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EmitterKind {
    Smoke,
    BlackSmoke,
    Fire,
}

#[derive(Component)]
pub struct Emitter {
    pub kind: EmitterKind,
    pub rate: f32,
    pub acc: f32,
}

#[derive(Component)]
struct Flash {
    t: f32,
    max: f32,
    intensity: f32,
}

#[derive(Component)]
struct Debris {
    t: f32,
}

#[derive(Resource, Default)]
struct SkidPool {
    marks: VecDeque<Entity>,
    last: HashMap<(Entity, usize), Vec3>,
}

const MAX_PARTICLES: usize = 1400;

fn setup_fx(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let blend = |c: Color| StandardMaterial { base_color: c, alpha_mode: AlphaMode::Blend, perceptual_roughness: 1.0, ..default() };
    // lit path on purpose: Bevy's unlit path ignores `emissive`
    let glow = |e: LinearRgba| StandardMaterial { base_color: Color::BLACK, emissive: e, perceptual_roughness: 1.0, reflectance: 0.0, alpha_mode: AlphaMode::Add, ..default() };
    commands.insert_resource(FxAssets {
        sphere: meshes.add(Sphere::new(1.0).mesh().ico(2).unwrap()),
        smoke: materials.add(blend(Color::srgba(0.62, 0.62, 0.62, 0.18))),
        black: materials.add(blend(Color::srgba(0.08, 0.08, 0.08, 0.3))),
        tire_smoke: materials.add(blend(Color::srgba(0.85, 0.85, 0.85, 0.22))),
        fire: materials.add(glow(LinearRgba::rgb(14.0, 4.0, 0.6))),
        fireball: materials.add(glow(LinearRgba::rgb(40.0, 14.0, 2.5))),
        spark: materials.add(glow(LinearRgba::rgb(40.0, 26.0, 8.0))),
        debris_mesh: meshes.add(Mesh::from(Cuboid::new(0.5, 0.12, 0.35)).with_generated_tangents().expect("cuboid tangents")),
        debris_mat: materials.add(StandardMaterial { base_color: Color::srgb(0.06, 0.05, 0.05), perceptual_roughness: 0.9, ..default() }),
        skid_mesh: meshes.add(Plane3d::default().mesh().size(1.0, 1.0)),
        skid_mat: materials.add(StandardMaterial {
            base_color: Color::srgba(0.015, 0.015, 0.015, 0.55),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 1.0,
            depth_bias: 50.0,
            ..default()
        }),
    });
}

pub fn particle(commands: &mut Commands, fx: &FxAssets, mat: Handle<StandardMaterial>, pos: Vec3, p: Particle) {
    let s = p.size;
    // particles never cast shadows (a smoke cloud's shadow is a black blob on a facade)
    commands.spawn((Mesh3d(fx.sphere.clone()), MeshMaterial3d(mat), Transform::from_translation(pos).with_scale(Vec3::splat(s)), p, NoRaytrace, bevy::light::NotShadowCaster));
}

fn emitters(
    mut commands: Commands,
    time: Res<Time>,
    fx: Res<FxAssets>,
    mut rng: ResMut<FxRng>,
    mut q: Query<(&mut Emitter, &GlobalTransform)>,
    count: Query<(), With<Particle>>,
) {
    let dt = time.delta_secs();
    let mut n = count.iter().count();
    let r = &mut rng.0;
    for (mut em, gt) in &mut q {
        em.acc += em.rate * dt;
        while em.acc >= 1.0 {
            em.acc -= 1.0;
            if n >= MAX_PARTICLES {
                continue;
            }
            n += 1;
            let pos = gt.translation() + Vec3::new(r.range(-0.4, 0.4), r.range(0.0, 0.3), r.range(-0.4, 0.4));
            match em.kind {
                EmitterKind::Smoke | EmitterKind::BlackSmoke => {
                    let mat = if em.kind == EmitterKind::Smoke { fx.smoke.clone() } else { fx.black.clone() };
                    let life = r.range(2.2, 3.6);
                    particle(
                        &mut commands,
                        &fx,
                        mat,
                        pos,
                        Particle { vel: Vec3::new(r.range(-0.4, 0.4), r.range(1.2, 2.2), r.range(-0.4, 0.4)), life, max: life, grow: 0.7, drag: 0.6, buoy: 0.5, size: r.range(0.2, 0.35), shrink: false },
                    );
                }
                EmitterKind::Fire => {
                    let life = r.range(0.4, 0.8);
                    particle(
                        &mut commands,
                        &fx,
                        fx.fire.clone(),
                        pos,
                        Particle { vel: Vec3::new(r.range(-0.5, 0.5), r.range(2.0, 4.0), r.range(-0.5, 0.5)), life, max: life, grow: -0.2, drag: 1.5, buoy: 2.0, size: r.range(0.35, 0.6), shrink: true },
                    );
                    if r.chance(0.35) {
                        let life = r.range(2.5, 4.0);
                        particle(
                            &mut commands,
                            &fx,
                            fx.black.clone(),
                            pos + Vec3::Y,
                            Particle { vel: Vec3::new(r.range(-0.3, 0.3), r.range(2.0, 3.0), r.range(-0.3, 0.3)), life, max: life, grow: 0.6, drag: 0.5, buoy: 0.5, size: 0.35, shrink: false },
                        );
                    }
                }
            }
        }
    }
}

fn update_particles(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Particle, &mut Transform)>) {
    let dt = time.delta_secs();
    for (e, mut p, mut t) in &mut q {
        p.life -= dt;
        if p.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let drag = (1.0 - p.drag * dt).max(0.0);
        p.vel *= drag;
        p.vel.y += p.buoy * dt;
        t.translation += p.vel * dt;
        let age = 1.0 - p.life / p.max;
        let mut s = p.size * (1.0 + p.grow * age * 3.0).max(0.05);
        if p.shrink {
            s *= p.life / p.max;
        } else if age > 0.7 {
            s *= 1.0 - (age - 0.7) / 0.3 * 0.8;
        }
        t.scale = Vec3::splat(s.max(0.01));
    }
}

pub fn spawn_explosion_fx(commands: &mut Commands, fx: &FxAssets, pos: Vec3) {
    let mut r = Rng::new((pos.x * 13.0 + pos.z * 7.0).abs() as u64 + 1);
    for _ in 0..22 {
        let dir = Vec3::new(r.range(-1.0, 1.0), r.range(0.1, 1.0), r.range(-1.0, 1.0)).normalize();
        let life = r.range(0.5, 1.0);
        particle(commands, fx, fx.fireball.clone(), pos + dir * 0.5, Particle { vel: dir * r.range(5.0, 13.0), life, max: life, grow: 0.6, drag: 3.0, buoy: 3.0, size: r.range(1.2, 2.4), shrink: true });
    }
    for _ in 0..16 {
        let dir = Vec3::new(r.range(-1.0, 1.0), r.range(0.3, 1.0), r.range(-1.0, 1.0)).normalize();
        let life = r.range(4.0, 7.0);
        particle(commands, fx, fx.black.clone(), pos + dir, Particle { vel: dir * r.range(2.0, 5.0) + Vec3::Y * 2.0, life, max: life, grow: 0.7, drag: 0.8, buoy: 0.8, size: r.range(0.7, 1.2), shrink: false });
    }
    for _ in 0..26 {
        let dir = Vec3::new(r.range(-1.0, 1.0), r.range(0.2, 1.2), r.range(-1.0, 1.0)).normalize();
        let life = r.range(0.6, 1.4);
        particle(commands, fx, fx.spark.clone(), pos, Particle { vel: dir * r.range(10.0, 24.0), life, max: life, grow: 0.0, drag: 0.4, buoy: -9.8, size: 0.06, shrink: true });
    }
    commands.spawn((
        PointLight { intensity: 6.0e8, range: 70.0, color: Color::srgb(1.0, 0.6, 0.25), shadow_maps_enabled: false, ..default() },
        Transform::from_translation(pos + Vec3::Y * 1.5),
        Flash { t: 0.0, max: 0.7, intensity: 6.0e8 },
    ));
    for _ in 0..7 {
        let dir = Vec3::new(r.range(-1.0, 1.0), r.range(0.6, 1.4), r.range(-1.0, 1.0)).normalize();
        commands.spawn((
            Mesh3d(fx.debris_mesh.clone()),
            MeshMaterial3d(fx.debris_mat.clone()),
            Transform::from_translation(pos + dir).with_rotation(Quat::from_rotation_y(r.range(0.0, 6.0))),
            RigidBody::Dynamic,
            Collider::cuboid(0.25, 0.06, 0.17),
            ColliderMassProperties::Mass(15.0),
            Velocity { linear: dir * r.range(8.0, 16.0), angular: Vec3::new(r.range(-9.0, 9.0), r.range(-9.0, 9.0), r.range(-9.0, 9.0)) },
            CollisionGroups::new(GROUP_WORLD, GROUP_WORLD | GROUP_VEHICLE),
            Debris { t: 0.0 },
        ));
    }
}

fn update_flashes(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Flash, &mut PointLight)>) {
    for (e, mut f, mut l) in &mut q {
        f.t += time.delta_secs();
        let k = (1.0 - f.t / f.max).max(0.0);
        l.intensity = f.intensity * k * k;
        if f.t >= f.max {
            commands.entity(e).despawn();
        }
    }
}

fn debris_cleanup(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Debris)>) {
    for (e, mut d) in &mut q {
        d.t += time.delta_secs();
        if d.t > 12.0 {
            commands.entity(e).despawn();
        }
    }
}

fn tire_fx(
    mut commands: Commands,
    time: Res<Time>,
    fx: Res<FxAssets>,
    mut rng: ResMut<FxRng>,
    mut pool: ResMut<SkidPool>,
    cars: Query<(Entity, &Transform, &Vehicle)>,
    count: Query<(), With<Particle>>,
) {
    let dt = time.delta_secs();
    let mut n = count.iter().count();
    let mut seen = Vec::new();
    for (e, t, v) in &cars {
        let up = t.rotation * Vec3::Y;
        for (i, w) in v.wheels.iter().enumerate() {
            let key = (e, i);
            if !w.contact || w.slip < 4.0 || v.wrecked {
                continue;
            }
            let contact = t.transform_point(w.mount) - up * (w.ext + v.spec.wheel_r) + up * 0.025;
            seen.push(key);
            if n < MAX_PARTICLES && rng.0.chance((dt * 18.0).min(1.0)) {
                n += 1;
                let life = rng.0.range(1.2, 2.2);
                let r = &mut rng.0;
                particle(
                    &mut commands,
                    &fx,
                    fx.tire_smoke.clone(),
                    contact + Vec3::Y * 0.3,
                    Particle { vel: Vec3::new(r.range(-0.6, 0.6), r.range(0.4, 1.0), r.range(-0.6, 0.6)), life, max: life, grow: 1.4, drag: 1.0, buoy: 0.3, size: 0.35, shrink: false },
                );
            }
            match pool.last.get(&key).copied() {
                Some(prev) if prev.distance(contact) > 0.35 => {
                    let d = contact - prev;
                    let len = d.length();
                    if len < 3.0 {
                        let mid = (prev + contact) * 0.5;
                        let yaw = (-d.x).atan2(-d.z);
                        let ent = commands
                            .spawn((
                                Mesh3d(fx.skid_mesh.clone()),
                                MeshMaterial3d(fx.skid_mat.clone()),
                                NoRaytrace,
                                Transform::from_translation(mid).with_rotation(Quat::from_rotation_y(yaw)).with_scale(Vec3::new(0.22, 1.0, len + 0.05)),
                            ))
                            .id();
                        pool.marks.push_back(ent);
                        while pool.marks.len() > 900 {
                            if let Some(old) = pool.marks.pop_front() {
                                commands.entity(old).despawn();
                            }
                        }
                    }
                    pool.last.insert(key, contact);
                }
                Some(_) => {}
                None => {
                    pool.last.insert(key, contact);
                }
            }
        }
    }
    pool.last.retain(|k, _| seen.contains(k));
}

#[derive(Component)]
struct Warmup(u32);

/// Draw each FX material once at startup (tiny, under the spawn camera) so their
/// render pipelines compile during loading rather than as a hitch mid-game.
fn warmup_fx(mut commands: Commands, fx: Res<FxAssets>) {
    let at = crate::world::spawn_point();
    let mats = [&fx.smoke, &fx.black, &fx.fire, &fx.fireball, &fx.spark, &fx.tire_smoke, &fx.debris_mat, &fx.skid_mat];
    for (i, m) in mats.iter().enumerate() {
        commands.spawn((
            Mesh3d(fx.sphere.clone()),
            MeshMaterial3d((*m).clone()),
            Transform::from_translation(at + Vec3::new(i as f32 * 0.01, 0.2, -1.0)).with_scale(Vec3::splat(0.001)),
            Warmup(30),
            NoRaytrace,
        ));
    }
    commands.spawn((
        PointLight { intensity: 1.0, range: 1.0, shadow_maps_enabled: false, ..default() },
        Transform::from_translation(at),
        Warmup(30),
    ));
}

fn warmup_cleanup(mut commands: Commands, mut q: Query<(Entity, &mut Warmup)>) {
    for (e, mut w) in &mut q {
        w.0 = w.0.saturating_sub(1);
        if w.0 == 0 {
            commands.entity(e).despawn();
        }
    }
}
