use std::{
   collections::{
      BTreeMap,
      HashMap,
      HashSet,
   },
   net::TcpStream,
   sync::mpsc::{
      Receiver,
      Sender,
   },
   thread,
   time::Duration,
};

use liveticker::parse_commentator_phrase;
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::{
   Map,
   Value,
};
use tungstenite::{
   Error as WsError,
   Message,
   WebSocket,
   connect,
   http::header::{
      HeaderValue,
      ORIGIN,
      USER_AGENT,
   },
   stream::MaybeTlsStream,
};

use crate::{
   adapters::insights::{
      session::{
         MetaSessionItem,
         fetch_meta_sessions_for_series,
         resolve_live_sid_for_series,
      },
      snapshot::{
         Snapshot,
         meaningful_snapshot_fingerprint,
         now_unix_ms,
         persist_snapshot,
         restore_snapshot_from_disk,
         snapshot_path,
      },
   },
   snapshot_runtime::derive_session_identifier,
   timing::{
      TimingClassColor,
      TimingEntry,
      TimingHeader,
      TimingMessage,
      TimingNotice,
      WecLivetickerEntry,
      canonicalize_class_name,
   },
   timing_persist::{
      PersistState,
      SeriesDebugOutput,
      debounce_elapsed,
      log_series_debug,
   },
};

pub mod liveticker;

const WEC_SERIES_ID: u64 = 10;
const NEGOTIATE_URL: &str =
   "https://insights.griiip.com/live-session-stream/negotiate?negotiateVersion=1";
const ORIGIN_URL: &str = "https://insights.griiip.com";
const LIVE_BASE_URL: &str = "https://insights.griiip.com/live";
const RECONNECT_DELAY: Duration = Duration::from_secs(4);
const SNAPSHOT_SAVE_DEBOUNCE: Duration = Duration::from_mins(3);
const SIGNALR_RS: char = '\u{1e}';
// Legacy: per-channel groups like SID-<sid>-ranks, SID-<sid>-gaps, etc.
// Kept for compatibility if server changes back to granular channel
// subscriptions. const WEC_SIGNALR_CHANNELS: &[&str; 8] = &[
//    "session-info",
//    "participants",
//    "ranks",
//    "gaps",
//    "laps",
//    "sectors",
//    "race-flags",
//    "session-clock",
// ];

/// Join the session-level group SID-<sid> (replaces granular per-channel
/// groups).
fn join_session_group(
   socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
   invocation_id: &mut u64,
   sid: u64,
) -> Result<(), String> {
   let group = format!("SID-{sid}");
   let payload = serde_json::json!({
       "type": 1,
       "invocationId": invocation_id.to_string(),
       "target": "JoinGroup",
       "arguments": [group],
   });
   *invocation_id += 1;
   send_signalr_json(socket, &payload)
}

#[derive(Debug, Deserialize)]
struct NegotiateResponse {
   url:          String,
   #[serde(rename = "accessToken")]
   access_token: String,
}

#[derive(Debug)]
enum SignalRFrame {
   HandshakeAck,
   Invocation {
      target:    String,
      arguments: Vec<Value>,
   },
   Completion {
      invocation_id: Option<String>,
      error:         Option<String>,
   },
   Ping,
   Close,
   Unknown,
}

type WecSnapshot = Snapshot;

#[derive(Debug, Deserialize)]
struct SessionResultsResponse {
   results: Vec<SessionResultRow>,
}

#[derive(Debug, Deserialize)]
struct SessionResultRow {
   #[serde(rename = "sessionParticipantId")]
   session_participant_id:      u64,
   #[serde(rename = "overallFinishedAt")]
   overall_finished_at:         Option<u32>,
   #[serde(rename = "finishedAt")]
   finished_at:                 Option<u32>,
   #[serde(rename = "overallGapFromFirst")]
   overall_gap_from_first:      Option<i64>,
   #[serde(rename = "overallGapFromFirstLaps")]
   overall_gap_from_first_laps: Option<i64>,
   #[serde(rename = "gapFromFirst")]
   gap_from_first:              Option<i64>,
   #[serde(rename = "gapFromFirstLaps")]
   gap_from_first_laps:         Option<i64>,
   #[serde(rename = "numberOfLapsCompleted")]
   number_of_laps_completed:    Option<u32>,
   #[serde(rename = "bestLapTime")]
   best_lap_time:               Option<i64>,
   #[serde(rename = "bestSectorsMillis1")]
   best_sector_1_ms:            Option<i64>,
   #[serde(rename = "bestSectorsMillis2")]
   best_sector_2_ms:            Option<i64>,
   #[serde(rename = "bestSectorsMillis3")]
   best_sector_3_ms:            Option<i64>,
}

#[derive(Debug, Deserialize)]
struct SessionParticipantRow {
   id:           u64,
   #[serde(rename = "carNumber")]
   car_number:   Option<String>,
   #[serde(rename = "classId")]
   class_id:     Option<String>,
   #[serde(rename = "teamName")]
   team_name:    Option<String>,
   manufacturer: Option<String>,
   #[serde(rename = "displayName")]
   display_name: Option<String>,
   #[serde(default)]
   drivers:      Vec<ParticipantDriver>,
}

#[derive(Debug, Deserialize)]
struct ParticipantDriver {
   #[serde(rename = "displayName")]
   display_name: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct WecLiveState {
   header:            TimingHeader,
   rows:              HashMap<String, WecCarState>,
   class_names:       HashMap<String, String>,
   class_colors:      BTreeMap<String, TimingClassColor>,
   /// Session length in milliseconds (for TTE calculation)
   session_length_ms: Option<u64>,
}

#[derive(Debug, Default, Clone)]
struct WecCarState {
   car_number:        String,
   class_id:          Option<String>,
   class_name:        Option<String>,
   position:          Option<u32>,
   class_rank:        Option<u32>,
   driver:            Option<String>,
   vehicle:           Option<String>,
   team:              Option<String>,
   laps:              Option<u32>,
   gap_overall:       Option<String>,
   gap_next_in_class: Option<String>,
   last_lap_ms:       Option<i64>,
   best_lap_ms:       Option<i64>,
   best_lap_no:       Option<u32>,
   pit:               Option<bool>,
   sector_times:      [Option<i64>; 3],
   sector_laps:       [Option<u32>; 3],
}

pub fn websocket_worker(tx: &Sender<TimingMessage>, source_id: u64, stop_rx: &Receiver<()>) {
   websocket_worker_with_debug(tx, source_id, stop_rx, &SeriesDebugOutput::Silent);
}

pub fn websocket_worker_with_debug(
   tx: &Sender<TimingMessage>,
   source_id: u64,
   stop_rx: &Receiver<()>,
   debug_output: &SeriesDebugOutput,
) {
   let client = match Client::builder().timeout(Duration::from_secs(12)).build() {
      Ok(client) => client,
      Err(err) => {
         let _ = tx.send(TimingMessage::Error {
            source_id,
            text: format!("WEC HTTP client init failed: {err}"),
         });
         return;
      },
   };

   let mut persist = PersistState::new(snapshot_path("wec_snapshot.json"));
   let mut last_snapshot =
      restore_snapshot_from_disk(&mut persist, tx, source_id, "WEC", debug_output);
   if last_snapshot.is_some() {
      let _ = tx.send(TimingMessage::Status {
         source_id,
         text: "[SNAPSHOT] Restored from saved data".to_string(),
      });
   }
   let mut last_session_id = last_snapshot
      .as_ref()
      .and_then(|snapshot| snapshot.session_id.clone());
   let mut fallback_detail_logged = false;

   'outer: loop {
      if stop_rx.try_recv().is_ok() {
         if let Some(snapshot) = last_snapshot.as_ref() {
            if persist.dirty_since_last_save {
               persist_snapshot(&mut persist, snapshot, now_unix_ms(), "WEC", debug_output);
            }
         }
         break;
      }

      let _ = tx.send(TimingMessage::Status {
         source_id,
         text: "Connecting to WEC live stream...".to_string(),
      });

      let sid = match resolve_active_sid(&client) {
         Ok(sid) => {
            fallback_detail_logged = false;
            sid
         },
         Err(err) => {
            match fetch_latest_finished_race_snapshot(&client) {
               Ok(snapshot) => {
                  emit_snapshot(
                     (tx, source_id),
                     snapshot.header,
                     snapshot.entries,
                     &mut persist,
                     &mut last_snapshot,
                     &mut last_session_id,
                     debug_output,
                  );
                  if !fallback_detail_logged {
                     log_series_debug(
                        debug_output,
                        "WEC",
                        format!(
                           "No active FIA WEC live session; showing latest finished race results \
                            [ts={}]",
                           now_unix_ms()
                        ),
                     );
                     fallback_detail_logged = true;
                  }
                  let _ = tx.send(TimingMessage::Status {
                     source_id,
                     text: "WEC offline: latest race results".to_string(),
                  });
               },
               Err(fallback_err) => {
                  let _ = tx.send(TimingMessage::Error {
                     source_id,
                     text: format!("{err}; fallback failed: {fallback_err}"),
                  });
               },
            }
            if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
               break;
            }
            continue;
         },
      };

      let negotiated = match negotiate(&client) {
         Ok(negotiated) => negotiated,
         Err(err) => {
            let _ = tx.send(TimingMessage::Error {
               source_id,
               text: err,
            });
            if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
               break;
            }
            continue;
         },
      };

      let ws_url = websocket_url_from_negotiate(&negotiated.url, &negotiated.access_token);
      let request = match build_request(&ws_url) {
         Ok(request) => request,
         Err(err) => {
            let _ = tx.send(TimingMessage::Error {
               source_id,
               text: err,
            });
            if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
               break;
            }
            continue;
         },
      };

      let (mut socket, _) = match connect(request) {
         Ok(pair) => pair,
         Err(err) => {
            let _ = tx.send(TimingMessage::Error {
               source_id,
               text: format!("WEC websocket connect failed: {err}"),
            });
            if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
               break;
            }
            continue;
         },
      };
      set_socket_timeout(&mut socket);

      if let Err(err) = send_signalr_handshake(&mut socket) {
         let _ = tx.send(TimingMessage::Error {
            source_id,
            text: err,
         });
         if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
            break;
         }
         continue;
      }

      let mut handshake_complete = false;
      for _ in 0..6 {
         match read_signalr_text(&mut socket) {
            Ok(Some(raw)) => {
               let mut failed = false;
               for frame in split_signalr_frames(&raw) {
                  match parse_signalr_frame(frame) {
                     SignalRFrame::HandshakeAck => handshake_complete = true,
                     SignalRFrame::Close => {
                        failed = true;
                        break;
                     },
                     _ => {},
                  }
               }
               if failed || handshake_complete {
                  break;
               }
            },
            Ok(None) => {},
            Err(err) => {
               let _ = tx.send(TimingMessage::Error {
                  source_id,
                  text: err,
               });
               break;
            },
         }
      }

      if !handshake_complete {
         let _ = tx.send(TimingMessage::Error {
            source_id,
            text: "WEC SignalR handshake did not complete".to_string(),
         });
         if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
            break;
         }
         continue;
      }

      // Join the single session-level group (SID-<sid>) instead of multiple
      // per-channel groups. This is the new SignalR subscription strategy that
      // replaces the granular channel approach.
      let mut invocation_id = 1_u64;
      if let Err(err) = join_session_group(&mut socket, &mut invocation_id, sid) {
         let _ = tx.send(TimingMessage::Error {
            source_id,
            text: err,
         });
         if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
            break 'outer;
         }
         continue 'outer;
      }

      // Legacy: join multiple per-channel groups (kept for reference, disabled).
      // for channel in WEC_SIGNALR_CHANNELS {
      //    if let Err(err) = join_group(&mut socket, &mut invocation_id, sid,
      // channel) {       let _ = tx.send(TimingMessage::Error {
      //          source_id,
      //          text: err,
      //       });
      //       if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
      //          break 'outer;
      //       }
      //       continue 'outer;
      //    }
      // }

      let mut live_state = WecLiveState::default();
      if let Err(err) = bootstrap_live_state(&client, sid, &mut live_state) {
         let _ = tx.send(TimingMessage::Error {
            source_id,
            text: err,
         });
      } else if let Some((header, entries)) = snapshot_from_live_state(&live_state) {
         emit_snapshot(
            (tx, source_id),
            header,
            entries,
            &mut persist,
            &mut last_snapshot,
            &mut last_session_id,
            debug_output,
         );
      }

      let _ = tx.send(TimingMessage::Status {
         source_id,
         text: format!("WEC stream connected (sid={sid})"),
      });

      // Spawn race control messages polling thread
      let (rc_stop_tx, rc_stop_rx) = std::sync::mpsc::channel::<()>();
      let rc_client = client.clone();
      let rc_thread =
         spawn_racecontrol_polling_thread(tx.clone(), source_id, rc_stop_rx, rc_client, sid);

      loop {
         if stop_rx.try_recv().is_ok() {
            if let Some(snapshot) = last_snapshot.as_ref() {
               if persist.dirty_since_last_save {
                  persist_snapshot(&mut persist, snapshot, now_unix_ms(), "WEC", debug_output);
               }
            }
            break 'outer;
         }

         let raw = match read_signalr_text(&mut socket) {
            Ok(raw) => raw,
            Err(err) => {
               let _ = tx.send(TimingMessage::Error {
                  source_id,
                  text: err,
               });
               break;
            },
         };

         let Some(raw) = raw else {
            continue;
         };

         let mut closed = false;
         let mut liveticker_entries: Vec<WecLivetickerEntry> = Vec::new();
         for frame in split_signalr_frames(&raw) {
            match parse_signalr_frame(frame) {
               SignalRFrame::Invocation { target, arguments } => {
                  if !target.starts_with("lv-") && target != "ReceiveBatch" {
                     continue;
                  }
                  // Extract liveticker entries from ReceiveBatch
                  if target == "ReceiveBatch" {
                     liveticker_entries.extend(extract_liveticker_entries(&arguments));
                  }
                  if apply_signalr_arguments(&mut live_state, &target, &arguments) {
                     if let Some((header, entries)) = snapshot_from_live_state(&live_state) {
                        emit_snapshot(
                           (tx, source_id),
                           header,
                           entries,
                           &mut persist,
                           &mut last_snapshot,
                           &mut last_session_id,
                           debug_output,
                        );
                     }
                  }
               },
               SignalRFrame::Completion {
                  invocation_id,
                  error,
               } => {
                  if let Some(error) = error {
                     let label = invocation_id.unwrap_or_else(|| "?".to_string());
                     let _ = tx.send(TimingMessage::Error {
                        source_id,
                        text: format!("WEC SignalR invocation {label} failed: {error}"),
                     });
                  }
               },
               SignalRFrame::Close => {
                  closed = true;
                  break;
               },
               SignalRFrame::Ping | SignalRFrame::HandshakeAck | SignalRFrame::Unknown => {},
            }
         }

         // Send liveticker entries if any were collected
         if !liveticker_entries.is_empty() {
            let _ = tx.send(TimingMessage::WecLiveticker {
               source_id,
               entries: liveticker_entries,
            });
         }

         if closed {
            let _ = tx.send(TimingMessage::Error {
               source_id,
               text: "WEC websocket closed".to_string(),
            });
            break;
         }
      }

      let _ = tx.send(TimingMessage::Status {
         source_id,
         text: "WEC reconnecting in 4s...".to_string(),
      });

      // Stop the race control polling thread
      let _ = rc_stop_tx.send(());
      drop(rc_thread);

      if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
         break;
      }
   }
}

