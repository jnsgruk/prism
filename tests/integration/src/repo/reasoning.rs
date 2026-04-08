use crate::common::db::RepoTestContext;
use ps_core::models::EnrichmentType;
use ps_core::repo::reasoning::{
    CreateConversationParams, CreateMessageParams, EnrichmentResult, UpsertEnrichmentParams,
};
use time::OffsetDateTime;
use uuid::Uuid;

/// Insert a minimal contribution for testing enrichments against.
async fn insert_contribution(pool: &sqlx::PgPool, platform_id: &str) -> Uuid {
    let id = Uuid::now_v7();
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "INSERT INTO activity.contributions \
         (id, platform, contribution_type, platform_id, title, state, created_at, metrics, metadata) \
         VALUES ($1, 'github', 'pr_review', $2, 'Test PR', 'open', $3, '{}'::jsonb, '{}'::jsonb)",
    )
    .bind(id)
    .bind(platform_id)
    .bind(now)
    .execute(pool)
    .await
    .expect("insert contribution");
    id
}

/// Insert a contribution with specific type and content for enrichment eligibility testing.
async fn insert_typed_contribution(
    pool: &sqlx::PgPool,
    platform_id: &str,
    ctype: &str,
    content: Option<&str>,
    additions: Option<i32>,
    deletions: Option<i32>,
) -> Uuid {
    let id = Uuid::now_v7();
    let now = OffsetDateTime::now_utc();
    let metrics = serde_json::json!({
        "additions": additions,
        "deletions": deletions,
    });
    sqlx::query(
        "INSERT INTO activity.contributions \
         (id, platform, contribution_type, platform_id, title, state, created_at, metrics, metadata, content) \
         VALUES ($1, 'github', $2, $3, 'Test', 'open', $4, $5, '{}'::jsonb, $6)",
    )
    .bind(id)
    .bind(ctype)
    .bind(platform_id)
    .bind(now)
    .bind(&metrics)
    .bind(content)
    .execute(pool)
    .await
    .expect("insert contribution");
    id
}

// ---------------------------------------------------------------------------
// API usage
// ---------------------------------------------------------------------------

#[tokio::test]
async fn log_api_usage_and_query_by_task() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;

    repos
        .reasoning
        .log_api_usage("google", "gemini-pro", "enrichment", 1000, 500)
        .await
        .unwrap();
    repos
        .reasoning
        .log_api_usage("google", "gemini-pro", "agentic", 500, 200)
        .await
        .unwrap();

    let since = OffsetDateTime::now_utc() - time::Duration::hours(1);
    let until = OffsetDateTime::now_utc() + time::Duration::hours(1);

    let breakdown = repos
        .reasoning
        .get_usage_by_task(since, until)
        .await
        .unwrap();
    assert_eq!(breakdown.len(), 2);
    assert_eq!(breakdown[0].request_count, 1);

    ctx.teardown().await;
}

#[tokio::test]
async fn get_usage_by_model() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;

    repos
        .reasoning
        .log_api_usage("google", "gemini-pro", "enrichment", 100, 50)
        .await
        .unwrap();
    repos
        .reasoning
        .log_api_usage("google", "gemini-2.5-pro", "agentic", 200, 100)
        .await
        .unwrap();

    let since = OffsetDateTime::now_utc() - time::Duration::hours(1);
    let until = OffsetDateTime::now_utc() + time::Duration::hours(1);

    let summary = repos
        .reasoning
        .get_usage_by_model(since, until)
        .await
        .unwrap();
    assert_eq!(summary.len(), 2);

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Enrichments
// ---------------------------------------------------------------------------

