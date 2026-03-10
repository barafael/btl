use avian2d::prelude::{AngularVelocity, LinearVelocity};
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::client::{LocalShip, TorpedoInitialized};
use btl_protocol::{Fuel, ShipInput};
use btl_shared::SHIP_RADIUS;
use lightyear::prelude::input::native::{ActionState, InputMarker};

// Torpedo plume constants
const TORP_PLUME_RATE: f32 = 200.0;
const TORP_PLUME_LIFETIME: f32 = 0.25;
const TORP_PLUME_SPEED: f32 = 120.0;
const TORP_PLUME_SIZE_START: f32 = 2.5;
const TORP_PLUME_CONE: f32 = 0.25;
const TORP_COLOR_START: Vec3 = Vec3::new(0.9, 0.85, 0.5);

// Main thruster particle constants
const PARTICLE_LIFETIME: f32 = 0.12;
const PARTICLE_SPEED_MIN: f32 = 800.0;
const PARTICLE_SPEED_MAX: f32 = 1200.0;
const PARTICLE_SPAWN_RATE: f32 = 600.0;
const PARTICLE_SIZE_START: f32 = 1.2;
const PARTICLE_SIZE_END: f32 = 0.2;
const CONE_HALF_ANGLE: f32 = 0.10;

const VELOCITY_INHERIT: f32 = 0.3;

const EMBER_CHANCE: f32 = 0.08;
const EMBER_LIFETIME_MULT: f32 = 4.0;
const EMBER_SPEED_MULT: f32 = 0.3;
const EMBER_SIZE: f32 = 1.0;

// HDR color gradient
const COLOR_HOT: Vec3 = Vec3::new(4.0, 4.0, 5.0);
const COLOR_MID: Vec3 = Vec3::new(2.0, 1.2, 0.4);
const COLOR_COOL: Vec3 = Vec3::new(0.4, 0.3, 0.3);

const HALO_SIZE_MULT: f32 = 3.0;
const HALO_ALPHA_MULT: f32 = 0.25;
const HALO_CHANCE: f32 = 0.3;

// Afterburner multipliers
const AB_SPAWN_RATE_MULT: f32 = 2.5;
const AB_LIFETIME_MULT: f32 = 1.8;
const AB_SPEED_MULT: f32 = 1.3;
const AB_SIZE_MULT: f32 = 1.6;
const COLOR_AB_HOT: Vec3 = Vec3::new(5.0, 5.0, 8.0);
const COLOR_AB_MID: Vec3 = Vec3::new(3.0, 2.0, 4.0);
const COLOR_AB_COOL: Vec3 = Vec3::new(1.5, 0.5, 0.8);
const SPUTTER_THRESHOLD: f32 = 0.2;

// --- RCS Thruster Visuals ---
const RCS_CONE_LENGTH: f32 = 8.8;
const RCS_CONE_HALF_WIDTH: f32 = 1.0;
const MAIN_GLOW_SIZE: f32 = 4.0;
const RCS_GLOW_SIZE: f32 = 2.4;

// HDR colors for RCS flames (blue-to-white)
const RCS_CONE_COLOR: Vec3 = Vec3::new(2.0, 2.5, 5.0);
const RCS_GLOW_COLOR: Vec3 = Vec3::new(3.0, 3.5, 6.0);
const MAIN_GLOW_COLOR: Vec3 = Vec3::new(6.0, 6.0, 10.0);

const NOZZLE_COUNT: usize = 10;

#[derive(Component)]
struct Particle {
    velocity: Vec2,
    lifetime: f32,
    max_lifetime: f32,
    start_alpha: f32,
    is_halo: bool,
    start_size: f32,
    afterburner: bool,
    /// Whether to stretch the sprite along velocity direction.
    stretched: bool,
}

/// Flame cone mesh for an RCS nozzle (child of ship).
#[derive(Component)]
struct ThrusterCone(usize);

/// Glow sprite for a thruster nozzle (child of ship).
#[derive(Component)]
struct ThrusterGlow(usize);

