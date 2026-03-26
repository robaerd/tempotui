use chrono::NaiveDate;
use reqwest::{
    StatusCode, Url,
    blocking::{Client, Response},
};
use serde::Deserialize;
use thiserror::Error;

use crate::http::{build_blocking_client, response_details, send_get_with_retry};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TempoWorklog {
    #[serde(rename = "startDate")]
    pub start_date: NaiveDate,
    #[serde(rename = "timeSpentSeconds")]
    pub time_spent_seconds: i64,
}

#[derive(Debug)]
pub struct TempoClient {
    client: Client,
    base_url: Url,
    token: String,
}

#[derive(Debug, Error)]
pub enum TempoError {
    #[error("Invalid Tempo base URL `{value}`.")]
    InvalidBaseUrl { value: String },
    #[error("Failed to build Tempo client: {0}")]
    ClientBuild(#[source] reqwest::Error),
    #[error("Failed to construct Tempo request URL.")]
    UrlBuild,
    #[error(
        "Tempo pagination returned a cross-origin next URL `{url}`. Expected origin `{expected_origin}` but received `{received_origin}`."
    )]
    PaginationOriginMismatch {
        expected_origin: String,
        received_origin: String,
        url: String,
    },
    #[error("Tempo request to `{url}` failed: {source}")]
    Request {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error(
        "Tempo returned {status} for `{url}`. Check the saved Tempo API token, account ID, and base URL in Connection Setup. Tempo commonly returns 401 when the token is valid for a different region.{details}"
    )]
    HttpStatus {
        status: StatusCode,
        url: String,
        details: String,
    },
    #[error("Failed to decode Tempo response from `{url}`: {source}")]
    Decode {
        url: String,
        #[source]
        source: reqwest::Error,
    },
}

#[derive(Debug, Deserialize)]
struct WorklogPage {
    metadata: PageMetadata,
    #[serde(default)]
    results: Vec<TempoWorklog>,
}

#[derive(Debug, Deserialize)]
struct PageMetadata {
    next: Option<String>,
}

impl TempoClient {
    pub fn new(base_url: String, token: String) -> Result<Self, TempoError> {
        let base_url = Url::parse(&base_url).map_err(|_| TempoError::InvalidBaseUrl {
            value: base_url.clone(),
        })?;
        let client = build_blocking_client("tempotui/0.1.0").map_err(TempoError::ClientBuild)?;

        Ok(Self {
            client,
            base_url,
            token,
        })
    }

    pub fn fetch_worklogs_for_user(
        &self,
        account_id: &str,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<TempoWorklog>, TempoError> {
        let mut next_url = Some(self.user_worklogs_url(account_id, from, to)?);
        let mut worklogs = Vec::new();

        while let Some(url) = next_url {
            let response = self.send(url.clone())?;
            let page = self.decode(url.clone(), response)?;
            worklogs.extend(page.results);
            next_url = page
                .metadata
                .next
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .map(|value| self.parse_next_url(value))
                .transpose()?;
        }

        Ok(worklogs)
    }

    fn user_worklogs_url(
        &self,
        account_id: &str,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Url, TempoError> {
        let mut url = self.base_url.clone();
        url.path_segments_mut()
            .map_err(|_| TempoError::UrlBuild)?
            .extend(["4", "worklogs", "user", account_id]);
        url.query_pairs_mut()
            .append_pair("from", &from.format("%Y-%m-%d").to_string())
            .append_pair("to", &to.format("%Y-%m-%d").to_string())
            .append_pair("limit", "100");
        Ok(url)
    }

    fn send(&self, url: Url) -> Result<Response, TempoError> {
        send_get_with_retry(
            &self.client,
            url,
            |request| request.bearer_auth(&self.token),
            http_status_error,
            |url, source| TempoError::Request {
                url: url.to_string(),
                source,
            },
        )
    }

    fn decode(&self, url: Url, response: Response) -> Result<WorklogPage, TempoError> {
        response.json().map_err(|source| TempoError::Decode {
            url: url.to_string(),
            source,
        })
    }

    fn parse_next_url(&self, value: &str) -> Result<Url, TempoError> {
        match Url::parse(value) {
            Ok(url) => {
                self.ensure_same_origin(&url)?;
                Ok(url)
            }
            Err(_) => self.base_url.join(value).map_err(|_| TempoError::UrlBuild),
        }
    }

    fn ensure_same_origin(&self, next_url: &Url) -> Result<(), TempoError> {
        if same_origin(&self.base_url, next_url) {
            Ok(())
        } else {
            Err(TempoError::PaginationOriginMismatch {
                expected_origin: origin_string(&self.base_url),
                received_origin: origin_string(next_url),
                url: next_url.to_string(),
            })
        }
    }
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

fn origin_string(url: &Url) -> String {
    let host = url.host_str().unwrap_or("<unknown-host>");
    match url.port_or_known_default() {
        Some(port) => format!("{}://{}:{}", url.scheme(), host, port),
        None => format!("{}://{}", url.scheme(), host),
    }
}

fn http_status_error(url: Url, response: Response) -> TempoError {
    let (status, details) = response_details(response, 160);

    TempoError::HttpStatus {
        status,
        url: url.to_string(),
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Matcher, Server};
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    #[test]
    fn fetch_worklogs_follows_pagination() {
        let mut server = Server::new();
        let next_url = format!("{}/next-page", server.url());

        let first = server
            .mock("GET", "/4/worklogs/user/test-user")
            .match_header("authorization", "Bearer test-token")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("from".into(), "2026-03-01".into()),
                Matcher::UrlEncoded("to".into(), "2026-03-31".into()),
                Matcher::UrlEncoded("limit".into(), "100".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!(
                r#"{{
                    "metadata": {{
                        "count": 1,
                        "offset": 0,
                        "limit": 100,
                        "next": "{next_url}"
                    }},
                    "results": [
                        {{ "startDate": "2026-03-01", "timeSpentSeconds": 3600 }}
                    ]
                }}"#
            ))
            .create();

        let second = server
            .mock("GET", "/next-page")
            .match_header("authorization", "Bearer test-token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "metadata": {
                        "count": 1,
                        "offset": 100,
                        "limit": 100
                    },
                    "results": [
                        { "startDate": "2026-03-02", "timeSpentSeconds": 7200 }
                    ]
                }"#,
            )
            .create();

