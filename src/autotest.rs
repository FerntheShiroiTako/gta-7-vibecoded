//! Scripted smoke test: `HH_AUTOTEST=1 cargo run --release`.
//! Drives the game through `InputOverride`, saves screenshots to ./autotest/,
//! prints frame-time stats and exits.

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy_rapier3d::prelude::Velocity;

use crate::camera::CameraOverride;
use crate::input::{GameInput, InputOverride};
use crate::player::Player;
use crate::vehicle::{Showcase, Vehicle};

pub struct AutotestPlugin;

impl Plugin for AutotestPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("HH_TOGGLETEST").is_ok() {
            let _ = std::fs::create_dir_all("autotest");
            app.insert_resource(Toggle::default()).add_systems(Update, toggletest.before(crate::lighting::LightingToggleSet));
        } else if std::env::var("HH_WORLDTEST").is_ok() {
            let _ = std::fs::create_dir_all("autotest");
            app.insert_resource(World2::default()).add_systems(Update, worldtest.before(crate::player::PlayerMoveSet));
        } else if std::env::var("HH_FLIGHTTEST").is_ok() {
            let _ = std::fs::create_dir_all("autotest");
            app.insert_resource(Flight::default()).add_systems(Update, flighttest.before(crate::player::PlayerMoveSet));
        } else if std::env::var("HH_GUNTEST").is_ok() {
            let _ = std::fs::create_dir_all("autotest");
            app.insert_resource(GunTest::default()).add_systems(Update, guntest.before(crate::player::PlayerMoveSet));
        } else if std::env::var("HH_JOBTEST").is_ok() {
            let _ = std::fs::create_dir_all("autotest");
            app.insert_resource(JobTest::default()).add_systems(Update, jobtest.before(crate::player::PlayerMoveSet));
        } else if std::env::var("HH_COMBATTEST").is_ok() {
            let _ = std::fs::create_dir_all("autotest");
            app.insert_resource(Combat::default()).add_systems(Update, combattest.before(crate::player::PlayerMoveSet));
        } else if let Ok(dir) = std::env::var("HH_SHOTS") {
            let _ = std::fs::create_dir_all(&dir);
            app.insert_resource(Shots { dir, ..default() }).add_systems(Update, shots);
        } else if std::env::var("HH_WALKTEST").is_ok() {
            app.insert_resource(AutoTest::default()).add_systems(Update, walktest);
        } else if std::env::var("HH_BENCH").is_ok() {
            app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin).insert_resource(Bench::default()).add_systems(Update, bench);
        } else if std::env::var("HH_AUTOTEST").is_ok() {
            let _ = std::fs::create_dir_all("autotest");
            app.insert_resource(AutoTest::default()).add_systems(Update, autotest);
        }
    }
}

#[derive(Resource, Default)]
struct AutoTest {
    t: f32,
    done: Vec<&'static str>,
    frame_times: Vec<f32>,
    log: Vec<String>,
}

impl AutoTest {
    /// true exactly once when `t` passes `at`
    fn once(&mut self, at: f32, id: &'static str) -> bool {
        if self.t >= at && !self.done.contains(&id) {
            self.done.push(id);
            println!("AUTOTEST step {id} at t={:.2}", self.t);
            true
        } else {
            false
        }
    }
}

fn shot(commands: &mut Commands, name: &str) {
    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(format!("autotest/{name}.png")));
}