#[derive(Resource, Deref, DerefMut)]
struct ParticleRng(btl_shared::rng::Rng);

struct NozzleDef {
    local_pos: Vec2,
    local_dir: Vec2,
}

fn nozzle_defs() -> [NozzleDef; NOZZLE_COUNT] {
    let r = SHIP_RADIUS;
    [
        // 0: main rear — particles + glow only (no cone)
        NozzleDef {
            local_pos: Vec2::new(0.0, -r),
            local_dir: Vec2::new(0.0, -1.0),
        },
        // 1: reverse (front)
        NozzleDef {
            local_pos: Vec2::new(0.0, r * 1.2),
            local_dir: Vec2::new(0.0, 1.0),
        },
        // 2: strafe-R front
        NozzleDef {
            local_pos: Vec2::new(r * 0.6, r * 0.5),
            local_dir: Vec2::new(1.0, 0.0),
        },
        // 3: strafe-R back
        NozzleDef {
            local_pos: Vec2::new(r * 0.6, -r * 0.5),
            local_dir: Vec2::new(1.0, 0.0),
        },
        // 4: strafe-L front
        NozzleDef {
            local_pos: Vec2::new(-r * 0.6, r * 0.5),
            local_dir: Vec2::new(-1.0, 0.0),
        },
        // 5: strafe-L back
        NozzleDef {
            local_pos: Vec2::new(-r * 0.6, -r * 0.5),
            local_dir: Vec2::new(-1.0, 0.0),
        },
        // 6: rotate-CW front (at right-front, exhaust left)
        NozzleDef {
            local_pos: Vec2::new(r * 0.6, r * 0.8),
            local_dir: Vec2::new(-1.0, 0.0),
        },
        // 7: rotate-CW back (at left-rear, exhaust right)
        NozzleDef {
            local_pos: Vec2::new(-r * 0.6, -r * 0.8),
            local_dir: Vec2::new(1.0, 0.0),
        },
        // 8: rotate-CCW front (at left-front, exhaust right)
        NozzleDef {
            local_pos: Vec2::new(-r * 0.6, r * 0.8),
            local_dir: Vec2::new(1.0, 0.0),
        },
        // 9: rotate-CCW back (at right-rear, exhaust left)
        NozzleDef {
            local_pos: Vec2::new(r * 0.6, -r * 0.8),
            local_dir: Vec2::new(-1.0, 0.0),
        },
    ]
}

fn create_flame_cone_mesh(length: f32, half_width: f32) -> Mesh {
    // Triangle pointing in +Y direction from origin (tip at nozzle, base extends outward)
    let positions = vec![
        [0.0, 0.0, 0.0],
        [-half_width, length, 0.0],
        [half_width, length, 0.0],
    ];
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U16(vec![0, 1, 2]));
    mesh
}

