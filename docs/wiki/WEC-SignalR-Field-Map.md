# WEC SignalR Field Map

Reference for WEC live timing payloads from the Griiip/Azure SignalR stack and REST bootstrap endpoints.

## Transport and Bootstrap Flow

1. `POST https://insights.griiip.com/live-session-stream/negotiate?negotiateVersion=1`
2. Connect websocket to the returned Azure SignalR URL using `access_token` query parameter.
3. Send SignalR JSON handshake frame: `{"protocol":"json","version":1}` followed by record separator (`\u001e`).
4. Resolve active `sid` from `GET https://insights.griiip.com/meta/sessions-schedule-live`.
   - App-side guard: validate candidate sessions through `/live/session-info/<sid>` and accept only `seriesId = 10` (FIA World Endurance Championship), so other RealWorld series are ignored.
5. Join SignalR groups: `SID-<sid>-<channel>` for:
   - `session-info`
   - `session-clock`
   - `race-flags`
   - `participants`
   - `ranks`
   - `gaps`
   - `laps`
   - `sectors`
6. Consume `lv-*` invocation events as deltas and merge into adapter state.
7. Bootstrap state once from REST endpoints:
   - `/live/session-info/<sid>`
   - `/live/session-clock/<sid>`
   - `/live/race-flags/<sid>`
   - `/live/participants/<sid>`
   - `/live/ranks/<sid>`
   - `/live/gaps/<sid>`
   - `/live/laps/<sid>`
   - `/live/sectors/<sid>`

## Header Mapping Used by App

| Header field | Source payload field(s) | Notes |
| --- | --- | --- |
| `session_name` | `session-info.sessionName` | Used as-is. |
| `session_type_raw` | `session-info.sessionType` | Used for gap logic context in UI. |
| `event_name` | `session-info.eventName` | Used as-is. |
| `track_name` | `session-info.trackName` | Used as-is. |
| `flag` | `race-flags[].flag` (latest), fallback `session-info.connectionStatus` | Normalized to title-case status labels. |
| `day_time` | `session-clock.tsNow` | Compact ISO time string (time component only). |
| `time_to_go` | `session-clock.elapsedTimeMillisNow` | Rendered as `HH:MM:SS` or `MM:SS`. |
| `class_colors` | `session-info.sessionClasses[].classColor` | Stored as class swatches; WEC row text uses this color. |

## Entry Mapping Used by App

Rows are keyed by `pid` (preferred) or `carNumber` and updated incrementally per channel.

| `TimingEntry` field | Primary source field(s) | Notes |
| --- | --- | --- |
| `position` | `ranks.overallPosition` | Overall order. |
| `car_number` | `*.carNumber` | Preserved with leading zeros. |
| `class_name` | `participants.classId` + `session-info.sessionClasses` | Class ID is resolved to display name (`HYPER`, `LMGT3`, ...). |
| `class_rank` | `ranks.position` | Position within class. |
| `driver` | `participants.drivers[].displayName` matched by `currentDriverId` | Driver casing is normalized for readability. |
| `vehicle` | `participants.manufacturer` | Used as displayed model/manufacturer string. |
| `team` | `participants.teamName` | Feed value is kept as-is. |
| `laps` | `ranks.lapNumber` (fallback `gaps/laps.lapNumber`) | Latest seen lap count. |
| `gap_overall` | `gaps.gapToFirstMillis` / `gaps.gapToFirstLaps` | Time rendered as `+S.mmm`, laps as `+N L`, leader as `-`. |
| `gap_next_in_class` | `gaps.gapToAheadMillis` / `gaps.gapToAheadLaps` | Time rendered as `+S.mmm`, laps as `+N L`, class leader as `-`. |
| `last_lap` | `laps.lapTimeMillis` | Rendered as `M:SS.mmm`. |
| `best_lap` | Min observed `laps.lapTimeMillis` | Rendered as `M:SS.mmm`. |
| `best_lap_no` | `laps.lapNumber` at best lap | Stored as text for table display. |
| `pit` | `laps.isEndedInPit` / `laps.isStartedInPit` | Displayed as `Yes` / `No`. |
| `sector_1..sector_3` | `sectors.sectorTimeMillis` by `sectorNumber` | Latest sector times per car, rendered as `SS.mmm` or `M:SS.mmm`. |

## SignalR Event Types Observed

- Handshake ack: `{}`
- Invocation: `type = 1`, with targets such as `lv-ranks`, `lv-gaps`, `lv-laps`, `lv-sectors`, `lv-participants`, `lv-session-info`, `lv-session-clock`, `lv-race-flags`
- Completion: `type = 3` (used for `JoinGroup` responses)
- Ping: `type = 6`
- Close: `type = 7`

## Quick Validation Workflow

Resolve active session ID:

```bash
python3 -c "import requests; print(requests.get('https://insights.griiip.com/meta/sessions-schedule-live', timeout=10).json()[0]['sid'])"
```

Inspect class metadata and colors:

