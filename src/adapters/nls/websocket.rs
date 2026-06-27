// NLS WebSocket handling

use std::{
   sync::mpsc::{
      Receiver,
      Sender,
   },
   time::{
      Duration,
      Instant,
   },
};

use serde_json::json;
use tungstenite::{
   connect,
   Message,
   WebSocket,
};

use crate::{
   adapters::{
      nls::{
         protocol::{
            is_retriable_timeout,
            is_transient_disconnect,
            notices_from_ws_message,
            parse_ws_message,
            websocket_ping_due,
            websocket_stale_elapsed,
         },
         snapshot::persist_snapshot_if_dirty,
         state::{
            build_website_client,
            check_event_id_change,
            refresh_website_data,
            update_timing_state,
            NlsWorkerContext,
         },
      },
      nurburgring_ws,
   },
   timing::TimingMessage,
   timing_persist::{
      log_series_debug,
      SeriesDebugOutput,
   },
};

const WS_URL: &str = "wss://livetiming.azurewebsites.net/";
const RECONNECT_DELAY: Duration = Duration::from_secs(3);
const NLS_PING_INTERVAL: Duration = Duration::from_secs(10);
const NLS_STALE_TIMEOUT: Duration = Duration::from_secs(30);

/// Type alias for the websocket socket type.
pub type NlsWebSocket = WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

pub fn websocket_worker(tx: &Sender<TimingMessage>, source_id: u64, stop_rx: &Receiver<()>) {
   websocket_worker_with_debug(tx, source_id, stop_rx, &SeriesDebugOutput::Silent);
}

pub fn websocket_worker_with_debug(
   tx: &Sender<TimingMessage>,
   source_id: u64,
   stop_rx: &Receiver<()>,
   debug_output: &SeriesDebugOutput,
) {
   let client = build_website_client();
   let mut ctx = NlsWorkerContext::new(tx, source_id, debug_output);

   'outer: loop {
      if stop_rx.try_recv().is_ok() {
         if let Some(snapshot) = ctx.last_good_live_snapshot.as_ref() {
            persist_snapshot_if_dirty(
               &mut ctx.persist,
               snapshot,
               u64::try_from(now_millis()).expect("timestamp should fit in u64"),
               debug_output,
            );
         }
         break;
      }

      refresh_website_data(&client, &mut ctx, tx, source_id);

      let Some((mut socket, subscribed_event_id)) =
         connect_to_websocket(tx, source_id, stop_rx, debug_output, &ctx.active_event_id)
      else {
         continue;
      };

      let mut connected_status_sent = true;

      let mut message_loop = MessageLoop {
         socket: &mut socket,
         ctx: &mut ctx,
         tx,
         source_id,
         stop_rx,
         debug_output,
         client: client.as_ref(),
         subscribed_event_id: &subscribed_event_id,
         connected_status_sent: &mut connected_status_sent,
      };

      if handle_message_loop(&mut message_loop) {
         break 'outer;
      }

      let _ = tx.send(TimingMessage::Status {
         source_id,
         text: "NLS reconnecting in 3s...".to_string(),
      });
      log_series_debug(debug_output, "NLS", "reconnecting in 3s");

      if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
         break;
      }
   }
}

/// Establish websocket connection and return socket with subscribed event ID.
fn connect_to_websocket(
   tx: &Sender<TimingMessage>,
   source_id: u64,
   stop_rx: &Receiver<()>,
   debug_output: &SeriesDebugOutput,
   active_event_id: &str,
) -> Option<(NlsWebSocket, String)> {
   let _ = tx.send(TimingMessage::Status {
      source_id,
      text: "Connecting to NLS websocket...".to_string(),
   });
   log_series_debug(debug_output, "NLS", "connecting websocket");

   let request = nurburgring_ws::build_request(WS_URL, "https://livetiming.azurewebsites.net");
   let connection = connect(request);

   let (mut socket, response) = match connection {
      Ok(ok) => ok,
      Err(err) => {
         let _ = tx.send(TimingMessage::Error {
            source_id,
            text: format!("connect failed: {err}"),
         });
         if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
            return None;
         }
         return None;
      },
   };

   nurburgring_ws::set_socket_timeout(&mut socket);

   let _ = tx.send(TimingMessage::Status {
      source_id,
      text: format!("NLS connected ({})", response.status()),
   });
   log_series_debug(
      debug_output,
      "NLS",
      format!("websocket connected ({})", response.status()),
   );

   let subscribe = json!({
       "clientLocalTime": now_millis(),
       "eventId": active_event_id,
       "eventPid": [0, 3, 4]
   });

   if let Err(err) = socket.send(Message::Text(subscribe.to_string().into())) {
      let _ = tx.send(TimingMessage::Error {
         source_id,
         text: format!("subscribe failed: {err}"),
      });
      if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
         return None;
      }
      return None;
   }

   log_series_debug(
      debug_output,
      "NLS",
      format!("subscribed eventId {active_event_id}"),
   );

   Some((socket, active_event_id.to_string()))
}

/// Main message loop for processing websocket messages.
/// Returns true if worker should stop (break outer loop).
struct MessageLoop<'a> {
   socket:                &'a mut NlsWebSocket,
   ctx:                   &'a mut NlsWorkerContext,
   tx:                    &'a Sender<TimingMessage>,
   source_id:             u64,
   stop_rx:               &'a Receiver<()>,
   debug_output:          &'a SeriesDebugOutput,
   client:                Option<&'a reqwest::blocking::Client>,
   subscribed_event_id:   &'a str,
   connected_status_sent: &'a mut bool,
}

