# BTL — Technology Decisions

## Engine

**Bevy** (Rust) — ECS-based game engine. Chosen for performance, Rust's safety guarantees, and strong ecosystem for 2D games.

## Networking

**Lightyear** — server-authoritative networking library for Bevy.

### Why Lightyear

- Server-authoritative model suits competitive 6v6 (12 players total)
- Built-in client-side prediction with rollback — essential for responsive Newtonian physics
- Snapshot interpolation for smooth rendering of remote entities
- First-class integration with Avian2D physics (`lightyear_avian2d`)
- Interest management (rooms-based) for bandwidth optimization
- Transport: **WebTransport (QUIC)** — encrypted by default, multiplexed streams, lower latency than TCP. Self-signed certs for development, proper certs for production

### Alternatives considered

- **bevy_replicon** — excellent API and docs, but physics rollback is DIY (compose multiple crates). More effort for comparable result.
- **bevy_ggrs** — P2P rollback. Doesn't scale to 12 players, requires fully deterministic simulation (cross-platform float determinism is impractical), no server authority.
- **Naia** — lagging behind on Bevy version support, declining community adoption.

## Physics

**Avian2D** — 2D physics engine for Bevy with Lightyear integration.

- Handles Newtonian movement (no dampening)
- Collision detection and response (ship-to-ship, ship-to-asteroid)
- Integrates with Lightyear for networked prediction and rollback via `lightyear_avian2d`

## Architecture

### Client-Server Model

- Dedicated server runs authoritative simulation (physics, game state, capture logic)
- Clients send inputs, receive replicated state
- Client-side prediction for local ship (rollback on mismatch)
- Interpolation for remote ships and entities

### Tick Rate

- Server tick rate: TBD (likely 30-60 Hz, balance between responsiveness and bandwidth)
- Client render rate: uncapped / vsync
- Input send rate: matches server tick rate

### Entity Replication Strategy

- **Replicated:** Ship positions/velocities, projectiles, capture state, objective health, drone positions
- **Predicted (client-side):** Local ship movement, local projectiles
- **Interpolated:** Remote ships, remote projectiles, drones
- **Server-only:** Game state machine (round start/end/reshuffle), anti-cheat validation

### Network Topology

```
[Client 1] ──┐
[Client 2] ──┤
   ...       ├──► [Dedicated Server] ◄──► [Game State]
[Client 11] ─┤
[Client 12] ─┘
```

## Scalability

Current architecture targets 6v6 (12 players). Here's how the decisions hold at higher counts.

| Scale | Feasibility | Changes needed |
|-------|------------|----------------|
| 6v6 (12) | Comfortable | None |
| 8v8 (16) | Fine | Interest management tuning |
| 16v16 (32) | Possible | Aggressive interest management, profile physics, reduce drone counts |
| 32v32 (64) | Major rework | Map sharding, multi-server, simplified physics for distant entities |

### Networking (Lightyear)

- Server-authoritative scales linearly with client count (no N² peer connections).
- **Bandwidth is the primary bottleneck.** Each player adds replicated entities (ship, projectiles, drones). At 12 players with full drone swarms, expect ~100+ replicated entities. At 32 players, potentially 300+.
- Lightyear's interest management (rooms-based) mitigates this by limiting replication to nearby/relevant entities. Essential at 16+ players.
- Prediction/rollback cost scales with entity count — more players means more entities to roll back per mismatch. At very high counts, rollback becomes expensive.

### Physics (Avian2D)

- Simulation cost scales with entity count and collision pairs. 12 ships + projectiles + drones is comfortable.
- Server CPU is the limit (physics runs server-side). Fine up to ~30-40 players. Beyond that, profile and consider simplifying collision shapes or capping drone counts.

### Server Architecture

- Single dedicated server is sufficient up to ~32 players for a physics game of this complexity.
- Beyond 32 players: spatial partitioning with multiple server processes — a fundamentally different architecture (map sharding, cross-server entity handoff).

## Web / WASM Support

The client runs in the browser via `wasm32-unknown-unknown`. The full stack (Bevy + Lightyear + Avian2D) supports WASM.

### Renderer

- **WebGL2** for broadest browser compatibility (enable via `bevy/webgl2` feature)
- WebGPU available but still maturing in browsers; not used initially

### Networking in Browser

- WebTransport (QUIC) works from browser — same transport as native client
- Browser support: Chrome 97+, Edge 98+, Firefox 114+, Safari 26.4+
- **Certificate handling differs from native:**
  - Server generates short-lived ECDSA P-256 self-signed cert (max 14-day validity per spec)
  - Server exposes cert SHA-256 digest via HTTP endpoint
  - WASM client fetches digest, passes to Lightyear's `WebTransportClientIo { certificate_digest }`
  - Browser uses `serverCertificateHashes` API to validate
  - Native client uses `dangerous_configuration` for dev (skips validation)
  - Production: use CA-signed certs, no digest dance needed
- **Fallback:** WebSocket transport for browsers without WebTransport support

### Build Tooling

- **Bevy CLI** (`bevy_cli`) — recommended first-party tool
- `bevy run web --open` for dev, `bevy build --release web --bundle` for deployment
- Alternative: manual `cargo build --target wasm32-unknown-unknown` + `wasm-bindgen`

### WASM Constraints

- Single-threaded (no WASM threads) — physics + rendering + networking share one core
- Binary size ~15MB after `wasm-opt` with LTO
- `load_folder()` unavailable — must load assets individually
- `clap` CLI parsing gated behind `#[cfg(not(target_arch = "wasm32"))]`
- Audio requires user interaction before playback (browser autoplay policy)

### Browser Compatibility

| Browser | WebTransport | Cert pinning (`serverCertificateHashes`) |
|---------|-------------|----------------------------------------|
| Chrome 97+ | Yes | Yes |
| Edge 98+ | Yes | Yes |
| Firefox 114+ | Yes | Partial |
| Safari 26.4+ | Yes | Unknown |

## Open Technical Questions

- Exact Bevy + Lightyear + Avian2D version pinning (check latest compatible set)
- Server hosting strategy (dedicated binary, containerized, cloud provider)
- Lobby / matchmaking system (separate service or integrated)
- Asset pipeline for 2.5D visuals (sprite sheets, parallax layers, lighting)
- Audio engine choice (Bevy built-in vs kira)
- CI/CD pipeline for server and client builds
