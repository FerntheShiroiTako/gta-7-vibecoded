//! Street-level side systems:
//! - pickups (health, body armour, cash, guns) that bob and spin, respawn a
//!   while after you take them, and get dropped by the dead;
//! - Spray & Go respray garages: drive in, the door rolls down, and you come out
//!   repaired, repainted and off the police radar ($100);
//! - stunt ramps: slow motion over the jump, a bonus for each ramp the first
//!   time, and airtime rewards (distance, height, flips, rolls, spins) for any
//!   big jump.

use std::f32::consts::TAU;

use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::audio::{Sfx, SfxKind};
use crate::city::CityInfo;
use crate::hud::HudMessages;
use crate::lighting::NoRaytrace;
use crate::map::MapBlip;
use crate::meshgen::{MeshBuilder, Rng, WHITE};
use crate::player::Player;
use crate::seats::{Seat, SeatKind};
use crate::vehicle::Vehicle;
use crate::wanted::Wanted;
use crate::weapons::{Arsenal, WEAPONS};
use crate::world::*;

pub struct ActivitiesPlugin;

impl Plugin for ActivitiesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Drops>()
            .init_resource::<GlowMats>()
            .init_resource::<StuntState>()
            .init_resource::<ResprayState>()
            .add_systems(Startup, (setup_pickups, setup_respray, setup_ramps).after(crate::city::setup_city))
            .add_systems(Update, (spawn_drops, animate_pickups, collect_pickups, respawn_pickups).chain())
            .add_systems(Update, (respray, stunts, scale_glow));
    }
}

/// Additive glow materials (markers, halos). Emissive isn't exposure-scaled, so
/// they'd look washed out against a sunlit street: give them a boost by day.
#[derive(Resource, Default)]
pub struct GlowMats(pub Vec<(Handle<StandardMaterial>, LinearRgba)>);

fn scale_glow(cam: Query<&bevy::camera::Exposure, With<crate::camera::MainCamera>>, glow: Res<GlowMats>, mut materials: ResMut<Assets<StandardMaterial>>, mut last: Local<f32>) {
    let ev = cam.single().map(|e| e.ev100).unwrap_or(13.2);
    if (ev - *last).abs() < 0.05 && !glow.is_changed() {
        return;
    }
    *last = ev;
    let k = 1.0 + ((ev - 6.2) / 7.0).clamp(0.0, 1.0) * 1.5;
    for (h, base) in &glow.0 {
        if let Some(mut m) = materials.get_mut(h) {
            m.emissive = *base * k;
        }
    }
}

pub fn glow_material(materials: &mut Assets<StandardMaterial>, glow: &mut GlowMats, c: LinearRgba) -> Handle<StandardMaterial> {
    let h = unlit_add(materials, c);
    glow.0.push((h.clone(), c));
    h
}

fn unlit_add(materials: &mut Assets<StandardMaterial>, c: LinearRgba) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: c,
        perceptual_roughness: 1.0,
        reflectance: 0.0,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..default()
    })
}

// ================================================================================================
// Pickups
// ================================================================================================

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PickupKind {
    Health,
    Armor,
    Cash(i64),
    /// weapon slot (1 = pistol .. 4 = rocket launcher)
    Weapon(usize),
}

/// Pickups other systems want dropped this frame (dead peds, cops).
#[derive(Resource, Default)]
pub struct Drops(pub Vec<(Vec3, PickupKind)>);

#[derive(Component)]
pub struct Pickup {
    pub kind: PickupKind,
    slot: Option<usize>,
    base: Vec3,
    phase: f32,
    /// dropped pickups fade out after a while
    life: Option<f32>,
}

#[derive(Resource)]
struct PickupAssets {
    /// per model id: (mesh, material) parts
    models: Vec<Vec<(Handle<Mesh>, Handle<StandardMaterial>)>>,
    halo: Handle<Mesh>,
    halo_mats: [Handle<StandardMaterial>; 4],
}

#[derive(Resource)]
struct PickupSlots {
    defs: Vec<(Vec3, PickupKind)>,
    timers: Vec<f32>,
    alive: Vec<bool>,
}

const RESPAWN_S: f32 = 75.0;

fn model_id(k: PickupKind) -> usize {
    match k {
        PickupKind::Health => 0,
        PickupKind::Armor => 1,
        PickupKind::Cash(_) => 2,
        PickupKind::Weapon(i) => 3 + i.min(crate::weapons::WEAPON_COUNT - 1),
    }
}

fn halo_id(k: PickupKind) -> usize {
    match k {
        PickupKind::Health => 0,
        PickupKind::Armor => 1,
        PickupKind::Cash(_) => 2,
        PickupKind::Weapon(_) => 3,
    }
}