/// Spawn RCS flame cone meshes and glow sprites as children of the local ship.
pub fn spawn_thruster_nozzles(
    commands: &mut Commands,
    ship_entity: Entity,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
) {
    let defs = nozzle_defs();
    let cone_mesh = meshes.add(create_flame_cone_mesh(RCS_CONE_LENGTH, RCS_CONE_HALF_WIDTH));

    let cone_material = materials.add(ColorMaterial::from(Color::LinearRgba(
        LinearRgba::new(RCS_CONE_COLOR.x, RCS_CONE_COLOR.y, RCS_CONE_COLOR.z, 0.7),
    )));

    for (i, def) in defs.iter().enumerate() {
        // Rotation to align +Y with exhaust direction
        let rot_angle = f32::atan2(-def.local_dir.x, def.local_dir.y);

        if i == 0 {
            // Main thruster: only a glow sprite (particles handle the plume)
            commands.spawn((
                ChildOf(ship_entity),
                ThrusterGlow(i),
                Sprite {
                    color: Color::LinearRgba(LinearRgba::new(
                        MAIN_GLOW_COLOR.x,
                        MAIN_GLOW_COLOR.y,
                        MAIN_GLOW_COLOR.z,
                        0.6,
                    )),
                    custom_size: Some(Vec2::splat(MAIN_GLOW_SIZE)),
                    ..default()
                },
                Transform::from_xyz(def.local_pos.x, def.local_pos.y, -0.5),
                Visibility::Hidden,
            ));
            continue;
        }

        // RCS nozzles: cone mesh + glow sprite
        commands.spawn((
            ChildOf(ship_entity),
            ThrusterCone(i),
            Mesh2d(cone_mesh.clone()),
            MeshMaterial2d(cone_material.clone()),
            Transform::from_xyz(def.local_pos.x, def.local_pos.y, -0.5)
                .with_rotation(Quat::from_rotation_z(rot_angle)),
            Visibility::Hidden,
        ));

        commands.spawn((
            ChildOf(ship_entity),
            ThrusterGlow(i),
            Sprite {
                color: Color::LinearRgba(LinearRgba::new(
                    RCS_GLOW_COLOR.x,
                    RCS_GLOW_COLOR.y,
                    RCS_GLOW_COLOR.z,
                    0.5,
                )),
                custom_size: Some(Vec2::splat(RCS_GLOW_SIZE)),
                ..default()
            },
            Transform::from_xyz(def.local_pos.x, def.local_pos.y, -0.3),
            Visibility::Hidden,
        ));
    }
}

pub struct ParticlePlugin;

impl Plugin for ParticlePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ParticleRng(btl_shared::rng::Rng::new(
            0xDEAD_BEEF_CAFE_1234,
        )));
        app.add_systems(
            Update,
            (
                spawn_main_thruster_particles,
                spawn_torpedo_plume,
                update_particles,
                update_thruster_nozzles,
            ),
        );
    }
}

