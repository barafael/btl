use std::net::SocketAddr;
use std::time::Duration;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::sprite::Anchor;
use lightyear::prelude::client::input::InputSystems;
use lightyear::prelude::client::*;
use lightyear::prelude::input::native::{ActionState, InputMarker};
use lightyear::prelude::*;
use lightyear::webtransport::prelude::client::WebTransportClientIo;

use avian2d::prelude::*;

use btl_protocol::*;
use btl_shared::{
    Ammo, Asteroid, Cloak, DCOMMANDER_MASS, DCOMMANDER_RADIUS, DEFENSE_TURRET_MOUNTS,
    DRONE_LASER_RANGE, DRONE_RADIUS, Drone, DroneKind, FrameInterpolate, drone_laser_firing, GUNSHIP_MASS,
    GUNSHIP_RADIUS, LASER_RANGE, MINE_RADIUS, MINE_TRIGGER_RADIUS, Mine, PULSE_RADIUS, Position,
    Projectile, RailgunCharge, Rotation, SHIP_MASS, SHIP_RADIUS, SNIPER_MASS, SNIPER_RADIUS,
    TBOAT_MASS, TBOAT_RADIUS, TORPEDO_RADIUS, TURRET_MOUNTS, Torpedo, ray_circle_intersect,
};

/// Convert the cursor position to world coordinates using the primary window and camera.
fn cursor_world_pos(
    windows: &Query<&Window>,
    camera_query: &Query<(&Camera, &GlobalTransform), With<Camera2d>>,
) -> Option<Vec2> {
    let cursor_pos = windows.single().ok()?.cursor_position()?;
    let (camera, cam_gt) = camera_query.single().ok()?;
    camera.viewport_to_world_2d(cam_gt, cursor_pos).ok()
}

fn team_color(team: &Team) -> Color {
    match team {
        Team::Red => Color::srgb(1.0, 0.3, 0.3),
        Team::Blue => Color::srgb(0.3, 0.3, 1.0),
    }
}

fn spawn_gun_barrel(commands: &mut Commands, parent: Entity) {
    commands.spawn((
        ChildOf(parent),
        GunBarrel,
        Sprite {
            color: Color::srgba(0.45, 0.45, 0.5, 0.85),
            custom_size: Some(Vec2::new(14.0, 1.5)),
            ..default()
        },
        Anchor::CENTER_LEFT,
        Transform::from_xyz(0.0, 0.0, 0.1)
            .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
    ));
}

fn spawn_defense_turret_barrels(commands: &mut Commands, parent: Entity) {
    for (i, mount) in DEFENSE_TURRET_MOUNTS.iter().enumerate() {
        commands.spawn((
            ChildOf(parent),
            TurretBarrel(i),
            Sprite {
                color: Color::srgba(0.4, 0.6, 0.5, 0.85),
                custom_size: Some(Vec2::new(8.0, 1.2)),
                ..default()
            },
            Anchor::CENTER_LEFT,
            Transform::from_xyz(mount.x, mount.y, 0.1),
        ));
    }
}

fn spawn_turret_barrels(commands: &mut Commands, parent: Entity) {
    for (i, mount) in TURRET_MOUNTS.iter().enumerate() {
        commands.spawn((
            ChildOf(parent),
            TurretBarrel(i),
            Sprite {
                color: Color::srgba(0.5, 0.5, 0.55, 0.85),
                custom_size: Some(Vec2::new(10.0, 1.5)),
                ..default()
            },
            Anchor::CENTER_LEFT,
            Transform::from_xyz(mount.x, mount.y, 0.1),
        ));
    }
}

/// Marker for the locally controlled ship.
#[derive(Component)]
pub struct LocalShip;

/// Marker to track that we've already initialized rendering for a predicted entity.
#[derive(Component)]
struct ShipInitialized;

/// Marker for asteroid entities that have been given visuals.
#[derive(Component)]
struct AsteroidInitialized;

/// Marker for projectiles that have been given visuals.
#[derive(Component)]
struct ProjectileInitialized;

/// Marker for mines that have been given visuals.
#[derive(Component)]
struct MineInitialized;

#[derive(Component)]
pub struct TorpedoInitialized;

#[derive(Component)]
struct DroneInitialized;

/// Marker for the gun barrel child entity.
#[derive(Component)]
struct GunBarrel;

/// Marker for turret barrel children (stores which mount index).
#[derive(Component)]
struct TurretBarrel(usize);

/// Tracks the class picker overlay state.
#[derive(Resource, Default)]
struct ClassPicker {
    open: bool,
    /// Set for one frame when a class is selected, then cleared.
    pending_request: u8,
}

/// Marker for the class picker overlay root node.
#[derive(Component)]
struct ClassPickerOverlay;

/// Marker for a class picker button. Stores which class it selects.
#[derive(Component)]
struct ClassPickerButton(ShipClass);

// --- Query filter aliases (tame clippy::type_complexity) ---

type UninitPredicted = (With<Predicted>, Without<ShipInitialized>);
type UninitInterpolated = (With<Interpolated>, Without<ShipInitialized>);
type GunBarrelFilter = (With<GunBarrel>, Without<LocalShip>);

// --- Route planning ---

const ROUTE_ZOOM_SCALE: f32 = 4.0;
const ROUTE_ZOOM_SPEED: f32 = 6.0;
const ROUTE_SAMPLE_COUNT: usize = 128;
/// Minimum angle (radians) between consecutive waypoint segments.
/// Derived from min turn radius: at cruise speed ~360, R_min = 360/6 = 60.
/// Angles sharper than ~60° are rejected.
const MIN_WAYPOINT_ANGLE: f32 = std::f32::consts::FRAC_PI_3; // 60°
/// Pure-pursuit look-ahead: scales with speed, clamped to this range.
const LOOK_AHEAD_MIN: f32 = 40.0;
const LOOK_AHEAD_MAX: f32 = 200.0;
const LOOK_AHEAD_TIME: f32 = 0.5; // seconds of travel to look ahead

#[derive(Resource)]
struct RoutePlanner {
    active: bool,
    waypoints: Vec<Vec2>,
    path: Vec<Vec2>,
    /// Per-sample curvature (inverse turning radius) — used for speed control
    curvatures: Vec<f32>,
    /// True if the last waypoint was rejected for being too sharp
    last_rejected: bool,
    target_zoom: f32,
    current_zoom: f32,
}

impl Default for RoutePlanner {
    fn default() -> Self {
        Self {
            active: false,
            waypoints: Vec::new(),
            path: Vec::new(),
            curvatures: Vec::new(),
            last_rejected: false,
            target_zoom: 1.0,
            current_zoom: 1.0,
        }
    }
}

/// Attached to the local ship while it's following a route.
#[derive(Component)]
struct RouteFollowing {
    path: Vec<Vec2>,
    curvatures: Vec<f32>,
    /// Cumulative arc length at each path point (for accurate distance→index lookup).
    arc_lengths: Vec<f32>,
    /// Precomputed max speed at each path point (braking-aware).
    speed_profile: Vec<f32>,
    /// Progress along the path as a fractional index (continuous, not discrete)
    progress: f32,
    /// Integral accumulator for cross-track error (PID I-term).
    cte_integral: f32,
}

/// Interceptor hull mesh: elongated needle/wedge, Razorback-inspired.
/// Long narrow body tapering to a sharp nose. No wings.
fn create_interceptor_mesh(r: f32) -> Mesh {
    // Simple elongated hexagon — narrow and long (Y+ = forward)
    let verts: Vec<[f32; 3]> = vec![
        [0.0, r * 1.6, 0.0],       // 0: nose tip
        [r * 0.25, r * 0.3, 0.0],  // 1: right shoulder
        [r * 0.3, -r * 0.6, 0.0],  // 2: right rear
        [0.0, -r * 0.9, 0.0],      // 3: tail
        [-r * 0.3, -r * 0.6, 0.0], // 4: left rear
        [-r * 0.25, r * 0.3, 0.0], // 5: left shoulder
    ];

    // Centroid for fan triangulation
    let n = verts.len() as f32;
    let cx: f32 = verts.iter().map(|v| v[0]).sum::<f32>() / n;
    let cy: f32 = verts.iter().map(|v| v[1]).sum::<f32>() / n;

    let mut positions = vec![[cx, cy, 0.0]]; // index 0 = center
    positions.extend_from_slice(&verts);

    let num = verts.len();
    let mut indices: Vec<u16> = Vec::with_capacity(num * 3);
    for i in 0..num {
        indices.push(0);
        indices.push((i + 1) as u16);
        indices.push(((i + 1) % num + 1) as u16);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U16(indices));
    mesh
}

/// Gunship hull mesh: wider, blockier than the Interceptor. Armored look.
fn create_gunship_mesh(r: f32) -> Mesh {
    let verts: Vec<[f32; 3]> = vec![
        [0.0, r * 1.2, 0.0],        // 0: nose (blunter than interceptor)
        [r * 0.45, r * 0.5, 0.0],   // 1: right forward
        [r * 0.55, -r * 0.1, 0.0],  // 2: right mid (widest)
        [r * 0.4, -r * 0.7, 0.0],   // 3: right rear
        [0.0, -r * 0.85, 0.0],      // 4: tail
        [-r * 0.4, -r * 0.7, 0.0],  // 5: left rear
        [-r * 0.55, -r * 0.1, 0.0], // 6: left mid (widest)
        [-r * 0.45, r * 0.5, 0.0],  // 7: left forward
    ];

    let n = verts.len() as f32;
    let cx: f32 = verts.iter().map(|v| v[0]).sum::<f32>() / n;
    let cy: f32 = verts.iter().map(|v| v[1]).sum::<f32>() / n;

    let mut positions = vec![[cx, cy, 0.0]];
    positions.extend_from_slice(&verts);

    let num = verts.len();
    let mut indices: Vec<u16> = Vec::with_capacity(num * 3);
    for i in 0..num {
        indices.push(0);
        indices.push((i + 1) as u16);
        indices.push(((i + 1) % num + 1) as u16);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U16(indices));
    mesh
}