        let client = TempoClient::new(server.url(), "test-token".to_string()).unwrap();
        let worklogs = client
            .fetch_worklogs_for_user(
                "test-user",
                NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
            )
            .unwrap();

        first.assert();
        second.assert();
        assert_eq!(worklogs.len(), 2);
        assert_eq!(worklogs[0].time_spent_seconds, 3600);
        assert_eq!(worklogs[1].time_spent_seconds, 7200);
    }

    #[test]
    fn http_errors_include_configuration_hints() {
        let mut server = Server::new();

        let mock = server
            .mock("GET", "/4/worklogs/user/test-user")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("from".into(), "2026-03-01".into()),
                Matcher::UrlEncoded("to".into(), "2026-03-31".into()),
                Matcher::UrlEncoded("limit".into(), "100".into()),
            ]))
            .with_status(401)
            .with_body("Unauthorized")
            .create();

        let client = TempoClient::new(server.url(), "bad-token".to_string()).unwrap();
        let err = client
            .fetch_worklogs_for_user(
                "test-user",
                NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
            )
            .unwrap_err();

        mock.assert();
        let message = err.to_string();
        assert!(message.contains("401 Unauthorized"));
        assert!(message.contains("Connection Setup"));
        assert!(message.contains("Unauthorized"));
    }

    #[test]
    fn rejects_cross_origin_pagination_urls() {
        let mut server = Server::new();

        let first = server
            .mock("GET", "/4/worklogs/user/test-user")
            .match_header("authorization", "Bearer test-token")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("from".into(), "2026-03-01".into()),
                Matcher::UrlEncoded("to".into(), "2026-03-31".into()),
                Matcher::UrlEncoded("limit".into(), "100".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "metadata": {
                        "count": 1,
                        "offset": 0,
                        "limit": 100,
                        "next": "https://example.com/4/worklogs/user/test-user?offset=100"
                    },
                    "results": [
                        { "startDate": "2026-03-01", "timeSpentSeconds": 3600 }
                    ]
                }"#,
            )
            .create();

        let client = TempoClient::new(server.url(), "test-token".to_string()).unwrap();
        let err = client
            .fetch_worklogs_for_user(
                "test-user",
                NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
            )
            .unwrap_err();

        first.assert();
        let message = err.to_string();
        assert!(message.contains("cross-origin next URL"));
        assert!(message.contains("example.com"));
    }

    #[test]
    fn retries_retryable_server_errors_and_eventually_succeeds() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();

        let server = thread::spawn(move || {
            for attempt in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buffer = [0_u8; 4096];
                let _ = stream.read(&mut buffer).unwrap();

                let body = if attempt == 0 {
                    "temporary failure".to_string()
                } else {
                    r#"{"metadata":{"count":1,"offset":0,"limit":100},"results":[{"startDate":"2026-03-01","timeSpentSeconds":3600}]}"#
                        .to_string()
                };
                let status_line = if attempt == 0 {
                    "HTTP/1.1 500 Internal Server Error"
                } else {
                    "HTTP/1.1 200 OK"
                };
                let content_type = if attempt == 0 {
                    "text/plain"
                } else {
                    "application/json"
                };

                let response = format!(
                    "{status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).unwrap();
                stream.flush().unwrap();
            }
        });

        let client =
            TempoClient::new(format!("http://{}", address), "test-token".to_string()).unwrap();
        let worklogs = client
            .fetch_worklogs_for_user(
                "test-user",
                NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
            )
            .unwrap();

        server.join().unwrap();
        assert_eq!(worklogs.len(), 1);
        assert_eq!(worklogs[0].time_spent_seconds, 3600);
    }
}
