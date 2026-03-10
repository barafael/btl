use bevy::prelude::*;
use btl_protocol::{PlayerId, Team};
use btl_shared::{
    MAP_RADIUS, OBJECTIVE_ZONE_RADIUS,
    objective_zone_positions,
};

use crate::client::LocalShip;

/// Minimap size in pixels
const MINIMAP_SIZE: f32 = 200.0;
/// Margin from screen edge
const MINIMAP_MARGIN: f32 = 12.0;
/// Scale: world units -> minimap pixels
const MINIMAP_SCALE: f32 = MINIMAP_SIZE / (MAP_RADIUS * 2.0);

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

fn spawn_minimap(mut commands: Commands) {
    let center = MINIMAP_SIZE / 2.0;

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
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.05, 0.1, 0.7)),
            ZIndex(100),
        ))
        .id();

    // Map boundary circle
    let boundary_dots = 60;
    let boundary_r = MAP_RADIUS * MINIMAP_SCALE;
    for i in 0..boundary_dots {
        let angle = (i as f32 / boundary_dots as f32) * std::f32::consts::TAU;
        let x = center + boundary_r * angle.cos() - 1.0;
        let y = center - boundary_r * angle.sin() - 1.0;
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
            BackgroundColor(Color::srgba(0.3, 0.1, 0.1, 0.6)),
        ));
    }

    // Objective zone circles
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
    ships: Query<(Entity, &Transform, &Team), With<PlayerId>>,
    mut dots: Query<(Entity, &MinimapDot, &mut Node), Without<MinimapViewport>>,
    minimap_root: Query<Entity, With<MinimapRoot>>,
    mut viewport: Query<&mut Node, (With<MinimapViewport>, Without<MinimapDot>)>,
    camera_query: Query<(&Transform, &Projection), With<Camera2d>>,
    local_ship: Query<(), With<LocalShip>>,
) {
    let Ok(root) = minimap_root.single() else { return };
    let center = MINIMAP_SIZE / 2.0;

    let mut existing: std::collections::HashSet<Entity> = std::collections::HashSet::new();
    let mut to_remove = Vec::new();

    for (dot_entity, dot, mut node) in dots.iter_mut() {
        if let Ok((_, transform, _)) = ships.get(dot.tracked) {
            existing.insert(dot.tracked);
            let x = center + transform.translation.x * MINIMAP_SCALE - 2.0;
            let y = center - transform.translation.y * MINIMAP_SCALE - 2.0;
            node.left = Val::Px(x);
            node.top = Val::Px(y);
        } else {
            to_remove.push(dot_entity);
        }
    }

    for entity in to_remove {
        commands.entity(entity).despawn();
    }

    for (entity, transform, team) in ships.iter() {
        if existing.contains(&entity) {
            continue;
        }

        let color = match team {
            Team::Red => Color::srgb(1.0, 0.3, 0.3),
            Team::Blue => Color::srgb(0.3, 0.3, 1.0),
        };

        let is_local = local_ship.get(entity).is_ok();
        let size = if is_local { 5.0 } else { 4.0 };

        let x = center + transform.translation.x * MINIMAP_SCALE - size / 2.0;
        let y = center - transform.translation.y * MINIMAP_SCALE - size / 2.0;

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
            BackgroundColor(color.into()),
        ));
    }

    // Update viewport rectangle
    if let Ok((cam_tf, projection)) = camera_query.single() {
        if let Projection::Orthographic(ortho) = projection {
            if let Ok(mut node) = viewport.single_mut() {
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
    }
}
