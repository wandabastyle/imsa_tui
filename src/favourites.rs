// Shared favourite-key validation used by TUI and WebUI persistence.

use std::collections::HashSet;

use crate::timing::Series;

pub fn favourite_key(series: Series, stable_id: &str) -> String {
    format!("{}|{}", series.as_key_prefix(), stable_id.trim())
}

pub fn normalize_favourite_key(raw: &str) -> Option<String> {
    let (series_raw, stable_raw) = raw.split_once('|')?;
    let series = parse_series_key(series_raw.trim())?;
    let stable = stable_raw.trim();
    if stable.is_empty() || has_legacy_class_suffix(series, stable) {
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

fn has_legacy_class_suffix(series: Series, stable_id: &str) -> bool {
    match series {
        Series::Imsa => stable_id.starts_with("fallback:") && stable_id.matches(':').count() > 1,
        Series::Nls | Series::Dhlm => {
            stable_id.starts_with("stnr:") && stable_id.matches(':').count() > 1
        }
        Series::F1 | Series::Wec => false,
    }
}

fn parse_series_key(value: &str) -> Option<Series> {
    match value {
        "imsa" => Some(Series::Imsa),
        "nls" => Some(Series::Nls),
        "f1" => Some(Series::F1),
        "wec" => Some(Series::Wec),
        "dhlm" => Some(Series::Dhlm),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_legacy_imsa_and_nls_keys() {
        assert_eq!(normalize_favourite_key("imsa|fallback:7:GTP"), None);
        assert_eq!(normalize_favourite_key("nls|stnr:632:AT2"), None);
        assert_eq!(
            normalize_favourite_key("f1|f1:driver:12"),
            Some("f1|f1:driver:12".to_string())
        );
    }

    #[test]
    fn keeps_only_valid_classless_keys() {
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
