use std::{collections::BTreeMap, sync::mpsc, time::Duration};

use chrono::NaiveDate;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

use super::{
    action::{Action, ConnectionAction, EditDayAction, SettingsAction},
    input, render,
    runtime::LoaderResponse,
    state::{CachedMonth, ConnectionState, MonthLoadState, Panel, Route, adjust_time},
    *,
};
use crate::{
    config::{AppConfig, MonthWindow, parse_start_time},
    report::MonthlyReport,
    storage::{EmptyDayTimeDisplay, JiraSettings, PersistedState, Preferences, TempoSettings},
    tempo::TempoWorklog,
};

fn worklog(date: &str, seconds: i64) -> TempoWorklog {
    TempoWorklog {
        start_date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
        time_spent_seconds: seconds,
    }
}

fn configured_state() -> PersistedState {
    PersistedState {
        tempo: TempoSettings::normalized(
            "token".to_string(),
            "discovered-account".to_string(),
            crate::storage::DEFAULT_TEMPO_BASE_URL.to_string(),
        ),
        jira: JiraSettings::normalized(
            "https://example.atlassian.net".to_string(),
            "me@example.com".to_string(),
            "jira-token".to_string(),
        ),
        preferences: Preferences::default(),
        day_overrides: BTreeMap::new(),
    }
}

fn app_config() -> AppConfig {
    AppConfig {
        today: NaiveDate::from_ymd_opt(2026, 3, 20).unwrap(),
        initial_month: MonthWindow::from_label("2026-03").unwrap(),
        cli_start_time: None,
    }
}

fn app_config_with_start(start: Option<&str>) -> AppConfig {
    AppConfig {
        cli_start_time: start.map(|value| parse_start_time(value).unwrap()),
        ..app_config()
    }
}

fn successful_discoverer(settings: &JiraSettings) -> Result<String, String> {
    assert_eq!(settings.site_url, "https://example.atlassian.net");
    assert_eq!(settings.email, "me@example.com");
    assert_eq!(settings.api_token, "jira-token");
    Ok("discovered-account".to_string())
}

fn failing_discoverer(_settings: &JiraSettings) -> Result<String, String> {
    Err("Jira discovery failed.".to_string())
}

fn successful_tempo_verifier(
    _settings: &TempoSettings,
    _probe_date: NaiveDate,
) -> Result<(), String> {
    Ok(())
}

fn failing_tempo_verifier(_settings: &TempoSettings, _probe_date: NaiveDate) -> Result<(), String> {
    Err("Tempo validation failed.".to_string())
}

fn test_app(persisted: PersistedState) -> TuiApp {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let store = AppStateStore::new(std::env::temp_dir().join(format!(
        "tempotui-tui-test-app-{}-{unique}.toml",
        std::process::id()
    )));
    TuiApp::new_with_hooks(
        app_config(),
        store,
        persisted,
        successful_discoverer,
        successful_tempo_verifier,
    )
}

fn flush_connection_work(app: &mut TuiApp) {
    for _ in 0..20 {
        app.process_connection_messages();
        if app.state.connection.request_id().is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("connection worker did not finish in time");
}

fn dispatch_key(app: &mut TuiApp, key: KeyEvent) {
    if let Some(action) = input::map_key(&app.state, key) {
        app.dispatch(action);
    }
}

fn render_app(app: &mut TuiApp, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render::draw(frame, &app.state, app.store.path()))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line);
    }

    lines.join("\n")
}

#[test]
fn adjust_time_moves_in_quarter_hour_steps() {
    let time = adjust_time(parse_start_time("09:00").unwrap(), 1);
    assert_eq!(time, parse_start_time("09:15").unwrap());
}

#[test]
fn adjust_time_clamps_to_day_bounds() {
    assert_eq!(
        adjust_time(parse_start_time("00:00").unwrap(), -1),
        parse_start_time("00:00").unwrap()
    );
    assert_eq!(
        adjust_time(parse_start_time("23:45").unwrap(), 1),
        parse_start_time("23:45").unwrap()
    );
}

#[test]
fn start_end_display_blanks_empty_rows_when_configured() {
    let row = crate::report::ReportRow {
        date: NaiveDate::from_ymd_opt(2026, 3, 2).unwrap(),
        worked_seconds: 0,
        break_seconds: 0,
        tracked_seconds: 0,
        effective_start_seconds: 9 * 60 * 60,
        effective_end_seconds: 9 * 60 * 60,
        has_override: false,
        is_empty: true,
    };

    assert_eq!(
        view_model::start_end_display(&row, EmptyDayTimeDisplay::Blank),
        (String::new(), String::new())
    );
}

