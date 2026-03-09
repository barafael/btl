use std::net::SocketAddr;
use std::time::Duration;

use bevy::prelude::*;
use lightyear::prelude::client::*;
use lightyear::prelude::client::input::InputSystems;
use lightyear::prelude::input::native::{ActionState, InputMarker};
use lightyear::prelude::*;
use lightyear::webtransport::prelude::client::WebTransportClientIo;

use btl_protocol::*;
use btl_shared::SHIP_RADIUS;

pub struct ClientPlugin {
    pub server_addr: SocketAddr,
    pub client_id: u64,
}

impl Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        let server_addr = self.server_addr;
        let client_id = self.client_id;

        app.add_plugins(lightyear::prelude::client::ClientPlugins {
            tick_duration: Duration::from_secs_f64(1.0 / FIXED_TIMESTEP_HZ),
        });

        app.insert_resource(ClientConnectionConfig {
            server_addr,
            client_id,
        });

        app.add_systems(Startup, connect_to_server);
        app.add_systems(
            FixedPreUpdate,
            buffer_input.in_set(InputSystems::WriteClientInputs),
        );
        app.add_observer(handle_predicted_spawn);
    }
}

#[derive(Resource)]
struct ClientConnectionConfig {
    server_addr: SocketAddr,
    client_id: u64,
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

    // Empty certificate_digest + dangerous_configuration = skip cert validation (dev only)
    let entity = commands
        .spawn((
            Client::default(),
            netcode,
            PeerAddr(config.server_addr),
            WebTransportClientIo {
                certificate_digest: String::new(),
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
            afterburner: keypress.pressed(KeyCode::ShiftLeft),
        };
    }
}

/// When a predicted entity spawns for us, add rendering and input handling.
fn handle_predicted_spawn(
    trigger: On<Add, Predicted>,
    mut commands: Commands,
    query: Query<(&PlayerId, &Team, Has<Controlled>), With<Predicted>>,
) {
    let Ok((player_id, team, is_controlled)) = query.get(trigger.entity) else {
        return;
    };

    let color = match team {
        Team::Red => Color::srgb(1.0, 0.3, 0.3),
        Team::Blue => Color::srgb(0.3, 0.3, 1.0),
    };

    // Add a sprite to visualize the ship
    commands.entity(trigger.entity).insert(Sprite {
        color,
        custom_size: Some(Vec2::splat(SHIP_RADIUS * 2.0)),
        ..default()
    });

    if is_controlled {
        // This is our ship — attach input marker
        commands
            .entity(trigger.entity)
            .insert(InputMarker::<ShipInput>::default());
        info!("Spawned local ship for {:?} on {:?} team", player_id.0, team);
    } else {
        info!(
            "Spawned predicted ship for {:?} on {:?} team",
            player_id.0, team
        );
    }
}
