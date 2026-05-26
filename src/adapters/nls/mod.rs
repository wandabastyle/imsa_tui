// NLS websocket adapter: subscribes to livetiming hub events and maps payloads
// to timing rows.

pub mod countdown;
pub mod liveticker;
pub mod protocol;
mod schedule;
pub mod snapshot;
pub mod state;
pub mod websocket;

pub use countdown::CountdownState;

// Re-export main entry points
pub use websocket::{websocket_worker, websocket_worker_with_debug};

#[cfg(test)]
mod tests {
	use serde_json::json;
	use std::time::Duration;

	use crate::adapters::nls::{
		countdown::{self, refresh_header_time_to_go},
		protocol::{parse_ws_message, notices_from_ws_message, entry_from_value, set_tcp_read_timeout, should_emit_connected_status_on_update, refresh_active_event_id},
		schedule::{parse_german_date_range, parse_termine_entries, CalendarDate, TermineScheduleEntry, N24_EVENT_ID, N24_TARGET_EVENT_TITLE, html_to_text_lines, extract_date_range_for_event_title, select_active_termine_event_title, title_matches_24h_qualifiers, discover_termine_url_from_homepage_html, DEFAULT_NLS_EVENT_ID},
		CountdownState,
		snapshot::{NlsSnapshot, restore_snapshot_from_disk, persist_snapshot_if_dirty},
	};
	use crate::timing::{TimingHeader, TimingMessage};
	use crate::timing_persist::SeriesDebugOutput;

