use std::{collections::BTreeMap, time::Instant};

use ratatui::style::{Color, Modifier, Style};

use crate::{
    adapters::imsa::normalize_class_name,
    timing::{Series, TimingClassColor},
};

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    let t = t.clamp(0.0, 1.0);
    ((a as f32) + ((b as f32) - (a as f32)) * t).round() as u8
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    match (a, b) {
        (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) => {
            Color::Rgb(lerp_u8(ar, br, t), lerp_u8(ag, bg, t), lerp_u8(ab, bb, t))
        }
        _ => b,
    }
}

fn base_flag_colors(flag: &str) -> (String, Color, Color, bool) {
    match flag.trim().to_ascii_lowercase().as_str() {
        "green" | "normal" => (
            "Green".to_string(),
            Color::Rgb(0, 153, 68),
            Color::Black,
            false,
        ),
        "yellow" => (
            "Yellow".to_string(),
            Color::Rgb(255, 221, 0),
            Color::Black,
            true,
        ),
        "red" => (
            "Red".to_string(),
            Color::Rgb(200, 16, 46),
            Color::White,
            false,
        ),
        "checkered" | "chequered" => (
            "Checkered".to_string(),
            Color::Rgb(245, 245, 245),
            Color::Black,
            false,
        ),
        "-" | "" => (
            "Green".to_string(),
            Color::Rgb(0, 153, 68),
            Color::Black,
            false,
        ),
        other => (
            other.to_string(),
            Color::Rgb(0, 153, 68),
            Color::Black,
            false,
        ),
    }
}

pub(crate) fn animated_flag_theme(
    flag: &str,
    previous_flag: &str,
    transition_started_at: Instant,
) -> (String, Style, Style) {
    let (flag_text, target_bg, target_fg, _) = base_flag_colors(flag);
    let (_, previous_bg, _, _) = base_flag_colors(previous_flag);

    let transition_t = (transition_started_at.elapsed().as_millis() as f32 / 450.0).clamp(0.0, 1.0);
    let bg = lerp_color(previous_bg, target_bg, transition_t);

    let header_style = Style::default().fg(target_fg).bg(bg);
    let flag_span_style = header_style.add_modifier(Modifier::BOLD);

    (flag_text, flag_span_style, header_style)
}

pub(crate) fn class_style(
    class_name: &str,
    active_series: Series,
    class_colors: &BTreeMap<String, TimingClassColor>,
) -> Style {
    if matches!(active_series, Series::Nls | Series::Dhlm) {
        return Style::default();
    }

    let key = normalize_class_key(class_name);
    if let Some(live) = resolve_live_class_color(class_colors, &key) {
        return Style::default().fg(live).add_modifier(Modifier::BOLD);
    }

    if active_series == Series::Wec {
        return class_style_wec_static(&key);
    }

    match key.as_str() {
        "GTP" => Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
        "LMP2" => Style::default()
            .fg(Color::Rgb(63, 144, 218))
            .add_modifier(Modifier::BOLD),
        "GTD-PRO" | "GTDPRO" => Style::default()
            .fg(Color::Rgb(210, 38, 48))
            .add_modifier(Modifier::BOLD),
        "PRO" => Style::default()
            .fg(Color::Rgb(230, 126, 34))
            .add_modifier(Modifier::BOLD),
        "PRO-AM" => Style::default()
            .fg(Color::Rgb(76, 175, 80))
            .add_modifier(Modifier::BOLD),
        "MASTERS" => Style::default()
            .fg(Color::Rgb(241, 211, 2))
            .add_modifier(Modifier::BOLD),
        "GTD" => Style::default()
            .fg(Color::Rgb(0, 166, 81))
            .add_modifier(Modifier::BOLD),
        "LMH" => Style::default()
            .fg(Color::Rgb(220, 20, 60))
            .add_modifier(Modifier::BOLD),
        "LMGT3" => Style::default()
            .fg(Color::Rgb(30, 144, 255))
            .add_modifier(Modifier::BOLD),
        _ => Style::default(),
    }
}

