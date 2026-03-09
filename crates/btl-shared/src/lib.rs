use avian2d::prelude::*;
use bevy::prelude::*;
use lightyear::avian2d::plugin::{AvianReplicationMode, LightyearAvianPlugin};
use lightyear::prelude::input::native::ActionState;
use lightyear::prelude::*;
pub use avian2d::prelude::{Position, Rotation};
pub use lightyear::frame_interpolation::prelude::{FrameInterpolate, FrameInterpolationPlugin};

use btl_protocol::*;

// --- Ship constants ---

pub const SHIP_THRUST: f32 = 200.0;
pub const SHIP_AFTERBURNER_THRUST: f32 = 500.0;
pub const SHIP_RADIUS: f32 = 16.0;
pub const SHIP_MAX_SPEED: f32 = 600.0;
pub const SHIP_MAX_ANGULAR_SPEED: f32 = 6.0;
pub const SHIP_STABILIZE_DECEL: f32 = 60.0;
pub const SHIP_STABILIZE_ANG_DECEL: f32 = 10.0;
pub const SHIP_ANGULAR_DECEL: f32 = 20.0;
pub const SHIP_STRAFE_THRUST: f32 = 120.0;

// --- Map constants ---

pub const MAP_RADIUS: f32 = 6000.0;
// Ships start slowing in the boundary zone and get reflected
const BOUNDARY_ZONE: f32 = 200.0;
const BOUNDARY_REFLECT_SPEED: f32 = 50.0;

// --- Shared plugin ---

pub struct SharedPlugin;

impl Plugin for SharedPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ProtocolPlugin);

        // Game components need prediction so they're synced to the Predicted entity
        app.register_component::<PlayerId>().add_prediction();
        app.register_component::<Team>().add_prediction();

        // Register Avian physics components for prediction/interpolation/rollback
        // (requires lightyear_avian2d for Diffable trait impls)
        app.register_component::<Position>()
            .add_prediction()
            .add_should_rollback(|this: &Position, that: &Position| {
                (this.0 - that.0).length() >= 2.0
            })
            .add_linear_interpolation()
            .add_linear_correction_fn();

        app.register_component::<Rotation>()
            .add_prediction()
            .add_should_rollback(|this: &Rotation, that: &Rotation| {
                this.angle_between(*that) >= 0.1
            })
            .add_linear_interpolation()
            .add_linear_correction_fn();

        app.register_component::<LinearVelocity>()
            .add_prediction()
            .add_should_rollback(|this: &LinearVelocity, that: &LinearVelocity| {
                (this.0 - that.0).length() >= 2.0
            });
        app.register_component::<AngularVelocity>()
            .add_prediction()
            .add_should_rollback(|this: &AngularVelocity, that: &AngularVelocity| {
                (this.0 - that.0).abs() >= 0.5
            });

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

        // Frame interpolation for smooth rendering between physics ticks
        app.add_plugins(FrameInterpolationPlugin::<Position>::default());
        app.add_plugins(FrameInterpolationPlugin::<Rotation>::default());

        // No gravity in space
        app.insert_resource(Gravity(Vec2::ZERO));

        // Shared systems run on both client (prediction) and server (authority)
        app.add_systems(FixedUpdate, (apply_ship_input, enforce_map_boundary));
    }
}

// --- Ship bundle ---

pub const SHIP_MASS: f32 = 10.0;

#[derive(Bundle)]
pub struct ShipBundle {
    pub player_id: PlayerId,
    pub team: Team,
    pub rigid_body: RigidBody,
    pub collider: Collider,
    pub restitution: Restitution,
    pub mass: Mass,
    pub angular_inertia: AngularInertia,
    pub position: Position,
    pub rotation: Rotation,
    pub linear_velocity: LinearVelocity,
    pub angular_velocity: AngularVelocity,
    pub linear_damping: LinearDamping,
    pub angular_damping: AngularDamping,
}

