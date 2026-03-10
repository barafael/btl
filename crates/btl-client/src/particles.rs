use avian2d::prelude::{AngularVelocity, LinearVelocity};
use bevy::prelude::*;

use crate::client::LocalShip;
use btl_protocol::{Fuel, ShipInput};
use btl_shared::SHIP_RADIUS;
use lightyear::prelude::input::native::{ActionState, InputMarker};

const PARTICLE_LIFETIME: f32 = 0.12;
const PARTICLE_SPEED_MIN: f32 = 800.0;
const PARTICLE_SPEED_MAX: f32 = 1200.0;
const PARTICLE_SPAWN_RATE: f32 = 600.0;
const PARTICLE_SIZE_START: f32 = 1.2;
const PARTICLE_SIZE_END: f32 = 0.2;
// Half-angle of the exhaust cone in radians (~6 degrees)
const CONE_HALF_ANGLE: f32 = 0.10;

// Velocity inheritance: particles pick up a fraction of the ship's velocity
const VELOCITY_INHERIT: f32 = 0.3;

// Ember/spark particles: longer-lived, slower, less frequent
const EMBER_CHANCE: f32 = 0.08;
const EMBER_LIFETIME_MULT: f32 = 4.0;
const EMBER_SPEED_MULT: f32 = 0.3;
const EMBER_SIZE: f32 = 1.0;

// Color gradient: hot core → cool tail
// HDR values > 1.0 will bloom
const COLOR_HOT: Vec3 = Vec3::new(4.0, 4.0, 5.0); // bright blue-white (HDR)
const COLOR_MID: Vec3 = Vec3::new(2.0, 1.2, 0.4); // orange
const COLOR_COOL: Vec3 = Vec3::new(0.4, 0.3, 0.3); // dim gray

// Halo layer: wider, dimmer particles behind the core beam
const HALO_SIZE_MULT: f32 = 3.0;
const HALO_ALPHA_MULT: f32 = 0.25;
const HALO_CHANCE: f32 = 0.3;

// Afterburner boost multipliers
const AB_SPAWN_RATE_MULT: f32 = 2.5;
const AB_LIFETIME_MULT: f32 = 1.8;
const AB_SPEED_MULT: f32 = 1.3;
const AB_SIZE_MULT: f32 = 1.6;
// Afterburner color: intense blue-white HDR
const COLOR_AB_HOT: Vec3 = Vec3::new(5.0, 5.0, 8.0); // blue-white blaze
const COLOR_AB_MID: Vec3 = Vec3::new(3.0, 2.0, 4.0); // violet-blue
const COLOR_AB_COOL: Vec3 = Vec3::new(1.5, 0.5, 0.8); // fading purple
// Fuel sputter threshold (fraction)
const SPUTTER_THRESHOLD: f32 = 0.2;

#[derive(Component)]
struct Particle {
    velocity: Vec2,
    lifetime: f32,
    max_lifetime: f32,
    start_alpha: f32,
    is_halo: bool,
    start_size: f32,
    afterburner: bool,
}

#[derive(Resource, Deref, DerefMut)]
struct ParticleRng(btl_shared::rng::Rng);

pub struct ParticlePlugin;

impl Plugin for ParticlePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ParticleRng(btl_shared::rng::Rng::new(0xDEAD_BEEF_CAFE_1234)));
        app.add_systems(Update, (spawn_thruster_particles, update_particles));
    }
}

