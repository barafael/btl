use avian2d::prelude::{AngularVelocity, LinearVelocity};
use bevy::prelude::*;

use crate::client::LocalShip;
use btl_protocol::ShipInput;
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

#[derive(Component)]
struct Particle {
    velocity: Vec2,
    lifetime: f32,
    max_lifetime: f32,
    start_alpha: f32,
    is_halo: bool,
    start_size: f32,
}

/// Simple fast hash for pseudo-random particle variation
#[derive(Resource)]
struct ParticleRng(u64);

impl ParticleRng {
    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 16) as u32
    }

    /// Returns value in 0..1
    fn next_f32(&mut self) -> f32 {
        (self.next_u32() & 0x00FF_FFFF) as f32 / 16777216.0
    }

    /// Returns value in -1..1
    fn next_signed(&mut self) -> f32 {
        self.next_f32() * 2.0 - 1.0
    }
}

pub struct ParticlePlugin;

impl Plugin for ParticlePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ParticleRng(0xDEAD_BEEF_CAFE_1234));
        app.add_systems(Update, (spawn_thruster_particles, update_particles));
    }
}

fn spawn_thruster_particles(
    mut commands: Commands,
    ship_query: Query<
        (&Transform, &ActionState<ShipInput>, &LinearVelocity, &AngularVelocity),
        (With<LocalShip>, With<InputMarker<ShipInput>>),
    >,
    time: Res<Time>,
    mut rng: ResMut<ParticleRng>,
) {
    let Ok((transform, action_state, lin_vel, ang_vel)) = ship_query.single() else {
        return;
    };
    let input = &action_state.0;
    let dt = time.delta_secs();

    let forward = transform.up().truncate();
    let right = transform.right().truncate();
    let ship_pos = transform.translation.truncate();

    // Ship velocity for interpolating spawn positions across the frame
    let ship_vel = lin_vel.0;

    let mut spawn_cone = |pos: Vec2, dir: Vec2, alpha: f32, spread: f32, count: usize| {
        for i in 0..count {
            // Distribute spawn times evenly across the frame to avoid clumping
            let frac = if count > 1 { i as f32 / count as f32 } else { rng.next_f32() };
            // Offset spawn position back along ship's trajectory
            let time_offset = frac * dt;
            let pos_offset = pos - ship_vel * time_offset;

            // Random angle within cone
            let angle = rng.next_signed() * spread;
            let cos_a = angle.cos();
            let sin_a = angle.sin();
            let varied_dir = Vec2::new(
                dir.x * cos_a - dir.y * sin_a,
                dir.x * sin_a + dir.y * cos_a,
            );

            // Decide if this is an ember (slow, long-lived spark)
            let is_ember = rng.next_f32() < EMBER_CHANCE;
            // Decide if this is a halo particle (wider, dimmer)
            let is_halo = !is_ember && rng.next_f32() < HALO_CHANCE;

            let base_speed = PARTICLE_SPEED_MIN + rng.next_f32() * (PARTICLE_SPEED_MAX - PARTICLE_SPEED_MIN);
            let speed = if is_ember { base_speed * EMBER_SPEED_MULT } else { base_speed };

            // Slight positional jitter at nozzle
            let jitter = rng.next_signed() * if is_halo { 3.0 } else { 1.5 };
            let perp = Vec2::new(-dir.y, dir.x);
            let spawn_pos = pos_offset + perp * jitter;

            // Velocity: particle direction + inherited ship velocity
            let vel = varied_dir * speed + ship_vel * VELOCITY_INHERIT;

            let lifetime = if is_ember {
                PARTICLE_LIFETIME * EMBER_LIFETIME_MULT * (0.8 + rng.next_f32() * 0.4)
            } else {
                PARTICLE_LIFETIME * (0.8 + rng.next_f32() * 0.4)
            };

            let particle_alpha = if is_halo { alpha * HALO_ALPHA_MULT } else { alpha };

            // Size variation: randomize start size
            let size = if is_ember {
                EMBER_SIZE
            } else if is_halo {
                PARTICLE_SIZE_START * HALO_SIZE_MULT * (0.8 + rng.next_f32() * 0.4)
            } else {
                PARTICLE_SIZE_START * (0.6 + rng.next_f32() * 0.8)
            };

            // Initial color: hot HDR white-blue at birth
            let color = Color::LinearRgba(LinearRgba::new(
                COLOR_HOT.x, COLOR_HOT.y, COLOR_HOT.z, particle_alpha,
            ));

            commands.spawn((
                Particle {
                    velocity: vel,
                    lifetime: lifetime - time_offset,
                    max_lifetime: lifetime,
                    start_alpha: particle_alpha,
                    is_halo,
                    start_size: size,
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

    let count = (PARTICLE_SPAWN_RATE * dt).max(1.0) as usize;
    let speed = lin_vel.0.length();
    let max_speed = if input.afterburner { btl_shared::SHIP_MAX_SPEED * 1.5 } else { btl_shared::SHIP_MAX_SPEED };
    let at_max_speed = speed >= max_speed - 0.5;

    // Main thruster (rear) — only fire if thrust would still accelerate
    if input.thrust_forward && !(at_max_speed && lin_vel.0.dot(forward) > 0.0) {
        let alpha = if input.afterburner { 0.9 } else { 0.7 };
        let base = ship_pos - forward * SHIP_RADIUS;
        spawn_cone(base, -forward, alpha, CONE_HALF_ANGLE, count);
    }

    // Reverse thruster (front) — only fire if thrust would still decelerate
    if input.thrust_backward && !(at_max_speed && lin_vel.0.dot(-forward) > 0.0) {
        let base = ship_pos + forward * SHIP_RADIUS * 1.2;
        spawn_cone(base, forward, 0.5, CONE_HALF_ANGLE * 1.5, (count / 2).max(1));
    }

    let rcs_count = (count / 5).max(1);
    let ang = ang_vel.0 as f32;
    let at_max_spin = ang.abs() >= btl_shared::SHIP_MAX_ANGULAR_SPEED - 0.1;

    // Strafe thrusters (side jets firing in unison)
    let strafe_count = (count / 3).max(1);
    if input.strafe_left && !(at_max_speed && lin_vel.0.dot(-right) > 0.0) {
        let fr = ship_pos + forward * SHIP_RADIUS * 0.5 + right * SHIP_RADIUS * 0.6;
        let br = ship_pos - forward * SHIP_RADIUS * 0.5 + right * SHIP_RADIUS * 0.6;
        spawn_cone(fr, right, 0.5, CONE_HALF_ANGLE * 1.2, strafe_count);
        spawn_cone(br, right, 0.5, CONE_HALF_ANGLE * 1.2, strafe_count);
    }
    if input.strafe_right && !(at_max_speed && lin_vel.0.dot(right) > 0.0) {
        let fl = ship_pos + forward * SHIP_RADIUS * 0.5 - right * SHIP_RADIUS * 0.6;
        let bl = ship_pos - forward * SHIP_RADIUS * 0.5 - right * SHIP_RADIUS * 0.6;
        spawn_cone(fl, -right, 0.5, CONE_HALF_ANGLE * 1.2, strafe_count);
        spawn_cone(bl, -right, 0.5, CONE_HALF_ANGLE * 1.2, strafe_count);
    }

    // Rotation thrusters (side jets) — only fire while still accelerating
    if input.rotate_left && !(at_max_spin && ang > 0.0) {
        let rf = ship_pos + forward * SHIP_RADIUS * 0.8 + right * SHIP_RADIUS * 0.6;
        let lr = ship_pos - forward * SHIP_RADIUS * 0.8 - right * SHIP_RADIUS * 0.6;
        spawn_cone(rf, -right, 0.35, CONE_HALF_ANGLE * 1.2, rcs_count);
        spawn_cone(lr, right, 0.35, CONE_HALF_ANGLE * 1.2, rcs_count);
    }
    if input.rotate_right && !(at_max_spin && ang < 0.0) {
        let lf = ship_pos + forward * SHIP_RADIUS * 0.8 - right * SHIP_RADIUS * 0.6;
        let rr = ship_pos - forward * SHIP_RADIUS * 0.8 + right * SHIP_RADIUS * 0.6;
        spawn_cone(lf, right, 0.35, CONE_HALF_ANGLE * 1.2, rcs_count);
        spawn_cone(rr, -right, 0.35, CONE_HALF_ANGLE * 1.2, rcs_count);
    }

    // Stabilize: fire fixed thrusters that oppose current velocity components
    if input.stabilize {
        let vel = lin_vel.0;
        let fwd_component = vel.dot(forward);
        let right_component = vel.dot(right);

        // Moving forward → fire front (reverse) thruster
        if fwd_component > 0.5 {
            let base = ship_pos + forward * SHIP_RADIUS * 1.2;
            spawn_cone(base, forward, 0.35, CONE_HALF_ANGLE * 1.5, rcs_count);
        }
        // Moving backward → fire rear (main) thruster
        if fwd_component < -0.5 {
            let base = ship_pos - forward * SHIP_RADIUS;
            spawn_cone(base, -forward, 0.35, CONE_HALF_ANGLE * 1.5, rcs_count);
        }
        // Moving right → fire right-side jets (push left)
        if right_component > 0.5 {
            let fr = ship_pos + forward * SHIP_RADIUS * 0.5 + right * SHIP_RADIUS * 0.6;
            let br = ship_pos - forward * SHIP_RADIUS * 0.5 + right * SHIP_RADIUS * 0.6;
            spawn_cone(fr, right, 0.35, CONE_HALF_ANGLE * 1.2, rcs_count);
            spawn_cone(br, right, 0.35, CONE_HALF_ANGLE * 1.2, rcs_count);
        }
        // Moving left → fire left-side jets (push right)
        if right_component < -0.5 {
            let fl = ship_pos + forward * SHIP_RADIUS * 0.5 - right * SHIP_RADIUS * 0.6;
            let bl = ship_pos - forward * SHIP_RADIUS * 0.5 - right * SHIP_RADIUS * 0.6;
            spawn_cone(fl, -right, 0.35, CONE_HALF_ANGLE * 1.2, rcs_count);
            spawn_cone(bl, -right, 0.35, CONE_HALF_ANGLE * 1.2, rcs_count);
        }

        // Angular stabilize
        let ang = ang_vel.0 as f32;
        if ang.abs() > 0.05 {
            if ang > 0.0 {
                let lf = ship_pos + forward * SHIP_RADIUS * 0.8 - right * SHIP_RADIUS * 0.6;
                let rr = ship_pos - forward * SHIP_RADIUS * 0.8 + right * SHIP_RADIUS * 0.6;
                spawn_cone(lf, right, 0.35, CONE_HALF_ANGLE * 1.2, rcs_count);
                spawn_cone(rr, -right, 0.35, CONE_HALF_ANGLE * 1.2, rcs_count);
            } else {
                let rf = ship_pos + forward * SHIP_RADIUS * 0.8 + right * SHIP_RADIUS * 0.6;
                let lr = ship_pos - forward * SHIP_RADIUS * 0.8 - right * SHIP_RADIUS * 0.6;
                spawn_cone(rf, -right, 0.35, CONE_HALF_ANGLE * 1.2, rcs_count);
                spawn_cone(lr, right, 0.35, CONE_HALF_ANGLE * 1.2, rcs_count);
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

        // Color gradient: hot → mid → cool
        let color_rgb = if t > 0.5 {
            // Hot to mid (first half of life)
            let blend = (1.0 - t) * 2.0; // 0 at birth, 1 at midlife
            COLOR_HOT.lerp(COLOR_MID, blend)
        } else {
            // Mid to cool (second half of life)
            let blend = (0.5 - t) * 2.0; // 0 at midlife, 1 at death
            COLOR_MID.lerp(COLOR_COOL, blend)
        };

        // Alpha: quadratic drop-off for focused beam look
        let alpha = particle.start_alpha * t * t;

        sprite.color = Color::LinearRgba(LinearRgba::new(
            color_rgb.x, color_rgb.y, color_rgb.z, alpha,
        ));

        // Particles shrink from start to end size as they travel
        let end_size = if particle.is_halo { PARTICLE_SIZE_END * HALO_SIZE_MULT } else { PARTICLE_SIZE_END };
        let size = particle.start_size * t + end_size * (1.0 - t);
        sprite.custom_size = Some(Vec2::splat(size));
    }
}