#[tokio::test]
async fn upsert_enrichment_and_retrieve() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let contrib_id = insert_contribution(pool, "enrich-1").await;

    let params = UpsertEnrichmentParams {
        contribution_id: contrib_id,
        enrichment_type: EnrichmentType::ReviewDepth,
        value: &serde_json::json!({"depth": "thorough", "categories": ["architecture"]}),
        model_name: "gemini-pro",
        confidence: Some(0.9),
        input_hash: Some("abc123"),
        input_preview: Some("This PR refactors..."),
    };

    let id = repos.reasoning.upsert_enrichment(&params).await.unwrap();

    let enrichments = repos
        .reasoning
        .get_enrichments_for_contribution(contrib_id)
        .await
        .unwrap();
    assert_eq!(enrichments.len(), 1);
    assert_eq!(enrichments[0].id, id);
    assert_eq!(enrichments[0].enrichment_type, EnrichmentType::ReviewDepth);
    assert!((enrichments[0].confidence.unwrap() - 0.9).abs() < 0.01);

    ctx.teardown().await;
}

#[tokio::test]
async fn upsert_enrichment_replaces_on_conflict() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let contrib_id = insert_contribution(pool, "enrich-replace").await;

    let params1 = UpsertEnrichmentParams {
        contribution_id: contrib_id,
        enrichment_type: EnrichmentType::Sentiment,
        value: &serde_json::json!({"sentiment": "positive"}),
        model_name: "gemini-pro",
        confidence: Some(0.7),
        input_hash: None,
        input_preview: None,
    };
    repos.reasoning.upsert_enrichment(&params1).await.unwrap();

    let params2 = UpsertEnrichmentParams {
        contribution_id: contrib_id,
        enrichment_type: EnrichmentType::Sentiment,
        value: &serde_json::json!({"sentiment": "neutral"}),
        model_name: "gemini-pro-2",
        confidence: Some(0.85),
        input_hash: None,
        input_preview: None,
    };
    repos.reasoning.upsert_enrichment(&params2).await.unwrap();

    let enrichments = repos
        .reasoning
        .get_enrichments_for_contribution(contrib_id)
        .await
        .unwrap();
    assert_eq!(enrichments.len(), 1);
    assert_eq!(enrichments[0].value["sentiment"], "neutral");
    assert_eq!(enrichments[0].model_name, "gemini-pro-2");

    ctx.teardown().await;
}

#[tokio::test]
async fn bulk_upsert_enrichments() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let c1 = insert_contribution(pool, "bulk-e-1").await;
    let c2 = insert_contribution(pool, "bulk-e-2").await;

    let results = vec![
        EnrichmentResult {
            contribution_id: c1,
            enrichment_type: EnrichmentType::ReviewDepth,
            value: serde_json::json!({"depth": "surface"}),
            confidence: 0.6,
            input_hash: "h1".into(),
            input_preview: "preview 1".into(),
        },
        EnrichmentResult {
            contribution_id: c2,
            enrichment_type: EnrichmentType::ReviewDepth,
            value: serde_json::json!({"depth": "thorough"}),
            confidence: 0.9,
            input_hash: "h2".into(),
            input_preview: "preview 2".into(),
        },
    ];

    let count = repos
        .reasoning
        .bulk_upsert_enrichments(&results, "gemini-pro")
        .await
        .unwrap();
    assert_eq!(count, 2);

    let e1 = repos
        .reasoning
        .get_enrichments_for_contribution(c1)
        .await
        .unwrap();
    assert_eq!(e1.len(), 1);
    assert_eq!(e1[0].model_name, "gemini-pro");

    ctx.teardown().await;
}

#[tokio::test]
async fn bulk_upsert_enrichments_empty_is_noop() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let count = repos
        .reasoning
        .bulk_upsert_enrichments(&[], "model")
        .await
        .unwrap();
    assert_eq!(count, 0);

    ctx.teardown().await;
}