   #[test]
   fn current_time_to_end_at_counts_down_for_relative_mode() {
      let header = TimingHeader {
         time_to_go: "-".to_string(),
         ..TimingHeader::default()
      };

      let rendered = countdown::current_time_to_end_at(&header, 120_000, "0", 1_000_000, 1_030_500);
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
   fn current_time_to_end_returns_zero_when_endtime_is_zero() {
      let header = TimingHeader {
         time_to_go: "00:00:01".to_string(),
         ..TimingHeader::default()
      };

      let rendered = countdown::current_time_to_end_at(&header, 0, "0", 0, 1_000_000);
      assert_eq!(rendered, "00:00:00");
   }

   #[test]
   fn refresh_sets_checkered_when_tte_reaches_zero_on_green() {
      let mut header = TimingHeader {
         flag: "Green".to_string(),
         session_name: "Race".to_string(),
         ..TimingHeader::default()
      };
      let countdown = CountdownState {
         end_time_raw:    0,
         time_state_raw:  "0".to_string(),
         received_at_ms:  0,
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
         end_time_raw:    0,
         time_state_raw:  "0".to_string(),
         received_at_ms:  0,
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
         end_time_raw:    0,
         time_state_raw:  "0".to_string(),
         received_at_ms:  0,
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
         end_time_raw:    0,
         time_state_raw:  "0".to_string(),
         received_at_ms:  0,
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
         end_time_raw:    0,
         time_state_raw:  "0".to_string(),
         received_at_ms:  0,
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
   fn restore_sanitizes_event50_race_with_one_second_left_to_zero_and_checkered() {
      use std::sync::mpsc::{
         channel,
         Receiver,
      };

      use crate::adapters::nls::snapshot::restore_snapshot_from_disk;
      use crate::timing_persist::PersistState;

      // Create a temporary directory for the test
      let temp_dir = std::env::temp_dir();
      let snapshot_path = temp_dir.join(format!("nls_test_snapshot_{}.json", std::process::id()));

      // Create persisted snapshot with event 50 race session and 1 second remaining
      // Use minimal JSON that matches the actual struct fields
      let snapshot_json = r#"{
            "saved_unix_ms": 1000000,
            "session_id": "50-123",
            "meaningful_fingerprint": 12345,
            "header": {
                "session_name": "Race",
                "session_type_raw": "R",
                "event_name": "ADAC RAVENOL 24h Nürburgring",
                "event_id": "50",
                "track_name": "Nürburgring",
                "day_time": "12:00:00",
                "flag": "Green",
                "time_to_go": "00:00:01",
                "class_colors": {}
            },
            "entries": [
                {
                    "position": 1,
                    "car_number": "1",
                    "class_name": "SP9",
                    "class_rank": "1",
                    "driver": "Driver",
                    "vehicle": "Car",
                    "team": "Team",
                    "laps": "150",
                    "gap_overall": "-",
                    "gap_class": "-",
                    "gap_next_in_class": "-",
                    "last_lap": "8:00.000",
                    "best_lap": "7:55.000",
                    "sector_1": "-",
                    "sector_2": "-",
                    "sector_3": "-",
                    "sector_4": "-",
                    "sector_5": "-",
                    "best_lap_no": "-",
                    "pit": "-",
                    "pit_stops": "-",
                    "fastest_driver": "-",
                    "stable_id": "1-SP9"
                }
            ]
        }"#;

      std::fs::write(&snapshot_path, snapshot_json).expect("write test snapshot");

      let (tx, rx): (
         std::sync::mpsc::Sender<TimingMessage>,
         Receiver<TimingMessage>,
      ) = channel();
      let mut persist = PersistState::new(Some(snapshot_path.clone()));
      let mut header = TimingHeader::default();
      let mut entries = Vec::new();

      let session_id = restore_snapshot_from_disk(
         &mut persist,
         &mut header,
         &mut entries,
         &tx,
         999,
         &SeriesDebugOutput::Silent,
      );

      // Verify session_id is returned
      assert_eq!(session_id, Some("50-123".to_string()));

      // Verify header was sanitized
      assert_eq!(header.event_id, "50");
      assert_eq!(header.session_type_raw, "R");
      assert_eq!(header.time_to_go, "00:00:00");
      assert_eq!(header.flag, "Checkered");

      // Verify message was sent
      let msg = rx.recv().expect("receive message");
      match msg {
         TimingMessage::Snapshot {
            source_id,
            header: snapshot_header,
            entries: snapshot_entries,
         } => {
            assert_eq!(source_id, 999);
            assert_eq!(snapshot_header.time_to_go, "00:00:00");
            assert_eq!(snapshot_header.flag, "Checkered");
            assert_eq!(snapshot_entries.len(), 1);
         },
         _ => panic!("expected Snapshot message"),
      }

      // Cleanup
      let _ = std::fs::remove_file(&snapshot_path);
   }

   #[test]
   fn restore_leaves_non_event50_snapshots_unmodified() {
      use std::sync::mpsc::{
         channel,
         Receiver,
      };

      use crate::adapters::nls::snapshot::restore_snapshot_from_disk;
      use crate::timing_persist::PersistState;

      // Create a temporary directory for the test
      let temp_dir = std::env::temp_dir();
      let snapshot_path = temp_dir.join(format!(
         "nls_test_snapshot_regular_{}.json",
         std::process::id()
      ));

      // Create persisted snapshot with regular NLS event (not 50)
      // Use minimal JSON that matches the actual struct fields
      let snapshot_json = r#"{
            "saved_unix_ms": 1000000,
            "session_id": "20-456",
            "meaningful_fingerprint": 54321,
            "header": {
                "session_name": "Race",
                "session_type_raw": "R",
                "event_name": "NLS Race",
                "event_id": "20",
                "track_name": "Nürburgring",
                "day_time": "12:00:00",
                "flag": "Green",
                "time_to_go": "00:00:01",
                "class_colors": {}
            },
            "entries": []
        }"#;

      std::fs::write(&snapshot_path, snapshot_json).expect("write test snapshot");

      let (tx, rx): (
         std::sync::mpsc::Sender<TimingMessage>,
         Receiver<TimingMessage>,
      ) = channel();
      let mut persist = PersistState::new(Some(snapshot_path.clone()));
      let mut header = TimingHeader::default();
      let mut entries = Vec::new();

      let session_id = restore_snapshot_from_disk(
         &mut persist,
         &mut header,
         &mut entries,
         &tx,
         999,
         &SeriesDebugOutput::Silent,
      );

      // Verify session_id is returned
      assert_eq!(session_id, Some("20-456".to_string()));

      // Verify header was NOT sanitized - should keep original values
      assert_eq!(header.event_id, "20");
      assert_eq!(header.time_to_go, "00:00:01");
      assert_eq!(header.flag, "Green");

      // Verify message was sent with original values
      let msg = rx.recv().expect("receive message");
      match msg {
         TimingMessage::Snapshot {
            source_id,
            header: snapshot_header,
            entries: _,
         } => {
            assert_eq!(source_id, 999);
            assert_eq!(snapshot_header.time_to_go, "00:00:01");
            assert_eq!(snapshot_header.flag, "Green");
         },
         _ => panic!("expected Snapshot message"),
      }

      // Cleanup
      let _ = std::fs::remove_file(&snapshot_path);
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
   fn entry_from_value_24h_uses_s9_for_pit_state() {
      // For 24h (event_id "50"), S9 contains pit state, S8 contains time
      let row = json!({
          "POSITION": "1",
          "STNR": "84",
          "CLASSNAME": "SP 9",
          "CLASSRANK": "1",
          "NAME": "Driver",
          "CAR": "Car",
          "TEAM": "Team",
          "LAPS": "50",
          "GAP": "-",
          "LASTLAPTIME": "8:20.000",
          "FASTESTLAP": "8:10.000",
          "S8TIME": "1:30.000",
          "S9TIME": "PIT"
      });

      let entry = entry_from_value(&row, "50").expect("entry");
      // sector_5 should display "PIT" when S9 contains pit state
      assert_eq!(entry.sector_5, "PIT");
      // pit should be derived from raw S9 which contains "PIT"
      assert_eq!(entry.pit, "Yes");
   }

   #[test]
   fn entry_from_value_24h_s9_out_means_not_in_pit() {
      let row = json!({
          "POSITION": "1",
          "STNR": "84",
          "CLASSNAME": "SP 9",
          "CLASSRANK": "1",
          "NAME": "Driver",
          "CAR": "Car",
          "TEAM": "Team",
          "LAPS": "50",
          "GAP": "-",
          "LASTLAPTIME": "8:20.000",
          "FASTESTLAP": "8:10.000",
          "S8TIME": "1:30.000",
          "S9TIME": "OUT"
      });

      let entry = entry_from_value(&row, "50").expect("entry");
      // sector_5 should display "OUT" when S9 contains pit out state
      assert_eq!(entry.sector_5, "OUT");
      // pit should be derived from raw S9 which contains "OUT"
      assert_eq!(entry.pit, "No");
   }

   #[test]
   fn parse_german_date_range_handles_24h_format() {
      let parsed = parse_german_date_range("14. – 17.05.2026").expect("range");
      assert_eq!(parsed.0, CalendarDate {
         year:  2026,
         month: 5,
         day:   14,
      });
      assert_eq!(parsed.1, CalendarDate {
         year:  2026,
         month: 5,
         day:   17,
      });
   }

   #[test]
   fn extract_target_date_range_picks_current_year() {
      let html = r"
            <div>ADAC RAVENOL 24h Nürburgring</div>
            <div>14. &#8211; 17.05.2026</div>
            <div>27. &#8211; 30.05.2027</div>
        ";
      let lines = html_to_text_lines(html);

      let parsed = extract_date_range_for_event_title(&lines, N24_TARGET_EVENT_TITLE, 2027)
         .expect("year range");
      assert_eq!(parsed.0.day, 27);
      assert_eq!(parsed.1.day, 30);
      assert_eq!(parsed.0.month, 5);
   }

   #[test]
   fn extract_date_range_finds_body_when_metadata_has_title_first() {
      let html = r#"
            <title>ADAC RAVENOL 24h Nürburgring</title>
            <meta property="og:title" content="ADAC RAVENOL 24h Nürburgring">
            <div>Some other content</div>
            <div>ADAC RAVENOL 24h Nürburgring</div>
            <div>14. &#8211; 17.05.2026</div>
            <div>Other event</div>
        "#;
      let lines = html_to_text_lines(html);

      let parsed = extract_date_range_for_event_title(&lines, N24_TARGET_EVENT_TITLE, 2026)
         .expect("should find date range from body, not metadata");
      assert_eq!(parsed.0.day, 14);
      assert_eq!(parsed.1.day, 17);
      assert_eq!(parsed.0.month, 5);
      assert_eq!(parsed.1.month, 5);
      assert_eq!(parsed.0.year, 2026);
      assert_eq!(parsed.1.year, 2026);
   }

   #[test]
   fn qualifiers_title_matcher_accepts_common_variant() {
      let html = r"
            <div>ADAC 24h Qualifiers</div>
            <div>18. &#8211; 19.04.2026</div>
            <div>17. &#8211; 18.04.2027</div>
        ";
      let lines = html_to_text_lines(html);

      let line = lines
         .into_iter()
         .find(|line| line.contains("Qualifiers"))
         .expect("qualifier title line");
      assert!(title_matches_24h_qualifiers(&line));
   }

   #[test]
   fn qualifiers_title_matcher_accepts_nuerburgring_variant() {
      let html = r"
            <div>ADAC 24h Nürburgring Qualifiers</div>
            <div>17. &#8211; 19.04.2026</div>
            <div>16. &#8211; 18.04.2027</div>
        ";
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
               year:  2026,
               month: 4,
               day:   11,
            },
            end:   CalendarDate {
               year:  2026,
               month: 4,
               day:   11,
            },
            title: "NLS3: 57. Adenauer ADAC Rundstrecken-Trophy (4h)".to_string(),
         },
         TermineScheduleEntry {
            start: CalendarDate {
               year:  2026,
               month: 4,
               day:   18,
            },
            end:   CalendarDate {
               year:  2026,
               month: 4,
               day:   19,
            },
            title: "ADAC 24h Qualifiers (2x4h)".to_string(),
         },
      ];