/// Torpedo boat hull mesh: sleek medium body with side nacelles.
fn create_torpedo_boat_mesh(r: f32) -> Mesh {
    let verts: Vec<[f32; 3]> = vec![
        [0.0, r * 1.3, 0.0],        // 0: nose
        [r * 0.2, r * 0.6, 0.0],    // 1: right forward
        [r * 0.45, r * 0.2, 0.0],   // 2: right nacelle front
        [r * 0.5, -r * 0.4, 0.0],   // 3: right nacelle rear
        [r * 0.3, -r * 0.75, 0.0],  // 4: right tail
        [0.0, -r * 0.6, 0.0],       // 5: center tail
        [-r * 0.3, -r * 0.75, 0.0], // 6: left tail
        [-r * 0.5, -r * 0.4, 0.0],  // 7: left nacelle rear
        [-r * 0.45, r * 0.2, 0.0],  // 8: left nacelle front
        [-r * 0.2, r * 0.6, 0.0],   // 9: left forward
    ];

    let n = verts.len() as f32;
    let cx: f32 = verts.iter().map(|v| v[0]).sum::<f32>() / n;
    let cy: f32 = verts.iter().map(|v| v[1]).sum::<f32>() / n;

    let mut positions = vec![[cx, cy, 0.0]];
    positions.extend_from_slice(&verts);

    let num = verts.len();
    let mut indices: Vec<u16> = Vec::with_capacity(num * 3);
    for i in 0..num {
        indices.push(0);
        indices.push((i + 1) as u16);
        indices.push(((i + 1) % num + 1) as u16);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U16(indices));
    mesh
}

/// Sniper hull mesh: slim, angular stealth profile.
fn create_sniper_mesh(r: f32) -> Mesh {
    let verts: Vec<[f32; 3]> = vec![
        [0.0, r * 1.5, 0.0],       // 0: sharp nose
        [r * 0.15, r * 0.8, 0.0],  // 1: right forward (very narrow)
        [r * 0.35, r * 0.1, 0.0],  // 2: right wing tip
        [r * 0.2, -r * 0.5, 0.0],  // 3: right rear
        [r * 0.1, -r * 0.9, 0.0],  // 4: right tail fin
        [0.0, -r * 0.7, 0.0],      // 5: center tail
        [-r * 0.1, -r * 0.9, 0.0], // 6: left tail fin
        [-r * 0.2, -r * 0.5, 0.0], // 7: left rear
        [-r * 0.35, r * 0.1, 0.0], // 8: left wing tip
        [-r * 0.15, r * 0.8, 0.0], // 9: left forward
    ];

    let n = verts.len() as f32;
    let cx: f32 = verts.iter().map(|v| v[0]).sum::<f32>() / n;
    let cy: f32 = verts.iter().map(|v| v[1]).sum::<f32>() / n;

    let mut positions = vec![[cx, cy, 0.0]];
    positions.extend_from_slice(&verts);

    let num = verts.len();
    let mut indices: Vec<u16> = Vec::with_capacity(num * 3);
    for i in 0..num {
        indices.push(0);
        indices.push((i + 1) as u16);
        indices.push(((i + 1) % num + 1) as u16);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U16(indices));
    mesh
}

/// Drone Commander hull mesh: wide, flat hexagonal carrier shape.
fn create_drone_commander_mesh(r: f32) -> Mesh {
    let verts: Vec<[f32; 3]> = vec![
        [0.0, r * 1.0, 0.0],       // 0: nose (blunt)
        [r * 0.5, r * 0.6, 0.0],   // 1: right forward
        [r * 0.65, 0.0, 0.0],      // 2: right mid (widest)
        [r * 0.5, -r * 0.5, 0.0],  // 3: right rear
        [r * 0.2, -r * 0.8, 0.0],  // 4: right tail
        [0.0, -r * 0.65, 0.0],     // 5: center tail
        [-r * 0.2, -r * 0.8, 0.0], // 6: left tail
        [-r * 0.5, -r * 0.5, 0.0], // 7: left rear
        [-r * 0.65, 0.0, 0.0],     // 8: left mid (widest)
        [-r * 0.5, r * 0.6, 0.0],  // 9: left forward
    ];

    let n = verts.len() as f32;
    let cx: f32 = verts.iter().map(|v| v[0]).sum::<f32>() / n;
    let cy: f32 = verts.iter().map(|v| v[1]).sum::<f32>() / n;

    let mut positions = vec![[cx, cy, 0.0]];
    positions.extend_from_slice(&verts);

    let num = verts.len();
    let mut indices: Vec<u16> = Vec::with_capacity(num * 3);
    for i in 0..num {
        indices.push(0);
        indices.push((i + 1) as u16);
        indices.push(((i + 1) % num + 1) as u16);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U16(indices));
    mesh
}

pub struct ClientPlugin {
    pub server_addr: SocketAddr,
    pub client_id: u64,
    pub cert_hash: String,
}

impl Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        let server_addr = self.server_addr;
        let client_id = self.client_id;
        let cert_hash = self.cert_hash.clone();

        app.add_plugins(lightyear::prelude::client::ClientPlugins {
            tick_duration: Duration::from_secs_f64(1.0 / FIXED_TIMESTEP_HZ),
        });

        app.insert_resource(ClientConnectionConfig {
            server_addr,
            client_id,
            cert_hash,
        });

        app.init_resource::<RoutePlanner>();
        app.init_resource::<ClassPicker>();

        app.add_systems(Startup, connect_to_server);
        app.add_systems(
            FixedPreUpdate,
            (buffer_input, route_follow).in_set(InputSystems::WriteClientInputs),
        );
        app.add_observer(log_connected);
        app.add_systems(
            Update,
            (
                init_predicted_ships,
                init_interpolated_ships,
                init_asteroids,
                init_projectiles,
                init_mines,
                init_torpedoes,
                update_projectile_visuals,
                update_mine_visuals,
                update_torpedo_visuals,
                update_gun_barrels,
                update_turret_barrels,
                camera_follow_local_ship,
                update_hud,
            ),
        );
        app.add_systems(
            Update,
            (
                render_laser_beams,
                render_railgun,
                update_cloak_visuals,
                init_drones,
                update_drone_visuals,
                render_drone_lasers,
                render_pulse_indicator,
                route_planning_input,
                route_zoom,
                class_picker_input,
                class_picker_button_interaction,
                render_route_gizmos,
            ),
        );
        app.add_systems(Startup, (spawn_hud, spawn_class_picker));
    }
}

#[derive(Resource)]
struct ClientConnectionConfig {
    server_addr: SocketAddr,
    client_id: u64,
    cert_hash: String,
}

fn connect_to_server(mut commands: Commands, config: Res<ClientConnectionConfig>) {
    let auth = Authentication::Manual {
        server_addr: config.server_addr,
        client_id: config.client_id,
        private_key: PRIVATE_KEY,
        protocol_id: PROTOCOL_ID,
    };

    let netcode = NetcodeClient::new(
        auth,
        NetcodeConfig {
            client_timeout_secs: 5,
            token_expire_secs: -1,
            ..default()
        },
    )
    .expect("Failed to create NetcodeClient");

    let entity = commands
        .spawn((
            Client::default(),
            netcode,
            PeerAddr(config.server_addr),
            WebTransportClientIo {
                certificate_digest: config.cert_hash.clone(),
            },
            ReplicationReceiver::default(),
            PredictionManager::default(),
        ))
        .id();

    commands.trigger(Connect { entity });
    info!(
        "Connecting to server at {} as client {}",
        config.server_addr, config.client_id
    );
}

/// Read keyboard + mouse input and write it to the input buffer.
/// Skipped when route-following is active (route_follow writes inputs instead)
/// or when route planning mode is active (don't fly while planning).
fn buffer_input(
    mut query: Query<&mut ActionState<ShipInput>, With<InputMarker<ShipInput>>>,
    keypress: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    ship_query: Query<&Transform, With<LocalShip>>,
    route_following: Query<(), (With<LocalShip>, With<RouteFollowing>)>,
    planner: Res<RoutePlanner>,
    mut picker: ResMut<ClassPicker>,
) {
    // Don't overwrite inputs while route following or planning
    if route_following.single().is_ok() || planner.active {
        return;
    }

    // Compute aim angle: direction from ship to mouse cursor in world space
    let aim_angle = cursor_world_pos(&windows, &camera_query)
        .and_then(|world_pos| {
            let ship_pos = ship_query.single().ok()?.translation.truncate();
            let delta = world_pos - ship_pos;
            (delta.length_squared() > 1.0).then(|| delta.y.atan2(delta.x))
        })
        .unwrap_or(std::f32::consts::FRAC_PI_2); // default: aim up

    // Consume pending class request (one-shot)
    let class_request = std::mem::take(&mut picker.pending_request);

    let key = |k| f32::from(keypress.pressed(k));
    let axis = |pos, neg| key(pos) - key(neg);

    for mut action_state in query.iter_mut() {
        action_state.0 = ShipInput {
            thrust_forward: key(KeyCode::KeyW),
            thrust_backward: key(KeyCode::KeyS),
            rotate: axis(KeyCode::KeyA, KeyCode::KeyD),
            strafe: axis(KeyCode::KeyQ, KeyCode::KeyE),
            afterburner: keypress.pressed(KeyCode::ShiftLeft),
            stabilize: key(KeyCode::KeyR),
            fire: mouse_button.pressed(MouseButton::Left),
            drop_mine: keypress.just_pressed(KeyCode::KeyX),
            aim_angle,
            class_request,
        };
    }
}

/// Log when our client connection is established.
fn log_connected(trigger: On<Add, Connected>, query: Query<(), With<Client>>) {
    if query.get(trigger.entity).is_ok() {
        info!("Connected to server!");
    }
}