#[tokio::test]
async fn get_enrichments_for_contributions_batch() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let c1 = insert_contribution(pool, "batch-e-1").await;
    let c2 = insert_contribution(pool, "batch-e-2").await;

    let params = UpsertEnrichmentParams {
        contribution_id: c1,
        enrichment_type: EnrichmentType::ReviewDepth,
        value: &serde_json::json!({"depth": "thorough"}),
        model_name: "model",
        confidence: None,
        input_hash: None,
        input_preview: None,
    };
    repos.reasoning.upsert_enrichment(&params).await.unwrap();

    let all = repos
        .reasoning
        .get_enrichments_for_contributions(&[c1, c2])
        .await
        .unwrap();
    assert_eq!(all.len(), 1); // Only c1 has an enrichment

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Enrichment status & unenriched
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_enrichment_status_empty() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let status = repos.reasoning.get_enrichment_status().await.unwrap();
    assert_eq!(status.total_enrichments, 0);
    assert!(status.last_enrichment_at.is_none());

    ctx.teardown().await;
}

#[tokio::test]
async fn find_unenriched_contributions() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    // Create a PR review with content (eligible for review_depth)
    let c1 = insert_typed_contribution(pool, "unenriched-1", "pr_review", Some("LGTM"), None, None)
        .await;
    // Create another with no content (not eligible)
    let _c2 = insert_typed_contribution(pool, "unenriched-2", "pr_review", None, None, None).await;

    let results = repos
        .reasoning
        .find_unenriched_contributions(EnrichmentType::ReviewDepth, 10)
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, c1);

    ctx.teardown().await;
}

#[tokio::test]
async fn find_unenriched_prs_by_size() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    // PR with >50 lines (eligible for significance)
    let _c1 =
        insert_typed_contribution(pool, "big-pr", "pull_request", None, Some(40), Some(20)).await;
    // PR with <=50 lines (not eligible)
    let _c2 =
        insert_typed_contribution(pool, "small-pr", "pull_request", None, Some(10), Some(5)).await;

    let results = repos
        .reasoning
        .find_unenriched_contributions(EnrichmentType::Significance, 10)
        .await
        .unwrap();
    assert_eq!(results.len(), 1);

    ctx.teardown().await;
}

// The previous test for unknown enrichment type returning empty is no longer
// needed — the EnrichmentType enum makes invalid types a compile-time error.

#[tokio::test]
async fn delete_enrichments_by_type() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let c1 = insert_contribution(pool, "del-type-1").await;

    let params = UpsertEnrichmentParams {
        contribution_id: c1,
        enrichment_type: EnrichmentType::ReviewDepth,
        value: &serde_json::json!({}),
        model_name: "model",
        confidence: None,
        input_hash: None,
        input_preview: None,
    };
    repos.reasoning.upsert_enrichment(&params).await.unwrap();

    let params2 = UpsertEnrichmentParams {
        contribution_id: c1,
        enrichment_type: EnrichmentType::Sentiment,
        value: &serde_json::json!({}),
        model_name: "model",
        confidence: None,
        input_hash: None,
        input_preview: None,
    };
    repos.reasoning.upsert_enrichment(&params2).await.unwrap();

    let deleted = repos
        .reasoning
        .delete_enrichments_by_type(EnrichmentType::ReviewDepth)
        .await
        .unwrap();
    assert_eq!(deleted, 1);

    let remaining = repos
        .reasoning
        .get_enrichments_for_contribution(c1)
        .await
        .unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].enrichment_type, EnrichmentType::Sentiment);

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Conversations
// ---------------------------------------------------------------------------

/// Insert a user for FK satisfaction. Returns the user_id.
async fn insert_user(pool: &sqlx::PgPool) -> Uuid {
    let (user_id, _) = crate::common::fixtures::create_admin_user(pool).await;
    user_id
}

#[tokio::test]
async fn conversation_create_and_get() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let user_id = insert_user(pool).await;

    let conv = repos
        .reasoning
        .create_conversation(&CreateConversationParams {
            user_id,
            title: Some("Test conversation"),
            model_name: "anthropic/claude-sonnet-4-6",
        })
        .await
        .unwrap();

    assert_eq!(conv.user_id, user_id);
    assert_eq!(conv.title.as_deref(), Some("Test conversation"));
    assert_eq!(conv.status, "active");
    assert_eq!(conv.container_status, "pending");
    assert_eq!(conv.total_tool_calls, 0);

    // Get by ID.
    let fetched = repos.reasoning.get_conversation(conv.id).await.unwrap();
    assert!(fetched.is_some());
    let fetched = fetched.unwrap();
    assert_eq!(fetched.id, conv.id);
    assert_eq!(fetched.model_name, "anthropic/claude-sonnet-4-6");

    // Get non-existent returns None.
    let missing = repos
        .reasoning
        .get_conversation(Uuid::now_v7())
        .await
        .unwrap();
    assert!(missing.is_none());

    ctx.teardown().await;
}

