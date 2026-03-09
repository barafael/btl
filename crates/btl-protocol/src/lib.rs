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

// --- Inputs ---

#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone, Reflect)]
pub struct ShipInput {
    pub thrust_forward: bool,
    pub thrust_backward: bool,
    pub rotate_left: bool,
    pub rotate_right: bool,
    pub strafe_left: bool,
    pub strafe_right: bool,
    pub afterburner: bool,
    pub stabilize: bool,
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
    }
}
