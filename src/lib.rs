mod config;
mod http;
mod jira;
mod report;
mod storage;
mod tempo;
mod tui;

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Europe::Vienna;
use clap::Parser;
use thiserror::Error;

use config::{AppConfig, Cli};
use storage::AppStateStore;

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Config(#[from] config::ConfigError),
    #[error(transparent)]
    Storage(#[from] storage::StorageError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn run() -> Result<(), AppError> {
    let cli = Cli::parse();
    let today = austrian_today(Utc::now());
    let config = AppConfig::load(cli, today)?;
    let store = AppStateStore::from_default_location()?;
    let persisted = store.load()?;

    tui::run(config, store, persisted)?;
    Ok(())
}

fn austrian_today(now_utc: DateTime<Utc>) -> NaiveDate {
    // Anchor month defaults to Austria's local date so month selection stays stable even when the
    // host machine is configured for another timezone.
    now_utc.with_timezone(&Vienna).date_naive()
}

pub mod prelude {
    pub use crate::config::{AppConfig, Cli, MonthWindow, parse_start_time};
    pub use crate::jira::{JiraClient, JiraError};
    pub use crate::report::{
        BREAK_DURATION_SECONDS, BREAK_THRESHOLD_SECONDS, MonthlyReport, ReportRow, ReportTotals,
        build_report_rows, format_clock_time, format_duration, render_report,
        statutory_break_seconds,
    };
    pub use crate::storage::{
        AppStateStore, DEFAULT_TEMPO_BASE_URL, EmptyDayTimeDisplay, JiraSettings, PersistedState,
        Preferences, StorageError, TempoSettings,
    };
    pub use crate::tempo::{TempoClient, TempoWorklog};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn austrian_today_uses_vienna_calendar_day() {
        let now = DateTime::parse_from_rfc3339("2026-03-31T22:30:00Z")
            .unwrap()
            .with_timezone(&Utc);

        assert_eq!(
            austrian_today(now),
            chrono::NaiveDate::from_ymd_opt(2026, 4, 1).unwrap()
        );
    }
}