/// Initialize rendering for predicted ships once their components are synced.
fn init_predicted_ships(
    mut commands: Commands,
    query: Query<
        (Entity, &PlayerId, &Team, &ShipClass, &Position, &Rotation, Has<Controlled>),
        UninitPredicted,
    >,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, player_id, team, class, pos, rot, is_controlled) in query.iter() {
        let (radius, mass) = match class {
            ShipClass::Interceptor => (SHIP_RADIUS, SHIP_MASS),
            ShipClass::Gunship => (GUNSHIP_RADIUS, GUNSHIP_MASS),
            ShipClass::TorpedoBoat => (TBOAT_RADIUS, TBOAT_MASS),
            ShipClass::Sniper => (SNIPER_RADIUS, SNIPER_MASS),
            ShipClass::DroneCommander => (DCOMMANDER_RADIUS, DCOMMANDER_MASS),
        };
        let ship_mesh = match class {
            ShipClass::Interceptor => meshes.add(create_interceptor_mesh(radius)),
            ShipClass::Gunship => meshes.add(create_gunship_mesh(radius)),
            ShipClass::TorpedoBoat => meshes.add(create_torpedo_boat_mesh(radius)),
            ShipClass::Sniper => meshes.add(create_sniper_mesh(radius)),
            ShipClass::DroneCommander => meshes.add(create_drone_commander_mesh(radius)),
        };
        commands.entity(entity).insert((
            Mesh2d(ship_mesh),
            MeshMaterial2d(materials.add(team_color(team))),
            Transform::from_xyz(pos.0.x, pos.0.y, 0.0)
                .with_rotation(Quat::from_rotation_z(rot.as_radians())),
            ShipInitialized,
            FrameInterpolate::<Position> {
                trigger_change_detection: true,
                ..default()
            },
            FrameInterpolate::<Rotation> {
                trigger_change_detection: true,
                ..default()
            },
        ));
        spawn_gun_barrel(&mut commands, entity);
        if *class == ShipClass::Gunship {
            spawn_turret_barrels(&mut commands, entity);
        } else if *class == ShipClass::DroneCommander {
            spawn_defense_turret_barrels(&mut commands, entity);
        }

        if is_controlled {
            let angular_inertia = 0.5 * mass * radius * radius;
            commands.entity(entity).insert((
                RigidBody::Dynamic,
                Collider::circle(radius),
                Mass(mass),
                AngularInertia(angular_inertia),
                LinearDamping(0.0),
                AngularDamping(0.0),
                InputMarker::<ShipInput>::default(),
                LocalShip,
            ));
            info!(
                "Spawned local {class:?} for {:?} on {:?} team",
                player_id.0, team
            );
        } else {
            info!(
                "Spawned remote {class:?} for {:?} on {:?} team",
                player_id.0, team
            );
        }
    }
}

/// Initialize rendering for interpolated (remote) ships.
fn init_interpolated_ships(
    mut commands: Commands,
    query: Query<(Entity, &PlayerId, &Team, &ShipClass, &Position, &Rotation), UninitInterpolated>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, player_id, team, class, pos, rot) in query.iter() {
        let radius = match class {
            ShipClass::Interceptor => SHIP_RADIUS,
            ShipClass::Gunship => GUNSHIP_RADIUS,
            ShipClass::TorpedoBoat => TBOAT_RADIUS,
            ShipClass::Sniper => SNIPER_RADIUS,
            ShipClass::DroneCommander => DCOMMANDER_RADIUS,
        };
        let ship_mesh = match class {
            ShipClass::Interceptor => meshes.add(create_interceptor_mesh(radius)),
            ShipClass::Gunship => meshes.add(create_gunship_mesh(radius)),
            ShipClass::TorpedoBoat => meshes.add(create_torpedo_boat_mesh(radius)),
            ShipClass::Sniper => meshes.add(create_sniper_mesh(radius)),
            ShipClass::DroneCommander => meshes.add(create_drone_commander_mesh(radius)),
        };
        commands.entity(entity).insert((
            Mesh2d(ship_mesh),
            MeshMaterial2d(materials.add(team_color(team))),
            Transform::from_xyz(pos.0.x, pos.0.y, 0.0)
                .with_rotation(Quat::from_rotation_z(rot.as_radians())),
            ShipInitialized,
        ));
        spawn_gun_barrel(&mut commands, entity);
        if *class == ShipClass::Gunship {
            spawn_turret_barrels(&mut commands, entity);
        } else if *class == ShipClass::DroneCommander {
            spawn_defense_turret_barrels(&mut commands, entity);
        }

        info!(
            "Spawned interpolated {class:?} for {:?} on {:?} team",
            player_id.0, team
        );
    }
}

/// Initialize rendering for replicated asteroid entities.
fn init_asteroids(
    mut commands: Commands,
    query: Query<(Entity, &Asteroid), Without<AsteroidInitialized>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, asteroid) in query.iter() {
        let r = asteroid.radius;
        let seed = entity.to_bits();

        // Use a regular polygon (7-sided) as asteroid shape
        let mesh = meshes.add(RegularPolygon::new(r, 7));

        // Brownish-gray color with slight variation per asteroid
        let hash = seed.wrapping_mul(2654435761);
        let gray = 0.25 + 0.1 * ((hash % 1000) as f32 / 1000.0);
        let color = Color::srgb(gray + 0.05, gray, gray - 0.03);

        commands.entity(entity).insert((
            Mesh2d(mesh),
            MeshMaterial2d(materials.add(color)),
            AsteroidInitialized,
        ));
    }
}

/// Initialize rendering for replicated projectile entities.
/// Visual style depends on ProjectileKind.
fn init_projectiles(
    mut commands: Commands,
    query: Query<
        (Entity, &LinearVelocity, &Position, Option<&ProjectileKind>),
        (With<Projectile>, Without<ProjectileInitialized>),
    >,
) {
    for (entity, vel, pos, kind) in query.iter() {
        let kind = kind.copied().unwrap_or_default();
        let (color, size) = match kind {
            // Autocannon: bright yellow, small
            ProjectileKind::Autocannon => (
                Color::LinearRgba(LinearRgba::new(3.0, 2.5, 0.8, 1.0)),
                Vec2::new(8.0, 2.0),
            ),
            // Heavy cannon: orange-red, larger
            ProjectileKind::HeavyCannon => (
                Color::LinearRgba(LinearRgba::new(4.0, 1.5, 0.3, 1.0)),
                Vec2::new(12.0, 4.0),
            ),
            // Turret: cyan-blue, small
            ProjectileKind::Turret => (
                Color::LinearRgba(LinearRgba::new(0.5, 2.0, 3.0, 1.0)),
                Vec2::new(6.0, 1.5),
            ),
            // Railgun: bright white-blue, long tracer
            ProjectileKind::Railgun => (
                Color::LinearRgba(LinearRgba::new(2.0, 3.0, 5.0, 1.0)),
                Vec2::new(24.0, 3.0),
            ),
        };
        let angle = vel.0.y.atan2(vel.0.x);

        commands.entity(entity).insert((
            Sprite {
                color,
                custom_size: Some(size),
                ..default()
            },
            Transform::from_xyz(pos.0.x, pos.0.y, 5.0).with_rotation(Quat::from_rotation_z(angle)),
            ProjectileInitialized,
        ));
    }
}

/// Orient projectiles along their velocity each frame.
fn update_projectile_visuals(
    mut query: Query<(&mut Transform, &LinearVelocity), With<ProjectileInitialized>>,
) {
    for (mut tf, vel) in query.iter_mut() {
        if vel.0.length_squared() > 0.1 {
            let angle = vel.0.y.atan2(vel.0.x);
            tf.rotation = Quat::from_rotation_z(angle);
        }
    }
}

/// Initialize rendering for replicated mine entities.
/// Per design doc: black octagon with white shadow/outline and pulsing red core.
fn init_mines(
    mut commands: Commands,
    query: Query<(Entity, &Mine), Without<MineInitialized>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, _mine) in query.iter() {
        // Subtle shadow/outline (slightly larger octagon behind, very dim)
        let shadow_mesh = meshes.add(RegularPolygon::new(MINE_RADIUS + 1.5, 8));
        let shadow_color = Color::LinearRgba(LinearRgba::new(0.3, 0.3, 0.3, 0.12));
        commands.spawn((
            MineShadow { parent: entity },
            Mesh2d(shadow_mesh),
            MeshMaterial2d(materials.add(shadow_color)),
            Transform::from_xyz(0.0, 0.0, 4.9),
        ));

        // Dark octagonal shell
        let shell_mesh = meshes.add(RegularPolygon::new(MINE_RADIUS, 8));
        let shell_color = Color::srgb(0.03, 0.03, 0.03);
        commands.entity(entity).insert((
            Mesh2d(shell_mesh),
            MeshMaterial2d(materials.add(shell_color)),
            MineInitialized,
        ));

        // Dim pulsing red core (smaller inner octagon)
        let core_mesh = meshes.add(RegularPolygon::new(MINE_RADIUS * 0.35, 8));
        let core_color = Color::LinearRgba(LinearRgba::new(0.8, 0.08, 0.04, 0.4));
        commands.spawn((
            MineCore { parent: entity },
            Mesh2d(core_mesh),
            MeshMaterial2d(materials.add(core_color)),
            Transform::from_xyz(0.0, 0.0, 5.0),
        ));
    }
}

/// Marker linking a mine core glow to its mine entity.
#[derive(Component)]
struct MineCore {
    parent: Entity,
}

/// Marker linking a mine shadow to its mine entity.
#[derive(Component)]
struct MineShadow {
    parent: Entity,
}

type MineCoreFilter = (Without<MineInitialized>, Without<MineShadow>);
type MineShadowFilter = (Without<MineInitialized>, Without<MineCore>);
type ShipForMineFilter = (
    With<ShipInitialized>,
    Without<MineInitialized>,
    Without<MineCore>,
    Without<MineShadow>,
);

