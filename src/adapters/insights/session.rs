use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::Value;

use super::common::is_closed_status;

pub(crate) const SCHEDULE_URL: &str = "https://insights.griiip.com/meta/sessions-schedule-live";
pub(crate) const META_SESSIONS_URL: &str = "https://insights.griiip.com/meta/sessions";
pub(crate) const LIVE_BASE_URL: &str = "https://insights.griiip.com/live";
pub(crate) const REALWORLD_DOMAIN: &str = "RealWorld";
pub(crate) const META_SESSIONS_REFERENCE_FALLBACK: &str = "2026-01-01T00:00:00Z";

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MetaSessionItem {
    pub(crate) id: u64,
    pub(crate) name: Option<String>,
    #[serde(rename = "sessionType")]
    pub(crate) session_type: Option<String>,
    #[serde(rename = "isRunning", default)]
    pub(crate) is_running: bool,
    #[serde(rename = "hasResult", default)]
    pub(crate) has_result: bool,
    #[serde(rename = "startTime")]
    pub(crate) start_time: Option<String>,
    #[serde(rename = "endTime")]
    pub(crate) end_time: Option<String>,
    #[serde(default)]
    pub(crate) event: Option<MetaEventInfo>,
    #[serde(rename = "trackConfig")]
    pub(crate) track_config: Option<MetaTrackConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MetaEventInfo {
    pub(crate) name: Option<String>,
    #[serde(rename = "trackConfig")]
    pub(crate) track_config: Option<MetaTrackConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MetaTrackConfig {
    pub(crate) name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ScheduleItem {
    sid: u64,
    #[serde(rename = "isStarted", default)]
    is_started: bool,
    #[serde(rename = "connectionStatus")]
    connection_status: Option<String>,
}

pub(crate) fn resolve_live_sid_for_series(client: &Client, series_id: u64) -> Result<u64, String> {
    let response = client
        .get(SCHEDULE_URL)
        .send()
        .map_err(|err| format!("schedule request failed: {err}"))?;
    if !response.status().is_success() {
        return Err(format!("schedule failed with HTTP {}", response.status()));
    }
    let body = response
        .text()
        .map_err(|err| format!("schedule body read failed: {err}"))?;
    let sessions = serde_json::from_str::<Vec<ScheduleItem>>(&body)
        .map_err(|err| format!("schedule decode failed: {err}"))?;

    let mut candidate_sids = Vec::with_capacity(sessions.len());
    for session in sessions.iter().filter(|session| {
        session.is_started && !is_closed_status(session.connection_status.as_deref())
    }) {
        candidate_sids.push(session.sid);
    }
    for session in &sessions {
        if !candidate_sids.contains(&session.sid) {
            candidate_sids.push(session.sid);
        }
    }

    for sid in candidate_sids {
        let info = fetch_live_json(client, sid, "session-info")?;
        let Some(map) = info.as_object() else {
            continue;
        };
        let sid_series = map.get("seriesId").and_then(Value::as_u64);
        let domain = map.get("domain").and_then(Value::as_str);
        if sid_series == Some(series_id)
            && domain
                .map(|value| value.eq_ignore_ascii_case(REALWORLD_DOMAIN))
                .unwrap_or(true)
        {
            return Ok(sid);
        }
    }

    Err(format!(
        "no active live session found for seriesId={series_id}"
    ))
}

pub(crate) fn fetch_meta_sessions_for_series(
    client: &Client,
    series_id: u64,
) -> Result<Vec<MetaSessionItem>, String> {
    let reference_time = resolve_meta_sessions_reference_time(client);
    let series_id_text = series_id.to_string();
    let response = client
        .get(META_SESSIONS_URL)
        .query(&[
            ("dateTime", reference_time.as_str()),
            ("forward", "false"),
            ("seriesIds", series_id_text.as_str()),
            ("domains", REALWORLD_DOMAIN),
        ])
        .send()
        .map_err(|err| format!("meta sessions request failed: {err}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "meta sessions request failed with HTTP {}",
            response.status()
        ));
    }
    let body = response
        .text()
        .map_err(|err| format!("meta sessions body read failed: {err}"))?;
    serde_json::from_str::<Vec<MetaSessionItem>>(&body)
        .map_err(|err| format!("meta sessions decode failed: {err}"))
}

pub(crate) fn choose_latest_finished_race_session(
    sessions: &[MetaSessionItem],
) -> Option<MetaSessionItem> {
    let mut candidates: Vec<_> = sessions
        .iter()
        .filter(|session| {
            session
                .session_type
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case("Race"))
                .unwrap_or(false)
                && session.has_result
        })
        .cloned()
        .collect();
    candidates.sort_by_key(|session| {
        session
            .end_time
            .clone()
            .or_else(|| session.start_time.clone())
            .unwrap_or_default()
    });
    candidates.reverse();

    candidates
        .iter()
        .find(|session| !session.is_running)
        .cloned()
        .or_else(|| candidates.first().cloned())
}

pub(crate) fn resolve_meta_sessions_reference_time(client: &Client) -> String {
    let Ok(response) = client.get(SCHEDULE_URL).send() else {
        return META_SESSIONS_REFERENCE_FALLBACK.to_string();
    };
    if !response.status().is_success() {
        return META_SESSIONS_REFERENCE_FALLBACK.to_string();
    }
    let Ok(body) = response.text() else {
        return META_SESSIONS_REFERENCE_FALLBACK.to_string();
    };
    let Ok(payload) = serde_json::from_str::<Vec<Value>>(&body) else {
        return META_SESSIONS_REFERENCE_FALLBACK.to_string();
    };
    for item in payload {
        let Some(clock) = item.get("clock") else {
            continue;
        };
        if let Some(ts_now) = clock.get("tsNow").and_then(Value::as_str) {
            return ts_now.to_string();
        }
        if let Some(ts) = clock.get("ts").and_then(Value::as_str) {
            return ts.to_string();
        }
    }
    META_SESSIONS_REFERENCE_FALLBACK.to_string()
}

pub(crate) fn fetch_live_json(client: &Client, sid: u64, route: &str) -> Result<Value, String> {
    let url = format!("{LIVE_BASE_URL}/{route}/{sid}");
    let response = client
        .get(&url)
        .send()
        .map_err(|err| format!("live request failed ({route}): {err}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "live endpoint {route} failed with HTTP {}",
            response.status()
        ));
    }
    let body = response
        .text()
        .map_err(|err| format!("live body read failed ({route}): {err}"))?;
    serde_json::from_str::<Value>(&body)
        .map_err(|err| format!("live decode failed ({route}): {err}"))
}
