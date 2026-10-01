use ps_core::{
    models::{ContributionType, Platform},
    repo::org::UpdateIdentityParams,
};
use ps_workers::infra::registry::create_source;
use uuid::Uuid;

use super::ongoing_tracking::person;
use crate::common::wiremock_helpers::SourceTestContext;

#[tokio::test]
async fn supplementary_store_holds_identity_lock_until_commit_and_rejects_reused_stale_login() {
    let ctx = SourceTestContext::new().await;
    let ingestion = ctx
        .build_ingestion_ctx(
            "github",
            Platform::Github,
            serde_json::json!({"orgs":["testorg"]}),
            None,
            None,
            None,
        )
        .await;
    let identity = person(&ctx, Platform::Github, "manual", None).await;
    let mut item = super::scoped_storage::item(
        Platform::Github,
        ContributionType::PullRequest,
        "testorg/project/pull/1",
        "manual",
        "2026-10-01T00:00:00Z",
    );
    item.metadata = serde_json::json!({
        "supplementary_discovery_identity":identity,
        "supplementary_discovery_source_id":ingestion.source_config.id,
    });
    let mut blocker = ctx.pool.begin().await.unwrap();
    let lock_key = format!("{}:{}", item.platform, item.platform_id);
    sqlx::query!(
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
        lock_key
    )
    .execute(&mut *blocker)
    .await
    .unwrap();
    let activity = ctx.repos.activity.clone();
    let selected = identity.person_id.into_inner();
    let stored_item = item.clone();
    let store = tokio::spawn(async move {
        activity
            .bulk_upsert_contributions(&[Uuid::now_v7()], &[Some(selected)], &[&stored_item])
            .await
    });
    // The store has validated and locked the identity before waiting on its key.
    for _ in 0..100 {
        let waiting = sqlx::query_scalar!(
            "SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND NOT granted AND database = (SELECT oid FROM pg_database WHERE datname = current_database())"
        ).fetch_one(&ctx.pool).await.unwrap().unwrap_or(0);
        if waiting > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let org = ctx.repos.org.clone();
    let frozen = identity.clone();
    let mut edit = tokio::spawn(async move {
        org.update_person_identity(UpdateIdentityParams {
            person_id: frozen.person_id,
            identity_id: frozen.identity_id,
            username: Some("changed".into()),
            platform_user_id: None,
        })
        .await
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut edit)
            .await
            .is_err(),
        "identity edit must wait for the target's store transaction"
    );
    blocker.rollback().await.unwrap();
    let first = store.await.unwrap().unwrap();
    assert_eq!(first.len(), 1);
    edit.await.unwrap().unwrap();
    let replacement = person(&ctx, Platform::Github, "manual", None).await;
    assert_ne!(replacement.person_id, identity.person_id);
    let source = create_source(&Platform::Github).unwrap();
    assert!(
        source.store_batch(&ingestion, &[item]).await.is_err(),
        "stale fetched data must not resolve the reused username to another person"
    );
    let row = sqlx::query!("SELECT id, person_id FROM activity.contributions WHERE platform_id = 'testorg/project/pull/1'").fetch_one(&ctx.pool).await.unwrap();
    assert_eq!(row.id, first[0].0);
    assert_eq!(row.person_id, Some(identity.person_id.into_inner()));
    ctx.teardown().await;
}
