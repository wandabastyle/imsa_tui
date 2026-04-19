# WEC SignalR Notes

The WEC adapter now uses the Griiip live stream endpoint and SignalR transport.

Protocol flow:

1. `POST https://insights.griiip.com/live-session-stream/negotiate?negotiateVersion=1`
2. Read `url` + `accessToken` from negotiate response.
3. Connect websocket to the returned Azure SignalR client URL with `access_token` query arg.
4. Send SignalR JSON handshake frame (`{"protocol":"json","version":1}` + record separator `\u001e`).
5. Parse incoming SignalR frames and map invocation payloads to timing snapshots.

Current adapter behavior:

- Treats SignalR invocation payloads as dynamic JSON and extracts leaderboard rows heuristically.
- Emits snapshot updates when row arrays with car numbers are found.
- Persists WEC snapshots to local disk for fast restore after restart.

There are no WEC-specific runtime environment toggles in this stack.
