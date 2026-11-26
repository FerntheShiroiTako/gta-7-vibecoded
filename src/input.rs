//! Device input -> `GameInput`, through a remappable binding table loaded from
//! `keybinds.cfg` (written with defaults on first run). Gameplay reads only
//! `GameInput`, so the autotest drives the game through exactly the same path.

use std::collections::HashMap;

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Bindings::load())
            .init_resource::<GameInput>()
            .init_resource::<InputOverride>()
            .init_resource::<UiCursor>()
            .add_systems(PreUpdate, gather_input.in_set(GameInputSet).after(InputSystems))
            .add_systems(Update, grab_cursor);
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct GameInputSet;

/// Every bindable action.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Action {
    Forward,
    Back,
    Left,
    Right,
    Sprint,
    Jump,
    Crouch,
    Enter,
    Fire,
    Aim,
    Reload,
    Weapon1,
    Weapon2,
    Weapon3,
    Weapon4,
    Weapon5,
    NextWeapon,
    PrevWeapon,
    Handbrake,
    Horn,
    LookBehind,
    CameraMode,
    Map,
    Pause,
    SpawnMenu,
    Fps,
    Lighting,
    YawLeft,
    YawRight,
    ThrottleUp,
    ThrottleDown,
    Job,
    WeaponWheel,
    Walk,
}

