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
    }
}