fn build_pickup_models(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Vec<Vec<(Handle<Mesh>, Handle<StandardMaterial>)>> {
    let pbr = |materials: &mut Assets<StandardMaterial>, c: Color, rough: f32, metal: f32, glow: LinearRgba| {
        materials.add(StandardMaterial { base_color: c, perceptual_roughness: rough, metallic: metal, emissive: glow, ..default() })
    };
    let white = pbr(materials, Color::srgb(0.92, 0.92, 0.9), 0.4, 0.0, LinearRgba::BLACK);
    let red = pbr(materials, Color::srgb(0.9, 0.05, 0.05), 0.4, 0.0, LinearRgba::rgb(6.0, 0.2, 0.2));
    let blue = pbr(materials, Color::srgb(0.1, 0.2, 0.55), 0.6, 0.0, LinearRgba::rgb(0.05, 0.1, 0.4));
    let navy = pbr(materials, Color::srgb(0.04, 0.06, 0.15), 0.5, 0.1, LinearRgba::BLACK);
    let green = pbr(materials, Color::srgb(0.32, 0.6, 0.3), 0.7, 0.0, LinearRgba::rgb(0.1, 0.4, 0.1));
    let band = pbr(materials, Color::srgb(0.95, 0.9, 0.6), 0.5, 0.0, LinearRgba::BLACK);
    let gun = pbr(materials, Color::srgb(0.07, 0.07, 0.075), 0.35, 0.8, LinearRgba::BLACK);
    let olive = pbr(materials, Color::srgb(0.25, 0.3, 0.15), 0.6, 0.2, LinearRgba::BLACK);
    let mut add = |mb: MeshBuilder, m: &Handle<StandardMaterial>| (meshes.add(mb.build()), m.clone());
    let boxes = |parts: &[(Vec3, Vec3)]| {
        let mut mb = MeshBuilder::new();
        for (c, h) in parts {
            mb.box_at(Affine3A::from_translation(*c), *h, WHITE, 1.0, true, true);
        }
        mb
    };
    let x = |r: f32| Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2) * Quat::from_rotation_z(r);
    let mut models = Vec::new();
    // first-aid kit with a glowing cross on both faces
    models.push(vec![
        add(boxes(&[(Vec3::ZERO, Vec3::new(0.28, 0.21, 0.1)), (Vec3::new(0.0, 0.24, 0.0), Vec3::new(0.08, 0.03, 0.03))]), &white),
        add(boxes(&[(Vec3::ZERO, Vec3::new(0.16, 0.05, 0.105)), (Vec3::ZERO, Vec3::new(0.05, 0.16, 0.105))]), &red),
    ]);
    // body armour vest
    models.push(vec![
        add(boxes(&[(Vec3::ZERO, Vec3::new(0.22, 0.27, 0.09)), (Vec3::new(-0.13, 0.3, 0.0), Vec3::new(0.06, 0.05, 0.08)), (Vec3::new(0.13, 0.3, 0.0), Vec3::new(0.06, 0.05, 0.08))]), &blue),
        add(boxes(&[(Vec3::new(0.0, -0.02, 0.0), Vec3::new(0.16, 0.17, 0.1)), (Vec3::new(0.0, -0.22, 0.0), Vec3::new(0.225, 0.03, 0.095))]), &navy),
    ]);
    // two bundles of cash with paper bands
    {
        let mut notes = MeshBuilder::new();
        let mut bands = MeshBuilder::new();
        for (k, (y, r)) in [(0.0f32, 0.0f32), (0.1, 0.35)].iter().enumerate() {
            let q = Quat::from_rotation_y(*r);
            let c = Vec3::new(0.0, *y - 0.05, k as f32 * 0.02);
            notes.box_at(Affine3A::from_rotation_translation(q, c), Vec3::new(0.22, 0.045, 0.1), WHITE, 1.0, true, true);
            bands.box_at(Affine3A::from_rotation_translation(q, c), Vec3::new(0.04, 0.048, 0.103), WHITE, 1.0, true, true);
        }
        models.push(vec![add(notes, &green), add(bands, &band)]);
    }
    // every gun, from its hand model scaled up (index = 3 + weapon slot)
    for w in crate::weapons::WEAPONS {
        let mut mb = MeshBuilder::new();
        if let Some(g) = crate::weapons::gun_model(w) {
            // centre it on the pickup and show it side-on
            mb.append(&g, Affine3A::from_scale_rotation_translation(Vec3::splat(2.6), Quat::from_rotation_y(std::f32::consts::FRAC_PI_2), Vec3::new(0.0, -0.1, 0.0)));
        } else {
            mb.cuboid(Vec3::ZERO, Vec3::splat(0.12), WHITE, 1.0);
        }
        let m = match w {
            crate::weapons::Weapon::Rocket | crate::weapons::Weapon::Grenade => &olive,
            crate::weapons::Weapon::Bat => &band,
            _ => &gun,
        };
        models.push(vec![add(mb, m)]);
    }
    models
}

