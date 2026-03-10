# BTL — Implementation Plan

## Phase 1: Foundation ✅

### 1.1 Project Structure ✅

- [x] 4-crate workspace: `btl-protocol`, `btl-shared`, `btl-client`, `btl-server`
- [x] Shared lib: components, protocol (ShipInput, replicated types), physics, game constants
- [x] Client binary: rendering, input handling, prediction, UI
- [x] Server binary: authoritative simulation, no rendering
- [x] WASM support via Trunk (feature-gated `native` vs browser builds)

### 1.2 Lightyear Networking Skeleton ✅

- [x] Lightyear protocol: `ShipInput` with thrust, rotate, strafe, afterburner, stabilize
- [x] Server: UDP listen, accept connections, assign client IDs
- [x] Client: connect to server, send inputs, receive replicated state
- [x] Prediction on local ship (PredictionTarget), interpolation on remote ships (InterpolationTarget)
- [x] Rollback thresholds on Position (≥2.0), Rotation (≥0.1), LinearVelocity (≥2.0), AngularVelocity (≥0.5)
- [x] WebTransport support for browser clients (cert hash flow)

### 1.3 Ship Movement with Physics ✅

- [x] Ship entity: `RigidBody::Dynamic`, zero damping, Avian2D collider
- [x] Input: W/S thrust, A/D rotate, Q/E strafe, Shift afterburner, R stabilize
- [x] Control scheme: **keyboard = thrusters** (movement), **mouse = weapons** (aim + fire)
- [x] Server-authoritative physics: forces applied from client inputs
- [x] Client-side prediction with rollback correction
- [x] Interpolation: remote ships render smoothly
- [x] Ship-ship collision with restitution (0.8)
- [x] Speed cap (600) and angular speed cap (6.0)
- [x] Stabilize system: deceleration using fixed thruster nozzle positions

---

## Phase 2: World ✅

### 2.1 Map

- [x] Circular boundary with soft zone (progressive drag + hard edge reflection at MAP_RADIUS=6000)
- [x] Visual boundary ring (720 dim red markers)
- [x] Static asteroid obstacles: 80 asteroids, 4 size variants (20/50/100/200), deterministic seed
  - `RigidBody::Static` with circle colliders, replicated from server
  - 7-sided polygon meshes with brownish-gray color variation
  - Uniform area distribution, min 800 from center
- [x] Tridrant sector markings: 3 dotted lines from center to boundary (120° apart)
- [x] Objective zone circles: 3 zones at 60% MAP_RADIUS, 300-unit radius, yellow dotted rings
- [x] Camera follows local player's ship

### 2.2 2.5D Visuals

- [x] Parallax background: 4 star layers (parallax 0.05/0.15/0.3/0.5), infinite wrapping, 400 stars
- [x] Ship rendering: procedural interceptor mesh (6-vertex needle hull), team-colored, `FrameInterpolate`
- [x] Gun barrel: child sprite rotating toward mouse cursor (aim direction visualization)
- [x] Asteroid meshes: 7-sided polygons with per-entity color variation
- [x] Procedural nebula background: seeded expression-tree VM (R/G/B channels), 128×128 animated texture, slow ~9min cycle
- [x] Minimap overlay (bottom-right corner)
  - Map boundary circle, objective zone circles, ship dots (team-colored)
  - Camera viewport rectangle indicator
  - Nebula background layer
  - Clipped to minimap bounds
- [ ] Per-class ship sprites: replace procedural mesh with proper sprites/meshes per class (deferred to Phase 5)
- [ ] Basic lighting/shadow pass (deferred to Phase 5)

### 2.3 HUD

- [x] Health bar (HP) with red fill, bottom-left
- [x] Fuel gauge (FU) with blue fill, afterburner consumption/regen system
- [x] Speed indicator + coordinate display
- [x] Ammo indicator bar (AM) with fill, per-weapon
- [ ] Team color indicators on HUD and ship labels
- [ ] Kill feed / event log (deferred until Phase 3)

**Phase 2 is complete when:** Players fly around a circular map with asteroids, parallax background, minimap, and a basic HUD showing health/fuel/speed.

---

## Phase 3: Combat

### 3.1 Weapon System Framework ✅

