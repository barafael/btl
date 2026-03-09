mod client;

use std::net::SocketAddr;

use bevy::prelude::*;
use btl_protocol::SERVER_PORT;
use btl_shared::SharedPlugin;
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

fn default_server_addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], SERVER_PORT))
}

fn main() {
    let cli = Cli::parse();

    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(SharedPlugin)
        .add_plugins(client::ClientPlugin {
            server_addr: cli.server,
            client_id: cli.id,
        })
        .add_systems(Startup, setup_camera)
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d::default());
}
