use std::{
    collections::HashMap,
    sync::mpsc::{self, Sender},
    thread,
};

use crate::{
    f1::signalr_worker,
    imsa::polling_worker,
    nls::websocket_worker,
    timing::{Series, TimingMessage},
};

use super::state::WebAppState;

pub struct FeedController {
    stop_txs: Vec<Sender<()>>,
}

impl FeedController {
    pub fn stop_all(&self) {
        for stop_tx in &self.stop_txs {
            let _ = stop_tx.send(());
        }
    }
}

pub fn start_feed_bridge(state: WebAppState) -> FeedController {
    let (tx, rx) = mpsc::channel::<TimingMessage>();
    let mut stop_txs = Vec::new();
    let mut source_to_series = HashMap::new();

    // We keep source ids stable and explicit so incoming worker messages can be
    // routed to the right series without guessing.
    for (source_id, series) in [
        (1_u64, Series::Imsa),
        (2_u64, Series::Nls),
        (3_u64, Series::F1),
    ] {
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        stop_txs.push(stop_tx);
        source_to_series.insert(source_id, series);
        let worker_tx = tx.clone();
        thread::spawn(move || match series {
            Series::Imsa => polling_worker(worker_tx, source_id, stop_rx),
            Series::Nls => websocket_worker(worker_tx, source_id, stop_rx),
            Series::F1 => signalr_worker(worker_tx, source_id, stop_rx),
        });
    }

    thread::spawn(move || {
        while let Ok(message) = rx.recv() {
            let source_id = match &message {
                TimingMessage::Status { source_id, .. }
                | TimingMessage::Error { source_id, .. }
                | TimingMessage::Snapshot { source_id, .. } => *source_id,
            };

            let Some(series) = source_to_series.get(&source_id).copied() else {
                continue;
            };

            state.apply_timing_message(series, &message);
            state.notify_series_update(series);
        }
    });

    FeedController { stop_txs }
}
