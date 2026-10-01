//! Bounded HTTP access to Jira Cloud user search.

use super::{SEARCH_WINDOW, SearchPage};
use ps_proto::canonical::prism::v1::{JiraAccountCandidate, LookupJiraAccountsResponse};
use reqwest::{Client, Url};
use serde::Deserialize;
use std::time::Duration;
use tonic::Status;

const MAX_RESPONSE_BYTES: usize = 256 * 1024;

pub(super) fn build_client(timeout: Duration) -> Result<Client, Status> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .map_err(|error| {
            tracing::error!(error = %error, "Jira lookup HTTP client setup failed");
            Status::internal("internal error")
        })
}

pub(super) struct JiraSearch<'a> {
    pub(super) url: Url,
    pub(super) query: &'a str,
    pub(super) page: SearchPage,
    pub(super) email: &'a str,
    pub(super) token: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JiraUser {
    account_id: String,
    display_name: String,
    email_address: Option<String>,
    active: bool,
}

pub(super) async fn search_accounts(
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
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::super::REQUEST_TIMEOUT;
    use super::*;
    use ps_proto::canonical::prism::v1::LookupJiraAccountsRequest;
    use serde_json::json;
    use tonic::Code;
    use uuid::Uuid;
    use wiremock::matchers::{basic_auth, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn request() -> LookupJiraAccountsRequest {
        LookupJiraAccountsRequest {
            source_id: Uuid::now_v7().to_string(),
            query: "Alice + Bob & 特".into(),
            start_at: 7,
            max_results: 2,
        }
    }

    async fn search(
        mock: &MockServer,
        timeout: Duration,
    ) -> Result<LookupJiraAccountsResponse, Status> {
        let req = request();

        search_accounts(
            &build_client(timeout)?,
            JiraSearch {
                url: Url::parse(&format!("{}/rest/api/3/user/search", mock.uri()))
                    .expect("mock URL"),
                query: &req.query,
                page: SearchPage::from_request(&req)?,
                email: "admin@example.com",
                token: "secret-token",
            },
        )
        .await
    }

    #[tokio::test]
    async fn encoded_search_pagination_auth_and_exact_candidates() {
        let mock = MockServer::start().await;
        let users = json!([
            {
                "accountId": "Opaque:ABC",
                "displayName": "Same Name",
                "emailAddress": "a@example.com",
                "active": true
            },
            {
                "accountId": "Opaque:abc",
                "displayName": "Same Name",
                "active": false
            }
        ]);
        let response = ResponseTemplate::new(200).set_body_json(users);

        Mock::given(method("GET"))
            .and(path("/rest/api/3/user/search"))
            .and(query_param("query", "Alice + Bob & 特"))
            .and(query_param("startAt", "7"))
            .and(query_param("maxResults", "2"))
            .and(basic_auth("admin@example.com", "secret-token"))
            .respond_with(response)
            .expect(1)
            .mount(&mock)
            .await;

        let result = search(&mock, REQUEST_TIMEOUT).await.expect("lookup");

        assert_eq!(result.accounts.len(), 2);
        assert_eq!(result.accounts[0].account_id, "Opaque:ABC");
        assert_eq!(result.accounts[1].account_id, "Opaque:abc");
        assert_eq!(
            result.accounts[0].display_name,
            result.accounts[1].display_name
        );
        assert_eq!(result.accounts[0].email.as_deref(), Some("a@example.com"));
        assert!(result.accounts[1].email.is_none());
        assert!(!result.accounts[1].active);
        assert_eq!(result.next_start_at, Some(9));
    }

    #[tokio::test]
    async fn empty_results_have_no_next_page() {
        let mock = MockServer::start().await;

        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&mock)
            .await;

        let result = search(&mock, REQUEST_TIMEOUT).await.expect("lookup");

        assert!(result.accounts.is_empty());
        assert!(result.next_start_at.is_none());
    }

    #[tokio::test]
    async fn upstream_failures_are_actionable_and_masked() {
        for (status, code) in [
            (401, Code::FailedPrecondition),
            (403, Code::PermissionDenied),
            (429, Code::ResourceExhausted),
            (503, Code::Unavailable),
        ] {
            let mock = MockServer::start().await;
            let response = ResponseTemplate::new(status)
                .set_body_string("secret-token admin@example.com provider internals");

            Mock::given(method("GET"))
                .respond_with(response)
                .mount(&mock)
                .await;

            let error = search(&mock, REQUEST_TIMEOUT)
                .await
                .expect_err("upstream failure");

            assert_eq!(error.code(), code);
            assert!(!error.message().contains("secret-token"));
            assert!(!error.message().contains("admin@example.com"));
            assert!(!error.message().contains("provider internals"));
        }
    }

    #[tokio::test]
    async fn timeout_is_bounded() {
        let mock = MockServer::start().await;
        let response = ResponseTemplate::new(200)
            .set_body_json(json!([]))
            .set_delay(Duration::from_millis(200));

        Mock::given(method("GET"))
            .respond_with(response)
            .mount(&mock)
            .await;

        let error = search(&mock, Duration::from_millis(20))
            .await
            .expect_err("timeout");

        assert_eq!(error.code(), Code::DeadlineExceeded);
    }

    #[tokio::test]
    async fn redirects_are_not_followed() {
        let mock = MockServer::start().await;
        let response = ResponseTemplate::new(302)
            .insert_header("Location", format!("{}/unexpected", mock.uri()));

        Mock::given(path("/rest/api/3/user/search"))
            .respond_with(response)
            .expect(1)
            .mount(&mock)
            .await;
        Mock::given(path("/unexpected"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&mock)
            .await;

        let error = search(&mock, REQUEST_TIMEOUT)
            .await
            .expect_err("redirect denied");

        assert_eq!(error.code(), Code::Unavailable);
    }

    #[tokio::test]
    async fn malformed_oversized_or_overfull_responses_are_rejected() {
        let blank_account_id = json!([
            {
                "accountId": "  ",
                "displayName": "No ID",
                "active": true
            }
        ])
        .to_string();
        let overfull_page = json!([
            {
                "accountId": "A",
                "displayName": "A",
                "active": true
            },
            {
                "accountId": "B",
                "displayName": "B",
                "active": true
            },
            {
                "accountId": "C",
                "displayName": "C",
                "active": true
            }
        ])
        .to_string();

        for body in [
            "not json secret-token".to_owned(),
            "x".repeat(MAX_RESPONSE_BYTES + 1),
            blank_account_id,
            overfull_page,
        ] {
            let mock = MockServer::start().await;

            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200).set_body_string(body))
                .mount(&mock)
                .await;

            let error = search(&mock, REQUEST_TIMEOUT)
                .await
                .expect_err("invalid response");

            assert_eq!(error.code(), Code::Unavailable);
            assert!(!error.message().contains("secret-token"));
        }
    }

    #[tokio::test]
    async fn last_search_window_page_has_no_continuation() {
        let mock = MockServer::start().await;
        let users = json!([
            {
                "accountId": "Exact",
                "displayName": "Final",
                "active": true
            }
        ]);
        let response = ResponseTemplate::new(200).set_body_json(users);
        let mut req = request();
        req.start_at = 999;

        Mock::given(method("GET"))
            .and(query_param("startAt", "999"))
            .and(query_param("maxResults", "1"))
            .respond_with(response)
            .expect(1)
            .mount(&mock)
            .await;

        let result = search_accounts(
            &build_client(REQUEST_TIMEOUT).expect("client"),
            JiraSearch {
                url: Url::parse(&format!("{}/rest/api/3/user/search", mock.uri())).expect("URL"),
                query: &req.query,
                page: SearchPage::from_request(&req).expect("page"),
                email: "admin@example.com",
                token: "secret-token",
            },
        )
        .await
        .expect("final page");

        assert_eq!(result.accounts.len(), 1);
        assert!(result.next_start_at.is_none());
    }
}
