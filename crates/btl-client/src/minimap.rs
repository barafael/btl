use std::collections::HashSet;

use bevy::prelude::*;
use btl_protocol::{Cloak, PlayerId, Team};
use btl_shared::{MAP_RADIUS, OBJECTIVE_ZONE_RADIUS, objective_zone_positions};

use crate::client::{LocalShip, team_color};

/// Minimap size in pixels
const MINIMAP_SIZE: f32 = 200.0;
/// Margin from screen edge
const MINIMAP_MARGIN: f32 = 12.0;
/// Scale: world units -> minimap pixels
const MINIMAP_SCALE: f32 = MINIMAP_SIZE / (MAP_RADIUS * 2.0);
/// Radius of the circular minimap in pixels
const MINIMAP_RADIUS: f32 = MINIMAP_SIZE / 2.0;

/// How far allied ships can "see" enemies on the minimap (world units).
const SENSOR_RANGE: f32 = 1200.0;
/// Cloaked enemy snipers appear offset by up to this many world units on the minimap.
const SHIMMER_RADIUS: f32 = 130.0;
/// How many seconds between cloaked-sniper position updates.
const SHIMMER_UPDATE_SECS: f32 = 0.45;

pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_minimap);
        app.add_systems(Update, update_minimap_dots);
    }
}

#[derive(Component)]
pub(crate) struct MinimapRoot;

#[derive(Component)]
struct MinimapDot {
    tracked: Entity,
}

#[derive(Component)]
struct MinimapViewport;

/// Check if a pixel position is inside the circular minimap.
fn inside_circle(x: f32, y: f32) -> bool {
    let dx = x - MINIMAP_RADIUS;
    let dy = y - MINIMAP_RADIUS;
    dx * dx + dy * dy <= MINIMAP_RADIUS * MINIMAP_RADIUS
}

/// Deterministic shimmer offset for a cloaked ship.
/// Changes every SHIMMER_UPDATE_SECS based on entity identity + time bucket.
fn shimmer_offset(entity_bits: u64, elapsed: f32) -> Vec2 {
    let bucket = (elapsed / SHIMMER_UPDATE_SECS) as u64;
    let hx = entity_bits
        .wrapping_mul(6364136223846793005)
        .wrapping_add(bucket)
        .wrapping_mul(1442695040888963407);
    let hy = entity_bits
        .wrapping_mul(1442695040888963407)
        .wrapping_add(bucket.wrapping_add(1))
        .wrapping_mul(6364136223846793005);
    let ox = (hx >> 32) as i32 as f32 / 2147483648.0 * SHIMMER_RADIUS;
    let oy = (hy >> 32) as i32 as f32 / 2147483648.0 * SHIMMER_RADIUS;
    Vec2::new(ox, oy)
}

fn spawn_minimap(mut commands: Commands) {
    let center = MINIMAP_RADIUS;

    let root = commands
        .spawn((
            MinimapRoot,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(MINIMAP_MARGIN),
                right: Val::Px(MINIMAP_MARGIN),
                width: Val::Px(MINIMAP_SIZE),
                height: Val::Px(MINIMAP_SIZE),
                overflow: Overflow::clip(),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.05, 0.1, 0.7)),
            ZIndex(100),
        ))
        .id();

    // Objective zone circles — only spawn dots inside round minimap
    let zones = objective_zone_positions();
    let zone_dots = 30;
    let zone_r = OBJECTIVE_ZONE_RADIUS * MINIMAP_SCALE;
    for zone_center in zones {
        let cx = center + zone_center.x * MINIMAP_SCALE;
        let cy = center - zone_center.y * MINIMAP_SCALE;
        for i in 0..zone_dots {
            let angle = (i as f32 / zone_dots as f32) * std::f32::consts::TAU;
            let x = cx + zone_r * angle.cos() - 1.0;
            let y = cy + zone_r * angle.sin() - 1.0;
            if !inside_circle(x, y) {
                continue;
            }
            commands.spawn((
                ChildOf(root),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(x),
                    top: Val::Px(y),
                    width: Val::Px(2.0),
                    height: Val::Px(2.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.4, 0.4, 0.2, 0.5)),
            ));
        }
    }

    // Viewport rectangle
    commands.spawn((
        ChildOf(root),
        MinimapViewport,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Px(30.0),
            height: Val::Px(20.0),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(Color::srgba(0.8, 0.8, 0.8, 0.4)),
        BackgroundColor(Color::NONE),
    ));
}