#[tokio::test]
async fn conversation_list_with_counts() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let user_id = insert_user(pool).await;

    // Create 3 conversations.
    for i in 0..3 {
        let conv = repos
            .reasoning
            .create_conversation(&CreateConversationParams {
                user_id,
                title: Some(&format!("Conv {i}")),
                model_name: "test-model",
            })
            .await
            .unwrap();

        // Add a message to the first two.
        if i < 2 {
            repos
                .reasoning
                .create_message(&CreateMessageParams {
                    conversation_id: conv.id,
                    role: "user",
                    content: "Hello",
                    reasoning_trace: None,
                    supporting_data: None,
                    prompt_tokens: 10,
                    completion_tokens: 0,
                })
                .await
                .unwrap();
        }
    }

    let (list, total) = repos
        .reasoning
        .list_conversations(user_id, 10, 0)
        .await
        .unwrap();

    assert_eq!(total, 3);
    assert_eq!(list.len(), 3);
    // Newest first.
    assert_eq!(list[0].title.as_deref(), Some("Conv 2"));
    // First two have 1 message, third has 0.
    assert_eq!(list[2].message_count, 1); // Conv 0 (oldest, at end)
    assert_eq!(list[0].message_count, 0); // Conv 2 (newest)

    // Pagination.
    let (page, _) = repos
        .reasoning
        .list_conversations(user_id, 2, 0)
        .await
        .unwrap();
    assert_eq!(page.len(), 2);

    ctx.teardown().await;
}

#[tokio::test]
async fn conversation_multi_turn_messages() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let user_id = insert_user(pool).await;

    let conv = repos
        .reasoning
        .create_conversation(&CreateConversationParams {
            user_id,
            title: None,
            model_name: "test-model",
        })
        .await
        .unwrap();

    // Simulate 4 alternating turns.
    let trace = serde_json::json!({"steps": [{"tool_name": "list_teams"}]});
    let citations = serde_json::json!({"sources": ["team_abc"]});

    repos
        .reasoning
        .create_message(&CreateMessageParams {
            conversation_id: conv.id,
            role: "user",
            content: "How is Team A doing?",
            reasoning_trace: None,
            supporting_data: None,
            prompt_tokens: 20,
            completion_tokens: 0,
        })
        .await
        .unwrap();

    repos
        .reasoning
        .create_message(&CreateMessageParams {
            conversation_id: conv.id,
            role: "assistant",
            content: "Team A is doing great.",
            reasoning_trace: Some(&trace),
            supporting_data: Some(&citations),
            prompt_tokens: 0,
            completion_tokens: 50,
        })
        .await
        .unwrap();

    repos
        .reasoning
        .create_message(&CreateMessageParams {
            conversation_id: conv.id,
            role: "user",
            content: "Compare with Team B?",
            reasoning_trace: None,
            supporting_data: None,
            prompt_tokens: 15,
            completion_tokens: 0,
        })
        .await
        .unwrap();

    repos
        .reasoning
        .create_message(&CreateMessageParams {
            conversation_id: conv.id,
            role: "assistant",
            content: "Team B trails behind.",
            reasoning_trace: Some(&trace),
            supporting_data: None,
            prompt_tokens: 0,
            completion_tokens: 40,
        })
        .await
        .unwrap();

    // List messages — oldest first.
    let messages = repos.reasoning.list_messages(conv.id).await.unwrap();
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[0].role, "user");
    assert_eq!(messages[1].role, "assistant");
    assert_eq!(messages[2].role, "user");
    assert_eq!(messages[3].role, "assistant");
    assert!(messages[1].reasoning_trace.is_some());
    assert!(messages[3].supporting_data.is_none());

    ctx.teardown().await;
}

