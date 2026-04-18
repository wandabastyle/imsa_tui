// NLS websocket adapter: subscribes to livetiming hub events and maps payloads to timing rows.

mod countdown;
mod protocol;
mod schedule;
mod snapshot;

use std::{
    sync::mpsc::{Receiver, Sender},
    time::{Duration, Instant},
};

use reqwest::blocking::Client;
use serde_json::json;
use tungstenite::{
    client::IntoClientRequest,
    connect,
    http::header::{HeaderValue, ORIGIN, USER_AGENT},
    Message,
};

use crate::{
    timing::{TimingEntry, TimingHeader, TimingMessage},
    timing_persist::{debounce_elapsed, log_series_debug, PersistState, SeriesDebugOutput},
};

use self::{
    countdown::{now_millis, refresh_header_time_to_go, CountdownState},
    protocol::{
        is_retriable_timeout, parse_ws_message, refresh_active_event_id, set_socket_timeout,
        should_emit_connected_status_on_update,
    },
    schedule::{
        determine_active_nuerburgring_event_id, fetch_homepage_event_name, fetch_termine_event_name,
    },
    snapshot::{
        derive_session_id, meaningful_snapshot_fingerprint, nls_snapshot_path, persist_snapshot,
        persist_snapshot_if_dirty, restore_snapshot_from_disk, NlsSnapshot,
    },
};

#[cfg(test)]
use self::protocol::{entry_from_value, set_tcp_read_timeout};

#[cfg(test)]
use self::schedule::{
    discover_termine_url_from_homepage_html, extract_date_range_for_event_title,
    html_to_text_lines, parse_german_date_range, parse_termine_entries,
    select_active_termine_event_title, title_matches_24h_qualifiers, CalendarDate,
    TermineScheduleEntry,
};

const WS_URL: &str = "wss://livetiming.azurewebsites.net/";
const DEFAULT_NLS_EVENT_ID: &str = "20";
#[cfg(test)]
const N24_EVENT_ID: &str = "50";
#[cfg(test)]
const N24_TARGET_EVENT_TITLE: &str = "ADAC RAVENOL 24h Nürburgring";
const WEBSITE_EVENT_REFRESH_INTERVAL: Duration = Duration::from_secs(10 * 60);
const SNAPSHOT_SAVE_DEBOUNCE: Duration = Duration::from_secs(180);

fn build_request() -> tungstenite::handshake::client::Request {
    let mut request = WS_URL
        .into_client_request()
        .expect("failed to create websocket request");

    request.headers_mut().insert(
        ORIGIN,
        HeaderValue::from_static("https://livetiming.azurewebsites.net"),
    );
    request
        .headers_mut()
        .insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0"));

    request
}

pub fn websocket_worker(tx: Sender<TimingMessage>, source_id: u64, stop_rx: Receiver<()>) {
    websocket_worker_with_debug(tx, source_id, stop_rx, SeriesDebugOutput::Silent)
}

