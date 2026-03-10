pub mod nebula;
pub mod rng;

use avian2d::prelude::*;
pub use avian2d::prelude::{Position, Rotation};
use bevy::prelude::*;
use lightyear::avian2d::plugin::{AvianReplicationMode, LightyearAvianPlugin};
pub use lightyear::frame_interpolation::prelude::{FrameInterpolate, FrameInterpolationPlugin};
use lightyear::prelude::input::native::ActionState;
use lightyear::prelude::*;
use std::ops::DerefMut;

use btl_protocol::*;
pub use btl_protocol::{
    Ammo, Asteroid, FireCooldown, Fuel, Health, Mine, MineCooldown, NebulaSeed, Projectile,
};

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
pub const SHIP_MAX_HEALTH: f32 = 100.0;
pub const SHIP_MAX_FUEL: f32 = 100.0;
/// Fuel consumed per second while afterburner is active
pub const FUEL_BURN_RATE: f32 = 20.0;
/// Fuel regenerated per second when afterburner is off
pub const FUEL_REGEN_RATE: f32 = 8.0;
/// Max autocannon ammo
pub const SHIP_MAX_AMMO: f32 = 60.0;
/// Ammo consumed per autocannon shot
pub const AMMO_COST: f32 = 1.0;
/// Ammo regenerated per second (passive)
pub const AMMO_REGEN_RATE: f32 = 2.0;

// --- Autocannon constants (Interceptor primary weapon) ---

/// Rounds per second
pub const AUTOCANNON_FIRE_RATE: f32 = 8.0;
/// Cooldown between shots
pub const AUTOCANNON_COOLDOWN: f32 = 1.0 / AUTOCANNON_FIRE_RATE;
/// Projectile speed (added to ship velocity)
pub const AUTOCANNON_SPEED: f32 = 800.0;
/// Damage per hit
pub const AUTOCANNON_DAMAGE: f32 = 8.0;
/// Projectile lifetime in seconds
pub const AUTOCANNON_LIFETIME: f32 = 1.5;
/// Projectile collider radius
pub const PROJECTILE_RADIUS: f32 = 3.0;
/// Muzzle offset from ship center (spawn at ship edge)
pub const MUZZLE_OFFSET: f32 = SHIP_RADIUS + PROJECTILE_RADIUS + 2.0;

// --- Mine constants (Interceptor secondary weapon) ---

/// Damage dealt by mine detonation
pub const MINE_DAMAGE: f32 = 40.0;
/// Mine lifetime in seconds
pub const MINE_LIFETIME: f32 = 30.0;
/// Time before mine arms after dropping (seconds)
pub const MINE_ARM_TIME: f32 = 1.0;
/// Proximity trigger radius (distance from mine center to ship center)
pub const MINE_TRIGGER_RADIUS: f32 = 60.0;
/// Visual radius of the mine entity
pub const MINE_RADIUS: f32 = 8.0;
/// Cooldown between mine drops
pub const MINE_COOLDOWN: f32 = 2.0;
/// Max active mines per player
pub const MINE_MAX_ACTIVE: usize = 5;
/// Backward velocity offset when dropping (subtracted from ship velocity)
pub const MINE_DROP_SPEED: f32 = 30.0;

// --- Map constants ---

pub const MAP_RADIUS: f32 = 6000.0;
// Ships start slowing in the boundary zone and get reflected
const BOUNDARY_ZONE: f32 = 200.0;
const BOUNDARY_REFLECT_SPEED: f32 = 50.0;

// --- Tridrant / objective constants ---

/// Number of tridrant sectors (3 = 120° each)
pub const TRIDRANT_COUNT: usize = 3;
/// Objective zones sit at 60% of MAP_RADIUS along each tridrant's bisector
pub const OBJECTIVE_DISTANCE: f32 = MAP_RADIUS * 0.6;
/// Radius of each capture zone
pub const OBJECTIVE_ZONE_RADIUS: f32 = 300.0;

/// Returns the center positions of the 3 objective zones.
/// Tridrant bisectors are at 90°, 210°, 330° (first one points up).
pub fn objective_zone_positions() -> [Vec2; 3] {
    let base_angle = std::f32::consts::FRAC_PI_2; // 90° = up
    let sector = std::f32::consts::TAU / TRIDRANT_COUNT as f32;
    [
        Vec2::new(
            OBJECTIVE_DISTANCE * (base_angle).cos(),
            OBJECTIVE_DISTANCE * (base_angle).sin(),
        ),
        Vec2::new(
            OBJECTIVE_DISTANCE * (base_angle + sector).cos(),
            OBJECTIVE_DISTANCE * (base_angle + sector).sin(),
        ),
        Vec2::new(
            OBJECTIVE_DISTANCE * (base_angle + 2.0 * sector).cos(),
            OBJECTIVE_DISTANCE * (base_angle + 2.0 * sector).sin(),
        ),
    ]
}

