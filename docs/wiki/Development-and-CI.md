# Development and CI

## Local Verification

Rust checks:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features
cargo test
```

Web checks:

```bash
cd web
pnpm install --frozen-lockfile
pnpm run verify
```

## CI Jobs

- `rust-checks`: format, clippy, tests.
- `web-checks`: frontend verify.
- `integration-embed-ui`: build web assets and run Rust check with all features.
- `docker-compose-smoke`: builds and boots Compose stack, probes `/healthz`, then tears down.

## Docker Smoke Expectations

- Image builds from repository root.
- Service binds on `18080` host port.
- `GET /healthz` returns `ok`.
