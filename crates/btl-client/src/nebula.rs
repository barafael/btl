//! Client-side nebula background rendering.
//!
//! Receives a `NebulaSeed` from the server, generates the procedural texture
//! locally, and renders it as:
//! - A large world-space sprite behind everything (slowly animated)
//! - A background image filling the minimap (with radial fade)

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use btl_shared::nebula::{self, NebulaPrograms};
use btl_shared::{MAP_RADIUS, NebulaSeed};

use crate::minimap::MinimapRoot;

/// Nebula texture resolution (stretched over the map — low res is fine)
const NEBULA_SIZE: u32 = 128;
/// Animation speed: full cycle ~ 9 minutes
const NEBULA_ANIM_SPEED: f32 = 0.4;
/// World-space sprite opacity (barely visible — space is still black)
const WORLD_ALPHA: f32 = 0.09;
/// Minimap background opacity
const MINIMAP_ALPHA: f32 = 0.35;
/// Radius (in normalized 0..1 coords) where minimap fade starts
const FADE_START: f32 = 0.8;

pub struct NebulaPlugin;

impl Plugin for NebulaPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (init_nebula, animate_nebula));
    }
}

/// Holds the generated programs and image handles for animation.
#[derive(Resource)]
struct NebulaState {
    programs: NebulaPrograms,
    world_image: Handle<Image>,
    minimap_image: Handle<Image>,
    last_t: f32,
}

#[derive(Component)]
struct NebulaSprite;

/// Detect a replicated NebulaSeed, generate programs, create texture and sprites.
fn init_nebula(
    mut commands: Commands,
    query: Query<&NebulaSeed, Without<NebulaSprite>>,
    nebula_state: Option<Res<NebulaState>>,
    mut images: ResMut<Assets<Image>>,
    minimap_root: Query<Entity, With<MinimapRoot>>,
) {
    // Only initialize once
    if nebula_state.is_some() {
        return;
    }

    let Some(seed) = query.iter().next() else {
        return;
    };

    info!("Generating nebula from seed {:#X}", seed.0);
    let programs = nebula::generate_nebula(seed.0);

    let extent = Extent3d {
        width: NEBULA_SIZE,
        height: NEBULA_SIZE,
        depth_or_array_layers: 1,
    };
    let usage = RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD;

    // Render initial frame at t=0
    let world_pixels = render_nebula_pixels(&programs, 0.0);
    let world_handle = images.add(Image::new(
        extent,
        TextureDimension::D2,
        world_pixels,
        TextureFormat::Rgba8UnormSrgb,
        usage,
    ));

    // Separate minimap texture with radial alpha fade
    let minimap_pixels = render_minimap_pixels(&programs, 0.0);
    let minimap_handle = images.add(Image::new(
        extent,
        TextureDimension::D2,
        minimap_pixels,
        TextureFormat::Rgba8UnormSrgb,
        usage,
    ));

    // World-space sprite: covers the entire map, behind everything
    let map_diameter = MAP_RADIUS * 2.0;
    commands.spawn((
        NebulaSprite,
        Sprite {
            image: world_handle.clone(),
            custom_size: Some(Vec2::splat(map_diameter)),
            color: Color::srgba(0.5, 0.4, 0.8, WORLD_ALPHA),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, -95.0),
    ));

    // Minimap background: fill the minimap with the faded nebula
    if let Ok(root) = minimap_root.single() {
        commands.spawn((
            ChildOf(root),
            ImageNode::new(minimap_handle.clone()).with_color(Color::srgba(
                0.5,
                0.4,
                0.8,
                MINIMAP_ALPHA,
            )),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            ZIndex(-1),
        ));
    }

    commands.insert_resource(NebulaState {
        programs,
        world_image: world_handle,
        minimap_image: minimap_handle,
        last_t: 0.0,
    });
}

/// Slowly animate the nebula texture by updating the time parameter.
fn animate_nebula(
    mut images: ResMut<Assets<Image>>,
    mut state: Option<ResMut<NebulaState>>,
    time: Res<Time>,
) {
    let Some(state) = state.as_mut() else { return };

    let t = (time.elapsed_secs() * NEBULA_ANIM_SPEED).sin();

    // Skip re-render if t hasn't changed meaningfully
    if (t - state.last_t).abs() < 0.001 {
        return;
    }
    state.last_t = t;

    let world_pixels = render_nebula_pixels(&state.programs, t);
    if let Some(image) = images.get_mut(&state.world_image) {
        match &mut image.data {
            Some(data) if data.len() == world_pixels.len() => data.copy_from_slice(&world_pixels),
            slot => *slot = Some(world_pixels),
        }
    }

    let minimap_pixels = render_minimap_pixels(&state.programs, t);
    if let Some(image) = images.get_mut(&state.minimap_image) {
        match &mut image.data {
            Some(data) if data.len() == minimap_pixels.len() => {
                data.copy_from_slice(&minimap_pixels)
            }
            slot => *slot = Some(minimap_pixels),
        }
    }
}

/// Render the nebula texture into an RGBA pixel buffer (fully opaque).
fn render_nebula_pixels(programs: &NebulaPrograms, t: f32) -> Vec<u8> {
    let size = (NEBULA_SIZE * NEBULA_SIZE * 4) as usize;
    let mut pixels = vec![255u8; size];

    for (i, chunk) in pixels.chunks_mut(4).enumerate() {
        let i = i as u32;
        let py = (i / NEBULA_SIZE) as f32 / NEBULA_SIZE as f32 * 2.0 - 1.0;
        let px = (i % NEBULA_SIZE) as f32 / NEBULA_SIZE as f32 * 2.0 - 1.0;
        chunk[0] = nebula::channel(nebula::eval_program(&programs.r, px, py, t));
        chunk[1] = nebula::channel(nebula::eval_program(&programs.g, px, py, t));
        chunk[2] = nebula::channel(nebula::eval_program(&programs.b, px, py, t));
    }

    pixels
}

/// Render the minimap nebula with radial alpha fade at edges.
fn render_minimap_pixels(programs: &NebulaPrograms, t: f32) -> Vec<u8> {
    let size = (NEBULA_SIZE * NEBULA_SIZE * 4) as usize;
    let mut pixels = vec![0u8; size];

    for (i, chunk) in pixels.chunks_mut(4).enumerate() {
        let i = i as u32;
        let py = (i / NEBULA_SIZE) as f32 / NEBULA_SIZE as f32 * 2.0 - 1.0;
        let px = (i % NEBULA_SIZE) as f32 / NEBULA_SIZE as f32 * 2.0 - 1.0;

        // Distance from center (0..~1.41)
        let dist = (px * px + py * py).sqrt();

        // Fade: full opacity inside FADE_START, smooth falloff to 0 at radius 1.0
        let alpha = if dist <= FADE_START {
            1.0
        } else if dist >= 1.0 {
            0.0
        } else {
            let t = (dist - FADE_START) / (1.0 - FADE_START);
            // Smooth hermite interpolation
            let t = t * t * (3.0 - 2.0 * t);
            1.0 - t
        };

        chunk[0] = nebula::channel(nebula::eval_program(&programs.r, px, py, t));
        chunk[1] = nebula::channel(nebula::eval_program(&programs.g, px, py, t));
        chunk[2] = nebula::channel(nebula::eval_program(&programs.b, px, py, t));
        chunk[3] = (alpha * 255.0) as u8;
    }

    pixels
}
