use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use bevy::prelude::*;
use lightyear::netcode::NetcodeServer;
use lightyear::prelude::server::*;
use lightyear::prelude::*;
use lightyear::webtransport::prelude::{Identity, server::WebTransportServerIo};

use avian2d::prelude::*;
use btl_protocol::{*, ZoneState};
use btl_shared::{
    AMMO_COST, AUTOCANNON_COOLDOWN, AUTOCANNON_DAMAGE, AUTOCANNON_LIFETIME, AUTOCANNON_SPEED,
    Asteroid, CLOAK_COOLDOWN, CLOAK_DURATION, Cloak, DEFENSE_TURRET_COOLDOWN,
    DEFENSE_TURRET_DAMAGE, DEFENSE_TURRET_FIRE_TOLERANCE, DEFENSE_TURRET_LIFETIME,
    DEFENSE_TURRET_MOUNTS, DEFENSE_TURRET_RANGE, DEFENSE_TURRET_SLEW_RATE, DEFENSE_TURRET_SPEED,
    DRONE_AGGRO_RANGE, DRONE_DETONATION_DAMAGE, DRONE_DETONATION_RADIUS, DRONE_KAMIKAZE_HEALTH,
    DRONE_KAMIKAZE_SPEED, DRONE_LASER_COUNT, DRONE_LASER_HEALTH, DRONE_LASER_RANGE,
    DRONE_MAX_COUNT, DRONE_ORBIT_RADIUS, DRONE_RESPAWN_TIME, DRONE_SPEED, Drone,
    HEAVY_CANNON_AMMO_COST, HEAVY_CANNON_COOLDOWN, HEAVY_CANNON_DAMAGE, HEAVY_CANNON_LIFETIME,
    HEAVY_CANNON_SPEED, HEAVY_MUZZLE_OFFSET, LASER_AMMO_COST, LASER_DPS, LASER_RANGE,
    MINE_ARM_TIME, MINE_COOLDOWN, MINE_DAMAGE, MINE_DROP_SPEED, MINE_LIFETIME, MINE_MAX_ACTIVE,
    MUZZLE_OFFSET, NebulaSeed, OBJECTIVE_ZONE_RADIUS, PULSE_COOLDOWN, PULSE_RADIUS,
    RAILGUN_CHARGE_TIME, RAILGUN_COOLDOWN, RAILGUN_DAMAGE, RAILGUN_LIFETIME, RAILGUN_SPEED,
    RailgunCharge, SCORE_LIMIT, ShipBundle, TBOAT_RADIUS, TORPEDO_COOLDOWN, TORPEDO_DAMAGE,
    TORPEDO_LIFETIME, TORPEDO_MAX_ACTIVE, TORPEDO_MUZZLE_OFFSET, TORPEDO_SPEED, TORPEDO_TURN_RATE,
    TURRET_COOLDOWN, TURRET_DAMAGE, TURRET_FIRE_TOLERANCE, TURRET_LIFETIME, TURRET_MOUNTS,
    TURRET_RANGE, TURRET_SLEW_RATE, TURRET_SPEED, Torpedo, ZONE_SCORE_RATE,
    CollisionGrids, FIXED_DT, MAX_ASTEROID_RADIUS, MAX_SHIP_RADIUS,
    generate_asteroid_layout, objective_zone_positions, ray_circle_intersect,
    CAPTURE_RATE, DECAP_RATE, DAMAGE_FLASH_DURATION, DamageFlash, SpawnProtection,
    ship_radius, ship_max_health, ship_ammo_regen,
    COLLISION_DAMAGE_VELOCITY_THRESHOLD, COLLISION_DAMAGE_PER_VELOCITY, COLLISION_FASTER_SHIP_MULT,
    ZONE_HP_REGEN, ZONE_REGEN_MULT, FUEL_REGEN_RATE,
    OBJECTIVE_KINDS, ObjectiveKind, ZoneDrone, ZoneRailgun, ZoneShield,
    FACTORY_LASER_DRONES, FACTORY_KAMIKAZE_DRONES, FACTORY_DRONE_HEALTH,
    FACTORY_DRONE_SPEED, FACTORY_DRONE_ORBIT_RADIUS, FACTORY_DRONE_AGGRO_RANGE,
    FACTORY_DRONE_LASER_RANGE, FACTORY_DRONE_LASER_DPS, FACTORY_DRONE_KAMIKAZE_DAMAGE,
    FACTORY_DRONE_RESPAWN_TIME,
    ZONE_RAILGUN_RANGE, ZONE_RAILGUN_SLEW_RATE, ZONE_RAILGUN_CHARGE_TIME,
    ZONE_RAILGUN_LOCK_TIME, ZONE_RAILGUN_DAMAGE, ZONE_RAILGUN_COOLDOWN,
    ZONE_RAILGUN_PROJECTILE_SPEED, ZONE_RAILGUN_PROJECTILE_LIFETIME,
    ZONE_SHIELD_RADIUS, RailgunTurretState,
    ROUND_END_DISPLAY_TIME, ROUND_RESTART_COUNTDOWN, KILL_FEED_MAX,
};

/// Server-only component tracking drone squad state for Drone Commander ships.
#[derive(Component)]
struct DroneSquad {
    pub respawn_timer: f32,
}

/// Server-only resource tracking zone defense drone respawn timers (one per Factory zone).
#[derive(Resource, Default)]
struct ZoneDefenseTimers {
    /// Per-zone respawn timer. Only used for Factory zones.
    respawn_timers: [f32; 3],
    /// Track the last known controller per zone to detect flips.
    last_controller: [u8; 3],
}

/// Pending respawn entry
struct PendingRespawn {
    peer_id: PeerId,
    team: Team,
    class: ShipClass,
    link_entity: Entity,
    timer: f32,
}

/// Server-side resource tracking respawn timers.
#[derive(Resource, Default)]
struct RespawnQueue(Vec<PendingRespawn>);

/// Per-player kill counts accumulated during the current round.
#[derive(Resource, Default)]
struct MatchStats {
    /// Maps PeerId → (team, kill_count)
    kills: HashMap<PeerId, (Team, u32)>,
}

/// Whether the game is in lobby or active play.
#[derive(Resource, Default, PartialEq)]
enum ServerGamePhase {
    #[default]
    Lobby,
    InGame,
}

const RESPAWN_DELAY: f32 = 3.0;

pub struct ServerPlugin;

impl Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(lightyear::prelude::server::ServerPlugins {
            tick_duration: Duration::from_secs_f64(1.0 / FIXED_TIMESTEP_HZ),
        });

        app.init_resource::<RespawnQueue>();
        app.init_resource::<ZoneDefenseTimers>();
        app.init_resource::<MatchStats>();
        app.init_resource::<ServerGamePhase>();
        app.add_systems(
            Startup,
            (start_server, spawn_asteroids, spawn_nebula, spawn_scores),
        );
        // Always-on: lobby, class switching, physics, collision, respawn
        app.add_systems(
            FixedUpdate,
            (
                lobby_management,
                handle_class_switch,
                server_turret_ai,
                server_torpedo_homing,
                btl_shared::check_projectile_hits.after(btl_shared::rebuild_collision_grids),
                btl_shared::check_projectile_asteroid_hits.after(btl_shared::rebuild_collision_grids),
                btl_shared::check_mine_detonations,
                btl_shared::update_torpedo_lifetime,
                btl_shared::check_torpedo_shootdown,
                btl_shared::check_torpedo_hits,
                despawn_dead_ships,
                despawn_orphaned_drones,
                process_respawns,
                collision_damage.after(btl_shared::rebuild_collision_grids),
            ),
        );
        // In-game only: weapons + drones
        app.add_systems(
            FixedUpdate,
            (
                server_fire_projectiles,
                server_fire_laser,
                server_drop_mines,
                server_launch_torpedoes,
                server_railgun,
                server_cloak,
            ).run_if(in_game_phase),
        );
        app.add_systems(
            FixedUpdate,
            (
                server_init_drone_squads,
                server_spawn_initial_drones,
                server_drone_respawn,
                server_drone_ai,
                server_anti_drone_pulse,
                btl_shared::check_projectile_drone_hits.after(btl_shared::rebuild_collision_grids),
                btl_shared::drone_laser_damage,
                btl_shared::drone_kamikaze_impact,
            ),
        );
        app.add_systems(FixedUpdate, round_management);
        // In-game only: zone scoring + defenses
        app.add_systems(
            FixedUpdate,
            (update_zone_scores, zone_benefits).run_if(in_game_phase),
        );
        app.add_systems(
            FixedUpdate,
            (
                zone_defense_management,
                zone_factory_drones,
                zone_factory_drone_ai.after(btl_shared::rebuild_collision_grids),
                zone_factory_drone_laser.after(btl_shared::rebuild_collision_grids),
                zone_factory_drone_kamikaze.after(btl_shared::rebuild_collision_grids),
                zone_drone_death,
                zone_railgun_ai,
                zone_shield_deflect,
                btl_shared::check_projectile_zone_drone_hits.after(btl_shared::rebuild_collision_grids),
            ).run_if(in_game_phase),
        );
        app.add_observer(handle_new_client_link);
        app.add_observer(handle_client_connected);
    }
}

