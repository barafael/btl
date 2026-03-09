use bevy::prelude::*;

const STAR_COUNT: usize = 400;
const FIELD_HALF_SIZE: f32 = 2000.0;
const STAR_LAYERS: &[(f32, f32, Color)] = &[
    // (parallax_factor, size, color)
    // Lower parallax = further away, moves less
    (0.05, 1.0, Color::srgba(0.6, 0.6, 0.7, 0.3)),
    (0.15, 1.5, Color::srgba(0.7, 0.7, 0.8, 0.5)),
    (0.3, 2.0, Color::srgba(0.8, 0.8, 0.9, 0.7)),
    (0.5, 2.5, Color::srgba(1.0, 1.0, 1.0, 0.9)),
];

#[derive(Component)]
struct Star {
    /// World position of this star (never changes)
    base_pos: Vec2,
    /// How much this star moves relative to the camera (0 = fixed, 1 = moves with world)
    parallax: f32,
}

pub struct StarfieldPlugin;

impl Plugin for StarfieldPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_stars);
        app.add_systems(PostUpdate, update_star_positions);
    }
}

fn spawn_stars(mut commands: Commands) {
    let stars_per_layer = STAR_COUNT / STAR_LAYERS.len();
    // Simple deterministic hash for reproducible star positions
    let mut seed: u64 = 0xDEAD_BEEF;
    let mut rng = move || -> f32 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        // Map to 0..1
        (seed % 10000) as f32 / 10000.0
    };

    for &(parallax, size, color) in STAR_LAYERS {
        for _ in 0..stars_per_layer {
            let x = (rng() - 0.5) * 2.0 * FIELD_HALF_SIZE;
            let y = (rng() - 0.5) * 2.0 * FIELD_HALF_SIZE;

            commands.spawn((
                Star {
                    base_pos: Vec2::new(x, y),
                    parallax,
                },
                Sprite {
                    color,
                    custom_size: Some(Vec2::splat(size)),
                    ..default()
                },
                // Z far behind ships (ships are at z=0)
                Transform::from_xyz(x, y, -100.0),
            ));
        }
    }
}

fn update_star_positions(
    camera_query: Query<&Transform, (With<Camera2d>, Without<Star>)>,
    mut star_query: Query<(&Star, &mut Transform)>,
) {
    let Ok(cam) = camera_query.single() else {
        return;
    };
    let cam_pos = Vec2::new(cam.translation.x, cam.translation.y);

    for (star, mut transform) in star_query.iter_mut() {
        // Parallax: star moves with camera but slower based on depth
        let offset = cam_pos * (1.0 - star.parallax);
        let mut pos = star.base_pos + offset;

        // Wrap stars so the field tiles infinitely around the camera
        let field = FIELD_HALF_SIZE * 2.0;
        pos.x = ((pos.x - cam_pos.x + FIELD_HALF_SIZE).rem_euclid(field)) - FIELD_HALF_SIZE + cam_pos.x;
        pos.y = ((pos.y - cam_pos.y + FIELD_HALF_SIZE).rem_euclid(field)) - FIELD_HALF_SIZE + cam_pos.y;

        transform.translation.x = pos.x;
        transform.translation.y = pos.y;
    }
}
