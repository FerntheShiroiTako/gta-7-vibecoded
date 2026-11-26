//! Lofted car bodies: smooth cross-sections along the car with rounded shoulders,
//! tapered nose/tail and curved wheel arches; a glass greenhouse with painted
//! pillars and roof; lathed tyres and multi-spoke rims; lights, grille, mirrors,
//! door seams and handles.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::math::Affine3A;
use bevy::prelude::*;

use crate::meshgen::{MeshBuilder, WHITE};
use crate::vehicle::{CarKind, CarSpec, Shape};

// output slots (match the order returned by `build_car`)
const PAINT: usize = 0;
const GLASS: usize = 1;
const TRIM: usize = 2;
const CHROME: usize = 3;
const HEAD: usize = 4;
const TAIL: usize = 5;
const PLATE: usize = 6;

/// Where the front seats and the driver sit, sized to the cabin so low sports
/// cars recline the seat instead of poking heads through the roof.
#[derive(Clone, Copy, Debug)]
pub struct SeatLayout {
    pub floor: f32,
    /// vertical scale of the seat furniture
    pub k: f32,
    pub seat_z: f32,
    /// driver's hip point (car-local)
    pub hip: Vec3,
    pub recline: f32,
    pub figure_scale: f32,
}

pub fn seat_layout(s: &CarSpec) -> SeatLayout {
    let sh = s.shape;
    let floor = sh.bottom + 0.03;
    let roof = sh.roof_front.1.min(sh.roof_rear.1);
    let avail = roof - floor;
    let k = ((avail - 0.1) / 1.0).clamp(0.62, 1.0);
    let seat_z = sh.cowl.0 + 0.85;
    let recline: f32 = if avail < 1.0 { 0.42 } else { 0.16 };
    let hip = Vec3::new(-s.width * 0.21, floor + 0.27 * k, seat_z + 0.04);
    // seated hip-to-crown is ~0.87 m for a full-size figure
    let figure_scale = ((roof - 0.07 - hip.y) / (0.87 * recline.cos())).clamp(0.78, 1.0);
    SeatLayout { floor, k, seat_z, hip, recline, figure_scale }
}

/// x of the inner edge of the body shoulder (where the flat top begins).
fn shoulder_x(hw: f32, b: f32, t: f32) -> f32 {
    let t = t.max(b + 0.04);
    let h = t - b;
    hw - (h * 0.06).min(0.05) - (h * 0.42).min(0.2)
}

fn top_outline(s: &Shape) -> Vec<Vec2> {
    vec![
        Vec2::new(s.front, s.nose_y),
        Vec2::new(s.front + 0.07, s.light_y),
        Vec2::new(s.hood_front.0, s.hood_front.1),
        Vec2::new(s.cowl.0, s.cowl.1),
        Vec2::new(s.rear_base.0, s.rear_base.1),
        Vec2::new(s.deck_end.0, s.deck_end.1),
        Vec2::new(s.rear, s.tail_y),
    ]
}

fn top_at(outline: &[Vec2], z: f32) -> f32 {
    if z <= outline[0].x {
        return outline[0].y;
    }
    for w in outline.windows(2) {
        if z >= w[0].x && z <= w[1].x {
            let t = (z - w[0].x) / (w[1].x - w[0].x).max(1e-5);
            return w[0].y + (w[1].y - w[0].y) * t;
        }
    }
    outline.last().unwrap().y
}

struct Body<'a> {
    s: &'a CarSpec,
    outline: Vec<Vec2>,
}

