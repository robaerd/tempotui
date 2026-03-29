use ratatui::text::Line;

use crate::{
    jira::{JiraError, validate_site_url},
    report::{MonthlyReport, format_clock_time, format_duration, statutory_break_seconds},
    storage::EmptyDayTimeDisplay,
    tempo::{TempoError, validate_base_url},
};

use super::{
    state::{AppState, BannerTone, ConnectionField, ConnectionState, SettingsField},
    update::edit_day_preview_end,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTone {
    Success,
    Warning,
    Danger,
    Muted,
}

#[derive(Debug, Clone)]
pub struct StatusView {
    pub text: String,
    pub tone: StatusTone,
}

#[derive(Debug, Clone)]
pub struct ConnectionFieldView {
    pub selected: bool,
    pub label: &'static str,
    pub value: String,
    pub note: String,
}

pub fn current_month_status(state: &AppState) -> StatusView {
    if matches!(state.connection, ConnectionState::VerifyingSaved { .. }) {
        return StatusView {
            text: "Checking".to_string(),
            tone: StatusTone::Warning,
        };
    }

    if !state.loader_available {
        return StatusView {
            text: "Needs setup".to_string(),
            tone: StatusTone::Danger,
        };
    }

    if let Some(cached) = state.current_month_entry() {
        use super::state::MonthLoadState;
        return match &cached.load_state {
            MonthLoadState::Refreshing { .. } => StatusView {
                text: "Refreshing".to_string(),
                tone: StatusTone::Warning,
            },
            MonthLoadState::Loading { .. } => StatusView {
                text: "Loading".to_string(),
                tone: StatusTone::Warning,
            },
            MonthLoadState::Failed { stale: true, .. } => StatusView {
                text: "Stale data".to_string(),
                tone: StatusTone::Warning,
            },
            MonthLoadState::Failed { stale: false, .. } => StatusView {
                text: "Couldn't load".to_string(),
                tone: StatusTone::Danger,
            },
            MonthLoadState::Ready => StatusView {
                text: "Ready".to_string(),
                tone: StatusTone::Success,
            },
            MonthLoadState::Idle => StatusView {
                text: "Not loaded".to_string(),
                tone: StatusTone::Muted,
            },
        };
    }

    StatusView {
        text: "Not loaded".to_string(),
        tone: StatusTone::Muted,
    }
}

pub fn pending_or_error_message(state: &AppState) -> String {
    if matches!(state.connection, ConnectionState::VerifyingSaved { .. }) {
        return "Checking your saved Tempo and Jira settings before loading this month.\n\nPress s to open Connection Setup or q to quit."
            .to_string();
    }

    if !state.loader_available {
        return "Set up Tempo and Jira before loading a month.\n\nPress s to open Connection Setup."
            .to_string();
    }

    if let Some(cached) = state.current_month_entry() {
        use super::state::MonthLoadState;
        match &cached.load_state {
            MonthLoadState::Loading { .. } | MonthLoadState::Refreshing { .. } => {
                return format!("Loading {} from Tempo...", cached.month.label);
            }
            MonthLoadState::Failed { message, .. } => {
                return format!(
                    "Couldn't load {}.\n\n{}\n\nPress r to try again, s to update the connection, or Left/Right to switch months.",
                    cached.month.label, message
                );
            }
            MonthLoadState::Idle | MonthLoadState::Ready => {}
        }
    }

    "Use Left or Right to change months, Home to jump to this month, or r to reload.".to_string()
}

pub fn connection_status(state: &AppState) -> StatusView {
    match state.connection {
        ConnectionState::Connecting { .. } => StatusView {
            text: "Checking...".to_string(),
            tone: StatusTone::Warning,
        },
        ConnectionState::VerifyingSaved { .. } => StatusView {
            text: "Checking...".to_string(),
            tone: StatusTone::Warning,
        },
        ConnectionState::Verified => StatusView {
            text: "Ready".to_string(),
            tone: StatusTone::Success,
        },
        ConnectionState::SavedUnverified => StatusView {
            text: "Not checked".to_string(),
            tone: StatusTone::Warning,
        },
        ConnectionState::NeedsSetup => StatusView {
            text: "Needs setup".to_string(),
            tone: StatusTone::Warning,
        },
        ConnectionState::Invalid { .. } => StatusView {
            text: "Needs attention".to_string(),
            tone: StatusTone::Danger,
        },
    }
}

pub fn connection_field_rows(state: &AppState) -> Vec<ConnectionFieldView> {
    let selected = state.connection_form.selected_field();
    vec![
        ConnectionFieldView {
            selected: selected == ConnectionField::TempoApiToken,
            label: ConnectionField::TempoApiToken.label(),
            value: connection_field_value(state, ConnectionField::TempoApiToken),
            note: connection_field_note(state, ConnectionField::TempoApiToken),
        },
        ConnectionFieldView {
            selected: selected == ConnectionField::TempoBaseUrl,
            label: ConnectionField::TempoBaseUrl.label(),
            value: connection_field_value(state, ConnectionField::TempoBaseUrl),
            note: connection_field_note(state, ConnectionField::TempoBaseUrl),
        },
        ConnectionFieldView {
            selected: selected == ConnectionField::JiraSiteUrl,
            label: ConnectionField::JiraSiteUrl.label(),
            value: connection_field_value(state, ConnectionField::JiraSiteUrl),
            note: connection_field_note(state, ConnectionField::JiraSiteUrl),
        },
        ConnectionFieldView {
            selected: selected == ConnectionField::JiraEmail,
            label: ConnectionField::JiraEmail.label(),
            value: connection_field_value(state, ConnectionField::JiraEmail),
            note: connection_field_note(state, ConnectionField::JiraEmail),
        },
        ConnectionFieldView {
            selected: selected == ConnectionField::JiraApiToken,
            label: ConnectionField::JiraApiToken.label(),
            value: connection_field_value(state, ConnectionField::JiraApiToken),
            note: connection_field_note(state, ConnectionField::JiraApiToken),
        },
        ConnectionFieldView {
            selected: selected == ConnectionField::Connect,
            label: ConnectionField::Connect.label(),
            value: connection_field_value(state, ConnectionField::Connect),
            note: connection_field_note(state, ConnectionField::Connect),
        },
    ]
    .into_iter()
    .chain(
        state
            .connection_form
            .can_cancel
            .then(|| ConnectionFieldView {
                selected: selected == ConnectionField::Cancel,
                label: ConnectionField::Cancel.label(),
                value: connection_field_value(state, ConnectionField::Cancel),
                note: connection_field_note(state, ConnectionField::Cancel),
            }),
    )
    .collect()
}

pub fn connection_field_value(state: &AppState, field: ConnectionField) -> String {
    match field {
        ConnectionField::TempoApiToken => {
            if state.connection_form.editing && state.connection_form.selected_field() == field {
                state.connection_form.tempo_api_token.with_cursor()
            } else {
                masked_secret(&state.connection_form.tempo_api_token.value)
            }
        }
        ConnectionField::TempoBaseUrl => {
            if state.connection_form.editing && state.connection_form.selected_field() == field {
                state.connection_form.tempo_base_url.with_cursor()
            } else {
                blank_placeholder(&state.connection_form.tempo_base_url.value)
            }
        }
        ConnectionField::JiraSiteUrl => {
            if state.connection_form.editing && state.connection_form.selected_field() == field {
                state.connection_form.jira_site_url.with_cursor()
            } else {
                blank_placeholder(&state.connection_form.jira_site_url.value)
            }
        }
        ConnectionField::JiraEmail => {
            if state.connection_form.editing && state.connection_form.selected_field() == field {
                state.connection_form.jira_email.with_cursor()
            } else {
                blank_placeholder(&state.connection_form.jira_email.value)
            }
        }
        ConnectionField::JiraApiToken => {
            if state.connection_form.editing && state.connection_form.selected_field() == field {
                state.connection_form.jira_api_token.with_cursor()
            } else {
                masked_secret(&state.connection_form.jira_api_token.value)
            }
        }
        ConnectionField::Connect => {
            if matches!(state.connection, ConnectionState::Connecting { .. }) {
                "Checking...".to_string()
            } else {
                "Press Enter".to_string()
            }
        }
        ConnectionField::Cancel => "Press Enter".to_string(),
    }
}

pub fn connection_field_note(state: &AppState, field: ConnectionField) -> String {
    match field {
        ConnectionField::TempoApiToken => {
            if state
                .connection_form
                .tempo_api_token
                .value
                .trim()
                .is_empty()
            {
                "Missing".to_string()
            } else {
                "Looks good".to_string()
            }
        }
        ConnectionField::TempoBaseUrl => {
            let value = state.connection_form.tempo_base_url.value.trim();
            if value.is_empty() {
                "Uses default".to_string()
            } else {
                match validate_base_url(value) {
                    Ok(()) => "Looks good".to_string(),
                    Err(TempoError::InsecureBaseUrl { .. }) => "Use HTTPS".to_string(),
                    Err(_) => "Check URL".to_string(),
                }
            }
        }
        ConnectionField::JiraSiteUrl => {
            let value = state.connection_form.jira_site_url.value.trim();
            if value.is_empty() {
                "Missing".to_string()
            } else {
                match validate_site_url(value) {
                    Ok(()) => "Used to find account".to_string(),
                    Err(JiraError::InsecureSiteUrl { .. }) => "Use HTTPS".to_string(),
                    Err(_) => "Check URL".to_string(),
                }
            }
        }
        ConnectionField::JiraEmail => {
            if state.connection_form.jira_email.value.trim().is_empty() {
                "Missing".to_string()
            } else {
                "Looks good".to_string()
            }
        }
        ConnectionField::JiraApiToken => {
            if state.connection_form.jira_api_token.value.trim().is_empty() {
                "Missing".to_string()
            } else {
                "Looks good".to_string()
            }
        }
        ConnectionField::Connect => {
            if matches!(state.connection, ConnectionState::Connecting { .. }) {
                "Checking".to_string()
            } else if can_save_connection(state) {
                "Ready".to_string()
            } else {
                "Missing fields".to_string()
            }
        }
        ConnectionField::Cancel => "Back to month view".to_string(),
    }
}

pub fn selected_connection_help(state: &AppState) -> String {
    let field = state.connection_form.selected_field();
    if matches!(state.connection, ConnectionState::Connecting { .. }) {
        return "Looking up your Jira account and checking Tempo access. Press Esc to stop waiting and keep editing.".to_string();
    }

    match field {
        ConnectionField::TempoApiToken => {
            "Paste a Tempo token for the same region as the API URL.".to_string()
        }
        ConnectionField::TempoBaseUrl => {
            "Usually https://api.eu.tempo.io. Leave it blank to use the default.".to_string()
        }
        ConnectionField::JiraSiteUrl => "Used to look up your Atlassian account ID.".to_string(),
        ConnectionField::JiraEmail => "Used only for the Jira account lookup.".to_string(),
        ConnectionField::JiraApiToken => "Used only for the Jira account lookup.".to_string(),
        ConnectionField::Connect => {
            "Looks up your Jira account first, then checks Tempo before saving.".to_string()
        }
        ConnectionField::Cancel => "Go back without changing the saved settings.".to_string(),
    }
}

pub fn settings_status_copy(state: &AppState) -> &'static str {
    match SettingsField::all()[state.settings.selected_field] {
        SettingsField::DefaultStartTime => state.default_start_status_message(),
        SettingsField::ShowEmptyWeekdays => "Shows or hides weekdays with no Tempo worklogs.",
        SettingsField::EmptyDayTimeDisplay => {
            "Choose whether empty days stay blank or show default start and end times."
        }
        SettingsField::Connection => "Open your Tempo and Jira settings and run the checks again.",
    }
}

