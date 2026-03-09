use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use bevy::prelude::*;
use lightyear::netcode::NetcodeServer;
use lightyear::prelude::server::*;
use lightyear::prelude::*;
use lightyear::webtransport::prelude::{Identity, server::WebTransportServerIo};

use btl_protocol::*;
use btl_shared::ShipBundle;

pub struct ServerPlugin;

impl Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(lightyear::prelude::server::ServerPlugins {
            tick_duration: Duration::from_secs_f64(1.0 / FIXED_TIMESTEP_HZ),
        });

        app.add_systems(Startup, start_server);
        app.add_observer(handle_new_client_link);
        app.add_observer(handle_client_connected);
    }
}

fn start_server(mut commands: Commands) {
    let server_addr = SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), SERVER_PORT);

    // Self-signed certificate for development
    let certificate = Identity::self_signed(["localhost", "127.0.0.1", "::1"])
        .expect("Failed to generate self-signed certificate");

    // Print cert hash for WASM clients (hex without colons)
    let cert_hash = certificate.certificate_chain().as_slice()[0].hash();
    let hash_hex: String = cert_hash.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    info!("Certificate hash (for browser clients): {hash_hex}");

    let netcode = NetcodeServer::new(NetcodeConfig {
        protocol_id: PROTOCOL_ID,
        private_key: PRIVATE_KEY,
        ..Default::default()
    });

    let entity = commands
        .spawn((
            netcode,
            LocalAddr(server_addr),
            WebTransportServerIo { certificate },
        ))
        .id();

    commands.trigger(Start { entity });
    info!("Server starting on {server_addr} (WebTransport/QUIC)");
}

/// When a new client link is created, attach a ReplicationSender.
fn handle_new_client_link(trigger: On<Add, LinkOf>, mut commands: Commands) {
    info!(
        "New client link entity {:?} — attaching ReplicationSender",
        trigger.entity
    );
    commands.entity(trigger.entity).insert((
        ReplicationSender::new(
            REPLICATION_INTERVAL,
            SendUpdatesMode::SinceLastAck,
            false,
        ),
        Name::from("Client"),
    ));
}

/// When a client is confirmed connected, spawn their ship.
fn handle_client_connected(
    trigger: On<Add, Connected>,
    query: Query<&RemoteId, With<ClientOf>>,
    existing_players: Query<&Team>,
    mut commands: Commands,
) {
    let Ok(client_id) = query.get(trigger.entity) else {
        return;
    };
    let peer_id = client_id.0;

    // Assign team based on current balance
    let mut red_count = 0u32;
    let mut blue_count = 0u32;
    for team in existing_players.iter() {
        match team {
            Team::Red => red_count += 1,
            Team::Blue => blue_count += 1,
        }
    }
    let team = if red_count <= blue_count {
        Team::Red
    } else {
        Team::Blue
    };

    // Spawn position based on team
    let spawn_pos = match team {
        Team::Red => Vec2::new(-50.0, 0.0),
        Team::Blue => Vec2::new(50.0, 0.0),
    };

    info!(
        "Client {peer_id:?} connected -> {team:?} team (link entity: {:?})",
        trigger.entity
    );

    let ship = commands.spawn((
        ShipBundle::new(peer_id, team, spawn_pos),
        // Replicate to all clients
        Replicate::to_clients(NetworkTarget::All),
        // Owning client gets prediction
        PredictionTarget::to_clients(NetworkTarget::Single(peer_id)),
        // Everyone else gets interpolation
        InterpolationTarget::to_clients(NetworkTarget::AllExceptSingle(peer_id)),
        // Mark ownership
        ControlledBy {
            owner: trigger.entity,
            lifetime: Default::default(),
        },
    )).id();

    info!("Spawned ship entity {ship:?} for {peer_id:?} at {spawn_pos}");
}
