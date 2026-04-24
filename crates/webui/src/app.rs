use std::{
    cell::Cell,
    collections::{BTreeMap, HashMap, HashSet},
    rc::Rc,
};

use gloo_events::EventListener;
use gloo_net::http::Request;
use gloo_timers::future::TimeoutFuture;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_shared::{
    class_display_name, DemoStateResponse, LoginRequest, NlsLivetickerResponse, Preferences,
    Series, SeriesSnapshot, SessionStateResponse, SnapshotResponse, TimingClassColor, TimingEntry,
    TimingNotice,
};
use yew::prelude::*;

const ALL_SERIES: [Series; 5] = [
    Series::Imsa,
    Series::Nls,
    Series::F1,
    Series::Wec,
    Series::Dhlm,
];

#[derive(Clone, PartialEq)]
enum ViewMode {
    Overall,
    Grouped,
    Class(usize),
    Favourites,
}

#[derive(Clone, PartialEq)]
struct SearchState {
    query: String,
    current_match: usize,
    input_active: bool,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            query: String::new(),
            current_match: 0,
            input_active: false,
        }
    }
}

#[derive(Clone, PartialEq)]
struct AppState {
    snapshots: HashMap<Series, SeriesSnapshot>,
    active_series: Series,
    favourites: HashSet<String>,
    view_mode: ViewMode,
    selected_row: usize,
    gap_anchor_stable_id: Option<String>,
    show_help: bool,
    show_messages: bool,
    show_liveticker: bool,
    notices: Vec<TimingNotice>,
    nls_liveticker: NlsLivetickerResponse,
    search: SearchState,
    demo_enabled: bool,
    connection_errors: Vec<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            snapshots: HashMap::new(),
            active_series: Series::Imsa,
            favourites: HashSet::new(),
            view_mode: ViewMode::Overall,
            selected_row: 0,
            gap_anchor_stable_id: None,
            show_help: false,
            show_messages: false,
            show_liveticker: false,
            notices: Vec::new(),
            nls_liveticker: NlsLivetickerResponse::default(),
            search: SearchState::default(),
            demo_enabled: false,
            connection_errors: Vec::new(),
        }
    }
}

#[derive(Clone, PartialEq)]
struct GroupSection {
    name: String,
    entries: Vec<TimingEntry>,
    start: usize,
}

fn series_label(series: Series) -> &'static str {
    match series {
        Series::Imsa => "IMSA",
        Series::Nls => "NLS",
        Series::F1 => "F1",
        Series::Wec => "WEC",
        Series::Dhlm => "DHLM",
    }
}

fn series_from_index(idx: usize) -> Series {
    ALL_SERIES.get(idx).copied().unwrap_or(Series::Imsa)
}

fn series_index(series: Series) -> usize {
    ALL_SERIES
        .iter()
        .position(|candidate| *candidate == series)
        .unwrap_or(0)
}

fn normalize_stable_id(series: Series, stable_id: &str) -> String {
    match series {
        Series::Imsa => trim_legacy_class_suffix(stable_id, "fallback"),
        Series::Nls | Series::Dhlm => trim_legacy_class_suffix(stable_id, "stnr"),
        Series::F1 | Series::Wec => stable_id.to_string(),
    }
}

fn trim_legacy_class_suffix(stable_id: &str, expected_prefix: &str) -> String {
    if !stable_id.starts_with(&format!("{expected_prefix}:")) {
        return stable_id.to_string();
    }
    let parts: Vec<&str> = stable_id.split(':').collect();
    if parts.len() < 3 {
        return stable_id.to_string();
    }
    format!("{}:{}", parts[0], parts[1])
}

fn favourite_key(series: Series, stable_id: &str) -> String {
    format!(
        "{}|{}",
        series.as_key_prefix(),
        normalize_stable_id(series, stable_id)
    )
}

fn grouped_entries(entries: &[TimingEntry]) -> Vec<(String, Vec<TimingEntry>)> {
    let mut grouped = BTreeMap::<String, Vec<TimingEntry>>::new();
    for entry in entries {
        grouped
            .entry(class_display_name(&entry.class_name))
            .or_default()
            .push(entry.clone());
    }

    let mut groups: Vec<(String, Vec<TimingEntry>)> = grouped.into_iter().collect();
    for (_, group_entries) in &mut groups {
        group_entries.sort_by(|a, b| {
            let ar = a.class_rank.parse::<u32>().unwrap_or(u32::MAX);
            let br = b.class_rank.parse::<u32>().unwrap_or(u32::MAX);
            ar.cmp(&br).then_with(|| a.position.cmp(&b.position))
        });
    }

    groups.sort_by(|(an, ae), (bn, be)| {
        let a_best = ae.iter().map(|e| e.position).min().unwrap_or(u32::MAX);
        let b_best = be.iter().map(|e| e.position).min().unwrap_or(u32::MAX);
        a_best.cmp(&b_best).then_with(|| an.cmp(bn))
    });

    groups
}

fn next_view_mode(current: &ViewMode, groups_len: usize) -> ViewMode {
    if groups_len == 0 {
        return match current {
            ViewMode::Overall => ViewMode::Grouped,
            ViewMode::Grouped => ViewMode::Favourites,
            _ => ViewMode::Overall,
        };
    }
    match current {
        ViewMode::Overall => ViewMode::Grouped,
        ViewMode::Grouped => ViewMode::Class(0),
        ViewMode::Class(idx) => {
            if *idx + 1 < groups_len {
                ViewMode::Class(*idx + 1)
            } else {
                ViewMode::Favourites
            }
        }
        ViewMode::Favourites => ViewMode::Overall,
    }
}

fn view_mode_label(mode: &ViewMode, groups: &[(String, Vec<TimingEntry>)]) -> String {
    match mode {
        ViewMode::Overall => "Overall".to_string(),
        ViewMode::Grouped => "Grouped".to_string(),
        ViewMode::Class(index) => {
            let name = groups
                .get(*index)
                .map(|(name, _)| name.as_str())
                .unwrap_or("");
            format!("Class {name}")
        }
        ViewMode::Favourites => "Favourites".to_string(),
    }
}

fn entries_for_view(
    mode: &ViewMode,
    active_entries: &[TimingEntry],
    groups: &[(String, Vec<TimingEntry>)],
    favourites: &HashSet<String>,
    active_series: Series,
) -> Vec<TimingEntry> {
    match mode {
        ViewMode::Overall => active_entries.to_vec(),
        ViewMode::Grouped => groups
            .iter()
            .flat_map(|(_, list)| list.iter().cloned())
            .collect(),
        ViewMode::Class(idx) => groups
            .get(*idx)
            .map(|(_, list)| list.clone())
            .unwrap_or_default(),
        ViewMode::Favourites => active_entries
            .iter()
            .filter(|entry| favourites.contains(&favourite_key(active_series, &entry.stable_id)))
            .cloned()
            .collect(),
    }
}

fn entry_matches_search(entry: &TimingEntry, query: &str) -> bool {
    let needle = query.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return false;
    }
    entry.car_number.to_ascii_lowercase().contains(&needle)
        || entry.driver.to_ascii_lowercase().contains(&needle)
        || entry.vehicle.to_ascii_lowercase().contains(&needle)
        || entry.team.to_ascii_lowercase().contains(&needle)
}

