use std::collections::HashMap;

use bevy::prelude::*;

use btl_protocol::{Drone, Mine, PlayerId, Projectile, Team};
use btl_shared::Position;

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EntityPositionCache>();
        app.insert_resource(EffectRng(btl_shared::rng::Rng::new(0xEFFE_C700_DEAD_CAFE)));
        app.add_systems(
            Update,
            (
                // detect_despawned_effects must read the cache BEFORE update_entity_cache clears it
                detect_despawned_effects.before(update_entity_cache),
                update_entity_cache,
                spawn_muzzle_flashes,
                spawn_mine_drop_flashes,
                update_effect_particles,
                update_flash_effects,
            ),
        );
    }
}

/// Caches entity positions from the previous frame for despawn detection.
#[derive(Resource, Default)]
struct EntityPositionCache {
    projectiles: HashMap<Entity, (Vec2, f32)>, // (position, remaining lifetime)
    mines: HashMap<Entity, (Vec2, f32)>,       // (position, remaining lifetime)
    ships: HashMap<Entity, (Vec2, Team)>,      // (position, team) for death explosions
    drones: HashMap<Entity, Vec2>,             // (position) for detonation explosions
}

#[derive(Resource, Deref, DerefMut)]
struct EffectRng(btl_shared::rng::Rng);

#[derive(Component)]
struct EffectParticle {
    velocity: Vec2,
    lifetime: f32,
    max_lifetime: f32,
    start_size: f32,
    end_size: f32,
}

#[derive(Component)]
struct FlashEffect {
    lifetime: f32,
}

/// Runs BEFORE cache update — uses last frame's cache to detect despawns.
fn detect_despawned_effects(
    mut commands: Commands,
    cache: Res<EntityPositionCache>,
    mut removed_projectiles: RemovedComponents<Projectile>,
    mut removed_mines: RemovedComponents<Mine>,
    mut removed_ships: RemovedComponents<PlayerId>,
    mut removed_drones: RemovedComponents<Drone>,
    mut rng: ResMut<EffectRng>,
) {
    for entity in removed_projectiles.read() {
        if let Some(&(pos, lifetime)) = cache.projectiles.get(&entity)
            && lifetime > 0.2
        {
            spawn_impact_sparks(&mut commands, pos, &mut rng);
        }
    }

    for entity in removed_mines.read() {
        if let Some(&(pos, lifetime)) = cache.mines.get(&entity) {
            // Only show detonation effect if the mine didn't just expire
            if lifetime > 0.5 {
                spawn_mine_detonation(&mut commands, pos, &mut rng);
            }
        }
    }

    for entity in removed_ships.read() {
        if let Some(&(pos, team)) = cache.ships.get(&entity) {
            spawn_ship_explosion(&mut commands, pos, &team, &mut rng);
        }
    }

    for entity in removed_drones.read() {
        if let Some(&pos) = cache.drones.get(&entity) {
            spawn_drone_detonation(&mut commands, pos, &mut rng);
        }
    }
}

/// Update cache with current frame's positions (runs after despawn detection).
fn update_entity_cache(
    projectiles: Query<(Entity, &Position, &Projectile)>,
    mines: Query<(Entity, &Position, &Mine)>,
    ships: Query<(Entity, &Position, &Team), With<PlayerId>>,
    drones: Query<(Entity, &Position), With<Drone>>,
    mut cache: ResMut<EntityPositionCache>,
) {
    cache.projectiles.clear();
    for (entity, pos, proj) in projectiles.iter() {
        cache.projectiles.insert(entity, (pos.0, proj.lifetime));
    }
    cache.mines.clear();
    for (entity, pos, mine) in mines.iter() {
        cache.mines.insert(entity, (pos.0, mine.lifetime));
    }
    cache.ships.clear();
    for (entity, pos, team) in ships.iter() {
        cache.ships.insert(entity, (pos.0, *team));
    }
    cache.drones.clear();
    for (entity, pos) in drones.iter() {
        cache.drones.insert(entity, pos.0);
    }
}

