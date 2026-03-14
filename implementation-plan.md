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
- [x] Native client `--cert` flag for connecting to remote servers with self-signed certs

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

### 2.1 Map ✅

- [x] Circular boundary with soft zone (progressive drag + hard edge reflection at MAP_RADIUS=10000)
- [x] Visual boundary ring (720 dim red markers)
- [x] Static asteroid obstacles: 80 asteroids, 4 size variants (20/50/100/200), deterministic seed
  - `RigidBody::Static` with circle colliders, replicated from server
  - 7-sided polygon meshes with brownish-gray color variation
  - Uniform area distribution, min 800 from center
- [x] Tridrant sector markings: 3 dotted lines from center to boundary (120° apart)
- [x] Objective zone circles: 3 zones at 60% MAP_RADIUS, 300-unit radius, yellow dotted rings
- [x] Camera follows local player's ship

### 2.2 2.5D Visuals ✅

- [x] Parallax background: 4 star layers (parallax 0.05/0.15/0.3/0.5), infinite wrapping, 400 stars
- [x] Per-class procedural ship meshes: Interceptor (elongated hexagon), Gunship (wide octagon), TorpedoBoat (submarine wedge), Sniper (slim needle with fins), DroneCommander (wide hex carrier)
- [x] Gun barrel / turret barrels: child sprites rotating toward aim direction, per-class mount positions
- [x] Asteroid meshes: 7-sided polygons with per-entity color variation
- [x] Procedural nebula background: seeded expression-tree VM (R/G/B channels), 128×128 animated texture, slow ~9min cycle
- [x] Minimap overlay (bottom-right corner)
  - Map boundary circle, objective zone circles, ship dots (team-colored)
  - Camera viewport rectangle indicator
  - Nebula background layer
  - Clipped to minimap bounds
- [ ] Basic lighting/shadow pass

### 2.3 HUD ✅

- [x] Health bar (HP) with red fill, bottom-left
- [x] Fuel gauge (FU) with blue fill, afterburner consumption/regen system
- [x] Speed indicator + coordinate display
- [x] Ammo indicator bar (AM) with fill, per-weapon
- [x] Score display: red/blue scores with progress bars, top-center
- [x] Victory overlay: team-colored win announcement with dark backdrop
- [ ] Team color indicators on HUD and ship labels
- [ ] Kill feed / event log

---

## Phase 3: Combat ✅

### 3.1 Weapon System Framework ✅

- [x] Projectile entity: replicated Position, LinearVelocity, lifetime timer, damage value, owner ID
- [x] Hit detection: circle-circle overlap (server-authoritative, no physics engine)
- [x] Damage application: reduce HP, clamp to 0
- [x] Ship destruction at 0 HP: despawn + queue for respawn (3s timer)
- [x] Respawn system: server-side queue, respawn near team-controlled zones (or random fallback)
- [x] Projectile lifetime: despawn after max time (shared system)
- [x] Muzzle offset: spawn projectiles at ship edge, not center
- [x] Fire cooldown: shared generic tick-down system, server-authoritative firing
- [x] Client rendering: velocity-aligned rectangular projectiles with per-kind sizing/color
- [x] Kill attribution tracking (LastDamagedBy component)
- [ ] Drifting wreck entity on destruction
- [ ] Damage flash: brief white flash on ship when hit

### 3.2 Ship Classes ✅

All 5 classes implemented with unique meshes, stats, and weapon systems:

1. **Interceptor** ✅ — autocannon + mines + afterburner
   - [x] Autocannon: 8 rounds/s, 8 damage, 800 speed, 3.2s lifetime
   - [x] Mines: X key, proximity detonation (60u radius), 40 damage, 30s lifetime, max 5 active, 1s arm time
   - [x] Standard afterburner (with fuel system)
2. **Gunship** ✅ — heavy cannon + 3 auto-turrets
   - [x] Heavy cannon: 1.5 rounds/s, 35 damage, 600 speed, player-aimed
   - [x] 3 auto-turrets: independent targeting AI, 3 rounds/s each, 5 damage, 1600u range
   - [x] Turret AI: slew-rate limited tracking (4 rad/s), fire within 0.15 rad tolerance
   - [x] Larger hull (radius 22, mass 18, 150 HP)
3. **Torpedo Boat** ✅ — continuous laser + homing torpedoes
   - [x] Laser: continuous beam, 20 DPS, raycast with distance falloff (100%→30%), 1280u range
   - [x] Laser visuals: segmented core + glow, stops at first obstacle (asteroid/ship)
   - [x] Torpedoes: homing (0.8 rad/s turn rate), 70 damage, 110 speed, 32s lifetime, max 3 active
   - [x] Torpedo shootdown: projectiles can intercept torpedoes
   - [x] Torpedo plume particles (tan exhaust trail)
4. **Sniper** ✅ — charge-up railgun + mines + cloak
   - [x] Railgun: 2s charge, 120 damage at full, 3500 speed projectile, 5s cooldown
   - [x] Railgun charge glow visual (bright circle scaling with charge)
   - [x] Proximity mines (same as Interceptor)
   - [x] Cloak: 8s duration, 15s cooldown, broken by firing
   - [x] Cloak visuals: own ship 40% opacity, allies 50%, enemies shimmer (8% with sine wave)
   - [x] Glass cannon stats: 70 HP, highest mobility (220 thrust, 550 afterburner)
