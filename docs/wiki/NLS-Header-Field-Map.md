# NLS Header Field Map

Quick reference for how websocket payload keys map into the displayed header.

| Header field | Source payload field(s) | Notes |
| --- | --- | --- |
| `session_name` | `HEAT` (preferred), else `HEATTYPE` | `HEATTYPE` is normalized (`R` -> `Race`, `Q` -> `Qualifying`, `T` -> `Practice`). |
| `event_name` | Website event name (if available), else `CUP`/`EVENTNAME` | Website value wins when present. |
| `track_name` | `TRACKNAME` or `TRACK` | Falls back to `NLS` if empty during `PID=4`. |
| `flag` | `TRACKSTATE` | Mapped as `0` -> `Green`, `1` -> `Yellow`, `2` -> `Code 60`, otherwise raw value. |
| `day_time` | `TIME` | Raw feed value. |
| `time_to_go` | Computed from `ENDTIME` + `TIMESTATE` | Refreshed on each emitted snapshot using countdown state captured from `PID=4`. |

## Checkered Auto-Promotion Rule

- If current flag is `Green` or `-`, set flag to `Checkered` only when both are true:
  - computed `time_to_go` reaches zero (`0`, `0:00`, `00:00`, `00:00:00`) or is unknown (`-`), and
  - `HEATTYPE=R` on the websocket metadata update (`PID=4`).
- Do not override non-green control states (`Yellow`, `Red`, `Code 60`, etc.).

## Flow Sketch (`PID=4` update path)

```text
PID=4 payload
  |
  +--> TRACKSTATE ---------> header.flag
  +--> TIME ---------------> header.day_time
  +--> TRACKNAME/TRACK ----> header.track_name
  +--> CUP/EVENTNAME ------> header.event_name (unless website name is present)
  +--> ENDTIME + TIMESTATE -> countdown state -> refresh_header_time_to_go()
                                             |
                                             +--> header.time_to_go
                                             +--> optional flag promotion to Checkered
```

## Row Sector Mapping (`PID=0` `RESULT` rows)

- `sector_1..sector_5` read explicit keys `S1TIME..S5TIME` (with direct `S1..S5` fallback only).
- Non-standard sector aliases are intentionally ignored to keep mapping predictable.
