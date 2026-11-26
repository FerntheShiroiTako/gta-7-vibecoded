//! Procedural audio: every sound is synthesised at startup into WAV buffers.
//! Engine (rpm/gear pitch), tyre screech, sirens, guns, explosions, crashes,
//! footsteps, pickups, and three radio stations of generated music.

use std::f32::consts::TAU;
use std::sync::Arc;

use bevy::audio::{AudioSink, AudioSinkPlayback, PlaybackMode, Volume};
use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::camera::MainCamera;
use crate::input::GameInput;
use crate::player::Player;
use crate::seats::{Seat, SeatKind};
use crate::vehicle::{Driver, Explosion, Vehicle, VehicleControl};
use crate::wanted::{PoliceCar, Wanted};

pub struct AudioFxPlugin;

impl Plugin for AudioFxPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Sfx>()
            .init_resource::<Radio>()
            .add_systems(Startup, setup_audio)
            .add_systems(Update, (play_sfx, explosion_sounds, vehicle_loops, siren_loop, radio, footsteps));
    }
}

const RATE: u32 = 44100;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SfxKind {
    Pistol,
    Smg,
    Shotgun,
    Rocket,
    Explosion,
    Crash,
    Horn,
    Step,
    Pickup,
    Punch,
    Ricochet,
    Reward,
}

/// Fire-and-forget positional sound.
#[derive(Message, Clone, Copy)]
pub struct Sfx {
    pub kind: SfxKind,
    pub pos: Vec3,
    pub volume: f32,
}

#[derive(Resource)]
pub struct Sounds {
    one_shots: Vec<(SfxKind, Handle<AudioSource>)>,
    engine: Handle<AudioSource>,
    screech: Handle<AudioSource>,
    siren: Handle<AudioSource>,
    stations: Vec<(&'static str, Handle<AudioSource>)>,
}

#[derive(Resource, Default)]
pub struct Radio {
    pub station: usize,
    playing: Option<(Entity, usize)>,
}

#[derive(Component)]
struct EngineLoop;
#[derive(Component)]
struct ScreechLoop;
#[derive(Component)]
struct SirenLoop;
#[derive(Component)]
struct RadioSink;

// ---------------------------------------------------------------------------------------------
// synthesis helpers
// ---------------------------------------------------------------------------------------------

fn wav(samples: &[f32]) -> AudioSource {
    let mut b = Vec::with_capacity(44 + samples.len() * 2);
    let data_len = (samples.len() * 2) as u32;
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&RATE.to_le_bytes());
    b.extend_from_slice(&(RATE * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32000.0) as i16).to_le_bytes());
    }
    AudioSource { bytes: Arc::from(b.into_boxed_slice()) }
}

struct Noise(u32);
impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

fn len(sec: f32) -> usize {
    (sec * RATE as f32) as usize
}

/// One-pole low-pass in place.
fn lowpass(s: &mut [f32], cutoff: f32) {
    let a = 1.0 - (-TAU * cutoff / RATE as f32).exp();
    let mut y = 0.0;
    for v in s.iter_mut() {
        y += a * (*v - y);
        *v = y;
    }
}

fn normalize(s: &mut [f32], peak: f32) {
    let m = s.iter().fold(0.0f32, |a, v| a.max(v.abs())).max(1e-6);
    for v in s.iter_mut() {
        *v *= peak / m;
    }
}

fn gunshot(sec: f32, body_hz: f32, cutoff: f32, seed: u32) -> Vec<f32> {
    let n = len(sec);
    let mut r = Noise(seed);
    let mut s: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let env = (-t * 28.0 / sec.max(0.05)).exp();
            let crack = r.next() * env;
            let thump = (TAU * body_hz * t * (1.0 - t * 2.0).max(0.3)).sin() * (-t * 18.0).exp() * 0.9;
            crack + thump
        })
        .collect();
    lowpass(&mut s, cutoff);
    normalize(&mut s, 0.95);
    s
}

fn explosion() -> Vec<f32> {
    let n = len(2.6);
    let mut r = Noise(99);
    let mut s: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let env = if t < 0.02 { t / 0.02 } else { (-(t - 0.02) * 1.6).exp() };
            r.next() * env + (TAU * 38.0 * t).sin() * (-t * 3.0).exp() * 0.6
        })
        .collect();
    lowpass(&mut s, 700.0);
    lowpass(&mut s, 900.0);
    normalize(&mut s, 0.95);
    s
}

