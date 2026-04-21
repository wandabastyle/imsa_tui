use serde_json::{Map, Value};

pub(crate) fn format_gap(gap_ms: Option<i64>, gap_laps: Option<i64>) -> Option<String> {
    if let Some(laps) = gap_laps {
        if laps > 0 {
            return Some(format!("+{laps} L"));
        }
    }
    let millis = gap_ms?;
    if millis <= 0 {
        return Some("-".to_string());
    }
    let secs = millis / 1000;
    let rem = millis % 1000;
    Some(format!("+{secs}.{rem:03}"))
}

pub(crate) fn format_lap_time_ms(ms: i64) -> String {
    if ms <= 0 {
        return "-".to_string();
    }
    let total_ms = ms as u64;
    let minutes = total_ms / 60_000;
    let seconds = (total_ms % 60_000) / 1000;
    let millis = total_ms % 1000;
    format!("{minutes}:{seconds:02}.{millis:03}")
}

pub(crate) fn map_str(map: &Map<String, Value>, key: &str) -> Option<String> {
    let raw = map.get(key)?.as_str()?.trim();
    if raw.is_empty() {
        None
    } else {
        Some(raw.to_string())
    }
}

pub(crate) fn map_i64(map: &Map<String, Value>, key: &str) -> Option<i64> {
    map.get(key).and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
            .or_else(|| value.as_str()?.trim().parse::<i64>().ok())
    })
}

pub(crate) fn map_u32(map: &Map<String, Value>, key: &str) -> Option<u32> {
    map_i64(map, key).and_then(|number| u32::try_from(number).ok())
}

pub(crate) fn map_bool(map: &Map<String, Value>, key: &str) -> Option<bool> {
    map.get(key).and_then(|value| {
        value.as_bool().or_else(|| {
            value
                .as_str()
                .and_then(|raw| match raw.trim().to_ascii_lowercase().as_str() {
                    "true" | "1" | "yes" => Some(true),
                    "false" | "0" | "no" => Some(false),
                    _ => None,
                })
        })
    })
}

pub(crate) fn map_text(map: &Map<String, Value>, key: &str) -> Option<String> {
    let value = map.get(key)?;
    match value {
        Value::String(text) => Some(text.trim().to_string()).filter(|value| !value.is_empty()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(if *value { "true" } else { "false" }.to_string()),
        _ => None,
    }
}

pub(crate) fn normalize_driver_name(raw: &str) -> String {
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
        && (letters.chars().all(|ch| ch.is_uppercase())
            || letters.chars().all(|ch| ch.is_lowercase()));
    if !needs_normalization {
        return token.to_string();
    }

    let mut out = String::with_capacity(token.len());
    let mut seen_alpha = false;
    for ch in token.chars() {
        if ch.is_alphabetic() {
            if !seen_alpha {
                out.extend(ch.to_uppercase());
                seen_alpha = true;
            } else {
                out.extend(ch.to_lowercase());
            }
        } else {
            seen_alpha = false;
            out.push(ch);
        }
    }
    out
}

pub(crate) fn is_closed_status(status: Option<&str>) -> bool {
    let Some(status) = status else {
        return false;
    };
    let normalized = status.trim().to_ascii_lowercase();
    normalized == "closed" || normalized == "ended" || normalized == "finished"
}
