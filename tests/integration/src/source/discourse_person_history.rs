use super::*;

#[tokio::test]
async fn activity_pagination_has_no_latest_listing_page_cap() {
    let ctx = SourceTestContext::new().await;
    let context = person_context(&ctx, false).await;
    let all: Vec<_> = (1..=2651)
        .map(|id| action(id, 6, id, 10, "alice"))
        .collect();
    Mock::given(path("/user_actions.json"))
        .respond_with(move |request: &wiremock::Request| {
            let offset = request
                .url
                .query_pairs()
                .find(|(key, _)| key == "offset")
                .unwrap()
                .1
                .parse::<usize>()
                .unwrap();
            let page = all
                .iter()
                .skip(offset)
                .take(60)
                .cloned()
                .collect::<Vec<_>>();
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"user_actions":page}))
        })
        .mount(&ctx.mock_server)
        .await;
    let source = discourse_source();
    let plan = source.plan(&context).await.unwrap();
    let mut cursor = source.initial_cursor(&context, &plan);
    let mut pages = 0;
    loop {
        let result = source.fetch_batch(&context, &cursor).await.unwrap();
        pages += 1;
        assert!(result.items.is_empty());
        match result.next_cursor {
            Some(next) => cursor = next,
            None => break,
        }
        assert!(pages < 60);
    }
    assert!(pages > 50);
    ctx.teardown().await;
}

#[tokio::test]
async fn overlap_resumes_with_new_actions_and_detects_deleted_or_repeated_pages() {
    for mutation in [false, true] {
        let ctx = SourceTestContext::new().await;
        let context = person_context(&ctx, false).await;
        let actions: Vec<_> = (1..=110).map(|id| action(id, 6, id, 10, "alice")).collect();
        let first = actions[..60].to_vec();
        Mock::given(path("/user_actions.json"))
            .and(query_param("offset", "0"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"user_actions":first})),
            )
            .mount(&ctx.mock_server)
            .await;
        let initial = fetch_first(&context).await;
        let cursor = initial.next_cursor.unwrap();
        ctx.mock_server.reset().await;
        let second = if mutation {
            actions[..60].to_vec()
        } else {
            actions[47..107].to_vec()
        };
        Mock::given(path("/user_actions.json"))
            .and(query_param("offset", "50"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"user_actions":second})),
            )
            .mount(&ctx.mock_server)
            .await;
        let resumed = discourse_source()
            .fetch_batch(&context, &cursor)
            .await
            .unwrap();
        let evidence: serde_json::Value =
            serde_json::from_str(resumed.etag.as_ref().unwrap()).unwrap();
        if mutation {
            assert!(resumed.next_cursor.is_none());
            assert!(!evidence["failed_items"].as_array().unwrap().is_empty());
        } else {
            assert!(resumed.next_cursor.is_some());
            assert!(evidence["failed_items"].as_array().unwrap().is_empty());
            assert_eq!(evidence["seen_actions"].as_array().unwrap().len(), 60);
        }
        ctx.teardown().await;
    }
}

#[tokio::test]
async fn cursor_freezes_source_settings_and_window_filters_inclusively() {
    let ctx = SourceTestContext::new().await;
    let mut context = person_context(&ctx, true).await;
    let mut lower = action(1, 5, 1001, 101, "alice");
    lower["created_at"] = "2025-03-01T00:00:00Z".into();
    let mut upper = action(2, 1, 1002, 102, "alice");
    upper["created_at"] = "2025-04-01T00:00:00Z".into();
    let mut future = action(3, 1, 1003, 103, "alice");
    future["created_at"] = "2025-04-01T00:00:01Z".into();
    let mut older = action(4, 5, 1004, 104, "alice");
    older["created_at"] = "2025-02-28T23:59:59Z".into();
    mount_feed(&ctx, vec![future, upper, lower, older]).await;
    mount_detail(&ctx, 1001, 101, "alice", 50, 1, 100).await;
    mount_detail(&ctx, 1002, 102, "bob", 5, 1, 100).await;
    let source = discourse_source();
    let plan = source.plan(&context).await.unwrap();
    let cursor = source.initial_cursor(&context, &plan);
    context.source_config.settings["base_url"] = "https://changed.invalid".into();
    context.source_config.settings["categories"] = serde_json::json!([900]);
    context.source_config.settings["fetch_likes"] = false.into();
    let result = source.fetch_batch(&context, &cursor).await.unwrap();
    assert_eq!(result.items.len(), 2);
    assert!(
        result
            .items
            .iter()
            .any(|i| i.platform_id.as_str() == "like-1002-alice")
    );
    assert!(
        result
            .items
            .iter()
            .all(|i| i.url.as_ref().unwrap().starts_with(&ctx.mock_server.uri()))
    );
    // A different instance-bound snapshot may never consume this checkpoint.
    context.request.as_mut().unwrap().source.platform = Platform::Discourse("snapcraft".into());
    assert!(source.fetch_batch(&context, &cursor).await.is_err());
    ctx.teardown().await;
}

