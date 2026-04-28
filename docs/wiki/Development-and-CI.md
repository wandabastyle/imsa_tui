# Development and CI

## Local Verification

Rust checks:

```bash
cargo fmt --check
cargo clippy --all-targets --no-default-features -- -D warnings
cargo test
```

Web checks:

```bash
rustup target add wasm32-unknown-unknown
cargo check -p webui --target wasm32-unknown-unknown
trunk build --release --config web/Trunk.toml
```

## CI Jobs

- `rust-checks`: format, clippy, tests.
- `web-checks`: wasm compile for `webui` and release `trunk build`.
- `integration-embed-ui`: trunk build of web assets and Rust check with all features.
- `docker-compose-smoke`: builds and boots Compose stack, probes `/healthz`, then tears down.

## NLS WebSocket Field Catalog

Use these scripts when websocket fields change or when adding new parser mappings:

```bash
python3 scripts/nls_ws_capture.py --seconds 180
python3 scripts/nls_ws_analyze.py --markdown-out docs/data/nls_ws_field_catalog.md --json-out docs/data/nls_ws_field_catalog.json
```

Then copy relevant findings into `docs/wiki/NLS-WebSocket-Field-Map.md`.

For WEC SignalR/live endpoint mapping updates, refresh and sync:

- `docs/wiki/WEC-SignalR-Field-Map.md`

## Docker Smoke Expectations

- Image builds from repository root.
- Service binds on `18080` host port.
- `GET /healthz` returns `ok`.
