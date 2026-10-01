use super::*;

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
