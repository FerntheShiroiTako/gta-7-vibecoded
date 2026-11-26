//! Weapons: fists, baseball bat, pistol, revolver, SMG, assault rifle, pump
//! shotgun, sniper rifle, minigun, grenades and a rocket launcher.
//!
//! Gunplay: shots go through the screen centre inside a spread cone that
//! depends on the gun, aiming, moving and sustained fire (bloom), and are then
//! re-traced from the muzzle so cover works. Recoil kicks the camera and mostly
//! settles back. Hits push bodies (and ragdolls) along the shot, peds flinch,
//! headshots count extra, the crosshair opens with spread and shows hit/kill
//! markers. Hold Tab for the weapon wheel (time slows while it's open).

use std::collections::VecDeque;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::camera::{CameraRig, MainCamera};
use crate::fx::{particle, FxAssets, Particle};
use crate::hud::{font, shadow, HudFonts};
use crate::humanoid::Rig;
use crate::input::GameInput;
use crate::lighting::NoRaytrace;
use crate::meshgen::{MeshBuilder, Rng, WHITE};
use crate::peds::{Ped, PedState};
use crate::player::Player;
use crate::vehicle::{Explosion, Vehicle};
use crate::wanted::{report_crime, Crime};

pub struct WeaponPlugin;

impl Plugin for WeaponPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Arsenal>()
            .init_resource::<Decals>()
            .init_resource::<HitMarker>()
            .init_resource::<Wheel>()
            .add_systems(Startup, (setup_weapon_assets, setup_weapon_ui.after(crate::hud::HudSetup)))
            .add_systems(
                Update,
                (weapon_wheel, select_weapon, fire_weapons, update_rockets, update_grenades, update_transients, muzzle_light, attach_weapon_model, weapon_ui)
                    .chain()
                    .after(crate::player::PlayerMoveSet),
            );
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Weapon {
    Fists,
    Pistol,
    Smg,
    Shotgun,
    Rocket,
    Bat,
    Revolver,
    Rifle,
    Sniper,
    Grenade,
    Minigun,
}

pub const WEAPON_COUNT: usize = 11;

/// Index order (number keys 1-5 pick the first five).
pub const WEAPONS: [Weapon; WEAPON_COUNT] = [
    Weapon::Fists,
    Weapon::Pistol,
    Weapon::Smg,
    Weapon::Shotgun,
    Weapon::Rocket,
    Weapon::Bat,
    Weapon::Revolver,
    Weapon::Rifle,
    Weapon::Sniper,
    Weapon::Grenade,
    Weapon::Minigun,
];

