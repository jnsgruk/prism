use crate::common::db::RepoTestContext;
use ps_core::models::{Platform, TeamType};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helper: create a person directly via SQL (no repo method for raw insert)
// ---------------------------------------------------------------------------

async fn insert_person(pool: &sqlx::PgPool, name: &str, email: Option<&str>) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO org.people (id, name, email) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(name)
        .bind(email)
        .execute(pool)
        .await
        .expect("insert person");
    id
}

async fn insert_identity(pool: &sqlx::PgPool, person_id: Uuid, platform: &str, username: &str) {
    sqlx::query(
        "INSERT INTO org.platform_identities (id, person_id, platform, platform_username) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(Uuid::now_v7())
    .bind(person_id)
    .bind(platform)
    .bind(username.to_lowercase())
    .execute(pool)
    .await
    .expect("insert identity");
}

// ---------------------------------------------------------------------------
// Teams
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_team_and_get() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let team = repos
        .org
        .create_team("Kernel", "Canonical", TeamType::Team, None, None)
        .await
        .unwrap();

    assert_eq!(team.name, "Kernel");
    assert_eq!(team.org_name, "Canonical");
    assert_eq!(team.team_type, TeamType::Team);
    assert_eq!(team.member_count, 0);

    let fetched = repos.org.get_team(team.id).await.unwrap().unwrap();
    assert_eq!(fetched.name, "Kernel");

    ctx.teardown().await;
}

#[tokio::test]
async fn list_teams_with_filters() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let parent = repos
        .org
        .create_team("Engineering", "Canonical", TeamType::Group, None, None)
        .await
        .unwrap();
    repos
        .org
        .create_team("Kernel", "Canonical", TeamType::Team, Some(parent.id), None)
        .await
        .unwrap();
    repos
        .org
        .create_team(
            "Desktop",
            "Canonical",
            TeamType::Team,
            Some(parent.id),
            None,
        )
        .await
        .unwrap();

    // No filter
    let all = repos.org.list_teams(None, None).await.unwrap();
    assert_eq!(all.len(), 3);

    // Filter by parent
    let children = repos.org.list_teams(Some(parent.id), None).await.unwrap();
    assert_eq!(children.len(), 2);

    // Filter by type
    let groups = repos
        .org
        .list_teams(None, Some(TeamType::Group))
        .await
        .unwrap();
    assert_eq!(groups.len(), 1);

    ctx.teardown().await;
}

#[tokio::test]
async fn create_team_hierarchy() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let org = repos
        .org
        .create_team("Canonical", "Canonical", TeamType::Org, None, None)
        .await
        .unwrap();
    let group = repos
        .org
        .create_team(
            "Engineering",
            "Canonical",
            TeamType::Group,
            Some(org.id),
            None,
        )
        .await
        .unwrap();
    let team = repos
        .org
        .create_team("Kernel", "Canonical", TeamType::Team, Some(group.id), None)
        .await
        .unwrap();

    assert_eq!(group.parent_team_id, Some(org.id));
    assert_eq!(team.parent_team_id, Some(group.id));

    ctx.teardown().await;
}

#[tokio::test]
async fn get_all_teams_flat() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    repos
        .org
        .create_team("A", "Org", TeamType::Team, None, None)
        .await
        .unwrap();
    repos
        .org
        .create_team("B", "Org", TeamType::Team, None, None)
        .await
        .unwrap();

    let teams = repos.org.get_all_teams().await.unwrap();
    assert_eq!(teams.len(), 2);
    // Ordered by name
    assert_eq!(teams[0].name, "A");

    ctx.teardown().await;
}

#[tokio::test]
async fn list_team_ids() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let t1 = repos
        .org
        .create_team("A", "Org", TeamType::Team, None, None)
        .await
        .unwrap();
    let t2 = repos
        .org
        .create_team("B", "Org", TeamType::Team, None, None)
        .await
        .unwrap();

    let ids = repos.org.list_team_ids().await.unwrap();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&t1.id));
    assert!(ids.contains(&t2.id));

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// People & memberships
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_team_members_returns_active_only() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let team = repos
        .org
        .create_team("Team", "Org", TeamType::Team, None, None)
        .await
        .unwrap();

    let alice = insert_person(pool, "Alice", Some("alice@example.com")).await;
    let bob = insert_person(pool, "Bob", Some("bob@example.com")).await;

    repos
        .org
        .assign_person_to_team(alice.into(), team.id.into())
        .await
        .unwrap();
    repos
        .org
        .assign_person_to_team(bob.into(), team.id.into())
        .await
        .unwrap();

    let members = repos.org.get_team_members(team.id.into()).await.unwrap();
    assert_eq!(members.len(), 2);

    // Deactivate bob
    repos.org.deactivate_person(bob).await.unwrap();
    let members = repos.org.get_team_members(team.id.into()).await.unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].name, "Alice");

    ctx.teardown().await;
}

#[tokio::test]
async fn assign_person_to_team_ends_old_membership() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let team_a = repos
        .org
        .create_team("A", "Org", TeamType::Team, None, None)
        .await
        .unwrap();
    let team_b = repos
        .org
        .create_team("B", "Org", TeamType::Team, None, None)
        .await
        .unwrap();

    let alice = insert_person(pool, "Alice", None).await;

    repos
        .org
        .assign_person_to_team(alice.into(), team_a.id.into())
        .await
        .unwrap();
    let members_a = repos.org.get_team_members(team_a.id.into()).await.unwrap();
    assert_eq!(members_a.len(), 1);

    // Move to team B
    repos
        .org
        .assign_person_to_team(alice.into(), team_b.id.into())
        .await
        .unwrap();
    let members_a = repos.org.get_team_members(team_a.id.into()).await.unwrap();
    assert_eq!(members_a.len(), 0);
    let members_b = repos.org.get_team_members(team_b.id.into()).await.unwrap();
    assert_eq!(members_b.len(), 1);

    ctx.teardown().await;
}