fn send_signalr_handshake(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>) -> Result<(), String> {
   let payload = format!("{{\"protocol\":\"json\",\"version\":1}}{SIGNALR_RS}");
   socket
      .send(Message::Text(payload.into()))
      .map_err(|err| format!("WEC handshake send failed: {err}"))
}

fn resolve_active_sid(client: &Client) -> Result<u64, String> {
   resolve_live_sid_for_series(client, WEC_SERIES_ID)
      .map_err(|err| format!("No active FIA WEC session found in live schedule ({err})"))
}

#[cfg(test)]
#[derive(Debug, Clone)]
struct SessionScheduleItem {
   sid:               u64,
   is_started:        bool,
   connection_status: Option<String>,
}

#[cfg(test)]
fn choose_candidate_sids(sessions: &[SessionScheduleItem]) -> Result<Vec<u64>, String> {
   if sessions.is_empty() {
      return Err("WEC session schedule returned no sessions".to_string());
   }

   let mut prioritized = Vec::with_capacity(sessions.len());
   for session in sessions.iter().filter(|session| {
      session.is_started && !is_closed_status(session.connection_status.as_deref())
   }) {
      prioritized.push(session.sid);
   }
   for session in sessions {
      if !prioritized.contains(&session.sid) {
         prioritized.push(session.sid);
      }
   }
   Ok(prioritized)
}

#[cfg(test)]
fn is_closed_status(status: Option<&str>) -> bool {
   let Some(status) = status else {
      return false;
   };
   let normalized = status.trim().to_ascii_lowercase();
   normalized == "closed" || normalized == "ended" || normalized == "finished"
}

fn fetch_latest_finished_race_snapshot(client: &Client) -> Result<WecSnapshot, String> {
   let sessions = fetch_meta_sessions_for_series(client, WEC_SERIES_ID)
      .map_err(|err| format!("WEC meta sessions request failed: {err}"))?;
   let Some(session) = choose_latest_finished_race_session(&sessions) else {
      return Err("No finished FIA WEC race session with results found".to_string());
   };

   let results_url = format!(
      "https://insights.griiip.com/meta/sessions/{}/results",
      session.id
   );
   let results_response = client.get(&results_url).send().map_err(|err| {
      format!(
         "WEC results request failed for session {}: {err}",
         session.id
      )
   })?;
   if !results_response.status().is_success() {
      return Err(format!(
         "WEC results request failed for session {} with HTTP {}",
         session.id,
         results_response.status()
      ));
   }
   let results_body = results_response.text().map_err(|err| {
      format!(
         "WEC results body read failed for session {}: {err}",
         session.id
      )
   })?;
   let results_payload =
      serde_json::from_str::<SessionResultsResponse>(&results_body).map_err(|err| {
         format!(
            "WEC results decode failed for session {}: {err}",
            session.id
         )
      })?;

   let participants_url = format!(
      "https://insights.griiip.com/meta/sessions/{}/participants",
      session.id
   );
   let participants_response = client.get(&participants_url).send().map_err(|err| {
      format!(
         "WEC participants request failed for session {}: {err}",
         session.id
      )
   })?;
   if !participants_response.status().is_success() {
      return Err(format!(
         "WEC participants request failed for session {} with HTTP {}",
         session.id,
         participants_response.status()
      ));
   }
   let participants_body = participants_response.text().map_err(|err| {
      format!(
         "WEC participants body read failed for session {}: {err}",
         session.id
      )
   })?;
   let participants = serde_json::from_str::<Vec<SessionParticipantRow>>(&participants_body)
      .map_err(|err| {
         format!(
            "WEC participants decode failed for session {}: {err}",
            session.id
         )
      })?;

   let mut participants_by_id = HashMap::new();
   for participant in participants {
      participants_by_id.insert(participant.id, participant);
   }

   let mut entries = Vec::new();
   for (idx, row) in results_payload.results.into_iter().enumerate() {
      let participant = participants_by_id.get(&row.session_participant_id);
      let car_number = participant
         .and_then(|item| item.car_number.as_deref())
         .map(str::trim)
         .filter(|value| !value.is_empty())
         .unwrap_or("-")
         .to_string();

      let class_name = participant
         .and_then(|item| item.class_id.as_deref())
         .map_or_else(|| "-".to_string(), format_wec_class_name);

      let driver_name = participant
         .and_then(|item| item.drivers.first())
         .and_then(|driver| driver.display_name.as_deref())
         .map_or_else(|| "-".to_string(), normalize_driver_name);

      let team_name = participant
         .and_then(|item| item.team_name.clone())
         .or_else(|| participant.and_then(|item| item.display_name.clone()))
         .unwrap_or_else(|| "-".to_string());

      let vehicle = participant
         .and_then(|item| item.manufacturer.clone())
         .unwrap_or_else(|| "-".to_string());

      let position = row
         .overall_finished_at
         .unwrap_or(u32::try_from(idx + 1).expect("position should fit in u32"));
      let class_rank = row
         .finished_at
         .map_or_else(|| "-".to_string(), |rank| rank.to_string());

      let stable_id = if car_number == "-" {
         format!("wec:participant:{}", row.session_participant_id)
      } else {
         format!("wec:{car_number}")
      };

      entries.push(TimingEntry {
         position,
         car_number,
         class_name,
         class_rank,
         driver: driver_name,
         vehicle,
         team: team_name,
         laps: row
            .number_of_laps_completed
            .map_or_else(|| "-".to_string(), |laps| laps.to_string()),
         gap_overall: format_gap(row.overall_gap_from_first, row.overall_gap_from_first_laps)
            .unwrap_or_else(|| "-".to_string()),
         gap_class: "-".to_string(),
         gap_next_in_class: format_gap(row.gap_from_first, row.gap_from_first_laps)
            .unwrap_or_else(|| "-".to_string()),
         last_lap: "-".to_string(),
         best_lap: row
            .best_lap_time
            .map_or_else(|| "-".to_string(), format_lap_time_ms),
         sector_1: row
            .best_sector_1_ms
            .map_or_else(|| "-".to_string(), format_sector_time_ms),
         sector_2: row
            .best_sector_2_ms
            .map_or_else(|| "-".to_string(), format_sector_time_ms),
         sector_3: row
            .best_sector_3_ms
            .map_or_else(|| "-".to_string(), format_sector_time_ms),
         sector_4: "-".to_string(),
         sector_5: "-".to_string(),
         best_lap_no: "-".to_string(),
         pit: "No".to_string(),
         pit_stops: "-".to_string(),
         fastest_driver: "-".to_string(),
         stable_id,
      });
   }

   entries.sort_by_key(|entry| entry.position);

   let mut header = TimingHeader {
      session_name: session.name.clone().unwrap_or_else(|| "Race".to_string()),
      session_type_raw: session
         .session_type
         .clone()
         .unwrap_or_else(|| "Race".to_string()),
      event_name: session
         .event
         .as_ref()
         .and_then(|event| event.name.clone())
         .unwrap_or_else(|| "FIA WEC".to_string()),
      track_name: session
         .track_config
         .as_ref()
         .and_then(|track| track.name.clone())
         .or_else(|| {
            session
               .event
               .as_ref()
               .and_then(|event| event.track_config.as_ref())
               .and_then(|track| track.name.clone())
         })
         .unwrap_or_else(|| "-".to_string()),
      day_time: "-".to_string(),
      flag: "Checkered".to_string(),
      time_to_go: "00:00".to_string(),
      ..TimingHeader::default()
   };
   header
      .class_colors
      .insert("HYPER".to_string(), TimingClassColor {
         color: "#e21e19".to_string(),
      });
   header
      .class_colors
      .insert("LMGT3".to_string(), TimingClassColor {
         color: "#0b9314".to_string(),
      });

   let session_id = derive_session_identifier(&header);
   let fingerprint = meaningful_snapshot_fingerprint(&header, &entries);

   Ok(WecSnapshot {
      header,
      entries,
      session_id,
      fingerprint,
      extra: (),
   })
}

fn choose_latest_finished_race_session(sessions: &[MetaSessionItem]) -> Option<MetaSessionItem> {
   sessions
      .iter()
      .filter(|session| {
         session
            .session_type
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case("Race"))
            && session.has_result
            && !session.is_running
      })
      .max_by_key(|session| {
         session
            .end_time
            .clone()
            .or_else(|| session.start_time.clone())
            .unwrap_or_default()
      })
      .cloned()
}

fn format_wec_class_name(raw: &str) -> String {
   canonicalize_class_name(raw)
}

fn negotiate(client: &Client) -> Result<NegotiateResponse, String> {
   let response = client
      .post(NEGOTIATE_URL)
      .body("")
      .send()
      .map_err(|err| format!("WEC negotiate request failed: {err}"))?;
   if !response.status().is_success() {
      return Err(format!(
         "WEC negotiate failed with HTTP {}",
         response.status()
      ));
   }
   let body = response
      .text()
      .map_err(|err| format!("WEC negotiate body read failed: {err}"))?;
   serde_json::from_str::<NegotiateResponse>(&body)
      .map_err(|err| format!("WEC negotiate decode failed: {err}"))
}

fn websocket_url_from_negotiate(base_url: &str, token: &str) -> String {
   let mut ws_url = base_url
      .strip_prefix("https://")
      .map_or_else(|| base_url.to_string(), |rest| format!("wss://{rest}"));
   let separator = if ws_url.contains('?') { '&' } else { '?' };
   ws_url.push(separator);
   ws_url.push_str("access_token=");
   ws_url.push_str(token);
   ws_url
}

// Legacy: kept for fallback if per-channel group subscription is needed.
// fn join_group(
//    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
//    invocation_id: &mut u64,
//    sid: u64,
//    channel: &str,
// ) -> Result<(), String> {
//    let group = format!("SID-{sid}-{channel}");
//    let payload = serde_json::json!({
//        "type": 1,
//        "invocationId": invocation_id.to_string(),
//        "target": "JoinGroup",
//        "arguments": [group],
//    });
//    *invocation_id += 1;
//    send_signalr_json(socket, &payload)
// }

fn send_signalr_json(
   socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
   payload: &Value,
) -> Result<(), String> {
   let mut encoded = serde_json::to_string(payload)
      .map_err(|err| format!("WEC SignalR payload encode failed: {err}"))?;
   encoded.push(SIGNALR_RS);
   socket
      .send(Message::Text(encoded.into()))
      .map_err(|err| format!("WEC SignalR send failed: {err}"))
}

fn build_request(url: &str) -> Result<tungstenite::handshake::client::Request, String> {
   let mut request = tungstenite::client::IntoClientRequest::into_client_request(url)
      .map_err(|err| format!("failed to build websocket request: {err}"))?;
   request
      .headers_mut()
      .insert(ORIGIN, HeaderValue::from_static(ORIGIN_URL));
   request
      .headers_mut()
      .insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0"));
   Ok(request)
}

fn set_socket_timeout(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>) {
   if let MaybeTlsStream::Plain(stream) = socket.get_mut() {
      let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
   }
}

fn read_signalr_text(
   socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
) -> Result<Option<String>, String> {
   match socket.read() {
      Ok(Message::Text(text)) => Ok(Some(text.to_string())),
      Ok(Message::Binary(data)) => Ok(String::from_utf8(data.to_vec()).ok()),
      Ok(Message::Ping(data)) => {
         socket
            .send(Message::Pong(data))
            .map_err(|err| format!("WEC ping/pong handling failed: {err}"))?;
         Ok(None)
      },
      Ok(Message::Pong(_) | Message::Frame(_)) => Ok(None),
      Ok(Message::Close(_)) => Ok(Some(format!("{{\"type\":7}}{SIGNALR_RS}"))),
      Err(WsError::Io(err))
         if err.kind() == std::io::ErrorKind::WouldBlock
            || err.kind() == std::io::ErrorKind::TimedOut =>
      {
         Ok(None)
      },
      Err(err) => Err(format!("WEC websocket read failed: {err}")),
   }
}

fn split_signalr_frames(raw: &str) -> Vec<&str> {
   raw.split(SIGNALR_RS)
      .map(str::trim)
      .filter(|frame| !frame.is_empty())
      .collect()
}

fn parse_signalr_frame(frame: &str) -> SignalRFrame {
   if frame == "{}" {
      return SignalRFrame::HandshakeAck;
   }
   let Ok(value) = serde_json::from_str::<Value>(frame) else {
      return SignalRFrame::Unknown;
   };
   let Some(message_type) = value.get("type").and_then(Value::as_u64) else {
      return SignalRFrame::Unknown;
   };

   match message_type {
      1 => {
         let target = value
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
         let arguments = value
            .get("arguments")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
         SignalRFrame::Invocation { target, arguments }
      },
      3 => {
         SignalRFrame::Completion {
            invocation_id: value
               .get("invocationId")
               .and_then(Value::as_str)
               .map(str::to_string),
            error:         value
               .get("error")
               .and_then(Value::as_str)
               .map(str::to_string),
         }
      },
      6 => SignalRFrame::Ping,
      7 => SignalRFrame::Close,
      _ => SignalRFrame::Unknown,
   }
}

