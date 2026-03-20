use std::collections::BTreeMap;

use chrono::{NaiveDate, NaiveTime, Timelike};
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
    pub start_seconds: i64,
    pub end_seconds: i64,
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
    pub start_time: NaiveTime,
    pub rows: Vec<ReportRow>,
    pub totals: ReportTotals,
}

impl MonthlyReport {
    pub fn from_worklogs(
        month_label: String,
        range_start: NaiveDate,
        range_end: NaiveDate,
        start_time: NaiveTime,
        worklogs: Vec<TempoWorklog>,
    ) -> Self {
        let rows = build_report_rows(&worklogs, start_time);
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
            start_time,
            rows,
            totals,
        }
    }
}

pub fn build_report_rows(worklogs: &[TempoWorklog], start_time: NaiveTime) -> Vec<ReportRow> {
    let mut by_day = BTreeMap::<NaiveDate, i64>::new();
    for worklog in worklogs {
        *by_day.entry(worklog.start_date).or_default() += worklog.time_spent_seconds;
    }

    let start_seconds = i64::from(start_time.num_seconds_from_midnight());
    by_day
        .into_iter()
        .map(|(date, worked_seconds)| {
            let break_seconds = statutory_break_seconds(worked_seconds);
            let tracked_seconds = worked_seconds + break_seconds;

            ReportRow {
                date,
                worked_seconds,
                break_seconds,
                tracked_seconds,
                start_seconds,
                end_seconds: start_seconds + tracked_seconds,
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
        "Tempo monthly span report for {}\nRange: {} to {}\nStart time: {}\nBreak rule: add {} when worked > {}\n\n",
        report.month_label,
        report.range_start,
        report.range_end,
        report.start_time.format("%H:%M"),
        format_duration(BREAK_DURATION_SECONDS),
        format_duration(BREAK_THRESHOLD_SECONDS),
    ));

    if report.rows.is_empty() {
        output.push_str("No worklogs found for this period.\n");
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
    ]);

    for row in &report.rows {
        table.add_row(vec![
            Cell::new(row.date.format("%Y-%m-%d").to_string()),
            Cell::new(row.date.format("%a").to_string()),
            numeric_cell(format_duration(row.worked_seconds)),
            numeric_cell(format_duration(row.break_seconds)),
            numeric_cell(format_duration(row.tracked_seconds)),
            numeric_cell(format_clock_time(row.start_seconds)),
            numeric_cell(format_clock_time(row.end_seconds)),
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
            NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        );

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].date, NaiveDate::from_ymd_opt(2026, 3, 5).unwrap());
        assert_eq!(rows[0].worked_seconds, 8 * 60 * 60);
        assert_eq!(rows[0].break_seconds, BREAK_DURATION_SECONDS);
        assert_eq!(
            rows[0].tracked_seconds,
            8 * 60 * 60 + BREAK_DURATION_SECONDS
        );
        assert_eq!(rows[0].end_seconds, (17 * 60 + 30) * 60);
        assert_eq!(rows[1].break_seconds, 0);
    }

    #[test]
    fn render_report_contains_expected_table_values() {
        let report = MonthlyReport::from_worklogs(
            "2026-03".to_string(),
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
            NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            vec![worklog("2026-03-03", 8 * 60 * 60)],
        );

        let rendered = render_report(&report);

        assert!(rendered.contains("Tempo monthly span report for 2026-03"));
        assert!(rendered.contains("Worked"));
        assert!(rendered.contains("Tracked"));
        assert!(rendered.contains("17:30"));
        assert!(rendered.contains("TOTAL"));
        assert!(rendered.contains("8:30"));
    }

    #[test]
    fn format_clock_time_marks_next_day_when_needed() {
        assert_eq!(format_clock_time(24 * 3600 + 15 * 60), "00:15 (+1d)");
    }
}
