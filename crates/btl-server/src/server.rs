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
    generate_asteroid_layout, objective_zone_positions, ray_circle_intersect,
};

/// Server-only component tracking drone squad state for Drone Commander ships.
#[derive(Component)]
struct DroneSquad {
    pub respawn_timer: f32,
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

const RESPAWN_DELAY: f32 = 3.0;

pub struct ServerPlugin;

impl Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(lightyear::prelude::server::ServerPlugins {
            tick_duration: Duration::from_secs_f64(1.0 / FIXED_TIMESTEP_HZ),
        });

        app.init_resource::<RespawnQueue>();
        app.add_systems(
            Startup,
            (start_server, spawn_asteroids, spawn_nebula, spawn_scores),
        );
        app.add_systems(
            FixedUpdate,
            (
                handle_class_switch,
                server_fire_projectiles,
                server_fire_laser,
                server_drop_mines,
                server_launch_torpedoes,
                server_turret_ai,
                server_torpedo_homing,
                server_railgun,
                server_cloak,
                btl_shared::check_projectile_hits,
                btl_shared::check_projectile_asteroid_hits,
                btl_shared::check_mine_detonations,
                btl_shared::update_torpedo_lifetime,
                btl_shared::check_torpedo_shootdown,
                btl_shared::check_torpedo_hits,
                despawn_dead_ships,
                process_respawns,
            ),
        );
        app.add_systems(
            FixedUpdate,
            (
                server_init_drone_squads,
                server_spawn_initial_drones,
                server_drone_respawn,
                server_drone_ai,
                server_anti_drone_pulse,
                btl_shared::check_projectile_drone_hits,
                btl_shared::drone_laser_damage,
                btl_shared::drone_kamikaze_impact,
            ),
        );
        app.add_systems(FixedUpdate, update_zone_scores);
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
    let hash_hex: String = cert_hash
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
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
        ReplicationSender::new(REPLICATION_INTERVAL, SendUpdatesMode::SinceLastAck, false),
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

    let class = ShipClass::Interceptor;

    let ship = commands
        .spawn((
            ShipBundle::new(peer_id, team, class, spawn_pos),
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
        ))
        .id();

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
    mut respawn_queue: ResMut<RespawnQueue>,
) {
    for (entity, health, player_id, team, class, controlled_by, last_hit) in query.iter() {
        if health.current <= 0.0 {
            if let Some(killer) = last_hit.attacker {
                info!(
                    "Ship {:?} destroyed (player {:?}) — killed by {:?}",
                    entity, player_id.0, killer
                );
            } else {
                info!("Ship {:?} destroyed (player {:?})", entity, player_id.0);
            }
            respawn_queue.0.push(PendingRespawn {
                peer_id: player_id.0,
                team: *team,
                class: *class,
                link_entity: controlled_by.owner,
                timer: RESPAWN_DELAY,
            });
            commands.entity(entity).despawn();
        }
    }
}

