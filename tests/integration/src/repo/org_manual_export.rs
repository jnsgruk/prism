use super::org_manual_import::{directory, manual};
use crate::common::db::RepoTestContext;
use ps_core::models::Platform;
use ps_core::repo::org::{IdentityInput, ImportIdentity, OrgExport};

#[tokio::test]
async fn export_replace_preserves_distinct_uuids_with_shared_emails() {
    let ctx = RepoTestContext::new().await;
    let mut original_ids = Vec::new();

    for (name, username) in [("First", "first-login"), ("Second", "second-login")] {
        let mut input = manual(name, Some("shared@example.com"));
        input.identities.push(IdentityInput {
            platform: Platform::Github,
            username: username.into(),
            platform_user_id: None,
        });
        original_ids.push(ctx.repos.org.create_person(input).await.unwrap().person.id);
    }

    let export = ctx.repos.org.export_org().await.unwrap();
    for _ in 0..2 {
        let result = ctx.repos.org.import_org(&export, true).await.unwrap();
        assert_eq!(result.people_created, 2);
        assert_eq!(result.people_updated, 0);
        assert_eq!(result.identities_created, 2);
        assert!(result.warnings.is_empty());

        let people = ctx.repos.org.list_people(false).await.unwrap();
        assert_eq!(people.len(), 2);
        for (id, username) in original_ids.iter().zip(["first-login", "second-login"]) {
            assert!(people.iter().any(|person| person.id == *id));
            let identities = ctx
                .repos
                .org
                .get_identities_for_people(&[*id])
                .await
                .unwrap();
            assert_eq!(identities.len(), 1);
            assert_eq!(identities[0].platform_username, username);
        }
    }

    ctx.teardown().await;
}

#[tokio::test]
async fn export_merge_imports_all_manual_accounts_before_protecting_the_platform() {
    let ctx = RepoTestContext::new().await;
    let person = ctx
        .repos
        .org
        .create_person(manual("Existing", Some("existing@example.com")))
        .await
        .unwrap();
    let mut export = ctx.repos.org.export_org().await.unwrap();
    export.people[0].identities = vec!["first-login", "second-login"]
        .into_iter()
        .map(|username| ps_core::repo::org::export::ExportIdentity {
            platform: Platform::Github.to_string(),
            username: username.into(),
            platform_user_id: None,
            management: ps_core::models::Management::Manual,
        })
        .collect();

    let result = ctx.repos.org.import_org(&export, false).await.unwrap();
    assert_eq!(result.identities_created, 2);
    assert!(result.warnings.is_empty());

    let identities = ctx
        .repos
        .org
        .get_identities_for_people(&[person.person.id])
        .await
        .unwrap();
    assert_eq!(identities.len(), 2);
    for username in ["first-login", "second-login"] {
        assert!(
            identities
                .iter()
                .any(|identity| identity.platform_username == username)
        );
    }

    export.people[0].identities[0].username = "unwanted-login".into();
    let repeated = ctx.repos.org.import_org(&export, false).await.unwrap();
    assert_eq!(repeated.identities_created, 0);
    assert!(!repeated.warnings.is_empty());
    let protected = ctx
        .repos
        .org
        .get_identities_for_people(&[person.person.id])
        .await
        .unwrap();
    assert_eq!(protected.len(), 2);
    assert!(
        !protected
            .iter()
            .any(|identity| identity.platform_username == "unwanted-login")
    );

    ctx.teardown().await;
}

