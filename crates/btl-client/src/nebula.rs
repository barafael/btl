//! Client-side nebula background rendering.
//!
//! Receives a `NebulaSeed` from the server, generates the procedural texture
//! locally, and renders it as:
//! - A large world-space sprite behind everything (slowly animated)
//! - A background image filling the minimap

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use btl_shared::nebula::{self, NebulaPrograms};
use btl_shared::{MAP_RADIUS, NebulaSeed};

use crate::minimap::MinimapRoot;

/// Nebula texture resolution (stretched over the map — low res is fine)
const NEBULA_SIZE: u32 = 128;
/// Animation speed: full cycle ~ 9 minutes
const NEBULA_ANIM_SPEED: f32 = 0.048;
/// World-space sprite opacity (barely visible — space is still black)
const WORLD_ALPHA: f32 = 0.09;
/// Minimap background opacity
const MINIMAP_ALPHA: f32 = 0.35;

pub struct NebulaPlugin;

impl Plugin for NebulaPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (init_nebula, animate_nebula));
    }
}

/// Holds the generated programs and image handle for animation.
#[derive(Resource)]
struct NebulaState {
    programs: NebulaPrograms,
    image: Handle<Image>,
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

    // Render initial frame at t=0
    let pixels = render_nebula_pixels(&programs, 0.0);
    let image = Image::new(
        Extent3d {
            width: NEBULA_SIZE,
            height: NEBULA_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    let handle = images.add(image);

    // World-space sprite: covers the entire map, behind everything
    let map_diameter = MAP_RADIUS * 2.0;
    commands.spawn((
        NebulaSprite,
        Sprite {
            image: handle.clone(),
            custom_size: Some(Vec2::splat(map_diameter)),
            // Purple-blue tint at low opacity for space aesthetic
            color: Color::srgba(0.5, 0.4, 0.8, WORLD_ALPHA),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, -95.0),
    ));

    // Minimap background: fill the minimap with the nebula
    if let Ok(root) = minimap_root.single() {
        commands.spawn((
            ChildOf(root),
            ImageNode::new(handle.clone()).with_color(Color::srgba(0.5, 0.4, 0.8, MINIMAP_ALPHA)),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            ZIndex(-1),
        ));
    }

    commands.insert_resource(NebulaState {
        programs,
        image: handle,
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

    let pixels = render_nebula_pixels(&state.programs, t);

    if let Some(image) = images.get_mut(&state.image) {
        match &mut image.data {
            Some(data) if data.len() == pixels.len() => data.copy_from_slice(&pixels),
            slot => *slot = Some(pixels),
        }
    }
}

/// Render the nebula texture into an RGBA pixel buffer.
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
        // chunk[3] stays 255 (fully opaque — alpha handled by sprite tint)
    }

    pixels
}