5. **Drone Commander** ✅ — defense turrets + attack drones + anti-drone pulse
   - [x] 5 defense turrets: auto-targeting, 3 damage, 1200u range, 5 rad/s slew
   - [x] 7 attack drones (4 laser, 3 kamikaze): swarm AI with aggro range (1920u)
   - [x] Laser drones: erratic burst fire, 15 DPS, orbit commander
   - [x] Kamikaze drones: 40 damage on impact, 600 speed charge
   - [x] Anti-drone pulse: destroys all drones in 400u radius, each deals 25 AoE damage, 20s cooldown
   - [x] Drone respawn: 8s timer, respawns most-depleted type
   - [x] Drone visuals: team-tinted triangles (laser) and octagons (kamikaze)
   - [x] Pulse ready indicator (subtle green circle)

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

### 3.4 Ship Selection ✅

- [x] Tab-key class picker overlay with 5 buttons (name + description + color border)
- [x] Server handles class switch: despawns old ship, respawns at same position with new class
- [x] Team assignment: server assigns to team with fewer players on connect

---

## Phase 4: Objectives 🔧

### 4.1 Capture Zones — Partial ✅

Zone control is implemented but simplified from the original design:

- [x] 3 circular zones at 60% MAP_RADIUS, 300-unit radius
- [x] Zone control: server counts ships per team in each zone each tick
- [x] Majority-based control (more ships = team controls zone, equal = contested)
- [x] Replicated TeamScores with per-zone control state
- [x] King-of-the-hill scoring: 1 point/sec per controlled zone, first to 100 wins
- [x] Victory state: RoundState::Won(team), client shows victory overlay
- [x] Gradual capture progress (0.0→1.0) with diminishing returns for multiple ships
- [x] Visual zone ring color shift (neutral gray → team color) with progress fill
- [x] Passive decap (unattended zones drift to neutral at DECAP_RATE=0.02/s)

**Design note:** Current implementation uses instant majority-based control + score-to-100, which differs from the design doc's "team with 0 objectives loses" win condition. Both could coexist (score win + elimination win). Decide during playtesting which feels better.

### 4.2 Objective Defenses ❌

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

### 4.3 Objective Benefits ❌

- All captured objectives:
  - Repair nearby allied ships (slow HP regen when within zone, ~5 HP/s)
  - Resupply ammo (refill rate 2x passive rate)
  - Resupply fuel (refill rate 2x passive rate)
  - Resupply drones (Drone Commander gets 1 drone per 4s instead of 8s)
- Respawn at random captured objective (if multiple held, random selection)

### 4.4 Round Management ❌

