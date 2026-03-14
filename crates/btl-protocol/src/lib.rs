use bevy::ecs::entity::MapEntities;
use bevy::prelude::*;
use lightyear::prelude::input::native::InputPlugin;
use lightyear::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;

// --- Constants ---

pub const FIXED_TIMESTEP_HZ: f64 = 60.0;
pub const REPLICATION_INTERVAL: Duration = Duration::from_millis(50);
pub const PROTOCOL_ID: u64 = 0xB7_0000;
pub const PRIVATE_KEY: [u8; 32] = [0u8; 32];
pub const SERVER_PORT: u16 = 5888;

// --- Components ---

/// Identifies which player owns this entity.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlayerId(pub PeerId);

/// Team assignment for a player.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Copy)]
pub enum Team {
    Red,
    Blue,
}

/// Ship class determines weapons, stats, and visuals.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Copy, Default)]
pub enum ShipClass {
    #[default]
    Interceptor,
    Gunship,
    TorpedoBoat,
    Sniper,
    DroneCommander,
}

impl ShipClass {
    /// Convert from class_request u8: 1=Interceptor, 2=Gunship, 3=TorpedoBoat, 4=Sniper, 5=DroneCommander, 0/other=None.
    pub fn from_request(v: u8) -> Option<Self> {
        match v {
            1 => Some(ShipClass::Interceptor),
            2 => Some(ShipClass::Gunship),
            3 => Some(ShipClass::TorpedoBoat),
            4 => Some(ShipClass::Sniper),
            5 => Some(ShipClass::DroneCommander),
            _ => None,
        }
    }

    pub fn to_request(self) -> u8 {
        match self {
            ShipClass::Interceptor => 1,
            ShipClass::Gunship => 2,
            ShipClass::TorpedoBoat => 3,
            ShipClass::Sniper => 4,
            ShipClass::DroneCommander => 5,
        }
    }
}

/// State of a single turret mount on a ship.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TurretState {
    /// Current aim angle in world-space radians.
    pub aim_angle: f32,
    /// Cooldown remaining before next shot.
    pub cooldown: f32,
}

/// Auto-turrets on a ship. Empty for classes without turrets.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Turrets {
    pub mounts: Vec<TurretState>,
}

/// Visual kind of a projectile (for client rendering differentiation).
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Copy, Default)]
pub enum ProjectileKind {
    #[default]
    Autocannon,
    HeavyCannon,
    Turret,
    Railgun,
}

/// Marker for asteroid entities. Stores the radius for rendering/collision.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Asteroid {
    pub radius: f32,
}

/// A current/max gauge — shared structure for health, fuel, ammo, etc.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Gauge {
    pub current: f32,
    pub max: f32,
}

impl Gauge {
    pub fn new(max: f32) -> Self {
        Self { current: max, max }
    }

    pub fn fraction(&self) -> f32 {
        if self.max > 0.0 {
            self.current / self.max
        } else {
            0.0
        }
    }
}

/// Ship health points.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Deref, DerefMut)]
pub struct Health(pub Gauge);
impl Health {
    pub fn new(max: f32) -> Self {
        Self(Gauge::new(max))
    }
}

/// Afterburner fuel.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Deref, DerefMut)]
pub struct Fuel(pub Gauge);
impl Fuel {
    pub fn new(max: f32) -> Self {
        Self(Gauge::new(max))
    }
}

/// Autocannon ammunition.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Deref, DerefMut)]
pub struct Ammo(pub Gauge);
impl Ammo {
    pub fn new(max: f32) -> Self {
        Self(Gauge::new(max))
    }
}

/// Projectile marker. Carries damage, owner, and remaining lifetime.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Projectile {
    pub damage: f32,
    pub owner: PeerId,
    pub owner_team: Team,
    pub lifetime: f32,
}

/// Proximity mine. Detonates when an enemy ship enters its trigger radius.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Mine {
    pub damage: f32,
    pub owner: PeerId,
    pub owner_team: Team,
    pub lifetime: f32,
    pub arm_timer: f32,
}