/// Spawn particles only for the main rear thruster.
/// RCS thrusters use mesh cones + glow sprites instead.
fn spawn_main_thruster_particles(
    mut commands: Commands,
    ship_query: Query<
        (
            &Transform,
            &ActionState<ShipInput>,
            &LinearVelocity,
            &Fuel,
        ),
        (With<LocalShip>, With<InputMarker<ShipInput>>),
    >,
    time: Res<Time>,
    mut rng: ResMut<ParticleRng>,
) {
    let Ok((transform, action_state, lin_vel, fuel)) = ship_query.single() else {
        return;
    };
    let input = &action_state.0;
    let dt = time.delta_secs();

    let afterburner_active = input.afterburner && fuel.current > 0.0;
    let fuel_frac = fuel.fraction();
    let sputtering = afterburner_active && fuel_frac < SPUTTER_THRESHOLD;

    let forward = transform.up().truncate();
    let ship_pos = transform.translation.truncate();
    let ship_vel = lin_vel.0;

    let fwd_mag = input.thrust_forward.abs();
    if fwd_mag < 0.05 {
        return;
    }

    let speed = lin_vel.0.length();
    let max_speed = if afterburner_active {
        btl_shared::SHIP_MAX_SPEED * 1.5
    } else {
        btl_shared::SHIP_MAX_SPEED
    };
    let at_max_speed = speed >= max_speed - 0.5;

    if at_max_speed && lin_vel.0.dot(forward) > 0.0 {
        return;
    }

    let base_rate = if afterburner_active {
        PARTICLE_SPAWN_RATE * AB_SPAWN_RATE_MULT
    } else {
        PARTICLE_SPAWN_RATE
    };
    let count = ((base_rate * dt * fwd_mag).max(1.0)) as usize;
    let alpha = if afterburner_active {
        0.95
    } else {
        0.7 * fwd_mag.max(0.3)
    };
    let is_ab = afterburner_active;

    let nozzle_pos = ship_pos - forward * SHIP_RADIUS;
    let exhaust_dir = -forward;

    if sputtering && rng.next_f32() < 0.5 {
        return;
    }

    for i in 0..count {
        let frac = if count > 1 {
            i as f32 / count as f32
        } else {
            rng.next_f32()
        };
        let time_offset = frac * dt;
        let pos_offset = nozzle_pos - ship_vel * time_offset;

        let angle = rng.next_signed() * CONE_HALF_ANGLE;
        let cos_a = angle.cos();
        let sin_a = angle.sin();
        let varied_dir = Vec2::new(
            exhaust_dir.x * cos_a - exhaust_dir.y * sin_a,
            exhaust_dir.x * sin_a + exhaust_dir.y * cos_a,
        );

        let is_ember = rng.next_f32() < EMBER_CHANCE;
        let is_halo = !is_ember && rng.next_f32() < HALO_CHANCE;

        let base_speed =
            PARTICLE_SPEED_MIN + rng.next_f32() * (PARTICLE_SPEED_MAX - PARTICLE_SPEED_MIN);
        let ab_speed = if is_ab { AB_SPEED_MULT } else { 1.0 };
        let speed = if is_ember {
            base_speed * EMBER_SPEED_MULT
        } else {
            base_speed * ab_speed
        };

        let jitter = rng.next_signed() * if is_halo { 3.0 } else { 1.5 };
        let perp = Vec2::new(-exhaust_dir.y, exhaust_dir.x);
        let spawn_pos = pos_offset + perp * jitter;

        let vel = varied_dir * speed + ship_vel * VELOCITY_INHERIT;

        let ab_life = if is_ab { AB_LIFETIME_MULT } else { 1.0 };
        let lifetime = if is_ember {
            PARTICLE_LIFETIME * EMBER_LIFETIME_MULT * ab_life * (0.8 + rng.next_f32() * 0.4)
        } else {
            PARTICLE_LIFETIME * ab_life * (0.8 + rng.next_f32() * 0.4)
        };

        let particle_alpha = if is_halo {
            alpha * HALO_ALPHA_MULT
        } else {
            alpha
        };

        let ab_size = if is_ab { AB_SIZE_MULT } else { 1.0 };
        let size = if is_ember {
            EMBER_SIZE * ab_size
        } else if is_halo {
            PARTICLE_SIZE_START * HALO_SIZE_MULT * ab_size * (0.8 + rng.next_f32() * 0.4)
        } else {
            PARTICLE_SIZE_START * ab_size * (0.6 + rng.next_f32() * 0.8)
        };

        let hot = if is_ab { COLOR_AB_HOT } else { COLOR_HOT };
        let color = Color::LinearRgba(LinearRgba::new(hot.x, hot.y, hot.z, particle_alpha));

        commands.spawn((
            Particle {
                velocity: vel,
                lifetime: lifetime - time_offset,
                max_lifetime: lifetime,
                start_alpha: particle_alpha,
                is_halo,
                start_size: size,
                afterburner: is_ab,
                stretched: !is_ember && !is_halo,
            },
            Sprite {
                color,
                custom_size: Some(Vec2::splat(size)),
                ..default()
            },
            Transform::from_xyz(
                spawn_pos.x + vel.x * time_offset,
                spawn_pos.y + vel.y * time_offset,
                if is_halo { -2.0 } else { -1.0 },
            ),
        ));
    }
}

