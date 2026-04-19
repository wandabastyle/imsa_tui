use std::{
    sync::mpsc::{Receiver, Sender},
    thread,
};

use crate::{
    adapters::{
        dhlm::websocket_worker_with_debug as dhlm_worker, f1::worker_with_debug as f1_worker,
        imsa::polling_worker_with_debug, nls::websocket_worker_with_debug as nls_worker,
        wec::websocket_worker_with_debug as wec_websocket_worker,
    },
    timing::{Series, TimingMessage},
    timing_persist::SeriesDebugOutput,
};

pub fn spawn_series_worker(
    series: Series,
    worker_tx: Sender<TimingMessage>,
    source_id: u64,
    stop_rx: Receiver<()>,
    debug_output: SeriesDebugOutput,
) {
    thread::spawn(move || match series {
        Series::Imsa => polling_worker_with_debug(worker_tx, source_id, stop_rx, debug_output),
        Series::Nls => nls_worker(worker_tx, source_id, stop_rx, debug_output),
        Series::Dhlm => dhlm_worker(worker_tx, source_id, stop_rx, debug_output),
        Series::F1 => f1_worker(worker_tx, source_id, stop_rx, debug_output),
        Series::Wec => wec_websocket_worker(worker_tx, source_id, stop_rx, debug_output),
    });
}
