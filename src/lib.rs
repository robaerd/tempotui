mod config;
mod report;
mod tempo;

use std::{
    io::{self, Write},
    path::Path,
};

use chrono::Local;
use clap::Parser;
use thiserror::Error;

use config::{AppConfig, Cli};
use report::{MonthlyReport, render_report};
use tempo::TempoClient;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Failed to load `.env`: {0}")]
    Dotenv(#[source] dotenvy::Error),
    #[error(transparent)]
    Config(#[from] config::ConfigError),
    #[error(transparent)]
    Tempo(#[from] tempo::TempoError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

pub fn run() -> Result<(), AppError> {
    load_dotenv_from_path(".env")?;

    let cli = Cli::parse();
    let today = Local::now().date_naive();
    let config = AppConfig::load(cli, today)?;

    let client = TempoClient::new(config.base_url.clone(), config.tempo_api_token.clone())?;
    let worklogs = client.fetch_worklogs_for_user(
        &config.tempo_account_id,
        config.month.start,
        config.month.end,
    )?;

    let report = MonthlyReport::from_worklogs(
        config.month.label,
        config.month.start,
        config.month.end,
        config.start_time,
        worklogs,
    );

    let mut stdout = io::stdout().lock();
    stdout.write_all(render_report(&report).as_bytes())?;
    Ok(())
}

fn load_dotenv_from_path(path: impl AsRef<Path>) -> Result<(), AppError> {
    match dotenvy::from_path(path) {
        Ok(()) => Ok(()),
        Err(err) if err.not_found() => Ok(()),
        Err(err) => Err(AppError::Dotenv(err)),
    }
}

pub mod prelude {
    pub use crate::config::{AppConfig, Cli, MonthWindow};
    pub use crate::report::{
        BREAK_DURATION_SECONDS, BREAK_THRESHOLD_SECONDS, MonthlyReport, ReportRow, ReportTotals,
        build_report_rows, format_clock_time, format_duration, render_report,
        statutory_break_seconds,
    };
    pub use crate::tempo::{TempoClient, TempoWorklog};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn unique_temp_path(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "tempo-log-{name}-{}-{unique}.env",
            std::process::id()
        ))
    }

    #[test]
    fn dotenv_loader_ignores_missing_file() {
        let path = unique_temp_path("missing");
        assert!(load_dotenv_from_path(&path).is_ok());
    }

    #[test]
    fn dotenv_loader_surfaces_parse_errors() {
        let path = unique_temp_path("invalid");
        fs::write(&path, "NOT A VALID .ENV LINE\n").unwrap();

        let err = load_dotenv_from_path(&path).unwrap_err();

        assert!(err.to_string().contains("Failed to load `.env`"));
        fs::remove_file(path).unwrap();
    }
}
