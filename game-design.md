# BTL — Space Arena Game Design

## Overview

2D game with 2.5D visual style (parallax layers, lighting, shadows). Newtonian physics, no dampening. Team-based (red vs blue), networked, up to 6v6. Built with Bevy.

## Map

- Circular, large (scaled for 6v6)
- Divided into 3 tridrants (120° each), one objective per tridrant
- Indestructible asteroid obstacles scattered throughout
- Wrecks from destroyed ships drift for 10 seconds then fade

## Win Condition

- Team with 0 captured objectives loses. Round restarts, teams reshuffle.
- Respawn at a random captured objective with brief spawn protection (2–3s invulnerability).
- Last-stand bonus: a team's sole remaining objective has strengthened defenses (more Factory drones, faster Railgun fire rate, stronger Powerplant shield).

## Capture Mechanics

- Proximity-based: presence in zone captures/decaps
- Capture rate is constant regardless of ship count (1 ship caps as fast as 6)
- Unattended objectives passively decap toward neutral over time (slow decay when no allied ships are present)
- Stays captured until enemy actively decaps or passive decay returns it to neutral
- Capturing an objective breaks Sniper cloak

## Objectives

All three objectives repair nearby allied ships when captured and resupply ammo, fuel, and drones.

### Factory

- 11 weak defense drones: 4 explosive (kamikaze), 7 forward-firing laser drones
- Explosive drones make approaching chaotic, laser drones are predictable but dangerous in numbers
- ~3 ships needed to overwhelm

### Railgun

