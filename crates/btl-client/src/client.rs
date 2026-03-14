use std::net::SocketAddr;
use std::time::Duration;

use bevy::asset::RenderAssetUsages;
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::window::CursorOptions;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::sprite::Anchor;
use lightyear::prelude::client::input::InputSystems;
use lightyear::prelude::client::*;
use lightyear::prelude::input::native::{ActionState, InputMarker};
use lightyear::prelude::*;
use lightyear::webtransport::prelude::client::WebTransportClientIo;

use avian2d::prelude::*;

use btl_protocol::*;
use btl_shared::{
    Ammo, Asteroid, Cloak, DCOMMANDER_MASS, DCOMMANDER_RADIUS, DAMAGE_FLASH_DURATION,
    DamageFlash, DEFENSE_TURRET_MOUNTS, DRONE_LASER_RANGE, DRONE_RADIUS, Drone, DroneKind,
    FrameInterpolate, GUNSHIP_MASS, GUNSHIP_RADIUS, LASER_RANGE, MINE_RADIUS,
    MINE_TRIGGER_RADIUS, Mine, PULSE_RADIUS, Position, Projectile, RailgunCharge, Rotation,
    SHIP_MASS, SHIP_RADIUS, SNIPER_MASS, SNIPER_RADIUS, SpawnProtection, TBOAT_MASS,
    TBOAT_RADIUS, TORPEDO_RADIUS, TURRET_MOUNTS, Torpedo,
    ZoneDrone, ZoneRailgun, ZoneShield,
    FACTORY_DRONE_LASER_RANGE, OBJECTIVE_ZONE_RADIUS, ZONE_SHIELD_RADIUS, RailgunTurretState,
    ROUND_RESTART_COUNTDOWN, objective_zone_positions,
    compute_intercept, drone_laser_firing, primary_projectile_speed, ray_circle_intersect,
};

use crate::ZoneMarker;

/// Convert the cursor position to world coordinates using the primary window and camera.
fn cursor_world_pos(
    windows: &Query<&Window>,
    camera_query: &Query<(&Camera, &GlobalTransform), With<Camera2d>>,
) -> Option<Vec2> {
    let cursor_pos = windows.single().ok()?.cursor_position()?;
    let (camera, cam_gt) = camera_query.single().ok()?;
    camera.viewport_to_world_2d(cam_gt, cursor_pos).ok()
}

fn team_color(team: &Team) -> Color {
    match team {
        Team::Red => Color::srgb(1.0, 0.3, 0.3),
        Team::Blue => Color::srgb(0.3, 0.3, 1.0),
    }
}

/// Spawn a team-color indicator bar that floats above the ship in world space.
fn spawn_team_label(commands: &mut Commands, ship_entity: Entity, team: &Team, is_local: bool) {
    let (width, color) = if is_local {
        let c = match team {
            Team::Red => LinearRgba::new(2.5, 0.35, 0.2, 1.0),
            Team::Blue => LinearRgba::new(0.2, 0.55, 2.5, 1.0),
        };
        (22.0_f32, Color::LinearRgba(c))
    } else {
        let c = match team {
            Team::Red => Color::srgba(0.9, 0.2, 0.2, 0.65),
            Team::Blue => Color::srgba(0.2, 0.4, 0.9, 0.65),
        };
        (16.0_f32, c)
    };
    commands.spawn((
        ShipLabel,
        ShipLabelFor(ship_entity),
        Sprite {
            color,
            custom_size: Some(Vec2::new(width, 3.0)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 1.0),
        Visibility::Hidden,
    ));
}

/// Keep ship label positions in sync with their ships; despawn orphaned labels.
fn update_ship_labels(
    mut commands: Commands,
    mut labels: Query<(Entity, &ShipLabelFor, &mut Transform, &mut Visibility)>,
    ships: Query<&Transform, (With<ShipInitialized>, Without<ShipLabel>)>,
) {
    for (label_entity, ship_ref, mut label_tf, mut vis) in labels.iter_mut() {
        if let Ok(ship_tf) = ships.get(ship_ref.0) {
            label_tf.translation.x = ship_tf.translation.x;
            label_tf.translation.y = ship_tf.translation.y + 32.0;
            label_tf.translation.z = 1.0;
            *vis = Visibility::Inherited;
        } else {
            commands.entity(label_entity).despawn();
        }
    }
}

fn spawn_gun_barrel(commands: &mut Commands, parent: Entity, pivot_y: f32) {
    commands.spawn((
        ChildOf(parent),
        GunBarrel,
        Sprite {
            color: Color::srgba(0.45, 0.45, 0.5, 0.85),
            custom_size: Some(Vec2::new(14.0, 1.5)),
            ..default()
        },
        Anchor::CENTER_LEFT,
        Transform::from_xyz(0.0, pivot_y, 0.1)
            .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
    ));
}

fn spawn_defense_turret_barrels(commands: &mut Commands, parent: Entity) {
    for (i, mount) in DEFENSE_TURRET_MOUNTS.iter().enumerate() {
        commands.spawn((
            ChildOf(parent),
            TurretBarrel(i),
            Sprite {
                color: Color::srgba(0.4, 0.6, 0.5, 0.85),
                custom_size: Some(Vec2::new(8.0, 1.2)),
                ..default()
            },
            Anchor::CENTER_LEFT,
            Transform::from_xyz(mount.x, mount.y, 0.1),
        ));
    }
}

fn spawn_turret_barrels(commands: &mut Commands, parent: Entity) {
    for (i, mount) in TURRET_MOUNTS.iter().enumerate() {
        commands.spawn((
            ChildOf(parent),
            TurretBarrel(i),
            Sprite {
                color: Color::srgba(0.5, 0.5, 0.55, 0.85),
                custom_size: Some(Vec2::new(10.0, 1.5)),
                ..default()
            },
            Anchor::CENTER_LEFT,
            Transform::from_xyz(mount.x, mount.y, 0.1),
        ));
    }
}

/// Marker for the locally controlled ship.
#[derive(Component)]
pub struct LocalShip;

/// Marker to track that we've already initialized rendering for a predicted entity.
#[derive(Component)]
struct ShipInitialized;

/// Team-color indicator bar spawned above each ship.
#[derive(Component)]
struct ShipLabel;

/// Points from a ShipLabel back to the ship it tracks.
#[derive(Component)]
struct ShipLabelFor(Entity);

/// Marker for asteroid entities that have been given visuals.
#[derive(Component)]
struct AsteroidInitialized;

/// Marker for projectiles that have been given visuals.
#[derive(Component)]
struct ProjectileInitialized;

/// Marker for mines that have been given visuals.
#[derive(Component)]
struct MineInitialized;

#[derive(Component)]
pub struct TorpedoInitialized;

#[derive(Component)]
struct DroneInitialized;

#[derive(Component)]
struct ZoneDroneInitialized;

#[derive(Component)]
struct ZoneRailgunInitialized;

#[derive(Component)]
struct ZoneShieldInitialized;

/// Marker for the gun barrel child entity.
#[derive(Component)]
struct GunBarrel;

/// Marker for turret barrel children (stores which mount index).
#[derive(Component)]
struct TurretBarrel(usize);

/// Tracks the class picker overlay state.
#[derive(Resource, Default)]
struct ClassPicker {
    open: bool,
    /// Set for one frame when a class is selected, then cleared.
    pending_request: u8,
    /// Currently selected ship class (for the HUD indicator).
    selected: ShipClass,
}

/// Marker for the class indicator text in the HUD.
#[derive(Component)]
struct ClassIndicator;

/// Marker for the class picker overlay root node.
#[derive(Component)]
struct ClassPickerOverlay;

/// Marker for a class picker button, storing which class it selects.
#[derive(Component)]
struct ClassPickerButton(ShipClass);

// --- Query filter aliases (tame clippy::type_complexity) ---

type UninitPredicted = (With<Predicted>, Without<ShipInitialized>);
type UninitInterpolated = (With<Interpolated>, Without<ShipInitialized>);
type GunBarrelFilter = (With<GunBarrel>, Without<LocalShip>);

// --- Camera zoom ---

const ZOOM_MIN: f32 = 1.0;
const ZOOM_MAX: f32 = 6.0;
const ZOOM_DEFAULT: f32 = ZOOM_MIN + (ZOOM_MAX - ZOOM_MIN) / 3.0;
/// Scroll sensitivity (scale change per scroll tick)
const ZOOM_SCROLL_STEP: f32 = 0.1;

#[derive(Resource)]
struct CameraZoom {
    scale: f32,
}

/// Transient camera shake state (decays each frame).
#[derive(Resource, Default)]
struct CameraShake {
    intensity: f32,
    remaining: f32,
}

impl Default for CameraZoom {
    fn default() -> Self {
        Self { scale: ZOOM_DEFAULT }
    }
}

// --- Route planning ---

const ROUTE_ZOOM_SCALE: f32 = 4.8;
const ROUTE_ZOOM_SPEED: f32 = 6.0;
const ROUTE_SAMPLE_COUNT: usize = 128;
/// Minimum angle (radians) between consecutive waypoint segments.
/// Derived from min turn radius: at cruise speed ~360, R_min = 360/6 = 60.
/// Angles sharper than ~60° are rejected.
const MIN_WAYPOINT_ANGLE: f32 = std::f32::consts::FRAC_PI_3; // 60°

#[derive(Resource)]
struct RoutePlanner {
    active: bool,
    waypoints: Vec<Vec2>,
    path: Vec<Vec2>,
    /// Per-sample curvature (inverse turning radius) — used for speed control
    curvatures: Vec<f32>,
    /// True if the last waypoint was rejected for being too sharp
    last_rejected: bool,
    target_zoom: f32,
    current_zoom: f32,
}

impl Default for RoutePlanner {
    fn default() -> Self {
        Self {
            active: false,
            waypoints: Vec::new(),
            path: Vec::new(),
            curvatures: Vec::new(),
            last_rejected: false,
            target_zoom: ZOOM_DEFAULT,
            current_zoom: ZOOM_DEFAULT,
        }
    }
}

/// Which algorithm `route_follow` runs for this ship.
/// Add a new variant here to implement an alternative autopilot.
#[derive(Clone, Debug, Default)]
enum AutopilotAlgorithm {
    #[default]
    VelocityVector,
    /// Rotation-first: face the velocity-error direction, fire main thruster;
    /// strafe runs an independent PD loop on cross-track error.
    ThrusterRotate,
    /// Analytic path tracking: scans ahead to determine when to start rotating,
    /// faces the future path tangent so the main thruster pushes along the path,
    /// strafe handles only minor lateral corrections.
    SniperPath,
}

/// Parse ship class from `BTL_AP_CLASS` env var (defaults to TorpedoBoat).
fn ap_class_from_env() -> ShipClass {
    match std::env::var("BTL_AP_CLASS").as_deref().unwrap_or("") {
        "Sniper" | "sniper" => ShipClass::Sniper,
        "Interceptor" | "interceptor" => ShipClass::Interceptor,
        "Gunship" | "gunship" => ShipClass::Gunship,
        "DroneCommander" | "dronecommander" | "drone_commander" => ShipClass::DroneCommander,
        _ => ShipClass::TorpedoBoat,
    }
}

/// Per-tick inputs assembled by `route_follow` before dispatching to the algorithm.
struct AutopilotInput<'a> {
    ship_fwd: Vec2,
    ship_right: Vec2,
    lin_vel: Vec2,
    speed: f32,
    current_omega: f32,
    path: &'a [Vec2],
    progress: f32,
    cte: f32,
    tangent: Vec2,
    path_normal: Vec2,
    target_speed_raw: f32,
    remaining: f32,
}

/// Per-tick outputs returned by the algorithm to `route_follow`.
struct AutopilotOutput {
    rotate: f32,
    thrust_forward: f32,
    stabilize: f32,
    strafe: f32,
    afterburner: bool,
    /// Desired heading — used as aim_angle fallback when no cursor is available.
    desired_angle: f32,
    // Diagnostic fields for debug logging
    target_speed: f32,
    heading_err: f32,
    fwd_vel_error: f32,
    lat_vel_error: f32,
}

/// Tuning coefficients for the autopilot.
/// One instance per ship class — swap freely without touching algorithm code.
#[derive(Clone, Debug)]
struct AutopilotConfig {
    /// Which algorithm to run.
    algorithm: AutopilotAlgorithm,
    // Speed profile (computed once at route injection)
    smooth_window: usize,
    curvature_margin: f32,
    curvature_divisor: f32,
    speed_cap: f32,        // fraction of SHIP_MAX_SPEED
    centripetal_thrust: f32, // if > 0, caps speed at sqrt(centripetal_thrust/k) per curve
    accel: f32,
    decel: f32,
    // CTE speed reduction
    cte_divisor: f32,
    cte_speed_floor: f32,
    // Desired-velocity / CTE correction
    correction_gain: f32, // k_p: lateral correction strength
    correction_kd: f32,   // k_d: derivative damping on lateral velocity (ThrusterRotate only)
    correction_cap: f32,  // max correction speed (px/s)
    // Look-ahead (ThrusterRotate only)
    look_ahead_time: f32,
    look_ahead_min: f32,
    look_ahead_max: f32,
    // Thrust / strafe / brake scaling
    vel_error_scale: f32,
    // Afterburner gate
    afterburner_fwd_threshold: f32,
    afterburner_heading_min: f32,
    afterburner_cte_max: f32,
    // Safety margin before end of route
    stopping_dist_margin: f32,
}

impl AutopilotConfig {
    fn for_class(class: ShipClass) -> Self {
        use btl_shared::{
            DCOMMANDER_AFTERBURNER_THRUST, GUNSHIP_AFTERBURNER_THRUST,
            SNIPER_AFTERBURNER_THRUST, TBOAT_AFTERBURNER_THRUST, TBOAT_THRUST,
        };
        let ab = match class {
            ShipClass::Interceptor    => SHIP_AFTERBURNER_THRUST,
            ShipClass::Gunship        => GUNSHIP_AFTERBURNER_THRUST,
            ShipClass::TorpedoBoat    => TBOAT_AFTERBURNER_THRUST,
            ShipClass::Sniper         => SNIPER_AFTERBURNER_THRUST,
            ShipClass::DroneCommander => DCOMMANDER_AFTERBURNER_THRUST,
        };
        // Sniper gets the analytic path-tracking algorithm.
        if class == ShipClass::Sniper {
            return Self {
                algorithm: AutopilotAlgorithm::SniperPath,
                smooth_window: 25,
                curvature_margin: 0.32,
                curvature_divisor: 180.0,
                speed_cap: 0.82,
                centripetal_thrust: 0.0,
                accel: ab,
                decel: SHIP_STABILIZE_DECEL * 0.8,
                cte_divisor: 80.0,
                cte_speed_floor: 0.35,
                correction_gain: 0.4,  // unused
                correction_kd: 0.4,    // lateral velocity damping for strafe
                correction_cap: 300.0, // strafe divisor (larger = weaker strafe)
                // look_ahead_time = early-rotation margin factor: start rotating when
                // you still have this many × the minimum required rotation time left.
                look_ahead_time: 2.5,
                look_ahead_min: 150.0,
                look_ahead_max: 1200.0,
                vel_error_scale: 80.0,
                afterburner_fwd_threshold: 100.0,
                afterburner_heading_min: 0.5,
                afterburner_cte_max: 150.0,
                stopping_dist_margin: 1.5,
            };
        }
        // TorpedoBoat gets a dedicated rotation-first algorithm.
        if class == ShipClass::TorpedoBoat {
            return Self {
                algorithm: AutopilotAlgorithm::ThrusterRotate,
                smooth_window: 40,
                curvature_margin: 0.28,
                curvature_divisor: 180.0,
                speed_cap: 0.75,
                centripetal_thrust: TBOAT_THRUST,
                accel: ab,
                decel: SHIP_STABILIZE_DECEL * 0.7,
                cte_divisor: 70.0,
                cte_speed_floor: 0.40,
                correction_gain: 0.5,
                correction_kd: 0.8,
                correction_cap: 200.0,
                look_ahead_time: 0.9,
                look_ahead_min: 80.0,
                look_ahead_max: 400.0,
                vel_error_scale: 80.0,
                afterburner_fwd_threshold: 100.0,
                afterburner_heading_min: 0.5,
                afterburner_cte_max: 150.0,
                stopping_dist_margin: 1.5,
            };
        }
        Self {
            algorithm: AutopilotAlgorithm::VelocityVector,
            smooth_window: 25,
            curvature_margin: 0.32,
            curvature_divisor: 180.0,
            speed_cap: 0.78,
            centripetal_thrust: 0.0,
            accel: ab,
            decel: SHIP_STABILIZE_DECEL * 0.8,
            cte_divisor: 60.0,
            cte_speed_floor: 0.40,
            correction_gain: 0.5,
            correction_kd: 0.0,
            correction_cap: 250.0,
            look_ahead_time: 0.0,
            look_ahead_min: 0.0,
            look_ahead_max: 0.0,
            vel_error_scale: 80.0,
            afterburner_fwd_threshold: 100.0,
            afterburner_heading_min: 0.5,
            afterburner_cte_max: 150.0,
            stopping_dist_margin: 1.5,
        }
    }
}

/// Attached to the local ship while it's following a route.
#[derive(Component)]
struct RouteFollowing {
    path: Vec<Vec2>,
    curvatures: Vec<f32>,
    /// Precomputed max speed at each path point (braking-aware).
    speed_profile: Vec<f32>,
    config: AutopilotConfig,
    /// Progress along the path as a fractional index (continuous, not discrete)
    progress: f32,
    /// Tick counter for periodic debug logging (tuning only).
    debug_tick: u32,
}

/// Test route injector for autopilot tuning.
/// Cycles through predefined routes once ship has stopped after each one.
#[derive(Resource, Default)]
struct TestRoutes {
    /// Absolute waypoints per route (world coordinates, not relative to ship).
    routes: Vec<Vec<Vec2>>,
    current: usize,
}