#[test]
fn start_end_display_can_show_default_start_for_empty_rows() {
    let row = crate::report::ReportRow {
        date: NaiveDate::from_ymd_opt(2026, 3, 2).unwrap(),
        worked_seconds: 0,
        break_seconds: 0,
        tracked_seconds: 0,
        effective_start_seconds: 9 * 60 * 60,
        effective_end_seconds: 9 * 60 * 60,
        has_override: false,
        is_empty: true,
    };

    assert_eq!(
        view_model::start_end_display(&row, EmptyDayTimeDisplay::DefaultStart),
        ("09:00".to_string(), "09:00".to_string())
    );
}

#[test]
fn month_table_rows_insert_week_headers_and_map_selection() {
    let report = MonthlyReport::from_worklogs(
        "2026-03".to_string(),
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
        parse_start_time("09:00").unwrap(),
        false,
        &BTreeMap::new(),
        &[
            worklog("2026-03-02", 8 * 60 * 60),
            worklog("2026-03-09", 7 * 60 * 60),
        ],
    );

    let (rows, selected) = render::build_month_table_rows(
        &report,
        NaiveDate::from_ymd_opt(2026, 3, 20).unwrap(),
        EmptyDayTimeDisplay::Blank,
        1,
    );

    assert!(rows.len() > report.rows.len());
    assert_eq!(selected, Some(3));
}

#[test]
fn app_starts_in_connection_setup_when_tempo_settings_are_missing() {
    let store = AppStateStore::new(std::env::temp_dir().join("tempotui-tui-startup-test.toml"));
    let app = TuiApp::new(app_config(), store, PersistedState::default());

    assert!(matches!(app.state.route, Route::Setup));
    assert!(matches!(app.state.connection, ConnectionState::NeedsSetup));
    assert!(app.loader.is_none());
}

#[test]
fn app_starts_in_connection_setup_when_jira_settings_are_missing() {
    let store =
        AppStateStore::new(std::env::temp_dir().join("tempotui-tui-startup-jira-test.toml"));
    let persisted = PersistedState {
        tempo: TempoSettings::normalized(
            "token".to_string(),
            "discovered-account".to_string(),
            crate::storage::DEFAULT_TEMPO_BASE_URL.to_string(),
        ),
        ..PersistedState::default()
    };

    let app = TuiApp::new(app_config(), store, persisted);

    assert!(matches!(app.state.route, Route::Setup));
    assert!(matches!(app.state.connection, ConnectionState::NeedsSetup));
    assert!(app.loader.is_none());
}

#[test]
fn saving_connection_settings_discovers_account_id_and_opens_month() {
    let store = AppStateStore::new(std::env::temp_dir().join("tempotui-tui-connection-save.toml"));
    let mut app = TuiApp::new_with_hooks(
        app_config(),
        store,
        PersistedState::default(),
        successful_discoverer,
        successful_tempo_verifier,
    );
    app.state
        .connection_form
        .tempo_api_token
        .set("token".to_string());
    app.state
        .connection_form
        .tempo_base_url
        .set(crate::storage::DEFAULT_TEMPO_BASE_URL.to_string());
    app.state
        .connection_form
        .jira_site_url
        .set("example.atlassian.net".to_string());
    app.state
        .connection_form
        .jira_email
        .set("me@example.com".to_string());
    app.state
        .connection_form
        .jira_api_token
        .set("jira-token".to_string());
    app.state.connection_form.selected_field = 5;

    app.dispatch(Action::Connection(ConnectionAction::ActivateSelected));
    flush_connection_work(&mut app);

    assert!(matches!(app.state.route, Route::Month));
    assert_eq!(app.state.panel, Panel::None);
    assert!(app.loader.is_some());
    assert!(matches!(app.state.connection, ConnectionState::Verified));
    assert_eq!(app.state.persisted.tempo.api_token, "token");
    assert_eq!(app.state.persisted.tempo.account_id, "discovered-account");
    assert_eq!(app.state.persisted.jira.email, "me@example.com");
}

