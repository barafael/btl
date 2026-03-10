use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use bevy::prelude::*;
use lightyear::netcode::NetcodeServer;
use lightyear::prelude::server::*;
use lightyear::prelude::*;
use lightyear::webtransport::prelude::{Identity, server::WebTransportServerIo};

use avian2d::prelude::*;
use btl_protocol::*;
use btl_shared::{
    Asteroid, NebulaSeed, ShipBundle, generate_asteroid_layout,
    AUTOCANNON_COOLDOWN, AUTOCANNON_DAMAGE, AUTOCANNON_LIFETIME, AUTOCANNON_SPEED,
    MUZZLE_OFFSET, AMMO_COST,
    MINE_COOLDOWN, MINE_DAMAGE, MINE_LIFETIME, MINE_ARM_TIME, MINE_DROP_SPEED, MINE_MAX_ACTIVE,
};

/// Pending respawn entry
struct PendingRespawn {
    peer_id: PeerId,
    team: Team,
    link_entity: Entity,
    timer: f32,
}

/// Server-side resource tracking respawn timers.
#[derive(Resource, Default)]
struct RespawnQueue(Vec<PendingRespawn>);

const RESPAWN_DELAY: f32 = 3.0;

pub struct ServerPlugin;

impl Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(lightyear::prelude::server::ServerPlugins {
            tick_duration: Duration::from_secs_f64(1.0 / FIXED_TIMESTEP_HZ),
        });

        app.init_resource::<RespawnQueue>();
        app.add_systems(Startup, (start_server, spawn_asteroids, spawn_nebula));
        app.add_systems(FixedUpdate, (
            server_fire_projectiles,
            server_drop_mines,
            btl_shared::check_projectile_hits,
            btl_shared::check_mine_detonations,
            despawn_dead_ships,
            process_respawns,
        ));
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

/// Spawn static asteroid obstacles from the deterministic layout.
fn spawn_asteroids(mut commands: Commands) {
    let layout = generate_asteroid_layout();
    for (pos, radius, rotation) in &layout {
        commands.spawn((
            Asteroid { radius: *radius },
            RigidBody::Static,
            Collider::circle(*radius),
            Restitution::new(0.9),
            Position(*pos),
            Rotation::radians(*rotation),
            Replicate::to_clients(NetworkTarget::All),
        ));
    }
    info!("Spawned {} asteroids", layout.len());
}

/// Spawn nebula background seed. Each server session gets a unique nebula.
fn spawn_nebula(mut commands: Commands) {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0xDEAD_BEEF);
    commands.spawn((
        NebulaSeed(seed),
        Replicate::to_clients(NetworkTarget::All),
    ));
    info!("Spawned nebula with seed {seed:#X}");
}

/// Server spawns projectiles when ships fire (authoritative).
fn server_fire_projectiles(
    mut commands: Commands,
    mut query: Query<(
        &lightyear::prelude::input::native::ActionState<ShipInput>,
        &PlayerId,
        &Team,
        &Position,
        &LinearVelocity,
        &mut FireCooldown,
        &mut Ammo,
    )>,
) {
    for (input, player_id, team, pos, lin_vel, mut cooldown, mut ammo) in query.iter_mut() {
        if !input.0.fire || cooldown.remaining > 0.0 || ammo.current < AMMO_COST {
            continue;
        }

        // Fire!
        cooldown.remaining = AUTOCANNON_COOLDOWN;
        ammo.current -= AMMO_COST;

        // Aim direction from client's mouse cursor
        let aim_dir = Vec2::new(input.0.aim_angle.cos(), input.0.aim_angle.sin());
        let spawn_pos = pos.0 + aim_dir * MUZZLE_OFFSET;
        let proj_vel = lin_vel.0 + aim_dir * AUTOCANNON_SPEED;

        commands.spawn((
            Projectile {
                damage: AUTOCANNON_DAMAGE,
                owner: player_id.0,
                owner_team: *team,
                lifetime: AUTOCANNON_LIFETIME,
            },
            Position(spawn_pos),
            LinearVelocity(proj_vel),
            Replicate::to_clients(NetworkTarget::All),
        ));
    }
}

/// Server drops mines when ships request it (authoritative).
fn server_drop_mines(
    mut commands: Commands,
    mut query: Query<(
        &lightyear::prelude::input::native::ActionState<ShipInput>,
        &PlayerId,
        &Team,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut MineCooldown,
    )>,
    existing_mines: Query<&Mine>,
) {
    for (input, player_id, team, pos, rot, lin_vel, mut cooldown) in query.iter_mut() {
        if !input.0.drop_mine || cooldown.remaining > 0.0 {
            continue;
        }

        // Count active mines for this player
        let active_count = existing_mines
            .iter()
            .filter(|m| m.owner == player_id.0)
            .count();
        if active_count >= MINE_MAX_ACTIVE {
            continue;
        }

        cooldown.remaining = MINE_COOLDOWN;

        // Drop behind the ship with a small backward offset from ship velocity
        let backward = *rot * -Vec2::Y;
        let spawn_pos = pos.0 + backward * 20.0;
        let mine_vel = lin_vel.0 + backward * MINE_DROP_SPEED;

        commands.spawn((
            Mine {
                damage: MINE_DAMAGE,
                owner: player_id.0,
                owner_team: *team,
                lifetime: MINE_LIFETIME,
                arm_timer: MINE_ARM_TIME,
            },
            Position(spawn_pos),
            LinearVelocity(mine_vel),
            Replicate::to_clients(NetworkTarget::All),
        ));
    }
}

/// Despawn ships that have reached 0 HP and queue them for respawn.
fn despawn_dead_ships(
    mut commands: Commands,
    query: Query<(Entity, &Health, &PlayerId, &Team, &ControlledBy)>,
    mut respawn_queue: ResMut<RespawnQueue>,
) {
    for (entity, health, player_id, team, controlled_by) in query.iter() {
        if health.current <= 0.0 {
            info!("Ship {:?} destroyed (player {:?})", entity, player_id.0);
            respawn_queue.0.push(PendingRespawn {
                peer_id: player_id.0,
                team: *team,
                link_entity: controlled_by.owner,
                timer: RESPAWN_DELAY,
            });
            commands.entity(entity).despawn();
        }
    }
}

/// Tick respawn timers and respawn ships when ready.
fn process_respawns(
    mut commands: Commands,
    mut respawn_queue: ResMut<RespawnQueue>,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;

    respawn_queue.0.retain_mut(|entry| {
        entry.timer -= dt;
        if entry.timer <= 0.0 {
            // Respawn at a random-ish position based on team
            let angle = (entry.peer_id.to_bits() as f32 * 2.3) % std::f32::consts::TAU;
            let dist = 200.0;
            let spawn_pos = Vec2::new(dist * angle.cos(), dist * angle.sin());

            let ship = commands.spawn((
                ShipBundle::new(entry.peer_id, entry.team, spawn_pos),
                Replicate::to_clients(NetworkTarget::All),
                PredictionTarget::to_clients(NetworkTarget::Single(entry.peer_id)),
                InterpolationTarget::to_clients(NetworkTarget::AllExceptSingle(entry.peer_id)),
                ControlledBy {
                    owner: entry.link_entity,
                    lifetime: Default::default(),
                },
            )).id();

            info!("Respawned ship {ship:?} for {:?}", entry.peer_id);
            false // remove from queue
        } else {
            true // keep waiting
        }
    });
}
