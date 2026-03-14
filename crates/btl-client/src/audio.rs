#![allow(clippy::type_complexity, clippy::too_many_arguments)]

use avian2d::prelude::LinearVelocity;
use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;
use lightyear::prelude::client::*;
use lightyear::prelude::input::native::ActionState;
use lightyear::prelude::*;
use btl_protocol::{Mine, PlayerId, Projectile, ProjectileKind, RailgunCharge, ShipClass, ShipInput, Team, TeamScores, Torpedo};
use btl_shared::{Ammo, SHIP_MAX_SPEED};
use crate::client::LocalShip;

// --- Resources ---

#[derive(Resource)]
pub struct AudioAssets {
    pub engine_hum: Handle<AudioSource>,
    pub ambient_drone: Handle<AudioSource>,
    pub autocannon: Handle<AudioSource>,
    pub heavy_cannon: Handle<AudioSource>,
    pub laser_loop: Handle<AudioSource>,
    pub torpedo_launch: Handle<AudioSource>,
    pub railgun_charge: Handle<AudioSource>,
    pub railgun_fire: Handle<AudioSource>,
    pub explosion_large: Handle<AudioSource>,
    pub explosion_medium: Handle<AudioSource>,
    pub zone_capture: Handle<AudioSource>,
    pub zone_flip: Handle<AudioSource>,
    pub respawn: Handle<AudioSource>,
}

#[derive(Resource, Default)]
pub(crate) struct LocalSpawnCount(u32);

// --- Marker components ---

#[derive(Component)]
pub struct EngineHumSink;

#[derive(Component)]
pub struct LaserLoopSink;

#[derive(Component)]
pub struct RailgunChargeSink;

// --- Systems ---

pub fn load_audio_assets(mut commands: Commands, asset_server: Res<AssetServer>) {
    let assets = AudioAssets {
        engine_hum: asset_server.load("audio/engine_hum.wav"),
        ambient_drone: asset_server.load("audio/ambient_drone.wav"),
        autocannon: asset_server.load("audio/autocannon.wav"),
        heavy_cannon: asset_server.load("audio/heavy_cannon.wav"),
        laser_loop: asset_server.load("audio/laser_loop.wav"),
        torpedo_launch: asset_server.load("audio/torpedo_launch.wav"),
        railgun_charge: asset_server.load("audio/railgun_charge.wav"),
        railgun_fire: asset_server.load("audio/railgun_fire.wav"),
        explosion_large: asset_server.load("audio/explosion_large.wav"),
        explosion_medium: asset_server.load("audio/explosion_medium.wav"),
        zone_capture: asset_server.load("audio/zone_capture.wav"),
        zone_flip: asset_server.load("audio/zone_flip.wav"),
        respawn: asset_server.load("audio/respawn.wav"),
    };
    commands.insert_resource(assets);
}

pub fn spawn_ambient(mut commands: Commands, assets: Res<AudioAssets>) {
    commands.spawn((
        AudioPlayer::new(assets.ambient_drone.clone()),
        PlaybackSettings {
            mode: PlaybackMode::Loop,
            volume: Volume::Linear(0.08),
            ..default()
        },
    ));
}

pub fn spawn_engine_hum(
    mut commands: Commands,
    assets: Res<AudioAssets>,
    new_local_ships: Query<(), Added<LocalShip>>,
    existing_sinks: Query<Entity, With<EngineHumSink>>,
) {
    if new_local_ships.is_empty() {
        return;
    }
    // Despawn any existing engine hum sinks
    for entity in &existing_sinks {
        commands.entity(entity).despawn();
    }
    // Spawn new engine hum
    commands.spawn((
        AudioPlayer::new(assets.engine_hum.clone()),
        PlaybackSettings {
            mode: PlaybackMode::Loop,
            volume: Volume::Linear(0.35),
            ..default()
        },
        EngineHumSink,
    ));
}

pub fn update_engine_hum(
    local_ships: Query<&LinearVelocity, With<LocalShip>>,
    engine_sinks: Query<&AudioSink, With<EngineHumSink>>,
) {
    let Ok(vel) = local_ships.single() else {
        return;
    };
    let Ok(sink) = engine_sinks.single() else {
        return;
    };
    let speed = vel.length();
    let pitch = 0.55 + (speed / SHIP_MAX_SPEED).min(1.0) * 0.85;
    sink.set_speed(pitch);
}

pub fn trigger_weapon_sounds(
    mut commands: Commands,
    assets: Res<AudioAssets>,
    new_projectiles: Query<(&Projectile, Option<&ProjectileKind>), Added<Projectile>>,
    new_torpedoes: Query<&Torpedo, Added<Torpedo>>,
    local_ship: Query<&PlayerId, With<LocalShip>>,
) {
    let Ok(local_id) = local_ship.single() else {
        return;
    };

    for (proj, kind) in &new_projectiles {
        if proj.owner != local_id.0 {
            continue;
        }
        let (handle, volume) = match kind {
            Some(ProjectileKind::HeavyCannon) => (assets.heavy_cannon.clone(), 0.55),
            Some(ProjectileKind::Railgun) => (assets.railgun_fire.clone(), 0.65),
            Some(ProjectileKind::Turret) => continue,
            // Autocannon or None
            _ => (assets.autocannon.clone(), 0.45),
        };
        commands.spawn((
            AudioPlayer::new(handle),
            PlaybackSettings {
                mode: PlaybackMode::Despawn,
                volume: Volume::Linear(volume),
                ..default()
            },
        ));
    }

    for torpedo in &new_torpedoes {
        if torpedo.owner != local_id.0 {
            continue;
        }
        commands.spawn((
            AudioPlayer::new(assets.torpedo_launch.clone()),
            PlaybackSettings {
                mode: PlaybackMode::Despawn,
                volume: Volume::Linear(0.5),
                ..default()
            },
        ));
    }
}

