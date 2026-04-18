# NLS WebSocket Field Map

Reference for NLS/DHLM websocket payload fields observed from `wss://livetiming.azurewebsites.net/`.

## Header Mapping Used by App

| Header field | Source payload field(s) | Notes |
| --- | --- | --- |
| `session_name` | `HEAT` (preferred), else `HEATTYPE` | `HEATTYPE` is normalized (`R` -> `Race`, `Q` -> `Qualifying`, `T` -> `Practice`). |
| `event_name` | Website event name (if available), else `CUP`/`EVENTNAME` | Website value wins when present, except DHLM handling where websocket `CUP` is preferred. |
| `track_name` | `TRACKNAME` or `TRACK` | Falls back to `NLS` if empty during `PID=4`. |
| `flag` | `TRACKSTATE` | Mapped as `0` -> `Green`, `1` -> `Yellow`, `2` -> `Code 60`, otherwise raw value. |
| `day_time` | `TIME` | Raw feed value. |
| `time_to_go` | Computed from `ENDTIME` + `TIMESTATE` | Refreshed on each emitted snapshot using countdown state captured from `PID=4`. |

## Observed PIDs by eventId

From current capture slices:

- `eventId=20`: `LTS_TIMESYNC`, `LTS_NOT_FOUND`
- `eventId=50`: `LTS_TIMESYNC`, `0`, `3`, `4`

## Field Catalog (All Fields Currently Observed)

### `eventId=20` -> `PID=LTS_TIMESYNC`

Top-level keys: `clientLocalTime`, `eventId`, `eventPid`, `PID`, `serverLocalTime`

Nested/array paths: `eventPid[]`

### `eventId=20` -> `PID=LTS_NOT_FOUND`

Top-level keys: `PID`

### `eventId=50` -> `PID=LTS_TIMESYNC`

Top-level keys: `clientLocalTime`, `eventId`, `eventPid`, `PID`, `serverLocalTime`

Nested/array paths: `eventPid[]`

### `eventId=50` -> `PID=0` (session/entries baseline)

Top-level keys:

- `PID`
- `RECNUM`
- `SND`
- `RCV`
- `VER`
- `EXPORTID`
- `HEATTYPE`
- `SESSION`
- `NROFINTERMEDIATETIMES`
- `TRACKNAME`
- `TRACKLENGTH`
- `S1L`, `S2L`, `S3L`, `S4L`, `S5L`, `S6L`, `S7L`, `S8L`, `S9L`
- `APL`
- `BEST`
- `TRACKSTATE`
- `HEATNUMBER`
- `CUP`
- `HEAT`
- `TOD`
- `STQ`
- `RESULT`

Nested/array paths:

- `BEST[]`
- `BEST[][]`
- `RESULT[]`

### `eventId=50` -> `PID=4` (countdown/track state)

Top-level keys:

- `PID`
- `RECNUM`
- `SND`
- `RCV`
- `VER`
- `EXPORTID`
- `TRACKSTATE`
- `TIMESTATE`
- `ENDTIME`
- `TOD`

### `eventId=50` -> `PID=3` (race control messages)

Top-level keys:

- `PID`
- `RECNUM`
- `SND`
- `RCV`
- `EXPORTID`
- `TRACKSTATE`
- `HEATNUMBER`
- `CUP`
- `HEAT`
- `TOD`
- `MESSAGES`

Nested message paths:

- `MESSAGES[]`
- `MESSAGES[].ID`
- `MESSAGES[].MESSAGETIME`
- `MESSAGES[].MESSAGE`
- `MESSAGES[].MESSAGEGROUP`

Example observed payload values in `MESSAGES[].MESSAGE`:

- `#999 non respect of code 60 - time penalty 95 sec after first lap in race`
- `#155 non respect of code 60 - timepenalty 45 sec in race after first lap`
- `#89 non respect of Pit speed - time penalty 30 sec after the first lap in race`
- `Reminder for 24H`

## Capture and Refresh Workflow

Install dependency:

```bash
python3 -m pip install websocket-client
```

Capture raw websocket frames:

```bash
python3 scripts/nls_ws_capture.py --seconds 180
```

Build markdown + JSON summaries:

```bash
python3 scripts/nls_ws_analyze.py --markdown-out docs/data/nls_ws_field_catalog.md --json-out docs/data/nls_ws_field_catalog.json
```

Notes:

- Raw captures are written as NDJSON in `docs/data/`.
- Files are named `nls_ws_raw_<eventId>_<timestamp>.ndjson`.
- Raw NDJSON and generated local field-catalog files under `docs/data/` are ignored by git.
- Use the generated files to refresh this page whenever new keys appear.
