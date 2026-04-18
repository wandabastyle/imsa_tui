use std::time::{SystemTime, UNIX_EPOCH};

use crate::timing::TimingHeader;

#[derive(Debug, Clone)]
pub(super) struct CountdownState {
    pub(super) end_time_raw: u64,
    pub(super) time_state_raw: String,
    pub(super) received_at_ms: u64,
    pub(super) is_race_session: bool,
}

pub(super) fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis()
}

fn format_duration_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

pub(super) fn current_time_to_end(
    header: &TimingHeader,
    end_time_raw: u64,
    time_state_raw: &str,
    received_at_ms: u64,
) -> String {
    current_time_to_end_at(
        header,
        end_time_raw,
        time_state_raw,
        received_at_ms,
        now_millis() as u64,
    )
}

pub(super) fn current_time_to_end_at(
    header: &TimingHeader,
    end_time_raw: u64,
    time_state_raw: &str,
    received_at_ms: u64,
    now_ms: u64,
) -> String {
    if end_time_raw == 0 {
        return header.time_to_go.clone();
    }

    let remaining_ms = if time_state_raw == "0" {
        let elapsed = now_ms.saturating_sub(received_at_ms);
        end_time_raw.saturating_sub(elapsed)
    } else {
        end_time_raw.saturating_sub(now_ms)
    };

    format_duration_ms(remaining_ms)
}

pub(super) fn refresh_header_time_to_go(
    header: &mut TimingHeader,
    countdown: Option<&CountdownState>,
) {
    let Some(countdown) = countdown else {
        return;
    };

    header.time_to_go = current_time_to_end(
        header,
        countdown.end_time_raw,
        &countdown.time_state_raw,
        countdown.received_at_ms,
    );

    if should_promote_to_checkered(header, countdown.is_race_session) {
        header.flag = "Checkered".to_string();
    }
}

fn is_zero_time_to_go(value: &str) -> bool {
    let trimmed = value.trim();
    matches!(trimmed, "0" | "0:00" | "00:00" | "00:00:00")
}

fn is_unknown_time_to_go(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.is_empty() || trimmed == "-"
}

pub(super) fn should_promote_to_checkered_with_inputs(
    flag: &str,
    time_to_go: &str,
    is_race_session: bool,
) -> bool {
    let normalized_flag = flag.trim();
    let flag_is_promotable =
        normalized_flag == "-" || normalized_flag.eq_ignore_ascii_case("green");
    if !flag_is_promotable {
        return false;
    }

    (is_zero_time_to_go(time_to_go) || is_unknown_time_to_go(time_to_go)) && is_race_session
}

pub(super) fn should_promote_to_checkered(header: &TimingHeader, is_race_session: bool) -> bool {
    should_promote_to_checkered_with_inputs(&header.flag, &header.time_to_go, is_race_session)
}