fn start_server(mut commands: Commands) {
    let port = std::env::var("BTL_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(SERVER_PORT);
    let server_addr = SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), port);

    // Self-signed certificate for development
    let certificate = Identity::self_signed(["localhost", "127.0.0.1", "::1"])
        .expect("Failed to generate self-signed certificate");

    // Print cert hash for WASM clients (hex without colons)
    let cert_hash = certificate.certificate_chain().as_slice()[0].hash();
    let hash_hex: String = cert_hash
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    info!("Certificate hash (for browser clients): {hash_hex}");

    // Serve cert hash over HTTP so WASM clients can auto-connect without ?cert= param
    let http_port = port + 1;
    let hash_for_http = hash_hex.clone();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        let Ok(listener) = std::net::TcpListener::bind(
            SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), http_port),
        ) else {
            error!("Failed to start cert hash HTTP server on port {http_port}");
            return;
        };
        info!("Cert hash available at http://0.0.0.0:{http_port}/");
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let body = &hash_for_http;
            let response = format!(
                "HTTP/1.1 200 OK\r\n\
                 Access-Control-Allow-Origin: *\r\n\
                 Content-Type: text/plain\r\n\
                 Content-Length: {}\r\n\
                 \r\n\
                 {body}",
                body.len(),
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });

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
/// Run condition: fire / zone systems only when game is in-progress (not in lobby or countdown).
fn in_game_phase(scores_q: Query<&TeamScores>) -> bool {
    scores_q
        .single()
        .map(|s| matches!(s.lobby_phase, LobbyPhase::InGame))
        .unwrap_or(false)
}

/// Manages the pre-game lobby: tracks player ready states and transitions to InGame.
fn lobby_management(
    mut scores_q: Query<&mut TeamScores>,
    ships: Query<(
        &lightyear::prelude::input::native::ActionState<ShipInput>,
        &PlayerId,
        &Team,
        &ShipClass,
    )>,
    mut server_phase: ResMut<ServerGamePhase>,
) {
    let Ok(mut scores) = scores_q.single_mut() else {
        return;
    };

    // Build the current roster from all live ships.
    let new_roster: Vec<LobbyEntry> = ships
        .iter()
        .map(|(action, player_id, team, class)| LobbyEntry {
            peer_id: player_id.0,
            team: *team,
            class: *class,
            ready: action.0.lobby_ready,
        })
        .collect();

    match scores.lobby_phase {
        LobbyPhase::InGame => {
            // Mark all players as ready in the roster for display purposes.
            scores.lobby_roster = new_roster
                .into_iter()
                .map(|mut e| { e.ready = true; e })
                .collect();
        }
        LobbyPhase::Lobby => {
            scores.lobby_roster = new_roster.clone();
            let all_ready = !new_roster.is_empty() && new_roster.iter().all(|e| e.ready);
            if all_ready {
                scores.lobby_phase = LobbyPhase::Countdown(5.0);
            }
        }
        LobbyPhase::Countdown(t) => {
            scores.lobby_roster = new_roster.clone();
            let all_still_ready = !new_roster.is_empty() && new_roster.iter().all(|e| e.ready);
            if !all_still_ready {
                scores.lobby_phase = LobbyPhase::Lobby;
                return;
            }
            let new_t = t - FIXED_DT;
            if new_t <= 0.0 {
                scores.lobby_phase = LobbyPhase::InGame;
                *server_phase = ServerGamePhase::InGame;
            } else {
                scores.lobby_phase = LobbyPhase::Countdown(new_t);
            }
        }
    }
}

fn handle_new_client_link(trigger: On<Add, LinkOf>, mut commands: Commands) {
    info!(
        "New client link entity {:?} — attaching ReplicationSender",
        trigger.entity
    );
    commands.entity(trigger.entity).insert((
        ReplicationSender::new(REPLICATION_INTERVAL, SendUpdatesMode::SinceLastAck, false),
        Name::from("Client"),
    ));
}

/// Spawn a replicated, predicted, interpolated player ship and return its entity.
fn spawn_player_ship(
    commands: &mut Commands,
    peer_id: PeerId,
    team: Team,
    class: ShipClass,
    pos: Vec2,
    link_entity: Entity,
) -> Entity {
    commands
        .spawn((
            ShipBundle::new(peer_id, team, class, pos),
            Replicate::to_clients(NetworkTarget::All),
            PredictionTarget::to_clients(NetworkTarget::Single(peer_id)),
            InterpolationTarget::to_clients(NetworkTarget::AllExceptSingle(peer_id)),
            ControlledBy {
                owner: link_entity,
                lifetime: Default::default(),
            },
        ))
        .id()
}

/// When a client is confirmed connected, spawn their ship.
fn handle_client_connected(
    trigger: On<Add, Connected>,
    query: Query<&RemoteId, With<ClientOf>>,
    existing_players: Query<&Team>,
    scores_q: Query<&TeamScores>,
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

    let zone_states = scores_q
        .single()
        .map(|s| s.zones)
        .unwrap_or_default();
    let spawn_pos = pick_spawn_position(team, &zone_states, peer_id.to_bits());

    info!(
        "Client {peer_id:?} connected -> {team:?} team (link entity: {:?})",
        trigger.entity
    );

    let class = ShipClass::default();

    let ship = spawn_player_ship(&mut commands, peer_id, team, class, spawn_pos, trigger.entity);

    info!("Spawned {class:?} ship {ship:?} for {peer_id:?} at {spawn_pos}");
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
    commands.spawn((NebulaSeed(seed), Replicate::to_clients(NetworkTarget::All)));
    info!("Spawned nebula with seed {seed:#X}");
}

/// Server spawns projectiles when ships fire (authoritative).
/// Weapon stats depend on ship class.
fn server_fire_projectiles(
    mut commands: Commands,
    mut query: Query<(
        &lightyear::prelude::input::native::ActionState<ShipInput>,
        &PlayerId,
        &Team,
        &ShipClass,
        &Position,
        &LinearVelocity,
        &mut FireCooldown,
        &mut Ammo,
    )>,
) {
    for (input, player_id, team, class, pos, lin_vel, mut cooldown, mut ammo) in query.iter_mut() {
        let (cd, cost, speed, damage, lifetime, muzzle, kind) = match class {
            ShipClass::Interceptor => (
                AUTOCANNON_COOLDOWN,
                AMMO_COST,
                AUTOCANNON_SPEED,
                AUTOCANNON_DAMAGE,
                AUTOCANNON_LIFETIME,
                MUZZLE_OFFSET,
                ProjectileKind::Autocannon,
            ),
            ShipClass::Gunship => (
                HEAVY_CANNON_COOLDOWN,
                HEAVY_CANNON_AMMO_COST,
                HEAVY_CANNON_SPEED,
                HEAVY_CANNON_DAMAGE,
                HEAVY_CANNON_LIFETIME,
                HEAVY_MUZZLE_OFFSET,
                ProjectileKind::HeavyCannon,
            ),
            // TorpedoBoat uses laser, Sniper uses railgun, DroneCommander uses turrets
            ShipClass::TorpedoBoat | ShipClass::Sniper | ShipClass::DroneCommander => continue,
        };

        if !input.0.fire || cooldown.remaining > 0.0 || ammo.current < cost {
            continue;
        }

        cooldown.remaining = cd;
        ammo.current -= cost;

        let aim_dir = Vec2::new(input.0.aim_angle.cos(), input.0.aim_angle.sin());
        let spawn_pos = pos.0 + aim_dir * muzzle;
        let proj_vel = lin_vel.0 + aim_dir * speed;

        commands.spawn((
            Projectile {
                damage,
                owner: player_id.0,
                owner_team: *team,
                lifetime,
            },
            kind,
            Position(spawn_pos),
            LinearVelocity(proj_vel),
            Replicate::to_clients(NetworkTarget::All),
        ));
    }
}

