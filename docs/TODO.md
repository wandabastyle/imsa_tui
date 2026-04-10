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

## Next Phases (Planned)

- Planned: **Phase C3 - Validation and regression coverage**
  - Add backend tests for new web path resolution (`data_local_dir` locations).
  - Verify unauthenticated access is still blocked for protected API/SSE routes.
  - Update operational docs/troubleshooting for new storage layout.