- [x] Projectile entity: replicated Position, LinearVelocity, lifetime timer, damage value, owner ID
- [x] Hit detection: circle-circle overlap (server-authoritative, no physics engine)
- [x] Damage application: reduce HP, clamp to 0
- [x] Ship destruction at 0 HP: despawn + queue for respawn (3s timer)
- [x] Respawn system: server-side queue, respawn at team-based positions
- [x] Projectile lifetime: despawn after max time (shared system)
- [x] Muzzle offset: spawn projectiles at ship edge, not center
- [x] Fire cooldown: shared generic tick-down system, server-authoritative firing
- [x] Client rendering: HDR team-colored projectile circles (bloom-compatible)
- [x] VFX: muzzle flash (HDR sprite, 0.04s), impact sparks (5-point burst), mine drop flash, mine detonation (central flash + expanding ring + debris)
- [x] Mine visuals: circle mesh + shadow, rotation + bob animation, pulsing while active
- [ ] Drifting wreck entity on destruction (deferred)
- [ ] Damage flash: brief white flash on ship when hit

### 3.2 Ship Classes

Implement one at a time, in this order:

1. **Interceptor** ✅ — autocannon + mines + afterburner. Simplest weapons, good baseline.
   - [x] Autocannon: rapid-fire (8 rounds/s), low damage, medium range
   - [x] Mines: drop behind ship (X key), proximity detonation (60u radius), 30s lifetime, max 5 active, 1s arm time
   - [x] Standard afterburner (with fuel system)
2. **Gunship** — heavy cannon (player) + 3 auto-turrets. Introduces autonomous targeting AI.
   - Heavy cannon: slow fire rate (1.5 rounds/s), high damage, long range, player-aimed
   - Auto-turrets: 3 turrets with independent targeting, medium fire rate, low damage, limited range
   - Turret AI: target nearest enemy in range, imperfect tracking (slight aim lag)
3. **Torpedo Boat** — continuous laser + tracking torpedoes. Introduces homing projectiles.
   - Laser: continuous beam, low DPS, instant hit (raycast), medium range
   - Torpedoes: lock-on after 1s aim, slow speed, homing with limited turn radius, heavy damage, 8s lifetime
   - Torpedo countermeasures: can be shot down, detonated by Powerplant shield
4. **Sniper** — charge-up railgun + proximity mines + cloak. Introduces stealth mechanic.
   - Railgun: 2s charge-up (visible glow), massive damage, infinite range (raycast), 5s cooldown
   - Proximity mines: same as Interceptor mines
   - Cloak: toggle, 8s duration, 15s cooldown, broken by firing (not by damage), render as faint shimmer for enemies
5. **Drone Commander** — defense lasers + attack drones + anti-drone pulse. Most complex: drone AI, friendly fire on own drones.
   - Defense lasers: 5 auto-targeting, weak, medium range, can hit own drones (friendly fire)
   - Attack drones: 7 small AI-controlled entities, swarm behavior, low HP, respawn slowly (1 per 8s, faster at objectives)
   - Anti-drone pulse: AoE ability, destroys ALL nearby drones (friend and foe), 20s cooldown

### 3.3 Route Planning & Autopilot ✅

- [x] Ctrl-click waypoint placement with visual path preview (gizmos)
- [x] Catmull-Rom spline interpolation between waypoints
- [x] Curvature-based speed profile: auto-brakes for tight turns, accelerates on straights
- [x] Path coloring: green (fast) → yellow → red (slow) based on curvature
- [x] 60° minimum turn angle validation (rejects hairpin turns with red X indicator)
- [x] Proportional autopilot: throttle, rotation, strafe, and stabilize from path following
- [x] Cross-track error correction via strafing (PID-style lateral control)
- [x] Arc-length parameterization for accurate distance-based progress
- [x] Camera zoom-out animation during planning mode (4× zoom)
- [x] Manual override: any movement key cancels autopilot instantly
- [x] Mouse weapons still active during autopilot (aim + fire while following route)

### 3.4 Ship Selection

- Pre-spawn class selection UI overlay
- Team assignment (red/blue) — server assigns to balance teams
- Class selection persists across respawns until changed
- Spawn at captured objective (or default spawn point if none captured)

**Phase 3 is complete when:** All 5 ship classes are playable with distinct weapons, ships can destroy each other, wrecks drift and fade.

---

## Phase 4: Objectives

### 4.1 Capture Zones

- 3 circular zones, one per tridrant, at ~60% MAP_RADIUS along tridrant bisector
- Zone radius: ~300 units (tunable)
- Capture logic:
  - Count ships per team inside zone each tick
  - Net presence = (team_a_count - team_b_count), sign determines capture direction
  - Capture progress: 0.0 (neutral) to 1.0 (captured), rate scales with net ship count
  - Multiple ships accelerate capture (diminishing returns: 1x, 1.5x, 1.8x, 2.0x for 1–4 ships)
  - Contested (equal ships) = progress frozen
