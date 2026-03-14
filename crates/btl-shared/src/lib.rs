#![allow(clippy::type_complexity, clippy::too_many_arguments)]

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
    Ammo, Asteroid, Cloak, DamageFlash, Drone, DroneKind, FireCooldown, Fuel, Health, Mine,
    MineCooldown, NebulaSeed, ObjectiveKind, Projectile, ProjectileKind, RailgunCharge,
    RailgunTurretState, ShipClass, SpawnProtection, Torpedo, TurretState, Turrets,
    ZoneDrone, ZoneRailgun, ZoneShield,
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
pub const AUTOCANNON_LIFETIME: f32 = 3.2;
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

// --- Gunship constants ---

pub const GUNSHIP_RADIUS: f32 = 22.0;
pub const GUNSHIP_MASS: f32 = 18.0;
pub const GUNSHIP_MAX_HEALTH: f32 = 150.0;
pub const GUNSHIP_THRUST: f32 = 150.0;
pub const GUNSHIP_AFTERBURNER_THRUST: f32 = 350.0;
pub const GUNSHIP_STRAFE_THRUST: f32 = 80.0;
pub const GUNSHIP_MAX_AMMO: f32 = 30.0;
pub const GUNSHIP_AMMO_REGEN: f32 = 1.0;

// --- Heavy cannon (Gunship primary, player-aimed) ---

pub const HEAVY_CANNON_COOLDOWN: f32 = 0.667; // ~1.5 rounds/s
pub const HEAVY_CANNON_SPEED: f32 = 600.0;
pub const HEAVY_CANNON_DAMAGE: f32 = 35.0;
pub const HEAVY_CANNON_LIFETIME: f32 = 4.8;
pub const HEAVY_CANNON_AMMO_COST: f32 = 3.0;
pub const HEAVY_PROJECTILE_RADIUS: f32 = 5.0;
pub const HEAVY_MUZZLE_OFFSET: f32 = GUNSHIP_RADIUS + HEAVY_PROJECTILE_RADIUS + 2.0;

// --- Auto-turret (Gunship secondary, AI-controlled) ---

pub const TURRET_COUNT: usize = 3;
pub const TURRET_COOLDOWN: f32 = 0.333; // ~3 rounds/s
pub const TURRET_SPEED: f32 = 700.0;
pub const TURRET_DAMAGE: f32 = 5.0;
pub const TURRET_LIFETIME: f32 = 1.6;
pub const TURRET_RANGE: f32 = 1600.0;
/// Max turret rotation speed (radians/sec)
pub const TURRET_SLEW_RATE: f32 = 4.0;
/// Angle tolerance for firing (radians)
pub const TURRET_FIRE_TOLERANCE: f32 = 0.15;
pub const TURRET_PROJECTILE_RADIUS: f32 = 2.0;

/// Turret mount offsets in local ship space (Y+ = forward).
pub const TURRET_MOUNTS: [Vec2; 3] = [
    Vec2::new(0.0, GUNSHIP_RADIUS * 0.6),
    Vec2::new(-GUNSHIP_RADIUS * 0.7, -GUNSHIP_RADIUS * 0.4),
    Vec2::new(GUNSHIP_RADIUS * 0.7, -GUNSHIP_RADIUS * 0.4),
];

// --- Torpedo Boat constants ---

pub const TBOAT_RADIUS: f32 = 18.0;
pub const TBOAT_MASS: f32 = 12.0;
pub const TBOAT_MAX_HEALTH: f32 = 110.0;
pub const TBOAT_THRUST: f32 = 180.0;
pub const TBOAT_AFTERBURNER_THRUST: f32 = 450.0;
pub const TBOAT_STRAFE_THRUST: f32 = 100.0;
pub const TBOAT_MAX_AMMO: f32 = 80.0;
pub const TBOAT_AMMO_REGEN: f32 = 3.0;

// --- Laser (Torpedo Boat primary, continuous beam) ---

pub const LASER_RANGE: f32 = 1280.0;
pub const LASER_DPS: f32 = 20.0;
/// Ammo consumed per second while laser is firing
pub const LASER_AMMO_COST: f32 = 5.0;

// --- Torpedo (Torpedo Boat secondary, homing) ---

pub const TORPEDO_SPEED: f32 = 110.0;
pub const TORPEDO_DAMAGE: f32 = 70.0;
pub const TORPEDO_LIFETIME: f32 = 32.0;
/// Max homing turn rate in radians/sec
pub const TORPEDO_TURN_RATE: f32 = 0.8;
pub const TORPEDO_COOLDOWN: f32 = 3.0;
pub const TORPEDO_MAX_ACTIVE: usize = 3;
pub const TORPEDO_RADIUS: f32 = 4.0;
pub const TORPEDO_MUZZLE_OFFSET: f32 = TBOAT_RADIUS + TORPEDO_RADIUS + 2.0;

// --- Sniper constants ---

pub const SNIPER_RADIUS: f32 = 15.0;
pub const SNIPER_MASS: f32 = 8.0;
pub const SNIPER_MAX_HEALTH: f32 = 70.0;
pub const SNIPER_THRUST: f32 = 220.0;
pub const SNIPER_AFTERBURNER_THRUST: f32 = 550.0;
pub const SNIPER_STRAFE_THRUST: f32 = 140.0;
pub const SNIPER_MAX_AMMO: f32 = 50.0;
pub const SNIPER_AMMO_REGEN: f32 = 1.5;

// --- Railgun (Sniper primary, charge-up fast projectile) ---

/// Time to fully charge the railgun (seconds)
pub const RAILGUN_CHARGE_TIME: f32 = 2.0;
/// Damage at full charge
pub const RAILGUN_DAMAGE: f32 = 120.0;
/// Cooldown after firing
pub const RAILGUN_COOLDOWN: f32 = 5.0;
/// Railgun projectile speed (very fast)
pub const RAILGUN_SPEED: f32 = 3500.0;
/// Railgun projectile lifetime (seconds)
pub const RAILGUN_LIFETIME: f32 = 2.4;
/// Railgun projectile collision radius
pub const RAILGUN_PROJECTILE_RADIUS: f32 = 3.0;

// --- Cloak (Sniper ability, replaces afterburner input) ---

