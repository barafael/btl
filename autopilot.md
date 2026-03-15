# Autopilot System

The autopilot follows player-drawn routes using a three-stage pipeline:
**path generation** (Catmull-Rom spline from waypoints),
**speed profiling** (curvature-aware braking), and
**per-tick control** (one of three algorithms selected per ship class).

All code lives in `crates/btl-client/src/client.rs`.

## Route Planning

The player holds **Ctrl** and clicks waypoints on the map. Each click is validated
against a minimum turning angle (based on the ship's turning radius at cruise speed).
On Ctrl release the waypoints are interpolated into a 128-sample centripetal
Catmull-Rom spline (`catmull_rom_sample`). Centripetal parameterization (alpha = 0.5)
avoids cusps and self-intersections.

A `RouteFollowing` component is attached to the ship, carrying the path, curvatures,
speed profile, and an `AutopilotConfig` for the ship's class.

## Speed Profile

Computed once at route injection (`compute_speed_profile`):

1. **Smoothed curvature**: for each of the 128 path points, take the max curvature
   in a forward window of `smooth_window` samples.
2. **Curvature speed limit**: `SHIP_MAX_ANGULAR_SPEED * margin / k`, where
   `margin = curvature_margin / (1 + k * curvature_divisor)`. Capped at
   `speed_cap * SHIP_MAX_SPEED`.
3. **Centripetal thrust limit** (TorpedoBoat only): `sqrt(centripetal_thrust / k)` —
   ensures the main thruster can supply enough centripetal acceleration.
4. **End-of-route stop**: last point speed = 0.
5. **Forward kinematic pass**: speed can't exceed what's reachable by accelerating
   from the previous point (`v² = v_prev² + 2·a·Δs`).
6. **Backward kinematic pass**: must be able to brake in time for every upcoming
   slow section (`v² = v_next² + 2·decel·Δs`).

## Per-Tick Control Loop (`route_follow`)

Every fixed-update tick:

1. **Update progress**: project the ship position onto the path
   (`find_closest_on_path`), capped to prevent wild jumps on netcode rollback.
2. **Compute cross-track error** (CTE): signed perpendicular distance from ship to
   path. Positive = ship is left of path direction.
3. **Interpolate target speed** from the precomputed profile at current progress.
4. **Assemble `AutopilotInput`** (ship frame vectors, velocity, CTE, tangent, etc.)
   and dispatch to the class-specific algorithm.
5. **Write `ShipInput`**: thrust, rotation, strafe, stabilize, afterburner. Weapons
   still track the mouse cursor independently.

Manual override (any WASD/QE key) instantly cancels the route.

## Algorithms

### VelocityVector (Interceptor, Gunship, DroneCommander)

Default algorithm. Computes a **desired velocity vector** = path tangent × target
speed + lateral CTE correction, then drives toward it.

- **CTE speed reduction**: `1 / (1 + (|cte| / cte_divisor)²)`, floored at
  `cte_speed_floor`. Slows the ship when off-track.
- **Heading**: face the desired velocity vector.
- **Rotation**: time-optimal bang-bang with angular velocity damping
  (`ω_fb = sign(err) · √(2 · α · |err|)`, then `rotate = (ω_fb - ω) / ω_max`).
- **Thrust**: proportional to forward velocity error, gated by heading alignment
  (suppressed when ship isn't facing forward) and remaining distance to route end.
- **Stabilize**: proportional to speed excess over desired magnitude.
- **Strafe**: proportional to lateral velocity error (drives ship sideways to
  correct CTE).
- **Afterburner**: fires when forward deficit is large, heading is aligned, and CTE
  is low.

### ThrusterRotate (TorpedoBoat)

Rotation-first algorithm designed for the slow-turning TorpedoBoat. Two modes
per tick:

- **Main-thrust mode** (curves / acceleration): rotate to face `Δv` (the difference
  between desired and actual velocity), fire main thruster. The desired velocity uses
  a **look-ahead tangent** so the ship preemptively rotates into curves. Heading is
  clamped to ±60° from the look-ahead tangent to prevent pathological angles when
  speed ≈ target.
- **Tangent mode** (near-straights / deceleration): face the look-ahead tangent,
  strafe corrects CTE via a PD loop (`correction_gain * cte + correction_kd *
  lateral_velocity`).

Rotation uses a proportional controller with crossover to bang-bang at large errors
(`K_p = ANGULAR_DECEL / 4`).

Stabilize is gated by heading alignment — suppressed while rotating so retro-thrust
doesn't fight the correction the rotation is setting up.

### SniperPath (Sniper)

Analytic path-tracking algorithm. Key idea: **scan ahead** to find when to start
rotating so the ship arrives at curves already facing the right direction.

- **Look-ahead scan**: 24 candidates from `look_ahead_min` to `look_ahead_max`.
  For each, compute rotation time (time-optimal bang-bang: `2√(|Δθ| / α)`) and
  travel time (`distance / speed`). Trigger when `rotation_time * margin ≥
  travel_time`. Keep the furthest triggering point.
- **Heading blend**: when on-path, face the future path tangent (pre-rotation);
  when off-path, blend toward the look-ahead position so thrust pushes the ship
  back.
- **Curve thrust gate**: measures upcoming heading change between current and
  look-ahead tangents. High upcoming curvature → reduce thrust so the ship enters
  curves on pre-built momentum.
- **Strafe**: minimal lateral velocity damping only — CTE correction comes from
  rotation, not strafing.

## Per-Class Configuration

Each ship class gets an `AutopilotConfig` from `AutopilotConfig::for_class()`:

| Parameter | Interceptor | TorpedoBoat | Sniper |
|-----------|-------------|-------------|--------|
| Algorithm | VelocityVector | ThrusterRotate | SniperPath |
| `curvature_margin` | 0.32 | 0.28 | 0.32 |
| `curvature_divisor` | 180 | 180 | 180 |
| `speed_cap` | 0.78 | 0.75 | 0.82 |
| `cte_divisor` | 60 | 70 | 80 |
| `cte_speed_floor` | 0.40 | 0.40 | 0.35 |
| `smooth_window` | 25 | 40 | 25 |
| `correction_gain` | 0.5 | 0.5 | 0.4 (unused) |
| `correction_kd` | 0.0 | 0.8 | 0.4 |

Gunship and DroneCommander use the same config as Interceptor (VelocityVector)
with their own afterburner thrust values.

## Gizmo Visualization

`render_route_gizmos` draws the active or planned route using Bevy gizmos:

- **Planning mode**: segments colored green → yellow → red by curvature ratio.
  Waypoints shown as yellow crosses. Invalid waypoint placement shown as a red X.
- **Execution mode**: muted blue segments (lighter = less curvature).

## Key Design Decisions

- **Velocity-vector tracking** replaced an earlier Pure Pursuit + PID approach.
  Pursuit had trouble with CTE correction fighting heading control.
- **Centripetal Catmull-Rom** (alpha = 0.5) instead of uniform — eliminates cusps
  at sharp waypoint transitions.
- **Progress cap** (`min(progress + max_advance)`) prevents wild jumps when the
  ship is far from the path (e.g., after netcode rollback).
- **CTE speed reduction uses inverse-square** rather than linear — gentle at small
  errors, aggressive at large errors, with a hard floor to prevent crawling.
- **CTE look-ahead boost** (looking further ahead when off-track, NOT closer)
  gives smoother rejoin arcs. The inverse (squeezing look-ahead) caused violent
  oscillation.
