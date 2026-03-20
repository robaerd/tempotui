use std::env;

use chrono::{Datelike, NaiveDate, NaiveTime};
use clap::Parser;
use thiserror::Error;

const DEFAULT_BASE_URL: &str = "https://api.eu.tempo.io";

#[derive(Debug, Parser)]
#[command(
    name = "tempo-log",
    about = "Print a monthly Tempo report with synthetic start/end times and statutory break handling."
)]
pub struct Cli {
    #[arg(long, value_name = "YYYY-MM")]
    pub month: Option<String>,

    #[arg(long, default_value = "09:00", value_name = "HH:MM")]
    pub start: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonthWindow {
    pub label: String,
    pub start: NaiveDate,
    pub end: NaiveDate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub tempo_api_token: String,
    pub tempo_account_id: String,
    pub base_url: String,
    pub month: MonthWindow,
    pub start_time: NaiveTime,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Missing required environment variable `{key}`.")]
    MissingEnv { key: &'static str },
    #[error("Environment variable `{key}` must not be empty.")]
    EmptyEnv { key: &'static str },
    #[error("Invalid month `{value}`. Expected the format `YYYY-MM`.")]
    InvalidMonth { value: String },
    #[error("Invalid start time `{value}`. Expected the format `HH:MM`.")]
    InvalidStartTime { value: String },
}

impl AppConfig {
    pub fn load(cli: Cli, today: NaiveDate) -> Result<Self, ConfigError> {
        let tempo_api_token = required_env("TEMPO_API_TOKEN")?;
        let tempo_account_id = required_env("TEMPO_ACCOUNT_ID")?;
        let base_url =
            optional_env("TEMPO_BASE_URL").unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let month = resolve_month_window(cli.month.as_deref(), today)?;
        let start_time = parse_start_time(&cli.start)?;

        Ok(Self {
            tempo_api_token,
            tempo_account_id,
            base_url,
            month,
            start_time,
        })
    }
}

fn required_env(key: &'static str) -> Result<String, ConfigError> {
    let value = env::var(key).map_err(|_| ConfigError::MissingEnv { key })?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ConfigError::EmptyEnv { key });
    }
    Ok(trimmed.to_string())
}

fn optional_env(key: &'static str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
}

fn parse_start_time(value: &str) -> Result<NaiveTime, ConfigError> {
    if value.len() != 5 || value.as_bytes().get(2).is_none_or(|byte| *byte != b':') {
        return Err(ConfigError::InvalidStartTime {
            value: value.to_string(),
        });
    }

    NaiveTime::parse_from_str(value, "%H:%M").map_err(|_| ConfigError::InvalidStartTime {
        value: value.to_string(),
    })
}

fn resolve_month_window(
    month_arg: Option<&str>,
    today: NaiveDate,
) -> Result<MonthWindow, ConfigError> {
    let (year, month) = match month_arg {
        Some(value) => parse_month_spec(value)?,
        None => (today.year(), today.month()),
    };

    let start =
        NaiveDate::from_ymd_opt(year, month, 1).ok_or_else(|| ConfigError::InvalidMonth {
            value: format!("{year:04}-{month:02}"),
        })?;

    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };

    let next_start = NaiveDate::from_ymd_opt(next_year, next_month, 1).ok_or_else(|| {
        ConfigError::InvalidMonth {
            value: format!("{year:04}-{month:02}"),
        }
    })?;
    let end = next_start
        .pred_opt()
        .expect("next month start must have a previous day");

    Ok(MonthWindow {
        label: format!("{year:04}-{month:02}"),
        start,
        end,
    })
}

fn parse_month_spec(value: &str) -> Result<(i32, u32), ConfigError> {
    if value.len() != 7 || value.as_bytes().get(4).is_none_or(|byte| *byte != b'-') {
        return Err(ConfigError::InvalidMonth {
            value: value.to_string(),
        });
    }

    let year = value[0..4]
        .parse::<i32>()
        .map_err(|_| ConfigError::InvalidMonth {
            value: value.to_string(),
        })?;
    let month = value[5..7]
        .parse::<u32>()
        .map_err(|_| ConfigError::InvalidMonth {
            value: value.to_string(),
        })?;

    if !(1..=12).contains(&month) {
        return Err(ConfigError::InvalidMonth {
            value: value.to_string(),
        });
    }

    Ok((year, month))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_current_month_when_no_argument_is_given() {
        let window =
            resolve_month_window(None, NaiveDate::from_ymd_opt(2026, 3, 20).unwrap()).unwrap();

        assert_eq!(window.label, "2026-03");
        assert_eq!(window.start, NaiveDate::from_ymd_opt(2026, 3, 1).unwrap());
        assert_eq!(window.end, NaiveDate::from_ymd_opt(2026, 3, 31).unwrap());
    }

    #[test]
    fn parses_specific_month_and_handles_year_boundary() {
        let window = resolve_month_window(
            Some("2025-12"),
            NaiveDate::from_ymd_opt(2026, 3, 20).unwrap(),
        )
        .unwrap();

        assert_eq!(window.start, NaiveDate::from_ymd_opt(2025, 12, 1).unwrap());
        assert_eq!(window.end, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
    }

    #[test]
    fn rejects_invalid_month_values() {
        let err = resolve_month_window(
            Some("2025-13"),
            NaiveDate::from_ymd_opt(2026, 3, 20).unwrap(),
        )
        .unwrap_err();

        assert!(err.to_string().contains("Expected the format `YYYY-MM`"));
    }

    #[test]
    fn parses_valid_start_time() {
        let time = parse_start_time("09:15").unwrap();
        assert_eq!(time, NaiveTime::from_hms_opt(9, 15, 0).unwrap());
    }

    #[test]
    fn rejects_invalid_start_time() {
        let err = parse_start_time("9:15").unwrap_err();
        assert!(err.to_string().contains("HH:MM"));
    }
}
