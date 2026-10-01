use crate::common::db::RepoTestContext;
use ps_core::models::{Management, Platform, TeamType};
use ps_core::repo::org::{
    CreatePersonParams, IdentityInput, ImportIdentity, ImportRecord, OrgExport,
};

pub(super) fn manual(name: &str, email: Option<&str>) -> CreatePersonParams {
    CreatePersonParams {
        name: name.into(),
        email: email.map(str::to_owned),
        level: None,
        team_id: None,
        identities: vec![],
    }
}

pub(super) fn directory(
    name: &str,
    email: Option<&str>,
    directory_id: Option<&str>,
) -> ImportRecord {
    ImportRecord {
        name: name.into(),
        email: email.map(str::to_owned),
        directory_id: directory_id.map(str::to_owned),
        level: None,
        team: Some("Imported team".into()),
        team_type: Some(TeamType::Team),
        org: Some("Canonical".into()),
        identities: vec![],
        manager_name: None,
        depth: None,
        has_reports: false,
        group: None,
    }
}

#[tokio::test]
async fn manual_people_reconcile_json_then_html_with_uuid_and_team_choice_preserved() {
    let ctx = RepoTestContext::new().await;

    let unassigned = ctx
        .repos
        .org
        .create_person(manual("Manual", Some("manual@example.com")))
        .await
        .unwrap();

    let team = ctx
        .repos
        .org
        .create_team("Chosen team", "Canonical", TeamType::Team, None, None)
        .await
        .unwrap();

    let mut params = manual("Selected", Some("selected@example.com"));
    params.team_id = Some(team.id.into());
    let selected = ctx.repos.org.create_person(params).await.unwrap();

    let records = vec![
        directory("Directory name", Some("MANUAL@example.com"), Some("dir1")),
        directory("Selected", Some("selected@example.com"), Some("dir2")),
    ];
    for _ in 0..2 {
        let result = ctx.repos.org.import_records(&records, false).await.unwrap();

        assert_eq!(result.people_imported, 0);
        assert_eq!(result.people_updated, 2);
    }

    let fetched = ctx
        .repos
        .org
        .get_person(unassigned.person.id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(fetched.team_id, None);
    assert_eq!(
        ctx.repos
            .org
            .get_person(selected.person.id)
            .await
            .unwrap()
            .unwrap()
            .team_id,
        Some(team.id)
    );

    let exported = ctx.repos.org.export_org().await.unwrap();

    let row = exported
        .people
        .iter()
        .find(|p| p.id == Some(unassigned.person.id))
        .unwrap();

    assert_eq!(row.directory_id.as_deref(), Some("dir1"));
    assert!(row.last_import_at.is_some());
    assert_eq!(row.membership_management, Management::Manual);

    let html = directory("HTML name", Some("manual@example.com"), None);

    assert_eq!(
        ctx.repos
            .org
            .import_records(&[html], false)
            .await
            .unwrap()
            .people_updated,
        1
    );
    assert_eq!(ctx.repos.org.list_people(false).await.unwrap().len(), 2);

    ctx.teardown().await;
}

#[tokio::test]
async fn directory_ambiguous_or_conflicting_email_is_skipped_and_manual_people_are_not_stale() {
    let ctx = RepoTestContext::new().await;

    ctx.repos
        .org
        .create_person(manual("First", Some("same@example.com")))
        .await
        .unwrap();
    ctx.repos
        .org
        .create_person(manual("Second", Some("SAME@example.com")))
        .await
        .unwrap();

    let ambiguous = ctx
        .repos
        .org
        .import_records(
            &[directory(
                "Incoming",
                Some("same@example.com"),
                Some("incoming"),
            )],
            true,
        )
        .await
        .unwrap();

    assert_eq!(ambiguous.people_imported, 0);
    assert_eq!(ambiguous.people_updated, 0);
    assert!(!ambiguous.warnings.is_empty());
    assert_eq!(ambiguous.stale_people_count, 0);

    ctx.repos
        .org
        .import_records(
            &[directory(
                "Existing",
                Some("existing@example.com"),
                Some("dir1"),
            )],
            false,
        )
        .await
        .unwrap();

    let conflict = ctx
        .repos
        .org
        .import_records(
            &[directory(
                "Other",
                Some("existing@example.com"),
                Some("dir2"),
            )],
            false,
        )
        .await
        .unwrap();

    assert_eq!(conflict.people_imported, 0);
    assert_eq!(conflict.people_updated, 0);
    assert!(!conflict.warnings.is_empty());

    let missing = ctx
        .repos
        .org
        .import_records(&[directory("No email", None, Some("separate"))], false)
        .await
        .unwrap();

    assert_eq!(missing.people_imported, 1);

    ctx.teardown().await;
}

#[tokio::test]
async fn directory_jira_csv_and_resolution_preserve_manual_accounts_and_removal() {
    let ctx = RepoTestContext::new().await;

    let mut params = manual("Owner", Some("owner@example.com"));
    params.identities = vec![
        IdentityInput {
            platform: Platform::Github,
            username: "owned".into(),
            platform_user_id: None,
        },
        IdentityInput {
            platform: Platform::Jira,
            username: "owner@example.com".into(),
            platform_user_id: Some("Opaque:Case".into()),
        },
    ];
    let owner = ctx.repos.org.create_person(params).await.unwrap();

    let other = ctx
        .repos
        .org
        .create_person(manual("Other", Some("other@example.com")))
        .await
        .unwrap();

    let mut incoming = directory("Other", Some("other@example.com"), Some("other"));
    incoming.identities.push(ImportIdentity {
        platform: Platform::Github.to_string(),
        username: "OWNED".into(),
    });
    let result = ctx
        .repos
        .org
        .import_records(&[incoming], false)
        .await
        .unwrap();

    assert_eq!(result.identities_mapped, 0);
    assert!(!result.warnings.is_empty());
    assert!(matches!(
        ctx.repos
            .org
            .resolve_identity(other.person.id, "github", "owned")
            .await,
        Err(ps_core::Error::Conflict(_))
    ));

    let jira = ps_core::directory::JiraUserRecord {
        display_name: "Other".into(),
        email: "other@example.com".into(),
        account_id: "Opaque:Case".into(),
    };
    let (mapped, _, warnings) = ctx.repos.org.import_jira_users(&[jira]).await.unwrap();

    assert_eq!(mapped, 0);
    assert!(!warnings.is_empty());

    let overwrite = ps_core::directory::JiraUserRecord {
        display_name: "Owner".into(),
        email: "owner@example.com".into(),
        account_id: "Wrong:ID".into(),
    };

    assert_eq!(
        ctx.repos
            .org
            .import_jira_users(&[overwrite])
            .await
            .unwrap()
            .0,
        0
    );

    let saved = ctx
        .repos
        .org
        .get_identities_for_people(&[owner.person.id])
        .await
        .unwrap();

    assert_eq!(
        saved
            .iter()
            .find(|i| i.platform == "jira")
            .unwrap()
            .platform_user_id
            .as_deref(),
        Some("Opaque:Case")
    );

    let github = saved.iter().find(|i| i.platform == "github").unwrap();
    ctx.repos
        .org
        .remove_person_identity(owner.person.id.into(), github.id)
        .await
        .unwrap();

    assert!(matches!(
        ctx.repos
            .org
            .resolve_identity(owner.person.id, "github", "owned")
            .await,
        Err(ps_core::Error::Conflict(_))
    ));

    let mut reimport = directory("Owner", Some("owner@example.com"), Some("owner"));
    reimport.identities.push(ImportIdentity {
        platform: "github".into(),
        username: "owned".into(),
    });

    assert_eq!(
        ctx.repos
            .org
            .import_records(&[reimport], false)
            .await
            .unwrap()
            .identities_mapped,
        0
    );

    ctx.teardown().await;
}

#[tokio::test]
async fn old_exports_read_and_new_exports_roundtrip_account_ids_manual_metadata_and_same_names() {
    let ctx = RepoTestContext::new().await;

    let old: OrgExport = serde_json::from_value(serde_json::json!({
        "version": 1,
        "exported_at": "2026-01-01T00:00:00Z",
        "teams": [],
        "people": [
            {
                "name": "Old",
                "active": true,
                "identities": [
                    {
                        "platform": "github",
                        "username": "old"
                    }
                ]
            }
        ]
    }))
    .unwrap();

    ctx.repos.org.import_org(&old, false).await.unwrap();

    let mut params = manual("Duplicate name", None);
    params.identities.push(IdentityInput {
        platform: Platform::Jira,
        username: "jira-label".into(),
        platform_user_id: Some("MixedCase:123".into()),
    });
    let original = ctx.repos.org.create_person(params).await.unwrap();
    ctx.repos
        .org
        .create_person(manual("Duplicate name", None))
        .await
        .unwrap();

    let export = ctx.repos.org.export_org().await.unwrap();

    let export: OrgExport = serde_json::from_slice(&serde_json::to_vec(&export).unwrap()).unwrap();
    ctx.repos.org.reset_all().await.unwrap();
    ctx.repos.org.import_org(&export, false).await.unwrap();

    let person = ctx
        .repos
        .org
        .get_person(original.person.id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(person.team_id, None);

    let identities = ctx
        .repos
        .org
        .get_identities_for_people(&[original.person.id])
        .await
        .unwrap();

    assert_eq!(identities.len(), 1);
    assert_eq!(
        identities[0].platform_user_id.as_deref(),
        Some("MixedCase:123")
    );
    assert_eq!(identities[0].management, Management::Manual);

    let repeated = ctx.repos.org.import_org(&export, false).await.unwrap();

    assert_eq!(repeated.people_created, 0);
    assert_eq!(
        ctx.repos
            .org
            .import_records(&[], true)
            .await
            .unwrap()
            .stale_people_count,
        0
    );

    ctx.teardown().await;
}

#[tokio::test]
async fn unmatched_directory_name_or_manual_account_requires_explicit_repair() {
    let ctx = RepoTestContext::new().await;

    let mut params = manual("No Email Manual", None);
    params.identities.push(IdentityInput {
        platform: Platform::Github,
        username: "manually-owned-account".into(),
        platform_user_id: None,
    });
    let person = ctx.repos.org.create_person(params).await.unwrap();

    let name_only = directory("  no email manual  ", None, None);
    let mut account_only = directory(
        "Different Display Name",
        Some("unknown@example.com"),
        Some("unlinked-directory"),
    );
    account_only.identities.push(ImportIdentity {
        platform: Platform::Github.to_string(),
        username: " MANUALLY-OWNED-ACCOUNT ".into(),
    });
    let result = ctx
        .repos
        .org
        .import_records(&[name_only, account_only], true)
        .await
        .unwrap();

    assert_eq!(result.people_imported, 0);
    assert_eq!(result.people_updated, 0);
    assert_eq!(result.identities_mapped, 0);
    assert_eq!(result.warnings.len(), 2);
    assert!(
        result
            .warnings
            .iter()
            .all(|warning| warning.contains("repair directory mapping"))
    );
    assert_eq!(result.stale_people_count, 0);

    let people = ctx.repos.org.list_people(false).await.unwrap();

    assert_eq!(people.len(), 1);
    assert_eq!(people[0].id, person.person.id);
    assert_eq!(people[0].name, "No Email Manual");

    let exported = ctx.repos.org.export_org().await.unwrap();

    assert!(exported.people[0].directory_id.is_none());
    assert!(exported.people[0].last_import_at.is_none());

    let unrelated = ctx
        .repos
        .org
        .import_records(&[directory("Unrelated", None, None)], false)
        .await
        .unwrap();

    assert_eq!(unrelated.people_imported, 1);
    assert!(unrelated.warnings.is_empty());

    ctx.teardown().await;
}
