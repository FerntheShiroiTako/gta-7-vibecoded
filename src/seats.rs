//! Anything the player can drive or fly carries a `Seat`: enter/exit (with
//! carjacking), the camera rig and the HUD work on seats, whatever the vehicle.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::humanoid::{HumanoidAssets, Look};
use crate::input::GameInput;
use crate::peds::{spawn_ped, Ped, PedState};
use crate::player::Player;
use crate::vehicle::{Driver, Vehicle};

pub struct SeatPlugin;

impl Plugin for SeatPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreUpdate, sync_seats.after(crate::input::GameInputSet))
            .add_systems(Update, (enter_exit, driver_figures).chain())
            .add_systems(
                PostUpdate,
                follow_seat_with_player.after(PhysicsSet::Writeback).before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SeatKind {
    Car,
    Plane,
    Heli,
}

#[derive(Component, Clone)]
pub struct Seat {
    pub name: &'static str,
    pub kind: SeatKind,
    /// local footprint half extents (x = half width, y = height, z = half length)
    pub half: Vec3,
    pub driver: Driver,
    pub cam_dist: f32,
    pub cam_height: f32,
    /// wrecked / burning: can't be entered
    pub broken: bool,
}

/// Seat is the source of truth for who is driving; mirror it into car physics.
fn sync_seats(mut q: Query<(&mut Seat, Option<&mut Vehicle>)>) {
    for (mut seat, v) in &mut q {
        if let Some(mut v) = v {
            if v.driver != seat.driver {
                v.driver = seat.driver;
            }
            let broken = v.wrecked || v.burn_t.is_some();
            if seat.broken != broken {
                seat.broken = broken;
            }
        }
    }
}

/// Closest horizontal distance from `p` to a seat's footprint box.
pub fn dist_to_seat(p: Vec3, t: &Transform, s: &Seat) -> f32 {
    let local = t.rotation.inverse() * (p - t.translation);
    let half = Vec3::new(s.half.x, 10.0, s.half.z);
    let q = local.clamp(-half, half);
    (local - q).with_y(0.0).length()
}

#[allow(clippy::too_many_arguments)]
fn enter_exit(
    mut commands: Commands,
    mut input: ResMut<GameInput>,
    mut player: Query<(&mut Player, &mut Transform, &mut Visibility), Without<Seat>>,
    mut seats: Query<(Entity, &Transform, &mut Seat, &Velocity)>,
    rapier: ReadRapierContext,
    hum: Option<Res<HumanoidAssets>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut hud: ResMut<crate::hud::HudMessages>,
) {
    if !input.enter {
        return;
    }
    input.enter = false;
    let Ok((mut p, mut pt, mut vis)) = player.single_mut() else { return };
    if p.health <= 0.0 {
        return;
    }
    if let Some(ve) = p.vehicle {
        let Ok((_, st, mut seat, vel)) = seats.get_mut(ve) else {
            p.vehicle = None;
            *vis = Visibility::Inherited;
            return;
        };
        let side = st.rotation * Vec3::X;
        let base = st.translation + Vec3::Y * 0.4;
        let mut out = None;
        if let Ok(ctx) = rapier.single() {
            for dir in [-side, side] {
                let reach = seat.half.x + 0.9;
                let blocked = ctx.cast_ray(base, dir, reach, true, QueryFilter::default().exclude_rigid_body(ve).exclude_sensors()).is_some();
                if !blocked {
                    out = Some(base + dir * reach + Vec3::Y * 0.5);
                    break;
                }
            }
        }
        pt.translation = out.unwrap_or(st.translation + Vec3::Y * (seat.half.y + 1.4));
        p.vel = vel.linear * 0.6;
        p.vehicle = None;
        *vis = Visibility::Inherited;
        seat.driver = Driver::None;
        return;
    }
    let mut best: Option<(Entity, f32)> = None;
    for (e, st, seat, _) in seats.iter() {
        if seat.broken {
            continue;
        }
        let d = dist_to_seat(pt.translation, st, &seat);
        if d < 2.2 && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((e, d));
        }
    }
    let Some((ce, _)) = best else { return };
    let Ok((_, st, mut seat, _)) = seats.get_mut(ce) else { return };
    match seat.driver {
        Driver::Npc(seed) | Driver::Cop(seed) => {
            if let Some(h) = &hum {
                let door = st.translation + st.rotation * Vec3::new(-(seat.half.x + 0.9), 0.0, -0.3);
                let pos = Vec3::new(door.x, st.translation.y + 0.6, door.z);
                let look = if matches!(seat.driver, Driver::Cop(_)) { Look::cop() } else { Look::random(seed) };
                spawn_ped(&mut commands, h, &mut materials, pos, look, Ped { state: PedState::Flee { from: pt.translation, t: 8.0 }, ambient: true, ..Ped::new(seed) });
            }
            hud.push("Carjacked!");
            crate::wanted::report_crime(&mut commands, crate::wanted::Crime::Carjack, pt.translation);
        }
        _ => {}
    }
    seat.driver = Driver::Player;
    p.vehicle = Some(ce);
    *vis = Visibility::Hidden;
    hud.push(seat.name);
}

fn follow_seat_with_player(mut player: Query<(&Player, &mut Transform), Without<Seat>>, seats: Query<&Transform, With<Seat>>) {
    let Ok((p, mut pt)) = player.single_mut() else { return };
    if let Some(ve) = p.vehicle {
        if let Ok(st) = seats.get(ve) {
            pt.translation = st.translation + Vec3::Y * 0.6;
        }
    }
}

/// The visible person behind the wheel (NPC, cop or the player).
#[derive(Component)]
pub struct DriverFigure {
    pub figure: Entity,
    pub who: Driver,
}

fn driver_figures(
    mut commands: Commands,
    hum: Option<Res<HumanoidAssets>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    seats: Query<(Entity, &Seat, Option<&DriverFigure>, Option<&Vehicle>)>,
    mut rigs: Query<&mut crate::humanoid::Rig>,
) {
    let Some(hum) = hum else { return };
    for (e, seat, fig, veh) in &seats {
        let want = if seat.kind == SeatKind::Car && !seat.broken && seat.driver != Driver::None { Some(seat.driver) } else { None };
        match (want, fig) {
            (Some(w), Some(f)) if f.who == w => {}
            (want, fig) => {
                if let Some(f) = fig {
                    commands.entity(f.figure).despawn();
                    commands.entity(e).remove::<DriverFigure>();
                }
                if let (Some(w), Some(v)) = (want, veh) {
                    let look = match w {
                        Driver::Player => Look::player(),
                        Driver::Cop(_) => Look::cop(),
                        Driver::Npc(seed) => Look::random(seed),
                        Driver::None => continue,
                    };
                    let figure = crate::humanoid::spawn_humanoid(&mut commands, &hum, &mut materials, look);
                    // pivot the figure about its hip so it reclines into the seat
                    let lay = crate::car_model::seat_layout(&v.spec);
                    let rot = Quat::from_rotation_x(lay.recline);
                    let hip_local = Vec3::Y * 0.93 * lay.figure_scale;
                    let tf = Transform { translation: lay.hip - rot * hip_local, rotation: rot, scale: Vec3::splat(lay.figure_scale) };
                    commands.entity(figure).insert(tf);
                    crate::humanoid::no_shadows(&mut commands, figure);
                    commands.entity(e).add_child(figure).insert(DriverFigure { figure, who: w });
                    if let Ok(mut r) = rigs.get_mut(figure) {
                        r.seated = true;
                    }
                    commands.entity(figure).insert(crate::humanoid::Rig { seated: true, ..default() });
                }
            }
        }
    }
}