impl TestRoutes {
    fn new() -> Self {
        // Routes chain: first waypoint of each route = last waypoint of previous.
        // Ship brakes to a stop between routes, so it starts each route near WP[0].
        let routes = vec![
            // 1. Sweeping S-curve (from spawn, ends at (6500,0))
            vec![
                Vec2::new(-200.0, 0.0), Vec2::new(1500.0, 0.0), Vec2::new(2500.0, 1000.0),
                Vec2::new(4000.0, 1200.0), Vec2::new(5500.0, 1000.0), Vec2::new(6500.0, 0.0),
            ],
            // 2. Southern sweep (from (6500,0), arcs south to (0,-3500))
            vec![
                Vec2::new(6500.0, 0.0), Vec2::new(7000.0, -1000.0), Vec2::new(7000.0, -2500.0),
                Vec2::new(6000.0, -4000.0), Vec2::new(4000.0, -5000.0), Vec2::new(0.0, -3500.0),
            ],
            // 3. Chicane (from (0,-3500), alternating west to (-5500,-4000))
            vec![
                Vec2::new(0.0, -3500.0), Vec2::new(-2000.0, -2500.0), Vec2::new(-3000.0, -4500.0),
                Vec2::new(-4500.0, -3000.0), Vec2::new(-5500.0, -4500.0),
            ],
            // 4. Wide U-turn (from (-5500,-4500), sweeps north to (-5000,3000))
            vec![
                Vec2::new(-5500.0, -4500.0), Vec2::new(-7000.0, -2500.0), Vec2::new(-7000.0, 0.0),
                Vec2::new(-6500.0, 2000.0), Vec2::new(-5000.0, 3000.0),
            ],
            // 5. Lazy loop back (from (-5000,3000), sweeps back toward origin)
            vec![
                Vec2::new(-5000.0, 3000.0), Vec2::new(-2500.0, 5000.0), Vec2::new(0.0, 5000.0),
                Vec2::new(2500.0, 4000.0), Vec2::new(3500.0, 2000.0), Vec2::new(2000.0, 0.0),
            ],
        ];
        Self { routes, current: 0 }
    }
}

/// Build a fan-triangulated mesh from a convex polygon defined by `verts`.
/// The centroid is computed and used as the center vertex (index 0).
fn build_fan_mesh(verts: Vec<[f32; 3]>) -> Mesh {
    let n = verts.len();
    let cx: f32 = verts.iter().map(|v| v[0]).sum::<f32>() / n as f32;
    let cy: f32 = verts.iter().map(|v| v[1]).sum::<f32>() / n as f32;

    let mut positions = Vec::with_capacity(n + 1);
    positions.push([cx, cy, 0.0]); // index 0 = center
    positions.extend_from_slice(&verts);

    let mut indices: Vec<u16> = Vec::with_capacity(n * 3);
    for i in 0..n {
        indices.push(0);
        indices.push((i + 1) as u16);
        indices.push(((i + 1) % n + 1) as u16);
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U16(indices));
    mesh
}

/// Interceptor hull mesh: elongated needle/wedge, Razorback-inspired.
/// Long narrow body tapering to a sharp nose. No wings.
fn create_interceptor_mesh(r: f32) -> Mesh {
    build_fan_mesh(vec![
        [0.0, r * 1.6, 0.0],       // 0: nose tip
        [r * 0.25, r * 0.3, 0.0],  // 1: right shoulder
        [r * 0.3, -r * 0.6, 0.0],  // 2: right rear
        [0.0, -r * 0.9, 0.0],      // 3: tail
        [-r * 0.3, -r * 0.6, 0.0], // 4: left rear
        [-r * 0.25, r * 0.3, 0.0], // 5: left shoulder
    ])
}

/// Gunship hull mesh: wider, blockier than the Interceptor. Armored look.
fn create_gunship_mesh(r: f32) -> Mesh {
    build_fan_mesh(vec![
        [0.0, r * 1.2, 0.0],        // 0: nose (blunter than interceptor)
        [r * 0.45, r * 0.5, 0.0],   // 1: right forward
        [r * 0.55, -r * 0.1, 0.0],  // 2: right mid (widest)
        [r * 0.4, -r * 0.7, 0.0],   // 3: right rear
        [0.0, -r * 0.85, 0.0],      // 4: tail
        [-r * 0.4, -r * 0.7, 0.0],  // 5: left rear
        [-r * 0.55, -r * 0.1, 0.0], // 6: left mid (widest)
        [-r * 0.45, r * 0.5, 0.0],  // 7: left forward
    ])
}

/// Torpedo boat hull mesh: sleek medium body with side nacelles.
fn create_torpedo_boat_mesh(r: f32) -> Mesh {
    build_fan_mesh(vec![
        [0.0, r * 1.35, 0.0],       // 0: bow tip
        [r * 0.15, r * 1.2, 0.0],   // 1: right bow curve
        [r * 0.28, r * 0.9, 0.0],   // 2: right forward hull
        [r * 0.32, r * 0.4, 0.0],   // 3: right mid-forward (widest)
        [r * 0.3, -r * 0.1, 0.0],   // 4: right mid-rear
        [r * 0.25, -r * 0.5, 0.0],  // 5: right rear taper
        [r * 0.15, -r * 0.8, 0.0],  // 6: right stern
        [0.0, -r * 0.9, 0.0],       // 7: stern tip
        [-r * 0.15, -r * 0.8, 0.0], // 8: left stern
        [-r * 0.25, -r * 0.5, 0.0], // 9: left rear taper
        [-r * 0.3, -r * 0.1, 0.0],  // 10: left mid-rear
        [-r * 0.32, r * 0.4, 0.0],  // 11: left mid-forward (widest)
        [-r * 0.28, r * 0.9, 0.0],  // 12: left forward hull
        [-r * 0.15, r * 1.2, 0.0],  // 13: left bow curve
    ])
}

/// Sniper hull mesh: slim, angular stealth profile.
fn create_sniper_mesh(r: f32) -> Mesh {
    build_fan_mesh(vec![
        [0.0, r * 1.5, 0.0],       // 0: sharp nose
        [r * 0.15, r * 0.8, 0.0],  // 1: right forward (very narrow)
        [r * 0.35, r * 0.1, 0.0],  // 2: right wing tip
        [r * 0.2, -r * 0.5, 0.0],  // 3: right rear
        [r * 0.1, -r * 0.9, 0.0],  // 4: right tail fin
        [0.0, -r * 0.7, 0.0],      // 5: center tail
        [-r * 0.1, -r * 0.9, 0.0], // 6: left tail fin
        [-r * 0.2, -r * 0.5, 0.0], // 7: left rear
        [-r * 0.35, r * 0.1, 0.0], // 8: left wing tip
        [-r * 0.15, r * 0.8, 0.0], // 9: left forward
    ])
}

/// Drone Commander hull mesh: wide, flat hexagonal carrier shape.
fn create_drone_commander_mesh(r: f32) -> Mesh {
    build_fan_mesh(vec![
        [0.0, r * 1.0, 0.0],       // 0: nose (blunt)
        [r * 0.5, r * 0.6, 0.0],   // 1: right forward
        [r * 0.65, 0.0, 0.0],      // 2: right mid (widest)
        [r * 0.5, -r * 0.5, 0.0],  // 3: right rear
        [r * 0.2, -r * 0.8, 0.0],  // 4: right tail
        [0.0, -r * 0.65, 0.0],     // 5: center tail
        [-r * 0.2, -r * 0.8, 0.0], // 6: left tail
        [-r * 0.5, -r * 0.5, 0.0], // 7: left rear
        [-r * 0.65, 0.0, 0.0],     // 8: left mid (widest)
        [-r * 0.5, r * 0.6, 0.0],  // 9: left forward
    ])
}

pub struct ClientPlugin {
    pub server_addr: SocketAddr,
    pub client_id: u64,
    pub cert_hash: String,
}

impl Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        let server_addr = self.server_addr;
        let client_id = self.client_id;
        let cert_hash = self.cert_hash.clone();

        app.add_plugins(lightyear::prelude::client::ClientPlugins {
            tick_duration: Duration::from_secs_f64(1.0 / FIXED_TIMESTEP_HZ),
        });

        app.insert_resource(ClientConnectionConfig {
            server_addr,
            client_id,
            cert_hash,
        });

        app.init_resource::<CameraZoom>();
        app.init_resource::<CameraShake>();
        app.init_resource::<RoutePlanner>();
        // Pre-select class from BTL_AP_CLASS env var so it spawns immediately on connect (tuning mode).
        let ap_class = ap_class_from_env();
        app.insert_resource(ClassPicker {
            pending_request: ap_class.to_request(),
            selected: ap_class,
            open: false,
        });
        app.insert_resource(TestRoutes::new());

        app.add_systems(Startup, connect_to_server);
        app.add_systems(
            FixedPreUpdate,
            (inject_test_route, buffer_input, brake_between_routes, route_follow)
                .chain()
                .in_set(InputSystems::WriteClientInputs),
        );
        app.add_observer(log_connected);
        app.add_systems(
            Update,
            (
                init_predicted_ships,
                init_interpolated_ships,
                init_asteroids,
                init_projectiles,
                init_mines,
                init_torpedoes,
                update_projectile_visuals,
                update_mine_visuals,
                update_torpedo_visuals,
                update_gun_barrels,
                update_turret_barrels,
                camera_follow_local_ship,
                scroll_zoom,
                update_hud,
                update_score_hud,
                update_zone_colors,
                render_zone_capture_arcs,
            ),
        );
        app.add_systems(
            Update,
            (
                render_laser_beams,
                render_railgun,
                render_aim_helpers,
                update_cloak_visuals,
                update_damage_flash_visuals,
                update_spawn_protection_visuals,
                init_drones,
                update_drone_visuals,
                render_drone_lasers,
                render_pulse_indicator,
                route_planning_input,
                route_zoom,
                class_picker_input,
                class_picker_click,
                update_class_indicator,
                render_route_gizmos,
            ),
        );
        app.add_systems(
            Update,
            (
                init_zone_drones,
                update_zone_drone_visuals,
                render_zone_drone_lasers,
                init_zone_railguns,
                update_zone_railgun_visuals,
                init_zone_shields,
                render_zone_shields,
            ),
        );
        app.add_systems(
            Startup,
            (spawn_hud, spawn_score_hud, spawn_victory_overlay, spawn_kill_feed, spawn_class_picker, hide_window_cursor),
        );
        app.add_systems(Update, (update_victory_overlay, update_kill_feed, shake_on_damage, render_custom_cursor, update_ship_labels));
    }
}

#[derive(Resource)]
struct ClientConnectionConfig {
    server_addr: SocketAddr,
    client_id: u64,
    cert_hash: String,
}

fn connect_to_server(mut commands: Commands, config: Res<ClientConnectionConfig>) {
    let auth = Authentication::Manual {
        server_addr: config.server_addr,
        client_id: config.client_id,
        private_key: PRIVATE_KEY,
        protocol_id: PROTOCOL_ID,
    };

    let netcode = NetcodeClient::new(
        auth,
        NetcodeConfig {
            client_timeout_secs: 5,
            token_expire_secs: -1,
            ..default()
        },
    )
    .expect("Failed to create NetcodeClient");

    let entity = commands
        .spawn((
            Client::default(),
            netcode,
            PeerAddr(config.server_addr),
            WebTransportClientIo {
                certificate_digest: config.cert_hash.clone(),
            },
            ReplicationReceiver::default(),
            PredictionManager::default(),
        ))
        .id();

    commands.trigger(Connect { entity });
    info!(
        "Connecting to server at {} as client {}",
        config.server_addr, config.client_id
    );
}

/// Read keyboard + mouse input and write it to the input buffer.
/// Skipped when route-following is active (route_follow writes inputs instead)
/// or when route planning mode is active (don't fly while planning).
fn buffer_input(
    mut query: Query<&mut ActionState<ShipInput>, With<InputMarker<ShipInput>>>,
    keypress: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    ship_query: Query<(&Transform, &LinearVelocity), With<LocalShip>>,
    route_following: Query<(), (With<LocalShip>, With<RouteFollowing>)>,
    planner: Res<RoutePlanner>,
    mut picker: ResMut<ClassPicker>,
) {
    // Don't overwrite inputs while route following or planning
    if route_following.single().is_ok() || planner.active {
        return;
    }

    // Compute aim angle: direction from ship to mouse cursor in world space
    let aim_angle = cursor_world_pos(&windows, &camera_query)
        .and_then(|world_pos| {
            let ship_pos = ship_query.single().ok()?.0.translation.truncate();
            let delta = world_pos - ship_pos;
            (delta.length_squared() > 1.0).then(|| delta.y.atan2(delta.x))
        })
        .unwrap_or(std::f32::consts::FRAC_PI_2); // default: aim up

    let key = |k| f32::from(keypress.pressed(k));
    let axis = |pos, neg| key(pos) - key(neg);

    for mut action_state in query.iter_mut() {
        // Consume pending class request only when we have an action state to write to
        let class_request = std::mem::take(&mut picker.pending_request);
        action_state.0 = ShipInput {
            thrust_forward: key(KeyCode::KeyW),
            thrust_backward: key(KeyCode::KeyS),
            rotate: axis(KeyCode::KeyA, KeyCode::KeyD) * 0.6,
            strafe: axis(KeyCode::KeyQ, KeyCode::KeyE),
            afterburner: keypress.pressed(KeyCode::ShiftLeft),
            stabilize: key(KeyCode::KeyR),
            fire: mouse_button.pressed(MouseButton::Left),
            drop_mine: keypress.just_pressed(KeyCode::KeyX),
            aim_angle,
            class_request,
        };
    }
}

/// Log when our client connection is established.
fn log_connected(trigger: On<Add, Connected>, query: Query<(), With<Client>>) {
    if query.get(trigger.entity).is_ok() {
        info!("Connected to server!");
    }
}

/// Initialize rendering for predicted ships once their components are synced.
fn init_predicted_ships(
    mut commands: Commands,
    query: Query<
        (
            Entity,
            &PlayerId,
            &Team,
            &ShipClass,
            &Position,
            &Rotation,
            Has<Controlled>,
        ),
        UninitPredicted,
    >,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, player_id, team, class, pos, rot, is_controlled) in query.iter() {
        let (radius, mass) = match class {
            ShipClass::Interceptor => (SHIP_RADIUS, SHIP_MASS),
            ShipClass::Gunship => (GUNSHIP_RADIUS, GUNSHIP_MASS),
            ShipClass::TorpedoBoat => (TBOAT_RADIUS, TBOAT_MASS),
            ShipClass::Sniper => (SNIPER_RADIUS, SNIPER_MASS),
            ShipClass::DroneCommander => (DCOMMANDER_RADIUS, DCOMMANDER_MASS),
        };
        let ship_mesh = match class {
            ShipClass::Interceptor => meshes.add(create_interceptor_mesh(radius)),
            ShipClass::Gunship => meshes.add(create_gunship_mesh(radius)),
            ShipClass::TorpedoBoat => meshes.add(create_torpedo_boat_mesh(radius)),
            ShipClass::Sniper => meshes.add(create_sniper_mesh(radius)),
            ShipClass::DroneCommander => meshes.add(create_drone_commander_mesh(radius)),
        };
        commands.entity(entity).insert((
            Mesh2d(ship_mesh),
            MeshMaterial2d(materials.add(team_color(team))),
            Transform::from_xyz(pos.0.x, pos.0.y, 0.0)
                .with_rotation(Quat::from_rotation_z(rot.as_radians())),
            ShipInitialized,
            FrameInterpolate::<Position> {
                trigger_change_detection: true,
                ..default()
            },
            FrameInterpolate::<Rotation> {
                trigger_change_detection: true,
                ..default()
            },
        ));
        let barrel_pivot_y = if *class == ShipClass::Interceptor { SHIP_RADIUS * 0.4 } else { 0.0 };
        spawn_gun_barrel(&mut commands, entity, barrel_pivot_y);
        if *class == ShipClass::Gunship {
            spawn_turret_barrels(&mut commands, entity);
        } else if *class == ShipClass::DroneCommander {
            spawn_defense_turret_barrels(&mut commands, entity);
        }
        spawn_team_label(&mut commands, entity, team, is_controlled);

        if is_controlled {
            let angular_inertia = 0.5 * mass * radius * radius;
            commands.entity(entity).insert((
                RigidBody::Dynamic,
                Collider::circle(radius),
                Mass(mass),
                AngularInertia(angular_inertia),
                LinearDamping(0.0),
                AngularDamping(0.0),
                InputMarker::<ShipInput>::default(),
                LocalShip,
            ));
            crate::particles::spawn_thruster_nozzles(
                &mut commands,
                entity,
                &mut meshes,
                &mut materials,
            );
            info!(
                "Spawned local {class:?} for {:?} on {:?} team",
                player_id.0, team
            );
        } else {
            info!(
                "Spawned remote {class:?} for {:?} on {:?} team",
                player_id.0, team
            );
        }
    }
}