/// Max cloak duration (seconds)
pub const CLOAK_DURATION: f32 = 8.0;
/// Cooldown after cloak ends
pub const CLOAK_COOLDOWN: f32 = 15.0;

// --- Drone Commander constants ---

pub const DCOMMANDER_RADIUS: f32 = 20.0;
pub const DCOMMANDER_MASS: f32 = 15.0;
pub const DCOMMANDER_MAX_HEALTH: f32 = 120.0;
pub const DCOMMANDER_THRUST: f32 = 110.0;
pub const DCOMMANDER_AFTERBURNER_THRUST: f32 = 260.0;
pub const DCOMMANDER_STRAFE_THRUST: f32 = 60.0;
pub const DCOMMANDER_MAX_AMMO: f32 = 40.0;
pub const DCOMMANDER_AMMO_REGEN: f32 = 2.0;

// --- Defense turrets (Drone Commander auto-targeting) ---

pub const DEFENSE_TURRET_COUNT: usize = 5;
pub const DEFENSE_TURRET_COOLDOWN: f32 = 0.5;
pub const DEFENSE_TURRET_SPEED: f32 = 600.0;
pub const DEFENSE_TURRET_DAMAGE: f32 = 3.0;
pub const DEFENSE_TURRET_LIFETIME: f32 = 1.28;
pub const DEFENSE_TURRET_RANGE: f32 = 1200.0;
pub const DEFENSE_TURRET_SLEW_RATE: f32 = 5.0;
pub const DEFENSE_TURRET_FIRE_TOLERANCE: f32 = 0.2;
pub const DEFENSE_TURRET_PROJECTILE_RADIUS: f32 = 2.0;

/// Defense turret mount offsets in local ship space (Y+ = forward).
pub const DEFENSE_TURRET_MOUNTS: [Vec2; 5] = [
    Vec2::new(0.0, DCOMMANDER_RADIUS * 0.7),
    Vec2::new(DCOMMANDER_RADIUS * 0.6, DCOMMANDER_RADIUS * 0.2),
    Vec2::new(DCOMMANDER_RADIUS * 0.5, -DCOMMANDER_RADIUS * 0.5),
    Vec2::new(-DCOMMANDER_RADIUS * 0.5, -DCOMMANDER_RADIUS * 0.5),
    Vec2::new(-DCOMMANDER_RADIUS * 0.6, DCOMMANDER_RADIUS * 0.2),
];

// --- Attack drones ---

pub const DRONE_LASER_COUNT: usize = 4;
pub const DRONE_KAMIKAZE_COUNT: usize = 3;
pub const DRONE_MAX_COUNT: usize = DRONE_LASER_COUNT + DRONE_KAMIKAZE_COUNT;
pub const DRONE_RADIUS: f32 = 6.0;
pub const DRONE_SPEED: f32 = 500.0;
pub const DRONE_AGGRO_RANGE: f32 = 1920.0;
pub const DRONE_ORBIT_RADIUS: f32 = 80.0;
pub const DRONE_RESPAWN_TIME: f32 = 8.0;
// Laser drone stats
pub const DRONE_LASER_HEALTH: f32 = 12.0;
pub const DRONE_LASER_RANGE: f32 = 800.0;
pub const DRONE_LASER_DPS: f32 = 15.0; // higher to compensate for pulsed firing
pub const DRONE_LASER_BURST: f32 = 0.25; // seconds firing per burst
pub const DRONE_LASER_PAUSE_MIN: f32 = 0.4;
pub const DRONE_LASER_PAUSE_MAX: f32 = 0.9;

/// Erratic pulse pattern for drone lasers. Each drone gets a unique rhythm
/// based on entity bits, producing irregular short bursts.
pub fn drone_laser_firing(entity_bits: u64, elapsed_secs: f32) -> bool {
    let seed = entity_bits.wrapping_mul(2654435761);
    let phase1 = (seed % 1000) as f32 * 0.001;
    let phase2 = ((seed >> 16) % 1000) as f32 * 0.001;
    let cycle = DRONE_LASER_BURST + (DRONE_LASER_PAUSE_MIN + DRONE_LASER_PAUSE_MAX) * 0.5;
    let wave1 = ((elapsed_secs / cycle + phase1) * std::f32::consts::TAU).sin();
    let wave2 = ((elapsed_secs / cycle * 1.7 + phase2) * std::f32::consts::TAU).sin();
    wave1 > 0.2 && wave2 > -0.3
}

// Kamikaze drone stats
pub const DRONE_KAMIKAZE_HEALTH: f32 = 8.0;
pub const DRONE_KAMIKAZE_DAMAGE: f32 = 40.0;
pub const DRONE_KAMIKAZE_SPEED: f32 = 600.0;

// --- Anti-drone pulse / drone detonation ---

pub const PULSE_RADIUS: f32 = 400.0;
pub const PULSE_COOLDOWN: f32 = 20.0;
/// Blast radius of each drone when detonated by pulse.
pub const DRONE_DETONATION_RADIUS: f32 = 80.0;
/// Damage dealt by each detonating drone to nearby enemy ships.
pub const DRONE_DETONATION_DAMAGE: f32 = 25.0;

/// Fixed timestep delta time in seconds (1 / FIXED_TIMESTEP_HZ).
pub const FIXED_DT: f32 = 1.0 / FIXED_TIMESTEP_HZ as f32;

// --- Map constants ---

pub const MAP_RADIUS: f32 = 10000.0;
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
/// Points per second while controlling a zone
pub const ZONE_SCORE_RATE: f32 = 1.0;
/// Score needed to win
pub const SCORE_LIMIT: f32 = 100.0;
/// Seconds to show victory screen before restart countdown.
pub const ROUND_END_DISPLAY_TIME: f32 = 5.0;
/// Seconds for restart countdown after victory display.
pub const ROUND_RESTART_COUNTDOWN: f32 = 3.0;
/// Base capture rate per second (progress per second with 1 ship)
/// Diminishing returns: 1x, 1.5x, 1.8x, 2.0x for 1-4 ships
pub const CAPTURE_RATE: f32 = 0.1;
/// Maximum number of kill events stored in the kill feed.
pub const KILL_FEED_MAX: usize = 5;
/// Rate at which an uncontested zone drifts back toward neutral (progress per second).
/// ~50s to fully decap from max; contested (equal ships) zones stay frozen.
pub const DECAP_RATE: f32 = 0.02;