async fn fetch_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    let response = Request::get(path)
        .send()
        .await
        .map_err(|err| format!("request failed: {err}"))?;
    if !response.ok() {
        return Err(format!("request failed ({})", response.status()));
    }
    response
        .json::<T>()
        .await
        .map_err(|err| format!("invalid response payload: {err}"))
}

async fn fetch_session_state() -> Result<bool, String> {
    let session = fetch_json::<SessionStateResponse>("/auth/session").await?;
    Ok(session.authenticated)
}

async fn login_with_access_code(access_code: String) -> Result<(), String> {
    let payload = LoginRequest { access_code };
    let response = Request::post("/auth/login")
        .header("content-type", "application/json")
        .body(
            serde_json::to_string(&payload)
                .map_err(|err| format!("encode payload failed: {err}"))?,
        )
        .map_err(|err| format!("build request failed: {err}"))?
        .send()
        .await
        .map_err(|err| format!("request failed: {err}"))?;
    if response.ok() {
        Ok(())
    } else {
        Err(format!("login failed ({})", response.status()))
    }
}

async fn logout() {
    let _ = Request::post("/auth/logout").send().await;
}

async fn fetch_preferences() -> Result<Preferences, String> {
    fetch_json::<Preferences>("/api/preferences").await
}

async fn update_preferences(preferences: &Preferences) -> Result<Preferences, String> {
    let response = Request::put("/api/preferences")
        .header("content-type", "application/json")
        .body(
            serde_json::to_string(preferences)
                .map_err(|err| format!("encode payload failed: {err}"))?,
        )
        .map_err(|err| format!("build request failed: {err}"))?
        .send()
        .await
        .map_err(|err| format!("request failed: {err}"))?;
    if !response.ok() {
        return Err(format!("preferences update failed ({})", response.status()));
    }
    response
        .json::<Preferences>()
        .await
        .map_err(|err| format!("invalid response payload: {err}"))
}

async fn fetch_demo_state() -> Result<DemoStateResponse, String> {
    fetch_json::<DemoStateResponse>("/api/demo").await
}

async fn update_demo_state(enabled: bool) -> Result<DemoStateResponse, String> {
    let response = Request::put("/api/demo")
        .header("content-type", "application/json")
        .body(
            serde_json::to_string(&web_shared::PutDemoRequest { enabled })
                .map_err(|err| format!("encode payload failed: {err}"))?,
        )
        .map_err(|err| format!("build request failed: {err}"))?
        .send()
        .await
        .map_err(|err| format!("request failed: {err}"))?;
    if !response.ok() {
        return Err(format!("demo update failed ({})", response.status()));
    }
    response
        .json::<DemoStateResponse>()
        .await
        .map_err(|err| format!("invalid response payload: {err}"))
}

async fn fetch_snapshot(series: Series) -> Result<SnapshotResponse, String> {
    fetch_json::<SnapshotResponse>(&format!("/api/snapshot/{}", series.as_key_prefix())).await
}

async fn fetch_nls_liveticker() -> Result<NlsLivetickerResponse, String> {
    fetch_json::<NlsLivetickerResponse>("/api/nls/liveticker").await
}

fn persist_preferences_from_state(state: &AppState) {
    let snapshot = state.clone();
    spawn_local(async move {
        let payload = Preferences {
            favourites: {
                let mut values: Vec<String> = snapshot.favourites.iter().cloned().collect();
                values.sort();
                values
            },
            selected_series: snapshot.active_series,
        };
        let _ = update_preferences(&payload).await;
    });
}