/// Initialize rendering for interpolated (remote) ships.
fn init_interpolated_ships(
    mut commands: Commands,
    query: Query<(Entity, &PlayerId, &Team, &ShipClass, &Position, &Rotation), UninitInterpolated>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, player_id, team, class, pos, rot) in query.iter() {
        let radius = match class {
            ShipClass::Interceptor => SHIP_RADIUS,
            ShipClass::Gunship => GUNSHIP_RADIUS,
            ShipClass::TorpedoBoat => TBOAT_RADIUS,
            ShipClass::Sniper => SNIPER_RADIUS,
            ShipClass::DroneCommander => DCOMMANDER_RADIUS,
        };
        let ship_mesh = match class {
            ShipClass::Interceptor => meshes.add(create_interceptor_mesh(radius)),
            ShipClass::Gunship => meshes.add(create_gunship_mesh(radius)),
            ShipClass::TorpedoBoat => meshes.add(create_torpedo_boat_mesh(radius)),
            ShipClass::Sniper => meshes.add(create_sniper_mesh(radius)),
            ShipClass::DroneCommander => meshes.add(create_drone_commander_mesh(radius)),
        };
        commands.entity(entity).insert((
            Mesh2d(ship_mesh),
            MeshMaterial2d(materials.add(team_color(team))),
            Transform::from_xyz(pos.0.x, pos.0.y, 0.0)
                .with_rotation(Quat::from_rotation_z(rot.as_radians())),
            ShipInitialized,
        ));
        let barrel_pivot_y = if *class == ShipClass::Interceptor { SHIP_RADIUS * 0.4 } else { 0.0 };
        spawn_gun_barrel(&mut commands, entity, barrel_pivot_y);
        if *class == ShipClass::Gunship {
            spawn_turret_barrels(&mut commands, entity);
        } else if *class == ShipClass::DroneCommander {
            spawn_defense_turret_barrels(&mut commands, entity);
        }
        spawn_team_label(&mut commands, entity, team, false);

        info!(
            "Spawned interpolated {class:?} for {:?} on {:?} team",
            player_id.0, team
        );
    }
}

/// Initialize rendering for replicated asteroid entities.
fn init_asteroids(
    mut commands: Commands,
    query: Query<(Entity, &Asteroid), Without<AsteroidInitialized>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, asteroid) in query.iter() {
        let r = asteroid.radius;
        let seed = entity.to_bits();

        // Use a regular polygon (7-sided) as asteroid shape
        let mesh = meshes.add(RegularPolygon::new(r, 7));

        // Brownish-gray color with slight variation per asteroid
        let hash = seed.wrapping_mul(2654435761);
        let gray = 0.25 + 0.1 * ((hash % 1000) as f32 / 1000.0);
        let color = Color::srgb(gray + 0.05, gray, gray - 0.03);

        commands.entity(entity).insert((
            Mesh2d(mesh),
            MeshMaterial2d(materials.add(color)),
            AsteroidInitialized,
        ));
    }
}

/// Initialize rendering for replicated projectile entities.
/// Visual style depends on ProjectileKind.
fn init_projectiles(
    mut commands: Commands,
    query: Query<
        (Entity, &LinearVelocity, &Position, Option<&ProjectileKind>),
        (With<Projectile>, Without<ProjectileInitialized>),
    >,
) {
    for (entity, vel, pos, kind) in query.iter() {
        let kind = kind.copied().unwrap_or_default();
        let (color, size) = match kind {
            // Autocannon: bright yellow, small
            ProjectileKind::Autocannon => (
                Color::LinearRgba(LinearRgba::new(3.0, 2.5, 0.8, 1.0)),
                Vec2::new(8.0, 2.0),
            ),
            // Heavy cannon: orange-red, larger
            ProjectileKind::HeavyCannon => (
                Color::LinearRgba(LinearRgba::new(4.0, 1.5, 0.3, 1.0)),
                Vec2::new(12.0, 4.0),
            ),
            // Turret: cyan-blue, small
            ProjectileKind::Turret => (
                Color::LinearRgba(LinearRgba::new(0.5, 2.0, 3.0, 1.0)),
                Vec2::new(6.0, 1.5),
            ),
            // Railgun: bright white-blue, long tracer
            ProjectileKind::Railgun => (
                Color::LinearRgba(LinearRgba::new(2.0, 3.0, 5.0, 1.0)),
                Vec2::new(24.0, 3.0),
            ),
        };
        let angle = vel.0.y.atan2(vel.0.x);

        commands.entity(entity).insert((
            Sprite {
                color,
                custom_size: Some(size),
                ..default()
            },
            Transform::from_xyz(pos.0.x, pos.0.y, 5.0).with_rotation(Quat::from_rotation_z(angle)),
            ProjectileInitialized,
        ));
    }
}

/// Orient projectiles along their velocity each frame.
fn update_projectile_visuals(
    mut query: Query<(&mut Transform, &LinearVelocity), With<ProjectileInitialized>>,
) {
    for (mut tf, vel) in query.iter_mut() {
        if vel.0.length_squared() > 0.1 {
            let angle = vel.0.y.atan2(vel.0.x);
            tf.rotation = Quat::from_rotation_z(angle);
        }
    }
}

/// Initialize rendering for replicated mine entities.
/// Per design doc: black octagon with white shadow/outline and pulsing red core.
fn init_mines(
    mut commands: Commands,
    query: Query<(Entity, &Mine), Without<MineInitialized>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, _mine) in query.iter() {
        // Subtle shadow/outline (slightly larger octagon behind, very dim)
        let shadow_mesh = meshes.add(RegularPolygon::new(MINE_RADIUS + 1.5, 8));
        let shadow_color = Color::LinearRgba(LinearRgba::new(0.3, 0.3, 0.3, 0.12));
        commands.spawn((
            MineShadow { parent: entity },
            Mesh2d(shadow_mesh),
            MeshMaterial2d(materials.add(shadow_color)),
            Transform::from_xyz(0.0, 0.0, 4.9),
        ));

        // Dark octagonal shell
        let shell_mesh = meshes.add(RegularPolygon::new(MINE_RADIUS, 8));
        let shell_color = Color::srgb(0.03, 0.03, 0.03);
        commands.entity(entity).insert((
            Mesh2d(shell_mesh),
            MeshMaterial2d(materials.add(shell_color)),
            MineInitialized,
        ));

        // Dim pulsing red core (smaller inner octagon)
        let core_mesh = meshes.add(RegularPolygon::new(MINE_RADIUS * 0.35, 8));
        let core_color = Color::LinearRgba(LinearRgba::new(0.8, 0.08, 0.04, 0.4));
        commands.spawn((
            MineCore { parent: entity },
            Mesh2d(core_mesh),
            MeshMaterial2d(materials.add(core_color)),
            Transform::from_xyz(0.0, 0.0, 5.0),
        ));
    }
}

/// Marker linking a mine core glow to its mine entity.
#[derive(Component)]
struct MineCore {
    parent: Entity,
}

/// Marker linking a mine shadow to its mine entity.
#[derive(Component)]
struct MineShadow {
    parent: Entity,
}

type MineCoreFilter = (Without<MineInitialized>, Without<MineShadow>);
type MineShadowFilter = (Without<MineInitialized>, Without<MineCore>);
type ShipForMineFilter = (
    With<ShipInitialized>,
    Without<MineInitialized>,
    Without<MineCore>,
    Without<MineShadow>,
);

/// Pulse mine cores, position shadows, proximity warning, and clean up orphaned children.
fn update_mine_visuals(
    mut commands: Commands,
    mines: Query<(Entity, &Mine, &Transform), With<MineInitialized>>,
    mut cores: Query<
        (
            Entity,
            &MineCore,
            &mut Transform,
            &mut MeshMaterial2d<ColorMaterial>,
        ),
        MineCoreFilter,
    >,
    mut shadows: Query<(Entity, &MineShadow, &mut Transform), MineShadowFilter>,
    ships: Query<(&Transform, &Team), ShipForMineFilter>,
    time: Res<Time>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let t = time.elapsed_secs();

    for (mine_entity, mine, mine_tf) in mines.iter() {
        let mine_pos = mine_tf.translation.truncate();

        // Check proximity to enemy ships for warning pulse
        let mut closest_enemy_dist = f32::MAX;
        for (ship_tf, ship_team) in ships.iter() {
            if *ship_team == mine.owner_team {
                continue;
            }
            let dist = (ship_tf.translation.truncate() - mine_pos).length();
            if dist < closest_enemy_dist {
                closest_enemy_dist = dist;
            }
        }

        // Proximity boost: pulse faster as enemies approach trigger radius
        let proximity_mult = if closest_enemy_dist < MINE_TRIGGER_RADIUS * 2.5 {
            1.0 + (1.0 - closest_enemy_dist / (MINE_TRIGGER_RADIUS * 2.5)) * 4.0
        } else {
            1.0
        };

        let base_rate = if mine.arm_timer > 0.0 { 0.5 } else { 1.5 };
        let pulse_rate = base_rate * proximity_mult;
        let pulse = ((t * pulse_rate * std::f32::consts::TAU).sin() * 0.5 + 0.5).powi(2);
        let intensity = 0.3 + pulse * 0.6;

        for (_core_entity, core, mut core_tf, mat_handle) in cores.iter_mut() {
            if core.parent == mine_entity {
                core_tf.translation.x = mine_tf.translation.x;
                core_tf.translation.y = mine_tf.translation.y;
                core_tf.translation.z = mine_tf.translation.z + 0.1;

                if let Some(mat) = materials.get_mut(&mat_handle.0) {
                    mat.color = Color::LinearRgba(LinearRgba::new(
                        intensity,
                        0.04 * pulse,
                        0.02,
                        0.2 + 0.2 * pulse,
                    ));
                }
            }
        }

        for (_shadow_entity, shadow, mut shadow_tf) in shadows.iter_mut() {
            if shadow.parent == mine_entity {
                shadow_tf.translation.x = mine_tf.translation.x;
                shadow_tf.translation.y = mine_tf.translation.y;
                shadow_tf.translation.z = mine_tf.translation.z - 0.1;
            }
        }
    }

    // Clean up orphaned cores and shadows
    for (core_entity, core, _, _) in cores.iter() {
        if mines.get(core.parent).is_err() {
            commands.entity(core_entity).despawn();
        }
    }
    for (shadow_entity, shadow, _) in shadows.iter() {
        if mines.get(shadow.parent).is_err() {
            commands.entity(shadow_entity).despawn();
        }
    }
}

/// Initialize rendering for replicated torpedo entities.
fn init_torpedoes(
    mut commands: Commands,
    query: Query<
        (Entity, &LinearVelocity, &Position),
        (With<Torpedo>, Without<TorpedoInitialized>),
    >,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, vel, pos) in query.iter() {
        let angle = vel.0.y.atan2(vel.0.x);
        let r = TORPEDO_RADIUS;
        // Torpedo body: pointed ellipse shape (no glow — values <= 1.0)
        let mesh = meshes.add(Triangle2d::new(
            Vec2::new(r * 2.0, 0.0),
            Vec2::new(-r * 1.0, r * 0.8),
            Vec2::new(-r * 1.0, -r * 0.8),
        ));
        let mat = materials.add(ColorMaterial::from_color(Color::srgb(0.6, 0.75, 0.5)));
        commands.entity(entity).insert((
            Mesh2d(mesh),
            MeshMaterial2d(mat),
            Transform::from_xyz(pos.0.x, pos.0.y, 5.0).with_rotation(Quat::from_rotation_z(angle)),
            TorpedoInitialized,
        ));
    }
}

/// Orient torpedoes along their velocity each frame.
fn update_torpedo_visuals(
    mut query: Query<(&mut Transform, &LinearVelocity), With<TorpedoInitialized>>,
) {
    for (mut tf, vel) in query.iter_mut() {
        if vel.0.length_squared() > 0.1 {
            let angle = vel.0.y.atan2(vel.0.x);
            tf.rotation = Quat::from_rotation_z(angle);
        }
    }
}

/// Shared visual setup for player and zone defense drones.
/// `marker` is inserted as the "initialized" tag (e.g., `DroneInitialized`).
fn insert_drone_mesh_visuals(
    commands: &mut Commands,
    entity: Entity,
    team: Team,
    kind: DroneKind,
    pos: Vec2,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    marker: impl Bundle,
) {
    let team_tint = match team {
        Team::Red => LinearRgba::new(1.5, 0.5, 0.3, 0.9),
        Team::Blue => LinearRgba::new(0.3, 0.5, 1.5, 0.9),
    };
    match kind {
        DroneKind::Laser => {
            let r = DRONE_RADIUS;
            let mesh = meshes.add(Triangle2d::new(
                Vec2::new(r * 1.5, 0.0),
                Vec2::new(-r * 0.8, r * 0.6),
                Vec2::new(-r * 0.8, -r * 0.6),
            ));
            let mat = materials.add(ColorMaterial::from_color(Color::LinearRgba(team_tint)));
            commands.entity(entity).insert((
                Mesh2d(mesh),
                MeshMaterial2d(mat),
                Transform::from_xyz(pos.x, pos.y, 4.0),
                marker,
            ));
        }
        DroneKind::Kamikaze => {
            let r = DRONE_RADIUS * 0.9;
            let mesh = meshes.add(RegularPolygon::new(r, 8));
            let kaze_tint = LinearRgba::new(
                team_tint.red * 0.8 + 0.5,
                team_tint.green * 0.5,
                team_tint.blue * 0.3,
                0.9,
            );
            let mat = materials.add(ColorMaterial::from_color(Color::LinearRgba(kaze_tint)));
            commands.entity(entity).insert((
                Mesh2d(mesh),
                MeshMaterial2d(mat),
                Transform::from_xyz(pos.x, pos.y, 4.0),
                marker,
            ));
        }
    }
}

/// Initialize rendering for replicated drone entities (small triangles).
fn init_drones(
    mut commands: Commands,
    query: Query<(Entity, &Drone, &Position), Without<DroneInitialized>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, drone, pos) in query.iter() {
        insert_drone_mesh_visuals(
            &mut commands, entity, drone.owner_team, drone.kind, pos.0,
            &mut meshes, &mut materials, DroneInitialized,
        );
    }
}

/// Orient drones along their velocity.
fn update_drone_visuals(
    mut query: Query<(&mut Transform, &LinearVelocity), With<DroneInitialized>>,
) {
    for (mut tf, vel) in query.iter_mut() {
        if vel.0.length_squared() > 1.0 {
            let angle = vel.0.y.atan2(vel.0.x);
            tf.rotation = Quat::from_rotation_z(angle);
        }
    }
}

/// Render thin laser beams from laser drones to their nearest enemy target.
fn render_drone_lasers(
    drones: Query<(Entity, &Drone, &Transform), With<DroneInitialized>>,
    enemies: Query<(&Transform, &Team), With<ShipInitialized>>,
    mut gizmos: Gizmos,
    time: Res<Time>,
) {
    let elapsed = time.elapsed_secs();
    for (drone_entity, drone, drone_tf) in drones.iter() {
        if drone.kind != DroneKind::Laser {
            continue;
        }
        if !drone_laser_firing(drone_entity.to_bits(), elapsed) {
            continue;
        }
        let drone_pos = drone_tf.translation.truncate();
        let range_sq = DRONE_LASER_RANGE * DRONE_LASER_RANGE;

        let mut best_dist_sq = range_sq;
        let mut best_pos = None;
        for (enemy_tf, enemy_team) in enemies.iter() {
            if *enemy_team == drone.owner_team {
                continue;
            }
            let dist_sq = (drone_pos - enemy_tf.translation.truncate()).length_squared();
            if dist_sq < best_dist_sq {
                best_dist_sq = dist_sq;
                best_pos = Some(enemy_tf.translation.truncate());
            }
        }

        if let Some(target_pos) = best_pos {
            let dist = (target_pos - drone_pos).length();
            let fade = 1.0 - 0.7 * (dist / DRONE_LASER_RANGE);
            let base = match drone.owner_team {
                Team::Red => LinearRgba::new(1.5, 0.3, 0.2, 0.5),
                Team::Blue => LinearRgba::new(0.2, 0.3, 1.5, 0.5),
            };
            let faded = LinearRgba::new(
                base.red * fade,
                base.green * fade,
                base.blue * fade,
                base.alpha * fade,
            );
            gizmos.line_2d(drone_pos, target_pos, Color::LinearRgba(faded));
        }
    }
}

/// Render anti-drone pulse radius indicator for DroneCommander.
fn render_pulse_indicator(
    ships: Query<(&ShipClass, &Transform, &MineCooldown), With<LocalShip>>,
    mut gizmos: Gizmos,
) {
    for (class, ship_tf, cooldown) in ships.iter() {
        if *class != ShipClass::DroneCommander {
            continue;
        }
        let ship_pos = ship_tf.translation.truncate();

        // Show pulse radius when ready (subtle circle)
        if cooldown.remaining <= 0.0 {
            gizmos.circle_2d(ship_pos, PULSE_RADIUS, Color::srgba(0.5, 0.8, 0.5, 0.1));
        }
    }
}

/// Initialize rendering for zone defense drones (reuse drone visual style).
fn init_zone_drones(
    mut commands: Commands,
    query: Query<(Entity, &ZoneDrone, &Position), Without<ZoneDroneInitialized>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, drone, pos) in query.iter() {
        insert_drone_mesh_visuals(
            &mut commands, entity, drone.team, drone.kind, pos.0,
            &mut meshes, &mut materials, ZoneDroneInitialized,
        );
    }
}

/// Orient zone drones along their velocity (same as player drones).
fn update_zone_drone_visuals(
    mut query: Query<(&mut Transform, &LinearVelocity), With<ZoneDroneInitialized>>,
) {
    for (mut tf, vel) in query.iter_mut() {
        if vel.0.length_squared() > 1.0 {
            let angle = vel.0.to_angle();
            tf.rotation = Quat::from_rotation_z(angle);
        }
    }
}

/// Render laser beams from zone defense laser drones to their targets.
fn render_zone_drone_lasers(
    drones: Query<(&ZoneDrone, &Transform), With<ZoneDroneInitialized>>,
    enemies: Query<(&Transform, &Team), With<ShipInitialized>>,
    mut gizmos: Gizmos,
) {
    let range_sq = FACTORY_DRONE_LASER_RANGE * FACTORY_DRONE_LASER_RANGE;

    for (drone, drone_tf) in drones.iter() {
        if !matches!(drone.kind, DroneKind::Laser) {
            continue;
        }

        let drone_pos = drone_tf.translation.truncate();
        let mut best_dist_sq = range_sq;
        let mut best_pos = None;

        for (enemy_tf, enemy_team) in enemies.iter() {
            if *enemy_team == drone.team {
                continue;
            }
            let enemy_pos = enemy_tf.translation.truncate();
            let dist_sq = (enemy_pos - drone_pos).length_squared();
            if dist_sq < best_dist_sq {
                best_dist_sq = dist_sq;
                best_pos = Some(enemy_pos);
            }
        }

        if let Some(target_pos) = best_pos {
            let beam_color = match drone.team {
                Team::Red => Color::srgba(1.0, 0.3, 0.2, 0.6),
                Team::Blue => Color::srgba(0.2, 0.3, 1.0, 0.6),
            };
            gizmos.line_2d(drone_pos, target_pos, beam_color);
        }
    }
}

