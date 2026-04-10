// Shared favourite-key normalization used by TUI and WebUI persistence.

use std::collections::HashSet;

use crate::timing::Series;

pub fn favourite_key(series: Series, stable_id: &str) -> String {
    let normalized_stable = normalize_stable_id(series, stable_id.trim());
    format!("{}|{}", series.as_key_prefix(), normalized_stable)
}

pub fn normalize_favourite_key(raw: &str) -> Option<String> {
    let (series_raw, stable_raw) = raw.split_once('|')?;
    let series = parse_series_key(series_raw.trim())?;
    let stable = normalize_stable_id(series, stable_raw.trim());
    if stable.is_empty() {
        return None;
    }
    Some(format!("{}|{}", series.as_key_prefix(), stable))
}

pub fn normalize_favourites(values: impl IntoIterator<Item = String>) -> HashSet<String> {
    values
        .into_iter()
        .filter_map(|value| normalize_favourite_key(&value))
        .collect()
}

fn normalize_stable_id(series: Series, stable_id: &str) -> String {
    match series {
        Series::Imsa => trim_class_suffix(stable_id, "fallback"),
        Series::Nls => trim_class_suffix(stable_id, "stnr"),
        Series::F1 => stable_id.to_string(),
    }
}

fn trim_class_suffix(stable_id: &str, expected_prefix: &str) -> String {
    let prefix = format!("{expected_prefix}:");
    if !stable_id.starts_with(&prefix) {
        return stable_id.to_string();
    }

    let mut parts = stable_id.split(':');
    let Some(first) = parts.next() else {
        return stable_id.to_string();
    };
    let Some(second) = parts.next() else {
        return stable_id.to_string();
    };
    if parts.next().is_none() {
        return stable_id.to_string();
    }

    format!("{first}:{second}")
}

fn parse_series_key(value: &str) -> Option<Series> {
    match value {
        "imsa" => Some(Series::Imsa),
        "nls" => Some(Series::Nls),
        "f1" => Some(Series::F1),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_legacy_imsa_and_nls_keys() {
        assert_eq!(
            normalize_favourite_key("imsa|fallback:7:GTP"),
            Some("imsa|fallback:7".to_string())
        );
        assert_eq!(
            normalize_favourite_key("nls|stnr:632:AT2"),
            Some("nls|stnr:632".to_string())
        );
        assert_eq!(
            normalize_favourite_key("f1|f1:driver:12"),
            Some("f1|f1:driver:12".to_string())
        );
    }

    #[test]
    fn deduplicates_during_normalization() {
        let normalized = normalize_favourites(vec![
            "imsa|fallback:7:GTP".to_string(),
            "imsa|fallback:7".to_string(),
            "nls|stnr:632:AT2".to_string(),
            "nls|stnr:632".to_string(),
        ]);

        assert_eq!(normalized.len(), 2);
        assert!(normalized.contains("imsa|fallback:7"));
        assert!(normalized.contains("nls|stnr:632"));
    }
}