/// Weapon-wheel / cycling order, clockwise from the top.
pub const WHEEL_ORDER: [Weapon; WEAPON_COUNT] = [
    Weapon::Fists,
    Weapon::Bat,
    Weapon::Pistol,
    Weapon::Revolver,
    Weapon::Smg,
    Weapon::Rifle,
    Weapon::Shotgun,
    Weapon::Sniper,
    Weapon::Minigun,
    Weapon::Rocket,
    Weapon::Grenade,
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FireKind {
    Melee,
    Hitscan,
    Rocket,
    Thrown,
}

pub struct WeaponStats {
    pub name: &'static str,
    pub blurb: &'static str,
    pub kind: FireKind,
    pub damage: f32,
    pub interval: f32,
    pub clip: u32,
    pub pellets: u32,
    /// spread cone (radians) from the hip / when aiming
    pub spread: f32,
    pub aim_spread: f32,
    /// extra spread per shot and its cap
    pub bloom: f32,
    pub bloom_max: f32,
    pub range: f32,
    pub reload: f32,
    pub auto: bool,
    /// camera kick per shot (pitch, random yaw)
    pub kick: f32,
    pub kick_yaw: f32,
    /// push on whatever is hit
    pub force: f32,
    pub two_handed: bool,
    pub move_mult: f32,
    /// field of view while aiming (degrees)
    pub aim_fov: f32,
    /// seconds of barrel spin before it fires
    pub spin_up: f32,
    pub pickup_ammo: u32,
    pub max_ammo: u32,
    /// headshot multiplier
    pub head: f32,
}

impl WeaponStats {
    pub fn melee(&self) -> bool {
        self.kind == FireKind::Melee
    }
}

impl Weapon {
    pub fn index(self) -> usize {
        WEAPONS.iter().position(|w| *w == self).unwrap_or(0)
    }
    pub fn stats(self) -> WeaponStats {
        let base = WeaponStats {
            name: "",
            blurb: "",
            kind: FireKind::Hitscan,
            damage: 0.0,
            interval: 0.5,
            clip: 0,
            pellets: 1,
            spread: 0.03,
            aim_spread: 0.008,
            bloom: 0.0,
            bloom_max: 0.0,
            range: 150.0,
            reload: 1.5,
            auto: false,
            kick: 0.0,
            kick_yaw: 0.0,
            force: 200.0,
            two_handed: false,
            move_mult: 1.0,
            aim_fov: 46.0,
            spin_up: 0.0,
            pickup_ammo: 0,
            max_ammo: 0,
            head: 3.0,
        };
        match self {
            Weapon::Fists => WeaponStats { name: "Fists", blurb: "Bare knuckles", kind: FireKind::Melee, damage: 22.0, interval: 0.42, range: 1.7, force: 180.0, ..base },
            Weapon::Bat => WeaponStats { name: "Baseball Bat", blurb: "Knocks people flat", kind: FireKind::Melee, damage: 60.0, interval: 0.75, range: 2.3, force: 520.0, two_handed: true, ..base },
            Weapon::Pistol => WeaponStats {
                name: "Pistol",
                blurb: "Reliable semi-auto",
                damage: 34.0,
                interval: 0.17,
                clip: 12,
                spread: 0.03,
                aim_spread: 0.005,
                bloom: 0.012,
                bloom_max: 0.06,
                reload: 1.2,
                kick: 0.02,
                kick_yaw: 0.008,
                force: 260.0,
                pickup_ammo: 36,
                max_ammo: 250,
                ..base
            },
            Weapon::Revolver => WeaponStats {
                name: "Revolver",
                blurb: "Six shots, each one counts",
                damage: 95.0,
                interval: 0.55,
                clip: 6,
                spread: 0.035,
                aim_spread: 0.003,
                bloom: 0.05,
                bloom_max: 0.1,
                range: 200.0,
                reload: 2.3,
                kick: 0.075,
                kick_yaw: 0.02,
                force: 750.0,
                pickup_ammo: 18,
                max_ammo: 120,
                head: 4.0,
                ..base
            },
            Weapon::Smg => WeaponStats {
                name: "SMG",
                blurb: "Sprays fast, kicks light",
                damage: 19.0,
                interval: 0.07,
                clip: 30,
                spread: 0.05,
                aim_spread: 0.016,
                bloom: 0.007,
                bloom_max: 0.07,
                range: 120.0,
                reload: 1.6,
                auto: true,
                kick: 0.011,
                kick_yaw: 0.008,
                force: 160.0,
                pickup_ammo: 90,
                max_ammo: 600,
                ..base
            },
            Weapon::Rifle => WeaponStats {
                name: "Assault Rifle",
                blurb: "Accurate full-auto",
                damage: 30.0,
                interval: 0.1,
                clip: 30,
                spread: 0.04,
                aim_spread: 0.006,
                bloom: 0.006,
                bloom_max: 0.05,
                range: 300.0,
                reload: 2.0,
                auto: true,
                kick: 0.016,
                kick_yaw: 0.007,
                force: 320.0,
                two_handed: true,
                aim_fov: 40.0,
                pickup_ammo: 60,
                max_ammo: 600,
                ..base
            },
            Weapon::Shotgun => WeaponStats {
                name: "Pump Shotgun",
                blurb: "Nine pellets, close range",
                damage: 15.0,
                interval: 0.85,
                clip: 6,
                pellets: 9,
                spread: 0.09,
                aim_spread: 0.065,
                range: 45.0,
                reload: 2.4,
                kick: 0.085,
                kick_yaw: 0.02,
                force: 230.0,
                two_handed: true,
                pickup_ammo: 16,
                max_ammo: 120,
                head: 2.0,
                ..base
            },
            Weapon::Sniper => WeaponStats {
                name: "Sniper Rifle",
                blurb: "Scoped - hold aim to zoom",
                damage: 160.0,
                interval: 1.25,
                clip: 5,
                spread: 0.07,
                aim_spread: 0.0,
                bloom: 0.05,
                bloom_max: 0.08,
                range: 900.0,
                reload: 2.7,
                kick: 0.09,
                kick_yaw: 0.015,
                force: 900.0,
                two_handed: true,
                move_mult: 0.85,
                aim_fov: 13.0,
                pickup_ammo: 10,
                max_ammo: 60,
                head: 4.0,
                ..base
            },
            Weapon::Minigun => WeaponStats {
                name: "Minigun",
                blurb: "Spins up, never stops",
                damage: 22.0,
                interval: 0.04,
                clip: 500,
                spread: 0.06,
                aim_spread: 0.04,
                bloom: 0.002,
                bloom_max: 0.08,
                range: 200.0,
                reload: 4.0,
                auto: true,
                kick: 0.006,
                kick_yaw: 0.008,
                force: 240.0,
                two_handed: true,
                move_mult: 0.55,
                spin_up: 0.7,
                pickup_ammo: 300,
                max_ammo: 2000,
                head: 2.0,
                ..base
            },
            Weapon::Rocket => WeaponStats {
                name: "Rocket Launcher",
                blurb: "Point at problem",
                kind: FireKind::Rocket,
                interval: 1.4,
                clip: 1,
                range: 400.0,
                reload: 2.0,
                kick: 0.06,
                two_handed: true,
                move_mult: 0.75,
                pickup_ammo: 3,
                max_ammo: 20,
                ..base
            },
            Weapon::Grenade => WeaponStats {
                name: "Grenades",
                blurb: "Cook, throw, run",
                kind: FireKind::Thrown,
                interval: 0.9,
                clip: 1,
                reload: 0.45,
                pickup_ammo: 4,
                max_ammo: 25,
                ..base
            },
        }
    }
}

/// The player's weapons: (clip, reserve) per slot (index order of `WEAPONS`).
#[derive(Resource)]
pub struct Arsenal {
    pub current: usize,
    pub ammo: [(u32, u32); WEAPON_COUNT],
    pub cooldown: f32,
    pub reloading: f32,
    pub shown: Option<usize>,
    /// extra spread from sustained fire
    pub bloom: f32,
    /// minigun barrel spin 0..1
    pub spin: f32,
    rng: Rng,
}

impl Default for Arsenal {
    fn default() -> Self {
        let mut ammo = [(0, 0); WEAPON_COUNT];
        // you start with a pistol; the rest is found on the street (pickups,
        // dropped by cops) - the combat autotest gets the full set
        ammo[1] = (12, 48);
        if std::env::var("HH_COMBATTEST").is_ok() {
            for (i, w) in WEAPONS.iter().enumerate() {
                let st = w.stats();
                ammo[i] = (st.clip, st.pickup_ammo * 3);
            }
        }
        Arsenal { current: 1, ammo, cooldown: 0.0, reloading: 0.0, shown: None, bloom: 0.0, spin: 0.0, rng: Rng::new(99) }
    }
}

impl Arsenal {
    pub fn weapon(&self) -> Weapon {
        WEAPONS[self.current]
    }
    pub fn has(&self, i: usize) -> bool {
        WEAPONS[i].stats().melee() && (i == 0 || self.ammo[i].1 > 0 || self.ammo[i].0 > 0) || self.ammo[i].0 + self.ammo[i].1 > 0
    }
    /// Add a weapon / ammo (pickups). Returns true if it was new.
    pub fn give(&mut self, i: usize, amount: u32) -> bool {
        let st = WEAPONS[i].stats();
        let had = self.has(i);
        let a = &mut self.ammo[i];
        if st.melee() {
            a.1 = 1;
        } else {
            a.1 = (a.1 + amount).min(st.max_ammo);
            if a.0 == 0 {
                let c = st.clip.min(a.1);
                a.0 = c;
                a.1 -= c;
            }
        }
        !had
    }
}

#[derive(Resource)]
pub struct WeaponAssets {
    meshes: [Option<Handle<Mesh>>; WEAPON_COUNT],
    gun_mat: Handle<StandardMaterial>,
    wood_mat: Handle<StandardMaterial>,
    tracer_mesh: Handle<Mesh>,
    tracer_mat: Handle<StandardMaterial>,
    flash_mat: Handle<StandardMaterial>,
    decal_mesh: Handle<Mesh>,
    decal_mat: Handle<StandardMaterial>,
    rocket_mesh: Handle<Mesh>,
    grenade_mesh: Handle<Mesh>,
    blood_mat: Handle<StandardMaterial>,
    brass_mat: Handle<StandardMaterial>,
    dust_mat: Handle<StandardMaterial>,
}

#[derive(Resource, Default)]
struct Decals(VecDeque<Entity>);

#[derive(Component)]
struct Transient(f32);

#[derive(Component)]
pub struct WeaponModel;

#[derive(Component)]
struct Rocket {
    vel: Vec3,
    life: f32,
}

#[derive(Component)]
struct Grenade {
    vel: Vec3,
    fuse: f32,
}

#[derive(Component)]
struct MuzzleLight;

/// Crosshair feedback: time left, and whether it was a kill.
#[derive(Resource, Default)]
pub struct HitMarker {
    pub t: f32,
    pub kill: bool,
    pub hits: u32,
    pub kills: u32,
}

/// Weapon wheel state (hold Tab).
#[derive(Resource, Default)]
pub struct Wheel {
    pub open: bool,
    pub cursor: Vec2,
    /// index into `WHEEL_ORDER`
    pub hover: Option<usize>,
}

/// Hand-space gun model (grip at the origin, barrel toward -Z). Also used,
/// scaled up, for weapon pickups.
pub fn gun_model(w: Weapon) -> Option<MeshBuilder> {
    let mut mb = MeshBuilder::new();
    let bx = |mb: &mut MeshBuilder, c: Vec3, h: Vec3| mb.box_at(Affine3A::from_translation(c), h, WHITE, 1.0, true, true);
    let tube = |mb: &mut MeshBuilder, c: Vec3, r: f32, len: f32| {
        mb.cylinder_at(Affine3A::from_rotation_translation(Quat::from_rotation_x(-FRAC_PI_2), c + Vec3::Z * (len * 0.5)), r, r, len, 10, WHITE, true)
    };
    match w {
        Weapon::Fists => return None,
        Weapon::Pistol => {
            bx(&mut mb, Vec3::ZERO, Vec3::new(0.016, 0.05, 0.022));
            bx(&mut mb, Vec3::new(0.0, 0.055, -0.06), Vec3::new(0.017, 0.022, 0.1));
        }
        Weapon::Revolver => {
            bx(&mut mb, Vec3::new(0.0, 0.0, 0.01), Vec3::new(0.017, 0.055, 0.024));
            mb.cylinder_at(Affine3A::from_rotation_translation(Quat::from_rotation_x(-FRAC_PI_2), Vec3::new(0.0, 0.058, -0.03)), 0.03, 0.03, 0.05, 12, WHITE, true);
            tube(&mut mb, Vec3::new(0.0, 0.065, -0.21), 0.011, 0.16);
            bx(&mut mb, Vec3::new(0.0, 0.07, -0.06), Vec3::new(0.012, 0.012, 0.08));
        }
        Weapon::Smg => {
            bx(&mut mb, Vec3::ZERO, Vec3::new(0.018, 0.055, 0.022));
            bx(&mut mb, Vec3::new(0.0, 0.06, -0.1), Vec3::new(0.022, 0.035, 0.18));
            bx(&mut mb, Vec3::new(0.0, -0.03, -0.12), Vec3::new(0.014, 0.08, 0.018));
            bx(&mut mb, Vec3::new(0.0, 0.06, 0.12), Vec3::new(0.012, 0.025, 0.08));
        }
        Weapon::Rifle => {
            bx(&mut mb, Vec3::ZERO, Vec3::new(0.017, 0.055, 0.022)); // grip
            bx(&mut mb, Vec3::new(0.0, 0.065, -0.08), Vec3::new(0.024, 0.04, 0.24)); // receiver
            bx(&mut mb, Vec3::new(0.0, -0.01, -0.15), Vec3::new(0.016, 0.075, 0.03)); // magazine
            bx(&mut mb, Vec3::new(0.0, 0.06, 0.25), Vec3::new(0.02, 0.045, 0.12)); // stock
            bx(&mut mb, Vec3::new(0.0, 0.06, -0.38), Vec3::new(0.026, 0.03, 0.08)); // handguard
            tube(&mut mb, Vec3::new(0.0, 0.07, -0.66), 0.011, 0.22);
            bx(&mut mb, Vec3::new(0.0, 0.115, -0.08), Vec3::new(0.006, 0.012, 0.12)); // rail
        }
        Weapon::Shotgun => {
            bx(&mut mb, Vec3::ZERO, Vec3::new(0.018, 0.05, 0.022));
            bx(&mut mb, Vec3::new(0.0, 0.05, 0.14), Vec3::new(0.02, 0.035, 0.14));
            bx(&mut mb, Vec3::new(0.0, 0.035, -0.2), Vec3::new(0.022, 0.022, 0.12));
            tube(&mut mb, Vec3::new(0.0, 0.065, -0.6), 0.016, 0.55);
            tube(&mut mb, Vec3::new(0.0, 0.03, -0.45), 0.013, 0.35);
        }
        Weapon::Sniper => {
            bx(&mut mb, Vec3::ZERO, Vec3::new(0.017, 0.055, 0.022));
            bx(&mut mb, Vec3::new(0.0, 0.06, -0.12), Vec3::new(0.024, 0.035, 0.3));
            bx(&mut mb, Vec3::new(0.0, 0.05, 0.27), Vec3::new(0.022, 0.05, 0.14));
            tube(&mut mb, Vec3::new(0.0, 0.065, -1.0), 0.012, 0.58);
            tube(&mut mb, Vec3::new(0.0, 0.125, -0.24), 0.022, 0.3); // scope
            bx(&mut mb, Vec3::new(0.0, -0.005, -0.08), Vec3::new(0.014, 0.04, 0.03));
        }
        Weapon::Minigun => {
            bx(&mut mb, Vec3::new(0.0, 0.0, 0.02), Vec3::new(0.02, 0.06, 0.03)); // handle
            bx(&mut mb, Vec3::new(0.0, 0.07, 0.05), Vec3::new(0.07, 0.07, 0.15)); // motor
            for k in 0..6 {
                let a = k as f32 / 6.0 * TAU;
                tube(&mut mb, Vec3::new(a.cos() * 0.035, 0.07 + a.sin() * 0.035, -0.72), 0.011, 0.62);
            }
            mb.cylinder_at(Affine3A::from_rotation_translation(Quat::from_rotation_x(-FRAC_PI_2), Vec3::new(0.0, 0.07, -0.55)), 0.055, 0.055, 0.03, 12, WHITE, true);
        }
        Weapon::Rocket => {
            tube(&mut mb, Vec3::new(0.0, 0.09, -0.65), 0.06, 1.1);
            bx(&mut mb, Vec3::ZERO, Vec3::new(0.016, 0.05, 0.022));
        }
        Weapon::Bat => {
            mb.cylinder_at(Affine3A::from_rotation_translation(Quat::from_rotation_x(-FRAC_PI_2), Vec3::new(0.0, 0.0, 0.08)), 0.018, 0.045, 0.85, 12, WHITE, true);
        }
        Weapon::Grenade => {
            mb.ellipsoid(Vec3::new(0.0, 0.0, -0.02), Vec3::new(0.035, 0.035, 0.045), 12, 8, WHITE);
            bx(&mut mb, Vec3::new(0.0, 0.04, -0.02), Vec3::new(0.012, 0.012, 0.02));
        }
    }
    Some(mb)
}

fn setup_weapon_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let mut proj = MeshBuilder::new();
    proj.cylinder_at(Affine3A::from_rotation_translation(Quat::from_rotation_x(-FRAC_PI_2), Vec3::new(0.0, 0.0, 0.3)), 0.045, 0.045, 0.5, 8, WHITE, true);
    proj.cylinder_at(Affine3A::from_rotation_translation(Quat::from_rotation_x(-FRAC_PI_2), Vec3::new(0.0, 0.0, -0.2)), 0.045, 0.0, 0.14, 8, WHITE, false);
    let mut decal = MeshBuilder::new();
    decal.quad(
        [Vec3::new(-0.05, -0.05, 0.0), Vec3::new(0.05, -0.05, 0.0), Vec3::new(0.05, 0.05, 0.0), Vec3::new(-0.05, 0.05, 0.0)],
        Vec3::Z,
        [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y],
        WHITE,
    );
    let mut tracer = MeshBuilder::new();
    tracer.box_at(Affine3A::from_translation(Vec3::new(0.0, 0.0, -0.5)), Vec3::new(0.012, 0.012, 0.5), WHITE, 1.0, true, true);
    let mut grenade = MeshBuilder::new();
    grenade.ellipsoid(Vec3::ZERO, Vec3::new(0.06, 0.06, 0.075), 12, 8, WHITE);
    let ms: Vec<Option<Handle<Mesh>>> = WEAPONS.iter().map(|w| gun_model(*w).map(|m| meshes.add(m.build()))).collect();
    let meshes_arr: [Option<Handle<Mesh>>; WEAPON_COUNT] = ms.try_into().unwrap_or_else(|_| unreachable!());
    // glows go through the lit path: Bevy's unlit path ignores `emissive`
    let glow = |e: LinearRgba, add: bool| StandardMaterial {
        base_color: Color::BLACK,
        emissive: e,
        perceptual_roughness: 1.0,
        reflectance: 0.0,
        alpha_mode: if add { AlphaMode::Add } else { AlphaMode::Opaque },
        ..default()
    };
    commands.insert_resource(WeaponAssets {
        meshes: meshes_arr,
        gun_mat: materials.add(StandardMaterial { base_color: Color::srgb(0.06, 0.06, 0.065), metallic: 0.8, perceptual_roughness: 0.35, ..default() }),
        wood_mat: materials.add(StandardMaterial { base_color: Color::srgb(0.55, 0.38, 0.2), perceptual_roughness: 0.6, ..default() }),
        tracer_mesh: meshes.add(tracer.build()),
        tracer_mat: materials.add(glow(LinearRgba::rgb(30.0, 20.0, 7.0), true)),
        flash_mat: materials.add(glow(LinearRgba::rgb(60.0, 35.0, 10.0), true)),
        decal_mesh: meshes.add(decal.build()),
        decal_mat: materials.add(StandardMaterial { base_color: Color::srgba(0.02, 0.02, 0.02, 0.85), alpha_mode: AlphaMode::Blend, perceptual_roughness: 1.0, depth_bias: 30.0, ..default() }),
        rocket_mesh: meshes.add(proj.build()),
        grenade_mesh: meshes.add(grenade.build()),
        blood_mat: materials.add(StandardMaterial { base_color: Color::srgba(0.45, 0.02, 0.02, 0.9), alpha_mode: AlphaMode::Blend, perceptual_roughness: 0.5, ..default() }),
        brass_mat: materials.add(StandardMaterial { base_color: Color::srgb(0.85, 0.65, 0.25), metallic: 1.0, perceptual_roughness: 0.3, ..default() }),
        dust_mat: materials.add(StandardMaterial { base_color: Color::srgba(0.7, 0.66, 0.6, 0.35), alpha_mode: AlphaMode::Blend, perceptual_roughness: 1.0, ..default() }),
    });
    commands.spawn((PointLight { intensity: 0.0, range: 14.0, color: Color::srgb(1.0, 0.72, 0.4), shadow_maps_enabled: false, ..default() }, Transform::default(), Visibility::Hidden, MuzzleLight));
}

