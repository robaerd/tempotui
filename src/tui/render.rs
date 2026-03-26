use std::path::Path;

use chrono::{Datelike, Duration as DateDuration, NaiveDate};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Cell, HighlightSpacing, Paragraph, Row, Table, TableState, Wrap},
};

use crate::{
    config::MonthWindow,
    report::{MonthlyReport, format_duration},
    storage::EmptyDayTimeDisplay,
};

use super::{
    state::{AppState, Panel, SettingsField},
    view_model::{
        StatusTone, banner_tone, blank_placeholder, connection_field_rows, connection_status,
        current_month_status, edit_day_lines, pending_or_error_message,
        resolved_account_id_preview, selected_connection_help, selected_row_panel_lines,
        settings_status_copy, start_end_display,
    },
};

const PANEL_BORDER: Color = Color::Rgb(87, 100, 122);
const PANEL_TITLE: Color = Color::Rgb(168, 208, 255);
const ACCENT: Color = Color::Rgb(118, 182, 255);
const SELECTED_BG: Color = Color::Rgb(44, 73, 112);
const SELECTED_FG: Color = Color::Rgb(244, 247, 252);
const MUTED_TEXT: Color = Color::Rgb(150, 160, 177);
const SOFT_TEXT: Color = Color::Rgb(214, 220, 231);
const WEEK_BAND_BG: Color = Color::Rgb(32, 46, 67);
const EMPTY_ROW: Color = Color::Rgb(120, 129, 144);
const SUCCESS: Color = Color::Rgb(122, 190, 124);
const WARNING: Color = Color::Rgb(232, 189, 96);
const DANGER: Color = Color::Rgb(230, 117, 117);

pub fn draw(frame: &mut Frame, state: &AppState, store_path: &Path) {
    match state.route {
        super::state::Route::Setup => draw_setup(frame, state, store_path),
        super::state::Route::Month => draw_month(frame, state, store_path),
    }
}

fn draw_setup(frame: &mut Frame, state: &AppState, store_path: &Path) {
    draw_connection_form(
        frame,
        frame.area(),
        state,
        store_path,
        "Connection Setup",
        "Save Tempo and Jira credentials in config.toml and discover your Jira account automatically.",
    );
}

fn draw_month(frame: &mut Frame, state: &AppState, store_path: &Path) {
    let area = frame.area();
    let narrow = area.width < 110;
    let banner_height = if state.banner.is_some() { 3 } else { 0 };
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(if narrow { 7 } else { 5 }),
            Constraint::Min(8),
            Constraint::Length(banner_height),
            Constraint::Length(3),
        ])
        .split(area);

    let status = current_month_status(state);
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "TempoTUI",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                friendly_month_label(&state.month.month),
                Style::default().fg(SOFT_TEXT).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                month_range_label(state.month.month.start, state.month.month.end),
                Style::default().fg(MUTED_TEXT),
            ),
            Span::raw("  •  "),
            Span::styled(
                format!("Default start {}", state.active_default_start_label()),
                Style::default().fg(MUTED_TEXT),
            ),
            Span::raw("  •  "),
            Span::styled(status.text, status_style(status.tone)),
        ]),
    ])
    .block(panel_block("Month"));
    frame.render_widget(header, layout[0]);

    render_summary_cards(frame, state, layout[1], narrow);
    render_month_body(frame, state, layout[2], store_path, narrow);

    if let Some(banner) = &state.banner {
        frame.render_widget(
            Paragraph::new(banner.text.clone())
                .block(panel_block("Status"))
                .style(Style::default().fg(status_color(banner_tone(banner.tone)))),
            layout[3],
        );
    }

    frame.render_widget(
        shortcut_bar(
            "Shortcuts",
            &[
                ("Month", "Left/Right"),
                ("Rows", "Up/Down"),
                ("Edit", "Enter"),
                ("Refresh", "r"),
                ("Settings", "s"),
                ("Help", "?"),
                ("Quit", "q / Ctrl+C"),
            ],
        ),
        layout[4],
    );
}

