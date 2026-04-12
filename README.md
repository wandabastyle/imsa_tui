# imsa_tui

A terminal user interface (TUI) for live IMSA, NLS, and F1 timing data.

`imsa_tui` is a Rust application that pulls live timing feeds and renders a continuously updating leaderboard in your terminal using `ratatui`.

## Features

- Live IMSA polling (JSONP), NLS websocket streaming, and F1 SignalR-style live streaming.
- Overall leaderboard table with position, car number, class, driver, laps, gaps, lap times, and pit information.
- Multiple viewing modes:
  - **Overall** (all cars)
  - **Grouped** (separate class sections)
  - **Class** (single class focus)
- Header with event/session metadata, track, time-to-go, flag state, and update age.
- Row selection, favourites, and in-table search (car, driver, or team).
- Pit transition state machine highlighting in TUI and Web UI:
  - `IN` phase (short light-blue/cyan event highlight)
  - `PIT` phase (steady yellow in-pit highlight)
  - `OUT` phase (short magenta exit highlight)
  - IMSA/F1 use feed `pit=yes` signal; NLS uses `S5 == PIT`.
- Animated flag color transitions.
- Demo flag mode for UI testing without live flag changes.
- Built-in help popup with keybindings.

## Requirements

- Rust toolchain (stable recommended)
- Network access to IMSA timing endpoints

## Installation

Clone the repository and build with Cargo:

```bash
git clone <your-repo-url>
cd imsa_tui
cargo build --release
```

The binary will be available at:

```text
target/release/imsa_tui
```

## Running

Development run:

```bash
cargo run
```

Development run with static demo data (no live feed connections):

```bash
cargo run --features dev-mode -- --dev
```

> `--dev` is only available when built with the `dev-mode` feature.

Release run:

```bash
./target/release/imsa_tui
```

Web UI run (served by Rust backend):

```bash
cd web
pnpm install
pnpm run build
cd ..

cargo run --bin web_server
```

Frontend asset modes:

- Embedded mode is the runtime default on this branch (`feat/embed-ui`).
- Set `WEBUI_EMBED_UI=0` to force disk-served assets from `web/build` (or `WEB_DIST_DIR`).

Build/run matrix:

```bash
# embedded assets mode at runtime
WEBUI_EMBED_UI=1 cargo run --bin web_server

# disk-served mode override
WEBUI_EMBED_UI=0 cargo run --bin web_server

# build without embedded assets capability
cargo run --no-default-features --bin web_server

# custom disk asset path override
WEB_DIST_DIR=/path/to/web/build cargo run --bin web_server
```

Web UI daemon commands:

```bash
# start in background and return shell
cargo run --bin web_server -- --daemon

# check daemon state and URLs
cargo run --bin web_server -- --status

# restart daemon (stops stale/runtime leftovers automatically)
cargo run --bin web_server -- --restart

# print last ~100 log lines (or set a custom count)
cargo run --bin web_server -- --logs
cargo run --bin web_server -- --logs=250

# stop daemon
cargo run --bin web_server -- --stop
```

Release binary commands:

```bash
./target/release/web_server --daemon
./target/release/web_server --status
./target/release/web_server --restart
./target/release/web_server --logs
./target/release/web_server --stop
```

Notes:

