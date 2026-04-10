# TODO

## Status

- Completed: **Phase A1** on branch `webui`
  - Commit: `64641ad` (`Harden web login sessions and secure cookie defaults`)
- Completed: **Phase A2** on branch `webui`
  - Added backend auth/session and protected-route regression coverage
  - Added parity regressions for grouped ordering, favourites counting, and header formatting
- Completed: **Phase A3** on branch `webui`
  - Added daemon lifecycle commands `--restart` and `--logs` (default tail view)
  - Improved stale PID/runtime diagnostics in `--status` and `--stop`
  - Updated README ops docs for daemon lifecycle and troubleshooting
- Cancelled: **Systemd service phase**
  - Service/unit work removed from planned scope
- Completed: **Phase B** on branch `feat/embed-ui`
  - Added feature-gated embedded frontend assets mode (`embed-ui`)
  - Kept disk-served frontend as default behavior
  - Documented build/run matrix for disk and embedded modes
- Completed: **Phase C1** on branch `feat/web-storage-profiles`
  - Standardized `ProjectDirs` to `ProjectDirs::from("", "", "imsa_tui")`
  - Moved web runtime/auth artifacts to `data_local_dir` (`~/.local/share/imsa_tui` on Linux)
  - Kept TUI config in `config_dir` (`~/.config/imsa_tui/config.toml` on Linux)
  - Added startup cleanup for legacy web artifacts in the config directory
  - Updated README storage-path documentation

## Next Phases (Planned)

- Planned: **Phase C2 - Per-browser WebUI profiles**
  - Introduce a persistent profile cookie (opaque random id) separate from auth concerns.
  - Store profile preferences server-side in `data_local_dir/profiles/<profile_id>.toml`.
  - Update `/api/preferences` to load/save by profile id instead of one shared global file.
  - Keep current login-code flow; no separate account system.
  - Ensure profile creation and persistence are transparent to the frontend.

- Planned: **Phase C2.5 - Session-only auth cookie hardening (recommended)**
  - Make `imsa_session` a browser-session cookie (no `Max-Age`/`Expires`) so login is required after browser restart.
  - Keep `imsa_profile` persistent so per-browser WebUI preferences remain available after re-login.
  - Verify logout still clears auth cookie immediately.

- Planned: **Phase C3 - Validation and regression coverage**
  - Add backend tests for new web path resolution (`data_local_dir` locations).
  - Add tests for per-profile preference isolation across different profile cookies.
  - Verify unauthenticated access is still blocked for protected API/SSE routes.
  - Update operational docs/troubleshooting for new storage layout.
