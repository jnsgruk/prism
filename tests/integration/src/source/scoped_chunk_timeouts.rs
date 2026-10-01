//! Slow provider calls must not force scoped pages to re-fetch on suspension.
use std::time::Duration;

use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use super::scoped_chunks::{ACCOUNT, chunk_request, issue, scoped_jira};
use crate::common::restate::RestateTestContext;
use crate::common::wiremock_helpers::{SourceTestContext, jira_search_response};

#[tokio::test]
async fn slow_scoped_pages_finish_once_despite_short_server_inactivity_default() {
    let ctx = SourceTestContext::new().await;
    let ingestion = scoped_jira(&ctx).await;

    // Each response outlasts the runtime's one-second inactivity default.
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(jira_search_response(&[], false, Some("page-two")))
                .set_delay(Duration::from_secs(2)),
        )
        .with_priority(2)
        .expect(1)
        .mount(&ctx.mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .and(query_param("nextPageToken", "page-two"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(jira_search_response(
                    &[issue("PROJ-1", ACCOUNT)],
                    true,
                    None,
                ))
                .set_delay(Duration::from_secs(2)),
        )
        .with_priority(1)
        .expect(1)
        .mount(&ctx.mock_server)
        .await;

    let runtime = RestateTestContext::new(ctx.repos.clone()).await;
    let request = chunk_request(&ingestion, 2).await;
    let invocation = runtime.send_chunk(&request).await;
    let result = tokio::time::timeout(Duration::from_secs(15), runtime.result(&invocation))
        .await
        .expect("slow scoped pages should finish without repeated replay");

    assert!(result.is_complete);
    assert_eq!(result.items_stored, 1);
    ctx.mock_server.verify().await;

    drop(runtime);
    ctx.teardown().await;
}
