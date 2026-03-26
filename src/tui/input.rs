use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{
    action::{Action, ConnectionAction, EditDayAction, SettingsAction},
    state::{AppState, ConnectionState, Panel, Route},
};

pub(super) fn map_key(state: &AppState, key: KeyEvent) -> Option<Action> {
    if is_force_quit_key(key) {
        return Some(Action::ForceQuit);
    }

    match state.route {
        Route::Setup => map_connection_key(state, key),
        Route::Month => match state.panel {
            Panel::None => map_month_key(state, key),
            Panel::SettingsDrawer => map_settings_key(key),
            Panel::HelpDrawer => map_help_key(key),
            Panel::ConnectionDrawer => map_connection_key(state, key),
            Panel::EditDayInspector => map_edit_day_key(key),
        },
    }
}

fn map_month_key(state: &AppState, key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('q') => Some(Action::ForceQuit),
        KeyCode::Esc => None,
        KeyCode::Char('s') => {
            if matches!(state.connection, ConnectionState::VerifyingSaved { .. }) {
                Some(Action::SkipSavedVerification)
            } else {
                Some(Action::OpenSettings)
            }
        }
        KeyCode::Char('?') => Some(Action::ToggleHelp),
        KeyCode::Left => Some(Action::NavigateMonth(-1)),
        KeyCode::Right => Some(Action::NavigateMonth(1)),
        KeyCode::Home => Some(Action::JumpToCurrentMonth),
        KeyCode::Up => Some(Action::MoveSelection(-1)),
        KeyCode::Down => Some(Action::MoveSelection(1)),
        KeyCode::Enter => Some(Action::OpenEditDay),
        KeyCode::Char('r') => Some(Action::RefreshMonth),
        _ => None,
    }
}

fn map_settings_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('q') => Some(Action::ForceQuit),
        KeyCode::Esc | KeyCode::Char('s') => Some(Action::ClosePanel),
        KeyCode::Char('?') => Some(Action::ToggleHelp),
        KeyCode::Tab | KeyCode::Down => Some(Action::Settings(SettingsAction::Advance(1))),
        KeyCode::BackTab | KeyCode::Up => Some(Action::Settings(SettingsAction::Advance(-1))),
        KeyCode::Left => Some(Action::Settings(SettingsAction::Adjust(-1))),
        KeyCode::Right => Some(Action::Settings(SettingsAction::Adjust(1))),
        KeyCode::Enter => Some(Action::Settings(SettingsAction::ActivateSelected)),
        _ => None,
    }
}

fn map_help_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('q') => Some(Action::ForceQuit),
        KeyCode::Esc | KeyCode::Char('?') => Some(Action::ClosePanel),
        _ => None,
    }
}

fn map_edit_day_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('q') => Some(Action::ForceQuit),
        KeyCode::Esc => Some(Action::EditDay(EditDayAction::Cancel)),
        KeyCode::Left => Some(Action::EditDay(EditDayAction::Adjust(-1))),
        KeyCode::Right => Some(Action::EditDay(EditDayAction::Adjust(1))),
        KeyCode::Backspace => Some(Action::EditDay(EditDayAction::Backspace)),
        KeyCode::Delete => Some(Action::EditDay(EditDayAction::Delete)),
        KeyCode::Home => Some(Action::EditDay(EditDayAction::MoveHome)),
        KeyCode::End => Some(Action::EditDay(EditDayAction::MoveEnd)),
        KeyCode::Char('0') => Some(Action::EditDay(EditDayAction::Reset)),
        KeyCode::Char(ch)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && matches!(ch, '0'..='9' | ':') =>
        {
            Some(Action::EditDay(EditDayAction::Insert(ch)))
        }
        KeyCode::Enter => Some(Action::EditDay(EditDayAction::Apply)),
        _ => None,
    }
}

fn map_connection_key(state: &AppState, key: KeyEvent) -> Option<Action> {
    if matches!(state.connection, ConnectionState::Connecting { .. }) {
        return match key.code {
            KeyCode::Char('q') => Some(Action::ForceQuit),
            KeyCode::Esc => Some(Action::Connection(ConnectionAction::Cancel)),
            _ => None,
        };
    }

    if state.connection_form.editing {
        return map_connection_edit_key(key);
    }

    match key.code {
        KeyCode::Char('q') => Some(Action::ForceQuit),
        KeyCode::Esc => Some(Action::Connection(ConnectionAction::Cancel)),
        KeyCode::Tab | KeyCode::Down => Some(Action::Connection(ConnectionAction::AdvanceField(1))),
        KeyCode::BackTab | KeyCode::Up => {
            Some(Action::Connection(ConnectionAction::AdvanceField(-1)))
        }
        KeyCode::Enter => Some(Action::Connection(ConnectionAction::ActivateSelected)),
        _ => None,
    }
}

fn map_connection_edit_key(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Esc => Some(Action::Connection(ConnectionAction::CancelEdit)),
        KeyCode::Enter => Some(Action::Connection(ConnectionAction::FinishEdit)),
        KeyCode::Tab => Some(Action::Connection(ConnectionAction::AdvanceField(1))),
        KeyCode::BackTab => Some(Action::Connection(ConnectionAction::AdvanceField(-1))),
        KeyCode::Left => Some(Action::Connection(ConnectionAction::MoveCursorLeft)),
        KeyCode::Right => Some(Action::Connection(ConnectionAction::MoveCursorRight)),
        KeyCode::Home => Some(Action::Connection(ConnectionAction::MoveCursorHome)),
        KeyCode::End => Some(Action::Connection(ConnectionAction::MoveCursorEnd)),
        KeyCode::Backspace => Some(Action::Connection(ConnectionAction::Backspace)),
        KeyCode::Delete => Some(Action::Connection(ConnectionAction::Delete)),
        KeyCode::Char(ch)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            Some(Action::Connection(ConnectionAction::Insert(ch)))
        }
        _ => None,
    }
}

fn is_force_quit_key(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c' | 'C'))
}