/// Seamless engine loop at 50 Hz firing frequency; pitch is set via playback speed.
fn engine() -> Vec<f32> {
    let n = RATE as usize; // 1 s, 50 cycles -> seamless
    let mut r = Noise(7);
    let mut s: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let p = TAU * 50.0 * t;
            let pulse = p.sin() * 0.5 + (2.0 * p).sin() * 0.35 + (3.0 * p).sin() * 0.2 + (0.5 * p).sin() * 0.3;
            pulse + r.next() * 0.08
        })
        .collect();
    lowpass(&mut s, 1400.0);
    normalize(&mut s, 0.8);
    s
}

fn screech() -> Vec<f32> {
    let n = RATE as usize;
    let mut r = Noise(31);
    let mut s: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let wob = 1.0 + 0.05 * (TAU * 6.0 * t).sin();
            (TAU * 1300.0 * wob * t).sin() * 0.35 + (TAU * 2100.0 * t).sin() * 0.15 + r.next() * 0.3
        })
        .collect();
    normalize(&mut s, 0.6);
    s
}

fn siren() -> Vec<f32> {
    // 2 s wail, phase-continuous so it loops cleanly
    let n = len(2.0);
    let mut phase = 0.0f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let f = 760.0 + 420.0 * (0.5 - 0.5 * (TAU * t / 2.0).cos());
            phase += TAU * f / RATE as f32;
            (phase.sin() * 0.6 + (2.0 * phase).sin() * 0.25) * 0.7
        })
        .collect()
}

fn tone_burst(freqs: &[(f32, f32, f32)], sec: f32, square: bool) -> Vec<f32> {
    let n = len(sec);
    let mut s: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let mut v = 0.0;
            for &(f, start, dur) in freqs {
                if t >= start && t < start + dur {
                    let lt = t - start;
                    let env = (lt / 0.01).min(1.0) * ((start + dur - t) / 0.03).min(1.0);
                    let x = (TAU * f * lt).sin();
                    v += if square { x.signum() * 0.5 } else { x } * env;
                }
            }
            v
        })
        .collect();
    if square {
        lowpass(&mut s, 2500.0);
    }
    normalize(&mut s, 0.8);
    s
}

fn thud(sec: f32, hz: f32, seed: u32) -> Vec<f32> {
    let n = len(sec);
    let mut r = Noise(seed);
    let mut s: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            ((TAU * hz * t).sin() * 0.8 + r.next() * 0.5) * (-t * 30.0).exp()
        })
        .collect();
    lowpass(&mut s, 1200.0);
    normalize(&mut s, 0.9);
    s
}

// ---------------------------------------------------------------------------------------------
// music: three original procedurally composed loops
// ---------------------------------------------------------------------------------------------

fn note(midi: f32) -> f32 {
    440.0 * 2f32.powf((midi - 69.0) / 12.0)
}