/// Spawn invulnerability duration (seconds)
pub const SPAWN_PROTECTION_DURATION: f32 = 3.0;
/// Duration of white damage flash on hit (seconds)
pub const DAMAGE_FLASH_DURATION: f32 = 0.12;

/// Zone benefit: HP regen per second when inside a friendly zone
pub const ZONE_HP_REGEN: f32 = 5.0;
/// Zone benefit: ammo/fuel regen multiplier (added on top of passive)
pub const ZONE_REGEN_MULT: f32 = 2.0;

// --- Objective defense constants ---

/// Which defense each zone index gets.
pub const OBJECTIVE_KINDS: [ObjectiveKind; 3] = [
    ObjectiveKind::Factory,
    ObjectiveKind::Railgun,
    ObjectiveKind::Powerplant,
];

// Factory defense drones
pub const FACTORY_LASER_DRONES: usize = 7;
pub const FACTORY_KAMIKAZE_DRONES: usize = 4;
pub const FACTORY_DRONE_HEALTH: f32 = 15.0;
pub const FACTORY_DRONE_SPEED: f32 = 180.0;
pub const FACTORY_DRONE_ORBIT_RADIUS: f32 = OBJECTIVE_ZONE_RADIUS;
pub const FACTORY_DRONE_AGGRO_RANGE: f32 = 500.0;
pub const FACTORY_DRONE_LASER_RANGE: f32 = 250.0;
pub const FACTORY_DRONE_LASER_DPS: f32 = 8.0;
pub const FACTORY_DRONE_KAMIKAZE_DAMAGE: f32 = 30.0;
pub const FACTORY_DRONE_RESPAWN_TIME: f32 = 10.0;

// Railgun turret
pub const ZONE_RAILGUN_RANGE: f32 = 800.0;
pub const ZONE_RAILGUN_SLEW_RATE: f32 = 1.5;
pub const ZONE_RAILGUN_CHARGE_TIME: f32 = 2.0;
pub const ZONE_RAILGUN_LOCK_TIME: f32 = 0.5;
pub const ZONE_RAILGUN_DAMAGE: f32 = 60.0;
pub const ZONE_RAILGUN_COOLDOWN: f32 = 4.0;
pub const ZONE_RAILGUN_PROJECTILE_SPEED: f32 = 3000.0;
pub const ZONE_RAILGUN_PROJECTILE_LIFETIME: f32 = 1.5;

// Powerplant shield
pub const ZONE_SHIELD_RADIUS: f32 = 350.0;
pub const ZONE_SHIELD_LASER_MULT: f32 = 0.3;

/// Collision damage constants
pub const COLLISION_DAMAGE_VELOCITY_THRESHOLD: f32 = 100.0;
/// Damage per unit of relative velocity above threshold
pub const COLLISION_DAMAGE_PER_VELOCITY: f32 = 0.15;
/// Extra damage multiplier on the faster ship in a ship-ship collision
pub const COLLISION_FASTER_SHIP_MULT: f32 = 1.2;

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

pub const ASTEROID_COUNT: usize = 32;
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

/// Largest asteroid collision radius (matches the last entry in ASTEROID_SIZES).
pub const MAX_ASTEROID_RADIUS: f32 = 200.0;
/// Largest ship collision radius across all classes.
pub const MAX_SHIP_RADIUS: f32 = GUNSHIP_RADIUS;

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

// ============================================================
// Spatial hash grid
// ============================================================

/// World-space cell size. Large enough that most entities fit in one cell; small
/// enough that typical projectile searches only touch a handful of cells.
const SPATIAL_CELL: f32 = 256.0;

/// A lightweight spatial hash that maps grid cells to lists of copied items.
/// Cleared and repopulated each tick by `rebuild_collision_grids`.
pub struct SpatialHash<T> {
    cells: std::collections::HashMap<(i32, i32), Vec<T>>,
}

impl<T> Default for SpatialHash<T> {
    fn default() -> Self {
        SpatialHash { cells: std::collections::HashMap::new() }
    }
}

impl<T: Copy> SpatialHash<T> {
    fn cell_key(pos: Vec2) -> (i32, i32) {
        (
            (pos.x / SPATIAL_CELL).floor() as i32,
            (pos.y / SPATIAL_CELL).floor() as i32,
        )
    }

    /// Retain allocated memory but remove all entries.
    pub fn clear(&mut self) {
        for v in self.cells.values_mut() {
            v.clear();
        }
    }

    /// Insert an item at a world-space position.
    pub fn insert(&mut self, pos: Vec2, item: T) {
        self.cells.entry(Self::cell_key(pos)).or_default().push(item);
    }

    /// Call `f` for every item stored in cells that overlap the circle (pos, radius).
    /// Does **not** do final distance filtering — callers handle that.
    pub fn for_each_candidate(&self, pos: Vec2, radius: f32, mut f: impl FnMut(T)) {
        let r = (radius / SPATIAL_CELL).ceil() as i32 + 1;
        let (cx, cy) = Self::cell_key(pos);
        for dx in -r..=r {
            for dy in -r..=r {
                if let Some(items) = self.cells.get(&(cx + dx, cy + dy)) {
                    for &item in items {
                        f(item);
                    }
                }
            }
        }
    }
}

/// Collision look-up grids rebuilt once per fixed tick before collision systems run.
/// Only stores (Entity, Vec2) or (Vec2, f32) — enough to filter candidates before
/// doing per-component lookups via `Query::get_mut`.
#[derive(Resource, Default)]
pub struct CollisionGrids {
    /// Ships that have `Health`: (entity, world pos).
    pub ships: SpatialHash<(Entity, Vec2)>,
    /// Asteroids: (world pos, radius).
    pub asteroids: SpatialHash<(Vec2, f32)>,
    /// Player drones (`Drone` component): (entity, world pos).
    pub drones: SpatialHash<(Entity, Vec2)>,
    /// Zone defense drones (`ZoneDrone` component): (entity, world pos).
    pub zone_drones: SpatialHash<(Entity, Vec2)>,
}

