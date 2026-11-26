//! Rendering setup (sky, sun, post-processing) and the third-person / chase camera
//! with a collision-aware spring arm.

use std::f32::consts::PI;

use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::camera::Exposure;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{light_consts::lux, Atmosphere, CascadeShadowConfigBuilder, SunDisk};
use bevy::light::atmosphere::ScatteringMedium;
use bevy::pbr::{AtmosphereSettings, DistanceFog, FogFalloff};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::player::Player;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraRig>()
            .init_resource::<CameraOverride>()
            .init_resource::<CameraShake>()
            .add_systems(Startup, setup_rendering)
            .add_systems(
                PostUpdate,
                update_camera
                    .after(PhysicsSet::Writeback)
                    .before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

#[derive(Component)]
pub struct MainCamera;

#[derive(Component)]
pub struct Sun;

#[derive(Resource)]
pub struct CameraRig {
    pub aim: bool,
    pub recoil: f32,
    pub zoom: usize,
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub cur_dist: f32,
    pub idle: f32,
    pub fov: f32,
    /// field of view while aiming (set by the current weapon)
    pub aim_fov: f32,
    pub pos: Vec3,
    pub pivot: Vec3,
    pub initialized: bool,
}

impl Default for CameraRig {
    fn default() -> Self {
        CameraRig { aim: false, recoil: 0.0, zoom: 1, yaw: 0.0, pitch: -0.18, dist: 4.2, cur_dist: 4.2, idle: 10.0, fov: 62.0, aim_fov: 46.0, pos: Vec3::ZERO, pivot: Vec3::ZERO, initialized: false }
    }
}

/// Autotest / cutscene hook: when set, the camera is placed here instead.
#[derive(Resource, Default)]
pub struct CameraOverride(pub Option<Transform>);

#[derive(Resource, Default)]
pub struct CameraShake {
    pub trauma: f32,
}

pub fn env_flag(k: &str) -> bool {
    std::env::var(k).is_ok()
}

fn setup_rendering(mut commands: Commands, mut mediums: ResMut<Assets<ScatteringMedium>>) {
    let atmo = !env_flag("HH_NO_ATMO");
    if atmo {
        let medium = mediums.add(ScatteringMedium::earth(256, 256));
        commands.spawn(Atmosphere::earth(medium));
    }
    let cam = commands
        .spawn((
            Camera3d::default(),
            MainCamera,
            Projection::Perspective(PerspectiveProjection { fov: 62f32.to_radians(), near: 0.1, far: 6000.0, ..default() }),
            Transform::from_xyz(0.0, 20.0, 40.0).looking_at(Vec3::ZERO, Vec3::Y),
            Exposure { ev100: 13.2 },
            Tonemapping::AcesFitted,
            Bloom::NATURAL,
            DistanceFog {
                color: Color::srgba(0.62, 0.7, 0.8, 1.0),
                directional_light_color: Color::srgba(1.0, 0.9, 0.75, 0.4),
                directional_light_exponent: 24.0,
                falloff: FogFalloff::from_visibility_colors(2600.0, Color::srgb(0.45, 0.55, 0.68), Color::srgb(0.75, 0.82, 0.92)),
            },
        ))
        .id();
    if atmo {
        // the atmosphere draws the sky; image-based light comes from lighting::SkyCubemap
        commands.entity(cam).insert(AtmosphereSettings::default());
    }
    // MSAA 4x by default: TAA needs a full depth + motion-vector prepass, which
    // costs ~8 ms on this scene (measured with HH_BENCH). HH_TAA=1 opts back in.
    // TAA in both lighting modes: it anti-aliases the deferred G-buffer and
    // resolves SSAO / SSR / Solari noise over time.
    commands.entity(cam).insert((Msaa::Off, TemporalAntiAliasing::default()));
    // every material renders deferred: the camera must always own the deferred
    // prepass, whatever lighting mode (and post effects) are active
    commands.entity(cam).insert((bevy::core_pipeline::prepass::DepthPrepass, bevy::core_pipeline::prepass::DeferredPrepass));
    commands.spawn((
        Sun,
        DirectionalLight { shadow_maps_enabled: !env_flag("HH_NO_SHADOW"), illuminance: lux::RAW_SUNLIGHT, ..default() },
        CascadeShadowConfigBuilder { num_cascades: 4, first_cascade_far_bound: 18.0, maximum_distance: 420.0, overlap_proportion: 0.25, ..default() }.build(),
        SunDisk::EARTH,
        // late-afternoon sun from the south-west
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, -2.4, -0.62, 0.0)),
    ));
}

pub fn yaw_of(rot: Quat) -> f32 {
    let f = rot * Vec3::NEG_Z;
    (-f.x).atan2(-f.z)
}

fn wrap_angle(a: f32) -> f32 {
    (a + PI).rem_euclid(2.0 * PI) - PI
}