fn render_summary_cards(frame: &mut Frame, state: &AppState, area: Rect, narrow: bool) {
    let summary = if narrow {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Length(3)])
            .split(area);
        let top = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[0]);
        let bottom = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[1]);
        vec![top[0], top[1], bottom[0], bottom[1]]
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(25); 4])
            .split(area)
            .to_vec()
    };

    if let Some(report) = state.current_report() {
        let logged_days = report.rows.iter().filter(|row| !row.is_empty).count();
        let empty_days = report.rows.iter().filter(|row| row.is_empty).count();
        let override_days = report.rows.iter().filter(|row| row.has_override).count();

        frame.render_widget(
            summary_card(
                "Worked",
                &format_duration(report.totals.worked_seconds),
                "Tempo only",
            ),
            summary[0],
        );
        frame.render_widget(
            summary_card(
                "Breaks",
                &format_duration(report.totals.break_seconds),
                &format!(
                    "{} rule when worked > 6:00",
                    format_duration(crate::report::BREAK_DURATION_SECONDS)
                ),
            ),
            summary[1],
        );
        frame.render_widget(
            summary_card(
                "Tracked",
                &format_duration(report.totals.tracked_seconds),
                "Manual tool span",
            ),
            summary[2],
        );
        frame.render_widget(
            summary_card(
                "Days",
                &format!("{logged_days} logged"),
                &format!("{empty_days} empty • {override_days} overrides"),
            ),
            summary[3],
        );
    } else {
        frame.render_widget(summary_card("Worked", "--", "Waiting for data"), summary[0]);
        frame.render_widget(summary_card("Breaks", "--", "Waiting for data"), summary[1]);
        frame.render_widget(
            summary_card("Tracked", "--", "Waiting for data"),
            summary[2],
        );
        frame.render_widget(summary_card("Days", "--", "Waiting for data"), summary[3]);
    }
}

fn render_month_body(
    frame: &mut Frame,
    state: &AppState,
    area: Rect,
    store_path: &Path,
    narrow: bool,
) {
    match state.panel {
        Panel::None => {
            let body = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(8), Constraint::Length(5)])
                .split(area);
            render_calendar(frame, state, body[0], narrow);
            render_selected_day(frame, state, body[1]);
        }
        _ if narrow => {
            let body = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(8), Constraint::Length(10)])
                .split(area);
            render_calendar(frame, state, body[0], true);
            render_panel(frame, state, body[1], store_path, true);
        }
        _ => {
            let body = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
                .split(area);
            let main = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(8), Constraint::Length(5)])
                .split(body[0]);
            render_calendar(frame, state, main[0], false);
            render_selected_day(frame, state, main[1]);
            render_panel(frame, state, body[1], store_path, false);
        }
    }
}

