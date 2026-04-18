# imsa_tui

A terminal user interface (TUI) for live IMSA, NLS, F1, and WEC timing data.

`imsa_tui` is a Rust application that pulls live timing feeds and renders a continuously updating leaderboard in your terminal using `ratatui`.

Project wiki (operator-focused deployment/runbooks):

- https://github.com/wandabastyle/imsa_tui/wiki

## Features

- Live IMSA polling (JSONP), NLS websocket streaming, F1 SignalR-style live streaming, and WEC SockJS/DDP streaming.
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

Demo mode can be toggled at runtime with `d` in both TUI and Web.

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

Docker Compose deployment (local build):

```bash
# build image from current repository checkout
docker compose build

# start the web server container in background
docker compose up -d

# inspect status and logs
docker compose ps
docker compose logs -f imsa-web

# verify local health endpoint (returns: ok)
curl http://127.0.0.1:18080/healthz
```

Docker/web quick notes:

- `compose.yml` maps host `18080` to container `8080`.
- `WEBUI_AUTO_FUNNEL=0` is the default in container deployment.
- Runtime/auth data persists in volume `imsa-web-data` mounted at `/data`.
- Put nginx or nginx-proxy-manager in front of `http://127.0.0.1:18080` and terminate TLS at the proxy.

Detailed deployment and operations docs are in the wiki:

- Quick Start (Compose + NPM): https://github.com/wandabastyle/imsa_tui/wiki/Quick-Start-Compose-and-NPM
- Docker Compose deployment: https://github.com/wandabastyle/imsa_tui/wiki/Deployment-Docker-Compose
- Plain Docker deployment: https://github.com/wandabastyle/imsa_tui/wiki/Deployment-Docker
- Reverse proxy (Nginx Proxy Manager): https://github.com/wandabastyle/imsa_tui/wiki/Reverse-Proxy-Nginx-Proxy-Manager
- Reverse proxy (Nginx): https://github.com/wandabastyle/imsa_tui/wiki/Reverse-Proxy-Nginx
- Web auth and sessions: https://github.com/wandabastyle/imsa_tui/wiki/Web-Auth-and-Sessions
- Operations runbook: https://github.com/wandabastyle/imsa_tui/wiki/Operations-Runbook
- Troubleshooting: https://github.com/wandabastyle/imsa_tui/wiki/Troubleshooting

## Controls

- `h` — toggle help popup
- `m` — toggle race messages popup (NLS/DHLM), dismiss selected message with `Enter`/`d`; `c` clears active list; `C` resets persisted dismissal history
- `g` — cycle view modes (Overall → Grouped → each class → Favourites)
- `o` — jump to Overall view
- `t` — switch series (IMSA → NLS → F1 → WEC)
- `d` — toggle demo/live data source
- `space` — toggle favourite for selected row
- `f` — jump to next favourite in current view
- `s` — start search mode (car #, driver, or team), `Enter` to apply, `Esc` to cancel
- `n` / `p` — next / previous search match
- `↑` / `↓` (`k` / `j`) — move selection
- `PgUp` / `PgDn`, `Home` / `End` — faster navigation
- `q` — quit (or close help popup first)
- `Esc` — close help popup / quit

## Configuration

The TUI stores configuration in a TOML file under the platform config directory (`ProjectDirs::config_dir`).

- Linux: `~/.config/imsa_tui/config.toml`

Current configuration fields:

- `favourites`: list of stable per-series car IDs used for highlighting and the **Favourites** view.
- `selected_series`: the last active series (`imsa`, `nls`, `f1`, or `wec`) restored on startup.

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

WEC:
- `https://livetiming.alkamelsystems.com/fiawec` public LT2 page
- SockJS + Meteor DDP over `wss://livetiming.alkamelsystems.com/sockjs/.../websocket`

WEC reverse-engineered flow notes:

- `docs/wec-lt2-ddp.md`

If a payload is raw JSON instead of JSONP, the parser handles both formats.

NLS protocol/header mapping details are documented in the wiki:

- https://github.com/wandabastyle/imsa_tui/wiki/NLS-WebSocket-Field-Map

## Troubleshooting

See the dedicated troubleshooting page:

- https://github.com/wandabastyle/imsa_tui/wiki/Troubleshooting

## Development

Quick checks:

```bash
cargo fmt
cargo clippy --all-targets --all-features
cargo check
```

## License

No license file is currently included in this repository. Add a `LICENSE` file if you plan to distribute the project.