```bash
python3 -c "import requests; sid=requests.get('https://insights.griiip.com/meta/sessions-schedule-live', timeout=10).json()[0]['sid']; print(requests.get(f'https://insights.griiip.com/live/session-info/{sid}', timeout=10).json()['sessionClasses'])"
```

Inspect sectors payload shape:

```bash
python3 -c "import requests; sid=requests.get('https://insights.griiip.com/meta/sessions-schedule-live', timeout=10).json()[0]['sid']; data=requests.get(f'https://insights.griiip.com/live/sectors/{sid}', timeout=10).json(); print(len(data), list(data[0].keys()) if data else [])"
```

## RealWorld Series Overview

All RealWorld series can be listed via:

- `GET https://insights.griiip.com/meta/series`
- Filter where `domains` contains `RealWorld`

Refresh command:

```bash
python3 -c "import requests; data=requests.get('https://insights.griiip.com/meta/series', timeout=20).json(); rows=sorted([s for s in data if 'RealWorld' in (s.get('domains') or [])], key=lambda s:(s.get('name') or '').lower()); print(f'RealWorld series: {len(rows)}'); [print(f'{s.get("id")}\t{s.get("name")}') for s in rows]"
```

Current RealWorld series list (ID -> Name):

- `311` -> `Repco Supercars`
- `362` -> `Repco Supercars - Test`
- `397` -> `1 Nine`
- `365` -> `ADAC GT4 Germany`
- `388` -> `Apex Challenge GT`
- `389` -> `Apex Challenge MX5 CUP`
- `390` -> `Apex Challenge Radical`
- `396` -> `Asian Le Mans Series`
- `109` -> `Castrol Toyota Formula Regional Oceania Championship`
- `118` -> `CTFROC`
- `361` -> `Demo`
- `14` -> `DTM`
- `358` -> `DTM-Test`
- `13` -> `EUROCUP3`
- `409` -> `European Le Mans Series`
- `12` -> `F4 Spain`
- `115` -> `F5000`
- `20` -> `Fanatec GT World Challenge Europe Powered by AWS`
- `16` -> `Ferrari Challenge Europe`
- `10` -> `FIA World Endurance Championship`
- `370` -> `Formula 1`
- `278` -> `Formula 4`
- `2` -> `Formula E`
- `6` -> `GT America powered by AWS`
- `27` -> `GT NZ`
- `5` -> `GT World Challenge America`
- `28` -> `GTR NZ`
- `116` -> `Historic Touring Cars NZ`
- `159` -> `IMSA WeatherTech Championship`
- `216` -> `ITCC Endurance`
- `141` -> `ITCC Touring A`
- `411` -> `Kellymoss`
- `160` -> `Lamborghini Super Trofeo Series`
- `183` -> `MA - Junior Cup`
- `152` -> `MA - Loop Test`
- `149` -> `MA - Mission King of the Baggers`
- `151` -> `MA - Mission Super Hooligan`
- `208` -> `MA - Royal Enfield Build. Train. Race.`
- `184` -> `MA - Stock 1000`
- `182` -> `MA - Superbike`
- `181` -> `MA - Supersport`
- `150` -> `MA - Twins Cup`
- `29` -> `Mazda Racing Series`
- `410` -> `Michelin Le Mans Cup`
- `180` -> `Moto America`
- `148` -> `MotoAmerica Daytona 200`
- `334` -> `MotoAmerica Talent Cup`
- `194` -> `MotoGP`
- `163` -> `Motul 12 Hours Of Sepang`
- `17` -> `NTT INDYCAR`
- `366` -> `NXT Gen Cup`
- `381` -> `PCCB`
- `7` -> `Pirelli GT4 America`
- `30` -> `Pirelli Porsche NZ`
- `161` -> `Porsche Carrera Cup NA`
- `213` -> `Porsche Endurance Challenge NA`
- `412` -> `Porsche Sprint Challenge North America`
- `364` -> `Prototype Cup Germany`
- `4` -> `PSC USA West`
- `15` -> `PSCNA Cayman`
- `3` -> `PSCNA GT3 Cup`
- `18` -> `Radical Cup North America`
- `179` -> `Radical Cup UK`
- `391` -> `SB test`
- `261` -> `Shanghai 8 Hours`
- `9` -> `Skip Barber Formula Race Series`
- `371` -> `Skip Barber Racing School`
- `192` -> `SRO America`
- `207` -> `SRO America - GT2 & SRO3 Testing`
- `206` -> `SRO America - GT4 Testing`
- `275` -> `Stock Car Pro`
- `276` -> `Stock Light`
- `31` -> `Super V8`
- `178` -> `Swedish Touring Endurance Championship`
- `8` -> `TC America powered Skip Barber`
- `374` -> `TCR SA`
- `11` -> `TGRNA GR Cup North America`
- `32` -> `TR86 Championship`
- `277` -> `Turismo Nacional`
- `162` -> `Whelen Mazda MX-5 Cup Presented by Michelin`

## Notes

- There are no WEC-specific runtime env toggles in this stack.
- If payload keys change, update this page and keep `README.md` links aligned.