- Visual indicator: zone ring color shifts (neutral gray → team color), fill opacity shows progress
- Replicate capture state (owner team, progress) to all clients

### 4.2 Objective Defenses

Defenses activate when objective is captured, deactivate when neutral/enemy-owned.

1. **Factory** — spawns 11 defense drones (4 explosive, 7 laser)
   - Explosive drones: kamikaze AI, patrol zone, charge enemies on detection, AoE damage on impact
   - Laser drones: orbit zone, fire at nearest enemy in range, low HP
   - Drones respawn slowly while Factory is held (1 per 10s until full)
   - Drones despawn if Factory is lost
2. **Railgun** — auto-targeting turret entity at zone center
   - Targets nearest enemy in range (~1500 units)
   - Telegraph: tracks target for 2s, then stops tracking for 0.5s (telegraph), then fires
   - High damage, narrow beam, long cooldown (8s)
   - Skilled players read the tracking-stop and dodge
3. **Powerplant** — energy shield bubble (~400 unit radius)
   - Deflects projectiles (bullets bounce off at reflection angle)
   - Weakens lasers (50% damage reduction through shield)
   - Detonates torpedoes on contact with shield (AoE at shield surface, not at target)
   - Allied ships inside are protected; enemies must breach to fight effectively

### 4.3 Objective Benefits

- All captured objectives:
  - Repair nearby allied ships (slow HP regen when within zone, ~5 HP/s)
  - Resupply ammo (refill rate 2x passive rate)
  - Resupply fuel (refill rate 2x passive rate)
  - Resupply drones (Drone Commander gets 1 drone per 4s instead of 8s)
- Respawn at random captured objective (if multiple held, random selection)

### 4.4 Win Condition

- Team with 0 objectives loses → round over
- Round restart: reset all entity positions, reset objective capture states to neutral, reshuffle teams
- Edge case: team loses last point → cannot respawn → round ends after last ship destroyed (or immediate end)
- Victory screen: show stats (kills, captures, damage dealt), 10s countdown to next round
- Match structure: best of N rounds (configurable, default 3)

**Phase 4 is complete when:** Full game loop works — capture objectives, benefit from them, lose all three and the round resets.

---

## Phase 5: Game Feel

### 5.1 Fog of War

