use chrono::Timelike;

use crate::{
    jira::JiraClient,
    report::statutory_break_seconds,
    storage::{JiraSettings, TempoSettings},
    tempo::TempoClient,
};

use super::{
    action::{Action, ConnectionAction, EditDayAction, Effect, SettingsAction, month_effect},
    state::{
        AppState, BannerState, BannerTone, CachedMonth, ConnectionField, ConnectionFormState,
        ConnectionState, EditDayState, MonthLoadState, Panel, Route, SettingsField, adjust_time,
        parse_edit_time,
    },
};

pub fn reduce(state: &mut AppState, action: Action) -> Vec<Effect> {
    match action {
        Action::Boot => boot(state),
        Action::ForceQuit => {
            state.should_quit = true;
            Vec::new()
        }
        Action::ClosePanel => close_panel(state),
        Action::ToggleHelp => toggle_help(state),
        Action::OpenSettings => open_settings(state),
        Action::SkipSavedVerification => skip_saved_verification(state),
        Action::NavigateMonth(delta) => navigate_month(state, delta),
        Action::JumpToCurrentMonth => jump_to_current_month(state),
        Action::MoveSelection(delta) => {
            move_selection(state, delta);
            Vec::new()
        }
        Action::RefreshMonth => request_month_load(state, true),
        Action::OpenEditDay => {
            open_edit_day(state);
            Vec::new()
        }
        Action::Connection(action) => reduce_connection(state, action),
        Action::Settings(action) => reduce_settings(state, action),
        Action::EditDay(action) => reduce_edit_day(state, action),
        Action::SavedConnectionVerified => saved_connection_verified(state),
        Action::SavedConnectionRejected { message } => saved_connection_rejected(state, message),
        Action::ConnectionEstablished { tempo, jira } => connection_established(state, tempo, jira),
        Action::ConnectionEstablishFailed { message } => {
            connection_establish_failed(state, message);
            Vec::new()
        }
        Action::PersistedSaveSucceeded { message } => {
            state.banner = Some(BannerState {
                tone: BannerTone::Success,
                text: message,
            });
            Vec::new()
        }
        Action::PersistedSaveFailed { message } => {
            state.banner = Some(BannerState {
                tone: BannerTone::Warning,
                text: message,
            });
            Vec::new()
        }
        Action::MonthLoaded {
            request_id,
            month,
            worklogs,
        } => {
            apply_month_loaded(state, request_id, month, worklogs);
            Vec::new()
        }
        Action::MonthLoadFailed {
            request_id,
            month,
            message,
        } => {
            apply_month_load_failed(state, request_id, month, message);
            Vec::new()
        }
        Action::LoaderDisconnected { loader_available } => {
            loader_disconnected(state, loader_available);
            Vec::new()
        }
        Action::ConnectionRuntimeDisconnected => {
            connection_runtime_disconnected(state);
            Vec::new()
        }
    }
}

fn boot(state: &mut AppState) -> Vec<Effect> {
    if state.route == Route::Month
        && state.loader_available
        && state.setup_complete()
        && matches!(
            state.connection,
            ConnectionState::SavedUnverified | ConnectionState::Verified
        )
    {
        let request_id = state.next_request_id();
        state.connection = ConnectionState::VerifyingSaved { request_id };
        return vec![Effect::VerifySavedConnection {
            request_id,
            tempo: state.persisted.tempo.clone(),
        }];
    }

    Vec::new()
}

fn close_panel(state: &mut AppState) -> Vec<Effect> {
    match state.panel {
        Panel::None => {}
        Panel::SettingsDrawer | Panel::HelpDrawer | Panel::ConnectionDrawer => {
            state.panel = Panel::None;
            state.connection_form.editing = false;
            state.connection_form.edit_snapshot = None;
        }
        Panel::EditDayInspector => {
            state.panel = Panel::None;
            state.edit_day = None;
        }
    }
    Vec::new()
}

