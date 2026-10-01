use super::*;

#[tokio::test]
async fn historical_date_correction_clears_old_year_period_sources_and_populates_new() {
    let ctx = RepoTestContext::new().await;
    let person =
        create_person_with_identity(&ctx.pool, "Selected", &Platform::Github, "selected").await;
    let team = team(&ctx, "Team", None).await;
    membership(&ctx, person, team, date!(2020 - 01 - 01), None).await;
    let contribution_id = Uuid::now_v7();
    let old = datetime!(2025-12-28 12:00 UTC);
    let new = datetime!(2026-01-08 12:00 UTC);
    ctx.repos
        .activity
        .upsert_contribution(
            contribution_id,
            Some(person),
            &contribution("corrected", old),
        )
        .await
        .unwrap();
    let owner = pipeline(&ctx).await;
    invalidate(&ctx, owner, contribution_id, None, person, &[old.date()]).await;
    assert_eq!(drain(&ctx.repos, Some(owner), false).await, 3);
    assert_snapshot(&ctx, team, old.date(), 1, &[contribution_id]).await;

    sqlx::query!(
        "UPDATE activity.contributions SET created_at=$2,closed_at=$2 WHERE id=$1",
        contribution_id,
        new
    )
    .execute(&ctx.pool)
    .await
    .unwrap();
    invalidate(
        &ctx,
        owner,
        contribution_id,
        Some(person),
        person,
        &[old.date(), new.date()],
    )
    .await;
    assert_eq!(drain(&ctx.repos, Some(owner), false).await, 6);
    assert_snapshot(&ctx, team, old.date(), 0, &[]).await;
    assert_snapshot(&ctx, team, new.date(), 1, &[contribution_id]).await;
    assert_eq!(drain(&ctx.repos, Some(owner), false).await, 0);
    ctx.teardown().await;
}

#[tokio::test]
async fn historical_attribution_and_state_changes_refresh_both_teams_and_ancestors() {
    let ctx = RepoTestContext::new().await;
    let first = create_person_with_identity(&ctx.pool, "First", &Platform::Github, "first").await;
    let second =
        create_person_with_identity(&ctx.pool, "Second", &Platform::Github, "second").await;
    let parent = team(&ctx, "Parent", None).await;
    let first_team = team(&ctx, "First", Some(parent)).await;
    let second_team = team(&ctx, "Second", Some(parent)).await;
    membership(&ctx, first, first_team, date!(2020 - 01 - 01), None).await;
    membership(&ctx, second, second_team, date!(2020 - 01 - 01), None).await;
    let created = datetime!(2025-05-12 12:00 UTC);
    let id = Uuid::now_v7();
    ctx.repos
        .activity
        .upsert_contribution(id, Some(first), &contribution("attribution", created))
        .await
        .unwrap();
    let owner = pipeline(&ctx).await;
    invalidate(&ctx, owner, id, None, first, &[created.date()]).await;
    drain(&ctx.repos, Some(owner), false).await;
    sqlx::query!(
        "UPDATE activity.contributions SET person_id=$2 WHERE id=$1",
        id,
        second
    )
    .execute(&ctx.pool)
    .await
    .unwrap();
    invalidate(&ctx, owner, id, Some(first), second, &[created.date()]).await;
    drain(&ctx.repos, Some(owner), false).await;
    assert_snapshot(&ctx, first_team, created.date(), 0, &[]).await;
    assert_snapshot(&ctx, second_team, created.date(), 1, &[id]).await;
    assert_snapshot(&ctx, parent, created.date(), 1, &[id]).await;

    let state = ContributionState::Open.as_str();
    sqlx::query!(
        "UPDATE activity.contributions SET state=$2,metrics='{}' WHERE id=$1",
        id,
        state
    )
    .execute(&ctx.pool)
    .await
    .unwrap();
    invalidate(&ctx, owner, id, Some(second), second, &[created.date()]).await;
    drain(&ctx.repos, Some(owner), false).await;
    assert_snapshot(&ctx, second_team, created.date(), 0, &[id]).await;
    assert_snapshot(&ctx, parent, created.date(), 0, &[id]).await;
    ctx.teardown().await;
}

#[tokio::test]
async fn historical_membership_uses_period_end_and_unassigned_rows_stay_individually_visible() {
    let ctx = RepoTestContext::new().await;
    let person =
        create_person_with_identity(&ctx.pool, "Unassigned", &Platform::Github, "selected").await;
    let late_team = team(&ctx, "Late", None).await;
    let eligible_team = team(&ctx, "Eligible", None).await;
    let created = datetime!(2025-02-12 12:00 UTC);
    let id = Uuid::now_v7();
    ctx.repos
        .activity
        .upsert_contribution(id, Some(person), &contribution("unassigned", created))
        .await
        .unwrap();
    let owner = pipeline(&ctx).await;
    invalidate(&ctx, owner, id, None, person, &[created.date()]).await;
    drain(&ctx.repos, Some(owner), false).await;
    assert_snapshot(&ctx, late_team, created.date(), 0, &[]).await;
    let activity = ctx
        .repos
        .metrics
        .get_person_activity_summary(person, date!(2025 - 02 - 01), date!(2025 - 02 - 28))
        .await
        .unwrap();
    assert_eq!(activity.len(), 1);
    assert_eq!(activity[0].contribution_count, 1);

    assert_eq!(
        ctx.repos
            .metrics
            .get_contribution_by_id(id)
            .await
            .unwrap()
            .unwrap()
            .person_id,
        Some(person)
    );

    membership(&ctx, person, late_team, date!(2025 - 04 - 01), None).await;
    membership(
        &ctx,
        person,
        eligible_team,
        date!(2025 - 02 - 16),
        Some(date!(2025 - 04 - 01)),
    )
    .await;
    invalidate(&ctx, owner, id, Some(person), person, &[created.date()]).await;
    drain(&ctx.repos, Some(owner), false).await;
    assert_snapshot(&ctx, late_team, created.date(), 0, &[]).await;
    assert_snapshot(&ctx, eligible_team, created.date(), 1, &[id]).await;
    let dates = sqlx::query_scalar!(
        "SELECT start_date FROM org.team_memberships WHERE person_id=$1 ORDER BY start_date",
        person
    )
    .fetch_all(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(dates, vec![date!(2025 - 02 - 16), date!(2025 - 04 - 01)]);
    ctx.teardown().await;
}