pub fn selected_row_panel_lines(state: &AppState, report: &MonthlyReport) -> Vec<Line<'static>> {
    let Some(row) = report.rows.get(state.month.selected_row) else {
        return vec![Line::from("No days to show for this month.")];
    };

    let origin = if row.has_override {
        "Manual override"
    } else if state.session_default_start_time.is_some() {
        "Session override"
    } else {
        "Saved default"
    };
    let (start, end) = start_end_display(row, state.persisted.preferences.empty_day_time_display);

    vec![
        Line::from(row.date.format("%A, %d %B %Y").to_string()),
        Line::from(format!(
            "Worked: {}   Break: {}   Total: {}",
            format_duration(row.worked_seconds),
            format_duration(row.break_seconds),
            format_duration(row.tracked_seconds)
        )),
        Line::from(format!(
            "Start: {}   End: {}   Source: {}",
            blank_placeholder(&start),
            blank_placeholder(&end),
            origin
        )),
    ]
}

pub fn edit_day_lines(state: &AppState) -> Vec<Line<'static>> {
    let Some(edit_day) = &state.edit_day else {
        return vec![Line::from("No day selected.")];
    };
    let action_preview = match crate::config::parse_start_time(&edit_day.time_input.value) {
        Ok(time) if time == edit_day.baseline_time => "Press Enter to remove the override.",
        Ok(_) => "Press Enter to save this override.",
        Err(_) => "Enter a start time as HH:MM before saving.",
    };
    vec![
        Line::from(edit_day.date.format("%A, %d %B %Y").to_string()),
        Line::from(format!(
            "Worked: {}   Break: {}",
            format_duration(edit_day.worked_seconds),
            format_duration(statutory_break_seconds(edit_day.worked_seconds))
        )),
        Line::from(format!(
            "Default: {}   Saved: {}",
            edit_day.baseline_time.format("%H:%M"),
            edit_day.original_time.format("%H:%M")
        )),
        Line::from(format!("Start time: {}", edit_day.time_input.with_cursor())),
        Line::from(format!(
            "Ends at: {}",
            edit_day_preview_end(edit_day).unwrap_or_else(|| "(invalid time)".to_string())
        )),
        Line::from(
            edit_day
                .validation_error
                .clone()
                .unwrap_or_else(|| action_preview.to_string()),
        ),
    ]
}