- [ ] Round restart: reset all entity positions, reset zone control to neutral
- [ ] Team reshuffle between rounds
- [ ] Spawn protection (2–3s invulnerability after respawn)
- [ ] Victory screen with stats (kills, captures, damage dealt), countdown to next round
- [ ] Match structure: best of N rounds (configurable, default 3)
- [ ] Last-stand defense bonus (strengthened defenses on team's final objective)

**Phase 4 is complete when:** Full game loop works — capture objectives, benefit from them, round resets on victory.

---

## Phase 5: Game Feel

### 5.1 Fog of War ❌

- Each ship has a sensor range (~800 units)
- Minimap only shows entities within allied sensor range (union of all allied ships' ranges)
- Cloaked Sniper: appears as inaccurate shimmer on minimap (position offset by random ±100 units, updates slowly)
- Objective zones always visible (known fixed locations)
- Ships outside sensor range: not rendered on minimap, but still rendered in game view if on screen (minimap fog only)

### 5.2 Visual Effects 🔧

- [x] Thruster particles: HDR color gradient, bloom, velocity inheritance, halo/ember variants, directional cones per input
- [x] Afterburner flare: enhanced thruster particles at higher intensity, fuel sputter when low
- [x] RCS thruster cones: mesh triangles + glow per nozzle, activation-scaled
- [x] Muzzle flash: bright HDR sprite (0.04s) on projectile spawn
- [x] Impact sparks: 5-point yellow burst at projectile hit point
- [x] Mine placement flash: white HDR flash on deploy
- [x] Mine detonation: central flash + 16-particle expanding ring + 8 debris particles
- [x] Mine pulsing glow while active (rotation + bob animation)
- [x] Ship destruction explosion: large flash + secondary flash + 24 fireball particles + 12 team-colored hull chunks + 16 hot sparks
- [x] Drone destruction: cyan flash + 10 fast sparks + 5 hot debris
- [x] Laser beam rendering: segmented core + glow, distance fade, obstacle blocking
- [x] Drone laser beams: thin team-colored lines from drone to target
- [x] Torpedo exhaust plume: 200 particles/s, tan color
- [x] Railgun charge glow: bright circle scaling with charge progress
- [x] Cloak visuals: own ship 40% opacity, allies 50%, enemies shimmer (8% + sine wave)
- [x] Pulse ready indicator: subtle green circle around Drone Commander
- [ ] Heavy cannon smoke trail (~0.3s orange trail behind projectile)
- [ ] Torpedo lock-on indicator (flashing red diamond on targeted ship)
- [ ] Shield impact ripple effect (Powerplant shield — depends on Phase 4.2)
- [ ] Damage flash: brief white flash on ship when hit
- [ ] Railgun beam effect: instantaneous thick line across screen (~0.15s fade)

### 5.3 Audio ❌

- Engine hum: looping sound, pitch shifts with velocity magnitude
- Weapon sounds: distinct per weapon type (autocannon rattle, cannon boom, laser hum, torpedo whoosh, railgun charge+crack)
- Explosion: ship destruction, torpedo detonation, mine detonation (varying intensity)
- Ambient space: low background drone
- Capture progress: rising tone while capturing, alert tone when zone flips
- Shield impact: energy crackle
- UI sounds: selection clicks, respawn chime

### 5.4 Collision Polish ❌

- Ship-to-ship: both take damage proportional to relative impact velocity, faster ship takes 1.2x more
- Ship-to-asteroid: damage based on impact velocity (threshold: no damage below 100 velocity)
- Screen shake: intensity scales with impact force, short duration (0.1–0.3s)
- Impact particles: sparks at collision point

---

## Phase 6: Polish and Infrastructure

### 6.1 Lobby System ❌

- Server browser: query running servers, show player count, ping
- Or simple matchmaking: queue → auto-assign to available server
- Team balancing on join (assign to smaller team)
- Ship class selection in lobby (changeable until round starts)
- Ready-up system: round starts when all players ready (or timeout)

### 6.2 Balance Pass ❌

- Tune all weapon damage, fire rates, projectile speeds, ranges
- Tune capture speeds, objective defense strengths, zone radii
- Tune fuel/ammo consumption and regen rates (passive vs at-objective)
- Tune drone counts, drone AI aggression, drone HP
- Tune ship HP, mass, thrust values, speed caps
- Tune objective defense parameters (railgun cooldown, shield strength, drone count)
- Playtest-driven iteration with metrics logging

### 6.3 Server Infrastructure 🔧

- [x] Dedicated headless server binary (MinimalPlugins + ScheduleRunnerPlugin, no GPU)
- [x] Deployed to DigitalOcean VPS (178.128.206.71)
- [x] WebTransport with self-signed certs (server prints cert hash on startup)
- [ ] Containerization (Docker)
- [ ] Multiple server region support
- [ ] Connection quality monitoring and adaptive send rates

### 6.4 Anti-cheat ❌

- Server-authoritative validation (already built-in via architecture)
- Input sanity checks: rate limiting, impossible input combinations, input timing validation
- Position reconciliation bounds: reject client predictions that diverge too far from server state
- Kick/ban system for repeated violations

---

## Current Progress

```text
Phase 1: Foundation ✅ ─── project structure → networking → ship movement
Phase 2: World ✅ ─────── asteroids ✅ → tridrants ✅ → minimap ✅ → HUD ✅ → nebula ✅
Phase 3: Combat ✅ ─────── all 5 classes ✅ → route planner ✅ → VFX ✅ → class selection ✅
Phase 4: Objectives 🔧 ── zone control ✅ → scoring ✅ → defenses 🔧 → benefits 🔧 → rounds ❌
Phase 5: Game Feel 🔧 ── particles ✅ → combat VFX ✅ → fog ❌ → audio ❌ → collision polish ❌
Phase 6: Polish 🔧 ────── headless server ✅ → deployment ✅ → lobby ❌ → balance ❌
```

### Design Decisions to Revisit

These items differ between the design doc and the current implementation. They need a deliberate decision during playtesting:

1. **Win condition**: Design says "team with 0 objectives loses"; implementation uses score-to-100 king-of-the-hill. Could support both (score win + elimination win).
2. **Capture mechanic**: Design says constant rate regardless of ship count; implementation uses instant majority-based control. Need gradual progress for more strategic depth.
3. **Railgun projectile vs raycast**: Design says instant-hit raycast; implementation fires a very fast projectile (3500 speed). Projectile approach may be better for networking (visible travel, dodgeable at extreme range).
4. **Torpedo lock-on**: Design says 1s lock-on before launch; implementation fires immediately. Lock-on would add counterplay (warning indicator on target).
5. **Capturing breaks Sniper cloak**: Design doc specifies this; not implemented yet. Adds interesting tradeoff.

### Next Up

1. **Phase 4.1**: Gradual capture progress + visual feedback (zone ring color shifts)
2. **Phase 4.2**: Objective defenses (Factory, Railgun turret, Powerplant shield) — the most ambitious remaining feature
3. **Phase 4.3**: Zone benefits (repair/resupply/respawn)
4. **Phase 4.4**: Round management (restart, stats, countdown)
5. **Phase 5**: Damage flash, collision damage, fog of war, audio

Each phase builds on the previous. Phases are playable milestones — after each one, the game is testable and demonstrable at that level of completeness.