impl Action {
    pub const ALL: [Action; 34] = [
        Action::Forward,
        Action::Back,
        Action::Left,
        Action::Right,
        Action::Sprint,
        Action::Jump,
        Action::Crouch,
        Action::Enter,
        Action::Fire,
        Action::Aim,
        Action::Reload,
        Action::Weapon1,
        Action::Weapon2,
        Action::Weapon3,
        Action::Weapon4,
        Action::Weapon5,
        Action::NextWeapon,
        Action::PrevWeapon,
        Action::Handbrake,
        Action::Horn,
        Action::LookBehind,
        Action::CameraMode,
        Action::Map,
        Action::Pause,
        Action::SpawnMenu,
        Action::Fps,
        Action::Lighting,
        Action::YawLeft,
        Action::YawRight,
        Action::ThrottleUp,
        Action::ThrottleDown,
        Action::Job,
        Action::WeaponWheel,
        Action::Walk,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Action::Forward => "forward",
            Action::Back => "back",
            Action::Left => "left",
            Action::Right => "right",
            Action::Sprint => "sprint",
            Action::Jump => "jump",
            Action::Crouch => "crouch",
            Action::Enter => "enter_vehicle",
            Action::Fire => "fire",
            Action::Aim => "aim",
            Action::Reload => "reload",
            Action::Weapon1 => "weapon_fists",
            Action::Weapon2 => "weapon_pistol",
            Action::Weapon3 => "weapon_smg",
            Action::Weapon4 => "weapon_shotgun",
            Action::Weapon5 => "weapon_rocket",
            Action::NextWeapon => "next_weapon",
            Action::PrevWeapon => "prev_weapon",
            Action::Handbrake => "handbrake",
            Action::Horn => "horn",
            Action::LookBehind => "look_behind",
            Action::CameraMode => "camera_mode",
            Action::Map => "map",
            Action::Pause => "pause",
            Action::SpawnMenu => "spawn_menu",
            Action::Fps => "fps_counter",
            Action::Lighting => "toggle_raytracing",
            Action::YawLeft => "aircraft_yaw_left",
            Action::YawRight => "aircraft_yaw_right",
            Action::ThrottleUp => "aircraft_throttle_up",
            Action::ThrottleDown => "aircraft_throttle_down",
            Action::Job => "taxi_job",
            Action::WeaponWheel => "weapon_wheel",
            Action::Walk => "walk",
        }
    }
    fn defaults(self) -> Vec<Key> {
        use KeyCode as K;
        let k = |c: KeyCode| Key::Kb(c);
        match self {
            Action::Forward => vec![k(K::KeyW), k(K::ArrowUp)],
            Action::Back => vec![k(K::KeyS), k(K::ArrowDown)],
            Action::Left => vec![k(K::KeyA), k(K::ArrowLeft)],
            Action::Right => vec![k(K::KeyD), k(K::ArrowRight)],
            Action::Sprint => vec![k(K::ShiftLeft)],
            Action::Jump => vec![k(K::Space)],
            Action::Crouch => vec![k(K::ControlLeft)],
            Action::Enter => vec![k(K::KeyF), k(K::Enter)],
            Action::Fire => vec![Key::Mouse(MouseButton::Left)],
            Action::Aim => vec![Key::Mouse(MouseButton::Right)],
            Action::Reload => vec![k(K::KeyR)],
            Action::Weapon1 => vec![k(K::Digit1)],
            Action::Weapon2 => vec![k(K::Digit2)],
            Action::Weapon3 => vec![k(K::Digit3)],
            Action::Weapon4 => vec![k(K::Digit4)],
            Action::Weapon5 => vec![k(K::Digit5)],
            Action::NextWeapon => vec![k(K::KeyE)],
            Action::PrevWeapon => vec![k(K::KeyQ)],
            Action::Handbrake => vec![k(K::Space)],
            Action::Horn => vec![k(K::KeyH)],
            Action::LookBehind => vec![k(K::KeyC)],
            Action::CameraMode => vec![k(K::KeyV)],
            Action::Map => vec![k(K::KeyM)],
            Action::Pause => vec![k(K::Escape), k(K::KeyP)],
            Action::SpawnMenu => vec![k(K::F1)],
            Action::Fps => vec![k(K::F3)],
            Action::Lighting => vec![k(K::F4)],
            Action::YawLeft => vec![k(K::KeyQ)],
            Action::YawRight => vec![k(K::KeyE)],
            Action::ThrottleUp => vec![k(K::ShiftLeft)],
            Action::ThrottleDown => vec![k(K::ControlLeft)],
            Action::Job => vec![k(K::KeyJ)],
            Action::WeaponWheel => vec![k(K::Tab)],
            Action::Walk => vec![k(K::AltLeft)],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    Kb(KeyCode),
    Mouse(MouseButton),
}

const KEY_NAMES: &[(&str, KeyCode)] = &[
    ("A", KeyCode::KeyA), ("B", KeyCode::KeyB), ("C", KeyCode::KeyC), ("D", KeyCode::KeyD), ("E", KeyCode::KeyE),
    ("F", KeyCode::KeyF), ("G", KeyCode::KeyG), ("H", KeyCode::KeyH), ("I", KeyCode::KeyI), ("J", KeyCode::KeyJ),
    ("K", KeyCode::KeyK), ("L", KeyCode::KeyL), ("M", KeyCode::KeyM), ("N", KeyCode::KeyN), ("O", KeyCode::KeyO),
    ("P", KeyCode::KeyP), ("Q", KeyCode::KeyQ), ("R", KeyCode::KeyR), ("S", KeyCode::KeyS), ("T", KeyCode::KeyT),
    ("U", KeyCode::KeyU), ("V", KeyCode::KeyV), ("W", KeyCode::KeyW), ("X", KeyCode::KeyX), ("Y", KeyCode::KeyY),
    ("Z", KeyCode::KeyZ), ("0", KeyCode::Digit0), ("1", KeyCode::Digit1), ("2", KeyCode::Digit2), ("3", KeyCode::Digit3),
    ("4", KeyCode::Digit4), ("5", KeyCode::Digit5), ("6", KeyCode::Digit6), ("7", KeyCode::Digit7), ("8", KeyCode::Digit8),
    ("9", KeyCode::Digit9), ("F1", KeyCode::F1), ("F2", KeyCode::F2), ("F3", KeyCode::F3), ("F4", KeyCode::F4),
    ("F5", KeyCode::F5), ("F6", KeyCode::F6), ("F7", KeyCode::F7), ("F8", KeyCode::F8), ("F9", KeyCode::F9),
    ("F10", KeyCode::F10), ("F11", KeyCode::F11), ("F12", KeyCode::F12), ("Space", KeyCode::Space),
    ("LShift", KeyCode::ShiftLeft), ("RShift", KeyCode::ShiftRight), ("LCtrl", KeyCode::ControlLeft),
    ("RCtrl", KeyCode::ControlRight), ("LAlt", KeyCode::AltLeft), ("RAlt", KeyCode::AltRight), ("Tab", KeyCode::Tab),
    ("Escape", KeyCode::Escape), ("Enter", KeyCode::Enter), ("Backspace", KeyCode::Backspace),
    ("Up", KeyCode::ArrowUp), ("Down", KeyCode::ArrowDown), ("Left", KeyCode::ArrowLeft), ("Right", KeyCode::ArrowRight),
    ("Backquote", KeyCode::Backquote), ("Minus", KeyCode::Minus), ("Equal", KeyCode::Equal),
];

impl Key {
    pub fn name(self) -> String {
        match self {
            Key::Mouse(MouseButton::Left) => "MouseLeft".into(),
            Key::Mouse(MouseButton::Right) => "MouseRight".into(),
            Key::Mouse(MouseButton::Middle) => "MouseMiddle".into(),
            Key::Mouse(_) => "MouseOther".into(),
            Key::Kb(k) => KEY_NAMES.iter().find(|(_, c)| *c == k).map(|(n, _)| n.to_string()).unwrap_or_else(|| format!("{k:?}")),
        }
    }
    fn parse(s: &str) -> Option<Key> {
        match s {
            "MouseLeft" => return Some(Key::Mouse(MouseButton::Left)),
            "MouseRight" => return Some(Key::Mouse(MouseButton::Right)),
            "MouseMiddle" => return Some(Key::Mouse(MouseButton::Middle)),
            _ => {}
        }
        KEY_NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(s)).map(|(_, c)| Key::Kb(*c))
    }
}