#[tokio::test]
async fn conversation_container_status_transitions() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let user_id = insert_user(pool).await;

    let conv = repos
        .reasoning
        .create_conversation(&CreateConversationParams {
            user_id,
            title: Some("Container test"),
            model_name: "test-model",
        })
        .await
        .unwrap();
    assert_eq!(conv.container_status, "pending");

    // Transition: pending → active.
    repos
        .reasoning
        .update_container_status(
            conv.id,
            Some("prism-agent-abc123"),
            "active",
            Some("oc-session-xyz"),
        )
        .await
        .unwrap();

    let fetched = repos
        .reasoning
        .get_conversation(conv.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.container_status, "active");
    assert_eq!(
        fetched.container_pod_name.as_deref(),
        Some("prism-agent-abc123")
    );
    assert_eq!(
        fetched.opencode_session_id.as_deref(),
        Some("oc-session-xyz")
    );

    // Transition: active → reaped.
    repos
        .reasoning
        .update_container_status(conv.id, None, "reaped", None)
        .await
        .unwrap();

    let fetched = repos
        .reasoning
        .get_conversation(conv.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.container_status, "reaped");
    // Pod name is preserved (COALESCE keeps existing value).
    assert_eq!(
        fetched.container_pod_name.as_deref(),
        Some("prism-agent-abc123")
    );

    // Transition: reaped → active (resume with new pod).
    repos
        .reasoning
        .update_container_status(
            conv.id,
            Some("prism-agent-def456"),
            "active",
            Some("oc-session-new"),
        )
        .await
        .unwrap();

    let fetched = repos
        .reasoning
        .get_conversation(conv.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.container_status, "active");
    assert_eq!(
        fetched.container_pod_name.as_deref(),
        Some("prism-agent-def456")
    );

    ctx.teardown().await;
}

#[tokio::test]
async fn conversation_update_totals() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let user_id = insert_user(pool).await;

    let conv = repos
        .reasoning
        .create_conversation(&CreateConversationParams {
            user_id,
            title: None,
            model_name: "test-model",
        })
        .await
        .unwrap();

    // First turn.
    repos
        .reasoning
        .update_conversation_totals(conv.id, 5, 1000, 500)
        .await
        .unwrap();

    // Second turn.
    repos
        .reasoning
        .update_conversation_totals(conv.id, 3, 800, 400)
        .await
        .unwrap();

    let fetched = repos
        .reasoning
        .get_conversation(conv.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.total_tool_calls, 8);
    assert_eq!(fetched.total_prompt_tokens, 1800);
    assert_eq!(fetched.total_completion_tokens, 900);

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Conversation export (backup)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn conversation_export_roundtrip() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let user_id = insert_user(pool).await;

    let conv = repos
        .reasoning
        .create_conversation(&CreateConversationParams {
            user_id,
            title: Some("Export test"),
            model_name: "test-model",
        })
        .await
        .unwrap();

    let msg = repos
        .reasoning
        .create_message(&CreateMessageParams {
            conversation_id: conv.id,
            role: "user",
            content: "What is the team velocity?",
            reasoning_trace: None,
            supporting_data: None,
            prompt_tokens: 25,
            completion_tokens: 0,
        })
        .await
        .unwrap();

    let _assistant_msg = repos
        .reasoning
        .create_message(&CreateMessageParams {
            conversation_id: conv.id,
            role: "assistant",
            content: "The team velocity is 42 points per sprint.",
            reasoning_trace: Some(&serde_json::json!({"steps": ["query_metrics"]})),
            supporting_data: None,
            prompt_tokens: 0,
            completion_tokens: 60,
        })
        .await
        .unwrap();

    // Verify count.
    let count = repos.reasoning.count_conversations().await.unwrap();
    assert_eq!(count, 1);

    // Export conversations.
    let exported_convs = repos.reasoning.export_conversations().await.unwrap();
    assert_eq!(exported_convs.len(), 1);
    assert_eq!(exported_convs[0]["id"], conv.id.to_string());
    assert_eq!(exported_convs[0]["title"], "Export test");
    assert_eq!(exported_convs[0]["status"], "active");
    assert_eq!(exported_convs[0]["model_name"], "test-model");
    assert_eq!(exported_convs[0]["user_id"], user_id.to_string());

    // Export messages.
    let exported_msgs = repos
        .reasoning
        .export_conversation_messages()
        .await
        .unwrap();
    assert_eq!(exported_msgs.len(), 2);
    assert_eq!(exported_msgs[0]["role"], "user");
    assert_eq!(exported_msgs[0]["content"], "What is the team velocity?");
    assert_eq!(exported_msgs[0]["conversation_id"], conv.id.to_string());
    assert_eq!(exported_msgs[1]["role"], "assistant");
    assert!(exported_msgs[1]["reasoning_trace"].is_object());

    drop(msg);

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Conversation events — Plan 57
// ---------------------------------------------------------------------------