pub fn manage_laser_audio(
    mut commands: Commands,
    assets: Res<AudioAssets>,
    local_ship: Query<(&ShipClass, &ActionState<ShipInput>, &Ammo), With<LocalShip>>,
    laser_sinks: Query<Entity, With<LaserLoopSink>>,
) {
    let is_firing = if let Ok((class, input, ammo)) = local_ship.single() {
        *class == ShipClass::TorpedoBoat && input.0.fire && ammo.current > 0.0
    } else {
        false
    };

    if is_firing && laser_sinks.is_empty() {
        commands.spawn((
            AudioPlayer::new(assets.laser_loop.clone()),
            PlaybackSettings {
                mode: PlaybackMode::Loop,
                volume: Volume::Linear(0.5),
                ..default()
            },
            LaserLoopSink,
        ));
    } else if !is_firing {
        for entity in &laser_sinks {
            commands.entity(entity).despawn();
        }
    }
}

pub fn manage_railgun_charge_audio(
    mut commands: Commands,
    assets: Res<AudioAssets>,
    local_ship: Query<(&ShipClass, &RailgunCharge), With<LocalShip>>,
    charge_sinks: Query<Entity, With<RailgunChargeSink>>,
) {
    let is_charging = if let Ok((class, charge)) = local_ship.single() {
        *class == ShipClass::Sniper && charge.charge > 0.02
    } else {
        false
    };

    if is_charging && charge_sinks.is_empty() {
        commands.spawn((
            AudioPlayer::new(assets.railgun_charge.clone()),
            PlaybackSettings {
                mode: PlaybackMode::Once,
                volume: Volume::Linear(0.55),
                ..default()
            },
            RailgunChargeSink,
        ));
    } else if !is_charging {
        for entity in &charge_sinks {
            commands.entity(entity).despawn();
        }
    }
}

pub fn trigger_explosion_sounds(
    mut commands: Commands,
    assets: Res<AudioAssets>,
    mut removed_players: RemovedComponents<PlayerId>,
    mut removed_torpedoes: RemovedComponents<Torpedo>,
    mut removed_mines: RemovedComponents<Mine>,
) {
    for _entity in removed_players.read() {
        commands.spawn((
            AudioPlayer::new(assets.explosion_large.clone()),
            PlaybackSettings {
                mode: PlaybackMode::Despawn,
                volume: Volume::Linear(0.6),
                ..default()
            },
        ));
    }

    for _entity in removed_torpedoes.read() {
        commands.spawn((
            AudioPlayer::new(assets.explosion_medium.clone()),
            PlaybackSettings {
                mode: PlaybackMode::Despawn,
                volume: Volume::Linear(0.45),
                ..default()
            },
        ));
    }

    for _entity in removed_mines.read() {
        commands.spawn((
            AudioPlayer::new(assets.explosion_medium.clone()),
            PlaybackSettings {
                mode: PlaybackMode::Despawn,
                volume: Volume::Linear(0.35),
                ..default()
            },
        ));
    }
}

pub fn detect_zone_events(
    mut commands: Commands,
    assets: Res<AudioAssets>,
    scores_query: Query<Ref<TeamScores>>,
    mut prev_progresses: Local<Option<[f32; 3]>>,
) {
    let Ok(scores) = scores_query.single() else {
        return;
    };

    let current: [f32; 3] = [
        scores.zones[0].progress,
        scores.zones[1].progress,
        scores.zones[2].progress,
    ];

    let Some(prev) = *prev_progresses else {
        *prev_progresses = Some(current);
        return;
    };

    if scores.is_changed() {
        for i in 0..3 {
            let p = prev[i];
            let c = current[i];

            // Zone flip: sign change (crossing zero from one team's control to the other)
            if p.signum() != c.signum() && p != 0.0 && c != 0.0 {
                commands.spawn((
                    AudioPlayer::new(assets.zone_flip.clone()),
                    PlaybackSettings {
                        mode: PlaybackMode::Despawn,
                        volume: Volume::Linear(0.5),
                        ..default()
                    },
                ));
            }

            // Full capture: |progress| crosses 0.95 from below
            if p.abs() < 0.95 && c.abs() >= 0.95 {
                commands.spawn((
                    AudioPlayer::new(assets.zone_capture.clone()),
                    PlaybackSettings {
                        mode: PlaybackMode::Despawn,
                        volume: Volume::Linear(0.55),
                        ..default()
                    },
                ));
            }
        }
    }

    *prev_progresses = Some(current);
}

pub fn detect_local_respawn(
    mut commands: Commands,
    assets: Res<AudioAssets>,
    new_local_ships: Query<(), Added<LocalShip>>,
    mut spawn_count: ResMut<LocalSpawnCount>,
) {
    for _ in &new_local_ships {
        if spawn_count.0 == 0 {
            // First spawn — just count it, no sound
        } else {
            commands.spawn((
                AudioPlayer::new(assets.respawn.clone()),
                PlaybackSettings {
                    mode: PlaybackMode::Despawn,
                    volume: Volume::Linear(0.55),
                    ..default()
                },
            ));
        }
        spawn_count.0 += 1;
    }
}

// --- Plugin ---

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LocalSpawnCount>();
        app.add_systems(Startup, (load_audio_assets, spawn_ambient).chain());
        app.add_systems(
            Update,
            (
                spawn_engine_hum,
                update_engine_hum,
                trigger_weapon_sounds,
                manage_laser_audio,
                manage_railgun_charge_audio,
                trigger_explosion_sounds,
                detect_zone_events,
                detect_local_respawn,
            ),
        );
    }
}
