# Yew + Trunk Migration Plan

## Goals

- Replace Svelte/Vite/TypeScript frontend with Rust/Yew frontend.
- Keep backend/domain logic as Rust source of truth.
- Reuse typed shared Rust DTOs for web API contracts.
- Keep production static serving from `web/build` via existing backend routes.

## Replacement Scope

- Replace `web/src/**/*.svelte` and TS store/api/table logic with Yew app in `crates/webui`.
- Replace Vite/Svelte tooling with Trunk (`web/index.html`, `web/Trunk.toml`).
- Keep backend web routes (`/auth`, `/api/*`, `/healthz`, `/readyz`) and static fallback model.

## Shared Modules

- New `crates/web-shared` provides shared serde contracts and enums:
  - `Series`, `TimingHeader`, `TimingEntry`, `SeriesSnapshot`, `SnapshotResponse`
  - `Preferences`, login/session/demo payloads
  - class label canonicalization helpers
- Backend converts internal timing/prefs models to `web-shared` API DTOs at the web boundary.

## WebUI Placement

- New workspace member: `crates/webui` (Yew app, wasm target).
- Keep backend in root crate.
- Keep static dist output in `web/build` for compatibility with disk and embedded modes.

## Static Serving Model

- Dev:
  - `cargo run --bin web_server` on `:8080`
  - `trunk serve --config web/Trunk.toml` on `:1420` with reverse proxy to backend APIs
- Prod:
  - `trunk build --release --config web/Trunk.toml` outputs to `web/build`
  - backend serves `web/build` from disk, or embeds the same path with `embed-ui`

## Browser State/Data Mapping

- State container in Yew function component (`AppState`) replaces Svelte store.
- Auth/session: `/auth/session`, `/auth/login`, `/auth/logout` unchanged.
- Snapshots: initial typed fetch for all series, then periodic active-series refresh.
- Views: overall/grouped/class/favourites model preserved.
- Selectors: series picker/group picker + keyboard-first controls preserved.
- Favourites/preferences: typed `/api/preferences` persistence preserved.
- Loading/error/empty states maintained.

## Migration Steps

1. Add workspace and shared crate.
2. Add Yew webui crate + Trunk entry/config.
3. Wire backend API DTO boundary to shared crate.
4. Port shell/header/table/modals and auth flow.
5. Port grouping/search/favourites/demo toggles and live refresh.
6. Remove obsolete Svelte/Vite/TypeScript files.
7. Update CI/docs and verification commands.