fn toggle_help(state: &mut AppState) -> Vec<Effect> {
    if state.route != Route::Month {
        return Vec::new();
    }

    state.panel = if state.panel == Panel::HelpDrawer {
        Panel::None
    } else {
        Panel::HelpDrawer
    };
    Vec::new()
}

fn open_settings(state: &mut AppState) -> Vec<Effect> {
    if matches!(state.connection, ConnectionState::VerifyingSaved { .. }) {
        return skip_saved_verification(state);
    }

    if state.route == Route::Month {
        state.panel = Panel::SettingsDrawer;
    }
    Vec::new()
}

fn skip_saved_verification(state: &mut AppState) -> Vec<Effect> {
    if !matches!(state.connection, ConnectionState::VerifyingSaved { .. }) {
        return Vec::new();
    }

    transition_to_setup(
        state,
        Some("Saved connection verification skipped. Update the settings to continue.".to_string()),
    );
    state.connection = ConnectionState::SavedUnverified;
    Vec::new()
}

fn navigate_month(state: &mut AppState, delta: i32) -> Vec<Effect> {
    if state.route != Route::Month {
        return Vec::new();
    }

    state.month.month = state.month.month.shift_months(delta);
    state.month.selected_row = 0;
    request_month_load(state, false)
}

fn jump_to_current_month(state: &mut AppState) -> Vec<Effect> {
    if state.route != Route::Month {
        return Vec::new();
    }

    let current = crate::config::MonthWindow::current(state.today);
    if state.month.month.label != current.label {
        state.month.month = current;
        state.month.selected_row = 0;
        return request_month_load(state, false);
    }

    Vec::new()
}

fn move_selection(state: &mut AppState, delta: i32) {
    let Some(report) = state.current_report() else {
        return;
    };
    if report.rows.is_empty() {
        state.month.selected_row = 0;
        return;
    }

    let max_index = report.rows.len().saturating_sub(1) as i32;
    let next = (state.month.selected_row as i32 + delta).clamp(0, max_index) as usize;
    state.month.selected_row = next;
}

fn open_edit_day(state: &mut AppState) {
    let Some(report) = state.current_report() else {
        return;
    };
    let Some(row) = report.rows.get(state.month.selected_row) else {
        return;
    };
    let baseline = state.effective_default_start_time();
    let current = state
        .persisted
        .day_overrides
        .get(&row.date)
        .copied()
        .unwrap_or(baseline);
    state.edit_day = Some(EditDayState {
        date: row.date,
        worked_seconds: row.worked_seconds,
        time_input: super::state::EditableText::new(current.format("%H:%M").to_string()),
        original_time: current,
        baseline_time: baseline,
        validation_error: None,
    });
    state.panel = Panel::EditDayInspector;
}

fn reduce_connection(state: &mut AppState, action: ConnectionAction) -> Vec<Effect> {
    if matches!(state.connection, ConnectionState::Connecting { .. }) {
        if matches!(action, ConnectionAction::Cancel) {
            state.connection = ConnectionState::SavedUnverified;
            state.connection_form.message = Some(
                "Stopped waiting for verification. You can edit the fields and try again."
                    .to_string(),
            );
        }
        return Vec::new();
    }

    if state.connection_form.editing {
        return reduce_connection_editing(state, action);
    }

    match action {
        ConnectionAction::Cancel => {
            if state.route == Route::Setup {
                return Vec::new();
            }
            if state.connection_form.can_cancel {
                state.panel = Panel::None;
                state.connection_form.editing = false;
                state.connection_form.edit_snapshot = None;
                state.connection_form.message = None;
            }
            Vec::new()
        }
        ConnectionAction::AdvanceField(delta) => {
            let count = ConnectionField::all(state.connection_form.can_cancel).len() as i32;
            let next = (state.connection_form.selected_field as i32 + delta).rem_euclid(count);
            state.connection_form.selected_field = next as usize;
            Vec::new()
        }
        ConnectionAction::ActivateSelected => activate_connection_field(state),
        ConnectionAction::FinishEdit
        | ConnectionAction::CancelEdit
        | ConnectionAction::MoveCursorLeft
        | ConnectionAction::MoveCursorRight
        | ConnectionAction::MoveCursorHome
        | ConnectionAction::MoveCursorEnd
        | ConnectionAction::Backspace
        | ConnectionAction::Delete
        | ConnectionAction::Insert(_) => Vec::new(),
    }
}

