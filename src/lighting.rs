//! Lighting modes.
//!
//! * **Raster** (default): cascaded sun shadows, SSAO, screen-space reflections,
//!   atmosphere-driven environment lighting, TAA.
//! * **Ray traced** (F4, or `HH_RT=1` at start): Bevy Solari - ReSTIR direct
//!   lighting from the sun and emissive surfaces, raytraced shadows and GI.
//!   Without DLSS Ray Reconstruction (which needs NVIDIA's separately downloaded
//!   SDK) its output is noisy, so it is opt-in.

use bevy::camera::CameraMainTextureUsages;
use bevy::pbr::{ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel, ScreenSpaceReflections};
use bevy::light::{EnvironmentMapLight, GeneratedEnvironmentMapLight};
use bevy::anti_alias::dlss::{Dlss, DlssRayReconstructionFeature, DlssRayReconstructionSupported};
use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::prelude::*;
use bevy::render::render_resource::TextureUsages;
use bevy::render::renderer::RenderDevice;
use bevy::solari::prelude::{RaytracingMesh3d, SolariLighting, SolariPlugins};

use crate::camera::{MainCamera, Sun};

pub struct LightingPlugin;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LightingToggleSet;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(SolariPlugins)
            .init_resource::<Lighting>()
            .add_systems(Startup, (detect_rt, build_sky_cubemap))
            .init_resource::<QuitRequest>()
            .add_systems(Update, (log_dlss, toggle_mode, graceful_quit, attach_raytracing_meshes, apply_mode).chain().in_set(LightingToggleSet));
    }
}

/// Procedural daylight sky cubemap: Solari only reads a cubemap environment light
/// (not the dynamic atmosphere one), so without this shaded areas go black.
#[derive(Resource)]
pub struct SkyCubemap(pub Handle<Image>);

fn build_sky_cubemap(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension};
    // >= 256 px: the environment-map prefilter needs at least 8 mip levels
    let n = 256u32;
    let srgb = |c: f32| (c.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0 + 0.5) as u8;
    let mut data = Vec::with_capacity((n * n * 6 * 4) as usize);
    for face in 0..6 {
        for y in 0..n {
            for x in 0..n {
                let u = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                let v = (y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                let d = match face {
                    0 => Vec3::new(1.0, -v, -u),
                    1 => Vec3::new(-1.0, -v, u),
                    2 => Vec3::new(u, 1.0, v),
                    3 => Vec3::new(u, -1.0, -v),
                    4 => Vec3::new(u, -v, 1.0),
                    _ => Vec3::new(-u, -v, -1.0),
                }
                .normalize();
                let e = d.y;
                let (zenith, horizon, ground) = (Vec3::new(0.22, 0.42, 0.85), Vec3::new(0.72, 0.82, 0.95), Vec3::new(0.32, 0.31, 0.29));
                let c = if e >= 0.0 { horizon.lerp(zenith, e.powf(0.6)) } else { horizon.lerp(ground, (-e * 4.0).min(1.0)) };
                data.extend_from_slice(&[srgb(c.x), srgb(c.y), srgb(c.z), 255]);
            }
        }
    }
    let img = Image {
        texture_view_descriptor: Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() }),
        ..Image::new(Extent3d { width: n, height: n, depth_or_array_layers: 6 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD)
    };
    commands.insert_resource(SkyCubemap(images.add(img)));
}

#[derive(Resource, Default)]
pub struct Lighting {
    pub rt_supported: bool,
    pub rt: bool,
    applied: Option<bool>,
}

/// Set to quit: ray tracing / DLSS is switched off first and the app exits a few
/// frames later, once the GPU has drained (closing mid-DLSS crashes the surface).
#[derive(Resource, Default)]
pub struct QuitRequest(pub Option<u32>);

pub fn request_quit(q: &mut QuitRequest) {
    if q.0.is_none() {
        q.0 = Some(0);
    }
}

fn graceful_quit(
    mut q: ResMut<QuitRequest>,
    mut lighting: ResMut<Lighting>,
    mut close: MessageReader<bevy::window::WindowCloseRequested>,
    mut exit: MessageWriter<AppExit>,
) {
    if close.read().count() > 0 {
        request_quit(&mut q);
    }
    let Some(n) = q.0.as_mut() else { return };
    lighting.rt = false;
    *n += 1;
    if *n > 8 {
        exit.write(AppExit::Success);
    }
}

/// Meshes that must stay raster-only (alpha-blended particles, decals).
#[derive(Component)]
pub struct NoRaytrace;

fn detect_rt(device: Option<Res<RenderDevice>>, mut lighting: ResMut<Lighting>) {
    let supported = device.map(|d| d.features().contains(SolariPlugins::required_wgpu_features())).unwrap_or(false);
    lighting.rt_supported = supported;
    lighting.rt = supported && std::env::var("HH_RT").is_ok();
    info!("ray tracing supported: {supported}, enabled: {}", lighting.rt);
}