fn setup_pickups(mut commands: Commands, info: Res<CityInfo>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, mut glow: ResMut<GlowMats>) {
    let models = build_pickup_models(&mut meshes, &mut materials);
    let mut halo = MeshBuilder::new();
    halo.cylinder(Vec3::ZERO, 0.55, 0.012, 24, WHITE, true);
    let pa = PickupAssets {
        models,
        halo: meshes.add(halo.build()),
        halo_mats: [
            glow_material(&mut materials, &mut glow, LinearRgba::rgb(0.1, 0.8, 0.15)),
            glow_material(&mut materials, &mut glow, LinearRgba::rgb(0.1, 0.25, 1.0)),
            glow_material(&mut materials, &mut glow, LinearRgba::rgb(0.4, 0.8, 0.12)),
            glow_material(&mut materials, &mut glow, LinearRgba::rgb(1.0, 0.45, 0.1)),
        ],
    };
    // hand-placed: hospital, police HQ, the parks, the quay, the promenade
    let ground = |p: Vec3| Vec3::new(p.x, CURB, p.z);
    let park = |bx: i32, bz: i32| Vec3::new(block_min(bx) + BLOCK * 0.5 + 7.0, CURB, block_min(bz) + BLOCK * 0.5 + 1.5);
    let mut defs: Vec<(Vec3, PickupKind)> = vec![
        (ground(hospital_point()) + Vec3::X * 4.0, PickupKind::Health),
        (ground(hospital_point()) - Vec3::X * 4.0, PickupKind::Health),
        (ground(police_point()) + Vec3::X * 4.0, PickupKind::Armor),
        (ground(spawn_point()) + Vec3::new(9.0, 0.0, 0.0), PickupKind::Cash(100)),
        (park(2, 3), PickupKind::Weapon(2)),
        (park(6, 7), PickupKind::Weapon(3)),
        (park(4, 1), PickupKind::Armor),
        (Vec3::new(500.0, 0.0, -250.0), PickupKind::Weapon(4)),
        (Vec3::new(120.0, 0.0, CITY_HALF + 9.0), PickupKind::Health),
        (Vec3::new(-220.0, 0.0, CITY_HALF + 9.0), PickupKind::Weapon(2)),
        (ground(spawn_point()) + Vec3::new(0.0, 0.0, -3.0), PickupKind::Weapon(5)),
        (ground(police_point()) - Vec3::X * 4.0, PickupKind::Weapon(7)),
        (ground(hospital_point()) + Vec3::new(10.0, 0.0, 0.0), PickupKind::Weapon(6)),
        (Vec3::new(500.0, 0.0, 250.0), PickupKind::Weapon(8)),
        (Vec3::new(block_min(9) + 40.0, CURB, block_min(3) + 2.0), PickupKind::Weapon(9)),
        (Vec3::new(-1100.0, 0.0, -200.0), PickupKind::Weapon(10)),
    ];
    // scattered along the sidewalks, spread out
    let mut rng = Rng::new(424242);
    let mut tries = 0;
    let n_fixed = defs.len();
    while defs.len() < n_fixed + 70 && tries < 4000 && !info.sidewalk_points.is_empty() {
        tries += 1;
        let p = *rng.pick(&info.sidewalk_points);
        if defs.iter().any(|(q, _)| q.distance(p) < 45.0) {
            continue;
        }
        let r = rng.f();
        let kind = if r < 0.32 {
            PickupKind::Cash(rng.int(3, 30) as i64 * 10)
        } else if r < 0.5 {
            PickupKind::Health
        } else if r < 0.64 {
            PickupKind::Armor
        } else {
            let w = rng.f();
            // pistol, SMG, shotgun, bat, revolver, rifle, grenades, sniper, rocket, minigun
            let table = [(0.18, 1), (0.34, 2), (0.46, 3), (0.55, 5), (0.65, 6), (0.77, 7), (0.88, 9), (0.94, 8), (0.98, 4), (1.01, 10)];
            PickupKind::Weapon(table.iter().find(|(p, _)| w < *p).map(|t| t.1).unwrap_or(1))
        };
        defs.push((p, kind));
    }
    let n = defs.len();
    for (i, (p, k)) in defs.iter().enumerate() {
        spawn_pickup(&mut commands, &pa, *k, *p, Some(i), None);
    }
    commands.insert_resource(PickupSlots { defs, timers: vec![0.0; n], alive: vec![true; n] });
    commands.insert_resource(pa);
}

fn spawn_pickup(commands: &mut Commands, pa: &PickupAssets, kind: PickupKind, ground: Vec3, slot: Option<usize>, life: Option<f32>) -> Entity {
    let base = ground + Vec3::Y * 0.6;
    let phase = (ground.x * 0.37 + ground.z * 0.11).sin() * TAU;
    let root = commands.spawn((Transform::from_translation(base), Visibility::default(), Pickup { kind, slot, base, phase, life })).id();
    for (m, mat) in &pa.models[model_id(kind)] {
        let c = commands.spawn((Mesh3d(m.clone()), MeshMaterial3d(mat.clone()), Transform::default())).id();
        commands.entity(root).add_child(c);
    }
    let h = commands.spawn((Mesh3d(pa.halo.clone()), MeshMaterial3d(pa.halo_mats[halo_id(kind)].clone()), Transform::from_xyz(0.0, -0.56, 0.0), NoRaytrace)).id();
    commands.entity(root).add_child(h);
    root
}