impl Body<'_> {
    /// smoothed top of the lower body (hood / beltline / deck)
    fn top(&self, z: f32) -> f32 {
        let d = 0.14;
        (top_at(&self.outline, z - d) + 2.0 * top_at(&self.outline, z) + top_at(&self.outline, z + d)) * 0.25
    }
    /// bottom of the body including the wheel-arch cut-outs
    fn bottom(&self, z: f32) -> f32 {
        let sh = &self.s.shape;
        let r = self.s.wheel_r + 0.09;
        let mut b = sh.bottom;
        for za in [-self.s.wheelbase * 0.5, self.s.wheelbase * 0.5] {
            let d = z - za;
            if d.abs() < r {
                b = b.max((r * r - d * d).sqrt() * 1.02);
            }
        }
        b.min(self.top(z) - 0.09)
    }
    /// half width with rounded plan-view corners
    fn half_width(&self, z: f32) -> f32 {
        let sh = &self.s.shape;
        let hw = self.s.width * 0.5;
        let taper = 0.5;
        let d = (z - sh.front).min(sh.rear - z).clamp(0.0, taper) / taper;
        hw * (0.74 + 0.26 * (1.0 - (1.0 - d) * (1.0 - d)).sqrt())
    }
    fn roof(&self, z: f32) -> f32 {
        let sh = &self.s.shape;
        let (cw, rf, rr, rb) = (sh.cowl, sh.roof_front, sh.roof_rear, sh.rear_base);
        if z <= rf.0 {
            let t = ((z - cw.0) / (rf.0 - cw.0)).clamp(0.0, 1.0);
            cw.1 + (rf.1 - cw.1) * (1.0 - (1.0 - t) * (1.0 - t))
        } else if z <= rr.0 {
            let t = (z - rf.0) / (rr.0 - rf.0).max(1e-4);
            rf.1 + (rr.1 - rf.1) * t
        } else {
            let t = ((z - rr.0) / (rb.0 - rr.0).max(1e-4)).clamp(0.0, 1.0);
            rr.1 + (rb.1 - rr.1) * t * t
        }
    }
    fn b_pillar(&self) -> f32 {
        let sh = &self.s.shape;
        (sh.roof_front.0 + sh.roof_rear.0) * 0.5 + 0.08
    }
}

/// Mirror a right-half profile (x >= 0, first and last points on the centreline)
/// into a closed ring at station z.
fn ring(right: &[Vec2], z: f32) -> Vec<Vec3> {
    let mut out: Vec<Vec3> = right.iter().map(|p| Vec3::new(p.x, p.y, z)).collect();
    for p in right[1..right.len() - 1].iter().rev() {
        out.push(Vec3::new(-p.x, p.y, z));
    }
    out
}

fn body_ring(hw: f32, b: f32, t: f32, z: f32) -> Vec<Vec3> {
    let t = t.max(b + 0.04);
    let h = t - b;
    let rt = (h * 0.42).min(0.2);
    let rb = (h * 0.2).min(0.06);
    let lean = (h * 0.06).min(0.05);
    let mut r = vec![Vec2::new(0.0, b), Vec2::new(hw - rb, b)];
    r.push(Vec2::new(hw - rb + rb * (PI * 0.25).cos(), b + rb - rb * (PI * 0.25).sin()));
    r.push(Vec2::new(hw, b + rb));
    r.push(Vec2::new(hw + 0.01, b + h * 0.5));
    let c = Vec2::new(hw - lean - rt, t - rt);
    for k in 0..=3 {
        let a = k as f32 / 3.0 * FRAC_PI_2;
        r.push(c + Vec2::new(a.cos(), a.sin()) * rt);
    }
    r.push(Vec2::new(0.0, t));
    ring(&r, z)
}
// segments of `body_ring` that form the underside / arch liners
const BODY_UNDERSIDE: [usize; 6] = [0, 1, 2, 15, 16, 17];

fn cabin_ring(wb: f32, b: f32, t: f32, z: f32) -> Vec<Vec3> {
    let t = t.max(b + 0.02);
    let h = t - b;
    let tumble = (h * 0.32).min(0.2);
    let wt = wb - tumble;
    let r = (h * 0.3).min(0.09);
    let c = Vec2::new(wt - r, t - r);
    let pts = vec![
        Vec2::new(0.0, b),
        Vec2::new(wb, b),
        Vec2::new(wt, t - r),
        c + Vec2::new((PI * 0.25).cos(), (PI * 0.25).sin()) * r,
        Vec2::new(wt - r, t),
        Vec2::new(0.0, t),
    ];
    ring(&pts, z)
}