/// Server drops mines when ships request it (Interceptor only).
fn server_drop_mines(
    mut commands: Commands,
    mut query: Query<(
        &lightyear::prelude::input::native::ActionState<ShipInput>,
        &PlayerId,
        &Team,
        &ShipClass,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut MineCooldown,
    )>,
    existing_mines: Query<&Mine>,
) {
    for (input, player_id, team, class, pos, rot, lin_vel, mut cooldown) in query.iter_mut() {
        // Only Interceptors and Snipers have mines (DroneCommander uses drop_mine for pulse)
        if *class != ShipClass::Interceptor && *class != ShipClass::Sniper {
            continue;
        }
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
    query: Query<(
        Entity,
        &Health,
        &PlayerId,
        &Team,
        &ShipClass,
        &ControlledBy,
        &LastDamagedBy,
    )>,
    all_ships: Query<(&PlayerId, &Team)>,
    mut respawn_queue: ResMut<RespawnQueue>,
    mut stats: ResMut<MatchStats>,
    mut scores_q: Query<&mut TeamScores>,
) {
    for (entity, health, player_id, team, class, controlled_by, last_hit) in query.iter() {
        if health.current <= 0.0 {
            let victim_peer = player_id.0;
            let victim_team = *team;
            let victim_class = *class;

            if let Some(killer_peer) = last_hit.attacker {
                // Find killer's team by scanning live ships
                let killer_team = all_ships
                    .iter()
                    .find(|(pid, _)| pid.0 == killer_peer)
                    .map(|(_, t)| *t);

                if let Some(killer_team) = killer_team {
                    let entry = stats.kills.entry(killer_peer).or_insert((killer_team, 0));
                    entry.1 += 1;

                    if let Ok(mut scores) = scores_q.single_mut() {
                        scores.kill_feed.insert(0, KillEvent {
                            killer_team,
                            victim_team,
                            victim_class,
                        });
                        scores.kill_feed.truncate(KILL_FEED_MAX);
                    }
                }
                info!("Ship destroyed (player {:?}) — killed by {:?}", victim_peer, killer_peer);
            } else {
                info!("Ship destroyed (player {:?})", victim_peer);
            }

            respawn_queue.0.push(PendingRespawn {
                peer_id: victim_peer,
                team: victim_team,
                class: victim_class,
                link_entity: controlled_by.owner,
                timer: RESPAWN_DELAY,
            });
            commands.entity(entity).despawn();
        }
    }
}

/// Despawn player drones whose DroneCommander ship has died this tick.
fn despawn_orphaned_drones(
    mut commands: Commands,
    drones: Query<(Entity, &Drone)>,
    commanders: Query<&PlayerId, With<DroneSquad>>,
) {
    let live: std::collections::HashSet<PeerId> =
        commanders.iter().map(|pid| pid.0).collect();
    for (entity, drone) in drones.iter() {
        if !live.contains(&drone.owner) {
            commands.entity(entity).try_despawn();
        }
    }
}

/// Pick a spawn position near a zone controlled by the given team, or fallback to random.
fn pick_spawn_position(team: Team, zone_states: &[ZoneState; 3], peer_bits: u64) -> Vec2 {
    let zones = objective_zone_positions();
    let team_code = match team {
        Team::Red => 1,
        Team::Blue => 2,
    };

    let owned: Vec<Vec2> = zone_states
        .iter()
        .enumerate()
        .filter(|(_, zs)| zs.controller == team_code)
        .map(|(i, _)| zones[i])
        .collect();

    // Pick one based on peer bits, spawn at edge of zone
    let angle = (peer_bits as f32 * 2.3) % std::f32::consts::TAU;
    if !owned.is_empty() {
        let idx = (peer_bits as usize) % owned.len();
        let center = owned[idx];
        center + Vec2::new(angle.cos(), angle.sin()) * (OBJECTIVE_ZONE_RADIUS * 0.5)
    } else {
        // No zones controlled — spawn at random position
        Vec2::new(200.0 * angle.cos(), 200.0 * angle.sin())
    }
}

/// Tick respawn timers and respawn ships when ready.
fn process_respawns(
    mut commands: Commands,
    mut respawn_queue: ResMut<RespawnQueue>,
    scores_q: Query<&TeamScores>,
) {
    let dt = FIXED_DT;
    let zone_states = scores_q
        .single()
        .map(|s| s.zones)
        .unwrap_or_default();

    respawn_queue.0.retain_mut(|entry| {
        entry.timer -= dt;
        if entry.timer <= 0.0 {
            let spawn_pos =
                pick_spawn_position(entry.team, &zone_states, entry.peer_id.to_bits());

            let ship = spawn_player_ship(
                &mut commands,
                entry.peer_id,
                entry.team,
                entry.class,
                spawn_pos,
                entry.link_entity,
            );

            info!("Respawned ship {ship:?} for {:?}", entry.peer_id);
            false // remove from queue
        } else {
            true // keep waiting
        }
    });
}

/// Handle class switch requests: if a player's input has class_request != 0
/// and it differs from current class, despawn and respawn with new class.
fn handle_class_switch(
    mut commands: Commands,
    query: Query<(
        Entity,
        &lightyear::prelude::input::native::ActionState<ShipInput>,
        &PlayerId,
        &Team,
        &ShipClass,
        &Position,
        &ControlledBy,
    )>,
) {
    for (entity, input, player_id, team, class, pos, controlled_by) in query.iter() {
        let Some(requested) = ShipClass::from_request(input.0.class_request) else {
            continue;
        };
        if requested == *class {
            continue;
        }

        info!(
            "Player {:?} switching from {class:?} to {requested:?}",
            player_id.0
        );

        let spawn_pos = pos.0;
        let link_entity = controlled_by.owner;
        let peer_id = player_id.0;
        let team = *team;

        commands.entity(entity).despawn();

        spawn_player_ship(&mut commands, peer_id, team, requested, spawn_pos, link_entity);
    }
}

/// Server applies continuous laser damage for TorpedoBoat (raycast while fire held).
/// Collects hits first, then applies damage to avoid query conflicts.
fn server_fire_laser(
    mut ships: Query<(
        &lightyear::prelude::input::native::ActionState<ShipInput>,
        &Team,
        &ShipClass,
        &Position,
        &PlayerId,
        &mut Ammo,
    )>,
    mut targets: Query<(Entity, &Position, &Team, &SpawnProtection, &mut Health, &mut DamageFlash, &mut LastDamagedBy)>,
    asteroids: Query<(&Position, &Asteroid)>,
) {
    let dt = FIXED_DT;

    let mut hits: Vec<(Entity, f32, PeerId)> = Vec::with_capacity(8);

    for (input, team, class, pos, player_id, mut ammo) in ships.iter_mut() {
        if *class != ShipClass::TorpedoBoat || !input.0.fire {
            continue;
        }

        let cost = LASER_AMMO_COST * dt;
        if ammo.current < cost {
            continue;
        }
        ammo.current -= cost;

        let aim_dir = Vec2::new(input.0.aim_angle.cos(), input.0.aim_angle.sin());

        let mut best_t = LASER_RANGE;
        let mut best_entity: Option<Entity> = None;

        for (ast_pos, ast) in asteroids.iter() {
            let t = ray_circle_intersect(pos.0, aim_dir, ast_pos.0, ast.radius);
            if t > 0.0 && t < best_t {
                best_t = t;
                best_entity = None;
            }
        }

        for (entity, target_pos, target_team, sp, _, _, _) in targets.iter() {
            if *target_team == *team || sp.remaining > 0.0 {
                continue;
            }
            let to_target = target_pos.0 - pos.0;
            let t = to_target.dot(aim_dir);
            if t < 0.0 || t > best_t {
                continue;
            }
            let closest_point = pos.0 + aim_dir * t;
            let dist_sq = (target_pos.0 - closest_point).length_squared();
            if dist_sq < TBOAT_RADIUS * TBOAT_RADIUS * 4.0 {
                best_t = t;
                best_entity = Some(entity);
            }
        }

        if let Some(entity) = best_entity {
            let falloff = 1.0 - 0.7 * (best_t / LASER_RANGE);
            hits.push((entity, LASER_DPS * falloff * dt, player_id.0));
        }
    }

    for (entity, damage, attacker) in hits {
        if let Ok((_, _, _, _, mut hp, mut flash, mut last_hit)) = targets.get_mut(entity) {
            hp.current -= damage;
            last_hit.attacker = Some(attacker);
            flash.timer = DAMAGE_FLASH_DURATION;
        }
    }
}

/// Server spawns homing torpedoes for TorpedoBoat (on drop_mine input).
fn server_launch_torpedoes(
    mut commands: Commands,
    mut query: Query<(
        &lightyear::prelude::input::native::ActionState<ShipInput>,
        &PlayerId,
        &Team,
        &ShipClass,
        &Position,
        &LinearVelocity,
        &mut MineCooldown,
    )>,
    existing_torpedoes: Query<&Torpedo>,
) {
    for (input, player_id, team, class, pos, lin_vel, mut cooldown) in query.iter_mut() {
        if *class != ShipClass::TorpedoBoat {
            continue;
        }
        if !input.0.drop_mine || cooldown.remaining > 0.0 {
            continue;
        }

        let active_count = existing_torpedoes
            .iter()
            .filter(|t| t.owner == player_id.0)
            .count();
        if active_count >= TORPEDO_MAX_ACTIVE {
            continue;
        }

        cooldown.remaining = TORPEDO_COOLDOWN;

        let aim_dir = Vec2::new(input.0.aim_angle.cos(), input.0.aim_angle.sin());
        let spawn_pos = pos.0 + aim_dir * TORPEDO_MUZZLE_OFFSET;
        let torp_vel = lin_vel.0 + aim_dir * TORPEDO_SPEED;

        commands.spawn((
            Torpedo {
                damage: TORPEDO_DAMAGE,
                owner: player_id.0,
                owner_team: *team,
                lifetime: TORPEDO_LIFETIME,
            },
            Position(spawn_pos),
            LinearVelocity(torp_vel),
            Replicate::to_clients(NetworkTarget::All),
        ));
    }
}

/// Server steers torpedoes toward nearest enemy ship (limited turn rate).
fn server_torpedo_homing(
    mut torpedoes: Query<(&Torpedo, &Position, &mut LinearVelocity)>,
    enemies: Query<(&Position, &Team), With<Health>>,
) {
    let dt = FIXED_DT;

    for (torpedo, torp_pos, mut torp_vel) in torpedoes.iter_mut() {
        // Find nearest enemy
        let mut best_dist_sq = f32::MAX;
        let mut best_dir: Option<Vec2> = None;

        for (enemy_pos, enemy_team) in enemies.iter() {
            if *enemy_team == torpedo.owner_team {
                continue;
            }
            let delta = enemy_pos.0 - torp_pos.0;
            let dist_sq = delta.length_squared();
            if dist_sq < best_dist_sq && dist_sq > 1.0 {
                best_dist_sq = dist_sq;
                best_dir = Some(delta.normalize());
            }
        }

        let Some(desired_dir) = best_dir else {
            continue;
        };

        // Current direction
        let speed = torp_vel.0.length();
        if speed < 1.0 {
            continue;
        }
        let current_dir = torp_vel.0 / speed;

        // Rotate toward desired direction with limited turn rate
        let current_angle = current_dir.y.atan2(current_dir.x);
        let desired_angle = desired_dir.y.atan2(desired_dir.x);
        let diff = angle_diff(current_angle, desired_angle);
        let max_turn = TORPEDO_TURN_RATE * dt;
        let turn = diff.clamp(-max_turn, max_turn);
        let new_angle = current_angle + turn;

        torp_vel.0 = Vec2::new(new_angle.cos(), new_angle.sin()) * speed;
    }
}

/// Server railgun: charge while fire held, spawn fast projectile on release or full charge.
/// Breaks cloak on fire.
fn server_railgun(
    mut commands: Commands,
    mut ships: Query<(
        &lightyear::prelude::input::native::ActionState<ShipInput>,
        &PlayerId,
        &Team,
        &ShipClass,
        &Position,
        &LinearVelocity,
        &mut FireCooldown,
        &mut RailgunCharge,
        &mut Cloak,
    )>,
) {
    let dt = FIXED_DT;

    for (input, player_id, team, class, pos, lin_vel, mut cooldown, mut charge, mut cloak) in
        ships.iter_mut()
    {
        if *class != ShipClass::Sniper {
            continue;
        }

        // Can't charge while on cooldown
        if cooldown.remaining > 0.0 {
            charge.charge = 0.0;
            continue;
        }

        let mut should_fire = false;
        let mut damage_mult = 1.0;

        if input.0.fire {
            // Charging
            charge.charge = (charge.charge + dt / RAILGUN_CHARGE_TIME).min(1.0);

            // Auto-fire at full charge
            if charge.charge >= 1.0 {
                should_fire = true;
            }
        } else if charge.charge > 0.1 {
            // Released with partial charge
            should_fire = true;
            damage_mult = charge.charge;
        } else {
            // Released too early or not pressing — reset
            charge.charge = 0.0;
        }

        if should_fire {
            let aim_dir = Vec2::new(input.0.aim_angle.cos(), input.0.aim_angle.sin());
            let spawn_pos = pos.0 + aim_dir * MUZZLE_OFFSET;
            let proj_vel = lin_vel.0 + aim_dir * RAILGUN_SPEED;

            commands.spawn((
                Projectile {
                    damage: RAILGUN_DAMAGE * damage_mult,
                    owner: player_id.0,
                    owner_team: *team,
                    lifetime: RAILGUN_LIFETIME,
                },
                ProjectileKind::Railgun,
                Position(spawn_pos),
                LinearVelocity(proj_vel),
                Replicate::to_clients(NetworkTarget::All),
            ));

            cooldown.remaining = RAILGUN_COOLDOWN;
            charge.charge = 0.0;

            // Break cloak on fire
            if cloak.active {
                cloak.active = false;
                cloak.cooldown = CLOAK_COOLDOWN;
            }
        }
    }
}

/// Server cloak: toggle on afterburner input for Sniper, manage duration/cooldown.
fn server_cloak(
    mut query: Query<(
        &lightyear::prelude::input::native::ActionState<ShipInput>,
        &ShipClass,
        &mut Cloak,
    )>,
) {
    let dt = FIXED_DT;

    for (input, class, mut cloak) in query.iter_mut() {
        if *class != ShipClass::Sniper {
            continue;
        }

        // Tick cooldown
        if cloak.cooldown > 0.0 {
            cloak.cooldown = (cloak.cooldown - dt).max(0.0);
        }

        if cloak.active {
            // Tick duration
            cloak.duration -= dt;
            if cloak.duration <= 0.0 {
                cloak.active = false;
                cloak.duration = 0.0;
                cloak.cooldown = CLOAK_COOLDOWN;
            }
        }

        // Toggle on afterburner press (rising edge: just_pressed equivalent)
        // Since we get continuous state, detect transition by checking afterburner
        // and cloak not currently active + cooldown ready
        if input.0.afterburner && !cloak.active && cloak.cooldown <= 0.0 && cloak.duration <= 0.0 {
            cloak.active = true;
            cloak.duration = CLOAK_DURATION;
        }
    }
}

/// Normalize angle to [-PI, PI].
fn angle_diff(from: f32, to: f32) -> f32 {
    let d = (to - from) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d - std::f32::consts::TAU
    } else if d < -std::f32::consts::PI {
        d + std::f32::consts::TAU
    } else {
        d
    }
}

