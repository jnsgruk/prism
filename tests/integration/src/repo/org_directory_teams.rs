use crate::common::db::RepoTestContext;
use ps_core::models::TeamType;
use ps_core::repo::org::ImportRecord;
use uuid::Uuid;

const MANAGER: &str = "Manager";

fn record(name: &str, leader: bool, team_type: TeamType) -> ImportRecord {
    ImportRecord {
        name: name.into(),
        email: Some(format!("{}@example.com", name.to_lowercase())),
        level: None,
        directory_id: Some(name.into()),
        team: Some(match team_type {
            TeamType::Squad => "Manager's Squad".into(),
            _ => "Manager's Team".into(),
        }),
        team_type: Some(team_type),
        org: Some("Canonical".into()),
        identities: vec![],
        manager_name: (!leader).then(|| MANAGER.into()),
        depth: Some(if leader { 2 } else { 3 }),
        has_reports: leader,
        group: None,
    }
}

#[tokio::test]
async fn directory_adds_new_reports_to_existing_lead_team_before_manager_record() {
    let ctx = RepoTestContext::new().await;
    let first = ctx
        .repos
        .org
        .import_records(&[record(MANAGER, true, TeamType::Team)], false)
        .await
        .unwrap();
    assert_eq!(first.teams_created, 1);
    let original = ctx.repos.org.get_all_teams().await.unwrap().remove(0);
    ctx.repos
        .org
        .update_team(original.id, Some("Real team"), None, None)
        .await
        .unwrap();

    // A stale generated name exists, but the existing lead mapping wins.
    let redundant = ctx
        .repos
        .org
        .create_team("Manager's Team", "Canonical", TeamType::Team, None, None)
        .await
        .unwrap();
    let records = [
        record("New report", false, TeamType::Team),
        record(MANAGER, true, TeamType::Team),
    ];
    for _ in 0..2 {
        let result = ctx.repos.org.import_records(&records, false).await.unwrap();
        assert_eq!(result.teams_created, 0);
        assert!(result.warnings.is_empty());
    }
    let teams = ctx.repos.org.get_all_teams().await.unwrap();
    assert_eq!(teams.len(), 2);
    let members = ctx
        .repos
        .org
        .get_team_members(original.id.into())
        .await
        .unwrap();
    assert_eq!(members.len(), 2);
    assert!(members.iter().any(|p| p.name == "New report"));
    assert!(
        ctx.repos
            .org
            .get_team_members(redundant.id.into())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        ctx.repos
            .org
            .get_team(original.id)
            .await
            .unwrap()
            .unwrap()
            .name,
        "Real team"
    );

    ctx.teardown().await;
}

#[tokio::test]
async fn directory_squad_alias_reuses_team_with_same_lead_and_preserves_manual_membership() {
    let ctx = RepoTestContext::new().await;
    ctx.repos
        .org
        .import_records(&[record(MANAGER, true, TeamType::Team)], false)
        .await
        .unwrap();
    let original = ctx.repos.org.get_all_teams().await.unwrap().remove(0);
    ctx.repos
        .org
        .update_team(original.id, Some("Existing team"), None, None)
        .await
        .unwrap();
    let other = ctx
        .repos
        .org
        .create_team("Other", "Canonical", TeamType::Team, None, None)
        .await
        .unwrap();
    // Deliberately put the manager in a different team from the one they lead.
    ctx.repos
        .org
        .assign_person_to_team(original.lead_id.unwrap().into(), other.id.into())
        .await
        .unwrap();

    let result = ctx
        .repos
        .org
        .import_records(
            &[
                record("New report", false, TeamType::Squad),
                record(MANAGER, true, TeamType::Squad),
            ],
            false,
        )
        .await
        .unwrap();
    assert_eq!(result.teams_created, 0);
    let members = ctx
        .repos
        .org
        .get_team_members(original.id.into())
        .await
        .unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].name, "New report");
    let manager = ctx
        .repos
        .org
        .get_person(original.lead_id.unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(manager.team_id, Some(other.id));

    ctx.teardown().await;
}

#[tokio::test]
async fn directory_first_import_creates_one_team_and_lead_regardless_of_record_order() {
    let ctx = RepoTestContext::new().await;
    let records = [
        record("Report", false, TeamType::Team),
        record(MANAGER, true, TeamType::Team),
    ];
    let result = ctx.repos.org.import_records(&records, false).await.unwrap();
    assert_eq!(result.teams_created, 1);
    let team = ctx.repos.org.get_all_teams().await.unwrap().remove(0);
    assert!(team.lead_id.is_some());
    assert_eq!(
        ctx.repos
            .org
            .get_team_members(team.id.into())
            .await
            .unwrap()
            .len(),
        2
    );
    let result = ctx.repos.org.import_records(&records, false).await.unwrap();
    assert_eq!(result.teams_created, 0);

    ctx.teardown().await;
}

#[tokio::test]
async fn directory_warns_on_ambiguous_lead_teams_without_creating_another() {
    let ctx = RepoTestContext::new().await;
    ctx.repos
        .org
        .import_records(&[record(MANAGER, true, TeamType::Team)], false)
        .await
        .unwrap();
    let original = ctx.repos.org.get_all_teams().await.unwrap().remove(0);
    ctx.repos
        .org
        .update_team(original.id, Some("One"), None, None)
        .await
        .unwrap();
    ctx.repos
        .org
        .create_team("Two", "Canonical", TeamType::Team, None, original.lead_id)
        .await
        .unwrap();

    let result = ctx
        .repos
        .org
        .import_records(
            &[
                record("Report", false, TeamType::Team),
                record(MANAGER, true, TeamType::Team),
            ],
            false,
        )
        .await
        .unwrap();
    assert_eq!(result.teams_created, 0);
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("Multiple teams"));
    let report = sqlx::query!("SELECT id FROM org.people WHERE directory_id = 'Report'")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert!(
        ctx.repos
            .org
            .get_person(report.id)
            .await
            .unwrap()
            .unwrap()
            .team_id
            .is_none()
    );
    assert_eq!(ctx.repos.org.get_all_teams().await.unwrap().len(), 2);

    ctx.teardown().await;
}

#[tokio::test]
async fn directory_does_not_match_a_lead_team_in_another_organization() {
    let ctx = RepoTestContext::new().await;
    let manager_id = Uuid::now_v7();
    sqlx::query!(
        "INSERT INTO org.people (id, name, directory_id) VALUES ($1, 'Manager', 'Manager')",
        manager_id
    )
    .execute(&ctx.pool)
    .await
    .unwrap();
    let unrelated = ctx
        .repos
        .org
        .create_team(
            "Foreign",
            "Other org",
            TeamType::Team,
            None,
            Some(manager_id),
        )
        .await
        .unwrap();
    let result = ctx
        .repos
        .org
        .import_records(
            &[
                record("Report", false, TeamType::Team),
                record(MANAGER, true, TeamType::Team),
            ],
            false,
        )
        .await
        .unwrap();
    assert_eq!(result.teams_created, 1);
    assert!(
        ctx.repos
            .org
            .get_team_members(unrelated.id.into())
            .await
            .unwrap()
            .is_empty()
    );
    let canonical = ctx
        .repos
        .org
        .get_all_teams()
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.org_name == "Canonical")
        .unwrap();
    assert_eq!(canonical.parent_team_id, None);
    assert_eq!(canonical.lead_id, Some(manager_id));
    assert_eq!(canonical.member_count, 2);

    ctx.teardown().await;
}
