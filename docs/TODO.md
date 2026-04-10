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

## Next Phases (Planned)

- None currently planned
