use std::{
    collections::VecDeque,
    sync::mpsc::{self, Receiver, Sender},
    time::Instant,
};

use crate::{
    feed::spawn::spawn_series_worker,
    timing::{Series, TimingEntry, TimingHeader, TimingMessage, TimingNotice},
    timing_persist::SeriesDebugOutput,
};

#[derive(Debug)]
pub(crate) struct ActiveFeed {
    pub(crate) source_id: u64,
    stop_tx: Sender<()>,
    debug_rx: Option<Receiver<String>>,
}

pub(crate) const IMSA_DEBUG_LOG_CAPACITY: usize = 150;

pub(crate) fn start_feed(series: Series, tx: Sender<TimingMessage>, source_id: u64) -> ActiveFeed {
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (debug_tx, debug_rx) = mpsc::channel::<String>();
    let debug_output = SeriesDebugOutput::Channel(debug_tx);

    spawn_series_worker(series, tx, source_id, stop_rx, debug_output);

    ActiveFeed {
        source_id,
        stop_tx,
        debug_rx: Some(debug_rx),
    }
}

pub(crate) fn stop_feed(feed: &mut Option<ActiveFeed>) {
    if let Some(active_feed) = feed.take() {
        let _ = active_feed.stop_tx.send(());
    }
}

pub(crate) fn push_series_debug_log(logs: &mut VecDeque<String>, line: String) {
    logs.push_back(line);
    while logs.len() > IMSA_DEBUG_LOG_CAPACITY {
        logs.pop_front();
    }
}

pub(crate) fn drain_series_debug_logs(feed: &Option<ActiveFeed>, logs: &mut VecDeque<String>) {
    let Some(active_feed) = feed.as_ref() else {
        return;
    };
    let Some(debug_rx) = active_feed.debug_rx.as_ref() else {
        return;
    };

    while let Ok(line) = debug_rx.try_recv() {
        push_series_debug_log(logs, line);
    }
}

pub(crate) fn drain_messages(
    rx: &Receiver<TimingMessage>,
    active_source_id: u64,
    header: &mut TimingHeader,
    entries: &mut Vec<TimingEntry>,
    status: &mut String,
    last_error: &mut Option<String>,
    last_update: &mut Option<Instant>,
) -> Vec<TimingNotice> {
    let mut notices = Vec::new();
    while let Ok(msg) = rx.try_recv() {
        match msg {
            TimingMessage::Status { source_id, text } if source_id == active_source_id => {
                *status = text
            }
            TimingMessage::Error { source_id, text } if source_id == active_source_id => {
                *last_error = Some(text)
            }
            TimingMessage::Snapshot {
                source_id,
                header: new_header,
                entries: new_entries,
            } if source_id == active_source_id => {
                if new_header.event_name != "-" {
                    header.event_name = new_header.event_name;
                }
                if new_header.session_name != "-" {
                    header.session_name = new_header.session_name;
                }
                if !new_header.session_type_raw.trim().is_empty()
                    && new_header.session_type_raw != "-"
                {
                    header.session_type_raw = new_header.session_type_raw;
                }
                if new_header.track_name != "-" {
                    header.track_name = new_header.track_name;
                }
                if new_header.day_time != "-" {
                    header.day_time = new_header.day_time;
                }
                if new_header.flag != "-" {
                    header.flag = new_header.flag;
                }
                if new_header.time_to_go != "-" {
                    header.time_to_go = new_header.time_to_go;
                }
                *entries = new_entries;
                *status = "Live timing connected".to_string();
                *last_error = None;
                *last_update = Some(Instant::now());
            }
            TimingMessage::Notice { source_id, notice } if source_id == active_source_id => {
                notices.push(notice)
            }
            _ => {}
        }
    }
    notices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_messages_merges_session_type_raw_from_snapshot() {
        let (tx, rx) = mpsc::channel::<TimingMessage>();
        let source_id = 7_u64;
        tx.send(TimingMessage::Snapshot {
            source_id,
            header: TimingHeader {
                session_name: "Race 1".to_string(),
                session_type_raw: "R".to_string(),
                ..TimingHeader::default()
            },
            entries: Vec::new(),
        })
        .expect("snapshot send should succeed");

        let mut header = TimingHeader::default();
        let mut entries = Vec::new();
        let mut status = String::new();
        let mut last_error = None;
        let mut last_update = None;

        let notices = drain_messages(
            &rx,
            source_id,
            &mut header,
            &mut entries,
            &mut status,
            &mut last_error,
            &mut last_update,
        );

        assert!(notices.is_empty());
        assert_eq!(header.session_name, "Race 1");
        assert_eq!(header.session_type_raw, "R");
        assert_eq!(status, "Live timing connected");
        assert!(last_error.is_none());
        assert!(last_update.is_some());
    }
}