/// Initialize rendering for zone railgun turrets.
fn init_zone_railguns(
    mut commands: Commands,
    query: Query<(Entity, &ZoneRailgun, &Position), Without<ZoneRailgunInitialized>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, turret, pos) in query.iter() {
        let color = match turret.team {
            Team::Red => LinearRgba::new(1.0, 0.2, 0.1, 0.8),
            Team::Blue => LinearRgba::new(0.1, 0.2, 1.0, 0.8),
        };
        // Railgun turret: a diamond/rhombus shape
        let r = 20.0;
        let mesh = meshes.add(Triangle2d::new(
            Vec2::new(r * 2.0, 0.0),
            Vec2::new(-r * 0.6, r * 0.8),
            Vec2::new(-r * 0.6, -r * 0.8),
        ));
        let mat = materials.add(ColorMaterial::from_color(Color::LinearRgba(color)));
        commands.entity(entity).insert((
            Mesh2d(mesh),
            MeshMaterial2d(mat),
            Transform::from_xyz(pos.0.x, pos.0.y, 5.0),
            ZoneRailgunInitialized,
        ));
    }
}

/// Update railgun turret visuals: rotation follows aim_angle, charge glow.
fn update_zone_railgun_visuals(
    mut query: Query<(&ZoneRailgun, &mut Transform), With<ZoneRailgunInitialized>>,
    mut gizmos: Gizmos,
) {
    for (turret, mut tf) in query.iter_mut() {
        tf.rotation = Quat::from_rotation_z(turret.aim_angle);
        let turret_pos = tf.translation.truncate();

        // Show charge indicator
        if turret.charge > 0.0 {
            let alpha = turret.charge * 0.5;
            let charge_color = Color::srgba(0.0, 1.0, 1.0, alpha);
            gizmos.circle_2d(turret_pos, 15.0 + turret.charge * 10.0, charge_color);
        }

        // Telegraph: when locked, draw a bright warning line
        if matches!(turret.state, RailgunTurretState::Locked(_)) {
            let dir = Vec2::new(turret.aim_angle.cos(), turret.aim_angle.sin());
            let end = turret_pos + dir * 800.0;
            let warn_color = Color::srgba(1.0, 0.0, 0.0, 0.7);
            gizmos.line_2d(turret_pos, end, warn_color);
        }
    }
}

/// Initialize rendering for zone shield bubbles.
fn init_zone_shields(
    mut commands: Commands,
    query: Query<(Entity, &ZoneShield, &Position), Without<ZoneShieldInitialized>>,
) {
    for (entity, _shield, _pos) in query.iter() {
        // Shield is rendered via gizmos, just add the marker
        commands.entity(entity).insert(ZoneShieldInitialized);
    }
}

/// Render shield bubble as a translucent circle.
fn render_zone_shields(
    shields: Query<(&ZoneShield, &Position), With<ZoneShieldInitialized>>,
    mut gizmos: Gizmos,
) {
    for (shield, pos) in shields.iter() {
        if !shield.active {
            continue;
        }
        let color = match shield.team {
            Team::Red => Color::srgba(1.0, 0.3, 0.2, 0.15),
            Team::Blue => Color::srgba(0.2, 0.3, 1.0, 0.15),
        };
        let edge_color = match shield.team {
            Team::Red => Color::srgba(1.0, 0.4, 0.3, 0.4),
            Team::Blue => Color::srgba(0.3, 0.4, 1.0, 0.4),
        };
        // Draw filled-ish shield with concentric rings
        gizmos.circle_2d(pos.0, ZONE_SHIELD_RADIUS, edge_color);
        gizmos.circle_2d(pos.0, ZONE_SHIELD_RADIUS * 0.95, color);
        gizmos.circle_2d(pos.0, ZONE_SHIELD_RADIUS * 0.9, color);
    }
}

/// Draw laser beam from TorpedoBoat ships that are firing.
fn render_laser_beams(
    ships: Query<
        (
            &ShipClass,
            &Transform,
            &ActionState<ShipInput>,
            &Team,
            &Ammo,
        ),
        With<LocalShip>,
    >,
    enemies: Query<(&Transform, &Team), (With<ShipInitialized>, Without<LocalShip>)>,
    asteroids: Query<(&Transform, &Asteroid)>,
    mut gizmos: Gizmos,
) {
    for (class, ship_tf, input, team, ammo) in ships.iter() {
        if *class != ShipClass::TorpedoBoat || !input.0.fire || ammo.current <= 0.0 {
            continue;
        }

        let ship_pos = ship_tf.translation.truncate();
        let aim_dir = Vec2::new(input.0.aim_angle.cos(), input.0.aim_angle.sin());

        // Find closest hit along beam (asteroids block, enemies take damage)
        let mut best_t = LASER_RANGE;

        // Check asteroids
        for (ast_tf, ast) in asteroids.iter() {
            let t =
                ray_circle_intersect(ship_pos, aim_dir, ast_tf.translation.truncate(), ast.radius);
            if t > 0.0 && t < best_t {
                best_t = t;
            }
        }

        // Check enemies
        for (enemy_tf, enemy_team) in enemies.iter() {
            if *enemy_team == *team {
                continue;
            }
            let enemy_pos = enemy_tf.translation.truncate();
            let to_enemy = enemy_pos - ship_pos;
            let t = to_enemy.dot(aim_dir);
            if t < 0.0 || t > best_t {
                continue;
            }
            let closest = ship_pos + aim_dir * t;
            let dist_sq = (enemy_pos - closest).length_squared();
            if dist_sq < TBOAT_RADIUS * TBOAT_RADIUS * 4.0 {
                best_t = t;
            }
        }

        // Draw beam as fading segments (fade by absolute distance, not relative)
        let segments = 12;
        let offset_dir = Vec2::new(-aim_dir.y, aim_dir.x);
        let seg_len = best_t / segments as f32;
        for i in 0..segments {
            let d0 = seg_len * i as f32;
            let d1 = seg_len * (i + 1) as f32;
            let p0 = ship_pos + aim_dir * d0;
            let p1 = ship_pos + aim_dir * d1;
            let mid_dist = (d0 + d1) * 0.5;
            let fade = 1.0 - 0.8 * (mid_dist / LASER_RANGE); // fade by absolute distance from source
            // Core beam
            gizmos.line_2d(
                p0,
                p1,
                Color::LinearRgba(LinearRgba::new(
                    2.0 * fade,
                    0.3 * fade,
                    0.3 * fade,
                    0.9 * fade,
                )),
            );
            // Glow
            let glow_a = 0.3 * fade;
            gizmos.line_2d(
                p0 + offset_dir,
                p1 + offset_dir,
                Color::LinearRgba(LinearRgba::new(1.0 * fade, 0.15 * fade, 0.1 * fade, glow_a)),
            );
            gizmos.line_2d(
                p0 - offset_dir,
                p1 - offset_dir,
                Color::LinearRgba(LinearRgba::new(1.0 * fade, 0.15 * fade, 0.1 * fade, glow_a)),
            );
        }
    }
}

/// Render railgun charge glow on Sniper ships.
/// The railgun projectile visual is handled by init_projectiles/update_projectile_visuals.
fn render_railgun(ships: Query<(&ShipClass, &Transform, &RailgunCharge)>, mut gizmos: Gizmos) {
    for (class, ship_tf, charge) in ships.iter() {
        if *class != ShipClass::Sniper {
            continue;
        }

        let ship_pos = ship_tf.translation.truncate();

        // Show charge glow (bright circle around ship, scales with charge)
        if charge.charge > 0.01 {
            let intensity = charge.charge;
            let glow_radius = SNIPER_RADIUS * (1.2 + 0.8 * intensity);
            gizmos.circle_2d(
                ship_pos,
                glow_radius,
                Color::LinearRgba(LinearRgba::new(
                    0.5 + 2.5 * intensity,
                    0.8 + 1.2 * intensity,
                    3.0 * intensity,
                    0.15 + 0.3 * intensity,
                )),
            );
        }
    }
}

/// Draw a lead-indicator crosshair at the predicted intercept point for each visible
/// enemy ship, accounting for projectile flight time and both ships' velocities.
/// Only shown for classes with a ballistic player-aimed primary weapon.
fn render_aim_helpers(
    local_ship: Query<(&Transform, &LinearVelocity, &ShipClass, &Team), With<LocalShip>>,
    enemies: Query<
        (&Transform, &LinearVelocity, &Team, &Cloak),
        (With<ShipInitialized>, Without<LocalShip>),
    >,
    mut gizmos: Gizmos,
) {
    let Ok((ship_tf, own_vel, class, my_team)) = local_ship.single() else {
        return;
    };
    let Some(speed) = primary_projectile_speed(*class) else {
        return;
    };

    let own_pos = ship_tf.translation.truncate();

    for (enemy_tf, enemy_vel, enemy_team, cloak) in enemies.iter() {
        if *enemy_team == *my_team || cloak.active {
            continue;
        }
        let target_pos = enemy_tf.translation.truncate();
        let Some(aim_pt) = compute_intercept(own_pos, own_vel.0, target_pos, enemy_vel.0, speed)
        else {
            continue;
        };

        let color = Color::srgba(1.0, 0.85, 0.1, 0.75);
        let dim = Color::srgba(1.0, 0.85, 0.1, 0.3);
        gizmos.circle_2d(aim_pt, 9.0, color);
        gizmos.line_2d(aim_pt - Vec2::X * 13.0, aim_pt + Vec2::X * 13.0, dim);
        gizmos.line_2d(aim_pt - Vec2::Y * 13.0, aim_pt + Vec2::Y * 13.0, dim);
    }
}

/// Hide the OS cursor; a per-class gizmo cursor is drawn in its place.
fn hide_window_cursor(mut cursor_opts: Query<&mut CursorOptions>) {
    if let Ok(mut opts) = cursor_opts.single_mut() {
        opts.visible = false;
    }
}

/// Draw a ship-class-appropriate cursor at the mouse position in world space.
/// All sizes are multiplied by `zoom.scale` so the cursor stays the same screen size.
fn render_custom_cursor(
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    local_ship: Query<&ShipClass, With<LocalShip>>,
    zoom: Res<CameraZoom>,
    picker: Res<ClassPicker>,
    mut gizmos: Gizmos,
) {
    if picker.open {
        return;
    }
    let Some(p) = cursor_world_pos(&windows, &camera_query) else {
        return;
    };
    let s = zoom.scale;
    let class = local_ship.single().ok().copied().unwrap_or_default();
    let bright = Color::srgba(0.95, 0.95, 0.95, 0.9);
    let dim = Color::srgba(0.95, 0.95, 0.95, 0.4);

    match class {
        ShipClass::Interceptor => {
            // Gap crosshair — point the autocannon at the target
            let gap = 4.0 * s;
            let arm = 10.0 * s;
            for dir in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
                gizmos.line_2d(p + dir * gap, p + dir * (gap + arm), bright);
            }
            gizmos.circle_2d(p, 1.5 * s, bright);
        }
        ShipClass::Gunship => {
            // Corner bracket reticle — heavy targeting lock for the cannon
            let hs = 10.0 * s;
            let arm = 5.0 * s;
            for (cx, cy) in [(hs, hs), (hs, -hs), (-hs, hs), (-hs, -hs)] {
                let c = p + Vec2::new(cx, cy);
                gizmos.line_2d(c, c + Vec2::new(-cx.signum() * arm, 0.0), bright);
                gizmos.line_2d(c, c + Vec2::new(0.0, -cy.signum() * arm), bright);
            }
        }
        ShipClass::TorpedoBoat => {
            // Concentric rings with ticks — laser beam contact point
            gizmos.circle_2d(p, 3.0 * s, bright);
            gizmos.circle_2d(p, 8.0 * s, dim);
            let inner = 10.0 * s;
            let outer = 13.0 * s;
            for dir in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
                gizmos.line_2d(p + dir * inner, p + dir * outer, bright);
            }
        }
        ShipClass::Sniper => {
            // Scope ring + crosshairs + outer ticks — precise railgun aim
            let r = 14.0 * s;
            let ext = 8.0 * s;
            gizmos.circle_2d(p, r, bright);
            gizmos.line_2d(p - Vec2::X * r, p + Vec2::X * r, dim);
            gizmos.line_2d(p - Vec2::Y * r, p + Vec2::Y * r, dim);
            for dir in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
                gizmos.line_2d(p + dir * (r + 3.0 * s), p + dir * (r + ext), bright);
            }
        }
        ShipClass::DroneCommander => {
            // Tri-spoke with capped tips — drone deployment spread indicator
            gizmos.circle_2d(p, 4.0 * s, dim);
            for i in 0..3 {
                let angle = i as f32 * std::f32::consts::TAU / 3.0;
                let dir = Vec2::new(angle.cos(), angle.sin());
                let perp = Vec2::new(-dir.y, dir.x);
                gizmos.line_2d(p + dir * 6.0 * s, p + dir * 14.0 * s, bright);
                let tip = p + dir * 14.0 * s;
                gizmos.line_2d(tip - perp * 2.5 * s, tip + perp * 2.5 * s, bright);
            }
        }
    }
}

/// Apply cloak visual: make cloaked enemy ships semi-transparent (faint shimmer).
/// Own cloaked ship gets slight transparency. Allied cloaked ships stay visible.
fn update_cloak_visuals(
    ships: Query<
        (
            Ref<Cloak>,
            &Team,
            &MeshMaterial2d<ColorMaterial>,
            Has<LocalShip>,
        ),
        With<ShipInitialized>,
    >,
    local_team: Query<&Team, With<LocalShip>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    time: Res<Time>,
) {
    let Ok(my_team) = local_team.single() else {
        return;
    };
    let t = time.elapsed_secs();

    for (cloak, team, mat_handle, is_local) in ships.iter() {
        // Skip if not cloaked and state hasn't changed (avoids spurious asset mutations)
        if !cloak.active && !cloak.is_changed() {
            continue;
        }
        let Some(mat) = materials.get_mut(&mat_handle.0) else {
            continue;
        };

        if cloak.active {
            if is_local {
                mat.color = mat.color.with_alpha(0.4);
            } else if *team == *my_team {
                mat.color = mat.color.with_alpha(0.5);
            } else {
                let shimmer = (t * 3.0).sin() * 0.05 + 0.08;
                mat.color = mat.color.with_alpha(shimmer);
            }
        } else {
            mat.color = mat.color.with_alpha(1.0);
        }
    }
}

/// White flash overlay when ship takes damage.
fn update_damage_flash_visuals(
    mut ships: Query<
        (&DamageFlash, &mut MeshMaterial2d<ColorMaterial>),
        With<ShipInitialized>,
    >,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (flash, mat_handle) in ships.iter_mut() {
        if flash.timer <= 0.0 {
            continue;
        }
        let Some(mat) = materials.get_mut(&mat_handle.0) else {
            continue;
        };
        // Blend toward white based on flash progress (1.0 at start, 0.0 at end)
        let t = (flash.timer / DAMAGE_FLASH_DURATION).clamp(0.0, 1.0);
        let r = mat.color.to_srgba();
        mat.color = Color::srgba(
            r.red + (1.0 - r.red) * t * 0.8,
            r.green + (1.0 - r.green) * t * 0.8,
            r.blue + (1.0 - r.blue) * t * 0.8,
            r.alpha,
        );
    }
}

/// Pulsing translucent effect during spawn invulnerability.
fn update_spawn_protection_visuals(
    mut ships: Query<
        (&SpawnProtection, &mut MeshMaterial2d<ColorMaterial>),
        With<ShipInitialized>,
    >,
    mut materials: ResMut<Assets<ColorMaterial>>,
    time: Res<Time>,
) {
    let t = time.elapsed_secs();
    for (sp, mat_handle) in ships.iter_mut() {
        if sp.remaining <= 0.0 {
            continue;
        }
        let Some(mat) = materials.get_mut(&mat_handle.0) else {
            continue;
        };
        // Pulsing alpha: oscillate between 0.3 and 0.7
        let pulse = (t * 6.0).sin() * 0.2 + 0.5;
        mat.color = mat.color.with_alpha(pulse);
    }
}

#[derive(Component)]
struct HudText;

#[derive(Component)]
struct HealthBarFill;

#[derive(Component)]
struct FuelBarFill;

#[derive(Component)]
struct AmmoBarFill;

#[derive(Component)]
struct ScoreBarRed;

#[derive(Component)]
struct ScoreBarBlue;

#[derive(Component)]
struct ScoreText;

#[derive(Component)]
struct VictoryOverlay;

#[derive(Component)]
struct VictoryText;

#[derive(Component)]
struct VictoryStatsText;

#[derive(Component)]
struct VictoryCountdownText;

#[derive(Component)]
struct KillFeedContainer;

#[derive(Component)]
struct KillFeedEntry;

type HealthBarFilter = (
    With<HealthBarFill>,
    Without<FuelBarFill>,
    Without<AmmoBarFill>,
    Without<HudText>,
);
type FuelBarFilter = (
    With<FuelBarFill>,
    Without<HealthBarFill>,
    Without<AmmoBarFill>,
    Without<HudText>,
);
type AmmoBarFilter = (
    With<AmmoBarFill>,
    Without<HealthBarFill>,
    Without<FuelBarFill>,
    Without<HudText>,
);