#[test]
fn saving_connection_settings_requires_token() {
    let store =
        AppStateStore::new(std::env::temp_dir().join("tempotui-tui-connection-invalid.toml"));
    let mut app = TuiApp::new_with_hooks(
        app_config(),
        store,
        PersistedState::default(),
        failing_discoverer,
        failing_tempo_verifier,
    );
    app.state
        .connection_form
        .jira_site_url
        .set("example.atlassian.net".to_string());
    app.state
        .connection_form
        .jira_email
        .set("me@example.com".to_string());
    app.state
        .connection_form
        .jira_api_token
        .set("jira-token".to_string());
    app.state.connection_form.selected_field = 5;

    app.dispatch(Action::Connection(ConnectionAction::ActivateSelected));

    assert!(matches!(app.state.route, Route::Setup));
    assert_eq!(
        app.state.connection_form.message.as_deref(),
        Some("Enter a Tempo API token.")
    );
}

#[test]
fn saving_connection_settings_requires_successful_tempo_validation() {
    let store =
        AppStateStore::new(std::env::temp_dir().join("tempotui-tui-connection-tempo-invalid.toml"));
    let mut app = TuiApp::new_with_hooks(
        app_config(),
        store,
        PersistedState::default(),
        successful_discoverer,
        failing_tempo_verifier,
    );
    app.state
        .connection_form
        .tempo_api_token
        .set("token".to_string());
    app.state
        .connection_form
        .tempo_base_url
        .set(crate::storage::DEFAULT_TEMPO_BASE_URL.to_string());
    app.state
        .connection_form
        .jira_site_url
        .set("example.atlassian.net".to_string());
    app.state
        .connection_form
        .jira_email
        .set("me@example.com".to_string());
    app.state
        .connection_form
        .jira_api_token
        .set("jira-token".to_string());
    app.state.connection_form.selected_field = 5;

    app.dispatch(Action::Connection(ConnectionAction::ActivateSelected));
    flush_connection_work(&mut app);

    assert!(matches!(app.state.route, Route::Setup));
    assert_eq!(
        app.state.connection_form.message.as_deref(),
        Some("Tempo validation failed.")
    );
    assert!(app.loader.is_none());
}

#[test]
fn saving_connection_settings_allows_blank_tempo_base_url() {
    let store =
        AppStateStore::new(std::env::temp_dir().join("tempotui-tui-connection-blank-base.toml"));
    let mut app = TuiApp::new_with_hooks(
        app_config(),
        store,
        PersistedState::default(),
        successful_discoverer,
        successful_tempo_verifier,
    );
    app.state
        .connection_form
        .tempo_api_token
        .set("token".to_string());
    app.state.connection_form.tempo_base_url.set(String::new());
    app.state
        .connection_form
        .jira_site_url
        .set("example.atlassian.net".to_string());
    app.state
        .connection_form
        .jira_email
        .set("me@example.com".to_string());
    app.state
        .connection_form
        .jira_api_token
        .set("jira-token".to_string());
    app.state.connection_form.selected_field = 5;

    app.dispatch(Action::Connection(ConnectionAction::ActivateSelected));
    flush_connection_work(&mut app);

    assert!(matches!(app.state.route, Route::Month));
    assert_eq!(
        app.state.persisted.tempo.base_url,
        crate::storage::DEFAULT_TEMPO_BASE_URL
    );
}

#[test]
fn saved_connection_is_verified_in_background_on_startup() {
    let mut app = test_app(configured_state());

    assert!(matches!(
        app.state.connection,
        ConnectionState::VerifyingSaved { .. }
    ));

    flush_connection_work(&mut app);

    assert!(matches!(app.state.connection, ConnectionState::Verified));
    assert!(matches!(app.state.route, Route::Month));
}

#[test]
fn startup_verification_failure_returns_to_connection_setup() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let store = AppStateStore::new(std::env::temp_dir().join(format!(
        "tempotui-tui-startup-verify-fail-{}-{unique}.toml",
        std::process::id()
    )));
    let mut app = TuiApp::new_with_hooks(
        app_config(),
        store,
        configured_state(),
        successful_discoverer,
        failing_tempo_verifier,
    );

    flush_connection_work(&mut app);

    assert!(matches!(app.state.route, Route::Setup));
    assert!(matches!(
        app.state.connection,
        ConnectionState::Invalid { .. }
    ));
    assert!(app.loader.is_none());
    assert!(
        app.state
            .connection_form
            .message
            .as_deref()
            .is_some_and(|message| message.contains("Tempo validation failed"))
    );
}

