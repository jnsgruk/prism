use super::*;

#[tokio::test]
async fn insight_refresh_lock_spans_pools_and_releases_waiting_connections() {
    let ctx = RepoTestContext::new().await;
    let guard = ctx.repos.metrics.lock_insight_refresh().await.unwrap();
    let waiting_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with((*ctx.pool.connect_options()).clone())
        .await
        .unwrap();
    let waiting_repos = Repos::new(waiting_pool.clone());
    let waiter = waiting_repos.metrics.lock_insight_refresh();
    tokio::pin!(waiter);
    let timeout = std::time::Duration::from_millis(150);

    assert!(tokio::time::timeout(timeout, &mut waiter).await.is_err());
    // A waiting replica must not monopolize its last pooled connection.
    let connection = tokio::time::timeout(timeout, waiting_pool.acquire())
        .await
        .unwrap()
        .unwrap();
    drop(connection);

    // Cancellation/rollback releases the database-wide budget automatically.
    guard.rollback().await.unwrap();
    let next_guard = tokio::time::timeout(timeout, &mut waiter)
        .await
        .unwrap()
        .unwrap();
    next_guard.rollback().await.unwrap();
    waiting_pool.close().await;
    ctx.teardown().await;
}

#[tokio::test]
async fn insight_refreshes_for_different_periods_share_the_memory_budget() {
    let ctx = RepoTestContext::new().await;
    {
        let guard = ctx.repos.metrics.lock_insight_refresh().await.unwrap();
        let timeout = std::time::Duration::from_millis(150);
        let week = ps_reasoning::features::insights::compute_all_snapshots(
            &ctx.repos,
            date!(2026 - 04 - 06),
            date!(2026 - 04 - 12),
            PeriodType::Week,
        );
        let quarter = ps_reasoning::features::insights::compute_all_snapshots(
            &ctx.repos,
            date!(2026 - 04 - 01),
            date!(2026 - 06 - 30),
            PeriodType::Quarter,
        );
        tokio::pin!(week, quarter);

        assert!(tokio::time::timeout(timeout, &mut week).await.is_err());
        assert!(tokio::time::timeout(timeout, &mut quarter).await.is_err());
        // Raw metrics retain their independent period lock and can still progress.
        let metrics_guard = tokio::time::timeout(
            timeout,
            ctx.repos.metrics.lock_snapshot_period(
                date!(2026 - 04 - 01),
                PeriodType::Quarter,
                false,
            ),
        )
        .await
        .unwrap()
        .unwrap();
        metrics_guard.rollback().await.unwrap();

        drop(guard);
        let (week_count, quarter_count) =
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                tokio::try_join!(&mut week, &mut quarter)
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!((week_count, quarter_count), (0, 0));
    }
    ctx.teardown().await;
}

