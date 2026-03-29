use std::{
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use chrono::NaiveDate;

use crate::{
    config::MonthWindow,
    jira::JiraClient,
    storage::{JiraSettings, TempoSettings},
    tempo::{TempoClient, TempoWorklog},
};

pub type AccountIdDiscoverer = fn(&JiraSettings) -> Result<String, String>;
pub type TempoVerifier = fn(&TempoSettings, NaiveDate) -> Result<(), String>;

#[derive(Debug)]
pub enum LoaderRequest {
    Load { request_id: u64, month: MonthWindow },
}

#[derive(Debug)]
pub struct LoaderResponse {
    pub request_id: u64,
    pub month: MonthWindow,
    pub result: Result<Vec<TempoWorklog>, String>,
}

pub struct LoaderRuntime {
    pub tx: Sender<LoaderRequest>,
    pub rx: Receiver<LoaderResponse>,
}

#[derive(Debug)]
pub enum ConnectionRequest {
    VerifySaved {
        request_id: u64,
        tempo: TempoSettings,
    },
    Connect {
        request_id: u64,
        tempo_api_token: String,
        tempo_base_url: String,
        jira: JiraSettings,
    },
}

#[derive(Debug)]
pub enum ConnectionResult {
    VerifiedSaved,
    Connected {
        tempo: TempoSettings,
        jira: JiraSettings,
    },
}

#[derive(Debug)]
pub struct ConnectionResponse {
    pub request_id: u64,
    pub result: Result<ConnectionResult, String>,
}

pub struct ConnectionRuntime {
    pub tx: Sender<ConnectionRequest>,
    pub rx: Receiver<ConnectionResponse>,
}

pub fn discover_account_id(settings: &JiraSettings) -> Result<String, String> {
    let client = JiraClient::new(settings).map_err(|err| err.to_string())?;
    client
        .discover_current_account_id()
        .map_err(|err| err.to_string())
}

pub fn verify_tempo_connection(
    settings: &TempoSettings,
    probe_date: NaiveDate,
) -> Result<(), String> {
    let client = TempoClient::new(settings.base_url.clone(), settings.api_token.clone())
        .map_err(|err| err.to_string())?;
    client
        .fetch_worklogs_for_user(&settings.account_id, probe_date, probe_date)
        .map(|_| ())
        .map_err(|err| err.to_string())
}

pub fn spawn_connection_runtime(
    account_id_discoverer: AccountIdDiscoverer,
    tempo_verifier: TempoVerifier,
    probe_date: NaiveDate,
) -> ConnectionRuntime {
    let (request_tx, request_rx) = mpsc::channel::<ConnectionRequest>();
    let (response_tx, response_rx) = mpsc::channel::<ConnectionResponse>();

    thread::spawn(move || {
        while let Ok(request) = request_rx.recv() {
            let response = match request {
                ConnectionRequest::VerifySaved { request_id, tempo } => ConnectionResponse {
                    request_id,
                    result: tempo_verifier(&tempo, probe_date)
                        .map(|_| ConnectionResult::VerifiedSaved),
                },
                ConnectionRequest::Connect {
                    request_id,
                    tempo_api_token,
                    tempo_base_url,
                    jira,
                } => {
                    let result = account_id_discoverer(&jira)
                        .map(|account_id| {
                            TempoSettings::normalized(tempo_api_token, account_id, tempo_base_url)
                        })
                        .and_then(|tempo| {
                            tempo_verifier(&tempo, probe_date)
                                .map(|_| ConnectionResult::Connected { tempo, jira })
                        });
                    ConnectionResponse { request_id, result }
                }
            };

            let _ = response_tx.send(response);
        }
    });

    ConnectionRuntime {
        tx: request_tx,
        rx: response_rx,
    }
}

pub fn build_loader_runtime(settings: &TempoSettings) -> Result<LoaderRuntime, String> {
    let client = TempoClient::new(settings.base_url.clone(), settings.api_token.clone())
        .map_err(|err| err.to_string())?;
    Ok(spawn_loader(client, settings.account_id.clone()))
}

fn spawn_loader(client: TempoClient, account_id: String) -> LoaderRuntime {
    let (request_tx, request_rx) = mpsc::channel::<LoaderRequest>();
    let (response_tx, response_rx) = mpsc::channel::<LoaderResponse>();

    thread::spawn(move || {
        while let Ok(request) = request_rx.recv() {
            match collapse_pending_load_request(request, &request_rx) {
                LoaderRequest::Load { request_id, month } => {
                    let result = client
                        .fetch_worklogs_for_user(&account_id, month.start, month.end)
                        .map_err(|err| err.to_string());
                    let _ = response_tx.send(LoaderResponse {
                        request_id,
                        month,
                        result,
                    });
                }
            }
        }
    });

    LoaderRuntime {
        tx: request_tx,
        rx: response_rx,
    }
}

fn collapse_pending_load_request(
    mut request: LoaderRequest,
    request_rx: &Receiver<LoaderRequest>,
) -> LoaderRequest {
    for next_request in request_rx.try_iter() {
        request = next_request;
    }

    request
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse_pending_load_request_keeps_latest_month() {
        let (request_tx, request_rx) = mpsc::channel();
        let first = LoaderRequest::Load {
            request_id: 1,
            month: MonthWindow::from_label("2026-03").unwrap(),
        };
        request_tx
            .send(LoaderRequest::Load {
                request_id: 2,
                month: MonthWindow::from_label("2026-04").unwrap(),
            })
            .unwrap();
        request_tx
            .send(LoaderRequest::Load {
                request_id: 3,
                month: MonthWindow::from_label("2026-05").unwrap(),
            })
            .unwrap();

        let LoaderRequest::Load { request_id, month } =
            collapse_pending_load_request(first, &request_rx);

        assert_eq!(request_id, 3);
        assert_eq!(month.label, "2026-05");
    }
}
