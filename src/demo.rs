// Deterministic demo snapshots used for UI development without live network feeds.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use crate::timing::{Series, TimingEntry, TimingHeader};

pub fn demo_snapshot(series: Series) -> (TimingHeader, Vec<TimingEntry>) {
    match series {
        Series::Imsa => (imsa_header(), imsa_entries()),
        Series::Nls => (nls_header(), nls_entries()),
        Series::F1 => (f1_header(), f1_entries()),
        Series::Wec => (wec_header(), wec_entries()),
    }
}

pub fn demo_snapshot_at(
    series: Series,
    seed: u64,
    elapsed_secs: u64,
) -> (TimingHeader, Vec<TimingEntry>) {
    let (mut header, mut entries) = demo_snapshot(series);

    let flag_names = ["Green", "Yellow", "Red", "White", "Checkered"];
    let flag_idx =
        ((elapsed_secs / 45) as usize + (seed as usize % flag_names.len())) % flag_names.len();
    header.flag = flag_names[flag_idx].to_string();

    match series {
        Series::F1 => {
            let base_lap = 34_u64;
            let lap = base_lap.saturating_add(elapsed_secs / 12).min(57);
            header.session_name = "Race".to_string();
            header.time_to_go = format!("Lap {lap}/57");
        }
        _ => {
            let hours = 6_u64.saturating_sub((elapsed_secs / 600).min(6));
            let mins = (45_u64 + ((elapsed_secs / 17) % 15)) % 60;
            let secs = (12_u64 + ((elapsed_secs + seed) % 48)) % 60;
            header.time_to_go = format!("{hours:02}:{mins:02}:{secs:02}");
        }
    }

    for (idx, entry) in entries.iter_mut().enumerate() {
        if let Some(base_laps) = parse_laps(&entry.laps) {
            let pace = if idx == 0 { 22 } else { 24 + (idx as u64 % 4) };
            entry.laps = base_laps.saturating_add(elapsed_secs / pace).to_string();
        }

        apply_demo_pit_state(series, entry, seed, elapsed_secs, idx as u64);

        if idx == 0 {
            entry.gap_overall = "-".to_string();
            entry.gap_class = "-".to_string();
            entry.gap_next_in_class = "-".to_string();
            continue;
        }

        let movement = (((elapsed_secs / 8) + seed + idx as u64) % 30) as f32 / 10.0;
        let base = idx as f32 * 2.3;
        let gap = base + movement;
        let gap_text = format!("+{gap:.3}");
        entry.gap_overall = gap_text.clone();
        entry.gap_class = gap_text;
        entry.gap_next_in_class = format!("+{:.3}", 1.1 + movement / 2.0);
    }

    (header, entries)
}

fn parse_laps(raw: &str) -> Option<u64> {
    let digits: String = raw.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u64>().ok()
}

fn apply_demo_pit_state(
    series: Series,
    entry: &mut TimingEntry,
    seed: u64,
    elapsed_secs: u64,
    row_idx: u64,
) {
    let lane = stable_lane_seed(seed, &entry.stable_id, row_idx);
    let cycle_secs = 180_u64 + (lane % 240);
    let pit_duration_secs = 18_u64 + ((lane / 7) % 18);
    let phase_offset = lane % cycle_secs;
    let in_pit = (elapsed_secs + phase_offset) % cycle_secs < pit_duration_secs;

    let extra_stops = (elapsed_secs + phase_offset) / cycle_secs;
    let base_stops = parse_stop_count(&entry.pit_stops).unwrap_or(0);
    entry.pit_stops = base_stops.saturating_add(extra_stops).to_string();

    match series {
        Series::Nls => {
            entry.pit = if in_pit { "Yes" } else { "No" }.to_string();
            entry.sector_5 = if in_pit {
                "PIT".to_string()
            } else {
                demo_nls_sector_5_time(lane, elapsed_secs)
            };
        }
        Series::Imsa | Series::F1 | Series::Wec => {
            entry.pit = if in_pit { "Yes" } else { "No" }.to_string();
        }
    }
}

fn demo_nls_sector_5_time(lane: u64, elapsed_secs: u64) -> String {
    let base_secs = 92.0 + ((lane % 17) as f32) * 0.7;
    let wobble = ((elapsed_secs % 19) as f32) * 0.031;
    format!("{:.3}", base_secs + wobble)
}