/// Auto-turret AI: track nearest enemy, slew toward target, fire when aligned.
/// Handles both Gunship (3 turrets) and DroneCommander (5 defense turrets).
fn server_turret_ai(
    mut commands: Commands,
    mut ships: Query<(
        &PlayerId,
        &Team,
        &ShipClass,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut Turrets,
    )>,
    enemies: Query<(&Position, &Team), With<Health>>,
    grids: Res<CollisionGrids>,
    mut candidates: Local<Vec<(Entity, Vec2)>>,
) {
    let dt = FIXED_DT;

    for (player_id, team, class, ship_pos, ship_rot, ship_vel, mut turrets) in ships.iter_mut() {
        if turrets.mounts.is_empty() {
            continue;
        }

        // Class-specific turret constants
        let is_dc = *class == ShipClass::DroneCommander;
        let (range, slew_rate, fire_tol, cd, speed, damage, lifetime) = if is_dc {
            (
                DEFENSE_TURRET_RANGE,
                DEFENSE_TURRET_SLEW_RATE,
                DEFENSE_TURRET_FIRE_TOLERANCE,
                DEFENSE_TURRET_COOLDOWN,
                DEFENSE_TURRET_SPEED,
                DEFENSE_TURRET_DAMAGE,
                DEFENSE_TURRET_LIFETIME,
            )
        } else {
            (
                TURRET_RANGE,
                TURRET_SLEW_RATE,
                TURRET_FIRE_TOLERANCE,
                TURRET_COOLDOWN,
                TURRET_SPEED,
                TURRET_DAMAGE,
                TURRET_LIFETIME,
            )
        };

        for (i, turret) in turrets.mounts.iter_mut().enumerate() {
            // Tick cooldown
            if turret.cooldown > 0.0 {
                turret.cooldown = (turret.cooldown - dt).max(0.0);
            }

            // Compute mount world position
            let mount_offset = if is_dc {
                DEFENSE_TURRET_MOUNTS.get(i).copied().unwrap_or(Vec2::ZERO)
            } else {
                TURRET_MOUNTS.get(i).copied().unwrap_or(Vec2::ZERO)
            };
            let mount_world = ship_pos.0 + *ship_rot * mount_offset;

            // Find nearest enemy in range via spatial grid.
            let mut best_dist_sq = range * range;
            let mut best_angle: Option<f32> = None;

            candidates.clear();
            grids.ships.for_each_candidate(mount_world, range, |e| candidates.push(e));

            for &(enemy_entity, _) in candidates.iter() {
                let Ok((enemy_pos, enemy_team)) = enemies.get(enemy_entity) else {
                    continue;
                };
                if *enemy_team == *team {
                    continue;
                }
                let delta = enemy_pos.0 - mount_world;
                let dist_sq = delta.length_squared();
                if dist_sq < best_dist_sq {
                    best_dist_sq = dist_sq;
                    best_angle = Some(delta.y.atan2(delta.x));
                }
            }

            // Slew toward target (or idle)
            if let Some(desired) = best_angle {
                let diff = angle_diff(turret.aim_angle, desired);
                let max_slew = slew_rate * dt;
                if diff.abs() <= max_slew {
                    turret.aim_angle = desired;
                } else {
                    turret.aim_angle += diff.signum() * max_slew;
                }

                // Fire if aligned and cooldown ready
                if diff.abs() < fire_tol && turret.cooldown <= 0.0 {
                    turret.cooldown = cd;

                    let aim_dir = Vec2::new(turret.aim_angle.cos(), turret.aim_angle.sin());
                    let spawn_pos = mount_world + aim_dir * 10.0;
                    let proj_vel = ship_vel.0 + aim_dir * speed;

                    commands.spawn((
                        Projectile {
                            damage,
                            owner: player_id.0,
                            owner_team: *team,
                            lifetime,
                        },
                        ProjectileKind::Turret,
                        Position(spawn_pos),
                        LinearVelocity(proj_vel),
                        Replicate::to_clients(NetworkTarget::All),
                    ));
                }
            }
        }
    }
}

/// Attach DroneSquad to newly spawned DroneCommander ships that don't have one yet.
fn server_init_drone_squads(
    mut commands: Commands,
    query: Query<(Entity, &ShipClass), Without<DroneSquad>>,
) {
    for (entity, class) in query.iter() {
        if *class == ShipClass::DroneCommander {
            commands.entity(entity).insert(DroneSquad { respawn_timer: 0.0 });
        }
    }
}

/// Spawn drones for DroneCommander ships up to max count. Also tick respawn timer.
fn server_drone_respawn(
    mut commands: Commands,
    mut commanders: Query<(&PlayerId, &Team, &Position, &mut DroneSquad)>,
    existing_drones: Query<&Drone>,
    mut counts: Local<std::collections::HashMap<PeerId, (usize, usize)>>,
) {
    let dt = FIXED_DT;

    // Build per-owner (laser, kamikaze) counts in one O(drones) pass.
    counts.clear();
    for d in existing_drones.iter() {
        let entry = counts.entry(d.owner).or_default();
        match d.kind {
            DroneKind::Laser => entry.0 += 1,
            DroneKind::Kamikaze => entry.1 += 1,
        }
    }

    for (player_id, team, pos, mut squad) in commanders.iter_mut() {
        let (laser_count, kamikaze_count) = counts.get(&player_id.0).copied().unwrap_or((0, 0));
        let active_count = laser_count + kamikaze_count;

        if active_count >= DRONE_MAX_COUNT {
            squad.respawn_timer = 0.0;
            continue;
        }

        squad.respawn_timer += dt;
        if squad.respawn_timer >= DRONE_RESPAWN_TIME {
            squad.respawn_timer = 0.0;

            // Respawn whichever type is most depleted
            let (kind, health) = if laser_count < DRONE_LASER_COUNT {
                (DroneKind::Laser, DRONE_LASER_HEALTH)
            } else {
                (DroneKind::Kamikaze, DRONE_KAMIKAZE_HEALTH)
            };

            let angle = (active_count as f32 * 0.9) + player_id.0.to_bits() as f32 * 0.1;
            let offset = Vec2::new(angle.cos(), angle.sin()) * DRONE_ORBIT_RADIUS;
            let spawn_pos = pos.0 + offset;

            commands.spawn((
                Drone {
                    owner: player_id.0,
                    owner_team: *team,
                    health,
                    kind,
                },
                Position(spawn_pos),
                LinearVelocity::default(),
                Replicate::to_clients(NetworkTarget::All),
            ));
        }
    }
}