/// Spawn exhaust plume particles behind torpedoes.
fn spawn_torpedo_plume(
    mut commands: Commands,
    torpedoes: Query<(&Transform, &LinearVelocity), With<TorpedoInitialized>>,
    time: Res<Time>,
    mut rng: ResMut<ParticleRng>,
) {
    let dt = time.delta_secs();
    let count_per_frame = (TORP_PLUME_RATE * dt).ceil() as usize;

    for (tf, vel) in torpedoes.iter() {
        let torp_pos = tf.translation.truncate();
        let speed = vel.0.length();
        if speed < 1.0 {
            continue;
        }
        let forward = vel.0 / speed;
        let exhaust_dir = -forward;
        let exhaust_pos = torp_pos + exhaust_dir * 6.0;

        for _ in 0..count_per_frame {
            let angle = rng.next_signed() * TORP_PLUME_CONE;
            let cos_a = angle.cos();
            let sin_a = angle.sin();
            let dir = Vec2::new(
                exhaust_dir.x * cos_a - exhaust_dir.y * sin_a,
                exhaust_dir.x * sin_a + exhaust_dir.y * cos_a,
            );

            let particle_speed = TORP_PLUME_SPEED * (0.6 + rng.next_f32() * 0.8);
            let particle_vel = dir * particle_speed + vel.0 * 0.3;

            let lifetime = TORP_PLUME_LIFETIME * (0.7 + rng.next_f32() * 0.6);
            let size = TORP_PLUME_SIZE_START * (0.6 + rng.next_f32() * 0.8);

            let jitter = rng.next_signed() * 2.0;
            let perp = Vec2::new(-exhaust_dir.y, exhaust_dir.x);
            let spawn_pos = exhaust_pos + perp * jitter;

            let color = Color::LinearRgba(LinearRgba::new(
                TORP_COLOR_START.x,
                TORP_COLOR_START.y,
                TORP_COLOR_START.z,
                0.8,
            ));

            commands.spawn((
                Particle {
                    velocity: particle_vel,
                    lifetime,
                    max_lifetime: lifetime,
                    start_alpha: 0.8,
                    is_halo: false,
                    start_size: size,
                    afterburner: false,
                    stretched: false,
                },
                Sprite {
                    color,
                    custom_size: Some(Vec2::splat(size)),
                    ..default()
                },
                Transform::from_xyz(spawn_pos.x, spawn_pos.y, 4.5),
            ));
        }
    }
}

fn update_particles(
    mut commands: Commands,
    mut query: Query<(Entity, &mut Particle, &mut Transform, &mut Sprite)>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    for (entity, mut particle, mut transform, mut sprite) in query.iter_mut() {
        particle.lifetime -= dt;
        if particle.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }

        transform.translation.x += particle.velocity.x * dt;
        transform.translation.y += particle.velocity.y * dt;

        let t = (particle.lifetime / particle.max_lifetime).clamp(0.0, 1.0);

        let (c_hot, c_mid, c_cool) = if particle.afterburner {
            (COLOR_AB_HOT, COLOR_AB_MID, COLOR_AB_COOL)
        } else {
            (COLOR_HOT, COLOR_MID, COLOR_COOL)
        };
        let color_rgb = if t > 0.5 {
            let blend = (1.0 - t) * 2.0;
            c_hot.lerp(c_mid, blend)
        } else {
            let blend = (0.5 - t) * 2.0;
            c_mid.lerp(c_cool, blend)
        };

        let alpha = particle.start_alpha * t * t;

        sprite.color = Color::LinearRgba(LinearRgba::new(
            color_rgb.x,
            color_rgb.y,
            color_rgb.z,
            alpha,
        ));

        let end_size = if particle.is_halo {
            PARTICLE_SIZE_END * HALO_SIZE_MULT
        } else {
            PARTICLE_SIZE_END
        };
        let size = particle.start_size * t + end_size * (1.0 - t);

        // Stretch main thruster particles along velocity direction
        if particle.stretched {
            let vel_speed = particle.velocity.length();
            if vel_speed > 50.0 {
                let stretch = (vel_speed / 400.0).clamp(1.5, 4.0);
                let vel_angle =
                    f32::atan2(-particle.velocity.x, particle.velocity.y);
                transform.rotation = Quat::from_rotation_z(vel_angle);
                sprite.custom_size = Some(Vec2::new(size * 0.4, size * stretch));
            } else {
                sprite.custom_size = Some(Vec2::splat(size));
            }
        } else {
            sprite.custom_size = Some(Vec2::splat(size));
        }
    }
}