fn render_calendar(frame: &mut Frame, state: &AppState, area: Rect, narrow: bool) {
    if let Some(report) = state.current_report() {
        let (rows, selected_display_row) = if narrow {
            build_compact_month_table_rows(
                &report,
                state.today,
                state.persisted.preferences.empty_day_time_display,
                state.month.selected_row,
            )
        } else {
            build_month_table_rows(
                &report,
                state.today,
                state.persisted.preferences.empty_day_time_display,
                state.month.selected_row,
            )
        };
        let mut table_state = TableState::default();
        table_state.select(selected_display_row);
        let table = Table::new(
            rows,
            if narrow {
                vec![
                    Constraint::Length(4),
                    Constraint::Length(10),
                    Constraint::Length(8),
                    Constraint::Length(6),
                    Constraint::Length(6),
                    Constraint::Length(4),
                ]
            } else {
                vec![
                    Constraint::Length(6),
                    Constraint::Length(12),
                    Constraint::Length(5),
                    Constraint::Length(8),
                    Constraint::Length(7),
                    Constraint::Length(8),
                    Constraint::Length(8),
                    Constraint::Length(8),
                    Constraint::Length(4),
                ]
            },
        )
        .header(
            Row::new(if narrow {
                vec!["Week", "Date", "Worked", "Start", "End", "Adj"]
            } else {
                vec![
                    "Week", "Date", "Day", "Worked", "Break", "Total", "Start", "End", "Adj",
                ]
            })
            .style(
                Style::default()
                    .fg(PANEL_TITLE)
                    .add_modifier(Modifier::BOLD),
            )
            .bottom_margin(1),
        )
        .column_spacing(1)
        .block(panel_block("Calendar"))
        .row_highlight_style(
            Style::default()
                .fg(SELECTED_FG)
                .bg(SELECTED_BG)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_spacing(HighlightSpacing::Always)
        .highlight_symbol(">>");
        frame.render_stateful_widget(table, area, &mut table_state);
    } else {
        frame.render_widget(
            Paragraph::new(pending_or_error_message(state))
                .block(panel_block("Calendar"))
                .wrap(Wrap { trim: false }),
            area,
        );
    }
}

fn render_selected_day(frame: &mut Frame, state: &AppState, area: Rect) {
    if let Some(report) = state.current_report() {
        frame.render_widget(
            Paragraph::new(selected_row_panel_lines(state, &report))
                .block(panel_block("Selected Day"))
                .wrap(Wrap { trim: false }),
            area,
        );
    } else {
        let message = state
            .banner
            .as_ref()
            .map(|banner| banner.text.clone())
            .unwrap_or_else(|| {
                "Load a month, refresh, or reopen Connection from settings when the saved token changes."
                    .to_string()
            });
        frame.render_widget(
            Paragraph::new(vec![Line::from(message)])
                .block(panel_block("Status"))
                .wrap(Wrap { trim: false }),
            area,
        );
    }
}

fn render_panel(frame: &mut Frame, state: &AppState, area: Rect, store_path: &Path, narrow: bool) {
    match state.panel {
        Panel::SettingsDrawer => draw_settings_drawer(frame, state, area, store_path, narrow),
        Panel::HelpDrawer => draw_help_drawer(frame, area),
        Panel::ConnectionDrawer => draw_connection_form(
            frame,
            area,
            state,
            store_path,
            "Connection Settings",
            "Update Tempo and Jira credentials, then verify and save them.",
        ),
        Panel::EditDayInspector => draw_edit_day_drawer(frame, state, area),
        Panel::None => {}
    }
}

fn draw_connection_form(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    store_path: &Path,
    heading: &str,
    subtitle: &str,
) {
    let narrow = area.width < 110;
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(12),
            Constraint::Length(4),
        ])
        .split(area);

    let title = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "TempoTUI",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {heading}"),
                Style::default().fg(SOFT_TEXT).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(subtitle, Style::default().fg(MUTED_TEXT))),
    ])
    .block(panel_block("Setup"))
    .alignment(Alignment::Left);
    frame.render_widget(title, layout[0]);

    let body = if narrow {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(8), Constraint::Length(8)])
            .split(layout[1])
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(layout[1])
    };

    let rows = connection_field_rows(state)
        .into_iter()
        .map(|row| config_row(row.selected, row.label, row.value, row.note))
        .collect::<Vec<_>>();
    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Length(if narrow { 26 } else { 28 }),
            Constraint::Min(14),
        ],
    )
    .header(
        Row::new(vec!["Field", "Value", "Status"])
            .style(
                Style::default()
                    .fg(PANEL_TITLE)
                    .add_modifier(Modifier::BOLD),
            )
            .bottom_margin(1),
    )
    .column_spacing(1)
    .block(panel_block("Connection"));
    frame.render_widget(table, body[0]);

    let connection_status = connection_status(state);
    let status_text = state
        .connection_form
        .message
        .clone()
        .unwrap_or_else(|| selected_connection_help(state));
    let status_style = if state.connection_form.message.is_some() {
        Style::default().fg(WARNING)
    } else {
        Style::default().fg(MUTED_TEXT)
    };
    let details = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Config:", label_style()),
            Span::styled(
                format!(" {}", display_store_path(store_path)),
                value_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Status:", label_style()),
            Span::styled(
                format!(" {}", connection_status.text),
                Style::default().fg(status_color(connection_status.tone)),
            ),
        ]),
        Line::from(vec![
            Span::styled("Resolved ID:", label_style()),
            Span::styled(
                format!(
                    " {}",
                    resolved_account_id_preview(&state.persisted.tempo.account_id)
                ),
                value_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Tempo URL:", label_style()),
            Span::styled(
                format!(
                    " {}",
                    blank_placeholder(&state.connection_form.tempo_base_url.value)
                ),
                value_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Jira Site:", label_style()),
            Span::styled(
                format!(
                    " {}",
                    blank_placeholder(&state.connection_form.jira_site_url.value)
                ),
                value_style(),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(status_text, status_style)),
        Line::from(""),
        Line::from(Span::styled(
            "Jira is used only to resolve your Atlassian account ID. Tokens are saved in config.toml with restricted file permissions.",
            Style::default().fg(MUTED_TEXT),
        )),
    ])
    .block(panel_block(if narrow { "Status" } else { "Details" }))
    .wrap(Wrap { trim: false });
    frame.render_widget(details, body[1]);

    let help_lines = if state.connection_form.editing {
        vec![
            Line::from("Move Left/Right/Home/End   Delete Backspace/Delete"),
            Line::from("Done Enter or Tab   Revert Esc"),
        ]
    } else if matches!(
        state.connection,
        super::state::ConnectionState::Connecting { .. }
    ) {
        vec![
            Line::from("Busy Discovering and verifying   Stop wait Esc"),
            Line::from("Quit q / Ctrl+C"),
        ]
    } else if state.connection_form.can_cancel {
        vec![
            Line::from("Navigate Up/Down or Tab   Edit/Run Enter"),
            Line::from("Cancel Esc   Quit q / Ctrl+C"),
        ]
    } else {
        vec![
            Line::from("Navigate Up/Down or Tab   Edit/Run Enter"),
            Line::from("Quit q / Ctrl+C"),
        ]
    };
    frame.render_widget(
        Paragraph::new(help_lines).block(panel_block("Keys")),
        layout[2],
    );
}

fn draw_settings_drawer(
    frame: &mut Frame,
    state: &AppState,
    area: Rect,
    store_path: &Path,
    narrow: bool,
) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(if narrow { 4 } else { 5 }),
            Constraint::Min(7),
            Constraint::Length(4),
        ])
        .split(area);
    let title = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "Settings",
                Style::default().fg(SOFT_TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                "Preferences apply immediately",
                Style::default().fg(MUTED_TEXT),
            ),
        ]),
        Line::from(Span::styled(
            "Use Left/Right to change values. Enter on Connection opens credentials.",
            Style::default().fg(MUTED_TEXT),
        )),
    ])
    .block(panel_block("Settings"));
    frame.render_widget(title, layout[0]);

    let connection = connection_status(state);
    let rows = vec![
        config_row(
            state.settings.selected_field == 0,
            SettingsField::DefaultStartTime.label(),
            state.active_default_start_label(),
            if state.session_default_start_time.is_some() {
                "Saved value updates later"
            } else {
                "Baseline for each tracked day"
            },
        ),
        config_row(
            state.settings.selected_field == 1,
            SettingsField::ShowEmptyWeekdays.label(),
            if state.persisted.preferences.show_empty_weekdays {
                "Yes".to_string()
            } else {
                "No".to_string()
            },
            "Include weekdays without worklogs",
        ),
        config_row(
            state.settings.selected_field == 2,
            SettingsField::EmptyDayTimeDisplay.label(),
            state
                .persisted
                .preferences
                .empty_day_time_display
                .label()
                .to_string(),
            "Blank or show default clocks",
        ),
        config_row(
            state.settings.selected_field == 3,
            SettingsField::Connection.label(),
            connection.text.clone(),
            "Open Tempo and Jira credentials",
        ),
    ];
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(19),
                Constraint::Length(18),
                Constraint::Min(20),
            ],
        )
        .header(
            Row::new(vec!["Setting", "Value", "Effect"])
                .style(
                    Style::default()
                        .fg(PANEL_TITLE)
                        .add_modifier(Modifier::BOLD),
                )
                .bottom_margin(1),
        )
        .column_spacing(1)
        .block(panel_block("Preferences")),
        layout[1],
    );

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(format!("Config file: {}", display_store_path(store_path))),
            Line::from(format!(
                "Saved start: {}",
                state
                    .persisted
                    .preferences
                    .default_start_time
                    .format("%H:%M")
            )),
            Line::from(format!(
                "Active start: {}",
                state.active_default_start_label()
            )),
            Line::from(format!("Connection: {}", connection_status(state).text)),
            Line::from(settings_status_copy(state)),
        ])
        .block(panel_block("Status"))
        .wrap(Wrap { trim: false }),
        layout[2],
    );
}

