//! Jira REST API client.
//!
//! Supports both Jira Cloud (API token + email Basic auth) and Jira Server/Data
//! Center (PAT Bearer auth). The `api_mode` setting in the source config
//! determines which auth scheme is used.

use base64::Engine;
use ps_core::models::RateLimitInfo;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::Deserialize;
use time::OffsetDateTime;
use tracing::debug;

/// Jira REST API client.
pub struct JiraClient {
    http: reqwest::Client,
    base_url: String,
    auth_header: String,
}

/// A page of search results from the Jira JQL search endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub start_at: i64,
    pub max_results: i64,
    pub total: i64,
    pub issues: Vec<JiraIssue>,
}

/// A single Jira issue from the search or issue detail endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct JiraIssue {
    pub key: String,
    #[serde(rename = "self")]
    pub self_url: Option<String>,
    pub fields: JiraFields,
    pub changelog: Option<JiraChangelog>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraFields {
    pub summary: Option<String>,
    pub status: Option<JiraStatus>,
    pub issuetype: Option<JiraIssueType>,
    pub priority: Option<JiraPriority>,
    pub labels: Option<Vec<String>>,
    pub assignee: Option<JiraUser>,
    pub reporter: Option<JiraUser>,
    pub created: Option<String>,
    pub updated: Option<String>,
    #[serde(rename = "resolutiondate")]
    pub resolution_date: Option<String>,
    pub parent: Option<JiraParent>,
    /// Story points — field name varies per instance, accessed via dynamic key.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraStatus {
    pub name: Option<String>,
    pub status_category: Option<JiraStatusCategory>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JiraStatusCategory {
    pub key: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JiraIssueType {
    pub name: Option<String>,
    pub subtask: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JiraPriority {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraUser {
    pub account_id: Option<String>,
    pub display_name: Option<String>,
    pub email_address: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JiraParent {
    pub key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraChangelog {
    pub histories: Vec<JiraChangeHistory>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JiraChangeHistory {
    pub created: Option<String>,
    pub items: Vec<JiraChangeItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraChangeItem {
    pub field: Option<String>,
    pub from_string: Option<String>,
    pub to_string: Option<String>,
}

impl JiraClient {
    /// Create a new Jira client.
    ///
    /// `api_mode` should be `"cloud"` for Jira Cloud (Basic auth with
    /// email:token) or `"server"` for Jira Server/Data Center (Bearer PAT).
    pub fn new(
        http: reqwest::Client,
        base_url: &str,
        api_mode: &str,
        email: Option<&str>,
        token: &str,
    ) -> Self {
        let base_url = base_url.trim_end_matches('/').to_string();

        let auth_header = if api_mode == "server" {
            format!("Bearer {token}")
        } else {
            // Cloud: Basic auth with email:token
            let credentials = base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{token}", email.unwrap_or_default()));
            format!("Basic {credentials}")
        };

        Self {
            http,
            base_url,
            auth_header,
        }
    }

    /// Run a JQL search query with pagination.
    ///
    /// `fields` specifies which fields to include. Use `"*all"` for full
    /// detail or a comma-separated list for lightweight radar queries.
    pub async fn search(
        &self,
        jql: &str,
        start_at: i64,
        max_results: i64,
        fields: &str,
        expand: &str,
    ) -> Result<(SearchResponse, Option<RateLimitInfo>), ps_core::Error> {
        let url = format!("{}/rest/api/3/search", self.base_url);

        let mut body = serde_json::json!({
            "jql": jql,
            "startAt": start_at,
            "maxResults": max_results,
            "fields": fields.split(',').map(str::trim).collect::<Vec<_>>(),
        });

        if !expand.is_empty()
            && let Some(obj) = body.as_object_mut()
        {
            obj.insert(
                "expand".into(),
                serde_json::json!(expand.split(',').map(str::trim).collect::<Vec<_>>()),
            );
        }

        debug!(jql, start_at, max_results, "jira search request");

        let resp = self
            .http
            .post(&url)
            .header(AUTHORIZATION, &self.auth_header)
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ps_core::Error::Internal(format!("jira request failed: {e}")))?;

        let rate_limit = extract_rate_limit(&resp);

        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(60);
            return Err(ps_core::Error::RateLimit {
                retry_after_secs: retry_after,
            });
        }

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ps_core::Error::Internal(format!(
                "jira search returned {status}: {body}"
            )));
        }

        let search_resp: SearchResponse = resp
            .json()
            .await
            .map_err(|e| ps_core::Error::Internal(format!("jira response parse error: {e}")))?;

        debug!(
            total = search_resp.total,
            returned = search_resp.issues.len(),
            "jira search response"
        );

        Ok((search_resp, rate_limit))
    }

    /// Fetch a single issue with changelog expanded.
    pub async fn get_issue_with_changelog(&self, key: &str) -> Result<JiraIssue, ps_core::Error> {
        let url = format!(
            "{}/rest/api/3/issue/{}?expand=changelog",
            self.base_url, key
        );

        let resp = self
            .http
            .get(&url)
            .header(AUTHORIZATION, &self.auth_header)
            .send()
            .await
            .map_err(|e| ps_core::Error::Internal(format!("jira request failed: {e}")))?;

        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(60);
            return Err(ps_core::Error::RateLimit {
                retry_after_secs: retry_after,
            });
        }

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ps_core::Error::Internal(format!(
                "jira issue {key} returned {status}: {body}"
            )));
        }

        resp.json()
            .await
            .map_err(|e| ps_core::Error::Internal(format!("jira issue parse error: {e}")))
    }

    /// Test the connection by fetching server info (Jira Cloud) or myself endpoint.
    pub async fn test_connection(&self) -> Result<String, ps_core::Error> {
        let url = format!("{}/rest/api/3/myself", self.base_url);

        let resp = self
            .http
            .get(&url)
            .header(AUTHORIZATION, &self.auth_header)
            .send()
            .await
            .map_err(|e| ps_core::Error::Internal(format!("jira connection test failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ps_core::Error::Internal(format!(
                "jira connection test returned {status}: {body}"
            )));
        }

        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let display_name = body
            .get("displayName")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        Ok(format!("Authenticated as {display_name}"))
    }
}

/// Extract rate limit info from Jira response headers.
fn extract_rate_limit(resp: &reqwest::Response) -> Option<RateLimitInfo> {
    let remaining = resp
        .headers()
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i32>().ok())?;
    let limit = resp
        .headers()
        .get("x-ratelimit-limit")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or(0);

    // Jira doesn't always provide a reset header; approximate as now + 60s.
    let reset_at = OffsetDateTime::now_utc() + time::Duration::seconds(60);

    Some(RateLimitInfo {
        remaining,
        limit,
        reset_at,
    })
}
