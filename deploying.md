# Deploying BTL Server

The btl-server can managed by systemd.

## First-time setup

```bash
# On the machine
mkdir -p /opt/btl
```

Copy the service file:

```bash
scp btl-server.service root@YOUR_SERVER_IP:/etc/systemd/system/
```

Enable and start:

```bash
systemctl daemon-reload
systemctl enable btl-server
systemctl start btl-server
```

## Building and deploying

Cross-compile for Linux on your local machine:

```bash
cargo build --release --package btl-server --target x86_64-unknown-linux-musl
scp target/x86_64-unknown-linux-musl/release/btl-server root@YOUR_SERVER_IP:/opt/btl/
```

Restart the service to pick up the new binary:

```bash
ssh root@YOUR_SERVER_IP systemctl restart btl-server
```

## Useful commands

```bash
# Follow live logs
journalctl -u btl-server -f

# Check status
systemctl status btl-server

# Stop / start
systemctl stop btl-server
systemctl start btl-server
```

## Notes

- The service file sets `Restart=on-failure` — the server auto-restarts after a crash with a few seconds of delay.
- Port defaults to `5888`. Override with `Environment=BTL_PORT=XXXX` in the service file.
- WebTransport uses a self-signed cert that rotates on every server restart.
- The server exposes the cert hash over HTTP on port `game_port + 1` (default 5889). WASM clients auto-fetch it on page load — no `?cert=` URL param needed.
- You can still pass `?cert=HASH` manually to override the auto-fetch.
