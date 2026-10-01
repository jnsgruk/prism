use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ps_core::{
    ingestion::{IdentitySnapshot, IngestionContext, Source},
    models::Platform,
    repo::org::{CreatePersonParams, IdentityInput, UpdateIdentityParams},
};
use ps_workers::{
    features::ingestion::lib::discovery::identity_version, infra::registry::create_source,
};
use serde_json::Value;
use time::{Duration, OffsetDateTime};
use wiremock::{
    Mock, Request, ResponseTemplate,
    matchers::{method, path},
};

use crate::common::wiremock_helpers::{
    SourceTestContext, graphql_pr_node, graphql_review_node, graphql_search_response,
};

pub(super) async fn person(
    ctx: &SourceTestContext,
    platform: Platform,
    username: &str,
    user_id: Option<&str>,
) -> IdentitySnapshot {
    let saved = ctx
        .repos
        .org
        .create_person(CreatePersonParams {
            name: username.into(),
            email: None,
            level: None,
            team_id: None,
            identities: vec![IdentityInput {
                platform: platform.clone(),
                username: username.into(),
                platform_user_id: user_id.map(str::to_owned),
            }],
        })
        .await
        .unwrap();
    let identity = &saved.identities[0];
    IdentitySnapshot {
        identity_id: identity.id,
        person_id: saved.person.id.into(),
        platform,
        username: identity.platform_username.clone().into(),
        platform_user_id: identity.platform_user_id.clone(),
    }
}

pub(super) async fn drive(
    ctx: &IngestionContext,
    source: &dyn Source,
    mut cursor: String,
) -> String {
    for _ in 0..100 {
        let fetched = source.fetch_batch(ctx, &cursor).await.unwrap();
        source.store_batch(ctx, &fetched.items).await.unwrap();
        let final_cursor = fetched
            .etag
            .as_deref()
            .or(fetched.next_cursor.as_deref())
            .unwrap_or(&cursor);
        source.checkpoint_batch(ctx, final_cursor).await.unwrap();
        match fetched.next_cursor {
            Some(next) => cursor = next,
            None => return final_cursor.to_owned(),
        }
    }
    panic!("source failed to complete within bounded test batches")
}

pub(super) fn timestamp(value: OffsetDateTime) -> String {
    value
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}

