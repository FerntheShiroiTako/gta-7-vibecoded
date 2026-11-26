//! Procedurally generated textures (with full CPU-built mip chains so window
//! grids and road noise don't shimmer at distance).

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::meshgen::Rng;

pub struct Tex {
    pub w: u32,
    pub h: u32,
    pub px: Vec<[f32; 4]>, // linear-ish working values 0..1
}

impl Tex {
    pub fn new(w: u32, h: u32, fill: [f32; 4]) -> Self {
        Tex { w, h, px: vec![fill; (w * h) as usize] }
    }
    pub fn set(&mut self, x: u32, y: u32, c: [f32; 4]) {
        let i = (y * self.w + x) as usize;
        self.px[i] = c;
    }
    pub fn get(&self, x: u32, y: u32) -> [f32; 4] {
        self.px[(y * self.w + x) as usize]
    }
}

fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(668265263) ^ seed.wrapping_mul(2246822519);
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    h ^= h >> 16;
    (h & 0xffffff) as f32 / 16777215.0
}

/// Tileable value noise with integer period `p` lattice cells across the texture.
fn vnoise(u: f32, v: f32, p: i32, seed: u32) -> f32 {
    let x = u * p as f32;
    let y = v * p as f32;
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);
    let w = |a: i32| a.rem_euclid(p);
    let a = hash2(w(x0), w(y0), seed);
    let b = hash2(w(x0 + 1), w(y0), seed);
    let c = hash2(w(x0), w(y0 + 1), seed);
    let d = hash2(w(x0 + 1), w(y0 + 1), seed);
    let top = a + (b - a) * sx;
    let bot = c + (d - c) * sx;
    top + (bot - top) * sy
}

pub fn fbm(u: f32, v: f32, base: i32, oct: u32, seed: u32) -> f32 {
    let mut s = 0.0;
    let mut amp = 0.5;
    let mut p = base;
    let mut norm = 0.0;
    for o in 0..oct {
        s += vnoise(u, v, p, seed + o * 17) * amp;
        norm += amp;
        amp *= 0.5;
        p *= 2;
    }
    s / norm
}