fn spawn_drops(mut commands: Commands, pa: Option<Res<PickupAssets>>, mut drops: ResMut<Drops>) {
    let Some(pa) = pa else { return };
    for (p, k) in drops.0.drain(..) {
        let g = Vec3::new(p.x, p.y.max(ground_height(p.x, p.z)), p.z);
        spawn_pickup(&mut commands, &pa, k, g, None, Some(40.0));
    }
}

fn animate_pickups(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Pickup, &mut Transform, &mut Visibility)>) {
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    for (e, mut p, mut tr, mut vis) in &mut q {
        tr.translation = p.base + Vec3::Y * ((t * 2.2 + p.phase).sin() * 0.08);
        tr.rotation = Quat::from_rotation_y(t * 1.8 + p.phase);
        if let Some(l) = p.life.as_mut() {
            *l -= dt;
            if *l <= 0.0 {
                commands.entity(e).despawn();
            } else if *l < 6.0 {
                // blink before vanishing
                let on = (*l * 6.0) as i32 % 2 == 0;
                let want = if on { Visibility::Inherited } else { Visibility::Hidden };
                if *vis != want {
                    *vis = want;
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn collect_pickups(
    mut commands: Commands,
    mut player: Query<(&mut Player, &Transform), Without<Pickup>>,
    seats: Query<&Transform, (With<Seat>, Without<Player>, Without<Pickup>)>,
    pickups: Query<(Entity, &Pickup, &Transform)>,
    mut ars: ResMut<Arsenal>,
    mut slots: Option<ResMut<PickupSlots>>,
    mut hud: ResMut<HudMessages>,
    mut sfx: MessageWriter<Sfx>,
) {
    let Ok((mut p, pt)) = player.single_mut() else { return };
    if p.health <= 0.0 {
        return;
    }
    let (pos, reach) = match p.vehicle.and_then(|v| seats.get(v).ok()) {
        Some(st) => (st.translation, 2.8),
        None => (pt.translation, 1.4),
    };
    for (e, pk, tr) in &pickups {
        let d = (tr.translation - pos).with_y(0.0).length();
        if d > reach || (tr.translation.y - pos.y).abs() > 2.6 {
            continue;
        }
        let msg = match pk.kind {
            PickupKind::Health => {
                if p.health >= 100.0 {
                    continue;
                }
                p.health = (p.health + 50.0).min(100.0);
                "+50 Health".to_string()
            }
            PickupKind::Armor => {
                if p.armor >= 100.0 {
                    continue;
                }
                p.armor = 100.0;
                "Body armour".to_string()
            }
            PickupKind::Cash(n) => {
                p.money += n;
                format!("+${n}")
            }
            PickupKind::Weapon(i) => {
                let st = WEAPONS[i].stats();
                let add = st.pickup_ammo;
                let had = !ars.give(i, add);
                if had && st.melee() {
                    continue;
                }
                if !had && p.vehicle.is_none() {
                    ars.current = i;
                    ars.reloading = 0.0;
                }
                if had { format!("+{add} {} ammo", st.name) } else { format!("Picked up: {}  (Tab: weapon wheel)", st.name) }
            }
        };
        hud.push(&msg);
        sfx.write(Sfx { kind: SfxKind::Pickup, pos, volume: 0.8 });
        if let (Some(i), Some(slots)) = (pk.slot, slots.as_mut()) {
            slots.alive[i] = false;
            slots.timers[i] = RESPAWN_S;
        }
        commands.entity(e).despawn();
    }
}

fn respawn_pickups(mut commands: Commands, time: Res<Time>, pa: Option<Res<PickupAssets>>, slots: Option<ResMut<PickupSlots>>, player: Query<&Transform, With<Player>>) {
    let (Some(pa), Some(mut slots)) = (pa, slots) else { return };
    let ppos = player.single().map(|t| t.translation).unwrap_or(Vec3::ZERO);
    let dt = time.delta_secs();
    for i in 0..slots.defs.len() {
        if slots.alive[i] {
            continue;
        }
        slots.timers[i] -= dt;
        let (p, k) = slots.defs[i];
        // respawn out of sight-ish, not under the player's nose
        if slots.timers[i] <= 0.0 && p.distance(ppos) > 30.0 {
            spawn_pickup(&mut commands, &pa, k, p, Some(i), None);
            slots.alive[i] = true;
        }
    }
}

// ================================================================================================
// Spray & Go
// ================================================================================================

pub const RESPRAY_COST: i64 = 100;

#[derive(Component)]
struct GarageDoor(usize);

#[derive(Component)]
struct FadeOverlay;

#[derive(Resource, Default)]
struct ResprayState {
    /// (shop, time into the sequence, car)
    active: Option<(usize, f32, Entity)>,
    /// a shop re-arms once the car has left its bay
    used: [bool; 2],
    closed: [f32; 2],
}

fn setup_respray(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let (hw, hd, h) = (BAY_HALF.x, BAY_HALF.z, BAY_HALF.y);
    let door_h = h - 1.0;
    // roller door: slats hanging from the top edge (scaled in y to roll it up)
    let mut door = MeshBuilder::new();
    let slats = 14;
    for k in 0..slats {
        let y = -(k as f32 + 0.5) * door_h / slats as f32;
        door.box_at(Affine3A::from_translation(Vec3::new(0.0, y, 0.0)), Vec3::new(hw + 0.15, door_h / slats as f32 * 0.5 - 0.012, 0.05), WHITE, 1.0, true, true);
    }
    let door_mesh = meshes.add(door.build());
    let door_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.62, 0.64, 0.66), metallic: 0.7, perceptual_roughness: 0.35, ..default() });
    // rooftop sign: glowing pink panel with a spray can
    let mut panel = MeshBuilder::new();
    panel.box_at(Affine3A::from_translation(Vec3::ZERO), Vec3::new(4.6, 0.9, 0.12), WHITE, 1.0, true, true);
    let mut frame = MeshBuilder::new();
    for sx in [-3.2f32, 3.2] {
        frame.box_at(Affine3A::from_translation(Vec3::new(sx, -1.2, 0.2)), Vec3::new(0.08, 0.4, 0.08), WHITE, 1.0, true, true);
    }
    frame.box_at(Affine3A::from_translation(Vec3::new(0.0, 0.0, 0.0)), Vec3::new(4.75, 1.02, 0.08), WHITE, 1.0, true, true);
    let mut can = MeshBuilder::new();
    can.cylinder(Vec3::new(-3.4, -0.6, -0.18), 0.32, 1.0, 16, WHITE, true);
    can.cylinder(Vec3::new(-3.4, 0.4, -0.18), 0.14, 0.25, 12, WHITE, true);
    let mut stripes = MeshBuilder::new();
    for k in 0..3 {
        let x = -1.6 + k as f32 * 1.9;
        stripes.box_at(Affine3A::from_rotation_translation(Quat::from_rotation_z(0.35), Vec3::new(x, 0.0, -0.13)), Vec3::new(0.55, 0.28, 0.02), WHITE, 1.0, true, true);
    }
    let pink = materials.add(StandardMaterial { base_color: Color::srgb(0.95, 0.25, 0.6), emissive: LinearRgba::rgb(6.0, 0.8, 2.6), ..default() });
    let teal = materials.add(StandardMaterial { base_color: Color::srgb(0.1, 0.85, 0.85), emissive: LinearRgba::rgb(0.3, 4.0, 4.0), ..default() });
    let dark = materials.add(StandardMaterial { base_color: Color::srgb(0.08, 0.08, 0.09), metallic: 0.6, perceptual_roughness: 0.4, ..default() });
    let white = materials.add(StandardMaterial { base_color: Color::srgb(0.92, 0.92, 0.92), metallic: 0.5, perceptual_roughness: 0.3, ..default() });
    let (panel, frame, can, stripes) = (meshes.add(panel.build()), meshes.add(frame.build()), meshes.add(can.build()), meshes.add(stripes.build()));
    for i in 0..AUTO_SHOPS.len() {
        let bay = respray_bay(i);
        let front = bay.z + hd;
        commands.spawn((
            Mesh3d(door_mesh.clone()),
            MeshMaterial3d(door_mat.clone()),
            Transform { translation: Vec3::new(bay.x, CURB + door_h, front - 0.25), scale: Vec3::new(1.0, 0.03, 1.0), ..default() },
            GarageDoor(i),
        ));
        let sign = commands
            .spawn((Transform::from_xyz(bay.x, CURB + h + 1.9, front - 1.0), Visibility::default(), MapBlip { color: Color::srgb(1.0, 0.35, 0.7), range: 450.0 }))
            .id();
        for (m, mat) in [(&panel, &pink), (&frame, &dark), (&can, &white), (&stripes, &teal)] {
            let c = commands.spawn((Mesh3d(m.clone()), MeshMaterial3d(mat.clone()), Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::PI)))).id();
            commands.entity(sign).add_child(c);
        }
    }
    commands.spawn((
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
        BackgroundColor(Color::NONE),
        GlobalZIndex(50),
        bevy::picking::Pickable::IGNORE,
        FadeOverlay,
    ));
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn respray(
    mut commands: Commands,
    time: Res<Time>,
    mut st: ResMut<ResprayState>,
    mut player: Query<&mut Player>,
    mut cars: Query<(&Transform, &mut Velocity, &mut Vehicle, &Seat)>,
    mut wanted: ResMut<Wanted>,
    mut hud: ResMut<HudMessages>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut doors: Query<(&GarageDoor, &mut Transform), Without<Vehicle>>,
    mut fade: Query<&mut BackgroundColor, With<FadeOverlay>>,
    mut sfx: MessageWriter<Sfx>,
) {
    let dt = time.delta_secs();
    let Ok(mut p) = player.single_mut() else { return };
    // is the player's car parked in a bay?
    let mut inside: Option<(usize, f32, Entity)> = None;
    if let Some(ce) = p.vehicle {
        if let Ok((t, vel, v, seat)) = cars.get(ce) {
            if seat.kind == SeatKind::Car && !v.wrecked {
                for i in 0..AUTO_SHOPS.len() {
                    let l = t.translation - respray_bay(i);
                    if l.x.abs() < BAY_HALF.x - 0.6 && l.z.abs() < BAY_HALF.z - 0.5 && l.y < 4.0 {
                        inside = Some((i, vel.linear.length(), ce));
                    }
                }
            }
        }
    }
    for i in 0..AUTO_SHOPS.len() {
        if inside.map(|x| x.0) != Some(i) && st.active.is_none_or(|a| a.0 != i) {
            st.used[i] = false;
        }
    }
    match st.active {
        None => {
            if let Some((i, speed, ce)) = inside {
                if !st.used[i] {
                    if speed < 1.2 {
                        st.used[i] = true;
                        if p.money < RESPRAY_COST {
                            hud.push("Spray & Go: $100 a job - come back with cash");
                        } else {
                            st.active = Some((i, 0.0, ce));
                        }
                    } else {
                        hud.extra_hint = format!("Spray & Go - stop inside: repair, new paint, cops lose you (${RESPRAY_COST})");
                    }
                }
            }
        }
        Some((i, t, ce)) => {
            let t2 = t + dt;
            // hold the car still while the door is down
            if let Ok((_, mut vel, _, _)) = cars.get_mut(ce) {
                if t2 < 2.6 {
                    vel.linear = Vec3::ZERO;
                    vel.angular = Vec3::ZERO;
                }
            }
            if t < 1.6 && t2 >= 1.6 {
                p.money -= RESPRAY_COST;
                let had = wanted.stars;
                wanted.clear();
                if let Ok((_, _, mut v, _)) = cars.get_mut(ce) {
                    v.health = 1000.0;
                    v.burn_t = None;
                    v.upside_t = 0.0;
                    if let Some(s) = v.smoke.take() {
                        commands.entity(s).despawn();
                    }
                    let seed = (time.elapsed_secs() * 1000.0) as u64;
                    let c = *Rng::new(seed).pick(&crate::vehicle::PAINTS);
                    if let Some(mut m) = materials.get_mut(&v.paint) {
                        m.base_color = Color::srgb(c.0, c.1, c.2);
                    }
                }
                hud.big("RESPRAYED", Color::srgb(1.0, 0.4, 0.75));
                hud.push(if had > 0 { "New paint, new plates - the cops lost you  -$100" } else { "Fixed up and freshly painted  -$100" });
                sfx.write(Sfx { kind: SfxKind::Reward, pos: respray_bay(i), volume: 0.9 });
            }
            st.active = if t2 > 3.6 { None } else { Some((i, t2, ce)) };
        }
    }
    // door and fade animation
    for i in 0..AUTO_SHOPS.len() {
        let target = match st.active {
            Some((a, t, _)) if a == i => {
                if t < 2.3 {
                    (t / 1.0).min(1.0)
                } else {
                    1.0 - ((t - 2.3) / 1.0).min(1.0)
                }
            }
            _ => 0.0,
        };
        st.closed[i] = target;
    }
    for (d, mut tr) in &mut doors {
        tr.scale.y = st.closed[d.0].max(0.03);
    }
    let alpha = match st.active {
        Some((_, t, _)) => smoothstep(0.7, 1.3, t) * (1.0 - smoothstep(1.8, 2.5, t)),
        None => 0.0,
    };
    if let Ok(mut bg) = fade.single_mut() {
        let want = Color::srgba(0.0, 0.0, 0.0, alpha);
        if bg.0 != want {
            bg.0 = want;
        }
    }
}

// ================================================================================================
// Stunt ramps
// ================================================================================================

#[derive(Component)]
pub struct Ramp {
    pub id: usize,
    pub lip: Vec3,
}

#[derive(Resource, Default)]
pub struct StuntState {
    pub done: Vec<bool>,
    jump: Option<Jump>,
    slow: bool,
}

struct Jump {
    ramp: Option<usize>,
    start: Vec3,
    t: f32,
    max_y: f32,
    spin: Vec3,
    land_t: f32,
}

/// (low end on the ground, travel direction, size scale)
fn ramp_defs() -> Vec<(Vec2, Vec2, f32)> {
    vec![
        // airport road, westbound: launch toward the airport
        (Vec2::new(-560.0, -3.5), Vec2::NEG_X, 1.0),
        // along the beach, both ways
        (Vec2::new(-300.0, CITY_HALF + 30.0), Vec2::X, 1.0),
        (Vec2::new(260.0, CITY_HALF + 30.0), Vec2::NEG_X, 1.0),
        // the big one on the apron
        (Vec2::new(-760.0, -190.0), Vec2::NEG_X, 1.6),
        // over the quay toward the water
        (Vec2::new(490.0, 380.0), Vec2::X, 1.2),
    ]
}

fn setup_ramps(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, mut stunts: ResMut<StuntState>) {
    let yellow = materials.add(StandardMaterial { base_color: Color::srgb(0.95, 0.75, 0.05), perceptual_roughness: 0.6, ..default() });
    let black = materials.add(StandardMaterial { base_color: Color::srgb(0.04, 0.04, 0.04), perceptual_roughness: 0.6, ..default() });
    let steel = materials.add(StandardMaterial { base_color: Color::srgb(0.45, 0.46, 0.48), metallic: 0.7, perceptual_roughness: 0.4, ..default() });
    let defs = ramp_defs();
    stunts.done = vec![false; defs.len()];
    for (id, (start, dir, s)) in defs.into_iter().enumerate() {
        let (len, w, h) = (9.0 * s, 5.5 * s.sqrt(), 2.2 * s);
        // sit flush on the lowest ground under the footprint
        let side = Vec2::new(-dir.y, dir.x);
        let mut base = f32::MAX;
        for a in [0.0, 0.5, 1.0] {
            for b in [-0.5, 0.0, 0.5] {
                let p = start + dir * len * a + side * w * b;
                base = base.min(ground_height(p.x, p.y));
            }
        }
        let base = base - 0.05;
        let rot = Quat::from_rotation_arc(Vec3::NEG_Z, Vec3::new(dir.x, 0.0, dir.y));
        let origin = Vec3::new(start.x, base, start.y);
        let (mut yb, mut bb, mut gb) = (MeshBuilder::new(), MeshBuilder::new(), MeshBuilder::new());
        let n = Vec3::new(0.0, len, h).normalize();
        let segs = 8;
        for k in 0..segs {
            let (z0, z1) = (-len * k as f32 / segs as f32, -len * (k + 1) as f32 / segs as f32);
            let (y0, y1) = (h * k as f32 / segs as f32, h * (k + 1) as f32 / segs as f32);
            let q = [Vec3::new(-w * 0.5, y0, z0), Vec3::new(w * 0.5, y0, z0), Vec3::new(w * 0.5, y1, z1), Vec3::new(-w * 0.5, y1, z1)];
            let mb = if k % 2 == 0 { &mut yb } else { &mut bb };
            mb.quad(q, n, [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y], WHITE);
        }
        for sx in [-1.0f32, 1.0] {
            let x = sx * w * 0.5;
            gb.tri([Vec3::new(x, 0.0, 0.0), Vec3::new(x, 0.0, -len), Vec3::new(x, h, -len)], Vec3::X * sx, [Vec2::ZERO, Vec2::X, Vec2::ONE], WHITE);
            // edge rails
            gb.box_at(Affine3A::from_rotation_translation(Quat::from_rotation_x((h / len).atan()), Vec3::new(x, h * 0.5 + 0.08, -len * 0.5)), Vec3::new(0.06, 0.08, (len * len + h * h).sqrt() * 0.5), WHITE, 1.0, true, true);
        }
        gb.quad([Vec3::new(-w * 0.5, 0.0, -len), Vec3::new(w * 0.5, 0.0, -len), Vec3::new(w * 0.5, h, -len), Vec3::new(-w * 0.5, h, -len)], Vec3::NEG_Z, [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y], WHITE);
        let pts = [
            Vec3::new(-w * 0.5, 0.0, 0.0),
            Vec3::new(w * 0.5, 0.0, 0.0),
            Vec3::new(-w * 0.5, 0.0, -len),
            Vec3::new(w * 0.5, 0.0, -len),
            Vec3::new(-w * 0.5, h, -len),
            Vec3::new(w * 0.5, h, -len),
        ];
        let lip = origin + rot * Vec3::new(0.0, h, -len);
        let mut e = commands.spawn((Transform::from_translation(origin).with_rotation(rot), Visibility::default(), Ramp { id, lip }, MapBlip { color: Color::srgb(0.95, 0.75, 0.1), range: 160.0 }));
        if let Some(c) = Collider::convex_hull(&pts) {
            e.insert((RigidBody::Fixed, c, Friction::coefficient(0.9)));
        }
        let root = e.id();
        for (mb, m) in [(yb, &yellow), (bb, &black), (gb, &steel)] {
            let c = commands.spawn((Mesh3d(meshes.add(mb.build())), MeshMaterial3d(m.clone()), Transform::default())).id();
            commands.entity(root).add_child(c);
        }
    }
}

fn trick_names(spin: Vec3) -> (Vec<String>, i64) {
    let mut names = Vec::new();
    let mut bonus = 0;
    let word = |n: i32| match n {
        1 => "",
        2 => "Double ",
        3 => "Triple ",
        _ => "Quad ",
    };
    let flips = (spin.x / (TAU * 0.9)).floor() as i32;
    let rolls = (spin.z / (TAU * 0.9)).floor() as i32;
    let spins = (spin.y / (TAU * 0.9)).floor() as i32;
    if flips > 0 {
        names.push(format!("{}Flip", word(flips)));
        bonus += flips as i64 * 300;
    }
    if rolls > 0 {
        names.push(format!("{}Barrel Roll", word(rolls)));
        bonus += rolls as i64 * 250;
    }
    if spins > 0 {
        names.push(format!("{}", spins * 360));
        bonus += spins as i64 * 150;
    }
    (names, bonus)
}

#[allow(clippy::too_many_arguments)]
fn stunts(
    time: Res<Time>,
    mut vt: ResMut<Time<Virtual>>,
    mut st: ResMut<StuntState>,
    mut player: Query<&mut Player>,
    cars: Query<(&Transform, &Velocity, &Vehicle)>,
    ramps: Query<&Ramp>,
    mut hud: ResMut<HudMessages>,
    mut sfx: MessageWriter<Sfx>,
) {
    let dt = time.delta_secs();
    let StuntState { done, jump, slow } = &mut *st;
    let mut want_slow = false;
    let car = player.single().ok().and_then(|p| p.vehicle).and_then(|v| cars.get(v).ok());
    match car {
        None => *jump = None,
        Some((t, vel, v)) => {
            let contacts = v.wheels.iter().filter(|w| w.contact).count();
            let speed = vel.linear.length();
            if jump.is_none() {
                if contacts == 0 && speed > 10.0 && !v.wrecked {
                    let ramp = ramps.iter().find(|r| r.lip.distance(t.translation) < 6.0 * 1.6).map(|r| r.id);
                    *jump = Some(Jump { ramp, start: t.translation, t: 0.0, max_y: t.translation.y, spin: Vec3::ZERO, land_t: 0.0 });
                }
            } else if let Some(j) = jump.as_mut() {
                j.t += dt;
                j.max_y = j.max_y.max(t.translation.y);
                j.spin += (t.rotation.inverse() * vel.angular).abs() * dt;
                if contacts >= 2 {
                    j.land_t += dt;
                } else {
                    j.land_t = 0.0;
                }
                want_slow = j.ramp.is_some_and(|r| !done[r]) && j.t > 0.2 && j.t < 6.0 && j.land_t == 0.0 && speed > 8.0;
                if j.land_t > 0.15 || j.t > 14.0 || v.wrecked {
                    let j = jump.take().unwrap();
                    let air = j.t - j.land_t;
                    let dist = (t.translation - j.start).with_y(0.0).length();
                    let height = (j.max_y - j.start.y).max(0.0);
                    let upright = (t.rotation * Vec3::Y).y > 0.35;
                    let unique = j.ramp.filter(|r| !done[*r]);
                    if air >= 1.0 && dist >= 14.0 {
                        if !upright || v.wrecked {
                            hud.push("Stunt failed - you have to land it");
                        } else if let Ok(mut p) = player.single_mut() {
                            let (names, trick_bonus) = trick_names(j.spin);
                            let mut reward = (dist * 2.0 + height * 12.0 + air * 40.0) as i64 + trick_bonus;
                            let tricks = if names.is_empty() { String::new() } else { format!("  {}", names.join(" + ")) };
                            if let Some(r) = unique {
                                done[r] = true;
                                reward += 500;
                                let n = done.iter().filter(|d| **d).count();
                                hud.big("STUNT JUMP!", Color::srgb(1.0, 0.8, 0.15));
                                hud.push(&format!("Stunt jump {n}/{}: {dist:.0} m, {height:.1} m high, {air:.1} s{tricks}  +${reward}", done.len()));
                            } else if air > 1.6 || trick_bonus > 0 {
                                hud.push(&format!("Airtime bonus: {dist:.0} m, {air:.1} s{tricks}  +${reward}"));
                            } else {
                                reward /= 3;
                                hud.push(&format!("Nice air: {dist:.0} m  +${reward}"));
                            }
                            p.money += reward;
                            sfx.write(Sfx { kind: SfxKind::Reward, pos: t.translation, volume: 0.9 });
                        }
                    } else if unique.is_some() && air > 0.3 {
                        hud.push("Stunt jump missed - hit the ramp faster");
                    }
                }
            }
        }
    }
    // slow motion over a new jump
    if want_slow != *slow {
        *slow = want_slow;
        vt.set_relative_speed(if want_slow { 0.4 } else { 1.0 });
    }
}