fn bootstrap_live_state(client: &Client, sid: u64, state: &mut WecLiveState) -> Result<(), String> {
   // Fetch session length from schedule endpoint first
   fetch_session_length_from_schedule(client, sid, state)?;

   apply_session_info(state, &fetch_live_json(client, sid, "session-info")?);
   apply_session_clock(state, &fetch_live_json(client, sid, "session-clock")?);
   apply_race_flags(state, &fetch_live_json(client, sid, "race-flags")?);
   apply_participants(state, &fetch_live_json(client, sid, "participants")?);
   apply_ranks(state, &fetch_live_json(client, sid, "ranks")?);
   apply_gaps(state, &fetch_live_json(client, sid, "gaps")?, true);
   apply_laps(state, &fetch_live_json(client, sid, "laps")?);
   apply_sectors(state, &fetch_live_json(client, sid, "sectors")?);
   Ok(())
}

/// Fetch session length from the schedule endpoint's lengthLimit field.
fn fetch_session_length_from_schedule(
   client: &Client,
   sid: u64,
   state: &mut WecLiveState,
) -> Result<(), String> {
   let url = format!("{ORIGIN_URL}/meta/sessions-schedule-live");
   let response = client
      .get(&url)
      .send()
      .map_err(|err| format!("WEC schedule request failed: {err}"))?;

   if !response.status().is_success() {
      return Err(format!(
         "WEC schedule failed with HTTP {}",
         response.status()
      ));
   }

   let body = response
      .text()
      .map_err(|err| format!("WEC schedule body read failed: {err}"))?;

   let sessions: Vec<Value> =
      serde_json::from_str(&body).map_err(|err| format!("WEC schedule decode failed: {err}"))?;

   // Find the session matching our sid
   for session in sessions {
      if let Some(session_sid) = session.get("sid").and_then(Value::as_u64) {
         if session_sid == sid {
            // Extract lengthLimit.timeLimitSeconds
            if let Some(length_limit) = session.get("lengthLimit").and_then(Value::as_object) {
               if let Some(time_limit_secs) =
                  length_limit.get("timeLimitSeconds").and_then(Value::as_i64)
               {
                  if time_limit_secs > 0 {
                     state.session_length_ms =
                        Some(u64::try_from(time_limit_secs).unwrap_or(0) * 1000);
                     return Ok(());
                  }
               }
            }
         }
      }
   }

   Err(format!(
      "Session {sid} not found in schedule or lengthLimit missing"
   ))
}

fn fetch_live_json(client: &Client, sid: u64, route: &str) -> Result<Value, String> {
   let url = format!("{LIVE_BASE_URL}/{route}/{sid}");
   let response = client
      .get(&url)
      .send()
      .map_err(|err| format!("WEC bootstrap request failed ({route}): {err}"))?;
   if !response.status().is_success() {
      return Err(format!(
         "WEC bootstrap endpoint {route} failed with HTTP {}",
         response.status()
      ));
   }
   let body = response
      .text()
      .map_err(|err| format!("WEC bootstrap body read failed ({route}): {err}"))?;
   serde_json::from_str::<Value>(&body)
      .map_err(|err| format!("WEC bootstrap decode failed ({route}): {err}"))
}

fn apply_signalr_arguments(state: &mut WecLiveState, target: &str, arguments: &[Value]) -> bool {
   let mut changed = false;
   if target == "ReceiveBatch" {
      return apply_receive_batch(state, arguments);
   }
   for argument in arguments {
      let was_changed = match target {
         "lv-session-info" => apply_session_info(state, argument),
         "lv-session-clock" => apply_session_clock(state, argument),
         "lv-race-flags" => apply_race_flags(state, argument),
         "lv-participants" => apply_participants(state, argument),
         "lv-ranks" => apply_ranks(state, argument),
         "lv-gaps" => apply_gaps(state, argument, true),
         "lv-laps" => apply_laps(state, argument),
         "lv-sectors" => apply_sectors(state, argument),
         _ => false,
      };
      changed |= was_changed;
   }
   changed
}

fn apply_receive_batch(state: &mut WecLiveState, arguments: &[Value]) -> bool {
   let mut changed = false;
   let mut latest_elapsed_ms: Option<i64> = None;

   for argument in arguments {
      // Each argument should have an "items" array
      let items = argument.get("items").and_then(Value::as_array);
      let Some(items) = items else {
         continue;
      };
      for item in items {
         let channel = item.get("channel").and_then(Value::as_str);
         let view = item.get("view");

         // Capture elapsed time from any item that has it
         if let Some(view_obj) = item.get("view").and_then(Value::as_object) {
            if let Some(elapsed) = map_i64(view_obj, "elapsedTimeMillis") {
               latest_elapsed_ms = Some(elapsed);
            }
         }

         let was_changed = match channel {
            // official-rank channel is intentionally ignored to prevent
            // bad gap_overall updates (e.g., "+1 L" overwrites)
            Some("official-rank" | "sector-cross-updates") => {
               apply_sector_cross_updates(state, view)
            },
            Some("laps") => {
               if let Some(view) = view {
                  apply_laps(state, view)
               } else {
                  false
               }
            },
            // Handle race-flags via ReceiveBatch (in addition to direct lv-race-flags)
            Some("race-flags") => {
               if let Some(view) = view {
                  apply_race_flags(state, view)
               } else {
                  false
               }
            },
            // commentator-phrase is handled separately for liveticker,
            // it doesn't affect timing data
            _ => false,
         };
         changed |= was_changed;
      }
   }

   // Recalculate TTE whenever we have elapsed time from any batch item
   if let (Some(elapsed_ms), Some(session_length_ms)) = (latest_elapsed_ms, state.session_length_ms)
   {
      if elapsed_ms >= 0 {
         let elapsed_u64 = u64::try_from(elapsed_ms).unwrap_or(0);
         let remaining_ms = session_length_ms.saturating_sub(elapsed_u64);
         if set_header_text(
            &mut state.header.time_to_go,
            Some(format_clock_ms(remaining_ms)),
         ) {
            changed = true;
         }
      }
   }

   changed
}

/// Extract liveticker entries from `ReceiveBatch` arguments.
fn extract_liveticker_entries(arguments: &[Value]) -> Vec<WecLivetickerEntry> {
   let mut entries = Vec::new();
   for argument in arguments {
      let items = argument.get("items").and_then(Value::as_array);
      let Some(items) = items else {
         continue;
      };
      for item in items {
         let channel = item.get("channel").and_then(Value::as_str);
         if channel == Some("commentator-phrase") {
            if let Some(view) = item.get("view") {
               if let Some(entry) = parse_commentator_phrase(view) {
                  entries.push(entry);
               }
            }
         }
      }
   }
   entries
}

#[cfg(test)]
fn apply_official_rank(state: &mut WecLiveState, view: Option<&Value>) -> bool {
   let Some(view) = view.and_then(Value::as_object) else {
      return false;
   };
   let mut changed = false;

   // Build a synthetic row that existing handlers can process
   // Extract pid or carNumber from the view
   let pid = map_i64(view, "pid");
   let car_number = map_str(view, "carNumber");

   if pid.is_none() && car_number.is_none() {
      return false;
   }

   // Create a synthetic map that upsert_car_state can use
   let mut synthetic_map = Map::new();
   if let Some(pid) = pid {
      synthetic_map.insert("pid".to_string(), Value::Number(pid.into()));
   }
   if let Some(ref car) = car_number {
      synthetic_map.insert("carNumber".to_string(), Value::String(car.clone()));
   }

   let Some((key, entry)) = upsert_car_state(state, &synthetic_map) else {
      return false;
   };

   // Update position from view.position
   if let Some(position) = map_u32(view, "position") {
      changed |= set_opt_u32(&mut entry.position, Some(position));
   }

   // Update gap_overall from view.gapToFirstMillis / view.gapToFirstLaps
   // Leader (position 1) should always have gap_overall = "-"
   let position = entry.position.unwrap_or(0);
   if position == 1 {
      changed |= set_opt_string(&mut entry.gap_overall, Some("-".to_string()));
   } else {
      let gap_ms = map_i64(view, "gapToFirstMillis");
      let gap_laps = map_i64(view, "gapToFirstLaps");
      if gap_ms.is_some() || gap_laps.is_some() {
         if let Some(gap) = format_gap(gap_ms, gap_laps) {
            changed |= set_opt_string(&mut entry.gap_overall, Some(gap));
         }
      }
   }

   // Update class_id from view.classId
   if let Some(class_id) = map_str(view, "classId") {
      changed |= set_opt_string(&mut entry.class_id, Some(class_id));
      refresh_class_name(state, &key);
   }

   changed
}

fn apply_sector_cross_updates(state: &mut WecLiveState, view: Option<&Value>) -> bool {
   let Some(view) = view.and_then(Value::as_object) else {
      return false;
   };
   let mut changed = false;

   // Process view.ranks.items -> reuse apply_ranks logic
   if let Some(ranks_view) = view.get("ranks") {
      changed |= apply_ranks(state, ranks_view);
   }

   // Process view.gaps.items -> reuse apply_gaps logic
   // Pass true for update_gap_overall to make sector-cross-updates the live
   // source for gap_overall values
   if let Some(gaps_view) = view.get("gaps") {
      changed |= apply_gaps(state, gaps_view, true);
   }

   // Process view.sectors -> reuse apply_sectors logic
   if let Some(sectors_view) = view.get("sectors") {
      changed |= apply_sectors(state, sectors_view);
   }

   changed
}

fn payload_rows(payload: &Value) -> Vec<&Value> {
   match payload {
      Value::Array(rows) => rows.iter().collect(),
      Value::Object(map) => {
         map.get("items")
            .and_then(Value::as_array)
            .map_or_else(|| vec![payload], |items| items.iter().collect())
      },
      _ => Vec::new(),
   }
}

fn apply_session_info(state: &mut WecLiveState, payload: &Value) -> bool {
   let Some(map) = payload.as_object() else {
      return false;
   };
   let mut changed = false;
   changed |= set_header_text(&mut state.header.event_name, map_str(map, "eventName"));
   changed |= set_header_text(&mut state.header.session_name, map_str(map, "sessionName"));
   changed |= set_header_text(&mut state.header.track_name, map_str(map, "trackName"));
   changed |= set_header_text(
      &mut state.header.session_type_raw,
      map_text(map, "sessionType"),
   );
   if let Some(classes) = map.get("sessionClasses").and_then(Value::as_array) {
      let mut class_names = HashMap::new();
      let mut class_colors = BTreeMap::new();
      for class_row in classes {
         let Some(class_map) = class_row.as_object() else {
            continue;
         };
         let Some(class_id) = map_str(class_map, "classId") else {
            continue;
         };
         let class_label_raw = map_str(class_map, "classThreeLettersName")
            .or_else(|| map_str(class_map, "className"))
            .unwrap_or_else(|| class_id.clone());
         let class_label = canonicalize_class_name(&class_label_raw);
         class_names.insert(class_id.clone(), class_label.clone());
         if let Some(color_hex) = map_str(class_map, "classColor") {
            class_colors.insert(class_label, TimingClassColor { color: color_hex });
         }
      }
      if !class_names.is_empty() {
         state.class_names = class_names;
         changed = true;
      }
      if !class_colors.is_empty() {
         state.class_colors = class_colors;
         state.header.class_colors = state.class_colors.clone();
         changed = true;
      }
   }
   changed
}

fn apply_session_clock(state: &mut WecLiveState, payload: &Value) -> bool {
   let Some(map) = payload.as_object() else {
      return false;
   };
   let mut changed = false;
   changed |= set_header_text(
      &mut state.header.day_time,
      map_str(map, "tsNow").map(|raw| compact_iso_timestamp(&raw)),
   );

   // Extract session length if available (various field names for compatibility)
   let session_length = map_i64(map, "sessionLengthMillis")
      .or_else(|| map_i64(map, "maxSessionLength"))
      .or_else(|| map_i64(map, "timeLimitMillis"))
      .or_else(|| map_i64(map, "lengthLimitMillis"));
   if let Some(length) = session_length {
      if length > 0 {
         state.session_length_ms = Some(u64::try_from(length).unwrap_or(0));
         changed = true;
      }
   }

   // Calculate time remaining (TTE) from session length minus elapsed time
   // Requires session_length_ms to be set from schedule's lengthLimit
   // Try elapsedTimeMillisNow first, fall back to elapsedTimeMillis
   let elapsed_ms =
      map_i64(map, "elapsedTimeMillisNow").or_else(|| map_i64(map, "elapsedTimeMillis"));
   if let (Some(ms), Some(session_length_ms)) = (elapsed_ms, state.session_length_ms) {
      if ms >= 0 {
         let elapsed_u64 = u64::try_from(ms).unwrap_or(0);
         // Calculate remaining time (TTE = session length - elapsed)
         let remaining_ms = session_length_ms.saturating_sub(elapsed_u64);
         changed |= set_header_text(
            &mut state.header.time_to_go,
            Some(format_clock_ms(remaining_ms)),
         );
      }
   }
   changed
}

fn apply_race_flags(state: &mut WecLiveState, payload: &Value) -> bool {
   let rows = payload_rows(payload);
   let latest = rows
      .into_iter()
      .filter_map(|row| row.as_object())
      .max_by_key(|row| map_str(row, "ts").unwrap_or_default());
   let Some(latest) = latest else {
      return false;
   };
   set_header_text(
      &mut state.header.flag,
      map_str(latest, "flag").map(|flag| normalize_flag(&flag)),
   )
}