#[tokio::test]
async fn list_unassigned_people() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let team = repos
        .org
        .create_team("Team", "Org", TeamType::Team, None, None)
        .await
        .unwrap();

    let alice = insert_person(pool, "Alice", None).await;
    let _bob = insert_person(pool, "Bob", None).await;

    repos
        .org
        .assign_person_to_team(alice.into(), team.id.into())
        .await
        .unwrap();

    let unassigned = repos.org.list_unassigned_people().await.unwrap();
    assert_eq!(unassigned.len(), 1);
    assert_eq!(unassigned[0].name, "Bob");

    ctx.teardown().await;
}

#[tokio::test]
async fn deactivate_and_reactivate_person() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let id = insert_person(pool, "Alice", None).await;

    let person = repos.org.get_person(id).await.unwrap().unwrap();
    assert!(person.active);

    repos.org.deactivate_person(id).await.unwrap();
    let person = repos.org.get_person(id).await.unwrap().unwrap();
    assert!(!person.active);

    repos.org.reactivate_person(id).await.unwrap();
    let person = repos.org.get_person(id).await.unwrap().unwrap();
    assert!(person.active);

    ctx.teardown().await;
}

#[tokio::test]
async fn update_person_fields() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let id = insert_person(pool, "Alice", None).await;

    let updated = repos
        .org
        .update_person(
            id,
            Some("Alice Updated"),
            Some("alice@new.com"),
            Some("senior"),
        )
        .await
        .unwrap();
    assert_eq!(updated.name, "Alice Updated");
    assert_eq!(updated.email.as_deref(), Some("alice@new.com"));
    assert_eq!(updated.level.as_deref(), Some("senior"));

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Identities
// ---------------------------------------------------------------------------

#[tokio::test]
async fn batch_resolve_person_ids() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let alice = insert_person(pool, "Alice", None).await;
    insert_identity(pool, alice, "github", "aliceg").await;

    let map = repos
        .org
        .batch_resolve_person_ids(&Platform::Github, &["aliceg".into()])
        .await
        .unwrap();
    assert_eq!(map.get("aliceg"), Some(&alice));

    ctx.teardown().await;
}

#[tokio::test]
async fn case_insensitive_identity_lookup() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let alice = insert_person(pool, "Alice", None).await;
    insert_identity(pool, alice, "github", "aliceg").await;

    // Lookup with mixed case — batch_resolve lowercases input
    let map = repos
        .org
        .batch_resolve_person_ids(&Platform::Github, &["AliceG".into()])
        .await
        .unwrap();
    assert_eq!(map.get("aliceg"), Some(&alice));

    ctx.teardown().await;
}

#[tokio::test]
async fn get_identities_for_people() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    let alice = insert_person(pool, "Alice", None).await;
    insert_identity(pool, alice, "github", "alice-gh").await;
    insert_identity(pool, alice, "jira", "alice-jira").await;

    let identities = repos.org.get_identities_for_people(&[alice]).await.unwrap();
    assert_eq!(identities.len(), 2);

    ctx.teardown().await;
}

#[tokio::test]
async fn batch_ensure_identities_creates_people() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    // batch_ensure_identities should auto-create people for unknown usernames
    let users: Vec<(String, Option<String>)> = vec![("newuser".into(), None)];
    let map = repos
        .org
        .batch_ensure_identities(&Platform::Github, &users)
        .await
        .unwrap();
    assert!(map.contains_key("newuser"));
    // The person was created
    let person_id = map["newuser"];
    let person = repos.org.get_person(person_id).await.unwrap();
    assert!(person.is_some());

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Directory import
// ---------------------------------------------------------------------------

#[tokio::test]
async fn import_records_creates_teams_and_people() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    use ps_core::repo::org::ImportRecord;

    let records = vec![
        ImportRecord {
            name: "Alice A".into(),
            email: Some("alice@example.com".into()),
            level: None,
            directory_id: Some("alice-1".into()),
            team: Some("Kernel".into()),
            team_type: Some(TeamType::Team),
            org: Some("Canonical".into()),
            identities: vec![],
            manager_name: None,
            depth: None,
            has_reports: false,
            group: None,
        },
        ImportRecord {
            name: "Bob B".into(),
            email: Some("bob@example.com".into()),
            level: None,
            directory_id: Some("bob-1".into()),
            team: Some("Kernel".into()),
            team_type: Some(TeamType::Team),
            org: Some("Canonical".into()),
            identities: vec![],
            manager_name: None,
            depth: None,
            has_reports: false,
            group: None,
        },
    ];

    let result = repos.org.import_records(&records).await.unwrap();
    assert_eq!(result.people_imported, 2);
    assert!(result.teams_created >= 1);

    // Both people assigned to the Kernel team
    let teams = repos.org.list_teams(None, None).await.unwrap();
    let kernel = teams.iter().find(|t| t.name == "Kernel").unwrap();
    assert_eq!(kernel.member_count, 2);

    ctx.teardown().await;
}
