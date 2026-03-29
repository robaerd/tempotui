use chrono::{Datelike, NaiveDate, NaiveTime};
use clap::Parser;
use thiserror::Error;

#[derive(Debug, Parser)]
#[command(
    name = "tempotui",
    about = "Review and adjust your monthly Tempo worklog from the terminal."
)]
pub struct Cli {
    #[arg(long, value_name = "YYYY-MM")]
    pub month: Option<String>,

    #[arg(long, value_name = "HH:MM")]
    pub start: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonthWindow {
    pub label: String,
    pub start: NaiveDate,
    pub end: NaiveDate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub today: NaiveDate,
    pub initial_month: MonthWindow,
    pub cli_start_time: Option<NaiveTime>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Couldn't read month `{value}`. Use YYYY-MM.")]
    InvalidMonth { value: String },
    #[error("Couldn't read start time `{value}`. Use HH:MM.")]
    InvalidStartTime { value: String },
}

impl AppConfig {
    pub fn load(cli: Cli, today: NaiveDate) -> Result<Self, ConfigError> {
        let initial_month = match cli.month.as_deref() {
            Some(value) => MonthWindow::from_label(value)?,
            None => MonthWindow::current(today),
        };
        let cli_start_time = cli.start.as_deref().map(parse_start_time).transpose()?;

        Ok(Self {
            today,
            initial_month,
            cli_start_time,
        })
    }
}

impl MonthWindow {
    pub fn current(today: NaiveDate) -> Self {
        Self::from_year_month(today.year(), today.month())
            .expect("current date should always resolve to a valid month window")
    }

    pub fn from_label(value: &str) -> Result<Self, ConfigError> {
        let (year, month) = parse_month_spec(value)?;
        Self::from_year_month(year, month)
    }

    pub fn shift_months(&self, delta: i32) -> Self {
        let month_index = self.start.year() * 12 + self.start.month0() as i32 + delta;
        let year = month_index.div_euclid(12);
        let month = month_index.rem_euclid(12) as u32 + 1;
        Self::from_year_month(year, month)
            .expect("shifted month index should always resolve to a valid month window")
    }

    fn from_year_month(year: i32, month: u32) -> Result<Self, ConfigError> {
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

        Ok(Self {
            label: format!("{year:04}-{month:02}"),
            start,
            end: next_start
                .pred_opt()
                .expect("next month start must have a previous day"),
        })
    }
}

pub fn parse_start_time(value: &str) -> Result<NaiveTime, ConfigError> {
    if value.len() != 5 || value.as_bytes().get(2).is_none_or(|byte| *byte != b':') {
        return Err(ConfigError::InvalidStartTime {
            value: value.to_string(),
        });
    }

    NaiveTime::parse_from_str(value, "%H:%M").map_err(|_| ConfigError::InvalidStartTime {
        value: value.to_string(),
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
        let config = AppConfig::load(
            Cli {
                month: None,
                start: None,
            },
            NaiveDate::from_ymd_opt(2026, 3, 20).unwrap(),
        )
        .unwrap();

        assert_eq!(config.initial_month.label, "2026-03");
        assert_eq!(
            config.initial_month.start,
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()
        );
        assert_eq!(
            config.initial_month.end,
            NaiveDate::from_ymd_opt(2026, 3, 31).unwrap()
        );
    }

    #[test]
    fn parses_specific_month_and_handles_year_boundary() {
        let window = MonthWindow::from_label("2025-12").unwrap();

        assert_eq!(window.start, NaiveDate::from_ymd_opt(2025, 12, 1).unwrap());
        assert_eq!(window.end, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
    }

    #[test]
    fn shifts_month_forward_and_backward() {
        let march = MonthWindow::from_label("2026-03").unwrap();

        assert_eq!(march.shift_months(-1).label, "2026-02");
        assert_eq!(march.shift_months(1).label, "2026-04");
    }

    #[test]
    fn rejects_invalid_month_values() {
        let err = MonthWindow::from_label("2025-13").unwrap_err();

        assert!(err.to_string().contains("Use YYYY-MM"));
    }

    #[test]
    fn parses_valid_start_time() {
        let time = parse_start_time("09:15").unwrap();
        assert_eq!(time, NaiveTime::from_hms_opt(9, 15, 0).unwrap());
    }

    #[test]
    fn rejects_invalid_start_time() {
        let err = parse_start_time("9:15").unwrap_err();
        assert!(err.to_string().contains("Use HH:MM"));
    }
}