- Each ship has a sensor range (~800 units)
- Minimap only shows entities within allied sensor range (union of all allied ships' ranges)
- Cloaked Sniper: appears as inaccurate shimmer on minimap (position offset by random ±100 units, updates slowly)
- Objective zones always visible (known fixed locations)
- Ships outside sensor range: not rendered on minimap, but still rendered in game view if on screen (minimap fog only)

### 5.2 Visual Effects

- [x] Thruster particles: HDR color gradient, bloom, velocity inheritance, halo/ember variants, directional cones per input
- [x] Afterburner flare: enhanced thruster particles at higher intensity
- [x] Muzzle flash: bright HDR sprite (0.04s) on projectile spawn
- [x] Impact sparks: 5-point yellow burst at projectile hit point
- [x] Mine placement flash: white HDR flash on deploy
- [x] Mine detonation: central flash + 16-particle expanding ring + 8 debris particles
- [x] Mine pulsing glow while active (rotation + bob animation)
- [ ] Beam glow (for lasers), torpedo trail (smoke particles)
- [ ] Ship destruction explosion: expanding ring + debris particles
- [ ] Shield impact: ripple effect at impact point on Powerplant shield
- [ ] Cloak shimmer: distortion/transparency effect on cloaked Sniper
- [ ] Damage flash: brief white flash on ship when hit

### 5.3 Audio

- Engine hum: looping sound, pitch shifts with velocity magnitude
- Weapon sounds: distinct per weapon type (autocannon rattle, cannon boom, laser hum, torpedo whoosh, railgun charge+crack)
- Explosion: ship destruction, torpedo detonation, mine detonation (varying intensity)
- Ambient space: low background drone
- Capture progress: rising tone while capturing, alert tone when zone flips
- Shield impact: energy crackle
- UI sounds: selection clicks, respawn chime

### 5.4 Collision Polish

- Ship-to-ship: both take damage proportional to relative impact velocity, faster ship takes 1.2x more
- Ship-to-asteroid: damage based on impact velocity (threshold: no damage below 100 velocity)
- Screen shake: intensity scales with impact force, short duration (0.1–0.3s)
- Impact particles: sparks at collision point

**Phase 5 is complete when:** The game feels good to play — responsive, readable, satisfying feedback on every action.

---

## Phase 6: Polish and Infrastructure

### 6.1 Lobby System

- Server browser: query running servers, show player count, ping
- Or simple matchmaking: queue → auto-assign to available server
- Team balancing on join (assign to smaller team)
- Ship class selection in lobby (changeable until round starts)
- Ready-up system: round starts when all players ready (or timeout)

### 6.2 Balance Pass

- Tune all weapon damage, fire rates, projectile speeds, ranges
- Tune capture speeds, objective defense strengths, zone radii
- Tune fuel/ammo consumption and regen rates (passive vs at-objective)
- Tune drone counts, drone AI aggression, drone HP
- Tune ship HP, mass, thrust values, speed caps
- Tune objective defense parameters (railgun cooldown, shield strength, drone count)
- Playtest-driven iteration with metrics logging

### 6.3 Server Infrastructure

- Dedicated server binary packaging (standalone, no GPU required)
- Containerization (Docker) for deployment
- Multiple server region support
- Server tick rate tuning (default 60Hz)
- Connection quality monitoring and adaptive send rates

### 6.4 Anti-cheat

- Server-authoritative validation (already built-in via architecture)
- Input sanity checks: rate limiting, impossible input combinations, input timing validation
- Position reconciliation bounds: reject client predictions that diverge too far from server state
- Kick/ban system for repeated violations

---

## Current Progress

```text
Phase 1: Foundation ✅ ─── project structure → networking → ship movement
Phase 2: World ✅ ─────── asteroids ✅ → tridrants ✅ → minimap ✅ → HUD ✅ → nebula ✅
Phase 3: Combat 🔧 ─────── Interceptor ✅ → route planner ✅ → VFX ✅ → 4 classes → selection
Phase 4: Objectives ❌ ─── capture zones → defenses → benefits → win condition
Phase 5: Game Feel 🔧 ─── particles ✅ → combat VFX ✅ → fog/audio ❌ → collision polish ❌
Phase 6: Polish ❌ ─────── lobby → balance → infrastructure → anti-cheat
```

### What's done

#### Foundation & Networking

- 4-crate workspace: Bevy 0.18, Lightyear 0.26, Avian2D 0.5
- Server-authoritative physics with client-side prediction and interpolation
- Rollback thresholds on Position/Rotation/Velocity to prevent jinking
- WASM/browser support via Trunk + WebTransport (Chrome/Edge)

#### Flight & Physics

- Full Newtonian flight model: thrust, strafe, rotate, afterburner (with fuel), stabilize
- Ship-ship and ship-asteroid collision with restitution (0.8)
- Circular map boundary (radius 6000) with soft zone + hard reflection
- Speed cap (600), angular speed cap (6.0)

#### World & Visuals

- 80 deterministic asteroids (4 size variants, 7-sided polygon meshes, seeded layout)
- 3 tridrant sector lines + 3 objective zone circles
- Parallax starfield (4 layers, 400 stars, infinite wrapping)
- Procedural nebula background (seeded expression-tree VM, 128×128 animated texture)
- Interceptor hull mesh (6-vertex needle shape) with gun barrel child sprite

#### HUD & UI

- Health, fuel, ammo bars (bottom-left) with live fill updates
- Speed indicator + coordinate display
- Minimap (bottom-right): boundary, zones, ship dots, viewport rect, nebula layer

#### Combat (Interceptor class)

- Autocannon: 8 rounds/s, server-authoritative spawn, HDR team-colored projectiles
- Mines: X key, proximity detonation (60u), 30s lifetime, 1s arm time, max 5 active
- Hit detection: circle-circle overlap (server-only), damage application, HP clamping
- Ship destruction at 0 HP with 3s respawn timer (server-side queue)

#### VFX

- HDR thruster particles: core/halo/ember variants, directional cones, afterburner flare, fuel sputter
- Muzzle flash, impact sparks, mine drop flash, mine detonation (flash + ring + debris)
- Mine visuals: mesh + shadow, rotation/bob animation

#### Route Planning & Autopilot

- Ctrl-click waypoint placement with Catmull-Rom spline path
- Curvature-aware speed profile with color-coded preview (green→red)
- Proportional autopilot with cross-track correction, manual override on any key
- Camera zoom-out during planning, weapons still active while following

### Next up (Phase 3: Combat, continued)

1. Gunship class: heavy cannon + 3 auto-turrets with independent targeting AI
2. Torpedo Boat class: continuous laser + homing torpedoes
3. Sniper class: charge-up railgun + proximity mines + cloak
4. Drone Commander class: defense lasers + attack drones + anti-drone pulse
5. Ship class selection UI (pre-spawn overlay)

Each phase builds on the previous. Phases are playable milestones — after each one, the game is testable and demonstrable at that level of completeness.
