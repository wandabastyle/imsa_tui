use reqwest::{blocking::Client, Url};

const DEFAULT_NLS_EVENT_ID: &str = "20";
const N24_EVENT_ID: &str = "50";
const NLS_HOME_URL: &str = "https://www.nuerburgring-langstrecken-serie.de/language/de/startseite/";
const N24_TERMINE_URL: &str = "https://www.24h-rennen.de/termine/";
const N24_TARGET_EVENT_TITLE: &str = "ADAC RAVENOL 24h Nürburgring";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CalendarDate {
    pub(super) year: i32,
    pub(super) month: u32,
    pub(super) day: u32,
}

#[derive(Debug, Clone)]
pub(super) struct TermineScheduleEntry {
    pub(super) start: CalendarDate,
    pub(super) end: CalendarDate,
    pub(super) title: String,
}

fn strip_tags(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut in_tag = false;
    for ch in raw.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
}

fn normalize_spaces(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn find_anchor_elements(html: &str) -> Vec<(String, String)> {
    let mut anchors = Vec::new();
    let mut offset = 0;

    while let Some(anchor_start_rel) = html[offset..].find("<a") {
        let anchor_start = offset + anchor_start_rel;
        let Some(tag_end_rel) = html[anchor_start..].find('>') else {
            break;
        };
        let tag_end = anchor_start + tag_end_rel;
        let Some(close_rel) = html[tag_end + 1..].find("</a>") else {
            break;
        };
        let close = tag_end + 1 + close_rel;

        let tag = &html[anchor_start..=tag_end];
        let body = &html[tag_end + 1..close];
        anchors.push((tag.to_string(), normalize_spaces(&strip_tags(body))));

        offset = close + 4;
    }

    anchors
}

fn extract_href_attr(anchor_tag: &str) -> Option<String> {
    let href_pos = anchor_tag.find("href=")?;
    let rest = &anchor_tag[href_pos + 5..];
    let quoted = rest.trim_start();
    let quote = quoted.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let value_with_quote = &quoted[1..];
    let end = value_with_quote.find(quote)?;
    Some(value_with_quote[..end].to_string())
}

fn resolve_url(base: &str, href: &str) -> Option<String> {
    let base_url = Url::parse(base).ok()?;
    base_url.join(href).ok().map(|url| url.to_string())
}

pub(super) fn discover_termine_url_from_homepage_html(homepage_html: &str) -> Option<String> {
    let mut generic_candidate: Option<String> = None;

    for (tag, label) in find_anchor_elements(homepage_html) {
        let normalized_label = label.trim();
        if !normalized_label.to_ascii_lowercase().contains("termine") {
            continue;
        }

        let href = extract_href_attr(&tag)?;
        if href.trim().is_empty() || href.trim() == "#" {
            continue;
        }

        let absolute = resolve_url(NLS_HOME_URL, href.trim())?;
        let looks_like_year_link = normalized_label.chars().any(|ch| ch.is_ascii_digit())
            || absolute.to_ascii_lowercase().contains("termine-");

        if looks_like_year_link {
            return Some(absolute);
        }
        if generic_candidate.is_none() {
            generic_candidate = Some(absolute);
        }
    }

    generic_candidate
}

fn discover_termine_url(client: &Client) -> Result<String, String> {
    let response = client
        .get(NLS_HOME_URL)
        .send()
        .map_err(|err| format!("failed to fetch NLS homepage: {err}"))?;
    let html = response
        .text()
        .map_err(|err| format!("failed to read NLS homepage: {err}"))?;

    discover_termine_url_from_homepage_html(&html)
        .ok_or_else(|| "failed to discover Termine URL from homepage".to_string())
}

fn parse_single_german_date(raw: &str) -> Option<CalendarDate> {
    let normalized = normalize_spaces(raw).replace(' ', "");
    let parts: Vec<&str> = normalized
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect();
    if parts.len() != 3 {
        return None;
    }

    let day = parts[0].parse::<u32>().ok()?;
    let month = parts[1].parse::<u32>().ok()?;
    let year = parts[2].parse::<i32>().ok()?;
    let max_day = days_in_month(year, month)?;
    if day == 0 || day > max_day {
        return None;
    }

    Some(CalendarDate { year, month, day })
}

fn parse_termine_date_window(raw: &str) -> Option<(CalendarDate, CalendarDate)> {
    parse_german_date_range(raw).or_else(|| parse_single_german_date(raw).map(|date| (date, date)))
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

pub(super) fn parse_termine_entries(html: &str) -> Vec<TermineScheduleEntry> {
    let mut entries = Vec::new();
    let mut offset = 0;

    while let Some(row_start_rel) = html[offset..].find("<tr") {
        let row_start = offset + row_start_rel;
        let Some(open_end_rel) = html[row_start..].find('>') else {
            break;
        };
        let open_end = row_start + open_end_rel;
        let Some(close_rel) = html[open_end + 1..].find("</tr>") else {
            break;
        };
        let close = open_end + 1 + close_rel;
        let row_html = &html[open_end + 1..close];
        let cells = extract_table_cells(row_html);
        if cells.len() >= 2 {
            let date_text = normalize_spaces(&strip_tags(&cells[0]));
            let Some((start, end)) = parse_termine_date_window(&date_text) else {
                offset = close + 5;
                continue;
            };

            let title_text = find_anchor_elements(&cells[1])
                .into_iter()
                .map(|(_, body)| body)
                .find(|value| !value.trim().is_empty())
                .unwrap_or_else(|| normalize_spaces(&strip_tags(&cells[1])));
            if title_text.trim().is_empty() {
                offset = close + 5;
                continue;
            }

            entries.push(TermineScheduleEntry {
                start,
                end,
                title: title_text,
            });
        }

        offset = close + 5;
    }

    entries
}

pub(super) fn select_active_termine_event_title(
    entries: &[TermineScheduleEntry],
    today: CalendarDate,
) -> Option<String> {
    if let Some(active) = entries
        .iter()
        .find(|entry| today >= entry.start && today <= entry.end)
    {
        return Some(active.title.clone());
    }

    if let Some(next_upcoming) = entries
        .iter()
        .filter(|entry| entry.start >= today)
        .min_by_key(|entry| entry.start)
    {
        return Some(next_upcoming.title.clone());
    }

    entries
        .iter()
        .filter(|entry| entry.end <= today)
        .max_by_key(|entry| entry.end)
        .map(|entry| entry.title.clone())
}

pub(super) fn fetch_termine_event_name(client: &Client) -> Result<String, String> {
    let termine_url = discover_termine_url(client)?;
    let response = client
        .get(&termine_url)
        .send()
        .map_err(|err| format!("failed to fetch Termine page: {err}"))?;
    let html = response
        .text()
        .map_err(|err| format!("failed to read Termine page: {err}"))?;

    let entries = parse_termine_entries(&html);
    if entries.is_empty() {
        return Err("failed to parse Termine entries".to_string());
    }

    let today = local_today().ok_or_else(|| "failed to resolve local date".to_string())?;
    select_active_termine_event_title(&entries, today)
        .ok_or_else(|| "no active Termine entry for current date".to_string())
}

fn extract_between<'a>(haystack: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = haystack.find(start)? + start.len();
    let rest = &haystack[from..];
    let to = rest.find(end)?;
    Some(&rest[..to])
}

fn parse_event_name_from_homepage(html: &str) -> Option<String> {
    let h1_marker = "<h1 class=\"font-weight-600 alt-font text-white width-95 sm-width-100\">";
    let h1_start = html.find(h1_marker)?;
    let h1_raw = extract_between(html, h1_marker, "</h1>")?;
    let nls_code = normalize_spaces(&strip_tags(h1_raw));
    if nls_code.is_empty() {
        return None;
    }

    let h5_raw = extract_between(&html[h1_start..], "<h5>", "</h5>")?;
    let h5_text = normalize_spaces(&strip_tags(h5_raw));
    let race_title = if let Some((first, rest)) = h5_text.split_once(' ') {
        if first.chars().filter(|c| *c == '.').count() == 2 && !rest.trim().is_empty() {
            rest.trim().to_string()
        } else {
            h5_text
        }
    } else {
        h5_text
    };

    if race_title.is_empty() {
        return None;
    }

    Some(format!("{} - {}", nls_code, race_title))
}

pub(super) fn fetch_homepage_event_name(client: &Client) -> Option<String> {
    let response = client.get(NLS_HOME_URL).send().ok()?;
    let html = response.text().ok()?;
    parse_event_name_from_homepage(&html)
}

fn decode_basic_html_entities(raw: &str) -> String {
    raw.replace("&#8211;", "-")
        .replace("&#8212;", "-")
        .replace("&ndash;", "-")
        .replace("&mdash;", "-")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

pub(super) fn html_to_text_lines(html: &str) -> Vec<String> {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;

    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                text.push('\n');
            }
            '>' => {
                in_tag = false;
                text.push('\n');
            }
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }

    decode_basic_html_entities(&text)
        .lines()
        .map(str::trim)
        .map(normalize_spaces)
        .filter(|line| !line.is_empty())
        .collect()
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn days_in_month(year: i32, month: u32) -> Option<u32> {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
        4 | 6 | 9 | 11 => Some(30),
        2 if is_leap_year(year) => Some(29),
        2 => Some(28),
        _ => None,
    }
}