fn stations(z0: f32, z1: f32, n: usize, extra: &[f32]) -> Vec<f32> {
    let mut v: Vec<f32> = (0..=n).map(|i| z0 + (z1 - z0) * i as f32 / n as f32).collect();
    v.extend(extra.iter().copied().filter(|z| *z > z0 && *z < z1));
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v.dedup_by(|a, b| (*a - *b).abs() < 0.01);
    v
}

/// Build all car parts in car-local space (origin at axle height, nose toward -Z).
/// Returns [paint, glass, trim, chrome, headlights, taillights, plates, tyre, rim, interior].
pub fn build_car(s: &CarSpec) -> [MeshBuilder; 10] {
    let sh = s.shape;
    let body = Body { s, outline: top_outline(&sh) };
    let mut out: [MeshBuilder; 7] = Default::default();

    // ---- lower body loft ----
    let fa = -s.wheelbase * 0.5;
    let ra = s.wheelbase * 0.5;
    let ar = s.wheel_r + 0.09;
    let (c0, c1) = (sh.cowl.0, sh.rear_base.0);
    let zs = stations(sh.front, sh.rear, 64, &[fa - ar, fa + ar, ra - ar, ra + ar, fa, ra, c0, c1]);
    let rings: Vec<Vec<Vec3>> = zs.iter().map(|&z| body_ring(body.half_width(z), body.bottom(z), body.top(z), z)).collect();
    crate::meshgen::loft(&rings, &mut out, &|seg, z| {
        if BODY_UNDERSIDE.contains(&seg) {
            TRIM
        } else if (seg == 8 || seg == 9) && z > c0 && z < c1 {
            usize::MAX // open top: the cabin tub is visible through the glass
        } else {
            PAINT
        }
    });
    // nose and tail caps
    for (r, n) in [(&rings[0], Vec3::NEG_Z), (rings.last().unwrap(), Vec3::Z)] {
        out[PAINT].polygon(r, n, WHITE);
    }

    // ---- greenhouse loft ----
    let (rf, rr) = (sh.roof_front.0, sh.roof_rear.0);
    let bp = body.b_pillar();
    let four_door = !matches!(s.kind, CarKind::Sports);
    let czs = stations(sh.cowl.0, sh.rear_base.0, 30, &[rf, rr, bp - 0.06, bp + 0.06]);
    let crings: Vec<Vec<Vec3>> = czs
        .iter()
        .map(|&z| cabin_ring(body.half_width(z) - 0.07, body.top(z) - 0.012, body.roof(z), z))
        .collect();
    crate::meshgen::loft(&crings, &mut out, &|seg, z| match seg {
        4 | 5 => {
            if z > rf + 0.03 && z < rr - 0.03 {
                PAINT
            } else {
                GLASS
            }
        }
        2 | 3 | 6 | 7 => PAINT,
        1 | 8 => {
            if four_door && (z - bp).abs() < 0.06 {
                PAINT
            } else {
                GLASS
            }
        }
        0 | 9 => usize::MAX,
        _ => GLASS,
    });

    // ---- details ----
    let hw_at = |z: f32| body.half_width(z);
    let belt_mid = body.top(bp);
    let zf = sh.front;
    let zr = sh.rear;
    // headlight lenses
    for sx in [-1.0f32, 1.0] {
        let z = zf + 0.16;
        let c = Vec3::new(sx * (hw_at(z) - 0.27), sh.light_y - 0.05, z - 0.02);
        out[HEAD].ellipsoid(c, Vec3::new(0.2, 0.065, 0.1), 12, 6, WHITE);
        out[CHROME].ellipsoid(c + Vec3::new(0.0, 0.0, 0.012), Vec3::new(0.215, 0.075, 0.1), 12, 6, WHITE);
        // tail lights: wide light bar segment
        let zt = zr - 0.02;
        let ty = sh.tail_y + (sh.deck_end.1 - sh.tail_y) * 0.55;
        out[TAIL].cuboid(Vec3::new(sx * (hw_at(zt) - 0.26), ty, zt + 0.02), Vec3::new(0.24, 0.055, 0.025), WHITE, 1.0);
    }
    // grille with chrome slats, lower intake
    let gy = (sh.nose_y + sh.light_y) * 0.5;
    let gw = hw_at(zf + 0.02) * 0.42;
    out[TRIM].cuboid(Vec3::new(0.0, gy, zf - 0.004), Vec3::new(gw, (sh.light_y - sh.nose_y) * 0.3, 0.015), WHITE, 1.0);
    for k in 0..3 {
        out[CHROME].cuboid(Vec3::new(0.0, gy - 0.04 + k as f32 * 0.04, zf - 0.02), Vec3::new(gw * 0.95, 0.006, 0.006), WHITE, 1.0);
    }
    out[TRIM].cuboid(Vec3::new(0.0, sh.bottom + 0.1, zf + 0.02), Vec3::new(hw_at(zf + 0.05) * 0.85, 0.05, 0.04), WHITE, 1.0);
    out[TRIM].cuboid(Vec3::new(0.0, sh.bottom + 0.09, zr - 0.02), Vec3::new(hw_at(zr - 0.05) * 0.85, 0.06, 0.04), WHITE, 1.0);
    // number plates
    out[PLATE].cuboid(Vec3::new(0.0, sh.bottom + 0.24, zf - 0.03), Vec3::new(0.25, 0.06, 0.008), WHITE, 1.0);
    out[PLATE].cuboid(Vec3::new(0.0, sh.tail_y + 0.04, zr + 0.01), Vec3::new(0.25, 0.06, 0.008), WHITE, 1.0);
    // exhaust tips
    for sx in if matches!(s.kind, CarKind::Sports) { vec![-1.0f32, 1.0] } else { vec![1.0f32] } {
        out[CHROME].cylinder_at(
            Affine3A::from_rotation_translation(Quat::from_rotation_x(FRAC_PI_2), Vec3::new(sx * hw_at(zr) * 0.5, sh.bottom + 0.06, zr - 0.15)),
            0.045,
            0.045,
            0.2,
            10,
            WHITE,
            true,
        );
    }
    // mirrors on stalks
    for sx in [-1.0f32, 1.0] {
        let z = sh.cowl.0 + 0.22;
        let x = sx * (hw_at(z) + 0.02);
        out[TRIM].cuboid(Vec3::new(x, sh.cowl.1 + 0.02, z), Vec3::new(0.05, 0.015, 0.03), WHITE, 1.0);
        out[PAINT].ellipsoid(Vec3::new(x + sx * 0.09, sh.cowl.1 + 0.08, z + 0.02), Vec3::new(0.09, 0.055, 0.045), 10, 6, WHITE);
    }
    // door seams + handles + sills
    let seams: Vec<f32> = if four_door {
        vec![sh.cowl.0 + 0.05, bp, (sh.rear_base.0 - 0.08).min(ra - ar - 0.05)]
    } else {
        vec![sh.cowl.0 + 0.05, bp + 0.3]
    };
    for sx in [-1.0f32, 1.0] {
        for &z in &seams {
            let b = body.bottom(z) + 0.06;
            let t = body.top(z) - 0.03;
            out[TRIM].cuboid(Vec3::new(sx * (hw_at(z) + 0.012), (b + t) * 0.5, z), Vec3::new(0.004, (t - b) * 0.5, 0.006), WHITE, 1.0);
        }
        for w in seams.windows(2) {
            let z = w[1] - 0.18;
            out[CHROME].cuboid(Vec3::new(sx * (hw_at(z) + 0.018), belt_mid - 0.1, z), Vec3::new(0.008, 0.012, 0.07), WHITE, 1.0);
        }
        // belt-line chrome strip along the side windows
        let z0 = sh.cowl.0 + 0.1;
        let z1 = sh.rear_base.0 - 0.1;
        out[CHROME].cuboid(Vec3::new(sx * (hw_at((z0 + z1) * 0.5) - 0.06), belt_mid + 0.004, (z0 + z1) * 0.5), Vec3::new(0.006, 0.006, (z1 - z0) * 0.5), WHITE, 1.0);
        // side skirt between the arches
        let s0 = fa + ar;
        let s1 = ra - ar;
        out[TRIM].cuboid(Vec3::new(sx * (hw_at(0.0) - 0.005), sh.bottom + 0.05, (s0 + s1) * 0.5), Vec3::new(0.012, 0.045, (s1 - s0) * 0.5), WHITE, 1.0);
    }
    match s.kind {
        CarKind::Sports => {
            let y = sh.deck_end.1 + 0.2;
            let z = zr - 0.22;
            out[TRIM].cuboid(Vec3::new(0.0, y, z), Vec3::new(hw_at(z) - 0.1, 0.02, 0.15), WHITE, 1.0);
            for sx in [-1.0f32, 1.0] {
                out[TRIM].cuboid(Vec3::new(sx * (hw_at(z) - 0.4), y - 0.1, z), Vec3::new(0.02, 0.1, 0.05), WHITE, 1.0);
            }
            // side intakes
            for sx in [-1.0f32, 1.0] {
                let z = ra - ar - 0.25;
                out[TRIM].cuboid(Vec3::new(sx * (hw_at(z) + 0.004), body.top(z) - 0.2, z), Vec3::new(0.01, 0.07, 0.2), WHITE, 1.0);
            }
        }
        CarKind::Taxi => {
            let z = (rf + rr) * 0.5;
            out[PLATE].cuboid(Vec3::new(0.0, body.roof(z) + 0.12, z), Vec3::new(0.32, 0.1, 0.12), WHITE, 1.0);
        }
        CarKind::Suv | CarKind::Van => {
            for sx in [-1.0f32, 1.0] {
                let z = (rf + rr) * 0.5;
                let x = sx * (hw_at(z) - 0.32);
                out[CHROME].cuboid(Vec3::new(x, body.roof(z) + 0.06, z), Vec3::new(0.02, 0.025, (rr - rf) * 0.45), WHITE, 1.0);
            }
        }
        CarKind::Sedan => {}
    }

    // ---- wheel: lathed tyre + rim (axis X, centred) ----
    let r = s.wheel_r;
    let w = s.wheel_w;
    let mut tire = MeshBuilder::new();
    let tp = [
        Vec2::new(0.64 * r, -w * 0.5 + 0.01),
        Vec2::new(0.84 * r, -w * 0.5 - 0.006),
        Vec2::new(0.95 * r, -w * 0.5 + 0.012),
        Vec2::new(r, -w * 0.5 + 0.055),
        Vec2::new(r, w * 0.5 - 0.055),
        Vec2::new(0.95 * r, w * 0.5 - 0.012),
        Vec2::new(0.84 * r, w * 0.5 + 0.006),
        Vec2::new(0.64 * r, w * 0.5 - 0.01),
    ];
    tire.lathe_x(Affine3A::IDENTITY, &tp, 28, WHITE);
    let mut rim = MeshBuilder::new();
    for side in [-1.0f32, 1.0] {
        let x = |d: f32| side * (w * 0.5 - d);
        // rim lip -> recessed dish -> hub
        let prof = [Vec2::new(0.66 * r, x(0.012)), Vec2::new(0.6 * r, x(0.03)), Vec2::new(0.3 * r, x(0.06)), Vec2::new(0.14 * r, x(0.05)), Vec2::new(0.0, x(0.045))];
        let mut p = prof.to_vec();
        if side < 0.0 {
            p.reverse();
        }
        rim.lathe_x(Affine3A::IDENTITY, &p, 24, WHITE);
        let spokes = if matches!(s.kind, CarKind::Sports) { 10 } else { 5 };
        for k in 0..spokes {
            let a = k as f32 / spokes as f32 * TAU;
            let rot = Quat::from_rotation_x(a);
            let c = Vec3::new(x(0.035), 0.0, 0.0) + rot * Vec3::Y * r * 0.4;
            rim.box_at(Affine3A::from_rotation_translation(rot, c), Vec3::new(0.012, r * 0.22, 0.028), WHITE, 1.0, true, true);
        }
    }
    // ---- interior (seen through the tinted glass; its own mesh so it can skip shadows) ----
    let mut inner = MeshBuilder::new();
    {
        let hw = s.width * 0.5;
        let lay = seat_layout(s);
        let (floor, k, seat_z) = (lay.floor, lay.k, lay.seat_z);
        let cz = sh.cowl.0;
        // tub: floor, inner door panels, firewall and rear bulkhead close the
        // opening in the body so nothing behind the glass is see-through
        let tz = stations(c0, c1, 16, &[]);
        let inner_x = |z: f32| shoulder_x(body.half_width(z), body.bottom(z), body.top(z));
        for w in tz.windows(2) {
            let (za, zb) = (w[0], w[1]);
            let (xa, xb) = (inner_x(za), inner_x(zb));
            let (ta, tb) = (body.top(za), body.top(zb));
            inner.quad([Vec3::new(-xa, floor, za), Vec3::new(xa, floor, za), Vec3::new(xb, floor, zb), Vec3::new(-xb, floor, zb)], Vec3::Y, [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y], WHITE);
            for sx in [-1.0f32, 1.0] {
                inner.quad([Vec3::new(sx * xa, floor, za), Vec3::new(sx * xb, floor, zb), Vec3::new(sx * xb, tb, zb), Vec3::new(sx * xa, ta, za)], Vec3::X * -sx, [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y], WHITE);
            }
        }
        for (z, n) in [(c0, Vec3::Z), (c1, Vec3::NEG_Z)] {
            let x = inner_x(z);
            let t = body.top(z);
            inner.quad([Vec3::new(-x, floor, z), Vec3::new(x, floor, z), Vec3::new(x, t, z), Vec3::new(-x, t, z)], n, [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y], WHITE);
        }
        // front seats lean back (+z is rearward)
        for sx in [-1.0f32, 1.0] {
            let x = sx * hw * 0.42;
            inner.cuboid(Vec3::new(x, floor + 0.2 * k, seat_z), Vec3::new(0.24, 0.07 * k, 0.26), WHITE, 1.0); // cushion
            let back = Quat::from_rotation_x(lay.recline + 0.08);
            let bc = Vec3::new(x, floor + 0.27 * k, seat_z + 0.26) + back * Vec3::Y * 0.33 * k;
            inner.box_at(Affine3A::from_rotation_translation(back, bc), Vec3::new(0.23, 0.33 * k, 0.06), WHITE, 1.0, true, true);
            let hc = Vec3::new(x, floor + 0.27 * k, seat_z + 0.26) + back * Vec3::Y * (0.72 * k + 0.06);
            inner.box_at(Affine3A::from_rotation_translation(back, hc), Vec3::new(0.12, 0.08, 0.05), WHITE, 1.0, true, true); // headrest
        }
        if four_door {
            let rz = seat_z + 1.0;
            if rz + 0.4 < c1 {
                inner.cuboid(Vec3::new(0.0, floor + 0.2 * k, rz), Vec3::new(hw * 0.78, 0.07 * k, 0.25), WHITE, 1.0);
                let back = Quat::from_rotation_x(0.2);
                let bc = Vec3::new(0.0, floor + 0.27 * k, rz + 0.26) + back * Vec3::Y * 0.3 * k;
                inner.box_at(Affine3A::from_rotation_translation(back, bc), Vec3::new(hw * 0.78, 0.3 * k, 0.06), WHITE, 1.0, true, true);
            }
        }
        // dashboard + centre console
        inner.cuboid(Vec3::new(0.0, sh.cowl.1 - 0.12, cz + 0.22), Vec3::new(hw - 0.12, 0.12, 0.2), WHITE, 1.0);
        inner.cuboid(Vec3::new(0.0, floor + 0.2, seat_z - 0.1), Vec3::new(0.1, 0.12, 0.45), WHITE, 1.0);
        // steering wheel (driver on the left)
        let wheel_c = Vec3::new(-hw * 0.42, sh.cowl.1 - 0.02, cz + 0.42);
        let tilt = Quat::from_rotation_x(-1.15);
        inner.cylinder_at(Affine3A::from_rotation_translation(tilt, wheel_c), 0.18, 0.18, 0.03, 16, WHITE, false);
        inner.cylinder_at(Affine3A::from_rotation_translation(tilt, wheel_c - tilt * Vec3::Y * 0.2), 0.03, 0.03, 0.2, 6, WHITE, false);
    }
    let [paint, glass, trim, chrome, head, tail, plate] = out;
    [paint, glass, trim, chrome, head, tail, plate, tire, rim, inner]
}
