# Design System

This project uses a dark, compact motorsport timing-screen visual system. Interfaces should feel like race control telemetry: dense, fast, legible, keyboard-first, and low-glare.

---

## Stack

- **TUI:** Rust, `ratatui`, `crossterm`
- **Web UI:** Yew compiled to WASM with Trunk
- **Web styling:** Plain CSS in `crates/webui/src/styles.css`
- **Backend:** Axum/Tokio web server with embedded or disk-served web assets
- **Shared web contracts:** `crates/web-shared`
- **Core TUI styling:** `src/ui/style.rs`, `src/ui/render.rs`, `src/ui/table.rs`

---

## Visual Direction

`imsa_tui` is a live timing cockpit for IMSA, NLS, DHLM, F1, and WEC, not a generic dashboard.

- Show timing data before decoration.
- Use dark navy surfaces, pale blue-white text, steel-blue borders, and semantic motorsport colors.
- Keep layouts compact, rectangular, and tabular.
- Use color as a state signal: flag, class, pit, search, favourite, selection, error.
- Avoid marketing-style gradients, oversized cards, decorative shadows, and spacious SaaS layouts.
- TUI and Web should feel related, but do not need pixel parity.

---

## Tokens

Web tokens live in `crates/webui/src/styles.css`.

| Token | Value | Usage |
| --- | --- | --- |
| `--bg` | `#050d17` | Page/app canvas |
| `--bg-panel` | `#081829` | Primary panels |
| `--bg-muted` | `#122236` | Inputs, controls, secondary panels |
| `--text` | `#d9e3f2` | Primary text |
| `--text-dim` | `#9fb3cf` | Supporting text and metadata |
| `--accent` | `#2e7bc6` | Selection, active controls, search emphasis |
| `--ok` | `#009944` | Green flag / successful state |
| `--warn` | `#ffdd00` | Yellow flag / car notice highlight |
| `--danger` | `#8f2d35` | Errors and red-state surfaces |
| `--border` | `#1c3b5d` | Panel, table, and control borders |

TUI colors should map to the same concepts through `ratatui::style::Color` helpers instead of drifting into unrelated palettes.

---

## Semantic Colors

### Flags

- **Green:** IMSA green, black text when used as a bright background.
- **Yellow / code 60 / safety:** Yellow background with black text.
- **Red:** Race red background with white or very light text.
- **Checkered:** Near-white background with black text.
- **Unknown/empty:** Treat as green/normal unless the feed provides a better semantic state.

TUI flag transitions are animated in `src/ui/style.rs`. Keep transitions short and readable.

### Classes

Class colors are racing-data semantics, not decoration.

- Prefer live feed class colors when available.
- Fall back to static mappings in `src/ui/style.rs`.
- IMSA examples: GTP white, LMP2 blue, GTD-PRO red, GTD green.
- WEC examples: Hypercar red, LMGT3 green, LMP2 blue.
- NLS/DHLM classes stay neutral unless reliable source colors are available.

### Pit State

Pit highlights must remain consistent across TUI and Web.

| State | Color | Meaning |
| --- | --- | --- |
| `IN` | Cyan / light blue | Short pit-entry pulse |
| `PIT` | Yellow | Steady in-pit state |
| `OUT` | Light magenta | Short pit-exit pulse |

Pit state can override class color, but should not hide row selection or search state entirely.

---

## Typography

### TUI

- Assume the user's terminal monospace font.
- Keep copy short because vertical space is limited.
- Do not rely on special glyphs as the only state indicator.
- The favourite marker may use a visible marker before the car number, but state should also be discoverable through behaviour and context.

### Web

- Use the current monospace stack: `"Iosevka", "JetBrains Mono", "Fira Code", monospace`.
- Table/body text should stay compact, generally `0.75rem` to `0.9rem`.
- Modal and card headings should be modest, not marketing-sized.
- Timing values, car numbers, gaps, sectors, and lap times must stay `nowrap` and monospace-aligned.

---

## Components

### Header

Purpose: event/session status, flag state, mode, update age, favourites count, key hints, search state, and errors.

- TUI implementation: `src/ui/render.rs`.
- Web implementation: `HeaderView` in `crates/webui/src/app.rs` plus `.header` styles.
- Flag state may theme the whole header.
- Keep key hints short and stable.
- Show `DEMO` prominently when enabled.
- Active errors belong in the status/key-hint line, not in a separate decorative alert.

### Timing Table

Purpose: primary live timing surface.