/// Spawn muzzle flash for newly appeared projectiles.
fn spawn_muzzle_flashes(
    mut commands: Commands,
    new_projectiles: Query<&Position, Added<Projectile>>,
) {
    for pos in new_projectiles.iter() {
        commands.spawn((
            FlashEffect { lifetime: 0.04 },
            Sprite {
                color: Color::LinearRgba(LinearRgba::new(4.0, 3.0, 1.5, 0.9)),
                custom_size: Some(Vec2::splat(5.0)),
                ..default()
            },
            Transform::from_xyz(pos.0.x, pos.0.y, 6.0),
        ));
    }
}

/// Spawn white flash when mines are dropped.
fn spawn_mine_drop_flashes(mut commands: Commands, new_mines: Query<&Position, Added<Mine>>) {
    for pos in new_mines.iter() {
        commands.spawn((
            FlashEffect { lifetime: 0.06 },
            Sprite {
                color: Color::LinearRgba(LinearRgba::new(2.0, 2.0, 2.0, 0.7)),
                custom_size: Some(Vec2::splat(12.0)),
                ..default()
            },
            Transform::from_xyz(pos.0.x, pos.0.y, 5.5),
        ));
    }
}

/// Yellow spark burst at projectile impact point.
fn spawn_impact_sparks(commands: &mut Commands, pos: Vec2, rng: &mut EffectRng) {
    let count = 5;
    for i in 0..count {
        let base_angle = (i as f32 / count as f32) * std::f32::consts::TAU;
        let angle = base_angle + (rng.next_f32() - 0.5) * 0.6;
        let speed = 150.0 + rng.next_f32() * 150.0;
        let vel = Vec2::new(angle.cos(), angle.sin()) * speed;
        commands.spawn((
            EffectParticle {
                velocity: vel,
                lifetime: 0.12 + rng.next_f32() * 0.06,
                max_lifetime: 0.15,
                start_size: 2.5,
                end_size: 0.5,
            },
            Sprite {
                color: Color::LinearRgba(LinearRgba::new(4.0, 3.0, 1.0, 1.0)),
                custom_size: Some(Vec2::splat(2.5)),
                ..default()
            },
            Transform::from_xyz(pos.x, pos.y, 6.0),
        ));
    }
}

/// Mine detonation: central flash + expanding ring of particles + debris.
fn spawn_mine_detonation(commands: &mut Commands, pos: Vec2, rng: &mut EffectRng) {
    // Central flash
    commands.spawn((
        FlashEffect { lifetime: 0.12 },
        Sprite {
            color: Color::LinearRgba(LinearRgba::new(6.0, 2.0, 1.0, 1.0)),
            custom_size: Some(Vec2::splat(40.0)),
            ..default()
        },
        Transform::from_xyz(pos.x, pos.y, 6.0),
    ));

    // Expanding ring particles
    let ring_count = 16;
    for i in 0..ring_count {
        let angle = (i as f32 / ring_count as f32) * std::f32::consts::TAU;
        let speed = 280.0 + rng.next_f32() * 120.0;
        let vel = Vec2::new(angle.cos(), angle.sin()) * speed;
        commands.spawn((
            EffectParticle {
                velocity: vel,
                lifetime: 0.2 + rng.next_f32() * 0.08,
                max_lifetime: 0.25,
                start_size: 4.0,
                end_size: 1.0,
            },
            Sprite {
                color: Color::LinearRgba(LinearRgba::new(4.0, 1.0, 0.5, 1.0)),
                custom_size: Some(Vec2::splat(4.0)),
                ..default()
            },
            Transform::from_xyz(pos.x, pos.y, 5.8),
        ));
    }

    // Scattered debris
    for _ in 0..8 {
        let angle = rng.next_f32() * std::f32::consts::TAU;
        let speed = 80.0 + rng.next_f32() * 180.0;
        let vel = Vec2::new(angle.cos(), angle.sin()) * speed;
        commands.spawn((
            EffectParticle {
                velocity: vel,
                lifetime: 0.3 + rng.next_f32() * 0.15,
                max_lifetime: 0.4,
                start_size: 2.0,
                end_size: 0.3,
            },
            Sprite {
                color: Color::LinearRgba(LinearRgba::new(2.0, 0.5, 0.2, 0.8)),
                custom_size: Some(Vec2::splat(2.0)),
                ..default()
            },
            Transform::from_xyz(pos.x, pos.y, 5.7),
        ));
    }
}

