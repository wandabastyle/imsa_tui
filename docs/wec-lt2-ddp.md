# WEC LT2 Reverse Engineering Notes

This adapter targets the public FIA WEC LT2 page (`https://livetiming.alkamelsystems.com/fiawec`).

Protocol observed in browser:

- Transport: SockJS over WebSocket (`wss://livetiming.alkamelsystems.com/sockjs/.../websocket`)
- App protocol: Meteor DDP JSON frames (`connect`, `sub`, `ready`, `added`, `changed`, `removed`, `ping`, `pong`)
- Ping handling: server sends `{"msg":"ping"}` and client replies `{"msg":"pong"}`

First-pass subscription flow implemented:

1. Subscribe to `livetimingFeed` with `"fiawec"`.
2. Discover `sessions` from feed docs and subscribe to `sessions`, `events`, `sessionInfo`.
3. Discover current session OID from `session_info`-style docs.
4. Subscribe to session-scoped publications:
   - `sessionClasses`
   - `trackInfo`
   - `standings`
   - `entry`
   - `pitInfo`
   - `raceControl`
   - `sessionResults`
   - `sessionStatus`
   - `weather`
   - `countStates`
   - `bestResults`
   - `sessionBestResultsByClass`

Known raw collections seen in live frames include:

- `events`
- `session_info`
- `session_classes`
- `track_info`
- `standings`
- `session_entry`
- `race_control`
- `session_status`

Debug helpers:

- Set `WEC_DDP_DUMP_PATH=/path/to/wec-ddp.log` to dump raw SockJS/DDP frames.
- Set `WEC_COLLECTION_COUNTS=1` to emit periodic collection/doc-count status lines.
- Set `WEC_DEBUG_UNKNOWN=1` to surface unknown frames in status output.
