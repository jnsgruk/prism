use crate::common::db::RepoTestContext;
use ps_core::{
    Error,
    models::{ContributionType, PeriodType, Platform, TeamType},
};
use uuid::Uuid;

#[tokio::test]
async fn deletion_removes_person_data_and_preserves_unattributed_source_activity() {
    let ctx = RepoTestContext::new().await;
    let person = crate::common::fixtures::create_person_with_identity(
        &ctx.pool,
        "Alice",
        &Platform::Github,
        "alice",
    )
    .await;
    let other = crate::common::fixtures::create_person_with_identity(
        &ctx.pool,
        "Bob",
        &Platform::Github,
        "bob",
    )
    .await;
    let team = ctx
        .repos
        .org
        .create_team("Team", "Org", TeamType::Team, None, Some(person))
        .await
        .unwrap();
    ctx.repos
        .org
        .assign_person_to_team(person.into(), team.id.into())
        .await
        .unwrap();
    let contribution = Uuid::now_v7();
    let platform = Platform::Github;
    let kind = ContributionType::PullRequest;
    sqlx::query!(
        "INSERT INTO activity.contributions (id, person_id, platform, contribution_type, platform_id, title, created_at) VALUES ($1, $2, $3, $4, 'pr-1', 'Original title', now())",
        contribution,
        person,
        platform as _,
        kind as _,
    )
    .execute(&ctx.pool)
    .await
    .unwrap();
    let profile = Uuid::now_v7();
    let period = PeriodType::Month;
    sqlx::query!(
        "INSERT INTO metrics.individual_profiles (id, person_id, period_start, period_end, period_type) VALUES ($1, $2, '2026-01-01', '2026-01-31', $3)",
        profile,
        person,
        period as _,
    )
    .execute(&ctx.pool)
    .await
    .unwrap();
    let (user, _) = crate::common::fixtures::create_admin_user(&ctx.pool).await;
    sqlx::query!(
        "UPDATE auth.users SET person_id = $1 WHERE id = $2",
        person,
        user
    )
    .execute(&ctx.pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO org.identity_resolutions (person_id, platform) VALUES ($1, $2)",
        person,
        platform as _
    )
    .execute(&ctx.pool)
    .await
    .unwrap();

    assert!(matches!(
        ctx.repos.org.delete_person(person).await,
        Err(Error::Conflict(_))
    ));
    assert!(ctx.repos.org.get_person(person).await.unwrap().is_some());
    assert_eq!(
        ctx.repos
            .org
            .get_identities_for_people(&[person])
            .await
            .unwrap()
            .len(),
        1
    );
    ctx.repos.org.deactivate_person(person).await.unwrap();
    ctx.repos.org.delete_person(person).await.unwrap();

    assert!(ctx.repos.org.get_person(person).await.unwrap().is_none());
    assert!(ctx.repos.org.get_person(other).await.unwrap().is_some());
    assert!(
        ctx.repos
            .org
            .get_identities_for_people(&[person])
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        ctx.repos
            .org
            .get_identities_for_people(&[other])
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        ctx.repos
            .org
            .get_team(team.id)
            .await
            .unwrap()
            .unwrap()
            .lead_id
            .is_none()
    );
    let saved = sqlx::query!(
        "SELECT person_id, title FROM activity.contributions WHERE id = $1",
        contribution
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert!(saved.person_id.is_none());
    assert_eq!(saved.title.as_deref(), Some("Original title"));
    assert!(
        sqlx::query_scalar!("SELECT person_id FROM auth.users WHERE id = $1", user)
            .fetch_one(&ctx.pool)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        sqlx::query_scalar!(
            "SELECT COUNT(*) FROM org.team_memberships WHERE person_id = $1",
            person
        )
        .fetch_one(&ctx.pool)
        .await
        .unwrap(),
        Some(0)
    );
    assert_eq!(
        sqlx::query_scalar!(
            "SELECT COUNT(*) FROM org.identity_resolutions WHERE person_id = $1",
            person
        )
        .fetch_one(&ctx.pool)
        .await
        .unwrap(),
        Some(0)
    );
    assert_eq!(
        sqlx::query_scalar!(
            "SELECT COUNT(*) FROM metrics.individual_profiles WHERE person_id = $1",
            person
        )
        .fetch_one(&ctx.pool)
        .await
        .unwrap(),
        Some(0)
    );
    ctx.teardown().await;
}