#[allow(clippy::too_many_arguments)]
fn autotest(
    mut commands: Commands,
    time: Res<Time>,
    mut at: ResMut<AutoTest>,
    mut over: ResMut<InputOverride>,
    mut cam: ResMut<CameraOverride>,
    mut player: Query<(&mut Player, &mut Transform), Without<Vehicle>>,
    mut cars: Query<(Entity, &mut Vehicle, &Transform, &Velocity)>,
    showcase: Res<Showcase>,
    rig: Res<crate::camera::CameraRig>,
    mut quit: ResMut<crate::lighting::QuitRequest>,
    all: Query<Entity>,
) {
    let dt = time.delta_secs();
    at.t += dt;
    let t = at.t;
    if t > 8.0 {
        at.frame_times.push(dt);
    }
    let mut gi = GameInput::default();
    let Ok((mut p, mut pt)) = player.single_mut() else { return };

    if at.once(7.0, "01") {
        shot(&mut commands, "01_spawn");
    }
    if (7.5..10.0).contains(&t) {
        gi.move_axis = Vec2::new(0.0, 1.0);
        gi.sprint = t > 8.5;
        gi.look = Vec2::new(-2.5, 0.0);
    }
    if at.once(10.0, "02") {
        shot(&mut commands, "02_walk");
    }
    if at.once(10.2, "jump") {
        gi.jump = true;
    }
    if at.once(10.45, "03") {
        shot(&mut commands, "03_jump");
    }
    let sports = showcase.cars.first().copied();
    if at.once(11.0, "tp_car") {
        if let Some((_, v, ct, _)) = sports.and_then(|e| cars.get(e).ok()) {
            pt.translation = ct.translation + ct.rotation * Vec3::new(-(v.spec.width * 0.5 + 1.0), 0.5, 0.0);
            p.vel = Vec3::ZERO;
        }
    }
    if at.once(11.6, "enter1") {
        gi.enter = true;
    }
    if at.once(12.4, "04") {
        let ok = p.vehicle.is_some();
        at.log.push(format!("entered car: {ok}"));
        shot(&mut commands, "04_in_car");
    }
    if (12.5..16.5).contains(&t) {
        gi.throttle = 1.0;
        gi.steer = 0.0;
    }
    if at.once(16.0, "05") {
        if let Some((_, _, ct, vel)) = p.vehicle.and_then(|e| cars.get(e).ok()) {
            at.log.push(format!("speed at t=16: {:.1} km/h, pos {:?}", vel.linear.length() * 3.6, ct.translation));
        }
        shot(&mut commands, "05_driving");
    }
    if (18.0..20.0).contains(&t) {
        gi.throttle = 0.6;
        gi.handbrake = t < 19.0;
        gi.steer = 1.0;
    }
    if at.once(19.2, "06") {
        if let Some((_, v, _, vel)) = p.vehicle.and_then(|e| cars.get(e).ok()) {
            let slip: f32 = v.wheels.iter().map(|w| w.slip).fold(0.0, f32::max);
            at.log.push(format!("drift t=19.2: speed {:.1} km/h, max wheel slip {:.1} m/s", vel.linear.length() * 3.6, slip));
        }
        shot(&mut commands, "06_drift");
    }
    if at.once(20.4, "pre_ignite") {
        at.log.push(format!("health before ignite: {:.0}", p.health));
    }
    if at.once(20.5, "ignite") {
        if let Some((_, mut v, _, _)) = p.vehicle.and_then(|e| cars.get_mut(e).ok()) {
            v.health = 0.0;
        }
    }
    if at.once(21.2, "bail") {
        gi.enter = true;
    }
    if (21.4..25.0).contains(&t) {
        // sprint straight away from the burning car (world direction -> camera-relative input)
        if let Some(cp) = cars.iter().filter(|c| c.1.burn_t.is_some()).map(|c| c.2.translation).next() {
            let away = (pt.translation - cp).with_y(0.0).normalize_or(Vec3::X);
            let fwd = Vec3::new(-rig.yaw.sin(), 0.0, -rig.yaw.cos());
            let right = Vec3::new(rig.yaw.cos(), 0.0, -rig.yaw.sin());
            gi.move_axis = Vec2::new(away.dot(right), away.dot(fwd));
            gi.sprint = true;
        }
    }
    if at.once(23.0, "07") {
        let d = cars.iter().filter(|c| c.1.burn_t.is_some()).map(|c| c.2.translation.distance(pt.translation)).fold(f32::MAX, f32::min);
        at.log.push(format!("on foot after bail: {} health {:.0} dist to burning car {:.1}", p.vehicle.is_none(), p.health, d));
        shot(&mut commands, "07_fire");
    }
    if at.once(26.2, "08") {
        shot(&mut commands, "08_explosion");
    }
    if at.once(28.5, "09") {
        let wrecks = cars.iter().filter(|c| c.1.wrecked).count();
        at.log.push(format!("wrecked cars: {wrecks}, player health {:.0}", p.health));
        shot(&mut commands, "09_aftermath");
    }
    if at.once(29.0, "tp_taxi") {
        if let Some((_, v, ct, _)) = showcase.npc_car.and_then(|e| cars.get(e).ok()) {
            pt.translation = ct.translation + ct.rotation * Vec3::new(-(v.spec.width * 0.5 + 1.0), 0.5, 0.0);
            p.vel = Vec3::ZERO;
        }
    }
    if at.once(29.6, "jack") {
        gi.enter = true;
    }
    if at.once(30.6, "10") {
        at.log.push(format!("carjack -> in vehicle: {}", p.vehicle.is_some()));
        shot(&mut commands, "10_carjack");
    }
    if at.once(31.2, "exit2") {
        gi.enter = true;
    }
    let vistas: [(f32, &'static str, Vec3, Vec3); 6] = [
        (31.5, "11_vista_hill", Vec3::new(-60.0, 120.0, -640.0), Vec3::new(0.0, 30.0, 0.0)),
        (34.5, "12_beach", Vec3::new(-120.0, 6.0, 610.0), Vec3::new(40.0, 25.0, 380.0)),
        (37.5, "13_airport", Vec3::new(-880.0, 35.0, 230.0), Vec3::new(-1100.0, 0.0, -120.0)),
        (40.5, "14_docks", Vec3::new(700.0, 28.0, 190.0), Vec3::new(520.0, 8.0, -80.0)),
        (43.5, "15_street", Vec3::new(crate::world::road_center(5) - 3.0, 1.8, crate::world::block_min(5) + 60.0), Vec3::new(crate::world::road_center(5) + 2.0, 12.0, crate::world::block_min(3))),
        (46.5, "16_downtown_air", Vec3::new(-250.0, 140.0, 300.0), Vec3::new(0.0, 40.0, 0.0)),
    ];
    for (start, name, pos, look) in vistas {
        if t >= start && t < start + 3.0 {
            cam.0 = Some(Transform::from_translation(pos).looking_at(look, Vec3::Y));
        }
        if at.once(start + 2.2, name) {
            shot(&mut commands, name);
        }
    }
    if at.once(50.0, "end") {
        let mut ft = at.frame_times.clone();
        let n = ft.len().max(1);
        let avg = ft.iter().sum::<f32>() / n as f32;
        ft.sort_by(|a, b| b.partial_cmp(a).unwrap());
        let p1 = ft[(n / 100).min(n - 1)];
        let worst = ft.first().copied().unwrap_or(0.0);
        println!("AUTOTEST frames={n} avg_fps={:.1} 1%_low_fps={:.1} worst_frame_ms={:.1} entities={}", 1.0 / avg, 1.0 / p1, worst * 1000.0, all.iter().count());
        for l in &at.log {
            println!("AUTOTEST {l}");
        }
    }
    if at.once(51.0, "quit") {
        crate::lighting::request_quit(&mut quit);
    }
    over.0 = Some(gi);
}

#[derive(Resource, Default)]
struct Bench {
    t: f32,
    seg: Vec<f32>,
    results: Vec<String>,
}

/// Fixed-view frame-rate benchmark: street level, aerial, third person.
fn bench(
    time: Res<Time>,
    mut b: ResMut<Bench>,
    mut cam: ResMut<CameraOverride>,
    mut over: ResMut<InputOverride>,
    mut quit: ResMut<crate::lighting::QuitRequest>,
    diags: Res<bevy::diagnostic::DiagnosticsStore>,
) {
    let dt = time.delta_secs();
    b.t += dt;
    over.0 = Some(GameInput::default());
    let views: [(&str, Option<(Vec3, Vec3)>); 3] = [
        ("street", Some((Vec3::new(crate::world::road_center(5) - 3.0, 1.8, crate::world::block_min(5) + 60.0), Vec3::new(crate::world::road_center(5) + 2.0, 12.0, crate::world::block_min(3))))),
        ("aerial", Some((Vec3::new(-250.0, 140.0, 300.0), Vec3::new(0.0, 40.0, 0.0)))),
        ("third_person", None),
    ];
    let start = 8.0;
    let seg_len = 6.0;
    let idx = ((b.t - start) / seg_len).floor();
    if idx < 0.0 {
        cam.0 = views[0].1.map(|(p, l)| Transform::from_translation(p).looking_at(l, Vec3::Y));
        return;
    }
    let idx = idx as usize;
    if idx >= views.len() {
        if b.seg.len() != usize::MAX {
            for r in &b.results {
                println!("BENCH {r}");
            }
            b.seg = Vec::new();
            b.seg.shrink_to(0);
        }
        b.results.clear();
        crate::lighting::request_quit(&mut quit);
        return;
    }
    cam.0 = views[idx].1.map(|(p, l)| Transform::from_translation(p).looking_at(l, Vec3::Y));
    let local = b.t - start - idx as f32 * seg_len;
    if local > 1.0 {
        b.seg.push(dt);
    }
    if local + dt >= seg_len && b.results.len() == idx {
        let n = b.seg.len().max(1) as f32;
        let avg = b.seg.iter().sum::<f32>() / n;
        let worst = b.seg.iter().cloned().fold(0.0, f32::max);
        let mut gpu: Vec<(String, f64)> = diags
            .iter()
            .filter(|d| d.path().as_str().ends_with("elapsed_gpu") && d.path().as_str().matches('/').count() == 2)
            .filter_map(|d| d.smoothed().map(|v| (d.path().as_str().trim_start_matches("render/").trim_end_matches("/elapsed_gpu").to_string(), v)))
            .collect();
        gpu.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let total: f64 = gpu.iter().map(|g| g.1).sum();
        let top: Vec<String> = gpu.iter().take(6).map(|(n, v)| format!("{n} {v:.2}")).collect();
        let r = format!(
            "{}: avg {:.1} fps ({:.2} ms), worst {:.1} ms | GPU total {:.2} ms [{}]",
            views[idx].0,
            1.0 / avg,
            avg * 1000.0,
            worst * 1000.0,
            total,
            top.join(", ")
        );
        b.results.push(r);
        b.seg.clear();
    }
}

/// Sprint in a straight line and log what the character controller actually does.
fn walktest(
    time: Res<Time>,
    mut at: ResMut<AutoTest>,
    mut over: ResMut<InputOverride>,
    q: Query<(&Transform, &Player, Option<&bevy_rapier3d::prelude::KinematicCharacterControllerOutput>)>,
    mut quit: ResMut<crate::lighting::QuitRequest>,
) {
    let dt = time.delta_secs();
    at.t += dt;
    let t = at.t;
    let mut gi = GameInput::default();
    if (4.0..7.0).contains(&t) {
        gi.move_axis = Vec2::new(0.0, 1.0);
        gi.sprint = true;
    }
    if (3.9..7.2).contains(&t) && (t * 4.0).floor() != ((t - dt) * 4.0).floor() {
        if let Ok((tr, p, out)) = q.single() {
            let (g, des, eff, n) = out.map(|o| (o.grounded, o.desired_translation, o.effective_translation, o.collisions.len())).unwrap_or_default();
            println!("WALK t={t:.2} pos {:.2?} vel {:.2?} grounded {g} desired {:.4?} effective {:.4?} collisions {n} dt {:.4}", tr.translation, p.vel, des, eff, dt);
        }
    }
    if t > 7.5 {
        crate::lighting::request_quit(&mut quit);
    }
    over.0 = Some(gi);
}

#[derive(Resource, Default)]
struct Shots {
    dir: String,
    t: f32,
    taken: usize,
}

/// Fixed comparison views -> `$HH_SHOTS/<name>.png`, then exit.
fn shots(
    mut commands: Commands,
    time: Res<Time>,
    mut st: ResMut<Shots>,
    mut cam: ResMut<CameraOverride>,
    mut over: ResMut<InputOverride>,
    mut quit: ResMut<crate::lighting::QuitRequest>,
    mut seats: Query<(&Transform, &mut crate::seats::Seat)>,
) {
    st.t += time.delta_secs();
    over.0 = Some(GameInput::default());
    let sp = crate::world::spawn_point();
    let car_row = Vec3::new(crate::world::block_min(4) + 20.0, 0.5, crate::world::road_center(6) + 5.25);
    let sports = Vec3::new(crate::world::block_min(4) + 38.0, 0.5, crate::world::road_center(6) + 5.25);
    // put people behind the wheel of the parked showcase cars
    if st.t > 2.0 && st.t < 2.5 {
        for (t, mut seat) in &mut seats {
            if seat.kind == crate::seats::SeatKind::Car && t.translation.distance(car_row) < 30.0 && seat.driver == crate::vehicle::Driver::None {
                seat.driver = crate::vehicle::Driver::Npc((t.translation.x * 10.0) as u32);
            }
        }
    }
    if std::env::var("HH_SHOTS_DEBUG").is_ok() && (st.t * 2.0) as i32 != ((st.t - time.delta_secs()) * 2.0) as i32 {
        for (t, seat) in &seats {
            if t.translation.distance(car_row) < 60.0 {
                println!("t={:.1} {} {:?} {:?}", st.t, seat.name, t.translation, seat.driver);
            }
        }
    }
    let bay = crate::world::respray_bay(0);
    let views: [(&str, Vec3, Vec3); 12] = [
        ("respray", bay + Vec3::new(7.0, 3.2, 20.0), bay + Vec3::new(0.0, 2.5, 3.0)),
        ("street_jobs", sp + Vec3::new(4.0, 0.9, 0.6), sp + Vec3::new(24.0, -0.4, 0.0)),
        ("marker", sp + Vec3::new(24.0, 1.0, 1.2), sp + Vec3::new(31.0, 0.0, 0.0)),
        ("ramp", Vec3::new(-548.0, 2.6, 5.0), Vec3::new(-566.0, 1.0, -3.5)),
        ("driver_sedan", car_row + Vec3::new(4.0, 1.5, -3.6), car_row + Vec3::new(3.0, 0.6, 0.0)),
        ("driver_sports", sports + Vec3::new(1.0, 1.4, -3.4), sports + Vec3::new(0.0, 0.4, 0.0)),
        ("car_front", sports + Vec3::new(4.8, 1.1, -3.4), sports + Vec3::new(0.0, 0.1, 0.0)),
        ("street", Vec3::new(crate::world::road_center(5) - 3.0, 1.8, crate::world::block_min(5) + 60.0), Vec3::new(crate::world::road_center(5) + 2.0, 12.0, crate::world::block_min(3))),
        ("cars", car_row + Vec3::new(-7.0, 1.6, 6.5), car_row + Vec3::new(6.0, 0.2, 0.0)),
        ("player", sp + Vec3::new(2.2, 0.8, -2.6), sp + Vec3::new(0.0, 0.0, 0.0)),
        ("aerial", Vec3::new(-250.0, 140.0, 300.0), Vec3::new(0.0, 40.0, 0.0)),
        ("docks", Vec3::new(700.0, 28.0, 190.0), Vec3::new(520.0, 8.0, -80.0)),
    ];
    let first = 9.0;
    let per = 4.0;
    let idx = if st.t < first { 0 } else { (((st.t - first) / per) as usize + 1).min(views.len()) };
    if idx >= views.len() {
        crate::lighting::request_quit(&mut quit);
        return;
    }
    let (name, pos, look) = views[idx];
    cam.0 = Some(Transform::from_translation(pos).looking_at(look, Vec3::Y));
    let due = first + idx as f32 * per - 0.3;
    if st.t >= due && st.taken == idx {
        st.taken += 1;
        let path = format!("{}/{}.png", st.dir, name);
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
}

#[derive(Resource, Default)]
struct Combat {
    t: f32,
    done: Vec<&'static str>,
}

impl Combat {
    fn once(&mut self, at: f32, id: &'static str) -> bool {
        if self.t >= at && !self.done.contains(&id) {
            self.done.push(id);
            println!("COMBAT step {id} at t={:.2}", self.t);
            true
        } else {
            false
        }
    }
}

/// Aim the camera rig so the screen centre points at `target`.
fn aim_at(rig: &mut crate::camera::CameraRig, from: Vec3, target: Vec3) {
    let d = target - from;
    rig.yaw = (-d.x).atan2(-d.z);
    rig.pitch = (d.y / d.length().max(0.01)).asin().clamp(-1.2, 0.6);
    rig.idle = 0.0;
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn combattest(
    mut commands: Commands,
    time: Res<Time>,
    mut ct: ResMut<Combat>,
    mut over: ResMut<InputOverride>,
    mut rig: ResMut<crate::camera::CameraRig>,
    player: Query<(&Player, &Transform)>,
    mut peds: Query<(&mut crate::peds::Ped, &mut Transform), Without<Player>>,
    police: Query<&Transform, (With<crate::wanted::PoliceCar>, Without<crate::peds::Ped>, Without<Player>)>,
    wanted: Res<crate::wanted::Wanted>,
    ars: Res<crate::weapons::Arsenal>,
    ragdolls: Query<&crate::ragdoll::RagdollParts>,
    cam: Query<&GlobalTransform, With<crate::camera::MainCamera>>,
    mut quit: ResMut<crate::lighting::QuitRequest>,
) {
    ct.t += time.delta_secs();
    let t = ct.t;
    let mut gi = GameInput::default();
    let Ok((p, pt)) = player.single() else { return };
    let pivot = cam.single().map(|c| c.translation()).unwrap_or(pt.translation + Vec3::Y * 0.7);
    // a test subject: the nearest civilian is moved 8 m in front of the player and held still
    if t > 6.4 && t < 6.6 {
        let fwd = pt.rotation * Vec3::NEG_Z;
        if let Some((mut pd, mut tf)) = peds.iter_mut().filter(|(pd, _)| pd.health > 0.0 && !pd.cop).min_by(|a, b| a.1.translation.distance(pt.translation).partial_cmp(&b.1.translation.distance(pt.translation)).unwrap()) {
            tf.translation = pt.translation + fwd * 8.0;
            pd.state = crate::peds::PedState::Idle { t: 1000.0 };
            pd.ambient = false;
        }
    }
    let nearest_ped = peds
        .iter()
        .filter(|(pd, _)| pd.health > 0.0 && !pd.cop)
        .map(|(_, t)| t.translation)
        .min_by(|a, b| a.distance(pt.translation).partial_cmp(&b.distance(pt.translation)).unwrap());
    if ct.once(6.0, "c01") {
        println!("COMBAT peds alive: {}", peds.iter().filter(|(pd, _)| pd.health > 0.0).count());
        shot(&mut commands, "c01_peds");
    }
    if (6.5..11.0).contains(&t) {
        gi.weapon_slot = if t < 6.6 { Some(1) } else { None };
        gi.aim = true;
        if let Some(target) = nearest_ped {
            aim_at(&mut rig, pivot, target + Vec3::Y * 0.3);
        }
        // tap the trigger every 0.4 s
        gi.fire_pressed = t > 7.0 && ((t * 2.5).fract() < 0.1);
        gi.fire = gi.fire_pressed;
    }
    if ct.once(7.3, "c02") {
        shot(&mut commands, "c02_aim");
    }
    if ct.once(11.5, "c03") {
        println!("COMBAT ragdolls: {} stars {} pistol ammo {:?}", ragdolls.iter().count(), wanted.stars, ars.ammo[1]);
        shot(&mut commands, "c03_ragdoll");
    }
    if ct.once(22.0, "c04") {
        println!("COMBAT stars {} police cars {} cops on foot {} player health {:.0}", wanted.stars, police.iter().count(), peds.iter().filter(|(pd, _)| pd.cop).count(), p.health);
        shot(&mut commands, "c04_police");
    }
    if (22.5..25.0).contains(&t) {
        gi.weapon_slot = if t < 22.6 { Some(4) } else { None };
        gi.aim = true;
        let target = police.iter().map(|t| t.translation).min_by(|a, b| a.distance(pt.translation).partial_cmp(&b.distance(pt.translation)).unwrap());
        if let Some(target) = target {
            aim_at(&mut rig, pivot, target);
        }
        gi.fire_pressed = (23.5..23.55).contains(&t);
    }
    if ct.once(24.2, "c05") {
        shot(&mut commands, "c05_rocket");
    }
    if (25.5..28.0).contains(&t) {
        gi.weapon_slot = if t < 25.6 { Some(2) } else { None };
        gi.aim = true;
        gi.fire = t > 26.0;
        if let Some(target) = nearest_ped {
            aim_at(&mut rig, pivot, target + Vec3::Y * 0.3);
        }
    }
    if ct.once(27.0, "c06") {
        shot(&mut commands, "c06_smg");
    }
    if ct.once(30.0, "end") {
        println!("COMBAT end: stars {} health {:.0} money {} ragdolls {} busted_t {:.1}", wanted.stars, p.health, p.money, ragdolls.iter().count(), p.busted_t);
    }
    if ct.once(31.0, "quit") {
        crate::lighting::request_quit(&mut quit);
    }
    over.0 = Some(gi);
}

#[derive(Resource, Default)]
struct World2 {
    t: f32,
    done: Vec<&'static str>,
    log_t: f32,
}

impl World2 {
    fn once(&mut self, at: f32, id: &'static str) -> bool {
        if self.t >= at && !self.done.contains(&id) {
            self.done.push(id);
            true
        } else {
            false
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn worldtest(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut st: ResMut<World2>,
    mut over: ResMut<InputOverride>,
    traffic: Query<(&Transform, &Velocity), With<crate::traffic::TrafficAi>>,
    police: Query<&crate::wanted::PoliceCar>,
    cops: Query<&crate::peds::Ped>,
    player: Query<(&Player, &Transform), Without<crate::traffic::TrafficAi>>,
    wanted: Res<crate::wanted::Wanted>,
    mut wp: ResMut<crate::map::Waypoint>,
    mut spawn_req: ResMut<crate::aircraft::SpawnRequest>,
    seats: Query<(&Transform, &crate::seats::Seat), Without<Player>>,
    mut quit: ResMut<crate::lighting::QuitRequest>,
) {
    st.t += time.delta_secs();
    let t = st.t;
    let mut gi = GameInput::default();
    let Ok((p, pt)) = player.single() else { return };
    if st.once(12.05, "traffic") {
        let n = traffic.iter().count();
        let moving = traffic.iter().filter(|(_, v)| v.linear.length() > 2.0).count();
        let near = traffic.iter().filter(|(t, _)| t.translation.distance(pt.translation) < 120.0).count();
        println!("WORLD traffic cars {n}, moving {moving}, within 120 m {near}");
        shot(&mut commands, "w01_traffic");
    }
    if st.once(9.0, "pause_open") {
        gi.pause = true;
    }
    if st.once(9.6, "pause_shot") {
        shot(&mut commands, "w00_pause");
    }
    if st.once(10.2, "pause_close") {
        gi.pause = true;
    }
    if st.once(10.6, "spawn_open") {
        gi.spawn_menu = true;
    }
    if st.once(11.2, "spawn_shot") {
        shot(&mut commands, "w00_spawnmenu");
    }
    if st.once(11.5, "spawn_req") {
        spawn_req.0 = Some(8);
    }
    if st.once(11.8, "spawn_check") {
        println!("WORLD aircraft within 30 m after spawn request: {}", seats.iter().filter(|(t, s)| matches!(s.kind, crate::seats::SeatKind::Heli) && t.translation.distance(pt.translation) < 30.0).count());
    }
    if st.once(13.0, "map_open") {
        gi.map = true;
        wp.0 = Some(Vec2::new(-1000.0, 100.0));
    }
    if st.once(14.0, "map_shot") {
        shot(&mut commands, "w02_map");
    }
    if st.once(14.5, "map_close") {
        gi.map = true;
    }
    if st.once(15.5, "minimap_shot") {
        shot(&mut commands, "w03_blips");
    }
    if st.once(16.0, "crime") {
        crate::wanted::report_crime(&mut commands, crate::wanted::Crime::Assault, pt.translation);
    }
    st.log_t -= time.delta_secs();
    if t > 16.0 && st.log_t <= 0.0 && !st.done.contains(&"busted") {
        st.log_t = 4.0;
        println!(
            "WORLD t={t:.0} stars {} police cars {} cops on foot {} deployed {}",
            wanted.stars,
            police.iter().count(),
            cops.iter().filter(|c| c.cop).count(),
            police.iter().filter(|c| c.deployed).count()
        );
    }
    if p.busted_t > 0.0 && st.once(t, "busted") {
        println!("WORLD BUSTED at t={t:.1}, money before fine {}", p.money);
        shot(&mut commands, "w04_busted");
    }
    if st.done.contains(&"busted") && p.busted_t == 0.0 && st.once(t, "released") {
        println!("WORLD released at {:?} (police station {:?}), money {}", pt.translation, crate::world::police_point(), p.money);
    }
    if st.once(80.0, "quit") || (st.done.contains(&"released") && st.once(t, "quit2")) {
        crate::lighting::request_quit(&mut quit);
    }
    over.0 = Some(gi);
}

#[derive(Resource, Default)]
struct Flight {
    t: f32,
    done: Vec<&'static str>,
    log_t: f32,
}

impl Flight {
    fn once(&mut self, at: f32, id: &'static str) -> bool {
        if self.t >= at && !self.done.contains(&id) {
            self.done.push(id);
            true
        } else {
            false
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn flighttest(
    mut commands: Commands,
    time: Res<Time>,
    mut st: ResMut<Flight>,
    mut over: ResMut<InputOverride>,
    mut player: Query<(&mut Player, &mut Transform), Without<crate::aircraft::Aircraft>>,
    air: Query<(Entity, &Transform, &Velocity, &crate::aircraft::Aircraft, &crate::seats::Seat)>,
    mut quit: ResMut<crate::lighting::QuitRequest>,
) {
    st.t += time.delta_secs();
    let t = st.t;
    let mut gi = GameInput::default();
    let Ok((mut p, mut pt)) = player.single_mut() else { return };
    if st.once(6.0, "tp_plane") {
        // the prop plane parked at the start of the runway (westernmost)
        let plane = air.iter().filter(|a| a.3.kind == crate::aircraft::AirKind::Prop).map(|a| (a.1.translation, a.1.rotation)).min_by(|a, b| a.0.x.partial_cmp(&b.0.x).unwrap());
        if let Some((pos, rot)) = plane {
            pt.translation = pos + rot * Vec3::new(-2.0, 0.6, -1.5);
            p.vel = Vec3::ZERO;
        }
    }
    if st.once(6.6, "enter_plane") {
        gi.enter = true;
    }
    let in_air = p.vehicle.and_then(|e| air.get(e).ok());
    if (7.0..40.0).contains(&t) {
        if let Some((_, _, v, _, _)) = in_air {
            gi.throttle_axis = 1.0;
            let spd = v.linear.length();
            // rotate at ~33 m/s, then hold a gentle climb
            gi.move_axis.y = if t > 30.0 { -0.15 } else if spd > 33.0 { -0.45 } else { 0.0 };
        }
    }
    st.log_t -= time.delta_secs();
    if st.log_t <= 0.0 && t > 7.0 && t < 57.0 {
        st.log_t = 3.0;
        if let Some((_, tr, v, a, s)) = in_air {
            println!("FLIGHT t={t:.0} {} speed {:.0} km/h alt {:.1} m throttle {:.2} health {:.0}", s.name, v.linear.length() * 3.6, tr.translation.y, a.throttle, a.health);
        }
    }
    if st.once(26.0, "shot_plane") {
        shot(&mut commands, "f01_plane");
    }
    if st.once(41.0, "tp_heli") {
        // leave the plane flying and jump straight into a helicopter
        p.vehicle = None;
        p.health = 100.0;
        let heli = air.iter().filter(|a| a.3.kind == crate::aircraft::AirKind::Heli).map(|a| (a.1.translation, a.1.rotation)).min_by(|a, b| a.0.x.partial_cmp(&b.0.x).unwrap());
        if let Some((pos, rot)) = heli {
            pt.translation = pos + rot * Vec3::new(-2.2, 1.0, 0.0);
            p.vel = Vec3::ZERO;
        }
    }
    if st.once(42.5, "enter_heli") {
        gi.enter = true;
    }
    if (43.5..50.0).contains(&t) {
        gi.throttle_axis = 1.0;
    }
    if (50.0..55.0).contains(&t) {
        gi.move_axis.y = 0.4;
    }
    if st.once(52.0, "shot_heli") {
        shot(&mut commands, "f02_heli");
    }
    if st.once(56.0, "quit") {
        crate::lighting::request_quit(&mut quit);
    }
    over.0 = Some(gi);
}

#[derive(Resource, Default)]
struct Toggle {
    t: f32,
    step: u32,
}

/// F4 at runtime: raster -> ray traced -> raster, screenshots after each switch.
fn toggletest(mut commands: Commands, time: Res<Time<Real>>, mut st: ResMut<Toggle>, mut over: ResMut<InputOverride>, lighting: Res<crate::lighting::Lighting>, mut quit: ResMut<crate::lighting::QuitRequest>) {
    st.t += time.delta_secs();
    let mut gi = GameInput::default();
    let plan = [(6.0, "toggle"), (12.0, "shot_rt"), (13.0, "toggle"), (17.0, "shot_raster"), (18.0, "quit")];
    if let Some(&(at, what)) = plan.get(st.step as usize) {
        if st.t >= at {
            st.step += 1;
            match what {
                "toggle" => gi.toggle_lighting = true,
                "shot_rt" => {
                    println!("TOGGLE rt on: {}", lighting.rt);
                    shot(&mut commands, "t01_toggled_rt");
                }
                "shot_raster" => {
                    println!("TOGGLE rt on: {}", lighting.rt);
                    shot(&mut commands, "t02_toggled_raster");
                }
                _ => crate::lighting::request_quit(&mut quit),
            }
        }
    }
    over.0 = Some(gi);
}

#[derive(Resource, Default)]
struct JobTest {
    t: f32,
    done: Vec<&'static str>,
    mark: i64,
    jobs_done: u32,
    tries: u32,
    pass: Vec<String>,
    fail: Vec<String>,
}

impl JobTest {
    fn once(&mut self, at: f32, id: &'static str) -> bool {
        if self.t >= at && !self.done.contains(&id) {
            self.done.push(id);
            println!("JOBTEST step {id} at t={:.2}", self.t);
            true
        } else {
            false
        }
    }
    fn check(&mut self, ok: bool, what: String) {
        println!("JOBTEST {} {what}", if ok { "PASS" } else { "FAIL" });
        if ok {
            self.pass.push(what)
        } else {
            self.fail.push(what)
        }
    }
}

/// Exercises pickups, every job type, the respray garage, a stunt ramp and a
/// ped fighting back, by teleporting through them.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn jobtest(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut st: ResMut<JobTest>,
    mut over: ResMut<InputOverride>,
    mut player: Query<(&mut Player, &mut Transform), (Without<crate::seats::Seat>, Without<crate::peds::Ped>, Without<crate::activities::Pickup>)>,
    mut seats: Query<(Entity, &mut Transform, &mut Velocity, &crate::seats::Seat, Option<&Vehicle>), (Without<Player>, Without<crate::peds::Ped>)>,
    pickups: Query<(&crate::activities::Pickup, &Transform), (Without<Player>, Without<crate::seats::Seat>, Without<crate::peds::Ped>)>,
    mut peds: Query<(&mut crate::peds::Ped, &mut Transform), (Without<Player>, Without<crate::seats::Seat>, Without<crate::activities::Pickup>)>,
    jobs: Res<crate::missions::Jobs>,
    mut wanted: ResMut<crate::wanted::Wanted>,
    ars: Res<crate::weapons::Arsenal>,
    stunts: Res<crate::activities::StuntState>,
    mut quit: ResMut<crate::lighting::QuitRequest>,
) {
    use crate::activities::PickupKind;
    let dt = time.delta_secs();
    st.t += dt;
    let t = st.t;
    let mut gi = GameInput::default();
    let Ok((mut p, mut pt)) = player.single_mut() else { return };
    let ground = |v: Vec3| Vec3::new(v.x, v.y.max(crate::world::ground_height(v.x, v.z)) + 1.0, v.z);
    let nearest = |kind: &dyn Fn(PickupKind) -> bool, from: Vec3| {
        pickups.iter().filter(|(k, _)| kind(k.kind)).map(|(_, tr)| tr.translation).min_by(|a, b| a.distance(from).total_cmp(&b.distance(from)))
    };
    // ---------------- pickups ----------------
    if st.once(6.0, "cash_go") {
        st.mark = p.money;
        if let Some(c) = nearest(&|k| matches!(k, PickupKind::Cash(_)), pt.translation) {
            pt.translation = c + Vec3::Y * 0.2;
        }
    }
    if st.once(6.8, "cash_check") {
        let gain = p.money - st.mark;
        st.check(gain > 0, format!("cash pickup: +${gain}"));
    }
    if st.once(7.0, "gun_go") {
        if let Some(c) = nearest(&|k| matches!(k, PickupKind::Weapon(2)), pt.translation) {
            pt.translation = c + Vec3::Y * 0.2;
        }
    }
    if st.once(7.8, "gun_check") {
        let a = ars.ammo[2];
        st.check(a.0 + a.1 > 0, format!("SMG pickup: ammo {a:?}"));
    }
    if st.once(8.0, "armor_go") {
        p.health = 60.0;
        if let Some(c) = nearest(&|k| matches!(k, PickupKind::Armor), pt.translation) {
            pt.translation = c + Vec3::Y * 0.2;
        }
    }
    if st.once(8.8, "armor_check") {
        let armor = p.armor;
        st.check(armor >= 99.0, format!("armour pickup: armour {armor:.0}"));
        p.hurt(30.0);
        let (a, h) = (p.armor, p.health);
        st.check((a - 70.0).abs() < 0.5 && (h - 60.0).abs() < 0.5, format!("armour soaks damage: armour {a:.0} health {h:.0}"));
        p.health = 100.0;
    }
    // ---------------- courier ----------------
    let givers = crate::missions::giver_positions();
    if st.once(9.5, "courier_go") {
        st.jobs_done = jobs.completed;
        st.mark = p.money;
        pt.translation = givers[0].1 + Vec3::Y * 1.0;
    }
    if st.once(10.5, "courier_started") {
        st.check(jobs.name() == "courier", format!("courier job started ({})", jobs.name()));
    }
    // hop to each drop until the job is done
    if t > 11.0 && t < 20.0 && jobs.name() == "courier" && (t % 1.0) < dt {
        if let Some(target) = jobs.target() {
            pt.translation = ground(target);
            st.tries += 1;
        }
    }
    if st.once(20.0, "courier_check") {
        let gain = p.money - st.mark;
        let ok = jobs.completed == st.jobs_done + 1 && gain >= 1200;
        let tries = st.tries;
        st.check(ok, format!("courier complete: +${gain} after {tries} hops"));
        shot(&mut commands, "j01_after_courier");
    }
    // ---------------- contract ----------------
    if st.once(24.5, "contract_go") {
        st.jobs_done = jobs.completed;
        st.mark = p.money;
        pt.translation = givers[1].1 + Vec3::Y * 1.0;
    }
    if st.once(25.5, "contract_started") {
        st.check(jobs.name() == "contract", format!("contract started ({})", jobs.name()));
        if let Some(place) = jobs.target() {
            // walk up on the crew
            pt.translation = ground(place + Vec3::new(12.0, 0.0, 0.0));
        }
    }
    if st.once(28.5, "contract_shot") {
        let gang = peds.iter().filter(|(pd, _)| pd.gang && pd.health > 0.0).count();
        let fighting = peds.iter().filter(|(pd, _)| pd.gang && matches!(pd.state, crate::peds::PedState::Fight { .. })).count();
        let h = p.health;
        st.check(gang == 4 && fighting > 0, format!("crew of {gang}, {fighting} engaging, player health {h:.0}"));
        shot(&mut commands, "j02_contract");
    }
    if st.once(29.0, "contract_kill") {
        p.health = 100.0;
        for (mut pd, _) in &mut peds {
            if pd.gang {
                pd.health = 0.0;
            }
        }
    }
    if st.once(30.0, "contract_check") {
        let gain = p.money - st.mark;
        let ok = jobs.completed == st.jobs_done + 1 && gain >= 2500;
        st.check(ok, format!("contract complete: +${gain}"));
        wanted.clear();
        p.health = 100.0;
    }
    // ---------------- export ----------------
    if st.once(34.5, "export_go") {
        st.jobs_done = jobs.completed;
        st.mark = p.money;
        pt.translation = givers[2].1 + Vec3::Y * 1.0;
    }
    if st.once(35.5, "export_find") {
        st.check(jobs.name() == "export", format!("export started ({})", jobs.name()));
        if let Some(car) = jobs.export_car() {
            if let Ok((_, ct, ..)) = seats.get(car) {
                pt.translation = ct.translation + ct.rotation * Vec3::new(-1.6, 0.4, 0.0);
            }
        }
    }
    if st.once(36.0, "export_enter") {
        gi.enter = true;
    }
    if st.once(37.0, "export_drive") {
        let in_car = p.vehicle.is_some() && p.vehicle == jobs.export_car();
        st.check(in_car, "in the export car".into());
        let stars = wanted.stars;
        st.check(stars >= 1, format!("car alarm: {stars} stars"));
        if let (Some(car), Some(target)) = (jobs.export_car(), jobs.target()) {
            if let Ok((_, mut ct, mut v, ..)) = seats.get_mut(car) {
                ct.translation = target + Vec3::Y * 0.6;
                v.linear = Vec3::ZERO;
                v.angular = Vec3::ZERO;
            }
        }
    }
    if st.once(39.0, "export_check") {
        let gain = p.money - st.mark;
        let ok = jobs.completed == st.jobs_done + 1 && gain >= 750;
        st.check(ok, format!("export complete: +${gain}"));
        wanted.clear();
        gi.enter = true;
    }
    // ---------------- taxi ----------------
    if st.once(40.0, "taxi_go") {
        let me = pt.translation;
        let cab = seats
            .iter()
            .filter(|(_, _, vel, seat, v)| v.is_some_and(|v| v.spec.kind == crate::vehicle::CarKind::Taxi && !v.wrecked) && vel.linear.length() < 0.5 && seat.driver != crate::vehicle::Driver::Player)
            .min_by(|a, b| a.1.translation.distance(me).total_cmp(&b.1.translation.distance(me)))
            .map(|(e, ct, ..)| (e, ct.translation, ct.rotation));
        if let Some((_, pos, rot)) = cab {
            pt.translation = pos + rot * Vec3::new(-1.6, 0.4, 0.0);
        }
    }
    if st.once(40.4, "taxi_enter") {
        gi.enter = true;
    }
    if st.once(41.4, "taxi_start") {
        st.mark = p.money;
        gi.job = true;
    }
    if st.once(42.0, "taxi_started") {
        st.check(jobs.name() == "taxi", format!("taxi job started ({})", jobs.name()));
    }
    if t > 42.5 && t < 48.0 && jobs.name() == "taxi" && (t % 1.5) < dt {
        if let (Some(v), Some(target)) = (p.vehicle, jobs.target()) {
            if let Ok((_, mut ct, mut vel, ..)) = seats.get_mut(v) {
                ct.translation = Vec3::new(target.x, target.y.max(0.0) + 0.6, target.z);
                vel.linear = Vec3::ZERO;
                vel.angular = Vec3::ZERO;
            }
        }
    }
    if st.once(48.0, "taxi_check") {
        let gain = p.money - st.mark;
        st.check(gain > 0, format!("taxi fares paid: +${gain}"));
        gi.job = true;
    }
    if st.once(48.6, "taxi_quit_check") {
        st.check(jobs.name() == "none", format!("J quits the taxi job ({})", jobs.name()));
    }
    // ---------------- respray ----------------
    if st.once(49.0, "respray_go") {
        wanted.stars = 3;
        st.mark = p.money;
        if let Some(v) = p.vehicle {
            if let Ok((_, mut ct, mut vel, ..)) = seats.get_mut(v) {
                let bay = crate::world::respray_bay(0);
                ct.translation = bay + Vec3::Y * 0.6;
                ct.rotation = Quat::IDENTITY;
                vel.linear = Vec3::ZERO;
                vel.angular = Vec3::ZERO;
            }
        }
    }
    if st.once(50.4, "respray_shot") {
        shot(&mut commands, "j03_respray");
    }
    if st.once(53.5, "respray_check") {
        let paid = st.mark - p.money;
        let stars = wanted.stars;
        st.check(stars == 0 && paid == crate::activities::RESPRAY_COST, format!("respray: stars {stars} paid ${paid}"));
    }
    // ---------------- stunt ramp ----------------
    if st.once(54.0, "stunt_go") {
        st.mark = p.money;
        if let Some(v) = p.vehicle {
            if let Ok((_, mut ct, mut vel, ..)) = seats.get_mut(v) {
                ct.translation = Vec3::new(-515.0, 0.7, -3.5);
                ct.rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
                vel.linear = Vec3::new(-34.0, 0.0, 0.0);
                vel.angular = Vec3::ZERO;
            }
        }
    }
    if t > 54.0 && t < 56.0 {
        gi.throttle = 1.0;
    }
    if st.once(56.0, "stunt_shot") {
        shot(&mut commands, "j04_stunt");
    }
    if st.once(63.0, "stunt_check") {
        let gain = p.money - st.mark;
        let done = stunts.done.first().copied().unwrap_or(false);
        st.check(done && gain > 500, format!("stunt jump: +${gain}"));
        gi.enter = true;
    }
    // ---------------- a ped fights back ----------------
    if st.once(64.0, "fight_walk") {
        pt.translation = crate::world::spawn_point();
    }
    if st.once(67.0, "fight_go") {
        p.health = 100.0;
        p.armor = 0.0;
        let me = pt.translation;
        if let Some((mut pd, mut ptr)) = peds
            .iter_mut()
            .filter(|(pd, _)| !pd.cop && !pd.gang && pd.health > 0.0)
            .min_by(|a, b| a.1.translation.distance(me).total_cmp(&b.1.translation.distance(me)))
        {
            println!("JOBTEST fight ped found at {:.1} m, player in car {}", ptr.translation.distance(me), p.vehicle.is_some());
            ptr.translation = me + Vec3::new(3.0, 0.2, 0.0);
            pd.armed = false;
            pd.state = crate::peds::PedState::Fight { t: 20.0 };
        }
    }
    if t > 67.1 && t < 72.0 && (t % 1.0) < dt {
        let me = pt.translation;
        for (pd, ptr) in &peds {
            if matches!(pd.state, crate::peds::PedState::Fight { .. }) {
                println!("JOBTEST fight dbg: in_car {} ped d {:.1} state {:?} knock {:.1} cd {:.1}", p.vehicle.is_some(), ptr.translation.distance(me), pd.state, pd.knock_t, pd.attack_cd);
            }
        }
    }
    if st.once(72.0, "fight_check") {
        let h = p.health;
        st.check(h < 100.0, format!("ped fought back: player health {h:.0}"));
    }
    if st.once(73.0, "quit") {
        println!("JOBTEST summary: {} passed, {} failed", st.pass.len(), st.fail.len());
        for f in &st.fail {
            println!("JOBTEST failed: {f}");
        }
        crate::lighting::request_quit(&mut quit);
    }
    over.0 = Some(gi);
}

#[derive(Resource, Default)]
struct GunTest {
    t: f32,
    done: Vec<&'static str>,
    mark: u32,
    pass: Vec<String>,
    fail: Vec<String>,
}

impl GunTest {
    fn once(&mut self, at: f32, id: &'static str) -> bool {
        if self.t >= at && !self.done.contains(&id) {
            self.done.push(id);
            true
        } else {
            false
        }
    }
    fn check(&mut self, ok: bool, what: String) {
        println!("GUNTEST {} {what}", if ok { "PASS" } else { "FAIL" });
        if ok {
            self.pass.push(what)
        } else {
            self.fail.push(what)
        }
    }
}

/// Movement + gunplay: films walk/jog/sprint/strafe from the side, drives the
/// weapon wheel, fires the new guns at targets and checks the results.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn guntest(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut st: ResMut<GunTest>,
    mut over: ResMut<InputOverride>,
    mut cam: ResMut<CameraOverride>,
    mut rig: ResMut<crate::camera::CameraRig>,
    mut ars: ResMut<crate::weapons::Arsenal>,
    marker: Res<crate::weapons::HitMarker>,
    mut player: Query<(&mut Player, &mut Transform), Without<crate::peds::Ped>>,
    mut peds: Query<(&mut crate::peds::Ped, &mut Transform), Without<Player>>,
    mut quit: ResMut<crate::lighting::QuitRequest>,
) {
    use crate::weapons::Weapon as W;
    let dt = time.delta_secs();
    st.t += dt;
    let t = st.t;
    let mut gi = GameInput::default();
    let Ok((mut p, mut pt)) = player.single_mut() else { return };
    // a straight park path, clear of street furniture
    let start = Vec3::new(crate::world::block_min(2) + 6.0, crate::world::CURB + 1.0, crate::world::block_min(3) + 40.0);
    let east = -std::f32::consts::FRAC_PI_2;
    let side_cam = |pos: Vec3| Transform::from_translation(pos + Vec3::new(-0.5, 0.9, 4.2)).looking_at(pos + Vec3::new(0.0, 0.0, 0.0), Vec3::Y);
    if st.once(5.0, "setup") {
        for i in 0..crate::weapons::WEAPON_COUNT {
            let a = crate::weapons::WEAPONS[i].stats().pickup_ammo * 4;
            ars.give(i, a);
        }
        ars.current = 1;
        pt.translation = start;
        p.vel = Vec3::ZERO;
        rig.yaw = east;
        rig.pitch = -0.1;
    }
    // ---- locomotion, filmed side-on ----
    for (from, to, id, shot_at, shot_id) in [(5.5, 7.5, "walk", 7.0, "g01_walk"), (7.6, 9.6, "jog", 9.1, "g02_jog"), (9.7, 11.7, "sprint", 11.2, "g03_sprint")] {
        if st.once(from, id) {
            pt.translation = start;
            p.vel = Vec3::ZERO;
            rig.yaw = east;
        }
        if t > from && t < to {
            gi.move_axis = Vec2::Y;
            gi.walk = id == "walk";
            gi.sprint = id == "sprint";
            cam.0 = Some(side_cam(pt.translation));
            if st.once(shot_at, shot_id) {
                let v = p.vel.with_y(0.0).length();
                println!("GUNTEST {id} speed {v:.1} m/s");
                shot(&mut commands, shot_id);
            }
        }
    }
    if st.once(11.8, "strafe_go") {
        pt.translation = start + Vec3::new(4.0, 0.0, 0.0);
        p.vel = Vec3::ZERO;
        rig.yaw = east;
    }
    if t > 11.8 && t < 13.5 {
        gi.aim = true;
        gi.move_axis = Vec2::new(-1.0, 0.0);
        cam.0 = None;
        if st.once(13.1, "g04_strafe") {
            let fwd = pt.rotation * Vec3::NEG_Z;
            let side = p.vel.with_y(0.0).normalize_or_zero().dot(fwd).abs();
            st.check(side < 0.3 && p.vel.length() > 2.0, format!("strafing while aiming: speed {:.1}, facing·move {side:.2}", p.vel.length()));
            shot(&mut commands, "g04_strafe");
        }
    }
    // ---- weapon wheel: push the cursor toward the assault rifle ----
    if t > 14.0 && t < 15.6 {
        gi.wheel = true;
        let n = crate::weapons::WHEEL_ORDER.len() as f32;
        let slot = crate::weapons::WHEEL_ORDER.iter().position(|w| *w == W::Rifle).unwrap() as f32;
        let a = slot / n * std::f32::consts::TAU;
        if t < 14.5 {
            gi.wheel_delta = Vec2::new(a.sin(), -a.cos()) * 12.0;
        }
        if st.once(15.0, "g05_wheel") {
            shot(&mut commands, "g05_wheel");
        }
    }
    if st.once(15.8, "wheel_check") {
        let w = ars.weapon();
        st.check(w == W::Rifle, format!("weapon wheel picked {:?}", w));
    }
    // ---- targets: grab the nearest pedestrian and stand them in front ----
    let place_target = |peds: &mut Query<(&mut crate::peds::Ped, &mut Transform), Without<Player>>, at: Vec3| {
        let me = at;
        if let Some((mut pd, mut tr)) = peds.iter_mut().filter(|(pd, _)| !pd.cop && pd.health > 0.0).min_by(|a, b| a.1.translation.distance(me).total_cmp(&b.1.translation.distance(me))) {
            tr.translation = at;
            pd.state = crate::peds::PedState::Idle { t: 1000.0 };
            pd.health = 100.0;
            pd.vel = Vec3::ZERO;
        }
    };
    let here = pt.translation;
    let ahead = |d: f32| here + Vec3::new(d, -0.05, 0.0);
    if st.once(16.0, "rifle_go") {
        pt.translation = start;
        p.vel = Vec3::ZERO;
        rig.yaw = east;
        rig.pitch = -0.08;
        cam.0 = None;
        st.mark = ars.ammo[W::Rifle.index()].0 + ars.ammo[W::Rifle.index()].1;
        place_target(&mut peds, start + Vec3::new(9.0, -0.05, 0.0));
    }
    if t > 16.6 && t < 18.0 {
        gi.aim = true;
        gi.fire = true;
        gi.fire_pressed = true;
        if st.once(17.0, "g06_rifle") {
            shot(&mut commands, "g06_rifle");
        }
    }
    if st.once(18.2, "rifle_check") {
        let left = ars.ammo[W::Rifle.index()].0 + ars.ammo[W::Rifle.index()].1;
        let fired = st.mark.saturating_sub(left);
        let hits = marker.hits;
        st.check(fired >= 10, format!("assault rifle full-auto: {fired} rounds"));
        st.check(hits > 0, format!("hit markers registered: {hits} (kills {})", marker.kills));
    }
    // ---- sniper scope ----
    if st.once(18.5, "sniper_go") {
        ars.current = W::Sniper.index();
        ars.cooldown = 0.0;
        ars.reloading = 0.0;
        place_target(&mut peds, ahead(25.0));
    }
    if t > 18.6 && t < 20.4 {
        gi.aim = true;
        if st.once(19.8, "g07_scope") {
            st.check(rig.fov < 20.0, format!("sniper zoom fov {:.0}", rig.fov));
            shot(&mut commands, "g07_scope");
        }
        if t > 20.0 {
            gi.fire_pressed = true;
            gi.fire = true;
        }
    }
    // ---- grenade ----
    if st.once(20.8, "grenade_go") {
        ars.current = W::Grenade.index();
        ars.cooldown = 0.0;
        ars.reloading = 0.0;
        st.mark = ars.ammo[W::Grenade.index()].0 + ars.ammo[W::Grenade.index()].1;
        rig.pitch = 0.05;
    }
    if t > 21.0 && t < 21.1 {
        gi.fire_pressed = true;
        gi.fire = true;
    }
    if st.once(23.9, "g08_grenade") {
        shot(&mut commands, "g08_grenade");
        let left = ars.ammo[W::Grenade.index()].0 + ars.ammo[W::Grenade.index()].1;
        let m = st.mark;
        st.check(left + 1 == m, format!("grenade thrown ({m} -> {left})"));
    }
    // ---- minigun spin-up ----
    if st.once(24.5, "minigun_go") {
        ars.current = W::Minigun.index();
        ars.cooldown = 0.0;
        ars.reloading = 0.0;
        rig.pitch = -0.08;
        st.mark = ars.ammo[W::Minigun.index()].0;
    }
    if t > 24.6 && t < 27.0 {
        gi.aim = true;
        gi.fire = true;
        if st.once(24.9, "spin_check") {
            let fired = st.mark.saturating_sub(ars.ammo[W::Minigun.index()].0);
            st.check(fired == 0, format!("minigun silent while spinning up ({fired} fired after 0.3 s)"));
        }
        if st.once(26.2, "g09_minigun") {
            shot(&mut commands, "g09_minigun");
        }
    }
    if st.once(27.1, "minigun_check") {
        let fired = st.mark.saturating_sub(ars.ammo[W::Minigun.index()].0);
        st.check(fired > 30, format!("minigun fired {fired} rounds in ~1.7 s after spin-up"));
    }
    // ---- bat ----
    if st.once(27.5, "bat_go") {
        ars.current = W::Bat.index();
        ars.cooldown = 0.0;
        st.mark = marker.hits;
        place_target(&mut peds, ahead(1.4));
    }
    if t > 28.0 && t < 28.1 {
        gi.fire_pressed = true;
        gi.fire = true;
    }
    if st.once(28.15, "g10_bat") {
        shot(&mut commands, "g10_bat");
    }
    if st.once(28.6, "bat_check") {
        let hits = marker.hits;
        let m = st.mark;
        st.check(hits > m, format!("bat connected (hits {m} -> {hits})"));
    }
    // ---- shotgun kill ----
    if st.once(29.0, "shotgun_go") {
        ars.current = W::Shotgun.index();
        ars.cooldown = 0.0;
        ars.reloading = 0.0;
        st.mark = marker.kills;
        place_target(&mut peds, ahead(4.0));
    }
    if t > 29.5 && t < 29.6 {
        gi.aim = true;
        gi.fire_pressed = true;
        gi.fire = true;
    }
    if t > 29.4 && t < 30.6 {
        gi.aim = true;
    }
    if st.once(29.75, "g11_shotgun") {
        shot(&mut commands, "g11_shotgun");
    }
    if st.once(31.0, "shotgun_check") {
        let kills = marker.kills;
        let m = st.mark;
        st.check(kills > m, format!("shotgun kill at 4 m (kills {m} -> {kills})"));
    }
    if st.once(32.0, "quit") {
        println!("GUNTEST summary: {} passed, {} failed", st.pass.len(), st.fail.len());
        for f in &st.fail {
            println!("GUNTEST failed: {f}");
        }
        crate::lighting::request_quit(&mut quit);
    }
    over.0 = Some(gi);
}
