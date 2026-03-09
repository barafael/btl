mod client;

use std::net::SocketAddr;

use bevy::prelude::*;
use btl_protocol::SERVER_PORT;
use btl_shared::SharedPlugin;

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
fn parse_config() -> (u64, SocketAddr) {
    // WASM: use defaults. TODO: read from URL query params or JS bridge.
    (1, default_server_addr())
}

fn main() {
    let (client_id, server_addr) = parse_config();

    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(SharedPlugin)
        .add_plugins(client::ClientPlugin {
            server_addr,
            client_id,
        })
        .add_systems(Startup, setup_camera)
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d::default());
}
