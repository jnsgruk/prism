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

// ---------------------------------------------------------------------------
// Org export / import
// ---------------------------------------------------------------------------

use ps_core::repo::org::OrgExport;

fn build_test_export() -> OrgExport {
    OrgExport {
        version: 1,
        exported_at: "2026-04-14T00:00:00Z".into(),
        teams: vec![
            ps_core::repo::org::export::ExportTeam {
                name: "Engineering".into(),
                org_name: "Acme".into(),
                team_type: "group".into(),
                parent_team: None,
                lead_email: Some("alice@example.com".into()),
                github_teams: vec![],
            },
            ps_core::repo::org::export::ExportTeam {
                name: "Backend".into(),
                org_name: "Acme".into(),
                team_type: "team".into(),
                parent_team: Some("Engineering".into()),
                lead_email: None,
                github_teams: vec![],
            },
        ],
        people: vec![
            ps_core::repo::org::export::ExportPerson {
                name: "Alice Smith".into(),
                email: Some("alice@example.com".into()),
                level: Some("Staff".into()),
                active: true,
                team: Some("Engineering".into()),
                identities: vec![ps_core::repo::org::export::ExportIdentity {
                    platform: "github".into(),
                    username: "asmith".into(),
                }],
            },
            ps_core::repo::org::export::ExportPerson {
                name: "Bob Jones".into(),
                email: Some("bob@example.com".into()),
                level: None,
                active: true,
                team: Some("Backend".into()),
                identities: vec![],
            },
        ],
    }
}

#[tokio::test]
async fn export_org_empty_db() {
    let ctx = RepoTestContext::new().await;
    let export = ctx.repos.org.export_org().await.unwrap();

    assert_eq!(export.version, 1);
    assert!(export.teams.is_empty());
    assert!(export.people.is_empty());

    ctx.teardown().await;
}

#[tokio::test]
async fn export_org_includes_teams_people_identities() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    // Create team hierarchy with lead.
    let alice = insert_person(pool, "Alice", Some("alice@example.com")).await;
    insert_identity(pool, alice, "github", "asmith").await;

    let group = repos
        .org
        .create_team("Engineering", "Acme", TeamType::Group, None, Some(alice))
        .await
        .unwrap();
    let _team = repos
        .org
        .create_team("Backend", "Acme", TeamType::Team, Some(group.id), None)
        .await
        .unwrap();

    repos
        .org
        .assign_person_to_team(alice.into(), group.id.into())
        .await
        .unwrap();

    let export = repos.org.export_org().await.unwrap();

    assert_eq!(export.version, 1);
    assert_eq!(export.teams.len(), 2);
    assert_eq!(export.people.len(), 1);

    let eng = export
        .teams
        .iter()
        .find(|t| t.name == "Engineering")
        .unwrap();
    assert_eq!(eng.team_type, "group");
    assert!(eng.parent_team.is_none());
    assert_eq!(eng.lead_email.as_deref(), Some("alice@example.com"));

    let backend = export.teams.iter().find(|t| t.name == "Backend").unwrap();
    assert_eq!(backend.parent_team.as_deref(), Some("Engineering"));

    let alice_exp = &export.people[0];
    assert_eq!(alice_exp.name, "Alice");
    assert_eq!(alice_exp.team.as_deref(), Some("Engineering"));
    assert_eq!(alice_exp.identities.len(), 1);
    assert_eq!(alice_exp.identities[0].platform, "github");
    assert_eq!(alice_exp.identities[0].username, "asmith");

    ctx.teardown().await;
}

#[tokio::test]
async fn import_org_merge_creates_on_empty_db() {
    let ctx = RepoTestContext::new().await;
    let export = build_test_export();

    let result = ctx.repos.org.import_org(&export, false).await.unwrap();

    assert_eq!(result.teams_created, 2);
    assert_eq!(result.teams_updated, 0);
    assert_eq!(result.people_created, 2);
    assert_eq!(result.people_updated, 0);
    assert_eq!(result.identities_created, 1);

    // Verify teams exist with correct hierarchy.
    let teams = ctx.repos.org.list_teams(None, None).await.unwrap();
    assert_eq!(teams.len(), 2);
    let eng = teams.iter().find(|t| t.name == "Engineering").unwrap();
    assert_eq!(eng.team_type, TeamType::Group);

    let backend = teams.iter().find(|t| t.name == "Backend").unwrap();
    assert_eq!(backend.parent_team_id, Some(eng.id));

    // Verify lead was wired.
    assert!(eng.lead_id.is_some());

    // Verify memberships.
    let eng_members = ctx.repos.org.get_team_members(eng.id.into()).await.unwrap();
    assert_eq!(eng_members.len(), 1);
    assert_eq!(eng_members[0].name, "Alice Smith");

    let backend_members = ctx
        .repos
        .org
        .get_team_members(backend.id.into())
        .await
        .unwrap();
    assert_eq!(backend_members.len(), 1);
    assert_eq!(backend_members[0].name, "Bob Jones");

    ctx.teardown().await;
}

