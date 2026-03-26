use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::{NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::parse_start_time;

pub const DEFAULT_TEMPO_BASE_URL: &str = "https://api.eu.tempo.io";

const CURRENT_STATE_VERSION: u32 = 1;
const DEFAULT_START_TIME: &str = "09:00";
const CONFIG_FILE_NAME: &str = "config.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EmptyDayTimeDisplay {
    #[default]
    Blank,
    DefaultStart,
}

impl EmptyDayTimeDisplay {
    pub fn label(self) -> &'static str {
        match self {
            Self::Blank => "Blank",
            Self::DefaultStart => "Default Start",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Blank => Self::DefaultStart,
            Self::DefaultStart => Self::Blank,
        }
    }

    pub fn previous(self) -> Self {
        self.next()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preferences {
    pub default_start_time: NaiveTime,
    pub show_empty_weekdays: bool,
    pub empty_day_time_display: EmptyDayTimeDisplay,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            default_start_time: parse_start_time(DEFAULT_START_TIME)
                .expect("default start time should always be valid"),
            show_empty_weekdays: true,
            empty_day_time_display: EmptyDayTimeDisplay::Blank,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TempoSettings {
    pub api_token: String,
    pub account_id: String,
    pub base_url: String,
}

impl Default for TempoSettings {
    fn default() -> Self {
        Self {
            api_token: String::new(),
            account_id: String::new(),
            base_url: DEFAULT_TEMPO_BASE_URL.to_string(),
        }
    }
}

impl TempoSettings {
    pub fn is_configured(&self) -> bool {
        !self.api_token.trim().is_empty() && !self.account_id.trim().is_empty()
    }

    pub fn normalized(api_token: String, account_id: String, base_url: String) -> Self {
        Self {
            api_token: api_token.trim().to_string(),
            account_id: account_id.trim().to_string(),
            base_url: normalize_tempo_base_url(&base_url),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JiraSettings {
    pub site_url: String,
    pub email: String,
    pub api_token: String,
}

impl JiraSettings {
    pub fn is_configured(&self) -> bool {
        !self.site_url.trim().is_empty()
            && !self.email.trim().is_empty()
            && !self.api_token.trim().is_empty()
    }

    pub fn normalized(site_url: String, email: String, api_token: String) -> Self {
        Self {
            site_url: normalize_jira_site_url(&site_url),
            email: email.trim().to_string(),
            api_token: api_token.trim().to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PersistedState {
    pub tempo: TempoSettings,
    pub jira: JiraSettings,
    pub preferences: Preferences,
    pub day_overrides: BTreeMap<NaiveDate, NaiveTime>,
}

#[derive(Debug, Clone)]
pub struct AppStateStore {
    path: PathBuf,
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Could not determine a config directory for TempoTUI.")]
    ConfigDirUnavailable,
    #[error("Failed to read state from `{path}`: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to write state to `{path}`: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to parse TOML state file `{path}`: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("Failed to encode TOML state file `{path}`: {source}")]
    Encode {
        path: PathBuf,
        #[source]
        source: toml::ser::Error,
    },
    #[error("Invalid saved default start time `{value}` in `{path}`.")]
    InvalidDefaultStartTime { path: PathBuf, value: String },
    #[error("Invalid saved override date `{value}` in `{path}`.")]
    InvalidOverrideDate { path: PathBuf, value: String },
    #[error("Invalid saved override time `{value}` in `{path}`.")]
    InvalidOverrideTime { path: PathBuf, value: String },
}

#[derive(Debug, Deserialize, Serialize)]
struct StoredState {
    #[serde(default = "default_state_version")]
    version: u32,
    #[serde(default)]
    tempo: StoredTempoSettings,
    #[serde(default)]
    jira: StoredJiraSettings,
    #[serde(default)]
    preferences: StoredPreferences,
    #[serde(default)]
    day_overrides: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct StoredTempoSettings {
    #[serde(default)]
    api_token: String,
    #[serde(default)]
    account_id: String,
    #[serde(default = "default_tempo_base_url_string")]
    base_url: String,
}

impl Default for StoredTempoSettings {
    fn default() -> Self {
        Self {
            api_token: String::new(),
            account_id: String::new(),
            base_url: default_tempo_base_url_string(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Default)]
struct StoredJiraSettings {
    #[serde(default)]
    site_url: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    api_token: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct StoredPreferences {
    #[serde(default = "default_start_time_string")]
    default_start_time: String,
    #[serde(default = "default_show_empty_weekdays")]
    show_empty_weekdays: bool,
    #[serde(default)]
    empty_day_time_display: EmptyDayTimeDisplay,
}

impl Default for StoredPreferences {
    fn default() -> Self {
        Self {
            default_start_time: default_start_time_string(),
            show_empty_weekdays: default_show_empty_weekdays(),
            empty_day_time_display: EmptyDayTimeDisplay::Blank,
        }
    }
}

impl AppStateStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn from_default_location() -> Result<Self, StorageError> {
        Ok(Self {
            path: default_config_path()?,
        })
    }

    pub fn load(&self) -> Result<PersistedState, StorageError> {
        if !self.path.exists() {
            return Ok(PersistedState::default());
        }

        let contents = fs::read_to_string(&self.path).map_err(|source| StorageError::Read {
            path: self.path.clone(),
            source,
        })?;
        let stored: StoredState =
            toml::from_str(&contents).map_err(|source| StorageError::Parse {
                path: self.path.clone(),
                source,
            })?;

        stored_state_to_persisted(&self.path, stored)
    }

    pub fn save(&self, state: &PersistedState) -> Result<(), StorageError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|source| StorageError::Write {
                path: parent.to_path_buf(),
                source,
            })?;
            if should_restrict_parent_permissions(&self.path) {
                restrict_directory_permissions(parent).map_err(|source| StorageError::Write {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
        }

        let stored = StoredState {
            version: CURRENT_STATE_VERSION,
            tempo: StoredTempoSettings {
                api_token: state.tempo.api_token.clone(),
                account_id: state.tempo.account_id.clone(),
                base_url: normalize_tempo_base_url(&state.tempo.base_url),
            },
            jira: StoredJiraSettings {
                site_url: normalize_jira_site_url(&state.jira.site_url),
                email: state.jira.email.clone(),
                api_token: state.jira.api_token.clone(),
            },
            preferences: StoredPreferences {
                default_start_time: state
                    .preferences
                    .default_start_time
                    .format("%H:%M")
                    .to_string(),
                show_empty_weekdays: state.preferences.show_empty_weekdays,
                empty_day_time_display: state.preferences.empty_day_time_display,
            },
            day_overrides: state
                .day_overrides
                .iter()
                .map(|(date, time)| {
                    (
                        date.format("%Y-%m-%d").to_string(),
                        time.format("%H:%M").to_string(),
                    )
                })
                .collect(),
        };

        let toml = toml::to_string_pretty(&stored).map_err(|source| StorageError::Encode {
            path: self.path.clone(),
            source,
        })?;

        write_restricted_file(&self.path, &toml).map_err(|source| StorageError::Write {
            path: self.path.clone(),
            source,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn stored_state_to_persisted(
    path: &Path,
    stored: StoredState,
) -> Result<PersistedState, StorageError> {
    let default_start_time =
        parse_start_time(&stored.preferences.default_start_time).map_err(|_| {
            StorageError::InvalidDefaultStartTime {
                path: path.to_path_buf(),
                value: stored.preferences.default_start_time.clone(),
            }
        })?;

    let mut day_overrides = BTreeMap::new();
    for (date, time) in stored.day_overrides {
        let parsed_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|_| {
            StorageError::InvalidOverrideDate {
                path: path.to_path_buf(),
                value: date.clone(),
            }
        })?;
        let parsed_time =
            parse_start_time(&time).map_err(|_| StorageError::InvalidOverrideTime {
                path: path.to_path_buf(),
                value: time.clone(),
            })?;
        day_overrides.insert(parsed_date, parsed_time);
    }

    Ok(PersistedState {
        tempo: TempoSettings::normalized(
            stored.tempo.api_token,
            stored.tempo.account_id,
            stored.tempo.base_url,
        ),
        jira: JiraSettings::normalized(
            stored.jira.site_url,
            stored.jira.email,
            stored.jira.api_token,
        ),
        preferences: Preferences {
            default_start_time,
            show_empty_weekdays: stored.preferences.show_empty_weekdays,
            empty_day_time_display: stored.preferences.empty_day_time_display,
        },
        day_overrides,
    })
}

fn normalize_tempo_base_url(value: &str) -> String {
    let trimmed = value.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        DEFAULT_TEMPO_BASE_URL.to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_jira_site_url(value: &str) -> String {
    let trimmed = value.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    }
}

fn resolve_config_root() -> Result<PathBuf, StorageError> {
    if let Some(path) = env::var_os("XDG_CONFIG_HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path).join("tempotui"));
    }

    if let Some(home) = env::var_os("HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(home).join(".config").join("tempotui"));
    }

    Err(StorageError::ConfigDirUnavailable)
}

fn default_config_path() -> Result<PathBuf, StorageError> {
    Ok(resolve_config_root()?.join(CONFIG_FILE_NAME))
}

fn default_show_empty_weekdays() -> bool {
    true
}

fn default_start_time_string() -> String {
    DEFAULT_START_TIME.to_string()
}

fn default_tempo_base_url_string() -> String {
    DEFAULT_TEMPO_BASE_URL.to_string()
}

fn default_state_version() -> u32 {
    CURRENT_STATE_VERSION
}

fn should_restrict_parent_permissions(path: &Path) -> bool {
    path.file_name().and_then(|value| value.to_str()) == Some(CONFIG_FILE_NAME)
}

fn write_restricted_file(path: &Path, contents: &str) -> std::io::Result<()> {
    let temporary_path = temporary_config_path(path);
    let mut file = create_restricted_file(&temporary_path)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    fs::rename(&temporary_path, path)?;
    restrict_file_permissions(path)
}

fn temporary_config_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(CONFIG_FILE_NAME);
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    path.with_file_name(format!(".{file_name}.tmp-{}-{unique}", std::process::id()))
}

#[cfg(unix)]
fn create_restricted_file(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_restricted_file(path: &Path) -> std::io::Result<fs::File> {
    fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
}

#[cfg(unix)]
fn restrict_directory_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_directory_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn restrict_file_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_file_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!(
            "tempotui-storage-{name}-{}-{unique}.toml",
            std::process::id()
        ))
    }

    fn nested_temp_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir()
            .join(format!(
                "tempotui-storage-dir-{name}-{}-{unique}",
                std::process::id()
            ))
            .join(CONFIG_FILE_NAME)
    }

    #[test]
    fn load_returns_defaults_when_file_is_missing() {
        let store = AppStateStore::new(temp_path("missing"));
        let state = store.load().unwrap();

        assert_eq!(state.tempo, TempoSettings::default());
        assert_eq!(state.jira, JiraSettings::default());
        assert_eq!(state.preferences, Preferences::default());
        assert!(state.day_overrides.is_empty());
    }

    #[test]
    fn save_and_load_round_trip() {
        let path = temp_path("round-trip");
        let store = AppStateStore::new(path.clone());
        let mut state = PersistedState {
            tempo: TempoSettings::normalized(
                "tempo-token".to_string(),
                "account-id".to_string(),
                "https://api.eu.tempo.io/".to_string(),
            ),
            jira: JiraSettings::normalized(
                "rewe.atlassian.net".to_string(),
                "me@example.com".to_string(),
                "jira-token".to_string(),
            ),
            ..PersistedState::default()
        };
        state.preferences.default_start_time = parse_start_time("08:30").unwrap();
        state.preferences.empty_day_time_display = EmptyDayTimeDisplay::DefaultStart;
        state.day_overrides.insert(
            NaiveDate::from_ymd_opt(2026, 3, 3).unwrap(),
            parse_start_time("09:30").unwrap(),
        );

        store.save(&state).unwrap();
        let loaded = store.load().unwrap();

        assert_eq!(loaded, state);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn save_writes_version_one() {
        let path = temp_path("version");
        let store = AppStateStore::new(path.clone());

        store.save(&PersistedState::default()).unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.starts_with("version = 1\n"));

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn jira_site_url_normalization_adds_https() {
        let settings = JiraSettings::normalized(
            "rewe.atlassian.net/".to_string(),
            "me@example.com".to_string(),
            "jira-token".to_string(),
        );

        assert_eq!(settings.site_url, "https://rewe.atlassian.net");
    }

    #[cfg(unix)]
    #[test]
    fn save_restricts_permissions_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let path = nested_temp_path("permissions");
        let store = AppStateStore::new(path.clone());

        store.save(&PersistedState::default()).unwrap();

        let directory_mode = fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let file_mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;

        assert_eq!(directory_mode, 0o700);
        assert_eq!(file_mode, 0o600);

        fs::remove_file(&path).unwrap();
        fs::remove_dir(path.parent().unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn save_restricts_permissions_on_existing_directory_and_file() {
        use std::os::unix::fs::PermissionsExt;

        let path = nested_temp_path("existing-permissions");
        let parent = path.parent().unwrap();
        fs::create_dir_all(parent).unwrap();
        fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(&path, "old config").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        let store = AppStateStore::new(path.clone());
        store.save(&PersistedState::default()).unwrap();

        let directory_mode = fs::metadata(parent).unwrap().permissions().mode() & 0o777;
        let file_mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;

        assert_eq!(directory_mode, 0o700);
        assert_eq!(file_mode, 0o600);

        fs::remove_file(&path).unwrap();
        fs::remove_dir(parent).unwrap();
    }
}
