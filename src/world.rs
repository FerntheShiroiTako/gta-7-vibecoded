//! World layout shared by city generation, physics, AI and the minimap.
//! Coordinates: x = east, z = south, y = up. Metres.

use bevy::prelude::*;

pub const ROAD: f32 = 14.0;
pub const BLOCK: f32 = 80.0;
pub const PITCH: f32 = ROAD + BLOCK;
pub const GRID: i32 = 10;
pub const CITY_HALF: f32 = (GRID as f32 * PITCH + ROAD) * 0.5; // 477
pub const SIDEWALK: f32 = 4.0;
pub const CURB: f32 = 0.15;
pub const SEA_LEVEL: f32 = -0.6;
pub const LANE: f32 = 3.5;

/// Western edge of road `i` (roads run both along x and along z).
pub fn road_min(i: i32) -> f32 {
    -CITY_HALF + i as f32 * PITCH
}
pub fn road_center(i: i32) -> f32 {
    road_min(i) + ROAD * 0.5
}
pub fn block_min(b: i32) -> f32 {
    road_min(b) + ROAD
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum District {
    Downtown,
    Midtown,
    Suburb,
    Industrial,
    Beachfront,
    Park,
    Hospital,
    PoliceHq,
    AutoShop,
}

pub fn district(bx: i32, bz: i32) -> District {
    match (bx, bz) {
        (1, 6) => return District::Hospital,
        (7, 4) => return District::PoliceHq,
        (2, 5) | (8, 2) => return District::AutoShop,
        (2, 3) | (6, 7) | (4, 1) => return District::Park,
        _ => {}
    }
    if bx == GRID - 1 {
        District::Industrial
    } else if bz == GRID - 1 {
        District::Beachfront
    } else if (3..=6).contains(&bx) && (3..=6).contains(&bz) {
        District::Downtown
    } else if bx <= 1 || bz <= 1 {
        District::Suburb
    } else {
        District::Midtown
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Area {
    pub min: Vec2,
    pub max: Vec2,
}
impl Area {
    pub const fn new(x0: f32, z0: f32, x1: f32, z1: f32) -> Self {
        Area { min: Vec2::new(x0, z0), max: Vec2::new(x1, z1) }
    }
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
    pub fn center(&self) -> Vec2 {
        (self.min + self.max) * 0.5
    }
    pub fn size(&self) -> Vec2 {
        self.max - self.min
    }
}

pub const CITY: Area = Area::new(-CITY_HALF, -CITY_HALF, CITY_HALF, CITY_HALF);
pub const AIRPORT: Area = Area::new(-1560.0, -330.0, -CITY_HALF, 330.0);
pub const RUNWAY: Area = Area::new(-1490.0, 95.0, -630.0, 140.0);
pub const TAXIWAY: Area = Area::new(-1490.0, 30.0, -630.0, 48.0);
pub const APRON: Area = Area::new(-1260.0, -215.0, -700.0, 30.0);
pub const AIRPORT_ROAD: Area = Area::new(-700.0, -7.0, -CITY_HALF, 7.0);
pub const QUAY: Area = Area::new(CITY_HALF, -CITY_HALF, 530.0, CITY_HALF);
pub const PIER_X: (f32, f32) = (530.0, 700.0);
pub const PIER_HALF_WIDTH: f32 = 17.0;
pub const PIERS_Z: [f32; 4] = [-330.0, -130.0, 70.0, 270.0];
pub const BEACH: Area = Area::new(-CITY_HALF - 70.0, CITY_HALF, CITY_HALF, CITY_HALF + 170.0);
pub const PROMENADE_DEPTH: f32 = 18.0;
pub const HILL: Area = Area::new(-620.0, -1190.0, 620.0, -CITY_HALF);

pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Terrain height of the northern hill (0 at the city edge, below sea at the coast).
pub fn hill_height(x: f32, z: f32) -> f32 {
    let peak = 118.0 * (-((x / 330.0).powi(2) + ((z + 840.0) / 235.0).powi(2))).exp();
    let ridge = 38.0 * (-(((x + 260.0) / 170.0).powi(2) + ((z + 650.0) / 140.0).powi(2))).exp();
    let shoulder = 24.0 * (-(((x - 300.0) / 150.0).powi(2) + ((z + 900.0) / 160.0).powi(2))).exp();
    let noise = 3.5 * (x * 0.021).sin() * (z * 0.017).cos() + 1.6 * (x * 0.053 + z * 0.041).sin();
    let base = peak + ridge + shoulder + noise * smoothstep(0.0, 40.0, peak + ridge);
    let edge = (HILL.max.x - x.abs()).min(z - HILL.min.y);
    let coast = smoothstep(0.0, 150.0, edge);
    let south = smoothstep(0.0, 70.0, -CITY_HALF - z);
    (base * coast + (coast - 1.0) * 9.0) * south
}

/// Beach height south of the city (promenade is flat at 0).
pub fn beach_height(x: f32, z: f32) -> f32 {
    let d = z - (CITY_HALF + PROMENADE_DEPTH);
    if d <= 0.0 {
        return 0.0;
    }
    let slope = -4.0 * smoothstep(0.0, 150.0, d);
    let dunes = 0.25 * (x * 0.08).sin() * (z * 0.11).cos() * (1.0 - smoothstep(30.0, 90.0, d));
    let side = smoothstep(0.0, 60.0, (x - BEACH.min.x).min(BEACH.max.x - x));
    (slope + dunes - 0.02) * side + (side - 1.0) * 4.0
}

/// Approximate ground height anywhere (used for spawning and AI).
pub fn ground_height(x: f32, z: f32) -> f32 {
    let p = Vec2::new(x, z);
    if HILL.contains(p) && z < -CITY_HALF {
        hill_height(x, z)
    } else if BEACH.contains(p) && z > CITY_HALF {
        beach_height(x, z)
    } else if is_land(p) {
        0.0
    } else {
        SEA_LEVEL - 4.0
    }
}

pub fn on_pier(p: Vec2) -> bool {
    p.x >= PIER_X.0 && p.x <= PIER_X.1 && PIERS_Z.iter().any(|z| (p.y - z).abs() <= PIER_HALF_WIDTH)
}

pub fn is_land(p: Vec2) -> bool {
    CITY.contains(p) || AIRPORT.contains(p) || QUAY.contains(p) || on_pier(p)
}

/// Block-local check: is this point on a road (not on a block)?
pub fn on_road(p: Vec2) -> bool {
    if !CITY.contains(p) {
        return AIRPORT_ROAD.contains(p);
    }
    let lx = (p.x + CITY_HALF).rem_euclid(PITCH);
    let lz = (p.y + CITY_HALF).rem_euclid(PITCH);
    lx < ROAD || lz < ROAD
}

pub fn spawn_point() -> Vec3 {
    // Sidewalk on the south-west corner of downtown block (4,5).
    Vec3::new(block_min(4) + 2.0, CURB + 1.0, block_min(5) + BLOCK - 2.0)
}

/// Blocks with a Spray & Go respray garage.
pub const AUTO_SHOPS: [(i32, i32); 2] = [(2, 5), (8, 2)];

/// Centre of the respray bay floor of shop `i` (the door faces +z, onto the street).
pub fn respray_bay(i: usize) -> Vec3 {
    let (bx, bz) = AUTO_SHOPS[i];
    Vec3::new(block_min(bx) + 26.0, CURB, block_min(bz) + BLOCK - SIDEWALK - 7.0)
}
pub const BAY_HALF: Vec3 = Vec3::new(5.0, 5.2, 7.0);

pub fn hospital_point() -> Vec3 {
    Vec3::new(block_min(1) + BLOCK * 0.5, CURB + 1.0, block_min(6) + BLOCK - 2.0)
}

pub fn police_point() -> Vec3 {
    Vec3::new(block_min(7) + BLOCK * 0.5, CURB + 1.0, block_min(4) + BLOCK - 2.0)
}
