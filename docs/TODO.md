# TODO

## Status

- Completed: **Phase A1** on branch `webui`
  - Commit: `64641ad` (`Harden web login sessions and secure cookie defaults`)
- Completed: **Phase A2** on branch `webui`
  - Added backend auth/session and protected-route regression coverage
  - Added parity regressions for grouped ordering, favourites counting, and header formatting

## Next Phases (Planned)

### Phase A3 (current branch: `webui`, non-systemd only)

- Daemon lifecycle polish:
  - add `--restart`
  - add `--logs` (tail last ~100 lines by default)
  - improve stale PID/status diagnostics and messaging
- Update README ops docs for daemon lifecycle and troubleshooting

### Phase B (new branch from latest `webui`: `feat/systemd-service`)

- Add systemd service unit template
- Add env/config template for service runtime
- Add installation/enable/start/restart docs

### Phase C (new branch from latest `webui`: `feat/embed-ui`)

- Add feature-gated embedded frontend assets mode (`embed-ui`)
- Keep disk-served frontend as default behavior
- Document build/run matrix for both modes
