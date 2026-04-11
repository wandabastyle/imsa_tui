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
- Completed: **Phase C2** on branch `feat/web-storage-profiles`
  - Added persistent profile cookie handling for authenticated preference requests
  - Switched web preferences to per-profile files in `data_local_dir/profiles/<profile_id>.toml`
  - Kept login-code auth flow unchanged while separating settings by browser profile
  - Added regression coverage for per-profile preference isolation
- Completed: **Phase C2.5** on branch `feat/web-storage-profiles`
  - Changed `imsa_session` to a browser-session cookie so login is required after browser restart
  - Kept persistent `imsa_profile` storage for per-browser preferences after re-login
  - Verified logout still clears the auth cookie (`Max-Age=0`)
- Completed: **Phase C3** on branch `feat/web-storage-profiles`
  - Added backend path-resolution tests for `data_local_dir` web auth/runtime files
  - Kept auth guard verification in regression tests for protected API/SSE routes
  - Updated troubleshooting docs with the new storage locations
- Completed: **Phase D1** on branch `main`
  - Documented auth/session/rate-limit defaults in README.
  - Added operator notes for private-network and public-exposure deployments.
  - Added lockout troubleshooting guidance and default timing references.
  - Deferred CSRF hardening as optional follow-up if risk/usage changes.
- Completed: **Phase D2** on branch `profile-lifecycle`
  - Added automatic startup cleanup for stale profile files in `data_local_dir/profiles/` (180-day default retention).
  - Added authenticated `POST /api/preferences/reset` flow for active browser profile reset.
  - Normalized IMSA/NLS favourites to classless key format in both TUI config and web profile storage.
  - Added legacy-key load normalization and save-time deduplication.
  - Documented retention, reset behavior, and key format updates.
- Completed: **Phase D2.x** on branch `main`
  - Dropped legacy class-suffixed IMSA/NLS favourite-key support (`imsa|fallback:7:GTP`, `nls|stnr:632:AT2`).
  - Updated IMSA/NLS stable ID generation to emit classless keys directly (`fallback:<car>`, `stnr:<car>`).
  - Simplified favourite key generation to series-prefix passthrough; removed normalization on config/profile load.
  - Updated regression and unit tests to expect only classless favourite keys.
- Completed: **Phase D6** on branch `main`
  - Kept ENDTIME/TIMESTATE metadata from `PID=4` updates as countdown state.
  - Recompute and refresh NLS `time_to_go` before every emitted snapshot (including `PID=0` result cycles).
  - Preserved fallback behavior when countdown metadata is missing.
  - Added countdown helper tests for relative and absolute timestamp modes.

## Next Phases (Planned)

- Planned: **Phase D3 - Observability improvements**
  - Add structured logs for auth outcomes and profile creation events (without secrets).
  - Add a minimal troubleshooting checklist covering:
    - storage paths (`~/.config/imsa_tui` vs `~/.local/share/imsa_tui`),
    - session-cookie re-login behavior after browser restart,
    - profile preference file location,
    - login lockout behavior,
    - first-line daemon checks (`web_server --status`, `web_server --logs`).

- Planned: **Phase D4 - NLS sector columns (S1-S5)**
  - Extend shared timing model with NLS-visible sector fields (`sector_1`..`sector_5`).
  - Parse up to 5 sector values from NLS websocket payloads with tolerant key lookup and `"-"` fallback.
  - Render always-on `S1`..`S5` columns for NLS in both TUI and Web UI tables.
  - Add parser coverage for full/partial/variant sector payload shapes.

- Planned: **Phase D5 - Favourite-relative gap reference (`f`)**
  - When `f` jumps to a favourite, set that row as the active gap reference anchor.
  - Display gap columns relative to the anchor row in both TUI and Web UI (`IMSA: Gap O/Gap C/Next C`, `NLS: Gap`, `F1: Gap/Int`).
  - Show `REF` for the anchor row and keep raw values as fallback when units cannot be compared.
  - Clear anchor on context changes (series/view/group switch) or when the anchor row is no longer present.
  - Add parser/formatting tests for time, lap, and mixed/unknown gap values.