fn handle_message_loop(loop_ctx: &mut MessageLoop<'_>) -> bool {
   let mut last_activity_at = Instant::now();
   let mut last_ping_at = last_activity_at;

   loop {
      if loop_ctx.stop_rx.try_recv().is_ok() {
         if let Some(snapshot) = loop_ctx.ctx.last_good_live_snapshot.as_ref() {
            persist_snapshot_if_dirty(
               &mut loop_ctx.ctx.persist,
               snapshot,
               now_unix_ms(),
               loop_ctx.debug_output,
            );
         }
         return true;
      }

      // Check for event ID changes requiring reconnection
      if std::time::Instant::now() >= loop_ctx.ctx.next_website_refresh
         && check_event_id_change(
            loop_ctx.ctx,
            loop_ctx.tx,
            loop_ctx.source_id,
            loop_ctx.client,
            loop_ctx.subscribed_event_id,
         )
      {
         return false;
      }

      let now = Instant::now();
      match loop_ctx.socket.read() {
         Ok(Message::Text(text)) => {
            last_activity_at = now;
            process_text_message(
               &text,
               loop_ctx.tx,
               loop_ctx.source_id,
               loop_ctx.debug_output,
               loop_ctx.ctx,
               loop_ctx.connected_status_sent,
            );
         },
         Ok(Message::Binary(data)) => {
            last_activity_at = now;
            process_binary_message(
               &data,
               loop_ctx.tx,
               loop_ctx.source_id,
               loop_ctx.debug_output,
               loop_ctx.ctx,
               loop_ctx.connected_status_sent,
            );
         },
         Ok(Message::Ping(data)) => {
            last_activity_at = now;
            if let Err(err) = loop_ctx.socket.send(Message::Pong(data)) {
               let _ = loop_ctx.tx.send(TimingMessage::Error {
                  source_id: loop_ctx.source_id,
                  text:      format!("pong failed: {err}"),
               });
               break;
            }
         },
         Ok(Message::Pong(_) | Message::Frame(_)) => {
            last_activity_at = now;
         },
         Ok(Message::Close(frame)) => {
            let _ = loop_ctx.tx.send(TimingMessage::Error {
               source_id: loop_ctx.source_id,
               text:      format!("socket closed: {frame:?}"),
            });
            break;
         },
         Err(err) if is_retriable_timeout(&err) => {
            if websocket_stale_elapsed(last_activity_at, now, NLS_STALE_TIMEOUT) {
               let _ = loop_ctx.tx.send(TimingMessage::Status {
                  source_id: loop_ctx.source_id,
                  text:      "NLS websocket stale for 30s; reconnecting".to_string(),
               });
               break;
            } else if websocket_ping_due(last_ping_at, now, NLS_PING_INTERVAL) {
               if let Err(err) = loop_ctx.socket.send(Message::Ping(Vec::new().into())) {
                  let _ = loop_ctx.tx.send(TimingMessage::Error {
                     source_id: loop_ctx.source_id,
                     text:      format!("ping failed: {err}"),
                  });
                  break;
               }
               last_ping_at = Instant::now();
            }
         },
         Err(err) => {
            if is_transient_disconnect(&err) {
               let _ = loop_ctx.tx.send(TimingMessage::Status {
                  source_id: loop_ctx.source_id,
                  text:      "NLS connection reset; reconnecting...".to_string(),
               });
            } else {
               let _ = loop_ctx.tx.send(TimingMessage::Error {
                  source_id: loop_ctx.source_id,
                  text:      format!("read failed: {err}"),
               });
            }
            break;
         },
      }
   }
   false
}

/// Process a text websocket message.
fn process_text_message(
   text: &str,
   tx: &Sender<TimingMessage>,
   source_id: u64,
   debug_output: &SeriesDebugOutput,
   ctx: &mut NlsWorkerContext,
   connected_status_sent: &mut bool,
) {
   for notice in notices_from_ws_message(text) {
      let _ = tx.send(TimingMessage::Notice { source_id, notice });
   }

   let Some((entries, header_changed)) = parse_ws_message(
      text,
      &mut ctx.header,
      ctx.termine_event_name.as_deref(),
      ctx.homepage_event_name.as_deref(),
      &mut ctx.countdown,
      &mut ctx.is_race_session,
      &ctx.active_event_id,
   ) else {
      return;
   };

   update_timing_state(
      entries,
      header_changed,
      tx,
      source_id,
      debug_output,
      ctx,
      connected_status_sent,
   );
}

/// Process a binary websocket message (converts to text then processes).
fn process_binary_message(
   data: &[u8],
   tx: &Sender<TimingMessage>,
   source_id: u64,
   debug_output: &SeriesDebugOutput,
   ctx: &mut NlsWorkerContext,
   connected_status_sent: &mut bool,
) {
   let Ok(text) = std::str::from_utf8(data) else {
      return;
   };

   for notice in notices_from_ws_message(text) {
      let _ = tx.send(TimingMessage::Notice { source_id, notice });
   }

   let Some((entries, header_changed)) = parse_ws_message(
      text,
      &mut ctx.header,
      ctx.termine_event_name.as_deref(),
      ctx.homepage_event_name.as_deref(),
      &mut ctx.countdown,
      &mut ctx.is_race_session,
      &ctx.active_event_id,
   ) else {
      return;
   };

   update_timing_state(
      entries,
      header_changed,
      tx,
      source_id,
      debug_output,
      ctx,
      connected_status_sent,
   );
}

fn now_millis() -> u128 {
   std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap()
      .as_millis()
}

fn now_unix_ms() -> u64 {
   u64::try_from(now_millis()).unwrap_or(u64::MAX)
}
