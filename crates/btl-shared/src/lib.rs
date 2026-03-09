use avian2d::prelude::*;
use bevy::prelude::*;
use lightyear::avian2d::plugin::{AvianReplicationMode, LightyearAvianPlugin};
use lightyear::prelude::input::native::ActionState;

use btl_protocol::*;

// --- Ship constants ---

pub const SHIP_THRUST: f32 = 200.0;
pub const SHIP_AFTERBURNER_THRUST: f32 = 500.0;
pub const SHIP_TORQUE: f32 = 15.0;
pub const SHIP_RADIUS: f32 = 16.0;

// --- Shared plugin ---

pub struct SharedPlugin;

impl Plugin for SharedPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ProtocolPlugin);

        // Lightyear <-> Avian2D integration
        app.add_plugins(LightyearAvianPlugin {
            replication_mode: AvianReplicationMode::Position,
            ..default()
        });

        // Avian physics (disable plugins that lightyear manages)
        app.add_plugins(
            PhysicsPlugins::default()
                .build()
                .disable::<PhysicsTransformPlugin>()
                .disable::<PhysicsInterpolationPlugin>(),
        );

        // No gravity in space
        app.insert_resource(Gravity(Vec2::ZERO));

        // Shared movement system runs on both client (prediction) and server (authority)
        app.add_systems(FixedUpdate, apply_ship_input);
    }
}

// --- Ship bundle ---

#[derive(Bundle)]
pub struct ShipBundle {
    pub player_id: PlayerId,
    pub team: Team,
    pub rigid_body: RigidBody,
    pub collider: Collider,
    pub position: Position,
    pub rotation: Rotation,
    pub linear_velocity: LinearVelocity,
    pub angular_velocity: AngularVelocity,
    pub linear_damping: LinearDamping,
    pub angular_damping: AngularDamping,
}

impl ShipBundle {
    pub fn new(player_id: lightyear::prelude::PeerId, team: Team, spawn_pos: Vec2) -> Self {
        Self {
            player_id: PlayerId(player_id),
            team,
            rigid_body: RigidBody::Dynamic,
            collider: Collider::circle(SHIP_RADIUS),
            position: Position(spawn_pos),
            rotation: Rotation::default(),
            linear_velocity: LinearVelocity::default(),
            angular_velocity: AngularVelocity::default(),
            linear_damping: LinearDamping(0.0),
            angular_damping: AngularDamping(0.0),
        }
    }
}

// --- Shared movement (runs on client for prediction, server for authority) ---

fn apply_ship_input(
    mut query: Query<(
        &ActionState<ShipInput>,
        &Rotation,
        &mut LinearVelocity,
        &mut AngularVelocity,
    )>,
) {
    for (input, rotation, mut lin_vel, mut ang_vel) in query.iter_mut() {
        let input = &input.0;
        let forward = *rotation * Vec2::Y;

        // Thrust
        let thrust = if input.afterburner {
            SHIP_AFTERBURNER_THRUST
        } else {
            SHIP_THRUST
        };

        if input.thrust_forward {
            lin_vel.0 += forward * thrust * (1.0 / FIXED_TIMESTEP_HZ as f32);
        }
        if input.thrust_backward {
            lin_vel.0 -= forward * thrust * 0.5 * (1.0 / FIXED_TIMESTEP_HZ as f32);
        }

        // Rotation
        if input.rotate_left {
            ang_vel.0 += SHIP_TORQUE * (1.0 / FIXED_TIMESTEP_HZ as f32);
        }
        if input.rotate_right {
            ang_vel.0 -= SHIP_TORQUE * (1.0 / FIXED_TIMESTEP_HZ as f32);
        }
    }
}