const BAR_WIDTH: f32 = 160.0;
const BAR_HEIGHT: f32 = 10.0;

fn spawn_hud(mut commands: Commands) {
    // Bottom-left HUD panel
    let panel = commands
        .spawn((Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(12.0),
            left: Val::Px(12.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        },))
        .id();

    // Health bar
    let health_row = commands
        .spawn((
            ChildOf(panel),
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .id();

    commands.spawn((
        ChildOf(health_row),
        Text::new("HP"),
        TextFont {
            font_size: 12.0,
            ..default()
        },
        TextColor(Color::srgba(0.8, 0.3, 0.3, 0.9)),
    ));

    let health_bg = commands
        .spawn((
            ChildOf(health_row),
            Node {
                width: Val::Px(BAR_WIDTH),
                height: Val::Px(BAR_HEIGHT),
                ..default()
            },
            BackgroundColor(Color::srgba(0.15, 0.05, 0.05, 0.8)),
        ))
        .id();

    commands.spawn((
        ChildOf(health_bg),
        HealthBarFill,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgb(0.8, 0.2, 0.2)),
    ));

    // Fuel bar
    let fuel_row = commands
        .spawn((
            ChildOf(panel),
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .id();

    commands.spawn((
        ChildOf(fuel_row),
        Text::new("FU"),
        TextFont {
            font_size: 12.0,
            ..default()
        },
        TextColor(Color::srgba(0.3, 0.5, 0.8, 0.9)),
    ));

    let fuel_bg = commands
        .spawn((
            ChildOf(fuel_row),
            Node {
                width: Val::Px(BAR_WIDTH),
                height: Val::Px(BAR_HEIGHT),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.05, 0.15, 0.8)),
        ))
        .id();

    commands.spawn((
        ChildOf(fuel_bg),
        FuelBarFill,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgb(0.2, 0.4, 0.8)),
    ));

    // Ammo bar
    let ammo_row = commands
        .spawn((
            ChildOf(panel),
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .id();

    commands.spawn((
        ChildOf(ammo_row),
        Text::new("AM"),
        TextFont {
            font_size: 12.0,
            ..default()
        },
        TextColor(Color::srgba(0.7, 0.6, 0.3, 0.9)),
    ));

    let ammo_bg = commands
        .spawn((
            ChildOf(ammo_row),
            Node {
                width: Val::Px(BAR_WIDTH),
                height: Val::Px(BAR_HEIGHT),
                ..default()
            },
            BackgroundColor(Color::srgba(0.1, 0.08, 0.02, 0.8)),
        ))
        .id();

    commands.spawn((
        ChildOf(ammo_bg),
        AmmoBarFill,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgb(0.7, 0.5, 0.2)),
    ));

    // Class indicator row at the bottom of the HUD panel
    commands.spawn((
        ClassIndicator,
        ChildOf(panel),
        Text::new("> INTERCEPTOR <"),
        TextFont {
            font_size: 12.0,
            ..default()
        },
        TextColor(Color::srgb(0.2, 0.6, 0.3)),
    ));

    // Speed + coords text (top-left)
    commands.spawn((
        HudText,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            left: Val::Px(12.0),
            ..default()
        },
        Text::new("SPD 0 | (0, 0)"),
        TextFont {
            font_size: 14.0,
            ..default()
        },
        TextColor(Color::srgba(0.7, 0.7, 0.7, 0.8)),
    ));
}

/// Spawn score HUD at top-center.
fn spawn_score_hud(mut commands: Commands) {
    let score_limit = btl_shared::SCORE_LIMIT as i32;

    // Top-center panel
    let panel = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(10.0),
            left: Val::Percent(50.0),
            margin: UiRect::left(Val::Px(-120.0)),
            width: Val::Px(240.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(4.0),
            ..default()
        })
        .id();

    // Score text: "RED 0 — 0 BLUE"
    commands.spawn((
        ChildOf(panel),
        ScoreText,
        Text::new(format!("RED 0 / {score_limit}  —  0 / {score_limit} BLUE")),
        TextFont {
            font_size: 16.0,
            ..default()
        },
        TextColor(Color::srgba(0.9, 0.9, 0.9, 0.9)),
    ));

    // Red score bar
    let bar_row = commands
        .spawn((
            ChildOf(panel),
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(4.0),
                ..default()
            },
        ))
        .id();

    let red_bg = commands
        .spawn((
            ChildOf(bar_row),
            Node {
                width: Val::Px(100.0),
                height: Val::Px(6.0),
                flex_direction: FlexDirection::RowReverse,
                ..default()
            },
            BackgroundColor(Color::srgba(0.15, 0.05, 0.05, 0.6)),
        ))
        .id();

    commands.spawn((
        ChildOf(red_bg),
        ScoreBarRed,
        Node {
            width: Val::Percent(0.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgb(0.9, 0.2, 0.2)),
    ));

    let blue_bg = commands
        .spawn((
            ChildOf(bar_row),
            Node {
                width: Val::Px(100.0),
                height: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.05, 0.15, 0.6)),
        ))
        .id();

    commands.spawn((
        ChildOf(blue_bg),
        ScoreBarBlue,
        Node {
            width: Val::Percent(0.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgb(0.2, 0.2, 0.9)),
    ));
}

/// Spawn hidden victory overlay (shown when a team wins).
fn spawn_victory_overlay(mut commands: Commands) {
    let overlay = commands
        .spawn((
            VictoryOverlay,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            GlobalZIndex(300),
            Visibility::Hidden,
        ))
        .id();

    let column = commands
        .spawn((
            ChildOf(overlay),
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(16.0),
                ..default()
            },
        ))
        .id();

    commands.spawn((
        ChildOf(column),
        VictoryText,
        Text::new(""),
        TextFont { font_size: 48.0, ..default() },
        TextColor(Color::WHITE),
    ));
    commands.spawn((
        ChildOf(column),
        VictoryStatsText,
        Text::new(""),
        TextFont { font_size: 18.0, ..default() },
        TextColor(Color::srgba(0.85, 0.85, 0.85, 1.0)),
    ));
    commands.spawn((
        ChildOf(column),
        VictoryCountdownText,
        Text::new(""),
        TextFont { font_size: 22.0, ..default() },
        TextColor(Color::srgba(0.65, 0.65, 0.65, 1.0)),
    ));
}

/// Spawn the kill feed container in the top-right corner.
fn spawn_kill_feed(mut commands: Commands) {
    commands.spawn((
        KillFeedContainer,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(58.0),
            right: Val::Px(12.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexEnd,
            row_gap: Val::Px(2.0),
            ..default()
        },
        GlobalZIndex(100),
    ));
}

/// Show/hide victory overlay based on round state.
fn update_victory_overlay(
    scores_q: Query<&TeamScores>,
    mut overlay_q: Query<&mut Visibility, With<VictoryOverlay>>,
    mut title_q: Query<(&mut Text, &mut TextColor), With<VictoryText>>,
    mut stats_q: Query<
        &mut Text,
        (With<VictoryStatsText>, Without<VictoryText>, Without<VictoryCountdownText>),
    >,
    mut countdown_q: Query<
        &mut Text,
        (With<VictoryCountdownText>, Without<VictoryText>, Without<VictoryStatsText>),
    >,
) {
    let Ok(scores) = scores_q.single() else {
        return;
    };
    let Ok(mut vis) = overlay_q.single_mut() else {
        return;
    };

    let show_winner = |winner: Team,
                       title_q: &mut Query<(&mut Text, &mut TextColor), With<VictoryText>>,
                       stats_q: &mut Query<&mut Text, (With<VictoryStatsText>, Without<VictoryText>, Without<VictoryCountdownText>)>,
                       countdown_q: &mut Query<&mut Text, (With<VictoryCountdownText>, Without<VictoryText>, Without<VictoryStatsText>)>,
                       end_stats: &[PlayerStat]| {
        if let Ok((mut text, mut color)) = title_q.single_mut() {
            match winner {
                Team::Red => {
                    **text = "RED TEAM WINS".into();
                    *color = TextColor(Color::srgb(1.0, 0.3, 0.3));
                }
                Team::Blue => {
                    **text = "BLUE TEAM WINS".into();
                    *color = TextColor(Color::srgb(0.3, 0.6, 1.0));
                }
            }
        }
        if let Ok(mut text) = stats_q.single_mut() {
            if end_stats.is_empty() {
                **text = "".into();
            } else {
                let lines: Vec<String> = end_stats
                    .iter()
                    .map(|s| {
                        let team = match s.team {
                            Team::Red => "RED",
                            Team::Blue => "BLUE",
                        };
                        let word = if s.kills == 1 { "kill" } else { "kills" };
                        format!("{}  {} {}", team, s.kills, word)
                    })
                    .collect();
                **text = lines.join("\n");
            }
        }
        if let Ok(mut text) = countdown_q.single_mut() {
            **text = "".into();
        }
    };

    match scores.round_state {
        RoundState::Playing => {
            *vis = Visibility::Hidden;
        }
        RoundState::Won(winner) => {
            *vis = Visibility::Inherited;
            show_winner(winner, &mut title_q, &mut stats_q, &mut countdown_q, &scores.end_stats);
        }
        RoundState::Restarting(remaining) => {
            *vis = Visibility::Inherited;
            if remaining > ROUND_RESTART_COUNTDOWN {
                // Display phase: show winner + stats
                if let Some(winner) = scores.last_winner {
                    show_winner(winner, &mut title_q, &mut stats_q, &mut countdown_q, &scores.end_stats);
                }
            } else {
                // Countdown phase: clear details, show timer
                if let Ok((mut text, mut color)) = title_q.single_mut() {
                    **text = "".into();
                    *color = TextColor(Color::WHITE);
                }
                if let Ok(mut text) = stats_q.single_mut() {
                    **text = "".into();
                }
                if let Ok(mut text) = countdown_q.single_mut() {
                    **text = format!("Next round in {}...", remaining.ceil() as u32);
                }
            }
        }
    }
}

/// Rebuild the kill feed rows with team-colored spans when the feed changes.
fn update_kill_feed(
    mut commands: Commands,
    scores_q: Query<Ref<TeamScores>>,
    container_q: Query<Entity, With<KillFeedContainer>>,
    entries_q: Query<Entity, With<KillFeedEntry>>,
) {
    let Ok(scores) = scores_q.single() else { return; };
    if !scores.is_changed() { return; }
    let Ok(container) = container_q.single() else { return; };

    // Despawn old rows.
    for e in entries_q.iter() {
        commands.entity(e).despawn();
    }

    let font = TextFont { font_size: 13.0, ..default() };
    let dim = Color::srgba(0.85, 0.85, 0.85, 0.6);

    for event in scores.kill_feed.iter() {
        let (killer_label, killer_color) = match event.killer_team {
            Team::Red => ("RED", Color::srgba(1.0, 0.35, 0.35, 0.95)),
            Team::Blue => ("BLU", Color::srgba(0.35, 0.6, 1.0, 0.95)),
        };
        let (victim_label, victim_color) = match event.victim_team {
            Team::Red => ("RED", Color::srgba(0.9, 0.25, 0.25, 0.7)),
            Team::Blue => ("BLU", Color::srgba(0.25, 0.5, 0.9, 0.7)),
        };
        let class_label = match event.victim_class {
            ShipClass::Interceptor => "Interceptor",
            ShipClass::Gunship => "Gunship",
            ShipClass::TorpedoBoat => "Torpedo",
            ShipClass::Sniper => "Sniper",
            ShipClass::DroneCommander => "Carrier",
        };

        // Row container (right-aligned).
        let row = commands.spawn((
            KillFeedEntry,
            Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, ..default() },
        )).id();

        let s0 = commands.spawn((Text::new(killer_label), font.clone(), TextColor(killer_color), ChildOf(row))).id();
        let s1 = commands.spawn((Text::new(" → "),         font.clone(), TextColor(dim),          ChildOf(row))).id();
        let s2 = commands.spawn((Text::new(victim_label), font.clone(), TextColor(victim_color), ChildOf(row))).id();
        let s3 = commands.spawn((Text::new(format!(" {}", class_label)), font.clone(), TextColor(dim), ChildOf(row))).id();
        let _ = (s0, s1, s2, s3); // suppress unused warnings

        commands.entity(container).add_child(row);
    }
}

/// Update score HUD from replicated TeamScores.
fn update_score_hud(
    scores_q: Query<&TeamScores>,
    mut text_q: Query<&mut Text, With<ScoreText>>,
    mut red_bar: Query<&mut Node, (With<ScoreBarRed>, Without<ScoreBarBlue>, Without<ScoreText>)>,
    mut blue_bar: Query<&mut Node, (With<ScoreBarBlue>, Without<ScoreBarRed>, Without<ScoreText>)>,
) {
    let Ok(scores) = scores_q.single() else {
        return;
    };

    let limit = btl_shared::SCORE_LIMIT;
    let red_i = scores.red as i32;
    let blue_i = scores.blue as i32;
    let limit_i = limit as i32;

    if let Ok(mut text) = text_q.single_mut() {
        **text = format!("RED {red_i} / {limit_i}  —  {blue_i} / {limit_i} BLUE");
    }

    if let Ok(mut node) = red_bar.single_mut() {
        node.width = Val::Percent((scores.red / limit * 100.0).min(100.0));
    }

    if let Ok(mut node) = blue_bar.single_mut() {
        node.width = Val::Percent((scores.blue / limit * 100.0).min(100.0));
    }
}

/// Color objective zone markers based on which team controls them.
fn update_zone_colors(
    scores_q: Query<&TeamScores>,
    mut markers: Query<(&ZoneMarker, &mut Sprite)>,
) {
    let Ok(scores) = scores_q.single() else {
        return;
    };

    for (zone, mut sprite) in markers.iter_mut() {
        let zs = &scores.zones[zone.0];
        // Progress: -1.0 = fully Red, 0.0 = neutral, 1.0 = fully Blue
        let p = zs.progress;
        if p < -0.01 {
            // Red capturing/controlled — intensity scales with progress
            let t = p.abs();
            sprite.color = Color::srgba(0.4 + 0.5 * t, 0.25, 0.2 + 0.05 * t, 0.4 + 0.2 * t);
        } else if p > 0.01 {
            // Blue capturing/controlled
            let t = p.abs();
            sprite.color = Color::srgba(0.2 + 0.05 * t, 0.25, 0.4 + 0.5 * t, 0.4 + 0.2 * t);
        } else {
            // Neutral
            sprite.color = Color::srgba(0.4, 0.4, 0.2, 0.5);
        }
    }
}

/// Draw a filled arc inside each objective zone ring showing capture progress.
///
/// Arc span = |progress| × 360°, centered at the top of the ring.
/// Colored red or blue depending on which team is ahead.
fn render_zone_capture_arcs(
    scores_q: Query<&TeamScores>,
    mut gizmos: Gizmos,
) {
    let Ok(scores) = scores_q.single() else { return; };
    let zones = objective_zone_positions();

    for (i, &center) in zones.iter().enumerate() {
        let progress = scores.zones[i].progress;
        if progress.abs() < 0.02 {
            continue;
        }

        let arc_span = progress.abs() * std::f32::consts::TAU;
        // Center the arc at the top (12 o'clock). Arc sweeps CCW from start_angle.
        let start_angle = std::f32::consts::FRAC_PI_2 + arc_span * 0.5;
        let iso = Isometry2d::new(center, Rot2::radians(start_angle));

        let t = progress.abs();
        let (r, g, b) = if progress < 0.0 {
            (0.85, 0.1, 0.1)
        } else {
            (0.1, 0.25, 0.85)
        };

        // Multiple concentric arcs create a "thick fill" appearance.
        // Opacity scales with progress so a nearly-captured zone is vivid.
        let inner = OBJECTIVE_ZONE_RADIUS * 0.55;
        let outer = OBJECTIVE_ZONE_RADIUS * 0.88;
        let steps = 8u32;
        for s in 0..=steps {
            let radius = inner + (outer - inner) * (s as f32 / steps as f32);
            let alpha = (0.06 + 0.18 * t) * (1.0 - (s as f32 / steps as f32 - 0.5).abs() * 0.8);
            gizmos.arc_2d(iso, arc_span, radius, Color::srgba(r, g, b, alpha));
        }
    }
}

/// Spawn the class picker overlay (hidden by default, toggled with Tab).
fn spawn_class_picker(mut commands: Commands) {
    // Full-screen centering container (Pickable::IGNORE so it doesn't eat clicks)
    let overlay = commands
        .spawn((
            ClassPickerOverlay,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Vw(100.0),
                height: Val::Vh(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            GlobalZIndex(200),
            Visibility::Hidden,
            Pickable::IGNORE,
        ))
        .id();

    // Inner panel
    let panel = commands
        .spawn((
            ChildOf(overlay),
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::all(Val::Px(16.0)),
                row_gap: Val::Px(12.0),
                width: Val::Px(300.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.05, 0.1, 0.9)),
        ))
        .id();

    // Title
    commands.spawn((
        ChildOf(panel),
        Text::new("SELECT CLASS"),
        TextFont {
            font_size: 18.0,
            ..default()
        },
        TextColor(Color::srgba(0.9, 0.9, 0.9, 0.95)),
    ));

    // Interceptor button
    spawn_class_button(
        &mut commands,
        panel,
        ShipClass::Interceptor,
        "INTERCEPTOR",
        "Fast & agile. Autocannon + mines.",
        Color::srgb(0.2, 0.6, 0.3),
    );

    // Gunship button
    spawn_class_button(
        &mut commands,
        panel,
        ShipClass::Gunship,
        "GUNSHIP",
        "Tough & heavy. Heavy cannon + turrets.",
        Color::srgb(0.5, 0.3, 0.2),
    );

    // Torpedo Boat button
    spawn_class_button(
        &mut commands,
        panel,
        ShipClass::TorpedoBoat,
        "TORPEDO BOAT",
        "Laser + homing torpedoes. Tactical.",
        Color::srgb(0.2, 0.4, 0.6),
    );

    // Sniper button
    spawn_class_button(
        &mut commands,
        panel,
        ShipClass::Sniper,
        "SNIPER",
        "Railgun + mines + cloak. Stealth.",
        Color::srgb(0.4, 0.2, 0.5),
    );

    // Drone Commander button
    spawn_class_button(
        &mut commands,
        panel,
        ShipClass::DroneCommander,
        "DRONE COMMANDER",
        "Defense turrets + attack drones + pulse.",
        Color::srgb(0.3, 0.5, 0.3),
    );

    // Hint text
    commands.spawn((
        ChildOf(panel),
        Text::new("[Tab] to close"),
        TextFont {
            font_size: 11.0,
            ..default()
        },
        TextColor(Color::srgba(0.5, 0.5, 0.5, 0.7)),
    ));
}