fn apply_participants(state: &mut WecLiveState, payload: &Value) -> bool {
   let mut changed = false;
   for row in payload_rows(payload) {
      let Some(map) = row.as_object() else {
         continue;
      };
      let Some((key, entry)) = upsert_car_state(state, map) else {
         continue;
      };
      changed |= set_row_text(&mut entry.car_number, map_str(map, "carNumber"));
      changed |= set_opt_string(
         &mut entry.team,
         map_str(map, "teamName").or_else(|| map_str(map, "displayName")),
      );
      changed |= set_opt_string(&mut entry.vehicle, map_str(map, "manufacturer"));
      changed |= set_opt_string(
         &mut entry.driver,
         current_driver_name(map).map(|name| normalize_driver_name(&name)),
      );
      changed |= set_opt_string(&mut entry.class_id, map_str(map, "classId"));
      refresh_class_name(state, &key);
   }
   changed
}

fn apply_ranks(state: &mut WecLiveState, payload: &Value) -> bool {
   let mut changed = false;
   for row in payload_rows(payload) {
      let Some(map) = row.as_object() else {
         continue;
      };
      let Some((key, entry)) = upsert_car_state(state, map) else {
         continue;
      };
      changed |= set_opt_u32(&mut entry.position, map_u32(map, "overallPosition"));
      changed |= set_opt_u32(&mut entry.class_rank, map_u32(map, "position"));
      changed |= set_opt_u32(&mut entry.laps, map_u32(map, "lapNumber"));
      changed |= set_opt_string(&mut entry.class_id, map_str(map, "classId"));
      refresh_class_name(state, &key);
   }
   changed
}

fn apply_sectors(state: &mut WecLiveState, payload: &Value) -> bool {
   let mut changed = false;
   for row in payload_rows(payload) {
      let Some(map) = row.as_object() else {
         continue;
      };
      let Some((_key, entry)) = upsert_car_state(state, map) else {
         continue;
      };
      let Some(sector_number) = map_u32(map, "sectorNumber") else {
         continue;
      };
      if !(1..=3).contains(&sector_number) {
         continue;
      }
      let Some(sector_ms) = map_i64(map, "sectorTimeMillis") else {
         continue;
      };
      if sector_ms <= 0 {
         continue;
      }
      let idx = (sector_number - 1) as usize;
      let incoming_lap = map_u32(map, "lapNumber").unwrap_or(0);
      let previous_lap = entry.sector_laps[idx].unwrap_or(0);
      let should_update = incoming_lap >= previous_lap || entry.sector_times[idx].is_none();
      if should_update {
         if entry.sector_times[idx] != Some(sector_ms) {
            entry.sector_times[idx] = Some(sector_ms);
            changed = true;
         }
         if entry.sector_laps[idx] != Some(incoming_lap) {
            entry.sector_laps[idx] = Some(incoming_lap);
            changed = true;
         }
      }
   }
   changed
}

fn apply_gaps(state: &mut WecLiveState, payload: &Value, update_gap_overall: bool) -> bool {
   let mut changed = false;
   for row in payload_rows(payload) {
      let Some(map) = row.as_object() else {
         continue;
      };
      let Some((_key, entry)) = upsert_car_state(state, map) else {
         continue;
      };
      if update_gap_overall {
         // Leader rule: if position == 1, gap_overall = "-"
         let position = entry.position.unwrap_or(0);
         if position == 1 {
            changed |= set_opt_string(&mut entry.gap_overall, Some("-".to_string()));
         } else {
            changed |= set_opt_string(
               &mut entry.gap_overall,
               format_gap(
                  map_i64(map, "gapToFirstMillis"),
                  map_i64(map, "gapToFirstLaps"),
               ),
            );
         }
      }
      changed |= set_opt_string(
         &mut entry.gap_next_in_class,
         format_gap(
            map_i64(map, "gapToAheadMillis"),
            map_i64(map, "gapToAheadLaps"),
         ),
      );
      changed |= set_opt_u32(&mut entry.laps, map_u32(map, "lapNumber"));
   }
   changed
}

fn apply_laps(state: &mut WecLiveState, payload: &Value) -> bool {
   let mut changed = false;
   for row in payload_rows(payload) {
      let Some(map) = row.as_object() else {
         continue;
      };
      let Some((_key, entry)) = upsert_car_state(state, map) else {
         continue;
      };

      let lap_no = map_u32(map, "lapNumber");
      if let Some(lap_ms) = map_i64(map, "lapTimeMillis") {
         if lap_ms > 0 {
            // Only update last_lap_ms if this lap is newer or same as best_lap_no
            if lap_no.unwrap_or(0) >= entry.best_lap_no.unwrap_or(0) || entry.last_lap_ms.is_none()
            {
               changed |= set_opt_i64(&mut entry.last_lap_ms, Some(lap_ms));
            }
            // Only update best lap if it's better than current best
            if entry.best_lap_ms.is_none() || Some(lap_ms) < entry.best_lap_ms {
               changed |= set_opt_i64(&mut entry.best_lap_ms, Some(lap_ms));
               // Only update best_lap_no if incoming lap is newer
               if lap_no.is_none_or(|n| n >= entry.best_lap_no.unwrap_or(0)) {
                  changed |= set_opt_u32(&mut entry.best_lap_no, lap_no);
               }
            }
         }
      }
      // Only update laps if incoming lapNumber is greater than existing
      // This prevents historical data from overwriting newer lap counts
      if lap_no.is_none_or(|n| n > entry.laps.unwrap_or(0)) {
         changed |= set_opt_u32(&mut entry.laps, lap_no);
      }
      changed |= set_opt_string(&mut entry.class_id, map_str(map, "classId"));
      if let Some(in_pit) =
         map_bool(map, "isEndedInPit").or_else(|| map_bool(map, "isStartedInPit"))
      {
         changed |= set_opt_bool(&mut entry.pit, Some(in_pit));
      }
   }
   changed
}

fn snapshot_from_live_state(state: &WecLiveState) -> Option<(TimingHeader, Vec<TimingEntry>)> {
   let mut entries: Vec<TimingEntry> = state
      .rows
      .values()
      .filter(|row| !row.car_number.trim().is_empty())
      .cloned()
      .map(|row| row_to_timing_entry(state, row))
      .collect();
   entries.sort_by_key(|entry| (entry.position, entry.car_number.clone()));
   for (idx, entry) in entries.iter_mut().enumerate() {
      if entry.position == 0 {
         // Safe: idx represents a position which should reasonably fit in u32
         entry.position = u32::try_from(idx + 1).expect("position should fit in u32");
      }
   }
   if entries.is_empty() {
      return None;
   }

   let mut header = state.header.clone();
   if header.event_name.trim().is_empty() {
      header.event_name = "WEC Live Timing".to_string();
   }
   if header.session_name.trim().is_empty() {
      header.session_name = "-".to_string();
   }
   if header.track_name.trim().is_empty() {
      header.track_name = "-".to_string();
   }
   if header.day_time.trim().is_empty() {
      header.day_time = "-".to_string();
   }
   if header.time_to_go.trim().is_empty() {
      header.time_to_go = "-".to_string();
   }
   if header.flag.trim().is_empty() {
      header.flag = "-".to_string();
   }
   header.class_colors = state.class_colors.clone();
   Some((header, entries))
}

fn row_to_timing_entry(state: &WecLiveState, row: WecCarState) -> TimingEntry {
   let class_name_raw = row
      .class_name
      .clone()
      .or_else(|| {
         row.class_id
            .as_ref()
            .and_then(|id| state.class_names.get(id).cloned())
      })
      .unwrap_or_else(|| "-".to_string());
   let class_name = canonicalize_class_name(&class_name_raw);
   let stable_id = format!("wec:{}", row.car_number);
   TimingEntry {
      position: row.position.unwrap_or(0),
      car_number: row.car_number,
      class_name,
      class_rank: row
         .class_rank
         .map_or_else(|| "-".to_string(), |rank| rank.to_string()),
      driver: row.driver.unwrap_or_else(|| "-".to_string()),
      vehicle: row.vehicle.unwrap_or_else(|| "-".to_string()),
      team: row.team.unwrap_or_else(|| "-".to_string()),
      laps: row
         .laps
         .map_or_else(|| "-".to_string(), |lap| lap.to_string()),
      gap_overall: row.gap_overall.unwrap_or_else(|| "-".to_string()),
      gap_class: "-".to_string(),
      gap_next_in_class: row.gap_next_in_class.unwrap_or_else(|| "-".to_string()),
      last_lap: row
         .last_lap_ms
         .map_or_else(|| "-".to_string(), format_lap_time_ms),
      best_lap: row
         .best_lap_ms
         .map_or_else(|| "-".to_string(), format_lap_time_ms),
      sector_1: row
         .sector_times
         .first()
         .copied()
         .flatten()
         .map_or_else(|| "-".to_string(), format_sector_time_ms),
      sector_2: row
         .sector_times
         .get(1)
         .copied()
         .flatten()
         .map_or_else(|| "-".to_string(), format_sector_time_ms),
      sector_3: row
         .sector_times
         .get(2)
         .copied()
         .flatten()
         .map_or_else(|| "-".to_string(), format_sector_time_ms),
      sector_4: "-".to_string(),
      sector_5: "-".to_string(),
      best_lap_no: row
         .best_lap_no
         .map_or_else(|| "-".to_string(), |lap| lap.to_string()),
      pit: if row.pit.unwrap_or(false) {
         "Yes".to_string()
      } else {
         "No".to_string()
      },
      pit_stops: "-".to_string(),
      fastest_driver: "-".to_string(),
      stable_id,
   }
}

fn upsert_car_state<'a>(
   state: &'a mut WecLiveState,
   row: &Map<String, Value>,
) -> Option<(String, &'a mut WecCarState)> {
   let key = map_i64(row, "pid")
      .filter(|pid| *pid > 0)
      .map(|pid| format!("pid:{pid}"))
      .or_else(|| map_str(row, "carNumber").map(|car| format!("car:{car}")))?;

   let car_number = map_str(row, "carNumber").unwrap_or_else(|| "-".to_string());
   let entry = state.rows.entry(key.clone()).or_default();
   if entry.car_number.trim().is_empty() && !car_number.trim().is_empty() {
      entry.car_number = car_number;
   }
   Some((key, entry))
}

fn refresh_class_name(state: &mut WecLiveState, key: &str) {
   if let Some(row) = state.rows.get_mut(key) {
      if row.class_name.is_none() {
         if let Some(class_id) = row.class_id.as_ref() {
            if let Some(class_name) = state.class_names.get(class_id) {
               row.class_name = Some(class_name.clone());
            }
         }
      }
   }
}

fn current_driver_name(row: &Map<String, Value>) -> Option<String> {
   if let Some(drivers) = row.get("drivers").and_then(Value::as_array) {
      let current_driver_id = map_str(row, "currentDriverId");
      if let Some(current_driver_id) = current_driver_id {
         for driver in drivers {
            let Some(driver_map) = driver.as_object() else {
               continue;
            };
            if map_text(driver_map, "driverId").as_deref() == Some(current_driver_id.as_str()) {
               if let Some(name) = map_str(driver_map, "displayName") {
                  return Some(name);
               }
            }
         }
      }
      for driver in drivers {
         let Some(driver_map) = driver.as_object() else {
            continue;
         };
         if let Some(name) = map_str(driver_map, "displayName") {
            return Some(name);
         }
      }
   }

   if let (Some(first), Some(last)) = (map_str(row, "firstname"), map_str(row, "lastname")) {
      return Some(format!("{first} {last}"));
   }
   map_str(row, "displayName")
}

fn format_gap(gap_ms: Option<i64>, gap_laps: Option<i64>) -> Option<String> {
   if let Some(laps) = gap_laps {
      if laps > 0 {
         return Some(format!("+{laps} L"));
      }
   }
   let millis = gap_ms?;
   if millis <= 0 {
      return Some("-".to_string());
   }
   let total_ms = u64::try_from(millis).ok()?;
   let minutes = total_ms / 60_000;
   let seconds = (total_ms % 60_000) / 1000;
   let rem = total_ms % 1000;
   if minutes > 0 {
      Some(format!("+{minutes}:{seconds:02}.{rem:03}"))
   } else {
      Some(format!("+{seconds}.{rem:03}"))
   }
}

fn format_lap_time_ms(ms: i64) -> String {
   if ms <= 0 {
      return "-".to_string();
   }
   // Safe: ms is checked to be > 0 above
   let total_ms = u64::try_from(ms).expect("ms is positive");
   let minutes = total_ms / 60_000;
   let seconds = (total_ms % 60_000) / 1000;
   let millis = total_ms % 1000;
   format!("{minutes}:{seconds:02}.{millis:03}")
}

fn format_sector_time_ms(ms: i64) -> String {
   if ms <= 0 {
      return "-".to_string();
   }
   // Safe: ms is checked to be > 0 above
   let total_ms = u64::try_from(ms).expect("ms is positive");
   if total_ms >= 60_000 {
      let minutes = total_ms / 60_000;
      let seconds = (total_ms % 60_000) / 1000;
      let millis = total_ms % 1000;
      return format!("{minutes}:{seconds:02}.{millis:03}");
   }
   let seconds = total_ms / 1000;
   let millis = total_ms % 1000;
   format!("{seconds}.{millis:03}")
}

fn normalize_driver_name(raw: &str) -> String {
   raw.split_whitespace()
      .map(normalize_driver_name_token)
      .collect::<Vec<_>>()
      .join(" ")
}

fn normalize_driver_name_token(token: &str) -> String {
   if token.chars().all(|ch| !ch.is_alphabetic()) {
      return token.to_string();
   }
   let letters: String = token.chars().filter(|ch| ch.is_alphabetic()).collect();
   let needs_normalization = !letters.is_empty()
      && (letters.chars().all(char::is_uppercase) || letters.chars().all(char::is_lowercase));
   if !needs_normalization {
      return token.to_string();
   }

   let mut out = String::with_capacity(token.len());
   let mut seen_alpha = false;
   for ch in token.chars() {
      if ch.is_alphabetic() {
         if seen_alpha {
            out.extend(ch.to_lowercase());
         } else {
            out.extend(ch.to_uppercase());
            seen_alpha = true;
         }
      } else {
         seen_alpha = false;
         out.push(ch);
      }
   }
   out
}