#[tokio::test]
async fn same_username_and_numeric_ids_remain_distinct_between_instances() {
    let ctx = SourceTestContext::new().await;
    let ubuntu = person_context(&ctx, true).await;
    mount_feed(&ctx, vec![action(1, 1, 1001, 101, "alice")]).await;
    mount_detail(&ctx, 1001, 101, "bob", 40, 1, 100).await;
    let result = fetch_first(&ubuntu).await;
    discourse_source()
        .store_batch(&ubuntu, &result.items)
        .await
        .unwrap();
    ctx.repos
        .activity
        .complete_pipeline(
            ubuntu.request.as_ref().unwrap().pipeline_id,
            ps_core::models::IngestionStatus::Completed.as_str(),
            &serde_json::json!([]),
            None,
        )
        .await
        .unwrap();
    let mut snapcraft = ctx
        .build_ingestion_ctx(
            "discourse-snapcraft",
            Platform::Discourse("snapcraft".into()),
            discourse_settings(&ctx.mock_server.uri()),
            Some("test-api-key".into()),
            None,
            Some("system".into()),
        )
        .await;
    snapcraft.source_config.settings["fetch_likes"] = true.into();
    let upper = ubuntu.request.as_ref().unwrap().run_started_at;
    ctx.with_person_scope(&mut snapcraft, "alice", None, "2025-03-01", upper)
        .await;
    let second = fetch_first(&snapcraft).await;
    discourse_source()
        .store_batch(&snapcraft, &second.items)
        .await
        .unwrap();
    assert_eq!(result.items[0].platform_id, second.items[0].platform_id);
    assert_ne!(result.items[0].platform, second.items[0].platform);
    let rows = ctx
        .repos
        .activity
        .get_contribution_ids_by_platform_ids("discourse-ubuntu", &["like-1001-alice".into()])
        .await
        .unwrap();
    let other = ctx
        .repos
        .activity
        .get_contribution_ids_by_platform_ids("discourse-snapcraft", &["like-1001-alice".into()])
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(other.len(), 1);
    assert_ne!(rows[0].0, other[0].0);
    ctx.teardown().await;
}

#[tokio::test]
async fn transient_feed_failure_retries_without_claiming_empty_history() {
    let ctx = SourceTestContext::new().await;
    let context = person_context(&ctx, false).await;
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let responses = calls.clone();
    Mock::given(path("/user_actions.json"))
        .respond_with(move |_: &wiremock::Request| {
            if responses.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                ResponseTemplate::new(503)
            } else {
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"user_actions":[action(1,5,1001,101,"alice")]}),
                )
            }
        })
        .mount(&ctx.mock_server)
        .await;
    mount_detail(&ctx, 1001, 101, "alice", 40, 1, 100).await;
    let result = fetch_first(&context).await;
    assert_eq!(result.items.len(), 1);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    ctx.teardown().await;
}

#[tokio::test]
async fn provider_error_bodies_and_invalid_event_values_do_not_enter_checkpoints() {
    const SENTINEL: &str = "private-provider-error-secret-sentinel";
    for malformed_time in [false, true] {
        let ctx = SourceTestContext::new().await;
        let context = person_context(&ctx, false).await;
        ctx.mock_server.reset().await;
        Mock::given(path("/categories.json"))
            .respond_with(ResponseTemplate::new(403).set_body_string(SENTINEL))
            .mount(&ctx.mock_server)
            .await;
        let mut event = action(1, 5, 1001, 101, "alice");
        if malformed_time {
            event["created_at"] = SENTINEL.into();
        }
        mount_feed(&ctx, vec![event]).await;
        Mock::given(path("/posts/1001.json"))
            .respond_with(ResponseTemplate::new(404).set_body_string(SENTINEL))
            .mount(&ctx.mock_server)
            .await;
        Mock::given(path("/t/101.json"))
            .respond_with(ResponseTemplate::new(403).set_body_string(SENTINEL))
            .mount(&ctx.mock_server)
            .await;
        let result = fetch_first(&context).await;
        assert!(result.items.is_empty());
        let checkpoint = result.etag.unwrap();
        assert!(!checkpoint.contains(SENTINEL));
        let checkpoint: serde_json::Value = serde_json::from_str(&checkpoint).unwrap();
        assert!(!checkpoint["failed_items"].as_array().unwrap().is_empty());
        if !malformed_time {
            ctx.mock_server.reset().await;
            Mock::given(path("/posts/1001.json"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": SENTINEL})),
                )
                .mount(&ctx.mock_server)
                .await;
            let client = ps_workers::features::ingestion::discourse::client::DiscourseClient::new(
                context.http_client.clone(),
                &ctx.mock_server.uri(),
                "test-api-key",
                "system",
            );
            let error = client.post(1001).await.unwrap_err();
            assert!(!error.to_string().contains(SENTINEL));
            assert!(error.is_transient());
        }
        ctx.teardown().await;
    }
}
