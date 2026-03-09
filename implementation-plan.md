# BTL — Implementation Plan

## Phase 1: Foundation

### 1.1 Project Structure

- Split into shared library, client binary, and server binary
- Shared lib contains: components, protocol (replicated types, inputs, messages), physics setup, game constants
- Client binary: rendering, input handling, prediction, UI
- Server binary: authoritative simulation, game state machine, no rendering
- Feature flags or workspace members (prefer workspace members for clean separation)

### 1.2 Lightyear Networking Skeleton

- Define the Lightyear `Protocol`: shared input types, replicated components, message channels
- Server: listen on UDP, accept connections, assign client IDs
- Client: connect to server, send inputs, receive replicated state
- Verify: two clients connect to one server, see each other's presence (log output only, no visuals yet)

### 1.3 Ship Movement with Physics

- Define ship entity: `RigidBody::Dynamic`, no linear/angular damping
- Input protocol: thrust forward/backward, rotate left/right, afterburner
- Server applies forces from client inputs to ship's `RigidBody`
- Client-side prediction: local ship moves immediately, rolls back on server correction
- Interpolation: remote ships render smoothly between server snapshots
- Verify: fly two ships in empty space, feel the Newtonian drift, see each other over the network

**Phase 1 is complete when:** Two clients connect, each controls a ship with Newtonian physics, and they see each other moving in real time.

---

## Phase 2: World

### 2.1 Map

- Circular boundary (invisible wall or kill zone outside)
- Static asteroid obstacles: `RigidBody::Static` colliders, various sizes
- Divide map into 3 tridrants (120° sectors), mark objective zones
- Camera follows local player's ship

### 2.2 2.5D Visuals

- Parallax background layers (stars at different depths)
- Ship sprites with rotation
- Asteroid sprites
- Basic lighting/shadow pass to sell the 2.5D look
- Minimap overlay (no fog of war yet)

### 2.3 HUD

- Ship health, fuel, ammo indicators
- Afterburner heat/cooldown
- Minimap with player positions
- Team color indicators (red/blue)

**Phase 2 is complete when:** Players fly around a circular map with asteroids, parallax background, and a basic HUD.

---

## Phase 3: Combat

### 3.1 Weapon System Framework

- Projectile entity: replicated position/velocity, lifetime, damage, owner
- Hit detection via physics collisions (Avian2D sensors or collision events)
- Damage application: reduce HP, destroy at 0
- Ship destruction: spawn drifting wreck (10s lifetime), trigger respawn timer

### 3.2 Ship Classes

Implement one at a time, in this order:

1. **Interceptor** — autocannon + mines + afterburner. Simplest weapons, good baseline.
2. **Gunship** — heavy cannon (player) + 3 auto-turrets. Introduces autonomous targeting AI.
3. **Torpedo Boat** — continuous laser + tracking torpedoes. Introduces homing projectiles.
4. **Sniper** — charge-up railgun + proximity mines + cloak. Introduces stealth mechanic.
5. **Drone Commander** — defense lasers + attack drones + anti-drone pulse. Most complex: drone AI, friendly fire on own drones.

### 3.3 Ship Selection

- Pre-spawn ship class selection screen
- Team assignment (red/blue)
- Spawn at captured objective

**Phase 3 is complete when:** All 5 ship classes are playable with distinct weapons, ships can destroy each other, wrecks drift and fade.

---

## Phase 4: Objectives

### 4.1 Capture Zones

- 3 circular zones, one per tridrant
- Capture logic: count nearby ships per team, progress toward capturing/decapping
- Multiple ships accelerate capture
- Visual indicator: zone color shifts with capture progress

### 4.2 Objective Defenses

1. **Factory** — spawns 11 defense drones (4 explosive, 7 laser). Drones patrol zone, attack enemies.
2. **Railgun** — auto-targeting turret, telegraphed shot (stops tracking before firing).
3. **Powerplant** — energy shield bubble. Deflects projectiles, weakens lasers, detonates torpedoes.

### 4.3 Objective Benefits

- All captured objectives: repair nearby allied ships, resupply ammo/fuel/drones
- Respawn at random captured objective

### 4.4 Win Condition

- Team with 0 objectives loses
- Round restart: reset map, reshuffle teams
- Handle edge case: team loses last point → no spawn → round ends immediately

**Phase 4 is complete when:** Full game loop works — capture objectives, benefit from them, lose all three and the round resets.

---

## Phase 5: Game Feel

### 5.1 Fog of War

- Minimap shows only what's in sensor range
- Cloaked Sniper appears as inaccurate shimmer on minimap
- Objective zones always visible (known locations)

### 5.2 Visual Effects

- Thruster flames (scale with input)
- Weapon fire effects (muzzle flash, beam glow, torpedo trail)
- Explosions on ship destruction and torpedo detonation
- Shield impact effects at Powerplant
- Afterburner flare

### 5.3 Audio

- Engine hum (pitch shifts with velocity)
- Weapon sounds per type
- Explosion sounds
- Ambient space atmosphere
- Capture progress audio cue

### 5.4 Collision Polish

- Ship-to-ship: both take damage, faster ship takes slightly more
- Ship-to-asteroid: damage based on impact velocity
- Screen shake on impacts

**Phase 5 is complete when:** The game feels good to play — responsive, readable, satisfying feedback on every action.

---

## Phase 6: Polish and Infrastructure

### 6.1 Lobby System

- Server browser or matchmaking
- Team balancing on join
- Ship selection in lobby

### 6.2 Balance Pass

- Tune all weapon damage, fire rates, projectile speeds
- Tune capture speeds, objective defense strengths
- Tune fuel/ammo consumption and regen rates
- Tune drone counts, drone AI aggression
- Playtest-driven iteration

### 6.3 Server Infrastructure

- Dedicated server binary packaging
- Containerization for deployment
- Multiple server region support

### 6.4 Anti-cheat

- Server-authoritative validation (already built-in via architecture)
- Input sanity checks (rate limiting, impossible inputs)
- Position reconciliation bounds

---

## Implementation Order Summary

```
Phase 1: Foundation ─── project structure → networking → ship movement
Phase 2: World ──────── map → visuals → HUD
Phase 3: Combat ─────── weapons framework → 5 ship classes → selection
Phase 4: Objectives ─── capture zones → defenses → benefits → win condition
Phase 5: Game Feel ──── fog of war → VFX → audio → collision polish
Phase 6: Polish ─────── lobby → balance → infrastructure → anti-cheat
```

Each phase builds on the previous. Phases are playable milestones — after each one, the game is testable and demonstrable at that level of completeness.