/// Pulse mine cores, position shadows, proximity warning, and clean up orphaned children.
fn update_mine_visuals(
    mut commands: Commands,
    mines: Query<(Entity, &Mine, &Transform), With<MineInitialized>>,
    mut cores: Query<
        (
            Entity,
            &MineCore,
            &mut Transform,
            &mut MeshMaterial2d<ColorMaterial>,
        ),
        MineCoreFilter,
    >,
    mut shadows: Query<(Entity, &MineShadow, &mut Transform), MineShadowFilter>,
    ships: Query<(&Transform, &Team), ShipForMineFilter>,
    time: Res<Time>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let t = time.elapsed_secs();

    for (mine_entity, mine, mine_tf) in mines.iter() {
        let mine_pos = mine_tf.translation.truncate();

        // Check proximity to enemy ships for warning pulse
        let mut closest_enemy_dist = f32::MAX;
        for (ship_tf, ship_team) in ships.iter() {
            if *ship_team == mine.owner_team {
                continue;
            }
            let dist = (ship_tf.translation.truncate() - mine_pos).length();
            if dist < closest_enemy_dist {
                closest_enemy_dist = dist;
            }
        }

        // Proximity boost: pulse faster as enemies approach trigger radius
        let proximity_mult = if closest_enemy_dist < MINE_TRIGGER_RADIUS * 2.5 {
            1.0 + (1.0 - closest_enemy_dist / (MINE_TRIGGER_RADIUS * 2.5)) * 4.0
        } else {
            1.0
        };

        let base_rate = if mine.arm_timer > 0.0 { 0.5 } else { 1.5 };
        let pulse_rate = base_rate * proximity_mult;
        let pulse = ((t * pulse_rate * std::f32::consts::TAU).sin() * 0.5 + 0.5).powi(2);
        let intensity = 0.3 + pulse * 0.6;

        for (_core_entity, core, mut core_tf, mat_handle) in cores.iter_mut() {
            if core.parent == mine_entity {
                core_tf.translation.x = mine_tf.translation.x;
                core_tf.translation.y = mine_tf.translation.y;
                core_tf.translation.z = mine_tf.translation.z + 0.1;

                if let Some(mat) = materials.get_mut(&mat_handle.0) {
                    mat.color = Color::LinearRgba(LinearRgba::new(
                        intensity,
                        0.04 * pulse,
                        0.02,
                        0.2 + 0.2 * pulse,
                    ));
                }
            }
        }

        for (_shadow_entity, shadow, mut shadow_tf) in shadows.iter_mut() {
            if shadow.parent == mine_entity {
                shadow_tf.translation.x = mine_tf.translation.x;
                shadow_tf.translation.y = mine_tf.translation.y;
                shadow_tf.translation.z = mine_tf.translation.z - 0.1;
            }
        }
    }

    // Clean up orphaned cores and shadows
    for (core_entity, core, _, _) in cores.iter() {
        if mines.get(core.parent).is_err() {
            commands.entity(core_entity).despawn();
        }
    }
    for (shadow_entity, shadow, _) in shadows.iter() {
        if mines.get(shadow.parent).is_err() {
            commands.entity(shadow_entity).despawn();
        }
    }
}

/// Initialize rendering for replicated torpedo entities.
fn init_torpedoes(
    mut commands: Commands,
    query: Query<
        (Entity, &LinearVelocity, &Position),
        (With<Torpedo>, Without<TorpedoInitialized>),
    >,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, vel, pos) in query.iter() {
        let angle = vel.0.y.atan2(vel.0.x);
        let r = TORPEDO_RADIUS;
        // Torpedo body: pointed ellipse shape (no glow — values <= 1.0)
        let mesh = meshes.add(Triangle2d::new(
            Vec2::new(r * 2.0, 0.0),
            Vec2::new(-r * 1.0, r * 0.8),
            Vec2::new(-r * 1.0, -r * 0.8),
        ));
        let mat = materials.add(ColorMaterial::from_color(Color::srgb(0.6, 0.75, 0.5)));
        commands.entity(entity).insert((
            Mesh2d(mesh),
            MeshMaterial2d(mat),
            Transform::from_xyz(pos.0.x, pos.0.y, 5.0).with_rotation(Quat::from_rotation_z(angle)),
            TorpedoInitialized,
        ));
    }
}

/// Orient torpedoes along their velocity each frame.
fn update_torpedo_visuals(
    mut query: Query<(&mut Transform, &LinearVelocity), With<TorpedoInitialized>>,
) {
    for (mut tf, vel) in query.iter_mut() {
        if vel.0.length_squared() > 0.1 {
            let angle = vel.0.y.atan2(vel.0.x);
            tf.rotation = Quat::from_rotation_z(angle);
        }
    }
}

/// Initialize rendering for replicated drone entities (small triangles).
fn init_drones(
    mut commands: Commands,
    query: Query<(Entity, &Drone, &Position), Without<DroneInitialized>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, drone, pos) in query.iter() {
        let team_tint = match drone.owner_team {
            Team::Red => LinearRgba::new(1.5, 0.5, 0.3, 0.9),
            Team::Blue => LinearRgba::new(0.3, 0.5, 1.5, 0.9),
        };

        match drone.kind {
            DroneKind::Laser => {
                // Mini ship shape: elongated diamond (4 vertices)
                let r = DRONE_RADIUS;
                let mesh = meshes.add(Triangle2d::new(
                    Vec2::new(r * 1.5, 0.0),
                    Vec2::new(-r * 0.8, r * 0.6),
                    Vec2::new(-r * 0.8, -r * 0.6),
                ));
                let mat = materials.add(ColorMaterial::from_color(Color::LinearRgba(team_tint)));
                commands.entity(entity).insert((
                    Mesh2d(mesh),
                    MeshMaterial2d(mat),
                    Transform::from_xyz(pos.0.x, pos.0.y, 4.0),
                    DroneInitialized,
                ));
            }
            DroneKind::Kamikaze => {
                // Mini mine shape: small octagon with warning tint
                let r = DRONE_RADIUS * 0.9;
                let mesh = meshes.add(RegularPolygon::new(r, 8));
                // Darker, more orange/red tint for kamikaze
                let kaze_tint = LinearRgba::new(
                    team_tint.red * 0.8 + 0.5,
                    team_tint.green * 0.5,
                    team_tint.blue * 0.3,
                    0.9,
                );
                let mat = materials.add(ColorMaterial::from_color(Color::LinearRgba(kaze_tint)));
                commands.entity(entity).insert((
                    Mesh2d(mesh),
                    MeshMaterial2d(mat),
                    Transform::from_xyz(pos.0.x, pos.0.y, 4.0),
                    DroneInitialized,
                ));
            }
        }
    }
}

/// Orient drones along their velocity.
fn update_drone_visuals(
    mut query: Query<(&mut Transform, &LinearVelocity), With<DroneInitialized>>,
) {
    for (mut tf, vel) in query.iter_mut() {
        if vel.0.length_squared() > 1.0 {
            let angle = vel.0.y.atan2(vel.0.x);
            tf.rotation = Quat::from_rotation_z(angle);
        }
    }
}

/// Render thin laser beams from laser drones to their nearest enemy target.
fn render_drone_lasers(
    drones: Query<(Entity, &Drone, &Transform), With<DroneInitialized>>,
    enemies: Query<(&Transform, &Team), With<ShipInitialized>>,
    mut gizmos: Gizmos,
    time: Res<Time>,
) {
    let elapsed = time.elapsed_secs();
    for (drone_entity, drone, drone_tf) in drones.iter() {
        if drone.kind != DroneKind::Laser {
            continue;
        }
        if !drone_laser_firing(drone_entity.to_bits(), elapsed) {
            continue;
        }
        let drone_pos = drone_tf.translation.truncate();
        let range_sq = DRONE_LASER_RANGE * DRONE_LASER_RANGE;

        let mut best_dist_sq = range_sq;
        let mut best_pos = None;
        for (enemy_tf, enemy_team) in enemies.iter() {
            if *enemy_team == drone.owner_team {
                continue;
            }
            let dist_sq = (drone_pos - enemy_tf.translation.truncate()).length_squared();
            if dist_sq < best_dist_sq {
                best_dist_sq = dist_sq;
                best_pos = Some(enemy_tf.translation.truncate());
            }
        }

        if let Some(target_pos) = best_pos {
            let dist = (target_pos - drone_pos).length();
            let fade = 1.0 - 0.7 * (dist / DRONE_LASER_RANGE);
            let base = match drone.owner_team {
                Team::Red => LinearRgba::new(1.5, 0.3, 0.2, 0.5),
                Team::Blue => LinearRgba::new(0.2, 0.3, 1.5, 0.5),
            };
            let faded = LinearRgba::new(
                base.red * fade,
                base.green * fade,
                base.blue * fade,
                base.alpha * fade,
            );
            gizmos.line_2d(drone_pos, target_pos, Color::LinearRgba(faded));
        }
    }
}

/// Render anti-drone pulse radius indicator for DroneCommander.
fn render_pulse_indicator(
    ships: Query<(&ShipClass, &Transform, &MineCooldown), With<LocalShip>>,
    mut gizmos: Gizmos,
) {
    for (class, ship_tf, cooldown) in ships.iter() {
        if *class != ShipClass::DroneCommander {
            continue;
        }
        let ship_pos = ship_tf.translation.truncate();

        // Show pulse radius when ready (subtle circle)
        if cooldown.remaining <= 0.0 {
            gizmos.circle_2d(ship_pos, PULSE_RADIUS, Color::srgba(0.5, 0.8, 0.5, 0.1));
        }
    }
}

/// Draw laser beam from TorpedoBoat ships that are firing.
fn render_laser_beams(
    ships: Query<
        (
            &ShipClass,
            &Transform,
            &ActionState<ShipInput>,
            &Team,
            &Ammo,
        ),
        With<LocalShip>,
    >,
    enemies: Query<(&Transform, &Team), (With<ShipInitialized>, Without<LocalShip>)>,
    asteroids: Query<(&Transform, &Asteroid)>,
    mut gizmos: Gizmos,
) {
    for (class, ship_tf, input, team, ammo) in ships.iter() {
        if *class != ShipClass::TorpedoBoat || !input.0.fire || ammo.current <= 0.0 {
            continue;
        }

        let ship_pos = ship_tf.translation.truncate();
        let aim_dir = Vec2::new(input.0.aim_angle.cos(), input.0.aim_angle.sin());

        // Find closest hit along beam (asteroids block, enemies take damage)
        let mut best_t = LASER_RANGE;

        // Check asteroids
        for (ast_tf, ast) in asteroids.iter() {
            let t =
                ray_circle_intersect(ship_pos, aim_dir, ast_tf.translation.truncate(), ast.radius);
            if t > 0.0 && t < best_t {
                best_t = t;
            }
        }

        // Check enemies
        for (enemy_tf, enemy_team) in enemies.iter() {
            if *enemy_team == *team {
                continue;
            }
            let enemy_pos = enemy_tf.translation.truncate();
            let to_enemy = enemy_pos - ship_pos;
            let t = to_enemy.dot(aim_dir);
            if t < 0.0 || t > best_t {
                continue;
            }
            let closest = ship_pos + aim_dir * t;
            let dist_sq = (enemy_pos - closest).length_squared();
            if dist_sq < TBOAT_RADIUS * TBOAT_RADIUS * 4.0 {
                best_t = t;
            }
        }

        // Draw beam as fading segments
        let segments = 12;
        let offset_dir = Vec2::new(-aim_dir.y, aim_dir.x);
        for i in 0..segments {
            let t0 = i as f32 / segments as f32;
            let t1 = (i + 1) as f32 / segments as f32;
            let p0 = ship_pos + aim_dir * (best_t * t0);
            let p1 = ship_pos + aim_dir * (best_t * t1);
            let fade = 1.0 - 0.8 * ((t0 + t1) * 0.5); // fade to 20% at end
            // Core beam
            gizmos.line_2d(
                p0,
                p1,
                Color::LinearRgba(LinearRgba::new(2.0 * fade, 0.3 * fade, 0.3 * fade, 0.9 * fade)),
            );
            // Glow
            let glow_a = 0.3 * fade;
            gizmos.line_2d(
                p0 + offset_dir,
                p1 + offset_dir,
                Color::LinearRgba(LinearRgba::new(1.0 * fade, 0.15 * fade, 0.1 * fade, glow_a)),
            );
            gizmos.line_2d(
                p0 - offset_dir,
                p1 - offset_dir,
                Color::LinearRgba(LinearRgba::new(1.0 * fade, 0.15 * fade, 0.1 * fade, glow_a)),
            );
        }
    }
}