pub const KEYBINDS_FILE: &str = "keybinds.cfg";

#[derive(Resource, Clone)]
pub struct Bindings(pub HashMap<Action, Vec<Key>>);

impl Bindings {
    pub fn defaults() -> Self {
        Bindings(Action::ALL.iter().map(|a| (*a, a.defaults())).collect())
    }
    /// Load `keybinds.cfg`; missing actions keep their defaults. Writes the file
    /// with defaults if it does not exist.
    pub fn load() -> Self {
        let mut b = Bindings::defaults();
        match std::fs::read_to_string(KEYBINDS_FILE) {
            Ok(text) => {
                for line in text.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    let Some((name, keys)) = line.split_once('=') else { continue };
                    let Some(action) = Action::ALL.iter().find(|a| a.name() == name.trim()) else {
                        warn!("keybinds.cfg: unknown action '{}'", name.trim());
                        continue;
                    };
                    let parsed: Vec<Key> = keys.split(',').filter_map(|k| Key::parse(k.trim())).collect();
                    if parsed.is_empty() {
                        warn!("keybinds.cfg: no valid keys for '{}'", name.trim());
                    } else {
                        b.0.insert(*action, parsed);
                    }
                }
            }
            Err(_) => {
                let _ = std::fs::write(KEYBINDS_FILE, b.to_text());
            }
        }
        b
    }
    pub fn to_text(&self) -> String {
        let mut s = String::from("# Harbor Heat key bindings: action = key[, key]\n# Keys: A-Z, 0-9, F1-F12, Space, LShift, LCtrl, LAlt, Tab, Escape, Enter, Up/Down/Left/Right, MouseLeft, MouseRight, MouseMiddle\n");
        for a in Action::ALL {
            let keys: Vec<String> = self.0[&a].iter().map(|k| k.name()).collect();
            s.push_str(&format!("{} = {}\n", a.name(), keys.join(", ")));
        }
        s
    }
    pub fn label(&self, a: Action) -> String {
        self.0.get(&a).and_then(|k| k.first()).map(|k| k.name()).unwrap_or_default()
    }
}

#[derive(Resource, Default, Clone, Debug)]
pub struct GameInput {
    /// x = strafe right, y = forward
    pub move_axis: Vec2,
    pub sprint: bool,
    pub crouch: bool,
    /// latched until consumed
    pub jump: bool,
    /// latched until consumed
    pub enter: bool,
    pub look: Vec2,
    pub throttle: f32,
    pub brake: f32,
    pub steer: f32,
    pub handbrake: bool,
    pub horn: bool,
    pub fire: bool,
    pub fire_pressed: bool,
    pub aim: bool,
    pub reload: bool,
    pub weapon_slot: Option<usize>,
    pub weapon_cycle: i32,
    pub look_behind: bool,
    pub camera_mode: bool,
    pub map: bool,
    pub pause: bool,
    pub spawn_menu: bool,
    pub toggle_fps: bool,
    pub toggle_lighting: bool,
    pub yaw: f32,
    pub throttle_axis: f32,
    pub scroll: f32,
    /// start / stop a taxi job (latched until consumed)
    pub job: bool,
    /// weapon wheel held open; mouse motion goes to `wheel_delta` instead of the camera
    pub wheel: bool,
    pub wheel_delta: Vec2,
    /// walk instead of jog
    pub walk: bool,
}

/// When `Some`, replaces device input entirely (used by the autotest).
#[derive(Resource, Default)]
pub struct InputOverride(pub Option<GameInput>);

/// While a menu/map is open the cursor is released and gameplay input is muted.
#[derive(Resource, Default)]
pub struct UiCursor(pub bool);