#[tokio::test]
async fn conversation_events_append_and_poll() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let user_id = insert_user(pool).await;
    let conv = repos
        .reasoning
        .create_conversation(&CreateConversationParams {
            user_id,
            title: Some("test events"),
            model_name: "test-model",
        })
        .await
        .unwrap();

    // Append events.
    repos
        .reasoning
        .append_event(
            conv.id,
            "container_status",
            &serde_json::json!({"status": "creating", "message": "Starting..."}),
            None,
            None,
        )
        .await
        .unwrap();
    repos
        .reasoning
        .append_event(
            conv.id,
            "tool_call_started",
            &serde_json::json!({"tool_name": "list_teams", "arguments_json": "{}"}),
            None,
            None,
        )
        .await
        .unwrap();

    // Poll from start.
    let events = repos.reasoning.poll_events(conv.id, 0).await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_type, "container_status");
    assert_eq!(events[1].event_type, "tool_call_started");

    // Poll from cursor (after first event).
    let events = repos
        .reasoning
        .poll_events(conv.id, events[0].id)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "tool_call_started");

    ctx.teardown().await;
}

#[tokio::test]
async fn conversation_events_poll_empty() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let user_id = insert_user(pool).await;
    let conv = repos
        .reasoning
        .create_conversation(&CreateConversationParams {
            user_id,
            title: Some("empty events"),
            model_name: "test-model",
        })
        .await
        .unwrap();

    let events = repos.reasoning.poll_events(conv.id, 0).await.unwrap();
    assert!(events.is_empty());

    ctx.teardown().await;
}

#[tokio::test]
async fn conversation_events_delete() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let user_id = insert_user(pool).await;
    let conv = repos
        .reasoning
        .create_conversation(&CreateConversationParams {
            user_id,
            title: Some("delete events"),
            model_name: "test-model",
        })
        .await
        .unwrap();

    repos
        .reasoning
        .append_event(
            conv.id,
            "container_status",
            &serde_json::json!({"status": "ready"}),
            None,
            None,
        )
        .await
        .unwrap();
    repos
        .reasoning
        .append_event(
            conv.id,
            "final_answer",
            &serde_json::json!({"answer": "done"}),
            None,
            None,
        )
        .await
        .unwrap();

    let deleted = repos.reasoning.delete_events(conv.id).await.unwrap();
    assert_eq!(deleted, 2);

    let events = repos.reasoning.poll_events(conv.id, 0).await.unwrap();
    assert!(events.is_empty());

    ctx.teardown().await;
}