- On first start, the server auto-generates a strong shared access code, saves it, and prints it.
- On later starts, the saved access code is reused automatically.
- Set `WEBUI_ROTATE_PASSWORD=1` to generate and persist a new access code on startup.
- The web app shows a login screen first; enter the shared access code to continue.
- Auth uses a browser-session cookie, so restarting the browser requires login again.
- Login attempts are rate-limited per client address to reduce brute-force retries.
- Cookie security defaults to `Secure` when `WEBUI_AUTO_FUNNEL` is enabled; override with `WEBUI_COOKIE_SECURE=1` or `WEBUI_COOKIE_SECURE=0`.
- `/healthz` and `/readyz` are intentionally public for probes.
- `tailscale funnel --bg http://127.0.0.1:<port>` is started automatically by default (set `WEBUI_AUTO_FUNNEL=0` to disable).
- `WEBUI_EMBED_UI=1`/`0` toggles embedded vs disk mode only when binaries are compiled with the `embed-ui` feature (enabled by default on this branch).
- Web auth/runtime artifacts are stored in the app data-local directory (Linux: `~/.local/share/imsa_tui/`): `web_auth.toml`, `web_server.log`, `web_server.pid`, `web_server.info.toml`.
- WebUI preferences are profile-scoped and stored at `~/.local/share/imsa_tui/profiles/<profile_id>.toml` (profile id is an opaque cookie value).
- Stale WebUI profile files older than 180 days are cleaned up automatically on server startup.
- `POST /api/preferences/reset` resets the active browser profile preferences to defaults (authentication required).

Auth defaults:

- Access control uses one shared access code stored in `~/.local/share/imsa_tui/web_auth.toml`.
- Successful login sets `imsa_session` as a browser-session cookie (`HttpOnly`, `SameSite=Lax`, optional `Secure`), so browser restart requires login again.
- Server-side session entries are in-memory with a `30d` TTL and are cleared on web server restart.
- Login retry protection is enabled by default per client key (`X-Forwarded-For`, then `X-Real-IP`, fallback `unknown-client`): `6` attempts per `60s`, then block for `300s`.

Operator notes:

- Private network / personal use: current defaults are usually sufficient.
- Public exposure (for example with Funnel): keep defaults or tighten them; keep `WEBUI_COOKIE_SECURE=1`.
- If repeatedly locked out while testing, wait 5 minutes or rotate/restart and retry once lockout expires.

Manual Tailscale Funnel commands (new CLI):

```bash
tailscale funnel --bg http://127.0.0.1:8080
tailscale funnel status
tailscale funnel reset
```

## Controls

