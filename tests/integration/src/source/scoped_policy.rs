use ps_core::{
    models::{ContributionType, Platform},
    repo::org::UpdateIdentityParams,
};
use ps_workers::infra::registry::create_source;

use super::scoped_storage::{context, item};
use crate::common::wiremock_helpers::SourceTestContext;

#[tokio::test]
async fn scoped_empty_and_nonempty_runs_never_create_global_checkpoint_but_all_does() {
    let ctx = SourceTestContext::new().await;
    let mut scoped = context(&ctx, Platform::Github).await;
    let source = create_source(&Platform::Github).unwrap();
    assert_eq!(source.store_batch(&scoped, &[]).await.unwrap(), 0);
    source
        .advance_watermark(&scoped, "empty-person", 0)
        .await
        .unwrap();
    let contribution = item(
        Platform::Github,
        ContributionType::PrReview,
        "selected-review",
        "selected",
        "2026-06-01T00:00:00Z",
    );
    source
        .store_batch(&scoped, &[contribution.clone()])
        .await
        .unwrap();
    source
        .advance_watermark(&scoped, "person-checkpoint", 1)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar!("SELECT count(*) FROM activity.ingestion_watermarks")
            .fetch_one(&ctx.pool)
            .await
            .unwrap(),
        Some(0)
    );
    scoped.request = None;
    scoped.run_id = None;
    source.store_batch(&scoped, &[contribution]).await.unwrap();
    source
        .advance_watermark(&scoped, "global-checkpoint", 1)
        .await
        .unwrap();
    let row = sqlx::query!("SELECT watermark_value, items_collected_last_run, last_successful_run FROM activity.ingestion_watermarks").fetch_one(&ctx.pool).await.unwrap();
    assert_eq!(row.watermark_value, "global-checkpoint");
    assert_eq!(row.items_collected_last_run, Some(1));
    assert!(row.last_successful_run.is_some());
    ctx.teardown().await;
}

#[tokio::test]
async fn changed_account_and_changed_admitted_interval_are_rejected() {
    for change_identity in [false, true] {
        let ctx = SourceTestContext::new().await;
        let mut scoped = context(&ctx, Platform::Jira).await;
        if change_identity {
            let identity = scoped
                .request
                .as_ref()
                .unwrap()
                .source
                .identity
                .as_ref()
                .unwrap();
            ctx.repos
                .org
                .update_person_identity(UpdateIdentityParams {
                    person_id: identity.person_id,
                    identity_id: identity.identity_id,
                    username: None,
                    platform_user_id: Some(Some("Cloud:Changed".into())),
                })
                .await
                .unwrap();
        } else {
            scoped.request.as_mut().unwrap().since_date = Some("2026-02-01".into());
        }
        let contribution = item(
            Platform::Jira,
            ContributionType::JiraTicket,
            "PROJ-1",
            "Cloud:Selected",
            "2026-06-01T00:00:00Z",
        );
        assert!(
            create_source(&Platform::Jira)
                .unwrap()
                .store_batch(&scoped, &[contribution])
                .await
                .is_err()
        );
        assert_eq!(
            sqlx::query_scalar!("SELECT count(*) FROM activity.contributions")
                .fetch_one(&ctx.pool)
                .await
                .unwrap(),
            Some(0)
        );
        assert_eq!(
            sqlx::query_scalar!("SELECT count(*) FROM reasoning.embedding_queue")
                .fetch_one(&ctx.pool)
                .await
                .unwrap(),
            Some(0)
        );
        ctx.teardown().await;
    }
}

#[tokio::test]
async fn changed_diff_context_enqueues_new_content_without_replacing_contribution_id() {
    let ctx = SourceTestContext::new().await;
    let scoped = context(&ctx, Platform::Github).await;
    let source = create_source(&Platform::Github).unwrap();
    let mut contribution = item(
        Platform::Github,
        ContributionType::PullRequest,
        "selected-pr",
        "selected",
        "2026-06-01T00:00:00Z",
    );
    source
        .store_batch(&scoped, &[contribution.clone()])
        .await
        .unwrap();
    let id = sqlx::query_scalar!("SELECT id FROM activity.contributions")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    contribution.enrichment_content =
        Some(serde_json::json!({"body":"source text","diff":"authoritative diff"}));
    source
        .store_batch(&scoped, &[contribution.clone()])
        .await
        .unwrap();
    source.store_batch(&scoped, &[contribution]).await.unwrap();
    let queued = sqlx::query!("SELECT contribution_id, content FROM reasoning.enrichment_queue")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(queued.contribution_id, id);
    assert_eq!(queued.content["diff"], "authoritative diff");
    assert_eq!(
        sqlx::query_scalar!("SELECT count(*) FROM activity.contribution_changes")
            .fetch_one(&ctx.pool)
            .await
            .unwrap(),
        Some(2)
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn same_jira_key_from_another_instance_cannot_be_overwritten() {
    let ctx = SourceTestContext::new().await;
    let scoped = context(&ctx, Platform::Jira).await;
    let mut contribution = item(
        Platform::Jira,
        ContributionType::JiraTicket,
        "PROJ-1",
        "Cloud:Selected",
        "2026-06-01T00:00:00Z",
    );
    let person = scoped
        .request
        .as_ref()
        .unwrap()
        .scope
        .person_id()
        .unwrap()
        .into_inner();
    let id = uuid::Uuid::now_v7();
    contribution.url = Some("https://other-instance.atlassian.net/browse/PROJ-1".into());
    ctx.repos
        .activity
        .upsert_contribution(id, Some(person), &contribution)
        .await
        .unwrap();
    contribution.url = Some("https://selected-instance.atlassian.net/browse/PROJ-1".into());
    assert!(
        create_source(&Platform::Jira)
            .unwrap()
            .store_batch(&scoped, &[contribution])
            .await
            .is_err()
    );
    let saved = sqlx::query!("SELECT id, url FROM activity.contributions")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(saved.id, id);
    assert_eq!(
        saved.url.as_deref(),
        Some("https://other-instance.atlassian.net/browse/PROJ-1")
    );
    ctx.teardown().await;
}