#[tokio::test]
async fn export_roundtrip_preserves_removed_accounts_and_manual_platform_choices() {
    let ctx = RepoTestContext::new().await;

    let mut params = manual("Roundtrip", Some("roundtrip@example.com"));
    params.identities = vec![
        IdentityInput {
            platform: Platform::Github,
            username: "removed-account".into(),
            platform_user_id: None,
        },
        IdentityInput {
            platform: Platform::Jira,
            username: "jira-label".into(),
            platform_user_id: Some("Opaque:ExactCase".into()),
        },
    ];
    let person = ctx.repos.org.create_person(params).await.unwrap();

    let removed = person
        .identities
        .iter()
        .find(|identity| identity.platform == Platform::Github.to_string())
        .unwrap();
    ctx.repos
        .org
        .remove_person_identity(person.person.id.into(), removed.id)
        .await
        .unwrap();

    let export = ctx.repos.org.export_org().await.unwrap();

    let row = export
        .people
        .iter()
        .find(|row| row.id == Some(person.person.id))
        .unwrap();

    assert!(
        row.manual_identity_platforms
            .contains(&Platform::Github.to_string())
    );
    assert!(
        row.manual_identity_platforms
            .contains(&Platform::Jira.to_string())
    );

    ctx.repos.org.reset_all().await.unwrap();

    let export: OrgExport = serde_json::from_value(serde_json::to_value(export).unwrap()).unwrap();
    ctx.repos.org.import_org(&export, false).await.unwrap();

    let statuses = ctx
        .repos
        .org
        .get_resolution_statuses(person.person.id)
        .await
        .unwrap();

    assert_eq!(statuses.len(), 2);
    assert!(
        statuses
            .iter()
            .all(|(_, status)| *status == ps_core::models::ResolutionStatus::Manual)
    );

    for _ in 0..2 {
        ctx.repos.org.import_org(&export, false).await.unwrap();

        let mut incoming = directory(
            "Roundtrip",
            Some("roundtrip@example.com"),
            Some("directory-roundtrip"),
        );
        incoming.identities = vec![
            ImportIdentity {
                platform: Platform::Github.to_string(),
                username: "removed-account".into(),
            },
            ImportIdentity {
                platform: Platform::Jira.to_string(),
                username: "other-jira-label".into(),
            },
        ];

        assert_eq!(
            ctx.repos
                .org
                .import_records(&[incoming], false)
                .await
                .unwrap()
                .identities_mapped,
            0
        );
        assert!(matches!(
            ctx.repos
                .org
                .resolve_identity(
                    person.person.id,
                    &Platform::Github.to_string(),
                    "removed-account"
                )
                .await,
            Err(ps_core::Error::Conflict(_))
        ));
    }

    let identities = ctx
        .repos
        .org
        .get_identities_for_people(&[person.person.id])
        .await
        .unwrap();

    assert_eq!(identities.len(), 1);
    assert_eq!(
        identities[0].platform_user_id.as_deref(),
        Some("Opaque:ExactCase")
    );
    assert_eq!(
        ctx.repos
            .org
            .get_person(person.person.id)
            .await
            .unwrap()
            .unwrap()
            .team_id,
        None
    );

    ctx.teardown().await;
}

#[tokio::test]
async fn export_merge_preserves_existing_manual_platform_choices() {
    let ctx = RepoTestContext::new().await;

    let mut params = manual("Protected", Some("protected@example.com"));
    params.identities.push(IdentityInput {
        platform: Platform::Github,
        username: "chosen-account".into(),
        platform_user_id: None,
    });
    let person = ctx.repos.org.create_person(params).await.unwrap();

    let incoming: OrgExport = serde_json::from_value(serde_json::json!({
        "version": 1,
        "exported_at": "2026-01-01T00:00:00Z",
        "teams": [],
        "people": [
            {
                "name": "Protected",
                "email": "protected@example.com",
                "active": true,
                "identities": [
                    {
                        "platform": "github",
                        "username": "imported-extra"
                    }
                ]
            }
        ]
    }))
    .unwrap();

    let result = ctx.repos.org.import_org(&incoming, false).await.unwrap();

    assert_eq!(result.identities_created, 0);
    assert!(!result.warnings.is_empty());

    let identities = ctx
        .repos
        .org
        .get_identities_for_people(&[person.person.id])
        .await
        .unwrap();

    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].platform_username, "chosen-account");

    ctx.teardown().await;
}

#[tokio::test]
async fn export_rows_skipped_as_ambiguous_do_not_apply_accounts_or_memberships() {
    let ctx = RepoTestContext::new().await;

    let initial: OrgExport = serde_json::from_value(serde_json::json!({
        "version": 1,
        "exported_at": "2026-01-01T00:00:00Z",
        "teams": [],
        "people": [
            {
                "name": "First",
                "email": "first@example.com",
                "active": true
            },
            {
                "name": "Second",
                "email": "second@example.com",
                "active": true
            }
        ]
    }))
    .unwrap();
    ctx.repos.org.import_org(&initial, false).await.unwrap();

    let first = ctx
        .repos
        .org
        .list_people(false)
        .await
        .unwrap()
        .into_iter()
        .find(|person| person.name == "First")
        .unwrap();

    let incoming: OrgExport = serde_json::from_value(serde_json::json!({
        "version": 1,
        "exported_at": "2026-01-01T00:00:00Z",
        "teams": [
            {
                "name": "Unexpected team",
                "org_name": "Canonical",
                "team_type": "team"
            }
        ],
        "people": [
            {
                "id": first.id,
                "name": "First",
                "email": "second@example.com",
                "active": true,
                "team": "Unexpected team",
                "identities": [
                    {
                        "platform": "github",
                        "username": "unexpected-account"
                    }
                ]
            }
        ]
    }))
    .unwrap();

    let result = ctx.repos.org.import_org(&incoming, false).await.unwrap();

    assert_eq!(result.identities_created, 0);
    assert!(!result.warnings.is_empty());
    assert!(
        ctx.repos
            .org
            .get_identities_for_people(&[first.id])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        ctx.repos
            .org
            .list_people(false)
            .await
            .unwrap()
            .iter()
            .all(|person| person.team_id.is_none())
    );

    ctx.teardown().await;
}