fn reduce_connection_editing(state: &mut AppState, action: ConnectionAction) -> Vec<Effect> {
    match action {
        ConnectionAction::CancelEdit => cancel_connection_edit(state),
        ConnectionAction::FinishEdit => {
            state.connection_form.editing = false;
            state.connection_form.edit_snapshot = None;
            Vec::new()
        }
        ConnectionAction::AdvanceField(delta) => {
            state.connection_form.editing = false;
            state.connection_form.edit_snapshot = None;
            let count = ConnectionField::all(state.connection_form.can_cancel).len() as i32;
            let next = (state.connection_form.selected_field as i32 + delta).rem_euclid(count);
            state.connection_form.selected_field = next as usize;
            Vec::new()
        }
        ConnectionAction::MoveCursorLeft => {
            mutate_selected_connection_text(state, |field| field.move_left());
            Vec::new()
        }
        ConnectionAction::MoveCursorRight => {
            mutate_selected_connection_text(state, |field| field.move_right());
            Vec::new()
        }
        ConnectionAction::MoveCursorHome => {
            mutate_selected_connection_text(state, |field| field.move_home());
            Vec::new()
        }
        ConnectionAction::MoveCursorEnd => {
            mutate_selected_connection_text(state, |field| field.move_end());
            Vec::new()
        }
        ConnectionAction::Backspace => {
            mutate_selected_connection_text(state, |field| field.backspace());
            Vec::new()
        }
        ConnectionAction::Delete => {
            mutate_selected_connection_text(state, |field| field.delete());
            Vec::new()
        }
        ConnectionAction::Insert(ch) => {
            mutate_selected_connection_text(state, |field| field.insert_char(ch));
            Vec::new()
        }
        ConnectionAction::Cancel => cancel_connection_edit(state),
        ConnectionAction::ActivateSelected => Vec::new(),
    }
}

fn activate_connection_field(state: &mut AppState) -> Vec<Effect> {
    state.connection_form.message = None;
    match state.connection_form.selected_field() {
        ConnectionField::TempoApiToken
        | ConnectionField::TempoBaseUrl
        | ConnectionField::JiraSiteUrl
        | ConnectionField::JiraEmail
        | ConnectionField::JiraApiToken => {
            start_connection_edit(state);
            Vec::new()
        }
        ConnectionField::Connect => submit_connection(state),
        ConnectionField::Cancel => reduce_connection(state, ConnectionAction::Cancel),
    }
}

fn start_connection_edit(state: &mut AppState) {
    let snapshot = match state.connection_form.selected_field() {
        ConnectionField::TempoApiToken => state.connection_form.tempo_api_token.value.clone(),
        ConnectionField::TempoBaseUrl => state.connection_form.tempo_base_url.value.clone(),
        ConnectionField::JiraSiteUrl => state.connection_form.jira_site_url.value.clone(),
        ConnectionField::JiraEmail => state.connection_form.jira_email.value.clone(),
        ConnectionField::JiraApiToken => state.connection_form.jira_api_token.value.clone(),
        ConnectionField::Connect | ConnectionField::Cancel => return,
    };
    state.connection_form.edit_snapshot = Some(snapshot);
    state.connection_form.editing = true;
}