#[test]
fn startup_verification_can_be_skipped_into_connection_setup() {
    let mut app = test_app(configured_state());

    dispatch_key(&mut app, KeyEvent::from(KeyCode::Char('s')));

    assert!(matches!(app.state.route, Route::Setup));
    assert!(matches!(
        app.state.connection,
        ConnectionState::SavedUnverified
    ));
    assert_eq!(
        app.state.connection_form.message.as_deref(),
        Some("Skipped checking your saved connection. Update the settings to continue.")
    );
}

#[test]
fn loader_disconnect_marks_pending_month_as_failed() {
    let mut app = test_app(configured_state());
    flush_connection_work(&mut app);

    let month = app.state.month.month.clone();
    let mut cached = CachedMonth::new(month.clone());
    cached.load_state = MonthLoadState::Loading { request_id: 42 };
    app.state.month_cache.insert(month.label.clone(), cached);

    let (request_tx, request_rx) = mpsc::channel::<LoaderRequest>();
    let (response_tx, response_rx) = mpsc::channel::<LoaderResponse>();
    drop(request_rx);
    drop(response_tx);
    app.loader = Some(LoaderRuntime {
        tx: request_tx,
        rx: response_rx,
    });

    app.process_loader_messages();

    let cached = app.state.month_cache.get(&month.label).unwrap();
    assert!(matches!(cached.load_state, MonthLoadState::Failed { .. }));
}

#[test]
fn settings_adjustment_updates_persisted_preferences() {
    let mut app = test_app(configured_state());
    flush_connection_work(&mut app);
    app.state.panel = Panel::SettingsDrawer;
    app.state.settings.selected_field = 0;
    app.dispatch(Action::Settings(SettingsAction::Adjust(-1)));
    app.state.settings.selected_field = 1;
    app.dispatch(Action::Settings(SettingsAction::Adjust(1)));
    app.state.settings.selected_field = 2;
    app.dispatch(Action::Settings(SettingsAction::Adjust(1)));

    assert_eq!(
        app.state.persisted.preferences.default_start_time,
        parse_start_time("08:45").unwrap()
    );
    assert!(!app.state.persisted.preferences.show_empty_weekdays);
    assert_eq!(
        app.state.persisted.preferences.empty_day_time_display,
        EmptyDayTimeDisplay::DefaultStart
    );
}

#[test]
fn esc_does_not_quit_from_month_view() {
    let mut app = test_app(configured_state());
    flush_connection_work(&mut app);

    dispatch_key(&mut app, KeyEvent::from(KeyCode::Esc));

    assert!(!app.state.should_quit);
}

#[test]
fn ctrl_c_quits_from_month_view() {
    let mut app = test_app(configured_state());
    flush_connection_work(&mut app);

    dispatch_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    );

    assert!(app.state.should_quit);
}

#[test]
fn enter_opens_edit_panel_and_apply_saves_override() {
    let mut app = test_app(configured_state());
    flush_connection_work(&mut app);
    let month = app.state.month.month.clone();
    app.state.month_cache.insert(
        month.label.clone(),
        CachedMonth {
            month: month.clone(),
            worklogs: vec![worklog("2026-03-02", 8 * 60 * 60)],
            load_state: MonthLoadState::Ready,
        },
    );
    app.state.month.selected_row = 0;

    app.dispatch(Action::OpenEditDay);
    assert_eq!(app.state.panel, Panel::EditDayInspector);

    app.dispatch(Action::EditDay(EditDayAction::Adjust(1)));
    app.dispatch(Action::EditDay(EditDayAction::Apply));

    assert_eq!(app.state.panel, Panel::None);
    assert_eq!(
        app.state
            .persisted
            .day_overrides
            .get(&NaiveDate::from_ymd_opt(2026, 3, 2).unwrap())
            .copied(),
        Some(parse_start_time("09:15").unwrap())
    );
}

#[test]
fn ctrl_d_does_not_quit_while_editing_a_day() {
    let mut app = test_app(configured_state());
    flush_connection_work(&mut app);
    let month = app.state.month.month.clone();
    app.state.month_cache.insert(
        month.label.clone(),
        CachedMonth {
            month: month.clone(),
            worklogs: vec![worklog("2026-03-02", 8 * 60 * 60)],
            load_state: MonthLoadState::Ready,
        },
    );
    app.state.month.selected_row = 0;
    app.dispatch(Action::OpenEditDay);

    dispatch_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
    );

    assert!(!app.state.should_quit);
    assert_eq!(app.state.panel, Panel::EditDayInspector);
}