fn stable_lane_seed(seed: u64, stable_id: &str, row_idx: u64) -> u64 {
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    stable_id.hash(&mut hasher);
    row_idx.hash(&mut hasher);
    hasher.finish()
}

fn parse_stop_count(raw: &str) -> Option<u64> {
    raw.trim().parse::<u64>().ok()
}

pub fn seed_demo_favourites(series: Series, favourites: &mut HashSet<String>) {
    for stable_id in demo_favourite_ids(series) {
        favourites.insert(format!("{}|{}", series.as_key_prefix(), stable_id));
    }
}

fn demo_favourite_ids(series: Series) -> &'static [&'static str] {
    match series {
        Series::Imsa => &["imsa:7", "imsa:31", "imsa:77"],
        Series::Nls => &["nls:911", "nls:27", "nls:18"],
        Series::F1 => &["f1:driver:1", "f1:driver:16", "f1:driver:4"],
        Series::Wec => &["wec:50", "wec:6", "wec:83"],
    }
}

fn header(
    event_name: &str,
    session_name: &str,
    track_name: &str,
    day_time: &str,
    flag: &str,
    time_to_go: &str,
) -> TimingHeader {
    TimingHeader {
        event_name: event_name.to_string(),
        session_name: session_name.to_string(),
        track_name: track_name.to_string(),
        day_time: day_time.to_string(),
        flag: flag.to_string(),
        time_to_go: time_to_go.to_string(),
    }
}

macro_rules! entry {
    (
        $position:expr,
        $car_number:expr,
        $class_name:expr,
        $class_rank:expr,
        $driver:expr,
        $vehicle:expr,
        $team:expr,
        $laps:expr,
        $gap_overall:expr,
        $gap_class:expr,
        $gap_next_in_class:expr,
        $last_lap:expr,
        $best_lap:expr,
        $best_lap_no:expr,
        $pit:expr,
        $pit_stops:expr,
        $fastest_driver:expr,
        $stable_id:expr $(,)?
    ) => {
        TimingEntry {
            position: $position,
            car_number: $car_number.to_string(),
            class_name: $class_name.to_string(),
            class_rank: $class_rank.to_string(),
            driver: $driver.to_string(),
            vehicle: $vehicle.to_string(),
            team: $team.to_string(),
            laps: $laps.to_string(),
            gap_overall: $gap_overall.to_string(),
            gap_class: $gap_class.to_string(),
            gap_next_in_class: $gap_next_in_class.to_string(),
            last_lap: $last_lap.to_string(),
            best_lap: $best_lap.to_string(),
            sector_1: "-".to_string(),
            sector_2: "-".to_string(),
            sector_3: "-".to_string(),
            sector_4: "-".to_string(),
            sector_5: "-".to_string(),
            best_lap_no: $best_lap_no.to_string(),
            pit: $pit.to_string(),
            pit_stops: $pit_stops.to_string(),
            fastest_driver: $fastest_driver.to_string(),
            stable_id: $stable_id.to_string(),
        }
    };
}

fn imsa_header() -> TimingHeader {
    header(
        "Rolex 24 At Daytona (Demo)",
        "Race - Hour 17",
        "Daytona International Speedway",
        "Sat 22:14",
        "Green",
        "06:45:12",
    )
}

fn nls_header() -> TimingHeader {
    header(
        "ADAC Ruhrpott Trophy (Demo)",
        "Race - Lap 22",
        "Nürburgring Nordschleife",
        "Sat 15:06",
        "Yellow",
        "02:13:47",
    )
}

fn f1_header() -> TimingHeader {
    header(
        "FORMULA 1 GRAND PRIX (Demo)",
        "Race",
        "Bahrain International Circuit",
        "Lap 34/57",
        "Green",
        "00:28:14",
    )
}

fn wec_header() -> TimingHeader {
    header(
        "6 Hours of Spa-Francorchamps (Demo)",
        "Race",
        "Circuit de Spa-Francorchamps",
        "Sat 14:36",
        "Green",
        "03:22:19",
    )
}