// ------------------------------------------------------------------------------------------------
// Selection: number keys, Q/E / wheel cycling, the weapon wheel
// ------------------------------------------------------------------------------------------------

fn switch_to(ars: &mut Arsenal, next: usize, hud: &mut crate::hud::HudMessages) {
    if next != ars.current {
        ars.current = next;
        ars.reloading = 0.0;
        ars.cooldown = 0.3;
        ars.spin = 0.0;
        hud.push(WEAPONS[next].stats().name);
    }
}

fn select_weapon(mut input: ResMut<GameInput>, mut ars: ResMut<Arsenal>, player: Query<&Player>, mut hud: ResMut<crate::hud::HudMessages>) {
    let Ok(p) = player.single() else { return };
    if p.vehicle.is_some() || p.health <= 0.0 {
        return;
    }
    if let Some(s) = input.weapon_slot {
        input.weapon_slot = None;
        if !ars.has(s) {
            hud.push(&format!("No {} yet - find one on the street", WEAPONS[s].stats().name));
            return;
        }
        switch_to(&mut ars, s, &mut hud);
    } else if input.weapon_cycle != 0 {
        // cycle in wheel order through what you're carrying
        let n = WHEEL_ORDER.len() as i32;
        let cur = WHEEL_ORDER.iter().position(|w| w.index() == ars.current).unwrap_or(0) as i32;
        let mut i = cur;
        for _ in 0..n {
            i = (i + input.weapon_cycle).rem_euclid(n);
            if ars.has(WHEEL_ORDER[i as usize].index()) {
                break;
            }
        }
        let next = WHEEL_ORDER[i as usize].index();
        switch_to(&mut ars, next, &mut hud);
    }
    if input.reload {
        input.reload = false;
        let st = WEAPONS[ars.current].stats();
        let (clip, res) = ars.ammo[ars.current];
        if st.clip > 0 && clip < st.clip && res > 0 && ars.reloading <= 0.0 {
            ars.reloading = st.reload;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn weapon_wheel(
    input: Res<GameInput>,
    mut wheel: ResMut<Wheel>,
    mut ars: ResMut<Arsenal>,
    player: Query<&Player>,
    menus: Res<crate::map::Menus>,
    mut vt: ResMut<Time<Virtual>>,
    mut hud: ResMut<crate::hud::HudMessages>,
) {
    let usable = player.single().is_ok_and(|p| p.vehicle.is_none() && p.health > 0.0) && !menus.any();
    let want = input.wheel && usable;
    if want && !wheel.open {
        wheel.open = true;
        wheel.cursor = Vec2::ZERO;
        wheel.hover = WHEEL_ORDER.iter().position(|w| w.index() == ars.current);
        vt.set_relative_speed(0.25);
    }
    if !want && wheel.open {
        wheel.open = false;
        vt.set_relative_speed(1.0);
        if let Some(h) = wheel.hover {
            let i = WHEEL_ORDER[h].index();
            if ars.has(i) {
                switch_to(&mut ars, i, &mut hud);
            }
        }
        return;
    }
    if !wheel.open {
        return;
    }
    // the mouse steers a cursor; past a small dead zone it picks the slot it points at
    let c = (wheel.cursor + Vec2::new(input.wheel_delta.x, -input.wheel_delta.y) * 0.012).clamp_length_max(1.0);
    wheel.cursor = c;
    if c.length() > 0.35 {
        let ang = c.x.atan2(c.y).rem_euclid(TAU);
        let n = WHEEL_ORDER.len();
        let slot = ((ang / TAU * n as f32 + 0.5).floor() as usize) % n;
        wheel.hover = Some(slot);
    }
    if input.scroll.abs() > 0.1 {
        let n = WHEEL_ORDER.len() as i32;
        let h = wheel.hover.unwrap_or(0) as i32 - input.scroll.signum() as i32;
        wheel.hover = Some(h.rem_euclid(n) as usize);
    }
}

// ------------------------------------------------------------------------------------------------
// Firing
// ------------------------------------------------------------------------------------------------

#[derive(bevy::ecs::system::SystemParam)]
struct FireRes<'w> {
    wa: Res<'w, WeaponAssets>,
    fx: Res<'w, FxAssets>,
    decals: ResMut<'w, Decals>,
    shake: ResMut<'w, crate::camera::CameraShake>,
    sfx: MessageWriter<'w, crate::audio::Sfx>,
    marker: ResMut<'w, HitMarker>,
    wheel: Res<'w, Wheel>,
}

/// Current spread cone (radians) for the HUD crosshair and for firing.
pub fn current_spread(st: &WeaponStats, ars: &Arsenal, aiming: bool, speed: f32, airborne: bool) -> f32 {
    let base = if aiming { st.aim_spread } else { st.spread };
    let moving = (speed / 7.0).clamp(0.0, 1.0) * if aiming { 0.02 } else { 0.035 };
    base + ars.bloom + moving + if airborne { 0.05 } else { 0.0 }
}

fn random_in_cone(rng: &mut Rng, dir: Vec3, cone: f32) -> Vec3 {
    if cone <= 0.0 {
        return dir;
    }
    let right = dir.cross(Vec3::Y).normalize_or(Vec3::X);
    let up = right.cross(dir).normalize();
    // uniform over the disc, slightly centre-weighted
    let r = cone * rng.f().powf(0.7);
    let a = rng.f() * TAU;
    (dir + right * (a.cos() * r) + up * (a.sin() * r)).normalize()
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn fire_weapons(
    mut commands: Commands,
    time: Res<Time>,
    input: Res<GameInput>,
    mut ars: ResMut<Arsenal>,
    mut rig: ResMut<CameraRig>,
    player: Query<(Entity, &Player, &Transform), Without<MainCamera>>,
    cam: Query<&GlobalTransform, With<MainCamera>>,
    mut rigs: Query<&mut Rig>,
    rapier: ReadRapierContext,
    mut peds: Query<(&mut Ped, &Transform), (Without<Player>, Without<MainCamera>)>,
    mut cars: Query<&mut Vehicle>,
    bodies: Query<(&ReadMassProperties, &GlobalTransform), With<RigidBody>>,
    mut impulses: Query<&mut ExternalImpulse>,
    mut light: Query<(&mut PointLight, &mut Transform, &mut Visibility), (With<MuzzleLight>, Without<Player>, Without<Ped>)>,
    mut res: FireRes,
) {
    let FireRes { wa, fx, decals, shake, sfx, marker, wheel } = &mut res;
    let dt = time.delta_secs();
    ars.cooldown -= dt;
    let Ok((pe, p, pt)) = player.single() else { return };
    let weapon = ars.weapon();
    let st = weapon.stats();
    let on_foot = p.vehicle.is_none() && p.health > 0.0;
    let aiming = on_foot && input.aim && !st.melee() && !wheel.open;
    rig.aim = aiming;
    rig.aim_fov = st.aim_fov;
    if let Ok(mut r) = rigs.get_mut(p.visual) {
        r.aim = on_foot && (input.aim || ars.cooldown > -0.5) && !st.melee() && st.kind != FireKind::Thrown;
        r.two_handed = st.two_handed;
    }
    // bloom settles quickly once you stop firing
    let settle = if aiming { 0.35 } else { 0.22 };
    ars.bloom = (ars.bloom - dt * settle).max(0.0);
    // the minigun has to spin up
    if st.spin_up > 0.0 {
        let spinning = on_foot && input.fire && !wheel.open && ars.reloading <= 0.0;
        ars.spin = (ars.spin + if spinning { dt / st.spin_up } else { -dt / (st.spin_up * 1.5) }).clamp(0.0, 1.0);
    }
    // reload in progress
    if ars.reloading > 0.0 {
        ars.reloading -= dt;
        if ars.reloading <= 0.0 {
            let i = ars.current;
            let (clip, res) = ars.ammo[i];
            let take = (st.clip - clip).min(res);
            ars.ammo[i] = (clip + take, res - take);
        }
        return;
    }
    if !on_foot || wheel.open {
        return;
    }
    let trigger = if st.auto { input.fire } else { input.fire_pressed };
    if !trigger || ars.cooldown > 0.0 || (st.spin_up > 0.0 && ars.spin < 1.0) {
        return;
    }
    let i = ars.current;
    if st.clip > 0 {
        let (clip, res) = ars.ammo[i];
        if clip == 0 {
            if res > 0 {
                ars.reloading = st.reload;
            } else if st.kind == FireKind::Thrown || input.fire_pressed {
                sfx.write(crate::audio::Sfx { kind: crate::audio::SfxKind::Ricochet, pos: pt.translation, volume: 0.25 });
                ars.cooldown = 0.3;
            }
            return;
        }
        ars.ammo[i].0 -= 1;
    }
    ars.cooldown = st.interval;
    let Ok(cgt) = cam.single() else { return };
    let Ok(ctx) = rapier.single() else { return };
    let fwd = cgt.forward().as_vec3();
    let facing = Quat::from_rotation_y((-fwd.x).atan2(-fwd.z));
    let barrel = match weapon {
        Weapon::Sniper => 1.25,
        Weapon::Rifle | Weapon::Minigun | Weapon::Shotgun => 0.85,
        Weapon::Rocket => 1.1,
        _ => 0.3,
    };
    let muzzle = pt.translation + Vec3::Y * 0.55 + facing * Vec3::new(0.22, 0.0, -0.35 - barrel * 0.6);

    {
        use crate::audio::SfxKind as K;
        let (kind, vol) = match weapon {
            Weapon::Fists | Weapon::Bat => (K::Punch, 0.5),
            Weapon::Pistol => (K::Pistol, 0.9),
            Weapon::Revolver => (K::Shotgun, 0.8),
            Weapon::Smg | Weapon::Minigun => (K::Smg, 0.9),
            Weapon::Rifle => (K::Smg, 1.0),
            Weapon::Shotgun | Weapon::Sniper => (K::Shotgun, 1.0),
            Weapon::Rocket => (K::Rocket, 0.9),
            Weapon::Grenade => (K::Punch, 0.2),
        };
        sfx.write(crate::audio::Sfx { kind, pos: pt.translation, volume: vol });
    }
    // ------------------------------------------------------------ melee
    if st.melee() {
        if let Ok(mut r) = rigs.get_mut(p.visual) {
            r.punch = 1.0;
        }
        // swing toward where the camera looks, hit the closest person in front
        let face = Quat::from_rotation_y((-fwd.x).atan2(-fwd.z)) * Vec3::NEG_Z;
        let mut target: Option<(f32, Vec3)> = None;
        for (ped, t) in peds.iter() {
            if ped.health <= 0.0 {
                continue;
            }
            let d = t.translation - pt.translation;
            let flat = d.with_y(0.0);
            if flat.length() < st.range && d.y.abs() < 1.5 && flat.normalize_or_zero().dot(face) > 0.35 && target.is_none_or(|(bd, _)| flat.length() < bd) {
                target = Some((flat.length(), t.translation));
            }
        }
        if let Some((_, tp)) = target {
            for (mut ped, t) in &mut peds {
                if t.translation != tp {
                    continue;
                }
                let dir = (t.translation - pt.translation).with_y(0.0).normalize_or_zero();
                let was_cop = ped.cop;
                hurt_ped(&mut ped, st.damage, dir * (st.force * 0.012) + Vec3::Y * (st.force * 0.004));
                if weapon == Weapon::Bat && ped.health > 0.0 {
                    ped.knock_t = 1.6;
                }
                if let Some(v) = ped.visual {
                    if let Ok(mut r) = rigs.get_mut(v) {
                        r.hit = 1.0;
                    }
                }
                let kill = ped.health <= 0.0;
                marker.t = 0.25;
                marker.kill = kill;
                marker.hits += 1;
                if kill {
                    marker.kills += 1;
                }
                shake.trauma = (shake.trauma + 0.15).min(0.5);
                let crime = if kill { if was_cop { Crime::KillCop } else { Crime::Murder } } else if was_cop { Crime::ShootCop } else { Crime::Assault };
                report_crime(&mut commands, crime, pt.translation);
                break;
            }
        }
        return;
    }
    // ------------------------------------------------------------ recoil
    let kick_scale = if aiming { 0.75 } else { 1.0 };
    let kick = st.kick * kick_scale * ars.rng.range(0.8, 1.2);
    rig.pitch += kick;
    rig.yaw += ars.rng.range(-1.0, 1.0) * st.kick_yaw * kick_scale;
    // most of the climb comes back down on its own
    rig.recoil += kick * 0.7;
    shake.trauma = (shake.trauma + st.kick * 1.5).min(0.45);
    // aim point through the screen centre, starting level with the player so
    // nothing behind you gets hit
    let start = cgt.translation() + fwd * (pt.translation - cgt.translation()).dot(fwd).max(0.0);
    let speed = p.vel.with_y(0.0).length();
    let cone = current_spread(&st, &ars, aiming, speed, !p.grounded);
    ars.bloom = (ars.bloom + st.bloom).min(st.bloom_max);
    let center_aim = ctx
        .cast_ray(start, fwd, st.range, true, QueryFilter::default().exclude_collider(pe).exclude_sensors())
        .map(|(_, toi)| start + fwd * toi)
        .unwrap_or(start + fwd * st.range);
    if st.kind == FireKind::Rocket || st.kind == FireKind::Thrown {
        let dir = (center_aim - muzzle).normalize_or(fwd);
        if st.kind == FireKind::Rocket {
            let em = commands.spawn((Transform::from_xyz(0.0, 0.0, 0.35), crate::fx::Emitter { kind: crate::fx::EmitterKind::Smoke, rate: 60.0, acc: 0.0 })).id();
            commands
                .spawn((Mesh3d(wa.rocket_mesh.clone()), MeshMaterial3d(wa.gun_mat.clone()), Transform::from_translation(muzzle).looking_to(dir, Vec3::Y), Rocket { vel: dir * 70.0, life: 6.0 }, NoRaytrace))
                .add_child(em);
            report_crime(&mut commands, Crime::Gunfire, pt.translation);
        } else {
            // lob it: aim a bit high, inherit your running speed
            if let Ok(mut r) = rigs.get_mut(p.visual) {
                r.punch = 1.0;
            }
            let throw = (fwd + Vec3::Y * 0.28).normalize() * 17.0 + p.vel.with_y(0.0) * 0.6;
            let from = pt.translation + Vec3::Y * 0.7 + facing * Vec3::new(0.25, 0.0, -0.4);
            commands.spawn((Mesh3d(wa.grenade_mesh.clone()), MeshMaterial3d(wa.gun_mat.clone()), Transform::from_translation(from), Grenade { vel: throw, fuse: 2.8 }, NoRaytrace));
            // grenades come one at a time out of the reserve
            ars.reloading = st.reload;
        }
        return;
    }
    // ------------------------------------------------------------ hitscan
    commands.spawn((Mesh3d(fx.sphere.clone()), MeshMaterial3d(wa.flash_mat.clone()), Transform::from_translation(muzzle).with_scale(Vec3::splat(if st.two_handed { 0.16 } else { 0.11 })), Transient(0.035), NoRaytrace));
    if let Ok((mut l, mut lt, mut lv)) = light.single_mut() {
        l.intensity = 1.6e6;
        lt.translation = muzzle;
        *lv = Visibility::Inherited;
    }
    // brass
    if weapon != Weapon::Shotgun || ars.rng.chance(0.3) {
        let r = &mut ars.rng;
        let side = facing * Vec3::X;
        let v = side * r.range(2.0, 3.5) + Vec3::Y * r.range(1.5, 3.0) + p.vel * 0.8;
        particle(&mut commands, fx, wa.brass_mat.clone(), muzzle - facing * Vec3::NEG_Z * (barrel * 0.4), Particle { vel: v, life: 0.6, max: 0.6, grow: 0.0, drag: 0.3, buoy: -12.0, size: 0.008, shrink: false });
    }
    let base_dir = (center_aim - start).normalize_or(fwd);
    let mut killed_any = false;
    let mut hit_any = false;
    for pellet in 0..st.pellets {
        let dir = random_in_cone(&mut ars.rng, base_dir, cone);
        // from the camera line (what you aimed at, peds are sensors so include them)…
        let filter = QueryFilter::default().exclude_collider(pe);
        let cam_hit = ctx.cast_ray_and_get_normal(start, dir, st.range, true, filter);
        let mut end = cam_hit.map(|(_, h)| h.point).unwrap_or(start + dir * st.range);
        let mut normal = cam_hit.map(|(_, h)| h.normal).unwrap_or(Vec3::Y);
        let mut target = cam_hit.map(|(e, _)| e);
        // …then from the muzzle, so a wall between the gun and the target stops it
        let md = end - muzzle;
        if md.length() > 0.3 {
            if let Some((e, h)) = ctx.cast_ray_and_get_normal(muzzle, md.normalize(), md.length() - 0.05, true, filter) {
                end = h.point;
                normal = h.normal;
                target = Some(e);
            }
        }
        let travel = (end - muzzle).normalize_or(dir);
        // tracers: every bullet for most guns, every other one for fast spitters
        let len = muzzle.distance(end);
        if len > 0.5 && (st.interval > 0.06 || pellet % 2 == 0 || ars.rng.chance(0.5)) && (st.pellets == 1 || pellet % 3 == 0) {
            commands.spawn((
                Mesh3d(wa.tracer_mesh.clone()),
                MeshMaterial3d(wa.tracer_mat.clone()),
                Transform::from_translation(muzzle).looking_at(end, Vec3::Y).with_scale(Vec3::new(1.0, 1.0, len)),
                Transient(0.05),
                NoRaytrace,
            ));
        }
        let Some(target) = target else { continue };
        // damage falls off past half range
        let dist = start.distance(end);
        let falloff = 1.0 - ((dist / st.range - 0.5) * 2.0).clamp(0.0, 1.0) * 0.5;
        if let Ok((mut ped, ptf)) = peds.get_mut(target) {
            if ped.health <= 0.0 {
                // already a ragdoll capsule; push the body parts below
            } else {
                let was_cop = ped.cop;
                // the capsule centre is at chest height; the top of it is the head
                let headshot = end.y - ptf.translation.y > 0.55;
                let dmg = st.damage * falloff * if headshot { st.head } else { 1.0 };
                hurt_ped(&mut ped, dmg, travel * (st.force * 0.004));
                let kill = ped.health <= 0.0;
                if kill {
                    // the body goes down along the shot
                    let fling = (st.force * 0.012 * if st.pellets > 1 { 0.35 } else { 1.0 }).min(9.0);
                    ped.vel += travel.with_y(0.0) * fling + Vec3::Y * (fling * 0.2);
                    killed_any = true;
                }
                if let Some(v) = ped.visual {
                    if let Ok(mut r) = rigs.get_mut(v) {
                        r.hit = 1.0;
                    }
                }
                hit_any = true;
                for k in 0..(if headshot { 7 } else { 4 }) {
                    let r = &mut ars.rng;
                    let v = (travel + Vec3::new(r.range(-0.5, 0.5), r.range(-0.2, 0.6), r.range(-0.5, 0.5))).normalize() * r.range(1.5, 4.0);
                    let life = 0.35 + k as f32 * 0.04;
                    particle(&mut commands, fx, wa.blood_mat.clone(), end, Particle { vel: v, life, max: life, grow: 0.3, drag: 1.5, buoy: -9.0, size: 0.035, shrink: true });
                }
                let crime = if kill { if was_cop { Crime::KillCop } else { Crime::Murder } } else if was_cop { Crime::ShootCop } else { Crime::Assault };
                report_crime(&mut commands, crime, pt.translation);
                continue;
            }
        }
        let mut metal = false;
        if let Ok(mut v) = cars.get_mut(target) {
            v.health -= st.damage * 2.0 * falloff;
            v.last_hit_by_player = true;
            metal = true;
            if matches!(v.driver, crate::vehicle::Driver::Cop(_)) {
                report_crime(&mut commands, Crime::ShootCop, pt.translation);
            }
        }
        if let (Ok((mp, gt)), Ok(mut imp)) = (bodies.get(target), impulses.get_mut(target)) {
            let com = gt.transform_point(mp.get().local_center_of_mass);
            let add = ExternalImpulse::at_point(travel * st.force, end, com);
            imp.impulse += add.impulse;
            imp.torque_impulse += add.torque_impulse;
        }
        // sparks off metal, dust off everything else, and a bullet hole
        let r = &mut ars.rng;
        if metal || r.chance(0.5) {
            for k in 0..4 {
                let v = (normal + Vec3::new(r.range(-0.6, 0.6), r.range(-0.6, 0.6), r.range(-0.6, 0.6))).normalize() * r.range(3.0, 7.0);
                let life = 0.2 + k as f32 * 0.05;
                particle(&mut commands, fx, fx.spark.clone(), end, Particle { vel: v, life, max: life, grow: 0.0, drag: 1.0, buoy: -9.0, size: 0.022, shrink: true });
            }
        }
        if !metal {
            let v = normal * r.range(0.6, 1.2);
            particle(&mut commands, fx, wa.dust_mat.clone(), end + normal * 0.05, Particle { vel: v, life: 0.7, max: 0.7, grow: 1.6, drag: 2.0, buoy: 0.3, size: 0.07, shrink: false });
        }
        let d = commands
            .spawn((
                Mesh3d(wa.decal_mesh.clone()),
                MeshMaterial3d(wa.decal_mat.clone()),
                Transform::from_translation(end + normal * 0.01).looking_to(-normal, if normal.y.abs() > 0.9 { Vec3::X } else { Vec3::Y }).with_scale(Vec3::splat(if st.damage > 80.0 { 1.6 } else { 1.0 })),
                NoRaytrace,
            ))
            .id();
        decals.0.push_back(d);
        while decals.0.len() > 250 {
            if let Some(old) = decals.0.pop_front() {
                commands.entity(old).despawn();
            }
        }
    }
    if hit_any {
        marker.t = 0.22;
        marker.kill = killed_any;
        marker.hits += 1;
        if killed_any {
            marker.kills += 1;
        }
    }
    report_crime(&mut commands, Crime::Gunfire, pt.translation);
}

pub fn hurt_ped(ped: &mut Ped, dmg: f32, push: Vec3) {
    ped.health -= dmg;
    ped.vel += push;
    if ped.health <= 0.0 {
        ped.knock_t = 3.0;
    } else if !ped.cop {
        // some people don't take it lying down
        if ped.gang || ped.seed % 10 < 3 {
            ped.state = PedState::Fight { t: 30.0 };
        } else if !matches!(ped.state, PedState::Fight { .. }) {
            ped.state = PedState::Flee { from: Vec3::ZERO, t: 10.0 };
            ped.flee_from_player = true;
        }
    }
}

fn muzzle_light(time: Res<Time>, mut q: Query<(&mut PointLight, &mut Visibility), With<MuzzleLight>>) {
    let Ok((mut l, mut v)) = q.single_mut() else { return };
    if l.intensity > 0.0 {
        l.intensity *= (-time.delta_secs() * 45.0).exp();
        if l.intensity < 2.0e4 {
            l.intensity = 0.0;
            *v = Visibility::Hidden;
        }
    }
}

fn update_rockets(mut commands: Commands, time: Res<Time>, mut rockets: Query<(Entity, &mut Rocket, &mut Transform)>, rapier: ReadRapierContext, player: Query<Entity, With<Player>>, mut ex: MessageWriter<Explosion>) {
    let dt = time.delta_secs();
    let Ok(ctx) = rapier.single() else { return };
    let pe = player.single().ok();
    for (e, mut r, mut t) in &mut rockets {
        r.life -= dt;
        r.vel.y -= 1.5 * dt;
        let step = r.vel * dt;
        let mut filter = QueryFilter::default().exclude_sensors();
        if let Some(pe) = pe {
            filter = filter.exclude_collider(pe);
        }
        let hit = ctx.cast_ray(t.translation, step.normalize_or(Vec3::Z), step.length(), true, filter);
        if let Some((_, toi)) = hit {
            let pos = t.translation + step.normalize_or(Vec3::Z) * toi;
            ex.write(Explosion { pos, radius: 9.0, force: 2.5, damage: 1200.0, by_player: true });
            commands.entity(e).despawn();
            continue;
        }
        if r.life <= 0.0 {
            ex.write(Explosion { pos: t.translation, radius: 9.0, force: 2.5, damage: 1200.0, by_player: true });
            commands.entity(e).despawn();
            continue;
        }
        t.translation += step;
        let dir = r.vel.normalize_or(Vec3::Z);
        t.look_to(dir, Vec3::Y);
    }
}

/// Grenades: ballistic, bounce off whatever they hit, roll to a stop, then blow.
fn update_grenades(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Grenade, &mut Transform)>, rapier: ReadRapierContext, player: Query<Entity, With<Player>>, mut ex: MessageWriter<Explosion>, mut sfx: MessageWriter<crate::audio::Sfx>) {
    let dt = time.delta_secs();
    let Ok(ctx) = rapier.single() else { return };
    let mut filter = QueryFilter::default().exclude_sensors();
    if let Ok(pe) = player.single() {
        filter = filter.exclude_collider(pe);
    }
    for (e, mut g, mut t) in &mut q {
        g.fuse -= dt;
        if g.fuse <= 0.0 {
            ex.write(Explosion { pos: t.translation, radius: 7.5, force: 2.2, damage: 900.0, by_player: true });
            commands.entity(e).despawn();
            continue;
        }
        g.vel.y -= 16.0 * dt;
        let step = g.vel * dt;
        let len = step.length();
        if len < 1e-5 {
            continue;
        }
        let dir = step / len;
        if let Some((_, h)) = ctx.cast_ray_and_get_normal(t.translation, dir, len + 0.07, true, filter) {
            // reflect, lose energy, rub along the ground
            let n = h.normal;
            let vn = g.vel.dot(n);
            if vn < -2.5 {
                sfx.write(crate::audio::Sfx { kind: crate::audio::SfxKind::Ricochet, pos: h.point, volume: 0.3 });
            }
            g.vel = (g.vel - n * vn * 1.4) * 0.6;
            if n.y > 0.6 && g.vel.length() < 1.0 {
                g.vel = Vec3::ZERO;
            }
            t.translation = h.point + n * 0.07;
        } else {
            t.translation += step;
        }
        let spin = g.vel.length() * dt * 3.0;
        t.rotate_local_x(spin);
    }
}

fn update_transients(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Transient)>) {
    for (e, mut tr) in &mut q {
        tr.0 -= time.delta_secs();
        if tr.0 <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

/// Keep the gun model in the player's right hand in sync with the selected weapon.
fn attach_weapon_model(mut commands: Commands, mut ars: ResMut<Arsenal>, wa: Res<WeaponAssets>, player: Query<&Player>, rigs: Query<&Rig>, models: Query<Entity, With<WeaponModel>>) {
    let Ok(p) = player.single() else { return };
    let want = if p.vehicle.is_some() { None } else { Some(ars.current) };
    if ars.shown == want && !models.is_empty() == want.is_some_and(|w| wa.meshes[w].is_some()) {
        return;
    }
    for m in &models {
        commands.entity(m).despawn();
    }
    ars.shown = want;
    let Some(w) = want else { return };
    let Some(mesh) = wa.meshes[w].clone() else { return };
    let Ok(rig) = rigs.get(p.visual) else { return };
    let Some(hand) = rig.hand_r else { return };
    let mat = if WEAPONS[w] == Weapon::Bat { wa.wood_mat.clone() } else { wa.gun_mat.clone() };
    let m = commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), Transform::from_xyz(0.0, -0.03, 0.0).with_rotation(Quat::from_rotation_x(-FRAC_PI_2)), WeaponModel)).id();
    commands.entity(hand).add_child(m);
}

