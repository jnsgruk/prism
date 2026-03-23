use crate::define_repo_test;
use ps_core::repo::reasoning::{EnrichmentResult, UpsertEnrichmentParams};
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

define_repo_test!(
    log_api_usage_and_get_daily_spend,
    |repos, _pool| async move {
        repos
            .reasoning
            .log_api_usage("google", "gemini-pro", "enrichment", 1000, 500, 0.05)
            .await
            .unwrap();
        repos
            .reasoning
            .log_api_usage("google", "gemini-pro", "enrichment", 2000, 1000, 0.10)
            .await
            .unwrap();

        let today = OffsetDateTime::now_utc().date();
        let spend = repos.reasoning.get_daily_spend(today).await.unwrap();
        assert!((spend - 0.15).abs() < 0.001);
    }
);

define_repo_test!(get_daily_spend_by_task, |repos, _pool| async move {
    repos
        .reasoning
        .log_api_usage("google", "gemini-pro", "enrichment", 1000, 500, 0.05)
        .await
        .unwrap();
    repos
        .reasoning
        .log_api_usage("google", "gemini-pro", "insights", 500, 200, 0.02)
        .await
        .unwrap();

    let today = OffsetDateTime::now_utc().date();
    let breakdown = repos
        .reasoning
        .get_daily_spend_by_task(today)
        .await
        .unwrap();
    assert_eq!(breakdown.len(), 2);
    // Ordered by cost DESC
    assert_eq!(breakdown[0].task_type, "enrichment");
    assert_eq!(breakdown[0].request_count, 1);
});

define_repo_test!(get_spend_summary, |repos, _pool| async move {
    repos
        .reasoning
        .log_api_usage("google", "gemini-pro", "enrichment", 100, 50, 0.01)
        .await
        .unwrap();
    repos
        .reasoning
        .log_api_usage("openrouter", "llama-3", "insights", 200, 100, 0.03)
        .await
        .unwrap();

    let since = OffsetDateTime::now_utc() - time::Duration::hours(1);
    let until = OffsetDateTime::now_utc() + time::Duration::hours(1);

    let summary = repos
        .reasoning
        .get_spend_summary(since, until)
        .await
        .unwrap();
    assert_eq!(summary.len(), 2);
});

// ---------------------------------------------------------------------------
// Enrichments
// ---------------------------------------------------------------------------

define_repo_test!(upsert_enrichment_and_retrieve, |repos, pool| async move {
    let contrib_id = insert_contribution(&pool, "enrich-1").await;

    let params = UpsertEnrichmentParams {
        contribution_id: contrib_id,
        enrichment_type: "review_depth",
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
    assert_eq!(enrichments[0].enrichment_type, "review_depth");
    assert!((enrichments[0].confidence.unwrap() - 0.9).abs() < 0.01);
});

define_repo_test!(
    upsert_enrichment_replaces_on_conflict,
    |repos, pool| async move {
        let contrib_id = insert_contribution(&pool, "enrich-replace").await;

        let params1 = UpsertEnrichmentParams {
            contribution_id: contrib_id,
            enrichment_type: "sentiment",
            value: &serde_json::json!({"sentiment": "positive"}),
            model_name: "gemini-pro",
            confidence: Some(0.7),
            input_hash: None,
            input_preview: None,
        };
        repos.reasoning.upsert_enrichment(&params1).await.unwrap();

        let params2 = UpsertEnrichmentParams {
            contribution_id: contrib_id,
            enrichment_type: "sentiment",
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
    }
);

define_repo_test!(bulk_upsert_enrichments, |repos, pool| async move {
    let c1 = insert_contribution(&pool, "bulk-e-1").await;
    let c2 = insert_contribution(&pool, "bulk-e-2").await;

    let results = vec![
        EnrichmentResult {
            contribution_id: c1,
            enrichment_type: "review_depth".into(),
            value: serde_json::json!({"depth": "surface"}),
            confidence: 0.6,
            input_hash: "h1".into(),
            input_preview: "preview 1".into(),
        },
        EnrichmentResult {
            contribution_id: c2,
            enrichment_type: "review_depth".into(),
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
});

define_repo_test!(
    bulk_upsert_enrichments_empty_is_noop,
    |repos, _pool| async move {
        let count = repos
            .reasoning
            .bulk_upsert_enrichments(&[], "model")
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
);

define_repo_test!(
    get_enrichments_for_contributions_batch,
    |repos, pool| async move {
        let c1 = insert_contribution(&pool, "batch-e-1").await;
        let c2 = insert_contribution(&pool, "batch-e-2").await;

        let params = UpsertEnrichmentParams {
            contribution_id: c1,
            enrichment_type: "review_depth",
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
    }
);

// ---------------------------------------------------------------------------
// Enrichment status & unenriched
// ---------------------------------------------------------------------------

define_repo_test!(get_enrichment_status_empty, |repos, _pool| async move {
    let status = repos.reasoning.get_enrichment_status().await.unwrap();
    assert_eq!(status.total_enrichments, 0);
    assert!(status.last_enrichment_at.is_none());
});

define_repo_test!(find_unenriched_contributions, |repos, pool| async move {
    // Create a PR review with content (eligible for review_depth)
    let c1 =
        insert_typed_contribution(&pool, "unenriched-1", "pr_review", Some("LGTM"), None, None)
            .await;
    // Create another with no content (not eligible)
    let _c2 = insert_typed_contribution(&pool, "unenriched-2", "pr_review", None, None, None).await;

    let results = repos
        .reasoning
        .find_unenriched_contributions("review_depth", 10)
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, c1);
});

define_repo_test!(find_unenriched_prs_by_size, |repos, pool| async move {
    // PR with >50 lines (eligible for significance)
    let _c1 =
        insert_typed_contribution(&pool, "big-pr", "pull_request", None, Some(40), Some(20)).await;
    // PR with <=50 lines (not eligible)
    let _c2 =
        insert_typed_contribution(&pool, "small-pr", "pull_request", None, Some(10), Some(5)).await;

    let results = repos
        .reasoning
        .find_unenriched_contributions("significance", 10)
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
});

define_repo_test!(
    find_unenriched_unknown_type_returns_empty,
    |repos, _pool| async move {
        let results = repos
            .reasoning
            .find_unenriched_contributions("nonexistent", 10)
            .await
            .unwrap();
        assert!(results.is_empty());
    }
);

define_repo_test!(delete_enrichments_by_type, |repos, pool| async move {
    let c1 = insert_contribution(&pool, "del-type-1").await;

    let params = UpsertEnrichmentParams {
        contribution_id: c1,
        enrichment_type: "review_depth",
        value: &serde_json::json!({}),
        model_name: "model",
        confidence: None,
        input_hash: None,
        input_preview: None,
    };
    repos.reasoning.upsert_enrichment(&params).await.unwrap();

    let params2 = UpsertEnrichmentParams {
        contribution_id: c1,
        enrichment_type: "sentiment",
        value: &serde_json::json!({}),
        model_name: "model",
        confidence: None,
        input_hash: None,
        input_preview: None,
    };
    repos.reasoning.upsert_enrichment(&params2).await.unwrap();

    let deleted = repos
        .reasoning
        .delete_enrichments_by_type("review_depth")
        .await
        .unwrap();
    assert_eq!(deleted, 1);

    let remaining = repos
        .reasoning
        .get_enrichments_for_contribution(c1)
        .await
        .unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].enrichment_type, "sentiment");
});
