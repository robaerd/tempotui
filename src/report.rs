use std::collections::{BTreeMap, BTreeSet};

use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Timelike, Weekday};
use comfy_table::{
    Attribute, Cell, CellAlignment, ContentArrangement, Table, presets::UTF8_FULL_CONDENSED,
};

use crate::tempo::TempoWorklog;

pub const BREAK_THRESHOLD_SECONDS: i64 = 6 * 60 * 60;
pub const BREAK_DURATION_SECONDS: i64 = 30 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportRow {
    pub date: NaiveDate,
    pub worked_seconds: i64,
    pub break_seconds: i64,
    pub tracked_seconds: i64,
    pub effective_start_seconds: i64,
    pub effective_end_seconds: i64,
    pub has_override: bool,
    pub is_empty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReportTotals {
    pub worked_seconds: i64,
    pub break_seconds: i64,
    pub tracked_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonthlyReport {
    pub month_label: String,
    pub range_start: NaiveDate,
    pub range_end: NaiveDate,
    pub default_start_time: NaiveTime,
    pub rows: Vec<ReportRow>,
    pub totals: ReportTotals,
}

impl MonthlyReport {
    pub fn from_worklogs(
        month_label: String,
        range_start: NaiveDate,
        range_end: NaiveDate,
        default_start_time: NaiveTime,
        show_empty_weekdays: bool,
        day_overrides: &BTreeMap<NaiveDate, NaiveTime>,
        worklogs: &[TempoWorklog],
    ) -> Self {
        let rows = build_report_rows(
            worklogs,
            range_start,
            range_end,
            default_start_time,
            show_empty_weekdays,
            day_overrides,
        );
        let totals = rows
            .iter()
            .fold(ReportTotals::default(), |mut totals, row| {
                totals.worked_seconds += row.worked_seconds;
                totals.break_seconds += row.break_seconds;
                totals.tracked_seconds += row.tracked_seconds;
                totals
            });

        Self {
            month_label,
            range_start,
            range_end,
            default_start_time,
            rows,
            totals,
        }
    }
}

pub fn build_report_rows(
    worklogs: &[TempoWorklog],
    range_start: NaiveDate,
    range_end: NaiveDate,
    default_start_time: NaiveTime,
    show_empty_weekdays: bool,
    day_overrides: &BTreeMap<NaiveDate, NaiveTime>,
) -> Vec<ReportRow> {
    let mut worked_by_day = BTreeMap::<NaiveDate, i64>::new();
    for worklog in worklogs {
        *worked_by_day.entry(worklog.start_date).or_default() += worklog.time_spent_seconds;
    }

    let mut visible_dates = BTreeSet::<NaiveDate>::new();
    if show_empty_weekdays {
        let mut date = range_start;
        while date <= range_end {
            if is_weekday(date.weekday()) {
                visible_dates.insert(date);
            }
            date += Duration::days(1);
        }
    }

    visible_dates.extend(worked_by_day.keys().copied());

    visible_dates
        .into_iter()
        .map(|date| {
            let worked_seconds = worked_by_day.get(&date).copied().unwrap_or_default();
            let break_seconds = statutory_break_seconds(worked_seconds);
            let tracked_seconds = worked_seconds + break_seconds;
            let effective_start_time = day_overrides
                .get(&date)
                .copied()
                .unwrap_or(default_start_time);
            let effective_start_seconds =
                i64::from(effective_start_time.num_seconds_from_midnight());

            ReportRow {
                date,
                worked_seconds,
                break_seconds,
                tracked_seconds,
                effective_start_seconds,
                effective_end_seconds: effective_start_seconds + tracked_seconds,
                has_override: day_overrides.contains_key(&date),
                is_empty: worked_seconds == 0,
            }
        })
        .collect()
}

pub fn statutory_break_seconds(worked_seconds: i64) -> i64 {
    if worked_seconds > BREAK_THRESHOLD_SECONDS {
        BREAK_DURATION_SECONDS
    } else {
        0
    }
}

pub fn render_report(report: &MonthlyReport) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "Tempo monthly span report for {}\nRange: {} to {}\nDefault start time: {}\nBreak rule: add {} when worked > {}\n\n",
        report.month_label,
        report.range_start,
        report.range_end,
        report.default_start_time.format("%H:%M"),
        format_duration(BREAK_DURATION_SECONDS),
        format_duration(BREAK_THRESHOLD_SECONDS),
    ));

    if report.rows.is_empty() {
        output.push_str("No rows available for this period.\n");
        return output;
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        header("Date"),
        header("Day"),
        header("Worked"),
        header("Break"),
        header("Tracked"),
        header("Start"),
        header("End"),
        header("Ov"),
    ]);

    for row in &report.rows {
        table.add_row(vec![
            Cell::new(row.date.format("%Y-%m-%d").to_string()),
            Cell::new(row.date.format("%a").to_string()),
            numeric_cell(format_duration(row.worked_seconds)),
            numeric_cell(format_duration(row.break_seconds)),
            numeric_cell(format_duration(row.tracked_seconds)),
            numeric_cell(if row.is_empty {
                String::new()
            } else {
                format_clock_time(row.effective_start_seconds)
            }),
            numeric_cell(if row.is_empty {
                String::new()
            } else {
                format_clock_time(row.effective_end_seconds)
            }),
            Cell::new(if row.has_override { "*" } else { "" }),
        ]);
    }

    table.add_row(vec![
        Cell::new("TOTAL").add_attribute(Attribute::Bold),
        Cell::new(""),
        numeric_cell(format_duration(report.totals.worked_seconds)).add_attribute(Attribute::Bold),
        numeric_cell(format_duration(report.totals.break_seconds)).add_attribute(Attribute::Bold),
        numeric_cell(format_duration(report.totals.tracked_seconds)).add_attribute(Attribute::Bold),
        Cell::new(""),
        Cell::new(""),
        Cell::new(""),
    ]);

    output.push_str(&table.to_string());
    output.push('\n');
    output
}