/// Render railgun charge glow on Sniper ships.
/// The railgun projectile visual is handled by init_projectiles/update_projectile_visuals.
fn render_railgun(ships: Query<(&ShipClass, &Transform, &RailgunCharge)>, mut gizmos: Gizmos) {
    for (class, ship_tf, charge) in ships.iter() {
        if *class != ShipClass::Sniper {
            continue;
        }

        let ship_pos = ship_tf.translation.truncate();

        // Show charge glow (bright circle around ship, scales with charge)
        if charge.charge > 0.01 {
            let intensity = charge.charge;
            let glow_radius = SNIPER_RADIUS * (1.2 + 0.8 * intensity);
            gizmos.circle_2d(
                ship_pos,
                glow_radius,
                Color::LinearRgba(LinearRgba::new(
                    0.5 + 2.5 * intensity,
                    0.8 + 1.2 * intensity,
                    3.0 * intensity,
                    0.15 + 0.3 * intensity,
                )),
            );
        }
    }
}

/// Apply cloak visual: make cloaked enemy ships semi-transparent (faint shimmer).
/// Own cloaked ship gets slight transparency. Allied cloaked ships stay visible.
fn update_cloak_visuals(
    mut ships: Query<
        (
            &Cloak,
            &Team,
            &mut MeshMaterial2d<ColorMaterial>,
            Has<LocalShip>,
        ),
        With<ShipInitialized>,
    >,
    local_team: Query<&Team, With<LocalShip>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    time: Res<Time>,
) {
    let Ok(my_team) = local_team.single() else {
        return;
    };
    let t = time.elapsed_secs();

    for (cloak, team, mat_handle, is_local) in ships.iter_mut() {
        let Some(mat) = materials.get_mut(&mat_handle.0) else {
            continue;
        };

        if cloak.active {
            if is_local {
                // Own ship: slightly transparent
                mat.color = mat.color.with_alpha(0.4);
            } else if *team == *my_team {
                // Allied cloaked ship: slightly transparent
                mat.color = mat.color.with_alpha(0.5);
            } else {
                // Enemy cloaked ship: faint shimmer
                let shimmer = (t * 3.0).sin() * 0.05 + 0.08;
                mat.color = mat.color.with_alpha(shimmer);
            }
        } else {
            // Not cloaked: full opacity
            mat.color = mat.color.with_alpha(1.0);
        }
    }
}

#[derive(Component)]
struct HudText;

#[derive(Component)]
struct HealthBarFill;

#[derive(Component)]
struct FuelBarFill;

#[derive(Component)]
struct AmmoBarFill;

type HealthBarFilter = (
    With<HealthBarFill>,
    Without<FuelBarFill>,
    Without<AmmoBarFill>,
    Without<HudText>,
);
type FuelBarFilter = (
    With<FuelBarFill>,
    Without<HealthBarFill>,
    Without<AmmoBarFill>,
    Without<HudText>,
);
type AmmoBarFilter = (
    With<AmmoBarFill>,
    Without<HealthBarFill>,
    Without<FuelBarFill>,
    Without<HudText>,
);

const BAR_WIDTH: f32 = 160.0;
const BAR_HEIGHT: f32 = 10.0;

fn spawn_hud(mut commands: Commands) {
    // Bottom-left HUD panel
    let panel = commands
        .spawn((Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(12.0),
            left: Val::Px(12.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        },))
        .id();

    // Health bar
    let health_row = commands
        .spawn((
            ChildOf(panel),
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .id();

    commands.spawn((
        ChildOf(health_row),
        Text::new("HP"),
        TextFont {
            font_size: 12.0,
            ..default()
        },
        TextColor(Color::srgba(0.8, 0.3, 0.3, 0.9)),
    ));

    let health_bg = commands
        .spawn((
            ChildOf(health_row),
            Node {
                width: Val::Px(BAR_WIDTH),
                height: Val::Px(BAR_HEIGHT),
                ..default()
            },
            BackgroundColor(Color::srgba(0.15, 0.05, 0.05, 0.8)),
        ))
        .id();

    commands.spawn((
        ChildOf(health_bg),
        HealthBarFill,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgb(0.8, 0.2, 0.2)),
    ));

    // Fuel bar
    let fuel_row = commands
        .spawn((
            ChildOf(panel),
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .id();

    commands.spawn((
        ChildOf(fuel_row),
        Text::new("FU"),
        TextFont {
            font_size: 12.0,
            ..default()
        },
        TextColor(Color::srgba(0.3, 0.5, 0.8, 0.9)),
    ));

    let fuel_bg = commands
        .spawn((
            ChildOf(fuel_row),
            Node {
                width: Val::Px(BAR_WIDTH),
                height: Val::Px(BAR_HEIGHT),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.05, 0.15, 0.8)),
        ))
        .id();

    commands.spawn((
        ChildOf(fuel_bg),
        FuelBarFill,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgb(0.2, 0.4, 0.8)),
    ));

    // Ammo bar
    let ammo_row = commands
        .spawn((
            ChildOf(panel),
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .id();

    commands.spawn((
        ChildOf(ammo_row),
        Text::new("AM"),
        TextFont {
            font_size: 12.0,
            ..default()
        },
        TextColor(Color::srgba(0.7, 0.6, 0.3, 0.9)),
    ));

    let ammo_bg = commands
        .spawn((
            ChildOf(ammo_row),
            Node {
                width: Val::Px(BAR_WIDTH),
                height: Val::Px(BAR_HEIGHT),
                ..default()
            },
            BackgroundColor(Color::srgba(0.1, 0.08, 0.02, 0.8)),
        ))
        .id();

    commands.spawn((
        ChildOf(ammo_bg),
        AmmoBarFill,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgb(0.7, 0.5, 0.2)),
    ));

    // Speed + coords text (top-left)
    commands.spawn((
        HudText,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            left: Val::Px(12.0),
            ..default()
        },
        Text::new("SPD 0 | (0, 0)"),
        TextFont {
            font_size: 14.0,
            ..default()
        },
        TextColor(Color::srgba(0.7, 0.7, 0.7, 0.8)),
    ));
}

/// Spawn the class picker overlay (hidden by default, toggled with Tab).
fn spawn_class_picker(mut commands: Commands) {
    // Center overlay panel
    let overlay = commands
        .spawn((
            ClassPickerOverlay,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(30.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-150.0)),
                width: Val::Px(300.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::all(Val::Px(16.0)),
                row_gap: Val::Px(12.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.05, 0.1, 0.9)),
            GlobalZIndex(200),
            Visibility::Hidden,
        ))
        .id();

    // Title
    commands.spawn((
        ChildOf(overlay),
        Text::new("SELECT CLASS"),
        TextFont {
            font_size: 18.0,
            ..default()
        },
        TextColor(Color::srgba(0.9, 0.9, 0.9, 0.95)),
    ));

    // Interceptor button
    spawn_class_button(
        &mut commands,
        overlay,
        ShipClass::Interceptor,
        "INTERCEPTOR",
        "Fast & agile. Autocannon + mines.",
        Color::srgb(0.2, 0.6, 0.3),
    );

    // Gunship button
    spawn_class_button(
        &mut commands,
        overlay,
        ShipClass::Gunship,
        "GUNSHIP",
        "Tough & heavy. Heavy cannon + turrets.",
        Color::srgb(0.5, 0.3, 0.2),
    );

    // Torpedo Boat button
    spawn_class_button(
        &mut commands,
        overlay,
        ShipClass::TorpedoBoat,
        "TORPEDO BOAT",
        "Laser + homing torpedoes. Tactical.",
        Color::srgb(0.2, 0.4, 0.6),
    );

    // Sniper button
    spawn_class_button(
        &mut commands,
        overlay,
        ShipClass::Sniper,
        "SNIPER",
        "Railgun + mines + cloak. Stealth.",
        Color::srgb(0.4, 0.2, 0.5),
    );

    // Drone Commander button
    spawn_class_button(
        &mut commands,
        overlay,
        ShipClass::DroneCommander,
        "DRONE COMMANDER",
        "Defense turrets + attack drones + pulse.",
        Color::srgb(0.3, 0.5, 0.3),
    );

    // Hint text
    commands.spawn((
        ChildOf(overlay),
        Text::new("[Tab] to close"),
        TextFont {
            font_size: 11.0,
            ..default()
        },
        TextColor(Color::srgba(0.5, 0.5, 0.5, 0.7)),
    ));
}

