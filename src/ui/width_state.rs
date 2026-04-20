use std::fs;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::timing::{Series, TimingEntry};

use super::{
    imsa_widths::ImsaColumnWidths,
    series_widths::{F1ColumnWidths, NlsColumnWidths},
    table::TableWidthBaselines,
    wec_widths::WecColumnWidths,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct SeriesWidthBaselines {
    persisted: PersistedSeriesWidthBaselines,
    dirty: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PersistedSeriesWidthBaselines {
    imsa: Option<ImsaColumnWidths>,
    nls: Option<NlsColumnWidths>,
    dhlm: Option<NlsColumnWidths>,
    f1: Option<F1ColumnWidths>,
    wec: Option<WecColumnWidths>,
}

impl SeriesWidthBaselines {
    pub(crate) fn load() -> Self {
        let Some(path) = width_baselines_path() else {
            return Self::default();
        };
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        let Ok(persisted) = serde_json::from_str::<PersistedSeriesWidthBaselines>(&text) else {
            return Self::default();
        };
        Self {
            persisted,
            dirty: false,
        }
    }

    pub(crate) fn table_baselines(&self, active_series: Series) -> TableWidthBaselines<'_> {
        let active_nls = match active_series {
            Series::Nls => self.persisted.nls.as_ref(),
            Series::Dhlm => self.persisted.dhlm.as_ref(),
            _ => None,
        };
        TableWidthBaselines {
            imsa: self.persisted.imsa.as_ref(),
            nls: active_nls,
            f1: self.persisted.f1.as_ref(),
            wec: self.persisted.wec.as_ref(),
        }
    }

    pub(crate) fn capture_if_missing(&mut self, series: Series, entries: &[TimingEntry]) {
        if entries.is_empty() {
            return;
        }

        let changed = match series {
            Series::Imsa => {
                if self.persisted.imsa.is_none() {
                    self.persisted.imsa = ImsaColumnWidths::from_entries(entries);
                    self.persisted.imsa.is_some()
                } else {
                    false
                }
            }
            Series::Nls => {
                if self.persisted.nls.is_none() {
                    self.persisted.nls = NlsColumnWidths::from_entries(entries);
                    self.persisted.nls.is_some()
                } else {
                    false
                }
            }
            Series::Dhlm => {
                if self.persisted.dhlm.is_none() {
                    self.persisted.dhlm = NlsColumnWidths::from_entries(entries);
                    self.persisted.dhlm.is_some()
                } else {
                    false
                }
            }
            Series::F1 => {
                if self.persisted.f1.is_none() {
                    self.persisted.f1 = F1ColumnWidths::from_entries(entries);
                    self.persisted.f1.is_some()
                } else {
                    false
                }
            }
            Series::Wec => {
                if self.persisted.wec.is_none() {
                    self.persisted.wec = WecColumnWidths::from_entries(entries);
                    self.persisted.wec.is_some()
                } else {
                    false
                }
            }
        };

        if changed {
            self.dirty = true;
        }
    }

    pub(crate) fn persist_if_dirty(&mut self) {
        if !self.dirty {
            return;
        }
        let Some(path) = width_baselines_path() else {
            self.dirty = false;
            return;
        };
        if let Some(parent) = path.parent() {
            if fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        let Ok(encoded) = serde_json::to_string_pretty(&self.persisted) else {
            return;
        };
        if fs::write(path, encoded).is_ok() {
            self.dirty = false;
        }
    }
}

fn width_baselines_path() -> Option<std::path::PathBuf> {
    let dirs = ProjectDirs::from("", "", "imsa_tui")?;
    Some(dirs.data_local_dir().join("series_column_widths.json"))
}