/// Drone AI: chase nearest enemy within aggro range, or orbit the commander.
fn server_drone_ai(
    mut drones: Query<(Entity, &Drone, &Position, &mut LinearVelocity), Without<DroneSquad>>,
    enemies: Query<(&Position, &Team), With<Health>>,
    commanders: Query<(&PlayerId, &Position, &LinearVelocity), With<DroneSquad>>,
    time: Res<Time>,
    grids: Res<CollisionGrids>,
    mut enemy_candidates: Local<Vec<(Entity, Vec2)>>,
    mut cmd_cache: Local<std::collections::HashMap<PeerId, (Vec2, Vec2)>>,
) {
    let dt = FIXED_DT;
    let steer_rate = 6.0 * dt;
    let elapsed = time.elapsed_secs();

    // Build commander lookup once per tick — O(1) per drone vs O(commanders) each.
    cmd_cache.clear();
    for (pid, pos, vel) in commanders.iter() {
        cmd_cache.insert(pid.0, (pos.0, vel.0));
    }

    for (drone_entity, drone, drone_pos, mut drone_vel) in drones.iter_mut() {
        // Per-drone jitter from entity bits — each drone drifts differently
        let seed = drone_entity.to_bits().wrapping_mul(2654435761);
        let phase = (seed % 1000) as f32 * 0.001 * std::f32::consts::TAU;
        let jitter = Vec2::new(
            (elapsed * 1.3 + phase).sin() * 80.0,
            (elapsed * 1.7 + phase * 1.4).cos() * 80.0,
        );

        // Find nearest enemy within aggro range via spatial grid.
        let mut best_dist_sq = DRONE_AGGRO_RANGE * DRONE_AGGRO_RANGE;
        let mut nearest_enemy: Option<Vec2> = None;

        enemy_candidates.clear();
        grids.ships.for_each_candidate(drone_pos.0, DRONE_AGGRO_RANGE, |e| enemy_candidates.push(e));

        for &(enemy_entity, _) in enemy_candidates.iter() {
            let Ok((enemy_pos, enemy_team)) = enemies.get(enemy_entity) else {
                continue;
            };
            if *enemy_team == drone.owner_team {
                continue;
            }
            let dist_sq = (enemy_pos.0 - drone_pos.0).length_squared();
            if dist_sq < best_dist_sq {
                best_dist_sq = dist_sq;
                nearest_enemy = Some(enemy_pos.0);
            }
        }

        let commander_data = cmd_cache.get(&drone.owner).copied();

        let desired_vel = match drone.kind {
            DroneKind::Kamikaze => {
                if let Some(target) = nearest_enemy {
                    let delta = target - drone_pos.0;
                    if delta.length_squared() > 1.0 {
                        delta.normalize() * DRONE_KAMIKAZE_SPEED
                    } else {
                        drone_vel.0
                    }
                } else {
                    swarm_commander(drone_pos.0, commander_data, DRONE_SPEED, jitter)
                }
            }
            DroneKind::Laser => {
                if let Some(target) = nearest_enemy {
                    let delta = target - drone_pos.0;
                    const ENGAGE_DIST_SQ: f32 =
                        DRONE_LASER_RANGE * 0.8 * (DRONE_LASER_RANGE * 0.8);
                    if delta.length_squared() > ENGAGE_DIST_SQ {
                        let chase_dir = delta.normalize();
                        let base = commander_data.map(|(_, v)| v).unwrap_or(Vec2::ZERO);
                        base + chase_dir * DRONE_SPEED + jitter
                    } else {
                        // In range: swarm around target with jitter
                        let base = commander_data.map(|(_, v)| v).unwrap_or(Vec2::ZERO);
                        base + jitter * 2.0
                    }
                } else {
                    swarm_commander(drone_pos.0, commander_data, DRONE_SPEED, jitter)
                }
            }
        };

        drone_vel.0 = drone_vel.0.lerp(desired_vel, steer_rate);
    }
}

/// Swarm around commander with random jitter instead of deterministic orbit.
fn swarm_commander(
    drone_pos: Vec2,
    commander: Option<(Vec2, Vec2)>,
    speed: f32,
    jitter: Vec2,
) -> Vec2 {
    if let Some((cmd_pos, cmd_vel)) = commander {
        let to_cmd = cmd_pos - drone_pos;
        const CHASE_DIST_SQ: f32 =
            DRONE_ORBIT_RADIUS * 2.0 * (DRONE_ORBIT_RADIUS * 2.0);
        if to_cmd.length_squared() > CHASE_DIST_SQ {
            cmd_vel + to_cmd.normalize() * speed
        } else {
            let pull = to_cmd * 0.5;
            cmd_vel + (jitter + pull).clamp_length_max(speed * 0.6)
        }
    } else {
        jitter.clamp_length_max(speed * 0.3)
    }
}

/// Anti-drone pulse: DroneCommander presses drop_mine to detonate all nearby drones.
/// Each drone explodes, dealing area damage to nearby enemy ships.
fn server_anti_drone_pulse(
    mut commands: Commands,
    mut query: Query<(
        &lightyear::prelude::input::native::ActionState<ShipInput>,
        &ShipClass,
        &Position,
        &Team,
        &mut MineCooldown,
    )>,
    drones: Query<(Entity, &Position, &Drone)>,
    zone_drones: Query<(Entity, &Position), With<ZoneDrone>>,
    mut ships: Query<(&Position, &Team, &SpawnProtection, &mut Health, &mut DamageFlash, &mut LastDamagedBy)>,
) {
    let pulse_dist_sq = PULSE_RADIUS * PULSE_RADIUS;
    let blast_dist_sq = DRONE_DETONATION_RADIUS * DRONE_DETONATION_RADIUS;

    for (input, class, pos, team, mut cooldown) in query.iter_mut() {
        if *class != ShipClass::DroneCommander {
            continue;
        }
        if !input.0.drop_mine || cooldown.remaining > 0.0 {
            continue;
        }

        cooldown.remaining = PULSE_COOLDOWN;

        // Destroy player drones in range
        for (drone_entity, drone_pos, drone) in drones.iter() {
            if (drone_pos.0 - pos.0).length_squared() < pulse_dist_sq {
                for (ship_pos, ship_team, sp, mut health, mut flash, mut last_hit) in ships.iter_mut() {
                    if *ship_team == *team || sp.remaining > 0.0 {
                        continue;
                    }
                    if (drone_pos.0 - ship_pos.0).length_squared() < blast_dist_sq {
                        health.current = (health.current - DRONE_DETONATION_DAMAGE).max(0.0);
                        last_hit.attacker = Some(drone.owner);
                        flash.timer = DAMAGE_FLASH_DURATION;
                    }
                }
                commands.entity(drone_entity).try_despawn();
            }
        }

        // Also destroy zone defense drones in range
        for (drone_entity, drone_pos) in zone_drones.iter() {
            if (drone_pos.0 - pos.0).length_squared() < pulse_dist_sq {
                commands.entity(drone_entity).try_despawn();
            }
        }
    }
}

/// Spawn the replicated TeamScores entity.
fn spawn_scores(mut commands: Commands) {
    commands.spawn((
        TeamScores::default(),
        Replicate::to_clients(NetworkTarget::All),
    ));
    info!("Spawned TeamScores entity (score limit: {SCORE_LIMIT})");
}

/// Diminishing returns multiplier for capture rate based on ship count.
fn capture_speed_mult(ships: u32) -> f32 {
    match ships {
        0 => 0.0,
        1 => 1.0,
        2 => 1.5,
        3 => 1.8,
        _ => 2.0,
    }
}

/// King-of-the-hill: gradual capture progress + score from controlled zones.
fn update_zone_scores(
    mut scores_q: Query<&mut TeamScores>,
    ships: Query<(&Position, &Team), With<Health>>,
) {
    let Ok(mut scores) = scores_q.single_mut() else {
        return;
    };

    if !matches!(scores.round_state, RoundState::Playing) {
        return;
    }

    let dt = FIXED_DT;
    let zones = objective_zone_positions();
    let r2 = OBJECTIVE_ZONE_RADIUS * OBJECTIVE_ZONE_RADIUS;

    for (i, center) in zones.iter().enumerate() {
        let mut red = 0u32;
        let mut blue = 0u32;

        for (pos, team) in ships.iter() {
            if (pos.0 - *center).length_squared() <= r2 {
                match team {
                    Team::Red => red += 1,
                    Team::Blue => blue += 1,
                }
            }
        }

        let zone = &mut scores.zones[i];

        // Progress: negative = Red capturing, positive = Blue capturing
        // -1.0 = fully Red, 0.0 = neutral, 1.0 = fully Blue
        if red > blue {
            let rate = CAPTURE_RATE * capture_speed_mult(red - blue);
            zone.progress = (zone.progress - rate * dt).max(-1.0);
        } else if blue > red {
            let rate = CAPTURE_RATE * capture_speed_mult(blue - red);
            zone.progress = (zone.progress + rate * dt).min(1.0);
        } else if red == 0 {
            // Empty zone: drift toward neutral
            if zone.progress > 0.0 {
                zone.progress = (zone.progress - DECAP_RATE * dt).max(0.0);
            } else if zone.progress < 0.0 {
                zone.progress = (zone.progress + DECAP_RATE * dt).min(0.0);
            }
        }
        // Contested (equal non-zero) = frozen (no change)

        // Update controller based on progress
        if zone.progress <= -1.0 {
            zone.controller = 1; // Red
        } else if zone.progress >= 1.0 {
            zone.controller = 2; // Blue
        } else if zone.progress.abs() < 0.01 {
            zone.controller = 0; // Neutral
        }
        // Otherwise keep current controller (partially decapped but not flipped)

        // Score from controlled zones
        match zone.controller {
            1 => scores.red += ZONE_SCORE_RATE * dt,
            2 => scores.blue += ZONE_SCORE_RATE * dt,
            _ => {}
        }
    }

    scores.red = scores.red.min(SCORE_LIMIT);
    scores.blue = scores.blue.min(SCORE_LIMIT);

    if scores.red >= SCORE_LIMIT {
        scores.round_state = RoundState::Won(Team::Red);
    } else if scores.blue >= SCORE_LIMIT {
        scores.round_state = RoundState::Won(Team::Blue);
    }
}