fn spawn_class_button(
    commands: &mut Commands,
    parent: Entity,
    class: ShipClass,
    title: &str,
    desc: &str,
    color: Color,
) {
    let btn = commands
        .spawn((
            ChildOf(parent),
            ClassPickerButton(class),
            Button,
            Node {
                width: Val::Px(260.0),
                padding: UiRect::axes(Val::Px(12.0), Val::Px(8.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(2.0),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BorderColor::all(color),
            BackgroundColor(Color::srgba(0.1, 0.1, 0.15, 0.8)),
        ))
        .id();

    commands.spawn((
        ChildOf(btn),
        Text::new(title.to_string()),
        TextFont {
            font_size: 16.0,
            ..default()
        },
        TextColor(color),
    ));

    commands.spawn((
        ChildOf(btn),
        Text::new(desc.to_string()),
        TextFont {
            font_size: 11.0,
            ..default()
        },
        TextColor(Color::srgba(0.6, 0.6, 0.6, 0.8)),
    ));
}

/// Toggle class picker with Tab key.
fn class_picker_input(
    keypress: Res<ButtonInput<KeyCode>>,
    mut picker: ResMut<ClassPicker>,
    mut overlay: Query<&mut Visibility, With<ClassPickerOverlay>>,
    mut cursor_opts: Query<&mut CursorOptions>,
) {
    if keypress.just_pressed(KeyCode::Tab) {
        picker.open = !picker.open;
        if let Ok(mut vis) = overlay.single_mut() {
            *vis = if picker.open {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
        if let Ok(mut opts) = cursor_opts.single_mut() {
            opts.visible = picker.open;
        }
    }
}

/// Handle class picker button clicks via direct Interaction polling.
fn class_picker_click(
    mut picker: ResMut<ClassPicker>,
    buttons: Query<(&ClassPickerButton, &Interaction), With<Button>>,
    mut overlay: Query<&mut Visibility, With<ClassPickerOverlay>>,
    mut cursor_opts: Query<&mut CursorOptions>,
) {
    if !picker.open {
        return;
    }
    for (btn, interaction) in buttons.iter() {
        if *interaction == Interaction::Pressed {
            picker.pending_request = btn.0.to_request();
            picker.selected = btn.0;
            picker.open = false;
            if let Ok(mut vis) = overlay.single_mut() {
                *vis = Visibility::Hidden;
            }
            if let Ok(mut opts) = cursor_opts.single_mut() {
                opts.visible = false;
            }
            return;
        }
    }
}

/// Update the HUD class indicator when the selected class changes.
fn update_class_indicator(
    picker: Res<ClassPicker>,
    mut indicator_q: Query<(&mut Text, &mut TextColor), With<ClassIndicator>>,
) {
    if !picker.is_changed() {
        return;
    }
    let Ok((mut text, mut color)) = indicator_q.single_mut() else {
        return;
    };
    let (name, col) = match picker.selected {
        ShipClass::Interceptor => ("INTERCEPTOR", Color::srgb(0.2, 0.6, 0.3)),
        ShipClass::Gunship => ("GUNSHIP", Color::srgb(0.5, 0.3, 0.2)),
        ShipClass::TorpedoBoat => ("TORPEDO BOAT", Color::srgb(0.2, 0.4, 0.6)),
        ShipClass::Sniper => ("SNIPER", Color::srgb(0.4, 0.2, 0.5)),
        ShipClass::DroneCommander => ("DRONE CMD", Color::srgb(0.3, 0.5, 0.3)),
    };
    text.0 = format!("> {} <", name);
    *color = TextColor(col);
}

fn update_hud(
    ship_query: Query<(&Transform, &Health, &Fuel, &Ammo, &LinearVelocity), With<LocalShip>>,
    mut text_query: Query<&mut Text, With<HudText>>,
    mut health_bar: Query<&mut Node, HealthBarFilter>,
    mut fuel_bar: Query<&mut Node, FuelBarFilter>,
    mut ammo_bar: Query<&mut Node, AmmoBarFilter>,
) {
    let Ok((ship_tf, health, fuel, ammo, lin_vel)) = ship_query.single() else {
        return;
    };

    if let Ok(mut text) = text_query.single_mut() {
        let x = ship_tf.translation.x as i32;
        let y = ship_tf.translation.y as i32;
        let speed = lin_vel.0.length() as i32;
        **text = format!("SPD {speed} | ({x}, {y})");
    }

    if let Ok(mut node) = health_bar.single_mut() {
        node.width = Val::Percent(health.fraction() * 100.0);
    }

    if let Ok(mut node) = fuel_bar.single_mut() {
        node.width = Val::Percent(fuel.fraction() * 100.0);
    }

    if let Ok(mut node) = ammo_bar.single_mut() {
        node.width = Val::Percent(ammo.fraction() * 100.0);
    }
}

/// Rotate the local ship's gun barrel toward the mouse cursor.
fn update_gun_barrels(
    local_ship: Query<(Entity, &Transform, &ActionState<ShipInput>), With<LocalShip>>,
    mut barrels: Query<(&ChildOf, &mut Transform), GunBarrelFilter>,
) {
    let Ok((ship_entity, ship_tf, input)) = local_ship.single() else {
        return;
    };
    let (_, _, ship_angle) = ship_tf.rotation.to_euler(EulerRot::XYZ);
    let local_angle = input.0.aim_angle - ship_angle;

    for (child_of, mut barrel_tf) in barrels.iter_mut() {
        if child_of.0 == ship_entity {
            barrel_tf.rotation = Quat::from_rotation_z(local_angle);
        }
    }
}

/// Update turret barrel rotations from replicated Turrets component.
fn update_turret_barrels(
    ships: Query<(&Transform, &Turrets)>,
    mut barrels: Query<(&ChildOf, &TurretBarrel, &mut Transform), Without<Turrets>>,
) {
    for (child_of, turret_barrel, mut barrel_tf) in barrels.iter_mut() {
        let Ok((ship_tf, turrets)) = ships.get(child_of.0) else {
            continue;
        };
        let Some(state) = turrets.mounts.get(turret_barrel.0) else {
            continue;
        };
        // Convert world-space aim angle to local-space rotation
        let (_, _, ship_angle) = ship_tf.rotation.to_euler(EulerRot::XYZ);
        let local_angle = state.aim_angle - ship_angle;
        barrel_tf.rotation = Quat::from_rotation_z(local_angle);
    }
}

/// Camera follows the locally controlled ship.
fn camera_follow_local_ship(
    ship_query: Query<&Transform, With<LocalShip>>,
    mut camera_query: Query<&mut Transform, (With<Camera2d>, Without<LocalShip>)>,
    mut shake: ResMut<CameraShake>,
    time: Res<Time>,
) {
    let Ok(ship_transform) = ship_query.single() else {
        return;
    };
    let Ok(mut cam_transform) = camera_query.single_mut() else {
        return;
    };
    cam_transform.translation.x = ship_transform.translation.x;
    cam_transform.translation.y = ship_transform.translation.y;

    if shake.remaining > 0.0 {
        let dt = time.delta_secs();
        shake.remaining = (shake.remaining - dt).max(0.0);
        let t = shake.remaining / 0.2; // 1.0→0.0 as shake decays
        let mag = shake.intensity * t;
        let phase = time.elapsed_secs() * 65.0;
        cam_transform.translation.x += phase.sin() * mag;
        cam_transform.translation.y += (phase * 1.618).cos() * mag;
    }
}

/// Trigger camera shake when the local ship takes a hit.
fn shake_on_damage(
    local_ship: Query<Ref<DamageFlash>, With<LocalShip>>,
    mut shake: ResMut<CameraShake>,
) {
    for flash in local_ship.iter() {
        if flash.is_changed() && flash.timer > 0.05 {
            shake.intensity = 10.0;
            shake.remaining = 0.2;
        }
    }
}

// --- Camera zoom systems ---

/// Handle scroll wheel to adjust camera zoom level.
fn scroll_zoom(
    scroll: Res<AccumulatedMouseScroll>,
    mut zoom: ResMut<CameraZoom>,
    planner: Res<RoutePlanner>,
    mut camera_query: Query<&mut Projection, With<Camera2d>>,
) {
    // Don't apply scroll zoom while route planning (ctrl-zoom takes over)
    if planner.active {
        return;
    }

    let delta = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
    };

    if delta == 0.0 {
        return;
    }

    // Scroll up = zoom in (smaller scale), scroll down = zoom out (larger scale)
    zoom.scale = (zoom.scale - delta * ZOOM_SCROLL_STEP).clamp(ZOOM_MIN, ZOOM_MAX);

    let Ok(mut projection) = camera_query.single_mut() else {
        return;
    };
    if let Projection::Orthographic(ref mut ortho) = *projection {
        ortho.scale = zoom.scale;
    }
}

// --- Route planning systems ---

use btl_shared::{
    SHIP_AFTERBURNER_THRUST, SHIP_ANGULAR_DECEL, SHIP_MAX_ANGULAR_SPEED, SHIP_MAX_SPEED,
    SHIP_STABILIZE_DECEL,
};

/// Normalize angle to [-PI, PI].
fn wrap_angle(mut a: f32) -> f32 {
    while a > std::f32::consts::PI {
        a -= std::f32::consts::TAU;
    }
    while a < -std::f32::consts::PI {
        a += std::f32::consts::TAU;
    }
    a
}

/// Evaluate a Catmull-Rom spline through `points` at parameter `t` in [0, 1].
/// Centripetal Catmull-Rom spline: smoother curvature transitions at waypoints
/// compared to uniform Catmull-Rom. Uses alpha=0.5 (centripetal parameterization)
/// which avoids cusps and self-intersections.
fn catmull_rom_sample(points: &[Vec2], t: f32) -> Vec2 {
    let n = points.len();
    if n == 0 {
        return Vec2::ZERO;
    }
    if n == 1 {
        return points[0];
    }

    let t_scaled = t * (n - 1) as f32;
    let i = (t_scaled as usize).min(n - 2);
    let local_t = t_scaled - i as f32;

    let p0 = if i > 0 {
        points[i - 1]
    } else {
        2.0 * points[0] - points[1]
    };
    let p1 = points[i];
    let p2 = points[i + 1];
    let p3 = if i + 2 < n {
        points[i + 2]
    } else {
        2.0 * points[n - 1] - points[n - 2]
    };

    centripetal_catmull_rom(p0, p1, p2, p3, local_t)
}

/// Centripetal Catmull-Rom interpolation between p1 and p2 at parameter t in [0,1].
/// Alpha = 0.5 gives centripetal parameterization (best curvature continuity).
fn centripetal_catmull_rom(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
    fn knot_interval(a: Vec2, b: Vec2) -> f32 {
        (b - a).length().sqrt().max(0.001)
    }

    // Knot values: k0=0, k1, k2, k3 spaced by sqrt(chord length)
    let k1 = knot_interval(p0, p1);
    let k2 = k1 + knot_interval(p1, p2);
    let k3 = k2 + knot_interval(p2, p3);

    let u = k1 + t * (k2 - k1);

    // Barry-Goldman pyramid with explicit knot values
    let a1 = p0 * ((k1 - u) / k1) + p1 * (u / k1);
    let a2 = p1 * ((k2 - u) / (k2 - k1)) + p2 * ((u - k1) / (k2 - k1));
    let a3 = p2 * ((k3 - u) / (k3 - k2)) + p3 * ((u - k2) / (k3 - k2));

    let b1 = a1 * ((k2 - u) / k2) + a2 * (u / k2);
    let b2 = a2 * ((k3 - u) / (k3 - k1)) + a3 * ((u - k1) / (k3 - k1));

    b1 * ((k2 - u) / (k2 - k1)) + b2 * ((u - k1) / (k2 - k1))
}

/// Check if adding `candidate` as a new waypoint creates a turn that's too sharp.
/// Returns true if the angle is acceptable.
fn waypoint_angle_ok(waypoints: &[Vec2], candidate: Vec2) -> bool {
    let n = waypoints.len();
    if n < 2 {
        return true;
    } // need at least 2 existing points to measure an angle

    let prev = waypoints[n - 1];
    let prev2 = waypoints[n - 2];

    let seg_in = prev - prev2;
    let seg_out = candidate - prev;

    let len_in = seg_in.length();
    let len_out = seg_out.length();
    if len_in < 1.0 || len_out < 1.0 {
        return false;
    } // degenerate

    // Angle between the two segments (0 = straight ahead, PI = U-turn)
    let cos_angle = seg_in.dot(seg_out) / (len_in * len_out);
    let angle = cos_angle.clamp(-1.0, 1.0).acos(); // angle of deviation

    // Also check that the turn can be achieved given the segment length.
    // Min turning radius at cruise speed: R = v / omega_max
    let cruise_speed = SHIP_MAX_SPEED * 0.6;
    let r_min = cruise_speed / SHIP_MAX_ANGULAR_SPEED;
    // The arc length needed for a turn of `angle` at radius r_min
    let arc_needed = r_min * angle;
    // The shorter segment must be long enough to accommodate the arc
    let shorter_seg = len_in.min(len_out);

    angle <= (std::f32::consts::PI - MIN_WAYPOINT_ANGLE) && shorter_seg >= arc_needed * 0.5
}

/// Compute curvature at each sample point using the discrete Menger curvature formula.
fn compute_curvatures(path: &[Vec2]) -> Vec<f32> {
    let n = path.len();
    if n < 3 {
        return vec![0.0; n];
    }

    let mut curvatures = Vec::with_capacity(n);
    curvatures.push(0.0); // first point: no curvature
    for i in 1..n - 1 {
        let a = path[i - 1];
        let b = path[i];
        let c = path[i + 1];
        // Menger curvature: κ = 2 * |cross(ab, ac)| / (|ab| * |bc| * |ac|)
        let ab = b - a;
        let bc = c - b;
        let ac = c - a;
        let cross = ab.x * ac.y - ab.y * ac.x;
        let denom = ab.length() * bc.length() * ac.length();
        if denom > 0.001 {
            curvatures.push((2.0 * cross.abs()) / denom);
        } else {
            curvatures.push(0.0);
        }
    }
    curvatures.push(0.0); // last point
    curvatures
}

/// Compute a braking-aware speed profile for the path.
///
/// 1. Forward pass: at each point, cap speed by curvature and by how fast we can
///    accelerate from the previous point.
/// 2. Backward pass: ensure we can decelerate in time for every upcoming slow section.
///    Uses v² = v_next² + 2·a·Δs (kinematic braking equation).
/// 3. End of path: speed ramps to zero.
fn compute_speed_profile(curvatures: &[f32], arc_lengths: &[f32], cfg: &AutopilotConfig) -> Vec<f32> {
    let n = curvatures.len();
    if n == 0 {
        return vec![];
    }

    // Curvature-based max speed at each point
    // Smooth curvatures forward: use max curvature in a look-ahead window
    let mut max_curvature: Vec<f32> = vec![0.0; n];
    for i in 0..n {
        let end = (i + cfg.smooth_window).min(n);
        let mut peak = curvatures[i];
        for j in i..end {
            peak = peak.max(curvatures[j]);
        }
        max_curvature[i] = peak;
    }

    let mut profile: Vec<f32> = max_curvature
        .iter()
        .map(|&k| {
            let v_angular = if k > 0.001 {
                let margin = cfg.curvature_margin / (1.0 + k * cfg.curvature_divisor);
                (SHIP_MAX_ANGULAR_SPEED * margin / k).min(SHIP_MAX_SPEED * cfg.speed_cap)
            } else {
                SHIP_MAX_SPEED * cfg.speed_cap
            };
            // Centripetal-thrust limit: v = sqrt(centripetal_thrust / k).
            // Ensures the main thruster can supply enough centripetal acceleration on curves.
            if cfg.centripetal_thrust > 0.0 && k > 1e-6 {
                v_angular.min((cfg.centripetal_thrust / k).sqrt())
            } else {
                v_angular
            }
        })
        .collect();

    // Last point: must stop
    profile[n - 1] = 0.0;

    // Forward pass: can't exceed what we could accelerate to from previous point
    // v² = v_prev² + 2·a·Δs
    for i in 1..n {
        let ds = arc_lengths[i] - arc_lengths[i - 1];
        let v_max_from_accel = (profile[i - 1] * profile[i - 1] + 2.0 * cfg.accel * ds).sqrt();
        profile[i] = profile[i].min(v_max_from_accel);
    }

    // Backward pass: must be able to brake in time
    // v² = v_next² + 2·decel·Δs
    for i in (0..n - 1).rev() {
        let ds = arc_lengths[i + 1] - arc_lengths[i];
        let v_max_from_brake = (profile[i + 1] * profile[i + 1] + 2.0 * cfg.decel * ds).sqrt();
        profile[i] = profile[i].min(v_max_from_brake);
    }

    profile
}

/// Compute cumulative arc length at each path point. arc_lengths[0] = 0.
fn compute_arc_lengths(path: &[Vec2]) -> Vec<f32> {
    let mut lengths = Vec::with_capacity(path.len());
    lengths.push(0.0);
    for i in 1..path.len() {
        lengths.push(lengths[i - 1] + (path[i] - path[i - 1]).length());
    }
    lengths
}


fn rebuild_route_path(planner: &mut RoutePlanner) {
    planner.path.clear();
    planner.curvatures.clear();
    if planner.waypoints.len() >= 2 {
        let wps = &planner.waypoints;
        let mut path = Vec::with_capacity(ROUTE_SAMPLE_COUNT);
        for i in 0..ROUTE_SAMPLE_COUNT {
            let t = i as f32 / (ROUTE_SAMPLE_COUNT - 1) as f32;
            path.push(catmull_rom_sample(wps, t));
        }
        let curvatures = compute_curvatures(&path);
        planner.path = path;
        planner.curvatures = curvatures;
    }
}

/// Interpolate a position on the path at a fractional index.
fn path_lerp(path: &[Vec2], idx: f32) -> Vec2 {
    let i = (idx as usize).min(path.len().saturating_sub(2));
    let frac = idx - i as f32;
    path[i] + (path[(i + 1).min(path.len() - 1)] - path[i]) * frac
}

/// Find the closest point on the path to `pos`, starting search from `start_idx`.
/// Returns the fractional index.
fn find_closest_on_path(path: &[Vec2], pos: Vec2, start_idx: f32) -> f32 {
    let start = (start_idx as usize).saturating_sub(5);
    let end = (start + 60).min(path.len() - 1); // search wide window ahead
    let mut best_idx = start_idx;
    let mut best_dist = f32::MAX;

    for i in start..end {
        let a = path[i];
        let b = path[i + 1];
        let ab = b - a;
        let ab_len_sq = ab.length_squared();
        let t = if ab_len_sq > 0.001 {
            ((pos - a).dot(ab) / ab_len_sq).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let proj = a + ab * t;
        let d = (pos - proj).length_squared();
        if d < best_dist {
            best_dist = d;
            best_idx = i as f32 + t;
        }
    }
    best_idx
}

/// Compute remaining arc length from fractional index to end of path.
fn remaining_arc_length(path: &[Vec2], from_idx: f32) -> f32 {
    let i = (from_idx as usize).min(path.len().saturating_sub(2));
    let frac = from_idx - i as f32;
    let first_seg = path[(i + 1).min(path.len() - 1)] - path[i];
    let mut total = first_seg.length() * (1.0 - frac);
    for j in (i + 1)..path.len().saturating_sub(1) {
        total += (path[j + 1] - path[j]).length();
    }
    total
}

/// Handle CTRL press/release and mouse clicks for route planning.
fn route_planning_input(
    mut commands: Commands,
    mut planner: ResMut<RoutePlanner>,
    zoom: Res<CameraZoom>,
    keypress: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    ship_query: Query<(Entity, &Transform, Option<&ShipClass>), With<LocalShip>>,
    route_query: Query<Entity, With<RouteFollowing>>,
) {
    let ctrl_held =
        keypress.pressed(KeyCode::ControlLeft) || keypress.pressed(KeyCode::ControlRight);
    let ctrl_just_pressed =
        keypress.just_pressed(KeyCode::ControlLeft) || keypress.just_pressed(KeyCode::ControlRight);
    let ctrl_just_released = keypress.just_released(KeyCode::ControlLeft)
        || keypress.just_released(KeyCode::ControlRight);

    if ctrl_just_pressed {
        for entity in route_query.iter() {
            commands.entity(entity).remove::<RouteFollowing>();
        }
        planner.active = true;
        planner.waypoints.clear();
        planner.path.clear();
        planner.curvatures.clear();
        planner.last_rejected = false;
        planner.target_zoom = ROUTE_ZOOM_SCALE;

        // Placeholder start point (updated to actual position on release)
        if let Ok((_entity, ship_tf, _)) = ship_query.single() {
            planner.waypoints.push(ship_tf.translation.truncate());
        }
    }

    // Left-click adds waypoint (with angle validation)
    if planner.active
        && mouse_button.just_pressed(MouseButton::Left)
        && let Some(world_pos) = cursor_world_pos(&windows, &camera_query)
    {
        if waypoint_angle_ok(&planner.waypoints, world_pos) {
            planner.waypoints.push(world_pos);
            planner.last_rejected = false;
            rebuild_route_path(&mut planner);
        } else {
            planner.last_rejected = true;
        }
    }

    // Right-click removes last waypoint
    if planner.active
        && mouse_button.just_pressed(MouseButton::Right)
        && planner.waypoints.len() > 1
    {
        planner.waypoints.pop();
        planner.last_rejected = false;
        rebuild_route_path(&mut planner);
    }

    // On CTRL release, commit the route
    if ctrl_just_released && planner.active {
        planner.active = false;
        planner.target_zoom = zoom.scale;

        // Update start point to ship's current position (was placeholder from press)
        if let Ok((_entity, ship_tf, _)) = ship_query.single() {
            if !planner.waypoints.is_empty() {
                planner.waypoints[0] = ship_tf.translation.truncate();
            }
            rebuild_route_path(&mut planner);
        }

        if planner.path.len() >= 2
            && let Ok((entity, _ship_tf, class)) = ship_query.single()
        {
            let cfg = AutopilotConfig::for_class(class.copied().unwrap_or_default());
            let arc_lengths = compute_arc_lengths(&planner.path);
            let speed_profile = compute_speed_profile(&planner.curvatures, &arc_lengths, &cfg);

            commands.entity(entity).insert(RouteFollowing {
                path: planner.path.clone(),
                curvatures: planner.curvatures.clone(),
                speed_profile,
                config: cfg,
                progress: 0.0,
                debug_tick: 0,
            });
        }
        planner.waypoints.clear();
        planner.path.clear();
        planner.curvatures.clear();
    }

    if !ctrl_held && planner.active {
        planner.active = false;
        planner.target_zoom = zoom.scale;
    }
}

/// Smoothly animate camera zoom for route planning.
fn route_zoom(
    mut planner: ResMut<RoutePlanner>,
    zoom: Res<CameraZoom>,
    mut camera_query: Query<&mut Projection, With<Camera2d>>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    // When not route-planning, track the scroll zoom level
    if !planner.active {
        planner.target_zoom = zoom.scale;
    }
    planner.current_zoom +=
        (planner.target_zoom - planner.current_zoom) * (ROUTE_ZOOM_SPEED * dt).min(1.0);

    if (planner.current_zoom - planner.target_zoom).abs() < 0.001 {
        planner.current_zoom = planner.target_zoom;
    }

    if planner.current_zoom <= 0.01 {
        return;
    }

    let Ok(mut projection) = camera_query.single_mut() else {
        return;
    };
    if let Projection::Orthographic(ref mut ortho) = *projection {
        ortho.scale = planner.current_zoom;
    }
}

/// Draw the planned route using gizmos, with curvature-colored segments.
fn render_route_gizmos(
    planner: Res<RoutePlanner>,
    route_query: Query<&RouteFollowing>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    mut gizmos: Gizmos,
) {
    let (path, curvatures, is_planning) = if planner.active && planner.path.len() >= 2 {
        (&planner.path, &planner.curvatures, true)
    } else if let Ok(following) = route_query.single() {
        (&following.path, &following.curvatures, false)
    } else {
        return;
    };

    if path.len() < 2 {
        return;
    }

    // Max curvature the ship can handle at cruise speed: κ_max = ω_max / v
    let cruise_speed = SHIP_MAX_SPEED * 0.6;
    let max_curvature = SHIP_MAX_ANGULAR_SPEED / cruise_speed;

    for i in 0..path.len() - 1 {
        let k = if i < curvatures.len() {
            curvatures[i]
        } else {
            0.0
        };
        let ratio = (k / max_curvature).clamp(0.0, 1.0);

        // Green → yellow → red based on curvature tightness
        let color = if is_planning {
            Color::srgba(
                0.2 + 0.7 * ratio,
                0.8 - 0.5 * ratio,
                0.2 * (1.0 - ratio),
                0.6,
            )
        } else {
            Color::srgba(0.2, 0.5 + 0.3 * (1.0 - ratio), 0.8, 0.4)
        };
        gizmos.line_2d(path[i], path[i + 1], color);
    }

    // Draw waypoints as crosses while planning
    if is_planning {
        let wp_color = Color::srgba(0.9, 0.9, 0.3, 0.8);
        let scale = planner.current_zoom.max(1.0);
        for &wp in &planner.waypoints {
            let s = 8.0 * scale;
            gizmos.line_2d(wp - Vec2::X * s, wp + Vec2::X * s, wp_color);
            gizmos.line_2d(wp - Vec2::Y * s, wp + Vec2::Y * s, wp_color);
        }

        // Show rejection indicator: red X at cursor position
        if planner.last_rejected
            && let Some(cursor_world) = cursor_world_pos(&windows, &camera_query)
        {
            let s = 12.0 * scale;
            let red = Color::srgba(1.0, 0.2, 0.2, 0.8);
            gizmos.line_2d(
                cursor_world + Vec2::new(-s, -s),
                cursor_world + Vec2::new(s, s),
                red,
            );
            gizmos.line_2d(
                cursor_world + Vec2::new(-s, s),
                cursor_world + Vec2::new(s, -s),
                red,
            );
        }
    }
}

/// Compute the unit tangent direction of the path at a fractional index.
fn path_tangent(path: &[Vec2], idx: f32) -> Vec2 {
    let i = (idx as usize).min(path.len().saturating_sub(2));
    let next = (i + 1).min(path.len() - 1);
    let dir = path[next] - path[i];
    let len = dir.length();
    if len > 0.001 { dir / len } else { Vec2::Y }
}

/// Compute cross-track error: signed perpendicular distance from ship to path.
/// Positive = ship is to the LEFT of the path direction.
fn cross_track_error(path: &[Vec2], ship_pos: Vec2, progress: f32) -> f32 {
    let nearest = path_lerp(path, progress);
    let tangent = path_tangent(path, progress);
    // Normal = tangent rotated 90° CCW
    let normal = Vec2::new(-tangent.y, tangent.x);
    (ship_pos - nearest).dot(normal)
}

/// Holds stabilize=1.0 between test routes until ship has stopped.  Remove when tuning is done.
fn brake_between_routes(
    test_routes: Res<TestRoutes>,
    mut input_query: Query<&mut ActionState<ShipInput>, With<InputMarker<ShipInput>>>,
    route_query: Query<(), With<RouteFollowing>>,
    ship_query: Query<&LinearVelocity, With<LocalShip>>,
) {
    // Only brake when test is running and no route is active
    if test_routes.current == 0 || test_routes.current >= test_routes.routes.len() {
        return;
    }
    if route_query.single().is_ok() {
        return;
    }
    let speed = ship_query.single().map(|v| v.0.length()).unwrap_or(0.0);
    if speed < 5.0 {
        return;
    }
    for mut action_state in input_query.iter_mut() {
        action_state.0 = ShipInput {
            stabilize: 1.0,
            ..default()
        };
    }
}

/// Injects the next test route when ship has stopped.  Remove when tuning is done.
fn inject_test_route(
    mut commands: Commands,
    mut test_routes: ResMut<TestRoutes>,
    ship_query: Query<(Entity, &LinearVelocity, Option<&ShipClass>), With<LocalShip>>,
    route_query: Query<(), With<RouteFollowing>>,
) {
    if test_routes.current >= test_routes.routes.len() {
        return;
    }
    if route_query.single().is_ok() {
        return; // still running a route
    }
    let Ok((entity, lin_vel, class)) = ship_query.single() else {
        return;
    };
    // Wait for the configured class to spawn — the default Interceptor spawns first during
    // the class-switch round-trip; injecting on it would use the wrong config.
    let expected_class = ap_class_from_env();
    if class.copied().unwrap_or_default() != expected_class {
        return;
    }
    if lin_vel.0.length() > 30.0 {
        return; // wait until nearly stopped
    }

    let route_idx = test_routes.current;
    test_routes.current += 1;

    // Absolute waypoints — use directly, no offset needed.
    let waypoints: Vec<Vec2> = test_routes.routes[route_idx].clone();

    let mut path = Vec::with_capacity(ROUTE_SAMPLE_COUNT);
    for i in 0..ROUTE_SAMPLE_COUNT {
        let t = i as f32 / (ROUTE_SAMPLE_COUNT - 1) as f32;
        path.push(catmull_rom_sample(&waypoints, t));
    }
    let cfg = AutopilotConfig::for_class(class.copied().unwrap_or_default());
    let curvatures = compute_curvatures(&path);
    let arc_lengths = compute_arc_lengths(&path);
    let speed_profile = compute_speed_profile(&curvatures, &arc_lengths, &cfg);

    let total_arc = arc_lengths.last().copied().unwrap_or(0.0);
    let max_k = curvatures.iter().cloned().fold(0.0f32, f32::max);
    let min_spd = speed_profile.iter().cloned().fold(f32::MAX, f32::min);
    let max_spd = speed_profile.iter().cloned().fold(0.0f32, f32::max);
    eprint!("STARTING TEST ROUTE {} pts={} arc={:.0} max_k={:.5} spd={:.0}..{:.0} wps:",
        route_idx + 1, path.len(), total_arc, max_k, min_spd, max_spd);
    for wp in &waypoints {
        eprint!(" ({:.0},{:.0})", wp.x, wp.y);
    }
    eprintln!();

    commands.entity(entity).insert(RouteFollowing {
        path,
        curvatures,
        speed_profile,
        config: cfg,
        progress: 0.0,
        debug_tick: 0,
    });
}

/// Velocity-vector tracking autopilot: targets desired_vel = tangent × speed_profile + CTE correction.
fn route_follow(
    mut commands: Commands,
    mut ship_query: Query<
        (
            Entity,
            &Transform,
            &LinearVelocity,
            &AngularVelocity,
            &mut RouteFollowing,
        ),
        With<LocalShip>,
    >,
    mut input_query: Query<&mut ActionState<ShipInput>, With<InputMarker<ShipInput>>>,
    keypress: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
) {
    let Ok((entity, ship_tf, lin_vel, _ang_vel, mut following)) = ship_query.single_mut() else {
        return;
    };

    // Cancel on any movement key
    let manual_override = keypress.pressed(KeyCode::KeyW)
        || keypress.pressed(KeyCode::KeyS)
        || keypress.pressed(KeyCode::KeyA)
        || keypress.pressed(KeyCode::KeyD)
        || keypress.pressed(KeyCode::KeyQ)
        || keypress.pressed(KeyCode::KeyE);

    if manual_override {
        commands.entity(entity).remove::<RouteFollowing>();
        return;
    }

    let max_idx = (following.path.len() - 1) as f32;

    // End route when progress reaches the end
    if following.progress >= max_idx - 0.1 {
        commands.entity(entity).remove::<RouteFollowing>();
        for mut action_state in input_query.iter_mut() {
            action_state.0 = ShipInput {
                stabilize: 1.0,
                ..default()
            };
        }
        return;
    }

    let ship_pos = ship_tf.translation.truncate();
    let speed = lin_vel.0.length();

    // 1. Update progress (capped to prevent wild jumps on rollback)
    let proj = find_closest_on_path(&following.path, ship_pos, following.progress);
    let max_advance = (speed / 20.0).max(2.0);
    following.progress = proj
        .max(following.progress)
        .min(following.progress + max_advance);
    following.debug_tick += 1;
    let debug_tick = following.debug_tick;

    let progress = following.progress;
    let path = &following.path;

    let tangent_here = path_tangent(path, progress);
    let path_normal = Vec2::new(-tangent_here.y, tangent_here.x);
    let cte = cross_track_error(path, ship_pos, progress);

    // 2. Target speed from precomputed profile
    let speed_profile = &following.speed_profile;
    let idx_i = (progress as usize).min(speed_profile.len().saturating_sub(2));
    let idx_frac = progress - idx_i as f32;
    let target_speed_raw = speed_profile[idx_i]
        + idx_frac * (speed_profile[(idx_i + 1).min(speed_profile.len() - 1)] - speed_profile[idx_i]);

    // Build shared per-tick input struct
    let fwd_3d = ship_tf.rotation * Vec3::Y; // ship mesh Y+ = forward
    let ship_heading = fwd_3d.y.atan2(fwd_3d.x);
    let ship_fwd = Vec2::new(fwd_3d.x, fwd_3d.y);
    let ship_right_3d = ship_tf.rotation * Vec3::X;
    let ship_right = Vec2::new(ship_right_3d.x, ship_right_3d.y);
    let remaining = remaining_arc_length(path, progress);

    let ap_input = AutopilotInput {
        ship_fwd,
        ship_right,
        lin_vel: lin_vel.0,
        speed,
        current_omega: _ang_vel.0,
        path,
        progress,
        cte,
        tangent: tangent_here,
        path_normal,
        target_speed_raw,
        remaining,
    };

    let cfg = &following.config;
    let out = match cfg.algorithm {
        AutopilotAlgorithm::VelocityVector => ap_velocity_vector(&ap_input, cfg, ship_heading),
        AutopilotAlgorithm::ThrusterRotate => ap_thruster_rotate(&ap_input, cfg, ship_heading),
        AutopilotAlgorithm::SniperPath => ap_sniper_path(&ap_input, cfg, ship_heading),
    };

    // Aim angle from mouse cursor (weapons still track mouse during autopilot)
    let aim_angle = cursor_world_pos(&windows, &camera_query)
        .and_then(|world_pos| {
            let delta = world_pos - ship_pos;
            (delta.length_squared() > 1.0).then(|| delta.y.atan2(delta.x))
        })
        .unwrap_or(out.desired_angle);

    // Periodic debug logging (every 30 ticks) — remove when tuning is done
    if debug_tick % 30 == 0 {
        let path_pos = path_lerp(path, progress);
        eprintln!(
            "AP t={} pos=({:.0},{:.0}) pp=({:.0},{:.0}) cte={:.1} spd={:.1}/{:.1}(raw={:.1}) \
             hdg={:.3} fwd_err={:.1} lat_err={:.1} thr={:.2} stab={:.2} str={:.2} ab={} prg={:.1}",
            debug_tick,
            ship_pos.x, ship_pos.y, path_pos.x, path_pos.y,
            cte, speed, out.target_speed, target_speed_raw, out.heading_err,
            out.fwd_vel_error, out.lat_vel_error,
            out.thrust_forward, out.stabilize, out.strafe, out.afterburner as u8,
            progress,
        );
    }

    for mut action_state in input_query.iter_mut() {
        action_state.0 = ShipInput {
            thrust_forward: out.thrust_forward,
            thrust_backward: 0.0,
            rotate: out.rotate,
            strafe: out.strafe,
            afterburner: out.afterburner,
            stabilize: out.stabilize,
            fire: mouse_button.pressed(MouseButton::Left),
            drop_mine: keypress.just_pressed(KeyCode::KeyX),
            aim_angle,
            class_request: 0,
        };
    }
}

/// Velocity-vector tracking algorithm.
/// Computes a desired velocity = path_tangent × speed + correction, then drives toward it.
fn ap_velocity_vector(i: &AutopilotInput, cfg: &AutopilotConfig, ship_heading: f32) -> AutopilotOutput {
    // CTE speed reduction
    let cte_speed_factor = (1.0 / (1.0 + (i.cte.abs() / cfg.cte_divisor).powi(2))).max(cfg.cte_speed_floor);
    let target_speed = i.target_speed_raw * cte_speed_factor;

    // Desired velocity vector: tangent direction at target speed + lateral correction
    let correction_speed = (-i.cte * cfg.correction_gain).clamp(-cfg.correction_cap, cfg.correction_cap);
    let desired_vel = i.tangent * target_speed + i.path_normal * correction_speed;

    // Heading: face the desired velocity (fallback to tangent at near-zero speed)
    let desired_angle = if desired_vel.length_squared() > 100.0 {
        desired_vel.y.atan2(desired_vel.x)
    } else {
        i.tangent.y.atan2(i.tangent.x)
    };

    // Rotation: time-optimal with angular velocity damping
    let heading_err = wrap_angle(desired_angle - ship_heading);
    let omega_fb = heading_err.signum() * (2.0 * SHIP_ANGULAR_DECEL * heading_err.abs()).sqrt();
    let rotate = ((omega_fb - i.current_omega) / SHIP_MAX_ANGULAR_SPEED).clamp(-1.0, 1.0);

    // Velocity error decomposed into ship frame
    let vel_error = desired_vel - i.lin_vel;
    let fwd_vel_error = vel_error.dot(i.ship_fwd);
    let lat_vel_error = vel_error.dot(i.ship_right);

    // Thrust: gated on heading alignment and remaining distance
    let heading_factor = (1.0 - heading_err.abs() / std::f32::consts::FRAC_PI_3).clamp(0.0, 1.0);
    let stopping_dist = i.speed * i.speed / (2.0 * SHIP_STABILIZE_DECEL);
    let thrust_forward = if fwd_vel_error > 0.0 && i.remaining > stopping_dist * cfg.stopping_dist_margin {
        (fwd_vel_error / cfg.vel_error_scale).clamp(0.0, 1.0) * heading_factor
    } else {
        0.0
    };
    let afterburner = fwd_vel_error > cfg.afterburner_fwd_threshold
        && heading_factor > cfg.afterburner_heading_min
        && i.cte.abs() < cfg.afterburner_cte_max;

    // Braking: speed excess over desired magnitude
    let speed_excess = (i.speed - desired_vel.length()).max(0.0);
    let stabilize = (speed_excess / cfg.vel_error_scale).clamp(0.0, 1.0);

    // Strafe: lateral velocity error (negated — physics applies -ship_right * strafe)
    let strafe = -(lat_vel_error / cfg.vel_error_scale).clamp(-1.0, 1.0);

    AutopilotOutput {
        rotate,
        thrust_forward,
        stabilize,
        strafe,
        afterburner,
        desired_angle,
        target_speed,
        heading_err,
        fwd_vel_error,
        lat_vel_error,
    }
}

/// Rotation-first thruster algorithm.
///
/// Two modes selected per-tick:
/// - Main-thrust mode (curves / acceleration): rotate to face delta_v (centripetal + correction),
///   fire main thruster. Strafe is off (ship faces centripetally, strafe would act along tangent).
/// - Tangent mode (near-straights / deceleration): face la_tangent, strafe corrects CTE.
///   Triggered when delta_v is backward or negligible (no meaningful rotation needed).
fn ap_thruster_rotate(i: &AutopilotInput, cfg: &AutopilotConfig, ship_heading: f32) -> AutopilotOutput {
    let cte_speed_factor = (1.0 / (1.0 + (i.cte.abs() / cfg.cte_divisor).powi(2))).max(cfg.cte_speed_floor);
    let target_speed = i.target_speed_raw * cte_speed_factor;

    // Look-ahead tangent: advance along the path so the ship preemptively rotates into curves
    // before it arrives at them rather than reacting after already going off-track.
    let look_ahead_dist = (i.speed * cfg.look_ahead_time).clamp(cfg.look_ahead_min, cfg.look_ahead_max);
    let local_step = if (i.progress as usize + 1) < i.path.len() {
        (i.path[i.progress as usize + 1] - i.path[i.progress as usize]).length().max(1.0)
    } else {
        1.0
    };
    let la_progress = (i.progress + look_ahead_dist / local_step).min((i.path.len() - 1) as f32);
    let la_tangent = path_tangent(i.path, la_progress);

    // Desired velocity: la_tangent at target speed.
    // la_tangent at the look-ahead is fully rotated into the curve; using it gives ~2× the
    // centripetal component in delta_v vs to_la_norm (which is only the chord angle).
    // More centripetal in delta_v → main thruster does the curve work, less strafe needed.
    let desired_vel = la_tangent * target_speed;
    let delta_v = desired_vel - i.lin_vel;

    // Heading: face delta_v so the main thruster delivers centripetal + speed corrections.
    // Clamp deviation from la_tangent to ±60°: this prevents the pathological heading when
    // speed≈target but direction changes slightly (e.g. descent after apex — delta_v becomes
    // nearly perpendicular to path → atan2 gives ~-97°), while still allowing large correction
    // angles on curves where delta_v correctly points far from the tangent.
    let desired_angle = if delta_v.length_squared() > 5.0 * 5.0 && delta_v.dot(la_tangent) >= 0.0 {
        let tangent_angle = la_tangent.y.atan2(la_tangent.x);
        let raw_angle = delta_v.y.atan2(delta_v.x);
        let deviation = wrap_angle(raw_angle - tangent_angle);
        tangent_angle + deviation.clamp(-std::f32::consts::FRAC_PI_3, std::f32::consts::FRAC_PI_3)
    } else {
        la_tangent.y.atan2(la_tangent.x)
    };

    // Rotation: bang-bang for large errors (full speed), proportional near target (no overshoot).
    // K_p = ANGULAR_DECEL/4 = 5 gives critically-damped response for |err| < crossover.
    // For |err| >= crossover = MAX_SPEED/K_p = 6/5 = 1.2 rad, just saturate at max omega.
    let heading_err = wrap_angle(desired_angle - ship_heading);
    let k_p = SHIP_ANGULAR_DECEL / 4.0; // 5.0
    let crossover = SHIP_MAX_ANGULAR_SPEED / k_p; // 1.2 rad
    let omega_fb = if heading_err.abs() >= crossover {
        heading_err.signum() * SHIP_MAX_ANGULAR_SPEED
    } else {
        heading_err * k_p
    };
    let rotate = ((omega_fb - i.current_omega) / SHIP_MAX_ANGULAR_SPEED).clamp(-1.0, 1.0);

    // Heading alignment gate
    let heading_factor = (1.0 - heading_err.abs() / std::f32::consts::FRAC_PI_3).clamp(0.0, 1.0);

    // Thrust: fires when there's a velocity deficit in the forward direction.
    // No heading_factor gate — the ship fires thrust while rotating so the correction
    // force is applied immediately, not only after the rotation completes.
    let fwd_delta_v = delta_v.dot(i.ship_fwd);
    let stopping_dist = i.speed * i.speed / (2.0 * SHIP_STABILIZE_DECEL);
    let thrust_forward = if fwd_delta_v > 0.0 && i.remaining > stopping_dist * cfg.stopping_dist_margin {
        (fwd_delta_v / cfg.vel_error_scale).clamp(0.0, 1.0)
    } else {
        0.0
    };
    // Only afterburn when below target speed — prevents overspeed on curve approaches where
    // delta_v can have a large forward component even when total speed already exceeds target.
    let afterburner = fwd_delta_v > cfg.afterburner_fwd_threshold
        && heading_factor > cfg.afterburner_heading_min
        && i.cte.abs() < cfg.afterburner_cte_max
        && i.speed < target_speed;

    // Stabilize: scalar speed excess gated by heading alignment.
    // Suppressed while rotating to a new heading — firing retro-thrust while rotating would
    // counteract the correction the rotation is setting up to deliver.
    let stabilize = ((i.speed - target_speed).max(0.0) / cfg.vel_error_scale * heading_factor)
        .clamp(0.0, 1.0);

    // Strafe: CTE correction + lateral velocity damping.
    // On curves with CTE≈0, strafe≈0 — centripetal force comes from the main thruster.
    let lat_vel_path = i.lin_vel.dot(i.path_normal);
    let cte_cmd = cfg.correction_gain * i.cte + cfg.correction_kd * lat_vel_path;
    let strafe = -(cte_cmd / cfg.vel_error_scale).clamp(-1.0, 1.0);

    let vel_error = desired_vel - i.lin_vel;
    let fwd_vel_error = vel_error.dot(i.ship_fwd);
    let lat_vel_error = vel_error.dot(i.ship_right);

    AutopilotOutput {
        rotate,
        thrust_forward,
        stabilize,
        strafe,
        afterburner,
        desired_angle,
        target_speed,
        heading_err,
        fwd_vel_error,
        lat_vel_error,
    }
}

/// Analytic path-tracking algorithm for the Sniper.
///
/// Design:
///   ROTATION — driven by the future path *tangent* at a look-ahead point chosen so that
///   the ship has exactly enough time to complete the rotation before arriving there.
///   `look_ahead_time` is an early-start margin (e.g. 2.5 = start rotating when you still
///   have 2.5× the minimum required time left), so the ship pre-rotates well before curves.
///
///   THRUST — gated on heading alignment AND on low path curvature ahead. The idea is to
///   enter curves already at the right heading and let momentum carry through; thrust only
///   on straight segments where it can efficiently accelerate the ship.
///
///   STRAFE — tiny lateral-velocity damping only; CTE is corrected by rotating toward the
///   look-ahead position rather than strafing the ship sideways.
fn ap_sniper_path(i: &AutopilotInput, cfg: &AutopilotConfig, ship_heading: f32) -> AutopilotOutput {
    let cte_speed_factor =
        (1.0 / (1.0 + (i.cte.abs() / cfg.cte_divisor).powi(2))).max(cfg.cte_speed_floor);
    let target_speed = i.target_speed_raw * cte_speed_factor;

    let max_idx = (i.path.len() - 1) as f32;
    let local_step = if (i.progress as usize + 1) < i.path.len() {
        (i.path[i.progress as usize + 1] - i.path[i.progress as usize])
            .length()
            .max(1.0)
    } else {
        1.0
    };

    // Reconstruct ship's actual world position from nearest path point + CTE offset.
    let nearest_pos = path_lerp(i.path, i.progress);
    let ship_pos = nearest_pos + i.path_normal * i.cte;

    // --- Analytic look-ahead ---
    //
    // Scan [look_ahead_min, look_ahead_max] for each candidate la_prog:
    //   rotation_angle = angle from current heading to path *tangent* at la_prog
    //   rotation_time  = 2√(|Δθ| / SHIP_ANGULAR_DECEL)   (time-optimal bang-bang)
    //   travel_time    = la_dist / speed
    //
    // Apply early-start margin (cfg.look_ahead_time): trigger when
    //   rotation_time * margin ≥ travel_time
    // so the ship begins rotating well before it's strictly necessary.
    // Keep the furthest triggering point → pre-solves the most demanding upcoming curve.
    //
    // The rotation TARGET is the path tangent at la_prog (future heading geometry).
    // The thrust TARGET is the direction toward la_pos from ship (CTE correction).
    let min_la_prog = (i.progress + cfg.look_ahead_min / local_step).min(max_idx);
    let min_la_tangent = path_tangent(i.path, min_la_prog);
    let mut rot_target_angle = min_la_tangent.y.atan2(min_la_tangent.x);
    let mut best_la_prog = min_la_prog;

    let margin = cfg.look_ahead_time.max(1.0);
    const SCAN_STEPS: usize = 24;
    for k in 0..SCAN_STEPS {
        let frac = (k + 1) as f32 / SCAN_STEPS as f32;
        let la_dist = cfg.look_ahead_min + frac * (cfg.look_ahead_max - cfg.look_ahead_min);
        let la_prog = (i.progress + la_dist / local_step).min(max_idx);
        let la_tangent = path_tangent(i.path, la_prog);
        let la_angle = la_tangent.y.atan2(la_tangent.x);

        let heading_delta = wrap_angle(la_angle - ship_heading).abs();
        let rot_time = 2.0 * (heading_delta / SHIP_ANGULAR_DECEL).sqrt();
        let travel_time = la_dist / i.speed.max(10.0);

        // Trigger early (margin > 1) so rotation starts with time to spare.
        if rot_time * margin >= travel_time {
            rot_target_angle = la_angle;
            best_la_prog = la_prog;
        }
    }

    // la_pos: the geometric look-ahead position on the path (used for thrust direction).
    let la_pos = path_lerp(i.path, best_la_prog);
    let to_la_vec = la_pos - ship_pos;
    let to_la_angle = if to_la_vec.length_squared() > 1.0 {
        to_la_vec.y.atan2(to_la_vec.x)
    } else {
        rot_target_angle
    };

    // When far from the path the tangent-based rot_target diverges from the direction
    // needed to actually push the ship back. Blend smoothly: off-path → face la_pos
    // so the thruster fires; on-path → face future tangent for pre-rotation.
    let on_path_factor = (1.0 - (i.cte.abs() / 250.0)).clamp(0.0, 1.0);
    let blended_angle = {
        let diff = wrap_angle(rot_target_angle - to_la_angle);
        to_la_angle + diff * on_path_factor
    };

    // Rotation: time-optimal with angular velocity damping, targeting blended angle.
    let heading_err = wrap_angle(blended_angle - ship_heading);
    let omega_fb = heading_err.signum() * (2.0 * SHIP_ANGULAR_DECEL * heading_err.abs()).sqrt();
    let rotate = ((omega_fb - i.current_omega) / SHIP_MAX_ANGULAR_SPEED).clamp(-1.0, 1.0);

    // Heading alignment gate (relative to blended target heading).
    let heading_factor =
        (1.0 - heading_err.abs() / std::f32::consts::FRAC_PI_3).clamp(0.0, 1.0);

    // Curvature-based thrust gate: measure heading change between current tangent and
    // la_tangent to detect upcoming curves. High upcoming curvature → reduce thrust so
    // the ship enters curves on pre-built momentum rather than fighting with the thruster.
    // Only apply when on-path (off-path, the ship needs full thrust to rejoin).
    let current_tangent_angle = i.tangent.y.atan2(i.tangent.x);
    let la_tangent_at_best = path_tangent(i.path, best_la_prog);
    let la_tangent_angle = la_tangent_at_best.y.atan2(la_tangent_at_best.x);
    let upcoming_turn = wrap_angle(la_tangent_angle - current_tangent_angle).abs();
    let curve_thrust_factor = on_path_factor
        * (1.0 - (upcoming_turn - 0.26) / 0.52).clamp(0.0, 1.0)
        + (1.0 - on_path_factor); // off-path: always allow full thrust

    // Thrust: forward component toward la_pos, gated by heading + curve + distance.
    let to_la = to_la_vec.normalize_or_zero();
    let desired_vel = to_la * target_speed;
    let delta_v = desired_vel - i.lin_vel;
    let fwd_delta_v = delta_v.dot(i.ship_fwd);
    let stopping_dist = i.speed * i.speed / (2.0 * SHIP_STABILIZE_DECEL);
    let thrust_forward =
        if fwd_delta_v > 0.0 && i.remaining > stopping_dist * cfg.stopping_dist_margin {
            (fwd_delta_v / cfg.vel_error_scale).clamp(0.0, 1.0) * heading_factor * curve_thrust_factor
        } else {
            0.0
        };
    let afterburner = fwd_delta_v > cfg.afterburner_fwd_threshold
        && heading_factor > cfg.afterburner_heading_min
        && curve_thrust_factor > 0.8
        && i.cte.abs() < cfg.afterburner_cte_max;

    // Stabilize: total speed excess, decoupled from heading.
    let stabilize = ((i.speed - target_speed).max(0.0) / cfg.vel_error_scale).clamp(0.0, 1.0);

    // Strafe: tiny lateral velocity damping only.
    let lat_vel_path = i.lin_vel.dot(i.path_normal);
    let strafe = -(cfg.correction_kd * lat_vel_path / cfg.correction_cap).clamp(-1.0, 1.0);

    // Diagnostics
    let fwd_vel_error = delta_v.dot(i.ship_fwd);
    let lat_vel_error = delta_v.dot(i.ship_right);

    AutopilotOutput {
        rotate,
        thrust_forward,
        stabilize,
        strafe,
        afterburner,
        desired_angle: blended_angle,
        target_speed,
        heading_err,
        fwd_vel_error,
        lat_vel_error,
    }
}
