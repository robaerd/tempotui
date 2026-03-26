use std::{
    io::{self, Stdout},
    sync::mpsc::TryRecvError,
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{
    config::AppConfig,
    storage::{AppStateStore, PersistedState},
};

mod action;
mod input;
mod render;
mod runtime;
mod state;
mod update;
mod view_model;

use self::{
    action::{Action, Effect},
    runtime::{
        AccountIdDiscoverer, ConnectionRequest, ConnectionResult, ConnectionRuntime, LoaderRequest,
        LoaderRuntime, TempoVerifier, build_loader_runtime, discover_account_id,
        spawn_connection_runtime, verify_tempo_connection,
    },
    state::{AppState, ConnectionState},
};

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub fn run(config: AppConfig, store: AppStateStore, persisted: PersistedState) -> io::Result<()> {
    let mut terminal = setup_terminal()?;
    let _guard = TerminalGuard;
    let mut app = TuiApp::new(config, store, persisted);
    let result = app.run(&mut terminal);
    let _ = terminal.show_cursor();
    result
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;
    Ok(terminal)
}

struct TuiApp {
    store: AppStateStore,
    state: AppState,
    loader: Option<LoaderRuntime>,
    connection_runtime: ConnectionRuntime,
    account_id_discoverer: AccountIdDiscoverer,
    tempo_verifier: TempoVerifier,
}

impl TuiApp {
    fn new(config: AppConfig, store: AppStateStore, persisted: PersistedState) -> Self {
        Self::new_with_hooks(
            config,
            store,
            persisted,
            discover_account_id,
            verify_tempo_connection,
        )
    }

    fn new_with_hooks(
        config: AppConfig,
        store: AppStateStore,
        persisted: PersistedState,
        account_id_discoverer: AccountIdDiscoverer,
        tempo_verifier: TempoVerifier,
    ) -> Self {
        let initial_loader = if persisted.tempo.is_configured() && persisted.jira.is_configured() {
            build_loader_runtime(&persisted.tempo).ok()
        } else {
            None
        };

        let mut app = Self {
            store,
            state: AppState::new(config.clone(), persisted, initial_loader.is_some()),
            loader: initial_loader,
            connection_runtime: spawn_connection_runtime(
                account_id_discoverer,
                tempo_verifier,
                config.today,
            ),
            account_id_discoverer,
            tempo_verifier,
        };
        app.dispatch(Action::Boot);
        app
    }

    fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
        while !self.state.should_quit {
            self.process_connection_messages();
            self.process_loader_messages();
            self.state.sync_selection();
            terminal.draw(|frame| render::draw(frame, &self.state, self.store.path()))?;

            if event::poll(EVENT_POLL_INTERVAL)?
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
                && let Some(action) = input::map_key(&self.state, key)
            {
                self.dispatch(action);
            }
        }

        Ok(())
    }

    fn dispatch(&mut self, action: Action) {
        let effects = update::reduce(&mut self.state, action);
        self.execute_effects(effects);
    }

    fn execute_effects(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::VerifySavedConnection { request_id, tempo } => {
                    if self
                        .connection_runtime
                        .tx
                        .send(ConnectionRequest::VerifySaved { request_id, tempo })
                        .is_err()
                    {
                        self.restart_connection_runtime();
                        self.dispatch(Action::ConnectionRuntimeDisconnected);
                    }
                }
                Effect::ConnectCredentials {
                    request_id,
                    tempo_api_token,
                    tempo_base_url,
                    jira,
                } => {
                    if self
                        .connection_runtime
                        .tx
                        .send(ConnectionRequest::Connect {
                            request_id,
                            tempo_api_token,
                            tempo_base_url,
                            jira,
                        })
                        .is_err()
                    {
                        self.restart_connection_runtime();
                        self.dispatch(Action::ConnectionRuntimeDisconnected);
                    }
                }
                Effect::LoadMonth { request_id, month } => {
                    let Some(loader) = &self.loader else {
                        self.dispatch(Action::LoaderDisconnected {
                            loader_available: false,
                        });
                        continue;
                    };

                    if loader
                        .tx
                        .send(LoaderRequest::Load { request_id, month })
                        .is_err()
                    {
                        self.loader = if self.state.persisted.tempo.is_configured() {
                            build_loader_runtime(&self.state.persisted.tempo).ok()
                        } else {
                            None
                        };
                        self.dispatch(Action::LoaderDisconnected {
                            loader_available: self.loader.is_some(),
                        });
                    }
                }
                Effect::SavePersisted {
                    success_message,
                    failure_prefix,
                } => match self.store.save(&self.state.persisted) {
                    Ok(()) => self.dispatch(Action::PersistedSaveSucceeded {
                        message: success_message,
                    }),
                    Err(err) => self.dispatch(Action::PersistedSaveFailed {
                        message: format!("{failure_prefix}: {err}"),
                    }),
                },
            }
        }
    }

    fn process_connection_messages(&mut self) {
        loop {
            match self.connection_runtime.rx.try_recv() {
                Ok(response) => {
                    if self.state.connection.request_id() != Some(response.request_id) {
                        continue;
                    }

                    match response.result {
                        Ok(ConnectionResult::VerifiedSaved) => {
                            self.dispatch(Action::SavedConnectionVerified);
                        }
                        Ok(ConnectionResult::Connected { tempo, jira }) => {
                            match build_loader_runtime(&tempo) {
                                Ok(loader) => {
                                    self.loader = Some(loader);
                                    self.dispatch(Action::ConnectionEstablished { tempo, jira });
                                }
                                Err(err) => {
                                    self.dispatch(Action::ConnectionEstablishFailed {
                                        message: err,
                                    });
                                }
                            }
                        }
                        Err(err) => match self.state.connection {
                            ConnectionState::VerifyingSaved { .. } => {
                                self.loader = None;
                                self.dispatch(Action::SavedConnectionRejected { message: err });
                            }
                            ConnectionState::Connecting { .. } => {
                                self.dispatch(Action::ConnectionEstablishFailed { message: err });
                            }
                            ConnectionState::NeedsSetup
                            | ConnectionState::SavedUnverified
                            | ConnectionState::Verified
                            | ConnectionState::Invalid { .. } => {}
                        },
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.restart_connection_runtime();
                    self.dispatch(Action::ConnectionRuntimeDisconnected);
                    break;
                }
            }
        }
    }

    fn process_loader_messages(&mut self) {
        while let Some(loader) = self.loader.as_ref() {
            match loader.rx.try_recv() {
                Ok(response) => match response.result {
                    Ok(worklogs) => self.dispatch(Action::MonthLoaded {
                        request_id: response.request_id,
                        month: response.month,
                        worklogs,
                    }),
                    Err(message) => self.dispatch(Action::MonthLoadFailed {
                        request_id: response.request_id,
                        month: response.month,
                        message,
                    }),
                },
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.loader = if self.state.persisted.tempo.is_configured() {
                        build_loader_runtime(&self.state.persisted.tempo).ok()
                    } else {
                        None
                    };
                    self.dispatch(Action::LoaderDisconnected {
                        loader_available: self.loader.is_some(),
                    });
                    break;
                }
            }
        }
    }

    fn restart_connection_runtime(&mut self) {
        self.connection_runtime = spawn_connection_runtime(
            self.account_id_discoverer,
            self.tempo_verifier,
            self.state.today,
        );
    }
}

#[cfg(test)]
mod tests;
