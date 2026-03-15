# Deploying BTL Server

The server runs on a DigitalOcean VPS at `178.128.206.71` managed by systemd.

## First-time setup

```bash
# On the VPS
mkdir -p /opt/btl
```

Copy the service file:
```bash
scp btl-server.service root@178.128.206.71:/etc/systemd/system/
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
scp target/x86_64-unknown-linux-musl/release/btl-server root@178.128.206.71:/opt/btl/
```

Restart the service to pick up the new binary:
```bash
ssh root@178.128.206.71 systemctl restart btl-server
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

- The service file sets `Restart=on-failure` — the server auto-restarts after a crash with a 5s delay.
- Port defaults to `5888`. Override with `Environment=BTL_PORT=XXXX` in the service file.
- WebTransport uses a self-signed cert. The cert hash is printed to the log on startup — browser clients need it.