fn format_clock_ms(ms: u64) -> String {
   let total_seconds = ms / 1000;
   let hours = total_seconds / 3600;
   let minutes = (total_seconds % 3600) / 60;
   let seconds = total_seconds % 60;
   if hours > 0 {
      format!("{hours:02}:{minutes:02}:{seconds:02}")
   } else {
      format!("{minutes:02}:{seconds:02}")
   }
}

fn compact_iso_timestamp(raw: &str) -> String {
   let trimmed = raw.trim();
   if let Some((_, rest)) = trimmed.split_once('T') {
      return rest
         .split(['+', 'Z'])
         .next()
         .unwrap_or(rest)
         .trim()
         .to_string();
   }
   trimmed.to_string()
}

fn map_str(map: &Map<String, Value>, key: &str) -> Option<String> {
   let raw = map.get(key)?.as_str()?.trim();
   if raw.is_empty() {
      None
   } else {
      Some(raw.to_string())
   }
}

fn map_i64(map: &Map<String, Value>, key: &str) -> Option<i64> {
   map.get(key).and_then(|value| {
      value
         .as_i64()
         .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
         .or_else(|| value.as_str()?.trim().parse::<i64>().ok())
   })
}

fn map_u32(map: &Map<String, Value>, key: &str) -> Option<u32> {
   map_i64(map, key).and_then(|number| u32::try_from(number).ok())
}

fn map_bool(map: &Map<String, Value>, key: &str) -> Option<bool> {
   map.get(key).and_then(|value| {
      value.as_bool().or_else(|| {
         value.as_str().and_then(|raw| {
            match raw.trim().to_ascii_lowercase().as_str() {
               "true" | "1" | "yes" => Some(true),
               "false" | "0" | "no" => Some(false),
               _ => None,
            }
         })
      })
   })
}

fn map_text(map: &Map<String, Value>, key: &str) -> Option<String> {
   let value = map.get(key)?;
   match value {
      Value::String(text) => {
         let trimmed = text.trim();
         if trimmed.is_empty() {
            None
         } else {
            Some(trimmed.to_string())
         }
      },
      Value::Number(number) => Some(number.to_string()),
      Value::Bool(value) => Some(if *value { "true" } else { "false" }.to_string()),
      _ => None,
   }
}

fn set_header_text(slot: &mut String, incoming: Option<String>) -> bool {
   let Some(incoming) = incoming else {
      return false;
   };
   if incoming.trim().is_empty() || *slot == incoming {
      return false;
   }
   *slot = incoming;
   true
}

fn set_row_text(slot: &mut String, incoming: Option<String>) -> bool {
   let Some(incoming) = incoming else {
      return false;
   };
   if incoming.trim().is_empty() || *slot == incoming {
      return false;
   }
   *slot = incoming;
   true
}

fn set_opt_string(slot: &mut Option<String>, incoming: Option<String>) -> bool {
   let Some(incoming) = incoming else {
      return false;
   };
   if incoming.trim().is_empty() || slot.as_ref() == Some(&incoming) {
      return false;
   }
   *slot = Some(incoming);
   true
}

fn set_opt_u32(slot: &mut Option<u32>, incoming: Option<u32>) -> bool {
   if incoming.is_none() || *slot == incoming {
      return false;
   }
   *slot = incoming;
   true
}

fn set_opt_i64(slot: &mut Option<i64>, incoming: Option<i64>) -> bool {
   if incoming.is_none() || *slot == incoming {
      return false;
   }
   *slot = incoming;
   true
}

fn set_opt_bool(slot: &mut Option<bool>, incoming: Option<bool>) -> bool {
   if incoming.is_none() || *slot == incoming {
      return false;
   }
   *slot = incoming;
   true
}

fn normalize_flag(raw: &str) -> String {
   let normalized = raw.trim().to_ascii_lowercase();
   if normalized.contains("check") || normalized.contains("finish") {
      "Checkered".to_string()
   } else if normalized.contains("red") {
      "Red".to_string()
   } else if normalized.contains("yellow") {
      "Yellow".to_string()
   } else if normalized.contains("safety") || normalized.contains("sc ") || normalized == "sc" {
      "Yellow (SC)".to_string()
   } else if normalized.contains("fcy") || normalized.contains("full course") {
      "Yellow (FCY)".to_string()
   } else if normalized.contains("code 60") || normalized == "60" {
      "Code 60".to_string()
   } else if normalized.contains("green") {
      "Green".to_string()
   } else if raw.trim().is_empty() {
      "-".to_string()
   } else {
      raw.trim().to_string()
   }
}

