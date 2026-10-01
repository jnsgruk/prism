use crate::common::server::ApiTestContext;
use ps_core::{crypto, models::Platform, repo::Repos};
use ps_proto::canonical::prism::v1::LookupJiraAccountsRequest;
use ps_proto::canonical::prism::v1::org_service_client::OrgServiceClient;
use serde_json::json;
use tonic::{Code, Request};
use uuid::Uuid;
use wiremock::matchers::{basic_auth, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SECRET_KEY: &[u8; 32] = b"test-secret-key-32-bytes-long!!!";

fn lookup_request(source_id: Uuid, token: &str) -> Request<LookupJiraAccountsRequest> {
    let mut request = Request::new(LookupJiraAccountsRequest {
        source_id: source_id.to_string(),
        query: "Same Name".into(),
        start_at: 0,
        max_results: 20,
    });
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {token}").parse().expect("metadata"),
    );
    request
}

async fn configure_source(repos: &Repos, base_url: &str) -> Uuid {
    let id = Uuid::now_v7();
    repos
        .config
        .create_source(
            id,
            &Platform::Jira.to_string(),
            "Cloud",
            &json!({"base_url":base_url,"api_mode":"cloud"}),
            None,
        )
        .await
        .expect("Jira source");
    for (name, value) in [
        ("email", "configured-admin@example.com"),
        ("api_token", "never-return-this-token"),
    ] {
        let encrypted = crypto::encrypt(SECRET_KEY, value.as_bytes()).expect("encrypt secret");
        repos
            .config
            .upsert_secret(Uuid::now_v7(), id, name, &encrypted)
            .await
            .expect("secret");
    }
    id
}

#[tokio::test]
async fn jira_lookup_requires_auth_and_uses_configured_cloud_credentials() {
    let ctx = ApiTestContext::new().await;
    let mock = MockServer::start().await;
    let repos = Repos::new(ctx.server.pool.clone());
    let source = configure_source(&repos, &mock.uri()).await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/user/search"))
        .and(query_param("query", "Same Name"))
        .and(query_param("startAt", "0"))
        .and(query_param("maxResults", "20"))
        .and(basic_auth("configured-admin@example.com", "never-return-this-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"accountId":"ID:OpaqueUpper", "displayName":"Same Name", "active":true},
            {"accountId":"ID:Other", "displayName":"Same Name", "emailAddress":"candidate@example.com", "active":false}
        ])))
        .expect(1).mount(&mock).await;
    let mut client = OrgServiceClient::new(ctx.server.channel.clone());
    let unauthenticated = client
        .lookup_jira_accounts(LookupJiraAccountsRequest {
            source_id: source.to_string(),
            query: "Same Name".into(),
            ..Default::default()
        })
        .await
        .expect_err("anonymous denied");
    assert_eq!(unauthenticated.code(), Code::Unauthenticated);
    let (_, token) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let response = client
        .lookup_jira_accounts(lookup_request(source, &token))
        .await
        .expect("admin lookup")
        .into_inner();
    assert_eq!(response.accounts[0].account_id, "ID:OpaqueUpper");
    assert!(response.accounts[0].email.is_none());
    assert_eq!(response.accounts[1].account_id, "ID:Other");
    assert!(!response.accounts[1].active);
    let serialized = format!("{response:?}");
    assert!(!serialized.contains("never-return-this-token"));
    assert!(!serialized.contains("configured-admin@example.com"));
    ctx.teardown().await;
}