fn wec_entries() -> Vec<TimingEntry> {
    vec![
        entry!(
            1,
            "50",
            "LMH",
            "1",
            "A. Fuoco",
            "Ferrari 499P",
            "Ferrari AF Corse",
            "112",
            "-",
            "-",
            "-",
            "2:03.402",
            "2:02.998",
            "61",
            "No",
            "4",
            "-",
            "wec:50",
        ),
        entry!(
            2,
            "6",
            "LMH",
            "2",
            "K. Estre",
            "Porsche 963",
            "Porsche Penske Motorsport",
            "112",
            "+2.146",
            "+2.146",
            "+2.146",
            "2:03.511",
            "2:03.084",
            "58",
            "No",
            "4",
            "-",
            "wec:6",
        ),
        entry!(
            3,
            "83",
            "LMGT3",
            "1",
            "Y. Shahin",
            "Lexus RC F GT3",
            "Akkodis ASP Team",
            "110",
            "+1L",
            "-",
            "-",
            "2:19.904",
            "2:19.221",
            "43",
            "No",
            "5",
            "-",
            "wec:83",
        ),
    ]
}

fn f1_entries() -> Vec<TimingEntry> {
    vec![
        entry!(
            1,
            "1",
            "F1",
            "1",
            "M. Verstappen",
            "-",
            "Red Bull Racing",
            "34",
            "-",
            "-",
            "-",
            "1:35.221",
            "1:34.882",
            "-",
            "No",
            "1",
            "-",
            "f1:driver:1",
        ),
        entry!(
            2,
            "16",
            "F1",
            "2",
            "C. Leclerc",
            "-",
            "Ferrari",
            "34",
            "+3.201",
            "+3.201",
            "-",
            "1:35.608",
            "1:35.144",
            "-",
            "No",
            "1",
            "-",
            "f1:driver:16",
        ),
        entry!(
            3,
            "4",
            "F1",
            "3",
            "L. Norris",
            "-",
            "McLaren",
            "34",
            "+7.991",
            "+4.790",
            "-",
            "1:35.844",
            "1:35.337",
            "-",
            "No",
            "1",
            "-",
            "f1:driver:4",
        ),
    ]
}