/// Round management: Won → display timer → Restarting countdown → reset to Playing.
fn round_management(
    mut scores_q: Query<&mut TeamScores>,
    mut commands: Commands,
    mut timers: ResMut<ZoneDefenseTimers>,
    mut match_stats: ResMut<MatchStats>,
    mut server_phase: ResMut<ServerGamePhase>,
    zone_drones: Query<Entity, With<ZoneDrone>>,
    zone_railguns: Query<Entity, With<ZoneRailgun>>,
    zone_shields: Query<Entity, With<ZoneShield>>,
    projectiles: Query<Entity, With<Projectile>>,
    mines: Query<Entity, With<Mine>>,
    torpedoes: Query<Entity, With<Torpedo>>,
    player_drones: Query<Entity, With<Drone>>,
    mut ships: Query<(&ShipClass, &mut Health, &mut Fuel, &mut Ammo, &mut SpawnProtection)>,
) {
    let Ok(mut scores) = scores_q.single_mut() else {
        return;
    };

    let dt = FIXED_DT;

    match scores.round_state {
        RoundState::Playing => {}
        RoundState::Won(winner) => {
            // Snapshot per-player kill stats for the victory screen
            let mut end_stats: Vec<PlayerStat> = match_stats.kills.iter()
                .map(|(peer, (team, kills))| PlayerStat { peer_id: *peer, team: *team, kills: *kills })
                .collect();
            end_stats.sort_by(|a, b| b.kills.cmp(&a.kills));
            scores.end_stats = end_stats;
            scores.last_winner = Some(winner);
            scores.kill_feed.clear();
            *match_stats = MatchStats::default();

            scores.round_state = RoundState::Restarting(ROUND_END_DISPLAY_TIME + ROUND_RESTART_COUNTDOWN);
        }
        RoundState::Restarting(remaining) => {
            let new_remaining = remaining - dt;
            if new_remaining <= 0.0 {
                // Reset everything
                scores.red = 0.0;
                scores.blue = 0.0;
                scores.zones = [ZoneState::default(); 3];
                scores.end_stats.clear();
                scores.last_winner = None;
                scores.round_state = RoundState::Playing;
                // Return to lobby; players must re-ready for the next round.
                scores.lobby_phase = LobbyPhase::Lobby;
                scores.lobby_roster.iter_mut().for_each(|e| e.ready = false);
                *server_phase = ServerGamePhase::Lobby;

                // Reset defense timers
                *timers = ZoneDefenseTimers::default();

                // Despawn all transient entities
                for entity in zone_drones.iter()
                    .chain(zone_railguns.iter())
                    .chain(zone_shields.iter())
                    .chain(projectiles.iter())
                    .chain(mines.iter())
                    .chain(torpedoes.iter())
                    .chain(player_drones.iter())
                {
                    commands.entity(entity).try_despawn();
                }

                // Reset all ships: full health, fuel, ammo + spawn protection
                for (class, mut health, mut fuel, mut ammo, mut sp) in ships.iter_mut() {
                    health.current = ship_max_health(class);
                    fuel.current = fuel.max;
                    ammo.current = ammo.max;
                    sp.remaining = btl_shared::SPAWN_PROTECTION_DURATION;
                }
            } else {
                scores.round_state = RoundState::Restarting(new_remaining);
            }
        }
    }
}

/// Zone benefits: ships inside a friendly-controlled zone get HP regen and boosted ammo/fuel regen.
/// Drone Commanders also get faster drone respawn.
fn zone_benefits(
    scores_q: Query<&TeamScores>,
    mut ships: Query<(&Position, &Team, &ShipClass, &mut Health, &mut Fuel, &mut Ammo)>,
    mut squads: Query<(&Position, &Team, &mut DroneSquad)>,
) {
    let Ok(scores) = scores_q.single() else {
        return;
    };

    let dt = FIXED_DT;
    let zones = objective_zone_positions();
    let r2 = OBJECTIVE_ZONE_RADIUS * OBJECTIVE_ZONE_RADIUS;

    // Helper: check if position is inside a zone controlled by the given team
    let team_code = |team: &Team| -> u8 {
        match team {
            Team::Red => 1,
            Team::Blue => 2,
        }
    };

    let in_friendly_zone = |pos: &Position, team: &Team| -> bool {
        let code = team_code(team);
        for (i, center) in zones.iter().enumerate() {
            if scores.zones[i].controller == code
                && (pos.0 - *center).length_squared() <= r2
            {
                return true;
            }
        }
        false
    };

    for (pos, team, class, mut health, mut fuel, mut ammo) in ships.iter_mut() {
        if !in_friendly_zone(pos, team) {
            continue;
        }

        // HP regen
        if health.current < health.max && health.current > 0.0 {
            health.current = (health.current + ZONE_HP_REGEN * dt).min(health.max);
        }

        // Bonus ammo regen (ZONE_REGEN_MULT - 1.0 to add on top of passive)
        let bonus_ammo = ship_ammo_regen(class) * (ZONE_REGEN_MULT - 1.0);
        if ammo.current < ammo.max {
            ammo.current = (ammo.current + bonus_ammo * dt).min(ammo.max);
        }

        // Bonus fuel regen
        let bonus_fuel = FUEL_REGEN_RATE * (ZONE_REGEN_MULT - 1.0);
        if fuel.current < fuel.max {
            fuel.current = (fuel.current + bonus_fuel * dt).min(fuel.max);
        }
    }

    // Drone Commander: faster respawn in friendly zone (halved timer)
    for (pos, team, mut squad) in squads.iter_mut() {
        if in_friendly_zone(pos, team) {
            // Add extra tick to respawn timer (effectively doubles respawn speed)
            squad.respawn_timer += dt;
        }
    }
}

/// Apply damage from ship-ship and ship-asteroid collisions based on relative velocity.
fn collision_damage(
    mut ships: Query<(
        Entity,
        &Position,
        &ShipClass,
        &LinearVelocity,
        &SpawnProtection,
        &mut Health,
        &mut DamageFlash,
    )>,
    grids: Res<CollisionGrids>,
    mut ast_candidates: Local<Vec<(Vec2, f32)>>,
) {
    // Ship-asteroid collisions
    let mut ast_hits: Vec<(Entity, f32)> = Vec::with_capacity(8);
    for (entity, pos, class, vel, sp, health, _) in ships.iter() {
        if sp.remaining > 0.0 || health.current <= 0.0 {
            continue;
        }
        let r = ship_radius(class);
        let speed_sq = vel.0.length_squared();
        if speed_sq < COLLISION_DAMAGE_VELOCITY_THRESHOLD * COLLISION_DAMAGE_VELOCITY_THRESHOLD {
            continue;
        }
        let speed = speed_sq.sqrt();
        ast_candidates.clear();
        grids.asteroids.for_each_candidate(pos.0, r + MAX_ASTEROID_RADIUS, |e| ast_candidates.push(e));
        for &(ast_pos, ast_radius) in ast_candidates.iter() {
            let hit_dist = r + ast_radius + 2.0;
            if (pos.0 - ast_pos).length_squared() < hit_dist * hit_dist {
                let damage = (speed - COLLISION_DAMAGE_VELOCITY_THRESHOLD) * COLLISION_DAMAGE_PER_VELOCITY;
                ast_hits.push((entity, damage));
                break;
            }
        }
    }
    for (entity, damage) in ast_hits {
        if let Ok((_, _, _, _, _, mut health, mut flash)) = ships.get_mut(entity) {
            health.current = (health.current - damage).max(0.0);
            flash.timer = DAMAGE_FLASH_DURATION;
        }
    }

    // Ship-ship collisions — e1 >= e2 guard guarantees each pair is seen at most once.
    let mut ship_hits: Vec<(Entity, f32)> = Vec::with_capacity(16);
    for (e1, p1, c1, v1, sp1, h1, _) in ships.iter() {
        if sp1.remaining > 0.0 || h1.current <= 0.0 {
            continue;
        }
        let r1 = ship_radius(c1);
        for (e2, p2, c2, v2, sp2, h2, _) in ships.iter() {
            if e1 >= e2 || sp2.remaining > 0.0 || h2.current <= 0.0 {
                continue;
            }
            let r2 = ship_radius(c2);
            let hit_dist = r1 + r2 + 2.0;
            if (p1.0 - p2.0).length_squared() < hit_dist * hit_dist {
                let rel_speed = (v1.0 - v2.0).length();
                if rel_speed < COLLISION_DAMAGE_VELOCITY_THRESHOLD {
                    continue;
                }
                let base_damage = (rel_speed - COLLISION_DAMAGE_VELOCITY_THRESHOLD) * COLLISION_DAMAGE_PER_VELOCITY;
                let (d1, d2) = if v1.0.length_squared() > v2.0.length_squared() {
                    (base_damage * COLLISION_FASTER_SHIP_MULT, base_damage)
                } else {
                    (base_damage, base_damage * COLLISION_FASTER_SHIP_MULT)
                };
                ship_hits.push((e1, d1));
                ship_hits.push((e2, d2));
            }
        }
    }
    for (entity, damage) in ship_hits {
        if let Ok((_, _, _, _, _, mut health, mut flash)) = ships.get_mut(entity) {
            health.current = (health.current - damage).max(0.0);
            flash.timer = DAMAGE_FLASH_DURATION;
        }
    }
}