// ------------------------------------------------------------------------------------------------
// UI: dynamic crosshair, hit markers, sniper scope, the weapon wheel
// ------------------------------------------------------------------------------------------------

#[derive(Component)]
struct CrossTick(usize);
#[derive(Component)]
struct HitTick(usize);
#[derive(Component)]
struct ScopeOverlay;
#[derive(Component)]
struct WheelRoot;
#[derive(Component)]
struct WheelSlot(usize);
#[derive(Component)]
struct WheelSlotText(usize);
#[derive(Component)]
struct WheelCenterText;

const WHEEL_RX: f32 = 340.0;
const WHEEL_RY: f32 = 235.0;
const SLOT_W: f32 = 150.0;
const SLOT_H: f32 = 64.0;

fn setup_weapon_ui(mut commands: Commands, fonts: Res<HudFonts>) {
    // crosshair ticks + hit marker, centred
    let root = commands
        .spawn((Node { position_type: PositionType::Absolute, left: percent(50), top: percent(50), width: px(0), height: px(0), ..default() }, bevy::picking::Pickable::IGNORE))
        .id();
    for k in 0..4 {
        let (w, h) = if k % 2 == 0 { (2.0, 9.0) } else { (9.0, 2.0) };
        let tick = commands
            .spawn((
                Node { position_type: PositionType::Absolute, width: px(w), height: px(h), ..default() },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
                BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.6), px(0), px(0), px(1), px(1)),
                Visibility::Hidden,
                CrossTick(k),
            ))
            .id();
        commands.entity(root).add_child(tick);
    }
    for k in 0..4 {
        let tick = commands
            .spawn((
                Node { position_type: PositionType::Absolute, width: px(12), height: px(3), left: px(0), top: px(0), ..default() },
                BackgroundColor(Color::WHITE),
                UiTransform { rotation: Rot2::radians(PI * 0.25 + k as f32 * FRAC_PI_2), ..default() },
                Visibility::Hidden,
                HitTick(k),
            ))
            .id();
        commands.entity(root).add_child(tick);
    }
    // sniper scope: dark mask around a square view + thin reticle
    commands
        .spawn((Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), justify_content: JustifyContent::Center, ..default() }, Visibility::Hidden, ScopeOverlay, GlobalZIndex(5), bevy::picking::Pickable::IGNORE))
        .with_children(|c| {
            let dark = Color::BLACK;
            c.spawn((Node { flex_grow: 1.0, height: percent(100), ..default() }, BackgroundColor(dark)));
            c.spawn((Node { width: Val::Vh(92.0), height: percent(100), flex_direction: FlexDirection::Column, ..default() }, BorderColor::all(Color::BLACK))).with_children(|m| {
                m.spawn((Node { width: percent(100), height: Val::Vh(4.0), ..default() }, BackgroundColor(dark)));
                m.spawn((Node { width: percent(100), flex_grow: 1.0, border: UiRect::all(px(3)), border_radius: BorderRadius::all(percent(50)), ..default() }, BorderColor::all(Color::BLACK))).with_children(|v| {
                    v.spawn((Node { position_type: PositionType::Absolute, left: percent(50), top: percent(0), width: px(1), height: percent(100), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.85))));
                    v.spawn((Node { position_type: PositionType::Absolute, top: percent(50), left: percent(0), height: px(1), width: percent(100), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.85))));
                });
                m.spawn((Node { width: percent(100), height: Val::Vh(4.0), ..default() }, BackgroundColor(dark)));
            });
            c.spawn((Node { flex_grow: 1.0, height: percent(100), ..default() }, BackgroundColor(dark)));
        });
    // weapon wheel
    let wheel = commands
        .spawn((
            Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            Visibility::Hidden,
            GlobalZIndex(20),
            WheelRoot,
        ))
        .id();
    let center = commands
        .spawn(Node { position_type: PositionType::Absolute, left: percent(50), top: percent(50), width: px(0), height: px(0), ..default() })
        .id();
    commands.entity(wheel).add_child(center);
    let n = WHEEL_ORDER.len();
    for (k, _) in WHEEL_ORDER.iter().enumerate() {
        let a = k as f32 / n as f32 * TAU;
        let (x, y) = (a.sin() * WHEEL_RX - SLOT_W * 0.5, -a.cos() * WHEEL_RY - SLOT_H * 0.5);
        let slot = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(x),
                    top: px(y),
                    width: px(SLOT_W),
                    height: px(SLOT_H),
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::all(px(10)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.05, 0.06, 0.8)),
                BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.15)),
                WheelSlot(k),
            ))
            .with_children(|s| {
                s.spawn((Text::new(""), font(&fonts, 17.0, 700), TextColor(Color::WHITE), TextLayout::justify(Justify::Center), WheelSlotText(k)));
            })
            .id();
        commands.entity(center).add_child(slot);
    }
    let txt = commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: px(-150), top: px(-60), width: px(300), height: px(120), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
        ))
        .with_children(|c| {
            c.spawn((Text::new(""), font(&fonts, 24.0, 800), TextColor(Color::srgb(1.0, 0.92, 0.7)), shadow(), TextLayout::justify(Justify::Center), WheelCenterText));
        })
        .id();
    commands.entity(center).add_child(txt);
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn weapon_ui(
    time: Res<Time<bevy::time::Real>>,
    ars: Res<Arsenal>,
    rig: Res<CameraRig>,
    wheel: Res<Wheel>,
    mut marker: ResMut<HitMarker>,
    player: Query<&Player>,
    windows: Query<&Window>,
    mut ticks: Query<(&CrossTick, &mut Node, &mut Visibility), (Without<HitTick>, Without<WheelSlot>)>,
    mut hits: Query<(&HitTick, &mut Node, &mut Visibility, &mut BackgroundColor), (Without<CrossTick>, Without<WheelSlot>)>,
    mut scope: Query<&mut Visibility, (With<ScopeOverlay>, Without<CrossTick>, Without<HitTick>, Without<WheelRoot>)>,
    mut wroot: Query<&mut Visibility, (With<WheelRoot>, Without<CrossTick>, Without<HitTick>, Without<ScopeOverlay>)>,
    mut slots: Query<(&WheelSlot, &mut BackgroundColor, &mut BorderColor), (Without<HitTick>, Without<CrossTick>)>,
    mut texts: ParamSet<(Query<(&WheelSlotText, &mut Text, &mut TextColor)>, Query<&mut Text, With<WheelCenterText>>)>,
    mut bodies: Query<&mut Visibility, (With<Rig>, Without<CrossTick>, Without<HitTick>, Without<ScopeOverlay>, Without<WheelRoot>)>,
) {
    let dt = time.delta_secs();
    let Ok(p) = player.single() else { return };
    let st = ars.weapon().stats();
    let on_foot = p.vehicle.is_none() && p.health > 0.0;
    let scoped = rig.aim && st.aim_fov < 20.0 && rig.fov < 24.0;
    let show_cross = on_foot && !st.melee() && !scoped && !wheel.open && (rig.aim || ars.cooldown > -0.6);
    // tick gap from the actual spread cone in screen pixels
    let h = windows.single().map(|w| w.height()).unwrap_or(900.0);
    let cone = current_spread(&st, &ars, rig.aim, p.vel.with_y(0.0).length(), !p.grounded);
    let gap = (cone.tan() / (rig.fov.to_radians() * 0.5).tan() * h * 0.5).clamp(4.0, 160.0);
    for (t, mut n, mut v) in &mut ticks {
        let want = if show_cross && st.kind == FireKind::Hitscan { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
        let (x, y) = match t.0 {
            0 => (-1.0, -gap - 9.0),
            1 => (gap, -1.0),
            2 => (-1.0, gap),
            _ => (-gap - 9.0, -1.0),
        };
        n.left = px(x);
        n.top = px(y);
    }
    marker.t = (marker.t - dt).max(0.0);
    for (t, mut n, mut v, mut bg) in &mut hits {
        let on = marker.t > 0.0 && on_foot;
        let want = if on { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
        if on {
            let a = PI * 0.25 + t.0 as f32 * FRAC_PI_2;
            let r = 10.0 + (0.22 - marker.t).max(0.0) * 20.0;
            n.left = px(a.cos() * r - 6.0);
            n.top = px(a.sin() * r - 1.5);
            bg.0 = if marker.kill { Color::srgb(1.0, 0.2, 0.15) } else { Color::WHITE };
        }
    }
    if let Ok(mut v) = bodies.get_mut(p.visual) {
        let want = if scoped && on_foot { Visibility::Hidden } else { Visibility::Inherited };
        if *v != want {
            *v = want;
        }
    }
    if let Ok(mut v) = scope.single_mut() {
        let want = if scoped && on_foot { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    if let Ok(mut v) = wroot.single_mut() {
        let want = if wheel.open { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    if !wheel.open {
        return;
    }
    for (s, mut bg, mut border) in &mut slots {
        let i = WHEEL_ORDER[s.0].index();
        let owned = ars.has(i);
        let hover = wheel.hover == Some(s.0);
        let current = i == ars.current;
        bg.0 = if hover {
            Color::srgba(0.95, 0.7, 0.2, 0.9)
        } else if owned {
            Color::srgba(0.08, 0.08, 0.1, 0.85)
        } else {
            Color::srgba(0.05, 0.05, 0.05, 0.45)
        };
        *border = BorderColor::all(if current { Color::WHITE } else { Color::srgba(1.0, 1.0, 1.0, 0.15) });
    }
    for (s, mut t, mut c) in &mut texts.p0() {
        let w = WHEEL_ORDER[s.0];
        let i = w.index();
        let st = w.stats();
        let owned = ars.has(i);
        let ammo = if st.melee() {
            String::new()
        } else if owned {
            format!("\n{}", ars.ammo[i].0 + ars.ammo[i].1)
        } else {
            "\n-".into()
        };
        t.0 = format!("{}{}", st.name, ammo);
        c.0 = if wheel.hover == Some(s.0) {
            Color::BLACK
        } else if owned {
            Color::WHITE
        } else {
            Color::srgba(1.0, 1.0, 1.0, 0.35)
        };
    }
    if let Ok(mut t) = texts.p1().single_mut() {
        t.0 = match wheel.hover {
            Some(h) => {
                let w = WHEEL_ORDER[h];
                let st = w.stats();
                let owned = ars.has(w.index());
                format!("{}\n{}", st.name.to_uppercase(), if owned { st.blurb } else { "not found yet" })
            }
            None => String::new(),
        };
    }
}
