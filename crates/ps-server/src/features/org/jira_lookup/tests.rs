#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::*;
use ps_core::models::SourceId;
use serde_json::json;
use tonic::Code;
use wiremock::matchers::{basic_auth, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

async fn search(
    mock: &MockServer,
    timeout: Duration,
) -> Result<LookupJiraAccountsResponse, Status> {
    let req = request();
    search_accounts(
        &build_client(timeout)?,
        JiraSearch {
            url: Url::parse(&format!("{}/rest/api/3/user/search", mock.uri())).expect("mock URL"),
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
    Mock::given(method("GET"))
        .and(path("/rest/api/3/user/search"))
        .and(query_param("query", "Alice + Bob & 特"))
        .and(query_param("startAt", "7"))
        .and(query_param("maxResults", "2"))
        .and(basic_auth("admin@example.com", "secret-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"accountId":"Opaque:ABC", "displayName":"Same Name", "emailAddress":"a@example.com", "active":true},
            {"accountId":"Opaque:abc", "displayName":"Same Name", "active":false}
        ])))
        .expect(1).mount(&mock).await;
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
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_string("secret-token admin@example.com provider internals"),
            )
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
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!([]))
                .set_delay(Duration::from_millis(200)),
        )
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
    Mock::given(path("/rest/api/3/user/search"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", format!("{}/unexpected", mock.uri())),
        )
        .expect(1)
        .mount(&mock)
        .await;
    Mock::given(path("/unexpected"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&mock)
        .await;
    assert_eq!(
        search(&mock, REQUEST_TIMEOUT)
            .await
            .expect_err("redirect denied")
            .code(),
        Code::Unavailable
    );
}

#[tokio::test]
async fn malformed_oversized_or_overfull_responses_are_rejected() {
    for body in [
        "not json secret-token".to_owned(),
        "x".repeat(MAX_RESPONSE_BYTES + 1),
        json!([{"accountId":"  ","displayName":"No ID","active":true}]).to_string(),
        json!([{"accountId":"A","displayName":"A","active":true},
               {"accountId":"B","displayName":"B","active":true},
               {"accountId":"C","displayName":"C","active":true}])
        .to_string(),
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
    let mut config = source(json!({"base_url":"https://jira.example.com/context/"}));
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

#[tokio::test]
async fn last_search_window_page_has_no_continuation() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("startAt", "999"))
        .and(query_param("maxResults", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"accountId":"Exact", "displayName":"Final", "active":true}
        ])))
        .expect(1)
        .mount(&mock)
        .await;
    let mut req = request();
    req.start_at = 999;
    let response = search_accounts(
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
    assert_eq!(response.accounts.len(), 1);
    assert!(response.next_start_at.is_none());
}