fn update_minimap_dots(
    mut commands: Commands,
    ships: Query<(Entity, &Transform, &Team, Option<&Cloak>), With<PlayerId>>,
    mut dots: Query<(Entity, &MinimapDot, &mut Node, &mut Visibility), Without<MinimapViewport>>,
    minimap_root: Query<Entity, With<MinimapRoot>>,
    mut viewport: Query<&mut Node, (With<MinimapViewport>, Without<MinimapDot>)>,
    camera_query: Query<(&Transform, &Projection), With<Camera2d>>,
    local_ship: Query<(Entity, &Team), With<LocalShip>>,
    time: Res<Time>,
) {
    let Ok(root) = minimap_root.single() else { return; };
    let center = MINIMAP_RADIUS;

    // Need local entity + team for fog-of-war checks.
    let Ok((local_entity, local_team)) = local_ship.single() else { return; };
    let local_team = *local_team;

    // Union of all allied ship positions for sensor coverage.
    let ally_positions: Vec<Vec2> = ships
        .iter()
        .filter(|(_, _, t, _)| **t == local_team)
        .map(|(_, tf, _, _)| tf.translation.truncate())
        .collect();

    let in_sensor_range = |world_pos: Vec2| -> bool {
        ally_positions
            .iter()
            .any(|&a| (a - world_pos).length_squared() <= SENSOR_RANGE * SENSOR_RANGE)
    };

    let elapsed = time.elapsed_secs();

    // ── Update existing dots ──────────────────────────────────────────────────
    let mut existing: HashSet<Entity> = HashSet::new();
    let mut to_remove: Vec<Entity> = Vec::new();

    for (dot_entity, dot, mut node, mut vis) in dots.iter_mut() {
        let Ok((_, transform, team, cloak)) = ships.get(dot.tracked) else {
            to_remove.push(dot_entity);
            continue;
        };
        existing.insert(dot.tracked);

        let world_pos = transform.translation.truncate();
        let is_ally = *team == local_team;
        let is_cloaked_enemy = !is_ally && cloak.map_or(false, |c| c.active);

        // Fog of war: hide enemy dots outside sensor range.
        if !is_ally && !in_sensor_range(world_pos) {
            *vis = Visibility::Hidden;
            continue;
        }

        // Cloaked enemy sniper: show at jittered position.
        let draw_pos = if is_cloaked_enemy {
            world_pos + shimmer_offset(dot.tracked.to_bits(), elapsed)
        } else {
            world_pos
        };

        let size = if dot.tracked == local_entity { 5.0 } else { 4.0 };
        let x = center + draw_pos.x * MINIMAP_SCALE - size / 2.0;
        let y = center - draw_pos.y * MINIMAP_SCALE - size / 2.0;
        node.left = Val::Px(x);
        node.top = Val::Px(y);
        *vis = if inside_circle(x, y) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }

    for entity in to_remove {
        commands.entity(entity).despawn();
    }

    // ── Spawn dots for newly-seen ships ──────────────────────────────────────
    for (entity, transform, team, _) in ships.iter() {
        if existing.contains(&entity) {
            continue;
        }

        let is_ally = *team == local_team;
        let world_pos = transform.translation.truncate();

        // Don't spawn a dot for an out-of-range enemy; it will be added when
        // it enters sensor range (since existing won't contain it).
        if !is_ally && !in_sensor_range(world_pos) {
            continue;
        }

        let is_local = entity == local_entity;
        let size = if is_local { 5.0 } else { 4.0 };
        let color = team_color(team);

        let x = center + world_pos.x * MINIMAP_SCALE - size / 2.0;
        let y = center - world_pos.y * MINIMAP_SCALE - size / 2.0;

        commands.spawn((
            ChildOf(root),
            MinimapDot { tracked: entity },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(x),
                top: Val::Px(y),
                width: Val::Px(size),
                height: Val::Px(size),
                ..default()
            },
            BackgroundColor(color),
        ));
    }

    // ── Update viewport rectangle ─────────────────────────────────────────────
    if let Ok((cam_tf, projection)) = camera_query.single()
        && let Projection::Orthographic(ortho) = projection
        && let Ok(mut node) = viewport.single_mut()
    {
        let cam_x = cam_tf.translation.x;
        let cam_y = cam_tf.translation.y;
        let half_w = ortho.area.width() / 2.0;
        let half_h = ortho.area.height() / 2.0;

        let vp_w = half_w * 2.0 * MINIMAP_SCALE;
        let vp_h = half_h * 2.0 * MINIMAP_SCALE;
        let vp_x = center + (cam_x - half_w) * MINIMAP_SCALE;
        let vp_y = center - (cam_y + half_h) * MINIMAP_SCALE;

        node.left = Val::Px(vp_x);
        node.top = Val::Px(vp_y);
        node.width = Val::Px(vp_w);
        node.height = Val::Px(vp_h);
    }
}