/// Returns the angles (in radians) of the tridrant boundary lines.
/// Lines go from center to MAP_RADIUS at 30°, 150°, 270°
/// (midway between the bisectors).
pub fn tridrant_boundary_angles() -> [f32; 3] {
    let base = std::f32::consts::FRAC_PI_2; // bisector at 90°
    let half_sector = std::f32::consts::TAU / (TRIDRANT_COUNT as f32 * 2.0); // 60°
    [
        base + half_sector,                                     // 150°
        base + half_sector + std::f32::consts::TAU / 3.0,       // 270°
        base + half_sector + 2.0 * std::f32::consts::TAU / 3.0, // 30° (wraps)
    ]
}

// --- Asteroid constants ---

pub const ASTEROID_COUNT: usize = 80;
pub const ASTEROID_SEED: u64 = 0xA57E_B01D;
/// Min distance from center — keep spawn area clear
const ASTEROID_MIN_DIST: f32 = 800.0;
/// Max distance — stay inside boundary zone
const ASTEROID_MAX_DIST: f32 = MAP_RADIUS - BOUNDARY_ZONE - 100.0;

/// Size variants: (radius, weight) — weight controls spawn probability
const ASTEROID_SIZES: &[(f32, f32)] = &[
    (20.0, 0.35),  // small
    (50.0, 0.30),  // medium
    (100.0, 0.20), // large
    (200.0, 0.15), // huge
];

/// Deterministic asteroid layout. Returns (position, radius, rotation_radians).
pub fn generate_asteroid_layout() -> Vec<(Vec2, f32, f32)> {
    let mut rng = rng::Rng::new(ASTEROID_SEED);
    let mut asteroids = Vec::with_capacity(ASTEROID_COUNT);

    for _ in 0..ASTEROID_COUNT {
        // Random angle and distance (sqrt for uniform area distribution)
        let angle = rng.next_f32() * std::f32::consts::TAU;
        let t = rng.next_f32();
        let dist = ASTEROID_MIN_DIST + (ASTEROID_MAX_DIST - ASTEROID_MIN_DIST) * t.sqrt();

        let pos = Vec2::new(dist * angle.cos(), dist * angle.sin());

        // Pick size based on weighted random
        let roll = rng.next_f32();
        let mut cumulative = 0.0;
        let mut radius = ASTEROID_SIZES[0].0;
        for &(r, w) in ASTEROID_SIZES {
            cumulative += w;
            if roll < cumulative {
                radius = r;
                break;
            }
        }

        let rotation = rng.next_f32() * std::f32::consts::TAU;

        asteroids.push((pos, radius, rotation));
    }

    asteroids
}

// --- Shared plugin ---

pub struct SharedPlugin;

impl Plugin for SharedPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ProtocolPlugin);

        // Game components need prediction so they're synced to the Predicted entity
        app.register_component::<PlayerId>().add_prediction();
        app.register_component::<Team>().add_prediction();
        // Asteroids are static — no prediction needed, just replication
        app.register_component::<Asteroid>();
        // Health and fuel need prediction for responsive HUD
        app.register_component::<Health>().add_prediction();
        app.register_component::<Fuel>().add_prediction();
        app.register_component::<Ammo>().add_prediction();
        app.register_component::<FireCooldown>().add_prediction();
        app.register_component::<MineCooldown>().add_prediction();
        app.register_component::<Projectile>();
        app.register_component::<Mine>();
        // Nebula seed: static, no prediction needed
        app.register_component::<NebulaSeed>();

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
        app.add_systems(
            FixedUpdate,
            (
                apply_ship_input,
                update_fuel,
                update_ammo,
                tick_cooldown::<FireCooldown>,
                tick_cooldown::<MineCooldown>,
                update_projectile_lifetime,
                check_projectile_asteroid_collisions,
                update_mine_lifetime,
                enforce_map_boundary,
            ),
        );

        // Sync Position→Transform for non-physics entities (projectiles, mines)
        // Avian only syncs entities with RigidBody; these don't have one.
        app.add_systems(PostUpdate, sync_position_to_transform);
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
    pub health: Health,
    pub fuel: Fuel,
    pub ammo: Ammo,
    pub fire_cooldown: FireCooldown,
    pub mine_cooldown: MineCooldown,
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
            health: Health::new(SHIP_MAX_HEALTH),
            fuel: Fuel::new(SHIP_MAX_FUEL),
            ammo: Ammo::new(SHIP_MAX_AMMO),
            fire_cooldown: FireCooldown::default(),
            mine_cooldown: MineCooldown::default(),
        }
    }
}