/// Spawn initial batch of drones when a DroneCommander first appears.
fn server_spawn_initial_drones(
    mut commands: Commands,
    query: Query<(&PlayerId, &Team, &Position, &DroneSquad), Added<DroneSquad>>,
) {
    for (player_id, team, pos, _squad) in query.iter() {
        for i in 0..DRONE_MAX_COUNT {
            let angle = i as f32 * std::f32::consts::TAU / DRONE_MAX_COUNT as f32;
            let offset = Vec2::new(angle.cos(), angle.sin()) * DRONE_ORBIT_RADIUS;
            let spawn_pos = pos.0 + offset;

            let (kind, health) = if i < DRONE_LASER_COUNT {
                (DroneKind::Laser, DRONE_LASER_HEALTH)
            } else {
                (DroneKind::Kamikaze, DRONE_KAMIKAZE_HEALTH)
            };

            commands.spawn((
                Drone {
                    owner: player_id.0,
                    owner_team: *team,
                    health,
                    kind,
                },
                Position(spawn_pos),
                LinearVelocity::default(),
                Replicate::to_clients(NetworkTarget::All),
            ));
        }
    }
}

// ============================================================
// Objective Defense Systems
// ============================================================

/// Manage factory defense drones: spawn when zone is captured, despawn when lost, respawn killed.
fn zone_factory_drones(
    mut commands: Commands,
    scores_q: Query<&TeamScores>,
    mut timers: ResMut<ZoneDefenseTimers>,
    existing_drones: Query<(Entity, &ZoneDrone)>,
) {
    let Ok(scores) = scores_q.single() else {
        return;
    };

    let dt = FIXED_DT;
    let zones = objective_zone_positions();

    for (i, kind) in OBJECTIVE_KINDS.iter().enumerate() {
        if !matches!(kind, ObjectiveKind::Factory) {
            continue;
        }

        let controller = scores.zones[i].controller;

        // Detect controller flip — despawn old drones
        if controller != timers.last_controller[i] {
            for (entity, drone) in existing_drones.iter() {
                if drone.zone_index == i as u8 {
                    commands.entity(entity).try_despawn();
                }
            }
            timers.respawn_timers[i] = 0.0;
            timers.last_controller[i] = controller;

            // Immediately spawn full complement if newly captured
            if controller != 0 {
                let team = if controller == 1 { Team::Red } else { Team::Blue };
                spawn_factory_drones(&mut commands, i, zones[i], team, FACTORY_LASER_DRONES + FACTORY_KAMIKAZE_DRONES);
            }
            continue;
        }

        // No controller = no drones
        if controller == 0 {
            continue;
        }

        let team = if controller == 1 { Team::Red } else { Team::Blue };

        // Count existing drones for this zone
        let mut count = 0usize;
        for (_entity, drone) in existing_drones.iter() {
            if drone.zone_index == i as u8 {
                count += 1;
            }
        }

        let total = FACTORY_LASER_DRONES + FACTORY_KAMIKAZE_DRONES;
        if count < total {
            timers.respawn_timers[i] += dt;
            if timers.respawn_timers[i] >= FACTORY_DRONE_RESPAWN_TIME {
                timers.respawn_timers[i] = 0.0;
                spawn_factory_drones(&mut commands, i, zones[i], team, 1);
            }
        } else {
            timers.respawn_timers[i] = 0.0;
        }
    }
}

fn spawn_factory_drones(commands: &mut Commands, zone_index: usize, center: Vec2, team: Team, count: usize) {
    for n in 0..count {
        let angle = n as f32 * std::f32::consts::TAU / (FACTORY_LASER_DRONES + FACTORY_KAMIKAZE_DRONES) as f32;
        let offset = Vec2::new(angle.cos(), angle.sin()) * FACTORY_DRONE_ORBIT_RADIUS * 0.5;
        let spawn_pos = center + offset;

        let kind = if n < FACTORY_LASER_DRONES {
            DroneKind::Laser
        } else {
            DroneKind::Kamikaze
        };

        commands.spawn((
            ZoneDrone {
                zone_index: zone_index as u8,
                team,
                kind,
                health: FACTORY_DRONE_HEALTH,
            },
            Position(spawn_pos),
            LinearVelocity::default(),
            Replicate::to_clients(NetworkTarget::All),
        ));
    }
}

/// AI for factory defense drones: orbit zone center, aggro on enemies.
fn zone_factory_drone_ai(
    mut drones: Query<(Entity, &ZoneDrone, &Position, &mut LinearVelocity)>,
    enemies: Query<(&Position, &Team, &SpawnProtection), With<Health>>,
    grids: Res<CollisionGrids>,
    mut candidates: Local<Vec<(Entity, Vec2)>>,
    time: Res<Time>,
) {
    let dt = FIXED_DT;
    let steer_rate = 5.0 * dt;
    let elapsed = time.elapsed_secs();
    let zones = objective_zone_positions();

    for (entity, drone, drone_pos, mut drone_vel) in drones.iter_mut() {
        let zone_center = zones[drone.zone_index as usize];

        // Per-drone jitter
        let seed = entity.to_bits().wrapping_mul(2654435761);
        let phase = (seed % 1000) as f32 * 0.001 * std::f32::consts::TAU;
        let jitter = Vec2::new(
            (elapsed * 1.3 + phase).sin() * 60.0,
            (elapsed * 1.7 + phase * 1.4).cos() * 60.0,
        );

        // Find nearest enemy within aggro range via spatial grid.
        let mut best_dist_sq = FACTORY_DRONE_AGGRO_RANGE * FACTORY_DRONE_AGGRO_RANGE;
        let mut nearest_enemy: Option<Vec2> = None;

        candidates.clear();
        grids.ships.for_each_candidate(drone_pos.0, FACTORY_DRONE_AGGRO_RANGE, |e| candidates.push(e));
        for &(enemy_entity, _) in candidates.iter() {
            let Ok((enemy_pos, enemy_team, sp)) = enemies.get(enemy_entity) else { continue; };
            if *enemy_team == drone.team || sp.remaining > 0.0 {
                continue;
            }
            let dist_sq = (enemy_pos.0 - drone_pos.0).length_squared();
            if dist_sq < best_dist_sq {
                best_dist_sq = dist_sq;
                nearest_enemy = Some(enemy_pos.0);
            }
        }

        let desired_vel = match drone.kind {
            DroneKind::Kamikaze => {
                if let Some(target) = nearest_enemy {
                    let delta = target - drone_pos.0;
                    if delta.length_squared() > 1.0 {
                        delta.normalize() * FACTORY_DRONE_SPEED * 1.3
                    } else {
                        drone_vel.0
                    }
                } else {
                    orbit_zone(drone_pos.0, zone_center, FACTORY_DRONE_SPEED, jitter)
                }
            }
            DroneKind::Laser => {
                if let Some(target) = nearest_enemy {
                    let delta = target - drone_pos.0;
                    let dist_sq = delta.length_squared();
                    const ENGAGE_DIST_SQ: f32 =
                        FACTORY_DRONE_LASER_RANGE * 0.8 * (FACTORY_DRONE_LASER_RANGE * 0.8);
                    if dist_sq > ENGAGE_DIST_SQ {
                        let chase_dir = delta / dist_sq.sqrt();
                        chase_dir * FACTORY_DRONE_SPEED + jitter
                    } else {
                        jitter * 2.0
                    }
                } else {
                    orbit_zone(drone_pos.0, zone_center, FACTORY_DRONE_SPEED, jitter)
                }
            }
        };

        drone_vel.0 = drone_vel.0.lerp(desired_vel, steer_rate);
    }
}

/// Orbit around a zone center with jitter.
fn orbit_zone(drone_pos: Vec2, center: Vec2, speed: f32, jitter: Vec2) -> Vec2 {
    let to_center = center - drone_pos;
    const CHASE_DIST_SQ: f32 =
        FACTORY_DRONE_ORBIT_RADIUS * 2.0 * (FACTORY_DRONE_ORBIT_RADIUS * 2.0);
    if to_center.length_squared() > CHASE_DIST_SQ {
        to_center.normalize() * speed
    } else {
        let pull = to_center * 0.5;
        (jitter + pull).clamp_length_max(speed * 0.6)
    }
}

/// Factory drone laser damage: zone drones with laser kind deal DPS to nearby enemies.
fn zone_factory_drone_laser(
    drones: Query<(&ZoneDrone, &Position)>,
    mut enemies: Query<(Entity, &Position, &Team, &SpawnProtection, &mut Health, &mut DamageFlash)>,
    grids: Res<CollisionGrids>,
    mut candidates: Local<Vec<(Entity, Vec2)>>,
) {
    let dt = FIXED_DT;
    let range_sq = FACTORY_DRONE_LASER_RANGE * FACTORY_DRONE_LASER_RANGE;

    for (drone, drone_pos) in drones.iter() {
        if !matches!(drone.kind, DroneKind::Laser) {
            continue;
        }

        // Find the nearest enemy entity in range via spatial grid.
        let mut best_dist_sq = range_sq;
        let mut best_entity: Option<Entity> = None;

        candidates.clear();
        grids.ships.for_each_candidate(drone_pos.0, FACTORY_DRONE_LASER_RANGE, |e| candidates.push(e));
        for &(entity, _) in candidates.iter() {
            let Ok((_, enemy_pos, enemy_team, sp, _, _)) = enemies.get(entity) else { continue; };
            if *enemy_team == drone.team || sp.remaining > 0.0 {
                continue;
            }
            let dist_sq = (enemy_pos.0 - drone_pos.0).length_squared();
            if dist_sq < best_dist_sq {
                best_dist_sq = dist_sq;
                best_entity = Some(entity);
            }
        }

        if let Some(entity) = best_entity {
            if let Ok((_, _, _, sp, mut health, mut flash)) = enemies.get_mut(entity) {
                if sp.remaining <= 0.0 {
                    health.current -= FACTORY_DRONE_LASER_DPS * dt;
                    flash.timer = DAMAGE_FLASH_DURATION;
                }
            }
        }
    }
}