#[function_component(App)]
pub fn app() -> Html {
    let app = use_state(AppState::default);
    let show_series_picker = use_state(|| false);
    let series_picker_index = use_state(|| 0usize);
    let show_group_picker = use_state(|| false);
    let group_picker_index = use_state(|| 0usize);
    let loading = use_state(|| true);
    let load_error = use_state(String::new);
    let authenticated = use_state(|| false);
    let auth_checking = use_state(|| true);
    let login_code = use_state(String::new);
    let login_error = use_state(String::new);
    let latest_app = use_mut_ref(|| AppState::default());
    let latest_series_picker_open = use_mut_ref(|| false);
    let latest_series_picker_index = use_mut_ref(|| 0usize);
    let latest_group_picker_open = use_mut_ref(|| false);
    let latest_group_picker_index = use_mut_ref(|| 0usize);

    {
        let latest_app = latest_app.clone();
        let app = app.clone();
        use_effect_with((*app).clone(), move |state| {
            *latest_app.borrow_mut() = state.clone();
            || ()
        });
    }

    {
        let latest_series_picker_open = latest_series_picker_open.clone();
        let show_series_picker = show_series_picker.clone();
        use_effect_with(*show_series_picker, move |open| {
            *latest_series_picker_open.borrow_mut() = *open;
            || ()
        });
    }

    {
        let latest_series_picker_index = latest_series_picker_index.clone();
        let series_picker_index = series_picker_index.clone();
        use_effect_with(*series_picker_index, move |index| {
            *latest_series_picker_index.borrow_mut() = *index;
            || ()
        });
    }

    {
        let latest_group_picker_open = latest_group_picker_open.clone();
        let show_group_picker = show_group_picker.clone();
        use_effect_with(*show_group_picker, move |open| {
            *latest_group_picker_open.borrow_mut() = *open;
            || ()
        });
    }

    {
        let latest_group_picker_index = latest_group_picker_index.clone();
        let group_picker_index = group_picker_index.clone();
        use_effect_with(*group_picker_index, move |index| {
            *latest_group_picker_index.borrow_mut() = *index;
            || ()
        });
    }

    {
        let app = app.clone();
        let loading = loading.clone();
        let load_error = load_error.clone();
        let authenticated = authenticated.clone();
        let auth_checking = auth_checking.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                match fetch_session_state().await {
                    Ok(is_authenticated) => {
                        auth_checking.set(false);
                        if !is_authenticated {
                            authenticated.set(false);
                            loading.set(false);
                            return;
                        }

                        match initialize_app_state(app.clone()).await {
                            Ok(()) => {
                                authenticated.set(true);
                                loading.set(false);
                            }
                            Err(err) => {
                                load_error.set(err);
                                loading.set(false);
                            }
                        }
                    }
                    Err(err) => {
                        auth_checking.set(false);
                        load_error.set(err);
                        loading.set(false);
                    }
                }
            });
            || ()
        });
    }

    {
        let authenticated = authenticated.clone();
        let app = app.clone();
        let latest_app = latest_app.clone();
        let latest_series_picker_open = latest_series_picker_open.clone();
        let latest_group_picker_open = latest_group_picker_open.clone();
        use_effect_with(
            ((*authenticated), (*app).active_series),
            move |(is_authenticated, series)| {
                let mut event_source = None;
                let mut snapshot_listener = None;
                let mut error_listener = None;

                if *is_authenticated {
                    let stream_path = format!("/api/stream/{}", series.as_key_prefix());
                    match web_sys::EventSource::new(&stream_path) {
                        Ok(created) => {
                            let snapshot = EventListener::new(&created, "snapshot", {
                                let app = app.clone();
                                let latest_app = latest_app.clone();
                                let latest_series_picker_open = latest_series_picker_open.clone();
                                let latest_group_picker_open = latest_group_picker_open.clone();
                                move |event| {
                                    let Some(message) = event.dyn_ref::<web_sys::MessageEvent>()
                                    else {
                                        return;
                                    };
                                    let Some(payload) = message.data().as_string() else {
                                        return;
                                    };
                                    let Ok(snapshot) =
                                        serde_json::from_str::<SnapshotResponse>(&payload)
                                    else {
                                        return;
                                    };

                                    let mut next = (*latest_app.borrow()).clone();
                                    if *latest_series_picker_open.borrow()
                                        || *latest_group_picker_open.borrow()
                                    {
                                        return;
                                    }
                                    if snapshot.series == next.active_series {
                                        next.notices = snapshot.snapshot.notices.clone();
                                    }
                                    next.snapshots.insert(snapshot.series, snapshot.snapshot);
                                    app.set(next.clone());
                                    *latest_app.borrow_mut() = next;
                                }
                            });

                            let errors = EventListener::new(&created, "error", {
                                let app = app.clone();
                                let latest_app = latest_app.clone();
                                let target_series = *series;
                                let latest_series_picker_open = latest_series_picker_open.clone();
                                let latest_group_picker_open = latest_group_picker_open.clone();
                                move |_| {
                                    let mut next = (*latest_app.borrow()).clone();
                                    if *latest_series_picker_open.borrow()
                                        || *latest_group_picker_open.borrow()
                                    {
                                        return;
                                    }
                                    next.connection_errors.push(format!(
                                        "stream reconnect: {}",
                                        series_label(target_series)
                                    ));
                                    if next.connection_errors.len() > 5 {
                                        let start = next.connection_errors.len().saturating_sub(5);
                                        next.connection_errors =
                                            next.connection_errors[start..].to_vec();
                                    }
                                    app.set(next.clone());
                                    *latest_app.borrow_mut() = next;
                                }
                            });

                            event_source = Some(created);
                            snapshot_listener = Some(snapshot);
                            error_listener = Some(errors);
                        }
                        Err(err) => {
                            let mut next = (*latest_app.borrow()).clone();
                            next.connection_errors.push(format!(
                                "stream open failed: {} ({err:?})",
                                series_label(*series)
                            ));
                            if next.connection_errors.len() > 5 {
                                let start = next.connection_errors.len().saturating_sub(5);
                                next.connection_errors = next.connection_errors[start..].to_vec();
                            }
                            app.set(next.clone());
                            *latest_app.borrow_mut() = next;
                        }
                    }
                }

                move || {
                    drop(snapshot_listener);
                    drop(error_listener);
                    if let Some(source) = event_source {
                        source.close();
                    }
                }
            },
        );
    }

    {
        let authenticated = authenticated.clone();
        let app = app.clone();
        let latest_app = latest_app.clone();
        use_effect_with(
            (
                (*authenticated),
                (*app).active_series,
                (*app).show_liveticker,
            ),
            move |(is_authenticated, active_series, show_liveticker)| {
                let mut cancelled = None;
                if *is_authenticated && (*active_series == Series::Nls || *show_liveticker) {
                    cancelled = Some(Rc::new(Cell::new(false)));
                    let cancelled_for_task = cancelled.as_ref().expect("cancel flag").clone();
                    spawn_local(async move {
                        while !cancelled_for_task.get() {
                            if let Ok(response) = fetch_nls_liveticker().await {
                                let mut next = (*latest_app.borrow()).clone();
                                next.nls_liveticker = response;
                                app.set(next.clone());
                                *latest_app.borrow_mut() = next;
                            }
                            TimeoutFuture::new(2500).await;
                        }
                    });
                }
                move || {
                    if let Some(flag) = cancelled {
                        flag.set(true);
                    }
                }
            },
        );
    }

    {
        let app = app.clone();
        let authenticated = authenticated.clone();
        let show_series_picker = show_series_picker.clone();
        let series_picker_index = series_picker_index.clone();
        let show_group_picker = show_group_picker.clone();
        let group_picker_index = group_picker_index.clone();
        let latest_app = latest_app.clone();
        let latest_series_picker_open = latest_series_picker_open.clone();
        let latest_series_picker_index = latest_series_picker_index.clone();
        let latest_group_picker_open = latest_group_picker_open.clone();
        let latest_group_picker_index = latest_group_picker_index.clone();
        use_effect_with(*authenticated, move |is_authenticated| {
            let mut listener: Option<EventListener> = None;
            if *is_authenticated {
                listener = Some(EventListener::new(&gloo_utils::window(), "keydown", {
                    let app = app.clone();
                    let show_series_picker = show_series_picker.clone();
                    let series_picker_index = series_picker_index.clone();
                    let show_group_picker = show_group_picker.clone();
                    let group_picker_index = group_picker_index.clone();
                    let latest_app = latest_app.clone();
                    let latest_series_picker_open = latest_series_picker_open.clone();
                    let latest_series_picker_index = latest_series_picker_index.clone();
                    let latest_group_picker_open = latest_group_picker_open.clone();
                    let latest_group_picker_index = latest_group_picker_index.clone();
                    move |event| {
                        let Some(keyboard_event) = event.dyn_ref::<web_sys::KeyboardEvent>() else {
                            return;
                        };
                        if keyboard_event.repeat() {
                            match keyboard_event.key().as_str() {
                                "t" | "G" | "Enter" | "Escape" => {
                                    keyboard_event.prevent_default();
                                    return;
                                }
                                _ => {}
                            }
                        }
                        let mut next = (*latest_app.borrow()).clone();

                        if next.search.input_active {
                            match keyboard_event.key().as_str() {
                                "Escape" => {
                                    next.search.input_active = false;
                                    next.search.query.clear();
                                    next.search.current_match = 0;
                                    app.set(next.clone());
                                    *latest_app.borrow_mut() = next;
                                    keyboard_event.prevent_default();
                                    return;
                                }
                                "Enter" => {
                                    next.search.input_active = false;
                                    next.search.current_match = 0;
                                    let active_snapshot =
                                        next.snapshots.get(&next.active_series).cloned();
                                    let active_entries =
                                        active_snapshot.map(|s| s.entries).unwrap_or_default();
                                    let groups = grouped_entries(&active_entries);
                                    let view_entries = entries_for_view(
                                        &next.view_mode,
                                        &active_entries,
                                        &groups,
                                        &next.favourites,
                                        next.active_series,
                                    );
                                    let search_matches: Vec<usize> = view_entries
                                        .iter()
                                        .enumerate()
                                        .filter_map(|(idx, entry)| {
                                            entry_matches_search(entry, &next.search.query)
                                                .then_some(idx)
                                        })
                                        .collect();
                                    if let Some(first_match) = search_matches.first().copied() {
                                        next.selected_row = first_match;
                                    }
                                    app.set(next.clone());
                                    *latest_app.borrow_mut() = next;
                                    keyboard_event.prevent_default();
                                    return;
                                }
                                "Backspace" => {
                                    next.search.query.pop();
                                    app.set(next.clone());
                                    *latest_app.borrow_mut() = next;
                                    keyboard_event.prevent_default();
                                    return;
                                }
                                _ => {
                                    let key = keyboard_event.key();
                                    if key.chars().count() == 1
                                        && !keyboard_event.ctrl_key()
                                        && !keyboard_event.meta_key()
                                    {
                                        next.search.query.push_str(&key);
                                        app.set(next.clone());
                                        *latest_app.borrow_mut() = next;
                                        keyboard_event.prevent_default();
                                        return;
                                    }
                                }
                            }
                        }

                        if *latest_series_picker_open.borrow() {
                            match keyboard_event.key().as_str() {
                                "Escape" => {
                                    show_series_picker.set(false);
                                    *latest_series_picker_open.borrow_mut() = false;
                                    keyboard_event.prevent_default();
                                    return;
                                }
                                "ArrowDown" | "j" => {
                                    let current = *latest_series_picker_index.borrow();
                                    let next_index = (current + 1) % ALL_SERIES.len();
                                    series_picker_index.set(next_index);
                                    *latest_series_picker_index.borrow_mut() = next_index;
                                    keyboard_event.prevent_default();
                                    return;
                                }
                                "ArrowUp" | "k" => {
                                    let current = *latest_series_picker_index.borrow();
                                    let next_index = if current == 0 {
                                        ALL_SERIES.len() - 1
                                    } else {
                                        current - 1
                                    };
                                    series_picker_index.set(next_index);
                                    *latest_series_picker_index.borrow_mut() = next_index;
                                    keyboard_event.prevent_default();
                                    return;
                                }
                                "Enter" => {
                                    next.active_series =
                                        series_from_index(*latest_series_picker_index.borrow());
                                    next.notices = next
                                        .snapshots
                                        .get(&next.active_series)
                                        .map(|snapshot| snapshot.notices.clone())
                                        .unwrap_or_default();
                                    next.view_mode = ViewMode::Overall;
                                    next.selected_row = 0;
                                    next.gap_anchor_stable_id = None;
                                    app.set(next.clone());
                                    *latest_app.borrow_mut() = next.clone();
                                    show_series_picker.set(false);
                                    *latest_series_picker_open.borrow_mut() = false;
                                    show_group_picker.set(false);
                                    *latest_group_picker_open.borrow_mut() = false;
                                    persist_preferences_from_state(&next);
                                    keyboard_event.prevent_default();
                                    return;
                                }
                                _ => {
                                    keyboard_event.prevent_default();
                                    return;
                                }
                            }
                        }

                        if *latest_group_picker_open.borrow() {
                            let preview_snapshot = next.snapshots.get(&next.active_series).cloned();
                            let preview_entries =
                                preview_snapshot.map(|s| s.entries).unwrap_or_default();
                            let groups = grouped_entries(&preview_entries);
                            match keyboard_event.key().as_str() {
                                "Escape" => {
                                    show_group_picker.set(false);
                                    *latest_group_picker_open.borrow_mut() = false;
                                    keyboard_event.prevent_default();
                                    return;
                                }
                                "ArrowDown" | "j" => {
                                    let current = *latest_group_picker_index.borrow();
                                    let next_index = if groups.is_empty() {
                                        0
                                    } else {
                                        (current + 1) % groups.len()
                                    };
                                    group_picker_index.set(next_index);
                                    *latest_group_picker_index.borrow_mut() = next_index;
                                    keyboard_event.prevent_default();
                                    return;
                                }
                                "ArrowUp" | "k" => {
                                    let current = *latest_group_picker_index.borrow();
                                    let next_index = if groups.is_empty() {
                                        0
                                    } else if current == 0 {
                                        groups.len() - 1
                                    } else {
                                        current - 1
                                    };
                                    group_picker_index.set(next_index);
                                    *latest_group_picker_index.borrow_mut() = next_index;
                                    keyboard_event.prevent_default();
                                    return;
                                }
                                "Enter" => {
                                    if !groups.is_empty() {
                                        let bounded = (*latest_group_picker_index.borrow())
                                            .min(groups.len() - 1);
                                        next.view_mode = ViewMode::Class(bounded);
                                        next.selected_row = 0;
                                        next.gap_anchor_stable_id = None;
                                        group_picker_index.set(bounded);
                                        *latest_group_picker_index.borrow_mut() = bounded;
                                    }
                                    app.set(next.clone());
                                    *latest_app.borrow_mut() = next;
                                    show_group_picker.set(false);
                                    *latest_group_picker_open.borrow_mut() = false;
                                    keyboard_event.prevent_default();
                                    return;
                                }
                                _ => {
                                    keyboard_event.prevent_default();
                                    return;
                                }
                            }
                        }

                        let active_snapshot = next.snapshots.get(&next.active_series).cloned();
                        let active_entries = active_snapshot.map(|s| s.entries).unwrap_or_default();
                        let groups = grouped_entries(&active_entries);
                        let view_entries = entries_for_view(
                            &next.view_mode,
                            &active_entries,
                            &groups,
                            &next.favourites,
                            next.active_series,
                        );
                        let search_matches: Vec<usize> = view_entries
                            .iter()
                            .enumerate()
                            .filter_map(|(idx, entry)| {
                                entry_matches_search(entry, &next.search.query).then_some(idx)
                            })
                            .collect();

                        match keyboard_event.key().as_str() {
                            "Escape" => {
                                if next.show_help {
                                    next.show_help = false;
                                } else if next.show_messages {
                                    next.show_messages = false;
                                } else if next.show_liveticker {
                                    next.show_liveticker = false;
                                } else if !next.search.query.is_empty() {
                                    next.search.query.clear();
                                    next.search.current_match = 0;
                                }
                            }
                            "h" | "?" => {
                                next.show_help = !next.show_help;
                            }
                            "m" => {
                                next.show_liveticker = false;
                                next.show_messages = !next.show_messages;
                            }
                            "l" => {
                                if next.active_series == Series::Nls {
                                    next.show_messages = false;
                                    next.show_liveticker = !next.show_liveticker;
                                }
                            }
                            "g" => {
                                next.view_mode = next_view_mode(&next.view_mode, groups.len());
                                next.selected_row = 0;
                                next.gap_anchor_stable_id = None;
                            }
                            "G" => {
                                show_group_picker.set(true);
                                *latest_group_picker_open.borrow_mut() = true;
                                group_picker_index.set(0);
                                *latest_group_picker_index.borrow_mut() = 0;
                            }
                            "o" => {
                                next.view_mode = ViewMode::Overall;
                                next.selected_row = 0;
                                next.gap_anchor_stable_id = None;
                            }
                            "t" => {
                                show_series_picker.set(true);
                                *latest_series_picker_open.borrow_mut() = true;
                                let active_index = series_index(next.active_series);
                                series_picker_index.set(active_index);
                                *latest_series_picker_index.borrow_mut() = active_index;
                                show_group_picker.set(false);
                                *latest_group_picker_open.borrow_mut() = false;
                            }
                            "ArrowDown" | "j" => {
                                let max = view_entries.len().saturating_sub(1);
                                next.selected_row = (next.selected_row + 1).min(max);
                            }
                            "ArrowUp" | "k" => {
                                next.selected_row = next.selected_row.saturating_sub(1);
                            }
                            "PageDown" => {
                                let max = view_entries.len().saturating_sub(1);
                                next.selected_row = (next.selected_row + 10).min(max);
                            }
                            "PageUp" => {
                                next.selected_row = next.selected_row.saturating_sub(10);
                            }
                            "Home" => {
                                next.selected_row = 0;
                            }
                            "End" => {
                                next.selected_row = view_entries.len().saturating_sub(1);
                            }
                            " " => {
                                if let Some(selected) = view_entries.get(next.selected_row) {
                                    let key =
                                        favourite_key(next.active_series, &selected.stable_id);
                                    if next.favourites.contains(&key) {
                                        next.favourites.remove(&key);
                                    } else {
                                        next.favourites.insert(key);
                                    }
                                    app.set(next.clone());
                                    *latest_app.borrow_mut() = next.clone();
                                    persist_preferences_from_state(&next);
                                    keyboard_event.prevent_default();
                                    return;
                                }
                            }
                            "f" => {
                                if !view_entries.is_empty() {
                                    let start = next.selected_row;
                                    for offset in 1..=view_entries.len() {
                                        let idx = (start + offset) % view_entries.len();
                                        let key = favourite_key(
                                            next.active_series,
                                            &view_entries[idx].stable_id,
                                        );
                                        if next.favourites.contains(&key) {
                                            next.selected_row = idx;
                                            next.gap_anchor_stable_id =
                                                Some(view_entries[idx].stable_id.clone());
                                            break;
                                        }
                                    }
                                }
                            }
                            "s" => {
                                next.search = SearchState {
                                    query: String::new(),
                                    current_match: 0,
                                    input_active: true,
                                };
                            }
                            "n" => {
                                if !search_matches.is_empty() {
                                    let start =
                                        next.search.current_match.min(search_matches.len() - 1);
                                    let next_match = (start + 1) % search_matches.len();
                                    next.search.current_match = next_match;
                                    next.selected_row = search_matches[next_match];
                                }
                            }
                            "p" => {
                                if !search_matches.is_empty() {
                                    let start =
                                        next.search.current_match.min(search_matches.len() - 1);
                                    let next_match =
                                        (start + search_matches.len() - 1) % search_matches.len();
                                    next.search.current_match = next_match;
                                    next.selected_row = search_matches[next_match];
                                }
                            }
                            "d" => {
                                let app_handle = app.clone();
                                let next_enabled = !next.demo_enabled;
                                spawn_local(async move {
                                    if let Ok(updated) = update_demo_state(next_enabled).await {
                                        let mut state = (*app_handle).clone();
                                        state.demo_enabled = updated.enabled;
                                        app_handle.set(state);
                                    }
                                });
                            }
                            "c" => {
                                if next.show_messages {
                                    next.notices.clear();
                                    if let Some(snapshot) =
                                        next.snapshots.get_mut(&next.active_series)
                                    {
                                        snapshot.notices.clear();
                                    }
                                }
                            }
                            _ => return,
                        }

                        app.set(next.clone());
                        *latest_app.borrow_mut() = next;
                        keyboard_event.prevent_default();
                    }
                }));
            }

            move || drop(listener)
        });
    }

    let active_snapshot = (*app).snapshots.get(&(*app).active_series).cloned();
    let active_entries = active_snapshot
        .as_ref()
        .map(|snapshot| snapshot.entries.clone())
        .unwrap_or_default();
    let groups = grouped_entries(&active_entries);
    let grouped_sections = {
        let mut start = 0usize;
        groups
            .iter()
            .map(|(name, entries)| {
                let section = GroupSection {
                    name: name.clone(),
                    entries: entries.clone(),
                    start,
                };
                start += entries.len();
                section
            })
            .collect::<Vec<_>>()
    };

    let view_entries = entries_for_view(
        &(*app).view_mode,
        &active_entries,
        &groups,
        &(*app).favourites,
        (*app).active_series,
    );
    let search_matches: Vec<usize> = view_entries
        .iter()
        .enumerate()
        .filter_map(|(idx, entry)| entry_matches_search(entry, &(*app).search.query).then_some(idx))
        .collect();
    let search_current_match = if search_matches.is_empty() {
        0
    } else {
        (*app).search.current_match.min(search_matches.len() - 1)
    };
    let marked_stable_id = search_matches
        .get(search_current_match)
        .and_then(|idx| view_entries.get(*idx))
        .map(|entry| entry.stable_id.clone());

    let view_mode_text = view_mode_label(&(*app).view_mode, &groups);
    let fav_count_for_series = (*app)
        .favourites
        .iter()
        .filter(|value| value.starts_with(&format!("{}|", (*app).active_series.as_key_prefix())))
        .count();

    let submit_login = {
        let login_code = login_code.clone();
        let login_error = login_error.clone();
        let authenticated = authenticated.clone();
        let loading = loading.clone();
        let app = app.clone();
        Callback::from(move |_| {
            login_error.set(String::new());
            loading.set(true);
            let login_code_value = (*login_code).clone();
            let login_error = login_error.clone();
            let authenticated = authenticated.clone();
            let loading = loading.clone();
            let app = app.clone();
            spawn_local(async move {
                match login_with_access_code(login_code_value).await {
                    Ok(()) => match initialize_app_state(app.clone()).await {
                        Ok(()) => {
                            authenticated.set(true);
                            loading.set(false);
                        }
                        Err(err) => {
                            login_error.set(err);
                            loading.set(false);
                        }
                    },
                    Err(err) => {
                        login_error.set(err);
                        loading.set(false);
                    }
                }
            });
        })
    };

    let on_login_input = {
        let login_code = login_code.clone();
        Callback::from(move |event: InputEvent| {
            let input: web_sys::HtmlInputElement = event.target_unchecked_into();
            login_code.set(input.value());
        })
    };

    let on_logout = {
        let authenticated = authenticated.clone();
        let app = app.clone();
        Callback::from(move |_| {
            let authenticated = authenticated.clone();
            let app = app.clone();
            spawn_local(async move {
                logout().await;
                authenticated.set(false);
                app.set(AppState::default());
            });
        })
    };

    html! {
        <main>
            <style>{include_str!("./styles.css")}</style>
            {
                if *auth_checking {
                    html! { <p>{"Checking access..."}</p> }
                } else if !*authenticated {
                    html! {
                        <section class="login-wrap">
                            <div class="login-card">
                                <h1>{"Live Timing Access"}</h1>
                                <p>{"Enter the shared access code to open the timing dashboard."}</p>
                                <form onsubmit={Callback::from(move |event: SubmitEvent| { event.prevent_default(); submit_login.emit(()); })}>
                                    <input
                                        placeholder="Access code"
                                        value={(*login_code).clone()}
                                        oninput={on_login_input}
                                        type="password"
                                        autocomplete="current-password"
                                    />
                                    <button type="submit">{"Enter"}</button>
                                </form>
                                if !(*login_error).is_empty() {
                                    <p class="login-error">{(*login_error).clone()}</p>
                                }
                            </div>
                        </section>
                    }
                } else if *loading {
                    html! { <p>{"Loading web UI..."}</p> }
                } else if !(*load_error).is_empty() {
                    html! { <p>{format!("Failed to initialize: {}", (*load_error).as_str())}</p> }
                } else {
                    html! {
                        <>
                            <div class="header-row">
                                <HeaderBar
                                    series={(*app).active_series}
                                    snapshot={active_snapshot.clone()}
                                    view_mode_label={view_mode_text.clone()}
                                    fav_count={fav_count_for_series}
                                    search_query={(*app).search.query.clone()}
                                    search_active={(*app).search.input_active}
                                    search_current={if search_matches.is_empty() { 0 } else { search_current_match + 1 }}
                                    search_total={search_matches.len()}
                                    notices_count={(*app).notices.len()}
                                    liveticker_count={(*app).nls_liveticker.entries.len()}
                                    demo_enabled={(*app).demo_enabled}
                                    error_text={active_snapshot.as_ref().and_then(|snapshot| snapshot.last_error.clone()).unwrap_or_default()}
                                />
                                <button class="logout-btn" onclick={on_logout}>{"Logout"}</button>
                            </div>

                            <TimingTable
                                title={view_mode_text}
                                series={(*app).active_series}
                                entries={view_entries}
                                class_colors={active_snapshot.as_ref().map(|snapshot| snapshot.header.class_colors.clone()).unwrap_or_default()}
                                grouped_sections={grouped_sections}
                                is_grouped_mode={matches!((*app).view_mode, ViewMode::Grouped)}
                                selected_row={(*app).selected_row}
                                marked_stable_id={marked_stable_id}
                                favourites={(*app).favourites.clone()}
                                gap_anchor_stable_id={(*app).gap_anchor_stable_id.clone()}
                            />

                            <HelpModal open={(*app).show_help} />
                            <GroupModal
                                open={*show_group_picker}
                                groups={groups.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>()}
                                selected_index={*group_picker_index}
                            />
                            <SeriesModal
                                open={*show_series_picker}
                                selected_series={series_from_index(*series_picker_index)}
                            />
                            <MessagesModal open={(*app).show_messages} notices={(*app).notices.clone()} />
                            <NlsLivetickerModal
                                open={(*app).show_liveticker}
                                entries={(*app).nls_liveticker.entries.clone()}
                                last_error={(*app).nls_liveticker.last_error.clone().unwrap_or_default()}
                            />
                        </>
                    }
                }
            }
        </main>
    }
}