pub fn websocket_worker_with_debug(
    tx: Sender<TimingMessage>,
    source_id: u64,
    stop_rx: Receiver<()>,
    debug_output: SeriesDebugOutput,
) {
    let mut header = TimingHeader {
        event_name: "NLS Live Timing".to_string(),
        track_name: "Nürburgring".to_string(),
        ..TimingHeader::default()
    };
    let mut latest_entries: Vec<TimingEntry> = Vec::new();
    let website_client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .ok();
    let mut termine_event_name: Option<String> = None;
    let mut homepage_event_name: Option<String> = None;
    let mut next_website_refresh = Instant::now();
    let mut countdown: Option<CountdownState> = None;
    let mut is_race_session = false;
    let mut active_event_id = DEFAULT_NLS_EVENT_ID;
    let mut persist = PersistState::new(nls_snapshot_path());
    let mut last_good_live_snapshot: Option<NlsSnapshot> = None;
    let mut last_session_id = restore_snapshot_from_disk(
        &mut persist,
        &mut header,
        &mut latest_entries,
        &tx,
        source_id,
        &debug_output,
    );

    'outer: loop {
        if stop_rx.try_recv().is_ok() {
            if let Some(snapshot) = last_good_live_snapshot.as_ref() {
                persist_snapshot_if_dirty(
                    &mut persist,
                    snapshot,
                    now_millis() as u64,
                    &debug_output,
                );
            }
            break;
        }

        if Instant::now() >= next_website_refresh {
            if let Some(client) = website_client.as_ref() {
                if let Ok(parsed_name) = fetch_termine_event_name(client) {
                    termine_event_name = Some(parsed_name.clone());
                    header.event_name = parsed_name;
                }

                homepage_event_name = fetch_homepage_event_name(client);

                if termine_event_name.is_none() {
                    if let Some(parsed_name) = homepage_event_name.as_ref() {
                        header.event_name = parsed_name.clone();
                    }
                }

                if let Some(status_text) = refresh_active_event_id(
                    &mut active_event_id,
                    determine_active_nuerburgring_event_id(client),
                ) {
                    let _ = tx.send(TimingMessage::Status {
                        source_id,
                        text: status_text,
                    });
                }
            }
            next_website_refresh = Instant::now() + WEBSITE_EVENT_REFRESH_INTERVAL;
        }

        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: "Connecting to NLS websocket...".to_string(),
        });
        log_series_debug(&debug_output, "NLS", "connecting websocket");

        let request = build_request();
        let connection = connect(request);

        let (mut socket, response) = match connection {
            Ok(ok) => ok,
            Err(err) => {
                let _ = tx.send(TimingMessage::Error {
                    source_id,
                    text: format!("connect failed: {err}"),
                });
                if stop_rx.recv_timeout(Duration::from_secs(3)).is_ok() {
                    break;
                }
                continue;
            }
        };

        set_socket_timeout(&mut socket);

        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: format!("NLS connected ({})", response.status()),
        });
        log_series_debug(
            &debug_output,
            "NLS",
            format!("websocket connected ({})", response.status()),
        );
        let mut connected_status_sent = true;

        let subscribe = json!({
            "clientLocalTime": now_millis(),
            "eventId": active_event_id,
            "eventPid": [0, 4]
        });

        if let Err(err) = socket.send(Message::Text(subscribe.to_string())) {
            let _ = tx.send(TimingMessage::Error {
                source_id,
                text: format!("subscribe failed: {err}"),
            });
            if stop_rx.recv_timeout(Duration::from_secs(3)).is_ok() {
                break;
            }
            continue;
        }

        loop {
            if stop_rx.try_recv().is_ok() {
                if let Some(snapshot) = last_good_live_snapshot.as_ref() {
                    persist_snapshot_if_dirty(
                        &mut persist,
                        snapshot,
                        now_millis() as u64,
                        &debug_output,
                    );
                }
                break 'outer;
            }

            match socket.read() {
                Ok(Message::Text(text)) => {
                    if let Some((entries, header_changed)) = parse_ws_message(
                        &text,
                        &mut header,
                        termine_event_name.as_deref(),
                        homepage_event_name.as_deref(),
                        &mut countdown,
                        &mut is_race_session,
                        active_event_id,
                    ) {
                        if let Some(new_entries) = entries {
                            latest_entries = new_entries;
                        }
                        refresh_header_time_to_go(&mut header, countdown.as_ref());
                        let session_id = derive_session_id(&header);
                        let snapshot = NlsSnapshot {
                            header: header.clone(),
                            entries: latest_entries.clone(),
                            session_id: session_id.clone(),
                            fingerprint: meaningful_snapshot_fingerprint(&header, &latest_entries),
                        };

                        let first_real_of_session =
                            !snapshot.entries.is_empty() && session_id != last_session_id;
                        let session_complete =
                            snapshot.header.flag.eq_ignore_ascii_case("checkered");
                        let materially_changed = last_good_live_snapshot
                            .as_ref()
                            .map(|prev| prev.fingerprint != snapshot.fingerprint)
                            .unwrap_or(true);

                        if materially_changed {
                            persist.dirty_since_last_save = true;
                        }

                        let never_persisted = persist.last_persisted_hash.is_none();
                        let save_now = never_persisted
                            || first_real_of_session
                            || session_complete
                            || (persist.dirty_since_last_save
                                && debounce_elapsed(persist.last_save_at, SNAPSHOT_SAVE_DEBOUNCE));

                        if save_now {
                            persist_snapshot(
                                &mut persist,
                                &snapshot,
                                now_millis() as u64,
                                &debug_output,
                            );
                        }

                        last_session_id = session_id;
                        last_good_live_snapshot = Some(snapshot);

                        let _ = tx.send(TimingMessage::Snapshot {
                            source_id,
                            header: header.clone(),
                            entries: latest_entries.clone(),
                        });
                        if should_emit_connected_status_on_update(
                            header_changed,
                            connected_status_sent,
                        ) {
                            let _ = tx.send(TimingMessage::Status {
                                source_id,
                                text: "NLS live timing connected".to_string(),
                            });
                            connected_status_sent = true;
                        }
                    }
                }
                Ok(Message::Binary(data)) => {
                    if let Ok(text) = std::str::from_utf8(&data) {
                        if let Some((entries, header_changed)) = parse_ws_message(
                            text,
                            &mut header,
                            termine_event_name.as_deref(),
                            homepage_event_name.as_deref(),
                            &mut countdown,
                            &mut is_race_session,
                            active_event_id,
                        ) {
                            if let Some(new_entries) = entries {
                                latest_entries = new_entries;
                            }
                            refresh_header_time_to_go(&mut header, countdown.as_ref());
                            let session_id = derive_session_id(&header);
                            let snapshot = NlsSnapshot {
                                header: header.clone(),
                                entries: latest_entries.clone(),
                                session_id: session_id.clone(),
                                fingerprint: meaningful_snapshot_fingerprint(
                                    &header,
                                    &latest_entries,
                                ),
                            };

                            let first_real_of_session =
                                !snapshot.entries.is_empty() && session_id != last_session_id;
                            let session_complete =
                                snapshot.header.flag.eq_ignore_ascii_case("checkered");
                            let materially_changed = last_good_live_snapshot
                                .as_ref()
                                .map(|prev| prev.fingerprint != snapshot.fingerprint)
                                .unwrap_or(true);

                            if materially_changed {
                                persist.dirty_since_last_save = true;
                            }

                            let never_persisted = persist.last_persisted_hash.is_none();
                            let save_now = never_persisted
                                || first_real_of_session
                                || session_complete
                                || (persist.dirty_since_last_save
                                    && debounce_elapsed(
                                        persist.last_save_at,
                                        SNAPSHOT_SAVE_DEBOUNCE,
                                    ));

                            if save_now {
                                persist_snapshot(
                                    &mut persist,
                                    &snapshot,
                                    now_millis() as u64,
                                    &debug_output,
                                );
                            }

                            last_session_id = session_id;
                            last_good_live_snapshot = Some(snapshot);

                            let _ = tx.send(TimingMessage::Snapshot {
                                source_id,
                                header: header.clone(),
                                entries: latest_entries.clone(),
                            });
                            if should_emit_connected_status_on_update(
                                header_changed,
                                connected_status_sent,
                            ) {
                                let _ = tx.send(TimingMessage::Status {
                                    source_id,
                                    text: "NLS live timing connected".to_string(),
                                });
                                connected_status_sent = true;
                            }
                        }
                    }
                }
                Ok(Message::Ping(data)) => {
                    if let Err(err) = socket.send(Message::Pong(data)) {
                        let _ = tx.send(TimingMessage::Error {
                            source_id,
                            text: format!("pong failed: {err}"),
                        });
                        break;
                    }
                }
                Ok(Message::Pong(_)) => {}
                Ok(Message::Close(frame)) => {
                    let _ = tx.send(TimingMessage::Error {
                        source_id,
                        text: format!("socket closed: {frame:?}"),
                    });
                    break;
                }
                Ok(Message::Frame(_)) => {}
                Err(err) if is_retriable_timeout(&err) => {
                    continue;
                }
                Err(err) => {
                    let _ = tx.send(TimingMessage::Error {
                        source_id,
                        text: format!("read failed: {err}"),
                    });
                    break;
                }
            }
        }

        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: "NLS reconnecting in 3s...".to_string(),
        });
        log_series_debug(&debug_output, "NLS", "reconnecting in 3s");
        if stop_rx.recv_timeout(Duration::from_secs(3)).is_ok() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn current_time_to_end_at_counts_down_for_relative_mode() {
        let header = TimingHeader {
            time_to_go: "-".to_string(),
            ..TimingHeader::default()
        };

        let rendered =
            countdown::current_time_to_end_at(&header, 120_000, "0", 1_000_000, 1_030_500);
        assert_eq!(rendered, "00:01:29");
    }

    #[test]
    fn current_time_to_end_at_uses_absolute_timestamp_mode() {
        let header = TimingHeader {
            time_to_go: "-".to_string(),
            ..TimingHeader::default()
        };

        let rendered = countdown::current_time_to_end_at(&header, 2_000_000, "1", 0, 1_940_000);
        assert_eq!(rendered, "00:01:00");
    }

    #[test]
    fn refresh_sets_checkered_when_tte_reaches_zero_on_green() {
        let mut header = TimingHeader {
            flag: "Green".to_string(),
            session_name: "Race".to_string(),
            ..TimingHeader::default()
        };
        let countdown = CountdownState {
            end_time_raw: 0,
            time_state_raw: "0".to_string(),
            received_at_ms: 0,
            is_race_session: true,
        };

        header.time_to_go = "0:00".to_string();
        refresh_header_time_to_go(&mut header, Some(&countdown));

        assert_eq!(header.flag, "Checkered");
    }

    #[test]
    fn refresh_keeps_non_green_flags_when_tte_reaches_zero() {
        let mut header = TimingHeader {
            flag: "Yellow".to_string(),
            session_name: "Race".to_string(),
            time_to_go: "0:00".to_string(),
            ..TimingHeader::default()
        };
        let countdown = CountdownState {
            end_time_raw: 0,
            time_state_raw: "0".to_string(),
            received_at_ms: 0,
            is_race_session: true,
        };

        refresh_header_time_to_go(&mut header, Some(&countdown));

        assert_eq!(header.flag, "Yellow");
    }

    #[test]
    fn refresh_sets_checkered_when_tte_unknown_in_race_session() {
        let mut header = TimingHeader {
            flag: "Green".to_string(),
            session_name: "Rennen".to_string(),
            time_to_go: "-".to_string(),
            ..TimingHeader::default()
        };
        let countdown = CountdownState {
            end_time_raw: 0,
            time_state_raw: "0".to_string(),
            received_at_ms: 0,
            is_race_session: true,
        };

        refresh_header_time_to_go(&mut header, Some(&countdown));

        assert_eq!(header.flag, "Checkered");
    }

    #[test]
    fn refresh_sets_checkered_when_tte_empty_in_race_session() {
        let mut header = TimingHeader {
            flag: "Green".to_string(),
            session_name: "Rennen".to_string(),
            time_to_go: String::new(),
            ..TimingHeader::default()
        };
        let countdown = CountdownState {
            end_time_raw: 0,
            time_state_raw: "0".to_string(),
            received_at_ms: 0,
            is_race_session: true,
        };

        refresh_header_time_to_go(&mut header, Some(&countdown));

        assert_eq!(header.flag, "Checkered");
    }

    #[test]
    fn refresh_keeps_green_when_tte_zero_but_not_race_session() {
        let mut header = TimingHeader {
            flag: "Green".to_string(),
            session_name: "Qualifying".to_string(),
            time_to_go: "0:00".to_string(),
            ..TimingHeader::default()
        };
        let countdown = CountdownState {
            end_time_raw: 0,
            time_state_raw: "0".to_string(),
            received_at_ms: 0,
            is_race_session: false,
        };

        refresh_header_time_to_go(&mut header, Some(&countdown));

        assert_eq!(header.flag, "Green");
    }

    #[test]
    fn pid0_race_session_stays_true_when_follow_up_payload_omits_heattype() {
        let mut header = TimingHeader::default();
        let mut countdown: Option<CountdownState> = None;
        let mut is_race_session = false;

        let first = r#"{"PID":"0","HEATTYPE":"R","RESULT":[]}"#;
        let second = r#"{"PID":"0","RESULT":[]}"#;

        let _ = parse_ws_message(
            first,
            &mut header,
            None,
            None,
            &mut countdown,
            &mut is_race_session,
            "20",
        );
        assert!(is_race_session);

        let _ = parse_ws_message(
            second,
            &mut header,
            None,
            None,
            &mut countdown,
            &mut is_race_session,
            "20",
        );
        assert!(is_race_session);
    }

    #[test]
    fn entry_from_value_reads_all_five_sectors() {
        let row = json!({
            "POSITION": "1",
            "STNR": "77",
            "CLASSNAME": "SP9",
            "CLASSRANK": "1",
            "NAME": "Driver",
            "CAR": "Car",
            "TEAM": "Team",
            "LAPS": "12",
            "GAP": "Leader",
            "LASTLAPTIME": "8:01.234",
            "FASTESTLAP": "7:59.111",
            "S1": "1:31.001",
            "S2": "2:00.002",
            "S3": "1:11.003",
            "S4": "1:45.004",
            "S5": "1:34.005"
        });

        let entry = entry_from_value(&row, "20").expect("entry");
        assert_eq!(entry.sector_1, "1:31.001");
        assert_eq!(entry.sector_2, "2:00.002");
        assert_eq!(entry.sector_3, "1:11.003");
        assert_eq!(entry.sector_4, "1:45.004");
        assert_eq!(entry.sector_5, "1:34.005");
    }

    #[test]
    fn entry_from_value_ignores_non_standard_sector_keys() {
        let row = json!({
            "POSITION": "4",
            "STNR": "911",
            "CLASSNAME": "SP9",
            "CLASSRANK": "3",
            "NAME": "Driver",
            "CAR": "Car",
            "TEAM": "Team",
            "LAPS": "7",
            "GAP": "+12.300",
            "LASTLAPTIME": "8:12.340",
            "FASTESTLAP": "8:05.900",
            "SECTOR_1": "1:32.100",
            "SEC2": "2:01.200",
            "SEKTOR3": "1:10.300",
            "SECTOR4": "1:46.400"
        });

        let entry = entry_from_value(&row, "20").expect("entry");
        assert_eq!(entry.sector_1, "-");
        assert_eq!(entry.sector_2, "-");
        assert_eq!(entry.sector_3, "-");
        assert_eq!(entry.sector_4, "-");
        assert_eq!(entry.sector_5, "-");
    }

    #[test]
    fn entry_from_value_prefers_sxtime_sector_keys() {
        let row = json!({
            "POSITION": "8",
            "STNR": "44",
            "CLASSNAME": "SP9",
            "CLASSRANK": "5",
            "NAME": "Driver",
            "CAR": "Car",
            "TEAM": "Team",
            "LAPS": "20",
            "GAP": "+23.000",
            "LASTLAPTIME": "8:11.111",
            "FASTESTLAP": "8:02.222",
            "S1TIME": "1:32.555",
            "S2TIME": "2:01.666",
            "S3TIME": "1:10.777",
            "S4TIME": "1:45.888",
            "S5TIME": "1:33.999"
        });

        let entry = entry_from_value(&row, "20").expect("entry");
        assert_eq!(entry.sector_1, "1:32.555");
        assert_eq!(entry.sector_2, "2:01.666");
        assert_eq!(entry.sector_3, "1:10.777");
        assert_eq!(entry.sector_4, "1:45.888");
        assert_eq!(entry.sector_5, "1:33.999");
        assert_eq!(entry.pit, "-");
    }

    #[test]
    fn entry_from_value_maps_s5_inout_to_pit_flag() {
        let row = json!({
            "POSITION": "9",
            "STNR": "632",
            "CLASSNAME": "AT",
            "CLASSRANK": "1",
            "NAME": "Driver",
            "CAR": "Car",
            "TEAM": "Team",
            "LAPS": "22",
            "GAP": "+44.000",
            "LASTLAPTIME": "8:20.000",
            "FASTESTLAP": "8:10.000",
            "S5TIME": "OUT"
        });

        let entry = entry_from_value(&row, "20").expect("entry");
        assert_eq!(entry.sector_5, "OUT");
        assert_eq!(entry.pit, "No");
    }

    #[test]
    fn parse_german_date_range_handles_24h_format() {
        let parsed = parse_german_date_range("14. – 17.05.2026").expect("range");
        assert_eq!(
            parsed.0,
            CalendarDate {
                year: 2026,
                month: 5,
                day: 14
            }
        );
        assert_eq!(
            parsed.1,
            CalendarDate {
                year: 2026,
                month: 5,
                day: 17
            }
        );
    }

    #[test]
    fn extract_target_date_range_picks_current_year() {
        let html = r#"
            <div>ADAC RAVENOL 24h Nürburgring</div>
            <div>14. &#8211; 17.05.2026</div>
            <div>27. &#8211; 30.05.2027</div>
        "#;
        let lines = html_to_text_lines(html);

        let parsed = extract_date_range_for_event_title(&lines, N24_TARGET_EVENT_TITLE, 2027)
            .expect("year range");
        assert_eq!(parsed.0.day, 27);
        assert_eq!(parsed.1.day, 30);
        assert_eq!(parsed.0.month, 5);
    }

    #[test]
    fn qualifiers_title_matcher_accepts_common_variant() {
        let html = r#"
            <div>ADAC 24h Qualifiers</div>
            <div>18. &#8211; 19.04.2026</div>
            <div>17. &#8211; 18.04.2027</div>
        "#;
        let lines = html_to_text_lines(html);

        let line = lines
            .into_iter()
            .find(|line| line.contains("Qualifiers"))
            .expect("qualifier title line");
        assert!(title_matches_24h_qualifiers(&line));
    }

    #[test]
    fn qualifiers_title_matcher_accepts_nuerburgring_variant() {
        let html = r#"
            <div>ADAC 24h Nürburgring Qualifiers</div>
            <div>17. &#8211; 19.04.2026</div>
            <div>16. &#8211; 18.04.2027</div>
        "#;
        let lines = html_to_text_lines(html);

        let line = lines
            .into_iter()
            .find(|line| line.contains("Nürburgring Qualifiers"))
            .expect("qualifier title line");
        assert!(title_matches_24h_qualifiers(&line));
    }

    #[test]
    fn discovers_termine_url_from_homepage_navigation() {
        let html = r##"
            <nav>
              <a href="#">Termine</a>
              <a href="/language/de/termine-adac-ravenol-nuerburgring-langstrecken-serie-2027/">Termine 2027</a>
            </nav>
        "##;

        let url = discover_termine_url_from_homepage_html(html).expect("termine url");
        assert_eq!(
            url,
            "https://www.nuerburgring-langstrecken-serie.de/language/de/termine-adac-ravenol-nuerburgring-langstrecken-serie-2027/"
        );
    }

    #[test]
    fn parse_termine_entries_preserves_linked_titles_without_dates() {
        let html = r#"
            <table>
              <tbody>
                <tr>
                  <td>11.04.2026</td>
                  <td><a href="https://example.invalid/r3">NLS3: 57. Adenauer ADAC Rundstrecken-Trophy (4h)</a></td>
                </tr>
                <tr>
                  <td>18.-19.04.2026</td>
                  <td><a href="https://example.invalid/24hq">ADAC 24h Qualifiers (2x4h)</a></td>
                </tr>
              </tbody>
            </table>
        "#;

        let entries = parse_termine_entries(html);
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].title,
            "NLS3: 57. Adenauer ADAC Rundstrecken-Trophy (4h)"
        );
        assert_eq!(entries[1].title, "ADAC 24h Qualifiers (2x4h)");
    }

    #[test]
    fn picks_active_termine_title_by_date_range() {
        let entries = vec![
            TermineScheduleEntry {
                start: CalendarDate {
                    year: 2026,
                    month: 4,
                    day: 11,
                },
                end: CalendarDate {
                    year: 2026,
                    month: 4,
                    day: 11,
                },
                title: "NLS3: 57. Adenauer ADAC Rundstrecken-Trophy (4h)".to_string(),
            },
            TermineScheduleEntry {
                start: CalendarDate {
                    year: 2026,
                    month: 4,
                    day: 18,
                },
                end: CalendarDate {
                    year: 2026,
                    month: 4,
                    day: 19,
                },
                title: "ADAC 24h Qualifiers (2x4h)".to_string(),
            },
        ];

        let today = CalendarDate {
            year: 2026,
            month: 4,
            day: 19,
        };
        let title = select_active_termine_event_title(&entries, today).expect("active title");

        assert_eq!(title, "ADAC 24h Qualifiers (2x4h)");
    }

    #[test]
    fn picks_upcoming_termine_title_when_none_active_today() {
        let entries = vec![
            TermineScheduleEntry {
                start: CalendarDate {
                    year: 2026,
                    month: 4,
                    day: 11,
                },
                end: CalendarDate {
                    year: 2026,
                    month: 4,
                    day: 11,
                },
                title: "NLS3: 57. Adenauer ADAC Rundstrecken-Trophy (4h)".to_string(),
            },
            TermineScheduleEntry {
                start: CalendarDate {
                    year: 2026,
                    month: 4,
                    day: 18,
                },
                end: CalendarDate {
                    year: 2026,
                    month: 4,
                    day: 19,
                },
                title: "ADAC 24h Qualifiers (2x4h)".to_string(),
            },
        ];

        let today = CalendarDate {
            year: 2026,
            month: 4,
            day: 16,
        };
        let title = select_active_termine_event_title(&entries, today).expect("fallback title");

        assert_eq!(title, "ADAC 24h Qualifiers (2x4h)");
    }

    #[test]
    fn pid4_prefers_ws_event_name_before_homepage_fallback() {
        let mut header = TimingHeader::default();
        let mut countdown: Option<CountdownState> = None;
        let mut is_race_session = false;
        let payload = r#"{"PID":"4","CUP":"NLS3: 57. Adenauer ADAC Rundstrecken-Trophy (4h)","TRACKSTATE":"0","HEATTYPE":"R","ENDTIME":"0","TIMESTATE":"0","TIME":"12:00:00"}"#;

        let _ = parse_ws_message(
            payload,
            &mut header,
            None,
            Some("24hQ - 18.-19.04.2026 ADAC 24h Nürburgring Qualifiers (2x4h)"),
            &mut countdown,
            &mut is_race_session,
            "20",
        );

        assert_eq!(
            header.event_name,
            "24hQ - 18.-19.04.2026 ADAC 24h Nürburgring Qualifiers (2x4h)"
        );
    }

    #[test]
    fn pid4_uses_websocket_cup_when_dhlm() {
        let mut header = TimingHeader::default();
        let mut countdown: Option<CountdownState> = None;
        let mut is_race_session = false;
        let payload = r#"{"PID":"4","CUP":"Deutsche Historische Langstrecken Meisterschaft (DHLM)","TRACKSTATE":"0","HEATTYPE":"R","ENDTIME":"0","TIMESTATE":"0","TIME":"12:00:00"}"#;

        let _ = parse_ws_message(
            payload,
            &mut header,
            Some("24hQ - 18.-19.04.2026 ADAC 24h Nürburgring Qualifiers (2x4h)"),
            Some("NLS Homepage Fallback"),
            &mut countdown,
            &mut is_race_session,
            "50",
        );

        assert_eq!(
            header.event_name,
            "Deutsche Historische Langstrecken Meisterschaft (DHLM)"
        );
    }

    #[test]
    fn chooses_24h_event_when_today_in_range() {
        let start = CalendarDate {
            year: 2026,
            month: 5,
            day: 14,
        };
        let end = CalendarDate {
            year: 2026,
            month: 5,
            day: 17,
        };
        let today = CalendarDate {
            year: 2026,
            month: 5,
            day: 15,
        };

        let event_id = if today >= start && today <= end {
            N24_EVENT_ID
        } else {
            DEFAULT_NLS_EVENT_ID
        };
        assert_eq!(event_id, N24_EVENT_ID);
    }

    #[test]
    fn chooses_24h_event_when_today_in_qualifiers_range() {
        let qualifiers_start = CalendarDate {
            year: 2026,
            month: 4,
            day: 18,
        };
        let qualifiers_end = CalendarDate {
            year: 2026,
            month: 4,
            day: 19,
        };
        let today = CalendarDate {
            year: 2026,
            month: 4,
            day: 19,
        };

        let event_id = if today >= qualifiers_start && today <= qualifiers_end {
            N24_EVENT_ID
        } else {
            DEFAULT_NLS_EVENT_ID
        };
        assert_eq!(event_id, N24_EVENT_ID);
    }

    #[test]
    fn timeout_helper_sets_tcp_read_timeout() {
        use std::net::{TcpListener, TcpStream};

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("local addr");

        let mut client = TcpStream::connect(addr).expect("connect client");
        let _server = listener.accept().expect("accept client");

        set_tcp_read_timeout(&mut client, Duration::from_millis(1234));
        assert_eq!(
            client.read_timeout().expect("read timeout"),
            Some(Duration::from_millis(1234))
        );
    }

    #[test]
    fn pid4_header_updates_do_not_emit_connected_status_updates() {
        assert!(!should_emit_connected_status_on_update(true, true));
        assert!(!should_emit_connected_status_on_update(true, false));
    }

    #[test]
    fn refresh_failure_keeps_previous_event_id() {
        let mut active_event_id = N24_EVENT_ID;
        let status = refresh_active_event_id(
            &mut active_event_id,
            Err("temporary schedule parse error".to_string()),
        )
        .expect("status message");

        assert_eq!(active_event_id, N24_EVENT_ID);
        assert!(status.contains("keeping eventId"));
        assert!(status.contains(N24_EVENT_ID));
    }
}
