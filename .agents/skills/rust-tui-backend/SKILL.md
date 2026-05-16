---
name: rust-tui-backend
description: Rust development patterns for imsa_tui TUI and Axum/Tokio backend. Use when working on ratatui/crossterm TUI code, Axum web server, tokio async patterns, cargo builds, or shared contracts in crates/web-shared.
---

# Rust TUI & Backend Development

Guidance for working with the imsa_tui Rust codebase, covering the TUI (ratatui/crossterm), Axum/Tokio web backend, and shared contracts.

## When to Use

- Modifying ratatui table rendering, styling, or terminal UI
- Working with crossterm input handling or terminal events
- Adding Axum routes, middleware, or API handlers
- Working with tokio async runtime, channels, or streams
- Modifying shared types in `crates/web-shared`
- Adding or modifying cargo features (especially `embed-ui`)
- Creating or modifying timing data adapters (IMSA, NLS, F1, WEC)

## Verification Commands

Always run before claiming work is complete:

```bash
# Format check
cargo fmt --check

# Lint (strict - deny warnings)
cargo clippy --all-targets --no-default-features -- -D warnings

# Run all tests
cargo test
```

For web server work, also verify:

```bash
# Check web UI types
cd web && pnpm run typecheck

# Full web verification
cd web && pnpm run verify
```

## TUI Patterns

### Ratatui Table Rendering

- Use `ratatui::widgets::Table` with custom constraints per series
- Width calculations live in `src/ui/*_widths.rs` files
- Selection state uses `ratatui::style::Style` with accent color
- Pit state machine: `IN` (cyan), `PIT` (yellow), `OUT` (magenta)

### Crossterm Event Handling

- Events processed in main loop: `crossterm::event::poll/read`
- Key matching on `KeyEvent` with `KeyCode` and `KeyModifiers`
- Terminal cleanup on exit using `LeaveAlternateScreen`, `ShowCursor`

### Styling

- Semantic colors defined in `src/ui/style.rs`
- Maps to CSS variables in Web: `--bg`, `--bg-panel`, `--accent`, etc.
- Class colors come from feed data when available, fallback to static mappings

## Backend Patterns

### Axum Routes

- Routes defined in `src/web/api.rs` and `src/web/auth.rs`
- State shared via `Arc<RwLock<AppState>>`
- SSE endpoints use `tokio_stream::wrappers::ReceiverStream`

### Tokio Async

- Use `tokio::spawn` for background tasks (feed polling, websocket)
- Channels for TUI ↔ backend communication: `tokio::sync::mpsc`
- Graceful shutdown handled via `tokio::signal::ctrl_c`

### Web-Shared Contracts

- Located in `crates/web-shared/src/lib.rs`
- Shared between Rust backend and TypeScript frontend
- Serde derives for JSON serialization
- Keep types minimal and focused on timing data

## Feature Flags

- `embed-ui` (default): Embeds web assets at compile time via `include_dir`
- Without `embed-ui`: Serves from disk at runtime
- Build with `--no-default-features` to disable embedded assets

## Adapter Patterns

Each series has its own adapter under `src/adapters/`:

- **IMSA**: JSONP polling via `RaceResults_JSONP.json`
- **NLS**: WebSocket to `livetiming.azurewebsites.net`
- **F1**: HTTP polling to `insights.griiip.com`
- **WEC**: SignalR websocket via negotiate endpoint

Common patterns:
- `snapshot.rs`: Current state snapshot for API
- `parser.rs`: Feed-specific parsing logic
- Protocol-specific modules (e.g., `nls/protocol.rs`, `wec/mod.rs`)

## Best Practices

- Keep TUI and Web behavior consistent (selection, search, favourites)
- Use `anyhow` for error handling in async contexts
- Prefer `tracing` for structured logging over `println!`
- Test feed parsing with demo data before live data
- Respect rate limits on external timing endpoints
- Keep feed-specific logic isolated in adapter modules