async fn initialize_app_state(app: UseStateHandle<AppState>) -> Result<(), String> {
    let preferences = fetch_preferences().await?;
    let demo = fetch_demo_state().await?;
    let mut snapshots = HashMap::new();
    for series in ALL_SERIES {
        if let Ok(snapshot) = fetch_snapshot(series).await {
            snapshots.insert(snapshot.series, snapshot.snapshot);
        }
    }

    let notices = snapshots
        .get(&preferences.selected_series)
        .map(|snapshot| snapshot.notices.clone())
        .unwrap_or_default();

    app.set(AppState {
        snapshots,
        active_series: preferences.selected_series,
        favourites: preferences.favourites.into_iter().collect(),
        notices,
        demo_enabled: demo.enabled,
        ..AppState::default()
    });
    Ok(())
}

#[derive(Properties, PartialEq)]
struct HeaderBarProps {
    series: Series,
    snapshot: Option<SeriesSnapshot>,
    view_mode_label: String,
    fav_count: usize,
    search_query: String,
    search_active: bool,
    search_current: usize,
    search_total: usize,
    notices_count: usize,
    liveticker_count: usize,
    demo_enabled: bool,
    error_text: String,
}

#[function_component(HeaderBar)]
fn header_bar(props: &HeaderBarProps) -> Html {
    let age_text = props
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.last_update_unix_ms)
        .map(|ts| {
            let now = js_sys::Date::now() as u64;
            format!("Upd {}s", now.saturating_sub(ts) / 1000)
        })
        .unwrap_or_else(|| "Upd -".to_string());
    let snapshot = props.snapshot.as_ref();
    let status = snapshot
        .map(|snapshot| snapshot.status.clone())
        .unwrap_or_else(|| "Starting live timing...".to_string());
    let event_name = snapshot
        .map(|snapshot| snapshot.header.event_name.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "-".to_string());
    let session_name = snapshot
        .map(|snapshot| snapshot.header.session_name.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "-".to_string());
    let time_to_go = snapshot
        .map(|snapshot| snapshot.header.time_to_go.clone())
        .unwrap_or_else(|| "-".to_string());
    let flag = snapshot
        .map(|snapshot| snapshot.header.flag.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "-".to_string());

    html! {
        <section class="header" data-flag={flag.to_ascii_lowercase()}>
            <div class="line">
                {format!(
                    "{} | {} | {} | {} | TTE {} | Mode {} | {} | {} | Favs {}",
                    status,
                    event_name,
                    session_name,
                    series_label(props.series),
                    time_to_go,
                    props.view_mode_label,
                    flag,
                    age_text,
                    props.fav_count,
                )}
            </div>
            <div class="line dim">
                {format!(
                    "Keys: h help | m messages ({}){} | d demo",
                    props.notices_count,
                    if props.series == Series::Nls {
                        format!(" | l ticker ({})", props.liveticker_count)
                    } else {
                        String::new()
                    }
                )}
                if props.search_active || !props.search_query.trim().is_empty() {
                    <>{" | "}<span class={classes!("search-label", if props.search_active { Some("active") } else { None })}>{"Search:"}</span>{format!(" {} ({}/{})", if props.search_query.is_empty() { "-".to_string() } else { props.search_query.clone() }, props.search_current, props.search_total)}</>
                }
                if props.demo_enabled {
                    {" | DEMO"}
                }
                if !props.error_text.is_empty() {
                    {format!(" | Error: {}", props.error_text)}
                }
            </div>
        </section>
    }
}