- `h` — toggle help popup
- `g` — cycle view modes (Overall → Grouped → each class → Favourites)
- `o` — jump to Overall view
- `t` — switch series (IMSA → NLS → F1)
- `r` — cycle demo flag (enables demo mode if disabled)
- `0` — return to live flag (disable demo mode)
- `space` — toggle favourite for selected row
- `f` — jump to next favourite in current view
- `s` — start search mode (car #, driver, or team), `Enter` to apply, `Esc` to cancel
- `n` / `p` — next / previous search match
- `↑` / `↓` (`k` / `j`) — move selection
- `PgUp` / `PgDn`, `Home` / `End` — faster navigation
- `q` — quit (or close help popup first)
- `Esc` — close help popup / quit

### Mobile Web UI

- Compact mobile mode auto-enables at `<= 900px` and can be overridden in the UI (`Compact: Auto/On/Off`).
- In compact mode, each row shows centered driver-first cards with compact `Pos`, `#`, and gap fields, plus optional `More` details.
- A touch action bar is shown on small screens for `View`, `Series`, `Group`, `Search`, `Fav`, and `Help`.
- Row tap selects the active entry; favourite and details controls are available directly in each row.

## Configuration

The TUI stores configuration in a TOML file under the platform config directory (`ProjectDirs::config_dir`).

- Linux: `~/.config/imsa_tui/config.toml`

Current configuration fields:

- `favourites`: list of stable per-series car IDs used for highlighting and the **Favourites** view.
- `selected_series`: the last active series (`imsa`, `nls`, or `f1`) restored on startup.

Example `config.toml`:

```toml
favourites = ["imsa|feed:7", "nls|stnr:911"]
selected_series = "nls"
```

Favourite-key note:

- IMSA/NLS favourites are stored without class suffix to remain stable if class mapping changes (for example `imsa|fallback:7` and `nls|stnr:632`).

## Data sources

IMSA:
- `RaceResults_JSONP.json`
- `RaceData_JSONP.json`

NLS:
- `wss://livetiming.azurewebsites.net/` websocket feed (`eventId = 20`)

F1:
- `https://livetiming.formula1.com/signalr/*` negotiate/start endpoints
- `wss://livetiming.formula1.com/signalr/connect` live stream feed

If a payload is raw JSON instead of JSONP, the parser handles both formats.

## NLS Header Field Map

Quick reference for how websocket payload keys map into the displayed header.

| Header field | Source payload field(s) | Notes |
| --- | --- | --- |
| `session_name` | `HEAT` (preferred), else `HEATTYPE` | `HEATTYPE` is normalized (`R` -> `Race`, `Q` -> `Qualifying`, `T` -> `Practice`). |
| `event_name` | Website event name (if available), else `CUP`/`EVENTNAME` | Website value wins when present. |
| `track_name` | `TRACKNAME` or `TRACK` | Falls back to `NLS` if empty during `PID=4`. |
| `flag` | `TRACKSTATE` | Mapped as `0` -> `Green`, `1` -> `Yellow`, `2` -> `Code 60`, otherwise raw value. |
| `day_time` | `TIME` | Raw feed value. |
| `time_to_go` | Computed from `ENDTIME` + `TIMESTATE` | Refreshed on each emitted snapshot using countdown state captured from `PID=4`. |

Checkered auto-promotion rule (NLS):

- If current flag is `Green` or `-`, set flag to `Checkered` only when both are true:
  - computed `time_to_go` reaches zero (`0`, `0:00`, `00:00`, `00:00:00`) or is unknown (`-`), and
  - `HEATTYPE=R` on the websocket metadata update (`PID=4`).
- Do not override non-green control states (`Yellow`, `Red`, `Code 60`, etc.).

Flow sketch (`PID=4` update path):

```text
PID=4 payload
  |
  +--> TRACKSTATE ---------> header.flag
  +--> TIME ---------------> header.day_time
  +--> TRACKNAME/TRACK ----> header.track_name
  +--> CUP/EVENTNAME ------> header.event_name (unless website name is present)
  +--> ENDTIME + TIMESTATE -> countdown state -> refresh_header_time_to_go()
                                             |
                                             +--> header.time_to_go
                                             +--> optional flag promotion to Checkered
```

NLS row sector mapping (from `PID=0` `RESULT` rows):

- `sector_1..sector_5` now read explicit keys `S1TIME..S5TIME` (with direct `S1..S5` fallback only).
- Non-standard sector aliases are intentionally ignored to keep mapping predictable.

## Troubleshooting

Web auth/profile quick checklist:

- Run `web_server --status` first, then `web_server --logs` for immediate daemon diagnostics.
- Verify storage paths: TUI config is `~/.config/imsa_tui/config.toml`, web runtime/auth/profile files are under `~/.local/share/imsa_tui/`.
- Browser restart requires login again (`imsa_session` is session-scoped), while profile preferences persist via `imsa_profile`.
- Per-profile preferences are stored at `~/.local/share/imsa_tui/profiles/<profile_id>.toml`.
- Lockout defaults: after `6` failed logins in `60s`, login is blocked for `300s`.

- If the table stays empty, wait a few polling cycles for the first successful snapshot.
- If you see repeated errors in the header, confirm outbound HTTPS access is available.
- If rendering looks off, resize your terminal to provide more width for table columns.
- If `--status` reports stale pid/runtime files, run `web_server --stop` once to clean them, then `web_server --daemon` or `web_server --restart`.
- If daemon startup info is delayed, check `web_server --logs` (or `web_server --logs=<n>` for more history).
- If you cannot find web artifacts, check `~/.local/share/imsa_tui/` (`web_auth.toml`, `web_server.log`, `web_server.pid`, `web_server.info.toml`) and `~/.local/share/imsa_tui/profiles/` for WebUI preference files.
- If login returns "too many login attempts", wait for lockout expiry (`300s` default) and retry with the correct access code.

## Development

Quick checks:

```bash
cargo fmt
cargo clippy --all-targets --all-features
cargo check
```

## License

No license file is currently included in this repository. Add a `LICENSE` file if you plan to distribute the project.
