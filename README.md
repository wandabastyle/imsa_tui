# imsa_tui

A terminal user interface (TUI) for live IMSA and NLS timing data.

`imsa_tui` is a Rust application that pulls live timing feeds and renders a continuously updating leaderboard in your terminal using `ratatui`.

## Features

- Live IMSA polling (JSONP, default every 5 seconds) and NLS websocket streaming.
- Overall leaderboard table with position, car number, class, driver, laps, gaps, lap times, and pit information.
- Multiple viewing modes:
  - **Overall** (all cars)
  - **Grouped** (separate class sections)
  - **Class** (single class focus)
- Header with event/session metadata, track, time-to-go, flag state, and update age.
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

Release run:

```bash
./target/release/imsa_tui
```

## Controls

- `h` — toggle help popup
- `g` — cycle view modes (Overall → Grouped → each class)
- `o` — jump to Overall view
- `t` — switch series (IMSA ↔ NLS)
- `r` — cycle demo flag (enables demo mode if disabled)
- `0` — return to live flag (disable demo mode)
- `q` — quit (or close help popup first)
- `Esc` — close help popup / quit

## Configuration

The app stores configuration in a TOML file at:

- Linux: `~/.config/imsa/imsa_tui/config.toml`
- macOS: `~/Library/Application Support/com.imsa.imsa_tui/config.toml`
- Windows: `%APPDATA%\\imsa\\imsa_tui\\config.toml`

Current configuration fields:

- `favourites`: list of car numbers to highlight and include in the **Favourites** view.
- `selected_series`: the last active series (`imsa` or `nls`) restored on startup.

Example `config.toml`:

```toml
favourites = ["imsa|feed:7", "nls|stnr:911:SP9"]
selected_series = "nls"
```

## Data sources

IMSA:
- `RaceResults_JSONP.json`
- `RaceData_JSONP.json`

NLS:
- `wss://livetiming.azurewebsites.net/` websocket feed (`eventId = 20`)

If a payload is raw JSON instead of JSONP, the parser handles both formats.

## Troubleshooting

- If the table stays empty, wait a few polling cycles for the first successful snapshot.
- If you see repeated errors in the header, confirm outbound HTTPS access is available.
- If rendering looks off, resize your terminal to provide more width for table columns.

## Development

Quick checks:

```bash
cargo fmt
cargo clippy --all-targets --all-features
cargo check
```

## License

No license file is currently included in this repository. Add a `LICENSE` file if you plan to distribute the project.