#[derive(Properties, PartialEq)]
struct TimingTableProps {
    title: String,
    series: Series,
    entries: Vec<TimingEntry>,
    grouped_sections: Vec<GroupSection>,
    class_colors: BTreeMap<String, TimingClassColor>,
    is_grouped_mode: bool,
    selected_row: usize,
    marked_stable_id: Option<String>,
    favourites: HashSet<String>,
    gap_anchor_stable_id: Option<String>,
}

fn columns_for_series(series: Series) -> &'static [&'static str] {
    match series {
        Series::Imsa => &[
            "Pos",
            "#",
            "Class",
            "PIC",
            "Driver",
            "Vehicle",
            "Laps",
            "Gap O",
            "Gap C",
            "Next C",
            "Last",
            "Best",
            "BL#",
            "Pit",
            "Stop",
            "Fastest Driver",
        ],
        Series::Nls | Series::Dhlm => &[
            "Pos", "#", "Class", "PIC", "Driver", "Vehicle", "Team", "Laps", "Gap", "Last", "Best",
            "S1", "S2", "S3", "S4", "S5",
        ],
        Series::F1 => &[
            "Pos", "#", "Driver", "Team", "Laps", "Gap", "Int", "Last", "Best", "Pit", "Stops",
            "PIC",
        ],
        Series::Wec => &[
            "Pos", "#", "Class", "PIC", "Driver", "Vehicle", "Team", "Laps", "Gap", "Last", "Best",
            "S1", "S2", "S3",
        ],
    }
}

