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
- Respawn at a random captured objective.

## Capture Mechanics

- Proximity-based: presence in zone captures/decaps
- Multiple ships accelerate capture
- Progress only changes with ships present — no natural decay to neutral
- Stays captured until enemy actively decaps

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

- **Primary:** Rapid-fire autocannon (player aimed)
- **Secondary:** Deployable mines (drop behind)
- **Utility:** Afterburner
- **Best at:** Harassing, area denial, kiting

### Gunship

- **Primary:** 1 heavy cannon (player controlled)
- **Secondary:** 3 autonomous turrets (auto-target enemies, imperfect accuracy)
- **Utility:** Afterburner
- **Best at:** Sustained fights, holding zones, multitasking

### Torpedo Boat

- **Primary:** Weak continuous laser (player aimed, low DPS)
- **Secondary:** Lock-on tracking torpedoes (slow, dodgeable by agile ships, heavy damage)
- **Utility:** Afterburner
- **Best at:** Objective pressure, punishing slow/distracted targets

### Sniper

- **Primary:** Railgun (charge-up, long range, player aimed)
- **Secondary:** Proximity mines
- **Utility:** Cloak (short duration, broken by firing, NOT by taking damage, shimmer visible like inaccurate radar return)
- **Best at:** Picks, ambush, area denial with mines

### Drone Commander

- **Primary:** 5 auto-targeting defense lasers (weak, medium range, friendly-fire on own drones when crossing paths)
- **Secondary:** 7 small attack drones (AI swarm behavior)
- **Utility:** Anti-drone pulse (destroys all nearby drones, friend and foe)
- **Drone resupply:** Slow passive rebuild (~1 per 8s), accelerated at captured objectives
- **Best at:** Zone flooding, countering Factory, area control

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
- **Torpedo Boat vs Powerplant** is hard — torpedoes detonate on shield. Forces Torpedo Boats to breach first, where they're weakest (laser only). Team composition matters.
- **Fuel starvation** when losing all objectives means the losing team can't afterburn, making the final push desperate and scrappy.

## Minimap

- Fog of war — rewards stealth ship and scouting
- Radar returns for cloaked Sniper (inaccurate shimmer on minimap)

## Open Questions (defer to prototyping)

- Exact capture zone radius and capture speed curves
- Drone AI behavior (formation, scatter, focus-fire)
- Railgun objective cooldown between shots
- Whether Powerplant shield has HP or is permanent while captured
- Spawn invulnerability duration (if any)
- Map size and asteroid density
- Exact balance numbers for all weapons and ships
