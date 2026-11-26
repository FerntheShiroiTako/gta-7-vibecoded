//! Harbor Heat — an original open-city crime sandbox set in Port Solace.

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::prelude::*;
use bevy::window::PresentMode;
use bevy_rapier3d::prelude::*;

mod activities;
mod aircraft;
mod audio;
mod autotest;
mod camera;
mod car_model;
mod city;
mod daynight;
mod fx;
mod hud;
mod humanoid;
mod input;
mod lighting;
mod map;
mod meshgen;
mod missions;
mod peds;
mod player;
mod ragdoll;
mod seats;
mod wanted;
mod weapons;
mod textures;
mod traffic;
mod vehicle;
mod world;

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(GlobalAmbientLight::NONE)
        .init_resource::<aircraft::SpawnRequest>()
        // project id for NVIDIA DLSS (Ray Reconstruction denoises the ray-traced mode)
        .insert_resource(bevy::anti_alias::dlss::DlssProjectId(bevy::asset::uuid::uuid!("cf0cf3e0-07f2-473a-a5c3-deca0c11f2d9")))
        // deferred shading: required for screen-space reflections, shared with Solari
        .insert_resource(bevy::pbr::DefaultOpaqueRendererMethod::deferred())
        .insert_resource(TimestepMode::Variable { max_dt: 1.0 / 30.0, time_scale: 1.0, substeps: 3 })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Harbor Heat".into(),
                resolution: (1600, 900).into(),
                present_mode: if std::env::var("HH_NOVSYNC").is_ok() { PresentMode::AutoNoVsync } else { PresentMode::AutoVsync },
                ..default()
            }),
            // closing goes through lighting::graceful_quit (DLSS must be shut down first)
            close_when_requested: false,
            ..default()
        }))
        .add_plugins((RapierPhysicsPlugin::<NoUserData>::default(), FrameTimeDiagnosticsPlugin::default()))
        .add_plugins((
            input::InputPlugin,
            city::CityPlugin,
            humanoid::HumanoidPlugin,
            player::PlayerPlugin,
            peds::PedPlugin,
            camera::CameraPlugin,
            lighting::LightingPlugin,
            vehicle::VehiclePlugin,
            seats::SeatPlugin,
            weapons::WeaponPlugin,
            wanted::WantedPlugin,
            ragdoll::RagdollPlugin,
        ))
        .add_plugins((
            traffic::TrafficPlugin,
            map::MapPlugin,
            aircraft::AircraftPlugin,
            audio::AudioFxPlugin,
            daynight::DayNightPlugin,
            fx::FxPlugin,
            hud::HudPlugin,
            activities::ActivitiesPlugin,
            missions::MissionPlugin,
            autotest::AutotestPlugin,
        ))
        .run();
}