fn row_cells(series: Series, entry: &TimingEntry, favourites: &HashSet<String>) -> Vec<String> {
    let car_cell = if favourites.contains(&favourite_key(series, &entry.stable_id)) {
        format!("★ {}", entry.car_number)
    } else {
        entry.car_number.clone()
    };
    match series {
        Series::Imsa => vec![
            entry.position.to_string(),
            car_cell,
            entry.class_name.clone(),
            entry.class_rank.clone(),
            entry.driver.clone(),
            entry.vehicle.clone(),
            entry.laps.clone(),
            entry.gap_overall.clone(),
            entry.gap_class.clone(),
            entry.gap_next_in_class.clone(),
            entry.last_lap.clone(),
            entry.best_lap.clone(),
            entry.best_lap_no.clone(),
            entry.pit.clone(),
            entry.pit_stops.clone(),
            entry.fastest_driver.clone(),
        ],
        Series::Nls | Series::Dhlm => vec![
            entry.position.to_string(),
            car_cell,
            entry.class_name.clone(),
            entry.class_rank.clone(),
            entry.driver.clone(),
            entry.vehicle.clone(),
            entry.team.clone(),
            entry.laps.clone(),
            entry.gap_overall.clone(),
            entry.last_lap.clone(),
            entry.best_lap.clone(),
            entry.sector_1.clone(),
            entry.sector_2.clone(),
            entry.sector_3.clone(),
            entry.sector_4.clone(),
            entry.sector_5.clone(),
        ],
        Series::F1 => vec![
            entry.position.to_string(),
            car_cell,
            entry.driver.clone(),
            entry.team.clone(),
            entry.laps.clone(),
            entry.gap_overall.clone(),
            entry.gap_class.clone(),
            entry.last_lap.clone(),
            entry.best_lap.clone(),
            entry.pit.clone(),
            entry.pit_stops.clone(),
            entry.class_rank.clone(),
        ],
        Series::Wec => vec![
            entry.position.to_string(),
            car_cell,
            entry.class_name.clone(),
            entry.class_rank.clone(),
            entry.driver.clone(),
            entry.vehicle.clone(),
            entry.team.clone(),
            entry.laps.clone(),
            entry.gap_overall.clone(),
            entry.last_lap.clone(),
            entry.best_lap.clone(),
            entry.sector_1.clone(),
            entry.sector_2.clone(),
            entry.sector_3.clone(),
        ],
    }
}