/// Repopulate all collision grids. Must run before every collision system each tick.
pub fn rebuild_collision_grids(
    mut grids: ResMut<CollisionGrids>,
    ships: Query<(Entity, &Position), With<Health>>,
    asteroids: Query<(&Position, &Asteroid)>,
    drones: Query<(Entity, &Position), With<Drone>>,
    zone_drones: Query<(Entity, &Position), With<ZoneDrone>>,
) {
    grids.ships.clear();
    for (entity, pos) in ships.iter() {
        grids.ships.insert(pos.0, (entity, pos.0));
    }
    grids.asteroids.clear();
    for (pos, ast) in asteroids.iter() {
        grids.asteroids.insert(pos.0, (pos.0, ast.radius));
    }
    grids.drones.clear();
    for (entity, pos) in drones.iter() {
        grids.drones.insert(pos.0, (entity, pos.0));
    }
    grids.zone_drones.clear();
    for (entity, pos) in zone_drones.iter() {
        grids.zone_drones.insert(pos.0, (entity, pos.0));
    }
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
        // Ship class and turrets
        app.register_component::<ShipClass>().add_prediction();
        app.register_component::<Turrets>()
            .add_prediction()
            .add_should_rollback(|_: &Turrets, _: &Turrets| false);
        app.register_component::<ProjectileKind>();
        app.register_component::<Torpedo>();
        app.register_component::<Cloak>()
            .add_prediction()
            .add_should_rollback(|_: &Cloak, _: &Cloak| false);
        app.register_component::<RailgunCharge>()
            .add_prediction()
            .add_should_rollback(|_: &RailgunCharge, _: &RailgunCharge| false);
        app.register_component::<Drone>();
        app.register_component::<SpawnProtection>().add_prediction();
        app.register_component::<DamageFlash>().add_prediction();

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

        app.init_resource::<CollisionGrids>();
        app.add_systems(FixedUpdate, rebuild_collision_grids);

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
                check_projectile_asteroid_collisions.after(rebuild_collision_grids),
                update_mine_lifetime,
                update_torpedo_lifetime,
                update_drone_positions,
                enforce_map_boundary,
                tick_spawn_protection,
                tick_damage_flash,
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
    pub ship_class: ShipClass,
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
    pub turrets: Turrets,
    pub cloak: Cloak,
    pub railgun_charge: RailgunCharge,
    pub spawn_protection: SpawnProtection,
    pub damage_flash: DamageFlash,
    pub last_damaged_by: LastDamagedBy,
}

impl ShipBundle {
    pub fn new(
        player_id: lightyear::prelude::PeerId,
        team: Team,
        class: ShipClass,
        spawn_pos: Vec2,
    ) -> Self {
        let (radius, mass, max_health, max_ammo, turrets) = match class {
            ShipClass::Interceptor => (
                SHIP_RADIUS,
                SHIP_MASS,
                SHIP_MAX_HEALTH,
                SHIP_MAX_AMMO,
                Turrets::default(),
            ),
            ShipClass::Gunship => (
                GUNSHIP_RADIUS,
                GUNSHIP_MASS,
                GUNSHIP_MAX_HEALTH,
                GUNSHIP_MAX_AMMO,
                Turrets {
                    mounts: (0..TURRET_COUNT)
                        .map(|i| TurretState {
                            aim_angle: 0.0,
                            cooldown: TURRET_COOLDOWN * i as f32 / TURRET_COUNT as f32,
                        })
                        .collect(),
                },
            ),
            ShipClass::TorpedoBoat => (
                TBOAT_RADIUS,
                TBOAT_MASS,
                TBOAT_MAX_HEALTH,
                TBOAT_MAX_AMMO,
                Turrets::default(),
            ),
            ShipClass::Sniper => (
                SNIPER_RADIUS,
                SNIPER_MASS,
                SNIPER_MAX_HEALTH,
                SNIPER_MAX_AMMO,
                Turrets::default(),
            ),
            ShipClass::DroneCommander => (
                DCOMMANDER_RADIUS,
                DCOMMANDER_MASS,
                DCOMMANDER_MAX_HEALTH,
                DCOMMANDER_MAX_AMMO,
                Turrets {
                    mounts: (0..DEFENSE_TURRET_COUNT)
                        .map(|i| TurretState {
                            aim_angle: 0.0,
                            cooldown: DEFENSE_TURRET_COOLDOWN * i as f32
                                / DEFENSE_TURRET_COUNT as f32,
                        })
                        .collect(),
                },
            ),
        };
        let angular_inertia = 0.5 * mass * radius * radius;
        Self {
            player_id: PlayerId(player_id),
            team,
            ship_class: class,
            rigid_body: RigidBody::Dynamic,
            collider: Collider::circle(radius),
            restitution: Restitution::new(0.8),
            mass: Mass(mass),
            angular_inertia: AngularInertia(angular_inertia),
            position: Position(spawn_pos),
            rotation: Rotation::default(),
            linear_velocity: LinearVelocity::default(),
            angular_velocity: AngularVelocity::default(),
            linear_damping: LinearDamping(0.0),
            angular_damping: AngularDamping(0.0),
            health: Health::new(max_health),
            fuel: Fuel::new(SHIP_MAX_FUEL),
            ammo: Ammo::new(max_ammo),
            fire_cooldown: FireCooldown::default(),
            mine_cooldown: MineCooldown::default(),
            turrets,
            cloak: Cloak {
                active: false,
                duration: CLOAK_DURATION,
                cooldown: 0.0,
            },
            railgun_charge: RailgunCharge::default(),
            spawn_protection: SpawnProtection { remaining: SPAWN_PROTECTION_DURATION },
            damage_flash: DamageFlash::default(),
            last_damaged_by: LastDamagedBy::default(),
        }
    }
}

// --- Shared movement (runs on client for prediction, server for authority) ---

