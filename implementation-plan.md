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

## Phase 4: Objectives ✅

### 4.1 Capture Zones ✅

- [x] 3 circular zones at 60% MAP_RADIUS (6000px), 300-unit radius
- [x] Gradual capture progress: progress ∈ [-1.0 Red, 0.0 neutral, +1.0 Blue]
- [x] Diminishing returns: 1×/1.5×/1.8×/2.0× for 1–4 ships in zone
- [x] Contested (equal non-zero ships) = frozen; empty = passive decap at 0.02/s (~50s full reset)
- [x] Visual zone ring color shifts from gray → team color as progress fills
- [x] King-of-the-hill scoring: 1 pt/s per controlled zone, first to 100 wins
- [x] Victory overlay with round restart (5s display → 3s countdown → full reset)

### 4.2 Objective Defenses ✅

All three defense types spawn when zone captured, despawn when lost:

1. **Factory (zone 0)** — 11 defense drones (7 laser, 4 kamikaze), 15 HP each
   - Laser drones: orbit zone, 8 DPS at 250px range, respawn every 10s
   - Kamikaze drones: 30 damage on contact, swarm to 2× zone radius then charge
2. **Railgun (zone 1)** — auto-targeting turret at zone center
   - Idle → Tracking (1.5 rad/s slew, 2s charge) → Locked (0.5s telegraph) → fires (60 dmg, 3000px/s) → 4s cooldown
3. **Powerplant (zone 2)** — 350px energy shield bubble
   - Reflects enemy projectiles (bounce off surface normal)
   - Detonates enemy torpedoes on contact

### 4.3 Zone Benefits ✅

All implemented and active when inside a friendly-controlled zone:

- [x] +5 HP/s regen (ships below max HP)
- [x] 2× ammo and fuel regen (on top of passive rate)
- [x] Drone Commander: drone respawn timer ticks at double speed
- [x] Respawn position: server picks edge of a randomly-selected controlled zone

### 4.4 Round Management 🔧

- [x] Round restart: despawns all projectiles/mines/torpedoes/drones/zone defense entities, resets HP/fuel/ammo/spawn protection on all ships, resets zone progress to neutral
- [x] Spawn protection: 3s invulnerability after every spawn/respawn (SpawnProtection component)
- [x] Victory stats screen: kill counts per player shown on round-end overlay
- [x] Kill feed: top-right scrolling log (last 5 kills with team + class)
- [ ] Team labels: player name / team-color indicator above each ship
- [ ] Team reshuffle between rounds (rebalance if teams are uneven)
- [ ] Match structure: best of N rounds with series score (stretch)
- [ ] Last-stand defense bonus: strengthened zone defenses when a team holds only 1 objective (stretch)

---

## Phase 5: Polish

### 5.1 Combat Feedback ❌

Missing visual effects for high-impact moments:

- [x] Damage flash: white overlay on ship when hit (DamageFlash component, fully implemented)
- [x] Collision damage: velocity-threshold system, faster ship takes multiplied damage
- [ ] Railgun beam: instantaneous thick bright line (zone railgun + Sniper) with ~0.15s fade
- [ ] Shield impact ripple: expanding ring at projectile contact point on Powerplant shield
- [ ] Screen shake: short burst on collision/explosion, intensity scales with force
- [ ] Heavy cannon smoke trail: ~0.3s fading orange trail behind Gunship projectile
- [ ] Torpedo lock-on indicator: flashing red diamond above the targeted ship

### 5.2 Readability ❌

Small additions that make the game state legible at a glance:

- [ ] Kill feed: scrolling event log (top-left or top-right), shows "Player A killed Player B" with class icons
- [ ] Team labels: name tag above each ship, team-colored, fades near screen edge
- [ ] Zone capture arc: filled arc inside zone ring showing progress toward next controller flip (currently only ring color changes)
- [ ] Team color indicators on HUD bars (health bar tinted to team color)

### 5.3 Fog of War ❌