#[tokio::test]
async fn active_unassigned_discovery_is_instance_bound_and_identity_edits_reset_only_affected_coverage()
 {
    let ctx = SourceTestContext::new().await;
    let platform = Platform::Discourse("ubuntu".into());
    let ingestion = ctx
        .build_ingestion_ctx(
            "forum",
            platform.clone(),
            serde_json::json!({"base_url":ctx.mock_server.uri()}),
            None,
            None,
            None,
        )
        .await;
    let active = person(&ctx, platform.clone(), "active", None).await;
    let second = person(&ctx, platform.clone(), "second", None).await;
    let team = ctx
        .repos
        .org
        .create_team(
            "Assigned",
            "testorg",
            ps_core::models::TeamType::Team,
            None,
            None,
        )
        .await
        .unwrap();
    ctx.repos
        .org
        .assign_person_to_team(second.person_id, team.id.into())
        .await
        .unwrap();
    let inactive = person(&ctx, platform.clone(), "inactive", None).await;
    person(
        &ctx,
        Platform::Discourse("other".into()),
        "wrong-instance",
        None,
    )
    .await;
    ctx.repos
        .org
        .deactivate_person(inactive.person_id.into_inner())
        .await
        .unwrap();
    let targets = ctx
        .repos
        .org
        .active_discovery_identities(&platform)
        .await
        .unwrap();
    assert_eq!(targets.len(), 2);
    assert!(targets.contains(&active));
    assert!(
        targets.contains(&second),
        "membership must not change account eligibility"
    );
    assert_eq!(
        sqlx::query_scalar!(
            "SELECT count(*) FROM org.team_memberships WHERE person_id=$1",
            active.person_id.into_inner()
        )
        .fetch_one(&ctx.pool)
        .await
        .unwrap(),
        Some(0)
    );
    let cutoff = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap();
    for identity in [&active, &second, &inactive] {
        ctx.repos
            .activity
            .advance_identity_discovery(
                ingestion.source_config.id.into_inner(),
                identity,
                &identity_version(&ingestion, identity),
                cutoff,
            )
            .await
            .unwrap();
    }
    assert!(
        ctx.repos
            .activity
            .identity_discovery_cutoff(
                ingestion.source_config.id.into_inner(),
                inactive.identity_id,
                &identity_version(&ingestion, &inactive)
            )
            .await
            .unwrap()
            .is_none()
    );
    let other_source = ctx
        .build_ingestion_ctx(
            "other-forum",
            platform.clone(),
            ingestion.source_config.settings.clone(),
            None,
            None,
            None,
        )
        .await;
    assert!(
        ctx.repos
            .activity
            .identity_discovery_cutoff(
                other_source.source_config.id.into_inner(),
                second.identity_id,
                &identity_version(&other_source, &second)
            )
            .await
            .unwrap()
            .is_none(),
        "coverage belongs to one exact source config"
    );
    ctx.repos
        .org
        .update_person_identity(UpdateIdentityParams {
            person_id: active.person_id,
            identity_id: active.identity_id,
            username: Some("changed".into()),
            platform_user_id: None,
        })
        .await
        .unwrap();
    assert!(
        ctx.repos
            .activity
            .identity_discovery_cutoff(
                ingestion.source_config.id.into_inner(),
                active.identity_id,
                &identity_version(&ingestion, &active)
            )
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        ctx.repos
            .activity
            .identity_discovery_cutoff(
                ingestion.source_config.id.into_inner(),
                second.identity_id,
                &identity_version(&ingestion, &second)
            )
            .await
            .unwrap(),
        Some(cutoff)
    );
    // A late old-run completion cannot reseed an edited identity.
    ctx.repos
        .activity
        .advance_identity_discovery(
            ingestion.source_config.id.into_inner(),
            &active,
            &identity_version(&ingestion, &active),
            cutoff,
        )
        .await
        .unwrap();
    assert!(
        ctx.repos
            .activity
            .identity_discovery_cutoff(
                ingestion.source_config.id.into_inner(),
                active.identity_id,
                &identity_version(&ingestion, &active)
            )
            .await
            .unwrap()
            .is_none()
    );
    ctx.repos.activity.reset_all().await.unwrap();
    assert!(
        ctx.repos
            .activity
            .identity_discovery_cutoff(
                ingestion.source_config.id.into_inner(),
                second.identity_id,
                &identity_version(&ingestion, &second),
            )
            .await
            .unwrap()
            .is_none(),
        "resetting harvested data must reset supplementary discovery coverage",
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn github_ordinary_discovery_tracks_manual_author_and_cross_repo_review_after_backfill_and_deferral()
 {
    let ctx = SourceTestContext::new().await;
    let mut ingestion = ctx.build_ingestion_ctx("github", Platform::Github, serde_json::json!({"base_url":ctx.mock_server.uri(), "orgs":["testorg"], "exclude_archived":false}), Some("fake-token".into()), None, None).await;
    let seeded = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap() - Duration::hours(2);
    ctx.with_person_scope(&mut ingestion, "manual", None, "2020-01-01", seeded)
        .await;
    let identity = ingestion
        .request
        .as_ref()
        .unwrap()
        .source
        .identity
        .as_ref()
        .unwrap()
        .clone();
    let active = Arc::new(AtomicBool::new(false));
    let deferred = Arc::new(AtomicBool::new(false));
    let blocked = Arc::new(AtomicBool::new(false));
    let event = timestamp(seeded + Duration::hours(1));
    let provider_active = active.clone();
    let provider_deferred = deferred.clone();
    let provider_blocked = blocked.clone();
    Mock::given(method("POST")).and(path("/graphql")).respond_with(move |request: &Request| {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let variables = &body["variables"];
        let query = variables["query"].as_str().unwrap_or_default();
        if !provider_active.load(Ordering::SeqCst) || (!query.contains("created:") && variables["number"].is_null()) {
            return ResponseTemplate::new(200).set_body_json(graphql_search_response(&[], false, None));
        }
        if query.contains("reviewed-by:") {
            if provider_blocked.load(Ordering::SeqCst) { return ResponseTemplate::new(403); }
            if !provider_deferred.swap(true, Ordering::SeqCst) {
                return ResponseTemplate::new(429).insert_header("x-ratelimit-limit", "5000").insert_header("x-ratelimit-remaining", "0");
            }
            let mut parent = graphql_pr_node("testorg", "outside", 42, "other", "Old reviewed PR", "OPEN", "2010-01-01T00:00:00Z", &event, 0, 0, &[graphql_review_node("other", "APPROVED", &event, 1)]);
            parent["reviews"]["totalCount"] = 2.into();
            parent["reviews"]["pageInfo"] = serde_json::json!({"hasNextPage":true, "endCursor":"review-page-two"});
            return ResponseTemplate::new(200).set_body_json(graphql_search_response(&[parent], false, None));
        }
        if !variables["number"].is_null() {
            return ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data":{"repository":{"pullRequest":{"reviews":{
                    "totalCount":2,"pageInfo":{"hasNextPage":false,"endCursor":null},
                    "nodes":[graphql_review_node("manual", "APPROVED", &event, 2)]
                }}}}, "extensions":{"rateLimit":{"remaining":4900,"limit":5000,"resetAt":"2099-01-01T00:00:00Z"}}
            }));
        }
        let authored = graphql_pr_node("testorg", "project", 7, "manual", "New manual PR", "OPEN", &event, &event, 0, 0, &[]);
        ResponseTemplate::new(200).set_body_json(graphql_search_response(&[authored], false, None))
    }).mount(&ctx.mock_server).await;
    Mock::given(method("GET"))
        .and(path("/orgs/testorg/repos"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&ctx.mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/testorg/project/pulls/7/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&ctx.mock_server)
        .await;
    let source = create_source(&Platform::Github).unwrap();
    let plan = source.plan(&ingestion).await.unwrap();
    drive(
        &ingestion,
        source.as_ref(),
        source.initial_cursor(&ingestion, &plan),
    )
    .await;
    let version = identity_version(&ingestion, &identity);
    assert_eq!(
        ctx.repos
            .activity
            .identity_discovery_cutoff(
                ingestion.source_config.id.into_inner(),
                identity.identity_id,
                &version
            )
            .await
            .unwrap(),
        Some(seeded)
    );
    ingestion.request = None;
    ingestion.run_id = None;
    active.store(true, Ordering::SeqCst);
    // Global source coverage deliberately excludes all current activity.
    ctx.repos
        .activity
        .upsert_watermark("github", "2099-01-01T00:00:00Z", 0)
        .await
        .unwrap();
    let plan = source.plan(&ingestion).await.unwrap();
    let mut cursor = source.initial_cursor(&ingestion, &plan);
    let mut observed_deferral = false;
    for _ in 0..100 {
        // Recreate the adapter on every batch to verify persisted cursor resume.
        let source = create_source(&Platform::Github).unwrap();
        let fetched = source.fetch_batch(&ingestion, &cursor).await.unwrap();
        source
            .store_batch(&ingestion, &fetched.items)
            .await
            .unwrap();
        let state = fetched
            .etag
            .as_deref()
            .or(fetched.next_cursor.as_deref())
            .unwrap_or(&cursor);
        source.checkpoint_batch(&ingestion, state).await.unwrap();
        if fetched
            .rate_limit
            .as_ref()
            .is_some_and(|limit| limit.remaining == 0)
        {
            observed_deferral = true;
            assert_eq!(
                ctx.repos
                    .activity
                    .identity_discovery_cutoff(
                        ingestion.source_config.id.into_inner(),
                        identity.identity_id,
                        &version
                    )
                    .await
                    .unwrap(),
                Some(seeded)
            );
        }
        match fetched.next_cursor {
            Some(next) => cursor = next,
            None => break,
        }
    }
    assert!(observed_deferral);
    let completed = ctx
        .repos
        .activity
        .identity_discovery_cutoff(
            ingestion.source_config.id.into_inner(),
            identity.identity_id,
            &version,
        )
        .await
        .unwrap()
        .unwrap();
    assert!(completed > seeded);
    let rows = sqlx::query!(
        "SELECT contribution_type, person_id FROM activity.contributions ORDER BY contribution_type"
    )
    .fetch_all(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|row| row.person_id == Some(identity.person_id.into_inner()))
    );
    assert!(rows.iter().any(|row| row.contribution_type == "pr_review"));
    // Incomplete traversal may store data but must retain the prior cutoff.
    blocked.store(true, Ordering::SeqCst);
    let plan = source.plan(&ingestion).await.unwrap();
    let final_cursor = drive(
        &ingestion,
        source.as_ref(),
        source.initial_cursor(&ingestion, &plan),
    )
    .await;
    assert!(
        !ps_workers::features::ingestion::lib::finalise::extract_failed_items(&final_cursor)
            .is_empty()
    );
    assert_eq!(
        ctx.repos
            .activity
            .identity_discovery_cutoff(
                ingestion.source_config.id.into_inner(),
                identity.identity_id,
                &version
            )
            .await
            .unwrap(),
        Some(completed)
    );
    blocked.store(false, Ordering::SeqCst);
    let plan = source.plan(&ingestion).await.unwrap();
    drive(
        &ingestion,
        source.as_ref(),
        source.initial_cursor(&ingestion, &plan),
    )
    .await;
    assert_eq!(
        sqlx::query_scalar!("SELECT count(*) FROM activity.contributions")
            .fetch_one(&ctx.pool)
            .await
            .unwrap(),
        Some(2)
    );
    ctx.teardown().await;
}