/// Factory kamikaze drone detonation: explode on contact with enemy ships.
fn zone_factory_drone_kamikaze(
    mut commands: Commands,
    drones: Query<(Entity, &ZoneDrone, &Position)>,
    mut enemies: Query<(Entity, &Position, &Team, &ShipClass, &SpawnProtection, &mut Health, &mut DamageFlash)>,
    grids: Res<CollisionGrids>,
    mut candidates: Local<Vec<(Entity, Vec2)>>,
) {
    const ZONE_DRONE_RADIUS: f32 = 8.0;
    for (drone_entity, drone, drone_pos) in drones.iter() {
        if !matches!(drone.kind, DroneKind::Kamikaze) {
            continue;
        }

        candidates.clear();
        grids.ships.for_each_candidate(drone_pos.0, MAX_SHIP_RADIUS + ZONE_DRONE_RADIUS, |e| candidates.push(e));
        for &(enemy_entity, _) in candidates.iter() {
            let Ok((_, enemy_pos, enemy_team, class, sp, mut health, mut flash)) =
                enemies.get_mut(enemy_entity)
            else {
                continue;
            };
            if *enemy_team == drone.team || sp.remaining > 0.0 {
                continue;
            }
            let hit_dist = ship_radius(class) + ZONE_DRONE_RADIUS;
            if (enemy_pos.0 - drone_pos.0).length_squared() < hit_dist * hit_dist {
                health.current -= FACTORY_DRONE_KAMIKAZE_DAMAGE;
                flash.timer = DAMAGE_FLASH_DURATION;
                commands.entity(drone_entity).try_despawn();
                break;
            }
        }
    }
}

/// Destroy zone drones when their health reaches zero (hit by projectiles/pulses).
fn zone_drone_death(
    mut commands: Commands,
    drones: Query<(Entity, &ZoneDrone)>,
) {
    for (entity, drone) in drones.iter() {
        if drone.health <= 0.0 {
            commands.entity(entity).try_despawn();
        }
    }
}

/// Railgun turret AI: track nearest enemy, telegraph, fire.
fn zone_railgun_ai(
    mut commands: Commands,
    mut turrets: Query<(&mut ZoneRailgun, &Position)>,
    enemies: Query<(&Position, &Team, &SpawnProtection), With<Health>>,
    grids: Res<CollisionGrids>,
    mut candidates: Local<Vec<(Entity, Vec2)>>,
) {
    let dt = FIXED_DT;

    for (mut turret, turret_pos) in turrets.iter_mut() {
        let range_sq = ZONE_RAILGUN_RANGE * ZONE_RAILGUN_RANGE;

        // Find nearest enemy via spatial grid.
        let mut best_dist_sq = range_sq;
        let mut nearest_target: Option<(Vec2, f32)> = None;

        candidates.clear();
        grids.ships.for_each_candidate(turret_pos.0, ZONE_RAILGUN_RANGE, |e| candidates.push(e));

        for &(enemy_entity, _) in candidates.iter() {
            let Ok((enemy_pos, enemy_team, sp)) = enemies.get(enemy_entity) else {
                continue;
            };
            let is_enemy = match turret.team {
                Team::Red => *enemy_team == Team::Blue,
                Team::Blue => *enemy_team == Team::Red,
            };
            if !is_enemy || sp.remaining > 0.0 {
                continue;
            }
            let dist_sq = (enemy_pos.0 - turret_pos.0).length_squared();
            if dist_sq < best_dist_sq {
                best_dist_sq = dist_sq;
                let angle = (enemy_pos.0 - turret_pos.0).to_angle();
                nearest_target = Some((enemy_pos.0, angle));
            }
        }

        match turret.state {
            RailgunTurretState::Idle => {
                if let Some((_target_pos, target_angle)) = nearest_target {
                    turret.aim_angle = target_angle;
                    turret.charge = 0.0;
                    turret.state = RailgunTurretState::Tracking;
                }
            }
            RailgunTurretState::Tracking => {
                if let Some((_target_pos, target_angle)) = nearest_target {
                    // Slew toward target
                    let mut diff = target_angle - turret.aim_angle;
                    if diff > std::f32::consts::PI { diff -= std::f32::consts::TAU; }
                    if diff < -std::f32::consts::PI { diff += std::f32::consts::TAU; }
                    let max_slew = ZONE_RAILGUN_SLEW_RATE * dt;
                    turret.aim_angle += diff.clamp(-max_slew, max_slew);

                    // Build charge
                    turret.charge = (turret.charge + dt / ZONE_RAILGUN_CHARGE_TIME).min(1.0);

                    // If fully charged and on-target, lock
                    if turret.charge >= 1.0 && diff.abs() < 0.1 {
                        turret.state = RailgunTurretState::Locked(ZONE_RAILGUN_LOCK_TIME);
                    }
                } else {
                    // Lost target
                    turret.charge = (turret.charge - dt * 2.0).max(0.0);
                    if turret.charge <= 0.0 {
                        turret.state = RailgunTurretState::Idle;
                    }
                }
            }
            RailgunTurretState::Locked(remaining) => {
                let new_remaining = remaining - dt;
                if new_remaining <= 0.0 {
                    // Fire!
                    let dir = Vec2::new(turret.aim_angle.cos(), turret.aim_angle.sin());
                    let spawn_pos = turret_pos.0 + dir * 20.0;
                    commands.spawn((
                        Projectile {
                            damage: ZONE_RAILGUN_DAMAGE,
                            owner: PeerId::Server,
                            owner_team: turret.team,
                            lifetime: ZONE_RAILGUN_PROJECTILE_LIFETIME,
                        },
                        ProjectileKind::Railgun,
                        Position(spawn_pos),
                        LinearVelocity(dir * ZONE_RAILGUN_PROJECTILE_SPEED),
                        Replicate::to_clients(NetworkTarget::All),
                    ));

                    turret.charge = 0.0;
                    turret.cooldown = ZONE_RAILGUN_COOLDOWN;
                    turret.state = RailgunTurretState::Cooldown;
                } else {
                    turret.state = RailgunTurretState::Locked(new_remaining);
                }
            }
            RailgunTurretState::Cooldown => {
                turret.cooldown -= dt;
                if turret.cooldown <= 0.0 {
                    turret.state = RailgunTurretState::Idle;
                }
            }
        }
    }
}

/// Manage railgun turret and shield entities: spawn/despawn based on zone control.
fn zone_defense_management(
    mut commands: Commands,
    scores_q: Query<&TeamScores>,
    existing_railguns: Query<(Entity, &ZoneRailgun)>,
    existing_shields: Query<(Entity, &ZoneShield)>,
    mut timers: ResMut<ZoneDefenseTimers>,
) {
    let Ok(scores) = scores_q.single() else {
        return;
    };
    let zones = objective_zone_positions();

    for (i, kind) in OBJECTIVE_KINDS.iter().enumerate() {
        let controller = scores.zones[i].controller;
        let changed = controller != timers.last_controller[i];

        // Factory drones are handled by zone_factory_drones which also updates last_controller.
        // For non-Factory zones, we need to track last_controller here.
        if matches!(kind, ObjectiveKind::Factory) {
            continue;
        }

        if !changed {
            continue;
        }

        timers.last_controller[i] = controller;

        match kind {
            ObjectiveKind::Railgun => {
                for (entity, rg) in existing_railguns.iter() {
                    if rg.zone_index == i as u8 {
                        commands.entity(entity).try_despawn();
                    }
                }
                if controller != 0 {
                    let team = if controller == 1 { Team::Red } else { Team::Blue };
                    commands.spawn((
                        ZoneRailgun {
                            zone_index: i as u8,
                            team,
                            aim_angle: 0.0,
                            charge: 0.0,
                            cooldown: 0.0,
                            state: RailgunTurretState::Idle,
                        },
                        Position(zones[i]),
                        Replicate::to_clients(NetworkTarget::All),
                    ));
                }
            }
            ObjectiveKind::Powerplant => {
                for (entity, shield) in existing_shields.iter() {
                    if shield.zone_index == i as u8 {
                        commands.entity(entity).try_despawn();
                    }
                }
                if controller != 0 {
                    let team = if controller == 1 { Team::Red } else { Team::Blue };
                    commands.spawn((
                        ZoneShield {
                            zone_index: i as u8,
                            team,
                            active: true,
                        },
                        Position(zones[i]),
                        Replicate::to_clients(NetworkTarget::All),
                    ));
                }
            }
            ObjectiveKind::Factory => unreachable!(),
        }
    }
}

/// Powerplant shield: deflect/destroy enemy projectiles, detonate torpedoes.
fn zone_shield_deflect(
    mut commands: Commands,
    shields: Query<(&ZoneShield, &Position)>,
    mut projectiles: Query<(Entity, &Projectile, &Position, &mut LinearVelocity)>,
    torpedoes: Query<(Entity, &Torpedo, &Position)>,
) {
    let r2 = ZONE_SHIELD_RADIUS * ZONE_SHIELD_RADIUS;

    for (shield, shield_pos) in shields.iter() {
        if !shield.active {
            continue;
        }

        // Deflect enemy projectiles
        for (_proj_entity, proj, proj_pos, mut proj_vel) in projectiles.iter_mut() {
            if proj.owner_team == shield.team {
                continue;
            }
            let dist_sq = (proj_pos.0 - shield_pos.0).length_squared();
            if dist_sq < r2 {
                let normal = (proj_pos.0 - shield_pos.0).normalize_or_zero();
                let dot = proj_vel.0.dot(normal);
                if dot < 0.0 {
                    proj_vel.0 -= 2.0 * dot * normal;
                }
            }
        }

        // Detonate enemy torpedoes on shield contact
        for (torp_entity, torp, torp_pos) in torpedoes.iter() {
            if torp.owner_team == shield.team {
                continue;
            }
            let dist_sq = (torp_pos.0 - shield_pos.0).length_squared();
            if dist_sq < r2 {
                commands.entity(torp_entity).try_despawn();
            }
        }
    }
}