/// Homing torpedo. Steers toward nearest enemy, can be shot down.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Torpedo {
    pub damage: f32,
    pub owner: PeerId,
    pub owner_team: Team,
    pub lifetime: f32,
}

/// Cloak state for Sniper ships.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Cloak {
    pub active: bool,
    /// Remaining cloak duration when active.
    pub duration: f32,
    /// Cooldown remaining before cloak can be activated again.
    pub cooldown: f32,
}

/// Railgun charge state for Sniper ships.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct RailgunCharge {
    /// Charge progress: 0.0 (empty) to 1.0 (fully charged).
    pub charge: f32,
}

/// Drone variant: laser (orbiting mini ship) or kamikaze (tracking mine).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Copy)]
pub enum DroneKind {
    /// Orbits near commander, shoots short-range laser at enemies.
    Laser,
    /// Tracks nearest enemy and detonates on contact.
    Kamikaze,
}

/// Attack drone entity. Small AI-controlled unit belonging to a Drone Commander.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Drone {
    pub owner: PeerId,
    pub owner_team: Team,
    pub health: f32,
    pub kind: DroneKind,
}

/// A cooldown timer that ticks toward zero.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Cooldown {
    pub remaining: f32,
}

/// Tracks when a ship can next fire.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default, Deref, DerefMut)]
pub struct FireCooldown(pub Cooldown);

/// Tracks when a ship can next drop a mine.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default, Deref, DerefMut)]
pub struct MineCooldown(pub Cooldown);

/// Seed for procedural nebula background generation.
/// Server picks a seed; clients generate the texture locally.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct NebulaSeed(pub u64);

/// Tracks who last dealt damage to this ship (for kill attribution).
/// Server-only — not replicated.
#[derive(Component, Clone, Debug, Default)]
pub struct LastDamagedBy {
    pub attacker: Option<PeerId>,
}

/// Spawn invulnerability timer. Ship cannot take damage while > 0.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct SpawnProtection {
    pub remaining: f32,
}

/// Damage flash timer. Client uses this to render a white flash overlay.
/// Set by server when damage is taken; ticks down on both sides.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct DamageFlash {
    pub timer: f32,
}

/// Which type of defense each objective zone has.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Copy)]
pub enum ObjectiveKind {
    /// 11 defense drones (4 kamikaze, 7 laser)
    Factory,
    /// Auto-targeting railgun turret with telegraph
    Railgun,
    /// Energy shield bubble deflecting projectiles
    Powerplant,
}

/// Defense drone belonging to an objective zone (Factory).
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ZoneDrone {
    pub zone_index: u8,
    pub team: Team,
    pub kind: DroneKind,
    pub health: f32,
}

/// Auto-turret at a Railgun objective zone.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ZoneRailgun {
    pub zone_index: u8,
    pub team: Team,
    /// Current aim angle in world-space radians.
    pub aim_angle: f32,
    /// Charge progress: 0.0 to 1.0
    pub charge: f32,
    /// Cooldown remaining after firing.
    pub cooldown: f32,
    /// State machine: Tracking, Locked (telegraph pause), Firing, Cooldown
    pub state: RailgunTurretState,
}

/// Railgun turret state machine.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Copy, Default)]
pub enum RailgunTurretState {
    /// No valid target
    #[default]
    Idle,
    /// Tracking a target, building charge
    Tracking,
    /// Locked on — brief pause before firing (telegraph)
    Locked(f32),
    /// Cooling down after shot
    Cooldown,
}

/// Energy shield bubble at a Powerplant objective zone.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ZoneShield {
    pub zone_index: u8,
    pub team: Team,
    pub active: bool,
}

/// Round state: 0=Playing, 1=Red won, 2=Blue won.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default, Copy)]
pub enum RoundState {
    #[default]
    Playing,
    Won(Team),
    /// Countdown to next round (seconds remaining).
    Restarting(f32),
}