#[test]
fn cli_start_time_overrides_saved_default_for_current_session() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let store = AppStateStore::new(std::env::temp_dir().join(format!(
        "tempotui-tui-cli-start-{}-{unique}.toml",
        std::process::id()
    )));
    let mut persisted = configured_state();
    persisted.preferences.default_start_time = parse_start_time("08:45").unwrap();
    let mut app = TuiApp::new_with_hooks(
        app_config_with_start(Some("07:30")),
        store,
        persisted,
        successful_discoverer,
        successful_tempo_verifier,
    );
    flush_connection_work(&mut app);

    let month = app.state.month.month.clone();
    app.state.month_cache.insert(
        month.label.clone(),
        CachedMonth {
            month,
            worklogs: vec![worklog("2026-03-02", 8 * 60 * 60)],
            load_state: MonthLoadState::Ready,
        },
    );

    let report = app.state.current_report().unwrap();
    assert_eq!(
        report.default_start_time,
        parse_start_time("07:30").unwrap()
    );
    assert_eq!(
        app.state.persisted.preferences.default_start_time,
        parse_start_time("08:45").unwrap()
    );

    app.dispatch(Action::OpenEditDay);
    assert_eq!(
        app.state.edit_day.as_ref().unwrap().baseline_time,
        parse_start_time("07:30").unwrap()
    );
}

#[test]
fn edit_day_requires_valid_hhmm_input() {
    let mut app = test_app(configured_state());
    flush_connection_work(&mut app);
    let month = app.state.month.month.clone();
    app.state.month_cache.insert(
        month.label.clone(),
        CachedMonth {
            month,
            worklogs: vec![worklog("2026-03-02", 8 * 60 * 60)],
            load_state: MonthLoadState::Ready,
        },
    );

    app.dispatch(Action::OpenEditDay);
    app.state
        .edit_day
        .as_mut()
        .unwrap()
        .time_input
        .set("9".to_string());
    app.dispatch(Action::EditDay(EditDayAction::Apply));

    assert_eq!(app.state.panel, Panel::EditDayInspector);
    assert_eq!(
        app.state
            .edit_day
            .as_ref()
            .unwrap()
            .validation_error
            .as_deref(),
        Some("Enter a start time as HH:MM.")
    );
}

#[test]
fn startup_verification_and_narrow_month_view_render_expected_copy() {
    let mut startup = test_app(configured_state());
    let startup_render = render_app(&mut startup, 80, 24);
    assert!(startup_render.contains("Checking"));
    assert!(startup_render.contains("saved Tempo and Jira"));

    flush_connection_work(&mut startup);
    let month = startup.state.month.month.clone();
    startup.state.month_cache.insert(
        month.label.clone(),
        CachedMonth {
            month,
            worklogs: vec![worklog("2026-03-02", 8 * 60 * 60)],
            load_state: MonthLoadState::Ready,
        },
    );
    let month_render = render_app(&mut startup, 80, 24);
    assert!(month_render.contains("Worked"));
    assert!(month_render.contains("Week"));
    assert!(month_render.contains("Adj"));
}

#[test]
fn settings_render_in_a_drawer_without_hiding_the_month_title() {
    let mut app = test_app(configured_state());
    flush_connection_work(&mut app);
    app.dispatch(Action::OpenSettings);

    let render = render_app(&mut app, 120, 30);
    assert!(render.contains("Month"));
    assert!(render.contains("Settings"));
    assert!(render.contains("Preferences"));
}

#[test]
fn edit_mode_renders_in_the_selected_day_panel() {
    let mut app = test_app(configured_state());
    flush_connection_work(&mut app);
    let month = app.state.month.month.clone();
    app.state.month_cache.insert(
        month.label.clone(),
        CachedMonth {
            month,
            worklogs: vec![worklog("2026-03-02", 8 * 60 * 60)],
            load_state: MonthLoadState::Ready,
        },
    );
    app.dispatch(Action::OpenEditDay);

    let render = render_app(&mut app, 120, 30);
    assert!(render.contains("Edit Day"));
    assert!(render.contains("Start time"));
    assert!(render.contains("Ends at"));
}