fn imsa_entries() -> Vec<TimingEntry> {
    vec![
        entry!(
            1,
            "7",
            "GTP",
            "1",
            "F. Nasr",
            "Porsche 963",
            "Porsche Penske",
            "412",
            "-",
            "-",
            "-",
            "1:36.234",
            "1:35.991",
            "378",
            "OUT",
            "8",
            "M. Campbell",
            "imsa:7",
        ),
        entry!(
            2,
            "31",
            "GTP",
            "2",
            "J. Aitken",
            "Cadillac V-Series.R",
            "Whelen Engineering",
            "412",
            "+2.181",
            "+2.181",
            "+2.181",
            "1:36.481",
            "1:36.102",
            "364",
            "OUT",
            "9",
            "J. Aitken",
            "imsa:31",
        ),
        entry!(
            3,
            "01",
            "GTP",
            "3",
            "S. Bourdais",
            "Cadillac V-Series.R",
            "Cadillac Racing",
            "411",
            "+1 Lap",
            "+1 Lap",
            "+14.332",
            "1:36.707",
            "1:36.255",
            "359",
            "PIT",
            "10",
            "R. van der Zande",
            "imsa:01",
        ),
        entry!(
            4,
            "24",
            "GTP",
            "4",
            "R. Taylor",
            "BMW M Hybrid V8",
            "BMW M Team RLL",
            "410",
            "+2 Laps",
            "+2 Laps",
            "+7.884",
            "1:37.102",
            "1:36.803",
            "311",
            "OUT",
            "9",
            "P. Eng",
            "imsa:24",
        ),
        entry!(
            5,
            "10",
            "GTP",
            "5",
            "F. Albuquerque",
            "Acura ARX-06",
            "Konica Minolta Acura",
            "409",
            "+3 Laps",
            "+3 Laps",
            "+23.419",
            "1:37.220",
            "1:36.774",
            "288",
            "OUT",
            "8",
            "R. Taylor",
            "imsa:10",
        ),
        entry!(
            6,
            "52",
            "LMP2",
            "1",
            "T. Dillmann",
            "ORECA 07 Gibson",
            "PR1/Mathiasen",
            "406",
            "+6 Laps",
            "-",
            "-",
            "1:39.112",
            "1:38.804",
            "344",
            "OUT",
            "9",
            "M. Jensen",
            "imsa:52",
        ),
        entry!(
            7,
            "18",
            "LMP2",
            "2",
            "D. Goldburg",
            "ORECA 07 Gibson",
            "Era Motorsport",
            "405",
            "+7 Laps",
            "+1 Lap",
            "+1 Lap",
            "1:39.984",
            "1:39.201",
            "276",
            "PIT",
            "10",
            "C. Rasmussen",
            "imsa:18",
        ),
        entry!(
            8,
            "11",
            "LMP2",
            "3",
            "S. Thomas",
            "ORECA 07 Gibson",
            "TDS Racing",
            "404",
            "+8 Laps",
            "+2 Laps",
            "+16.980",
            "1:40.125",
            "1:39.648",
            "241",
            "OUT",
            "8",
            "M. Beche",
            "imsa:11",
        ),
        entry!(
            9,
            "77",
            "GTD PRO",
            "1",
            "K. Bachler",
            "Porsche 911 GT3 R",
            "AO Racing - Rexy",
            "402",
            "+10 Laps",
            "-",
            "-",
            "1:46.832",
            "1:46.291",
            "319",
            "OUT",
            "11",
            "L. Heinrich",
            "imsa:77",
        ),
        entry!(
            10,
            "3",
            "GTD PRO",
            "2",
            "A. Sims",
            "Corvette Z06 GT3.R",
            "Corvette Racing by Pratt Miller",
            "402",
            "+10 Laps",
            "+11.244",
            "+11.244",
            "1:47.214",
            "1:46.602",
            "297",
            "PIT",
            "12",
            "N. Catsburg",
            "imsa:3",
        ),
        entry!(
            11,
            "62",
            "GTD PRO",
            "3",
            "D. Serra",
            "Ferrari 296 GT3",
            "Risi Competizione",
            "401",
            "+11 Laps",
            "+1 Lap",
            "+38.507",
            "1:47.951",
            "1:47.009",
            "221",
            "OUT",
            "10",
            "A. Pier Guidi",
            "imsa:62",
        ),
        entry!(
            12,
            "27",
            "GTD",
            "1",
            "R. De Angelis",
            "Aston Martin Vantage GT3",
            "Heart of Racing Team",
            "400",
            "+12 Laps",
            "-",
            "-",
            "1:48.560",
            "1:48.022",
            "305",
            "OUT",
            "11",
            "Z. Robichon",
            "imsa:27",
        ),
        entry!(
            13,
            "120",
            "GTD",
            "2",
            "R. Foley",
            "BMW M4 GT3",
            "Turner Motorsport With Extra-Long Team Name",
            "399",
            "+13 Laps",
            "+1 Lap",
            "+9.102",
            "1:49.318",
            "1:48.417",
            "198",
            "PIT",
            "12",
            "P. Gallagher",
            "imsa:120",
        ),
        entry!(
            14,
            "32",
            "GTD",
            "3",
            "M. Skeen",
            "Mercedes-AMG GT3",
            "Korthoff Preston Motorsports",
            "399",
            "+13 Laps",
            "+1 Lap",
            "+0.442",
            "1:49.504",
            "1:48.635",
            "254",
            "OUT",
            "10",
            "D. Dontje",
            "imsa:32",
        ),
    ]
}

