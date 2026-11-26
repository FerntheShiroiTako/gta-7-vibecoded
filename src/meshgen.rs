//! Procedural mesh building: boxes, cylinders, ellipsoids, extruded profiles.
//! Every face is emitted with an *expected outward normal* and the winding is
//! fixed automatically, so back-face culling never eats geometry.

use bevy::asset::RenderAssetUsages;
use bevy::math::Affine3A;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::prelude::*;

pub type Rgba = [f32; 4];

pub const WHITE: Rgba = [1.0, 1.0, 1.0, 1.0];

pub fn rgb(r: f32, g: f32, b: f32) -> Rgba {
    [r, g, b, 1.0]
}

#[derive(Default, Clone)]
pub struct MeshBuilder {
    pub pos: Vec<[f32; 3]>,
    pub nrm: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub col: Vec<Rgba>,
    pub idx: Vec<u32>,
}

impl MeshBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.idx.is_empty()
    }

    pub fn vertex_count(&self) -> usize {
        self.pos.len()
    }

    fn vert(&mut self, p: Vec3, n: Vec3, uv: Vec2, c: Rgba) -> u32 {
        let i = self.pos.len() as u32;
        self.pos.push(p.to_array());
        self.nrm.push(n.to_array());
        self.uv.push(uv.to_array());
        self.col.push(c);
        i
    }

    /// Triangle whose front face points along `n`.
    pub fn tri(&mut self, p: [Vec3; 3], n: Vec3, uv: [Vec2; 3], c: Rgba) {
        let flip = (p[1] - p[0]).cross(p[2] - p[0]).dot(n) < 0.0;
        let a = self.vert(p[0], n, uv[0], c);
        let b = self.vert(p[1], n, uv[1], c);
        let d = self.vert(p[2], n, uv[2], c);
        if flip {
            self.idx.extend_from_slice(&[a, d, b]);
        } else {
            self.idx.extend_from_slice(&[a, b, d]);
        }
    }

    /// Quad p0..p3 (in loop order) whose front face points along `n`.
    pub fn quad(&mut self, p: [Vec3; 4], n: Vec3, uv: [Vec2; 4], c: Rgba) {
        let flip = (p[1] - p[0]).cross(p[2] - p[0]).dot(n) < 0.0;
        let i: Vec<u32> = (0..4).map(|k| self.vert(p[k], n, uv[k], c)).collect();
        if flip {
            self.idx.extend_from_slice(&[i[0], i[2], i[1], i[0], i[3], i[2]]);
        } else {
            self.idx.extend_from_slice(&[i[0], i[1], i[2], i[0], i[2], i[3]]);
        }
    }

    /// Quad with per-vertex normals (smooth shading). `face_n` decides the winding.
    pub fn quad_smooth(&mut self, p: [Vec3; 4], n: [Vec3; 4], face_n: Vec3, uv: [Vec2; 4], c: Rgba) {
        let flip = (p[1] - p[0]).cross(p[2] - p[0]).dot(face_n) < 0.0;
        let i: Vec<u32> = (0..4).map(|k| self.vert(p[k], n[k], uv[k], c)).collect();
        if flip {
            self.idx.extend_from_slice(&[i[0], i[2], i[1], i[0], i[3], i[2]]);
        } else {
            self.idx.extend_from_slice(&[i[0], i[1], i[2], i[0], i[2], i[3]]);
        }
    }

    /// Axis-aligned box. `uv_scale` = metres per UV unit on every face.
    pub fn cuboid(&mut self, center: Vec3, half: Vec3, c: Rgba, uv_scale: f32) {
        self.box_at(Affine3A::from_translation(center), half, c, uv_scale, true, true);
    }

    /// Oriented box. `top`/`bottom` toggle those faces (skip hidden faces).
    pub fn box_at(&mut self, m: Affine3A, half: Vec3, c: Rgba, uv_scale: f32, top: bool, bottom: bool) {
        // (normal, u axis, v axis) with u x v = normal
        let faces = [
            (Vec3::X, Vec3::NEG_Z, Vec3::Y),
            (Vec3::NEG_X, Vec3::Z, Vec3::Y),
            (Vec3::Z, Vec3::X, Vec3::Y),
            (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
            (Vec3::Y, Vec3::X, Vec3::NEG_Z),
            (Vec3::NEG_Y, Vec3::X, Vec3::Z),
        ];
        for (fi, (n, u, v)) in faces.iter().enumerate() {
            if fi == 4 && !top {
                continue;
            }
            if fi == 5 && !bottom {
                continue;
            }
            let hn = (*n * half).length();
            let hu = (*u * half).length();
            let hv = (*v * half).length();
            let c0 = *n * hn;
            let corners = [
                c0 - *u * hu - *v * hv,
                c0 + *u * hu - *v * hv,
                c0 + *u * hu + *v * hv,
                c0 - *u * hu + *v * hv,
            ];
            let s = 1.0 / uv_scale.max(1e-4);
            let uvs = [
                Vec2::new(0.0, 2.0 * hv * s),
                Vec2::new(2.0 * hu * s, 2.0 * hv * s),
                Vec2::new(2.0 * hu * s, 0.0),
                Vec2::new(0.0, 0.0),
            ];
            let wn = m.transform_vector3(*n).normalize();
            let wp = corners.map(|p| m.transform_point3(p));
            self.quad(wp, wn, uvs, c);
        }
    }

    /// Building-style box: side faces get facade UVs (`cell` = metres per UV unit
    /// horizontally/vertically, offset by `uv_off`), the roof is emitted into `roof`.
    pub fn facade_box(
        &mut self,
        min: Vec3,
        max: Vec3,
        c: Rgba,
        cell: Vec2,
        uv_off: Vec2,
        roof: Option<(&mut MeshBuilder, Rgba)>,
    ) {
        let sx = max.x - min.x;
        let sz = max.z - min.z;
        let h = max.y - min.y;
        let v0 = uv_off.y + min.y / cell.y;
        let v1 = uv_off.y + max.y / cell.y;
        let wall = |b: &mut MeshBuilder, a: Vec3, bb: Vec3, n: Vec3, len: f32, off: f32| {
            let u0 = uv_off.x + off / cell.x;
            let u1 = u0 + len / cell.x;
            b.quad(
                [a, bb, bb + Vec3::Y * h, a + Vec3::Y * h],
                n,
                [Vec2::new(u0, v1), Vec2::new(u1, v1), Vec2::new(u1, v0), Vec2::new(u0, v0)],
                c,
            );
        };
        let y = min.y;
        // south (+z), east (+x), north (-z), west (-x) walked as a ring for continuous UVs
        wall(self, Vec3::new(min.x, y, max.z), Vec3::new(max.x, y, max.z), Vec3::Z, sx, 0.0);
        wall(self, Vec3::new(max.x, y, max.z), Vec3::new(max.x, y, min.z), Vec3::X, sz, sx);
        wall(self, Vec3::new(max.x, y, min.z), Vec3::new(min.x, y, min.z), Vec3::NEG_Z, sx, sx + sz);
        wall(self, Vec3::new(min.x, y, min.z), Vec3::new(min.x, y, max.z), Vec3::NEG_X, sz, 2.0 * sx + sz);
        if let Some((rb, rc)) = roof {
            rb.quad(
                [
                    Vec3::new(min.x, max.y, max.z),
                    Vec3::new(max.x, max.y, max.z),
                    Vec3::new(max.x, max.y, min.z),
                    Vec3::new(min.x, max.y, min.z),
                ],
                Vec3::Y,
                [
                    Vec2::new(0.0, 0.0),
                    Vec2::new(sx / 4.0, 0.0),
                    Vec2::new(sx / 4.0, sz / 4.0),
                    Vec2::new(0.0, sz / 4.0),
                ],
                rc,
            );
        }
    }

    /// Vertical cylinder (axis +Y) standing on `base`.
    pub fn cylinder(&mut self, base: Vec3, r: f32, h: f32, seg: u32, c: Rgba, caps: bool) {
        self.cylinder_at(Affine3A::from_translation(base), r, r, h, seg, c, caps);
    }

    /// Cylinder/frustum along local +Y from 0 to h, transformed by m.
    pub fn cylinder_at(&mut self, m: Affine3A, r0: f32, r1: f32, h: f32, seg: u32, c: Rgba, caps: bool) {
        let seg = seg.max(3);
        let slope = (r0 - r1) / h.max(1e-4);
        for i in 0..seg {
            let a0 = i as f32 / seg as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / seg as f32 * std::f32::consts::TAU;
            let d0 = Vec3::new(a0.cos(), 0.0, a0.sin());
            let d1 = Vec3::new(a1.cos(), 0.0, a1.sin());
            let p = [d0 * r0, d1 * r0, d1 * r1 + Vec3::Y * h, d0 * r1 + Vec3::Y * h];
            let n0 = (d0 + Vec3::Y * slope).normalize();
            let n1 = (d1 + Vec3::Y * slope).normalize();
            let fnrm = ((d0 + d1) * 0.5 + Vec3::Y * slope).normalize();
            let u0 = i as f32 / seg as f32;
            let u1 = (i + 1) as f32 / seg as f32;
            let wp = p.map(|q| m.transform_point3(q));
            let wn = [n0, n1, n1, n0].map(|q| m.transform_vector3(q).normalize());
            self.quad_smooth(
                wp,
                wn,
                m.transform_vector3(fnrm).normalize(),
                [Vec2::new(u0, 1.0), Vec2::new(u1, 1.0), Vec2::new(u1, 0.0), Vec2::new(u0, 0.0)],
                c,
            );
            if caps {
                let up = m.transform_vector3(Vec3::Y).normalize();
                if r1 > 0.0 {
                    self.tri(
                        [
                            m.transform_point3(Vec3::Y * h),
                            m.transform_point3(d0 * r1 + Vec3::Y * h),
                            m.transform_point3(d1 * r1 + Vec3::Y * h),
                        ],
                        up,
                        [Vec2::splat(0.5), Vec2::new(0.5 + d0.x * 0.5, 0.5 + d0.z * 0.5), Vec2::new(0.5 + d1.x * 0.5, 0.5 + d1.z * 0.5)],
                        c,
                    );
                }
                if r0 > 0.0 {
                    self.tri(
                        [m.transform_point3(Vec3::ZERO), m.transform_point3(d0 * r0), m.transform_point3(d1 * r0)],
                        -up,
                        [Vec2::splat(0.5), Vec2::new(0.5 + d0.x * 0.5, 0.5 + d0.z * 0.5), Vec2::new(0.5 + d1.x * 0.5, 0.5 + d1.z * 0.5)],
                        c,
                    );
                }
            }
        }
    }

    /// Low-poly ellipsoid, smooth normals.
    pub fn ellipsoid(&mut self, center: Vec3, radii: Vec3, seg: u32, rings: u32, c: Rgba) {
        let pt = |i: u32, j: u32| -> (Vec3, Vec3) {
            let th = i as f32 / seg as f32 * std::f32::consts::TAU;
            let ph = j as f32 / rings as f32 * std::f32::consts::PI;
            let unit = Vec3::new(ph.sin() * th.cos(), ph.cos(), ph.sin() * th.sin());
            let p = center + unit * radii;
            let n = (unit / radii).normalize_or(Vec3::Y);
            (p, n)
        };
        for j in 0..rings {
            for i in 0..seg {
                let (p0, n0) = pt(i, j);
                let (p1, n1) = pt(i + 1, j);
                let (p2, n2) = pt(i + 1, j + 1);
                let (p3, n3) = pt(i, j + 1);
                let fc = (p0 + p1 + p2 + p3) * 0.25 - center;
                let uv = [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y];
                if j == 0 {
                    // p0 == p1 at the pole: emit a triangle
                    let flip = (p2 - p0).cross(p3 - p0).dot(fc) < 0.0;
                    let a = self.vert(p0, n0, uv[0], c);
                    let b = self.vert(p2, n2, uv[2], c);
                    let d = self.vert(p3, n3, uv[3], c);
                    if flip { self.idx.extend_from_slice(&[a, d, b]) } else { self.idx.extend_from_slice(&[a, b, d]) }
                } else if j == rings - 1 {
                    let flip = (p1 - p0).cross(p2 - p0).dot(fc) < 0.0;
                    let a = self.vert(p0, n0, uv[0], c);
                    let b = self.vert(p1, n1, uv[1], c);
                    let d = self.vert(p2, n2, uv[2], c);
                    if flip { self.idx.extend_from_slice(&[a, d, b]) } else { self.idx.extend_from_slice(&[a, b, d]) }
                } else {
                    self.quad_smooth([p0, p1, p2, p3], [n0, n1, n2, n3], fc, uv, c);
                }
            }
        }
    }

    /// Extrude a convex profile given in the (z, y) plane between x0 and x1,
    /// transformed by m. Side caps get normal ±X. Flat shaded.
    pub fn extrude_x(&mut self, m: Affine3A, profile: &[Vec2], x0: f32, x1: f32, c: Rgba, uv_scale: f32) {
        let n = profile.len();
        if n < 3 {
            return;
        }
        let centroid = profile.iter().copied().sum::<Vec2>() / n as f32;
        let s = 1.0 / uv_scale;
        for i in 0..n {
            let a = profile[i];
            let b = profile[(i + 1) % n];
            let e = b - a;
            let mut nn = Vec2::new(e.y, -e.x).normalize_or_zero();
            if nn.dot((a + b) * 0.5 - centroid) < 0.0 {
                nn = -nn;
            }
            let n3 = Vec3::new(0.0, nn.y, nn.x);
            let p = [
                Vec3::new(x0, a.y, a.x),
                Vec3::new(x1, a.y, a.x),
                Vec3::new(x1, b.y, b.x),
                Vec3::new(x0, b.y, b.x),
            ];
            let len = e.length();
            let w = x1 - x0;
            self.quad(
                p.map(|q| m.transform_point3(q)),
                m.transform_vector3(n3).normalize(),
                [Vec2::new(0.0, 0.0), Vec2::new(w * s, 0.0), Vec2::new(w * s, len * s), Vec2::new(0.0, len * s)],
                c,
            );
        }
        for (x, nx) in [(x0, -1.0f32), (x1, 1.0f32)] {
            let nrm = m.transform_vector3(Vec3::X * nx).normalize();
            for i in 1..n - 1 {
                let tri = [profile[0], profile[i], profile[i + 1]];
                self.tri(
                    tri.map(|q| m.transform_point3(Vec3::new(x, q.y, q.x))),
                    nrm,
                    tri.map(|q| Vec2::new(q.x * s, q.y * s)),
                    c,
                );
            }
        }
    }

    /// Generic convex polygon (planar, any orientation) with expected normal.
    pub fn polygon(&mut self, pts: &[Vec3], n: Vec3, c: Rgba) {
        for i in 1..pts.len().saturating_sub(1) {
            self.tri([pts[0], pts[i], pts[i + 1]], n, [Vec2::ZERO, Vec2::X, Vec2::Y], c);
        }
    }

    /// Flat horizontal quad (decal-like), facing up.
    pub fn flat_rect(&mut self, min: Vec2, max: Vec2, y: f32, c: Rgba, uv_scale: f32) {
        let s = 1.0 / uv_scale;
        self.quad(
            [
                Vec3::new(min.x, y, max.y),
                Vec3::new(max.x, y, max.y),
                Vec3::new(max.x, y, min.y),
                Vec3::new(min.x, y, min.y),
            ],
            Vec3::Y,
            [
                Vec2::new(min.x * s, max.y * s),
                Vec2::new(max.x * s, max.y * s),
                Vec2::new(max.x * s, min.y * s),
                Vec2::new(min.x * s, min.y * s),
            ],
            c,
        );
    }

    /// Flat oriented strip on the ground from a to b (xz), width w.
    pub fn ground_strip(&mut self, a: Vec2, b: Vec2, w: f32, y: f32, c: Rgba) {
        let d = (b - a).normalize_or_zero();
        let side = Vec2::new(-d.y, d.x) * (w * 0.5);
        let p = [a - side, b - side, b + side, a + side].map(|q| Vec3::new(q.x, y, q.y));
        self.quad(p, Vec3::Y, [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y], c);
    }

    pub fn append(&mut self, other: &MeshBuilder, m: Affine3A) {
        let base = self.pos.len() as u32;
        for i in 0..other.pos.len() {
            self.pos.push(m.transform_point3(Vec3::from(other.pos[i])).to_array());
            self.nrm.push(m.transform_vector3(Vec3::from(other.nrm[i])).normalize_or(Vec3::Y).to_array());
        }
        self.uv.extend_from_slice(&other.uv);
        self.col.extend_from_slice(&other.col);
        self.idx.extend(other.idx.iter().map(|i| i + base));
    }

    /// Surface of revolution around the local X axis. `profile` = (radius, x) pairs,
    /// open polyline; smooth normals from the profile tangent.
    pub fn lathe_x(&mut self, m: Affine3A, profile: &[Vec2], seg: u32, c: Rgba) {
        let n = profile.len();
        if n < 2 {
            return;
        }
        // 2D outward normal per profile vertex (in the (x, r) half-plane)
        let pn: Vec<Vec2> = (0..n)
            .map(|i| {
                let a = profile[i.saturating_sub(1)];
                let b = profile[(i + 1).min(n - 1)];
                let t = Vec2::new(b.y - a.y, b.x - a.x); // (dx, dr)
                Vec2::new(-t.y, t.x).normalize_or(Vec2::Y) // (nx, nr)
            })
            .collect();
        for s in 0..seg {
            let a0 = s as f32 / seg as f32 * std::f32::consts::TAU;
            let a1 = (s + 1) as f32 / seg as f32 * std::f32::consts::TAU;
            let dir = |a: f32| Vec3::new(0.0, a.cos(), a.sin());
            for i in 0..n - 1 {
                let (p, q) = (profile[i], profile[i + 1]);
                let pt = |r: f32, x: f32, a: f32| m.transform_point3(Vec3::X * x + dir(a) * r);
                let nm = |k: usize, a: f32| m.transform_vector3(Vec3::X * pn[k].x + dir(a) * pn[k].y).normalize_or(Vec3::Y);
                let pts = [pt(p.x, p.y, a0), pt(p.x, p.y, a1), pt(q.x, q.y, a1), pt(q.x, q.y, a0)];
                let ns = [nm(i, a0), nm(i, a1), nm(i + 1, a1), nm(i + 1, a0)];
                let face = (ns[0] + ns[1] + ns[2] + ns[3]).normalize_or(Vec3::Y);
                let v0 = i as f32 / (n - 1) as f32;
                let v1 = (i + 1) as f32 / (n - 1) as f32;
                let u0 = s as f32 / seg as f32;
                let u1 = (s + 1) as f32 / seg as f32;
                self.quad_smooth(pts, ns, face, [Vec2::new(u0, v0), Vec2::new(u1, v0), Vec2::new(u1, v1), Vec2::new(u0, v1)], c);
            }
        }
    }

    /// Raytracing-compatible mesh: exactly {POSITION, NORMAL, UV_0, TANGENT} + u32
    /// indices (what Solari requires). Vertex colours are NOT emitted; callers that
    /// tint geometry use [`MeshBuilder::split_by_color`] and per-colour materials.
    pub fn build(self) -> Mesh {
        let n = self.pos.len();
        if n == 0 {
            // Solari builds acceleration structures for every compatible mesh and
            // panics on empty ones, so opt empty meshes out.
            // a single zero-area triangle keeps every render pipeline happy
            let mut empty = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3])
                .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; 3])
                .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32; 2]; 3])
                .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[1.0f32, 0.0, 0.0, 1.0]; 3])
                .with_inserted_indices(Indices::U32(vec![0, 1, 2]));
            empty.enable_raytracing = false;
            return empty;
        }
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
            .with_inserted_indices(Indices::U32(self.idx));
        if mesh.generate_tangents().is_err() {
            // degenerate UVs: any tangent perpendicular to the normal will do
            let t: Vec<[f32; 4]> = (0..n)
                .map(|i| {
                    let nn = Vec3::from(self.nrm[i]);
                    let a = if nn.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
                    let t = a.cross(nn).normalize_or(Vec3::X);
                    [t.x, t.y, t.z, 1.0]
                })
                .collect();
            mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, t);
        }
        mesh
    }

    /// Split into one builder per (quantised) vertex colour. Triangles are keyed by
    /// their first vertex; every primitive this builder emits has a uniform colour.
    pub fn split_by_color(self) -> Vec<(Rgba, MeshBuilder)> {
        let key = |c: Rgba| -> [u8; 4] { c.map(|v| (v * 50.0).round().clamp(0.0, 255.0) as u8) };
        let mut groups: Vec<([u8; 4], Rgba, MeshBuilder, std::collections::HashMap<u32, u32>)> = Vec::new();
        for tri in self.idx.chunks_exact(3) {
            let c = self.col[tri[0] as usize];
            let k = key(c);
            let gi = match groups.iter().position(|g| g.0 == k) {
                Some(i) => i,
                None => {
                    groups.push((k, c, MeshBuilder::new(), Default::default()));
                    groups.len() - 1
                }
            };
            let g = &mut groups[gi];
            for &v in tri {
                let nv = *g.3.entry(v).or_insert_with(|| {
                    let i = g.2.pos.len() as u32;
                    g.2.pos.push(self.pos[v as usize]);
                    g.2.nrm.push(self.nrm[v as usize]);
                    g.2.uv.push(self.uv[v as usize]);
                    g.2.col.push(self.col[v as usize]);
                    i
                });
                g.2.idx.push(nv);
            }
        }
        groups.into_iter().map(|(_, c, mb, _)| (c, mb)).collect()
    }

    /// Positions + triangle indices for a Rapier trimesh collider.
    pub fn collider_data(&self) -> (Vec<Vec3>, Vec<[u32; 3]>) {
        let v = self.pos.iter().map(|p| Vec3::from(*p)).collect();
        let t = self.idx.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
        (v, t)
    }
}