fn emit_snapshot(
   emitter: (&Sender<TimingMessage>, u64),
   header: TimingHeader,
   entries: Vec<TimingEntry>,
   persist: &mut PersistState,
   last_snapshot: &mut Option<WecSnapshot>,
   last_session_id: &mut Option<String>,
   debug_output: &SeriesDebugOutput,
) {
   let (tx, source_id) = emitter;
   let session_id = derive_session_identifier(&header);
   let snapshot = WecSnapshot {
      header:      header.clone(),
      entries:     entries.clone(),
      session_id:  session_id.clone(),
      fingerprint: meaningful_snapshot_fingerprint(&header, &entries),
      extra:       (),
   };
   let first_real_of_session = !snapshot.entries.is_empty() && session_id != *last_session_id;
   let session_complete = snapshot.header.flag.eq_ignore_ascii_case("checkered");
   let materially_changed = last_snapshot
      .as_ref()
      .is_none_or(|prev| prev.fingerprint != snapshot.fingerprint);
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
      persist_snapshot(persist, &snapshot, now_unix_ms(), "WEC", debug_output);
   }

   *last_session_id = session_id;
   *last_snapshot = Some(snapshot);

   let _ = tx.send(TimingMessage::Snapshot {
      source_id,
      header,
      entries,
   });
   let _ = tx.send(TimingMessage::Status {
      source_id,
      text: "WEC live timing connected".to_string(),
   });
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn split_signalr_frames_handles_record_separator() {
      let raw = "{}\u{1e}{\"type\":6}\u{1e}";
      let frames = split_signalr_frames(raw);
      assert_eq!(frames, vec!["{}", "{\"type\":6}"]);
   }

   #[test]
   fn parse_signalr_frame_recognizes_invocation() {
      let frame =
         "{\"type\":1,\"target\":\"x\",\"arguments\":[{\"cars\":[{\"carNumber\":\"50\"}]}]}";
      match parse_signalr_frame(frame) {
         SignalRFrame::Invocation { target, arguments } => {
            assert_eq!(target, "x");
            assert_eq!(arguments.len(), 1);
         },
         _ => panic!("expected invocation"),
      }
   }

   #[test]
   fn choose_candidate_sids_prefers_started_session() {
      let sessions = vec![
         SessionScheduleItem {
            sid:               10,
            is_started:        false,
            connection_status: Some("Green".to_string()),
         },
         SessionScheduleItem {
            sid:               11,
            is_started:        true,
            connection_status: Some("Green".to_string()),
         },
      ];
      assert_eq!(choose_candidate_sids(&sessions).unwrap(), vec![11, 10]);
   }

   #[test]
   fn choose_candidate_sids_falls_back_to_first() {
      let sessions = vec![SessionScheduleItem {
         sid:               20,
         is_started:        false,
         connection_status: None,
      }];
      assert_eq!(choose_candidate_sids(&sessions).unwrap(), vec![20]);
   }

   #[test]
   fn choose_candidate_sids_rejects_empty_schedule() {
      let sessions = Vec::<SessionScheduleItem>::new();
      assert!(choose_candidate_sids(&sessions).is_err());
   }

   #[test]
   fn apply_signalr_arguments_builds_clean_snapshot() {
      let mut state = WecLiveState::default();

      apply_signalr_arguments(
         &mut state,
         "lv-session-info",
         &[serde_json::json!({
             "eventName": "WEC",
             "sessionName": "Race",
             "trackName": "Imola",
             "connectionStatus": "Green",
             "sessionClasses": [
                 {"classId":"HYPERCAR","classThreeLettersName":"HYPER","classColor":"#ff0000"}
             ]
         })][..],
      );
      apply_signalr_arguments(
         &mut state,
         "lv-participants",
         &[serde_json::json!({
             "items": [
                 {
                     "pid": 1,
                     "carNumber": "50",
                     "classId": "HYPERCAR",
                     "teamName": "Ferrari AF Corse",
                     "manufacturer": "Ferrari 499P",
                     "drivers": [{"displayName":"ALESSANDRO PIER GUIDI"}]
                 }
             ]
         })][..],
      );
      apply_signalr_arguments(
         &mut state,
         "lv-ranks",
         &[serde_json::json!({
             "items": [
                 {
                     "pid": 1,
                     "carNumber": "50",
                     "overallPosition": 1,
                     "position": 1,
                     "lapNumber": 160,
                     "sectorNumber": 2,
                     "classId": "HYPERCAR"
                 }
             ]
         })][..],
      );
      apply_signalr_arguments(
         &mut state,
         "lv-gaps",
         &[serde_json::json!({
             "items": [
                 {
                     "pid": 1,
                     "carNumber": "50",
                     "gapToFirstMillis": -1,
                     "gapToAheadMillis": 0,
                     "gapToAheadLaps": 0,
                     "lapNumber": 160
                 }
             ]
         })][..],
      );
      apply_signalr_arguments(
         &mut state,
         "lv-laps",
         &[serde_json::json!({
             "pid": 1,
             "carNumber": "50",
             "lapNumber": 160,
             "lapTimeMillis": 95321,
             "isEndedInPit": false
         })][..],
      );
      apply_signalr_arguments(
         &mut state,
         "lv-sectors",
         &[serde_json::json!({
             "pid": 1,
             "carNumber": "50",
             "sectorNumber": 1,
             "lapNumber": 160,
             "sectorTimeMillis": 19512
         })][..],
      );
      apply_signalr_arguments(
         &mut state,
         "lv-sectors",
         &[serde_json::json!({
             "pid": 1,
             "carNumber": "50",
             "sectorNumber": 2,
             "lapNumber": 160,
             "sectorTimeMillis": 31856
         })][..],
      );
      apply_signalr_arguments(
         &mut state,
         "lv-sectors",
         &[serde_json::json!({
             "pid": 1,
             "carNumber": "50",
             "sectorNumber": 3,
             "lapNumber": 160,
             "sectorTimeMillis": 44169
         })][..],
      );

      let (header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      assert_eq!(header.session_name, "Race");
      assert_eq!(header.event_name, "WEC");
      assert_eq!(entries.len(), 1);
      assert_eq!(entries[0].car_number, "50");
      assert_eq!(entries[0].team, "Ferrari AF Corse");
      assert_eq!(entries[0].driver, "Alessandro Pier Guidi");
      assert_eq!(entries[0].class_name, "HYPER");
      assert_eq!(entries[0].best_lap, "1:35.321");
      assert_eq!(entries[0].gap_overall, "-");
      assert_eq!(entries[0].sector_1, "19.512");
      assert_eq!(entries[0].sector_2, "31.856");
      assert_eq!(entries[0].sector_3, "44.169");
   }

   #[test]
   fn apply_signalr_arguments_merges_deltas_without_dropping_rows() {
      let mut state = WecLiveState::default();
      apply_signalr_arguments(
         &mut state,
         "lv-ranks",
         &[serde_json::json!({
             "items": [
                 {"pid": 1, "carNumber":"50", "overallPosition":1, "position":1, "lapNumber":10},
                 {"pid": 2, "carNumber":"6", "overallPosition":2, "position":2, "lapNumber":10}
             ]
         })][..],
      );
      apply_signalr_arguments(
         &mut state,
         "lv-gaps",
         &[serde_json::json!({
             "items": [
                 {"pid": 2, "carNumber":"6", "gapToFirstMillis":1200, "gapToAheadMillis":1200, "gapToAheadLaps":0}
             ]
         })][..],
      );

      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      assert_eq!(entries.len(), 2);
      assert!(entries.iter().any(|entry| entry.car_number == "50"));
      assert!(entries.iter().any(|entry| entry.car_number == "6"));
   }

   #[test]
   fn normalize_driver_name_handles_mixed_case_tokens() {
      assert_eq!(
         normalize_driver_name("António FÉLIX DA COSTA"),
         "António Félix Da Costa"
      );
      assert_eq!(normalize_driver_name("Kevin MAGNUSSEN"), "Kevin Magnussen");
      assert_eq!(normalize_driver_name("Mike Conway"), "Mike Conway");
   }

   #[test]
   fn choose_latest_finished_race_session_picks_newest_finished_race() {
      let sessions = vec![
         MetaSessionItem {
            id:           1,
            name:         Some("Practice".to_string()),
            session_type: Some("Practice".to_string()),
            is_running:   false,
            has_result:   true,
            start_time:   Some("2026-04-18T08:00:00+00:00".to_string()),
            end_time:     Some("2026-04-18T09:00:00+00:00".to_string()),
            event:        None,
            track_config: None,
         },
         MetaSessionItem {
            id:           2,
            name:         Some("Race Old".to_string()),
            session_type: Some("Race".to_string()),
            is_running:   false,
            has_result:   true,
            start_time:   Some("2026-04-18T10:00:00+00:00".to_string()),
            end_time:     Some("2026-04-18T12:00:00+00:00".to_string()),
            event:        None,
            track_config: None,
         },
         MetaSessionItem {
            id:           3,
            name:         Some("Race New".to_string()),
            session_type: Some("Race".to_string()),
            is_running:   false,
            has_result:   true,
            start_time:   Some("2026-04-19T10:00:00+00:00".to_string()),
            end_time:     Some("2026-04-19T12:00:00+00:00".to_string()),
            event:        None,
            track_config: None,
         },
         MetaSessionItem {
            id:           4,
            name:         Some("Race Running".to_string()),
            session_type: Some("Race".to_string()),
            is_running:   true,
            has_result:   true,
            start_time:   Some("2026-04-20T10:00:00+00:00".to_string()),
            end_time:     Some("2026-04-20T12:00:00+00:00".to_string()),
            event:        None,
            track_config: None,
         },
      ];

      let picked = choose_latest_finished_race_session(&sessions).expect("expected race session");
      assert_eq!(picked.id, 3);
   }

   // =========================================================================
   // ReceiveBatch SignalR frame handling tests
   // =========================================================================

   #[test]
   fn apply_receive_batch_official_rank_is_ignored() {
      // official-rank channel is now ignored to prevent bad gap_overall
      // updates (e.g., "+1 L" overwrites). gap_overall should come from
      // sector-cross-updates.gaps.items instead.
      let mut state = WecLiveState::default();

      // Set up session info with class names
      apply_signalr_arguments(
         &mut state,
         "lv-session-info",
         &[serde_json::json!({
             "eventName": "WEC",
             "sessionName": "Race",
             "sessionClasses": [{"classId":"HYPERCAR","classThreeLettersName":"HYP"}]
         })][..],
      );

      // First set up a participant
      apply_signalr_arguments(
         &mut state,
         "lv-participants",
         &[serde_json::json!({
             "items": [{ "pid": 1, "carNumber": "8", "teamName": "Toyota Gazoo Racing" }]
         })][..],
      );

      // Set initial position via lv-ranks
      apply_signalr_arguments(
         &mut state,
         "lv-ranks",
         &[serde_json::json!({
             "items": [{ "pid": 1, "carNumber": "8", "overallPosition": 1 }]
         })][..],
      );

      // Apply official-rank batch update - should be ignored
      let batch = serde_json::json!({
         "items": [{
             "channel": "official-rank",
             "view": {
                 "pid": 1,
                 "position": 1,
                 "gapToFirstMillis": 0,
                 "gapToFirstLaps": 0,
                 "carNumber": "8",
                 "classId": "HYPERCAR",
                 "ts": "2026-06-13T17:27:14.7309842+00:00"
             }
         }]
      });

      let changed = apply_receive_batch(&mut state, &[batch]);
      assert!(!changed, "official-rank batch should be ignored");

      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      assert_eq!(entries.len(), 1);
      assert_eq!(entries[0].car_number, "8");
      // Position should come from lv-ranks, not official-rank
      assert_eq!(entries[0].position, 1);
      // gap_overall should be "-" since it's position 1 (leader rule)
      assert_eq!(entries[0].gap_overall, "-");
   }

   #[test]
   fn sector_cross_updates_gaps_items_updates_gap_overall() {
      // Verify that sector-cross-updates.gaps.items updates gap_overall
      // for non-leader cars (P2 and below)
      let mut state = WecLiveState::default();

      // Set up participants
      apply_signalr_arguments(
         &mut state,
         "lv-participants",
         &[serde_json::json!({
             "items": [
                 { "pid": 1, "carNumber": "8", "teamName": "Toyota" },
                 { "pid": 2, "carNumber": "50", "teamName": "Ferrari" }
             ]
         })][..],
      );

      // Set initial positions: car 8 is leading (P1), car 50 is 2nd (P2)
      apply_signalr_arguments(
         &mut state,
         "lv-ranks",
         &[serde_json::json!({
             "items": [
                 { "pid": 1, "carNumber": "8", "overallPosition": 1 },
                 { "pid": 2, "carNumber": "50", "overallPosition": 2 }
             ]
         })][..],
      );

      // Set initial gap_overall via lv-gaps (simulating bootstrap)
      apply_signalr_arguments(
         &mut state,
         "lv-gaps",
         &[serde_json::json!({
             "items": [
                 { "pid": 1, "carNumber": "8", "gapToFirstMillis": -1, "gapToAheadMillis": 0 },
                 { "pid": 2, "carNumber": "50", "gapToFirstMillis": 5000, "gapToAheadMillis": 5000 }
             ]
         })][..],
      );

      // Verify initial state
      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      let car_50 = entries
         .iter()
         .find(|e| e.car_number == "50")
         .expect("car 50 exists");
      assert_eq!(car_50.position, 2);
      assert_eq!(car_50.gap_overall, "+5.000");

      // Apply sector-cross-updates batch with updated gaps
      // This should update gap_overall for P2 car
      let batch = serde_json::json!({
         "items": [{
             "channel": "sector-cross-updates",
             "view": {
                 "ranks": {
                     "items": [
                         { "pid": 1, "carNumber": "8", "overallPosition": 1, "position": 1 },
                         { "pid": 2, "carNumber": "50", "overallPosition": 2, "position": 2 }
                     ]
                 },
                 "gaps": {
                     "items": [
                         { "pid": 1, "carNumber": "8", "gapToFirstMillis": -1, "gapToAheadMillis": 0 },
                         { "pid": 2, "carNumber": "50", "gapToFirstMillis": 12500, "gapToAheadMillis": 12500 }
                     ]
                 }
             }
         }]
      });

      let changed = apply_receive_batch(&mut state, &[batch]);
      assert!(changed, "sector-cross-updates should cause changes");

      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      let car_8 = entries
         .iter()
         .find(|e| e.car_number == "8")
         .expect("car 8 exists");
      let car_50 = entries
         .iter()
         .find(|e| e.car_number == "50")
         .expect("car 50 exists");

      // Leader should have gap_overall = "-"
      assert_eq!(car_8.position, 1);
      assert_eq!(car_8.gap_overall, "-");

      // P2 should have updated gap_overall from sector-cross-updates.gaps.items
      assert_eq!(car_50.position, 2);
      assert_eq!(car_50.gap_overall, "+12.500");
   }

   #[test]
   fn apply_receive_batch_sector_cross_updates_all_nested() {
      let mut state = WecLiveState::default();

      // Set up participants and class info
      apply_signalr_arguments(
         &mut state,
         "lv-session-info",
         &[serde_json::json!({
             "sessionClasses": [{"classId":"HYPERCAR","classThreeLettersName":"HYP"}]
         })][..],
      );
      apply_signalr_arguments(
         &mut state,
         "lv-participants",
         &[serde_json::json!({
             "items": [{ "pid": 101, "carNumber": "101", "classId": "HYPERCAR" }]
         })][..],
      );

      // First set gap_overall via lv-gaps (this is the live source)
      apply_signalr_arguments(
         &mut state,
         "lv-gaps",
         &[serde_json::json!({
             "items": [{ "pid": 101, "carNumber": "101", "gapToFirstMillis": 50000, "gapToAheadMillis": 0 }]
         })][..],
      );

      // Apply sector-cross-updates batch with nested structure
      // This should update gap_overall from sector-cross-updates.gaps.items
      let batch = serde_json::json!({
         "items": [{
             "channel": "sector-cross-updates",
             "view": {
                 "sectors": [
                     {"sectorNumber": 1, "lapNumber": 58, "sectorTimeMillis": 33445, "pid": 101, "carNumber": "101"}
                 ],
                 "ranks": {"items": [{"overallPosition": 9, "position": 9, "carNumber": "101", "lapNumber": 58, "pid": 101}]},
                 "gaps": {"items": [{"gapToFirstMillis": 86183, "gapToAheadMillis": 4940, "carNumber": "101", "lapNumber": 58, "pid": 101}]}
             }
         }]
      });

      let changed = apply_receive_batch(&mut state, &[batch]);
      assert!(changed, "sector-cross-updates should cause changes");

      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      assert_eq!(entries.len(), 1);
      let entry = &entries[0];
      assert_eq!(entry.car_number, "101");
      assert_eq!(entry.position, 9);
      assert_eq!(entry.class_rank, "9");
      // gap_overall should BE updated from sector-cross-updates.gaps.items
      // since this is now the live source for gap_overall
      // Note: 86.183 seconds = 1 minute 26.183 seconds, formatted as "+1:26.183"
      assert_eq!(
         entry.gap_overall, "+1:26.183",
         "gap_overall should be updated from sector-cross-updates.gaps.items"
      );
      // gap_next_in_class should be updated from sector-cross-updates
      assert_eq!(entry.gap_next_in_class, "+4.940");
      assert_eq!(entry.sector_1, "33.445");
   }

   #[test]
   fn gap_overall_preserved_when_sector_cross_updates_has_gap_to_first_laps() {
      // Test: sector-cross-updates.gaps.items should preserve gap_overall
      // even when it contains gapToFirstLaps.
      // The leader rule (position == 1 => gap_overall = "-") should still apply.
      let mut state = WecLiveState::default();

      // Set up participants and class info
      apply_signalr_arguments(
         &mut state,
         "lv-session-info",
         &[serde_json::json!({
             "sessionClasses": [{"classId":"HYPERCAR","classThreeLettersName":"HYP"}]
         })][..],
      );
      apply_signalr_arguments(
         &mut state,
         "lv-participants",
         &[serde_json::json!({
             "items": [
                 { "pid": 1, "carNumber": "8", "classId": "HYPERCAR" },
                 { "pid": 2, "carNumber": "50", "classId": "HYPERCAR" }
             ]
         })][..],
      );

      // Set initial positions via lv-ranks
      apply_signalr_arguments(
         &mut state,
         "lv-ranks",
         &[serde_json::json!({
             "items": [
                 { "pid": 1, "carNumber": "8", "overallPosition": 1, "position": 1 },
                 { "pid": 2, "carNumber": "50", "overallPosition": 2, "position": 2 }
             ]
         })][..],
      );

      // Set initial gap_overall via lv-gaps for car 50 (gapToFirstMillis: 41994 =
      // +41.994)
      apply_signalr_arguments(
         &mut state,
         "lv-gaps",
         &[serde_json::json!({
             "items": [
                 { "pid": 1, "carNumber": "8", "gapToFirstMillis": -1 },
                 { "pid": 2, "carNumber": "50", "gapToFirstMillis": 41994, "gapToAheadMillis": 41994 }
             ]
         })][..],
      );

      // Verify initial gap_overall is set correctly
      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      let car_50 = entries
         .iter()
         .find(|e| e.car_number == "50")
         .expect("car 50");
      assert_eq!(
         car_50.gap_overall, "+41.994",
         "gap_overall should be set from lv-gaps"
      );

      // Now apply sector-cross-updates with gapToFirstLaps: 1 (which would
      // show "+1 L" if used incorrectly)
      // Since sector-cross-updates.gaps.items is now the live source,
      // it should update gap_overall
      let sector_batch = serde_json::json!({
         "items": [{
             "channel": "sector-cross-updates",
             "view": {
                 "ranks": {"items": [{"overallPosition": 2, "position": 2, "carNumber": "50", "lapNumber": 120, "pid": 2}]},
                 "gaps": {"items": [{"gapToFirstMillis": 1000, "gapToFirstLaps": 1, "gapToAheadMillis": 2500, "gapToAheadLaps": 0, "carNumber": "50", "lapNumber": 120, "pid": 2}]}
             }
         }]
      });

      let changed = apply_receive_batch(&mut state, &[sector_batch]);
      assert!(changed, "sector-cross-updates should cause changes");

      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      let car_50 = entries
         .iter()
         .find(|e| e.car_number == "50")
         .expect("car 50");

      // gap_overall should be updated from sector-cross-updates.gaps.items
      // Since gapToFirstLaps is 1, it should show "+1 L"
      // This is now the correct behavior since we use sector-cross-updates
      assert_eq!(
         car_50.gap_overall, "+1 L",
         "gap_overall should be updated from sector-cross-updates.gaps.items"
      );
      // gap_next_in_class should be updated normally from gapToAheadMillis
      assert_eq!(
         car_50.gap_next_in_class, "+2.500",
         "gap_next_in_class should be updated from sector-cross-updates"
      );
   }

   #[test]
   fn apply_official_rank_leader_change() {
      // Test the apply_official_rank function directly (now unused in production,
      // but kept for potential future use). Verify it correctly sets leader
      // gap_overall to "-".
      let mut state = WecLiveState::default();

      // Set up two cars: BMW initially leading
      apply_signalr_arguments(
         &mut state,
         "lv-participants",
         &[serde_json::json!({
             "items": [
                 { "pid": 1, "carNumber": "20", "teamName": "BMW M Team WRT" },
                 { "pid": 2, "carNumber": "8", "teamName": "Toyota Gazoo Racing" }
             ]
         })][..],
      );

      // Initial ranking with BMW leading
      apply_signalr_arguments(
         &mut state,
         "lv-ranks",
         &[serde_json::json!({
             "items": [
                 { "pid": 1, "carNumber": "20", "overallPosition": 1, "position": 1 },
                 { "pid": 2, "carNumber": "8", "overallPosition": 2, "position": 2 }
             ]
         })][..],
      );

      // Official rank update: Toyota takes the lead (via direct function call)
      let official_rank = serde_json::json!({
         "position": 1,
         "gapToFirstMillis": 0,
         "gapToFirstLaps": 0,
         "carNumber": "8",
         "classId": "HYPERCAR"
      });

      let changed = apply_official_rank(&mut state, Some(&official_rank));
      assert!(changed, "leader change should be detected");

      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      let toyota = entries
         .iter()
         .find(|e| e.car_number == "8")
         .expect("toyota exists");
      assert_eq!(toyota.position, 1);
      assert_eq!(toyota.gap_overall, "-");
   }

   #[test]
   fn apply_signalr_arguments_receive_batch_routing() {
      let mut state = WecLiveState::default();

      // Set up participant
      apply_signalr_arguments(
         &mut state,
         "lv-participants",
         &[serde_json::json!({
             "items": [{ "pid": 1, "carNumber": "7", "teamName": "Toyota" }]
         })][..],
      );

      // Give it an initial position via lv-ranks
      apply_signalr_arguments(
         &mut state,
         "lv-ranks",
         &[serde_json::json!({
             "items": [{ "pid": 1, "carNumber": "7", "overallPosition": 1 }]
         })][..],
      );

      // Route through apply_signalr_arguments with ReceiveBatch target
      // Note: official-rank is now ignored in ReceiveBatch processing
      let args = [serde_json::json!({
         "items": [{
             "channel": "official-rank",
             "view": {
                 "pid": 1,
                 "position": 3,
                 "gapToFirstMillis": 12500,
                 "carNumber": "7",
                 "classId": "HYPERCAR"
             }
         }]
      })];

      let changed = apply_signalr_arguments(&mut state, "ReceiveBatch", &args);
      assert!(!changed, "official-rank in ReceiveBatch should be ignored");

      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      let entry = &entries[0];
      assert_eq!(entry.car_number, "7");
      // Position should remain from lv-ranks (1), not official-rank (3)
      assert_eq!(entry.position, 1);
      // gap_overall should be "-" (leader rule), not from official-rank
      assert_eq!(entry.gap_overall, "-");
   }

   #[test]
   fn apply_receive_batch_multiple_channels_in_one_batch() {
      let mut state = WecLiveState::default();

      // Set up participants
      apply_signalr_arguments(
         &mut state,
         "lv-participants",
         &[serde_json::json!({
             "items": [
                 { "pid": 1, "carNumber": "8", "teamName": "Toyota" },
                 { "pid": 2, "carNumber": "50", "teamName": "Ferrari" }
             ]
         })][..],
      );

      // Set initial positions via lv-ranks
      apply_signalr_arguments(
         &mut state,
         "lv-ranks",
         &[serde_json::json!({
             "items": [
                 { "pid": 1, "carNumber": "8", "overallPosition": 2 },
                 { "pid": 2, "carNumber": "50", "overallPosition": 1 }
             ]
         })][..],
      );

      // Set initial gaps via lv-gaps (since official-rank is ignored)
      apply_signalr_arguments(
         &mut state,
         "lv-gaps",
         &[serde_json::json!({
             "items": [
                 { "pid": 1, "carNumber": "8", "gapToFirstMillis": 3500, "gapToAheadMillis": 3500 },
                 { "pid": 2, "carNumber": "50", "gapToFirstMillis": -1, "gapToAheadMillis": 0 }
             ]
         })][..],
      );

      // Batch with multiple official-rank updates (simulating position swap)
      // Note: official-rank is now ignored in ReceiveBatch, so this should not
      // change anything
      let batch = serde_json::json!({
         "items": [
             {
                 "channel": "official-rank",
                 "view": {
                     "pid": 1,
                     "position": 1,
                     "gapToFirstMillis": 0,
                     "carNumber": "8",
                     "classId": "HYPERCAR"
                 }
             },
             {
                 "channel": "official-rank",
                 "view": {
                     "pid": 2,
                     "position": 2,
                     "gapToFirstMillis": 3500,
                     "carNumber": "50",
                     "classId": "HYPERCAR"
                 }
             }
         ]
      });

      // Since official-rank is ignored, this should return false (no changes)
      let changed = apply_receive_batch(&mut state, &[batch]);
      assert!(!changed, "official-rank batch should be ignored");

      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      assert_eq!(entries.len(), 2);

      let car_8 = entries.iter().find(|e| e.car_number == "8").expect("car 8");
      let car_50 = entries
         .iter()
         .find(|e| e.car_number == "50")
         .expect("car 50");

      // Positions and gaps should remain from lv-ranks/lv-gaps, not official-rank
      assert_eq!(car_8.position, 2);
      assert_eq!(car_8.gap_overall, "+3.500");
      assert_eq!(car_50.position, 1);
      assert_eq!(car_50.gap_overall, "-");
   }

   #[test]
   fn apply_receive_batch_empty_items_no_panic() {
      let mut state = WecLiveState::default();

      // Empty items array
      let batch = serde_json::json!({ "items": [] });
      let changed = apply_receive_batch(&mut state, &[batch]);
      assert!(!changed, "empty batch should not cause changes");

      // Missing items field
      let batch2 = serde_json::json!({ "other": "data" });
      let changed2 = apply_receive_batch(&mut state, &[batch2]);
      assert!(!changed2, "batch without items should not cause changes");
   }

   #[test]
   fn apply_receive_batch_unknown_channel_ignored() {
      let mut state = WecLiveState::default();

      // Set up participant
      apply_signalr_arguments(
         &mut state,
         "lv-participants",
         &[serde_json::json!({
             "items": [{ "pid": 1, "carNumber": "8", "teamName": "Toyota" }]
         })][..],
      );

      // Unknown channel type
      let batch = serde_json::json!({
         "items": [{
             "channel": "unknown-channel",
             "view": { "someData": "value" }
         }]
      });

      let changed = apply_receive_batch(&mut state, &[batch]);
      assert!(!changed, "unknown channel should be ignored");
   }

   #[test]
   fn apply_receive_batch_preserves_existing_lv_data() {
      let mut state = WecLiveState::default();

      // Set up session info with class colors
      apply_signalr_arguments(
         &mut state,
         "lv-session-info",
         &[serde_json::json!({
             "eventName": "WEC Test",
             "sessionName": "Race",
             "sessionClasses": [{"classId":"HYPERCAR","classThreeLettersName":"HYP","classColor":"#ff0000"}]
         })][..],
      );

      // Set up participants
      apply_signalr_arguments(
         &mut state,
         "lv-participants",
         &[serde_json::json!({
             "items": [{ "pid": 1, "carNumber": "8", "teamName": "Toyota", "classId": "HYPERCAR" }]
         })][..],
      );

      // Set lap times via legacy lv-laps
      apply_signalr_arguments(
         &mut state,
         "lv-laps",
         &[serde_json::json!({
             "pid": 1, "carNumber": "8", "lapNumber": 100, "lapTimeMillis": 95321
         })][..],
      );

      // Set position and gap via lv-ranks and lv-gaps
      apply_signalr_arguments(
         &mut state,
         "lv-ranks",
         &[serde_json::json!({
             "items": [{ "pid": 1, "carNumber": "8", "overallPosition": 1 }]
         })][..],
      );
      apply_signalr_arguments(
         &mut state,
         "lv-gaps",
         &[serde_json::json!({
             "items": [{ "pid": 1, "carNumber": "8", "gapToFirstMillis": -1 }]
         })][..],
      );

      // Attempt to update via ReceiveBatch official-rank (should be ignored)
      let batch = serde_json::json!({
         "items": [{
             "channel": "official-rank",
             "view": {
                 "position": 3,
                 "gapToFirstMillis": 5000,
                 "carNumber": "8",
                 "classId": "HYPERCAR"
             }
         }]
      });

      let changed = apply_receive_batch(&mut state, &[batch]);
      assert!(!changed, "official-rank batch should be ignored");

      let (header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      assert_eq!(header.event_name, "WEC Test");
      assert_eq!(header.session_name, "Race");

      let entry = &entries[0];
      assert_eq!(entry.car_number, "8");
      // Position should remain from lv-ranks (1), not official-rank
      assert_eq!(entry.position, 1);
      assert_eq!(entry.best_lap, "1:35.321"); // From lv-laps
      assert_eq!(entry.class_name, "HYP"); // Resolved from classId
   }

   // =========================================================================
   // Leader gap_overall fix tests
   // =========================================================================

   #[test]
   fn leader_gap_overall_is_always_dash() {
      // Regression test: Leader (position 1) should always have gap_overall = "-"
      // even if the server sends gapToFirstMillis=0, gapToFirstLaps=1 via gaps
      let mut state = WecLiveState::default();

      // Set up participant
      apply_signalr_arguments(
         &mut state,
         "lv-participants",
         &[serde_json::json!({
             "items": [{ "pid": 1, "carNumber": "8", "teamName": "Toyota" }]
         })][..],
      );

      // Set initial position via lv-ranks
      apply_signalr_arguments(
         &mut state,
         "lv-ranks",
         &[serde_json::json!({
             "items": [{ "pid": 1, "carNumber": "8", "overallPosition": 1 }]
         })][..],
      );

      // Official rank update is ignored, but we can test leader rule via
      // sector-cross-updates with gapToFirstLaps: 1 (which would show "+1 L")
      // for a leader
      let batch = serde_json::json!({
         "items": [{
             "channel": "sector-cross-updates",
             "view": {
                 "ranks": {"items": [{ "pid": 1, "carNumber": "8", "overallPosition": 1, "position": 1 }]},
                 "gaps": {"items": [{ "pid": 1, "carNumber": "8", "gapToFirstMillis": 0, "gapToFirstLaps": 1, "gapToAheadMillis": 0 }]}
             }
         }]
      });

      apply_receive_batch(&mut state, &[batch]);

      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      let entry = entries.iter().find(|e| e.car_number == "8").expect("car 8");

      assert_eq!(entry.position, 1, "Leader should be in position 1");
      assert_eq!(
         entry.gap_overall, "-",
         "Leader should have gap_overall = '-', not '+1 L'"
      );
   }

   #[test]
   fn apply_official_rank_leader_gap_overall_is_dash() {
      // Direct test of apply_official_rank with leader position
      let mut state = WecLiveState::default();

      // Set up participant
      apply_signalr_arguments(
         &mut state,
         "lv-participants",
         &[serde_json::json!({
             "items": [{ "pid": 1, "carNumber": "8", "teamName": "Toyota" }]
         })][..],
      );

      // Apply official rank with position 1 and gapToFirstLaps: 1
      let official_rank = serde_json::json!({
         "pid": 1,
         "position": 1,
         "gapToFirstMillis": 0,
         "gapToFirstLaps": 1,  // Server incorrectly sends this for leader
         "carNumber": "8",
         "classId": "HYPERCAR"
      });

      let changed = apply_official_rank(&mut state, Some(&official_rank));
      assert!(changed, "official-rank should cause changes");

      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      let entry = entries.iter().find(|e| e.car_number == "8").expect("car 8");

      assert_eq!(entry.position, 1);
      assert_eq!(entry.gap_overall, "-", "Leader gap_overall should be '-'");
   }

   // =========================================================================
   // Historical lap data protection tests
   // =========================================================================

   #[test]
   fn apply_laps_historical_data_cannot_reduce_lap_count() {
      // Regression test: Historical lap data should not reduce the lap count
      // /live/laps/{sid} returns historical rows newest first
      // Older rows should not overwrite newer lap counts
      let mut state = WecLiveState::default();

      // Set up participant
      apply_signalr_arguments(
         &mut state,
         "lv-participants",
         &[serde_json::json!({
             "items": [{ "pid": 1, "carNumber": "8", "teamName": "Toyota" }]
         })][..],
      );

      // Set initial laps to 100 via lv-laps
      apply_signalr_arguments(
         &mut state,
         "lv-laps",
         &[serde_json::json!({
             "pid": 1, "carNumber": "8", "lapNumber": 100, "lapTimeMillis": 95321
         })][..],
      );

      // Verify initial state
      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      let entry = entries.iter().find(|e| e.car_number == "8").expect("car 8");
      assert_eq!(entry.laps, "100", "Initial laps should be 100");

      // Apply historical lap data (older lap 50) - should NOT reduce laps
      apply_signalr_arguments(
         &mut state,
         "lv-laps",
         &[serde_json::json!({
             "pid": 1, "carNumber": "8", "lapNumber": 50, "lapTimeMillis": 98234
         })][..],
      );

      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      let entry = entries.iter().find(|e| e.car_number == "8").expect("car 8");
      assert_eq!(
         entry.laps, "100",
         "Laps should remain 100, not reduced to 50"
      );

      // Apply newer lap data (lap 150) - should increase laps
      apply_signalr_arguments(
         &mut state,
         "lv-laps",
         &[serde_json::json!({
             "pid": 1, "carNumber": "8", "lapNumber": 150, "lapTimeMillis": 96123
         })][..],
      );

      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      let entry = entries.iter().find(|e| e.car_number == "8").expect("car 8");
      assert_eq!(entry.laps, "150", "Laps should increase to 150");
   }

   #[test]
   fn apply_laps_newer_lap_updates_can_increase_lap_count() {
      // Test that newer lap updates correctly increase lap count
      let mut state = WecLiveState::default();

      // Set up participant
      apply_signalr_arguments(
         &mut state,
         "lv-participants",
         &[serde_json::json!({
             "items": [{ "pid": 1, "carNumber": "8", "teamName": "Toyota" }]
         })][..],
      );

      // Set initial laps to 50
      apply_signalr_arguments(
         &mut state,
         "lv-laps",
         &[serde_json::json!({
             "pid": 1, "carNumber": "8", "lapNumber": 50, "lapTimeMillis": 98234
         })][..],
      );

      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      let entry = entries.iter().find(|e| e.car_number == "8").expect("car 8");
      assert_eq!(entry.laps, "50", "Initial laps should be 50");

      // Apply newer lap (100) - should increase laps
      apply_signalr_arguments(
         &mut state,
         "lv-laps",
         &[serde_json::json!({
             "pid": 1, "carNumber": "8", "lapNumber": 100, "lapTimeMillis": 95321
         })][..],
      );

      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      let entry = entries.iter().find(|e| e.car_number == "8").expect("car 8");
      assert_eq!(entry.laps, "100", "Laps should increase to 100");

      // Apply even newer lap (200) - should increase laps further
      apply_signalr_arguments(
         &mut state,
         "lv-laps",
         &[serde_json::json!({
             "pid": 1, "carNumber": "8", "lapNumber": 200, "lapTimeMillis": 94000
         })][..],
      );

      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      let entry = entries.iter().find(|e| e.car_number == "8").expect("car 8");
      assert_eq!(entry.laps, "200", "Laps should increase to 200");
   }

   #[test]
   fn apply_receive_batch_laps_updates_with_newer_only_protection() {
      // Test ReceiveBatch laps channel with newer-only protection
      let mut state = WecLiveState::default();

      // Set up participant
      apply_signalr_arguments(
         &mut state,
         "lv-participants",
         &[serde_json::json!({
             "items": [{ "pid": 1, "carNumber": "8", "teamName": "Toyota" }]
         })][..],
      );

      // Set initial laps to 100 via ReceiveBatch laps channel
      let batch1 = serde_json::json!({
         "items": [{
             "channel": "laps",
             "view": {
                 "pid": 1,
                 "carNumber": "8",
                 "lapNumber": 100,
                 "lapTimeMillis": 95321
             }
         }]
      });

      apply_receive_batch(&mut state, &[batch1]);

      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      let entry = entries.iter().find(|e| e.car_number == "8").expect("car 8");
      assert_eq!(entry.laps, "100", "Initial laps should be 100");

      // Try to apply older lap (50) via ReceiveBatch - should NOT reduce laps
      let batch2 = serde_json::json!({
         "items": [{
             "channel": "laps",
             "view": {
                 "pid": 1,
                 "carNumber": "8",
                 "lapNumber": 50,
                 "lapTimeMillis": 98234
             }
         }]
      });

      apply_receive_batch(&mut state, &[batch2]);

      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      let entry = entries.iter().find(|e| e.car_number == "8").expect("car 8");
      assert_eq!(
         entry.laps, "100",
         "Laps should remain 100 after historical data"
      );

      // Apply newer lap (150) via ReceiveBatch - should increase laps
      let batch3 = serde_json::json!({
         "items": [{
             "channel": "laps",
             "view": {
                 "pid": 1,
                 "carNumber": "8",
                 "lapNumber": 150,
                 "lapTimeMillis": 94000
             }
         }]
      });

      apply_receive_batch(&mut state, &[batch3]);

      let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
      let entry = entries.iter().find(|e| e.car_number == "8").expect("car 8");
       assert_eq!(entry.laps, "150", "Laps should increase to 150");
   }

   #[test]
   fn apply_receive_batch_race_flags_updates_header_flag() {
      // Test that ReceiveBatch with channel "race-flags" updates header.flag
      let mut state = WecLiveState::default();

      // Apply initial Green flag via ReceiveBatch
      let batch1 = serde_json::json!({
         "items": [{
            "channel": "race-flags",
            "view": {
               "raceFlagID": "20260613222259842",
               "flag": "Green",
               "ts": "2026-06-13T22:22:59.842+00:00",
               "elapsedTimeMillis": 30_179_842
            }
         }]
      });

      let changed = apply_receive_batch(&mut state, &[batch1]);
      assert!(changed, "ReceiveBatch race-flags should update flag");
      assert_eq!(state.header.flag, "Green", "Flag should be Green");

      // Apply SafetyCar flag via ReceiveBatch - should normalize to Yellow (SC)
      let batch2 = serde_json::json!({
         "items": [{
            "channel": "race-flags",
            "view": {
               "raceFlagID": "20260613213804930",
               "flag": "SafetyCar",
               "ts": "2026-06-13T22:38:04.930+00:00",
               "elapsedTimeMillis": 27_484_930
            }
         }]
      });

      let changed2 = apply_receive_batch(&mut state, &[batch2]);
      assert!(changed2, "ReceiveBatch SafetyCar should update flag");
      assert_eq!(
         state.header.flag, "Yellow (SC)",
         "SafetyCar should normalize to Yellow (SC)"
      );

      // Apply newer Green flag - should override
      let batch3 = serde_json::json!({
         "items": [{
            "channel": "race-flags",
            "view": {
               "raceFlagID": "20260613230000000",
               "flag": "Green",
               "ts": "2026-06-13T23:00:00.000+00:00",
               "elapsedTimeMillis": 36_000_000
            }
         }]
      });

      let changed3 = apply_receive_batch(&mut state, &[batch3]);
      assert!(changed3, "Newer Green flag should update");
      assert_eq!(state.header.flag, "Green", "Flag should revert to Green");
   }

   // =========================================================================
   // Session clock TTE calculation tests
   // =========================================================================

   #[test]
   fn apply_session_clock_calculates_remaining_time_from_elapsed() {
      // Given a 6-hour race (21,600,000 ms) with 1 hour elapsed (3,600,000 ms)
      // Should display 5 hours remaining
      // Set session length from schedule (simulating
      // fetch_session_length_from_schedule)
      let mut state = WecLiveState {
         session_length_ms: Some(21_600_000),
         ..WecLiveState::default()
      };

      let payload = serde_json::json!({
          "elapsedTimeMillis": 3_600_000,
          "tsNow": "2026-06-13T12:00:00Z"
      });

      apply_session_clock(&mut state, &payload);

      // 6h - 1h = 5h remaining = 05:00:00
      assert_eq!(
         state.header.time_to_go, "05:00:00",
         "Expected 5 hours remaining (6h - 1h), got: {}",
         state.header.time_to_go
      );
   }

   #[test]
   fn apply_session_clock_uses_explicit_session_length() {
      // Given a 6-hour race (21,600,000 ms) with 2 hours elapsed
      // Should display 4 hours remaining
      let mut state = WecLiveState::default();
      let payload = serde_json::json!({
          "elapsedTimeMillis": 7_200_000,
          "sessionLengthMillis": 21_600_000,
          "tsNow": "2026-06-13T14:00:00Z"
      });

      apply_session_clock(&mut state, &payload);

      // 4 hours remaining = 14,400,000 ms = 04:00:00
      assert_eq!(
         state.header.time_to_go, "04:00:00",
         "Expected 4 hours remaining, got: {}",
         state.header.time_to_go
      );
   }

   #[test]
   fn apply_session_clock_elapsed_time_millis_now_priority() {
      // elapsedTimeMillisNow should take priority over elapsedTimeMillis
      let mut state = WecLiveState::default();
      let payload = serde_json::json!({
          "elapsedTimeMillis": 3_600_000,    // 1 hour (should be ignored)
          "elapsedTimeMillisNow": 7_200_000, // 2 hours (should be used)
          "sessionLengthMillis": 21_600_000, // 6 hours
          "tsNow": "2026-06-13T14:00:00Z"
      });

      apply_session_clock(&mut state, &payload);

      // 6 hours - 2 hours = 4 hours = 04:00:00
      assert_eq!(
         state.header.time_to_go, "04:00:00",
         "Expected elapsedTimeMillisNow to be prioritized"
      );
   }

   #[test]
   fn apply_session_clock_uses_fallback_field_names() {
      // Test that alternative field names work
      let mut state = WecLiveState::default();

      // Test with maxSessionLength
      let payload = serde_json::json!({
          "elapsedTimeMillis": 7_200_000,
          "maxSessionLength": 21_600_000,
          "tsNow": "2026-06-13T14:00:00Z"
      });

      apply_session_clock(&mut state, &payload);

      assert_eq!(
         state.header.time_to_go, "04:00:00",
         "Expected maxSessionLength to work"
      );
   }

   #[test]
   fn apply_session_clock_negative_elapsed_ignored() {
      // Negative elapsed time should be ignored (doesn't update time_to_go)
      // but tsNow still updates day_time, so overall changed will be true
      let mut state = WecLiveState::default();
      let payload = serde_json::json!({
          "elapsedTimeMillis": -1000,
          "tsNow": "2026-06-13T12:00:00Z"
      });

      let _changed = apply_session_clock(&mut state, &payload);

      // day_time should be updated from tsNow, but time_to_go should remain empty
      assert!(
         state.header.time_to_go.is_empty(),
         "time_to_go should remain empty with negative elapsed"
      );
      assert!(
         !state.header.day_time.is_empty(),
         "day_time should be updated from tsNow"
      );
   }

   #[test]
   fn apply_session_clock_zero_session_length_ignored() {
      // Zero or negative session length means no TTE calculation
      // (session length must come from schedule's lengthLimit)
      // Invalid zero length from bad schedule data
      let mut state = WecLiveState {
         session_length_ms: Some(0),
         ..WecLiveState::default()
      };
      let payload = serde_json::json!({
          "elapsedTimeMillis": 3_600_000,
          "tsNow": "2026-06-13T12:00:00Z"
      });

      let _changed = apply_session_clock(&mut state, &payload);

      // With zero session length, no TTE should be calculated
      // (the saturating_sub would result in 0, showing "00:00:00")
      assert_eq!(
         state.header.time_to_go, "00:00",
         "Should show 00:00 when session length is zero"
      );
   }

   #[test]
   fn apply_session_clock_session_length_persists() {
      // Session length should persist between calls
      let mut state = WecLiveState::default();

      // First call sets session length
      let payload1 = serde_json::json!({
          "elapsedTimeMillis": 3_600_000,
          "sessionLengthMillis": 21_600_000,
          "tsNow": "2026-06-13T12:00:00Z"
      });
      apply_session_clock(&mut state, &payload1);
      assert_eq!(state.header.time_to_go, "05:00:00");

      // Second call without session length should use cached value
      let payload2 = serde_json::json!({
          "elapsedTimeMillis": 7_200_000,
          "tsNow": "2026-06-13T13:00:00Z"
      });
      apply_session_clock(&mut state, &payload2);

      // Should use cached 6h length: 6h - 2h = 4h
      assert_eq!(
         state.header.time_to_go, "04:00:00",
         "Should use cached session length"
      );
   }

   #[test]
   fn apply_session_clock_updates_session_length() {
      // Session length can be updated
      let mut state = WecLiveState::default();

      // First call: 6 hour session with 1 hour elapsed
      let payload1 = serde_json::json!({
          "elapsedTimeMillis": 3_600_000,
          "sessionLengthMillis": 21_600_000,
          "tsNow": "2026-06-13T12:00:00Z"
      });
      apply_session_clock(&mut state, &payload1);
      assert_eq!(state.header.time_to_go, "05:00:00");

      // Second call updates to 8 hour session with 2 hours elapsed
      let payload2 = serde_json::json!({
          "elapsedTimeMillis": 7_200_000,
          "sessionLengthMillis": 28_800_000, // 8 hours
          "tsNow": "2026-06-13T13:00:00Z"
      });
      apply_session_clock(&mut state, &payload2);

      // 8h - 2h = 6h = 06:00:00
      assert_eq!(
         state.header.time_to_go, "06:00:00",
         "Should use updated session length"
      );
   }

   #[test]
   fn apply_session_clock_calculates_24h_race_correctly() {
      // Le Mans 24h race at 12 hours elapsed
      let mut state = WecLiveState::default();
      let payload = serde_json::json!({
          "elapsedTimeMillis": 43_200_000,    // 12 hours
          "sessionLengthMillis": 86_400_000, // 24 hours
          "tsNow": "2026-06-13T12:00:00Z"
      });

      apply_session_clock(&mut state, &payload);

      // Should show 12 hours remaining
      assert_eq!(
         state.header.time_to_go, "12:00:00",
         "Expected 12 hours remaining in 24h race"
      );
   }

   #[test]
   fn apply_session_clock_8h_race_with_session_length_from_schedule() {
      // Session length must come from schedule's lengthLimit
      // Set it explicitly here (simulating fetch_session_length_from_schedule)
      // 8 hours from schedule
      let mut state = WecLiveState {
         session_length_ms: Some(28_800_000),
         ..WecLiveState::default()
      };

      let payload = serde_json::json!({
          "elapsedTimeMillis": 28_800_000, // 8 hours elapsed
          "tsNow": "2026-06-13T12:00:00Z"
      });

      apply_session_clock(&mut state, &payload);

      // 8h - 8h = 0, so TTE should be 00:00
      assert_eq!(
         state.header.time_to_go, "00:00",
         "Expected 0 hours remaining (8h - 8h)"
      );
   }
}

/// Race control message from the WEC insights API.
#[derive(Debug, Deserialize)]
struct RaceControlMessage {
   #[serde(rename = "raceControlMessageID")]
   id:   String,
   text: String,
   #[serde(rename = "ts", default)]
   ts:   Option<String>,
}

/// Fetch and process race control messages from the WEC API.
/// Returns a Vec of `TimingNotice` for new messages.
fn fetch_racecontrol_messages(
   client: &Client,
   sid: u64,
   seen_ids: &mut HashSet<String>,
) -> Result<Vec<TimingNotice>, String> {
   let url = format!("{LIVE_BASE_URL}/racecontrol-messages/{sid}");
   let response = client
      .get(&url)
      .send()
      .map_err(|err| format!("racecontrol-messages request failed: {err}"))?;

   if !response.status().is_success() {
      return Err(format!(
         "racecontrol-messages endpoint failed with HTTP {}",
         response.status()
      ));
   }

   let body = response
      .text()
      .map_err(|err| format!("racecontrol-messages body read failed: {err}"))?;
   let messages: Vec<RaceControlMessage> = serde_json::from_str(&body)
      .map_err(|err| format!("racecontrol-messages decode failed: {err}"))?;

   let notices: Vec<TimingNotice> = messages
      .into_iter()
      .filter(|msg| !msg.text.trim().is_empty() && seen_ids.insert(msg.id.clone()))
      .map(|msg| {
         TimingNotice {
            id:   format!("wec_rc_{}", msg.id),
            time: msg.ts.as_deref().map_or_else(
               || {
                  // Use current time as fallback
                  let now = std::time::SystemTime::now()
                     .duration_since(std::time::UNIX_EPOCH)
                     .unwrap_or_default();
                  let total_secs = now.as_secs();
                  let hours = (total_secs / 3600) % 24;
                  let mins = (total_secs / 60) % 60;
                  let secs = total_secs % 60;
                  format!("{hours:02}:{mins:02}:{secs:02}")
               },
               compact_iso_timestamp,
            ),
            text: msg.text,
         }
      })
      .collect();

   Ok(notices)
}

const POLL_INTERVAL: Duration = Duration::from_secs(12);

/// Spawn a thread that polls the race control messages endpoint.
fn spawn_racecontrol_polling_thread(
   tx: Sender<TimingMessage>,
   source_id: u64,
   stop_rx: Receiver<()>,
   client: Client,
   sid: u64,
) -> thread::JoinHandle<()> {
   thread::spawn(move || {
      let mut seen_ids: HashSet<String> = HashSet::new();

      loop {
         // Check for stop signal
         if stop_rx.try_recv().is_ok() {
            break;
         }

         // Poll for messages
         match fetch_racecontrol_messages(&client, sid, &mut seen_ids) {
            Ok(notices) => {
               for notice in notices {
                  let _ = tx.send(TimingMessage::Notice { source_id, notice });
               }
            },
            Err(err) => {
               // Log error but don't crash - retry on next poll
               let _ = tx.send(TimingMessage::Error {
                  source_id,
                  text: format!("WEC racecontrol poll: {err}"),
               });
            },
         }

         // Wait for next poll or stop signal
         match stop_rx.recv_timeout(POLL_INTERVAL) {
            Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {},
         }
      }
   })
}
