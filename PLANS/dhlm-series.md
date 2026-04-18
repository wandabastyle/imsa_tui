# Plan: Add DHLM as Own Series

## Summary
Create DHLM (Deutsche Historische Langstrecken Meisterschaft) as its own series, separated from NLS, while reusing existing NLS adapter code.

## Background
- Both NLS and DHLM use the same websocket (`wss://livetiming.azurewebsites.net/`)
- Currently distinguished only by eventId: "20"=NLS, "50"=DHLM
- DHLM shows as "event name" within NLS when detected

## Changes

### 1. Timing Domain Model (`src/timing.rs`)
- Add `Series::Dhlm` variant to enum
- Update `all()` to return 5 series
- Add `label()` -> "DHLM"
- Add `as_key_prefix()` -> "dhlm"
- Update `FromStr` parser

### 2. Feed Spawn (`src/feed/spawn.rs`)
- Add DHLM case using NLS websocket worker with eventId "50"

### 3. Demo Data (`src/demo.rs`)
- Add DHLM demo snapshots
- Add to series match arms

### 4. UI - Table Columns (`src/ui/table.rs`)
- Add DHLM column layout (same as NLS)

### 5. UI - Pit Signal (`src/ui/pit.rs`)
- Add DHLM pit logic (same as NLS)

### 6. UI - Render (`src/ui/render.rs`)
- Add DHLM header rendering

### 7. UI - Styling (`src/ui/style.rs`)
- Add DHLM styling

### 8. Web API (`src/web/api.rs`, `src/web/state.rs`, `src/web/bridge.rs`)
- Add DHLM to all Series::all() loops

### 9. UI - Popup Ordering (`src/ui/popups.rs`)
- Sort series alphabetically in selection list

## Event ID
DHLM uses eventId="50" (same websocket as 24h, different event)

## Dependencies
Reuses existing NLS adapter: `nls::websocket_worker_with_debug()`

## Acceptance Criteria
- DHLM appears as separate option in series selection
- Selecting DHLM connects to same websocket with eventId "50"
- Sector columns work same as NLS
- Series list sorted alphabetically