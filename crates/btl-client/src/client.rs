use std::net::SocketAddr;
use std::time::Duration;

use bevy::prelude::*;
use lightyear::prelude::client::*;
use lightyear::prelude::client::input::InputSystems;
use lightyear::prelude::input::native::{ActionState, InputMarker};
use lightyear::prelude::*;
use lightyear::webtransport::prelude::client::WebTransportClientIo;

use avian2d::prelude::*;

use btl_protocol::*;
use btl_shared::{FrameInterpolate, Position, Rotation, SHIP_MASS, SHIP_RADIUS};

/// Marker for the locally controlled ship.
#[derive(Component)]
pub struct LocalShip;

/// Marker to track that we've already initialized rendering for a predicted entity.
#[derive(Component)]
struct ShipInitialized;

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

        app.add_systems(Startup, connect_to_server);
        app.add_systems(
            FixedPreUpdate,
            buffer_input.in_set(InputSystems::WriteClientInputs),
        );
        app.add_observer(log_connected);
        app.add_systems(Update, (init_predicted_ships, init_interpolated_ships, camera_follow_local_ship, update_hud));
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

/// Read keyboard input and write it to the input buffer.
fn buffer_input(
    mut query: Query<&mut ActionState<ShipInput>, With<InputMarker<ShipInput>>>,
    keypress: Res<ButtonInput<KeyCode>>,
) {
    for mut action_state in query.iter_mut() {
        action_state.0 = ShipInput {
            thrust_forward: keypress.pressed(KeyCode::KeyW),
            thrust_backward: keypress.pressed(KeyCode::KeyS),
            rotate_left: keypress.pressed(KeyCode::KeyA),
            rotate_right: keypress.pressed(KeyCode::KeyD),
            strafe_left: keypress.pressed(KeyCode::KeyQ),
            strafe_right: keypress.pressed(KeyCode::KeyE),
            afterburner: keypress.pressed(KeyCode::ShiftLeft),
            stabilize: keypress.pressed(KeyCode::KeyR),
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
        (Entity, &PlayerId, &Team, Has<Controlled>),
        (With<Predicted>, Without<ShipInitialized>),
    >,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, player_id, team, is_controlled) in query.iter() {
        let color = match team {
            Team::Red => Color::srgb(1.0, 0.3, 0.3),
            Team::Blue => Color::srgb(0.3, 0.3, 1.0),
        };

        // Triangle ship mesh pointing up (Y+)
        let r = SHIP_RADIUS;
        let ship_mesh = meshes.add(Triangle2d::new(
            Vec2::new(0.0, r * 1.5),  // nose
            Vec2::new(-r, -r),         // bottom-left
            Vec2::new(r, -r),          // bottom-right
        ));

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
            info!("Spawned local ship for {:?} on {:?} team", player_id.0, team);
        } else {
            info!("Spawned remote ship for {:?} on {:?} team", player_id.0, team);
        }
    }
}

/// Initialize rendering for interpolated (remote) ships.
fn init_interpolated_ships(
    mut commands: Commands,
    query: Query<
        (Entity, &PlayerId, &Team),
        (With<Interpolated>, Without<ShipInitialized>),
    >,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, player_id, team) in query.iter() {
        let color = match team {
            Team::Red => Color::srgb(1.0, 0.3, 0.3),
            Team::Blue => Color::srgb(0.3, 0.3, 1.0),
        };

        let r = SHIP_RADIUS;
        let ship_mesh = meshes.add(Triangle2d::new(
            Vec2::new(0.0, r * 1.5),
            Vec2::new(-r, -r),
            Vec2::new(r, -r),
        ));

        commands.entity(entity).insert((
            Mesh2d(ship_mesh),
            MeshMaterial2d(materials.add(color)),
            ShipInitialized,
        ));

        info!("Spawned interpolated ship for {:?} on {:?} team", player_id.0, team);
    }
}

#[derive(Component)]
struct CoordsText;

fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        CoordsText,
        Text::new("(0, 0)"),
        TextFont {
            font_size: 16.0,
            ..default()
        },
        TextColor(Color::srgba(0.7, 0.7, 0.7, 0.8)),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(12.0),
            left: Val::Percent(50.0),
            ..default()
        },
    ));
}

fn update_hud(
    ship_query: Query<&Transform, With<LocalShip>>,
    mut text_query: Query<&mut Text, With<CoordsText>>,
) {
    let Ok(ship_tf) = ship_query.single() else { return };
    let Ok(mut text) = text_query.single_mut() else { return };
    let x = ship_tf.translation.x as i32;
    let y = ship_tf.translation.y as i32;
    **text = format!("({x}, {y})");
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