/// Drone detonation: small bright cyan/white flash + fast sparks.
fn spawn_drone_detonation(commands: &mut Commands, pos: Vec2, rng: &mut EffectRng) {
    // Central flash — cyan/white
    commands.spawn((
        FlashEffect { lifetime: 0.08 },
        Sprite {
            color: Color::LinearRgba(LinearRgba::new(2.0, 4.0, 6.0, 1.0)),
            custom_size: Some(Vec2::splat(20.0)),
            ..default()
        },
        Transform::from_xyz(pos.x, pos.y, 6.0),
    ));

    // Fast sparks radiating outward
    for _ in 0..10 {
        let angle = rng.next_f32() * std::f32::consts::TAU;
        let speed = 200.0 + rng.next_f32() * 150.0;
        let vel = Vec2::new(angle.cos(), angle.sin()) * speed;
        commands.spawn((
            EffectParticle {
                velocity: vel,
                lifetime: 0.15 + rng.next_f32() * 0.08,
                max_lifetime: 0.2,
                start_size: 2.5,
                end_size: 0.5,
            },
            Sprite {
                color: Color::LinearRgba(LinearRgba::new(1.5, 3.0, 5.0, 0.9)),
                custom_size: Some(Vec2::splat(2.5)),
                ..default()
            },
            Transform::from_xyz(pos.x, pos.y, 5.8),
        ));
    }

    // Hot debris
    for _ in 0..5 {
        let angle = rng.next_f32() * std::f32::consts::TAU;
        let speed = 60.0 + rng.next_f32() * 100.0;
        let vel = Vec2::new(angle.cos(), angle.sin()) * speed;
        commands.spawn((
            EffectParticle {
                velocity: vel,
                lifetime: 0.2 + rng.next_f32() * 0.1,
                max_lifetime: 0.3,
                start_size: 1.5,
                end_size: 0.3,
            },
            Sprite {
                color: Color::LinearRgba(LinearRgba::new(3.0, 2.0, 1.0, 0.7)),
                custom_size: Some(Vec2::splat(1.5)),
                ..default()
            },
            Transform::from_xyz(pos.x, pos.y, 5.7),
        ));
    }
}