#[allow(clippy::too_many_arguments)]
fn gather_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    cursor: Query<&CursorOptions>,
    over: Res<InputOverride>,
    bindings: Res<Bindings>,
    ui: Res<UiCursor>,
    mut gi: ResMut<GameInput>,
) {
    if let Some(o) = &over.0 {
        let jump = gi.jump || o.jump;
        let enter = gi.enter || o.enter;
        *gi = o.clone();
        gi.jump = jump;
        gi.enter = enter;
        return;
    }
    let held = |a: Action| {
        bindings.0.get(&a).is_some_and(|ks| {
            ks.iter().any(|k| match k {
                Key::Kb(c) => keys.pressed(*c),
                Key::Mouse(m) => mouse.pressed(*m),
            })
        })
    };
    let pressed = |a: Action| {
        bindings.0.get(&a).is_some_and(|ks| {
            ks.iter().any(|k| match k {
                Key::Kb(c) => keys.just_pressed(*c),
                Key::Mouse(m) => mouse.just_pressed(*m),
            })
        })
    };
    // menu toggles always work
    gi.map = pressed(Action::Map);
    gi.pause = pressed(Action::Pause);
    gi.spawn_menu = pressed(Action::SpawnMenu);
    gi.toggle_fps = pressed(Action::Fps);
    gi.toggle_lighting = pressed(Action::Lighting);
    gi.scroll = scroll.delta.y;
    if ui.0 {
        let (map, pause, spawn, fps, light, scr) = (gi.map, gi.pause, gi.spawn_menu, gi.toggle_fps, gi.toggle_lighting, gi.scroll);
        *gi = GameInput { map, pause, spawn_menu: spawn, toggle_fps: fps, toggle_lighting: light, scroll: scr, ..default() };
        return;
    }
    let mut m = Vec2::ZERO;
    if held(Action::Forward) {
        m.y += 1.0;
    }
    if held(Action::Back) {
        m.y -= 1.0;
    }
    if held(Action::Right) {
        m.x += 1.0;
    }
    if held(Action::Left) {
        m.x -= 1.0;
    }
    gi.move_axis = m.clamp_length_max(1.0);
    gi.sprint = held(Action::Sprint);
    gi.crouch = held(Action::Crouch);
    if pressed(Action::Jump) {
        gi.jump = true;
    }
    if pressed(Action::Enter) {
        gi.enter = true;
    }
    gi.throttle = if m.y > 0.0 { 1.0 } else { 0.0 };
    gi.brake = if m.y < 0.0 { 1.0 } else { 0.0 };
    gi.steer = m.x;
    gi.handbrake = held(Action::Handbrake);
    gi.horn = held(Action::Horn);
    gi.fire = held(Action::Fire);
    gi.fire_pressed = pressed(Action::Fire);
    gi.aim = held(Action::Aim);
    gi.reload = pressed(Action::Reload);
    gi.weapon_slot = [Action::Weapon1, Action::Weapon2, Action::Weapon3, Action::Weapon4, Action::Weapon5].iter().position(|a| pressed(*a));
    gi.weapon_cycle = if pressed(Action::NextWeapon) || scroll.delta.y < -0.1 {
        1
    } else if pressed(Action::PrevWeapon) || scroll.delta.y > 0.1 {
        -1
    } else {
        0
    };
    if pressed(Action::Job) {
        gi.job = true;
    }
    gi.look_behind = held(Action::LookBehind);
    gi.camera_mode = pressed(Action::CameraMode);
    gi.yaw = (if held(Action::YawRight) { 1.0 } else { 0.0 }) - (if held(Action::YawLeft) { 1.0 } else { 0.0 });
    gi.throttle_axis = (if held(Action::ThrottleUp) { 1.0 } else { 0.0 }) - (if held(Action::ThrottleDown) { 1.0 } else { 0.0 });
    gi.walk = held(Action::Walk);
    let grabbed = cursor.iter().any(|c| c.grab_mode != CursorGrabMode::None);
    let motion = if grabbed { mouse_motion.delta } else { Vec2::ZERO };
    gi.wheel = held(Action::WeaponWheel);
    if gi.wheel {
        // the wheel owns the mouse: no camera turning, no shooting
        gi.wheel_delta = motion;
        gi.look = Vec2::ZERO;
        gi.fire = false;
        gi.fire_pressed = false;
        gi.aim = false;
        gi.weapon_cycle = 0;
    } else {
        gi.wheel_delta = Vec2::ZERO;
        gi.look = motion;
    }
}

fn grab_cursor(
    mut cursor: Query<&mut CursorOptions>,
    mouse: Res<ButtonInput<MouseButton>>,
    over: Res<InputOverride>,
    ui: Res<UiCursor>,
    mut first: Local<bool>,
) {
    let Ok(mut c) = cursor.single_mut() else { return };
    if ui.0 || over.0.is_some() {
        if c.grab_mode != CursorGrabMode::None {
            c.visible = true;
            c.grab_mode = CursorGrabMode::None;
        }
        return;
    }
    if !*first || mouse.just_pressed(MouseButton::Left) {
        *first = true;
        c.visible = false;
        c.grab_mode = CursorGrabMode::Locked;
    }
}
