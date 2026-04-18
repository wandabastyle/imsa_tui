use crate::timing::TimingEntry;

#[derive(Debug, Clone, Default)]
pub(crate) struct SearchState {
    pub(crate) query: String,
    pub(crate) matches: Vec<usize>,
    pub(crate) current_match: usize,
    pub(crate) input_active: bool,
}

fn entry_matches_search(entry: &TimingEntry, query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return false;
    }

    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return entry.car_number.trim() == trimmed;
    }

    let needle = trimmed.to_ascii_lowercase();
    entry.car_number.to_ascii_lowercase().contains(&needle)
        || entry.driver.to_ascii_lowercase().contains(&needle)
        || entry.vehicle.to_ascii_lowercase().contains(&needle)
        || entry.team.to_ascii_lowercase().contains(&needle)
}

pub(crate) fn refresh_search_matches(search: &mut SearchState, view_entries: &[&TimingEntry]) {
    if search.query.trim().is_empty() {
        search.matches.clear();
        search.current_match = 0;
        return;
    }

    search.matches = view_entries
        .iter()
        .enumerate()
        .filter_map(|(idx, entry)| entry_matches_search(entry, &search.query).then_some(idx))
        .collect();

    if search.matches.is_empty() || search.current_match >= search.matches.len() {
        search.current_match = 0;
    }
}