#[tokio::test]
async fn historical_insights_wait_for_enrichment_and_bound_period_membership_and_sources() {
    let ctx = RepoTestContext::new().await;
    let person =
        create_person_with_identity(&ctx.pool, "Selected", &Platform::Github, "selected").await;
    let team = team(&ctx, "Team", None).await;
    // Membership has ended today, but was effective at all affected period ends.
    membership(
        &ctx,
        person,
        team,
        date!(2024 - 01 - 01),
        Some(date!(2026 - 01 - 01)),
    )
    .await;
    let created = datetime!(2025-05-12 12:00 UTC);
    let id = Uuid::now_v7();
    ctx.repos
        .activity
        .upsert_contribution(id, Some(person), &contribution("historic", created))
        .await
        .unwrap();
    let future_id = Uuid::now_v7();
    ctx.repos
        .activity
        .upsert_contribution(
            future_id,
            Some(person),
            &contribution("future", datetime!(2025-11-12 12:00 UTC)),
        )
        .await
        .unwrap();
    let owner = pipeline(&ctx).await;
    invalidate(&ctx, owner, id, None, person, &[created.date()]).await;
    let queue_content = serde_json::json!({"body":"historic input"});
    ctx.repos
        .reasoning
        .bulk_enqueue_enrichments(&[ps_core::repo::reasoning::EnrichmentQueueEntry {
            contribution_id: id,
            content: queue_content.clone(),
            content_hash: ps_core::repo::reasoning::content_hash(&queue_content),
        }])
        .await
        .unwrap();
    drain(&ctx.repos, Some(owner), false).await;
    assert_eq!(drain(&ctx.repos, Some(owner), true).await, 0);
    assert_eq!(
        ctx.repos
            .activity
            .count_pending_snapshot_invalidations(owner)
            .await
            .unwrap(),
        (0, 3)
    );

    for contribution_id in [id, future_id] {
        ctx.repos
            .reasoning
            .upsert_enrichment(&UpsertEnrichmentParams {
                contribution_id,
                enrichment_type: EnrichmentType::Significance,
                value: &serde_json::json!({"significance":"significant"}),
                model_name: "test-model",
                confidence: Some(0.9),
                input_hash: None,
                input_preview: Some("fixture input"),
                source_content_hash: Some(&ps_core::repo::reasoning::content_hash(&queue_content)),
            })
            .await
            .unwrap();
    }
    ctx.repos
        .reasoning
        .delete_fully_enriched_entries()
        .await
        .unwrap();
    assert_eq!(drain(&ctx.repos, Some(owner), true).await, 3);
    for kind in [PeriodType::Week, PeriodType::Month, PeriodType::Quarter] {
        let (start, _) = period_boundaries(created.date(), kind);
        let row = sqlx::query!(
            "SELECT significant_count,raw_insights,enrichment_coverage FROM reasoning.insight_snapshots WHERE team_id=$1 AND period_start=$2 AND period_type=$3",
            team,start,kind.as_str(),
        ).fetch_one(&ctx.pool).await.unwrap();
        assert_eq!(row.significant_count, 1);
        assert_eq!(
            row.raw_insights["contribution_ids"],
            serde_json::json!([id])
        );
        assert_eq!(row.enrichment_coverage["total_contributions"], 1);
        let source_count = sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "count!" FROM reasoning.insight_snapshot_sources source
            JOIN reasoning.insight_snapshots snapshot ON snapshot.id=source.snapshot_id
            WHERE snapshot.team_id=$1 AND snapshot.period_start=$2 AND snapshot.period_type=$3"#,
            team,
            start,
            kind.as_str(),
        )
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
        assert_eq!(source_count, 1);
    }
    assert_eq!(
        ctx.repos
            .activity
            .count_pending_snapshot_invalidations(owner)
            .await
            .unwrap(),
        (0, 0)
    );

    // Correcting attribution clears obsolete insight values and provenance.
    sqlx::query!(
        "UPDATE activity.contributions SET person_id=NULL WHERE id=$1",
        id
    )
    .execute(&ctx.pool)
    .await
    .unwrap();
    invalidate(&ctx, owner, id, Some(person), person, &[created.date()]).await;
    drain(&ctx.repos, Some(owner), false).await;
    drain(&ctx.repos, Some(owner), true).await;
    let row = sqlx::query!("SELECT significant_count,raw_insights FROM reasoning.insight_snapshots WHERE team_id=$1 AND period_type='month' AND period_start='2025-05-01'",team)
        .fetch_one(&ctx.pool).await.unwrap();
    assert_eq!(row.significant_count, 0);
    assert_eq!(row.raw_insights["contribution_ids"], serde_json::json!([]));
    let count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "count!" FROM reasoning.insight_snapshot_sources source
        JOIN reasoning.insight_snapshots snapshot ON snapshot.id=source.snapshot_id
        WHERE snapshot.team_id=$1 AND snapshot.period_type='month' AND snapshot.period_start='2025-05-01'"#,
        team,
    ).fetch_one(&ctx.pool).await.unwrap();
    assert_eq!(count, 0);

    ctx.teardown().await;
}