Adds strategic depth; minimap becomes meaningful information asymmetry:

- Each ship has a sensor range (~800 units)
- Minimap only shows enemies within allied sensor range (union of all allied ships)
- Cloaked Sniper: appears as imprecise shimmer on minimap (±100px random offset, slow update)
- Objective zones always visible (fixed known locations)
- Enemies outside sensor range: hidden from minimap, still visible in main view if on-screen

### 5.4 Audio ❌

No audio exists. This is the largest remaining effort:

- Engine hum: looping tone, pitch scales with speed
- Weapon sounds per type: autocannon rapid rattle, heavy cannon boom, laser sustained hum, torpedo whoosh, railgun charge whine + crack
- Explosion variants: ship destruction (large), torpedo detonation (medium), mine detonation (sharp)
- Ambient: low background drone
- Zone events: rising capture tone, alert sting on zone flip
- Powerplant: energy crackle on shield impact
- UI: class selection click, respawn chime

---

## Phase 6: Infrastructure & Meta

### 6.1 Balance Pass 🔧 (ongoing)

Driven by playtesting. Tune iteratively:

- Weapon damage, fire rates, speeds, ranges per class
- Zone capture/decap rates, objective defense strengths
- Ship HP, mass, thrust, speed caps
- Fuel/ammo consumption and regen
- Drone counts, drone AI aggression, HP
- Zone benefit magnitudes (HP regen, resupply rates)

### 6.2 Server Infrastructure 🔧

- [x] Dedicated headless server (MinimalPlugins + ScheduleRunnerPlugin, no GPU)
- [x] Deployed to DigitalOcean VPS (178.128.206.71)
- [x] WebTransport with self-signed certs
- [ ] Docker containerization
- [ ] Connection quality monitoring / adaptive send rate

### 6.3 Lobby System ❌ (stretch)

- Server browser or simple matchmaking (queue → auto-assign)
- Ready-up system: round starts when all players ready or on timeout
- Class selection locked at round start

### 6.4 Anti-cheat ❌ (stretch)

- Architecture is already server-authoritative (primary protection in place)
- Input sanity checks: rate limiting, impossible combinations, timing validation
- Position reconciliation bounds: reject wildly diverged predictions

---

## Current Progress

```text
Phase 1: Foundation  ✅ ── project structure → networking → physics → prediction
Phase 2: World       ✅ ── map → asteroids → nebula → minimap → HUD → VFX
Phase 3: Combat      ✅ ── 5 classes → all weapons → drones → autopilot → class picker
Phase 4: Objectives  🔧 ── capture ✅ → defenses ✅ → benefits ✅ → round mgmt 🔧
Phase 5: Polish      ❌ ── combat feedback → readability → fog of war → audio
Phase 6: Meta        🔧 ── server infra 🔧 → balance 🔧 → lobby ❌ → anti-cheat ❌
```

### Design Decisions Resolved

1. **Capture mechanic**: gradual progress with diminishing returns — done ✅
2. **Win condition**: score-to-100 KotH — playtest to decide if elimination win (0 objectives = lose) should also apply

### Design Decisions Still Open

1. **Railgun: projectile vs raycast** — currently a fast projectile (3000px/s). Projectile is better for networking (visible, dodgeable). Keep unless it feels wrong during playtesting.
2. **Torpedo lock-on** — currently fires immediately. A 1s lock-on warning before launch would add counterplay. Implement as part of Phase 5.1.
3. **Capturing breaks Sniper cloak** — design doc specifies this; not implemented. Low complexity addition with interesting tradeoff.

### Next Up

1. **Phase 4.4**: Victory stats screen + kill feed (closes out objectives phase)
2. **Phase 5.1**: Railgun beam effect + screen shake (highest combat-feel impact)
3. **Phase 5.2**: Team labels + kill feed readability
4. **Phase 5.4**: Audio (largest effort, biggest feel improvement)
5. **Phase 5.3**: Fog of war (adds strategic depth)