fn draw_help_drawer(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("Month view  Left/Right switch month, Up/Down move rows, Home jumps to current month."),
            Line::from("Editing  Enter opens the selected day editor. Type HH:MM directly, use Left/Right for +/-15m, 0 to reset."),
            Line::from("Settings  s opens preferences. Enter on Connection edits Tempo/Jira credentials."),
            Line::from("Refresh  r reloads the visible month from Tempo."),
            Line::from("Dismiss  Esc closes drawers. Esc never quits from the main view."),
            Line::from("Quit  q and Ctrl+C quit the app."),
        ])
        .block(panel_block("Help"))
        .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_edit_day_drawer(frame: &mut Frame, state: &AppState, area: Rect) {
    frame.render_widget(
        Paragraph::new(edit_day_lines(state))
            .block(panel_block("Edit Day"))
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub(super) fn build_month_table_rows(
    report: &MonthlyReport,
    today: NaiveDate,
    display_mode: EmptyDayTimeDisplay,
    selected_row: usize,
) -> (Vec<Row<'static>>, Option<usize>) {
    let mut rows = Vec::new();
    let mut selected_display_row = None;
    let mut current_week_start = None;

    for (row_index, row) in report.rows.iter().enumerate() {
        let week_start = start_of_week(row.date);
        if current_week_start != Some(week_start) {
            let visible_week_start = week_start.max(report.range_start);
            let visible_week_end = (week_start + DateDuration::days(6)).min(report.range_end);
            rows.push(
                Row::new(vec![
                    Cell::from(format!("W{:02}", row.date.iso_week().week())),
                    Cell::from(compact_week_label(visible_week_start, visible_week_end)),
                    Cell::from(""),
                    Cell::from(""),
                    Cell::from(""),
                    Cell::from(""),
                    Cell::from(""),
                    Cell::from(""),
                    Cell::from(""),
                ])
                .style(
                    Style::default()
                        .fg(PANEL_TITLE)
                        .bg(WEEK_BAND_BG)
                        .add_modifier(Modifier::BOLD),
                ),
            );
            current_week_start = Some(week_start);
        }

        let (start, end) = start_end_display(row, display_mode);
        let row_style = if row.date == today {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else if row.is_empty {
            Style::default().fg(EMPTY_ROW)
        } else {
            Style::default().fg(SOFT_TEXT)
        };

        rows.push(
            Row::new(vec![
                Cell::from(""),
                Cell::from(row.date.format("%Y-%m-%d").to_string()),
                Cell::from(row.date.format("%a").to_string()),
                numeric_cell(format_duration(row.worked_seconds)),
                numeric_cell(format_duration(row.break_seconds)),
                numeric_cell(format_duration(row.tracked_seconds)),
                numeric_cell(start),
                numeric_cell(end),
                Cell::from(if row.has_override { "*" } else { "" }),
            ])
            .style(row_style),
        );

        if row_index == selected_row {
            selected_display_row = Some(rows.len().saturating_sub(1));
        }
    }

    (rows, selected_display_row)
}

pub(super) fn build_compact_month_table_rows(
    report: &MonthlyReport,
    today: NaiveDate,
    display_mode: EmptyDayTimeDisplay,
    selected_row: usize,
) -> (Vec<Row<'static>>, Option<usize>) {
    let mut rows = Vec::new();
    let mut selected_display_row = None;
    let mut current_week_start = None;

    for (row_index, row) in report.rows.iter().enumerate() {
        let week_start = start_of_week(row.date);
        if current_week_start != Some(week_start) {
            let visible_week_start = week_start.max(report.range_start);
            let visible_week_end = (week_start + DateDuration::days(6)).min(report.range_end);
            rows.push(
                Row::new(vec![
                    Cell::from(format!("W{:02}", row.date.iso_week().week())),
                    Cell::from(compact_week_label(visible_week_start, visible_week_end)),
                    Cell::from(""),
                    Cell::from(""),
                    Cell::from(""),
                    Cell::from(""),
                ])
                .style(
                    Style::default()
                        .fg(PANEL_TITLE)
                        .bg(WEEK_BAND_BG)
                        .add_modifier(Modifier::BOLD),
                ),
            );
            current_week_start = Some(week_start);
        }

        let (start, end) = start_end_display(row, display_mode);
        let row_style = if row.date == today {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else if row.is_empty {
            Style::default().fg(EMPTY_ROW)
        } else {
            Style::default().fg(SOFT_TEXT)
        };

        rows.push(
            Row::new(vec![
                Cell::from(""),
                Cell::from(row.date.format("%d %b").to_string()),
                numeric_cell(format_duration(row.worked_seconds)),
                numeric_cell(start),
                numeric_cell(end),
                Cell::from(if row.has_override { "*" } else { "" }),
            ])
            .style(row_style),
        );

        if row_index == selected_row {
            selected_display_row = Some(rows.len().saturating_sub(1));
        }
    }

    (rows, selected_display_row)
}

fn config_row(
    selected: bool,
    label: &str,
    value: impl Into<String>,
    effect: impl Into<String>,
) -> Row<'static> {
    let row_style = if selected {
        Style::default()
            .fg(SELECTED_FG)
            .bg(SELECTED_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(SOFT_TEXT)
    };

    Row::new(vec![
        Cell::from(label.to_string()),
        Cell::from(value.into()),
        Cell::from(effect.into()),
    ])
    .style(row_style)
}

fn summary_card(title: &str, value: &str, hint: &str) -> Paragraph<'static> {
    Paragraph::new(vec![
        Line::from(Span::styled(
            value.to_string(),
            Style::default().fg(SOFT_TEXT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            hint.to_string(),
            Style::default().fg(MUTED_TEXT),
        )),
    ])
    .block(panel_block(title))
}

fn panel_block(title: &str) -> Block<'static> {
    Block::bordered()
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(PANEL_BORDER))
        .title(format!(" {title} "))
        .title_style(
            Style::default()
                .fg(PANEL_TITLE)
                .add_modifier(Modifier::BOLD),
        )
}

fn label_style() -> Style {
    Style::default()
        .fg(PANEL_TITLE)
        .add_modifier(Modifier::BOLD)
}

fn value_style() -> Style {
    Style::default().fg(SOFT_TEXT)
}

fn numeric_cell(value: impl Into<String>) -> Cell<'static> {
    Cell::from(Line::from(value.into()).right_aligned())
}

fn shortcut_bar(title: &str, entries: &[(&str, &str)]) -> Paragraph<'static> {
    let mut spans = Vec::new();
    for (index, (label, value)) in entries.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("   "));
        }
        spans.push(Span::styled((*label).to_string(), label_style()));
        spans.push(Span::raw(format!(" {value}")));
    }

    Paragraph::new(Line::from(spans)).block(panel_block(title))
}