fn music(style: u32) -> Vec<f32> {
    let (bpm, bars, prog): (f32, usize, [i32; 4]) = match style {
        0 => (104.0, 8, [0, 8, 3, 10]),   // synthwave: i - VI - III - VII (A minor)
        1 => (88.0, 8, [0, 5, 3, 7]),     // boom-bap: i - iv - III - v
        _ => (132.0, 8, [0, 3, 5, 3]),    // rock: power chords
    };
    let beat = 60.0 / bpm;
    let total = beat * 4.0 * bars as f32;
    let n = len(total);
    let root = 57.0; // A3
    let mut r = Noise(1234 + style);
    let mut out = vec![0.0f32; n];
    for (i, o) in out.iter_mut().enumerate() {
        let t = i as f32 / RATE as f32;
        let b = t / beat; // beat position
        let bar = (b / 4.0) as usize;
        let chord = prog[bar % 4] as f32;
        let in_beat = b.fract();
        let in_8th = (b * 2.0).fract();
        let step16 = ((b * 4.0) as usize) % 16;
        let mut v = 0.0;
        // drums
        let kick = |x: f32| (TAU * (50.0 + 90.0 * (-x * 30.0).exp()) * x).sin() * (-x * 9.0).exp();
        let kick_on = match style {
            1 => matches!(step16, 0 | 7 | 10),
            _ => step16 % 4 == 0,
        };
        if kick_on {
            v += kick((b * 4.0).fract() * beat / 4.0) * 0.9;
        }
        if (b as usize) % 2 == 1 && in_beat < 0.5 {
            v += r.next() * (-in_beat * beat * 22.0).exp() * if style == 2 { 0.55 } else { 0.4 };
        }
        v += r.next() * (-in_8th * beat * 0.5 * 90.0).exp() * 0.08; // hats
        // bass
        let bass_f = note(root - 12.0 + chord);
        let bass = match style {
            0 => ((TAU * bass_f * t).sin() + 0.4 * (TAU * 2.0 * bass_f * t).sin()) * (1.0 - in_8th * 0.6),
            1 => (TAU * bass_f * t).sin() * (-in_beat * 2.0).exp() * 1.3,
            _ => ((TAU * bass_f * t).sin()).signum() * 0.5,
        };
        v += bass * 0.35;
        // harmony
        let tri = [0.0, 3.0, 7.0];
        match style {
            0 => {
                // detuned pad + arpeggio
                for k in tri {
                    let f = note(root + chord + k);
                    v += ((TAU * f * t).sin() + (TAU * f * 1.006 * t).sin()) * 0.07;
                }
                let arp = [0.0, 7.0, 12.0, 15.0][step16 % 4];
                let f = note(root + 12.0 + chord + arp);
                v += (TAU * f * t).sin().signum() * 0.06 * (-(b * 4.0).fract() * 6.0).exp();
            }
            1 => {
                // dusty electric piano stabs on 2 and 4-and
                if matches!(step16, 4 | 14) || (step16 == 12 && bar % 2 == 1) {
                    let lt = (b * 4.0).fract() * beat / 4.0;
                    for k in [0.0, 3.0, 7.0, 10.0] {
                        let f = note(root + 12.0 + chord + k);
                        v += (TAU * f * lt).sin() * (-lt * 5.0).exp() * 0.12;
                    }
                }
            }
            _ => {
                // overdriven power chord
                let mut g = 0.0;
                for k in [0.0, 7.0, 12.0] {
                    g += (TAU * note(root + chord + k - 12.0) * t).sin();
                }
                v += (g * 3.0).tanh() * 0.16 * (1.0 - in_8th * 0.5);
            }
        }
        *o = v;
    }
    lowpass(&mut out, if style == 1 { 6000.0 } else { 9000.0 });
    normalize(&mut out, 0.7);
    out
}

pub const STATIONS: [&str; 4] = ["Neon Tide 104.4 - synthwave", "Harbor Heat 88 - boom bap", "Solace Rock 132", "Radio off"];

fn setup_audio(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let mut add = |s: Vec<f32>| sources.add(wav(&s));
    let one_shots = vec![
        (SfxKind::Pistol, add(gunshot(0.35, 160.0, 5000.0, 11))),
        (SfxKind::Smg, add(gunshot(0.18, 200.0, 6500.0, 12))),
        (SfxKind::Shotgun, add(gunshot(0.6, 90.0, 3200.0, 13))),
        (SfxKind::Rocket, add(gunshot(1.0, 60.0, 1400.0, 14))),
        (SfxKind::Explosion, add(explosion())),
        (SfxKind::Crash, add(gunshot(0.5, 70.0, 2400.0, 15))),
        (SfxKind::Horn, add(tone_burst(&[(415.0, 0.0, 0.5), (523.0, 0.0, 0.5)], 0.5, true))),
        (SfxKind::Step, add(thud(0.09, 110.0, 16))),
        (SfxKind::Pickup, add(tone_burst(&[(880.0, 0.0, 0.08), (1320.0, 0.08, 0.12)], 0.22, false))),
        (SfxKind::Punch, add(thud(0.15, 80.0, 17))),
        (SfxKind::Ricochet, add(tone_burst(&[(2400.0, 0.0, 0.12), (1800.0, 0.04, 0.1)], 0.16, false))),
        (SfxKind::Reward, add(tone_burst(&[(523.0, 0.0, 0.15), (659.0, 0.15, 0.15), (784.0, 0.3, 0.15), (1047.0, 0.45, 0.4)], 0.9, false))),
    ];
    let stations = vec![(STATIONS[0], add(music(0))), (STATIONS[1], add(music(1))), (STATIONS[2], add(music(2)))];
    let sounds = Sounds { one_shots, engine: add(engine()), screech: add(screech()), siren: add(siren()), stations };
    // persistent loops, volume-controlled every frame
    let looped = |h: &Handle<AudioSource>| (AudioPlayer(h.clone()), PlaybackSettings { mode: PlaybackMode::Loop, volume: Volume::Linear(0.0), ..default() });
    commands.spawn((looped(&sounds.engine), EngineLoop));
    commands.spawn((looped(&sounds.screech), ScreechLoop));
    commands.spawn((looped(&sounds.siren), SirenLoop));
    commands.insert_resource(sounds);
}