/// Emit a loft of rings (same point count, closed loops) with smooth normals.
/// `classify(segment, z_mid)` picks the output builder for each face.
pub fn loft(rings: &[Vec<Vec3>], outs: &mut [MeshBuilder], classify: &dyn Fn(usize, f32) -> usize) {
    let nr = rings.len();
    let np = rings[0].len();
    let center = |i: usize| -> Vec3 {
        let c = rings[i].iter().copied().sum::<Vec3>() / np as f32;
        c
    };
    // oriented face normals
    let mut fnrm = vec![vec![Vec3::ZERO; np]; nr - 1];
    for i in 0..nr - 1 {
        let cm = (center(i) + center(i + 1)) * 0.5;
        for j in 0..np {
            let j1 = (j + 1) % np;
            let (a, b, c, d) = (rings[i][j], rings[i][j1], rings[i + 1][j1], rings[i + 1][j]);
            let mut n = (c - a).cross(d - b);
            if n.length_squared() < 1e-12 {
                n = (a + b + c + d) * 0.25 - cm;
            }
            let fc = (a + b + c + d) * 0.25;
            if n.dot(fc - cm) < 0.0 {
                n = -n;
            }
            fnrm[i][j] = n.normalize_or(Vec3::Y);
        }
    }
    let mut vn = vec![vec![Vec3::ZERO; np]; nr];
    for i in 0..nr - 1 {
        for j in 0..np {
            let j1 = (j + 1) % np;
            let n = fnrm[i][j];
            vn[i][j] += n;
            vn[i][j1] += n;
            vn[i + 1][j] += n;
            vn[i + 1][j1] += n;
        }
    }
    for i in 0..nr - 1 {
        for j in 0..np {
            let j1 = (j + 1) % np;
            let p = [rings[i][j], rings[i][j1], rings[i + 1][j1], rings[i + 1][j]];
            let n = [vn[i][j], vn[i][j1], vn[i + 1][j1], vn[i + 1][j]].map(|v| v.normalize_or(Vec3::Y));
            let z = (p[0].z + p[2].z) * 0.5;
            let o = classify(j, z);
            if o >= outs.len() {
                continue; // classifier asked for no face here
            }
            let uv = [Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(0.0, 1.0)];
            outs[o].quad_smooth(p, n, fnrm[i][j], uv, WHITE);
        }
    }
}


/// Small deterministic RNG (PCG-ish xorshift) so the city is identical every run.
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) | 1)
    }
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 32) as u32
    }
    pub fn f(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
    pub fn int(&mut self, a: i32, b: i32) -> i32 {
        a + (self.next_u32() % ((b - a + 1) as u32)) as i32
    }
    pub fn chance(&mut self, p: f32) -> bool {
        self.f() < p
    }
    pub fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[(self.next_u32() as usize) % v.len()]
    }
}
