#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedStandingsData {
    pub overall_position: Option<String>,
    pub car_number: Option<String>,
    pub status: Option<String>,
    pub class_position: Option<String>,
    pub pit_marker: Option<String>,
    pub pit_stops: Option<String>,
    pub segments: Vec<String>,
}

pub fn parse_standings_data(raw: &str) -> ParsedStandingsData {
    let segments: Vec<String> = raw.split(';').map(|part| part.trim().to_string()).collect();

    let overall_position = segment(&segments, 0);
    let car_number = segment(&segments, 1);
    let status = segment(&segments, 2);
    let class_position = segment(&segments, 3);

    let pit_marker = segment(&segments, 8).or_else(|| {
        segments
            .iter()
            .find(|part| is_pit_token(part))
            .cloned()
            .filter(|part| !part.is_empty())
    });
    let pit_stops = segment(&segments, 9);

    ParsedStandingsData {
        overall_position,
        car_number,
        status,
        class_position,
        pit_marker,
        pit_stops,
        segments,
    }
}

fn segment(parts: &[String], index: usize) -> Option<String> {
    parts
        .get(index)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn is_pit_token(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_uppercase().as_str(),
        "BOX" | "PIT" | "IN"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_core_fields_from_compact_standings_data() {
        let parsed = parse_standings_data("1;4;CLASSIFIED;1;0;0;0;;BOX;1;");

        assert_eq!(parsed.overall_position.as_deref(), Some("1"));
        assert_eq!(parsed.car_number.as_deref(), Some("4"));
        assert_eq!(parsed.status.as_deref(), Some("CLASSIFIED"));
        assert_eq!(parsed.class_position.as_deref(), Some("1"));
        assert_eq!(parsed.pit_marker.as_deref(), Some("BOX"));
        assert_eq!(parsed.pit_stops.as_deref(), Some("1"));
        assert_eq!(parsed.segments.len(), 11);
    }

    #[test]
    fn keeps_unknown_segments_for_future_extension() {
        let parsed = parse_standings_data("12;38;RUN;4;X;Y;Z;;PIT;3;EXTRA");

        assert_eq!(parsed.segments.get(4).map(String::as_str), Some("X"));
        assert_eq!(parsed.segments.get(10).map(String::as_str), Some("EXTRA"));
    }
}