fn friendly_month_label(month: &MonthWindow) -> String {
    month.start.format("%B %Y").to_string()
}

fn month_range_label(start: NaiveDate, end: NaiveDate) -> String {
    format!("{} - {}", start.format("%d %b %Y"), end.format("%d %b %Y"))
}

fn compact_week_label(start: NaiveDate, end: NaiveDate) -> String {
    if start.month() == end.month() && start.year() == end.year() {
        format!(
            "{}-{} {}",
            start.format("%d"),
            end.format("%d"),
            end.format("%b")
        )
    } else {
        format!("{}-{}", start.format("%d %b"), end.format("%d %b"))
    }
}

fn display_store_path(path: &Path) -> String {
    let Some(home) = std::env::var_os("HOME") else {
        return path.display().to_string();
    };
    let home = std::path::PathBuf::from(home);
    match path.strip_prefix(&home) {
        Ok(relative) => format!("~/{}", relative.display()),
        Err(_) => path.display().to_string(),
    }
}

fn start_of_week(date: NaiveDate) -> NaiveDate {
    date - DateDuration::days(i64::from(date.weekday().num_days_from_monday()))
}

fn status_style(tone: StatusTone) -> Style {
    Style::default().fg(status_color(tone))
}

fn status_color(tone: StatusTone) -> Color {
    match tone {
        StatusTone::Success => SUCCESS,
        StatusTone::Warning => WARNING,
        StatusTone::Danger => DANGER,
        StatusTone::Muted => MUTED_TEXT,
    }
}