/// Ship death explosion: large flash + expanding fireball + team-colored debris.
fn spawn_ship_explosion(commands: &mut Commands, pos: Vec2, team: &Team, rng: &mut EffectRng) {
    // Large central flash
    commands.spawn((
        FlashEffect { lifetime: 0.2 },
        Sprite {
            color: Color::LinearRgba(LinearRgba::new(8.0, 6.0, 3.0, 1.0)),
            custom_size: Some(Vec2::splat(80.0)),
            ..default()
        },
        Transform::from_xyz(pos.x, pos.y, 7.0),
    ));

    // Secondary flash (slightly delayed feel via smaller initial size)
    commands.spawn((
        FlashEffect { lifetime: 0.15 },
        Sprite {
            color: Color::LinearRgba(LinearRgba::new(10.0, 8.0, 6.0, 0.8)),
            custom_size: Some(Vec2::splat(50.0)),
            ..default()
        },
        Transform::from_xyz(pos.x, pos.y, 7.1),
    ));

    // Expanding fireball ring
    let ring_count = 24;
    for i in 0..ring_count {
        let angle = (i as f32 / ring_count as f32) * std::f32::consts::TAU
            + (rng.next_f32() - 0.5) * 0.3;
        let speed = 200.0 + rng.next_f32() * 200.0;
        let vel = Vec2::new(angle.cos(), angle.sin()) * speed;
        commands.spawn((
            EffectParticle {
                velocity: vel,
                lifetime: 0.3 + rng.next_f32() * 0.15,
                max_lifetime: 0.4,
                start_size: 6.0 + rng.next_f32() * 4.0,
                end_size: 1.0,
            },
            Sprite {
                color: Color::LinearRgba(LinearRgba::new(5.0, 2.0, 0.8, 1.0)),
                custom_size: Some(Vec2::splat(6.0)),
                ..default()
            },
            Transform::from_xyz(pos.x, pos.y, 6.5),
        ));
    }

    // Team-colored hull debris (larger, slower pieces)
    let team_color = match team {
        Team::Red => LinearRgba::new(1.2, 0.3, 0.2, 0.9),
        Team::Blue => LinearRgba::new(0.2, 0.3, 1.2, 0.9),
    };
    for _ in 0..12 {
        let angle = rng.next_f32() * std::f32::consts::TAU;
        let speed = 60.0 + rng.next_f32() * 160.0;
        let vel = Vec2::new(angle.cos(), angle.sin()) * speed;
        commands.spawn((
            EffectParticle {
                velocity: vel,
                lifetime: 0.5 + rng.next_f32() * 0.4,
                max_lifetime: 0.8,
                start_size: 3.0 + rng.next_f32() * 2.0,
                end_size: 0.5,
            },
            Sprite {
                color: Color::LinearRgba(team_color),
                custom_size: Some(Vec2::splat(3.0)),
                ..default()
            },
            Transform::from_xyz(pos.x, pos.y, 6.2),
        ));
    }

    // Hot sparks (fast, tiny, bright)
    for _ in 0..16 {
        let angle = rng.next_f32() * std::f32::consts::TAU;
        let speed = 300.0 + rng.next_f32() * 300.0;
        let vel = Vec2::new(angle.cos(), angle.sin()) * speed;
        commands.spawn((
            EffectParticle {
                velocity: vel,
                lifetime: 0.15 + rng.next_f32() * 0.1,
                max_lifetime: 0.2,
                start_size: 2.0,
                end_size: 0.3,
            },
            Sprite {
                color: Color::LinearRgba(LinearRgba::new(6.0, 5.0, 2.0, 1.0)),
                custom_size: Some(Vec2::splat(2.0)),
                ..default()
            },
            Transform::from_xyz(pos.x, pos.y, 6.8),
        ));
    }
}

/// Move and fade effect particles.
fn update_effect_particles(
    mut commands: Commands,
    mut query: Query<(Entity, &mut EffectParticle, &mut Transform, &mut Sprite)>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    for (entity, mut p, mut tf, mut sprite) in query.iter_mut() {
        p.lifetime -= dt;
        if p.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }

        tf.translation.x += p.velocity.x * dt;
        tf.translation.y += p.velocity.y * dt;

        let t = (p.lifetime / p.max_lifetime).clamp(0.0, 1.0);
        let size = p.start_size * t + p.end_size * (1.0 - t);
        sprite.custom_size = Some(Vec2::splat(size));

        // Fade alpha
        if let Color::LinearRgba(ref mut c) = sprite.color {
            c.alpha = t * t;
        }
    }
}

/// Fade and despawn flash effects.
fn update_flash_effects(
    mut commands: Commands,
    mut query: Query<(Entity, &mut FlashEffect, &mut Sprite)>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    for (entity, mut flash, mut sprite) in query.iter_mut() {
        flash.lifetime -= dt;
        if flash.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }

        // Rapid shrink + fade
        if let Color::LinearRgba(ref mut c) = sprite.color {
            c.alpha *= 0.7;
        }
        if let Some(ref mut size) = sprite.custom_size {
            *size *= 0.85;
        }
    }
}
