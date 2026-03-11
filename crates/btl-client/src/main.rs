#![allow(clippy::type_complexity, clippy::too_many_arguments)]

mod client;
mod effects;
mod minimap;
mod nebula;
mod particles;
mod starfield;

use std::net::SocketAddr;

use bevy::log::LogPlugin;
#[cfg(not(target_arch = "wasm32"))]
use bevy::post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter};
use bevy::prelude::*;
use btl_protocol::SERVER_PORT;
use btl_shared::{
    MAP_RADIUS, OBJECTIVE_ZONE_RADIUS, SharedPlugin, objective_zone_positions,
    tridrant_boundary_angles,
};

fn default_server_addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], SERVER_PORT))
}

#[cfg(feature = "native")]
fn parse_config() -> (u64, SocketAddr, String) {
    use clap::Parser;

    fn random_client_id() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64 ^ std::process::id() as u64)
            .unwrap_or(1)
    }

    #[derive(Parser, Debug)]
    #[command(name = "btl-client")]
    struct Cli {
        /// Client ID (must be unique per client; random if omitted)
        #[arg(short, long, default_value_t = random_client_id())]
        id: u64,

        /// Server address
        #[arg(short, long, default_value_t = default_server_addr())]
        server: SocketAddr,

        /// Server certificate hash (hex, required for remote servers with self-signed certs)
        #[arg(short, long, default_value = "")]
        cert: String,
    }

    let cli = Cli::parse();
    (cli.id, cli.server, cli.cert)
}

#[cfg(not(feature = "native"))]
fn parse_config() -> (u64, SocketAddr, String) {
    // WASM: read from URL query params (?id=1&server=127.0.0.1:5888&cert=abcdef...)
    let params = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default();
    let mut id = {
        let mut buf = [0u8; 8];
        getrandom::getrandom(&mut buf).unwrap_or_default();
        u64::from_le_bytes(buf)
    };
    let mut server = default_server_addr();
    let mut cert = String::new();
    for param in params.trim_start_matches('?').split('&') {
        if let Some((key, value)) = param.split_once('=') {
            match key {
                "id" => {
                    id = value.parse().unwrap_or(1);
                }
                "server" => {
                    server = value.parse().unwrap_or(server);
                }
                "cert" => {
                    cert = value.to_string();
                }
                _ => {}
            }
        }
    }
    (id, server, cert)
}

fn main() {
    // Surface panics to browser console in WASM builds
    #[cfg(not(feature = "native"))]
    console_error_panic_hook::set_once();

    let (client_id, server_addr, cert_hash) = parse_config();

    App::new()
        .add_plugins(
            DefaultPlugins
                .set(LogPlugin {
                    filter: "wgpu=error,naga=warn,bevy_render=warn,bevy_ecs=warn,\
                         btl_client=debug,btl_shared=debug,btl_protocol=debug,\
                         lightyear=info"
                        .into(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        fit_canvas_to_parent: true,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .insert_resource(ClearColor(Color::BLACK))
        .add_plugins(SharedPlugin)
        .add_plugins(client::ClientPlugin {
            server_addr,
            client_id,
            cert_hash,
        })
        .add_plugins(starfield::StarfieldPlugin)
        .add_plugins(particles::ParticlePlugin)
        .add_plugins(effects::EffectsPlugin)
        .add_plugins(minimap::MinimapPlugin)
        .add_plugins(nebula::NebulaPlugin)
        .add_systems(
            Startup,
            (
                setup_camera,
                spawn_boundary_ring,
                spawn_tridrant_markers,
                spawn_objective_zones,
            ),
        )
        .run();
}

fn setup_camera(mut commands: Commands) {
    let projection = Projection::Orthographic(OrthographicProjection {
        scale: 2.4,
        ..OrthographicProjection::default_2d()
    });

    #[cfg(not(target_arch = "wasm32"))]
    commands.spawn((
        Camera2d,
        projection,
        Bloom {
            intensity: 0.3,
            low_frequency_boost: 0.5,
            low_frequency_boost_curvature: 0.95,
            high_pass_frequency: 1.0,
            prefilter: BloomPrefilter {
                threshold: 0.8,
                threshold_softness: 0.3,
            },
            composite_mode: BloomCompositeMode::Additive,
            ..Bloom::NATURAL
        },
    ));

    #[cfg(target_arch = "wasm32")]
    commands.spawn((Camera2d, projection));
}

/// Draw dotted lines from center to boundary for each tridrant division.
fn spawn_tridrant_markers(mut commands: Commands) {
    let angles = tridrant_boundary_angles();
    let dot_spacing = 40.0;
    let dot_size = 2.0;
    let color = Color::srgba(0.2, 0.2, 0.3, 0.4);

    for angle in angles {
        let dir = Vec2::new(angle.cos(), angle.sin());
        let mut dist = 200.0; // start away from center
        while dist < MAP_RADIUS {
            let pos = dir * dist;
            commands.spawn((
                Sprite {
                    color,
                    custom_size: Some(Vec2::splat(dot_size)),
                    ..default()
                },
                Transform::from_xyz(pos.x, pos.y, -49.0),
            ));
            dist += dot_spacing;
        }
    }
}

/// Marker for zone circle sprites, storing which zone (0, 1, 2) they belong to.
#[derive(Component)]
pub struct ZoneMarker(pub usize);

/// Draw circular zone indicators at each objective position.
fn spawn_objective_zones(mut commands: Commands) {
    let zones = objective_zone_positions();
    let marker_count = 120;
    let marker_size = 3.0;
    let color = Color::srgba(0.4, 0.4, 0.2, 0.5);

    for (zone_idx, center) in zones.iter().enumerate() {
        for i in 0..marker_count {
            let angle = (i as f32 / marker_count as f32) * std::f32::consts::TAU;
            let x = center.x + OBJECTIVE_ZONE_RADIUS * angle.cos();
            let y = center.y + OBJECTIVE_ZONE_RADIUS * angle.sin();

            commands.spawn((
                ZoneMarker(zone_idx),
                Sprite {
                    color,
                    custom_size: Some(Vec2::splat(marker_size)),
                    ..default()
                },
                Transform::from_xyz(x, y, -48.0),
            ));
        }
    }
}

fn spawn_boundary_ring(mut commands: Commands) {
    let marker_count = 720;
    let marker_size = 4.0;
    let color = Color::srgba(0.3, 0.1, 0.1, 0.6);

    for i in 0..marker_count {
        let angle = (i as f32 / marker_count as f32) * std::f32::consts::TAU;
        let x = MAP_RADIUS * angle.cos();
        let y = MAP_RADIUS * angle.sin();

        commands.spawn((
            Sprite {
                color,
                custom_size: Some(Vec2::splat(marker_size)),
                ..default()
            },
            Transform::from_xyz(x, y, -50.0),
        ));
    }
}
