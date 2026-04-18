use std::{
    collections::HashSet,
    hash::{Hash, Hasher},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

use reqwest::blocking::Client;

const LIVETICKER_URL: &str =
    "https://www.nuerburgring-langstrecken-serie.de/wp-content/themes/pofo-child/liveticker.php";
const POLL_INTERVAL: Duration = Duration::from_secs(20);
const HTTP_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivetickerEntry {
    pub day_label: String,
    pub time_text: String,
    pub message: String,
    pub id: String,
}

#[derive(Debug, Clone)]
pub enum LivetickerWorkerMessage {
    Snapshot { entries: Vec<LivetickerEntry> },
    Error { text: String },
}

#[derive(Debug)]
pub struct ActiveLivetickerFeed {
    stop_tx: Sender<()>,
    pub rx: Receiver<LivetickerWorkerMessage>,
}

pub fn start_liveticker_feed() -> ActiveLivetickerFeed {
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (tx, rx) = mpsc::channel::<LivetickerWorkerMessage>();

    thread::spawn(move || {
        let client = Client::builder()
            .timeout(HTTP_TIMEOUT)
            .user_agent("imsa_tui/0.1")
            .build();

        let client = match client {
            Ok(ok) => ok,
            Err(err) => {
                let _ = tx.send(LivetickerWorkerMessage::Error {
                    text: format!("NLS liveticker client setup failed: {err}"),
                });
                return;
            }
        };

        let mut cached_entries: Vec<LivetickerEntry> = Vec::new();

        loop {
            if stop_rx.try_recv().is_ok() {
                break;
            }

            match fetch_liveticker_entries(&client) {
                Ok(latest_entries) => {
                    let merged_entries = merge_entries(&latest_entries, &cached_entries);
                    if merged_entries != cached_entries {
                        cached_entries = merged_entries.clone();
                        let _ = tx.send(LivetickerWorkerMessage::Snapshot {
                            entries: merged_entries,
                        });
                    }
                }
                Err(err) => {
                    let _ = tx.send(LivetickerWorkerMessage::Error {
                        text: format!("NLS liveticker fetch failed: {err}"),
                    });
                }
            }

            if stop_rx.recv_timeout(POLL_INTERVAL).is_ok() {
                break;
            }
        }
    });

    ActiveLivetickerFeed { stop_tx, rx }
}

pub fn stop_liveticker_feed(feed: &mut Option<ActiveLivetickerFeed>) {
    if let Some(active_feed) = feed.take() {
        let _ = active_feed.stop_tx.send(());
    }
}

fn merge_entries(
    latest_entries: &[LivetickerEntry],
    cached_entries: &[LivetickerEntry],
) -> Vec<LivetickerEntry> {
    let mut merged = Vec::with_capacity(latest_entries.len().max(cached_entries.len()));
    let mut seen = HashSet::new();

    for entry in latest_entries {
        if seen.insert(entry.id.clone()) {
            merged.push(entry.clone());
        }
    }

    for entry in cached_entries {
        if seen.insert(entry.id.clone()) {
            merged.push(entry.clone());
        }
    }

    merged
}

fn fetch_liveticker_entries(client: &Client) -> Result<Vec<LivetickerEntry>, String> {
    let response = client
        .get(LIVETICKER_URL)
        .send()
        .map_err(|err| format!("request error: {err}"))?;

    let body = response
        .text()
        .map_err(|err| format!("body read error: {err}"))?;

    Ok(parse_liveticker_entries(&body))
}

pub fn parse_liveticker_entries(raw: &str) -> Vec<LivetickerEntry> {
    let mut lines = extract_lines_from_table(raw);
    if lines.is_empty() {
        lines = fallback_text_lines(raw);
    }

    let mut entries = Vec::new();
    let mut current_day = String::new();
    let mut current_time = String::new();
    let mut current_message_lines: Vec<String> = Vec::new();

    for line in lines {
        if let Some((day_label, time_text, inline_message)) = parse_header_line(&line) {
            if !current_day.is_empty()
                || !current_time.is_empty()
                || !current_message_lines.is_empty()
            {
                entries.push(build_entry(
                    &current_day,
                    &current_time,
                    &current_message_lines,
                ));
            }

            current_day = day_label;
            current_time = time_text;
            current_message_lines.clear();
            if !inline_message.is_empty() {
                current_message_lines.push(inline_message);
            }
            continue;
        }

        if !line.is_empty() && (!current_day.is_empty() || !current_time.is_empty()) {
            current_message_lines.push(line);
        }
    }

    if !current_day.is_empty() || !current_time.is_empty() || !current_message_lines.is_empty() {
        entries.push(build_entry(
            &current_day,
            &current_time,
            &current_message_lines,
        ));
    }

    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    for entry in entries {
        if seen.insert(entry.id.clone()) {
            deduped.push(entry);
        }
    }
    deduped
}

fn build_entry(day_label: &str, time_text: &str, message_lines: &[String]) -> LivetickerEntry {
    let message = message_lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| normalize_spaces(line))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    let id = build_entry_id(day_label, time_text, &message);

    LivetickerEntry {
        day_label: day_label.to_string(),
        time_text: time_text.to_string(),
        message,
        id,
    }
}