fn cancel_connection_edit(state: &mut AppState) -> Vec<Effect> {
    let Some(snapshot) = state.connection_form.edit_snapshot.take() else {
        state.connection_form.editing = false;
        return Vec::new();
    };

    match state.connection_form.selected_field() {
        ConnectionField::TempoApiToken => state.connection_form.tempo_api_token.set(snapshot),
        ConnectionField::TempoBaseUrl => state.connection_form.tempo_base_url.set(snapshot),
        ConnectionField::JiraSiteUrl => state.connection_form.jira_site_url.set(snapshot),
        ConnectionField::JiraEmail => state.connection_form.jira_email.set(snapshot),
        ConnectionField::JiraApiToken => state.connection_form.jira_api_token.set(snapshot),
        ConnectionField::Connect | ConnectionField::Cancel => {}
    }
    state.connection_form.editing = false;
    Vec::new()
}

fn mutate_selected_connection_text(
    state: &mut AppState,
    operation: impl FnOnce(&mut super::state::EditableText),
) {
    match state.connection_form.selected_field() {
        ConnectionField::TempoApiToken => operation(&mut state.connection_form.tempo_api_token),
        ConnectionField::TempoBaseUrl => operation(&mut state.connection_form.tempo_base_url),
        ConnectionField::JiraSiteUrl => operation(&mut state.connection_form.jira_site_url),
        ConnectionField::JiraEmail => operation(&mut state.connection_form.jira_email),
        ConnectionField::JiraApiToken => operation(&mut state.connection_form.jira_api_token),
        ConnectionField::Connect | ConnectionField::Cancel => {}
    }
}

fn submit_connection(state: &mut AppState) -> Vec<Effect> {
    if state.connection.request_id().is_some() {
        state.connection_form.message =
            Some("Connection verification is already in progress.".to_string());
        return Vec::new();
    }

    let jira = JiraSettings::normalized(
        state.connection_form.jira_site_url.value.clone(),
        state.connection_form.jira_email.value.clone(),
        state.connection_form.jira_api_token.value.clone(),
    );
    let tempo_api_token = state
        .connection_form
        .tempo_api_token
        .value
        .trim()
        .to_string();
    let tempo_base_url = state.connection_form.tempo_base_url.value.clone();

    if tempo_api_token.is_empty() {
        state.connection_form.message = Some("Tempo API Token is required.".to_string());
        return Vec::new();
    }
    if jira.site_url.is_empty() {
        state.connection_form.message = Some("Jira Site URL is required.".to_string());
        return Vec::new();
    }
    if jira.email.is_empty() {
        state.connection_form.message = Some("Jira Email is required.".to_string());
        return Vec::new();
    }
    if jira.api_token.is_empty() {
        state.connection_form.message = Some("Jira API Token is required.".to_string());
        return Vec::new();
    }
    if !can_save_connection(state) {
        state.connection_form.message =
            Some("Connection settings are incomplete or invalid.".to_string());
        return Vec::new();
    }

    let request_id = state.next_request_id();
    state.connection = ConnectionState::Connecting { request_id };
    state.connection_form.message = None;
    vec![Effect::ConnectCredentials {
        request_id,
        tempo_api_token,
        tempo_base_url,
        jira,
    }]
}

fn reduce_settings(state: &mut AppState, action: SettingsAction) -> Vec<Effect> {
    match action {
        SettingsAction::Advance(delta) => {
            let count = SettingsField::all().len() as i32;
            let next = (state.settings.selected_field as i32 + delta).rem_euclid(count) as usize;
            state.settings.selected_field = next;
            Vec::new()
        }
        SettingsAction::Adjust(delta) => adjust_setting(state, delta),
        SettingsAction::ActivateSelected => {
            if matches!(
                SettingsField::all()[state.settings.selected_field],
                SettingsField::Connection
            ) {
                open_connection_form(state, true, None);
            }
            Vec::new()
        }
    }
}