fn apply_ship_input(
    mut query: Query<(
        &ActionState<ShipInput>,
        &Rotation,
        &Fuel,
        &ShipClass,
        &mut LinearVelocity,
        &mut AngularVelocity,
    )>,
) {
    for (input, rotation, fuel, class, mut lin_vel, mut ang_vel) in query.iter_mut() {
        let input = &input.0;
        let dt = FIXED_DT;
        let forward = *rotation * Vec2::Y;
        let right = *rotation * Vec2::X;

        // Class-specific stats
        let (base_thrust, ab_thrust, strafe_thrust) = match class {
            ShipClass::Interceptor => (SHIP_THRUST, SHIP_AFTERBURNER_THRUST, SHIP_STRAFE_THRUST),
            ShipClass::Gunship => (
                GUNSHIP_THRUST,
                GUNSHIP_AFTERBURNER_THRUST,
                GUNSHIP_STRAFE_THRUST,
            ),
            ShipClass::TorpedoBoat => (TBOAT_THRUST, TBOAT_AFTERBURNER_THRUST, TBOAT_STRAFE_THRUST),
            ShipClass::Sniper => (
                SNIPER_THRUST,
                SNIPER_AFTERBURNER_THRUST,
                SNIPER_STRAFE_THRUST,
            ),
            ShipClass::DroneCommander => (
                DCOMMANDER_THRUST,
                DCOMMANDER_AFTERBURNER_THRUST,
                DCOMMANDER_STRAFE_THRUST,
            ),
        };

        // Clamp continuous inputs to valid ranges
        let fwd = input.thrust_forward.clamp(0.0, 1.0);
        let bwd = input.thrust_backward.clamp(0.0, 1.0);
        let rot = input.rotate.clamp(-1.0, 1.0);
        let strf = input.strafe.clamp(-1.0, 1.0);
        let stab = input.stabilize.clamp(0.0, 1.0);

        // Afterburner only works with fuel (Sniper repurposes this input for cloak)
        let afterburner_active =
            input.afterburner && *class != ShipClass::Sniper && fuel.current > 0.0;

        // Thrust (continuous throttle)
        let thrust = if afterburner_active {
            ab_thrust
        } else {
            base_thrust
        };

        lin_vel.0 += forward * thrust * fwd * dt;
        lin_vel.0 -= forward * thrust * 0.5 * bwd * dt;

        // Strafe (continuous, positive = left, negative = right)
        lin_vel.0 -= right * strafe_thrust * strf * dt;

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
    let dt = FIXED_DT;
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
    let dt = FIXED_DT;
    for (entity, mut proj, mut pos, vel) in query.iter_mut() {
        // Move projectile (no physics engine — simple linear movement)
        pos.0 += vel.0 * dt;

        proj.lifetime -= dt;
        if proj.lifetime <= 0.0 {
            commands.entity(entity).try_despawn();
        }
    }
}

/// Tick mine arm timers and lifetime. Detonate expired mines (damage nearby enemies).
fn update_mine_lifetime(
    mut commands: Commands,
    mut mines: Query<(Entity, &mut Mine, &Position)>,
    mut ships: Query<(&Position, &Team, &SpawnProtection, &mut Health, &mut DamageFlash, &mut LastDamagedBy)>,
) {
    let dt = FIXED_DT;
    let trigger_dist_sq = MINE_TRIGGER_RADIUS * MINE_TRIGGER_RADIUS;

    for (entity, mut mine, mine_pos) in mines.iter_mut() {
        if mine.arm_timer > 0.0 {
            mine.arm_timer = (mine.arm_timer - dt).max(0.0);
        }
        mine.lifetime -= dt;
        if mine.lifetime <= 0.0 {
            for (ship_pos, ship_team, sp, mut health, mut flash, mut last_hit) in ships.iter_mut() {
                if *ship_team == mine.owner_team || sp.remaining > 0.0 {
                    continue;
                }
                if (mine_pos.0 - ship_pos.0).length_squared() < trigger_dist_sq {
                    health.current = (health.current - mine.damage).max(0.0);
                    last_hit.attacker = Some(mine.owner);
                    flash.timer = DAMAGE_FLASH_DURATION;
                }
            }
            commands.entity(entity).try_despawn();
        }
    }
}

/// Consume fuel while afterburner is active, regenerate when inactive.
/// Sniper uses afterburner input for cloak toggle — no fuel burn.
fn update_fuel(mut query: Query<(&ActionState<ShipInput>, &ShipClass, &mut Fuel)>) {
    let dt = FIXED_DT;
    for (input, class, mut fuel) in query.iter_mut() {
        // Sniper repurposes afterburner for cloak — skip fuel burn
        let burning = input.0.afterburner && *class != ShipClass::Sniper && fuel.current > 0.0;
        if burning {
            fuel.current = (fuel.current - FUEL_BURN_RATE * dt).max(0.0);
        } else if fuel.current < fuel.max {
            fuel.current = (fuel.current + FUEL_REGEN_RATE * dt).min(fuel.max);
        }
    }
}

/// Passive ammo regeneration (rate depends on ship class).
fn update_ammo(mut query: Query<(&ShipClass, &mut Ammo)>) {
    let dt = FIXED_DT;
    for (class, mut ammo) in query.iter_mut() {
        let regen = match class {
            ShipClass::Interceptor => AMMO_REGEN_RATE,
            ShipClass::Gunship => GUNSHIP_AMMO_REGEN,
            ShipClass::TorpedoBoat => TBOAT_AMMO_REGEN,
            ShipClass::Sniper => SNIPER_AMMO_REGEN,
            ShipClass::DroneCommander => DCOMMANDER_AMMO_REGEN,
        };
        if ammo.current < ammo.max {
            ammo.current = (ammo.current + regen * dt).min(ammo.max);
        }
    }
}

/// Despawn projectiles that hit asteroids.
fn check_projectile_asteroid_collisions(
    mut commands: Commands,
    projectiles: Query<(Entity, &Position), With<Projectile>>,
    grids: Res<CollisionGrids>,
    mut candidates: Local<Vec<(Vec2, f32)>>,
) {
    for (proj_entity, proj_pos) in projectiles.iter() {
        candidates.clear();
        grids.asteroids.for_each_candidate(
            proj_pos.0,
            PROJECTILE_RADIUS + MAX_ASTEROID_RADIUS,
            |e| candidates.push(e),
        );
        for &(ast_pos, ast_radius) in candidates.iter() {
            let hit_dist = PROJECTILE_RADIUS + ast_radius;
            if (proj_pos.0 - ast_pos).length_squared() < hit_dist * hit_dist {
                commands.entity(proj_entity).try_despawn();
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
    mut ships: Query<(Entity, &Position, &Team, &SpawnProtection, &mut Health, &mut DamageFlash, &mut LastDamagedBy)>,
) {
    let trigger_dist_sq = MINE_TRIGGER_RADIUS * MINE_TRIGGER_RADIUS;

    for (mine_entity, mine, mine_pos) in mines.iter() {
        if mine.arm_timer > 0.0 {
            continue;
        }

        let mut detonated = false;
        for (_ship_entity, ship_pos, ship_team, sp, mut health, mut flash, mut last_hit) in ships.iter_mut() {
            if *ship_team == mine.owner_team {
                continue;
            }

            let delta = mine_pos.0 - ship_pos.0;
            if delta.length_squared() < trigger_dist_sq {
                if sp.remaining <= 0.0 {
                    health.current = (health.current - mine.damage).max(0.0);
                    last_hit.attacker = Some(mine.owner);
                    flash.timer = DAMAGE_FLASH_DURATION;
                }
                detonated = true;
                break;
            }
        }

        if detonated {
            commands.entity(mine_entity).try_despawn();
        }
    }
}

/// Tick down spawn protection timer.
fn tick_spawn_protection(mut query: Query<&mut SpawnProtection>) {
    let dt = FIXED_DT;
    for mut sp in query.iter_mut() {
        if sp.remaining > 0.0 {
            sp.remaining = (sp.remaining - dt).max(0.0);
        }
    }
}

/// Tick down damage flash timer.
fn tick_damage_flash(mut query: Query<&mut DamageFlash>) {
    let dt = FIXED_DT;
    for mut flash in query.iter_mut() {
        if flash.timer > 0.0 {
            flash.timer = (flash.timer - dt).max(0.0);
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
/// Helper: get ship collision radius from class.
pub fn ship_radius(class: &ShipClass) -> f32 {
    match class {
        ShipClass::Interceptor => SHIP_RADIUS,
        ShipClass::Gunship => GUNSHIP_RADIUS,
        ShipClass::TorpedoBoat => TBOAT_RADIUS,
        ShipClass::Sniper => SNIPER_RADIUS,
        ShipClass::DroneCommander => DCOMMANDER_RADIUS,
    }
}

pub fn check_projectile_hits(
    mut commands: Commands,
    projectiles: Query<(Entity, &Projectile, &Position, Option<&ProjectileKind>)>,
    mut ships: Query<(
        Entity,
        &Position,
        &Team,
        &ShipClass,
        &SpawnProtection,
        &mut Health,
        &mut DamageFlash,
        &mut LastDamagedBy,
    )>,
    grids: Res<CollisionGrids>,
    mut candidates: Local<Vec<(Entity, Vec2)>>,
) {
    for (proj_entity, proj, proj_pos, proj_kind) in projectiles.iter() {
        let proj_radius = match proj_kind.copied().unwrap_or_default() {
            ProjectileKind::Autocannon => PROJECTILE_RADIUS,
            ProjectileKind::HeavyCannon => HEAVY_PROJECTILE_RADIUS,
            ProjectileKind::Turret => TURRET_PROJECTILE_RADIUS,
            ProjectileKind::Railgun => RAILGUN_PROJECTILE_RADIUS,
        };

        candidates.clear();
        grids.ships.for_each_candidate(
            proj_pos.0,
            proj_radius + MAX_SHIP_RADIUS,
            |e| candidates.push(e),
        );

        for &(ship_entity, _) in candidates.iter() {
            let Ok((_, ship_pos, ship_team, ship_class, sp, mut health, mut flash, mut last_hit)) =
                ships.get_mut(ship_entity)
            else {
                continue;
            };
            if *ship_team == proj.owner_team {
                continue;
            }
            let hit_dist = proj_radius + ship_radius(ship_class);
            if (proj_pos.0 - ship_pos.0).length_squared() < hit_dist * hit_dist {
                if sp.remaining <= 0.0 {
                    health.current = (health.current - proj.damage).max(0.0);
                    last_hit.attacker = Some(proj.owner);
                    flash.timer = DAMAGE_FLASH_DURATION;
                }
                commands.entity(proj_entity).try_despawn();
                break;
            }
        }
    }
}

/// Despawn projectiles that hit asteroids.
pub fn check_projectile_asteroid_hits(
    mut commands: Commands,
    projectiles: Query<(Entity, &Position, Option<&ProjectileKind>), With<Projectile>>,
    grids: Res<CollisionGrids>,
    mut candidates: Local<Vec<(Vec2, f32)>>,
) {
    for (proj_entity, proj_pos, proj_kind) in projectiles.iter() {
        let proj_radius = match proj_kind.copied().unwrap_or_default() {
            ProjectileKind::Autocannon => PROJECTILE_RADIUS,
            ProjectileKind::HeavyCannon => HEAVY_PROJECTILE_RADIUS,
            ProjectileKind::Turret => TURRET_PROJECTILE_RADIUS,
            ProjectileKind::Railgun => RAILGUN_PROJECTILE_RADIUS,
        };
        candidates.clear();
        grids.asteroids.for_each_candidate(
            proj_pos.0,
            proj_radius + MAX_ASTEROID_RADIUS,
            |e| candidates.push(e),
        );
        for &(ast_pos, ast_radius) in candidates.iter() {
            let hit_dist = proj_radius + ast_radius;
            if (proj_pos.0 - ast_pos).length_squared() < hit_dist * hit_dist {
                commands.entity(proj_entity).try_despawn();
                break;
            }
        }
    }
}

/// Ray-circle intersection. Returns the distance along `dir` to the closest
/// intersection point with the circle at `center` with given `radius`.
/// Returns `f32::MAX` if no intersection (ray misses or circle is behind origin).
pub fn ray_circle_intersect(origin: Vec2, dir: Vec2, center: Vec2, radius: f32) -> f32 {
    let oc = origin - center;
    let b = oc.dot(dir);
    let c = oc.dot(oc) - radius * radius;
    let discriminant = b * b - c;
    if discriminant < 0.0 {
        return f32::MAX;
    }
    let sqrt_d = discriminant.sqrt();
    let t1 = -b - sqrt_d;
    let t2 = -b + sqrt_d;
    if t1 > 0.0 {
        t1
    } else if t2 > 0.0 {
        t2
    } else {
        f32::MAX
    }
}

/// Compute the world-space lead point to aim at to intercept a moving target,
/// accounting for the shooter's own velocity (projectiles inherit ship velocity on spawn).
///
/// Solves for flight time `t` from the quadratic:
///   `(speed² - |v_rel|²)·t² - 2·(d·v_rel)·t - |d|² = 0`
/// where `d = target_pos - own_pos` and `v_rel = target_vel - own_vel`.
///
/// Returns `None` when no future intercept exists (target escapes the projectile).
pub fn compute_intercept(
    own_pos: Vec2,
    own_vel: Vec2,
    target_pos: Vec2,
    target_vel: Vec2,
    projectile_speed: f32,
) -> Option<Vec2> {
    let d = target_pos - own_pos;
    let v_rel = target_vel - own_vel;

    let a = projectile_speed * projectile_speed - v_rel.length_squared();
    let b = -2.0 * d.dot(v_rel);
    let c = -d.length_squared();

    let t = if a.abs() < 1.0 {
        // Degenerate: projectile speed ≈ relative closing speed → linear equation.
        if b.abs() < 1e-6 {
            return None;
        }
        -c / b
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 {
            return None;
        }
        let sq = disc.sqrt();
        let t1 = (-b - sq) / (2.0 * a);
        let t2 = (-b + sq) / (2.0 * a);
        match (t1 > 0.0, t2 > 0.0) {
            (true, true) => t1.min(t2),
            (false, true) => t2,
            (true, false) => t1,
            _ => return None,
        }
    };

    if t <= 0.0 {
        return None;
    }
    Some(target_pos + target_vel * t)
}

/// Projectile speed for the player-aimed primary weapon of each class.
/// Returns `None` for TorpedoBoat (continuous laser) and DroneCommander (AI turrets),
/// which don't have a ballistic player-aimed primary.
pub fn primary_projectile_speed(class: ShipClass) -> Option<f32> {
    match class {
        ShipClass::Interceptor => Some(AUTOCANNON_SPEED),
        ShipClass::Gunship => Some(HEAVY_CANNON_SPEED),
        ShipClass::Sniper => Some(RAILGUN_SPEED),
        ShipClass::TorpedoBoat | ShipClass::DroneCommander => None,
    }
}

/// Tick torpedo lifetime, move torpedoes, and despawn expired ones.
pub fn update_torpedo_lifetime(
    mut commands: Commands,
    mut query: Query<(Entity, &mut Torpedo, &mut Position, &LinearVelocity)>,
) {
    let dt = FIXED_DT;
    for (entity, mut torp, mut pos, vel) in query.iter_mut() {
        pos.0 += vel.0 * dt;
        torp.lifetime -= dt;
        if torp.lifetime <= 0.0 {
            commands.entity(entity).try_despawn();
        }
    }
}

/// Projectiles can shoot down torpedoes.
pub fn check_torpedo_shootdown(
    mut commands: Commands,
    projectiles: Query<(Entity, &Projectile, &Position)>,
    torpedoes: Query<(Entity, &Torpedo, &Position)>,
) {
    for (proj_entity, proj, proj_pos) in projectiles.iter() {
        for (torp_entity, torp, torp_pos) in torpedoes.iter() {
            if proj.owner_team == torp.owner_team {
                continue;
            }
            let hit_dist = PROJECTILE_RADIUS + TORPEDO_RADIUS;
            if (proj_pos.0 - torp_pos.0).length_squared() < hit_dist * hit_dist {
                commands.entity(proj_entity).try_despawn();
                commands.entity(torp_entity).try_despawn();
                break;
            }
        }
    }
}

/// Move drones by their velocity (no physics engine).
fn update_drone_positions(mut query: Query<(&mut Position, &LinearVelocity), With<Drone>>) {
    let dt = FIXED_DT;
    for (mut pos, vel) in query.iter_mut() {
        pos.0 += vel.0 * dt;
    }
}

/// Projectiles can hit drones. No team filtering — any projectile hits any drone (friendly fire).
pub fn check_projectile_drone_hits(
    mut commands: Commands,
    projectiles: Query<(Entity, &Projectile, &Position)>,
    mut drones: Query<(Entity, &mut Drone, &Position)>,
    grids: Res<CollisionGrids>,
    mut candidates: Local<Vec<(Entity, Vec2)>>,
) {
    const HIT_DIST_SQ: f32 = (PROJECTILE_RADIUS + DRONE_RADIUS) * (PROJECTILE_RADIUS + DRONE_RADIUS);
    for (proj_entity, proj, proj_pos) in projectiles.iter() {
        candidates.clear();
        grids.drones.for_each_candidate(
            proj_pos.0,
            PROJECTILE_RADIUS + DRONE_RADIUS,
            |e| candidates.push(e),
        );
        for &(drone_entity, cached_pos) in candidates.iter() {
            if (proj_pos.0 - cached_pos).length_squared() >= HIT_DIST_SQ {
                continue;
            }
            let Ok((_, mut drone, _)) = drones.get_mut(drone_entity) else {
                continue;
            };
            drone.health -= proj.damage;
            commands.entity(proj_entity).try_despawn();
            if drone.health <= 0.0 {
                commands.entity(drone_entity).try_despawn();
            }
            break;
        }
    }
}

/// Projectile hits on zone defense drones.
pub fn check_projectile_zone_drone_hits(
    mut commands: Commands,
    projectiles: Query<(Entity, &Projectile, &Position)>,
    mut drones: Query<(Entity, &mut ZoneDrone, &Position)>,
    grids: Res<CollisionGrids>,
    mut candidates: Local<Vec<(Entity, Vec2)>>,
) {
    const HIT_DIST_SQ: f32 = (PROJECTILE_RADIUS + DRONE_RADIUS) * (PROJECTILE_RADIUS + DRONE_RADIUS);
    for (proj_entity, proj, proj_pos) in projectiles.iter() {
        candidates.clear();
        grids.zone_drones.for_each_candidate(
            proj_pos.0,
            PROJECTILE_RADIUS + DRONE_RADIUS,
            |e| candidates.push(e),
        );
        for &(drone_entity, cached_pos) in candidates.iter() {
            if (proj_pos.0 - cached_pos).length_squared() >= HIT_DIST_SQ {
                continue;
            }
            let Ok((_, mut drone, _)) = drones.get_mut(drone_entity) else {
                continue;
            };
            // Don't friendly-fire zone drones
            if proj.owner_team == drone.team {
                continue;
            }
            drone.health -= proj.damage;
            commands.entity(proj_entity).try_despawn();
            if drone.health <= 0.0 {
                commands.entity(drone_entity).try_despawn();
            }
            break;
        }
    }
}

/// Laser drones fire erratic pulsed beams at nearest enemy within range.
pub fn drone_laser_damage(
    drones: Query<(Entity, &Drone, &Position)>,
    mut ships: Query<(Entity, &Position, &Team, &SpawnProtection, &mut Health, &mut DamageFlash, &mut LastDamagedBy)>,
    time: Res<Time>,
    grids: Res<CollisionGrids>,
    mut candidates: Local<Vec<(Entity, Vec2)>>,
) {
    let dt = FIXED_DT;
    let range_sq = DRONE_LASER_RANGE * DRONE_LASER_RANGE;
    let elapsed = time.elapsed_secs();

    let mut hits: Vec<(Entity, f32, PeerId)> = Vec::with_capacity(8);

    for (drone_entity, drone, drone_pos) in drones.iter() {
        if drone.kind != DroneKind::Laser {
            continue;
        }
        if !drone_laser_firing(drone_entity.to_bits(), elapsed) {
            continue;
        }
        let mut best_dist_sq = range_sq;
        let mut best_target = None;
        candidates.clear();
        grids.ships.for_each_candidate(drone_pos.0, DRONE_LASER_RANGE, |e| candidates.push(e));
        for &(ship_entity, _) in candidates.iter() {
            let Ok((entity, ship_pos, ship_team, sp, _, _, _)) = ships.get(ship_entity) else {
                continue;
            };
            if *ship_team == drone.owner_team || sp.remaining > 0.0 {
                continue;
            }
            let dist_sq = (drone_pos.0 - ship_pos.0).length_squared();
            if dist_sq < best_dist_sq {
                best_dist_sq = dist_sq;
                best_target = Some(entity);
            }
        }
        if let Some(target) = best_target {
            let dist = best_dist_sq.sqrt();
            let falloff = 1.0 - 0.7 * (dist / DRONE_LASER_RANGE);
            hits.push((target, DRONE_LASER_DPS * falloff * dt, drone.owner));
        }
    }

    for (entity, damage, owner) in hits {
        if let Ok((_, _, _, _, mut health, mut flash, mut last_hit)) = ships.get_mut(entity) {
            health.current = (health.current - damage).max(0.0);
            last_hit.attacker = Some(owner);
            flash.timer = DAMAGE_FLASH_DURATION;
        }
    }
}

/// Kamikaze drones explode on contact with enemy ships — burst damage + self-destruct.
pub fn drone_kamikaze_impact(
    mut commands: Commands,
    drones: Query<(Entity, &Drone, &Position)>,
    mut ships: Query<(
        &Position,
        &Team,
        &ShipClass,
        &SpawnProtection,
        &mut Health,
        &mut DamageFlash,
        &mut LastDamagedBy,
    )>,
    grids: Res<CollisionGrids>,
    mut candidates: Local<Vec<(Entity, Vec2)>>,
) {
    for (drone_entity, drone, drone_pos) in drones.iter() {
        if drone.kind != DroneKind::Kamikaze {
            continue;
        }
        candidates.clear();
        grids.ships.for_each_candidate(
            drone_pos.0,
            DRONE_RADIUS + MAX_SHIP_RADIUS,
            |e| candidates.push(e),
        );
        for &(ship_entity, _) in candidates.iter() {
            let Ok((ship_pos, ship_team, ship_class, sp, mut health, mut flash, mut last_hit)) =
                ships.get_mut(ship_entity)
            else {
                continue;
            };
            if *ship_team == drone.owner_team {
                continue;
            }
            let hit_dist = DRONE_RADIUS + ship_radius(ship_class);
            if (drone_pos.0 - ship_pos.0).length_squared() < hit_dist * hit_dist {
                if sp.remaining <= 0.0 {
                    health.current = (health.current - DRONE_KAMIKAZE_DAMAGE).max(0.0);
                    last_hit.attacker = Some(drone.owner);
                    flash.timer = DAMAGE_FLASH_DURATION;
                }
                commands.entity(drone_entity).try_despawn();
                break;
            }
        }
    }
}

/// Check torpedo-ship overlaps. Torpedoes deal damage and despawn on hit.
pub fn check_torpedo_hits(
    mut commands: Commands,
    torpedoes: Query<(Entity, &Torpedo, &Position)>,
    mut ships: Query<(
        &Position,
        &Team,
        &ShipClass,
        &SpawnProtection,
        &mut Health,
        &mut DamageFlash,
        &mut LastDamagedBy,
    )>,
    grids: Res<CollisionGrids>,
    mut candidates: Local<Vec<(Entity, Vec2)>>,
) {
    for (torp_entity, torp, torp_pos) in torpedoes.iter() {
        candidates.clear();
        grids.ships.for_each_candidate(
            torp_pos.0,
            TORPEDO_RADIUS + MAX_SHIP_RADIUS,
            |e| candidates.push(e),
        );
        for &(ship_entity, _) in candidates.iter() {
            let Ok((ship_pos, ship_team, ship_class, sp, mut health, mut flash, mut last_hit)) =
                ships.get_mut(ship_entity)
            else {
                continue;
            };
            if *ship_team == torp.owner_team {
                continue;
            }
            let hit_dist = TORPEDO_RADIUS + ship_radius(ship_class);
            if (torp_pos.0 - ship_pos.0).length_squared() < hit_dist * hit_dist {
                if sp.remaining <= 0.0 {
                    health.current = (health.current - torp.damage).max(0.0);
                    last_hit.attacker = Some(torp.owner);
                    flash.timer = DAMAGE_FLASH_DURATION;
                }
                commands.entity(torp_entity).try_despawn();
                break;
            }
        }
    }
}
