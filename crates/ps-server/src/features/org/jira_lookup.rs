//! Bounded, read-only Jira Cloud account lookup using configured source credentials.

#![allow(clippy::result_large_err)]

use std::time::Duration;

use ps_core::crypto;
use ps_core::models::{Platform, SourceConfig};
use ps_core::repo::Repos;
use ps_proto::canonical::prism::v1::{LookupJiraAccountsRequest, LookupJiraAccountsResponse};
use reqwest::Url;
use tonic::{Response, Status};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::common::db_err;

mod client;

use client::{JiraSearch, build_client, search_accounts};

const DEFAULT_PAGE_SIZE: i32 = 20;
const MAX_PAGE_SIZE: i32 = 50;
const SEARCH_WINDOW: i32 = 1000;
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use ps_core::models::SourceId;
    use serde_json::json;
    use tonic::Code;

    fn source(settings: serde_json::Value) -> SourceConfig {
        SourceConfig {
            id: SourceId::from(Uuid::now_v7()),
            source_type: Platform::Jira,
            name: "Jira".into(),
            enabled: true,
            settings,
            schedule_cron: None,
            created_at: time::OffsetDateTime::now_utc(),
            updated_at: time::OffsetDateTime::now_utc(),
        }
    }

    fn request() -> LookupJiraAccountsRequest {
        LookupJiraAccountsRequest {
            source_id: Uuid::now_v7().to_string(),
            query: "Alice + Bob & 特".into(),
            start_at: 7,
            max_results: 2,
        }
    }

    #[test]
    fn validation_rejects_unbounded_requests_and_caps_last_page() {
        let mut req = request();
        req.query = " ".into();
        assert!(SearchPage::from_request(&req).is_err());

        req.query = "x".repeat(257);
        assert!(SearchPage::from_request(&req).is_err());

        req.query = "Alice".into();
        for size in [-1, 51, i32::MAX] {
            req.max_results = size;
            assert!(SearchPage::from_request(&req).is_err());
        }

        req.max_results = 0;
        assert_eq!(
            SearchPage::from_request(&req).expect("default").max_results,
            20
        );

        for offset in [-1, 1000, i32::MAX] {
            req.start_at = offset;
            assert!(SearchPage::from_request(&req).is_err());
        }

        req.start_at = 999;
        assert_eq!(
            SearchPage::from_request(&req)
                .expect("last page")
                .max_results,
            1
        );
    }

    #[test]
    fn rejects_invalid_source_mode_and_url_before_dispatch() {
        let settings = json!({
            "base_url": "https://jira.example.com/context/"
        });
        let mut config = source(settings);

        assert_eq!(
            source_search_url(&config).expect("Cloud URL").as_str(),
            "https://jira.example.com/context/rest/api/3/user/search"
        );

        config.source_type = Platform::Github;
        assert_eq!(
            source_search_url(&config).expect_err("non-Jira").code(),
            Code::InvalidArgument
        );

        config.source_type = Platform::Jira;
        config.enabled = false;
        assert_eq!(
            source_search_url(&config).expect_err("disabled").code(),
            Code::FailedPrecondition
        );

        config.enabled = true;
        for mode in [
            json!("server"),
            json!("data-center"),
            json!(null),
            json!(true),
        ] {
            config.settings["api_mode"] = mode;
            assert_eq!(
                source_search_url(&config)
                    .expect_err("unsupported mode")
                    .code(),
                Code::FailedPrecondition
            );
        }

        config.settings["api_mode"] = json!("cloud");
        for base in [
            "",
            "invalid",
            "file:///etc",
            "https://user:token@jira.example",
            "https://jira.example?token=secret",
            "https://jira.example/#fragment",
        ] {
            config.settings["base_url"] = json!(base);
            assert_eq!(
                source_search_url(&config)
                    .expect_err("invalid base URL")
                    .code(),
                Code::FailedPrecondition
            );
        }
    }
}
