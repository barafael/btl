use std::collections::HashMap;

use bevy::prelude::*;

use btl_protocol::{Mine, Projectile};
use btl_shared::Position;

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EntityPositionCache>();
        app.insert_resource(EffectRng(0xEFFE_C700_DEAD_CAFE));
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
}

/// Simple RNG for effect variation.
#[derive(Resource)]
struct EffectRng(u64);

impl EffectRng {
    fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 16) as u32 & 0x00FF_FFFF) as f32 / 16777216.0
    }
}

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
    mut rng: ResMut<EffectRng>,
) {
    for entity in removed_projectiles.read() {
        if let Some(&(pos, lifetime)) = cache.projectiles.get(&entity) {
            if lifetime > 0.2 {
                spawn_impact_sparks(&mut commands, pos, &mut rng);
            }
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
}

/// Update cache with current frame's positions (runs after despawn detection).
fn update_entity_cache(
    projectiles: Query<(Entity, &Position, &Projectile)>,
    mines: Query<(Entity, &Position, &Mine)>,
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