fn parse_u32_fragment(raw: &str) -> Option<u32> {
    let digits: String = raw.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<u32>().ok()
    }
}

pub(super) fn parse_german_date_range(raw: &str) -> Option<(CalendarDate, CalendarDate)> {
    let normalized = normalize_spaces(&raw.replace(['–', '—', '−'], "-"));
    let (left, right) = normalized.split_once('-')?;

    let start_day = parse_u32_fragment(left)?;
    let mut right_parts = right.trim().split('.').map(str::trim);
    let end_day = parse_u32_fragment(right_parts.next()?)?;
    let month = parse_u32_fragment(right_parts.next()?)?;
    let year = i32::try_from(parse_u32_fragment(right_parts.next()?)?).ok()?;

    let max_day = days_in_month(year, month)?;
    if start_day == 0 || end_day == 0 || start_day > max_day || end_day > max_day {
        return None;
    }

    Some((
        CalendarDate {
            year,
            month,
            day: start_day,
        },
        CalendarDate {
            year,
            month,
            day: end_day,
        },
    ))
}

pub(super) fn extract_date_range_for_event_title(
    lines: &[String],
    target_event_title: &str,
    year: i32,
) -> Option<(CalendarDate, CalendarDate)> {
    let target_idx = lines
        .iter()
        .position(|line| line.contains(target_event_title))?;

    for line in lines.iter().skip(target_idx + 1).take(12) {
        let Some((start, end)) = parse_german_date_range(line) else {
            continue;
        };
        if start.year == year && end.year == year {
            return Some((start, end));
        }
    }

    None
}

