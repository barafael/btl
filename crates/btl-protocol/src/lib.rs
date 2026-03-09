use avian2d::prelude::*;
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
    pub afterburner: bool,
}

impl MapEntities for ShipInput {
    fn map_entities<M: EntityMapper>(&mut self, _entity_mapper: &mut M) {}
}

// --- Protocol Plugin ---

pub struct ProtocolPlugin;

impl Plugin for ProtocolPlugin {
    fn build(&self, app: &mut App) {
        // Inputs
        app.add_plugins(InputPlugin::<ShipInput>::default());

        // Components
        app.register_component::<PlayerId>();
        app.register_component::<Team>();

        // Avian physics components: prediction + interpolation + rollback thresholds
        app.register_component::<Position>()
            .add_prediction()
            .add_should_rollback(|this: &Position, that: &Position| {
                (this.0 - that.0).length() >= 0.01
            })
            .add_linear_interpolation()
            .add_linear_correction_fn();

        app.register_component::<Rotation>()
            .add_prediction()
            .add_should_rollback(|this: &Rotation, that: &Rotation| {
                this.angle_between(*that) >= 0.01
            })
            .add_linear_interpolation()
            .add_linear_correction_fn();

        app.register_component::<LinearVelocity>().add_prediction();
        app.register_component::<AngularVelocity>().add_prediction();
    }
}