pub fn format_duration(total_seconds: i64) -> String {
    let sign = if total_seconds < 0 { "-" } else { "" };
    let absolute = total_seconds.abs();
    let hours = absolute / 3600;
    let minutes = (absolute % 3600) / 60;
    let seconds = absolute % 60;

    if seconds == 0 {
        format!("{sign}{hours}:{minutes:02}")
    } else {
        format!("{sign}{hours}:{minutes:02}:{seconds:02}")
    }
}

pub fn format_clock_time(total_seconds: i64) -> String {
    let day_offset = total_seconds.div_euclid(24 * 3600);
    let within_day = total_seconds.rem_euclid(24 * 3600);

    let hours = within_day / 3600;
    let minutes = (within_day % 3600) / 60;
    let seconds = within_day % 60;

    let base = if seconds == 0 {
        format!("{hours:02}:{minutes:02}")
    } else {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    };

    if day_offset == 0 {
        base
    } else {
        format!("{base} (+{day_offset}d)")
    }
}

fn header(label: &str) -> Cell {
    Cell::new(label).add_attribute(Attribute::Bold)
}

fn numeric_cell(value: impl Into<String>) -> Cell {
    Cell::new(value.into()).set_alignment(CellAlignment::Right)
}

fn is_weekday(day: Weekday) -> bool {
    !matches!(day, Weekday::Sat | Weekday::Sun)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worklog(date: &str, seconds: i64) -> TempoWorklog {
        TempoWorklog {
            start_date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
            time_spent_seconds: seconds,
        }
    }

    #[test]
    fn statutory_break_applies_only_above_six_hours() {
        assert_eq!(statutory_break_seconds((5 * 60 + 59) * 60), 0);
        assert_eq!(statutory_break_seconds(6 * 60 * 60), 0);
        assert_eq!(
            statutory_break_seconds(6 * 60 * 60 + 60),
            BREAK_DURATION_SECONDS
        );
    }

    #[test]
    fn build_report_rows_aggregates_multiple_worklogs_for_one_day() {
        let rows = build_report_rows(
            &[
                worklog("2026-03-05", 3 * 60 * 60),
                worklog("2026-03-05", 5 * 60 * 60),
                worklog("2026-03-06", 2 * 60 * 60),
            ],
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
            NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            false,
            &BTreeMap::new(),
        );

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].date, NaiveDate::from_ymd_opt(2026, 3, 5).unwrap());
        assert_eq!(rows[0].worked_seconds, 8 * 60 * 60);
        assert_eq!(rows[0].break_seconds, BREAK_DURATION_SECONDS);
        assert_eq!(
            rows[0].tracked_seconds,
            8 * 60 * 60 + BREAK_DURATION_SECONDS
        );
        assert_eq!(rows[0].effective_end_seconds, (17 * 60 + 30) * 60);
        assert_eq!(rows[1].break_seconds, 0);
    }

    #[test]
    fn synthesizes_empty_weekdays_and_keeps_weekend_work() {
        let rows = build_report_rows(
            &[worklog("2026-03-07", 2 * 60 * 60)],
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 7).unwrap(),
            NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            true,
            &BTreeMap::new(),
        );

        let dates: Vec<_> = rows.iter().map(|row| row.date).collect();
        assert!(dates.contains(&NaiveDate::from_ymd_opt(2026, 3, 2).unwrap()));
        assert!(dates.contains(&NaiveDate::from_ymd_opt(2026, 3, 6).unwrap()));
        assert!(dates.contains(&NaiveDate::from_ymd_opt(2026, 3, 7).unwrap()));
        assert!(!dates.contains(&NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()));
    }

    #[test]
    fn applies_day_overrides_to_effective_start_time() {
        let mut overrides = BTreeMap::new();
        overrides.insert(
            NaiveDate::from_ymd_opt(2026, 3, 3).unwrap(),
            NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        );
        let report = MonthlyReport::from_worklogs(
            "2026-03".to_string(),
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
            NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            true,
            &overrides,
            &[worklog("2026-03-03", 8 * 60 * 60)],
        );

        let row = report
            .rows
            .iter()
            .find(|row| row.date == NaiveDate::from_ymd_opt(2026, 3, 3).unwrap())
            .unwrap();
        assert_eq!(row.effective_start_seconds, 10 * 60 * 60);
        assert!(row.has_override);
    }

    #[test]
    fn render_report_contains_expected_table_values() {
        let report = MonthlyReport::from_worklogs(
            "2026-03".to_string(),
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
            NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            false,
            &BTreeMap::new(),
            &[worklog("2026-03-03", 8 * 60 * 60)],
        );

        let rendered = render_report(&report);

        assert!(rendered.contains("Tempo monthly span report for 2026-03"));
        assert!(rendered.contains("Worked"));
        assert!(rendered.contains("17:30"));
        assert!(rendered.contains("TOTAL"));
        assert!(rendered.contains("8:30"));
    }

    #[test]
    fn format_clock_time_marks_next_day_when_needed() {
        assert_eq!(format_clock_time(24 * 3600 + 15 * 60), "00:15 (+1d)");
    }
}