pub(super) fn title_matches_24h_qualifiers(title: &str) -> bool {
    let normalized = title.to_ascii_lowercase();
    normalized.contains("24h") && normalized.contains("qualifier")
}

fn qualifiers_active_in_termine_entries(
    entries: &[TermineScheduleEntry],
    today: CalendarDate,
) -> bool {
    entries.iter().any(|entry| {
        today >= entry.start && today <= entry.end && title_matches_24h_qualifiers(&entry.title)
    })
}

fn qualifiers_active_on_nls_termine_page(
    client: &Client,
    today: CalendarDate,
) -> Result<bool, String> {
    let termine_url = discover_termine_url(client)?;
    let response = client
        .get(&termine_url)
        .send()
        .map_err(|err| format!("failed to fetch Termine page: {err}"))?;
    let html = response
        .text()
        .map_err(|err| format!("failed to read Termine page: {err}"))?;

    let entries = parse_termine_entries(&html);
    if entries.is_empty() {
        return Err("failed to parse Termine entries".to_string());
    }

    Ok(qualifiers_active_in_termine_entries(&entries, today))
}

fn date_within_range(today: CalendarDate, range: Option<(CalendarDate, CalendarDate)>) -> bool {
    let Some((start, end)) = range else {
        return false;
    };
    today >= start && today <= end
}

fn local_today() -> Option<CalendarDate> {
    let mut timestamp: libc::time_t = 0;
    unsafe {
        if libc::time(&mut timestamp) < 0 {
            return None;
        }
        let mut local_tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&timestamp, &mut local_tm).is_null() {
            return None;
        }
        Some(CalendarDate {
            year: local_tm.tm_year + 1900,
            month: u32::try_from(local_tm.tm_mon + 1).ok()?,
            day: u32::try_from(local_tm.tm_mday).ok()?,
        })
    }
}

pub(super) fn determine_active_nuerburgring_event_id(
    client: &Client,
) -> Result<&'static str, String> {
    let today = local_today().ok_or_else(|| "failed to resolve local date".to_string())?;

    let qualifiers_result = qualifiers_active_on_nls_termine_page(client, today);
    if let Ok(true) = qualifiers_result {
        return Ok(N24_EVENT_ID);
    }

    let response = client
        .get(N24_TERMINE_URL)
        .send()
        .map_err(|err| format!("failed to fetch 24h schedule: {err}"))?;
    let html = response
        .text()
        .map_err(|err| format!("failed to read 24h schedule: {err}"))?;

    let lines = html_to_text_lines(&html);
    let race_window =
        extract_date_range_for_event_title(&lines, N24_TARGET_EVENT_TITLE, today.year);

    if race_window.is_none() {
        if let Err(err) = qualifiers_result {
            return Err(format!(
                "qualifier check failed ({err}); could not parse {} date range for {}",
                N24_TARGET_EVENT_TITLE, today.year
            ));
        }
        return Err(format!(
            "could not parse {} date range for {}",
            N24_TARGET_EVENT_TITLE, today.year
        ));
    }

    if date_within_range(today, race_window) {
        Ok(N24_EVENT_ID)
    } else {
        Ok(DEFAULT_NLS_EVENT_ID)
    }
}
