// Worker bridge: starts/stops one worker per series on demand and folds updates into shared web state.

use std::{
    collections::HashMap,
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use crate::{
    f1::signalr_worker,
    imsa::polling_worker,
    nls::websocket_worker,
    timing::{Series, TimingMessage},
};

use super::state::WebAppState;

type WorkerSpawner = Arc<dyn Fn(Series, Sender<TimingMessage>, u64, Receiver<()>) + Send + Sync>;
type IdleTtlFn = Arc<dyn Fn(Series) -> Duration + Send + Sync>;

#[derive(Clone)]
pub struct FeedController {
    inner: Arc<FeedControllerInner>,
}

struct FeedControllerInner {
    state: WebAppState,
    worker_tx: Sender<TimingMessage>,
    runtimes: Mutex<HashMap<Series, SeriesRuntime>>,
    worker_spawner: WorkerSpawner,
    idle_ttl_for: IdleTtlFn,
}

#[derive(Debug, Clone, Default)]
struct SeriesRuntime {
    running: bool,
    active_clients: usize,
    idle_generation: u64,
    stop_tx: Option<Sender<()>>,
}

impl FeedController {
    pub fn register_client(&self, series: Series) {
        let mut guard = match self.inner.runtimes.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let runtime = guard.entry(series).or_default();
        runtime.active_clients = runtime.active_clients.saturating_add(1);
        runtime.idle_generation = runtime.idle_generation.saturating_add(1);

        if !runtime.running {
            let source_id = source_id_for(series);
            let (stop_tx, stop_rx) = mpsc::channel::<()>();
            (self.inner.worker_spawner)(series, self.inner.worker_tx.clone(), source_id, stop_rx);
            eprintln!(
                "{} feed worker started: first client connected.",
                series.label()
            );
            runtime.running = true;
            runtime.stop_tx = Some(stop_tx);
        }
    }

    pub fn unregister_client(&self, series: Series) {
        let (generation, should_schedule) = {
            let mut guard = match self.inner.runtimes.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            let runtime = guard.entry(series).or_default();
            if runtime.active_clients > 0 {
                runtime.active_clients -= 1;
            }
            if runtime.active_clients != 0 {
                (runtime.idle_generation, false)
            } else {
                runtime.idle_generation = runtime.idle_generation.saturating_add(1);
                (runtime.idle_generation, true)
            }
        };

        if !should_schedule {
            return;
        }

        let ttl = (self.inner.idle_ttl_for)(series);
        let controller = self.clone();
        thread::spawn(move || {
            thread::sleep(ttl);
            controller.stop_if_still_idle(series, generation);
        });
    }

    fn stop_if_still_idle(&self, series: Series, expected_generation: u64) {
        let stop_tx = {
            let mut guard = match self.inner.runtimes.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            let Some(runtime) = guard.get_mut(&series) else {
                return;
            };

            if runtime.active_clients != 0 || runtime.idle_generation != expected_generation {
                return;
            }
            if !runtime.running {
                return;
            }

            runtime.running = false;
            runtime.stop_tx.take()
        };

        if let Some(stop_tx) = stop_tx {
            let _ = stop_tx.send(());
            eprintln!(
                "{} feed worker stopped: idle timeout reached.",
                series.label()
            );
            self.inner.state.apply_timing_message(
                series,
                &TimingMessage::Status {
                    source_id: source_id_for(series),
                    text: "Idle (waiting for client)".to_string(),
                },
            );
            self.inner.state.notify_series_update(series);
        }
    }

    pub fn stop_all(&self) {
        let stop_txs = {
            let mut guard = match self.inner.runtimes.lock() {
                Ok(g) => g,
                Err(_) => return,
            };

            let mut txs = Vec::new();
            for runtime in guard.values_mut() {
                runtime.active_clients = 0;
                runtime.running = false;
                runtime.idle_generation = runtime.idle_generation.saturating_add(1);
                if let Some(stop_tx) = runtime.stop_tx.take() {
                    txs.push(stop_tx);
                }
            }
            txs
        };

        for stop_tx in stop_txs {
            let _ = stop_tx.send(());
        }
    }

    #[cfg(test)]
    fn with_runtime(
        state: WebAppState,
        worker_spawner: WorkerSpawner,
        idle_ttl_for: IdleTtlFn,
    ) -> Self {
        start_feed_bridge_internal(state, worker_spawner, idle_ttl_for)
    }
}

pub fn start_feed_bridge(state: WebAppState) -> FeedController {
    start_feed_bridge_internal(
        state,
        Arc::new(spawn_worker_thread),
        Arc::new(series_idle_ttl),
    )
}

fn start_feed_bridge_internal(
    state: WebAppState,
    worker_spawner: WorkerSpawner,
    idle_ttl_for: IdleTtlFn,
) -> FeedController {
    let (tx, rx) = mpsc::channel::<TimingMessage>();

    let source_to_series: HashMap<u64, Series> = Series::all()
        .into_iter()
        .map(|series| (source_id_for(series), series))
        .collect();
    let state_for_bridge = state.clone();
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

            state_for_bridge.apply_timing_message(series, &message);
            state_for_bridge.notify_series_update(series);
        }
    });

    FeedController {
        inner: Arc::new(FeedControllerInner {
            state,
            worker_tx: tx,
            runtimes: Mutex::new(HashMap::new()),
            worker_spawner,
            idle_ttl_for,
        }),
    }
}

