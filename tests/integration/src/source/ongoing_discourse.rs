use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ps_core::models::Platform;
use ps_workers::{
    features::ingestion::lib::discovery::identity_version, infra::registry::create_source,
};
use time::{Duration, OffsetDateTime};
use wiremock::{
    Mock, Request, ResponseTemplate,
    matchers::{method, path},
};

use super::ongoing_tracking::{drive, person, timestamp};
use crate::common::wiremock_helpers::*;

#[tokio::test]
async fn ordinary_discourse_captures_new_post_and_old_topic_like_with_independent_checkpoint_and_rate_resume()
 {
    let ctx = SourceTestContext::new().await;
    let platform = Platform::Discourse("ubuntu".into());
    let ingestion = ctx.build_ingestion_ctx("forum", platform.clone(), serde_json::json!({
        "base_url":ctx.mock_server.uri(), "categories":[1], "min_posts":2, "fetch_likes":true,
    }), None, None, None).await;
    let identity = person(&ctx, platform.clone(), "manual", None).await;
    let inactive = person(&ctx, platform.clone(), "inactive", None).await;
    ctx.repos
        .org
        .deactivate_person(inactive.person_id.into_inner())
        .await
        .unwrap();
    person(
        &ctx,
        Platform::Discourse("other".into()),
        "wrong-instance",
        None,
    )
    .await;
    let event =
        timestamp(OffsetDateTime::now_utc().replace_nanosecond(0).unwrap() - Duration::hours(1));
    let resumed = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(AtomicBool::new(false));
    let provider_resumed = resumed.clone();
    let provider_failed = failed.clone();
    let provider_event = event.clone();
    Mock::given(path("/categories.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(discourse_categories_response(&[(1, "General", "general")])),
        )
        .mount(&ctx.mock_server)
        .await;
    Mock::given(path("/c/1/l/latest.json"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(discourse_latest_response(&[], false)),
        )
        .mount(&ctx.mock_server)
        .await;
    Mock::given(path("/user_actions.json")).respond_with(move |request: &Request| {
        assert!(request.url.query_pairs().any(|(key,value)| key == "username" && value == "manual"));
        if provider_failed.load(Ordering::SeqCst) { return ResponseTemplate::new(403); }
        if !provider_resumed.swap(true, Ordering::SeqCst) { return ResponseTemplate::new(429).insert_header("retry-after", "1"); }
        ResponseTemplate::new(200).set_body_json(serde_json::json!({"user_actions":[
            {"action_type":5,"post_id":101,"topic_id":1,"acting_username":"manual","created_at":provider_event},
            {"action_type":1,"post_id":102,"topic_id":1,"acting_username":"manual","created_at":provider_event},
            {"action_type":1,"post_id":103,"topic_id":1,"acting_username":"manual","created_at":provider_event}
        ]}))
    }).mount(&ctx.mock_server).await;
    for (id, author) in [(101, "manual"), (102, "other"), (103, "other")] {
        Mock::given(path(format!("/posts/{id}.json")))
            .respond_with(ResponseTemplate::new(200).set_body_json(discourse_post(
                id,
                1,
                author,
                2,
                "2020-01-01T00:00:00Z",
                "Old post",
            )))
            .mount(&ctx.mock_server)
            .await;
    }
    let mut topic = discourse_topic_detail(1, "Old topic", "old-topic", &[]);
    topic["category_id"] = 1.into();
    topic["posts_count"] = 5.into();
    Mock::given(path("/t/1.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(topic))
        .mount(&ctx.mock_server)
        .await;
    ctx.repos
        .activity
        .upsert_watermark("forum", "2099-01-01T00:00:00Z", 0)
        .await
        .unwrap();
    let source = create_source(&platform).unwrap();
    // Existing estimates remain immutable until an audited explicit backfill.
    // New like rows use the action's actual time immediately.
    let mut estimate = super::scoped_storage::item(
        platform.clone(),
        ps_core::models::ContributionType::DiscourseLike,
        "like-103-manual",
        "manual",
        "2020-01-01T00:00:00Z",
    );
    assert_eq!(
        source
            .store_batch(&ingestion, &[estimate.clone(), estimate.clone()])
            .await
            .unwrap(),
        1,
        "duplicate natural keys must be stored once per batch"
    );
    let plan = source.plan(&ingestion).await.unwrap();
    let mut cursor = source.initial_cursor(&ingestion, &plan);
    let version = identity_version(&ingestion, &identity);
    let mut deferred = false;
    loop {
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
            deferred = true;
            assert!(
                ctx.repos
                    .activity
                    .identity_discovery_cutoff(
                        ingestion.source_config.id.into_inner(),
                        identity.identity_id,
                        &version
                    )
                    .await
                    .unwrap()
                    .is_none()
            );
        }
        match fetched.next_cursor {
            Some(next) => cursor = next,
            None => break,
        }
    }
    assert!(deferred);
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
    let rows = sqlx::query!("SELECT platform_id, created_at, metadata, person_id FROM activity.contributions ORDER BY platform_id").fetch_all(&ctx.pool).await.unwrap();
    assert_eq!(rows.len(), 3);
    let like = rows
        .iter()
        .find(|row| row.platform_id == "like-102-manual")
        .unwrap();
    assert_eq!(timestamp(like.created_at), event);
    assert_eq!(like.metadata["event_time_source"], "discourse_user_action");
    assert!(
        rows.iter()
            .all(|row| row.person_id == Some(identity.person_id.into_inner()))
    );
    let estimated = rows
        .iter()
        .find(|row| row.platform_id == "like-103-manual")
        .unwrap();
    assert_eq!(timestamp(estimated.created_at), "2020-01-01T00:00:00Z");
    assert_eq!(
        estimated.metadata["event_time_source"],
        "discourse_user_action_pending_correction"
    );
    assert_eq!(
        sqlx::query_scalar!("SELECT count(*) FROM activity.contribution_changes")
            .fetch_one(&ctx.pool)
            .await
            .unwrap(),
        Some(0)
    );
    // A later global estimate cannot undo confirmed action metadata or event time.
    estimate.platform_id = "like-102-manual".into();
    estimate.created_at = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap();
    source.store_batch(&ingestion, &[estimate]).await.unwrap();
    let row = sqlx::query!("SELECT created_at, metadata FROM activity.contributions WHERE platform_id = 'like-102-manual'").fetch_one(&ctx.pool).await.unwrap();
    assert_eq!(timestamp(row.created_at), event);
    assert_eq!(row.metadata["event_time_source"], "discourse_user_action");
    failed.store(true, Ordering::SeqCst);
    let plan = source.plan(&ingestion).await.unwrap();
    // A transient/request failure aborts without committing target coverage.
    let mut cursor = source.initial_cursor(&ingestion, &plan);
    let mut observed_failure = false;
    for _ in 0..10 {
        match source.fetch_batch(&ingestion, &cursor).await {
            Ok(result) => {
                if let Some(next) = result.next_cursor {
                    cursor = next
                } else {
                    break;
                }
            }
            Err(_) => {
                observed_failure = true;
                break;
            }
        }
    }
    assert!(observed_failure);
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
    failed.store(false, Ordering::SeqCst);
    let plan = source.plan(&ingestion).await.unwrap();
    drive(
        &ingestion,
        create_source(&platform).unwrap().as_ref(),
        source.initial_cursor(&ingestion, &plan),
    )
    .await;
    assert_eq!(
        sqlx::query_scalar!("SELECT count(*) FROM activity.contributions")
            .fetch_one(&ctx.pool)
            .await
            .unwrap(),
        Some(3)
    );
    // The explicit person backfill supplies owned, auditable timestamp repair.
    let before =
        sqlx::query!("SELECT id FROM activity.contributions WHERE platform_id = 'like-103-manual'")
            .fetch_one(&ctx.pool)
            .await
            .unwrap();
    let mut scoped = ingestion.clone();
    let request = ps_core::ingestion::SourceRunContext {
        pipeline_id: uuid::Uuid::now_v7(),
        scope: ps_core::ingestion::PipelineScope::Person {
            person_id: identity.person_id,
        },
        source: ps_core::ingestion::SelectedSource {
            source_id: ingestion.source_config.id,
            source_name: ingestion.source_config.name.clone(),
            platform: platform.clone(),
            identity: Some(identity.clone()),
        },
        since_date: Some("2020-01-01".into()),
        run_started_at: OffsetDateTime::now_utc().replace_nanosecond(0).unwrap(),
        processing: ps_core::ingestion::ProcessingScope::Person {
            person_id: identity.person_id,
        },
    };
    ctx.repos
        .activity
        .reserve_pipeline(
            request.pipeline_id,
            &serde_json::to_value(ps_core::ingestion::PipelineRequest {
                scope: request.scope.clone(),
                sources: vec![request.source.clone()],
                since_date: request.since_date.clone(),
                run_started_at: request.run_started_at,
                processing: request.processing.clone(),
            })
            .unwrap(),
            uuid::Uuid::now_v7(),
            "test-admin",
        )
        .await
        .unwrap();
    let run_id = uuid::Uuid::now_v7();
    ctx.repos
        .activity
        .create_pipeline_run(ps_core::repo::activity::PipelineRunParams {
            run_id,
            pipeline_id: request.pipeline_id,
            source_name: &ps_core::models::SourceName::new(&ingestion.source_config.name),
            handler_name: &ps_core::models::HandlerName::new("test-person-adapter"),
            method: &ps_core::models::HandlerMethod::new("run_scoped"),
            invocation_id: "test-invocation",
        })
        .await
        .unwrap();
    scoped.request = Some(request);
    scoped.run_id = Some(run_id);
    let plan = source.plan(&scoped).await.unwrap();
    drive(
        &scoped,
        source.as_ref(),
        source.initial_cursor(&scoped, &plan),
    )
    .await;
    let corrected = sqlx::query!("SELECT id, created_at, metadata FROM activity.contributions WHERE platform_id = 'like-103-manual'").fetch_one(&ctx.pool).await.unwrap();
    assert_eq!(corrected.id, before.id);
    assert_eq!(timestamp(corrected.created_at), event);
    assert_eq!(
        corrected.metadata["event_time_source"],
        "discourse_user_action"
    );
    let change = sqlx::query!("SELECT id, previous_created_at, current_created_at, affected_periods FROM activity.contribution_changes WHERE contribution_id=$1", before.id).fetch_one(&ctx.pool).await.unwrap();
    assert_eq!(
        timestamp(change.previous_created_at.unwrap()),
        "2020-01-01T00:00:00Z"
    );
    assert_eq!(timestamp(change.current_created_at), event);
    let periods = change.affected_periods.as_array().unwrap();
    assert!(
        periods
            .iter()
            .any(|period| period["period_start"].as_str().unwrap().starts_with("2020"))
    );
    assert!(
        periods
            .iter()
            .any(|period| period["period_start"].as_str().unwrap().starts_with("2026"))
    );
    assert!(
        sqlx::query_scalar!(
            "SELECT count(*) FROM activity.snapshot_invalidations WHERE change_id=$1",
            change.id
        )
        .fetch_one(&ctx.pool)
        .await
        .unwrap()
        .unwrap()
            >= 6
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn ordinary_jira_resolves_saved_unassigned_current_assignee_by_account_id() {
    let ctx = SourceTestContext::new().await;
    let ingestion = ctx
        .build_ingestion_ctx(
            "jira",
            Platform::Jira,
            serde_json::json!({"base_url":ctx.mock_server.uri(), "projects":["PROJ"]}),
            Some("fake-token".into()),
            Some("admin@example.com".into()),
            None,
        )
        .await;
    let selected = person(&ctx, Platform::Jira, "saved-login", Some("Cloud:Manual")).await;
    let event =
        timestamp(OffsetDateTime::now_utc().replace_nanosecond(0).unwrap() - Duration::hours(1));
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(jira_search_response(
                &[jira_issue_node(
                    "PROJ-1",
                    "Manual person's ticket",
                    "indeterminate",
                    "Cloud:Manual",
                    "2020-01-01T00:00:00Z",
                    &event,
                )],
                true,
                None,
            )),
        )
        .mount(&ctx.mock_server)
        .await;
    let source = create_source(&Platform::Jira).unwrap();
    let plan = source.plan(&ingestion).await.unwrap();
    drive(
        &ingestion,
        source.as_ref(),
        source.initial_cursor(&ingestion, &plan),
    )
    .await;
    let row =
        sqlx::query!("SELECT person_id FROM activity.contributions WHERE platform_id = 'PROJ-1'")
            .fetch_one(&ctx.pool)
            .await
            .unwrap();
    assert_eq!(row.person_id, Some(selected.person_id.into_inner()));
    ctx.teardown().await;
}
