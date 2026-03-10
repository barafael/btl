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
    Ammo, Asteroid, FrameInterpolate, MINE_RADIUS, MINE_TRIGGER_RADIUS, Mine, Position, Projectile,
    Rotation, SHIP_MASS, SHIP_RADIUS,
};

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

/// Marker for the gun barrel child entity.
#[derive(Component)]
struct GunBarrel;

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
                update_projectile_visuals,
                update_mine_visuals,
                update_gun_barrels,
                route_planning_input,
                route_zoom,
                camera_follow_local_ship,
                update_hud,
                render_route_gizmos,
            ),
        );
        app.add_systems(Startup, spawn_hud);
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
) {
    // Don't overwrite inputs while route following or planning
    if route_following.single().is_ok() || planner.active {
        return;
    }

    // Compute aim angle: direction from ship to mouse cursor in world space
    let aim_angle = (|| {
        let window = windows.single().ok()?;
        let cursor_pos = window.cursor_position()?;
        let (camera, cam_gt) = camera_query.single().ok()?;
        let world_pos = camera.viewport_to_world_2d(cam_gt, cursor_pos).ok()?;
        let ship_tf = ship_query.single().ok()?;
        let ship_pos = ship_tf.translation.truncate();
        let delta = world_pos - ship_pos;
        if delta.length_squared() > 1.0 {
            Some(delta.y.atan2(delta.x))
        } else {
            None
        }
    })()
    .unwrap_or(std::f32::consts::FRAC_PI_2); // default: aim up

    // Map keyboard booleans to continuous values (0.0 or 1.0)
    let rotate = match (
        keypress.pressed(KeyCode::KeyA),
        keypress.pressed(KeyCode::KeyD),
    ) {
        (true, false) => 1.0,
        (false, true) => -1.0,
        _ => 0.0,
    };
    let strafe = match (
        keypress.pressed(KeyCode::KeyQ),
        keypress.pressed(KeyCode::KeyE),
    ) {
        (true, false) => 1.0,
        (false, true) => -1.0,
        _ => 0.0,
    };

    for mut action_state in query.iter_mut() {
        action_state.0 = ShipInput {
            thrust_forward: if keypress.pressed(KeyCode::KeyW) {
                1.0
            } else {
                0.0
            },
            thrust_backward: if keypress.pressed(KeyCode::KeyS) {
                1.0
            } else {
                0.0
            },
            rotate,
            strafe,
            afterburner: keypress.pressed(KeyCode::ShiftLeft),
            stabilize: if keypress.pressed(KeyCode::KeyR) {
                1.0
            } else {
                0.0
            },
            fire: mouse_button.pressed(MouseButton::Left),
            drop_mine: keypress.just_pressed(KeyCode::KeyX),
            aim_angle,
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
    query: Query<(Entity, &PlayerId, &Team, Has<Controlled>), UninitPredicted>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, player_id, team, is_controlled) in query.iter() {
        let color = match team {
            Team::Red => Color::srgb(1.0, 0.3, 0.3),
            Team::Blue => Color::srgb(0.3, 0.3, 1.0),
        };

        // Interceptor dart hull
        let ship_mesh = meshes.add(create_interceptor_mesh(SHIP_RADIUS));

        commands.entity(entity).insert((
            Mesh2d(ship_mesh),
            MeshMaterial2d(materials.add(color)),
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

        // Gun barrel: thin gray rectangle, rotates toward mouse cursor
        commands.spawn((
            ChildOf(entity),
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

        if is_controlled {
            // Local ship needs physics components for client-side prediction
            let angular_inertia = 0.5 * SHIP_MASS * SHIP_RADIUS * SHIP_RADIUS;
            commands.entity(entity).insert((
                RigidBody::Dynamic,
                Collider::circle(SHIP_RADIUS),
                Mass(SHIP_MASS),
                AngularInertia(angular_inertia),
                LinearDamping(0.0),
                AngularDamping(0.0),
                InputMarker::<ShipInput>::default(),
                LocalShip,
            ));
            info!(
                "Spawned local ship for {:?} on {:?} team",
                player_id.0, team
            );
        } else {
            info!(
                "Spawned remote ship for {:?} on {:?} team",
                player_id.0, team
            );
        }
    }
}

/// Initialize rendering for interpolated (remote) ships.
fn init_interpolated_ships(
    mut commands: Commands,
    query: Query<(Entity, &PlayerId, &Team), UninitInterpolated>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, player_id, team) in query.iter() {
        let color = match team {
            Team::Red => Color::srgb(1.0, 0.3, 0.3),
            Team::Blue => Color::srgb(0.3, 0.3, 1.0),
        };

        let ship_mesh = meshes.add(create_interceptor_mesh(SHIP_RADIUS));

        commands.entity(entity).insert((
            Mesh2d(ship_mesh),
            MeshMaterial2d(materials.add(color)),
            ShipInitialized,
        ));

        // Gun barrel for remote ships (points forward by default)
        commands.spawn((
            ChildOf(entity),
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

        info!(
            "Spawned interpolated ship for {:?} on {:?} team",
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
/// Per design doc: bright-yellow elongated pellets (~4px long) with motion blur.
fn init_projectiles(
    mut commands: Commands,
    query: Query<(Entity, &Projectile, &LinearVelocity, &Position), Without<ProjectileInitialized>>,
) {
    for (entity, _proj, vel, pos) in query.iter() {
        // Bright yellow HDR pellet color (blooms nicely)
        let color = Color::LinearRgba(LinearRgba::new(3.0, 2.5, 0.8, 1.0));
        let angle = vel.0.y.atan2(vel.0.x);

        commands.entity(entity).insert((
            Sprite {
                color,
                // Elongated pellet: 8px long, 2px wide — reads as a streak at speed
                custom_size: Some(Vec2::new(8.0, 2.0)),
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
type ShipForMineFilter = (With<ShipInitialized>, Without<MineInitialized>, Without<MineCore>, Without<MineShadow>);

/// Pulse mine cores, position shadows, proximity warning, and clean up orphaned children.
fn update_mine_visuals(
    mut commands: Commands,
    mines: Query<(Entity, &Mine, &Transform), With<MineInitialized>>,
    mut cores: Query<(Entity, &MineCore, &mut Transform, &mut MeshMaterial2d<ColorMaterial>), MineCoreFilter>,
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

#[derive(Component)]
struct HudText;

#[derive(Component)]
struct HealthBarFill;

#[derive(Component)]
struct FuelBarFill;

#[derive(Component)]
struct AmmoBarFill;

type HealthBarFilter = (With<HealthBarFill>, Without<FuelBarFill>, Without<AmmoBarFill>, Without<HudText>);
type FuelBarFilter = (With<FuelBarFill>, Without<HealthBarFill>, Without<AmmoBarFill>, Without<HudText>);
type AmmoBarFilter = (With<AmmoBarFill>, Without<HealthBarFill>, Without<FuelBarFill>, Without<HudText>);

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

    // Speed + coords text
    commands.spawn((
        ChildOf(panel),
        HudText,
        Text::new("SPD 0 | (0, 0)"),
        TextFont {
            font_size: 12.0,
            ..default()
        },
        TextColor(Color::srgba(0.7, 0.7, 0.7, 0.8)),
    ));
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

    let t2 = local_t * local_t;
    let t3 = t2 * local_t;

    0.5 * ((2.0 * p1)
        + (-p0 + p2) * local_t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
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
    let mut profile: Vec<f32> = curvatures
        .iter()
        .map(|&k| {
            if k > 0.001 {
                // v_safe = ω_max / κ, with 0.6 margin to allow correction room
                (SHIP_MAX_ANGULAR_SPEED * 0.6 / k).min(SHIP_MAX_SPEED * 0.85)
            } else {
                SHIP_MAX_SPEED * 0.85
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
    let start = (start_idx as usize).saturating_sub(2);
    let end = (start + 20).min(path.len() - 1); // only search nearby
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
    if planner.active && mouse_button.just_pressed(MouseButton::Left)
        && let Some(world_pos) = (|| {
            let window = windows.single().ok()?;
            let cursor_pos = window.cursor_position()?;
            let (camera, cam_gt) = camera_query.single().ok()?;
            camera.viewport_to_world_2d(cam_gt, cursor_pos).ok()
        })() {
            if waypoint_angle_ok(&planner.waypoints, world_pos) {
                planner.waypoints.push(world_pos);
                planner.last_rejected = false;
                rebuild_route_path(&mut planner);
            } else {
                planner.last_rejected = true;
            }
        }

    // Right-click removes last waypoint
    if planner.active && mouse_button.just_pressed(MouseButton::Right)
        && planner.waypoints.len() > 1 {
            planner.waypoints.pop();
            planner.last_rejected = false;
            rebuild_route_path(&mut planner);
        }

    // On CTRL release, commit the route
    if ctrl_just_released && planner.active {
        planner.active = false;
        planner.target_zoom = 1.0;

        if planner.path.len() >= 2
            && let Ok((entity, _)) = ship_query.single() {
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
            && let Some(cursor_world) = (|| {
                let window = windows.single().ok()?;
                let cursor_pos = window.cursor_position()?;
                let (camera, cam_gt) = camera_query.single().ok()?;
                camera.viewport_to_world_2d(cam_gt, cursor_pos).ok()
            })() {
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
    //    Reset on sign change, clamp accumulator
    let prev_sign = following.cte_integral.signum();
    let cte_sign = cte.signum();
    if prev_sign != 0.0 && cte_sign != 0.0 && prev_sign != cte_sign {
        following.cte_integral = 0.0; // zero-crossing reset
    }
    following.cte_integral = (following.cte_integral + cte * dt).clamp(-200.0, 200.0);

    // Snapshot mutable fields, then take immutable refs
    let progress = following.progress;
    let cte_integral = following.cte_integral;
    let path = &following.path;
    let curvatures = &following.curvatures;
    let arc_lengths = &following.arc_lengths;

    let tangent_here = path_tangent(path, progress);

    // 4. ADAPTIVE LOOK-AHEAD using arc lengths
    //    Shorter look-ahead in tight curves, longer on straights
    let curvature_here = curvatures[progress as usize];
    let look_ahead_base = (speed * LOOK_AHEAD_TIME).clamp(LOOK_AHEAD_MIN, LOOK_AHEAD_MAX);
    // Reduce look-ahead proportionally to curvature (tighter curve → look closer)
    let curvature_factor = (1.0 / (1.0 + curvature_here * 200.0)).clamp(0.4, 1.0);
    let look_ahead_dist = look_ahead_base * curvature_factor;
    let look_ahead_idx = advance_by_arc_length(arc_lengths, progress, look_ahead_dist).min(max_idx);
    let look_ahead_pos = path_lerp(path, look_ahead_idx);
    let tangent_ahead = path_tangent(path, look_ahead_idx);

    // 5. Blended desired heading: tangent (on-path) vs pursuit (off-path)
    let pursuit_dir = {
        let d = look_ahead_pos - ship_pos;
        let len = d.length();
        if len > 0.1 { d / len } else { tangent_ahead }
    };
    let blend = (cte.abs() / 80.0).clamp(0.0, 1.0);
    let desired_dir =
        (tangent_ahead * (1.0 - blend) + pursuit_dir * blend).normalize_or(tangent_ahead);
    let desired_angle = desired_dir.y.atan2(desired_dir.x);

    // 6. Heading error
    let (_, _, ship_rot_z) = ship_tf.rotation.to_euler(EulerRot::XYZ);
    let ship_heading = ship_rot_z + std::f32::consts::FRAC_PI_2;
    let heading_err = wrap_angle(desired_angle - ship_heading);

    // 7. ROTATION CONTROL with curvature feedforward
    //    ω_feedforward = speed × κ_ahead anticipates turns
    //    ω_feedback = phase-plane optimal switching for residual heading error
    let alpha = SHIP_ANGULAR_DECEL;
    let kappa_ahead = curvatures[(look_ahead_idx as usize).min(curvatures.len() - 1)];
    // Signed curvature: use cross product of consecutive tangents to get turn direction
    let tangent_mid = path_tangent(path, (progress + look_ahead_idx) * 0.5);
    let cross = tangent_here.x * tangent_mid.y - tangent_here.y * tangent_mid.x;
    let omega_ff = speed * kappa_ahead * cross.signum();

    let omega_fb = heading_err.signum() * (2.0 * alpha * heading_err.abs()).sqrt();
    let omega_desired =
        (omega_ff + omega_fb).clamp(-SHIP_MAX_ANGULAR_SPEED, SHIP_MAX_ANGULAR_SPEED);
    let rotate = (omega_desired / SHIP_MAX_ANGULAR_SPEED).clamp(-1.0, 1.0);

    // 8. STRAFE PID CONTROL
    let path_normal = Vec2::new(-tangent_here.y, tangent_here.x); // left-of-path
    let ship_right = Vec2::new(ship_rot_z.cos(), ship_rot_z.sin()); // ship's local +X
    let normal_in_ship = path_normal.dot(ship_right);

    let lateral_vel = lin_vel.0.dot(path_normal);
    let cte_in_ship_right = cte * normal_in_ship;
    let integral_in_ship = cte_integral * normal_in_ship;

    let k_p = 0.03; // proportional gain
    let k_i = 0.005; // integral gain
    let k_d = 0.05; // derivative (velocity damping) gain
    let strafe_cmd =
        k_p * cte_in_ship_right + k_i * integral_in_ship + k_d * lateral_vel * normal_in_ship;
    let alignment_scale = (1.0 - (heading_err.abs() / std::f32::consts::FRAC_PI_4)).clamp(0.0, 1.0);
    let strafe = (strafe_cmd * alignment_scale).clamp(-1.0, 1.0);

    // 9. SPEED FROM PRECOMPUTED PROFILE
    //    The profile already accounts for curvature limits, braking distances,
    //    and end-of-path deceleration to zero.
    let speed_profile = &following.speed_profile;
    let idx_i = (progress as usize).min(speed_profile.len().saturating_sub(2));
    let idx_frac = progress - idx_i as f32;
    let target_speed = speed_profile[idx_i]
        + idx_frac
            * (speed_profile[(idx_i + 1).min(speed_profile.len() - 1)] - speed_profile[idx_i]);

    // 10. VELOCITY ALIGNMENT
    let vel_alignment = if speed > 5.0 {
        lin_vel.0.dot(tangent_here) / speed
    } else {
        1.0
    };

    // 11. CONTINUOUS THRUST AND STABILIZE
    let speed_error = target_speed - speed;
    let heading_factor = (1.0 - heading_err.abs() / std::f32::consts::FRAC_PI_2).clamp(0.0, 1.0);
    let alignment_factor = ((vel_alignment - 0.3) / 0.7).clamp(0.0, 1.0);

    let remaining = remaining_arc_length(path, progress);
    let stopping_dist = speed * speed / (2.0 * SHIP_STABILIZE_DECEL);

    let thrust_forward = if speed_error > 0.0 && remaining > stopping_dist * 1.2 {
        (speed_error / 100.0).clamp(0.0, 1.0) * heading_factor * alignment_factor
    } else {
        0.0
    };

    let speed_excess = (speed - target_speed).max(0.0);
    let sideslip_brake = if vel_alignment < 0.5 && speed > 20.0 {
        (0.5 - vel_alignment) * 2.0
    } else {
        0.0
    };
    let stabilize = ((speed_excess / 80.0) + sideslip_brake).clamp(0.0, 1.0);

    // Compute aim angle from mouse cursor
    let aim_angle = (|| {
        let window = windows.single().ok()?;
        let cursor_pos = window.cursor_position()?;
        let (camera, cam_gt) = camera_query.single().ok()?;
        let world_pos = camera.viewport_to_world_2d(cam_gt, cursor_pos).ok()?;
        let delta = world_pos - ship_pos;
        if delta.length_squared() > 1.0 {
            Some(delta.y.atan2(delta.x))
        } else {
            None
        }
    })()
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
        };
    }
}