fn source_id_for(series: Series) -> u64 {
    match series {
        Series::Imsa => 1,
        Series::Nls => 2,
        Series::F1 => 3,
    }
}

fn spawn_worker_thread(
    series: Series,
    worker_tx: Sender<TimingMessage>,
    source_id: u64,
    stop_rx: Receiver<()>,
) {
    thread::spawn(move || match series {
        Series::Imsa => polling_worker(worker_tx, source_id, stop_rx),
        Series::Nls => websocket_worker(worker_tx, source_id, stop_rx),
        Series::F1 => signalr_worker(worker_tx, source_id, stop_rx),
    });
}

fn series_idle_ttl(series: Series) -> Duration {
    match series {
        // IMSA polling reconnects quickly; keep the idle window short.
        Series::Imsa => Duration::from_secs(30),
        // NLS websocket reconnect is moderate; keep a bit more cushion.
        Series::Nls => Duration::from_secs(75),
        // F1 SignalR reconnect is heaviest; keep the longest idle window.
        Series::F1 => Duration::from_secs(120),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    fn short_ttl(_: Series) -> Duration {
        Duration::from_millis(120)
    }

    fn counting_spawner(
        starts: Arc<AtomicUsize>,
        stops: Arc<AtomicUsize>,
    ) -> impl Fn(Series, Sender<TimingMessage>, u64, Receiver<()>) + Send + Sync + 'static {
        move |_series, _tx, _source_id, stop_rx| {
            starts.fetch_add(1, Ordering::SeqCst);
            let stops = stops.clone();
            thread::spawn(move || {
                let _ = stop_rx.recv();
                stops.fetch_add(1, Ordering::SeqCst);
            });
        }
    }

    #[test]
    fn shared_worker_starts_once_per_series() {
        let starts = Arc::new(AtomicUsize::new(0));
        let stops = Arc::new(AtomicUsize::new(0));
        let spawner = counting_spawner(starts.clone(), stops.clone());
        let state = WebAppState::new();
        let controller =
            FeedController::with_runtime(state, Arc::new(spawner), Arc::new(short_ttl));

        controller.register_client(Series::Nls);
        controller.register_client(Series::Nls);

        assert_eq!(starts.load(Ordering::SeqCst), 1);
        controller.stop_all();
    }

    #[test]
    fn idle_timeout_stops_worker_after_last_disconnect() {
        let starts = Arc::new(AtomicUsize::new(0));
        let stops = Arc::new(AtomicUsize::new(0));
        let spawner = counting_spawner(starts.clone(), stops.clone());
        let state = WebAppState::new();
        let controller =
            FeedController::with_runtime(state, Arc::new(spawner), Arc::new(short_ttl));

        controller.register_client(Series::Imsa);
        controller.unregister_client(Series::Imsa);

        wait_for(|| stops.load(Ordering::SeqCst) == 1, Duration::from_secs(2));
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        controller.stop_all();
    }

    #[test]
    fn reconnect_before_ttl_cancels_pending_stop() {
        let starts = Arc::new(AtomicUsize::new(0));
        let stops = Arc::new(AtomicUsize::new(0));
        let spawner = counting_spawner(starts.clone(), stops.clone());
        let state = WebAppState::new();
        let controller =
            FeedController::with_runtime(state, Arc::new(spawner), Arc::new(short_ttl));

        controller.register_client(Series::F1);
        controller.unregister_client(Series::F1);
        thread::sleep(Duration::from_millis(50));
        controller.register_client(Series::F1);
        thread::sleep(Duration::from_millis(180));

        assert_eq!(stops.load(Ordering::SeqCst), 0);
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        controller.stop_all();
    }

    fn wait_for(condition: impl Fn() -> bool, timeout: Duration) {
        let start = Instant::now();
        while !condition() {
            if start.elapsed() >= timeout {
                panic!("condition not met before timeout");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}
