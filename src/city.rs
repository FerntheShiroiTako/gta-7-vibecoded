//! Procedural coastal city: road grid, districts, docks, beach, airport, hill, ocean.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::meshgen::{rgb, MeshBuilder, Rgba, Rng, WHITE};
use crate::textures::{self, to_image};
use crate::world::*;

pub struct CityPlugin;

impl Plugin for CityPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CityInfo>().add_systems(Startup, setup_city);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum M {
    Asphalt,
    Paint,
    Sidewalk,
    Concrete,
    Grass,
    Sand,
    Terrain,
    Office,
    Res,
    Ind,
    Roof,
    Metal,
    Foliage,
    Bark,
    Glass,
    Lamp,
    TileRoof,
    Water,
    Dark,
}

#[derive(Resource, Clone)]
pub struct CityMats {
    pub m: HashMap<M, Handle<StandardMaterial>>,
    /// base material x tint colour -> material (raytraced meshes can't carry vertex colours)
    pub tinted: HashMap<(M, [u8; 4]), Handle<StandardMaterial>>,
}

impl CityMats {
    pub fn get(&self, k: M) -> Handle<StandardMaterial> {
        self.m[&k].clone()
    }
    pub fn tint(&mut self, materials: &mut Assets<StandardMaterial>, k: M, c: Rgba) -> Handle<StandardMaterial> {
        if c.iter().take(3).all(|v| (v - 1.0).abs() < 0.01) {
            return self.get(k);
        }
        let key = (k, c.map(|v| (v * 50.0).round().clamp(0.0, 255.0) as u8));
        if let Some(h) = self.tinted.get(&key) {
            return h.clone();
        }
        let mut mat = materials.get(&self.get(k)).cloned().unwrap_or_default();
        let b = mat.base_color.to_linear();
        mat.base_color = Color::LinearRgba(LinearRgba::new(b.red * c[0], b.green * c[1], b.blue * c[2], b.alpha));
        let h = materials.add(mat);
        self.tinted.insert(key, h.clone());
        h
    }
}

/// Spawn a builder as one entity per tint colour.
pub fn spawn_tinted(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, mats: &mut CityMats, k: M, mb: MeshBuilder) {
    if mb.is_empty() {
        return;
    }
    for (c, part) in mb.split_by_color() {
        let mat = mats.tint(materials, k, c);
        commands.spawn((Mesh3d(meshes.add(part.build())), MeshMaterial3d(mat), Transform::IDENTITY));
    }
}

/// Footprint kinds for the minimap.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Foot {
    Building,
    Park,
    Water,
}

#[derive(Resource, Default)]
pub struct CityInfo {
    pub footprints: Vec<(Vec2, Vec2, Foot)>,
    pub parking: Vec<(Vec3, f32)>,
    pub lamps: Vec<Vec3>,
    pub sidewalk_points: Vec<Vec3>,
}

/// One mesh builder per material; flushed into entities.
#[derive(Default)]
pub struct Batch {
    b: HashMap<M, MeshBuilder>,
    pub colliders: Vec<(Collider, Transform)>,
}

impl Batch {
    pub fn b(&mut self, m: M) -> &mut MeshBuilder {
        self.b.entry(m).or_default()
    }
    pub fn solid(&mut self, center: Vec3, half: Vec3) {
        self.colliders.push((Collider::cuboid(half.x, half.y, half.z), Transform::from_translation(center)));
    }
    pub fn flush(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, mats: &mut CityMats) {
        for (k, mb) in self.b.drain() {
            spawn_tinted(commands, meshes, materials, mats, k, mb);
        }
        for (c, t) in self.colliders.drain(..) {
            commands.spawn((c, t));
        }
    }
}

fn make_materials(images: &mut Assets<Image>, materials: &mut Assets<StandardMaterial>) -> CityMats {
    let mut m = HashMap::new();
    let tex = |images: &mut Assets<Image>, t: &textures::Tex, srgb: bool| images.add(to_image(t, srgb));

    let asphalt = tex(images, &textures::noise_texture(512, [0.24, 0.24, 0.25], [0.12, 0.12, 0.12], 32, 7, 0.06), true);
    let paving = tex(images, &textures::paving_texture(256, 2, 3), true);
    let grass = tex(images, &textures::noise_texture(256, [0.33, 0.47, 0.2], [0.18, 0.2, 0.1], 16, 21, 0.03), true);
    let sand = tex(images, &textures::noise_texture(256, [0.82, 0.74, 0.56], [0.1, 0.1, 0.08], 32, 33, 0.05), true);
    let detail = tex(images, &textures::noise_texture(256, [0.92, 0.92, 0.92], [0.25, 0.25, 0.25], 16, 41, 0.0), true);
    let gravel = tex(images, &textures::noise_texture(256, [0.5, 0.49, 0.47], [0.25, 0.25, 0.25], 64, 55, 0.12), true);
    let concrete = tex(images, &textures::noise_texture(256, [0.6, 0.6, 0.58], [0.1, 0.1, 0.1], 16, 63, 0.02), true);

    let office = textures::facade_office(101);
    let res = textures::facade_residential(202);
    let ind = textures::facade_industrial(303);

    let facade = |images: &mut Assets<Image>, f: &textures::FacadeSet, metal_base: f32| StandardMaterial {
        base_color_texture: Some(images.add(to_image(&f.albedo, true))),
        metallic_roughness_texture: Some(images.add(to_image(&f.mr, false))),
        emissive_texture: Some(images.add(to_image(&f.emissive, true))),
        emissive: LinearRgba::BLACK,
        perceptual_roughness: 1.0,
        metallic: metal_base,
        ..default()
    };
    let office_m = facade(images, &office, 1.0);
    let res_m = facade(images, &res, 1.0);
    let ind_m = facade(images, &ind, 1.0);

    let plain = |c: Color, rough: f32, metal: f32, tex: Option<Handle<Image>>| StandardMaterial {
        base_color: c,
        base_color_texture: tex,
        perceptual_roughness: rough,
        metallic: metal,
        ..default()
    };

    m.insert(M::Asphalt, materials.add(plain(Color::WHITE, 0.88, 0.0, Some(asphalt))));
    m.insert(M::Paint, materials.add(plain(Color::WHITE, 0.6, 0.0, None)));
    m.insert(M::Sidewalk, materials.add(plain(Color::WHITE, 0.85, 0.0, Some(paving))));
    m.insert(M::Concrete, materials.add(plain(Color::WHITE, 0.9, 0.0, Some(concrete))));
    m.insert(M::Grass, materials.add(plain(Color::WHITE, 0.95, 0.0, Some(grass))));
    m.insert(M::Sand, materials.add(plain(Color::WHITE, 0.95, 0.0, Some(sand))));
    m.insert(M::Terrain, materials.add(plain(Color::WHITE, 0.95, 0.0, Some(detail))));
    m.insert(M::Office, materials.add(office_m));
    m.insert(M::Res, materials.add(res_m));
    m.insert(M::Ind, materials.add(ind_m));
    m.insert(M::Roof, materials.add(plain(Color::WHITE, 0.95, 0.0, Some(gravel))));
    m.insert(M::Metal, materials.add(plain(Color::WHITE, 0.4, 0.85, None)));
    m.insert(M::Foliage, materials.add(plain(Color::WHITE, 0.85, 0.0, None)));
    m.insert(M::Bark, materials.add(plain(Color::srgb(0.32, 0.24, 0.17), 0.95, 0.0, None)));
    m.insert(M::Glass, materials.add(plain(Color::srgb(0.05, 0.08, 0.1), 0.05, 0.3, None)));
    m.insert(
        M::Lamp,
        materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.95, 0.85),
            emissive: LinearRgba::rgb(0.6, 0.5, 0.35),
            ..default()
        }),
    );
    m.insert(M::TileRoof, materials.add(plain(Color::WHITE, 0.75, 0.0, None)));
    m.insert(
        M::Water,
        materials.add(StandardMaterial {
            base_color: Color::srgb(0.02, 0.09, 0.12),
            perceptual_roughness: 0.12,
            reflectance: 0.45,
            ..default()
        }),
    );
    m.insert(M::Dark, materials.add(plain(Color::srgb(0.06, 0.06, 0.07), 0.6, 0.2, None)));
    CityMats { m, tinted: HashMap::new() }
}

pub fn setup_city(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut info: ResMut<CityInfo>,
) {
    let mut mats = make_materials(&mut images, &mut materials);
    let mut rng = Rng::new(20261121);

    // ---------- ocean ----------
    let mut sea = MeshBuilder::new();
    sea.flat_rect(Vec2::new(-7400.0, -7200.0), Vec2::new(6600.0, 6800.0), 0.0, WHITE, 50.0);
    commands.spawn((Mesh3d(meshes.add(sea.build())), MeshMaterial3d(mats.get(M::Water)), Transform::from_xyz(0.0, SEA_LEVEL, 0.0)));
    // sea floor so the coast never looks like it floats
    commands.spawn((Collider::cuboid(7000.0, 1.0, 7000.0), Transform::from_xyz(0.0, SEA_LEVEL - 14.0, 0.0)));

    // ---------- ground slabs ----------
    let mut g = Batch::default();
    slab(&mut g, M::Asphalt, CITY, 6.0);
    slab(&mut g, M::Grass, AIRPORT, 8.0);
    slab(&mut g, M::Concrete, QUAY, 8.0);
    g.flush(&mut commands, &mut meshes, &mut materials, &mut mats);

    road_markings(&mut commands, &mut meshes, &mut materials, &mut mats);

    // ---------- city blocks ----------
    for bx in 0..GRID {
        for bz in 0..GRID {
            let mut b = Batch::default();
            city_block(&mut b, &mut rng, bx, bz, &mut info);
            b.flush(&mut commands, &mut meshes, &mut materials, &mut mats);
        }
    }

    // ---------- surroundings ----------
    let mut a = Batch::default();
    airport(&mut a, &mut rng, &mut info);
    a.flush(&mut commands, &mut meshes, &mut materials, &mut mats);

    let mut d = Batch::default();
    docks(&mut d, &mut rng, &mut info);
    d.flush(&mut commands, &mut meshes, &mut materials, &mut mats);

    terrain_patch(&mut commands, &mut meshes, &mut materials, &mut images, HILL, 8.0, hill_height, false);
    terrain_patch(&mut commands, &mut meshes, &mut materials, &mut images, BEACH, 6.0, beach_height, true);

    let mut h = Batch::default();
    hill_props(&mut h, &mut rng);
    beach_props(&mut h, &mut rng);
    h.flush(&mut commands, &mut meshes, &mut materials, &mut mats);

    commands.insert_resource(mats);
}

/// Thick ground slab whose top is at y = 0 (sides read as sea walls).
fn slab(b: &mut Batch, m: M, a: Area, uv: f32) {
    let half = Vec3::new(a.size().x * 0.5, 6.0, a.size().y * 0.5);
    let c = Vec3::new(a.center().x, -6.0, a.center().y);
    b.b(m).box_at(Affine3A::from_translation(c), half, WHITE, uv, true, false);
    b.solid(c, half);
}

// ------------------------------------------------------------------------------------------------
// Roads
// ------------------------------------------------------------------------------------------------

const WHITE_PAINT: Rgba = [0.92, 0.92, 0.9, 1.0];
const YELLOW_PAINT: Rgba = [0.95, 0.72, 0.12, 1.0];
const PAINT_Y: f32 = 0.015;

