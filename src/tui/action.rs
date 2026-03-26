use crate::{
    config::MonthWindow,
    storage::{JiraSettings, TempoSettings},
    tempo::TempoWorklog,
};

#[derive(Debug, Clone)]
pub enum Action {
    Boot,
    ForceQuit,
    ClosePanel,
    ToggleHelp,
    OpenSettings,
    SkipSavedVerification,
    NavigateMonth(i32),
    JumpToCurrentMonth,
    MoveSelection(i32),
    RefreshMonth,
    OpenEditDay,
    Connection(ConnectionAction),
    Settings(SettingsAction),
    EditDay(EditDayAction),
    SavedConnectionVerified,
    SavedConnectionRejected {
        message: String,
    },
    ConnectionEstablished {
        tempo: TempoSettings,
        jira: JiraSettings,
    },
    ConnectionEstablishFailed {
        message: String,
    },
    PersistedSaveSucceeded {
        message: String,
    },
    PersistedSaveFailed {
        message: String,
    },
    MonthLoaded {
        request_id: u64,
        month: MonthWindow,
        worklogs: Vec<TempoWorklog>,
    },
    MonthLoadFailed {
        request_id: u64,
        month: MonthWindow,
        message: String,
    },
    LoaderDisconnected {
        loader_available: bool,
    },
    ConnectionRuntimeDisconnected,
}

#[derive(Debug, Clone)]
pub enum ConnectionAction {
    Cancel,
    AdvanceField(i32),
    ActivateSelected,
    FinishEdit,
    CancelEdit,
    MoveCursorLeft,
    MoveCursorRight,
    MoveCursorHome,
    MoveCursorEnd,
    Backspace,
    Delete,
    Insert(char),
}

#[derive(Debug, Clone)]
pub enum SettingsAction {
    Advance(i32),
    Adjust(i32),
    ActivateSelected,
}

#[derive(Debug, Clone)]
pub enum EditDayAction {
    Cancel,
    Adjust(i32),
    Backspace,
    Delete,
    MoveHome,
    MoveEnd,
    Reset,
    Insert(char),
    Apply,
}

#[derive(Debug, Clone)]
pub enum Effect {
    VerifySavedConnection {
        request_id: u64,
        tempo: TempoSettings,
    },
    ConnectCredentials {
        request_id: u64,
        tempo_api_token: String,
        tempo_base_url: String,
        jira: JiraSettings,
    },
    LoadMonth {
        request_id: u64,
        month: MonthWindow,
    },
    SavePersisted {
        success_message: String,
        failure_prefix: &'static str,
    },
}

pub fn month_effect(request_id: u64, month: MonthWindow) -> Effect {
    Effect::LoadMonth { request_id, month }
}
