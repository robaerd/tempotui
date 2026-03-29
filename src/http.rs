use std::{thread, time::Duration};

use reqwest::{
    StatusCode, Url,
    blocking::{Client, RequestBuilder, Response},
    header::RETRY_AFTER,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_REQUEST_ATTEMPTS: usize = 3;
const BASE_RETRY_DELAY: Duration = Duration::from_millis(250);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(5);

pub(crate) fn build_blocking_client(user_agent: &str) -> Result<Client, reqwest::Error> {
    Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .user_agent(user_agent)
        .build()
}

pub(crate) fn send_get_with_retry<E, A, S, R>(
    client: &Client,
    url: Url,
    authorize: A,
    status_error: S,
    request_error: R,
) -> Result<Response, E>
where
    A: Fn(RequestBuilder) -> RequestBuilder,
    S: Fn(Url, Response) -> E,
    R: Fn(Url, reqwest::Error) -> E,
{
    for attempt in 1..=MAX_REQUEST_ATTEMPTS {
        match authorize(client.get(url.clone())).send() {
            Ok(response) if response.status().is_success() => return Ok(response),
            Ok(response) => {
                let should_retry =
                    attempt < MAX_REQUEST_ATTEMPTS && is_retryable_status(response.status());
                let retry_delay = retry_delay_for_response(&response, attempt);
                if should_retry {
                    thread::sleep(retry_delay);
                    continue;
                }
                return Err(status_error(url, response));
            }
            Err(source) => {
                let should_retry =
                    attempt < MAX_REQUEST_ATTEMPTS && is_retryable_request_error(&source);
                if should_retry {
                    thread::sleep(exponential_backoff(attempt));
                    continue;
                }
                return Err(request_error(url, source));
            }
        }
    }

    unreachable!("request loop must return or error before exhausting attempts");
}

pub(crate) fn response_details(response: Response, max_chars: usize) -> (StatusCode, String) {
    let status = response.status();
    let body = response.text().unwrap_or_default();
    let excerpt = body.trim();
    let details = if excerpt.is_empty() {
        String::new()
    } else {
        format!(
            " Response: {}",
            excerpt.chars().take(max_chars).collect::<String>()
        )
    };

    (status, details)
}

fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn is_retryable_request_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect()
}

fn retry_delay_for_response(response: &Response, attempt: usize) -> Duration {
    response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .map(|delay| delay.min(MAX_RETRY_DELAY))
        .unwrap_or_else(|| exponential_backoff(attempt))
}

fn exponential_backoff(attempt: usize) -> Duration {
    let multiplier = 1u32 << (attempt.saturating_sub(1) as u32);
    BASE_RETRY_DELAY
        .checked_mul(multiplier)
        .unwrap_or(MAX_RETRY_DELAY)
        .min(MAX_RETRY_DELAY)
}