#[tokio::test]
async fn import_org_merge_skips_existing() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    // Pre-populate with a matching team and person.
    repos
        .org
        .create_team("Engineering", "Acme", TeamType::Group, None, None)
        .await
        .unwrap();
    let _alice = insert_person(pool, "Alice Smith", Some("alice@example.com")).await;

    let export = build_test_export();
    let result = repos.org.import_org(&export, false).await.unwrap();

    // Engineering matched, Backend created.
    assert_eq!(result.teams_updated, 1);
    assert_eq!(result.teams_created, 1);
    // Alice matched, Bob created.
    assert_eq!(result.people_updated, 1);
    assert_eq!(result.people_created, 1);

    // Verify no duplicate teams.
    let teams = repos.org.list_teams(None, None).await.unwrap();
    assert_eq!(teams.len(), 2);

    ctx.teardown().await;
}

#[tokio::test]
async fn import_org_merge_adds_missing_identities() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    // Pre-populate person without identity.
    let _alice = insert_person(pool, "Alice Smith", Some("alice@example.com")).await;

    let export = build_test_export();
    let result = repos.org.import_org(&export, false).await.unwrap();

    // Alice's github identity should be created.
    assert_eq!(result.identities_created, 1);

    ctx.teardown().await;
}

#[tokio::test]
async fn import_org_merge_preserves_existing_memberships() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    // Create Team A and assign Alice.
    let team_a = repos
        .org
        .create_team("TeamA", "Acme", TeamType::Team, None, None)
        .await
        .unwrap();
    let alice = insert_person(pool, "Alice Smith", Some("alice@example.com")).await;
    repos
        .org
        .assign_person_to_team(alice.into(), team_a.id.into())
        .await
        .unwrap();

    // Import says Alice belongs to Engineering — but merge mode should preserve TeamA.
    let export = build_test_export();
    repos.org.import_org(&export, false).await.unwrap();

    // Alice should still be on TeamA since she already had an active membership.
    let members = repos.org.get_team_members(team_a.id.into()).await.unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].name, "Alice Smith");

    ctx.teardown().await;
}

#[tokio::test]
async fn import_org_warns_on_missing_github_teams() {
    let ctx = RepoTestContext::new().await;

    let mut export = build_test_export();
    export.teams[0]
        .github_teams
        .push(ps_core::repo::org::export::ExportGitHubTeamRef {
            github_org: "acme".into(),
            slug: "nonexistent-team".into(),
        });

    let result = ctx.repos.org.import_org(&export, false).await.unwrap();

    assert_eq!(result.github_mappings_skipped, 1);
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.contains("nonexistent-team"))
    );

    ctx.teardown().await;
}

#[tokio::test]
async fn import_org_replace_wipes_and_recreates() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    // Pre-populate with different data.
    repos
        .org
        .create_team("OldTeam", "OldOrg", TeamType::Team, None, None)
        .await
        .unwrap();
    let _charlie = insert_person(pool, "Charlie", Some("charlie@example.com")).await;

    let export = build_test_export();
    let result = repos.org.import_org(&export, true).await.unwrap();

    assert_eq!(result.teams_created, 2);
    assert_eq!(result.people_created, 2);

    // Old data should be gone.
    let teams = repos.org.list_teams(None, None).await.unwrap();
    assert!(!teams.iter().any(|t| t.name == "OldTeam"));
    assert_eq!(teams.len(), 2);

    ctx.teardown().await;
}

#[tokio::test]
async fn export_then_import_round_trip() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let pool = &ctx.pool;

    // Build an org.
    let alice = insert_person(pool, "Alice", Some("alice@example.com")).await;
    insert_identity(pool, alice, "github", "asmith").await;

    let group = repos
        .org
        .create_team("Engineering", "Acme", TeamType::Group, None, Some(alice))
        .await
        .unwrap();
    let team = repos
        .org
        .create_team("Backend", "Acme", TeamType::Team, Some(group.id), None)
        .await
        .unwrap();
    repos
        .org
        .assign_person_to_team(alice.into(), team.id.into())
        .await
        .unwrap();

    // Export.
    let export = repos.org.export_org().await.unwrap();
    assert_eq!(export.teams.len(), 2);
    assert_eq!(export.people.len(), 1);

    // Wipe and re-import.
    repos.org.reset_all().await.unwrap();
    let result = repos.org.import_org(&export, false).await.unwrap();

    assert_eq!(result.teams_created, 2);
    assert_eq!(result.people_created, 1);
    assert_eq!(result.identities_created, 1);

    // Verify hierarchy restored.
    let teams = repos.org.list_teams(None, None).await.unwrap();
    let eng = teams.iter().find(|t| t.name == "Engineering").unwrap();
    let backend = teams.iter().find(|t| t.name == "Backend").unwrap();
    assert_eq!(backend.parent_team_id, Some(eng.id));
    assert!(eng.lead_id.is_some());

    ctx.teardown().await;
}
