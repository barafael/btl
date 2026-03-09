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

- **WebGPU** for browser builds — required for Bevy's bloom/post-processing effects
- Enabled via `bevy/webgpu` feature in WASM target dependencies

### Networking in Browser

- WebTransport (QUIC) works from browser — same transport as native client
- **Certificate handling for dev (self-signed certs):**
  1. Server generates self-signed cert on startup and prints the SHA-256 hash
  2. Browser client receives the hash via URL query parameter `?cert=<hex>`
  3. Browser uses `serverCertificateHashes` WebTransport API to validate
  4. Native client uses `dangerous_configuration` feature to skip validation
  5. Production: use CA-signed certs, no cert hash needed

### Building and Running (WASM)

**Prerequisites:**
- [Trunk](https://trunkrs.dev/) — `cargo install trunk`
- WASM target — `rustup target add wasm32-unknown-unknown`

**Steps:**

1. Start the server (native):
   ```
   cargo run -p btl-server
   ```
   Note the certificate hash printed in the server log:
   ```
   Certificate hash (for browser clients): <hex_hash>
   ```

2. In a separate terminal, start the WASM client dev server:
   ```
   cd crates/btl-client
   trunk serve --release
   ```
   This builds the WASM client and serves it at `http://127.0.0.1:8080/`.

3. Open in a **Chromium-based browser** (Chrome or Edge):
   ```
   http://127.0.0.1:8080/?id=2&server=127.0.0.1:5888&cert=<hex_hash>
   ```
   Replace `<hex_hash>` with the certificate hash from step 1.

**Query parameters:**
- `id` — Client ID (must be unique per client, default: 1)
- `server` — Server address (default: 127.0.0.1:5888)
- `cert` — Server certificate SHA-256 hash (required for self-signed certs)

### Build Tooling

- **Trunk** — WASM bundler for Rust/Bevy, configured via `Trunk.toml` and `index.html`
- `trunk serve --release` for dev (auto-rebuilds on change)
- `trunk build --release` for deployment (outputs to `dist/`)
- Note: do NOT use `data-wasm-opt` in index.html unless `wasm-opt` is installed — Trunk will fail silently

### WASM Constraints

- Single-threaded (no WASM threads) — physics + rendering + networking share one core
- `load_folder()` unavailable — must load assets individually
- `clap` CLI parsing gated behind `#[cfg(not(target_arch = "wasm32"))]` — WASM uses URL query params instead
- Audio requires user interaction before playback (browser autoplay policy)
- `console_error_panic_hook` surfaces Rust panics to the browser console (enabled in main)

### Browser Compatibility

| Browser | Status | Notes |
|---------|--------|-------|
| Chrome 97+ | **Works** | Full WebTransport + cert pinning support |
| Edge 98+ | **Works** | Chromium-based, same as Chrome |
| Firefox | **Broken** | WebTransport `ReadableStream` incompatibility (see below) |
| Safari | **Untested** | WebTransport support is recent |

**Firefox incompatibility:** Lightyear's WebTransport dependency (`xwt-web` v0.15) calls `.get_reader()` on `ReadableByteStream` objects. Firefox requires `.get_reader({ mode: "byob" })` for byte streams, causing a `TypeError: Trying to read with incompatible controller`. This is a third-party crate issue — not fixable without an upstream patch to `xwt-web`. Use Chrome or Edge for now.

## Open Technical Questions

- Exact Bevy + Lightyear + Avian2D version pinning (check latest compatible set)
- Server hosting strategy (dedicated binary, containerized, cloud provider)
- Lobby / matchmaking system (separate service or integrated)
- Asset pipeline for 2.5D visuals (sprite sheets, parallax layers, lighting)
- Audio engine choice (Bevy built-in vs kira)
- CI/CD pipeline for server and client builds