fn adjust_setting(state: &mut AppState, delta: i32) -> Vec<Effect> {
    match SettingsField::all()[state.settings.selected_field] {
        SettingsField::DefaultStartTime => {
            state.persisted.preferences.default_start_time =
                adjust_time(state.persisted.preferences.default_start_time, delta);
            vec![Effect::SavePersisted {
                success_message: if state.session_default_start_time.is_some() {
                    "Saved default start time for future sessions.".to_string()
                } else {
                    "Saved default start time.".to_string()
                },
                failure_prefix: "Using in-memory settings only; save failed",
            }]
        }
        SettingsField::ShowEmptyWeekdays => {
            state.persisted.preferences.show_empty_weekdays =
                !state.persisted.preferences.show_empty_weekdays;
            vec![Effect::SavePersisted {
                success_message: "Saved weekday visibility preference.".to_string(),
                failure_prefix: "Using in-memory settings only; save failed",
            }]
        }
        SettingsField::EmptyDayTimeDisplay => {
            state.persisted.preferences.empty_day_time_display = if delta >= 0 {
                state.persisted.preferences.empty_day_time_display.next()
            } else {
                state
                    .persisted
                    .preferences
                    .empty_day_time_display
                    .previous()
            };
            vec![Effect::SavePersisted {
                success_message: "Saved empty-day display preference.".to_string(),
                failure_prefix: "Using in-memory settings only; save failed",
            }]
        }
        SettingsField::Connection => Vec::new(),
    }
}

fn reduce_edit_day(state: &mut AppState, action: EditDayAction) -> Vec<Effect> {
    match action {
        EditDayAction::Cancel => {
            state.edit_day = None;
            state.panel = Panel::None;
            Vec::new()
        }
        EditDayAction::Adjust(delta) => {
            if let Some(edit_day) = &mut state.edit_day {
                let current =
                    parse_edit_time(&edit_day.time_input.value).unwrap_or(edit_day.original_time);
                let adjusted = adjust_time(current, delta);
                edit_day
                    .time_input
                    .set(adjusted.format("%H:%M").to_string());
                edit_day.validation_error = None;
            }
            Vec::new()
        }
        EditDayAction::Backspace => mutate_edit_day(state, |field| field.backspace()),
        EditDayAction::Delete => mutate_edit_day(state, |field| field.delete()),
        EditDayAction::MoveHome => mutate_edit_day(state, |field| field.move_home()),
        EditDayAction::MoveEnd => mutate_edit_day(state, |field| field.move_end()),
        EditDayAction::Reset => {
            if let Some(edit_day) = &mut state.edit_day {
                edit_day
                    .time_input
                    .set(edit_day.baseline_time.format("%H:%M").to_string());
                edit_day.validation_error = None;
            }
            Vec::new()
        }
        EditDayAction::Insert(ch) => mutate_edit_day(state, |field| field.insert_char(ch)),
        EditDayAction::Apply => apply_edit_day(state),
    }
}

fn mutate_edit_day(
    state: &mut AppState,
    operation: impl FnOnce(&mut super::state::EditableText),
) -> Vec<Effect> {
    if let Some(edit_day) = &mut state.edit_day {
        operation(&mut edit_day.time_input);
        edit_day.validation_error = None;
    }
    Vec::new()
}

fn apply_edit_day(state: &mut AppState) -> Vec<Effect> {
    let Some(edit_day) = &mut state.edit_day else {
        return Vec::new();
    };
    let Ok(current_time) = parse_edit_time(&edit_day.time_input.value) else {
        edit_day.validation_error = Some("Enter the start time as HH:MM.".to_string());
        return Vec::new();
    };
    let edit_day = state
        .edit_day
        .take()
        .expect("edit state must still be present");

    if current_time == edit_day.baseline_time {
        state.persisted.day_overrides.remove(&edit_day.date);
    } else {
        state
            .persisted
            .day_overrides
            .insert(edit_day.date, current_time);
    }
    state.panel = Panel::None;
    vec![Effect::SavePersisted {
        success_message: if current_time == edit_day.baseline_time {
            format!(
                "Cleared the saved override for {}. Session default start remains {}.",
                edit_day.date,
                state.effective_default_start_time().format("%H:%M")
            )
        } else {
            format!(
                "Saved override for {} to {}.",
                edit_day.date,
                current_time.format("%H:%M")
            )
        },
        failure_prefix: "Override updated in memory only; save failed",
    }]
}