impl ShipBundle {
    pub fn new(player_id: lightyear::prelude::PeerId, team: Team, spawn_pos: Vec2) -> Self {
        // Angular inertia for a solid circle: I = 0.5 * m * r^2
        let angular_inertia = 0.5 * SHIP_MASS * SHIP_RADIUS * SHIP_RADIUS;
        Self {
            player_id: PlayerId(player_id),
            team,
            rigid_body: RigidBody::Dynamic,
            collider: Collider::circle(SHIP_RADIUS),
            restitution: Restitution::new(0.8),
            mass: Mass(SHIP_MASS),
            angular_inertia: AngularInertia(angular_inertia),
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
        let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;
        let forward = *rotation * Vec2::Y;

        // Thrust
        let thrust = if input.afterburner {
            SHIP_AFTERBURNER_THRUST
        } else {
            SHIP_THRUST
        };

        if input.thrust_forward {
            lin_vel.0 += forward * thrust * dt;
        }
        if input.thrust_backward {
            lin_vel.0 -= forward * thrust * 0.5 * dt;
        }

        // Strafe (lateral thrusters — weaker than main engine)
        let right = *rotation * Vec2::X;
        if input.strafe_left {
            lin_vel.0 -= right * SHIP_STRAFE_THRUST * dt;
        }
        if input.strafe_right {
            lin_vel.0 += right * SHIP_STRAFE_THRUST * dt;
        }

        // Rotation: input sets desired turn rate, thrusters steer toward it
        let desired_ang = if input.rotate_left && !input.rotate_right {
            SHIP_MAX_ANGULAR_SPEED
        } else if input.rotate_right && !input.rotate_left {
            -SHIP_MAX_ANGULAR_SPEED
        } else if input.stabilize {
            // Stabilize targets zero rotation
            0.0
        } else {
            // No input: keep current angular velocity (pure Newtonian)
            ang_vel.0
        };

        if desired_ang != ang_vel.0 {
            let ang_diff = desired_ang - ang_vel.0;
            let max_change = if input.stabilize {
                SHIP_STABILIZE_ANG_DECEL * dt
            } else {
                SHIP_ANGULAR_DECEL * dt
            };
            if ang_diff.abs() <= max_change {
                ang_vel.0 = desired_ang;
            } else {
                ang_vel.0 += ang_diff.signum() * max_change;
            }
        }

        // Stabilize: fire retro-thrusters to kill linear velocity
        if input.stabilize {
            let speed = lin_vel.0.length();
            if speed > 0.1 {
                let dir = lin_vel.0 / speed;
                let decel = (SHIP_STABILIZE_DECEL * dt).min(speed);
                lin_vel.0 -= dir * decel;
            } else {
                lin_vel.0 = Vec2::ZERO;
            }
        }

        // Clamp linear speed
        let speed = lin_vel.0.length();
        let max_speed = if input.afterburner { SHIP_MAX_SPEED * 1.5 } else { SHIP_MAX_SPEED };
        if speed > max_speed {
            lin_vel.0 = lin_vel.0.normalize() * max_speed;
        }
    }
}

/// Soft boundary: ships entering the boundary zone get slowed and reflected inward.
fn enforce_map_boundary(
    mut query: Query<(&Position, &mut LinearVelocity)>,
) {
    let inner_radius = MAP_RADIUS - BOUNDARY_ZONE;

    for (pos, mut lin_vel) in query.iter_mut() {
        let dist = pos.0.length();
        if dist <= inner_radius {
            continue;
        }

        let dir_from_center = pos.0 / dist;
        let penetration = (dist - inner_radius) / BOUNDARY_ZONE; // 0 at inner edge, 1 at map edge
        let t = penetration.clamp(0.0, 1.0);

        // Aggressively drag speed down the deeper they are
        let drag = 1.0 - t * 0.15; // lose up to 15% speed per tick
        lin_vel.0 *= drag;

        // At the hard edge, reflect velocity inward
        if dist >= MAP_RADIUS {
            let outward_component = lin_vel.0.dot(dir_from_center);
            if outward_component > 0.0 {
                // Remove outward component and add inward bounce
                lin_vel.0 -= dir_from_center * outward_component;
                lin_vel.0 -= dir_from_center * BOUNDARY_REFLECT_SPEED;
            }
        }
    }
}