fn spawn_class_button(
    commands: &mut Commands,
    parent: Entity,
    class: ShipClass,
    title: &str,
    desc: &str,
    color: Color,
) {
    let btn = commands
        .spawn((
            ChildOf(parent),
            ClassPickerButton(class),
            Button,
            Node {
                width: Val::Px(260.0),
                padding: UiRect::axes(Val::Px(12.0), Val::Px(8.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(2.0),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BorderColor::all(color),
            BackgroundColor(Color::srgba(0.1, 0.1, 0.15, 0.8)),
        ))
        .id();

    commands.spawn((
        ChildOf(btn),
        Text::new(title.to_string()),
        TextFont {
            font_size: 16.0,
            ..default()
        },
        TextColor(color),
    ));

    commands.spawn((
        ChildOf(btn),
        Text::new(desc.to_string()),
        TextFont {
            font_size: 11.0,
            ..default()
        },
        TextColor(Color::srgba(0.6, 0.6, 0.6, 0.8)),
    ));
}

/// Toggle class picker with Tab key.
fn class_picker_input(
    keypress: Res<ButtonInput<KeyCode>>,
    mut picker: ResMut<ClassPicker>,
    mut overlay: Query<&mut Visibility, With<ClassPickerOverlay>>,
) {
    if keypress.just_pressed(KeyCode::Tab) {
        picker.open = !picker.open;
        if let Ok(mut vis) = overlay.single_mut() {
            *vis = if picker.open {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
}

/// Handle clicks on class picker buttons.
fn class_picker_button_interaction(
    mut interaction_query: Query<(&Interaction, &ClassPickerButton), Changed<Interaction>>,
    mut picker: ResMut<ClassPicker>,
    mut overlay: Query<&mut Visibility, With<ClassPickerOverlay>>,
) {
    for (interaction, btn) in interaction_query.iter_mut() {
        if *interaction == Interaction::Pressed {
            picker.pending_request = btn.0.to_request();
            picker.open = false;
            if let Ok(mut vis) = overlay.single_mut() {
                *vis = Visibility::Hidden;
            }
        }
    }
}

fn update_hud(
    ship_query: Query<(&Transform, &Health, &Fuel, &Ammo, &LinearVelocity), With<LocalShip>>,
    mut text_query: Query<&mut Text, With<HudText>>,
    mut health_bar: Query<&mut Node, HealthBarFilter>,
    mut fuel_bar: Query<&mut Node, FuelBarFilter>,
    mut ammo_bar: Query<&mut Node, AmmoBarFilter>,
) {
    let Ok((ship_tf, health, fuel, ammo, lin_vel)) = ship_query.single() else {
        return;
    };

    if let Ok(mut text) = text_query.single_mut() {
        let x = ship_tf.translation.x as i32;
        let y = ship_tf.translation.y as i32;
        let speed = lin_vel.0.length() as i32;
        **text = format!("SPD {speed} | ({x}, {y})");
    }

    if let Ok(mut node) = health_bar.single_mut() {
        node.width = Val::Percent(health.fraction() * 100.0);
    }

    if let Ok(mut node) = fuel_bar.single_mut() {
        node.width = Val::Percent(fuel.fraction() * 100.0);
    }

    if let Ok(mut node) = ammo_bar.single_mut() {
        node.width = Val::Percent(ammo.fraction() * 100.0);
    }
}

/// Rotate the local ship's gun barrel toward the mouse cursor.
fn update_gun_barrels(
    local_ship: Query<(Entity, &Transform, &ActionState<ShipInput>), With<LocalShip>>,
    mut barrels: Query<(&ChildOf, &mut Transform), GunBarrelFilter>,
) {
    let Ok((ship_entity, ship_tf, input)) = local_ship.single() else {
        return;
    };
    let (_, _, ship_angle) = ship_tf.rotation.to_euler(EulerRot::XYZ);
    let local_angle = input.0.aim_angle - ship_angle;

    for (child_of, mut barrel_tf) in barrels.iter_mut() {
        if child_of.0 == ship_entity {
            barrel_tf.rotation = Quat::from_rotation_z(local_angle);
        }
    }
}

/// Update turret barrel rotations from replicated Turrets component.
fn update_turret_barrels(
    ships: Query<(&Transform, &Turrets)>,
    mut barrels: Query<(&ChildOf, &TurretBarrel, &mut Transform), Without<Turrets>>,
) {
    for (child_of, turret_barrel, mut barrel_tf) in barrels.iter_mut() {
        let Ok((ship_tf, turrets)) = ships.get(child_of.0) else {
            continue;
        };
        let Some(state) = turrets.mounts.get(turret_barrel.0) else {
            continue;
        };
        // Convert world-space aim angle to local-space rotation
        let (_, _, ship_angle) = ship_tf.rotation.to_euler(EulerRot::XYZ);
        let local_angle = state.aim_angle - ship_angle;
        barrel_tf.rotation = Quat::from_rotation_z(local_angle);
    }
}

/// Camera follows the locally controlled ship.
fn camera_follow_local_ship(
    ship_query: Query<&Transform, With<LocalShip>>,
    mut camera_query: Query<&mut Transform, (With<Camera2d>, Without<LocalShip>)>,
) {
    let Ok(ship_transform) = ship_query.single() else {
        return;
    };
    let Ok(mut cam_transform) = camera_query.single_mut() else {
        return;
    };
    cam_transform.translation.x = ship_transform.translation.x;
    cam_transform.translation.y = ship_transform.translation.y;
}

// --- Route planning systems ---

use btl_shared::{
    SHIP_ANGULAR_DECEL, SHIP_MAX_ANGULAR_SPEED, SHIP_MAX_SPEED, SHIP_STABILIZE_DECEL, SHIP_THRUST,
};

/// Normalize angle to [-PI, PI].
fn wrap_angle(mut a: f32) -> f32 {
    while a > std::f32::consts::PI {
        a -= std::f32::consts::TAU;
    }
    while a < -std::f32::consts::PI {
        a += std::f32::consts::TAU;
    }
    a
}

/// Evaluate a Catmull-Rom spline through `points` at parameter `t` in [0, 1].
/// Centripetal Catmull-Rom spline: smoother curvature transitions at waypoints
/// compared to uniform Catmull-Rom. Uses alpha=0.5 (centripetal parameterization)
/// which avoids cusps and self-intersections.
fn catmull_rom_sample(points: &[Vec2], t: f32) -> Vec2 {
    let n = points.len();
    if n == 0 {
        return Vec2::ZERO;
    }
    if n == 1 {
        return points[0];
    }

    let t_scaled = t * (n - 1) as f32;
    let i = (t_scaled as usize).min(n - 2);
    let local_t = t_scaled - i as f32;

    let p0 = if i > 0 {
        points[i - 1]
    } else {
        2.0 * points[0] - points[1]
    };
    let p1 = points[i];
    let p2 = points[i + 1];
    let p3 = if i + 2 < n {
        points[i + 2]
    } else {
        2.0 * points[n - 1] - points[n - 2]
    };

    centripetal_catmull_rom(p0, p1, p2, p3, local_t)
}

/// Centripetal Catmull-Rom interpolation between p1 and p2 at parameter t in [0,1].
/// Alpha = 0.5 gives centripetal parameterization (best curvature continuity).
fn centripetal_catmull_rom(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
    fn knot_interval(a: Vec2, b: Vec2) -> f32 {
        (b - a).length().sqrt().max(0.001)
    }

    // Knot values: k0=0, k1, k2, k3 spaced by sqrt(chord length)
    let k1 = knot_interval(p0, p1);
    let k2 = k1 + knot_interval(p1, p2);
    let k3 = k2 + knot_interval(p2, p3);

    let u = k1 + t * (k2 - k1);

    // Barry-Goldman pyramid with explicit knot values
    let a1 = p0 * ((k1 - u) / k1) + p1 * (u / k1);
    let a2 = p1 * ((k2 - u) / (k2 - k1)) + p2 * ((u - k1) / (k2 - k1));
    let a3 = p2 * ((k3 - u) / (k3 - k2)) + p3 * ((u - k2) / (k3 - k2));

    let b1 = a1 * ((k2 - u) / k2) + a2 * (u / k2);
    let b2 = a2 * ((k3 - u) / (k3 - k1)) + a3 * ((u - k1) / (k3 - k1));

    b1 * ((k2 - u) / (k2 - k1)) + b2 * ((u - k1) / (k2 - k1))
}

/// Check if adding `candidate` as a new waypoint creates a turn that's too sharp.
/// Returns true if the angle is acceptable.
fn waypoint_angle_ok(waypoints: &[Vec2], candidate: Vec2) -> bool {
    let n = waypoints.len();
    if n < 2 {
        return true;
    } // need at least 2 existing points to measure an angle

    let prev = waypoints[n - 1];
    let prev2 = waypoints[n - 2];

    let seg_in = prev - prev2;
    let seg_out = candidate - prev;

    let len_in = seg_in.length();
    let len_out = seg_out.length();
    if len_in < 1.0 || len_out < 1.0 {
        return false;
    } // degenerate

    // Angle between the two segments (0 = straight ahead, PI = U-turn)
    let cos_angle = seg_in.dot(seg_out) / (len_in * len_out);
    let angle = cos_angle.clamp(-1.0, 1.0).acos(); // angle of deviation

    // Also check that the turn can be achieved given the segment length.
    // Min turning radius at cruise speed: R = v / omega_max
    let cruise_speed = SHIP_MAX_SPEED * 0.6;
    let r_min = cruise_speed / SHIP_MAX_ANGULAR_SPEED;
    // The arc length needed for a turn of `angle` at radius r_min
    let arc_needed = r_min * angle;
    // The shorter segment must be long enough to accommodate the arc
    let shorter_seg = len_in.min(len_out);

    angle <= (std::f32::consts::PI - MIN_WAYPOINT_ANGLE) && shorter_seg >= arc_needed * 0.5
}

/// Compute curvature at each sample point using the discrete Menger curvature formula.
fn compute_curvatures(path: &[Vec2]) -> Vec<f32> {
    let n = path.len();
    if n < 3 {
        return vec![0.0; n];
    }

    let mut curvatures = Vec::with_capacity(n);
    curvatures.push(0.0); // first point: no curvature
    for i in 1..n - 1 {
        let a = path[i - 1];
        let b = path[i];
        let c = path[i + 1];
        // Menger curvature: κ = 2 * |cross(ab, ac)| / (|ab| * |bc| * |ac|)
        let ab = b - a;
        let bc = c - b;
        let ac = c - a;
        let cross = ab.x * ac.y - ab.y * ac.x;
        let denom = ab.length() * bc.length() * ac.length();
        if denom > 0.001 {
            curvatures.push((2.0 * cross.abs()) / denom);
        } else {
            curvatures.push(0.0);
        }
    }
    curvatures.push(0.0); // last point
    curvatures
}

/// Compute a braking-aware speed profile for the path.
///
/// 1. Forward pass: at each point, cap speed by curvature and by how fast we can
///    accelerate from the previous point.
/// 2. Backward pass: ensure we can decelerate in time for every upcoming slow section.
///    Uses v² = v_next² + 2·a·Δs (kinematic braking equation).
/// 3. End of path: speed ramps to zero.
fn compute_speed_profile(curvatures: &[f32], arc_lengths: &[f32]) -> Vec<f32> {
    let n = curvatures.len();
    if n == 0 {
        return vec![];
    }

    let accel = SHIP_THRUST * 0.8; // effective acceleration (conservative)
    let decel = SHIP_STABILIZE_DECEL;

    // Curvature-based max speed at each point (with safety margin)
    // Smooth curvatures forward: at each point, use the max curvature
    // within a look-ahead window. This makes the ship anticipate curves.
    let smooth_window = 15; // ~12% of 128-sample path
    let mut max_curvature: Vec<f32> = vec![0.0; n];
    for i in 0..n {
        let end = (i + smooth_window).min(n);
        let mut peak = curvatures[i];
        for j in i..end {
            peak = peak.max(curvatures[j]);
        }
        max_curvature[i] = peak;
    }

    let mut profile: Vec<f32> = max_curvature
        .iter()
        .map(|&k| {
            if k > 0.001 {
                // v_safe = ω_max / κ, with 0.35 safety margin
                (SHIP_MAX_ANGULAR_SPEED * 0.35 / k).min(SHIP_MAX_SPEED * 0.7)
            } else {
                SHIP_MAX_SPEED * 0.65
            }
        })
        .collect();

    // Last point: must stop
    profile[n - 1] = 0.0;

    // Forward pass: can't exceed what we could accelerate to from previous point
    // v² = v_prev² + 2·a·Δs
    for i in 1..n {
        let ds = arc_lengths[i] - arc_lengths[i - 1];
        let v_max_from_accel = (profile[i - 1] * profile[i - 1] + 2.0 * accel * ds).sqrt();
        profile[i] = profile[i].min(v_max_from_accel);
    }

    // Backward pass: must be able to brake in time
    // v² = v_next² + 2·decel·Δs
    for i in (0..n - 1).rev() {
        let ds = arc_lengths[i + 1] - arc_lengths[i];
        let v_max_from_brake = (profile[i + 1] * profile[i + 1] + 2.0 * decel * ds).sqrt();
        profile[i] = profile[i].min(v_max_from_brake);
    }

    profile
}

/// Compute cumulative arc length at each path point. arc_lengths[0] = 0.
fn compute_arc_lengths(path: &[Vec2]) -> Vec<f32> {
    let mut lengths = Vec::with_capacity(path.len());
    lengths.push(0.0);
    for i in 1..path.len() {
        lengths.push(lengths[i - 1] + (path[i] - path[i - 1]).length());
    }
    lengths
}

/// Convert an arc-length distance from a fractional index into a fractional index offset.
/// Walks forward along `arc_lengths` from `from_idx` by `dist` units and returns the new index.
fn advance_by_arc_length(arc_lengths: &[f32], from_idx: f32, dist: f32) -> f32 {
    let n = arc_lengths.len();
    if n < 2 {
        return from_idx;
    }
    let max_idx = (n - 1) as f32;

    // Arc length at from_idx (interpolated)
    let i = (from_idx as usize).min(n - 2);
    let frac = from_idx - i as f32;
    let arc_at_from = arc_lengths[i] + frac * (arc_lengths[i + 1] - arc_lengths[i]);
    let target_arc = arc_at_from + dist;

    // Binary search for the index where arc_lengths[idx] >= target_arc
    let mut lo = i;
    let mut hi = n - 1;
    while lo < hi {
        let mid = (lo + hi) / 2;
        if arc_lengths[mid] < target_arc {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    // Interpolate within the segment
    if lo == 0 {
        return 0.0;
    }
    let seg_start = arc_lengths[lo - 1];
    let seg_end = arc_lengths[lo];
    let seg_len = seg_end - seg_start;
    if seg_len > 0.001 {
        ((lo - 1) as f32 + (target_arc - seg_start) / seg_len).min(max_idx)
    } else {
        (lo as f32).min(max_idx)
    }
}

fn rebuild_route_path(planner: &mut RoutePlanner) {
    planner.path.clear();
    planner.curvatures.clear();
    if planner.waypoints.len() >= 2 {
        let wps = &planner.waypoints;
        let mut path = Vec::with_capacity(ROUTE_SAMPLE_COUNT);
        for i in 0..ROUTE_SAMPLE_COUNT {
            let t = i as f32 / (ROUTE_SAMPLE_COUNT - 1) as f32;
            path.push(catmull_rom_sample(wps, t));
        }
        let curvatures = compute_curvatures(&path);
        planner.path = path;
        planner.curvatures = curvatures;
    }
}

/// Interpolate a position on the path at a fractional index.
fn path_lerp(path: &[Vec2], idx: f32) -> Vec2 {
    let i = (idx as usize).min(path.len().saturating_sub(2));
    let frac = idx - i as f32;
    path[i] + (path[(i + 1).min(path.len() - 1)] - path[i]) * frac
}

/// Find the closest point on the path to `pos`, starting search from `start_idx`.
/// Returns the fractional index.
fn find_closest_on_path(path: &[Vec2], pos: Vec2, start_idx: f32) -> f32 {
    let start = (start_idx as usize).saturating_sub(5);
    let end = (start + 40).min(path.len() - 1); // search wider window
    let mut best_idx = start_idx;
    let mut best_dist = f32::MAX;

    for i in start..end {
        let a = path[i];
        let b = path[i + 1];
        let ab = b - a;
        let ab_len_sq = ab.length_squared();
        let t = if ab_len_sq > 0.001 {
            ((pos - a).dot(ab) / ab_len_sq).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let proj = a + ab * t;
        let d = (pos - proj).length_squared();
        if d < best_dist {
            best_dist = d;
            best_idx = i as f32 + t;
        }
    }
    best_idx
}

/// Compute remaining arc length from fractional index to end of path.
fn remaining_arc_length(path: &[Vec2], from_idx: f32) -> f32 {
    let i = (from_idx as usize).min(path.len().saturating_sub(2));
    let frac = from_idx - i as f32;
    let first_seg = path[(i + 1).min(path.len() - 1)] - path[i];
    let mut total = first_seg.length() * (1.0 - frac);
    for j in (i + 1)..path.len().saturating_sub(1) {
        total += (path[j + 1] - path[j]).length();
    }
    total
}

/// Handle CTRL press/release and mouse clicks for route planning.
fn route_planning_input(
    mut commands: Commands,
    mut planner: ResMut<RoutePlanner>,
    keypress: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    ship_query: Query<(Entity, &Transform), With<LocalShip>>,
    route_query: Query<Entity, With<RouteFollowing>>,
) {
    let ctrl_held =
        keypress.pressed(KeyCode::ControlLeft) || keypress.pressed(KeyCode::ControlRight);
    let ctrl_just_pressed =
        keypress.just_pressed(KeyCode::ControlLeft) || keypress.just_pressed(KeyCode::ControlRight);
    let ctrl_just_released = keypress.just_released(KeyCode::ControlLeft)
        || keypress.just_released(KeyCode::ControlRight);

    if ctrl_just_pressed {
        for entity in route_query.iter() {
            commands.entity(entity).remove::<RouteFollowing>();
        }
        planner.active = true;
        planner.waypoints.clear();
        planner.path.clear();
        planner.curvatures.clear();
        planner.last_rejected = false;
        planner.target_zoom = ROUTE_ZOOM_SCALE;

        if let Ok((_entity, ship_tf)) = ship_query.single() {
            planner.waypoints.push(ship_tf.translation.truncate());
        }
    }

    // Left-click adds waypoint (with angle validation)
    if planner.active
        && mouse_button.just_pressed(MouseButton::Left)
        && let Some(world_pos) = cursor_world_pos(&windows, &camera_query)
    {
        if waypoint_angle_ok(&planner.waypoints, world_pos) {
            planner.waypoints.push(world_pos);
            planner.last_rejected = false;
            rebuild_route_path(&mut planner);
        } else {
            planner.last_rejected = true;
        }
    }

    // Right-click removes last waypoint
    if planner.active
        && mouse_button.just_pressed(MouseButton::Right)
        && planner.waypoints.len() > 1
    {
        planner.waypoints.pop();
        planner.last_rejected = false;
        rebuild_route_path(&mut planner);
    }

    // On CTRL release, commit the route
    if ctrl_just_released && planner.active {
        planner.active = false;
        planner.target_zoom = 1.0;

        if planner.path.len() >= 2
            && let Ok((entity, _)) = ship_query.single()
        {
            let arc_lengths = compute_arc_lengths(&planner.path);
            let speed_profile = compute_speed_profile(&planner.curvatures, &arc_lengths);
            commands.entity(entity).insert(RouteFollowing {
                path: planner.path.clone(),
                curvatures: planner.curvatures.clone(),
                arc_lengths,
                speed_profile,
                progress: 0.0,
                cte_integral: 0.0,
            });
        }
        planner.waypoints.clear();
        planner.path.clear();
        planner.curvatures.clear();
    }

    if !ctrl_held && planner.active {
        planner.active = false;
        planner.target_zoom = 1.0;
    }
}

/// Smoothly animate camera zoom for route planning.
fn route_zoom(
    mut planner: ResMut<RoutePlanner>,
    mut camera_query: Query<&mut Projection, With<Camera2d>>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    planner.current_zoom +=
        (planner.target_zoom - planner.current_zoom) * (ROUTE_ZOOM_SPEED * dt).min(1.0);

    if (planner.current_zoom - planner.target_zoom).abs() < 0.001 {
        planner.current_zoom = planner.target_zoom;
    }

    if planner.current_zoom <= 0.01 {
        return;
    }

    let Ok(mut projection) = camera_query.single_mut() else {
        return;
    };
    if let Projection::Orthographic(ref mut ortho) = *projection {
        ortho.scale = planner.current_zoom;
    }
}

/// Draw the planned route using gizmos, with curvature-colored segments.
fn render_route_gizmos(
    planner: Res<RoutePlanner>,
    route_query: Query<&RouteFollowing>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    mut gizmos: Gizmos,
) {
    let (path, curvatures, is_planning) = if planner.active && planner.path.len() >= 2 {
        (&planner.path, &planner.curvatures, true)
    } else if let Ok(following) = route_query.single() {
        (&following.path, &following.curvatures, false)
    } else {
        return;
    };

    if path.len() < 2 {
        return;
    }

    // Max curvature the ship can handle at cruise speed: κ_max = ω_max / v
    let cruise_speed = SHIP_MAX_SPEED * 0.6;
    let max_curvature = SHIP_MAX_ANGULAR_SPEED / cruise_speed;

    for i in 0..path.len() - 1 {
        let k = if i < curvatures.len() {
            curvatures[i]
        } else {
            0.0
        };
        let ratio = (k / max_curvature).clamp(0.0, 1.0);

        // Green → yellow → red based on curvature tightness
        let color = if is_planning {
            Color::srgba(
                0.2 + 0.7 * ratio,
                0.8 - 0.5 * ratio,
                0.2 * (1.0 - ratio),
                0.6,
            )
        } else {
            Color::srgba(0.2, 0.5 + 0.3 * (1.0 - ratio), 0.8, 0.4)
        };
        gizmos.line_2d(path[i], path[i + 1], color);
    }

    // Draw waypoints as crosses while planning
    if is_planning {
        let wp_color = Color::srgba(0.9, 0.9, 0.3, 0.8);
        let scale = planner.current_zoom.max(1.0);
        for &wp in &planner.waypoints {
            let s = 8.0 * scale;
            gizmos.line_2d(wp - Vec2::X * s, wp + Vec2::X * s, wp_color);
            gizmos.line_2d(wp - Vec2::Y * s, wp + Vec2::Y * s, wp_color);
        }

        // Show rejection indicator: red X at cursor position
        if planner.last_rejected
            && let Some(cursor_world) = cursor_world_pos(&windows, &camera_query)
        {
            let s = 12.0 * scale;
            let red = Color::srgba(1.0, 0.2, 0.2, 0.8);
            gizmos.line_2d(
                cursor_world + Vec2::new(-s, -s),
                cursor_world + Vec2::new(s, s),
                red,
            );
            gizmos.line_2d(
                cursor_world + Vec2::new(-s, s),
                cursor_world + Vec2::new(s, -s),
                red,
            );
        }
    }
}

/// Compute the unit tangent direction of the path at a fractional index.
fn path_tangent(path: &[Vec2], idx: f32) -> Vec2 {
    let i = (idx as usize).min(path.len().saturating_sub(2));
    let next = (i + 1).min(path.len() - 1);
    let dir = path[next] - path[i];
    let len = dir.length();
    if len > 0.001 { dir / len } else { Vec2::Y }
}

/// Compute cross-track error: signed perpendicular distance from ship to path.
/// Positive = ship is to the LEFT of the path direction.
fn cross_track_error(path: &[Vec2], ship_pos: Vec2, progress: f32) -> f32 {
    let nearest = path_lerp(path, progress);
    let tangent = path_tangent(path, progress);
    // Normal = tangent rotated 90° CCW
    let normal = Vec2::new(-tangent.y, tangent.x);
    (ship_pos - nearest).dot(normal)
}

/// Proportional autopilot with continuous throttle, rotation, strafe, and stabilize.
fn route_follow(
    mut commands: Commands,
    mut ship_query: Query<
        (
            Entity,
            &Transform,
            &LinearVelocity,
            &AngularVelocity,
            &mut RouteFollowing,
        ),
        With<LocalShip>,
    >,
    mut input_query: Query<&mut ActionState<ShipInput>, With<InputMarker<ShipInput>>>,
    keypress: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
) {
    let Ok((entity, ship_tf, lin_vel, _ang_vel, mut following)) = ship_query.single_mut() else {
        return;
    };

    // Cancel on any movement key
    let manual_override = keypress.pressed(KeyCode::KeyW)
        || keypress.pressed(KeyCode::KeyS)
        || keypress.pressed(KeyCode::KeyA)
        || keypress.pressed(KeyCode::KeyD)
        || keypress.pressed(KeyCode::KeyQ)
        || keypress.pressed(KeyCode::KeyE);

    if manual_override {
        commands.entity(entity).remove::<RouteFollowing>();
        return;
    }

    let max_idx = (following.path.len() - 1) as f32;

    if following.progress >= max_idx - 0.1 {
        commands.entity(entity).remove::<RouteFollowing>();
        for mut action_state in input_query.iter_mut() {
            action_state.0 = ShipInput {
                stabilize: 1.0,
                ..default()
            };
        }
        return;
    }

    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;
    let ship_pos = ship_tf.translation.truncate();
    let speed = lin_vel.0.length();

    // 1. Update progress (mutable write first)
    let proj = find_closest_on_path(&following.path, ship_pos, following.progress);
    following.progress = proj.max(following.progress);

    // 2. Cross-track error (signed, positive = left of path)
    let cte = cross_track_error(&following.path, ship_pos, following.progress);

    // 3. Update CTE integral with anti-windup
    let prev_sign = following.cte_integral.signum();
    let cte_sign = cte.signum();
    if prev_sign != 0.0 && cte_sign != 0.0 && prev_sign != cte_sign {
        following.cte_integral = 0.0;
    }
    following.cte_integral = (following.cte_integral + cte * dt).clamp(-150.0, 150.0);

    let progress = following.progress;
    let cte_integral = following.cte_integral;
    let path = &following.path;
    let curvatures = &following.curvatures;
    let arc_lengths = &following.arc_lengths;

    let tangent_here = path_tangent(path, progress);

    // 4. PURE PURSUIT: look ahead along the path, aim directly at that point
    let curvature_here = curvatures[progress as usize];
    let look_ahead_base = (speed * LOOK_AHEAD_TIME).clamp(LOOK_AHEAD_MIN, LOOK_AHEAD_MAX);
    let curvature_factor = (1.0 / (1.0 + curvature_here * 200.0)).clamp(0.4, 1.0);
    let look_ahead_dist = look_ahead_base * curvature_factor;
    let look_ahead_idx = advance_by_arc_length(arc_lengths, progress, look_ahead_dist).min(max_idx);
    let look_ahead_pos = path_lerp(path, look_ahead_idx);

    // 5. Desired heading: always aim at the look-ahead point (pure pursuit)
    //    This naturally corrects cross-track error without needing blend tuning.
    let to_target = look_ahead_pos - ship_pos;
    let desired_angle = if to_target.length_squared() > 1.0 {
        to_target.y.atan2(to_target.x)
    } else {
        tangent_here.y.atan2(tangent_here.x)
    };

    // 6. Heading error — derive from transform forward vector directly (robust)
    let fwd_3d = ship_tf.rotation * Vec3::Y; // ship mesh Y+ = forward
    let ship_heading = fwd_3d.y.atan2(fwd_3d.x);
    let heading_err = wrap_angle(desired_angle - ship_heading);

    // 7. ROTATION CONTROL: bang-bang with damping
    let alpha = SHIP_ANGULAR_DECEL;
    let omega_fb = heading_err.signum() * (2.0 * alpha * heading_err.abs()).sqrt();
    let omega_desired = omega_fb.clamp(-SHIP_MAX_ANGULAR_SPEED, SHIP_MAX_ANGULAR_SPEED);
    let rotate = (omega_desired / SHIP_MAX_ANGULAR_SPEED).clamp(-1.0, 1.0);

    // 8. STRAFE: correct lateral drift when roughly aligned with path
    let path_normal = Vec2::new(-tangent_here.y, tangent_here.x);
    let ship_right_3d = ship_tf.rotation * Vec3::X;
    let ship_right = Vec2::new(ship_right_3d.x, ship_right_3d.y);
    let normal_in_ship = path_normal.dot(ship_right);

    let lateral_vel = lin_vel.0.dot(path_normal);
    let cte_in_ship_right = cte * normal_in_ship;
    let integral_in_ship = cte_integral * normal_in_ship;

    let k_p = 0.04;
    let k_i = 0.008;
    let k_d = 0.06;
    let strafe_cmd =
        k_p * cte_in_ship_right + k_i * integral_in_ship + k_d * lateral_vel * normal_in_ship;
    // Only strafe when heading is roughly correct (within ±45°)
    let alignment_scale = (1.0 - (heading_err.abs() / std::f32::consts::FRAC_PI_4)).clamp(0.0, 1.0);
    let strafe = (strafe_cmd * alignment_scale).clamp(-1.0, 1.0);

    // 9. SPEED FROM PRECOMPUTED PROFILE
    let speed_profile = &following.speed_profile;
    let idx_i = (progress as usize).min(speed_profile.len().saturating_sub(2));
    let idx_frac = progress - idx_i as f32;
    let target_speed = speed_profile[idx_i]
        + idx_frac
            * (speed_profile[(idx_i + 1).min(speed_profile.len() - 1)] - speed_profile[idx_i]);

    // 10. VELOCITY ALIGNMENT — how much velocity is along the path tangent
    let vel_alignment = if speed > 5.0 {
        lin_vel.0.dot(tangent_here) / speed
    } else {
        1.0
    };

    // 11. THRUST AND STABILIZE
    let speed_error = target_speed - speed;
    // Only gate thrust on heading — don't double-gate with alignment
    let heading_factor = (1.0 - heading_err.abs() / std::f32::consts::FRAC_PI_3).clamp(0.0, 1.0);

    let remaining = remaining_arc_length(path, progress);
    let stopping_dist = speed * speed / (2.0 * SHIP_STABILIZE_DECEL);

    let thrust_forward = if speed_error > 0.0 && remaining > stopping_dist * 1.5 {
        (speed_error / 80.0).clamp(0.0, 1.0) * heading_factor
    } else {
        0.0
    };

    // Brake when too fast or velocity is misaligned with path
    let speed_excess = (speed - target_speed).max(0.0);
    let sideslip_brake = if vel_alignment < 0.7 && speed > 15.0 {
        (0.7 - vel_alignment) * 1.5
    } else {
        0.0
    };
    let stabilize = ((speed_excess / 60.0) + sideslip_brake).clamp(0.0, 1.0);

    // Compute aim angle from mouse cursor
    let aim_angle = cursor_world_pos(&windows, &camera_query)
        .and_then(|world_pos| {
            let delta = world_pos - ship_pos;
            (delta.length_squared() > 1.0).then(|| delta.y.atan2(delta.x))
        })
        .unwrap_or(desired_angle);

    for mut action_state in input_query.iter_mut() {
        action_state.0 = ShipInput {
            thrust_forward,
            thrust_backward: 0.0,
            rotate,
            strafe,
            afterburner: false,
            stabilize,
            fire: mouse_button.pressed(MouseButton::Left),
            drop_mine: keypress.just_pressed(KeyCode::KeyX),
            aim_angle,
            class_request: 0,
        };
    }
}
