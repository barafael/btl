mod client;
mod particles;
mod starfield;

use std::net::SocketAddr;

use bevy::log::LogPlugin;
use bevy::post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter};
use bevy::prelude::*;
use btl_protocol::SERVER_PORT;
use btl_shared::{MAP_RADIUS, SharedPlugin};

fn default_server_addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], SERVER_PORT))
}

#[cfg(feature = "native")]
fn parse_config() -> (u64, SocketAddr) {
    use clap::Parser;

    #[derive(Parser, Debug)]
    #[command(name = "btl-client")]
    struct Cli {
        /// Client ID (must be unique per client)
        #[arg(short, long, default_value_t = 1)]
        id: u64,

        /// Server address
        #[arg(short, long, default_value_t = default_server_addr())]
        server: SocketAddr,
    }

    let cli = Cli::parse();
    (cli.id, cli.server)
}

#[cfg(not(feature = "native"))]
fn parse_config() -> (u64, SocketAddr, String) {
    // WASM: read from URL query params (?id=1&server=127.0.0.1:5888&cert=abcdef...)
    let params = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default();
    let mut id = 1u64;
    let mut server = default_server_addr();
    let mut cert = String::new();
    for param in params.trim_start_matches('?').split('&') {
        if let Some((key, value)) = param.split_once('=') {
            match key {
                "id" => { id = value.parse().unwrap_or(1); }
                "server" => { server = value.parse().unwrap_or(server); }
                "cert" => { cert = value.to_string(); }
                _ => {}
            }
        }
    }
    (id, server, cert)
}

#[cfg(feature = "native")]
fn parse_config_full() -> (u64, SocketAddr, String) {
    let (id, addr) = parse_config();
    (id, addr, String::new())
}

fn main() {
    // Surface panics to browser console in WASM builds
    #[cfg(not(feature = "native"))]
    console_error_panic_hook::set_once();

    #[cfg(feature = "native")]
    let (client_id, server_addr, cert_hash) = parse_config_full();
    #[cfg(not(feature = "native"))]
    let (client_id, server_addr, cert_hash) = parse_config();

    App::new()
        .add_plugins(DefaultPlugins
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
        .add_systems(Startup, (setup_camera, spawn_boundary_ring))
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d::default(),
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
