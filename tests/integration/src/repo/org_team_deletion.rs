use crate::common::db::RepoTestContext;
use ps_core::Error;
use ps_core::models::TeamType;
use uuid::Uuid;

#[tokio::test]
async fn delete_team_removes_ended_history_and_snapshots_but_preserves_people_and_repositories() {
    let ctx = RepoTestContext::new().await;
    let team = ctx
        .repos
        .org
        .create_team("Old", "Org", TeamType::Team, None, None)
        .await
        .unwrap();
    let other = ctx
        .repos
        .org
        .create_team("Current", "Org", TeamType::Team, None, None)
        .await
        .unwrap();
    let person_id = Uuid::now_v7();
    sqlx::query!(
        "INSERT INTO org.people (id, name) VALUES ($1, 'Alice')",
        person_id
    )
    .execute(&ctx.pool)
    .await
    .unwrap();

    // Reassignment ends today's old membership and creates an active one elsewhere.
    ctx.repos
        .org
        .assign_person_to_team(person_id.into(), team.id.into())
        .await
        .unwrap();
    ctx.repos
        .org
        .assign_person_to_team(person_id.into(), other.id.into())
        .await
        .unwrap();

    let repository_id = Uuid::now_v7();
    sqlx::query!(
        "INSERT INTO org.repositories (id, github_org, github_repo, team_id) VALUES ($1, 'org', 'repo', $2)",
        repository_id,
        team.id,
    )
    .execute(&ctx.pool)
    .await
    .unwrap();
    let snapshot_id = Uuid::now_v7();
    sqlx::query!(
        r#"
        INSERT INTO metrics.team_snapshots (id, team_id, period_start, period_end, period_type)
        VALUES ($1, $2, CURRENT_DATE, CURRENT_DATE, 'week')
        "#,
        snapshot_id,
        team.id,
    )
    .execute(&ctx.pool)
    .await
    .unwrap();
    sqlx::query!(
        r#"
        INSERT INTO reasoning.insight_snapshots (team_id, period_start, period_end, period_type)
        VALUES ($1, CURRENT_DATE, CURRENT_DATE, 'week')
        "#,
        team.id,
    )
    .execute(&ctx.pool)
    .await
    .unwrap();

    ctx.repos.org.delete_team(team.id).await.unwrap();

    assert!(ctx.repos.org.get_team(team.id).await.unwrap().is_none());
    assert!(ctx.repos.org.get_person(person_id).await.unwrap().is_some());
    let members = ctx
        .repos
        .org
        .get_team_members(other.id.into())
        .await
        .unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].id, person_id);
    let remaining = sqlx::query!(
        r#"
        SELECT
            (SELECT COUNT(*) FROM org.team_memberships WHERE team_id = $1) AS "memberships!",
            (SELECT COUNT(*) FROM metrics.team_snapshots WHERE team_id = $1) AS "metrics!",
            (SELECT COUNT(*) FROM reasoning.insight_snapshots WHERE team_id = $1) AS "insights!"
        "#,
        team.id,
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(remaining.memberships, 0);
    assert_eq!(remaining.metrics, 0);
    assert_eq!(remaining.insights, 0);
    let repository = sqlx::query!(
        "SELECT team_id FROM org.repositories WHERE id = $1",
        repository_id,
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert!(repository.team_id.is_none());

    ctx.teardown().await;
}

#[tokio::test]
async fn delete_team_rejects_open_and_future_ended_memberships_without_removing_history() {
    let ctx = RepoTestContext::new().await;
    let team = ctx
        .repos
        .org
        .create_team("Team", "Org", TeamType::Team, None, None)
        .await
        .unwrap();
    let person_id = Uuid::now_v7();
    // Inactive people still have membership history that must be protected.
    sqlx::query!(
        "INSERT INTO org.people (id, name, active) VALUES ($1, 'Alice', false)",
        person_id,
    )
    .execute(&ctx.pool)
    .await
    .unwrap();
    let membership_id = Uuid::now_v7();
    let historical_id = Uuid::now_v7();
    sqlx::query!(
        r#"
        INSERT INTO org.team_memberships (id, person_id, team_id, start_date, end_date)
        VALUES ($1, $3, $4, CURRENT_DATE, NULL),
               ($2, $3, $4, CURRENT_DATE - 10, CURRENT_DATE - 1)
        "#,
        membership_id,
        historical_id,
        person_id,
        team.id,
    )
    .execute(&ctx.pool)
    .await
    .unwrap();

    for future_end in [false, true] {
        if future_end {
            sqlx::query!(
                "UPDATE org.team_memberships SET end_date = CURRENT_DATE + 1 WHERE id = $1",
                membership_id,
            )
            .execute(&ctx.pool)
            .await
            .unwrap();
        }

        let error = ctx.repos.org.delete_team(team.id).await.unwrap_err();
        assert!(matches!(error, Error::Validation(message) if message.contains("active members")));
        assert!(ctx.repos.org.get_team(team.id).await.unwrap().is_some());
        let count = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM org.team_memberships WHERE team_id = $1",
            team.id,
        )
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
        assert_eq!(count, Some(2));
    }

    ctx.teardown().await;
}

#[tokio::test]
async fn delete_team_rejects_children() {
    let ctx = RepoTestContext::new().await;
    let parent = ctx
        .repos
        .org
        .create_team("Parent", "Org", TeamType::Group, None, None)
        .await
        .unwrap();
    let child = ctx
        .repos
        .org
        .create_team("Child", "Org", TeamType::Team, Some(parent.id), None)
        .await
        .unwrap();

    let error = ctx.repos.org.delete_team(parent.id).await.unwrap_err();
    assert!(matches!(error, Error::Validation(message) if message.contains("child teams")));
    assert!(ctx.repos.org.get_team(parent.id).await.unwrap().is_some());
    assert!(ctx.repos.org.get_team(child.id).await.unwrap().is_some());

    // Empty leaf teams remain removable.
    ctx.repos.org.delete_team(child.id).await.unwrap();
    ctx.repos.org.delete_team(parent.id).await.unwrap();
    assert!(ctx.repos.org.get_team(parent.id).await.unwrap().is_none());
    assert!(ctx.repos.org.get_team(child.id).await.unwrap().is_none());

    ctx.teardown().await;
}
