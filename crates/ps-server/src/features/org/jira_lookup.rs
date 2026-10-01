//! Bounded, read-only Jira Cloud account lookup using configured source credentials.

#![allow(clippy::result_large_err)]

use std::time::Duration;

use ps_core::crypto;
use ps_core::models::{Platform, SourceConfig};
use ps_core::repo::Repos;
use ps_proto::canonical::prism::v1::{
    JiraAccountCandidate, LookupJiraAccountsRequest, LookupJiraAccountsResponse,
};
use reqwest::{Client, Url};
use serde::Deserialize;
use tonic::{Response, Status};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::common::db_err;

const DEFAULT_PAGE_SIZE: i32 = 20;
const MAX_PAGE_SIZE: i32 = 50;
const SEARCH_WINDOW: i32 = 1000;
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) async fn handle_lookup_jira_accounts(
    repos: &Repos,
    secret_key: &[u8; 32],
    request: LookupJiraAccountsRequest,
) -> Result<Response<LookupJiraAccountsResponse>, Status> {
    let page = SearchPage::from_request(&request)?;
    let source_id: Uuid = request
        .source_id
        .parse()
        .map_err(|_| Status::invalid_argument("invalid source_id"))?;
    let source = repos
        .config
        .get_source(source_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| Status::not_found("source not found"))?;
    let url = source_search_url(&source)?;
    let (email, token) = tokio::try_join!(
        decrypt_credential(repos, secret_key, source_id, "email"),
        decrypt_credential(repos, secret_key, source_id, "api_token"),
    )?;
    let client = build_client(REQUEST_TIMEOUT)?;
    let response = search_accounts(
        &client,
        JiraSearch {
            url,
            query: request.query.trim(),
            page,
            email: &email,
            token: &token,
        },
    )
    .await?;
    Ok(Response::new(response))
}

#[derive(Clone, Copy)]
struct SearchPage {
    start_at: i32,
    max_results: i32,
}

impl SearchPage {
    fn from_request(request: &LookupJiraAccountsRequest) -> Result<Self, Status> {
        let query = request.query.trim();
        if query.is_empty() || query.chars().count() > 256 {
            return Err(Status::invalid_argument(
                "search text must contain 1 to 256 characters",
            ));
        }
        let max_results = match request.max_results {
            0 => DEFAULT_PAGE_SIZE,
            size if (1..=MAX_PAGE_SIZE).contains(&size) => size,
            _ => {
                return Err(Status::invalid_argument(
                    "max_results must be between 1 and 50",
                ));
            }
        };
        if !(0..SEARCH_WINDOW).contains(&request.start_at) {
            return Err(Status::invalid_argument(
                "start_at must be between 0 and 999",
            ));
        }
        Ok(Self {
            start_at: request.start_at,
            max_results: max_results.min(SEARCH_WINDOW - request.start_at),
        })
    }
}

fn source_search_url(source: &SourceConfig) -> Result<Url, Status> {
    if source.source_type != Platform::Jira {
        return Err(Status::invalid_argument("source must be a Jira source"));
    }
    if !source.enabled {
        return Err(Status::failed_precondition(
            "enable this Jira source before searching",
        ));
    }
    let mode = source.settings.get("api_mode");
    if mode.is_some_and(|mode| mode.as_str() != Some("cloud")) {
        return Err(Status::failed_precondition(
            "account lookup supports Jira Cloud only; Server/Data Center requires a separate identity adapter",
        ));
    }
    let base_url = source
        .settings
        .get("base_url")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Status::failed_precondition("configure the Jira source base URL"))?;
    let mut url = Url::parse(base_url)
        .map_err(|_| Status::failed_precondition("configure a valid Jira source base URL"))?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Status::failed_precondition(
            "configure a valid Jira source base URL",
        ));
    }
    url.set_path(&format!(
        "{}/rest/api/3/user/search",
        url.path().trim_end_matches('/')
    ));
    Ok(url)
}

