use ratatui::text::Line;

use crate::{
    jira::JiraClient,
    report::{MonthlyReport, format_clock_time, format_duration, statutory_break_seconds},
    storage::EmptyDayTimeDisplay,
    tempo::TempoClient,
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
            text: "Verifying".to_string(),
            tone: StatusTone::Warning,
        };
    }

    if !state.loader_available {
        return StatusView {
            text: "Setup needed".to_string(),
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
                text: "Load failed".to_string(),
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
        return "Verifying saved Tempo and Jira settings before loading this month.\n\nPress s to open Connection Setup or q to quit."
            .to_string();
    }

    if !state.loader_available {
        return "Tempo or Jira connection is not configured.\n\nPress s to open Connection Setup."
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
                    "Could not load {}:\n\n{}\n\nPress r to retry, s to update the connection, or Left/Right to switch months.",
                    cached.month.label, message
                );
            }
            MonthLoadState::Idle | MonthLoadState::Ready => {}
        }
    }

    "Press Left or Right to pick a month, Home to jump to the current month, or r to reload."
        .to_string()
}

pub fn connection_status(state: &AppState) -> StatusView {
    match state.connection {
        ConnectionState::Connecting { .. } => StatusView {
            text: "Connecting...".to_string(),
            tone: StatusTone::Warning,
        },
        ConnectionState::VerifyingSaved { .. } => StatusView {
            text: "Verifying...".to_string(),
            tone: StatusTone::Warning,
        },
        ConnectionState::Verified => StatusView {
            text: "Verified".to_string(),
            tone: StatusTone::Success,
        },
        ConnectionState::SavedUnverified => StatusView {
            text: "Saved, unverified".to_string(),
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
                "Connecting...".to_string()
            } else if can_save_connection(state) {
                "Discover and save".to_string()
            } else {
                "Fix required fields".to_string()
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
                "Required".to_string()
            } else {
                "Ready".to_string()
            }
        }
        ConnectionField::TempoBaseUrl => {
            if TempoClient::new(
                state.connection_form.tempo_base_url.value.clone(),
                state
                    .connection_form
                    .tempo_api_token
                    .value
                    .trim()
                    .to_string(),
            )
            .is_ok()
            {
                "Valid URL".to_string()
            } else {
                "Check URL".to_string()
            }
        }
        ConnectionField::JiraSiteUrl => {
            let value = state.connection_form.jira_site_url.value.trim();
            if value.is_empty() {
                "Required".to_string()
            } else if value.starts_with("http://") {
                "Use HTTPS".to_string()
            } else {
                "Used for account lookup".to_string()
            }
        }
        ConnectionField::JiraEmail => {
            if state.connection_form.jira_email.value.trim().is_empty() {
                "Required".to_string()
            } else {
                "Ready".to_string()
            }
        }
        ConnectionField::JiraApiToken => {
            if state.connection_form.jira_api_token.value.trim().is_empty() {
                "Required".to_string()
            } else {
                "Ready".to_string()
            }
        }
        ConnectionField::Connect => {
            if matches!(state.connection, ConnectionState::Connecting { .. }) {
                "Running".to_string()
            } else if can_save_connection(state) {
                "Ready to verify".to_string()
            } else {
                "Fix required fields".to_string()
            }
        }
        ConnectionField::Cancel => "Return to month view".to_string(),
    }
}

pub fn selected_connection_help(state: &AppState) -> String {
    let field = state.connection_form.selected_field();
    if matches!(state.connection, ConnectionState::Connecting { .. }) {
        return "Discovering your Jira account and validating Tempo access. Press Esc to stop waiting and keep editing.".to_string();
    }

    match field {
        ConnectionField::TempoApiToken => {
            "Paste the Tempo bearer token for the correct region endpoint.".to_string()
        }
        ConnectionField::TempoBaseUrl => {
            "Usually https://api.eu.tempo.io. Keep the region aligned with the token.".to_string()
        }
        ConnectionField::JiraSiteUrl => {
            "Atlassian site URL used to discover your account ID automatically.".to_string()
        }
        ConnectionField::JiraEmail => "Email is used only for Jira account discovery.".to_string(),
        ConnectionField::JiraApiToken => {
            "Atlassian API token used only for the account lookup request.".to_string()
        }
        ConnectionField::Connect => {
            "Runs Jira discovery first, then verifies Tempo before saving both settings."
                .to_string()
        }
        ConnectionField::Cancel => "Return without changing the saved credentials.".to_string(),
    }
}

pub fn settings_status_copy(state: &AppState) -> &'static str {
    match SettingsField::all()[state.settings.selected_field] {
        SettingsField::DefaultStartTime => state.default_start_status_message(),
        SettingsField::ShowEmptyWeekdays => {
            "Toggles whether weekdays without Tempo worklogs still appear in the month table."
        }
        SettingsField::EmptyDayTimeDisplay => {
            "Chooses whether empty days show blank times or the default start/end clocks."
        }
        SettingsField::Connection => {
            "Opens the Tempo and Jira credential editor and reruns discovery/validation."
        }
    }
}

pub fn selected_row_panel_lines(state: &AppState, report: &MonthlyReport) -> Vec<Line<'static>> {
    let Some(row) = report.rows.get(state.month.selected_row) else {
        return vec![Line::from("No rows available for this month.")];
    };

    let origin = if row.has_override {
        "Manual override"
    } else if state.session_default_start_time.is_some() {
        "Session default"
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
        Ok(time) if time == edit_day.baseline_time => "Press Enter to clear the override.",
        Ok(_) => "Press Enter to save a manual override.",
        Err(_) => "Enter the start time as HH:MM before saving.",
    };
    vec![
        Line::from(edit_day.date.format("%A, %d %B %Y").to_string()),
        Line::from(format!(
            "Worked: {}   Break: {}",
            format_duration(edit_day.worked_seconds),
            format_duration(statutory_break_seconds(edit_day.worked_seconds))
        )),
        Line::from(format!(
            "Default: {}   Current saved: {}",
            edit_day.baseline_time.format("%H:%M"),
            edit_day.original_time.format("%H:%M")
        )),
        Line::from(format!(
            "Start input: {}",
            edit_day.time_input.with_cursor()
        )),
        Line::from(format!(
            "Preview end: {}",
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
    let jira = crate::storage::JiraSettings::normalized(
        state.connection_form.jira_site_url.value.clone(),
        state.connection_form.jira_email.value.clone(),
        state.connection_form.jira_api_token.value.clone(),
    );
    let tempo_api_token = state.connection_form.tempo_api_token.value.trim();
    if tempo_api_token.is_empty() || !jira.is_configured() {
        return false;
    }

    JiraClient::new(&jira).is_ok()
        && TempoClient::new(
            state.connection_form.tempo_base_url.value.clone(),
            tempo_api_token.to_string(),
        )
        .is_ok()
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
        "(discovered on connect)".to_string()
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