fn resolve_class_color(
    series: Series,
    class_name: &str,
    colors: &BTreeMap<String, TimingClassColor>,
) -> Option<String> {
    if matches!(series, Series::Nls | Series::Dhlm) {
        return None;
    }

    let canonical = class_display_name(class_name);
    if let Some((_, live)) = colors
        .iter()
        .find(|(key, _)| class_display_name(key) == canonical)
    {
        if !live.color.trim().is_empty() {
            return Some(live.color.trim().to_string());
        }
    }

    let fallback = match series {
        Series::Wec => match canonical.as_str() {
            "HYPER" | "HYPERCAR" | "LMH" => Some("#e21e19"),
            "LMGT3" => Some("#0b9314"),
            "LMP1" => Some("#ff1053"),
            "LMP2" => Some("#3f90da"),
            "LMGTE" => Some("#ffa912"),
            "INV" => Some("#ffffff"),
            _ => None,
        },
        Series::Imsa | Series::F1 => match canonical.as_str() {
            "GTP" => Some("#ffffff"),
            "LMP2" => Some("#3f90da"),
            "GTD-PRO" => Some("#d22630"),
            "PRO" => Some("#e67e22"),
            "PRO-AM" => Some("#4caf50"),
            "MASTERS" => Some("#f1d302"),
            "GTD" => Some("#00a651"),
            "LMH" => Some("#dc143c"),
            "LMGT3" => Some("#1e90ff"),
            _ => None,
        },
        Series::Nls | Series::Dhlm => None,
    };

    fallback.map(str::to_string)
}

#[function_component(TimingTable)]
fn timing_table(props: &TimingTableProps) -> Html {
    let columns = columns_for_series(props.series);
    let class_column = columns.iter().position(|column| *column == "Class");
    html! {
        <section class="table-wrap">
            <div class="table-title">{props.title.clone()}</div>
            <div class="table-scroll">
                {
                    if props.is_grouped_mode {
                        html! {
                            <div class="group-stack">
                                {
                                    for props.grouped_sections.iter().map(|section| {
                                        html! {
                                            <section class="group-section">
                                                <div class="group-title">{format!("{} ({} cars)", section.name, section.entries.len())}</div>
                                                <table>
                                                    <thead>
                                                        <tr>{ for columns.iter().map(|column| html! { <th>{*column}</th> }) }</tr>
                                                    </thead>
                                                    <tbody>
                                                        {
                                                            for section.entries.iter().enumerate().map(|(index, entry)| {
                                                                let selected = section.start + index == props.selected_row;
                                                                let marked = props
                                                                    .marked_stable_id
                                                                    .as_ref()
                                                                    .map(|value| value == &entry.stable_id)
                                                                    .unwrap_or(false);
                                                                let row_classes = classes!(
                                                                    if selected { Some("selected") } else { None },
                                                                    if marked { Some("search-mark") } else { None },
                                                                );
                                                                let class_color = resolve_class_color(
                                                                    props.series,
                                                                    &entry.class_name,
                                                                    &props.class_colors,
                                                                );
                                                                html! {
                                                                    <tr class={row_classes}>
                                                                        {
                                                                            for row_cells(props.series, entry, &props.favourites)
                                                                                .into_iter()
                                                                                .enumerate()
                                                                                .map(|(cell_index, cell)| {
                                                                                    if class_column == Some(cell_index) {
                                                                                        if let Some(color) = class_color.clone() {
                                                                                            html! { <td style={format!("color: {}; font-weight: 700;", color)}>{cell}</td> }
                                                                                        } else {
                                                                                            html! { <td>{cell}</td> }
                                                                                        }
                                                                                    } else {
                                                                                        html! { <td>{cell}</td> }
                                                                                    }
                                                                                })
                                                                        }
                                                                    </tr>
                                                                }
                                                            })
                                                        }
                                                    </tbody>
                                                </table>
                                            </section>
                                        }
                                    })
                                }
                            </div>
                        }
                    } else {
                        html! {
                            <table>
                                <thead>
                                    <tr>{ for columns.iter().map(|column| html! { <th>{*column}</th> }) }</tr>
                                </thead>
                                <tbody>
                                    {
                                        if props.entries.is_empty() {
                                            html! { <tr><td colspan={columns.len().to_string()}>{"No timing data yet."}</td></tr> }
                                        } else {
                                            html! {
                                                for props.entries.iter().enumerate().map(|(index, entry)| {
                                                    let selected = index == props.selected_row;
                                                    let marked = props
                                                        .marked_stable_id
                                                        .as_ref()
                                                        .map(|value| value == &entry.stable_id)
                                                        .unwrap_or(false);
                                                    let row_classes = classes!(
                                                        if selected { Some("selected") } else { None },
                                                        if marked { Some("search-mark") } else { None },
                                                    );
                                                    let class_color = resolve_class_color(
                                                        props.series,
                                                        &entry.class_name,
                                                        &props.class_colors,
                                                    );
                                                    html! {
                                                        <tr class={row_classes}>
                                                            {
                                                                for row_cells(props.series, entry, &props.favourites)
                                                                    .into_iter()
                                                                    .enumerate()
                                                                    .map(|(cell_index, cell)| {
                                                                        if class_column == Some(cell_index) {
                                                                            if let Some(color) = class_color.clone() {
                                                                                html! { <td style={format!("color: {}; font-weight: 700;", color)}>{cell}</td> }
                                                                            } else {
                                                                                html! { <td>{cell}</td> }
                                                                            }
                                                                        } else {
                                                                            html! { <td>{cell}</td> }
                                                                        }
                                                                    })
                                                            }
                                                        </tr>
                                                    }
                                                })
                                            }
                                        }
                                    }
                                </tbody>
                            </table>
                        }
                    }
                }
            </div>
        </section>
    }
}