async fn decrypt_credential(
    repos: &Repos,
    secret_key: &[u8; 32],
    source_id: Uuid,
    name: &str,
) -> Result<Zeroizing<String>, Status> {
    let encrypted = repos
        .config
        .get_encrypted_secret(source_id, name)
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            Status::failed_precondition(format!("configure the Jira source {name} secret"))
        })?;
    let plaintext = Zeroizing::new(crypto::decrypt(secret_key, &encrypted).map_err(|error| {
        tracing::error!(source_id = %source_id, secret = name, error = %error, "Jira credential decryption failed");
        Status::internal("internal error")
    })?);
    let value = std::str::from_utf8(&plaintext).map_err(|_| {
        tracing::error!(source_id = %source_id, secret = name, "Jira credential is not UTF-8");
        Status::internal("internal error")
    })?;
    if value.trim().is_empty() {
        return Err(Status::failed_precondition(format!(
            "configure the Jira source {name} secret"
        )));
    }
    Ok(Zeroizing::new(value.to_owned()))
}

fn build_client(timeout: Duration) -> Result<Client, Status> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .map_err(|error| {
            tracing::error!(error = %error, "Jira lookup HTTP client setup failed");
            Status::internal("internal error")
        })
}

struct JiraSearch<'a> {
    url: Url,
    query: &'a str,
    page: SearchPage,
    email: &'a str,
    token: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JiraUser {
    account_id: String,
    display_name: String,
    email_address: Option<String>,
    active: bool,
}

async fn search_accounts(
    client: &Client,
    search: JiraSearch<'_>,
) -> Result<LookupJiraAccountsResponse, Status> {
    let mut response = client
        .get(search.url)
        .query(&[
            ("query", search.query.to_owned()),
            ("startAt", search.page.start_at.to_string()),
            ("maxResults", search.page.max_results.to_string()),
        ])
        .basic_auth(search.email, Some(search.token))
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(http_error)?;
    match response.status().as_u16() {
        200 => {}
        401 => {
            return Err(Status::failed_precondition(
                "Jira rejected the configured credentials; update the source email and API token",
            ));
        }
        403 => {
            return Err(Status::permission_denied(
                "Jira account search is forbidden; grant Browse users and groups permission or enter an account ID directly",
            ));
        }
        429 => {
            return Err(Status::resource_exhausted(
                "Jira account search is rate limited; retry later or enter an account ID directly",
            ));
        }
        _ => {
            return Err(Status::unavailable(
                "Jira account search is unavailable; retry later or enter an account ID directly",
            ));
        }
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(http_error)? {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(Status::unavailable(
                "Jira returned an oversized account-search response",
            ));
        }
        body.extend_from_slice(&chunk);
    }
    let users: Vec<JiraUser> = serde_json::from_slice(&body)
        .map_err(|_| Status::unavailable("Jira returned an invalid account-search response"))?;
    if users.len() > usize::try_from(search.page.max_results).unwrap_or_default()
        || users.iter().any(|user| user.account_id.trim().is_empty())
    {
        return Err(Status::unavailable(
            "Jira returned an invalid account-search response",
        ));
    }
    let next_offset = search.page.start_at + search.page.max_results;
    let next_start_at = (users.len()
        == usize::try_from(search.page.max_results).unwrap_or_default()
        && next_offset < SEARCH_WINDOW)
        .then_some(next_offset);
    Ok(LookupJiraAccountsResponse {
        accounts: users
            .into_iter()
            .map(|user| JiraAccountCandidate {
                account_id: user.account_id,
                display_name: user.display_name,
                email: user.email_address,
                active: user.active,
            })
            .collect(),
        next_start_at,
    })
}

fn http_error(error: reqwest::Error) -> Status {
    // Never propagate a request URL, credentials, or an upstream response body.
    let error = error.without_url();
    tracing::warn!(error = %error, "Jira account lookup request failed");
    if error.is_timeout() {
        Status::deadline_exceeded(
            "Jira account search timed out; retry or enter an account ID directly",
        )
    } else {
        Status::unavailable(
            "Jira account search is unavailable; retry or enter an account ID directly",
        )
    }
}

#[cfg(test)]
mod tests;
