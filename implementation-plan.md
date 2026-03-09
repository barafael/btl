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
- [x] Server-authoritative physics: forces applied from client inputs
- [x] Client-side prediction with rollback correction
- [x] Interpolation: remote ships render smoothly
- [x] Ship-ship collision with restitution (0.8)
- [x] Speed cap (600) and angular speed cap (6.0)
- [x] Stabilize system: deceleration using fixed thruster nozzle positions

---

## Phase 2: World (in progress)

### 2.1 Map

- [x] Circular boundary with soft zone (progressive drag + hard edge reflection at MAP_RADIUS=3000)
- [x] Visual boundary ring (360 dim red markers)
- [ ] Static asteroid obstacles: `RigidBody::Static` colliders, various sizes
  - Scatter 50–100 asteroids across the map
  - 3–5 size variants (small 20px, medium 50px, large 100px, huge 200px)
  - Random rotation for visual variety
  - Replicated so both client and server agree on placement
  - Server seeds asteroid positions deterministically from a fixed seed
- [ ] Divide map into 3 tridrants (120° sectors), mark objective zone positions
  - Visual sector lines or subtle coloring to hint at tridrant boundaries
  - Place objective zone centers at ~60% of MAP_RADIUS along each tridrant's bisector
- [x] Camera follows local player's ship

### 2.2 2.5D Visuals

- [x] Parallax background: 4 star layers (parallax 0.05/0.15/0.3/0.5), infinite wrapping, 400 stars
- [x] Ship rendering: Mesh2d hexagons with team color, rotation via `FrameInterpolate`
- [ ] Ship sprites: replace placeholder hexagons with proper ship sprites/meshes
  - Distinct silhouette per ship class (can start with Interceptor-only)
  - Team color tinting (red/blue)
  - Rotation-aligned rendering
- [ ] Asteroid sprites/meshes
  - Irregular polygon shapes or sprite assets
  - Slight color variation per asteroid
- [ ] Basic lighting/shadow pass
  - Ambient glow around ships
  - Subtle shadow under ships on asteroid surfaces
- [ ] Minimap overlay (no fog of war yet)
  - Small corner overlay showing full map
  - Dots for ships (team-colored), circles for objective zones
  - Camera viewport rectangle indicator
  - Asteroid silhouettes (optional, may be too noisy)

### 2.3 HUD

- [x] Coordinate display (bottom center)
- [ ] Ship health bar
  - Replicate HP component from server
  - Visual bar under ship or in HUD corner
- [ ] Fuel gauge (for afterburner)
  - Replicate fuel component, show consumption/regen
- [ ] Ammo indicator (per weapon, deferred until Phase 3)
- [ ] Afterburner heat/cooldown indicator
- [ ] Team color indicators (red/blue) on HUD and ship labels
- [ ] Speed indicator (current velocity magnitude)
- [ ] Kill feed / event log (deferred until Phase 3)

**Phase 2 is complete when:** Players fly around a circular map with asteroids, parallax background, minimap, and a basic HUD showing health/fuel/speed.

---

## Phase 3: Combat

### 3.1 Weapon System Framework

- Projectile entity: replicated Position, LinearVelocity, lifetime timer, damage value, owner ID
- Projectile collider: small circle sensor, collision events with ships
- Hit detection: Avian2D collision events between projectile sensors and ship colliders
- Damage application: reduce HP, clamp to 0
- Ship destruction at 0 HP:
  - Despawn ship entity
  - Spawn drifting wreck entity (inherits velocity, 10s lifetime, fading alpha)
  - Trigger respawn timer (5s), respawn at captured objective or default spawn
- Projectile lifetime: despawn after max range/time
- Muzzle offset: spawn projectiles at ship edge, not center (avoid self-collision)

### 3.2 Ship Classes

Implement one at a time, in this order:

1. **Interceptor** — autocannon + mines + afterburner. Simplest weapons, good baseline.
   - Autocannon: rapid-fire (8 rounds/s), low damage, medium range, slight spread
   - Mines: drop behind ship (inherit ship velocity minus small backward offset), proximity detonation, 30s lifetime, max 5 active
   - Standard afterburner
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

### 3.3 Ship Selection

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
- [ ] Weapon fire effects: muzzle flash (bright HDR sprite, 0.05s), beam glow (for lasers), torpedo trail (smoke particles)
- [ ] Explosions: expanding ring + debris particles on ship destruction and torpedo detonation
- [ ] Shield impact: ripple effect at impact point on Powerplant shield
- [ ] Mine placement: brief flash on deploy, pulsing glow while active
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

```
Phase 1: Foundation ✅ ─── project structure → networking → ship movement
Phase 2: World 🔧 ─────── map (boundary ✅, asteroids ❌) → visuals (starfield ✅, sprites ❌) → HUD (coords ✅, rest ❌)
Phase 3: Combat ❌ ─────── weapons framework → 5 ship classes → selection
Phase 4: Objectives ❌ ─── capture zones → defenses → benefits → win condition
Phase 5: Game Feel 🔧 ─── thruster particles ✅ → fog/VFX/audio ❌ → collision polish ❌
Phase 6: Polish ❌ ─────── lobby → balance → infrastructure → anti-cheat
```

### What's done

- 4-crate workspace with Bevy 0.18, Lightyear 0.26, Avian2D 0.5
- Full Newtonian flight model: thrust, strafe, rotate, afterburner, stabilize
- Server-authoritative networking with client prediction and interpolation
- Rollback thresholds to prevent jinking on low-latency connections
- Ship-ship collision with restitution
- Parallax starfield (4 layers, infinite wrapping)
- HDR particle system (thruster flames with bloom, halos, embers)
- Circular map boundary with soft zone + reflection
- Coordinate HUD
- WASM/browser proof of concept via Trunk + WebTransport

### Next up (Phase 2 remaining)

1. Asteroid obstacles (static rigid bodies, various sizes)
2. Tridrant sector markings and objective zone positions
3. Ship sprites (replace placeholder hexagons)
4. Minimap overlay
5. Health/fuel/speed HUD elements

Each phase builds on the previous. Phases are playable milestones — after each one, the game is testable and demonstrable at that level of completeness.
