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

## Next Phases (Planned)

- Planned: **Phase D2 - Profile lifecycle tooling**
  - Add automatic server-side cleanup for stale profile files in `data_local_dir/profiles/` (default retention: 180 days).
  - Add optional reset flow for the active browser profile preferences.
  - Document profile retention and cleanup behavior.
  - Normalize favourite key shape for IMSA/NLS in both local TUI config and server profile storage: drop class suffix from stored keys so class changes do not break favourites.
  - Favourite-key migration draft:
    - Current examples: `imsa|fallback:7:GTP`, `nls|stnr:632:AT2`.
    - Target examples: `imsa|fallback:7`, `nls|stnr:632` (class removed); F1 format remains unchanged.
    - On load: accept both legacy and target formats, normalize in-memory to target format.
    - On save: write only target format to `~/.config/imsa_tui/config.toml` and `~/.local/share/imsa_tui/profiles/*.toml`.
    - Add one-time cleanup/dedup pass to avoid duplicate favourites when legacy and normalized keys both exist.

- Planned: **Phase D3 - Observability improvements**
  - Add structured logs for auth outcomes and profile creation events (without secrets).
  - Add a minimal troubleshooting checklist covering:
    - storage paths (`~/.config/imsa_tui` vs `~/.local/share/imsa_tui`),
    - session-cookie re-login behavior after browser restart,
    - profile preference file location,
    - login lockout behavior,
    - first-line daemon checks (`web_server --status`, `web_server --logs`).