// --- Shared movement (runs on client for prediction, server for authority) ---

fn apply_ship_input(
    mut query: Query<(
        &ActionState<ShipInput>,
        &Rotation,
        &Fuel,
        &mut LinearVelocity,
        &mut AngularVelocity,
    )>,
) {
    for (input, rotation, fuel, mut lin_vel, mut ang_vel) in query.iter_mut() {
        let input = &input.0;
        let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;
        let forward = *rotation * Vec2::Y;
        let right = *rotation * Vec2::X;

        // Clamp continuous inputs to valid ranges
        let fwd = input.thrust_forward.clamp(0.0, 1.0);
        let bwd = input.thrust_backward.clamp(0.0, 1.0);
        let rot = input.rotate.clamp(-1.0, 1.0);
        let strf = input.strafe.clamp(-1.0, 1.0);
        let stab = input.stabilize.clamp(0.0, 1.0);

        // Afterburner only works with fuel
        let afterburner_active = input.afterburner && fuel.current > 0.0;

        // Thrust (continuous throttle)
        let thrust = if afterburner_active {
            SHIP_AFTERBURNER_THRUST
        } else {
            SHIP_THRUST
        };

        lin_vel.0 += forward * thrust * fwd * dt;
        lin_vel.0 -= forward * thrust * 0.5 * bwd * dt;

        // Strafe (continuous, positive = left, negative = right)
        lin_vel.0 -= right * SHIP_STRAFE_THRUST * strf * dt;

        // Rotation: continuous input sets desired turn rate as fraction of max
        let has_rotation_input = rot.abs() > 0.01;
        let desired_ang = if has_rotation_input {
            rot * SHIP_MAX_ANGULAR_SPEED
        } else if stab > 0.01 {
            0.0
        } else {
            ang_vel.0
        };

        if desired_ang != ang_vel.0 {
            let ang_diff = desired_ang - ang_vel.0;
            let max_change = if stab > 0.01 && !has_rotation_input {
                SHIP_STABILIZE_ANG_DECEL * stab * dt
            } else {
                SHIP_ANGULAR_DECEL * dt
            };
            if ang_diff.abs() <= max_change {
                ang_vel.0 = desired_ang;
            } else {
                ang_vel.0 += ang_diff.signum() * max_change;
            }
        }

        // Stabilize: retro-thrusters proportional to stabilize input
        if stab > 0.01 {
            let speed = lin_vel.0.length();
            if speed > 0.1 {
                let dir = lin_vel.0 / speed;
                let decel = (SHIP_STABILIZE_DECEL * stab * dt).min(speed);
                lin_vel.0 -= dir * decel;
            } else {
                lin_vel.0 = Vec2::ZERO;
            }
        }

        // Clamp linear speed
        let speed = lin_vel.0.length();
        let max_speed = if afterburner_active {
            SHIP_MAX_SPEED * 1.5
        } else {
            SHIP_MAX_SPEED
        };
        if speed > max_speed {
            lin_vel.0 = lin_vel.0.normalize() * max_speed;
        }
    }
}

/// Tick down any cooldown component.
fn tick_cooldown<
    T: Component<Mutability = bevy::ecs::component::Mutable> + DerefMut<Target = Cooldown>,
>(
    mut query: Query<&mut T>,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;
    for mut cd in query.iter_mut() {
        if cd.remaining > 0.0 {
            cd.remaining = (cd.remaining - dt).max(0.0);
        }
    }
}