#[allow(clippy::too_many_arguments)]
fn update_camera(
    time: Res<Time>,
    input: Res<crate::input::GameInput>,
    mut rig: ResMut<CameraRig>,
    over: Res<CameraOverride>,
    mut shake: ResMut<CameraShake>,
    player: Query<(Entity, &Transform, &Player), Without<MainCamera>>,
    vehicles: Query<(&Transform, &Velocity, &crate::seats::Seat), Without<MainCamera>>,
    mut cam: Query<(&mut Transform, &mut Projection), With<MainCamera>>,
    rapier: ReadRapierContext,
) {
    let Ok((mut ct, mut proj)) = cam.single_mut() else { return };
    if let Some(t) = over.0 {
        *ct = t;
        return;
    }
    let Ok((pe, pt, pl)) = player.single() else { return };
    let dt = time.delta_secs().min(0.05);

    let look = input.look * 0.0022;
    if look.length_squared() > 1e-8 {
        rig.idle = 0.0;
    } else {
        rig.idle += dt;
    }
    // shots kick the view instantly (weapons.rs); this settles most of it back
    let settle = rig.recoil * (1.0 - (-dt * 7.0).exp());
    rig.pitch -= settle;
    rig.recoil -= settle;
    if input.camera_mode {
        rig.zoom += 1;
    }
    // slower turning when zoomed in, in proportion to the zoom
    let look = if rig.aim { look * (rig.fov / 62.0).clamp(0.15, 1.0) * 0.85 } else { look };
    rig.yaw -= look.x;
    rig.pitch = (rig.pitch - look.y).clamp(-1.35, 0.75);

    let (pivot, want_dist, exclude_body, base_fov) = if let Some((vt, vel, veh)) = pl.vehicle.and_then(|v| vehicles.get(v).ok()) {
        let speed = vel.linear.length();
        let car_yaw = yaw_of(vt.rotation);
        // chase: drift back behind the car once the player stops steering the camera
        if rig.idle > 1.2 && speed > 2.0 {
            let fwd = vt.rotation * Vec3::NEG_Z;
            let reversing = vel.linear.dot(fwd) < -2.0;
            let target = if reversing { car_yaw + PI } else { car_yaw };
            let d = wrap_angle(target - rig.yaw);
            rig.yaw += d * (dt * 3.0).min(1.0);
            rig.pitch += (-0.2 - rig.pitch) * (dt * 1.5).min(1.0);
        }
        let p = vt.translation + Vec3::Y * veh.cam_height;
        let fov = 66.0 + (speed / 60.0).min(1.0) * 16.0;
        let mult = [0.75, 1.0, 1.45][rig.zoom % 3];
        if input.look_behind {
            rig.yaw = car_yaw + PI;
            rig.idle = 0.0;
        }
        (p, veh.cam_dist * mult, pl.vehicle, fov)
    } else {
        let right = Vec3::new(rig.yaw.cos(), 0.0, -rig.yaw.sin());
        let fwd = Vec3::new(-rig.yaw.sin(), 0.0, -rig.yaw.cos());
        if rig.aim && rig.aim_fov < 20.0 {
            // scope: look down the barrel from just in front of the head
            (pt.translation + Vec3::Y * 0.72 + fwd * 0.35 + right * 0.05, 0.02, None, rig.aim_fov)
        } else if rig.aim {
            (pt.translation + Vec3::Y * 0.68 + right * 0.58, 2.1, None, rig.aim_fov)
        } else {
            let d = [2.6, 3.6, 5.2][rig.zoom % 3];
            (pt.translation + Vec3::Y * 0.75 + right * 0.45, d, None, 62.0)
        }
    };
    rig.dist = want_dist;
    // aim zoom snaps in fast; speed-based vehicle FOV eases slowly
    let fov_rate = if pl.vehicle.is_some() { 2.0 } else { 12.0 };
    rig.fov += (base_fov - rig.fov) * (1.0 - (-dt * fov_rate).exp());

    // spring-smoothed pivot: filters suspension / contact jitter out of the view
    let target_pivot = pivot;
    if !rig.initialized || rig.pivot.distance(target_pivot) > 12.0 {
        rig.pivot = target_pivot;
    } else {
        let k = if exclude_body.is_some() { 16.0 } else { 22.0 };
        let p = rig.pivot;
        rig.pivot = p.lerp(target_pivot, 1.0 - (-dt * k).exp());
    }
    let pivot = rig.pivot;
    let rot = Quat::from_euler(EulerRot::YXZ, rig.yaw, rig.pitch, 0.0);
    let back = rot * Vec3::Z;
    // spring arm collision
    let mut dist = rig.dist;
    if let Ok(ctx) = rapier.single() {
        let mut filter = QueryFilter::default().exclude_collider(pe).exclude_sensors();
        if let Some(b) = exclude_body {
            filter = filter.exclude_rigid_body(b);
        }
        if let Some((_, toi)) = ctx.cast_ray(pivot, back, rig.dist + 0.3, true, filter) {
            dist = (toi - 0.3).max(0.4);
        }
    }
    if dist < rig.cur_dist || !rig.initialized {
        rig.cur_dist = dist;
    } else {
        rig.cur_dist += (dist - rig.cur_dist) * (dt * 3.0).min(1.0);
    }
    let mut pos = pivot + back * rig.cur_dist;
    // keep the camera above the ocean surface
    pos.y = pos.y.max(crate::world::SEA_LEVEL + 0.4);
    if !rig.initialized {
        rig.pos = pos;
        rig.initialized = true;
    }
    rig.pos = pos;

    shake.trauma = (shake.trauma - dt * 1.2).max(0.0);
    let t = time.elapsed_secs();
    let s = shake.trauma * shake.trauma;
    let jitter = Vec3::new((t * 37.0).sin(), (t * 41.0).cos(), (t * 29.0).sin()) * s * 0.35;
    *ct = Transform::from_translation(rig.pos + jitter).looking_at(pivot + jitter * 0.5, Vec3::Y);
    if let Projection::Perspective(p) = proj.as_mut() {
        p.fov = rig.fov.to_radians();
    }
}