#[derive(Properties, PartialEq)]
struct HelpModalProps {
    open: bool,
}

#[function_component(HelpModal)]
fn help_modal(props: &HelpModalProps) -> Html {
    if !props.open {
        return html! {};
    }
    html! {
        <div class="backdrop">
            <section class="modal">
                <h2>{"Keyboard Help"}</h2>
                <pre>{"h toggle help (? also works)\nm open/close messages (c clear while open)\nl open/close NLS ticker\ng cycle views\nG open group picker\no overall view\nt series picker\narrows/j/k move\nPgUp/PgDn fast scroll\nspace toggle favourite\nf jump favourite\ns search mode (type, Enter apply, Esc clear)\nn/p next/prev match\nd toggle demo/live data source\nEsc close popup"}</pre>
            </section>
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct GroupModalProps {
    open: bool,
    groups: Vec<String>,
    selected_index: usize,
}

#[function_component(GroupModal)]
fn group_modal(props: &GroupModalProps) -> Html {
    let selected_button_ref = use_node_ref();
    {
        let selected_button_ref = selected_button_ref.clone();
        use_effect_with(
            (props.open, props.selected_index, props.groups.len()),
            move |_| {
                if let Some(element) = selected_button_ref.cast::<web_sys::Element>() {
                    let options = web_sys::ScrollIntoViewOptions::new();
                    options.set_block(web_sys::ScrollLogicalPosition::Nearest);
                    options.set_inline(web_sys::ScrollLogicalPosition::Nearest);
                    element.scroll_into_view_with_scroll_into_view_options(&options);
                }
                || ()
            },
        );
    }

    if !props.open {
        return html! {};
    }
    html! {
        <div class="backdrop">
            <section class="modal">
                <h2>{"Select Group"}</h2>
                if props.groups.is_empty() {
                    <p class="empty">{"No groups available for current series."}</p>
                } else {
                    <div class="list">
                        {
                            for props.groups.iter().enumerate().map(|(idx, group)| {
                                let button_classes = classes!(if idx == props.selected_index { Some("selected") } else { None });
                                let button_ref = if idx == props.selected_index {
                                    selected_button_ref.clone()
                                } else {
                                    NodeRef::default()
                                };
                                html! {
                                    <button class={button_classes} ref={button_ref}>{group.clone()}</button>
                                }
                            })
                        }
                    </div>
                }
            </section>
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct SeriesModalProps {
    open: bool,
    selected_series: Series,
}

#[function_component(SeriesModal)]
fn series_modal(props: &SeriesModalProps) -> Html {
    let selected_button_ref = use_node_ref();
    {
        let selected_button_ref = selected_button_ref.clone();
        use_effect_with((props.open, props.selected_series), move |_| {
            if let Some(element) = selected_button_ref.cast::<web_sys::Element>() {
                let options = web_sys::ScrollIntoViewOptions::new();
                options.set_block(web_sys::ScrollLogicalPosition::Nearest);
                options.set_inline(web_sys::ScrollLogicalPosition::Nearest);
                element.scroll_into_view_with_scroll_into_view_options(&options);
            }
            || ()
        });
    }

    if !props.open {
        return html! {};
    }
    html! {
        <div class="backdrop">
            <section class="modal">
                <h2>{"Select Series"}</h2>
                <div class="list">
                    {
                        for ALL_SERIES.iter().map(|series| {
                            let selected = *series == props.selected_series;
                            let button_classes = classes!(if selected { Some("selected") } else { None });
                            let button_ref = if selected {
                                selected_button_ref.clone()
                            } else {
                                NodeRef::default()
                            };
                            html! {
                                <button class={button_classes} ref={button_ref}>{series_label(*series)}</button>
                            }
                        })
                    }
                </div>
            </section>
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct MessagesModalProps {
    open: bool,
    notices: Vec<TimingNotice>,
}

#[function_component(MessagesModal)]
fn messages_modal(props: &MessagesModalProps) -> Html {
    if !props.open {
        return html! {};
    }

    html! {
        <div class="backdrop">
            <section class="modal">
                <h2>{format!("Messages ({})", props.notices.len())}</h2>
                if props.notices.is_empty() {
                    <p class="empty">{"No messages available."}</p>
                } else {
                    <div class="list">
                        {
                            for props.notices.iter().rev().map(|notice| {
                                html! {
                                    <article class="notice-item">
                                        <div class="notice-time">{if notice.time.trim().is_empty() { "-".to_string() } else { notice.time.clone() }}</div>
                                        <div class="notice-text">{notice.text.clone()}</div>
                                    </article>
                                }
                            })
                        }
                    </div>
                }
            </section>
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct NlsLivetickerModalProps {
    open: bool,
    entries: Vec<web_shared::NlsLivetickerEntry>,
    last_error: String,
}

#[function_component(NlsLivetickerModal)]
fn nls_liveticker_modal(props: &NlsLivetickerModalProps) -> Html {
    if !props.open {
        return html! {};
    }

    html! {
        <div class="backdrop">
            <section class="modal">
                <h2>{format!("NLS Liveticker ({})", props.entries.len())}</h2>
                if !props.last_error.trim().is_empty() {
                    <p class="empty">{format!("Last error: {}", props.last_error)}</p>
                }
                if props.entries.is_empty() {
                    <p class="empty">{"No liveticker entries yet."}</p>
                } else {
                    <div class="list">
                        {
                            for props.entries.iter().map(|entry| {
                                html! {
                                    <article class="notice-item">
                                        <div class="notice-time">{format!("{} {}", entry.day_label, entry.time_text)}</div>
                                        <div class="notice-text">{entry.message.clone()}</div>
                                    </article>
                                }
                            })
                        }
                    </div>
                }
            </section>
        </div>
    }
}
