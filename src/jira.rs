use reqwest::{
    StatusCode, Url,
    blocking::{Client, Response},
};
use serde::Deserialize;
use thiserror::Error;

use crate::{
    http::{build_blocking_client, response_details, send_get_with_retry},
    storage::JiraSettings,
};

#[derive(Debug)]
pub struct JiraClient {
    client: Client,
    site_url: Url,
    email: String,
    api_token: String,
}

#[derive(Debug, Error)]
pub enum JiraError {
    #[error("Jira site URL `{value}` isn't a valid URL.")]
    InvalidSiteUrl { value: String },
    #[error("Jira site URL `{value}` must use HTTPS.")]
    InsecureSiteUrl { value: String },
    #[error("Enter a Jira email address.")]
    MissingEmail,
    #[error("Enter a Jira API token.")]
    MissingApiToken,
    #[error("Couldn't create the Jira client: {0}")]
    ClientBuild(#[source] reqwest::Error),
    #[error("Couldn't build the Jira request URL.")]
    UrlBuild,
    #[error("Jira request to `{url}` failed: {source}")]
    Request {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error(
        "Jira returned {status} for `{url}`. Check your saved Jira site URL, email, and API token in Connection Setup.{details}"
    )]
    HttpStatus {
        status: StatusCode,
        url: String,
        details: String,
    },
    #[error("Couldn't read the Jira response from `{url}`: {source}")]
    Decode {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("Jira response from `{url}` didn't include an `accountId`.")]
    MissingAccountId { url: String },
}

#[derive(Debug, Deserialize)]
struct JiraMyselfResponse {
    #[serde(rename = "accountId")]
    account_id: Option<String>,
}

impl JiraClient {
    pub fn new(settings: &JiraSettings) -> Result<Self, JiraError> {
        if settings.email.trim().is_empty() {
            return Err(JiraError::MissingEmail);
        }
        if settings.api_token.trim().is_empty() {
            return Err(JiraError::MissingApiToken);
        }

        let site_url = Url::parse(&settings.site_url).map_err(|_| JiraError::InvalidSiteUrl {
            value: settings.site_url.clone(),
        })?;
        if site_url.scheme() != "https" && !is_local_host(&site_url) {
            return Err(JiraError::InsecureSiteUrl {
                value: settings.site_url.clone(),
            });
        }

        let client = build_blocking_client("tempotui/0.1.0").map_err(JiraError::ClientBuild)?;

        Ok(Self {
            client,
            site_url,
            email: settings.email.clone(),
            api_token: settings.api_token.clone(),
        })
    }

    pub fn discover_current_account_id(&self) -> Result<String, JiraError> {
        let url = self.myself_url()?;
        let response = self.send(url.clone())?;
        let payload = self.decode(url.clone(), response)?;
        let Some(account_id) = payload.account_id.map(|value| value.trim().to_string()) else {
            return Err(JiraError::MissingAccountId {
                url: url.to_string(),
            });
        };
        if account_id.is_empty() {
            return Err(JiraError::MissingAccountId {
                url: url.to_string(),
            });
        }

        Ok(account_id)
    }

    fn myself_url(&self) -> Result<Url, JiraError> {
        let mut url = self.site_url.clone();
        url.path_segments_mut()
            .map_err(|_| JiraError::UrlBuild)?
            .clear()
            .extend(["rest", "api", "3", "myself"]);
        Ok(url)
    }

    fn send(&self, url: Url) -> Result<Response, JiraError> {
        send_get_with_retry(
            &self.client,
            url,
            |request| request.basic_auth(&self.email, Some(&self.api_token)),
            http_status_error,
            |url, source| JiraError::Request {
                url: url.to_string(),
                source,
            },
        )
    }

    fn decode(&self, url: Url, response: Response) -> Result<JiraMyselfResponse, JiraError> {
        response.json().map_err(|source| JiraError::Decode {
            url: url.to_string(),
            source,
        })
    }
}

fn is_local_host(url: &Url) -> bool {
    matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
}

fn http_status_error(url: Url, response: Response) -> JiraError {
    let (status, details) = response_details(response, 160);

    JiraError::HttpStatus {
        status,
        url: url.to_string(),
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    fn settings(site_url: String) -> JiraSettings {
        JiraSettings::normalized(
            site_url,
            "me@example.com".to_string(),
            "jira-token".to_string(),
        )
    }

    #[test]
    fn discovers_account_id_from_jira_myself() {
        let mut server = Server::new();
        let mock = server
            .mock("GET", "/rest/api/3/myself")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"accountId":"712020:989eb60d-f947-47f7-871b-b09e59c6b8bb"}"#)
            .create();

        let client = JiraClient::new(&settings(server.url())).unwrap();
        let account_id = client.discover_current_account_id().unwrap();

        mock.assert();
        assert_eq!(account_id, "712020:989eb60d-f947-47f7-871b-b09e59c6b8bb");
    }

    #[test]
    fn rejects_non_https_user_urls() {
        let err = JiraClient::new(&settings("http://example.com".to_string())).unwrap_err();

        assert!(matches!(err, JiraError::InsecureSiteUrl { .. }));
    }

    #[test]
    fn jira_http_errors_include_setup_guidance() {
        let mut server = Server::new();
        let mock = server
            .mock("GET", "/rest/api/3/myself")
            .with_status(401)
            .with_body("Unauthorized")
            .create();

        let client = JiraClient::new(&settings(server.url())).unwrap();
        let err = client.discover_current_account_id().unwrap_err();

        mock.assert();
        let message = err.to_string();
        assert!(message.contains("401 Unauthorized"));
        assert!(message.contains("Connection Setup"));
        assert!(message.contains("Unauthorized"));
    }
}