/// Update RCS cone meshes and glow sprites based on ship input.
fn update_thruster_nozzles(
    ship_query: Query<
        (
            &ActionState<ShipInput>,
            &LinearVelocity,
            &AngularVelocity,
            &Transform,
            &Fuel,
        ),
        With<LocalShip>,
    >,
    mut cones: Query<
        (&ThrusterCone, &mut Transform, &mut Visibility),
        (Without<LocalShip>, Without<ThrusterGlow>),
    >,
    mut glows: Query<
        (&ThrusterGlow, &mut Visibility, &mut Sprite),
        (Without<LocalShip>, Without<ThrusterCone>),
    >,
) {
    let Ok((action_state, lin_vel, ang_vel, ship_tf, fuel)) = ship_query.single() else {
        return;
    };
    let input = &action_state.0;
    let forward = ship_tf.up().truncate();
    let right = ship_tf.right().truncate();

    let afterburner_active = input.afterburner && fuel.current > 0.0;

    // Compute activation [0..1] per nozzle
    let mut a = [0.0f32; NOZZLE_COUNT];

    // Main thruster (0)
    if input.thrust_forward.abs() > 0.05 {
        a[0] = input.thrust_forward.abs();
        if afterburner_active {
            a[0] = 1.0;
        }
    }

    // Reverse (1)
    if input.thrust_backward.abs() > 0.05 {
        a[1] = input.thrust_backward.abs();
    }

    // Strafe
    if input.strafe > 0.1 {
        a[2] = input.strafe;
        a[3] = input.strafe;
    }
    if input.strafe < -0.1 {
        a[4] = -input.strafe;
        a[5] = -input.strafe;
    }

    // Rotate
    let ang = ang_vel.0;
    let at_max_spin = ang.abs() >= btl_shared::SHIP_MAX_ANGULAR_SPEED - 0.1;

    if input.rotate > 0.1 && !(at_max_spin && ang > 0.0) {
        a[6] = input.rotate;
        a[7] = input.rotate;
    }
    if input.rotate < -0.1 && !(at_max_spin && ang < 0.0) {
        a[8] = -input.rotate;
        a[9] = -input.rotate;
    }

    // Stabilize: overlay on same physical nozzles
    if input.stabilize > 0.05 {
        let s = input.stabilize;
        let vel = lin_vel.0;
        let fwd_comp = vel.dot(forward);
        let right_comp = vel.dot(right);

        if fwd_comp > 0.5 {
            a[1] = a[1].max(s);
        }
        if fwd_comp < -0.5 {
            a[0] = a[0].max(s);
        }
        if right_comp > 0.5 {
            a[2] = a[2].max(s);
            a[3] = a[3].max(s);
        }
        if right_comp < -0.5 {
            a[4] = a[4].max(s);
            a[5] = a[5].max(s);
        }
        if ang > 0.05 {
            a[8] = a[8].max(s);
            a[9] = a[9].max(s);
        }
        if ang < -0.05 {
            a[6] = a[6].max(s);
            a[7] = a[7].max(s);
        }
    }

    // Update cone meshes (indices 1..9 only; 0 = main thruster has no cone)
    for (cone, mut tf, mut vis) in cones.iter_mut() {
        let activation = a[cone.0].clamp(0.0, 1.0);
        if activation < 0.01 {
            *vis = Visibility::Hidden;
        } else {
            *vis = Visibility::Inherited;
            // Scale length (Y) by activation; width (X) scales slightly too
            tf.scale = Vec3::new(0.6 + activation * 0.4, activation, 1.0);
        }
    }

    // Update glow sprites
    for (glow, mut vis, mut sprite) in glows.iter_mut() {
        let activation = a[glow.0].clamp(0.0, 1.0);
        if activation < 0.01 {
            *vis = Visibility::Hidden;
        } else {
            *vis = Visibility::Inherited;
            let (base_color, base_alpha, base_size) = if glow.0 == 0 {
                (MAIN_GLOW_COLOR, 0.6, MAIN_GLOW_SIZE)
            } else {
                (RCS_GLOW_COLOR, 0.5, RCS_GLOW_SIZE)
            };
            let glow_size = base_size * (0.5 + activation * 0.5);
            sprite.custom_size = Some(Vec2::splat(glow_size));
            sprite.color = Color::LinearRgba(LinearRgba::new(
                base_color.x * activation,
                base_color.y * activation,
                base_color.z * activation,
                base_alpha * activation,
            ));
        }
    }
}