fn log_dlss(rr: Option<Res<DlssRayReconstructionSupported>>, mut done: Local<bool>) {
    if !*done {
        *done = true;
        info!("DLSS ray reconstruction supported: {}", rr.is_some());
    }
}

fn toggle_mode(input: Res<crate::input::GameInput>, mut lighting: ResMut<Lighting>, mut hud: ResMut<crate::hud::HudMessages>) {
    if input.toggle_lighting {
        if lighting.rt_supported {
            lighting.rt = !lighting.rt;
            hud.push(if lighting.rt { "Lighting: ray traced" } else { "Lighting: raster" });
        } else {
            hud.push("Ray tracing is not supported on this GPU");
        }
    }
}

/// Raytracing meshes only exist while Solari is on, so raster mode pays nothing
/// for acceleration-structure upkeep.
#[allow(clippy::type_complexity)]
fn attach_raytracing_meshes(
    mut commands: Commands,
    lighting: Res<Lighting>,
    mut was_on: Local<bool>,
    added: Query<(Entity, &Mesh3d), (Added<Mesh3d>, With<MeshMaterial3d<StandardMaterial>>, Without<NoRaytrace>, Without<RaytracingMesh3d>)>,
    all: Query<(Entity, &Mesh3d), (With<MeshMaterial3d<StandardMaterial>>, Without<NoRaytrace>, Without<RaytracingMesh3d>)>,
    tagged: Query<Entity, With<RaytracingMesh3d>>,
) {
    let on = lighting.rt_supported && lighting.rt;
    if on && !*was_on {
        for (e, m) in &all {
            commands.entity(e).insert(RaytracingMesh3d(m.0.clone()));
        }
    } else if on {
        for (e, m) in &added {
            commands.entity(e).insert(RaytracingMesh3d(m.0.clone()));
        }
    } else if *was_on {
        for e in &tagged {
            commands.entity(e).remove::<RaytracingMesh3d>();
        }
    }
    *was_on = on;
}

#[allow(clippy::too_many_arguments)]
fn apply_mode(
    mut commands: Commands,
    mut lighting: ResMut<Lighting>,
    cam: Query<Entity, With<MainCamera>>,
    mut sun: Query<&mut DirectionalLight, With<Sun>>,
    sky: Option<Res<SkyCubemap>>,
    dlss_rr: Option<Res<DlssRayReconstructionSupported>>,
    time: Res<Time<Real>>,
    has_env: Query<(), (With<MainCamera>, Or<(With<GeneratedEnvironmentMapLight>, With<EnvironmentMapLight>)>)>,
) {
    let Ok(cam) = cam.single() else { return };
    // One environment light for the whole session, never swapped: removing /
    // re-adding environment lights (or the atmosphere's own) mid-game breaks the
    // prefilter passes, and Solari can only read a plain cubemap light.
    if has_env.is_empty() {
        if let Some(sky) = &sky {
            commands.entity(cam).insert(GeneratedEnvironmentMapLight { environment_map: sky.0.clone(), intensity: 20000.0, ..default() });
        }
    }
    if lighting.applied == Some(lighting.rt) {
        return;
    }
    // Starting DLSS ray reconstruction while the window surface is still being
    // configured (first frames / initial resize) loses the device: wait a moment.
    if lighting.rt && time.elapsed_secs() < 2.5 {
        return;
    }
    if lighting.rt {
        commands
            .entity(cam)
            .remove::<(ScreenSpaceAmbientOcclusion, ScreenSpaceReflections)>()
            .insert((
                // ReSTIR resampling: much less noise for a few extra rays per pixel
                SolariLighting { restir: true, confidence_weight_cap: 20.0, primary_di_samples: 16, secondary_di_samples: 8, ..default() },
                CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING),
            ));
        // DLSS Ray Reconstruction is the denoiser Solari is built for; it replaces TAA
        if dlss_rr.is_some() {
            commands.entity(cam).remove::<TemporalAntiAliasing>().insert(Dlss::<DlssRayReconstructionFeature>::default());
        }
        for mut s in &mut sun {
            // Solari traces its own shadows
            s.shadow_maps_enabled = false;
        }
    } else {
        commands.entity(cam).remove::<(SolariLighting, Dlss<DlssRayReconstructionFeature>)>().insert((
            TemporalAntiAliasing::default(),
            ScreenSpaceAmbientOcclusion { quality_level: ScreenSpaceAmbientOcclusionQualityLevel::High, ..default() },
            ScreenSpaceReflections {
                // only genuinely glossy surfaces (glass, water); car paint and
                // concrete keep their environment-map reflections instead of SSR noise
                min_perceptual_roughness: 0.04..0.08,
                max_perceptual_roughness: 0.2..0.26,
                ..default()
            },
        ));
        for mut s in &mut sun {
            s.shadow_maps_enabled = !crate::camera::env_flag("HH_NO_SHADOW");
        }
    }
    lighting.applied = Some(lighting.rt);
}