fn nls_entries() -> Vec<TimingEntry> {
    vec![
        entry!(
            1,
            "911",
            "SP9",
            "1",
            "K. Estre",
            "Porsche 911 GT3 R",
            "Manthey EMA",
            "22",
            "-",
            "-",
            "-",
            "7:59.334",
            "7:57.102",
            "-",
            "-",
            "-",
            "-",
            "nls:911",
        ),
        entry!(
            2,
            "17",
            "SP9",
            "2",
            "M. Engel",
            "Mercedes-AMG GT3",
            "GetSpeed Performance",
            "22",
            "+4.223",
            "-",
            "-",
            "8:00.210",
            "7:58.001",
            "-",
            "-",
            "-",
            "-",
            "nls:17",
        ),
        entry!(
            3,
            "98",
            "SP9",
            "3",
            "S. van der Linde",
            "BMW M4 GT3",
            "ROWE Racing",
            "22",
            "+8.551",
            "-",
            "-",
            "8:00.817",
            "7:58.643",
            "-",
            "-",
            "-",
            "-",
            "nls:98",
        ),
        entry!(
            4,
            "54",
            "SP9",
            "4",
            "F. Schiller",
            "Ferrari 296 GT3",
            "Realize Kondo Racing with Rinaldi",
            "22",
            "+15.840",
            "-",
            "-",
            "8:01.424",
            "7:59.221",
            "-",
            "-",
            "-",
            "-",
            "nls:54",
        ),
        entry!(
            5,
            "44",
            "SP9",
            "5",
            "A. Mies",
            "Audi R8 LMS GT3 evo II",
            "Scherer Sport PHX",
            "21",
            "+1 Lap",
            "-",
            "-",
            "8:02.317",
            "7:59.772",
            "-",
            "-",
            "-",
            "-",
            "nls:44",
        ),
        entry!(
            6,
            "27",
            "SP10",
            "1",
            "J. Klingmann",
            "BMW M4 GT4",
            "FK Performance Motorsport",
            "21",
            "+1 Lap",
            "-",
            "-",
            "8:23.288",
            "8:20.141",
            "-",
            "-",
            "-",
            "-",
            "nls:27",
        ),
        entry!(
            7,
            "160",
            "SP10",
            "2",
            "L. Erhart",
            "Porsche 718 Cayman GT4 RS CS",
            "BLACK FALCON Team EAE",
            "21",
            "+1 Lap",
            "-",
            "-",
            "8:24.021",
            "8:20.914",
            "-",
            "-",
            "-",
            "-",
            "nls:160",
        ),
        entry!(
            8,
            "50",
            "VT2-RWD",
            "1",
            "D. Bohrer",
            "BMW 330i",
            "Adrenalin Motorsport Team Mainhattan Wheels",
            "20",
            "+2 Laps",
            "-",
            "-",
            "9:11.447",
            "9:05.234",
            "-",
            "-",
            "-",
            "-",
            "nls:50",
        ),
        entry!(
            9,
            "500",
            "VT2-RWD",
            "2",
            "P. Marx",
            "Toyota GR86 Cup",
            "Ring Racing Junior Squad",
            "20",
            "+2 Laps",
            "-",
            "-",
            "9:13.029",
            "9:06.812",
            "-",
            "-",
            "-",
            "-",
            "nls:500",
        ),
        entry!(
            10,
            "18",
            "Cup2",
            "1",
            "C. Krohn",
            "Porsche 911 GT3 Cup (992)",
            "Mühlner Motorsport",
            "20",
            "+2 Laps",
            "-",
            "-",
            "8:35.430",
            "8:30.220",
            "-",
            "-",
            "-",
            "-",
            "nls:18",
        ),
        entry!(
            11,
            "970",
            "Cup2",
            "2",
            "M. Oeverhaus",
            "Porsche 911 GT3 Cup (992)",
            "SRS Team Sorg Rennsport",
            "20",
            "+2 Laps",
            "-",
            "-",
            "8:36.004",
            "8:30.801",
            "-",
            "-",
            "-",
            "-",
            "nls:970",
        ),
        entry!(
            12,
            "608",
            "SP8T",
            "1",
            "N. Verdonck",
            "Toyota Supra GT4 EVO2",
            "Teichmann Racing With Very Long Team Name",
            "19",
            "+3 Laps",
            "-",
            "-",
            "8:49.312",
            "8:44.762",
            "-",
            "-",
            "-",
            "-",
            "nls:608",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imsa_demo_uses_yes_no_for_pit_signal() {
        let (_, entries) = demo_snapshot_at(Series::Imsa, 99, 240);
        assert!(!entries.is_empty());
        assert!(entries
            .iter()
            .all(|entry| entry.pit == "Yes" || entry.pit == "No"));
    }

    #[test]
    fn nls_demo_sector_5_is_pit_or_time() {
        let mut saw_pit = false;
        let mut saw_time = false;

        for t in (0..=1800).step_by(10) {
            let (_, entries) = demo_snapshot_at(Series::Nls, 7, t);
            for entry in &entries {
                if entry.sector_5 == "PIT" {
                    saw_pit = true;
                }
                if entry.sector_5 != "PIT"
                    && !entry.sector_5.is_empty()
                    && entry.sector_5 != "-"
                    && entry
                        .sector_5
                        .chars()
                        .all(|ch| ch.is_ascii_digit() || ch == '.')
                {
                    saw_time = true;
                }
            }
            if saw_pit && saw_time {
                break;
            }
        }

        assert!(saw_pit);
        assert!(saw_time);
    }
}