- Powerful single shot, sniper-like
- Telegraphed: turret tracks target, then stops briefly before firing
- Skilled pilots read the pause and dodge. Punishes straight-line approaches
- ~3 ships needed (spread out to split the railgun's attention)

### Powerplant

- Energy shield bubble over the zone
- Deflects projectiles (bullets bounce off), weakens lasers (reduced damage through shield), detonates torpedoes (explode on contact with shield, not the target)
- Allied ships inside the shield are protected
- ~3 ships needed (have to push inside the shield to deal real damage — close-range brawl)

### Assault Dynamics

Three distinct assault experiences: swarm-fight at Factory, evasion-puzzle at Railgun, breach-and-brawl at Powerplant. Teams naturally prefer attacking certain objectives based on their ship composition.

## Ships

All ships are medium weight with similar HP, speed, and mass. Differentiated by weapon systems and utility. All ships have afterburner (consumes fuel).

### Interceptor

- **Shape:** Narrow dart — long pointed nose, swept-back wings forming a shallow V, twin engine nubs at the rear. Smallest silhouette from the front, widest when banking. Reads as fast even when stationary.
- **Primary:** Rapid-fire autocannon (player aimed)
- **Secondary:** Deployable mines (drop behind)
- **Utility:** Afterburner
- **Best at:** Harassing, area denial, kiting

### Gunship

- **Shape:** Blocky hexagon — wide, flat hull with canted side panels like armored skirts. Three visible turret hardpoints (one dorsal, two flanking). Stubby front profile, broad shoulders. Reads as tough and grounded.
- **Primary:** 1 heavy cannon (player controlled)
- **Secondary:** 3 autonomous turrets (auto-target enemies, imperfect accuracy)
- **Utility:** Afterburner
- **Best at:** Sustained fights, holding zones, multitasking

### Torpedo Boat

- **Shape:** Elongated wedge — rectangular main hull tapering to a flat bow, with two torpedo rack pods mounted on short pylons to either side. Slightly asymmetric silhouette (laser emitter offset on the nose). Reads as purpose-built and industrial.
- **Primary:** Weak continuous laser (player aimed, low DPS)
- **Secondary:** Lock-on tracking torpedoes (slow, dodgeable by agile ships, heavy damage)
- **Utility:** Afterburner
- **Best at:** Objective pressure, punishing slow/distracted targets

### Sniper

- **Shape:** Needle with fins — extremely long and thin, dominated by the railgun barrel running most of the body length. Two small stabilizer fins angled rearward. Minimal cross-section. Reads as fragile and precise, like a rifle with an engine.
- **Primary:** Railgun (charge-up, long range, player aimed)
- **Secondary:** Proximity mines
- **Utility:** Cloak (short duration, broken by firing, NOT by taking damage, shimmer visible like inaccurate radar return)
- **Best at:** Picks, ambush, area denial with mines

### Drone Commander

- **Shape:** Flat disc with arms — wide circular core hull with four radial arms extending outward, each tipped with a drone bay hatch. Defense laser emitters ring the central disc. Largest overall footprint of any ship. Reads as a mobile hive or carrier.
- **Primary:** 5 auto-targeting defense lasers (weak, medium range, friendly-fire on own drones when crossing paths)
- **Secondary:** 7 small attack drones (AI swarm behavior)
- **Utility:** Anti-drone pulse (destroys all nearby drones, friend and foe)
- **Drone resupply:** Slow passive rebuild (~1 per 8s), accelerated at captured objectives
- **Best at:** Zone flooding, countering Factory, area control

## Weapon Visuals

### Projectile Weapons

**Autocannon (Interceptor)**
- **Projectile:** Small bright-yellow elongated pellets, ~4px long. Slight motion blur along travel direction.
- **Muzzle flash:** Tiny orange-white flare at barrel tip, 1 frame duration.
- **Impact:** Small yellow spark burst (4–6 particles, fast fade). On ship hit, adds a brief white damage flash on the target.
- **Trail:** None — pellets are fast enough that the stream of projectiles itself reads as a tracer line.

**Heavy Cannon (Gunship)**
- **Projectile:** Larger glowing orange-red round, ~8px diameter, with a faint bloom halo. Visible rotation spin.
- **Muzzle flash:** Bright orange flare with a brief expanding ring, slightly larger than autocannon flash.
- **Impact:** Medium explosion burst — orange sparks radiating outward, brief flash, faint expanding shockwave ring.
- **Trail:** Short warm-orange smoke trail that dissipates over ~0.3s.

**Auto-Turrets (Gunship)**
- **Projectile:** Small green-tinted rounds, ~3px, dimmer than the heavy cannon. Rapid cadence creates a stuttered stream.
- **Muzzle flash:** Tiny green-white flicker at each turret hardpoint.
- **Impact:** Small green spark burst, less dramatic than the main cannon.
- **Trail:** None.

### Beam Weapons

**Continuous Laser (Torpedo Boat)**
- **Beam:** Thin pale-blue line connecting emitter to target/max range. Inner bright core (~1px) with softer outer glow (~4px). Slight flicker/wobble to feel alive.
- **Origin:** Small blue lens flare at the offset nose emitter.
- **Impact point:** Bright blue-white hotspot on the target surface, with tiny sparks spraying perpendicular to the beam.
- **No beam:** When not firing, the emitter has a dim idle glow.

**Railgun (Sniper)**
- **Charge-up:** Bright cyan-white glow builds along the barrel over 2s, with small electric arcs crawling the hull. Intensity increases exponentially — faint at start, blinding at peak.
- **Beam:** Instantaneous thick cyan-white line spanning the full range, ~6px wide core with wide bloom. Screen-space effect — briefly washes out nearby colors. Lasts ~0.15s then rapidly fades.
- **Impact:** Large cyan flash at hit point, expanding ring, electric arc debris. On miss, the beam terminates at max range with a faint dissipation flicker.
- **Cooldown:** Barrel glows dim cyan, fading over the 5s cooldown. Ship appears "spent."

**Defense Lasers (Drone Commander)**
- **Beam:** Thin red-orange lines, ~1px core, minimal glow. Multiple beams firing simultaneously create a web-like visual around the disc hull.
- **Origin:** Small red pinprick at each emitter node ringing the central disc.
- **Impact:** Tiny red spark at contact point. Understated — these are suppression weapons, not spectacle.

### Tracking Weapons

**Torpedoes (Torpedo Boat)**
- **Body:** Elongated dark metallic shape, ~12px long, with a bright red-orange engine glow at the rear.
- **Trail:** Thick warm-orange smoke/exhaust trail that lingers for ~1.5s, curving visibly as the torpedo turns. The trail is the torpedo's most distinctive visual — makes tracking paths readable.
- **Lock-on indicator:** Targeted ship sees a flashing red diamond reticle and warning pulse while torpedo is homing.
- **Detonation:** Large orange-red explosion — expanding fireball, ring shockwave, scattered debris particles. Larger and more dramatic than cannon impacts.

**Attack Drones (Drone Commander)**
- **Body:** Tiny triangular shapes (~6px), colored slightly lighter than the owner's team color. Faint engine glow dot at rear.
- **Formation:** Swarm movement — slightly jittery individual paths that loosely cluster. Reads as a buzzing cloud when grouped.
- **Weapon fire:** Tiny rapid white-yellow pellets, minimal flash. Individually weak but visually overwhelming when 7 drones focus-fire.
- **Destruction:** Small pop — brief white flash, 2–3 tiny debris fragments, gone.

### Deployables

**Mines (Interceptor / Sniper)**
- **Deploy:** Brief white flash on drop, mine inherits ship velocity then decelerates to stationary.
- **Idle:** Small black octagon (~8px) with a faint white shadow/outline glow and a slow-pulsing red core. Pulse rate increases when an enemy enters proximity range (serves as subtle warning for attentive players).
- **Detonation:** Sharp red-white explosion, expanding ring, fast-fading. Similar intensity to a torpedo hit but more concentrated.

### Abilities

**Cloak (Sniper)**
- **Activation:** Ship rapidly fades to near-invisible over ~0.3s, with a brief ripple distortion effect.
- **Cloaked state:** Ship is a faint shimmer — a subtle refractive distortion, like heat haze. Barely visible against the starfield when stationary, slightly more visible when moving (distortion trails). Allies see a translucent ghost outline.
- **Decloak:** Reverse ripple effect, ship fades back in over ~0.2s. Firing while cloaked forces an instant decloak with a brighter flash.

**Anti-Drone Pulse (Drone Commander)**
- **Charge:** Brief inward energy gather — particles pull toward the disc center over ~0.3s.
- **Pulse:** Expanding cyan-white ring emanating from the ship, ~400 unit radius, rapid expansion. All drones caught in the ring spark and pop.
- **Aftermath:** Faint lingering static/interference shimmer in the pulse area for ~1s.

**Afterburner (All Ships)**
- **Visual:** Enhanced thruster output — flame particles become longer, brighter, and shift from orange to blue-white at peak intensity. Bloom increases. Ship leaves a brief bright trail.
- **Fuel depletion:** As fuel runs low, afterburner flame sputters — intermittent flickers, less stable output, until it cuts out entirely.

### Objective Defense Visuals

**Factory Drones**
- **Explosive (kamikaze):** Small bright-red pulsing shapes, slightly larger than attack drones (~8px). Trail of red-orange sparks when charging toward a target. Detonation is a sharp red AoE flash.
- **Laser drones:** Small shapes with a visible forward-facing emitter dot. Fire thin red beams similar to Drone Commander defense lasers. Orbit in loose formation around the Factory zone.

**Railgun Turret**
- **Structure:** Fixed turret base at zone center, rotating barrel assembly. Barrel visually tracks the current target.
- **Telegraph:** Same charge-up glow as the Sniper railgun but larger scale — the turret barrel glows brighter cyan, then the tracking stops (barrel freezes direction) for 0.5s before firing. The freeze is the dodge window.
- **Beam:** Same as Sniper railgun beam but thicker and brighter.

**Powerplant Shield**
- **Idle:** Translucent dome/bubble, team-colored tint (subtle), with slow-moving hexagonal grid pattern across the surface. Faint hum glow at the base.
- **Projectile deflection:** Brief bright flash at impact point, ripple propagates across the shield surface. Deflected bullet bounces off with a spark.
- **Laser attenuation:** Beam visibly dims and scatters as it passes through the shield — entry point glows.
- **Torpedo detonation:** Large explosion effect on the shield surface, dramatic ripple across the entire dome, brief opacity increase. The shield absorbs it but visibly shudders.

## Physics and Combat Rules

- Newtonian physics, no dampening
- Afterburner on all ships: consumes fuel, slow passive fuel regen, fast regen at captured objectives
- Ammo: slow passive regen, fast regen at captured objectives
- Collision: both ships take damage, faster ship takes slightly more (discourages ramming, rewards blocking)
- Torpedoes: tracking with limited turn radius, dodgeable by maneuverable ships

## Emergent Dynamics

- **Powerplant is the key objective** — its shield protects allied ships inside, making it the hardest to crack but the most valuable to hold. A team with Powerplant + one other point has a strong defensive position.
- **Drone Commander vs Factory** is a natural matchup — anti-drone pulse clears Factory drones, but then the Commander has no drones either. Bring allies to finish the capture.
- **Sniper at Railgun** is thematic and tactical — cloak lets you approach without the railgun tracking you (shimmer is inaccurate), then decloak and fire.
- **Torpedo Boat as shield-breaker** — torpedoes detonate on the Powerplant shield and greatly weaken it on impact. Torpedo Boats are the counter-pick for cracking shielded objectives, opening windows for teammates to push in.
- **Fuel starvation** when losing all objectives means the losing team can't afterburn, making the final push desperate and scrappy.

## Minimap

- Fog of war — rewards stealth ship and scouting
- Radar returns for cloaked Sniper (inaccurate shimmer on minimap)

## Open Questions (defer to prototyping)

- Exact capture zone radius and capture speed
- Passive decap rate (how fast unattended objectives drift to neutral)
- Last-stand defense bonus values (how much stronger the final objective gets)
- Drone AI behavior (formation, scatter, focus-fire)
- Railgun objective cooldown between shots
- Powerplant shield HP pool and torpedo damage to shield
- Spawn protection duration (2–3s range)
- Exact balance numbers for all weapons and ships
- Mine stacking: with 30s lifetime and multiple mine-layers (Interceptor + Sniper), dense permanent minefields around objectives may be oppressive — observe during playtesting, consider team mine cap or lifetime reduction if needed