pub fn start_end_display(
    row: &crate::report::ReportRow,
    display_mode: EmptyDayTimeDisplay,
) -> (String, String) {
    if row.is_empty {
        match display_mode {
            EmptyDayTimeDisplay::Blank => (String::new(), String::new()),
            EmptyDayTimeDisplay::DefaultStart => (
                format_clock_time(row.effective_start_seconds),
                format_clock_time(row.effective_end_seconds),
            ),
        }
    } else {
        (
            format_clock_time(row.effective_start_seconds),
            format_clock_time(row.effective_end_seconds),
        )
    }
}

fn can_save_connection(state: &AppState) -> bool {
    let tempo_api_token = state.connection_form.tempo_api_token.value.trim();
    if tempo_api_token.is_empty()
        || state.connection_form.jira_site_url.value.trim().is_empty()
        || state.connection_form.jira_email.value.trim().is_empty()
        || state.connection_form.jira_api_token.value.trim().is_empty()
    {
        return false;
    }

    validate_site_url(&state.connection_form.jira_site_url.value).is_ok()
        && validate_base_url(&state.connection_form.tempo_base_url.value).is_ok()
}

pub fn blank_placeholder(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "(not set)".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn resolved_account_id_preview(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "(found on save)".to_string()
    } else {
        trimmed.to_string()
    }
}

fn masked_secret(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "(not set)".to_string()
    } else {
        let mut chars = trimmed.chars();
        let suffix: String = trimmed
            .chars()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if chars.by_ref().count() <= 4 {
            "****".to_string()
        } else {
            format!("****{suffix}")
        }
    }
}

pub fn banner_tone(tone: BannerTone) -> StatusTone {
    match tone {
        BannerTone::Success => StatusTone::Success,
        BannerTone::Warning => StatusTone::Warning,
        BannerTone::Error => StatusTone::Danger,
    }
}