fn road_markings(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, mats: &mut CityMats) {
    for horizontal in [true, false] {
        for j in 0..=GRID {
            let mut mb = MeshBuilder::new();
            let c = road_center(j);
            for i in 0..GRID {
                let s0 = block_min(i);
                let s1 = s0 + BLOCK;
                // map (along, across) -> world xz
                let p = |along: f32, across: f32| -> Vec2 {
                    if horizontal { Vec2::new(along, c + across) } else { Vec2::new(c + across, along) }
                };
                let rect = |mb: &mut MeshBuilder, a0: f32, a1: f32, c0: f32, c1: f32, col: Rgba| {
                    let q0 = p(a0, c0);
                    let q1 = p(a1, c1);
                    mb.flat_rect(q0.min(q1), q0.max(q1), PAINT_Y, col, 1.0);
                };
                // double yellow centre line
                for off in [-0.22, 0.22] {
                    rect(&mut mb, s0 + 5.0, s1 - 5.0, off - 0.07, off + 0.07, YELLOW_PAINT);
                }
                // dashed lane dividers
                for off in [-LANE, LANE] {
                    let mut a = s0 + 7.0;
                    while a + 3.0 < s1 - 6.0 {
                        rect(&mut mb, a, a + 3.0, off - 0.07, off + 0.07, WHITE_PAINT);
                        a += 9.0;
                    }
                }
                // stop lines (right-hand traffic)
                rect(&mut mb, s1 - 5.4, s1 - 5.0, 0.3, ROAD * 0.5 - 0.3, WHITE_PAINT);
                rect(&mut mb, s0 + 5.0, s0 + 5.4, -ROAD * 0.5 + 0.3, -0.3, WHITE_PAINT);
                // zebra crossings at both ends
                for (z0, z1) in [(s0 + 0.8, s0 + 4.2), (s1 - 4.2, s1 - 0.8)] {
                    let mut k = -ROAD * 0.5 + 0.6;
                    while k + 0.55 < ROAD * 0.5 {
                        rect(&mut mb, z0, z1, k, k + 0.55, WHITE_PAINT);
                        k += 1.15;
                    }
                }
                // solid edge lines
                for off in [-ROAD * 0.5 + 0.35, ROAD * 0.5 - 0.35] {
                    rect(&mut mb, s0 + 5.0, s1 - 5.0, off - 0.06, off + 0.06, WHITE_PAINT);
                }
            }
            spawn_tinted(commands, meshes, materials, mats, M::Paint, mb);
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Blocks
// ------------------------------------------------------------------------------------------------

const OFFICE_CELL: Vec2 = Vec2::new(3.0 * 8.0, 3.6 * 8.0);
const RES_CELL: Vec2 = Vec2::new(3.2 * 8.0, 3.0 * 8.0);
const IND_CELL: Vec2 = Vec2::new(2.0 * 8.0, 3.0 * 8.0);

#[derive(Clone, Copy)]
enum Style {
    Office,
    Res,
    Ind,
}

impl Style {
    fn mat(self) -> M {
        match self {
            Style::Office => M::Office,
            Style::Res => M::Res,
            Style::Ind => M::Ind,
        }
    }
    fn cell(self) -> Vec2 {
        match self {
            Style::Office => OFFICE_CELL,
            Style::Res => RES_CELL,
            Style::Ind => IND_CELL,
        }
    }
}

fn city_block(b: &mut Batch, rng: &mut Rng, bx: i32, bz: i32, info: &mut CityInfo) {
    let x0 = block_min(bx);
    let z0 = block_min(bz);
    let dist = district(bx, bz);
    // Raised block slab: curb sides + sidewalk ring + inner surface.
    let half = Vec3::new(BLOCK * 0.5, (CURB + 0.3) * 0.5, BLOCK * 0.5);
    let center = Vec3::new(x0 + BLOCK * 0.5, (CURB - 0.3) * 0.5, z0 + BLOCK * 0.5);
    b.b(M::Concrete).box_at(Affine3A::from_translation(center), half, rgb(0.75, 0.75, 0.72), 2.0, false, false);
    b.solid(center, half);
    let inner_mat = match dist {
        District::Suburb | District::Park => M::Grass,
        _ => M::Sidewalk,
    };
    let (ix0, iz0, ix1, iz1) = (x0 + SIDEWALK, z0 + SIDEWALK, x0 + BLOCK - SIDEWALK, z0 + BLOCK - SIDEWALK);
    let sw = b.b(M::Sidewalk);
    sw.flat_rect(Vec2::new(x0, z0), Vec2::new(x0 + BLOCK, iz0), CURB, WHITE, 2.0);
    sw.flat_rect(Vec2::new(x0, iz1), Vec2::new(x0 + BLOCK, z0 + BLOCK), CURB, WHITE, 2.0);
    sw.flat_rect(Vec2::new(x0, iz0), Vec2::new(ix0, iz1), CURB, WHITE, 2.0);
    sw.flat_rect(Vec2::new(ix1, iz0), Vec2::new(x0 + BLOCK, iz1), CURB, WHITE, 2.0);
    b.b(inner_mat).flat_rect(Vec2::new(ix0, iz0), Vec2::new(ix1, iz1), CURB, WHITE, if inner_mat == M::Grass { 6.0 } else { 2.0 });

    // sidewalk sample points for pedestrians
    for k in 0..8 {
        let t = k as f32 / 8.0 * BLOCK;
        info.sidewalk_points.push(Vec3::new(x0 + t, CURB, z0 + 1.5));
        info.sidewalk_points.push(Vec3::new(x0 + t, CURB, z0 + BLOCK - 1.5));
    }

    let inner = Area::new(ix0, iz0, ix1, iz1);
    match dist {
        District::Downtown => downtown(b, rng, inner, bx, bz, info),
        District::Midtown => midtown(b, rng, inner, info),
        District::Suburb => suburb(b, rng, inner, info),
        District::Industrial => industrial(b, rng, inner, info),
        District::Beachfront => beachfront(b, rng, inner, info),
        District::Park => park(b, rng, inner, info),
        District::Hospital => hospital(b, rng, inner, info),
        District::PoliceHq => police_hq(b, rng, inner, info),
        District::AutoShop => auto_shop(b, rng, inner, bx, bz, info),
    }

    streetlights(b, x0, z0, info);
    street_props(b, rng, x0, z0, dist);
    if matches!(dist, District::Midtown | District::Suburb | District::Downtown) {
        sidewalk_trees(b, rng, x0, z0, dist == District::Downtown);
    }
    parking(rng, bx, bz, dist, info);
}

fn streetlights(b: &mut Batch, x0: f32, z0: f32, info: &mut CityInfo) {
    // Lights on all four sidewalks, 1 m from the curb, arm reaching over the road.
    for k in 0..3 {
        let t = 12.0 + k as f32 * 28.0;
        for (pos, out) in [
            (Vec3::new(x0 + t, CURB, z0 + 0.8), Vec3::NEG_Z),
            (Vec3::new(x0 + t, CURB, z0 + BLOCK - 0.8), Vec3::Z),
            (Vec3::new(x0 + 0.8, CURB, z0 + t), Vec3::NEG_X),
            (Vec3::new(x0 + BLOCK - 0.8, CURB, z0 + t), Vec3::X),
        ] {
            streetlight(b, pos, out);
            info.lamps.push(pos + Vec3::Y * 7.6 + out * 2.0);
        }
    }
}

pub fn streetlight(b: &mut Batch, pos: Vec3, out: Vec3) {
    let dark = rgb(0.18, 0.19, 0.2);
    b.b(M::Metal).cylinder(pos, 0.11, 8.0, 8, dark, false);
    b.b(M::Metal).cylinder(pos, 0.2, 0.6, 8, dark, true);
    let arm_c = pos + Vec3::Y * 7.9 + out * 1.1;
    let yaw = Quat::from_rotation_arc(Vec3::X, out);
    b.b(M::Metal).box_at(Affine3A::from_rotation_translation(yaw, arm_c), Vec3::new(1.15, 0.06, 0.06), dark, 1.0, true, true);
    let head = pos + Vec3::Y * 7.75 + out * 2.0;
    b.b(M::Metal).box_at(Affine3A::from_rotation_translation(yaw, head + Vec3::Y * 0.1), Vec3::new(0.45, 0.1, 0.2), dark, 1.0, true, true);
    b.b(M::Lamp).box_at(Affine3A::from_rotation_translation(yaw, head - Vec3::Y * 0.02), Vec3::new(0.38, 0.03, 0.15), WHITE, 1.0, false, true);
    b.colliders.push((Collider::cylinder(4.0, 0.15), Transform::from_translation(pos + Vec3::Y * 4.0)));
}

fn sidewalk_trees(b: &mut Batch, rng: &mut Rng, x0: f32, z0: f32, planters: bool) {
    for k in 0..2 {
        let t = 26.0 + k as f32 * 28.0;
        for pos in [
            Vec3::new(x0 + t, CURB, z0 + 2.0),
            Vec3::new(x0 + t, CURB, z0 + BLOCK - 2.0),
            Vec3::new(x0 + 2.0, CURB, z0 + t),
            Vec3::new(x0 + BLOCK - 2.0, CURB, z0 + t),
        ] {
            if rng.chance(0.75) {
                if planters {
                    b.b(M::Concrete).cuboid(pos + Vec3::Y * 0.25, Vec3::new(0.8, 0.25, 0.8), rgb(0.6, 0.6, 0.58), 1.0);
                }
                let h = rng.range(5.5, 8.0);
                tree(b, rng, pos, h);
            }
        }
    }
}

pub fn tree(b: &mut Batch, rng: &mut Rng, p: Vec3, h: f32) {
    let r = 0.11 + h * 0.018;
    b.b(M::Bark).cylinder_at(Affine3A::from_translation(p), r * 1.35, r * 0.8, h * 0.62, 9, WHITE, false);
    let greens = [rgb(0.2, 0.36, 0.12), rgb(0.26, 0.42, 0.15), rgb(0.17, 0.3, 0.1), rgb(0.3, 0.4, 0.14)];
    let top = p + Vec3::Y * h * 0.68;
    // branches reaching into the crown
    for k in 0..3 {
        let a = k as f32 / 3.0 * TAU + rng.range(-0.4, 0.4);
        let dir = Vec3::new(a.cos() * 0.75, 1.0, a.sin() * 0.75).normalize();
        let base = p + Vec3::Y * h * rng.range(0.42, 0.55);
        b.b(M::Bark).cylinder_at(Affine3A::from_rotation_translation(Quat::from_rotation_arc(Vec3::Y, dir), base), r * 0.45, r * 0.2, h * 0.32, 6, WHITE, false);
    }
    // crown made of several overlapping leaf clusters
    let n = rng.int(7, 10);
    for i in 0..n {
        let a = i as f32 / n as f32 * TAU + rng.range(-0.3, 0.3);
        let spread = h * rng.range(0.1, 0.2);
        let o = Vec3::new(a.cos() * spread, rng.range(-0.12, 0.25) * h, a.sin() * spread);
        let rad = h * rng.range(0.13, 0.19);
        let c = *rng.pick(&greens);
        b.b(M::Foliage).ellipsoid(top + o, Vec3::new(rad, rad * 0.82, rad), 10, 7, c);
    }
    let c = *rng.pick(&greens);
    b.b(M::Foliage).ellipsoid(top + Vec3::Y * h * 0.12, Vec3::new(h * 0.17, h * 0.15, h * 0.17), 10, 7, c);
    b.colliders.push((Collider::cylinder(h * 0.3, r * 1.4), Transform::from_translation(p + Vec3::Y * h * 0.3)));
}

pub fn palm(b: &mut Batch, rng: &mut Rng, p: Vec3, h: f32) {
    let lean = Vec3::new(rng.range(-1.0, 1.0), 0.0, rng.range(-1.0, 1.0)).normalize_or(Vec3::X) * rng.range(0.05, 0.2);
    let segs = 6;
    let mut cur = p;
    let seg_h = h / segs as f32;
    for i in 0..segs {
        let t = i as f32 / segs as f32;
        let dir = (Vec3::Y + lean * (t * 2.0)).normalize();
        let rot = Quat::from_rotation_arc(Vec3::Y, dir);
        let r0 = 0.26 - 0.08 * t;
        b.b(M::Bark).cylinder_at(Affine3A::from_rotation_translation(rot, cur), r0, r0 - 0.012, seg_h * 1.04, 7, rgb(0.75, 0.62, 0.48), false);
        cur += dir * seg_h;
    }
    let fronds = 9;
    for i in 0..fronds {
        let a = i as f32 / fronds as f32 * TAU + rng.range(-0.2, 0.2);
        let droop = rng.range(0.25, 0.55);
        let dir = Vec3::new(a.cos(), -droop, a.sin()).normalize();
        let len = rng.range(3.0, 4.2);
        let rot = Quat::from_rotation_arc(Vec3::X, dir);
        let c = cur + dir * len * 0.5;
        b.b(M::Foliage).box_at(Affine3A::from_rotation_translation(rot, c), Vec3::new(len * 0.5, 0.04, 0.45), rgb(0.22, 0.42, 0.14), 1.0, true, true);
    }
    b.colliders.push((Collider::cylinder(h * 0.5, 0.3), Transform::from_translation(p + Vec3::Y * h * 0.5)));
}

/// A building box with facade, gravel roof, parapet, rooftop clutter and a collider.
fn building(b: &mut Batch, rng: &mut Rng, min: Vec3, max: Vec3, style: Style, tint: Rgba, clutter: bool) {
    let off = Vec2::new(rng.int(0, 7) as f32 / 8.0, rng.int(0, 7) as f32 / 8.0);
    b.b(style.mat()).facade_box(min, max, tint, style.cell(), off, None);
    b.b(M::Roof).flat_rect(Vec2::new(min.x, min.z), Vec2::new(max.x, max.z), max.y, WHITE, 4.0);
    let c = (min + max) * 0.5;
    let h = (max - min) * 0.5;
    b.solid(c, h);
    // parapet
    let pc = rgb(tint[0] * 0.8, tint[1] * 0.8, tint[2] * 0.8);
    let ph = 0.55;
    let t = 0.25;
    let y = max.y + ph;
    let con = b.b(M::Concrete);
    con.cuboid(Vec3::new(c.x, y, min.z + t), Vec3::new(h.x, ph, t), pc, 2.0);
    con.cuboid(Vec3::new(c.x, y, max.z - t), Vec3::new(h.x, ph, t), pc, 2.0);
    con.cuboid(Vec3::new(min.x + t, y, c.z), Vec3::new(t, ph, h.z - 2.0 * t), pc, 2.0);
    con.cuboid(Vec3::new(max.x - t, y, c.z), Vec3::new(t, ph, h.z - 2.0 * t), pc, 2.0);
    let height = max.y - min.y;
    // cornice band just under the roof line
    if height > 10.0 {
        let o = 0.28;
        let cy = max.y - 0.35;
        let cc = rgb(tint[0] * 0.92, tint[1] * 0.92, tint[2] * 0.92);
        let con = b.b(M::Concrete);
        con.cuboid(Vec3::new(c.x, cy, min.z - o * 0.5), Vec3::new(h.x + o, 0.3, o * 0.5), cc, 2.0);
        con.cuboid(Vec3::new(c.x, cy, max.z + o * 0.5), Vec3::new(h.x + o, 0.3, o * 0.5), cc, 2.0);
        con.cuboid(Vec3::new(min.x - o * 0.5, cy, c.z), Vec3::new(o * 0.5, 0.3, h.z), cc, 2.0);
        con.cuboid(Vec3::new(max.x + o * 0.5, cy, c.z), Vec3::new(o * 0.5, 0.3, h.z), cc, 2.0);
    }
    // street level: stone plinth + glazed shopfronts with mullions
    let street = (min.y - CURB).abs() < 0.01 && height > 8.0 && !matches!(style, Style::Ind);
    if street {
        let pl = rgb(tint[0] * 0.55, tint[1] * 0.55, tint[2] * 0.55);
        let o = 0.06;
        let con = b.b(M::Concrete);
        con.cuboid(Vec3::new(c.x, min.y + 0.4, min.z - o), Vec3::new(h.x + o, 0.4, o), pl, 1.0);
        con.cuboid(Vec3::new(c.x, min.y + 0.4, max.z + o), Vec3::new(h.x + o, 0.4, o), pl, 1.0);
        con.cuboid(Vec3::new(min.x - o, min.y + 0.4, c.z), Vec3::new(o, 0.4, h.z), pl, 1.0);
        con.cuboid(Vec3::new(max.x + o, min.y + 0.4, c.z), Vec3::new(o, 0.4, h.z), pl, 1.0);
        let (y0, y1) = (min.y + 0.8, min.y + 3.9);
        let gy = (y0 + y1) * 0.5;
        let gh = (y1 - y0) * 0.5;
        b.b(M::Glass).cuboid(Vec3::new(c.x, gy, min.z - 0.03), Vec3::new(h.x - 0.4, gh, 0.02), WHITE, 1.0);
        b.b(M::Glass).cuboid(Vec3::new(c.x, gy, max.z + 0.03), Vec3::new(h.x - 0.4, gh, 0.02), WHITE, 1.0);
        b.b(M::Glass).cuboid(Vec3::new(min.x - 0.03, gy, c.z), Vec3::new(0.02, gh, h.z - 0.4), WHITE, 1.0);
        b.b(M::Glass).cuboid(Vec3::new(max.x + 0.03, gy, c.z), Vec3::new(0.02, gh, h.z - 0.4), WHITE, 1.0);
        let mc = rgb(0.25, 0.26, 0.28);
        let met = b.b(M::Metal);
        met.cuboid(Vec3::new(c.x, y1 + 0.08, min.z - 0.06), Vec3::new(h.x, 0.08, 0.06), mc, 1.0);
        met.cuboid(Vec3::new(c.x, y1 + 0.08, max.z + 0.06), Vec3::new(h.x, 0.08, 0.06), mc, 1.0);
        met.cuboid(Vec3::new(min.x - 0.06, y1 + 0.08, c.z), Vec3::new(0.06, 0.08, h.z), mc, 1.0);
        met.cuboid(Vec3::new(max.x + 0.06, y1 + 0.08, c.z), Vec3::new(0.06, 0.08, h.z), mc, 1.0);
        let mut x = min.x + 3.0;
        while x < max.x - 1.0 {
            met.cuboid(Vec3::new(x, gy, min.z - 0.06), Vec3::new(0.05, gh, 0.05), mc, 1.0);
            met.cuboid(Vec3::new(x, gy, max.z + 0.06), Vec3::new(0.05, gh, 0.05), mc, 1.0);
            x += 3.0;
        }
        let mut z = min.z + 3.0;
        while z < max.z - 1.0 {
            met.cuboid(Vec3::new(min.x - 0.06, gy, z), Vec3::new(0.05, gh, 0.05), mc, 1.0);
            met.cuboid(Vec3::new(max.x + 0.06, gy, z), Vec3::new(0.05, gh, 0.05), mc, 1.0);
            z += 3.0;
        }
    }
    // vertical aluminium fins on tall glass towers
    if matches!(style, Style::Office) && height > 30.0 {
        let fc = rgb(0.72, 0.74, 0.77);
        let fh = height * 0.5 - 0.4;
        let fy = (min.y + max.y) * 0.5 - 0.2;
        let met = b.b(M::Metal);
        let mut x = min.x + 4.5;
        while x < max.x - 1.0 {
            met.cuboid(Vec3::new(x, fy, min.z - 0.18), Vec3::new(0.06, fh, 0.18), fc, 1.0);
            met.cuboid(Vec3::new(x, fy, max.z + 0.18), Vec3::new(0.06, fh, 0.18), fc, 1.0);
            x += 4.5;
        }
        let mut z = min.z + 4.5;
        while z < max.z - 1.0 {
            met.cuboid(Vec3::new(min.x - 0.18, fy, z), Vec3::new(0.18, fh, 0.06), fc, 1.0);
            met.cuboid(Vec3::new(max.x + 0.18, fy, z), Vec3::new(0.18, fh, 0.06), fc, 1.0);
            z += 4.5;
        }
    }
    if !clutter {
        return;
    }
    if height > 12.0 && height < 70.0 && rng.chance(0.22) {
        let facing = *rng.pick(&[Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z]);
        let edge = c + Vec3::new(facing.x * (h.x - 2.0), 0.0, facing.z * (h.z - 2.0));
        billboard(b, rng, Vec3::new(edge.x, max.y, edge.z), facing);
    }
    let n = rng.int(1, 4);
    for _ in 0..n {
        let s = Vec3::new(rng.range(1.0, 2.5), rng.range(0.6, 1.4), rng.range(1.0, 2.5));
        let p = Vec3::new(rng.range(min.x + 2.0 + s.x, max.x - 2.0 - s.x), max.y + s.y, rng.range(min.z + 2.0 + s.z, max.z - 2.0 - s.z));
        if p.is_finite() {
            b.b(M::Metal).cuboid(p, s, rgb(0.62, 0.63, 0.64), 1.0);
        }
    }
    if matches!(style, Style::Res) && rng.chance(0.5) && h.x > 5.0 && h.z > 5.0 {
        // water tank on legs
        let p = Vec3::new(c.x + rng.range(-h.x * 0.4, h.x * 0.4), max.y, c.z + rng.range(-h.z * 0.4, h.z * 0.4));
        for (dx, dz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            b.b(M::Metal).cylinder(p + Vec3::new(dx, 0.0, dz) * 1.1, 0.08, 3.0, 5, rgb(0.3, 0.25, 0.2), false);
        }
        b.b(M::Bark).cylinder_at(Affine3A::from_translation(p + Vec3::Y * 3.0), 1.6, 1.6, 2.8, 12, rgb(1.4, 1.2, 1.0), true);
        b.b(M::Bark).cylinder_at(Affine3A::from_translation(p + Vec3::Y * 5.8), 1.7, 0.1, 1.0, 12, rgb(1.2, 1.0, 0.9), false);
    }
}

fn tower_tint(rng: &mut Rng) -> Rgba {
    *rng.pick(&[
        rgb(1.0, 1.0, 1.0),
        rgb(0.85, 0.95, 1.05),
        rgb(1.05, 1.0, 0.9),
        rgb(0.8, 0.9, 0.85),
        rgb(0.95, 0.9, 0.85),
    ])
}

fn wall_tint(rng: &mut Rng) -> Rgba {
    *rng.pick(&[
        rgb(0.95, 0.9, 0.82),
        rgb(0.85, 0.72, 0.6),
        rgb(0.78, 0.8, 0.82),
        rgb(0.72, 0.52, 0.42),
        rgb(0.92, 0.85, 0.7),
        rgb(0.6, 0.62, 0.66),
        rgb(0.9, 0.78, 0.68),
    ])
}

fn downtown(b: &mut Batch, rng: &mut Rng, a: Area, bx: i32, bz: i32, info: &mut CityInfo) {
    let centrality = 1.0 - (Vec2::new(bx as f32 - 4.5, bz as f32 - 4.5).length() / 3.2).min(1.0);
    let podium_h = rng.range(6.0, 11.0);
    let m = 2.0;
    let pmin = Vec3::new(a.min.x + m, CURB, a.min.y + m);
    let pmax = Vec3::new(a.max.x - m, CURB + podium_h, a.max.y - m);
    building(b, rng, pmin, pmax, Style::Office, rgb(0.9, 0.88, 0.85), false);
    info.footprints.push((Vec2::new(pmin.x, pmin.z), Vec2::new(pmax.x, pmax.z), Foot::Building));
    let twin = rng.chance(0.35);
    let towers: Vec<Area> = if twin {
        vec![
            Area::new(pmin.x + 3.0, pmin.z + 4.0, pmin.x + 31.0, pmax.z - 4.0),
            Area::new(pmax.x - 31.0, pmin.z + 4.0, pmax.x - 3.0, pmax.z - 4.0),
        ]
    } else {
        let s = rng.range(24.0, 36.0);
        let cx = a.center().x + rng.range(-4.0, 4.0);
        let cz = a.center().y + rng.range(-4.0, 4.0);
        vec![Area::new(cx - s * 0.5, cz - s * 0.5, cx + s * 0.5, cz + s * 0.5)]
    };
    for t in towers {
        let total = 45.0 + centrality * rng.range(70.0, 150.0) + rng.range(0.0, 30.0);
        let tint = tower_tint(rng);
        let tiers = if total > 110.0 { 3 } else { 2 };
        let mut y = pmax.y;
        let mut area = t;
        for k in 0..tiers {
            let h = if k == tiers - 1 { total - (y - CURB) } else { total * rng.range(0.35, 0.5) };
            if h < 6.0 {
                break;
            }
            let lo = Vec3::new(area.min.x, y, area.min.y);
            let hi = Vec3::new(area.max.x, y + h, area.max.y);
            building(b, rng, lo, hi, Style::Office, tint, k == tiers - 1);
            y += h;
            let inset = rng.range(2.0, 4.0);
            area = Area::new(area.min.x + inset, area.min.y + inset, area.max.x - inset, area.max.y - inset);
            if area.size().x < 10.0 {
                break;
            }
        }
        // spire / helipad on the tallest
        let c = Vec3::new(area.center().x, y, area.center().y);
        if total > 140.0 {
            b.b(M::Metal).cylinder_at(Affine3A::from_translation(c + Vec3::Y * 1.1), 0.4, 0.05, 22.0, 8, rgb(0.8, 0.8, 0.82), false);
        } else if rng.chance(0.5) {
            b.b(M::Dark).cylinder(c + Vec3::Y * 1.1, 6.0, 0.25, 20, rgb(0.5, 0.5, 0.5), true);
            b.b(M::Paint).cuboid(c + Vec3::new(-1.5, 1.37, 0.0), Vec3::new(0.3, 0.02, 2.0), WHITE_PAINT, 1.0);
            b.b(M::Paint).cuboid(c + Vec3::new(1.5, 1.37, 0.0), Vec3::new(0.3, 0.02, 2.0), WHITE_PAINT, 1.0);
            b.b(M::Paint).cuboid(c + Vec3::new(0.0, 1.37, 0.0), Vec3::new(1.2, 0.02, 0.3), WHITE_PAINT, 1.0);
        }
    }
}

fn midtown(b: &mut Batch, rng: &mut Rng, a: Area, info: &mut CityInfo) {
    let split_x = rng.chance(0.7);
    let split_z = rng.chance(0.6);
    let xs = if split_x { vec![(a.min.x, a.center().x), (a.center().x, a.max.x)] } else { vec![(a.min.x, a.max.x)] };
    let zs = if split_z { vec![(a.min.y, a.center().y), (a.center().y, a.max.y)] } else { vec![(a.min.y, a.max.y)] };
    for &(x0, x1) in &xs {
        for &(z0, z1) in &zs {
            let m = rng.range(1.5, 5.0);
            let h = rng.range(14.0, 52.0);
            let style = if rng.chance(0.55) { Style::Res } else { Style::Office };
            let tint = if matches!(style, Style::Res) { wall_tint(rng) } else { tower_tint(rng) };
            let lo = Vec3::new(x0 + m, CURB, z0 + m);
            let hi = Vec3::new(x1 - m, CURB + h, z1 - m);
            building(b, rng, lo, hi, style, tint, true);
            info.footprints.push((Vec2::new(lo.x, lo.z), Vec2::new(hi.x, hi.z), Foot::Building));
            // storefront awning on the street side
            if rng.chance(0.5) {
                let side = rng.int(0, 1);
                let z = if side == 0 { lo.z - 0.8 } else { hi.z + 0.8 };
                let col = *rng.pick(&[rgb(0.6, 0.1, 0.1), rgb(0.1, 0.3, 0.5), rgb(0.15, 0.4, 0.2), rgb(0.8, 0.55, 0.1)]);
                b.b(M::Paint).cuboid(Vec3::new((lo.x + hi.x) * 0.5, CURB + 3.6, z), Vec3::new((hi.x - lo.x) * 0.45, 0.08, 0.8), col, 1.0);
            }
        }
    }
}

fn gable_house(b: &mut Batch, rng: &mut Rng, c: Vec3, w: f32, d: f32, stories: i32, yaw: f32) {
    let wall_h = 3.1 * stories as f32;
    let tint = wall_tint(rng);
    let rot = Quat::from_rotation_y(yaw);
    let m = Affine3A::from_rotation_translation(rot, c);
    // walls via facade box in local space, then transformed
    let mut walls = MeshBuilder::new();
    walls.facade_box(Vec3::new(-w * 0.5, 0.0, -d * 0.5), Vec3::new(w * 0.5, wall_h, d * 0.5), tint, RES_CELL, Vec2::new(rng.int(0, 7) as f32 / 8.0, 0.0), None);
    b.b(M::Res).append(&walls, m);
    // roof prism along local x
    let rise = rng.range(1.6, 2.6);
    let ov = 0.5;
    let roof_c = *rng.pick(&[rgb(0.55, 0.22, 0.15), rgb(0.25, 0.25, 0.28), rgb(0.4, 0.3, 0.22), rgb(0.35, 0.18, 0.15)]);
    let prof = [Vec2::new(-d * 0.5 - ov, wall_h - 0.2), Vec2::new(d * 0.5 + ov, wall_h - 0.2), Vec2::new(0.0, wall_h + rise)];
    b.b(M::TileRoof).extrude_x(m, &prof, -w * 0.5 - ov, w * 0.5 + ov, roof_c, 2.0);
    // chimney + door
    if rng.chance(0.6) {
        let cp = m.transform_point3(Vec3::new(w * 0.3, wall_h + rise * 0.6, -d * 0.2));
        b.b(M::Concrete).box_at(Affine3A::from_rotation_translation(rot, cp), Vec3::new(0.4, rise * 0.6, 0.4), rgb(0.6, 0.4, 0.35), 1.0, true, false);
    }
    let door = m.transform_point3(Vec3::new(0.0, 1.05, d * 0.5 + 0.02));
    b.b(M::Dark).box_at(Affine3A::from_rotation_translation(rot, door), Vec3::new(0.5, 1.05, 0.04), rgb(0.4, 0.25, 0.15), 1.0, true, false);
    let hx = if yaw.abs() < 0.1 || (yaw.abs() - PI).abs() < 0.1 { w * 0.5 } else { d * 0.5 };
    let hz = if hx == w * 0.5 { d * 0.5 } else { w * 0.5 };
    b.solid(c + Vec3::Y * (wall_h + rise * 0.5) * 0.5, Vec3::new(hx, (wall_h + rise * 0.5) * 0.5, hz));
}

fn suburb(b: &mut Batch, rng: &mut Rng, a: Area, info: &mut CityInfo) {
    // Two rows of three houses facing the north and south streets.
    for row in 0..2 {
        for k in 0..3 {
            let lot_w = a.size().x / 3.0;
            let cx = a.min.x + lot_w * (k as f32 + 0.5);
            let facing_south = row == 1;
            let cz = if facing_south { a.max.y - 15.0 } else { a.min.y + 15.0 };
            let w = rng.range(10.0, 14.0);
            let d = rng.range(8.5, 11.0);
            let yaw = if facing_south { 0.0 } else { PI };
            let stories = if rng.chance(0.4) { 2 } else { 1 };
            gable_house(b, rng, Vec3::new(cx, CURB, cz), w, d, stories, yaw);
            info.footprints.push((Vec2::new(cx - w * 0.5, cz - d * 0.5), Vec2::new(cx + w * 0.5, cz + d * 0.5), Foot::Building));
            // driveway to the street
            let street_z = if facing_south { a.max.y } else { a.min.y };
            let dz0 = cz.min(street_z);
            let dz1 = cz.max(street_z);
            b.b(M::Concrete).flat_rect(Vec2::new(cx + w * 0.5 - 0.5, dz0), Vec2::new(cx + w * 0.5 + 3.0, dz1), CURB + 0.01, rgb(0.7, 0.7, 0.68), 2.0);
            // fence between lots
            if k > 0 {
                let fx = a.min.x + lot_w * k as f32;
                let (z0, z1) = if facing_south { (a.center().y + 2.0, a.max.y - 2.0) } else { (a.min.y + 2.0, a.center().y - 2.0) };
                b.b(M::Bark).cuboid(Vec3::new(fx, CURB + 0.6, (z0 + z1) * 0.5), Vec3::new(0.05, 0.6, (z1 - z0) * 0.5), rgb(0.85, 0.8, 0.72), 1.0);
            }
            if rng.chance(0.7) {
                let tp = Vec3::new(cx + rng.range(-lot_w * 0.35, lot_w * 0.35), CURB, (cz + a.center().y) * 0.5);
                let h = rng.range(6.0, 9.0);
                tree(b, rng, tp, h);
            }
        }
    }
}

fn industrial(b: &mut Batch, rng: &mut Rng, a: Area, info: &mut CityInfo) {
    let h = rng.range(10.0, 15.0);
    let lo = Vec3::new(a.min.x + 2.0, CURB, a.min.y + 3.0);
    let hi = Vec3::new(a.max.x - 14.0, CURB + h, a.max.y - 20.0);
    let tint = *rng.pick(&[rgb(0.85, 0.85, 0.85), rgb(0.6, 0.7, 0.8), rgb(0.8, 0.75, 0.65), rgb(0.7, 0.55, 0.45)]);
    building(b, rng, lo, hi, Style::Ind, tint, false);
    info.footprints.push((Vec2::new(lo.x, lo.z), Vec2::new(hi.x, hi.z), Foot::Building));
    // loading doors on the south face
    for k in 0..4 {
        let x = lo.x + 6.0 + k as f32 * 11.0;
        if x + 3.0 > hi.x {
            break;
        }
        b.b(M::Dark).cuboid(Vec3::new(x, CURB + 2.4, hi.z + 0.03), Vec3::new(2.2, 2.4, 0.05), rgb(0.35, 0.35, 0.33), 1.0);
    }
    // container stacks in the yard
    for k in 0..3 {
        let x = a.min.x + 8.0 + k as f32 * 14.0;
        let stack = rng.int(1, 3);
        for s in 0..stack {
            container(b, rng, Vec3::new(x, CURB + 1.3 + s as f32 * 2.6, a.max.y - 9.0), 0.0);
        }
    }
}

pub fn container(b: &mut Batch, rng: &mut Rng, c: Vec3, yaw: f32) {
    let col = *rng.pick(&[
        rgb(0.65, 0.15, 0.1),
        rgb(0.1, 0.3, 0.6),
        rgb(0.15, 0.45, 0.25),
        rgb(0.85, 0.5, 0.1),
        rgb(0.5, 0.52, 0.55),
        rgb(0.75, 0.72, 0.2),
    ]);
    let half = Vec3::new(1.22, 1.3, 6.1);
    let rot = Quat::from_rotation_y(yaw);
    let mut mb = MeshBuilder::new();
    mb.facade_box(-half, half, col, Vec2::new(8.0, 30.0), Vec2::ZERO, None);
    mb.flat_rect(Vec2::new(-half.x, -half.z), Vec2::new(half.x, half.z), half.y, col, 2.0);
    b.b(M::Ind).append(&mb, Affine3A::from_rotation_translation(rot, c));
    b.colliders.push((Collider::cuboid(half.x, half.y, half.z), Transform::from_translation(c).with_rotation(rot)));
}

fn beachfront(b: &mut Batch, rng: &mut Rng, a: Area, info: &mut CityInfo) {
    let n = 2;
    for k in 0..n {
        let w = a.size().x / n as f32;
        let x0 = a.min.x + w * k as f32 + 3.0;
        let x1 = x0 + w - 6.0;
        let h = rng.range(18.0, 46.0);
        let lo = Vec3::new(x0, CURB, a.min.y + 6.0);
        let hi = Vec3::new(x1, CURB + h, a.max.y - 12.0);
        let tint = *rng.pick(&[rgb(1.0, 0.95, 0.9), rgb(0.95, 0.85, 0.75), rgb(0.85, 0.95, 1.0), rgb(1.0, 0.9, 0.95)]);
        building(b, rng, lo, hi, Style::Res, tint, true);
        info.footprints.push((Vec2::new(lo.x, lo.z), Vec2::new(hi.x, hi.z), Foot::Building));
        // ocean-facing balconies
        let mut y = CURB + 3.0;
        while y < hi.y - 1.0 {
            b.b(M::Concrete).cuboid(Vec3::new((x0 + x1) * 0.5, y, hi.z + 0.9), Vec3::new((x1 - x0) * 0.5 - 1.0, 0.1, 0.9), WHITE, 2.0);
            b.b(M::Glass).cuboid(Vec3::new((x0 + x1) * 0.5, y + 0.55, hi.z + 1.75), Vec3::new((x1 - x0) * 0.5 - 1.0, 0.45, 0.03), WHITE, 1.0);
            y += 3.0;
        }
    }
    for k in 0..5 {
        let x = a.min.x + 6.0 + k as f32 * (a.size().x - 12.0) / 4.0;
        let h = rng.range(8.0, 12.0);
        palm(b, rng, Vec3::new(x, CURB, a.max.y + 2.0), h);
    }
}

fn park(b: &mut Batch, rng: &mut Rng, a: Area, info: &mut CityInfo) {
    info.footprints.push((a.min, a.max, Foot::Park));
    let c = a.center();
    let path = rgb(0.85, 0.8, 0.7);
    b.b(M::Sidewalk).flat_rect(Vec2::new(a.min.x, c.y - 2.0), Vec2::new(a.max.x, c.y + 2.0), CURB + 0.01, path, 2.0);
    b.b(M::Sidewalk).flat_rect(Vec2::new(c.x - 2.0, a.min.y), Vec2::new(c.x + 2.0, a.max.y), CURB + 0.012, path, 2.0);
    // fountain
    let fc = Vec3::new(c.x, CURB, c.y);
    b.b(M::Concrete).cylinder(fc, 6.0, 0.7, 24, rgb(0.8, 0.78, 0.74), true);
    b.b(M::Water).cylinder(fc + Vec3::Y * 0.01, 5.5, 0.62, 24, WHITE, true);
    b.b(M::Concrete).cylinder(fc, 0.8, 3.2, 12, rgb(0.8, 0.78, 0.74), true);
    b.b(M::Concrete).cylinder(fc + Vec3::Y * 2.2, 2.0, 0.3, 16, rgb(0.8, 0.78, 0.74), true);
    b.colliders.push((Collider::cylinder(0.35, 6.0), Transform::from_translation(fc + Vec3::Y * 0.35)));
    let mut placed = 0;
    let mut tries = 0;
    while placed < 22 && tries < 200 {
        tries += 1;
        let p = Vec2::new(rng.range(a.min.x + 3.0, a.max.x - 3.0), rng.range(a.min.y + 3.0, a.max.y - 3.0));
        if (p.x - c.x).abs() < 4.5 || (p.y - c.y).abs() < 4.5 || p.distance(c) < 10.0 {
            continue;
        }
        let h = rng.range(6.0, 11.0);
        tree(b, rng, Vec3::new(p.x, CURB, p.y), h);
        placed += 1;
    }
    // benches along paths
    for k in 0..4 {
        let t = a.min.x + 8.0 + k as f32 * 9.0;
        b.b(M::Bark).cuboid(Vec3::new(t, CURB + 0.45, c.y + 3.0), Vec3::new(0.9, 0.05, 0.25), rgb(0.6, 0.45, 0.3), 1.0);
        b.b(M::Metal).cuboid(Vec3::new(t, CURB + 0.2, c.y + 3.0), Vec3::new(0.8, 0.2, 0.05), rgb(0.2, 0.2, 0.2), 1.0);
    }
}

fn hospital(b: &mut Batch, rng: &mut Rng, a: Area, info: &mut CityInfo) {
    let lo = Vec3::new(a.min.x + 4.0, CURB, a.min.y + 4.0);
    let hi = Vec3::new(a.max.x - 4.0, CURB + 24.0, a.max.y - 16.0);
    building(b, rng, lo, hi, Style::Res, rgb(1.0, 1.0, 1.0), false);
    info.footprints.push((Vec2::new(lo.x, lo.z), Vec2::new(hi.x, hi.z), Foot::Building));
    let top = Vec3::new((lo.x + hi.x) * 0.5, hi.y + 0.02, (lo.z + hi.z) * 0.5);
    let red = rgb(0.85, 0.08, 0.08);
    b.b(M::Paint).cuboid(top + Vec3::new(-10.0, 0.0, 0.0), Vec3::new(5.0, 0.03, 1.6), red, 1.0);
    b.b(M::Paint).cuboid(top + Vec3::new(-10.0, 0.0, 0.0), Vec3::new(1.6, 0.03, 5.0), red, 1.0);
    b.b(M::Dark).cylinder(top + Vec3::new(10.0, 0.0, 0.0), 7.0, 0.2, 24, rgb(0.4, 0.4, 0.4), true);
    // emergency canopy
    b.b(M::Concrete).cuboid(Vec3::new(top.x, CURB + 4.2, hi.z + 5.0), Vec3::new(9.0, 0.25, 5.0), WHITE, 2.0);
    b.b(M::Paint).cuboid(Vec3::new(top.x, CURB + 3.9, hi.z + 10.0), Vec3::new(9.0, 0.08, 0.08), red, 1.0);
}

fn police_hq(b: &mut Batch, rng: &mut Rng, a: Area, info: &mut CityInfo) {
    let lo = Vec3::new(a.min.x + 4.0, CURB, a.min.y + 4.0);
    let hi = Vec3::new(a.max.x - 4.0, CURB + 20.0, a.max.y - 20.0);
    building(b, rng, lo, hi, Style::Office, rgb(0.75, 0.82, 1.0), true);
    info.footprints.push((Vec2::new(lo.x, lo.z), Vec2::new(hi.x, hi.z), Foot::Building));
    b.b(M::Paint).cuboid(Vec3::new((lo.x + hi.x) * 0.5, CURB + 6.0, hi.z + 0.05), Vec3::new((hi.x - lo.x) * 0.5, 0.5, 0.05), rgb(0.1, 0.2, 0.6), 1.0);
    let mast = Vec3::new(hi.x - 6.0, hi.y + 1.1, lo.z + 6.0);
    b.b(M::Metal).cylinder_at(Affine3A::from_translation(mast), 0.3, 0.08, 16.0, 6, rgb(0.8, 0.8, 0.8), false);
}

/// Spray & Go: an open-fronted respray garage on the street, a small office and
/// a used-car lot. The roll-up door and the sign are entities (activities.rs).
fn auto_shop(b: &mut Batch, rng: &mut Rng, a: Area, bx: i32, bz: i32, info: &mut CityInfo) {
    let i = AUTO_SHOPS.iter().position(|s| *s == (bx, bz)).unwrap_or(0);
    let bay = respray_bay(i);
    let (hw, hd, h, t) = (BAY_HALF.x, BAY_HALF.z, BAY_HALF.y, 0.4);
    let (cx, z0, z1) = (bay.x, bay.z - hd, bay.z + hd);
    let wall = rgb(0.9, 0.88, 0.84);
    let mut solid = |b: &mut Batch, c: Vec3, half: Vec3, m: M, col| {
        b.b(m).box_at(Affine3A::from_translation(c), half, col, 2.0, true, true);
        b.solid(c, half);
    };
    // walls, back, roof and the lintel over the door
    for sx in [-1.0f32, 1.0] {
        solid(b, Vec3::new(cx + sx * (hw + t * 0.5), CURB + h * 0.5, bay.z), Vec3::new(t * 0.5, h * 0.5, hd), M::Concrete, wall);
    }
    solid(b, Vec3::new(cx, CURB + h * 0.5, z0 - t * 0.5), Vec3::new(hw + t, h * 0.5, t * 0.5), M::Concrete, wall);
    solid(b, Vec3::new(cx, CURB + h + 0.2, bay.z - t * 0.5), Vec3::new(hw + t + 0.3, 0.2, hd + t * 0.5 + 0.3), M::Concrete, rgb(0.5, 0.5, 0.52));
    solid(b, Vec3::new(cx, CURB + h - 0.5, z1 - t * 0.5), Vec3::new(hw + t, 0.5, t * 0.5), M::Concrete, wall);
    // pink and teal stripes across the front, spray booth floor markings
    b.b(M::Paint).cuboid(Vec3::new(cx, CURB + h - 0.6, z1 + 0.01), Vec3::new(hw + t, 0.12, 0.02), rgb(0.95, 0.2, 0.6), 1.0);
    b.b(M::Paint).cuboid(Vec3::new(cx, CURB + h - 0.3, z1 + 0.01), Vec3::new(hw + t, 0.08, 0.02), rgb(0.1, 0.8, 0.8), 1.0);
    b.b(M::Paint).flat_rect(Vec2::new(cx - hw + 0.3, z0 + 0.3), Vec2::new(cx + hw - 0.3, z0 + 0.5), CURB + 0.012, rgb(0.95, 0.8, 0.1), 1.0);
    for sx in [-1.0f32, 1.0] {
        b.b(M::Paint).flat_rect(Vec2::new(cx + sx * (hw - 1.0) - 0.1, z0 + 0.5), Vec2::new(cx + sx * (hw - 1.0) + 0.1, z1), CURB + 0.012, rgb(0.95, 0.8, 0.1), 1.0);
        // spray nozzles on the walls
        for k in 0..3 {
            let z = z0 + 3.0 + k as f32 * 4.0;
            b.b(M::Metal).cylinder_at(Affine3A::from_rotation_translation(Quat::from_rotation_z(sx * FRAC_PI_2), Vec3::new(cx + sx * (hw - 0.1), CURB + 2.6, z)), 0.08, 0.05, 0.35, 8, rgb(0.6, 0.6, 0.62), true);
        }
    }
    info.footprints.push((Vec2::new(cx - hw - t, z0 - t), Vec2::new(cx + hw + t, z1), Foot::Building));
    // office at the back of the lot
    let lo = Vec3::new(a.min.x + 2.0, CURB, a.min.y + 2.0);
    let hi = Vec3::new(a.min.x + 30.0, CURB + rng.range(7.0, 10.0), a.min.y + 22.0);
    building(b, rng, lo, hi, Style::Ind, rgb(0.95, 0.9, 0.85), false);
    info.footprints.push((Vec2::new(lo.x, lo.z), Vec2::new(hi.x, hi.z), Foot::Building));
    // used-car lot east of the garage: painted bays, bunting poles, parked stock
    let lot_x0 = cx + hw + 4.0;
    let mut x = lot_x0;
    while x + 3.0 < a.max.x - 2.0 {
        b.b(M::Paint).flat_rect(Vec2::new(x - 0.06, z1 - 9.0), Vec2::new(x + 0.06, z1 - 3.0), CURB + 0.012, WHITE, 1.0);
        if rng.chance(0.6) {
            info.parking.push((Vec3::new(x + 1.6, CURB, z1 - 6.0), 0.0));
        }
        x += 3.2;
    }
    for k in 0..4 {
        let px = lot_x0 + k as f32 * ((a.max.x - 2.0 - lot_x0) / 3.0);
        b.b(M::Metal).cylinder(Vec3::new(px, CURB, z1 - 1.0), 0.06, 4.0, 6, rgb(0.8, 0.8, 0.8), true);
        let cols = [rgb(0.95, 0.2, 0.2), rgb(0.95, 0.85, 0.1), rgb(0.2, 0.5, 0.95)];
        b.b(M::Paint).cuboid(Vec3::new(px, CURB + 3.6, z1 - 1.0), Vec3::new(0.02, 0.25, 0.18), cols[k % 3], 1.0);
    }
}

fn parking(rng: &mut Rng, bx: i32, bz: i32, dist: District, info: &mut CityInfo) {
    let p = match dist {
        District::Suburb => 0.45,
        District::Midtown | District::Beachfront => 0.35,
        District::Industrial => 0.3,
        District::Downtown => 0.25,
        District::AutoShop => 0.0,
        _ => 0.15,
    };
    // Curbside spots in the outer lane of the street south of the block (eastbound lane)
    // and the street east of the block (northbound lane), facing the direction of travel.
    let x0 = block_min(bx);
    let z0 = block_min(bz);
    let south_road = road_center(bz + 1);
    let east_road = road_center(bx + 1);
    for k in 0..3 {
        if rng.chance(p) {
            let x = x0 + 14.0 + k as f32 * 24.0;
            info.parking.push((Vec3::new(x, 0.0, south_road + LANE * 1.5), -FRAC_PI_2));
        }
        if rng.chance(p) && bx + 1 < GRID {
            let z = z0 + 14.0 + k as f32 * 24.0;
            info.parking.push((Vec3::new(east_road + LANE * 1.5, 0.0, z), 0.0));
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Airport
// ------------------------------------------------------------------------------------------------

fn airport(b: &mut Batch, rng: &mut Rng, info: &mut CityInfo) {
    let asphalt = rgb(0.7, 0.7, 0.72);
    for (area, y) in [(RUNWAY, 0.02), (TAXIWAY, 0.02), (AIRPORT_ROAD, 0.02)] {
        b.b(M::Asphalt).flat_rect(area.min, area.max, y, asphalt, 8.0);
    }
    b.b(M::Concrete).flat_rect(APRON.min, APRON.max, 0.02, rgb(0.82, 0.82, 0.8), 8.0);
    // taxiway links
    for x in [-1440.0, -1060.0, -680.0] {
        b.b(M::Asphalt).flat_rect(Vec2::new(x - 10.0, TAXIWAY.max.y), Vec2::new(x + 10.0, RUNWAY.min.y), 0.021, asphalt, 8.0);
    }
    let paint = b.b(M::Paint);
    let rz = RUNWAY.center().y;
    // runway: centre dashes, edges, thresholds
    let mut x = RUNWAY.min.x + 60.0;
    while x + 30.0 < RUNWAY.max.x - 60.0 {
        paint.flat_rect(Vec2::new(x, rz - 0.45), Vec2::new(x + 30.0, rz + 0.45), 0.035, WHITE_PAINT, 1.0);
        x += 50.0;
    }
    for z in [RUNWAY.min.y + 1.0, RUNWAY.max.y - 1.0] {
        paint.flat_rect(Vec2::new(RUNWAY.min.x + 5.0, z - 0.45), Vec2::new(RUNWAY.max.x - 5.0, z + 0.45), 0.035, WHITE_PAINT, 1.0);
    }
    for (x0, dir) in [(RUNWAY.min.x + 8.0, 1.0f32), (RUNWAY.max.x - 8.0, -1.0)] {
        for k in 0..8 {
            let off = 4.0 + k as f32 * 2.2;
            for s in [-1.0, 1.0] {
                let z = rz + s * off;
                let xa = x0;
                let xb = x0 + dir * 40.0;
                paint.flat_rect(Vec2::new(xa.min(xb), z - 0.7), Vec2::new(xa.max(xb), z + 0.7), 0.035, WHITE_PAINT, 1.0);
            }
        }
        for k in 0..3 {
            let xa = x0 + dir * (150.0 + k as f32 * 150.0);
            let xb = xa + dir * 22.0;
            for s in [-1.0, 1.0] {
                paint.flat_rect(Vec2::new(xa.min(xb), rz + s * 9.0 - 1.5), Vec2::new(xa.max(xb), rz + s * 9.0 + 1.5), 0.035, WHITE_PAINT, 1.0);
            }
        }
    }
    let tz = TAXIWAY.center().y;
    paint.flat_rect(Vec2::new(TAXIWAY.min.x, tz - 0.2), Vec2::new(TAXIWAY.max.x, tz + 0.2), 0.035, YELLOW_PAINT, 1.0);

    // terminal
    let lo = Vec3::new(-1160.0, 0.0, -300.0);
    let hi = Vec3::new(-820.0, 16.0, -250.0);
    building(b, rng, lo, hi, Style::Office, rgb(0.85, 0.95, 1.0), true);
    info.footprints.push((Vec2::new(lo.x, lo.z), Vec2::new(hi.x, hi.z), Foot::Building));
    // jet bridges
    for k in 0..5 {
        let x = -1120.0 + k as f32 * 70.0;
        b.b(M::Concrete).cuboid(Vec3::new(x, 5.0, -230.0), Vec3::new(2.0, 1.6, 20.0), rgb(0.85, 0.85, 0.85), 2.0);
        b.solid(Vec3::new(x, 5.0, -230.0), Vec3::new(2.0, 1.6, 20.0));
        b.b(M::Metal).cylinder(Vec3::new(x, 0.0, -214.0), 0.4, 3.4, 8, rgb(0.5, 0.5, 0.5), false);
    }
    // control tower
    let tc = Vec3::new(-1300.0, 0.0, -160.0);
    b.b(M::Concrete).cylinder(tc, 3.5, 34.0, 16, rgb(0.85, 0.84, 0.8), false);
    b.b(M::Concrete).cylinder(tc + Vec3::Y * 34.0, 6.5, 1.0, 20, rgb(0.85, 0.84, 0.8), true);
    b.b(M::Glass).cylinder_at(Affine3A::from_translation(tc + Vec3::Y * 35.0), 6.0, 6.8, 5.0, 20, WHITE, false);
    b.b(M::Metal).cylinder(tc + Vec3::Y * 40.0, 7.0, 0.8, 20, rgb(0.4, 0.4, 0.42), true);
    b.colliders.push((Collider::cylinder(20.0, 6.5), Transform::from_translation(tc + Vec3::Y * 20.0)));
    info.footprints.push((Vec2::new(tc.x - 6.0, tc.z - 6.0), Vec2::new(tc.x + 6.0, tc.z + 6.0), Foot::Building));
    // hangars with arched roofs
    for k in 0..3 {
        let c = Vec3::new(-1440.0 + k as f32 * 62.0, 0.0, -150.0);
        hangar(b, c, 25.0, 30.0, 12.0);
        info.footprints.push((Vec2::new(c.x - 25.0, c.z - 30.0), Vec2::new(c.x + 25.0, c.z + 30.0), Foot::Building));
    }
    // fuel tanks
    for k in 0..3 {
        let c = Vec3::new(-760.0 + k as f32 * 22.0, 0.0, -300.0);
        b.b(M::Metal).cylinder(c, 8.0, 10.0, 24, rgb(0.85, 0.85, 0.85), true);
        b.colliders.push((Collider::cylinder(5.0, 8.0), Transform::from_translation(c + Vec3::Y * 5.0)));
    }
    // airport road lights
    let mut x = -690.0;
    while x < -CITY_HALF - 10.0 {
        streetlight(b, Vec3::new(x, 0.0, -9.0), Vec3::Z);
        streetlight(b, Vec3::new(x, 0.0, 9.0), Vec3::NEG_Z);
        x += 40.0;
    }
}

fn hangar(b: &mut Batch, c: Vec3, hw: f32, hd: f32, wall_h: f32) {
    let col = rgb(0.75, 0.78, 0.8);
    let mut walls = MeshBuilder::new();
    walls.facade_box(Vec3::new(-hw, 0.0, -hd), Vec3::new(hw, wall_h, hd), col, IND_CELL, Vec2::ZERO, None);
    b.b(M::Ind).append(&walls, Affine3A::from_translation(c));
    // arch roof spanning x, running along z
    let seg = 14;
    let rise = 7.0;
    let mut prev = None;
    for i in 0..=seg {
        let t = i as f32 / seg as f32;
        let a = PI * t;
        let p = Vec2::new(-hw * a.cos(), wall_h + rise * a.sin());
        if let Some(q) = prev {
            let q: Vec2 = q;
            let n2 = Vec2::new(-(p.y - q.y), p.x - q.x).normalize();
            let n = Vec3::new(-n2.x, n2.y.abs(), 0.0).normalize();
            let n = if n.y < 0.0 { -n } else { n };
            let pts = [
                c + Vec3::new(q.x, q.y, -hd - 0.5),
                c + Vec3::new(p.x, p.y, -hd - 0.5),
                c + Vec3::new(p.x, p.y, hd + 0.5),
                c + Vec3::new(q.x, q.y, hd + 0.5),
            ];
            let mid = (Vec3::new(p.x, p.y, 0.0) + Vec3::new(q.x, q.y, 0.0)) * 0.5 - Vec3::new(0.0, wall_h, 0.0);
            b.b(M::Metal).quad(pts, mid.normalize_or(n), [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y], rgb(0.55, 0.58, 0.6));
        }
        prev = Some(p);
    }
    // gable ends of the arch
    for z in [-hd, hd] {
        let nrm = Vec3::Z * z.signum();
        for i in 0..seg {
            let a0 = PI * i as f32 / seg as f32;
            let a1 = PI * (i + 1) as f32 / seg as f32;
            let p0 = c + Vec3::new(-hw * a0.cos(), wall_h + rise * a0.sin(), z);
            let p1 = c + Vec3::new(-hw * a1.cos(), wall_h + rise * a1.sin(), z);
            b.b(M::Ind).tri([c + Vec3::new(0.0, wall_h, z), p0, p1], nrm, [Vec2::ZERO, Vec2::X, Vec2::Y], col);
        }
        b.b(M::Dark).cuboid(c + Vec3::new(0.0, wall_h * 0.45, z + nrm.z * 0.05), Vec3::new(hw * 0.7, wall_h * 0.45, 0.05), rgb(0.3, 0.32, 0.34), 1.0);
    }
    b.solid(c + Vec3::Y * (wall_h + rise) * 0.5, Vec3::new(hw, (wall_h + rise) * 0.5, hd));
}

// ------------------------------------------------------------------------------------------------
// Docks
// ------------------------------------------------------------------------------------------------

fn docks(b: &mut Batch, rng: &mut Rng, info: &mut CityInfo) {
    // quay edge line
    b.b(M::Paint).flat_rect(Vec2::new(QUAY.max.x - 1.0, QUAY.min.y), Vec2::new(QUAY.max.x - 0.6, QUAY.max.y), 0.015, YELLOW_PAINT, 1.0);
    for &pz in &PIERS_Z {
        let c = Vec3::new((PIER_X.0 + PIER_X.1) * 0.5, -0.6, pz);
        let half = Vec3::new((PIER_X.1 - PIER_X.0) * 0.5, 0.6, PIER_HALF_WIDTH);
        b.b(M::Concrete).box_at(Affine3A::from_translation(c), half, rgb(0.72, 0.71, 0.68), 6.0, true, true);
        b.solid(c, half);
        info.footprints.push((Vec2::new(PIER_X.0, pz - PIER_HALF_WIDTH), Vec2::new(PIER_X.1, pz + PIER_HALF_WIDTH), Foot::Building));
        // pillars
        let mut x = PIER_X.0 + 6.0;
        while x < PIER_X.1 {
            for s in [-1.0, 1.0] {
                b.b(M::Concrete).cylinder(Vec3::new(x, -14.0, pz + s * (PIER_HALF_WIDTH - 2.0)), 0.7, 13.0, 8, rgb(0.5, 0.5, 0.48), false);
            }
            x += 14.0;
        }
        // bollards and edge paint
        let mut x = PIER_X.0 + 4.0;
        while x < PIER_X.1 - 2.0 {
            for s in [-1.0, 1.0] {
                b.b(M::Metal).cylinder(Vec3::new(x, 0.0, pz + s * (PIER_HALF_WIDTH - 0.8)), 0.25, 0.6, 8, rgb(0.15, 0.15, 0.15), true);
            }
            x += 12.0;
        }
        // containers
        for k in 0..rng.int(3, 6) {
            let x = PIER_X.0 + 20.0 + k as f32 * 24.0;
            let stack = rng.int(1, 4);
            for s in 0..stack {
                let dz = rng.range(-6.0, 6.0);
                container(b, rng, Vec3::new(x, 1.3 + s as f32 * 2.6, pz + dz), FRAC_PI_2);
            }
        }
    }
    // gantry cranes on the quay
    for &cz in &[-230.0f32, -30.0, 170.0] {
        crane(b, Vec3::new(510.0, 0.0, cz));
    }
    // moored cargo ship alongside pier 2
    ship(b, rng, Vec3::new(615.0, SEA_LEVEL, PIERS_Z[1] + PIER_HALF_WIDTH + 14.0), info);
}

fn crane(b: &mut Batch, base: Vec3) {
    let col = rgb(0.9, 0.55, 0.1);
    let leg_h = 32.0;
    for (dx, dz) in [(-8.0, -6.0), (8.0, -6.0), (-8.0, 6.0), (8.0, 6.0)] {
        b.b(M::Metal).cuboid(base + Vec3::new(dx, leg_h * 0.5, dz), Vec3::new(0.6, leg_h * 0.5, 0.6), col, 2.0);
        b.solid(base + Vec3::new(dx, leg_h * 0.5, dz), Vec3::new(0.6, leg_h * 0.5, 0.6));
    }
    for dz in [-6.0, 6.0] {
        b.b(M::Metal).cuboid(base + Vec3::new(0.0, leg_h, dz), Vec3::new(8.6, 0.9, 0.7), col, 2.0);
        b.b(M::Metal).cuboid(base + Vec3::new(0.0, 6.0, dz), Vec3::new(8.6, 0.4, 0.4), col, 2.0);
    }
    // boom over the water
    b.b(M::Metal).cuboid(base + Vec3::new(30.0, leg_h + 2.0, 0.0), Vec3::new(48.0, 1.0, 2.2), col, 2.0);
    b.b(M::Metal).cuboid(base + Vec3::new(0.0, leg_h + 5.0, 0.0), Vec3::new(3.0, 2.5, 3.0), rgb(0.8, 0.8, 0.8), 2.0);
    b.b(M::Metal).cuboid(base + Vec3::new(40.0, leg_h - 6.0, 0.0), Vec3::new(0.06, 7.0, 0.06), rgb(0.2, 0.2, 0.2), 1.0);
    b.b(M::Metal).cuboid(base + Vec3::new(40.0, leg_h - 13.5, 0.0), Vec3::new(1.4, 0.4, 3.0), rgb(0.85, 0.75, 0.1), 1.0);
}

fn ship(b: &mut Batch, rng: &mut Rng, c: Vec3, info: &mut CityInfo) {
    let l = 70.0;
    let beam = 12.0;
    let deck = 7.0;
    let hull_c = rgb(0.12, 0.2, 0.32);
    // hull outline (xz) at deck and keel, pointed bow toward +x
    let top: Vec<Vec3> = [(-l, -beam), (l - 12.0, -beam), (l + 10.0, 0.0), (l - 12.0, beam), (-l, beam)]
        .iter()
        .map(|&(x, z)| c + Vec3::new(x, deck, z))
        .collect();
    let bot: Vec<Vec3> = [(-l + 3.0, -beam * 0.8), (l - 14.0, -beam * 0.8), (l + 4.0, 0.0), (l - 14.0, beam * 0.8), (-l + 3.0, beam * 0.8)]
        .iter()
        .map(|&(x, z)| c + Vec3::new(x, -4.0, z))
        .collect();
    let center = c + Vec3::Y * 2.0;
    for i in 0..top.len() {
        let j = (i + 1) % top.len();
        let q = [bot[i], bot[j], top[j], top[i]];
        let mid = (q[0] + q[1] + q[2] + q[3]) * 0.25;
        let n = (mid - center).with_y(0.0).normalize_or(Vec3::X);
        b.b(M::Metal).quad(q, n, [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y], hull_c);
    }
    b.b(M::Concrete).polygon(&top, Vec3::Y, rgb(0.45, 0.2, 0.15));
    // red waterline band
    b.b(M::Paint).cuboid(c + Vec3::new(-2.0, 0.6, 0.0), Vec3::new(l - 2.0, 0.5, beam * 0.86), rgb(0.6, 0.1, 0.08), 1.0);
    // bridge + funnel
    let br = c + Vec3::new(-l + 12.0, deck, 0.0);
    building(b, rng, br + Vec3::new(-7.0, 0.0, -10.0), br + Vec3::new(7.0, 13.0, 10.0), Style::Res, rgb(0.95, 0.95, 0.95), false);
    b.b(M::Metal).cylinder(br + Vec3::new(-2.0, 13.0, 0.0), 2.2, 6.0, 12, rgb(0.8, 0.3, 0.1), true);
    for k in 0..6 {
        for s in 0..rng.int(1, 3) {
            let dz = rng.range(-5.0, 5.0);
            container(b, rng, c + Vec3::new(-l + 30.0 + k as f32 * 14.0, deck + 1.3 + s as f32 * 2.6, dz), FRAC_PI_2);
        }
    }
    b.solid(c + Vec3::new(0.0, 1.5, 0.0), Vec3::new(l + 4.0, 5.5, beam));
    info.footprints.push((Vec2::new(c.x - l, c.z - beam), Vec2::new(c.x + l + 10.0, c.z + beam), Foot::Building));
}

// ------------------------------------------------------------------------------------------------
// Terrain
// ------------------------------------------------------------------------------------------------

fn terrain_color(h: f32, slope: f32, sandy: bool) -> [f32; 3] {
    if sandy {
        let wet = smoothstep(0.0, -1.5, h);
        [0.95 - 0.3 * wet, 0.85 - 0.3 * wet, 0.65 - 0.25 * wet]
    } else if h < 1.2 {
        [0.85, 0.77, 0.58]
    } else if slope > 0.28 {
        [0.48, 0.45, 0.42]
    } else {
        let g = (h / 120.0).min(1.0);
        [0.36 - 0.06 * g, 0.48 - 0.08 * g, 0.22 - 0.04 * g]
    }
}

/// Heightfield patch with its colouring baked into a texture (raytraced meshes can't
/// carry vertex colours) and a trimesh collider.
#[allow(clippy::too_many_arguments)]
fn terrain_patch(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    area: Area,
    cell: f32,
    height: fn(f32, f32) -> f32,
    sandy: bool,
) {
    let nx = (area.size().x / cell).ceil() as u32;
    let nz = (area.size().y / cell).ceil() as u32;
    let dx = area.size().x / nx as f32;
    let dz = area.size().y / nz as f32;
    let normal_at = |x: f32, z: f32| {
        let e = 1.0;
        Vec3::new(height(x - e, z) - height(x + e, z), 2.0 * e, height(x, z - e) - height(x, z + e)).normalize()
    };
    let mut mb = MeshBuilder::new();
    for j in 0..=nz {
        for i in 0..=nx {
            let x = area.min.x + i as f32 * dx;
            let z = area.min.y + j as f32 * dz;
            mb.pos.push([x, height(x, z), z]);
            mb.nrm.push(normal_at(x, z).to_array());
            mb.uv.push([i as f32 / nx as f32, j as f32 / nz as f32]);
            mb.col.push(WHITE);
        }
    }
    let row = nx + 1;
    for j in 0..nz {
        for i in 0..nx {
            let a = j * row + i;
            let b = a + 1;
            let c = a + row;
            let d = c + 1;
            // CCW seen from +Y: (a, c, b) and (b, c, d) since z grows "down" the grid
            mb.idx.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    // bake colour x detail noise into a texture spanning the whole patch
    let tw = 2048u32;
    let th = ((tw as f32 * area.size().y / area.size().x) as u32).clamp(256, 2048).next_power_of_two();
    let mut tex = textures::Tex::new(tw, th, [1.0; 4]);
    for y in 0..th {
        for x in 0..tw {
            let u = (x as f32 + 0.5) / tw as f32;
            let v = (y as f32 + 0.5) / th as f32;
            let wx = area.min.x + u * area.size().x;
            let wz = area.min.y + v * area.size().y;
            let h = height(wx, wz);
            let slope = 1.0 - normal_at(wx, wz).y;
            let c = terrain_color(h, slope, sandy);
            let n = textures::fbm(wx / 96.0, wz / 96.0, 64, 4, 9) * 0.3 + textures::fbm(wx / 6.0, wz / 6.0, 64, 2, 3) * 0.16;
            let k = 0.8 + n;
            tex.set(x, y, [c[0] * k, c[1] * k, c[2] * k, 1.0]);
        }
    }
    let mat = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(to_image(&tex, true))),
        perceptual_roughness: 0.95,
        ..default()
    });
    let (v, t) = mb.collider_data();
    commands.spawn((Mesh3d(meshes.add(mb.build())), MeshMaterial3d(mat), Transform::IDENTITY));
    match Collider::trimesh(v, t) {
        Ok(c) => {
            commands.spawn((c, Transform::IDENTITY));
        }
        Err(e) => error!("terrain collider failed: {e:?}"),
    }
}

fn hill_props(b: &mut Batch, rng: &mut Rng) {
    let mut placed = 0;
    let mut tries = 0;
    while placed < 260 && tries < 4000 {
        tries += 1;
        let x = rng.range(HILL.min.x + 40.0, HILL.max.x - 40.0);
        let z = rng.range(HILL.min.y + 40.0, HILL.max.y - 25.0);
        let h = hill_height(x, z);
        if h < 3.0 {
            continue;
        }
        let e = 2.0;
        let slope = ((hill_height(x + e, z) - hill_height(x - e, z)).powi(2) + (hill_height(x, z + e) - hill_height(x, z - e)).powi(2)).sqrt() / (2.0 * e);
        if slope > 0.55 {
            continue;
        }
        let th = rng.range(6.0, 12.0);
        tree(b, rng, Vec3::new(x, h - 0.2, z), th);
        placed += 1;
    }
    // observatory on the summit
    let (mut bx, mut bz, mut bh) = (0.0, -840.0, f32::MIN);
    for i in -10..=10 {
        for j in -10..=10 {
            let x = i as f32 * 8.0;
            let z = -840.0 + j as f32 * 8.0;
            let h = hill_height(x, z);
            if h > bh {
                bh = h;
                bx = x;
                bz = z;
            }
        }
    }
    let base = Vec3::new(bx, bh - 1.0, bz);
    b.b(M::Concrete).cylinder(base, 14.0, 9.0, 28, rgb(0.92, 0.9, 0.86), true);
    b.b(M::Metal).ellipsoid(base + Vec3::Y * 9.0, Vec3::new(10.0, 9.0, 10.0), 20, 12, rgb(0.85, 0.85, 0.88));
    b.colliders.push((Collider::cylinder(8.0, 14.0), Transform::from_translation(base + Vec3::Y * 8.0)));
}

fn beach_props(b: &mut Batch, rng: &mut Rng) {
    // promenade slab (concrete) along the city's southern edge
    let z0 = CITY_HALF;
    let z1 = CITY_HALF + PROMENADE_DEPTH;
    let c = Vec3::new(0.0, -3.0, (z0 + z1) * 0.5);
    let half = Vec3::new(CITY_HALF, 3.0, PROMENADE_DEPTH * 0.5);
    b.b(M::Sidewalk).box_at(Affine3A::from_translation(c), half, rgb(0.95, 0.9, 0.82), 2.0, true, false);
    b.solid(c, half);
    // sea wall rail
    b.b(M::Metal).cuboid(Vec3::new(0.0, 0.9, z1 - 0.3), Vec3::new(CITY_HALF, 0.04, 0.04), rgb(0.85, 0.85, 0.85), 1.0);
    let mut x = -CITY_HALF + 3.0;
    while x < CITY_HALF {
        b.b(M::Metal).cylinder(Vec3::new(x, 0.0, z1 - 0.3), 0.04, 0.9, 5, rgb(0.85, 0.85, 0.85), false);
        x += 3.0;
    }
    // palms and lifeguard towers
    let mut x = -CITY_HALF + 10.0;
    while x < CITY_HALF - 10.0 {
        let dx = rng.range(-3.0, 3.0);
        let h = rng.range(8.0, 12.0);
        palm(b, rng, Vec3::new(x + dx, 0.0, z0 + 6.0), h);
        x += 26.0;
    }
    for k in 0..5 {
        let x = -360.0 + k as f32 * 180.0;
        let z = z1 + 45.0;
        let y = beach_height(x, z);
        let p = Vec3::new(x, y, z);
        for (dx, dz) in [(-1.2, -1.2), (1.2, -1.2), (-1.2, 1.2), (1.2, 1.2)] {
            b.b(M::Bark).cylinder(p + Vec3::new(dx, 0.0, dz), 0.1, 2.6, 5, rgb(1.6, 1.5, 1.4), false);
        }
        b.b(M::Paint).cuboid(p + Vec3::Y * 3.5, Vec3::new(1.7, 1.0, 1.7), rgb(0.95, 0.85, 0.2), 1.0);
        b.b(M::TileRoof).cuboid(p + Vec3::Y * 4.65, Vec3::new(2.0, 0.15, 2.0), rgb(0.85, 0.15, 0.1), 1.0);
        b.solid(p + Vec3::Y * 2.3, Vec3::new(1.7, 2.3, 1.7));
    }
}

/// Benches, bins, hydrants, newspaper boxes, bus shelters and rooftop billboards.
fn street_props(b: &mut Batch, rng: &mut Rng, x0: f32, z0: f32, dist: District) {
    if matches!(dist, District::Park | District::Hospital | District::AutoShop) {
        return;
    }
    let busy = matches!(dist, District::Downtown | District::Midtown | District::Beachfront);
    // (position, facing out to the road)
    let mut spots = Vec::new();
    for k in 0..5 {
        let t = 6.0 + k as f32 * 17.0;
        spots.push((Vec3::new(x0 + t, CURB, z0 + 1.6), Vec3::NEG_Z));
        spots.push((Vec3::new(x0 + t, CURB, z0 + BLOCK - 1.6), Vec3::Z));
        spots.push((Vec3::new(x0 + 1.6, CURB, z0 + t), Vec3::NEG_X));
        spots.push((Vec3::new(x0 + BLOCK - 1.6, CURB, z0 + t), Vec3::X));
    }
    let mut shelter_done = false;
    for (p, out) in spots {
        let yaw = Quat::from_rotation_arc(Vec3::NEG_Z, out);
        let roll = rng.f();
        if busy && !shelter_done && roll < 0.06 {
            // bus shelter: glass back, roof, bench, sign
            shelter_done = true;
            let c = p - out * 0.6;
            let m = Affine3A::from_rotation_translation(yaw, c);
            let pole = rgb(0.2, 0.22, 0.25);
            for sx in [-1.6f32, 1.6] {
                b.b(M::Metal).box_at(m * Affine3A::from_translation(Vec3::new(sx, 1.2, 0.7)), Vec3::new(0.05, 1.2, 0.05), pole, 1.0, true, true);
            }
            b.b(M::Glass).box_at(m * Affine3A::from_translation(Vec3::new(0.0, 1.25, 0.72)), Vec3::new(1.6, 1.0, 0.02), WHITE, 1.0, true, true);
            b.b(M::Metal).box_at(m * Affine3A::from_translation(Vec3::new(0.0, 2.45, 0.2)), Vec3::new(1.75, 0.05, 0.75), pole, 1.0, true, true);
            b.b(M::Bark).box_at(m * Affine3A::from_translation(Vec3::new(0.0, 0.45, 0.45)), Vec3::new(1.2, 0.04, 0.2), rgb(0.55, 0.4, 0.28), 1.0, true, true);
            b.b(M::Paint).box_at(m * Affine3A::from_translation(Vec3::new(1.85, 2.0, 0.0)), Vec3::new(0.25, 0.35, 0.03), rgb(0.1, 0.4, 0.8), 1.0, true, true);
            b.b(M::Metal).cylinder(p + yaw * Vec3::new(1.85, 0.0, 0.0), 0.04, 1.7, 6, pole, false);
            continue;
        }
        match (roll * 10.0) as u32 {
            0 | 1 => {
                // bench: wooden seat + back on cast legs
                let m = Affine3A::from_rotation_translation(yaw, p - out * 0.2);
                b.b(M::Bark).box_at(m * Affine3A::from_translation(Vec3::new(0.0, 0.45, 0.0)), Vec3::new(0.85, 0.03, 0.2), rgb(0.6, 0.42, 0.28), 1.0, true, true);
                b.b(M::Bark).box_at(m * Affine3A::from_translation(Vec3::new(0.0, 0.75, 0.2)), Vec3::new(0.85, 0.12, 0.025), rgb(0.6, 0.42, 0.28), 1.0, true, true);
                for sx in [-0.7f32, 0.7] {
                    b.b(M::Metal).box_at(m * Affine3A::from_translation(Vec3::new(sx, 0.3, 0.05)), Vec3::new(0.03, 0.3, 0.22), rgb(0.15, 0.15, 0.15), 1.0, true, true);
                }
            }
            2 => {
                // litter bin
                b.b(M::Metal).cylinder(p, 0.28, 0.95, 12, rgb(0.12, 0.3, 0.18), true);
                b.b(M::Metal).cylinder(p + Vec3::Y * 0.95, 0.31, 0.08, 12, rgb(0.1, 0.1, 0.1), true);
                b.colliders.push((Collider::cylinder(0.5, 0.3), Transform::from_translation(p + Vec3::Y * 0.5)));
            }
            3 => {
                // fire hydrant
                let red = rgb(0.75, 0.08, 0.06);
                b.b(M::Paint).cylinder(p, 0.15, 0.6, 10, red, true);
                b.b(M::Paint).ellipsoid(p + Vec3::Y * 0.62, Vec3::new(0.15, 0.12, 0.15), 10, 5, red);
                b.b(M::Paint).cylinder_at(Affine3A::from_rotation_translation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2), p + Vec3::new(0.2, 0.42, 0.0)), 0.06, 0.06, 0.4, 8, red, true);
                b.colliders.push((Collider::cylinder(0.35, 0.16), Transform::from_translation(p + Vec3::Y * 0.35)));
            }
            4 if busy => {
                // newspaper boxes
                for (k, col) in [rgb(0.8, 0.15, 0.1), rgb(0.1, 0.3, 0.7)].into_iter().enumerate() {
                    let c = p + yaw * Vec3::new(k as f32 * 0.5 - 0.25, 0.5, 0.0);
                    b.b(M::Paint).box_at(Affine3A::from_rotation_translation(yaw, c), Vec3::new(0.22, 0.5, 0.2), col, 1.0, true, true);
                }
            }
            _ => {}
        }
    }
}

/// Lit billboard on a rooftop (emissive panel faces the street).
pub fn billboard(b: &mut Batch, rng: &mut Rng, top: Vec3, facing: Vec3) {
    let yaw = Quat::from_rotation_arc(Vec3::Z, facing);
    let w = rng.range(6.0, 10.0);
    let h = w * 0.42;
    let m = Affine3A::from_rotation_translation(yaw, top + Vec3::Y * (h * 0.5 + 2.5));
    let frame = rgb(0.15, 0.15, 0.17);
    b.b(M::Metal).box_at(m, Vec3::new(w * 0.5 + 0.15, h * 0.5 + 0.15, 0.12), frame, 1.0, true, true);
    let cols = [rgb(0.95, 0.2, 0.35), rgb(0.1, 0.6, 0.95), rgb(0.95, 0.65, 0.1), rgb(0.3, 0.85, 0.4), rgb(0.7, 0.3, 0.95)];
    let a = *rng.pick(&cols);
    let c2 = *rng.pick(&cols);
    // two-tone advert panel + stripe (Lamp material glows at night)
    b.b(M::Paint).box_at(m * Affine3A::from_translation(Vec3::new(0.0, 0.0, 0.14)), Vec3::new(w * 0.5, h * 0.5, 0.02), a, 1.0, true, true);
    b.b(M::Paint).box_at(m * Affine3A::from_translation(Vec3::new(-w * 0.15, h * 0.1, 0.17)), Vec3::new(w * 0.25, h * 0.22, 0.02), c2, 1.0, true, true);
    b.b(M::Lamp).box_at(m * Affine3A::from_translation(Vec3::new(0.0, -h * 0.35, 0.17)), Vec3::new(w * 0.45, 0.08, 0.02), WHITE, 1.0, true, true);
    for sx in [-w * 0.3, w * 0.3] {
        b.b(M::Metal).box_at(Affine3A::from_rotation_translation(yaw, top + yaw * Vec3::new(sx, 1.25, -0.2)), Vec3::new(0.08, 1.25, 0.08), frame, 1.0, true, true);
    }
}