/// Per-zone capture state.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default, Copy)]
pub struct ZoneState {
    /// Capture progress: -1.0 = fully Red, 0.0 = neutral, 1.0 = fully Blue
    pub progress: f32,
    /// Current controller: 0=neutral, 1=Red, 2=Blue
    pub controller: u8,
}

/// A kill event recorded when a ship is destroyed by another player.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct KillEvent {
    pub killer_team: Team,
    pub victim_team: Team,
    pub victim_class: ShipClass,
}

/// Per-player end-of-round stats snapshot.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlayerStat {
    pub peer_id: PeerId,
    pub team: Team,
    pub kills: u32,
}

/// King-of-the-hill team scores. Replicated to all clients.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct TeamScores {
    pub red: f32,
    pub blue: f32,
    /// Per-zone capture state (3 zones)
    pub zones: [ZoneState; 3],
    /// Current round state
    pub round_state: RoundState,
    /// Recent kills, most-recent first (max KILL_FEED_MAX entries).
    pub kill_feed: Vec<KillEvent>,
    /// Per-player kill stats snapshotted at round end; cleared on restart.
    pub end_stats: Vec<PlayerStat>,
    /// Which team won the last round; preserved through Restarting phase.
    pub last_winner: Option<Team>,
}

// --- Inputs ---

#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone, Reflect)]
pub struct ShipInput {
    /// Forward thrust throttle: 0.0 (off) to 1.0 (full).
    pub thrust_forward: f32,
    /// Backward thrust throttle: 0.0 (off) to 1.0 (full, applied at 50% power).
    pub thrust_backward: f32,
    /// Rotation input: -1.0 (full right) to 1.0 (full left). 0.0 = no rotation command.
    pub rotate: f32,
    /// Strafe input: -1.0 (full right) to 1.0 (full left).
    pub strafe: f32,
    pub afterburner: bool,
    /// Stabilize throttle: 0.0 (off) to 1.0 (full retro-braking).
    pub stabilize: f32,
    pub fire: bool,
    pub drop_mine: bool,
    /// Aim direction in radians (angle from ship to cursor in world space).
    pub aim_angle: f32,
    /// Request to switch ship class: 0=none, 1=Interceptor, 2=Gunship, 3=TorpedoBoat.
    pub class_request: u8,
}

impl MapEntities for ShipInput {
    fn map_entities<M: EntityMapper>(&mut self, _entity_mapper: &mut M) {}
}

// --- Protocol Plugin ---

/// Registers protocol types (inputs, game components).
/// Does NOT register physics components — that's done in btl-shared
/// where the lightyear_avian2d integration is available.
pub struct ProtocolPlugin;

impl Plugin for ProtocolPlugin {
    fn build(&self, app: &mut App) {
        // Inputs
        app.add_plugins(InputPlugin::<ShipInput>::default());

        // Game components (prediction is added in btl-shared where the feature is available)
        app.register_component::<PlayerId>();
        app.register_component::<Team>();
        app.register_component::<Asteroid>();
        app.register_component::<Health>();
        app.register_component::<Fuel>();
        app.register_component::<Ammo>();
        app.register_component::<Projectile>();
        app.register_component::<Mine>();
        app.register_component::<FireCooldown>();
        app.register_component::<MineCooldown>();
        app.register_component::<NebulaSeed>();
        app.register_component::<ShipClass>();
        app.register_component::<Turrets>();
        app.register_component::<ProjectileKind>();
        app.register_component::<Torpedo>();
        app.register_component::<Cloak>();
        app.register_component::<RailgunCharge>();
        app.register_component::<Drone>();
        app.register_component::<TeamScores>();
        app.register_component::<SpawnProtection>();
        app.register_component::<DamageFlash>();
        app.register_component::<ZoneDrone>();
        app.register_component::<ZoneRailgun>();
        app.register_component::<ZoneShield>();
    }
}