/// Move projectiles and tick down lifetime. Despawn expired projectiles.
fn update_projectile_lifetime(
    mut commands: Commands,
    mut query: Query<(Entity, &mut Projectile, &mut Position, &LinearVelocity)>,
) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;
    for (entity, mut proj, mut pos, vel) in query.iter_mut() {
        // Move projectile (no physics engine — simple linear movement)
        pos.0 += vel.0 * dt;

        proj.lifetime -= dt;
        if proj.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// Tick mine arm timers and lifetime. Despawn expired mines.
fn update_mine_lifetime(mut commands: Commands, mut query: Query<(Entity, &mut Mine)>) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;
    for (entity, mut mine) in query.iter_mut() {
        if mine.arm_timer > 0.0 {
            mine.arm_timer = (mine.arm_timer - dt).max(0.0);
        }
        mine.lifetime -= dt;
        if mine.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// Consume fuel while afterburner is active, regenerate when inactive.
fn update_fuel(mut query: Query<(&ActionState<ShipInput>, &mut Fuel)>) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;
    for (input, mut fuel) in query.iter_mut() {
        if input.0.afterburner && fuel.current > 0.0 {
            fuel.current = (fuel.current - FUEL_BURN_RATE * dt).max(0.0);
        } else if fuel.current < fuel.max {
            fuel.current = (fuel.current + FUEL_REGEN_RATE * dt).min(fuel.max);
        }
    }
}

/// Passive ammo regeneration.
fn update_ammo(mut query: Query<&mut Ammo>) {
    let dt = 1.0 / FIXED_TIMESTEP_HZ as f32;
    for mut ammo in query.iter_mut() {
        if ammo.current < ammo.max {
            ammo.current = (ammo.current + AMMO_REGEN_RATE * dt).min(ammo.max);
        }
    }
}

/// Despawn projectiles that hit asteroids.
fn check_projectile_asteroid_collisions(
    mut commands: Commands,
    projectiles: Query<(Entity, &Position), With<Projectile>>,
    asteroids: Query<(&Position, &Asteroid)>,
) {
    for (proj_entity, proj_pos) in projectiles.iter() {
        for (ast_pos, asteroid) in asteroids.iter() {
            let hit_dist = PROJECTILE_RADIUS + asteroid.radius;
            if (proj_pos.0 - ast_pos.0).length_squared() < hit_dist * hit_dist {
                commands.entity(proj_entity).despawn();
                break;
            }
        }
    }
}

/// Soft boundary: ships entering the boundary zone get slowed and reflected inward.
fn enforce_map_boundary(mut query: Query<(&Position, &mut LinearVelocity)>) {
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

/// Check mine proximity and detonate armed mines near enemy ships.
/// Mines damage all enemy ships within trigger radius on detonation.
pub fn check_mine_detonations(
    mut commands: Commands,
    mines: Query<(Entity, &Mine, &Position)>,
    mut ships: Query<(Entity, &Position, &Team, &mut Health)>,
) {
    let trigger_dist_sq = MINE_TRIGGER_RADIUS * MINE_TRIGGER_RADIUS;

    for (mine_entity, mine, mine_pos) in mines.iter() {
        // Skip unarmed mines
        if mine.arm_timer > 0.0 {
            continue;
        }

        let mut detonated = false;
        for (_ship_entity, ship_pos, ship_team, mut health) in ships.iter_mut() {
            // No friendly fire — skip same team
            if *ship_team == mine.owner_team {
                continue;
            }

            let delta = mine_pos.0 - ship_pos.0;
            if delta.length_squared() < trigger_dist_sq {
                health.current = (health.current - mine.damage).max(0.0);
                detonated = true;
                break;
            }
        }

        if detonated {
            commands.entity(mine_entity).despawn();
        }
    }
}

/// Sync Position→Transform for entities without RigidBody (projectiles, mines).
/// Avian's PhysicsTransformPlugin is disabled and LightyearAvian only syncs physics entities.
fn sync_position_to_transform(mut query: Query<(&Position, &mut Transform), Without<RigidBody>>) {
    for (pos, mut transform) in query.iter_mut() {
        transform.translation.x = pos.0.x;
        transform.translation.y = pos.0.y;
    }
}

/// Check projectile-ship overlaps and apply damage. Despawn projectile on hit.
/// Uses simple circle-circle test (no physics engine for projectiles).
pub fn check_projectile_hits(
    mut commands: Commands,
    projectiles: Query<(Entity, &Projectile, &Position)>,
    mut ships: Query<(Entity, &Position, &Team, &mut Health)>,
) {
    let hit_dist = PROJECTILE_RADIUS + SHIP_RADIUS;
    let hit_dist_sq = hit_dist * hit_dist;

    for (proj_entity, proj, proj_pos) in projectiles.iter() {
        for (_ship_entity, ship_pos, ship_team, mut health) in ships.iter_mut() {
            // No friendly fire — skip same team
            if *ship_team == proj.owner_team {
                continue;
            }

            let delta = proj_pos.0 - ship_pos.0;
            if delta.length_squared() < hit_dist_sq {
                health.current = (health.current - proj.damage).max(0.0);
                commands.entity(proj_entity).despawn();
                break;
            }
        }
    }
}