#[tokio::test]
async fn jira_lookup_source_validation_and_credentials_fail_before_dispatch() {
    let ctx = ApiTestContext::new().await;
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&mock)
        .await;
    let repos = Repos::new(ctx.server.pool.clone());
    let (_, token) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let mut client = OrgServiceClient::new(ctx.server.channel.clone());
    assert_eq!(
        client
            .lookup_jira_accounts(lookup_request(Uuid::now_v7(), &token))
            .await
            .expect_err("missing source")
            .code(),
        Code::NotFound
    );
    let source = Uuid::now_v7();
    repos
        .config
        .create_source(
            source,
            &Platform::Jira.to_string(),
            "Cloud",
            &json!({"base_url":mock.uri(),"api_mode":"cloud"}),
            None,
        )
        .await
        .expect("source");
    let error = client
        .lookup_jira_accounts(lookup_request(source, &token))
        .await
        .expect_err("missing secrets");
    assert_eq!(error.code(), Code::FailedPrecondition);
    let blank = crypto::encrypt(SECRET_KEY, b" ").expect("blank secret");
    repos
        .config
        .upsert_secret(Uuid::now_v7(), source, "api_token", &blank)
        .await
        .expect("blank token");
    let email = crypto::encrypt(SECRET_KEY, b"admin@example.com").expect("email");
    repos
        .config
        .upsert_secret(Uuid::now_v7(), source, "email", &email)
        .await
        .expect("email");
    assert_eq!(
        client
            .lookup_jira_accounts(lookup_request(source, &token))
            .await
            .expect_err("blank token")
            .code(),
        Code::FailedPrecondition
    );
    repos
        .config
        .upsert_secret(
            Uuid::now_v7(),
            source,
            "api_token",
            b"invalid ciphertext sensitive contents",
        )
        .await
        .expect("corrupt token");
    let error = client
        .lookup_jira_accounts(lookup_request(source, &token))
        .await
        .expect_err("bad ciphertext");
    assert_eq!(error.code(), Code::Internal);
    assert_eq!(error.message(), "internal error");
    repos
        .config
        .update_source_enabled(source, false)
        .await
        .expect("disabled source");
    assert_eq!(
        client
            .lookup_jira_accounts(lookup_request(source, &token))
            .await
            .expect_err("disabled source")
            .code(),
        Code::FailedPrecondition
    );
    repos
        .config
        .update_source_enabled(source, true)
        .await
        .expect("enabled source");
    repos
        .config
        .update_source_settings(source, &json!({"base_url":mock.uri(),"api_mode":"server"}))
        .await
        .expect("server mode");
    assert_eq!(
        client
            .lookup_jira_accounts(lookup_request(source, &token))
            .await
            .expect_err("unsupported mode")
            .code(),
        Code::FailedPrecondition
    );
    let github = Uuid::now_v7();
    repos
        .config
        .create_source(
            github,
            &Platform::Github.to_string(),
            "GitHub",
            &json!({}),
            None,
        )
        .await
        .expect("Github source");
    assert_eq!(
        client
            .lookup_jira_accounts(lookup_request(github, &token))
            .await
            .expect_err("wrong platform")
            .code(),
        Code::InvalidArgument
    );
    let mut invalid = lookup_request(source, &token);
    invalid.get_mut().source_id = "invalid".into();
    assert_eq!(
        client
            .lookup_jira_accounts(invalid)
            .await
            .expect_err("invalid source id")
            .code(),
        Code::InvalidArgument
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn jira_lookup_api_masks_upstream_error_body() {
    let ctx = ApiTestContext::new().await;
    let mock = MockServer::start().await;
    let repos = Repos::new(ctx.server.pool.clone());
    let source = configure_source(&repos, &mock.uri()).await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(403).set_body_string(
            "never-return-this-token configured-admin@example.com provider internals",
        ))
        .expect(1)
        .mount(&mock)
        .await;
    let (_, token) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let mut client = OrgServiceClient::new(ctx.server.channel.clone());
    let error = client
        .lookup_jira_accounts(lookup_request(source, &token))
        .await
        .expect_err("forbidden");
    assert_eq!(error.code(), Code::PermissionDenied);
    assert!(error.message().contains("enter an account ID directly"));
    assert!(!error.message().contains("never-return-this-token"));
    assert!(!error.message().contains("configured-admin@example.com"));
    assert!(!error.message().contains("provider internals"));
    ctx.teardown().await;
}