/// Tick respawn timers and respawn ships when ready.
fn process_respawns(mut commands: Commands, mut respawn_queue: ResMut<RespawnQueue>) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;

    respawn_queue.0.retain_mut(|entry| {
        entry.timer -= dt;
        if entry.timer <= 0.0 {
            // Respawn at a random-ish position based on team
            let angle = (entry.peer_id.to_bits() as f32 * 2.3) % std::f32::consts::TAU;
            let dist = 200.0;
            let spawn_pos = Vec2::new(dist * angle.cos(), dist * angle.sin());

            let ship = commands
                .spawn((
                    ShipBundle::new(entry.peer_id, entry.team, entry.class, spawn_pos),
                    Replicate::to_clients(NetworkTarget::All),
                    PredictionTarget::to_clients(NetworkTarget::Single(entry.peer_id)),
                    InterpolationTarget::to_clients(NetworkTarget::AllExceptSingle(entry.peer_id)),
                    ControlledBy {
                        owner: entry.link_entity,
                        lifetime: Default::default(),
                    },
                ))
                .id();

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

        commands.spawn((
            ShipBundle::new(peer_id, team, requested, spawn_pos),
            Replicate::to_clients(NetworkTarget::All),
            PredictionTarget::to_clients(NetworkTarget::Single(peer_id)),
            InterpolationTarget::to_clients(NetworkTarget::AllExceptSingle(peer_id)),
            ControlledBy {
                owner: link_entity,
                lifetime: Default::default(),
            },
        ));
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
    mut targets: Query<(Entity, &Position, &Team, &mut Health, &mut LastDamagedBy)>,
    asteroids: Query<(&Position, &Asteroid)>,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;

    // Collect (hit_entity, damage, attacker) tuples first
    let mut hits: Vec<(Entity, f32, PeerId)> = Vec::new();

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

        // Raycast: find closest hit along aim direction within range
        let mut best_t = LASER_RANGE;
        let mut best_entity: Option<Entity> = None;

        // Check asteroids (block the beam, no damage)
        for (ast_pos, ast) in asteroids.iter() {
            let t = ray_circle_intersect(pos.0, aim_dir, ast_pos.0, ast.radius);
            if t > 0.0 && t < best_t {
                best_t = t;
                best_entity = None; // asteroid blocks, no damage target
            }
        }

        // Check enemy ships
        for (entity, target_pos, target_team, _hp, _) in targets.iter() {
            if *target_team == *team {
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
            // Damage falls off with distance (full at 0, 30% at max range)
            let falloff = 1.0 - 0.7 * (best_t / LASER_RANGE);
            hits.push((entity, LASER_DPS * falloff * dt, player_id.0));
        }
    }

    // Apply damage
    for (entity, damage, attacker) in hits {
        if let Ok((_, _, _, mut hp, mut last_hit)) = targets.get_mut(entity) {
            hp.current -= damage;
            last_hit.attacker = Some(attacker);
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
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;

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
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;

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
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;

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
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;

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

            // Find nearest enemy in range
            let mut best_dist_sq = range * range;
            let mut best_angle: Option<f32> = None;

            for (enemy_pos, enemy_team) in enemies.iter() {
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
    query: Query<Entity, (With<ShipClass>, Without<DroneSquad>)>,
    classes: Query<&ShipClass>,
) {
    for entity in query.iter() {
        if let Ok(class) = classes.get(entity)
            && *class == ShipClass::DroneCommander
        {
            commands
                .entity(entity)
                .insert(DroneSquad { respawn_timer: 0.0 });
        }
    }
}

/// Spawn drones for DroneCommander ships up to max count. Also tick respawn timer.
fn server_drone_respawn(
    mut commands: Commands,
    mut commanders: Query<(&PlayerId, &Team, &Position, &mut DroneSquad)>,
    existing_drones: Query<&Drone>,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;

    for (player_id, team, pos, mut squad) in commanders.iter_mut() {
        let mut laser_count = 0usize;
        let mut kamikaze_count = 0usize;
        for d in existing_drones.iter() {
            if d.owner == player_id.0 {
                match d.kind {
                    DroneKind::Laser => laser_count += 1,
                    DroneKind::Kamikaze => kamikaze_count += 1,
                }
            }
        }
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
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;
    let steer_rate = 6.0 * dt;
    let elapsed = time.elapsed_secs();

    for (drone_entity, drone, drone_pos, mut drone_vel) in drones.iter_mut() {
        // Per-drone jitter from entity bits — each drone drifts differently
        let seed = drone_entity.to_bits().wrapping_mul(2654435761);
        let phase = (seed % 1000) as f32 * 0.001 * std::f32::consts::TAU;
        let jitter = Vec2::new(
            (elapsed * 1.3 + phase).sin() * 80.0,
            (elapsed * 1.7 + phase * 1.4).cos() * 80.0,
        );

        // Find nearest enemy within aggro range
        let mut best_dist_sq = DRONE_AGGRO_RANGE * DRONE_AGGRO_RANGE;
        let mut nearest_enemy: Option<Vec2> = None;

        for (enemy_pos, enemy_team) in enemies.iter() {
            if *enemy_team == drone.owner_team {
                continue;
            }
            let dist_sq = (enemy_pos.0 - drone_pos.0).length_squared();
            if dist_sq < best_dist_sq {
                best_dist_sq = dist_sq;
                nearest_enemy = Some(enemy_pos.0);
            }
        }

        let commander_data = commanders
            .iter()
            .find(|(pid, _, _)| pid.0 == drone.owner)
            .map(|(_, pos, vel)| (pos.0, vel.0));

        let desired_vel = match drone.kind {
            DroneKind::Kamikaze => {
                if let Some(target) = nearest_enemy {
                    let delta = target - drone_pos.0;
                    let dist = delta.length();
                    if dist > 1.0 {
                        delta / dist * DRONE_KAMIKAZE_SPEED
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
                    let dist = delta.length();
                    if dist > DRONE_LASER_RANGE * 0.8 {
                        let chase_dir = delta / dist;
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
        let dist = to_cmd.length();
        if dist > DRONE_ORBIT_RADIUS * 2.0 {
            let chase_dir = to_cmd / dist;
            cmd_vel + chase_dir * speed
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
    mut ships: Query<(&Position, &Team, &mut Health, &mut LastDamagedBy)>,
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

        // Detonate ALL drones within pulse radius (friend and foe)
        for (drone_entity, drone_pos, drone) in drones.iter() {
            if (drone_pos.0 - pos.0).length_squared() < pulse_dist_sq {
                // Each drone explodes — deal area damage to nearby enemy ships
                for (ship_pos, ship_team, mut health, mut last_hit) in ships.iter_mut() {
                    if *ship_team == *team {
                        continue;
                    }
                    if (drone_pos.0 - ship_pos.0).length_squared() < blast_dist_sq {
                        health.current = (health.current - DRONE_DETONATION_DAMAGE).max(0.0);
                        last_hit.attacker = Some(drone.owner);
                    }
                }
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

/// King-of-the-hill: count ships per team in each zone, award points to majority holder.
fn update_zone_scores(
    mut scores_q: Query<&mut TeamScores>,
    ships: Query<(&Position, &Team), With<Health>>,
) {
    let Ok(mut scores) = scores_q.single_mut() else {
        return;
    };

    // Already won — freeze scores
    if scores.red >= SCORE_LIMIT || scores.blue >= SCORE_LIMIT {
        return;
    }

    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;
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

        if red > blue {
            scores.zone_control[i] = 1;
            scores.red += ZONE_SCORE_RATE * dt;
        } else if blue > red {
            scores.zone_control[i] = 2;
            scores.blue += ZONE_SCORE_RATE * dt;
        } else {
            scores.zone_control[i] = 0; // contested or empty
        }
    }

    // Clamp at limit and detect victory
    scores.red = scores.red.min(SCORE_LIMIT);
    scores.blue = scores.blue.min(SCORE_LIMIT);

    if scores.red >= SCORE_LIMIT {
        scores.round_state = RoundState::Won(Team::Red);
    } else if scores.blue >= SCORE_LIMIT {
        scores.round_state = RoundState::Won(Team::Blue);
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