fn saved_connection_verified(state: &mut AppState) -> Vec<Effect> {
    if !matches!(state.connection, ConnectionState::VerifyingSaved { .. }) {
        return Vec::new();
    }

    state.connection = ConnectionState::Verified;
    state.banner = None;
    state.route = Route::Month;
    request_month_load(state, false)
}

fn saved_connection_rejected(state: &mut AppState, message: String) -> Vec<Effect> {
    let had_loaded_month = state.any_loaded_month();
    let message = format!(
        "Saved connection settings could not be verified: {}. Press s to update them.",
        compact_error_message(&message, 120)
    );
    state.connection = ConnectionState::Invalid {
        message: message.clone(),
    };
    state.loader_available = false;
    state.month_cache.clear();
    state.banner = Some(BannerState {
        tone: BannerTone::Error,
        text: message.clone(),
    });

    if !had_loaded_month {
        transition_to_setup(state, Some(message));
    }
    Vec::new()
}

fn connection_established(
    state: &mut AppState,
    tempo: TempoSettings,
    jira: JiraSettings,
) -> Vec<Effect> {
    state.persisted.tempo = tempo;
    state.persisted.jira = jira;
    state.connection = ConnectionState::Verified;
    state.loader_available = true;
    state.month_cache.clear();
    state.next_request_id = 1;
    state.route = Route::Month;
    state.panel = Panel::None;
    state.connection_form = ConnectionFormState::from_settings(
        &state.persisted.tempo,
        &state.persisted.jira,
        false,
        None,
    );

    let request_id = state.next_request_id();
    let month = state.month.month.clone();
    let entry = state.current_month_entry_mut();
    entry.load_state = MonthLoadState::Loading { request_id };

    vec![
        Effect::SavePersisted {
            success_message: format!(
                "Saved and verified connection settings. Resolved Jira account ID {}.",
                state.persisted.tempo.account_id
            ),
            failure_prefix: "Using in-memory connection only; save failed",
        },
        month_effect(request_id, month),
    ]
}

fn connection_establish_failed(state: &mut AppState, message: String) {
    state.connection = ConnectionState::Invalid {
        message: message.clone(),
    };
    state.connection_form.message = Some(message);
}

fn apply_month_loaded(
    state: &mut AppState,
    request_id: u64,
    month: crate::config::MonthWindow,
    worklogs: Vec<crate::tempo::TempoWorklog>,
) {
    let entry = state
        .month_cache
        .entry(month.label.clone())
        .or_insert_with(|| CachedMonth::new(month.clone()));
    if entry.load_state.request_id() != Some(request_id) {
        return;
    }

    entry.worklogs = worklogs;
    entry.load_state = MonthLoadState::Ready;
    state.connection = ConnectionState::Verified;
    state.loader_available = true;
}

fn apply_month_load_failed(
    state: &mut AppState,
    request_id: u64,
    month: crate::config::MonthWindow,
    message: String,
) {
    let entry = state
        .month_cache
        .entry(month.label.clone())
        .or_insert_with(|| CachedMonth::new(month.clone()));
    if entry.load_state.request_id() != Some(request_id) {
        return;
    }

    let stale = entry.load_state.has_loaded();
    entry.load_state = MonthLoadState::Failed {
        stale,
        message: compact_error_message(&message, 160),
    };
}

fn loader_disconnected(state: &mut AppState, loader_available: bool) {
    for entry in state.month_cache.values_mut() {
        if entry.load_state.request_id().is_some() {
            let stale = entry.load_state.has_loaded();
            entry.load_state = MonthLoadState::Failed {
                stale,
                message: "Tempo background loader stopped unexpectedly. Press r to retry."
                    .to_string(),
            };
        }
    }

    state.loader_available = loader_available;
    state.banner = Some(BannerState {
        tone: BannerTone::Warning,
        text: "Tempo background loader stopped unexpectedly. Press r to retry.".to_string(),
    });
    if !loader_available {
        state.connection = ConnectionState::SavedUnverified;
    }
}