/// Convert to an Image with a full mip chain. `srgb` for colour data.
pub fn to_image(t: &Tex, srgb: bool) -> Image {
    let mut levels: Vec<Vec<[f32; 4]>> = vec![t.px.clone()];
    let (mut w, mut h) = (t.w, t.h);
    while w > 1 && h > 1 {
        let prev = levels.last().unwrap();
        let nw = w / 2;
        let nh = h / 2;
        let mut next = vec![[0.0f32; 4]; (nw * nh) as usize];
        for y in 0..nh {
            for x in 0..nw {
                let mut acc = [0.0f32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let p = prev[((y * 2 + dy) * w + x * 2 + dx) as usize];
                    for k in 0..4 {
                        acc[k] += p[k] * 0.25;
                    }
                }
                next[(y * nw + x) as usize] = acc;
            }
        }
        levels.push(next);
        w = nw;
        h = nh;
    }
    let mut data = Vec::new();
    for lvl in &levels {
        for p in lvl {
            for k in 0..4 {
                data.push((p[k].clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
            }
        }
    }
    let format = if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm };
    let lvl0: Vec<u8> = data[..(t.w * t.h * 4) as usize].to_vec();
    let mut img = Image::new(
        Extent3d { width: t.w, height: t.h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        lvl0,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.data = Some(data);
    img.texture_descriptor.mip_level_count = levels.len() as u32;
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    });
    img
}

/// metallic-roughness texel (glTF convention: G = roughness, B = metallic).
fn mr(rough: f32, metal: f32) -> [f32; 4] {
    [1.0, rough, metal, 1.0]
}

pub struct FacadeSet {
    pub albedo: Tex,
    pub mr: Tex,
    pub emissive: Tex,
}

pub const FACADE_CELLS: u32 = 8;
const CELL: u32 = 64;

/// Glass curtain wall for offices/towers.
pub fn facade_office(seed: u64) -> FacadeSet {
    let n = FACADE_CELLS * CELL;
    let mut a = Tex::new(n, n, [0.0; 4]);
    let mut m = Tex::new(n, n, [0.0; 4]);
    let mut e = Tex::new(n, n, [0.0, 0.0, 0.0, 1.0]);
    let mut rng = Rng::new(seed);
    for cy in 0..FACADE_CELLS {
        for cx in 0..FACADE_CELLS {
            let lit = rng.chance(0.38);
            let warm = rng.range(0.75, 1.0);
            let blind = rng.range(0.0, 0.35);
            let tint = rng.range(-0.03, 0.03);
            for y in 0..CELL {
                for x in 0..CELL {
                    let gx = cx * CELL + x;
                    let gy = cy * CELL + y;
                    let spandrel = y < 11;
                    let mullion = x < 3 || x > CELL - 4;
                    let (col, mrv, em) = if spandrel {
                        let v = 0.42 + 0.04 * hash2(gx as i32, gy as i32, 3);
                        ([v, v, v * 1.02, 1.0], mr(0.65, 0.0), [0.0, 0.0, 0.0, 1.0])
                    } else if mullion {
                        ([0.62, 0.64, 0.66, 1.0], mr(0.3, 0.9), [0.0, 0.0, 0.0, 1.0])
                    } else {
                        let g = (y as f32 - 11.0) / (CELL as f32 - 11.0);
                        let b = 0.16 + 0.10 * g + tint;
                        let blinds = if g < blind { 0.25 } else { 0.0 };
                        let c = [0.06 + blinds + tint, 0.11 + blinds * 0.9 + tint, b + blinds * 0.7, 1.0];
                        let emv = if lit {
                            let f = 0.6 + 0.4 * (1.0 - g);
                            [1.0 * f * warm, 0.78 * f * warm, 0.48 * f, 1.0]
                        } else {
                            [0.0, 0.0, 0.0, 1.0]
                        };
                        (c, mr(0.06, 0.0), emv)
                    };
                    a.set(gx, gy, col);
                    m.set(gx, gy, mrv);
                    e.set(gx, gy, em);
                }
            }
        }
    }
    FacadeSet { albedo: a, mr: m, emissive: e }
}

/// Punched windows in a light wall (tinted by vertex colour).
pub fn facade_residential(seed: u64) -> FacadeSet {
    let n = FACADE_CELLS * CELL;
    let mut a = Tex::new(n, n, [0.0; 4]);
    let mut m = Tex::new(n, n, [0.0; 4]);
    let mut e = Tex::new(n, n, [0.0, 0.0, 0.0, 1.0]);
    let mut rng = Rng::new(seed);
    for cy in 0..FACADE_CELLS {
        for cx in 0..FACADE_CELLS {
            let lit = rng.chance(0.32);
            let curtain = rng.range(0.0, 1.0);
            for y in 0..CELL {
                for x in 0..CELL {
                    let gx = cx * CELL + x;
                    let gy = cy * CELL + y;
                    let wn = fbm(gx as f32 / n as f32, gy as f32 / n as f32, 32, 3, 11);
                    let wall = 0.82 + 0.12 * (wn - 0.5);
                    let win = (14..50).contains(&x) && (14..48).contains(&y);
                    let frame = (12..52).contains(&x) && (12..50).contains(&y) && !win;
                    let sill = (10..54).contains(&x) && (50..54).contains(&y);
                    let (col, mrv, em) = if win {
                        let g = (y - 14) as f32 / 34.0;
                        let cur = if curtain > 0.6 && (x < 22 || x > 41) { 0.35 } else { 0.0 };
                        let c = [0.08 + cur, 0.12 + cur * 0.8, 0.17 + 0.08 * g + cur * 0.6, 1.0];
                        let emv = if lit { [0.95, 0.7, 0.42, 1.0] } else { [0.0, 0.0, 0.0, 1.0] };
                        (c, mr(0.08, 0.0), emv)
                    } else if frame {
                        ([0.9, 0.9, 0.88, 1.0], mr(0.5, 0.0), [0.0, 0.0, 0.0, 1.0])
                    } else if sill {
                        ([0.7, 0.7, 0.68, 1.0], mr(0.8, 0.0), [0.0, 0.0, 0.0, 1.0])
                    } else {
                        ([wall, wall, wall, 1.0], mr(0.92, 0.0), [0.0, 0.0, 0.0, 1.0])
                    };
                    a.set(gx, gy, col);
                    m.set(gx, gy, mrv);
                    e.set(gx, gy, em);
                }
            }
        }
    }
    FacadeSet { albedo: a, mr: m, emissive: e }
}

/// Corrugated metal cladding with a high clerestory window band.
pub fn facade_industrial(seed: u64) -> FacadeSet {
    let n = FACADE_CELLS * CELL;
    let mut a = Tex::new(n, n, [0.0; 4]);
    let mut m = Tex::new(n, n, [0.0; 4]);
    let mut e = Tex::new(n, n, [0.0, 0.0, 0.0, 1.0]);
    let mut rng = Rng::new(seed);
    for cy in 0..FACADE_CELLS {
        let band_lit = rng.chance(0.3);
        for y in 0..CELL {
            for gx in 0..n {
                let gy = cy * CELL + y;
                let rib = 0.5 + 0.5 * ((gx as f32) * std::f32::consts::TAU / 16.0).sin();
                let dirt = fbm(gx as f32 / n as f32, gy as f32 / n as f32, 16, 4, 5);
                let v = 0.62 + 0.18 * rib - 0.22 * (dirt - 0.4).max(0.0);
                let window = cy % 4 == 0 && (6..20).contains(&y) && (gx % 64) > 4 && (gx % 64) < 60;
                if window {
                    a.set(gx, gy, [0.1, 0.13, 0.15, 1.0]);
                    m.set(gx, gy, mr(0.15, 0.0));
                    if band_lit {
                        e.set(gx, gy, [0.8, 0.85, 0.9, 1.0]);
                    }
                } else {
                    a.set(gx, gy, [v, v, v, 1.0]);
                    m.set(gx, gy, mr(0.45 + 0.3 * dirt, 0.7));
                }
            }
        }
    }
    FacadeSet { albedo: a, mr: m, emissive: e }
}

pub fn noise_texture(size: u32, base: [f32; 3], var: [f32; 3], period: i32, seed: u32, speckle: f32) -> Tex {
    let mut t = Tex::new(size, size, [0.0; 4]);
    for y in 0..size {
        for x in 0..size {
            let u = x as f32 / size as f32;
            let v = y as f32 / size as f32;
            let n = fbm(u, v, period, 5, seed) - 0.5;
            let s = if hash2(x as i32, y as i32, seed + 99) < speckle { 0.12 } else { 0.0 };
            let c = [
                (base[0] + var[0] * n + s).clamp(0.0, 1.0),
                (base[1] + var[1] * n + s).clamp(0.0, 1.0),
                (base[2] + var[2] * n + s).clamp(0.0, 1.0),
                1.0,
            ];
            t.set(x, y, c);
        }
    }
    t
}

/// Concrete paving slabs: `tiles` slabs across the texture.
pub fn paving_texture(size: u32, tiles: u32, seed: u32) -> Tex {
    let mut t = noise_texture(size, [0.64, 0.63, 0.6], [0.12, 0.12, 0.12], 16, seed, 0.02);
    let step = size / tiles;
    for y in 0..size {
        for x in 0..size {
            if x % step < 2 || y % step < 2 {
                let c = t.get(x, y);
                t.set(x, y, [c[0] * 0.62, c[1] * 0.62, c[2] * 0.62, 1.0]);
            }
        }
    }
    t
}

pub fn white_tex() -> Tex {
    Tex::new(4, 4, [1.0; 4])
}