fn build_entry_id(day_label: &str, time_text: &str, message: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    day_label.hash(&mut hasher);
    time_text.hash(&mut hasher);
    message.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn extract_lines_from_table(raw: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut offset = 0;

    while let Some(row_start_rel) = raw[offset..].find("<tr") {
        let row_start = offset + row_start_rel;
        let Some(open_end_rel) = raw[row_start..].find('>') else {
            break;
        };
        let open_end = row_start + open_end_rel;
        let Some(close_rel) = raw[open_end + 1..].find("</tr>") else {
            break;
        };
        let close = open_end + 1 + close_rel;
        let row_html = &raw[open_end + 1..close];
        offset = close + 5;

        let cells = extract_table_cells(row_html);
        if cells.is_empty() {
            continue;
        }

        let time = normalize_spaces(&decode_html_entities(&html_fragment_to_text(&cells[0])));
        if !time.is_empty() {
            lines.push(time);
        }

        if let Some(message_raw) = cells.get(1) {
            for line in html_fragment_to_text(message_raw).lines() {
                let normalized = normalize_spaces(&decode_html_entities(line));
                if !normalized.is_empty() {
                    lines.push(normalized);
                }
            }
        }
    }

    lines
}

fn extract_table_cells(row_html: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut offset = 0;

    while let Some(cell_start_rel) = row_html[offset..].find("<td") {
        let cell_start = offset + cell_start_rel;
        let Some(open_end_rel) = row_html[cell_start..].find('>') else {
            break;
        };
        let open_end = cell_start + open_end_rel;
        let Some(close_rel) = row_html[open_end + 1..].find("</td>") else {
            break;
        };
        let close = open_end + 1 + close_rel;
        cells.push(row_html[open_end + 1..close].to_string());
        offset = close + 5;
    }

    cells
}

fn html_fragment_to_text(fragment: &str) -> String {
    let mut output = String::with_capacity(fragment.len());
    let mut in_tag = false;
    let mut tag_buf = String::new();

    for ch in fragment.chars() {
        if in_tag {
            if ch == '>' {
                in_tag = false;
                let tag_name = tag_buf.trim().trim_start_matches('/').to_ascii_lowercase();
                if tag_name.starts_with("br")
                    || tag_name == "p"
                    || tag_name == "div"
                    || tag_name == "li"
                    || tag_name == "tr"
                    || tag_name == "td"
                {
                    output.push('\n');
                }
                tag_buf.clear();
            } else {
                tag_buf.push(ch);
            }
            continue;
        }

        if ch == '<' {
            in_tag = true;
            tag_buf.clear();
            continue;
        }

        output.push(ch);
    }

    output
}

fn fallback_text_lines(raw: &str) -> Vec<String> {
    let text = html_fragment_to_text(raw);
    let decoded = decode_html_entities(&text);
    decoded
        .lines()
        .map(normalize_spaces)
        .filter(|line| !line.is_empty())
        .collect()
}

fn parse_header_line(line: &str) -> Option<(String, String, String)> {
    let trimmed = line.trim();
    let comma_idx = trimmed.find(',')?;
    let day = trimmed[..comma_idx].trim();
    if !matches!(day, "Mo" | "Di" | "Mi" | "Do" | "Fr" | "Sa" | "So") {
        return None;
    }

    let mut chars = trimmed[comma_idx + 1..].trim_start().chars().peekable();

    let mut hour = String::new();
    while let Some(ch) = chars.peek().copied() {
        if ch.is_ascii_digit() {
            hour.push(ch);
            chars.next();
            if hour.len() == 2 {
                break;
            }
        } else {
            break;
        }
    }
    if hour.is_empty() || hour.len() > 2 {
        return None;
    }

    if chars.next() != Some(':') {
        return None;
    }

    let mut minute = String::new();
    for _ in 0..2 {
        let ch = chars.next()?;
        if !ch.is_ascii_digit() {
            return None;
        }
        minute.push(ch);
    }

    let hour_num = hour.parse::<u32>().ok()?;
    let minute_num = minute.parse::<u32>().ok()?;
    if hour_num > 23 || minute_num > 59 {
        return None;
    }

    let trailing = chars.collect::<String>();
    let trailing = trailing.trim_start();
    if !trailing.to_ascii_lowercase().starts_with("uhr") {
        return None;
    }
    let inline_message = trailing[3..].trim_start().to_string();

    Some((
        day.to_string(),
        format!("{:02}:{minute}", hour_num),
        inline_message,
    ))
}

fn normalize_spaces(raw: &str) -> String {
    raw.replace('\u{00a0}', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode_html_entities(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let mut idx = 0usize;

    while idx < chars.len() {
        if chars[idx] != '&' {
            output.push(chars[idx]);
            idx += 1;
            continue;
        }

        let mut end = idx + 1;
        while end < chars.len() && end.saturating_sub(idx) <= 12 && chars[end] != ';' {
            end += 1;
        }
        if end >= chars.len() || chars[end] != ';' {
            output.push(chars[idx]);
            idx += 1;
            continue;
        }

        let entity: String = chars[idx + 1..end].iter().collect();
        let decoded = match entity.as_str() {
            "nbsp" => Some(' '),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "ndash" | "mdash" => Some('-'),
            _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                u32::from_str_radix(entity.trim_start_matches("#x").trim_start_matches("#X"), 16)
                    .ok()
                    .and_then(char::from_u32)
            }
            _ if entity.starts_with('#') => {
                entity[1..].parse::<u32>().ok().and_then(char::from_u32)
            }
            _ => None,
        };

        if let Some(ch) = decoded {
            output.push(ch);
            idx = end + 1;
            continue;
        }

        output.push('&');
        output.push_str(&entity);
        output.push(';');
        idx = end + 1;
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_handles_uhr_without_trailing_space() {
        let raw = "Sa,&nbsp;18:42&nbsp;UhrBei der Kollision";
        let entries = parse_liveticker_entries(raw);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].day_label, "Sa");
        assert_eq!(entries[0].time_text, "18:42");
        assert_eq!(entries[0].message, "Bei der Kollision");
    }

    #[test]
    fn parser_keeps_continuation_lines_on_current_entry() {
        let raw = "Sa,&nbsp;16:37&nbsp;UhrErste Zeile<br/>Fortsetzung<br/>Noch eine";
        let entries = parse_liveticker_entries(raw);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message, "Erste Zeile\nFortsetzung\nNoch eine");
    }

    #[test]
    fn parser_deduplicates_entries_by_stable_id() {
        let raw = "
            <table>
              <tr><td>Sa,&nbsp;17:55&nbsp;Uhr</td><td>ROTE FLAGGE</td></tr>
              <tr><td>Sa,&nbsp;17:55&nbsp;Uhr</td><td>ROTE FLAGGE</td></tr>
            </table>
        ";
        let entries = parse_liveticker_entries(raw);
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn merge_entries_keeps_newest_order_and_old_cache() {
        let latest = vec![LivetickerEntry {
            day_label: "Sa".to_string(),
            time_text: "18:42".to_string(),
            message: "new".to_string(),
            id: "new-id".to_string(),
        }];
        let cached = vec![LivetickerEntry {
            day_label: "Sa".to_string(),
            time_text: "18:06".to_string(),
            message: "old".to_string(),
            id: "old-id".to_string(),
        }];

        let merged = merge_entries(&latest, &cached);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].id, "new-id");
        assert_eq!(merged[1].id, "old-id");
    }
}