fn connection_runtime_disconnected(state: &mut AppState) {
    match state.connection {
        ConnectionState::Connecting { .. } => {
            state.connection = ConnectionState::Invalid {
                message: "Connection setup stopped unexpectedly. Try again.".to_string(),
            };
            state.connection_form.message =
                Some("Connection setup stopped unexpectedly. Try again.".to_string());
        }
        ConnectionState::VerifyingSaved { .. } => {
            let message =
                "Saved connection verification stopped unexpectedly. Press s to re-enter the settings."
                    .to_string();
            state.loader_available = false;
            state.connection = ConnectionState::Invalid {
                message: message.clone(),
            };
            state.banner = Some(BannerState {
                tone: BannerTone::Error,
                text: message.clone(),
            });
            transition_to_setup(state, Some(message));
        }
        ConnectionState::NeedsSetup
        | ConnectionState::SavedUnverified
        | ConnectionState::Verified
        | ConnectionState::Invalid { .. } => {}
    }
}

fn request_month_load(state: &mut AppState, force_refresh: bool) -> Vec<Effect> {
    if matches!(state.connection, ConnectionState::VerifyingSaved { .. }) {
        state.banner = Some(BannerState {
            tone: BannerTone::Warning,
            text: "Waiting for saved connection verification before loading Tempo data."
                .to_string(),
        });
        return Vec::new();
    }

    if !state.loader_available {
        state.banner = Some(BannerState {
            tone: BannerTone::Warning,
            text: "Tempo or Jira connection is not configured. Press s to open Connection Setup."
                .to_string(),
        });
        return Vec::new();
    }

    let month = state.month.month.clone();
    let request_id = state.next_request_id();
    let entry = state
        .month_cache
        .entry(month.label.clone())
        .or_insert_with(|| CachedMonth::new(month.clone()));

    if !force_refresh {
        match entry.load_state {
            MonthLoadState::Ready
            | MonthLoadState::Loading { .. }
            | MonthLoadState::Refreshing { .. } => return Vec::new(),
            MonthLoadState::Idle | MonthLoadState::Failed { .. } => {}
        }
    }

    let stale = entry.load_state.has_loaded();
    entry.load_state = if stale {
        MonthLoadState::Refreshing { request_id }
    } else {
        MonthLoadState::Loading { request_id }
    };

    vec![month_effect(request_id, month)]
}

fn open_connection_form(state: &mut AppState, can_cancel: bool, message: Option<String>) {
    state.connection_form = ConnectionFormState::from_settings(
        &state.persisted.tempo,
        &state.persisted.jira,
        can_cancel,
        message,
    );
    if state.route == Route::Month {
        state.panel = Panel::ConnectionDrawer;
    }
}

fn transition_to_setup(state: &mut AppState, message: Option<String>) {
    state.route = Route::Setup;
    state.panel = Panel::None;
    state.edit_day = None;
    state.connection_form = ConnectionFormState::from_settings(
        &state.persisted.tempo,
        &state.persisted.jira,
        false,
        message,
    );
}

fn can_save_connection(state: &AppState) -> bool {
    let jira = JiraSettings::normalized(
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

fn compact_error_message(value: &str, max_chars: usize) -> String {
    let single_line = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.chars().count() <= max_chars {
        return single_line;
    }

    single_line
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>()
        + "..."
}

pub fn edit_day_preview_end(edit_day: &EditDayState) -> Option<String> {
    let parsed_time = parse_edit_time(&edit_day.time_input.value).ok()?;
    let tracked_seconds =
        edit_day.worked_seconds + statutory_break_seconds(edit_day.worked_seconds);
    Some(crate::report::format_clock_time(
        i64::from(parsed_time.num_seconds_from_midnight()) + tracked_seconds,
    ))
}