      let today = CalendarDate {
         year:  2026,
         month: 4,
         day:   19,
      };
      let title = select_active_termine_event_title(&entries, today).expect("active title");

      assert_eq!(title, "ADAC 24h Qualifiers (2x4h)");
   }

   #[test]
   fn picks_upcoming_termine_title_when_none_active_today() {
      let entries = vec![
         TermineScheduleEntry {
            start: CalendarDate {
               year:  2026,
               month: 4,
               day:   11,
            },
            end:   CalendarDate {
               year:  2026,
               month: 4,
               day:   11,
            },
            title: "NLS3: 57. Adenauer ADAC Rundstrecken-Trophy (4h)".to_string(),
         },
         TermineScheduleEntry {
            start: CalendarDate {
               year:  2026,
               month: 4,
               day:   18,
            },
            end:   CalendarDate {
               year:  2026,
               month: 4,
               day:   19,
            },
            title: "ADAC 24h Qualifiers (2x4h)".to_string(),
         },
      ];

      let today = CalendarDate {
         year:  2026,
         month: 4,
         day:   16,
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

      // WebSocket CUP should now win over homepage fallback
      assert_eq!(
         header.event_name,
         "NLS3: 57. Adenauer ADAC Rundstrecken-Trophy (4h)"
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
   fn pid4_prefers_websocket_cup_for_event_id_50() {
      let mut header = TimingHeader::default();
      let mut countdown: Option<CountdownState> = None;
      let mut is_race_session = false;
      let payload = r#"{"PID":"4","CUP":"ADAC RAVENOL 24h Nürburgring","TRACKSTATE":"0","HEATTYPE":"R","ENDTIME":"0","TIMESTATE":"0","TIME":"12:00:00"}"#;

      let _ = parse_ws_message(
         payload,
         &mut header,
         Some("NLS6: 1. ADAC Eifel Trophy (4h)"), // termine_event_name from NLS Termine
         Some("NLS Homepage Fallback"),
         &mut countdown,
         &mut is_race_session,
         "50", // event_id for 24h
      );

      assert_eq!(header.event_name, "ADAC RAVENOL 24h Nürburgring");
   }

   #[test]
   fn pid4_prefers_websocket_cup_for_event_id_20() {
      let mut header = TimingHeader::default();
      let mut countdown: Option<CountdownState> = None;
      let mut is_race_session = false;
      let payload = r#"{"PID":"4","CUP":"ADAC RAVENOL 24h Nürburgring","TRACKSTATE":"0","HEATTYPE":"R","ENDTIME":"0","TIMESTATE":"0","TIME":"12:00:00"}"#;

      let _ = parse_ws_message(
         payload,
         &mut header,
         Some("NLS6: 1. ADAC Eifel Trophy (4h)"), // termine_event_name from NLS Termine
         Some("NLS Homepage Fallback"),
         &mut countdown,
         &mut is_race_session,
         "20", // event_id for regular NLS
      );

      // WebSocket CUP should now win even for non-24h events
      assert_eq!(header.event_name, "ADAC RAVENOL 24h Nürburgring");
   }

   #[test]
   fn pid4_event_id_50_without_ws_cup_uses_24h_fallback_not_termine() {
      let mut header = TimingHeader::default();
      let mut countdown: Option<CountdownState> = None;
      let mut is_race_session = false;
      // Payload without CUP/EVENTNAME
      let payload = r#"{"PID":"4","TRACKSTATE":"0","HEATTYPE":"R","ENDTIME":"0","TIMESTATE":"0","TIME":"12:00:00"}"#;

      let _ = parse_ws_message(
         payload,
         &mut header,
         Some("NLS6: 1. ADAC Eifel Trophy (4h)"),
         Some("NLS Homepage Fallback"),
         &mut countdown,
         &mut is_race_session,
         "50",
      );

      // Should use 24h fallback, not the termine event name
      assert_eq!(header.event_name, "ADAC RAVENOL 24h Nürburgring");
   }

   #[test]
   fn pid0_event_id_50_without_ws_cup_uses_24h_fallback_not_termine() {
      let mut header = TimingHeader::default();
      let mut countdown: Option<CountdownState> = None;
      let mut is_race_session = false;
      // Minimal PID 0 without CUP
      let payload = r#"{"PID":"0","HEATTYPE":"R","RESULT":[]}"#;

      let _ = parse_ws_message(
         payload,
         &mut header,
         Some("NLS6: 1. ADAC Eifel Trophy (4h)"),
         Some("NLS Homepage Fallback"),
         &mut countdown,
         &mut is_race_session,
         "50",
      );

      // Should use 24h fallback, not the termine event name
      assert_eq!(header.event_name, "ADAC RAVENOL 24h Nürburgring");
   }

   #[test]
   fn pid3_extracts_race_messages() {
      let payload = r##"{"PID":"3","MESSAGES":[{"ID":"1","MESSAGETIME":"15:04:42","MESSAGE":"#999 non respect of code 60 - time penalty 95 sec after first lap in race","MESSAGEGROUP":""},{"ID":"2","MESSAGETIME":"15:04:03","MESSAGE":"Reminder for 24H","MESSAGEGROUP":""}]}"##;

      let notices = notices_from_ws_message(payload);
      assert_eq!(notices.len(), 2);
      assert_eq!(notices[0].id, "1");
      assert_eq!(notices[0].time, "15:04:42");
      assert!(notices[0].text.contains("#999"));
      assert_eq!(notices[1].id, "2");
      assert_eq!(notices[1].time, "15:04:03");
      assert_eq!(notices[1].text, "Reminder for 24H");
   }

   #[test]
   fn pid3_ignores_empty_messages() {
      let payload =
         r#"{"PID":"3","MESSAGES":[{"ID":"9","MESSAGETIME":"15:10:00","MESSAGE":"   "}]}"#;
      assert!(notices_from_ws_message(payload).is_empty());
   }

   #[test]
   fn chooses_24h_event_when_today_in_range() {
      let start = CalendarDate {
         year:  2026,
         month: 5,
         day:   14,
      };
      let end = CalendarDate {
         year:  2026,
         month: 5,
         day:   17,
      };
      let today = CalendarDate {
         year:  2026,
         month: 5,
         day:   15,
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
         year:  2026,
         month: 4,
         day:   18,
      };
      let qualifiers_end = CalendarDate {
         year:  2026,
         month: 4,
         day:   19,
      };
      let today = CalendarDate {
         year:  2026,
         month: 4,
         day:   19,
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
      use std::net::{
         TcpListener,
         TcpStream,
      };

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
      let mut active_event_id = N24_EVENT_ID.to_string();
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