fn resolve_live_class_color(
    class_colors: &BTreeMap<String, TimingClassColor>,
    class_key: &str,
) -> Option<Color> {
    class_colors
        .iter()
        .find(|(raw_key, _)| normalize_class_key(raw_key) == class_key)
        .and_then(|(_, value)| parse_hex_color(&value.color))
}

fn normalize_class_key(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut pending_separator = false;

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_separator && !normalized.is_empty() {
                normalized.push('-');
            }
            normalized.push(ch.to_ascii_uppercase());
            pending_separator = false;
            continue;
        }

        if ch.is_whitespace() || ch == '_' || ch == '-' {
            pending_separator = !normalized.is_empty();
        }
    }

    normalized
}

fn parse_hex_color(value: &str) -> Option<Color> {
    let trimmed = value.trim();
    if trimmed.len() != 7 || !trimmed.starts_with('#') {
        return None;
    }
    let r = u8::from_str_radix(&trimmed[1..3], 16).ok()?;
    let g = u8::from_str_radix(&trimmed[3..5], 16).ok()?;
    let b = u8::from_str_radix(&trimmed[5..7], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

fn class_style_wec_static(class_key: &str) -> Style {
    match class_key {
        "HYPER" | "HYPERCAR" | "LMH" => Style::default()
            .fg(Color::Rgb(226, 30, 25))
            .add_modifier(Modifier::BOLD),
        "LMGT3" => Style::default()
            .fg(Color::Rgb(11, 147, 20))
            .add_modifier(Modifier::BOLD),
        "LMP1" => Style::default()
            .fg(Color::Rgb(255, 16, 83))
            .add_modifier(Modifier::BOLD),
        "LMP2" => Style::default()
            .fg(Color::Rgb(63, 144, 218))
            .add_modifier(Modifier::BOLD),
        "LMGTE" => Style::default()
            .fg(Color::Rgb(255, 169, 18))
            .add_modifier(Modifier::BOLD),
        "INV" => Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
        _ => Style::default(),
    }
}

pub(crate) fn class_display_name(name: &str) -> String {
    let normalized = normalize_class_name(name);
    match normalized.as_str() {
        "GTP" => "GTP".to_string(),
        "LMP2" => "LMP2".to_string(),
        "LMP1" => "LMP1".to_string(),
        "LMGTE" => "LMGTE".to_string(),
        "INV" => "INV".to_string(),
        "GTDPRO" => "GTD PRO".to_string(),
        "GTD" => "GTD".to_string(),
        "LMH" => "LMH".to_string(),
        "LMGT3" => "LMGT3".to_string(),
        _ => {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                "-".to_string()
            } else {
                trimmed.to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_style_returns_default_for_dhlm() {
        let colors = BTreeMap::new();
        let style = class_style("SP9", Series::Dhlm, &colors);
        assert_eq!(style, Style::default());
    }

    #[test]
    fn class_style_wec_prefers_live_class_color() {
        let mut colors = BTreeMap::new();
        colors.insert(
            "HYPER".to_string(),
            TimingClassColor {
                color: "#e21e19".to_string(),
            },
        );

        let style = class_style("HYPER", Series::Wec, &colors);
        assert_eq!(
            style,
            Style::default()
                .fg(Color::Rgb(226, 30, 25))
                .add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn class_style_non_wec_prefers_live_class_color() {
        let mut colors = BTreeMap::new();
        colors.insert(
            "GTP".to_string(),
            TimingClassColor {
                color: "#00ff00".to_string(),
            },
        );

        let style = class_style("GTP", Series::Imsa, &colors);
        assert_eq!(
            style,
            Style::default()
                .fg(Color::Rgb(0, 255, 0))
                .add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn class_style_porsche_cup_variants_use_canonical_separator_matching() {
        let colors = BTreeMap::new();
        let pro_am = class_style("Pro Am", Series::Imsa, &colors);
        let pro_am_dash = class_style("PRO-AM", Series::Imsa, &colors);
        let pro_am_underscore = class_style("PRO_AM", Series::Imsa, &colors);

        let expected = Style::default()
            .fg(Color::Rgb(76, 175, 80))
            .add_modifier(Modifier::BOLD);
        assert_eq!(pro_am, expected);
        assert_eq!(pro_am_dash, expected);
        assert_eq!(pro_am_underscore, expected);
    }
}
