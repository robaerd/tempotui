use std::collections::BTreeMap;

use chrono::{NaiveDate, NaiveTime, Timelike};

use crate::{
    config::{AppConfig, MonthWindow, parse_start_time},
    report::MonthlyReport,
    storage::{JiraSettings, PersistedState, TempoSettings},
    tempo::TempoWorklog,
};

pub(super) const START_TIME_STEP_MINUTES: i64 = 15;

const CONNECTION_FIELDS_NO_CANCEL: [ConnectionField; 6] = [
    ConnectionField::TempoApiToken,
    ConnectionField::TempoBaseUrl,
    ConnectionField::JiraSiteUrl,
    ConnectionField::JiraEmail,
    ConnectionField::JiraApiToken,
    ConnectionField::Connect,
];

const CONNECTION_FIELDS_WITH_CANCEL: [ConnectionField; 7] = [
    ConnectionField::TempoApiToken,
    ConnectionField::TempoBaseUrl,
    ConnectionField::JiraSiteUrl,
    ConnectionField::JiraEmail,
    ConnectionField::JiraApiToken,
    ConnectionField::Connect,
    ConnectionField::Cancel,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Route {
    Setup,
    Month,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Panel {
    None,
    SettingsDrawer,
    HelpDrawer,
    ConnectionDrawer,
    EditDayInspector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SettingsField {
    DefaultStartTime,
    ShowEmptyWeekdays,
    EmptyDayTimeDisplay,
    Connection,
}

impl SettingsField {
    pub(super) fn all() -> [Self; 4] {
        [
            Self::DefaultStartTime,
            Self::ShowEmptyWeekdays,
            Self::EmptyDayTimeDisplay,
            Self::Connection,
        ]
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::DefaultStartTime => "Default Start",
            Self::ShowEmptyWeekdays => "Show Empty Weekdays",
            Self::EmptyDayTimeDisplay => "Empty Days",
            Self::Connection => "Connection",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConnectionField {
    TempoApiToken,
    TempoBaseUrl,
    JiraSiteUrl,
    JiraEmail,
    JiraApiToken,
    Connect,
    Cancel,
}

impl ConnectionField {
    pub(super) fn all(can_cancel: bool) -> &'static [Self] {
        if can_cancel {
            &CONNECTION_FIELDS_WITH_CANCEL
        } else {
            &CONNECTION_FIELDS_NO_CANCEL
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::TempoApiToken => "Tempo Token",
            Self::TempoBaseUrl => "Tempo API URL",
            Self::JiraSiteUrl => "Jira Site",
            Self::JiraEmail => "Jira Email",
            Self::JiraApiToken => "Jira API Token",
            Self::Connect => "Verify & Save",
            Self::Cancel => "Cancel",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct EditableText {
    pub(super) value: String,
    pub(super) cursor: usize,
}

impl EditableText {
    pub(super) fn new(value: String) -> Self {
        let cursor = value.len();
        Self { value, cursor }
    }

    pub(super) fn set(&mut self, value: String) {
        self.value = value;
        self.cursor = self.value.len();
    }

    pub(super) fn move_left(&mut self) {
        self.cursor = previous_char_boundary(&self.value, self.cursor);
    }

    pub(super) fn move_right(&mut self) {
        self.cursor = next_char_boundary(&self.value, self.cursor);
    }

    pub(super) fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub(super) fn move_end(&mut self) {
        self.cursor = self.value.len();
    }

    pub(super) fn insert_char(&mut self, ch: char) {
        self.value.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    pub(super) fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let previous = previous_char_boundary(&self.value, self.cursor);
        self.value.replace_range(previous..self.cursor, "");
        self.cursor = previous;
    }

    pub(super) fn delete(&mut self) {
        if self.cursor >= self.value.len() {
            return;
        }
        let next = next_char_boundary(&self.value, self.cursor);
        self.value.replace_range(self.cursor..next, "");
    }

    pub(super) fn with_cursor(&self) -> String {
        let mut display = self.value.clone();
        display.insert(self.cursor, '|');
        display
    }
}

#[derive(Debug, Clone)]
pub(super) struct ConnectionFormState {
    pub(super) selected_field: usize,
    pub(super) editing: bool,
    pub(super) can_cancel: bool,
    pub(super) tempo_api_token: EditableText,
    pub(super) tempo_base_url: EditableText,
    pub(super) jira_site_url: EditableText,
    pub(super) jira_email: EditableText,
    pub(super) jira_api_token: EditableText,
    pub(super) message: Option<String>,
    pub(super) edit_snapshot: Option<String>,
}

impl ConnectionFormState {
    pub(super) fn from_settings(
        tempo: &TempoSettings,
        jira: &JiraSettings,
        can_cancel: bool,
        message: Option<String>,
    ) -> Self {
        Self {
            selected_field: 0,
            editing: false,
            can_cancel,
            tempo_api_token: EditableText::new(tempo.api_token.clone()),
            tempo_base_url: EditableText::new(tempo.base_url.clone()),
            jira_site_url: EditableText::new(jira.site_url.clone()),
            jira_email: EditableText::new(jira.email.clone()),
            jira_api_token: EditableText::new(jira.api_token.clone()),
            message,
            edit_snapshot: None,
        }
    }

    pub(super) fn selected_field(&self) -> ConnectionField {
        ConnectionField::all(self.can_cancel)[self.selected_field]
    }
}

#[derive(Debug, Clone)]
pub(super) struct SettingsState {
    pub(super) selected_field: usize,
}

#[derive(Debug, Clone)]
pub(super) struct MonthState {
    pub(super) month: MonthWindow,
    pub(super) selected_row: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MonthLoadState {
    Idle,
    Loading { request_id: u64 },
    Ready,
    Refreshing { request_id: u64 },
    Failed { stale: bool, message: String },
}

impl MonthLoadState {
    pub(super) fn request_id(&self) -> Option<u64> {
        match self {
            Self::Loading { request_id } | Self::Refreshing { request_id } => Some(*request_id),
            Self::Idle | Self::Ready | Self::Failed { .. } => None,
        }
    }

    pub(super) fn has_loaded(&self) -> bool {
        matches!(
            self,
            Self::Ready | Self::Refreshing { .. } | Self::Failed { stale: true, .. }
        )
    }
}

#[derive(Debug, Clone)]
pub(super) struct CachedMonth {
    pub(super) month: MonthWindow,
    pub(super) worklogs: Vec<TempoWorklog>,
    pub(super) load_state: MonthLoadState,
}

impl CachedMonth {
    pub(super) fn new(month: MonthWindow) -> Self {
        Self {
            month,
            worklogs: Vec::new(),
            load_state: MonthLoadState::Idle,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct EditDayState {
    pub(super) date: NaiveDate,
    pub(super) worked_seconds: i64,
    pub(super) time_input: EditableText,
    pub(super) original_time: NaiveTime,
    pub(super) baseline_time: NaiveTime,
    pub(super) validation_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BannerTone {
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub(super) struct BannerState {
    pub(super) tone: BannerTone,
    pub(super) text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ConnectionState {
    NeedsSetup,
    SavedUnverified,
    VerifyingSaved { request_id: u64 },
    Connecting { request_id: u64 },
    Verified,
    Invalid { message: String },
}

impl ConnectionState {
    pub(super) fn request_id(&self) -> Option<u64> {
        match self {
            Self::VerifyingSaved { request_id } | Self::Connecting { request_id } => {
                Some(*request_id)
            }
            Self::NeedsSetup | Self::SavedUnverified | Self::Verified | Self::Invalid { .. } => {
                None
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct AppState {
    pub(super) today: NaiveDate,
    pub(super) persisted: PersistedState,
    pub(super) session_default_start_time: Option<NaiveTime>,
    pub(super) route: Route,
    pub(super) panel: Panel,
    pub(super) connection_form: ConnectionFormState,
    pub(super) settings: SettingsState,
    pub(super) month: MonthState,
    pub(super) month_cache: BTreeMap<String, CachedMonth>,
    pub(super) edit_day: Option<EditDayState>,
    pub(super) connection: ConnectionState,
    pub(super) banner: Option<BannerState>,
    pub(super) should_quit: bool,
    pub(super) next_request_id: u64,
    pub(super) loader_available: bool,
}

impl AppState {
    pub(super) fn new(
        config: AppConfig,
        persisted: PersistedState,
        loader_available: bool,
    ) -> Self {
        let setup_complete = persisted.tempo.is_configured() && persisted.jira.is_configured();
        let (route, connection, message) = match (loader_available, setup_complete) {
            (true, true) => (Route::Month, ConnectionState::SavedUnverified, None),
            (false, true) => (
                Route::Setup,
                ConnectionState::Invalid {
                    message:
                        "Your saved Tempo or Jira settings are missing or invalid. Update them to continue."
                            .to_string(),
                },
                Some(
                    "Your saved Tempo or Jira settings are missing or invalid. Update them to continue."
                        .to_string(),
                ),
            ),
            (_, false) => (
                Route::Setup,
                ConnectionState::NeedsSetup,
                Some(
                    "Enter your Tempo and Jira settings. We'll look up your Atlassian account ID automatically."
                        .to_string(),
                ),
            ),
        };

        let connection_form =
            ConnectionFormState::from_settings(&persisted.tempo, &persisted.jira, false, message);

        Self {
            today: config.today,
            persisted,
            session_default_start_time: config.cli_start_time,
            route,
            panel: Panel::None,
            connection_form,
            settings: SettingsState { selected_field: 0 },
            month: MonthState {
                month: config.initial_month,
                selected_row: 0,
            },
            month_cache: BTreeMap::new(),
            edit_day: None,
            connection,
            banner: None,
            should_quit: false,
            next_request_id: 1,
            loader_available,
        }
    }

    pub(super) fn next_request_id(&mut self) -> u64 {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        request_id
    }

    pub(super) fn setup_complete(&self) -> bool {
        self.persisted.tempo.is_configured() && self.persisted.jira.is_configured()
    }

    pub(super) fn effective_default_start_time(&self) -> NaiveTime {
        self.session_default_start_time
            .unwrap_or(self.persisted.preferences.default_start_time)
    }

    pub(super) fn active_default_start_label(&self) -> String {
        let active = self
            .effective_default_start_time()
            .format("%H:%M")
            .to_string();
        if self.session_default_start_time.is_some() {
            format!("{active} this session")
        } else {
            active
        }
    }

    pub(super) fn default_start_status_message(&self) -> &'static str {
        if self.session_default_start_time.is_some() {
            "This session is using --start. Saved changes apply the next time you open the app."
        } else {
            "Sets the starting point for new days and optional empty-day times."
        }
    }

    pub(super) fn any_loaded_month(&self) -> bool {
        self.month_cache
            .values()
            .any(|entry| entry.load_state.has_loaded())
    }

    pub(super) fn current_report(&self) -> Option<MonthlyReport> {
        let cached = self.month_cache.get(&self.month.month.label)?;
        if !cached.load_state.has_loaded() {
            return None;
        }

        Some(MonthlyReport::from_worklogs(
            cached.month.label.clone(),
            cached.month.start,
            cached.month.end,
            self.effective_default_start_time(),
            self.persisted.preferences.show_empty_weekdays,
            &self.persisted.day_overrides,
            &cached.worklogs,
        ))
    }

    pub(super) fn current_month_entry(&self) -> Option<&CachedMonth> {
        self.month_cache.get(&self.month.month.label)
    }

    pub(super) fn current_month_entry_mut(&mut self) -> &mut CachedMonth {
        self.month_cache
            .entry(self.month.month.label.clone())
            .or_insert_with(|| CachedMonth::new(self.month.month.clone()))
    }

    pub(super) fn sync_selection(&mut self) {
        if let Some(report) = self.current_report() {
            if report.rows.is_empty() {
                self.month.selected_row = 0;
            } else {
                self.month.selected_row = self
                    .month
                    .selected_row
                    .min(report.rows.len().saturating_sub(1));
            }
        } else {
            self.month.selected_row = 0;
        }
    }
}

pub(super) fn adjust_time(time: NaiveTime, delta_steps: i32) -> NaiveTime {
    let minutes = i64::from(time.hour()) * 60 + i64::from(time.minute());
    let adjusted =
        (minutes + i64::from(delta_steps) * START_TIME_STEP_MINUTES).clamp(0, 23 * 60 + 45);
    NaiveTime::from_hms_opt((adjusted / 60) as u32, (adjusted % 60) as u32, 0)
        .expect("adjusted time should stay in HH:MM range")
}

pub(super) fn parse_edit_time(value: &str) -> Result<NaiveTime, String> {
    parse_start_time(value).map_err(|_| "Enter a start time as HH:MM.".to_string())
}

fn previous_char_boundary(value: &str, cursor: usize) -> usize {
    value[..cursor]
        .char_indices()
        .last()
        .map_or(0, |(index, _)| index)
}

fn next_char_boundary(value: &str, cursor: usize) -> usize {
    if cursor >= value.len() {
        return value.len();
    }

    cursor + value[cursor..].chars().next().map_or(0, |ch| ch.len_utf8())
}