#[tokio::test]
async fn conversation_events_cursor_ordering() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let user_id = insert_user(pool).await;
    let conv = repos
        .reasoning
        .create_conversation(&CreateConversationParams {
            user_id,
            title: Some("ordering test"),
            model_name: "test-model",
        })
        .await
        .unwrap();

    // Insert 5 events.
    for i in 0..5 {
        repos
            .reasoning
            .append_event(
                conv.id,
                "partial_answer",
                &serde_json::json!({"text": format!("answer {i}")}),
                None,
                None,
            )
            .await
            .unwrap();
    }

    let events = repos.reasoning.poll_events(conv.id, 0).await.unwrap();
    assert_eq!(events.len(), 5);
    // Verify monotonically increasing IDs.
    for i in 1..events.len() {
        assert!(events[i].id > events[i - 1].id);
    }
    // Verify order matches insertion order.
    for (i, event) in events.iter().enumerate() {
        let text = event.payload.get("text").unwrap().as_str().unwrap();
        assert_eq!(text, format!("answer {i}"));
    }

    ctx.teardown().await;
}

#[tokio::test]
async fn query_status_transitions() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let user_id = insert_user(pool).await;
    let conv = repos
        .reasoning
        .create_conversation(&CreateConversationParams {
            user_id,
            title: Some("status transitions"),
            model_name: "test-model",
        })
        .await
        .unwrap();

    // Default status is idle.
    assert_eq!(conv.query_status, "idle");

    // Transition through lifecycle.
    use ps_core::models::QueryStatus;
    for status in &[
        QueryStatus::Pending,
        QueryStatus::Running,
        QueryStatus::Completed,
    ] {
        repos
            .reasoning
            .update_query_status(conv.id, *status)
            .await
            .unwrap();
        let updated = repos
            .reasoning
            .get_conversation(conv.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.query_status, status.as_str());
    }

    ctx.teardown().await;
}

#[tokio::test]
async fn conversation_events_step_identity() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let user_id = insert_user(pool).await;
    let conv = repos
        .reasoning
        .create_conversation(&CreateConversationParams {
            user_id,
            title: Some("step identity test"),
            model_name: "test-model",
        })
        .await
        .unwrap();

    // Append events with step_id and step_seq.
    repos
        .reasoning
        .append_event(
            conv.id,
            "thinking",
            &serde_json::json!({"text": "thinking", "part_index": 0}),
            Some("think-0-0"),
            Some(0),
        )
        .await
        .unwrap();
    repos
        .reasoning
        .append_event(
            conv.id,
            "tool_call_started",
            &serde_json::json!({"tool_name": "bash", "call_id": "c1", "arguments_json": "{}"}),
            Some("tool-c1"),
            Some(1),
        )
        .await
        .unwrap();

    // Append event with NULL step_id/step_seq (backward compat).
    repos
        .reasoning
        .append_event(
            conv.id,
            "partial_answer",
            &serde_json::json!({"text": "answer"}),
            None,
            None,
        )
        .await
        .unwrap();

    let events = repos.reasoning.poll_events(conv.id, 0).await.unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].step_id.as_deref(), Some("think-0-0"));
    assert_eq!(events[0].step_seq, Some(0));
    assert_eq!(events[1].step_id.as_deref(), Some("tool-c1"));
    assert_eq!(events[1].step_seq, Some(1));
    assert_eq!(events[2].step_id, None);
    assert_eq!(events[2].step_seq, None);

    // Verify ordering is still by id.
    assert!(events[0].id < events[1].id);
    assert!(events[1].id < events[2].id);

    ctx.teardown().await;
}

#[tokio::test]
async fn query_status_cancel() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let user_id = insert_user(pool).await;
    let conv = repos
        .reasoning
        .create_conversation(&CreateConversationParams {
            user_id,
            title: Some("cancel test"),
            model_name: "test-model",
        })
        .await
        .unwrap();

    repos
        .reasoning
        .update_query_status(conv.id, ps_core::models::QueryStatus::Running)
        .await
        .unwrap();
    repos
        .reasoning
        .update_query_status(conv.id, ps_core::models::QueryStatus::Cancelled)
        .await
        .unwrap();

    let conv = repos
        .reasoning
        .get_conversation(conv.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(conv.query_status, "cancelled");

    ctx.teardown().await;
}