fn spawn_thruster_particles(
    mut commands: Commands,
    ship_query: Query<
        (
            &Transform,
            &ActionState<ShipInput>,
            &LinearVelocity,
            &AngularVelocity,
            &Fuel,
        ),
        (With<LocalShip>, With<InputMarker<ShipInput>>),
    >,
    time: Res<Time>,
    mut rng: ResMut<ParticleRng>,
) {
    let Ok((transform, action_state, lin_vel, ang_vel, fuel)) = ship_query.single() else {
        return;
    };
    let input = &action_state.0;
    let dt = time.delta_secs();

    let afterburner_active = input.afterburner && fuel.current > 0.0;
    // Fuel sputter: when fuel is low, randomly skip particle spawns
    let fuel_frac = fuel.fraction();
    let sputtering = afterburner_active && fuel_frac < SPUTTER_THRESHOLD;

    let forward = transform.up().truncate();
    let right = transform.right().truncate();
    let ship_pos = transform.translation.truncate();

    // Ship velocity for interpolating spawn positions across the frame
    let ship_vel = lin_vel.0;

    let mut spawn_cone =
        |pos: Vec2, dir: Vec2, alpha: f32, spread: f32, count: usize, is_ab: bool| {
            // Sputtering: skip entire bursts randomly when fuel is critically low
            if sputtering && rng.next_f32() < 0.5 {
                return;
            }
            for i in 0..count {
                // Distribute spawn times evenly across the frame to avoid clumping
                let frac = if count > 1 {
                    i as f32 / count as f32
                } else {
                    rng.next_f32()
                };
                // Offset spawn position back along ship's trajectory
                let time_offset = frac * dt;
                let pos_offset = pos - ship_vel * time_offset;

                // Random angle within cone
                let angle = rng.next_signed() * spread;
                let cos_a = angle.cos();
                let sin_a = angle.sin();
                let varied_dir =
                    Vec2::new(dir.x * cos_a - dir.y * sin_a, dir.x * sin_a + dir.y * cos_a);

                // Decide if this is an ember (slow, long-lived spark)
                let is_ember = rng.next_f32() < EMBER_CHANCE;
                // Decide if this is a halo particle (wider, dimmer)
                let is_halo = !is_ember && rng.next_f32() < HALO_CHANCE;

                let base_speed =
                    PARTICLE_SPEED_MIN + rng.next_f32() * (PARTICLE_SPEED_MAX - PARTICLE_SPEED_MIN);
                let ab_speed = if is_ab { AB_SPEED_MULT } else { 1.0 };
                let speed = if is_ember {
                    base_speed * EMBER_SPEED_MULT
                } else {
                    base_speed * ab_speed
                };

                // Slight positional jitter at nozzle
                let jitter = rng.next_signed() * if is_halo { 3.0 } else { 1.5 };
                let perp = Vec2::new(-dir.y, dir.x);
                let spawn_pos = pos_offset + perp * jitter;

                // Velocity: particle direction + inherited ship velocity
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

                // Size variation: randomize start size
                let ab_size = if is_ab { AB_SIZE_MULT } else { 1.0 };
                let size = if is_ember {
                    EMBER_SIZE * ab_size
                } else if is_halo {
                    PARTICLE_SIZE_START * HALO_SIZE_MULT * ab_size * (0.8 + rng.next_f32() * 0.4)
                } else {
                    PARTICLE_SIZE_START * ab_size * (0.6 + rng.next_f32() * 0.8)
                };

                // Initial color: hot HDR white-blue at birth (afterburner is more intense blue)
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
        };

    let base_rate = if afterburner_active {
        PARTICLE_SPAWN_RATE * AB_SPAWN_RATE_MULT
    } else {
        PARTICLE_SPAWN_RATE
    };
    let count = (base_rate * dt).max(1.0) as usize;
    let speed = lin_vel.0.length();
    let max_speed = if afterburner_active {
        btl_shared::SHIP_MAX_SPEED * 1.5
    } else {
        btl_shared::SHIP_MAX_SPEED
    };
    let at_max_speed = speed >= max_speed - 0.5;

    // Main thruster (rear) — scale particles by throttle
    let fwd_mag = input.thrust_forward.abs();
    if fwd_mag > 0.05 && !(at_max_speed && lin_vel.0.dot(forward) > 0.0) {
        let scaled = (count as f32 * fwd_mag).max(1.0) as usize;
        let alpha = if afterburner_active {
            0.95
        } else {
            0.7 * fwd_mag.max(0.3)
        };
        let base = ship_pos - forward * SHIP_RADIUS;
        spawn_cone(
            base,
            -forward,
            alpha,
            CONE_HALF_ANGLE,
            scaled,
            afterburner_active,
        );
    }

    // Reverse thruster (front)
    let bwd_mag = input.thrust_backward.abs();
    if bwd_mag > 0.05 && !(at_max_speed && lin_vel.0.dot(-forward) > 0.0) {
        let scaled = (count as f32 * 0.5 * bwd_mag).max(1.0) as usize;
        let base = ship_pos + forward * SHIP_RADIUS * 1.2;
        spawn_cone(
            base,
            forward,
            0.5 * bwd_mag.max(0.3),
            CONE_HALF_ANGLE * 1.5,
            scaled,
            false,
        );
    }

    let rcs_count = (count / 5).max(1);
    let ang = ang_vel.0;
    let at_max_spin = ang.abs() >= btl_shared::SHIP_MAX_ANGULAR_SPEED - 0.1;

    // Strafe thrusters — scale by magnitude, deadzone at 0.1
    let strafe_mag = input.strafe.abs();
    if strafe_mag > 0.1 {
        let scaled = (count as f32 / 3.0 * strafe_mag).max(1.0) as usize;
        let alpha = 0.5 * strafe_mag.max(0.3);
        if input.strafe > 0.0 && !(at_max_speed && lin_vel.0.dot(-right) > 0.0) {
            let fr = ship_pos + forward * SHIP_RADIUS * 0.5 + right * SHIP_RADIUS * 0.6;
            let br = ship_pos - forward * SHIP_RADIUS * 0.5 + right * SHIP_RADIUS * 0.6;
            spawn_cone(fr, right, alpha, CONE_HALF_ANGLE * 1.2, scaled, false);
            spawn_cone(br, right, alpha, CONE_HALF_ANGLE * 1.2, scaled, false);
        }
        if input.strafe < 0.0 && !(at_max_speed && lin_vel.0.dot(right) > 0.0) {
            let fl = ship_pos + forward * SHIP_RADIUS * 0.5 - right * SHIP_RADIUS * 0.6;
            let bl = ship_pos - forward * SHIP_RADIUS * 0.5 - right * SHIP_RADIUS * 0.6;
            spawn_cone(fl, -right, alpha, CONE_HALF_ANGLE * 1.2, scaled, false);
            spawn_cone(bl, -right, alpha, CONE_HALF_ANGLE * 1.2, scaled, false);
        }
    }

    // Rotation thrusters — scale by magnitude, deadzone at 0.1
    let rot_mag = input.rotate.abs();
    if rot_mag > 0.1 {
        let scaled = (rcs_count as f32 * rot_mag).max(1.0) as usize;
        let alpha = 0.35 * rot_mag.max(0.3);
        if input.rotate > 0.0 && !(at_max_spin && ang > 0.0) {
            let rf = ship_pos + forward * SHIP_RADIUS * 0.8 + right * SHIP_RADIUS * 0.6;
            let lr = ship_pos - forward * SHIP_RADIUS * 0.8 - right * SHIP_RADIUS * 0.6;
            spawn_cone(rf, -right, alpha, CONE_HALF_ANGLE * 1.2, scaled, false);
            spawn_cone(lr, right, alpha, CONE_HALF_ANGLE * 1.2, scaled, false);
        }
        if input.rotate < 0.0 && !(at_max_spin && ang < 0.0) {
            let lf = ship_pos + forward * SHIP_RADIUS * 0.8 - right * SHIP_RADIUS * 0.6;
            let rr = ship_pos - forward * SHIP_RADIUS * 0.8 + right * SHIP_RADIUS * 0.6;
            spawn_cone(lf, right, alpha, CONE_HALF_ANGLE * 1.2, scaled, false);
            spawn_cone(rr, -right, alpha, CONE_HALF_ANGLE * 1.2, scaled, false);
        }
    }

    // Stabilize: scale by magnitude
    let stab_mag = input.stabilize;
    if stab_mag > 0.05 {
        let stab_count = (rcs_count as f32 * stab_mag).max(1.0) as usize;
        let alpha = 0.35 * stab_mag.max(0.3);
        let vel = lin_vel.0;
        let fwd_component = vel.dot(forward);
        let right_component = vel.dot(right);

        if fwd_component > 0.5 {
            let base = ship_pos + forward * SHIP_RADIUS * 1.2;
            spawn_cone(
                base,
                forward,
                alpha,
                CONE_HALF_ANGLE * 1.5,
                stab_count,
                false,
            );
        }
        if fwd_component < -0.5 {
            let base = ship_pos - forward * SHIP_RADIUS;
            spawn_cone(
                base,
                -forward,
                alpha,
                CONE_HALF_ANGLE * 1.5,
                stab_count,
                false,
            );
        }
        if right_component > 0.5 {
            let fr = ship_pos + forward * SHIP_RADIUS * 0.5 + right * SHIP_RADIUS * 0.6;
            let br = ship_pos - forward * SHIP_RADIUS * 0.5 + right * SHIP_RADIUS * 0.6;
            spawn_cone(fr, right, alpha, CONE_HALF_ANGLE * 1.2, stab_count, false);
            spawn_cone(br, right, alpha, CONE_HALF_ANGLE * 1.2, stab_count, false);
        }
        if right_component < -0.5 {
            let fl = ship_pos + forward * SHIP_RADIUS * 0.5 - right * SHIP_RADIUS * 0.6;
            let bl = ship_pos - forward * SHIP_RADIUS * 0.5 - right * SHIP_RADIUS * 0.6;
            spawn_cone(fl, -right, alpha, CONE_HALF_ANGLE * 1.2, stab_count, false);
            spawn_cone(bl, -right, alpha, CONE_HALF_ANGLE * 1.2, stab_count, false);
        }

        let ang = ang_vel.0;
        if ang.abs() > 0.05 {
            if ang > 0.0 {
                let lf = ship_pos + forward * SHIP_RADIUS * 0.8 - right * SHIP_RADIUS * 0.6;
                let rr = ship_pos - forward * SHIP_RADIUS * 0.8 + right * SHIP_RADIUS * 0.6;
                spawn_cone(lf, right, alpha, CONE_HALF_ANGLE * 1.2, stab_count, false);
                spawn_cone(rr, -right, alpha, CONE_HALF_ANGLE * 1.2, stab_count, false);
            } else {
                let rf = ship_pos + forward * SHIP_RADIUS * 0.8 + right * SHIP_RADIUS * 0.6;
                let lr = ship_pos - forward * SHIP_RADIUS * 0.8 - right * SHIP_RADIUS * 0.6;
                spawn_cone(rf, -right, alpha, CONE_HALF_ANGLE * 1.2, stab_count, false);
                spawn_cone(lr, right, alpha, CONE_HALF_ANGLE * 1.2, stab_count, false);
            }
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

        // t goes from 1 (birth) to 0 (death)
        let t = (particle.lifetime / particle.max_lifetime).clamp(0.0, 1.0);

        // Color gradient: hot → mid → cool (afterburner uses blue-shifted palette)
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

        // Alpha: quadratic drop-off for focused beam look
        let alpha = particle.start_alpha * t * t;

        sprite.color = Color::LinearRgba(LinearRgba::new(
            color_rgb.x,
            color_rgb.y,
            color_rgb.z,
            alpha,
        ));

        // Particles shrink from start to end size as they travel
        let end_size = if particle.is_halo {
            PARTICLE_SIZE_END * HALO_SIZE_MULT
        } else {
            PARTICLE_SIZE_END
        };
        let size = particle.start_size * t + end_size * (1.0 - t);
        sprite.custom_size = Some(Vec2::splat(size));
    }
}
