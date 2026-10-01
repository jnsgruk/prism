use super::*;

#[tokio::test]
async fn committed_invalidations_survive_cancellation_and_resume_idempotently() {
    let ctx = RepoTestContext::new().await;
    let person =
        create_person_with_identity(&ctx.pool, "Selected", &Platform::Github, "selected").await;
    let team = team(&ctx, "Team", None).await;
    membership(&ctx, person, team, date!(2020 - 01 - 01), None).await;
    let created = datetime!(2025-05-12 12:00 UTC);
    let id = Uuid::now_v7();
    ctx.repos
        .activity
        .upsert_contribution(id, Some(person), &contribution("committed", created))
        .await
        .unwrap();
    let owner = pipeline(&ctx).await;
    invalidate(&ctx, owner, id, None, person, &[created.date()]).await;
    assert!(
        ctx.repos
            .activity
            .pending_snapshot_invalidations(None, false, 8)
            .await
            .unwrap()
            .is_empty()
    );
    let selected = ctx
        .repos
        .activity
        .pending_snapshot_invalidations(Some(owner), false, 8)
        .await
        .unwrap();
    // A period write can commit before a crash without its acknowledgement.
    let first = &selected[0];
    let (start, end) = period_boundaries(first.period_start, first.period_type);
    ps_metrics::compute_all_snapshots(&ctx.repos, start, end, first.period_type)
        .await
        .unwrap();
    assert_eq!(
        ctx.repos
            .activity
            .count_pending_snapshot_invalidations(owner)
            .await
            .unwrap(),
        (3, 3)
    );
    sqlx::query!("UPDATE activity.pipelines SET status='cancelled',cancellation_requested=true,completed_at=now() WHERE id=$1",owner)
        .execute(&ctx.pool).await.unwrap();
    assert_eq!(drain(&ctx.repos, None, false).await, 3);
    assert_snapshot(&ctx, team, created.date(), 1, &[id]).await;
    assert_eq!(drain(&ctx.repos, None, false).await, 0);
    assert_eq!(drain(&ctx.repos, None, true).await, 3);
    assert_eq!(
        ctx.repos
            .activity
            .count_pending_snapshot_invalidations(owner)
            .await
            .unwrap(),
        (0, 0)
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn concurrent_period_lock_waiters_leave_pool_available_to_computation() {
    let ctx = RepoTestContext::new().await;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .connect_with(ctx.pool.connect_options().as_ref().clone())
        .await
        .unwrap();
    let repos = Repos::new(pool.clone());
    let guard = repos
        .metrics
        .lock_snapshot_period(date!(2025 - 05 - 01), PeriodType::Month, false)
        .await
        .unwrap();
    let mut waiters = Vec::new();
    for _ in 0..12 {
        let repos = repos.clone();
        waiters.push(tokio::spawn(async move {
            let guard = repos
                .metrics
                .lock_snapshot_period(date!(2025 - 05 - 01), PeriodType::Month, false)
                .await
                .unwrap();
            guard.rollback().await.unwrap();
        }));
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let result =
        tokio::time::timeout(std::time::Duration::from_secs(2), repos.org.list_team_ids()).await;
    guard.rollback().await.unwrap();
    for waiter in waiters {
        tokio::time::timeout(std::time::Duration::from_secs(5), waiter)
            .await
            .unwrap()
            .unwrap();
    }
    assert!(
        result.is_ok(),
        "waiting recomputations exhausted the pool needed by the lock holder"
    );
    result.unwrap().unwrap();
    pool.close().await;
    ctx.teardown().await;
}