- TUI implementation: `src/ui/table.rs`.
- Web implementation: `TableView` in `crates/webui/src/app.rs` plus table styles.
- Tables are dense, full-width, and scroll the data area, not the whole page.
- Headers are sticky in Web and bold in TUI.
- Selected rows need a distinct blue/gray background and strong contrast.
- Search matches need a visible highlight even when not selected.
- Favourites use a marker near the car number.
- Long driver, vehicle, team, and fastest-driver cells may marquee only when selected.

Series-specific column sets should remain familiar:

- **IMSA:** position, car, class, PIC, driver, vehicle, laps, gaps, lap times, pit.
- **NLS/DHLM:** car/class details, team, laps, gap, lap times, sectors `S1` through `S5`.
- **F1:** position, number, driver, team, laps, gap, interval, lap times, pit, stops.
- **WEC:** position, car, class, PIC, driver, vehicle, team, laps, gap, lap times, sectors `S1` through `S3`.

### Grouped, Class, And Favourites Views

- Overall is the default broad race view.
- Grouped view preserves class order by best overall position.
- Class view focuses one class at a time.
- Favourites is a first-class view, not a secondary filter.
- Empty states should say what is missing and what the app is waiting for.

### Popups And Modals

Used for help, messages, NLS liveticker, logs, series picker, and group picker.

- TUI popups use centered bordered blocks with `Clear`.
- Web modals use a dark backdrop and compact bordered panel.
- Lists scroll internally.
- Selected picker options need both border and background contrast.
- Help text should be keyboard-focused and concise.

### Login

Web-only.

- Keep the card centered, compact, and terminal-like.
- Inputs and buttons share the monospace UI style.
- Login errors use red text and should not cause major layout shift.

---

## Layout

### TUI

- Use a vertical layout: fixed header, flexible timing surface.
- Preserve useful behaviour in small terminals.
- Avoid wrapping timing data inside table cells.
- Keep width calculations series-specific.
- Keep popup dimensions bounded so they fit common terminal sizes.

### Web

- `main` fills `100dvh`, uses a column layout, and hides page overflow.
- Header row sits above a flexing table panel.
- The table panel owns scrolling.
- Table headers remain sticky.
- On narrow screens, stack header controls vertically.
- Do not require horizontal mouse interaction for primary operation unless the data shape makes it unavoidable.

---

## Interaction

Keyboard-first behaviour is part of the product identity.

Core actions:

- `h`: help
- `m`: race messages
- `l`: NLS liveticker when available
- `L`: logs where supported
- `g`: cycle view mode
- `G`: group picker where supported
- `o`: overall view
- `t`: switch series
- `d`: demo mode
- `space`: toggle favourite
- `f`: jump to next favourite
- `s`: search
- `n` / `p`: next/previous search match
- `ArrowUp` / `ArrowDown` and `j` / `k`: row movement
- `PgUp`, `PgDn`, `Home`, `End`: fast navigation
- `Esc`: close popup or cancel mode
- `q`: quit TUI

Interaction rules:

- Selection should remain stable across refreshes when possible.
- Search matches car number, driver, vehicle, and team.
- Favourites use normalized stable IDs and persist across sessions.
- Demo mode must be visually obvious.
- Loading states should explain which series/feed is waiting for data.

---

## Accessibility

- Preserve strong contrast for all semantic states.
- Do not encode important state by color only; include text, markers, labels, or placement.
- Yellow backgrounds require black or near-black text.
- Red flag backgrounds require white or very light text.
- Keep focus and selection visible in both TUI and Web.
- Preserve keyboard access for every primary action.
- Keep motion subtle; flag transitions and selected-cell marquees must not prevent reading.
- Web table headers should remain visible during scroll.
- Use `nowrap` for timing data to prevent misleading wrapped values.

---

## Implementation Rules

- Centralize TUI semantic styling in `src/ui/style.rs`.
- Keep frame-level layout and header rendering in `src/ui/render.rs`.
- Keep row/table styling, selection, favourites, class styling, pit styling, and marquee behaviour in `src/ui/table.rs`.
- Keep Web design tokens and component styles in `crates/webui/src/styles.css`.
- Add new semantic Web colors as named CSS variables before using them broadly.
- Mirror new semantic color intent in TUI helpers.
- Avoid one-off inline colors in Yew markup unless the value comes from feed data, such as live class colors.
- Prefer small explicit style helpers over broad theme abstractions.
- Check visual changes in both TUI and Web for IMSA, NLS/DHLM, F1, and WEC data shapes.
- Run formatting and available checks before treating visual changes as complete.