fn listener(cam: &Query<&GlobalTransform, With<MainCamera>>) -> Vec3 {
    cam.single().map(|c| c.translation()).unwrap_or(Vec3::ZERO)
}

fn attenuate(d: f32, range: f32) -> f32 {
    let k = (1.0 - d / range).clamp(0.0, 1.0);
    k * k
}

fn play_sfx(mut commands: Commands, mut msgs: MessageReader<Sfx>, sounds: Option<Res<Sounds>>, cam: Query<&GlobalTransform, With<MainCamera>>) {
    let Some(sounds) = sounds else { return };
    let ear = listener(&cam);
    for m in msgs.read() {
        let range = match m.kind {
            SfxKind::Explosion => 500.0,
            SfxKind::Pistol | SfxKind::Smg | SfxKind::Shotgun | SfxKind::Rocket => 260.0,
            SfxKind::Step => 25.0,
            SfxKind::Pickup | SfxKind::Reward => 1.0e6,
            _ => 120.0,
        };
        let vol = m.volume * attenuate(m.pos.distance(ear), range);
        if vol < 0.01 {
            continue;
        }
        if let Some((_, h)) = sounds.one_shots.iter().find(|(k, _)| *k == m.kind) {
            commands.spawn((AudioPlayer(h.clone()), PlaybackSettings { mode: PlaybackMode::Despawn, volume: Volume::Linear(vol), speed: 0.95 + (m.pos.x.abs().fract()) * 0.1, ..default() }));
        }
    }
}

fn explosion_sounds(mut ex: MessageReader<Explosion>, mut out: MessageWriter<Sfx>) {
    for e in ex.read() {
        out.write(Sfx { kind: SfxKind::Explosion, pos: e.pos, volume: 1.0 });
    }
}

/// Engine pitch follows a simulated rpm through gears; tyre screech follows slip.
#[allow(clippy::type_complexity)]
fn vehicle_loops(
    time: Res<Time>,
    player: Query<&Player>,
    cars: Query<(&Velocity, &Transform, Option<&Vehicle>, Option<&VehicleControl>, Option<&crate::aircraft::Aircraft>, &Seat)>,
    mut engine: Query<&mut AudioSink, (With<EngineLoop>, Without<ScreechLoop>)>,
    mut screech: Query<&mut AudioSink, (With<ScreechLoop>, Without<EngineLoop>)>,
    mut rpm_s: Local<f32>,
) {
    let dt = time.delta_secs();
    let in_car = player.single().ok().and_then(|p| p.vehicle).and_then(|v| cars.get(v).ok());
    let (mut target_rpm, mut vol, mut slip) = (0.0, 0.0, 0.0);
    if let Some((vel, t, veh, ctl, air, seat)) = in_car {
        let speed = vel.linear.dot(t.rotation * Vec3::NEG_Z).abs();
        match seat.kind {
            SeatKind::Car => {
                let v = veh.unwrap();
                if !v.wrecked {
                    let thr = ctl.map(|c| c.throttle).unwrap_or(0.0);
                    // 5-speed box: rpm climbs through each gear, drops on upshift
                    let top = v.spec.top_speed;
                    let gears = [0.0, 0.16, 0.32, 0.52, 0.75, 1.05];
                    let frac = (speed / top).clamp(0.0, 1.05);
                    let g = gears.windows(2).position(|w| frac >= w[0] && frac < w[1]).unwrap_or(4);
                    let in_gear = (frac - gears[g]) / (gears[g + 1] - gears[g]);
                    target_rpm = 900.0 + in_gear * 5200.0 + thr * 700.0;
                    vol = 0.22 + thr * 0.25;
                    slip = v.wheels.iter().map(|w| if w.contact { w.slip } else { 0.0 }).fold(0.0, f32::max);
                }
            }
            SeatKind::Plane | SeatKind::Heli => {
                let a = air.unwrap();
                target_rpm = 1500.0 + a.rotor_speed * 70.0;
                vol = if a.dead { 0.0 } else { 0.18 + a.throttle * 0.3 };
            }
        }
    }
    *rpm_s += (target_rpm - *rpm_s) * (dt * 6.0).min(1.0);
    if let Ok(mut s) = engine.single_mut() {
        s.set_speed((*rpm_s / 3000.0).clamp(0.3, 2.6));
        s.set_volume(Volume::Linear(vol));
    }
    if let Ok(mut s) = screech.single_mut() {
        s.set_volume(Volume::Linear(((slip - 4.0) / 6.0).clamp(0.0, 0.45)));
    }
}

fn siren_loop(
    wanted: Res<Wanted>,
    police: Query<(&Transform, &Seat), With<PoliceCar>>,
    cam: Query<&GlobalTransform, With<MainCamera>>,
    mut sink: Query<&mut AudioSink, With<SirenLoop>>,
) {
    let ear = listener(&cam);
    let near = police.iter().filter(|(_, s)| matches!(s.driver, Driver::Cop(_))).map(|(t, _)| t.translation.distance(ear)).fold(f32::MAX, f32::min);
    let vol = if wanted.stars > 0 { attenuate(near, 220.0) * 0.5 } else { 0.0 };
    if let Ok(mut s) = sink.single_mut() {
        s.set_volume(Volume::Linear(vol));
    }
}

/// Car radio: a station starts when you get in; Q / E change station.
#[allow(clippy::too_many_arguments)]
fn radio(
    mut commands: Commands,
    input: Res<GameInput>,
    mut radio: ResMut<Radio>,
    sounds: Option<Res<Sounds>>,
    player: Query<&Player>,
    seats: Query<&Seat>,
    mut hud: ResMut<crate::hud::HudMessages>,
    mut horn_cd: Local<f32>,
    time: Res<Time>,
    mut sfx: MessageWriter<Sfx>,
    tf: Query<&Transform, With<Seat>>,
) {
    let Some(sounds) = sounds else { return };
    let in_car = player.single().ok().and_then(|p| p.vehicle).filter(|v| seats.get(*v).is_ok_and(|s| s.kind == SeatKind::Car));
    *horn_cd -= time.delta_secs();
    if let Some(v) = in_car {
        if input.weapon_cycle != 0 {
            radio.station = (radio.station as i32 + input.weapon_cycle).rem_euclid(STATIONS.len() as i32) as usize;
            hud.push(STATIONS[radio.station]);
        }
        if input.horn && *horn_cd <= 0.0 {
            *horn_cd = 0.45;
            if let Ok(t) = tf.get(v) {
                sfx.write(Sfx { kind: SfxKind::Horn, pos: t.translation, volume: 0.7 });
            }
        }
    }
    let want = in_car.map(|_| radio.station).filter(|s| *s < sounds.stations.len());
    if radio.playing.map(|p| p.1) != want {
        if let Some((e, _)) = radio.playing.take() {
            commands.entity(e).despawn();
        }
        if let Some(s) = want {
            let e = commands
                .spawn((AudioPlayer(sounds.stations[s].1.clone()), PlaybackSettings { mode: PlaybackMode::Loop, volume: Volume::Linear(0.35), ..default() }, RadioSink))
                .id();
            radio.playing = Some((e, s));
        }
    }
}

/// Footsteps on the walk-cycle beat.
fn footsteps(player: Query<(&Player, &Transform)>, rigs: Query<&crate::humanoid::Rig>, mut last: Local<f32>, mut sfx: MessageWriter<Sfx>) {
    let Ok((p, t)) = player.single() else { return };
    let Ok(r) = rigs.get(p.visual) else { return };
    if p.vehicle.is_some() || r.speed < 0.8 || r.airborne || r.ragdoll {
        return;
    }
    let step = (r.phase / std::f32::consts::PI).floor();
    if step != *last {
        *last = step;
        sfx.write(Sfx { kind: SfxKind::Step, pos: t.translation, volume: 0.25 + r.speed * 0.03 });
    }
}
